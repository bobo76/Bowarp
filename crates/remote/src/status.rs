//! A host's server state: what `agentws-server status` prints there, and the app's check of
//! it over ssh ([`check`]), classified ([`CheckOutcome`]) and put in words ([`status_line`]).
//!
//! `status` prints one `key=value` per line (`version`, `protocol`, `daemon`, `sessions`);
//! readers ignore keys they don't know, so later servers can add some.

use std::io::{self, Read, Write};
use std::process::{Command, ExitStatus, Stdio};
use std::sync::mpsc::{self, Receiver};
use std::thread;
use std::time::{Duration, Instant};

use crate::client::Client;
use crate::protocol::PROTOCOL_VERSION;
use crate::transport;

/// How long a whole check may take (ssh's own connect timeout is 10 s) before it gives up and
/// kills ssh.
pub const CHECK_TIMEOUT: Duration = Duration::from_secs(20);
/// A check keeps at most this much of ssh's output and error output each.
const OUTPUT_LIMIT: usize = 64 * 1024;
/// How long output may still come once ssh ended: a process it left behind (a `ControlPersist`
/// master) can hold its pipes open long after.
const OUTPUT_GRACE: Duration = Duration::from_millis(500);
const POLL_INTERVAL: Duration = Duration::from_millis(50);
/// How long `status` waits for the daemon's answer before reporting it as not answering.
pub const ASK_TIMEOUT: Duration = Duration::from_secs(5);

/// This user's daemon on the host.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DaemonState {
    Running,
    NotRunning,
    /// It runs but speaks another protocol than the installed `agentws-server`.
    Incompatible,
}

impl DaemonState {
    fn key(self) -> &'static str {
        match self {
            Self::Running => "running",
            Self::NotRunning => "not-running",
            Self::Incompatible => "incompatible",
        }
    }

    fn from_key(key: &str) -> Option<Self> {
        [Self::Running, Self::NotRunning, Self::Incompatible]
            .into_iter()
            .find(|state| state.key() == key)
    }
}

/// What `agentws-server status` reports.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ServerStatus {
    /// The installed program's version (not the running daemon's); empty when not given.
    pub version: String,
    /// The installed program's protocol.
    pub protocol: Option<u16>,
    pub daemon: DaemonState,
    /// The sessions the daemon holds (as `list` shows them), when it runs.
    pub sessions: Option<usize>,
}

impl ServerStatus {
    /// The `key=value` lines `status` prints.
    pub fn lines(&self) -> String {
        let mut lines = format!("version={}\n", self.version);
        if let Some(protocol) = self.protocol {
            lines.push_str(&format!("protocol={protocol}\n"));
        }
        lines.push_str(&format!("daemon={}\n", self.daemon.key()));
        if let Some(sessions) = self.sessions {
            lines.push_str(&format!("sessions={sessions}\n"));
        }
        lines
    }

    /// Read `status`'s lines; `None` without a known `daemon` state. Unknown keys and lines
    /// without `=` are skipped.
    pub fn parse(text: &str) -> Option<Self> {
        let mut version = String::new();
        let mut protocol = None;
        let mut daemon = None;
        let mut sessions = None;
        for (key, value) in text.lines().filter_map(|line| line.split_once('=')) {
            let value = value.trim();
            match key.trim() {
                "version" => version = value.to_owned(),
                "protocol" => protocol = value.parse().ok(),
                "daemon" => daemon = DaemonState::from_key(value),
                "sessions" => sessions = value.parse().ok(),
                _ => {}
            }
        }
        Some(Self {
            version,
            protocol,
            daemon: daemon?,
            sessions,
        })
    }
}

/// How a check of a host ended.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum CheckOutcome {
    Status(ServerStatus),
    /// ssh could not log in (it exits with 255); `stderr` is what it said.
    Unreachable {
        stderr: String,
    },
    /// The remote shell found no server program (exit 127).
    NotFound,
    /// The server answered with its usage: it has no `status` command.
    TooOld,
    /// Nothing within [`CHECK_TIMEOUT`].
    NoAnswer,
    /// Anything else: `status` failed, ssh could not run, or the answer made no sense.
    Failed {
        stderr: String,
    },
}

/// What a finished ssh run left: its exit code (`None`: killed by a signal) and output.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct CommandOutput {
    pub exit: Option<i32>,
    pub stdout: String,
    pub stderr: String,
}

/// What the run of `agentws-server status` through ssh says about the host; `None` is a run
/// that timed out.
pub fn classify(output: Option<CommandOutput>) -> CheckOutcome {
    let Some(output) = output else {
        return CheckOutcome::NoAnswer;
    };
    match output.exit {
        Some(0) => ServerStatus::parse(&output.stdout).map_or_else(
            || CheckOutcome::Failed {
                stderr: format!(
                    "not a status answer: {}",
                    last_line(&output.stdout).unwrap_or("(nothing)")
                ),
            },
            CheckOutcome::Status,
        ),
        Some(127) => CheckOutcome::NotFound,
        Some(255) => CheckOutcome::Unreachable {
            stderr: stderr_or(output.stderr, "ssh exited with 255"),
        },
        _ if output.stderr.contains("usage: agentws-server") => CheckOutcome::TooOld,
        Some(code) => CheckOutcome::Failed {
            stderr: stderr_or(output.stderr, &format!("exited with {code}")),
        },
        None => CheckOutcome::Failed {
            stderr: stderr_or(output.stderr, "ssh was killed"),
        },
    }
}

