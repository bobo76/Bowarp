//! What git says about the folder a tab works in: its branch, how many lines changed, which
//! entries are ignored; and, for the Git page, the repository's status, a file's diff, and
//! staging (whole files, hunks or single lines) and committing.

mod actions;
mod diff;
mod patch;
mod runner;
mod selection;
mod status;

use std::collections::{HashMap, HashSet};
use std::fs;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

pub use self::actions::{
    RepoEntry, apply_patch, commit, commit_failure, commit_message_ready, missing_identity,
    repos_of, stage, stage_all, unstage, unstage_all,
};
pub use self::diff::{
    DiffLine, DiffRow, FileDiff, Hunk, LineKind, MAX_DIFF_BYTES, TextDiff, read_diff,
};
pub use self::patch::{LineRef, partial_patch};
pub use self::runner::GitError;
use self::runner::{Access, execute, run_git};
pub use self::selection::{LineSelection, line_index};
pub use self::status::{
    Change, DiffSide, FileStatus, Head, MAX_STATUS_FILES, RepoStatus, StatusRow, read_status,
};
use crate::file_tree::DirEntry;

/// Length of the abbreviated commit id shown for a detached HEAD.
const SHORT_COMMIT_LEN: usize = 7;
/// How long [`working_changes`] waits for its `git` processes in all.
const GIT_TIMEOUT: Duration = Duration::from_secs(10);
/// How long [`ignored_names`] waits for `git check-ignore`.
const CHECK_IGNORE_TIMEOUT: Duration = Duration::from_secs(5);
/// Untracked files larger than this are not read, and count as nothing.
const MAX_UNTRACKED_FILE_BYTES: u64 = 1 << 20;
/// At most this many untracked files are read.
const MAX_UNTRACKED_FILES: usize = 1000;

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

/// How a file differs from HEAD in the working tree (staged or not).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum FileChange {
    Modified,
    Added,
    Deleted,
    Untracked,
    /// A merge conflict to resolve.
    Conflicted,
}

/// A changed file of the repository, with its line counts (`None` for binary files, and for
/// untracked files too large to read or past the first [`MAX_UNTRACKED_FILES`]).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ChangedFile {
    /// Absolute: the repository's top folder joined with git's path.
    pub path: PathBuf,
    pub change: FileChange,
    pub added: Option<u64>,
    pub removed: Option<u64>,
}

/// The files of a repository's working tree that differ from HEAD, in git's order (by path).
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct WorkingChanges {
    pub files: Vec<ChangedFile>,
}

impl WorkingChanges {
    /// Lines added in all: changes to tracked files plus the lines of untracked files.
    pub fn lines_added(&self) -> u64 {
        self.files.iter().filter_map(|file| file.added).sum()
    }

    /// Each changed file's change, and each folder holding one marked as
    /// [`FileChange::Modified`], up to the repository's top folder (not included), for marking
    /// the tree. Deleted files are left out: the tree doesn't show them.
    pub fn marks(&self) -> HashMap<PathBuf, FileChange> {
        let mut marks = HashMap::new();
        for file in &self.files {
            if file.change == FileChange::Deleted {
                continue;
            }
            marks.insert(file.path.clone(), file.change);
            for folder in file.path.ancestors().skip(1) {
                if marks.contains_key(folder) {
                    break;
                }
                marks.insert(folder.to_path_buf(), FileChange::Modified);
            }
        }
        marks
    }
}

/// Lines added in the working tree of the repository containing `dir` (see
/// [`WorkingChanges::lines_added`]). Same conditions and cost as [`working_changes`].
pub fn lines_added(dir: &Path) -> Option<u64> {
    working_changes(dir).map(|changes| changes.lines_added())
}

/// The changed files of the repository containing `dir`, compared with HEAD: tracked files
/// changed, staged or not (`git status`, `git diff HEAD --numstat`), and untracked files that
/// are not ignored, with their lines. Runs `git` processes, so call it off the UI thread and
/// only on demand; gives up after [`GIT_TIMEOUT`]. `None` when git is missing or too slow,
/// `dir` is not in a repository, or HEAD has no commit yet.
pub fn working_changes(dir: &Path) -> Option<WorkingChanges> {
    let deadline = Instant::now() + GIT_TIMEOUT;
    let top = repo_top(dir, deadline)?;
    let numstat = run_git(
        &top,
        &[
            "diff",
            "HEAD",
            "--numstat",
            "-z",
            "--no-renames",
            "--no-ext-diff",
            "--no-textconv",
        ],
        deadline,
    )?;
    let status = porcelain_status(&top, deadline)?;
    let counts = numstat_counts(&numstat);
    let mut untracked_read = 0;
    let files = status_entries(&status)
        .into_iter()
        .map(|(path, change)| {
            let full = top.join(native_separators(&path, cfg!(windows)));
            let (added, removed) = if change == FileChange::Untracked {
                untracked_read += 1;
                let lines = (untracked_read <= MAX_UNTRACKED_FILES)
                    .then(|| file_lines(&full))
                    .flatten();
                (lines, lines.map(|_| 0))
            } else {
                counts.get(path.as_str()).copied().unwrap_or((None, None))
            };
            ChangedFile {
                path: full,
                change,
                added,
                removed,
            }
        })
        .collect();
    Some(WorkingChanges { files })
}

