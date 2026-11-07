//! The server: runs sessions in ptys on this machine and streams them to attached clients.
//!
//! Each session is a real [`Session`] (it answers the program's terminal queries and tracks
//! shell integration) whose raw output is tapped. The tap feeds a [`Mirror`] and forwards the
//! bytes to every attached client, both under the session's fan-out lock, so a client that
//! attaches gets a snapshot of the mirror and then exactly the output after it.
//!
//! Lock order: the I/O thread holds the session's terminal lock while it calls the tap, which
//! takes the fan-out lock. So the fan-out lock is never held while calling anything on the
//! [`Session`] that locks its terminal (`resize`, ...).

use std::collections::HashMap;
use std::io::{self, BufWriter, Read, Write};
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, AtomicU64, AtomicUsize, Ordering};
use std::sync::mpsc::{self, Receiver, Sender};
use std::sync::{Arc, Mutex, MutexGuard};
use std::thread;
use std::time::{Duration, Instant};

use term_core::{LaunchSpec, Mirror, Palette, Session, SessionEvent, SessionKind, TermDimensions};
use workspace_model::AgentId;

use crate::protocol::{
    ClientMessage, PROTOCOL_VERSION, ServerMessage, SessionInfo, SpawnSpec, read_frame, write_frame,
};

/// A client whose queued output passes this is too slow to follow: it is disconnected, and can
/// attach again to get a fresh snapshot.
const MAX_QUEUED_BYTES: usize = 32 * 1024 * 1024;

/// The sessions this server runs. Cheap to clone; every connection shares it.
#[derive(Clone, Default)]
pub struct Server {
    inner: Arc<Inner>,
}

#[derive(Default)]
struct Inner {
    sessions: Mutex<HashMap<u64, Arc<Hosted>>>,
    next_session: AtomicU64,
    next_client: AtomicU64,
    connections: AtomicUsize,
    /// When the last connection ended or the last session was closed.
    idle_since: Mutex<Option<Instant>>,
}

struct Hosted {
    id: u64,
    kind: String,
    session: Session,
    fanout: Arc<Mutex<Fanout>>,
}

/// What a session's output goes to.
struct Fanout {
    mirror: Mirror,
    clients: HashMap<u64, Outbox>,
    title: Option<String>,
    exit: Option<Option<i32>>,
}

impl Fanout {
    fn broadcast(&mut self, message: impl Fn() -> ServerMessage) {
        self.clients.retain(|_, outbox| outbox.send(message()));
    }

    fn broadcast_except(&mut self, client: u64, message: impl Fn() -> ServerMessage) {
        self.clients
            .retain(|&id, outbox| id == client || outbox.send(message()));
    }
}

/// The queue of messages to one connection.
#[derive(Clone)]
struct Outbox {
    sender: Sender<ServerMessage>,
    queued: Arc<AtomicUsize>,
    overflowed: Arc<AtomicBool>,
}

impl Outbox {
    fn new() -> (Self, Receiver<ServerMessage>) {
        let (sender, receiver) = mpsc::channel();
        let outbox = Self {
            sender,
            queued: Arc::new(AtomicUsize::new(0)),
            overflowed: Arc::new(AtomicBool::new(false)),
        };
        (outbox, receiver)
    }

    /// Queue a message; `false` once the connection is gone or too far behind.
    fn send(&self, message: ServerMessage) -> bool {
        if self.overflowed.load(Ordering::Acquire) {
            return false;
        }
        let size = queued_size(&message);
        if self.queued.fetch_add(size, Ordering::AcqRel) + size > MAX_QUEUED_BYTES {
            self.overflowed.store(true, Ordering::Release);
            let _ = self.sender.send(ServerMessage::Failed {
                request: 0,
                message: "the connection is too slow for the output; attach again".into(),
            });
            return false;
        }
        self.sender.send(message).is_ok()
    }
}

fn queued_size(message: &ServerMessage) -> usize {
    match message {
        ServerMessage::Output { bytes, .. } => bytes.len(),
        ServerMessage::Attached { snapshot, .. } => snapshot.len(),
        _ => 0,
    }
}

impl Server {
    pub fn new() -> Self {
        Self::default()
    }

    /// How long the server had no connection and no session; `None` while it has either.
    pub fn idle_for(&self) -> Option<Duration> {
        if self.inner.connections.load(Ordering::Acquire) > 0 || !self.sessions().is_empty() {
            return None;
        }
        let mut idle_since = self.inner.idle_since.lock().unwrap();
        Some(idle_since.get_or_insert_with(Instant::now).elapsed())
    }

