//! The client: one connection to a server, and the remote sessions shown through it.
//!
//! A connection is any byte stream to `agentws-server proxy`, usually the stdin and stdout of
//! `ssh <host> agentws-server proxy` ([`Client::over_command`], [`crate::ssh_command`]). Each
//! remote session is a [`Session::remote`] fed by the connection's reader thread; what it
//! sends goes through a writer thread, so typing never blocks on the network.

use std::collections::HashMap;
use std::io::{self, BufReader, BufWriter, ErrorKind, Read, Write};
use std::process::{Child, Command, Stdio};
use std::sync::atomic::{AtomicBool, AtomicU32, Ordering};
use std::sync::mpsc::{self, Receiver, RecvTimeoutError, Sender};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::Duration;

use term_core::{
    EventCallback, LaunchSpec, Palette, RemoteLink, Session, SessionFeed, SessionKind,
    TermDimensions,
};
use workspace_model::AgentId;

use crate::protocol::{
    ClientMessage, PROTOCOL_VERSION, ServerMessage, SessionInfo, SpawnSpec, read_frame, write_frame,
};

/// How long a request waits for the server's answer.
const REPLY_TIMEOUT: Duration = Duration::from_secs(20);
/// How much of the transport command's error output is kept for error messages.
const MAX_STDERR: usize = 4096;

/// Called once when the connection ends, with the reason.
pub type DisconnectCallback = Box<dyn FnOnce(String) + Send>;

/// A session started or attached on the server, shown here.
pub struct RemoteSession {
    /// Dropping it ends the program on the server.
    pub session: Session,
    /// The session as the server described it when this client attached.
    pub info: SessionInfo,
}

/// How a session shows here: its local id (the app's tab id), colors and event callback.
pub struct LocalView {
    pub id: u64,
    pub palette: Palette,
    /// Gets the session's events on the connection's reader thread.
    pub callback: EventCallback,
}

/// A connection to a server.
pub struct Client {
    inner: Arc<Inner>,
}

struct Inner {
    outgoing: Mutex<Option<Sender<ClientMessage>>>,
    pending: Mutex<HashMap<u32, Pending>>,
    /// The attached sessions, by server session id.
    feeds: Mutex<HashMap<u64, SessionFeed>>,
    next_request: AtomicU32,
    connected: AtomicBool,
    child: Mutex<Option<Child>>,
}

type AttachReply = mpsc::Sender<Result<RemoteSession, String>>;

enum Pending {
    Attach {
        view: LocalView,
        reply: AttachReply,
    },
    List {
        reply: mpsc::Sender<Result<Vec<SessionInfo>, String>>,
    },
}

impl Pending {
    fn fail(self, message: String) {
        match self {
            Self::Attach { reply, .. } => drop(reply.send(Err(message))),
            Self::List { reply } => drop(reply.send(Err(message))),
        }
    }
}

impl SpawnSpec {
    /// What to start on the server for a launch made here.
    pub fn from_launch(spec: &LaunchSpec) -> Self {
        let mut env: Vec<(String, String)> = spec
            .env
            .iter()
            .map(|(key, value)| (key.clone(), value.clone()))
            .collect();
        env.sort();
        Self {
            kind: spec
                .kind
                .agent()
                .map_or_else(String::new, |agent| agent.cli_selector().to_owned()),
            program: spec.program.clone(),
            args: spec.args.clone(),
            cwd: spec
                .cwd
                .as_ref()
                .map(|cwd| cwd.to_string_lossy().into_owned()),
            env,
        }
    }
}

