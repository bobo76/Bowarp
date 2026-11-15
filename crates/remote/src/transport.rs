//! How a client reaches the server: the server is a per-user daemon on a local socket, and a
//! client runs `agentws-server proxy` on the server's machine (through ssh), which connects
//! to the daemon (starting it when none runs) and relays its stdin and stdout. So ssh does the
//! authentication and the encryption, the daemon is reachable only by its own user, and
//! sessions outlive the connection.

use std::io::{self, Read, Write};
use std::process::{Command, Stdio};
use std::sync::Arc;
use std::thread;
use std::time::{Duration, Instant};

use interprocess::local_socket::prelude::*;
use interprocess::local_socket::{GenericNamespaced, ListenerOptions, Stream};

use crate::server::Server;

/// Overrides the daemon's socket name (tests run their own daemon).
pub const SOCKET_ENV: &str = "AGENTWS_SERVER_SOCKET";
/// How long `proxy` waits for a daemon it started to listen.
const START_TIMEOUT: Duration = Duration::from_secs(5);
const IDLE_CHECK: Duration = Duration::from_millis(500);

/// The daemon's socket name for this user.
pub fn socket_name() -> String {
    std::env::var(SOCKET_ENV)
        .ok()
        .filter(|name| !name.is_empty())
        .unwrap_or_else(|| format!("agentws-server-{}.sock", user_tag()))
}

#[cfg(unix)]
fn user_tag() -> String {
    // SAFETY: `geteuid` has no preconditions and cannot fail.
    unsafe { libc::geteuid() }.to_string()
}

#[cfg(not(unix))]
fn user_tag() -> String {
    std::env::var("USERNAME").unwrap_or_default()
}

/// Run the daemon on `name` until it has had no session and no connection for `idle_exit`
/// (`None`: run until killed). Fails when another daemon already listens there.
pub fn serve(name: &str, idle_exit: Option<Duration>) -> io::Result<()> {
    let listener = ListenerOptions::new()
        .name(name.to_ns_name::<GenericNamespaced>()?)
        .create_sync()?;
    let server = Server::new();
    let accepting = server.clone();
    thread::Builder::new()
        .name("agentws-accept".into())
        .spawn(move || {
            for connection in listener.incoming() {
                let stream = match connection {
                    Ok(stream) => stream,
                    Err(err) => {
                        log::warn!("connection failed: {err}");
                        continue;
                    }
                };
                if !from_same_user(&stream) {
                    log::warn!("connection from another user refused");
                    continue;
                }
                let server = accepting.clone();
                let spawned = thread::Builder::new()
                    .name("agentws-connection".into())
                    .spawn(move || {
                        let (reader, writer) = halves(stream);
                        let hang_up = reader.hang_up();
                        server.serve_connection(reader, writer, hang_up);
                    });
                if let Err(err) = spawned {
                    log::warn!("no thread for a connection: {err}");
                }
            }
        })?;
    loop {
        thread::sleep(IDLE_CHECK);
        if let (Some(limit), Some(idle)) = (idle_exit, server.idle_for())
            && idle >= limit
        {
            return Ok(());
        }
    }
}

/// Whether the peer runs as this user. On Linux the socket is in the abstract namespace,
/// which has no file permissions.
#[cfg(unix)]
fn from_same_user(stream: &Stream) -> bool {
    // SAFETY: `geteuid` has no preconditions and cannot fail.
    let own = unsafe { libc::geteuid() };
    stream.peer_creds().ok().and_then(|creds| creds.euid()) == Some(own)
}

/// Named pipes are created with a security descriptor that lets only the owner (and
/// administrators) connect.
#[cfg(not(unix))]
fn from_same_user(_stream: &Stream) -> bool {
    true
}

pub fn connect(name: &str) -> io::Result<Stream> {
    Stream::connect(name.to_ns_name::<GenericNamespaced>()?)
}