fn stderr_or(stderr: String, fallback: &str) -> String {
    match stderr.trim() {
        "" => fallback.to_owned(),
        text => text.to_owned(),
    }
}

/// The last non-empty line of `text`, trimmed.
pub fn last_line(text: &str) -> Option<&str> {
    text.lines().map(str::trim).rfind(|line| !line.is_empty())
}

/// Whether failing to connect to the daemon's socket means no daemon listens on it.
pub fn daemon_absent(err: &io::Error) -> bool {
    matches!(
        err.kind(),
        io::ErrorKind::NotFound | io::ErrorKind::ConnectionRefused
    )
}

/// The daemon state and its session count from a connection to the daemon's socket; an error
/// when it is no agentws server, asking it failed or it gave no answer within `timeout`.
pub fn daemon_state(
    reader: impl Read + Send + 'static,
    writer: impl Write + Send + 'static,
    timeout: Duration,
) -> io::Result<(DaemonState, Option<usize>)> {
    let (sender, answer) = mpsc::channel();
    // The socket has no portable read timeout: ask from a thread, left blocked on a timeout.
    thread::Builder::new()
        .name("agentws-status-ask".into())
        .spawn(move || {
            let _ = sender.send(ask_daemon(reader, writer));
        })?;
    match answer.recv_timeout(timeout) {
        Ok(state) => state,
        Err(mpsc::RecvTimeoutError::Timeout) => Err(io::Error::new(
            io::ErrorKind::TimedOut,
            "the server did not answer",
        )),
        Err(mpsc::RecvTimeoutError::Disconnected) => {
            Err(io::Error::other("asking the server stopped unexpectedly"))
        }
    }
}

fn ask_daemon(
    reader: impl Read + Send + 'static,
    writer: impl Write + Send + 'static,
) -> io::Result<(DaemonState, Option<usize>)> {
    match Client::connect(reader, writer, None, Box::new(|_| {})) {
        Ok(client) => Ok((DaemonState::Running, Some(client.list()?.len()))),
        // The server refuses a hello in another protocol (`protocol.rs`).
        Err(err) if err.kind() == io::ErrorKind::ConnectionRefused => {
            Ok((DaemonState::Incompatible, None))
        }
        Err(err) => Err(err),
    }
}

/// Run `server_program status` on `host` through ssh and classify the answer. Blocks for up
/// to `timeout`, then kills ssh.
pub fn check(host: &str, server_program: &str, timeout: Duration) -> CheckOutcome {
    let failed = |err: io::Error| CheckOutcome::Failed {
        stderr: err.to_string(),
    };
    let command = match transport::ssh_server_command(host, server_program, "status") {
        Ok(command) => command,
        Err(err) => return failed(err),
    };
    match run(command, timeout) {
        Ok(output) => classify(output),
        Err(err) => failed(io::Error::new(
            err.kind(),
            format!("could not run ssh: {err}"),
        )),
    }
}

/// Run `command` with no input, collecting its output; `None` when it did not end within
/// `timeout` (it is killed then).
fn run(mut command: Command, timeout: Duration) -> io::Result<Option<CommandOutput>> {
    command
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    transport::no_console_window(&mut command);
    let deadline = Instant::now() + timeout;
    let mut child = command.spawn()?;
    let stdout = read_in_background(child.stdout.take());
    let stderr = read_in_background(child.stderr.take());
    let waited = wait_until(&mut child, deadline);
    let Ok(Some(status)) = waited else {
        let _ = child.kill();
        let _ = child.wait();
        return waited.map(|_| None);
    };
    // A process ssh left behind may hold the pipes open: wait for them only briefly.
    let output_deadline = deadline.min(Instant::now() + OUTPUT_GRACE);
    Ok(Some(CommandOutput {
        exit: status.code(),
        stdout: collect_output(&stdout, output_deadline),
        stderr: collect_output(&stderr, output_deadline),
    }))
}

/// The child's exit status, or `None` once `deadline` passed.
fn wait_until(
    child: &mut std::process::Child,
    deadline: Instant,
) -> io::Result<Option<ExitStatus>> {
    loop {
        if let Some(status) = child.try_wait()? {
            return Ok(Some(status));
        }
        if Instant::now() >= deadline {
            return Ok(None);
        }
        thread::sleep(POLL_INTERVAL);
    }
}