impl Client {
    /// Start `command` (e.g. [`crate::ssh_command`]) and speak to the server on its stdin and
    /// stdout. The command is killed when the client is dropped.
    pub fn over_command(
        mut command: Command,
        on_disconnect: DisconnectCallback,
    ) -> io::Result<Self> {
        command
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());
        #[cfg(windows)]
        {
            use std::os::windows::process::CommandExt;
            // No console window flashing up for ssh in the windowed app.
            const CREATE_NO_WINDOW: u32 = 0x0800_0000;
            command.creation_flags(CREATE_NO_WINDOW);
        }
        let mut child = command.spawn()?;
        let stdin = child.stdin.take().expect("piped stdin");
        let stdout = child.stdout.take().expect("piped stdout");
        let stderr = child.stderr.take().expect("piped stderr");
        let errors = Arc::new(Mutex::new(Vec::new()));
        let collected = errors.clone();
        let stderr_reader = thread::spawn(move || collect_stderr(stderr, &collected));
        match Self::connect(stdout, stdin, on_disconnect) {
            Ok(client) => {
                *client.inner.child.lock().unwrap() = Some(child);
                Ok(client)
            }
            Err(err) => {
                let _ = child.kill();
                let _ = child.wait();
                // It ends with the child: then it has read everything.
                let _ = stderr_reader.join();
                let stderr = String::from_utf8_lossy(&errors.lock().unwrap())
                    .trim()
                    .to_string();
                // What ssh said explains it better than the stream ending.
                Err(if stderr.is_empty() {
                    err
                } else {
                    io::Error::new(err.kind(), stderr)
                })
            }
        }
    }

    /// Speak to a server over `reader` and `writer`: sends the hello and waits for the
    /// server's. `on_disconnect` is called on the reader thread when the connection ends.
    pub fn connect(
        reader: impl Read + Send + 'static,
        mut writer: impl Write + Send + 'static,
        on_disconnect: DisconnectCallback,
    ) -> io::Result<Self> {
        let hello = ClientMessage::Hello {
            version: PROTOCOL_VERSION,
        };
        write_frame(&mut writer, &hello.encode())?;
        writer.flush()?;
        let mut reader = BufReader::with_capacity(64 * 1024, reader);
        let frame = read_frame(&mut reader)?.ok_or_else(|| {
            io::Error::new(ErrorKind::UnexpectedEof, "the server closed the connection")
        })?;
        match ServerMessage::decode(&frame)? {
            ServerMessage::Hello { version } if version == PROTOCOL_VERSION => {}
            ServerMessage::Failed { message, .. } => {
                return Err(io::Error::new(ErrorKind::ConnectionRefused, message));
            }
            other => {
                return Err(io::Error::new(
                    ErrorKind::InvalidData,
                    format!("not an agentws server (it answered {other:?})"),
                ));
            }
        }
        let (outgoing, queue) = mpsc::channel();
        thread::Builder::new()
            .name("agentws-remote-writer".into())
            .spawn(move || write_messages(writer, &queue))?;
        let inner = Arc::new(Inner {
            outgoing: Mutex::new(Some(outgoing)),
            pending: Mutex::new(HashMap::new()),
            feeds: Mutex::new(HashMap::new()),
            next_request: AtomicU32::new(1),
            connected: AtomicBool::new(true),
            child: Mutex::new(None),
        });
        let reading = inner.clone();
        thread::Builder::new()
            .name("agentws-remote-reader".into())
            .spawn(move || {
                let reason = match reading.read_messages(&mut reader) {
                    Ok(reason) => reason,
                    Err(err) => format!("connection lost: {err}"),
                };
                reading.disconnected(&reason);
                on_disconnect(reason);
            })?;
        Ok(Self { inner })
    }

    pub fn is_connected(&self) -> bool {
        self.inner.connected.load(Ordering::Acquire)
    }

    /// The sessions running on the server.
    pub fn list(&self) -> io::Result<Vec<SessionInfo>> {
        let (reply, answer) = mpsc::channel();
        self.inner
            .request(Pending::List { reply }, |request| ClientMessage::List {
                request,
            })?;
        wait(&answer)
    }

    /// End a session on the server, attached here or not.
    pub fn close(&self, session: u64) -> io::Result<()> {
        self.inner.send(ClientMessage::Close { session })
    }

    /// Start a session on the server and show it here.
    pub fn spawn(
        &self,
        spec: SpawnSpec,
        size: TermDimensions,
        view: LocalView,
    ) -> io::Result<RemoteSession> {
        self.attach_with(view, |request| ClientMessage::Spawn {
            request,
            spec,
            size,
        })
    }

    /// Show a session already running on the server, resizing it to `size`.
    pub fn attach(
        &self,
        session: u64,
        size: TermDimensions,
        view: LocalView,
    ) -> io::Result<RemoteSession> {
        self.attach_with(view, |request| ClientMessage::Attach {
            request,
            session,
            size,
        })
    }

    fn attach_with(
        &self,
        view: LocalView,
        message: impl FnOnce(u32) -> ClientMessage,
    ) -> io::Result<RemoteSession> {
        let (reply, answer) = mpsc::channel();
        self.inner
            .request(Pending::Attach { view, reply }, message)?;
        wait(&answer)
    }
}