    fn sessions(&self) -> MutexGuard<'_, HashMap<u64, Arc<Hosted>>> {
        self.inner.sessions.lock().unwrap()
    }

    fn session(&self, id: u64) -> Option<Arc<Hosted>> {
        self.sessions().get(&id).cloned()
    }

    /// Serve one client connection until it ends: blocks.
    pub fn serve_connection(&self, reader: impl Read, writer: impl Write + Send + 'static) {
        self.inner.connections.fetch_add(1, Ordering::AcqRel);
        let client = self.inner.next_client.fetch_add(1, Ordering::AcqRel);
        let (outbox, receiver) = Outbox::new();
        let writer_thread = thread::Builder::new()
            .name(format!("agentws-client-{client}"))
            .spawn({
                let queued = outbox.queued.clone();
                let overflowed = outbox.overflowed.clone();
                move || write_messages(writer, &receiver, &queued, &overflowed)
            });
        match writer_thread {
            Ok(_) => {
                if let Err(err) = self.read_messages(client, &outbox, reader) {
                    log::info!("client {client}: {err}");
                }
            }
            Err(err) => log::warn!("client {client}: no writer thread: {err}"),
        }
        for hosted in self.sessions().values() {
            hosted.fanout.lock().unwrap().clients.remove(&client);
        }
        *self.inner.idle_since.lock().unwrap() = Some(Instant::now());
        self.inner.connections.fetch_sub(1, Ordering::AcqRel);
    }

    fn read_messages(&self, client: u64, outbox: &Outbox, mut reader: impl Read) -> io::Result<()> {
        let Some(frame) = read_frame(&mut reader)? else {
            return Ok(());
        };
        match ClientMessage::decode(&frame)? {
            ClientMessage::Hello { version } if version == PROTOCOL_VERSION => {
                outbox.send(ServerMessage::Hello {
                    version: PROTOCOL_VERSION,
                });
            }
            ClientMessage::Hello { version } => {
                outbox.send(ServerMessage::Failed {
                    request: 0,
                    message: format!(
                        "the server speaks protocol {PROTOCOL_VERSION}, the app {version}: \
                         install the same version on both machines"
                    ),
                });
                return Ok(());
            }
            other => {
                return Err(io::Error::new(
                    io::ErrorKind::InvalidData,
                    format!("expected a hello, got {other:?}"),
                ));
            }
        }
        while let Some(frame) = read_frame(&mut reader)? {
            if outbox.overflowed.load(Ordering::Acquire) {
                return Ok(());
            }
            self.handle(client, outbox, ClientMessage::decode(&frame)?);
        }
        Ok(())
    }

    fn handle(&self, client: u64, outbox: &Outbox, message: ClientMessage) {
        match message {
            ClientMessage::Hello { .. } => {}
            ClientMessage::Spawn {
                request,
                spec,
                size,
            } => match self.spawn(&spec, size) {
                Ok(hosted) => self.attach(client, outbox, request, &hosted, size),
                Err(err) => {
                    outbox.send(ServerMessage::Failed {
                        request,
                        message: err.to_string(),
                    });
                }
            },
            ClientMessage::Attach {
                request,
                session,
                size,
            } => match self.session(session) {
                Some(hosted) => self.attach(client, outbox, request, &hosted, size),
                None => {
                    outbox.send(ServerMessage::Failed {
                        request,
                        message: format!("no session {session} on the server"),
                    });
                }
            },
            ClientMessage::Detach { session } => {
                if let Some(hosted) = self.session(session) {
                    hosted.fanout.lock().unwrap().clients.remove(&client);
                }
            }
            ClientMessage::List { request } => {
                let mut sessions: Vec<SessionInfo> = self
                    .sessions()
                    .values()
                    .map(|hosted| hosted.info())
                    .collect();
                sessions.sort_by_key(|info| info.id);
                outbox.send(ServerMessage::Sessions { request, sessions });
            }
            ClientMessage::Input { session, bytes } => {
                if let Some(hosted) = self.session(session) {
                    hosted.session.write(bytes);
                }
            }
            ClientMessage::Resize { session, size } => {
                if let Some(hosted) = self.session(session) {
                    hosted.resize(client, size);
                }
            }
            ClientMessage::Close { session } => self.close(session),
        }
    }

    fn spawn(&self, spec: &SpawnSpec, size: TermDimensions) -> io::Result<Arc<Hosted>> {
        let launch = launch_spec(spec)?;
        let id = self.inner.next_session.fetch_add(1, Ordering::AcqRel) + 1;
        let fanout = Arc::new(Mutex::new(Fanout {
            mirror: Mirror::new(size, Arc::new(|_| {})),
            clients: HashMap::new(),
            title: None,
            exit: None,
        }));
        let events = fanout.clone();
        let callback = Arc::new(move |event| match event {
            SessionEvent::Exited(code) => {
                let mut fanout = events.lock().unwrap();
                fanout.exit = Some(code);
                fanout.broadcast(|| ServerMessage::Exited { session: id, code });
            }
            SessionEvent::TitleChanged(title) => events.lock().unwrap().title = title,
            _ => {}
        });
        let output = fanout.clone();
        let tap = Box::new(move |bytes: &[u8]| {
            let mut fanout = output.lock().unwrap();
            fanout.mirror.feed(bytes);
            fanout.broadcast(|| ServerMessage::Output {
                session: id,
                bytes: bytes.to_vec(),
            });
        });
        let session =
            Session::spawn_with_tap(id, &launch, size, Palette::default(), callback, Some(tap))?;
        let hosted = Arc::new(Hosted {
            id,
            kind: spec.kind.clone(),
            session,
            fanout,
        });
        self.sessions().insert(id, hosted.clone());
        log::info!("session {id} started: {:?}", launch.program);
        Ok(hosted)
    }

    fn attach(
        &self,
        client: u64,
        outbox: &Outbox,
        request: u32,
        hosted: &Hosted,
        size: TermDimensions,
    ) {
        // Before the fan-out lock: resizing locks the session's terminal.
        hosted.session.resize(size);
        let mut fanout = hosted.fanout.lock().unwrap();
        let resized = fanout.mirror.size() != size;
        fanout.mirror.resize(size);
        let snapshot = fanout.mirror.snapshot();
        let info = hosted.info_locked(&fanout);
        let exit = fanout.exit;
        if resized {
            fanout.broadcast_except(client, || ServerMessage::Resized {
                session: hosted.id,
                size,
            });
        }
        if !outbox.send(ServerMessage::Attached {
            request,
            info,
            snapshot,
        }) {
            return;
        }
        match exit {
            Some(code) => {
                outbox.send(ServerMessage::Exited {
                    session: hosted.id,
                    code,
                });
            }
            None => {
                fanout.clients.insert(client, outbox.clone());
            }
        }
    }

    fn close(&self, session: u64) {
        let Some(hosted) = self.sessions().remove(&session) else {
            return;
        };
        {
            let mut fanout = hosted.fanout.lock().unwrap();
            if fanout.exit.is_none() {
                fanout.exit = Some(None);
                fanout.broadcast(|| ServerMessage::Exited {
                    session,
                    code: None,
                });
            }
            fanout.clients.clear();
        }
        if self.sessions().is_empty() {
            *self.inner.idle_since.lock().unwrap() = Some(Instant::now());
        }
        log::info!("session {session} closed");
        // Dropping the last `Hosted` hangs up on the program.
    }
}