/// Connect to the daemon on `name`, starting `server_program serve` in the background when
/// none listens yet.
pub fn connect_or_start(name: &str, server_program: &std::path::Path) -> io::Result<Stream> {
    if let Ok(stream) = connect(name) {
        return Ok(stream);
    }
    let mut command = Command::new(server_program);
    command
        .arg("serve")
        .env(SOCKET_ENV, name)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null());
    detach(&mut command);
    command.spawn()?;
    let deadline = Instant::now() + START_TIMEOUT;
    loop {
        match connect(name) {
            Ok(stream) => return Ok(stream),
            Err(err) if Instant::now() >= deadline => return Err(err),
            Err(_) => thread::sleep(Duration::from_millis(25)),
        }
    }
}

/// The daemon must outlive the ssh session that started it.
#[cfg(unix)]
fn detach(command: &mut Command) {
    use std::os::unix::process::CommandExt;
    // SAFETY: `setsid` is async-signal-safe, as `pre_exec` requires.
    unsafe {
        command.pre_exec(|| {
            if libc::setsid() == -1 {
                return Err(io::Error::last_os_error());
            }
            Ok(())
        });
    }
}

#[cfg(windows)]
fn detach(command: &mut Command) {
    use std::os::windows::process::CommandExt;
    const DETACHED_PROCESS: u32 = 0x0000_0008;
    const CREATE_NEW_PROCESS_GROUP: u32 = 0x0000_0200;
    command.creation_flags(DETACHED_PROCESS | CREATE_NEW_PROCESS_GROUP);
}

#[cfg(not(any(unix, windows)))]
fn detach(_command: &mut Command) {}

/// One end of a local socket connection, shared by its reading and writing threads: `&Stream`
/// reads and writes. Dropping one does not shut the connection down; dropping both closes it.
pub struct StreamHalf(Arc<Stream>);

impl Read for StreamHalf {
    fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        (&*self.0).read(buf)
    }
}

impl Write for StreamHalf {
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        (&*self.0).write(buf)
    }

    fn flush(&mut self) -> io::Result<()> {
        (&*self.0).flush()
    }
}

impl StreamHalf {
    /// What ends the connection for both sides from any thread: a blocked read returns, and
    /// the peer (the proxy) reads the end of the stream and exits. Named pipes have no such
    /// call: on Windows it does nothing, and the connection ends when the proxy does.
    pub(crate) fn hang_up(&self) -> impl FnOnce() + Send + 'static {
        let stream = self.0.clone();
        move || shut_down(&stream)
    }
}

#[cfg(unix)]
fn shut_down(stream: &Stream) {
    let Stream::UdSocket(socket) = stream;
    if let Err(err) = socket.inner().shutdown(std::net::Shutdown::Both) {
        log::info!("hanging up: {err}");
    }
}

#[cfg(not(unix))]
fn shut_down(_stream: &Stream) {}

/// A reader and a writer for `stream`, for two threads.
pub fn halves(stream: Stream) -> (StreamHalf, StreamHalf) {
    let shared = Arc::new(stream);
    (StreamHalf(shared.clone()), StreamHalf(shared))
}

/// Relay stdin to the daemon and the daemon to stdout until either side ends.
pub fn proxy(name: &str, server_program: &std::path::Path) -> io::Result<()> {
    let stream = connect_or_start(name, server_program)?;
    let (mut from_daemon, mut to_daemon) = halves(stream);
    let downstream = thread::spawn(move || {
        let mut stdout = io::stdout().lock();
        relay(&mut from_daemon, &mut stdout)
    });
    let upstream = thread::spawn(move || {
        let mut stdin = io::stdin().lock();
        relay(&mut stdin, &mut to_daemon)
    });
    // Whichever side ends first ends the proxy: the other may block on a read forever.
    loop {
        if downstream.is_finished() {
            return downstream.join().unwrap_or(Ok(()));
        }
        if upstream.is_finished() {
            return upstream.join().unwrap_or(Ok(()));
        }
        thread::sleep(Duration::from_millis(50));
    }
}

