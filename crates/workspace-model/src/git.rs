//! What git says about the folder a tab works in: its branch, how many lines changed, which
//! entries are ignored; and, for the Git page, the repository's status, a file's diff, and
//! staging (whole files, hunks or single lines) and committing.

mod actions;
mod diff;
mod patch;
mod runner;
mod selection;
mod status;

use std::collections::HashSet;
use std::fs;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

pub use self::actions::{
    RepoEntry, apply_patch, commit, commit_failure, commit_message_ready, missing_identity,
    repos_of, stage, stage_all, unstage, unstage_all,
};
pub use self::diff::{DiffLine, DiffRow, FileDiff, Hunk, LineKind, TextDiff, read_diff};
pub use self::patch::{LineRef, partial_patch};
pub use self::runner::GitError;
use self::runner::{Access, execute, run_git};
pub use self::selection::{LineSelection, line_index};
pub use self::status::{Change, DiffSide, FileStatus, RepoStatus, StatusRow, read_status};
use crate::file_tree::DirEntry;

/// Length of the abbreviated commit id shown for a detached HEAD.
const SHORT_COMMIT_LEN: usize = 7;
/// How long [`lines_added`] waits for its `git` processes in all.
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