impl Drop for Client {
    fn drop(&mut self) {
        if let Some(mut child) = self.inner.child.lock().unwrap().take() {
            let _ = child.kill();
            thread::spawn(move || child.wait());
        }
    }
}

fn wait<T>(answer: &Receiver<Result<T, String>>) -> io::Result<T> {
    match answer.recv_timeout(REPLY_TIMEOUT) {
        Ok(Ok(value)) => Ok(value),
        Ok(Err(message)) => Err(io::Error::other(message)),
        Err(RecvTimeoutError::Timeout) => Err(io::Error::new(
            ErrorKind::TimedOut,
            "the server did not answer",
        )),
        Err(RecvTimeoutError::Disconnected) => Err(not_connected()),
    }
}

fn not_connected() -> io::Error {
    io::Error::new(
        ErrorKind::NotConnected,
        "the connection to the server ended",
    )
}

fn collect_stderr(mut stderr: impl Read, errors: &Mutex<Vec<u8>>) {
    let mut chunk = [0u8; 1024];
    while let Ok(count) = stderr.read(&mut chunk) {
        if count == 0 {
            break;
        }
        let mut errors = errors.lock().unwrap();
        let room = MAX_STDERR.saturating_sub(errors.len());
        errors.extend_from_slice(&chunk[..count.min(room)]);
    }
}

/// Send queued messages until the queue closes or the connection fails; flushes once the
/// queue is empty, so a burst of keystrokes goes out in one write.
fn write_messages(writer: impl Write, queue: &Receiver<ClientMessage>) {
    let mut writer = BufWriter::new(writer);
    while let Ok(message) = queue.recv() {
        let mut next = Some(message);
        while let Some(message) = next.take() {
            if write_frame(&mut writer, &message.encode()).is_err() {
                return;
            }
            next = queue.try_recv().ok();
        }
        if writer.flush().is_err() {
            return;
        }
    }
}

impl Inner {
    /// Queue a message; never blocks.
    fn send(&self, message: ClientMessage) -> io::Result<()> {
        if !self.connected.load(Ordering::Acquire) {
            return Err(not_connected());
        }
        let outgoing = self.outgoing.lock().unwrap();
        match outgoing.as_ref() {
            Some(sender) => sender.send(message).map_err(|_| not_connected()),
            None => Err(not_connected()),
        }
    }

    fn request(
        &self,
        pending: Pending,
        message: impl FnOnce(u32) -> ClientMessage,
    ) -> io::Result<()> {
        let request = self.next_request.fetch_add(1, Ordering::AcqRel);
        self.pending.lock().unwrap().insert(request, pending);
        let sent = self.send(message(request));
        if sent.is_err() {
            self.pending.lock().unwrap().remove(&request);
        }
        sent
    }

    /// Handle the server's messages until the connection ends; returns why it ended.
    fn read_messages(self: &Arc<Self>, reader: &mut impl Read) -> io::Result<String> {
        while let Some(frame) = read_frame(reader)? {
            match ServerMessage::decode(&frame)? {
                ServerMessage::Failed {
                    request: 0,
                    message,
                } => return Ok(message),
                message => self.handle(message),
            }
        }
        Ok("the server closed the connection".into())
    }

