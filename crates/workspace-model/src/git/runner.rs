//! Running `git` without a console window, with a deadline, its environment cleared of
//! variables that would redirect it to another repository.

use std::fmt;
use std::io::{ErrorKind, Read, Write};
use std::path::Path;
use std::process::{Child, ChildStdin, Command, ExitStatus, Stdio};
use std::sync::mpsc::{self, Receiver};
use std::sync::{Arc, Mutex, PoisonError};
use std::thread;
use std::time::{Duration, Instant};

/// How often a running `git` is checked for exit.
const POLL_INTERVAL: Duration = Duration::from_millis(10);
/// How long the output is still read once `git` has exited. Its pipes close then, unless a
/// process a hook started in the background holds them: its output is not waited for.
const DRAIN_GRACE: Duration = Duration::from_secs(1);
/// How much of the output is read at a time.
const READ_CHUNK: usize = 64 * 1024;
/// Win32 process creation flag: start a console program without a console window.
#[cfg(windows)]
const CREATE_NO_WINDOW: u32 = 0x0800_0000;
/// Variables that would point every `git` at one repository, whatever folder it runs in.
const REPO_ENV: [&str; 4] = [
    "GIT_DIR",
    "GIT_WORK_TREE",
    "GIT_INDEX_FILE",
    "GIT_COMMON_DIR",
];

/// Why a `git` command gave no result.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum GitError {
    /// `git` could not be started (not installed, or not on `PATH`).
    Missing,
    /// `git` was still running at its deadline and was stopped.
    TimedOut,
    /// `git` exited with an error; its message (stderr, else stdout), trimmed.
    Failed(String),
}

impl fmt::Display for GitError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Missing => f.write_str("git could not be started; is it installed?"),
            Self::TimedOut => f.write_str("git took too long and was stopped"),
            Self::Failed(message) if message.is_empty() => f.write_str("git failed"),
            Self::Failed(message) => f.write_str(message),
        }
    }
}

impl std::error::Error for GitError {}

/// Whether a command may take locks and change the repository.
#[derive(Clone, Copy, PartialEq, Eq)]
pub(super) enum Access {
    /// Takes no optional lock, so a git command the user runs at the same time never fails
    /// because of us.
    Read,
    /// Changes the index or makes a commit (an action the user asked for).
    Write,
}

/// What a finished `git` printed.
pub(super) struct Output {
    pub status: ExitStatus,
    pub stdout: Vec<u8>,
    pub stderr: Vec<u8>,
}

impl Output {
    /// Stdout of a successful run, else the error git printed.
    pub fn into_result(self) -> Result<Vec<u8>, GitError> {
        if self.status.success() {
            return Ok(self.stdout);
        }
        // `git commit` with nothing staged explains itself on stdout.
        let message = if self.stderr.iter().all(u8::is_ascii_whitespace) {
            self.stdout
        } else {
            self.stderr
        };
        Err(GitError::Failed(
            String::from_utf8_lossy(&message).trim().to_string(),
        ))
    }
}

/// Run `git <args>` in `dir` with `input` on its stdin; stopped (and `TimedOut`) when still
/// running at `deadline`.
pub(super) fn execute(
    dir: &Path,
    args: &[&str],
    input: Option<Vec<u8>>,
    access: Access,
    deadline: Instant,
) -> Result<Output, GitError> {
    execute_capped(dir, args, input, access, deadline, usize::MAX)
}

/// [`execute`], keeping at most `max_stdout` bytes of stdout (the rest is read and dropped, so
/// git never stalls on a full pipe).
pub(super) fn execute_capped(
    dir: &Path,
    args: &[&str],
    input: Option<Vec<u8>>,
    access: Access,
    deadline: Instant,
    max_stdout: usize,
) -> Result<Output, GitError> {
    let stdin = if input.is_some() {
        Stdio::piped()
    } else {
        Stdio::null()
    };
    let mut child = git_command(dir, args, access, stdin)
        .spawn()
        .map_err(|_| GitError::Missing)?;
    feed(child.stdin.take(), input);
    // Read while waiting, so a large output cannot fill a pipe and stall git.
    let stdout = Capture::start(child.stdout.take(), max_stdout);
    let stderr = Capture::start(child.stderr.take(), usize::MAX);
    let Some(status) = wait_until(&mut child, deadline) else {
        let _ = child.kill();
        let _ = child.wait();
        return Err(GitError::TimedOut);
    };
    let drained = Instant::now() + DRAIN_GRACE;
    Ok(Output {
        status,
        stdout: stdout.finish(drained),
        stderr: stderr.finish(drained),
    })
}

