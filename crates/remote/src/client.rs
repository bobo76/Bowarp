//! The client: one connection to a server, and the remote sessions shown through it.
//!
//! A connection is any byte stream to `agentws-server proxy`, usually the stdin and stdout of
//! `ssh <host> agentws-server proxy` ([`Client::over_command`], [`crate::ssh_command`]). Each
//! [`RemoteSession`] shows its session in a [`Mirror`] fed by the connection's reader thread.

use std::collections::HashMap;
use std::io::{self, BufReader, ErrorKind, Read, Write};
use std::process::{Child, Command, Stdio};
use std::sync::atomic::{AtomicBool, AtomicU32, Ordering};
use std::sync::mpsc::{self, RecvTimeoutError};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::Duration;

use term_core::{EventCallback, Mirror, SessionEvent, TermDimensions};

use crate::protocol::{
    ClientMessage, PROTOCOL_VERSION, ServerMessage, SessionInfo, SpawnSpec, read_frame, write_frame,
};

/// How long a request waits for the server's answer.
const REPLY_TIMEOUT: Duration = Duration::from_secs(20);
/// How much of the transport command's error output is kept for error messages.
const MAX_STDERR: usize = 4096;

/// Called once when the connection ends, with the reason.
pub type DisconnectCallback = Box<dyn FnOnce(String) + Send>;

/// A connection to a server.
pub struct Client {
    inner: Arc<Inner>,
}

struct Inner {
    writer: Mutex<Box<dyn Write + Send>>,
    pending: Mutex<HashMap<u32, Pending>>,
    sessions: Mutex<HashMap<u64, Arc<Remote>>>,
    next_request: AtomicU32,
    connected: AtomicBool,
    child: Mutex<Option<Child>>,
}