/// The first [`OUTPUT_LIMIT`] bytes `pipe` gives, in chunks as they come, from a thread of its
/// own. It reads on to the end so the writer never fails on a closed pipe; the channel closes
/// then.
fn read_in_background(pipe: Option<impl Read + Send + 'static>) -> Receiver<Vec<u8>> {
    let (sender, receiver) = mpsc::channel();
    let Some(mut pipe) = pipe else {
        return receiver;
    };
    let _ = thread::Builder::new()
        .name("agentws-status-output".into())
        .spawn(move || {
            let mut kept = 0;
            let mut chunk = [0; 8192];
            while let Ok(read) = pipe.read(&mut chunk)
                && read > 0
            {
                let keep = read.min(OUTPUT_LIMIT.saturating_sub(kept));
                kept += keep;
                if keep > 0 {
                    let _ = sender.send(chunk[..keep].to_vec());
                }
            }
        });
    receiver
}

/// What the reader behind `chunks` got until it ended or `deadline` passed.
fn collect_output(chunks: &Receiver<Vec<u8>>, deadline: Instant) -> String {
    let mut bytes = Vec::new();
    loop {
        let remaining = deadline.saturating_duration_since(Instant::now());
        match chunks.recv_timeout(remaining) {
            Ok(chunk) => bytes.extend(chunk),
            Err(_) => break,
        }
    }
    String::from_utf8_lossy(&bytes).into_owned()
}

/// A host's check on the settings page: running, or how it ended.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum HostCheck {
    Checking,
    Done(CheckOutcome),
}

/// How a status line reads at a glance (its icon); never the only sign, the text says it too.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum StatusTone {
    /// Connected, or the server runs.
    Fine,
    /// Not checked, or the server doesn't run.
    Neutral,
    Checking,
    /// Something keeps remote tabs from opening.
    Problem,
}

/// A host's server state as the settings page shows it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct StatusLine {
    pub tone: StatusTone,
    pub text: String,
    /// What to do about it, shown after the text.
    pub hint: Option<&'static str>,
    /// ssh's whole error output, for a tooltip.
    pub details: Option<String>,
}

/// The status line of a host: `connected` is whether the app's connection to it is live,
/// `check` its last check this run.
pub fn status_line(connected: bool, check: Option<&HostCheck>) -> StatusLine {
    status_line_for(connected, check, PROTOCOL_VERSION)
}

fn status_line_for(connected: bool, check: Option<&HostCheck>, app_protocol: u16) -> StatusLine {
    let mut line = match check {
        None => plain(StatusTone::Neutral, "Not checked".to_owned()),
        Some(HostCheck::Checking) => plain(StatusTone::Checking, "Checking…".to_owned()),
        Some(HostCheck::Done(outcome)) => outcome_line(outcome, app_protocol),
    };
    if connected {
        line.text = format!("Connected · {}", line.text);
        if line.tone == StatusTone::Neutral {
            line.tone = StatusTone::Fine;
        }
    }
    line
}

fn plain(tone: StatusTone, text: String) -> StatusLine {
    StatusLine {
        tone,
        text,
        hint: None,
        details: None,
    }
}

fn outcome_line(outcome: &CheckOutcome, app_protocol: u16) -> StatusLine {
    match outcome {
        CheckOutcome::Status(status) => server_line(status, app_protocol),
        CheckOutcome::Unreachable { stderr } => with_details("Unreachable", stderr),
        CheckOutcome::NotFound => StatusLine {
            hint: Some("set Server program"),
            ..plain(StatusTone::Problem, "agentws-server not found".to_owned())
        },
        CheckOutcome::TooOld => plain(
            StatusTone::Problem,
            "Server too old to report status: update it".to_owned(),
        ),
        CheckOutcome::NoAnswer => {
            plain(StatusTone::Problem, "No answer from the server".to_owned())
        }
        CheckOutcome::Failed { stderr } => with_details("Check failed", stderr),
    }
}

/// `label: ` and the last line of `stderr`, the whole of it as details.
fn with_details(label: &str, stderr: &str) -> StatusLine {
    StatusLine {
        details: Some(stderr.trim().to_owned()),
        ..plain(
            StatusTone::Problem,
            format!("{label}: {}", last_line(stderr).unwrap_or("no message")),
        )
    }
}

fn server_line(status: &ServerStatus, app_protocol: u16) -> StatusLine {
    let mut line = match status.daemon {
        DaemonState::Running => {
            let sessions = status.sessions.unwrap_or(0);
            let noun = if sessions == 1 { "session" } else { "sessions" };
            plain(
                StatusTone::Fine,
                format!("Server running, {sessions} {noun}"),
            )
        }
        DaemonState::NotRunning => plain(StatusTone::Neutral, "Server not running".to_owned()),
        DaemonState::Incompatible => plain(
            StatusTone::Problem,
            "A server of another version is running: remote tabs can't open".to_owned(),
        ),
    };
    if !status.version.is_empty() {
        line.text
            .push_str(&format!(" · version {}", status.version));
    }
    // An incompatible daemon is already reported; the installed program's protocol says
    // nothing about it.
    let judged_by_program = status.daemon != DaemonState::Incompatible;
    if let Some(protocol) = status
        .protocol
        .filter(|protocol| judged_by_program && *protocol != app_protocol)
    {
        line.text.push_str(&format!(
            " · speaks protocol {protocol}, this app {app_protocol}: remote tabs can't open"
        ));
        line.tone = StatusTone::Problem;
    }
    line
}

#[cfg(test)]
mod tests;