impl Hosted {
    fn info(&self) -> SessionInfo {
        self.info_locked(&self.fanout.lock().unwrap())
    }

    fn info_locked(&self, fanout: &Fanout) -> SessionInfo {
        SessionInfo {
            id: self.id,
            kind: self.kind.clone(),
            size: fanout.mirror.size(),
            title: fanout.title.clone(),
            exit: fanout.exit,
        }
    }

    fn resize(&self, client: u64, size: TermDimensions) {
        self.session.resize(size);
        let mut fanout = self.fanout.lock().unwrap();
        if fanout.mirror.size() != size {
            fanout.mirror.resize(size);
            fanout.broadcast_except(client, || ServerMessage::Resized {
                session: self.id,
                size,
            });
        }
    }
}

fn launch_spec(spec: &SpawnSpec) -> io::Result<LaunchSpec> {
    let kind = if spec.kind.is_empty() {
        SessionKind::Shell
    } else {
        let agent = AgentId::from_cli_selector(&spec.kind).ok_or_else(|| {
            io::Error::new(
                io::ErrorKind::InvalidInput,
                format!("unknown session kind {:?}", spec.kind),
            )
        })?;
        SessionKind::Agent(agent)
    };
    let mut launch = LaunchSpec::new(
        kind,
        spec.program.clone(),
        spec.args.clone(),
        spec.cwd.as_ref().map(PathBuf::from),
    );
    launch.env = spec.env.iter().cloned().collect();
    Ok(launch)
}

/// Write queued messages until the connection or the queue ends; flushes once the queue is
/// empty, so a burst of output goes out in few writes.
fn write_messages(
    writer: impl Write,
    receiver: &Receiver<ServerMessage>,
    queued: &AtomicUsize,
    overflowed: &AtomicBool,
) {
    let mut writer = BufWriter::with_capacity(64 * 1024, writer);
    while let Ok(message) = receiver.recv() {
        let mut next = Some(message);
        while let Some(message) = next.take() {
            let last = matches!(message, ServerMessage::Failed { request: 0, .. });
            if write_frame(&mut writer, &message.encode()).is_err() {
                return;
            }
            queued.fetch_sub(queued_size(&message), Ordering::AcqRel);
            if last && overflowed.load(Ordering::Acquire) {
                let _ = writer.flush();
                return;
            }
            next = receiver.try_recv().ok();
        }
        if writer.flush().is_err() {
            return;
        }
    }
}

#[cfg(test)]
mod tests;