    fn handle(self: &Arc<Self>, message: ServerMessage) {
        match message {
            ServerMessage::Hello { .. } => {}
            ServerMessage::Attached {
                request,
                info,
                snapshot,
            } => {
                let Some(Pending::Attach { view, reply }) =
                    self.pending.lock().unwrap().remove(&request)
                else {
                    return;
                };
                self.attached(view, reply, info, &snapshot);
            }
            ServerMessage::Sessions { request, sessions } => {
                if let Some(Pending::List { reply }) = self.pending.lock().unwrap().remove(&request)
                {
                    let _ = reply.send(Ok(sessions));
                }
            }
            ServerMessage::Output { session, bytes } => {
                if let Some(feed) = self.feeds.lock().unwrap().get(&session) {
                    feed.output(&bytes);
                }
            }
            ServerMessage::Resized { session, size } => {
                if let Some(feed) = self.feeds.lock().unwrap().get(&session) {
                    feed.resized(size);
                }
            }
            ServerMessage::Exited { session, code } => {
                if let Some(feed) = self.feeds.lock().unwrap().get(&session) {
                    feed.exited(code);
                }
            }
            ServerMessage::FolderChanged { session, folder } => {
                if let Some(feed) = self.feeds.lock().unwrap().get(&session) {
                    feed.folder_changed(folder);
                }
            }
            ServerMessage::Failed { request, message } => {
                if let Some(pending) = self.pending.lock().unwrap().remove(&request) {
                    pending.fail(message);
                }
            }
        }
    }

    fn attached(
        self: &Arc<Self>,
        view: LocalView,
        reply: AttachReply,
        info: SessionInfo,
        snapshot: &[u8],
    ) {
        let kind =
            AgentId::from_cli_selector(&info.kind).map_or(SessionKind::Shell, SessionKind::Agent);
        let link = Link {
            client: self.clone(),
            session: info.id,
        };
        let (session, feed) = Session::remote(
            view.id,
            kind,
            info.size,
            view.palette,
            view.callback,
            Box::new(link),
        );
        feed.output(snapshot);
        if let Some(folder) = info.folder.clone() {
            feed.folder_changed(folder);
        }
        if let Some(code) = info.exit {
            feed.exited(code);
        }
        self.feeds.lock().unwrap().insert(info.id, feed);
        let id = info.id;
        if let Err(mpsc::SendError(Ok(unwanted))) = reply.send(Ok(RemoteSession { session, info }))
        {
            // Nobody waits for it any more (timed out): detach instead of ending it.
            self.feeds.lock().unwrap().remove(&id);
            drop(unwanted);
            let _ = self.send(ClientMessage::Detach { session: id });
        }
    }

    fn disconnected(&self, reason: &str) {
        self.connected.store(false, Ordering::Release);
        // The writer thread ends once its queue closes.
        self.outgoing.lock().unwrap().take();
        // Dropping the reply senders wakes the waiters with "connection ended".
        self.pending.lock().unwrap().clear();
        let notice = format!("\r\n\x1b[0;2m[connection to the server lost: {reason}]\x1b[0m\r\n");
        for feed in self.feeds.lock().unwrap().drain().map(|(_, feed)| feed) {
            if !feed.has_exited() {
                feed.output(notice.as_bytes());
            }
            feed.exited(None);
        }
    }
}

/// A remote session's way to its server.
struct Link {
    client: Arc<Inner>,
    session: u64,
}

impl RemoteLink for Link {
    fn write(&self, bytes: std::borrow::Cow<'static, [u8]>) {
        let message = ClientMessage::Input {
            session: self.session,
            bytes: bytes.into_owned(),
        };
        if let Err(err) = self.client.send(message) {
            log::debug!("remote input dropped: {err}");
        }
    }

    fn resize(&self, size: TermDimensions) {
        let message = ClientMessage::Resize {
            session: self.session,
            size,
        };
        if let Err(err) = self.client.send(message) {
            log::debug!("remote resize dropped: {err}");
        }
    }

    fn close(&self) {
        // Gone already when it was never handed out, or the connection ended.
        if self
            .client
            .feeds
            .lock()
            .unwrap()
            .remove(&self.session)
            .is_some()
        {
            let _ = self.client.send(ClientMessage::Close {
                session: self.session,
            });
        }
    }
}
