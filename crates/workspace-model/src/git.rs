use std::fs;
use std::io::Read;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, ExitStatus, Stdio};
use std::thread;
use std::time::{Duration, Instant};

/// Length of the abbreviated commit id shown for a detached HEAD.
const SHORT_COMMIT_LEN: usize = 7;
/// How long [`lines_added`] waits for its `git` processes in all.
const GIT_TIMEOUT: Duration = Duration::from_secs(10);
/// How often a running `git` is checked for exit.
const POLL_INTERVAL: Duration = Duration::from_millis(10);
/// Untracked files larger than this are not read, and count as nothing.
const MAX_UNTRACKED_FILE_BYTES: u64 = 1 << 20;
/// At most this many untracked files are read.
const MAX_UNTRACKED_FILES: usize = 1000;
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

/// What a repository's HEAD points at.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum GitHead {
    Branch(String),
    /// A checked-out commit, abbreviated.
    Detached(String),
}

impl GitHead {
    /// HEAD of the repository containing `dir`, read from the files under `.git` (no `git`
    /// process). `None` when `dir` is not inside a repository or HEAD cannot be read.
    pub fn read(dir: &Path) -> Option<Self> {
        let git_dir = find_git_dir(dir)?;
        let head = fs::read_to_string(git_dir.join("HEAD")).ok()?;
        Self::parse(&head)
    }

    pub fn name(&self) -> &str {
        match self {
            Self::Branch(name) | Self::Detached(name) => name,
        }
    }

    fn parse(head: &str) -> Option<Self> {
        let head = head.trim();
        if let Some(reference) = head.strip_prefix("ref:") {
            let reference = reference.trim();
            let branch = reference.strip_prefix("refs/heads/").unwrap_or(reference);
            return Some(Self::Branch(branch.to_string()));
        }
        let is_commit =
            head.len() > SHORT_COMMIT_LEN && head.bytes().all(|b| b.is_ascii_hexdigit());
        is_commit.then(|| Self::Detached(head[..SHORT_COMMIT_LEN].to_string()))
    }
}

/// The git directory for `dir`: the nearest `.git` folder walking up, or the folder a `.git`
/// file points to (`gitdir: ...`, used by worktrees and submodules).
fn find_git_dir(dir: &Path) -> Option<PathBuf> {
    dir.ancestors().find_map(|folder| {
        let dot_git = folder.join(".git");
        if dot_git.is_dir() {
            return Some(dot_git);
        }
        let pointer = fs::read_to_string(&dot_git).ok()?;
        let target = pointer.trim().strip_prefix("gitdir:")?.trim();
        Some(folder.join(target))
    })
}

/// Lines added in the working tree of the repository containing `dir`: changes to tracked
/// files, staged or not, compared with HEAD (`git diff HEAD --numstat`), plus the lines of
/// untracked files that are not ignored. Runs `git` processes, so call it off the UI thread and
/// only on demand; gives up after [`GIT_TIMEOUT`]. `None` when git is missing or too slow, `dir`
/// is not in a repository, or HEAD has no commit yet.
pub fn lines_added(dir: &Path) -> Option<u64> {
    let deadline = Instant::now() + GIT_TIMEOUT;
    let tracked = run_git(
        dir,
        &[
            "diff",
            "HEAD",
            "--numstat",
            "--no-ext-diff",
            "--no-textconv",
        ],
        deadline,
    )?;
    // `:/` lists the whole repository, not only `dir`, as the diff does; paths stay relative
    // to `dir`.
    let untracked = run_git(
        dir,
        &[
            "ls-files",
            "--others",
            "--exclude-standard",
            "-z",
            "--",
            ":/",
        ],
        deadline,
    )
    .unwrap_or_default();
    Some(numstat_added(&tracked) + untracked_lines(dir, &untracked))
}

/// The stdout of `git <args>` run in `dir`, or `None` when it fails or is still running at
/// `deadline` (it is then killed).
fn run_git(dir: &Path, args: &[&str], deadline: Instant) -> Option<String> {
    let mut command = Command::new("git");
    command
        // Hovering a tab must not run commands a repository's config names (`core.fsmonitor`).
        // Do not take the index lock to refresh stat info: a git command the user runs at the
        // same time must not fail because of us.
        .args(["-c", "core.fsmonitor=false", "--no-optional-locks"])
        .args(args)
        .current_dir(dir)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::null());
    for name in REPO_ENV {
        command.env_remove(name);
    }
    // A console child of a GUI process gets its own console window, which flashes on hover.
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        command.creation_flags(CREATE_NO_WINDOW);
    }
    let mut child = command.spawn().ok()?;
    let mut stdout = child.stdout.take()?;
    // Read while waiting, so a large output cannot fill the pipe and stall git.
    let reader = thread::spawn(move || {
        let mut output = Vec::new();
        stdout.read_to_end(&mut output).map(|_| output)
    });
    let Some(status) = wait_until(&mut child, deadline) else {
        let _ = child.kill();
        let _ = child.wait();
        return None;
    };
    let output = reader.join().ok()?.ok()?;
    status
        .success()
        .then(|| String::from_utf8_lossy(&output).into_owned())
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

/// Lines in the untracked files of `git ls-files -z` output, relative to `dir`. Like
/// `--numstat`, binary files count as nothing; so do files too large to read.
fn untracked_lines(dir: &Path, listing: &str) -> u64 {
    listing
        .split('\0')
        .filter(|path| !path.is_empty())
        .take(MAX_UNTRACKED_FILES)
        .map(|path| file_lines(&dir.join(path)))
        .sum()
}

fn file_lines(path: &Path) -> u64 {
    let readable = fs::symlink_metadata(path)
        .is_ok_and(|meta| meta.is_file() && meta.len() <= MAX_UNTRACKED_FILE_BYTES);
    if !readable {
        return 0;
    }
    fs::read(path).map_or(0, |content| count_lines(&content))
}

/// Lines in a text file, counting a last line without a newline; `0` for binary content (a NUL
/// byte, as git decides).
fn count_lines(content: &[u8]) -> u64 {
    if content.contains(&0) {
        return 0;
    }
    let newlines = content.iter().filter(|&&byte| byte == b'\n').count() as u64;
    newlines + u64::from(content.last().is_some_and(|&byte| byte != b'\n'))
}

/// Sum of the added-lines column of `git diff --numstat` output (`<added>\t<removed>\t<path>`).
/// Binary files show `-` and count as nothing.
fn numstat_added(numstat: &str) -> u64 {
    numstat
        .lines()
        .filter_map(|line| line.split('\t').next()?.parse::<u64>().ok())
        .sum()
}

#[cfg(test)]
pub(crate) mod tests;