enum Pending {
    Attach {
        callback: EventCallback,
        reply: mpsc::Sender<Result<Arc<Remote>, String>>,
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

/// The client's side of one remote session.
struct Remote {
    info: SessionInfo,
    mirror: Mirror,
    callback: EventCallback,
    exit: Mutex<Option<Option<i32>>>,
    connected: AtomicBool,
}

impl Client {
    /// Start `command` (e.g. [`crate::ssh_command`]) and speak to the server on its stdin and
    /// stdout. The command is killed when the client is dropped; the sessions go on running.
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
        thread::spawn(move || collect_stderr(stderr, &collected));
        match Self::connect(stdout, stdin, on_disconnect) {
            Ok(client) => {
                *client.inner.child.lock().unwrap() = Some(child);
                Ok(client)
            }
            Err(err) => {
                let _ = child.kill();
                let _ = child.wait();
                // The stderr reader ends with the child; give it a moment to see it all.
                thread::sleep(Duration::from_millis(50));
                let stderr = String::from_utf8_lossy(&errors.lock().unwrap())
                    .trim()
                    .to_string();
                Err(if stderr.is_empty() {
                    err
                } else {
                    io::Error::new(err.kind(), format!("{err}: {stderr}"))
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
        let inner = Arc::new(Inner {
            writer: Mutex::new(Box::new(writer)),
            pending: Mutex::new(HashMap::new()),
            sessions: Mutex::new(HashMap::new()),
            next_request: AtomicU32::new(1),
            connected: AtomicBool::new(true),
            child: Mutex::new(None),
        });
        let reading = inner.clone();
        thread::Builder::new()
            .name("agentws-remote".into())
            .spawn(move || {
                let reason = match reading.read_messages(&mut reader) {
                    Ok(reason) => reason,
                    Err(err) => format!("connection lost: {err}"),
                };
                reading.disconnected();
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

    /// Start a session on the server and attach to it. `callback` gets the mirror's events
    /// and [`SessionEvent::Exited`]; it is called on the connection's reader thread.
    pub fn spawn(
        &self,
        spec: SpawnSpec,
        size: TermDimensions,
        callback: EventCallback,
    ) -> io::Result<RemoteSession> {
        self.attach_with(callback, |request| ClientMessage::Spawn {
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
        callback: EventCallback,
    ) -> io::Result<RemoteSession> {
        self.attach_with(callback, |request| ClientMessage::Attach {
            request,
            session,
            size,
        })
    }

    fn attach_with(
        &self,
        callback: EventCallback,
        message: impl FnOnce(u32) -> ClientMessage,
    ) -> io::Result<RemoteSession> {
        let (reply, answer) = mpsc::channel();
        self.inner
            .request(Pending::Attach { callback, reply }, message)?;
        let remote = wait(&answer)?;
        Ok(RemoteSession {
            remote,
            client: self.inner.clone(),
            closed: false,
        })
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

fn wait<T>(answer: &mpsc::Receiver<Result<T, String>>) -> io::Result<T> {
    match answer.recv_timeout(REPLY_TIMEOUT) {
        Ok(Ok(value)) => Ok(value),
        Ok(Err(message)) => Err(io::Error::other(message)),
        Err(RecvTimeoutError::Timeout) => Err(io::Error::new(
            ErrorKind::TimedOut,
            "the server did not answer",
        )),
        Err(RecvTimeoutError::Disconnected) => Err(io::Error::new(
            ErrorKind::NotConnected,
            "the connection to the server ended",
        )),
    }
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

impl Inner {
    fn send(&self, message: &ClientMessage) -> io::Result<()> {
        if !self.connected.load(Ordering::Acquire) {
            return Err(io::Error::new(
                ErrorKind::NotConnected,
                "the connection to the server ended",
            ));
        }
        let mut writer = self.writer.lock().unwrap();
        write_frame(&mut *writer, &message.encode())?;
        writer.flush()
    }

    fn request(
        &self,
        pending: Pending,
        message: impl FnOnce(u32) -> ClientMessage,
    ) -> io::Result<()> {
        let request = self.next_request.fetch_add(1, Ordering::AcqRel);
        self.pending.lock().unwrap().insert(request, pending);
        let sent = self.send(&message(request));
        if sent.is_err() {
            self.pending.lock().unwrap().remove(&request);
        }
        sent
    }

    /// Handle the server's messages until the connection ends; returns why it ended.
    fn read_messages(&self, reader: &mut impl Read) -> io::Result<String> {
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

    fn handle(&self, message: ServerMessage) {
        match message {
            ServerMessage::Hello { .. } => {}
            ServerMessage::Attached {
                request,
                info,
                snapshot,
            } => {
                let Some(Pending::Attach { callback, reply }) =
                    self.pending.lock().unwrap().remove(&request)
                else {
                    return;
                };
                let remote = Arc::new(Remote {
                    mirror: Mirror::new(info.size, callback.clone()),
                    exit: Mutex::new(info.exit),
                    info,
                    callback,
                    connected: AtomicBool::new(true),
                });
                remote.mirror.feed(&snapshot);
                self.sessions
                    .lock()
                    .unwrap()
                    .insert(remote.info.id, remote.clone());
                if reply.send(Ok(remote.clone())).is_err() {
                    // Nobody waits for it any more (timed out): don't keep it attached.
                    self.sessions.lock().unwrap().remove(&remote.info.id);
                    let _ = self.send(&ClientMessage::Detach {
                        session: remote.info.id,
                    });
                }
            }
            ServerMessage::Sessions { request, sessions } => {
                if let Some(Pending::List { reply }) = self.pending.lock().unwrap().remove(&request)
                {
                    let _ = reply.send(Ok(sessions));
                }
            }
            ServerMessage::Output { session, bytes } => {
                if let Some(remote) = self.remote(session) {
                    remote.mirror.feed(&bytes);
                }
            }
            ServerMessage::Resized { session, size } => {
                if let Some(remote) = self.remote(session) {
                    remote.mirror.resize(size);
                }
            }
            ServerMessage::Exited { session, code } => {
                if let Some(remote) = self.remote(session) {
                    *remote.exit.lock().unwrap() = Some(code);
                    (remote.callback)(SessionEvent::Exited(code));
                }
            }
            ServerMessage::Failed { request, message } => {
                if let Some(pending) = self.pending.lock().unwrap().remove(&request) {
                    pending.fail(message);
                }
            }
        }
    }

    fn remote(&self, session: u64) -> Option<Arc<Remote>> {
        self.sessions.lock().unwrap().get(&session).cloned()
    }

    fn disconnected(&self) {
        self.connected.store(false, Ordering::Release);
        // Dropping the reply senders wakes the waiters with "connection ended".
        self.pending.lock().unwrap().clear();
        for remote in self.sessions.lock().unwrap().values() {
            remote.connected.store(false, Ordering::Release);
        }
    }
}

/// A session running on the server, shown here. Dropping it detaches: the session goes on
/// running on the server; [`RemoteSession::close`] ends it.
pub struct RemoteSession {
    remote: Arc<Remote>,
    client: Arc<Inner>,
    closed: bool,
}

impl RemoteSession {
    pub fn id(&self) -> u64 {
        self.remote.info.id
    }

    /// The session as it was when this client attached.
    pub fn info(&self) -> &SessionInfo {
        &self.remote.info
    }

    /// The terminal showing the session's output.
    pub fn mirror(&self) -> &Mirror {
        &self.remote.mirror
    }

    /// Type into the session's program.
    pub fn write(&self, bytes: impl Into<Vec<u8>>) {
        let message = ClientMessage::Input {
            session: self.id(),
            bytes: bytes.into(),
        };
        if let Err(err) = self.client.send(&message) {
            log::debug!("remote input dropped: {err}");
        }
    }

    /// Resize the mirror and the session's pty.
    pub fn resize(&self, size: TermDimensions) {
        if self.remote.mirror.size() == size {
            return;
        }
        self.remote.mirror.resize(size);
        let message = ClientMessage::Resize {
            session: self.id(),
            size,
        };
        if let Err(err) = self.client.send(&message) {
            log::debug!("remote resize dropped: {err}");
        }
    }

    pub fn has_exited(&self) -> bool {
        self.remote.exit.lock().unwrap().is_some()
    }

    pub fn exit_code(&self) -> Option<i32> {
        self.remote.exit.lock().unwrap().flatten()
    }

    /// Whether the connection this session is shown through is still up.
    pub fn is_connected(&self) -> bool {
        self.remote.connected.load(Ordering::Acquire)
    }

    /// End the session on the server.
    pub fn close(mut self) {
        self.closed = true;
        self.forget();
        let _ = self
            .client
            .send(&ClientMessage::Close { session: self.id() });
    }

    fn forget(&self) {
        self.client.sessions.lock().unwrap().remove(&self.id());
    }
}

impl Drop for RemoteSession {
    fn drop(&mut self) {
        if self.closed {
            return;
        }
        self.forget();
        let _ = self
            .client
            .send(&ClientMessage::Detach { session: self.id() });
    }
}