/// `git <args>` in `dir`, with output piped, no console window and no variable pointing it at
/// another repository.
fn git_command(dir: &Path, args: &[&str], access: Access, stdin: Stdio) -> Command {
    let mut command = Command::new("git");
    // A repository's config must not run commands (`core.fsmonitor`) behind the user's back.
    command.args(["-c", "core.fsmonitor=false"]);
    if access == Access::Read {
        command.arg("--no-optional-locks");
    }
    command
        .args(args)
        .current_dir(dir)
        .stdin(stdin)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        // Nobody can answer a credential prompt: fail instead of waiting for the deadline.
        .env("GIT_TERMINAL_PROMPT", "0");
    for name in REPO_ENV {
        command.env_remove(name);
    }
    // A console child of a GUI process gets its own console window, which flashes.
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        command.creation_flags(CREATE_NO_WINDOW);
    }
    command
}

/// Write `input` to git's `stdin` on a thread, so a git that stops reading cannot outlast the
/// deadline; dropping stdin at the end tells git the input is complete. A write error (git
/// exited) ends it.
fn feed(stdin: Option<ChildStdin>, input: Option<Vec<u8>>) {
    if let (Some(mut stdin), Some(input)) = (stdin, input) {
        thread::spawn(move || {
            let _ = stdin.write_all(&input);
        });
    }
}

/// The stdout of a read-only `git <args>` run in `dir`, or `None` when it fails or is still
/// running at `deadline`.
pub(super) fn run_git(dir: &Path, args: &[&str], deadline: Instant) -> Option<String> {
    let stdout = execute(dir, args, None, Access::Read, deadline)
        .ok()?
        .into_result()
        .ok()?;
    Some(String::from_utf8_lossy(&stdout).into_owned())
}

/// A pipe read on its own thread until it closes.
struct Capture {
    /// What was read so far, at most the limit.
    read: Arc<Mutex<Vec<u8>>>,
    /// Gets a message (or disconnects) when the pipe has closed.
    closed: Receiver<()>,
}

impl Capture {
    /// Read `pipe`, keeping its first `limit` bytes.
    fn start(pipe: Option<impl Read + Send + 'static>, limit: usize) -> Self {
        let read = Arc::new(Mutex::new(Vec::new()));
        let (done, closed) = mpsc::channel();
        let output = Arc::clone(&read);
        thread::spawn(move || {
            if let Some(pipe) = pipe {
                drain(pipe, limit, &output);
            }
            let _ = done.send(());
        });
        Self { read, closed }
    }

    /// What was read once the pipe closed, or what was read by `until` if it is still open.
    fn finish(self, until: Instant) -> Vec<u8> {
        let _ = self
            .closed
            .recv_timeout(until.saturating_duration_since(Instant::now()));
        std::mem::take(&mut *self.read.lock().unwrap_or_else(PoisonError::into_inner))
    }
}

/// Read `pipe` to its end into `output`, dropping what goes past `limit`.
fn drain(mut pipe: impl Read, limit: usize, output: &Mutex<Vec<u8>>) {
    let mut chunk = vec![0; READ_CHUNK];
    loop {
        let count = match pipe.read(&mut chunk) {
            Ok(0) => return,
            Ok(count) => count,
            Err(err) if err.kind() == ErrorKind::Interrupted => continue,
            Err(_) => return,
        };
        let mut output = output.lock().unwrap_or_else(PoisonError::into_inner);
        let kept = count.min(limit.saturating_sub(output.len()));
        output.extend_from_slice(&chunk[..kept]);
    }
}

/// The child's exit status, or `None` when it has not exited by `deadline`.
fn wait_until(child: &mut Child, deadline: Instant) -> Option<ExitStatus> {
    loop {
        if let Some(status) = child.try_wait().ok()? {
            return Some(status);
        }
        if Instant::now() >= deadline {
            return None;
        }
        thread::sleep(POLL_INTERVAL);
    }
}

#[cfg(test)]
mod tests;