/// The changed files of the repository containing `dir`, like [`working_changes`] but
/// without line counts (all `None`): one `git status`, no diff and no file reads, for when
/// only the kind of change matters. Unlike [`working_changes`], works before the first commit.
/// Call it off the UI thread; `None` when git is missing or too slow, or `dir` is not in a
/// repository.
pub fn changed_files(dir: &Path) -> Option<WorkingChanges> {
    let deadline = Instant::now() + GIT_TIMEOUT;
    let top = repo_top(dir, deadline)?;
    let status = porcelain_status(&top, deadline)?;
    let files = status_entries(&status)
        .into_iter()
        .map(|(path, change)| ChangedFile {
            path: top.join(native_separators(&path, cfg!(windows))),
            change,
            added: None,
            removed: None,
        })
        .collect();
    Some(WorkingChanges { files })
}

/// The top folder of the repository containing `dir`, with the platform's separators.
fn repo_top(dir: &Path, deadline: Instant) -> Option<PathBuf> {
    let top = run_git(dir, &["rev-parse", "--show-toplevel"], deadline)?;
    Some(PathBuf::from(native_separators(
        top.trim_end_matches(['\n', '\r']),
        cfg!(windows),
    )))
}

/// `git status` in machine form for the repository at `top`, untracked files listed one by
/// one, read by [`status_entries`].
fn porcelain_status(top: &Path, deadline: Instant) -> Option<String> {
    run_git(
        top,
        &[
            "status",
            "--porcelain=v1",
            "-z",
            "--untracked-files=all",
            "--no-renames",
        ],
        deadline,
    )
}

/// `path` as git prints it (always `/`-separated, even on Windows) with the platform's separator,
/// so paths built from it display like the ones the user typed.
fn native_separators(path: &str, windows: bool) -> String {
    if windows {
        path.replace('/', "\\")
    } else {
        path.to_string()
    }
}

/// The entries of `git status --porcelain=v1 -z --no-renames` output: (path, change). Ignored
/// entries (`!!`) are skipped.
fn status_entries(output: &str) -> Vec<(String, FileChange)> {
    output
        .split('\0')
        .filter_map(|entry| {
            let (code, path) = (entry.get(..2)?, entry.get(3..)?);
            if path.is_empty() {
                return None;
            }
            let (index, tree) = (code.as_bytes()[0], code.as_bytes()[1]);
            let change = match (index, tree) {
                (b'?', b'?') => FileChange::Untracked,
                (b'!', b'!') => return None,
                (b'U', _) | (_, b'U') | (b'A', b'A') | (b'D', b'D') => FileChange::Conflicted,
                (b'D', _) | (_, b'D') => FileChange::Deleted,
                (b'A', _) => FileChange::Added,
                _ => FileChange::Modified,
            };
            Some((path.to_string(), change))
        })
        .collect()
}

/// (added, removed) per path in `git diff --numstat -z --no-renames` output
/// (`<added>\t<removed>\t<path>\0`). Binary files show `-`: `None`.
fn numstat_counts(output: &str) -> HashMap<&str, (Option<u64>, Option<u64>)> {
    output
        .split('\0')
        .filter_map(|entry| {
            let mut fields = entry.splitn(3, '\t');
            let added = fields.next()?.parse().ok();
            let removed = fields.next()?.parse().ok();
            let path = fields.next().filter(|path| !path.is_empty())?;
            Some((path, (added, removed)))
        })
        .collect()
}

/// The names among `entries` (a folder's listing) that git ignores in `folder`
/// (`git check-ignore`), the repository's `.git` included. Tracked files are not ignored. Runs a
/// `git` process, so call it off the UI thread; gives up after [`CHECK_IGNORE_TIMEOUT`]. `None`
/// when git is missing, fails or is too slow, or `folder` is not in a work tree.
pub fn ignored_names(folder: &Path, entries: &[DirEntry]) -> Option<HashSet<String>> {
    if entries.is_empty() {
        return Some(HashSet::new());
    }
    let deadline = Instant::now() + CHECK_IGNORE_TIMEOUT;
    let input = check_ignore_input(entries);
    let output = execute(
        folder,
        &["check-ignore", "--stdin", "-z"],
        Some(input),
        Access::Read,
        deadline,
    )
    .ok()?;
    // 0: some are ignored, 1: none are; 128 (not in a work tree, ...) is a failure.
    matches!(output.status.code(), Some(0 | 1))
        .then(|| ignored_from_output(&output.stdout, entries))
}

/// `git check-ignore --stdin -z` input: each name as `./<name>`, so a name starting with `:` is
/// not read as pathspec magic (which fails the whole run).
fn check_ignore_input(entries: &[DirEntry]) -> Vec<u8> {
    let mut input = Vec::new();
    for entry in entries {
        input.extend_from_slice(b"./");
        input.extend_from_slice(entry.name.as_bytes());
        input.push(0);
    }
    input
}

/// The ignored names in `git check-ignore -z` output (the paths given that are ignored), plus
/// `.git`, which git never reports, when the listing has it.
fn ignored_from_output(output: &[u8], entries: &[DirEntry]) -> HashSet<String> {
    let reported: HashSet<&[u8]> = output
        .split(|&byte| byte == 0)
        .filter_map(|path| path.strip_prefix(b"./"))
        .collect();
    entries
        .iter()
        .filter(|entry| entry.name == ".git" || reported.contains(entry.name.as_bytes()))
        .map(|entry| entry.name.clone())
        .collect()
}

/// The lines of an untracked file; `None` when it is binary, too large or unreadable (like
/// `--numstat`, which shows `-` for binary files).
fn file_lines(path: &Path) -> Option<u64> {
    let readable = fs::symlink_metadata(path)
        .is_ok_and(|meta| meta.is_file() && meta.len() <= MAX_UNTRACKED_FILE_BYTES);
    if !readable {
        return None;
    }
    let content = fs::read(path).ok()?;
    (!content.contains(&0)).then(|| count_lines(&content))
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

#[cfg(test)]
pub(crate) mod tests;