/// Copy until the reader ends, flushing after each read (stdout buffers by line otherwise).
fn relay(reader: &mut impl Read, writer: &mut impl Write) -> io::Result<()> {
    let mut chunk = vec![0u8; 64 * 1024];
    loop {
        let count = match reader.read(&mut chunk) {
            Ok(0) => return Ok(()),
            Ok(count) => count,
            Err(err) if err.kind() == io::ErrorKind::Interrupted => continue,
            Err(err) => return Err(err),
        };
        writer.write_all(&chunk[..count])?;
        writer.flush()?;
    }
}

/// The ssh command that connects to the server on `host`: `server_program` is how the remote
/// shell finds `agentws-server`. ssh must log in without asking anything (a key or an agent):
/// there is no terminal to ask on.
pub fn ssh_command(host: &str, server_program: &str) -> io::Result<Command> {
    ssh_server_command(host, server_program, "proxy")
}

/// The ssh command that runs `server_program subcommand` on `host` (e.g. `status`), with
/// the same options as [`ssh_command`].
pub fn ssh_server_command(
    host: &str,
    server_program: &str,
    subcommand: &str,
) -> io::Result<Command> {
    let mut command = Command::new("ssh");
    command.args(ssh_args(host, server_program, subcommand)?);
    Ok(command)
}

/// No console window flashing up for a command the windowed app runs (ssh).
pub(crate) fn no_console_window(command: &mut Command) {
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        const CREATE_NO_WINDOW: u32 = 0x0800_0000;
        command.creation_flags(CREATE_NO_WINDOW);
    }
    #[cfg(not(windows))]
    let _ = command;
}

fn ssh_args(host: &str, server_program: &str, subcommand: &str) -> io::Result<Vec<String>> {
    let host = host.trim();
    // A host starting with `-` would be read as an option.
    if host.is_empty() || host.starts_with('-') || host.chars().any(char::is_whitespace) {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            format!("not a host name: {host:?}"),
        ));
    }
    let server_program = server_program.trim();
    if server_program.is_empty() {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "no server program",
        ));
    }
    Ok([
        "-T",
        "-o",
        "BatchMode=yes",
        "-o",
        "ConnectTimeout=10",
        "-o",
        "ServerAliveInterval=15",
        "-o",
        "ServerAliveCountMax=3",
        host,
        // The remote shell runs this line.
        &format!("{server_program} {subcommand}"),
    ]
    .map(String::from)
    .to_vec())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ssh_runs_the_proxy_without_a_terminal_or_prompts() {
        assert_eq!(
            ssh_args(" me@linux-box ", "~/.cargo/bin/agentws-server", "proxy").unwrap(),
            [
                "-T",
                "-o",
                "BatchMode=yes",
                "-o",
                "ConnectTimeout=10",
                "-o",
                "ServerAliveInterval=15",
                "-o",
                "ServerAliveCountMax=3",
                "me@linux-box",
                "~/.cargo/bin/agentws-server proxy",
            ]
        );
    }

    #[test]
    fn hosts_that_would_be_options_are_refused() {
        for host in ["", "  ", "-oProxyCommand=evil", "a b"] {
            assert!(
                ssh_args(host, "agentws-server", "status").is_err(),
                "{host:?}"
            );
        }
        assert!(ssh_args("box", " ", "proxy").is_err());
    }

    #[test]
    fn other_subcommands_run_with_the_same_options() {
        let args = ssh_args("box", "agentws-server", "status").unwrap();
        let proxy = ssh_args("box", "agentws-server", "proxy").unwrap();
        assert_eq!(args.last().unwrap(), "agentws-server status");
        assert_eq!(args[..args.len() - 1], proxy[..proxy.len() - 1]);
    }

    #[test]
    fn relay_copies_everything() {
        let mut out = Vec::new();
        relay(&mut &b"abc\x00def"[..], &mut out).unwrap();
        assert_eq!(out, b"abc\x00def");
    }
}
