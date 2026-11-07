//! A repository's changed files, staged and not, from `git status --porcelain=v2`.

use std::collections::HashSet;
use std::path::Path;
use std::time::{Duration, Instant};

use super::runner::{Access, GitError, execute};

/// How long [`read_status`] waits for `git status`.
const STATUS_TIMEOUT: Duration = Duration::from_secs(20);
/// At most this many files are listed; a repository with more shows the first ones.
const MAX_STATUS_FILES: usize = 10_000;
/// `# branch.oid` of a repository with no commit yet.
const INITIAL_OID: &str = "(initial)";
/// `# branch.head` of a detached HEAD.
const DETACHED_HEAD: &str = "(detached)";

/// How a file differs on one side: in the index from HEAD (staged), or in the working tree from
/// the index (unstaged).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Change {
    Modified,
    TypeChanged,
    Added,
    Deleted,
    Renamed,
    Copied,
    /// Not tracked and not ignored (unstaged side only).
    Untracked,
    /// Both sides of a merge changed it (unstaged side only); git cannot stage it in parts.
    Conflicted,
}

impl Change {
    /// The letter VS Code and `git status --short` show for it.
    pub fn letter(self) -> char {
        match self {
            Self::Modified => 'M',
            Self::TypeChanged => 'T',
            Self::Added => 'A',
            Self::Deleted => 'D',
            Self::Renamed => 'R',
            Self::Copied => 'C',
            Self::Untracked => 'U',
            Self::Conflicted => '!',
        }
    }

    fn from_code(code: u8) -> Option<Self> {
        match code {
            b'M' => Some(Self::Modified),
            b'T' => Some(Self::TypeChanged),
            b'A' => Some(Self::Added),
            b'D' => Some(Self::Deleted),
            b'R' => Some(Self::Renamed),
            b'C' => Some(Self::Copied),
            b'U' => Some(Self::Conflicted),
            _ => None,
        }
    }
}

/// Which of a file's two diffs: the staged one (HEAD to index) or the unstaged one (index to
/// working tree).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum DiffSide {
    Staged,
    Unstaged,
}

/// A changed file, relative to the repository root, with `/` separators.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct FileStatus {
    pub path: String,
    /// The path before a staged rename or copy.
    pub orig_path: Option<String>,
    pub staged: Option<Change>,
    pub unstaged: Option<Change>,
}

impl FileStatus {
    /// Its change on `side`, if it has one.
    pub fn change(&self, side: DiffSide) -> Option<Change> {
        match side {
            DiffSide::Staged => self.staged,
            DiffSide::Unstaged => self.unstaged,
        }
    }

    /// Whether lines of it can be staged or unstaged one by one on `side` (a text diff git can
    /// apply in parts: not untracked, not a merge conflict, not a type change, whose diff
    /// removes one file and adds another).
    pub fn stages_by_line(&self, side: DiffSide) -> bool {
        !matches!(
            self.change(side),
            None | Some(Change::Untracked | Change::Conflicted | Change::TypeChanged)
        )
    }
}

/// A row of the Git page's list of changes.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum StatusRow {
    /// A group's header ("Staged Changes" or "Changes") and how many files it has.
    Header { side: DiffSide, count: usize },
    /// `RepoStatus::files[index]`, in the group of `side`.
    File { side: DiffSide, index: usize },
}

/// What `git status` says about a repository.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct RepoStatus {
    /// The branch, `None` for a detached HEAD.
    pub branch: Option<String>,
    /// Whether HEAD has a commit (a new repository has none).
    pub has_commits: bool,
    pub files: Vec<FileStatus>,
    /// More than [`MAX_STATUS_FILES`] changed: only the first ones are in `files`.
    pub truncated: bool,
}

impl RepoStatus {
    /// The files with a change on `side`, in git's order (by path).
    pub fn files_on(&self, side: DiffSide) -> impl Iterator<Item = &FileStatus> {
        self.files
            .iter()
            .filter(move |file| file.change(side).is_some())
    }

    /// The list's rows, as VS Code shows them: the staged files, then the others, each group
    /// under its header and left out when empty; a `collapsed` group shows its header only.
    pub fn rows(&self, collapsed: &HashSet<DiffSide>) -> Vec<StatusRow> {
        let mut rows = Vec::new();
        for side in [DiffSide::Staged, DiffSide::Unstaged] {
            let files: Vec<usize> = self
                .files
                .iter()
                .enumerate()
                .filter(|(_, file)| file.change(side).is_some())
                .map(|(index, _)| index)
                .collect();
            if files.is_empty() {
                continue;
            }
            rows.push(StatusRow::Header {
                side,
                count: files.len(),
            });
            if !collapsed.contains(&side) {
                rows.extend(
                    files
                        .into_iter()
                        .map(|index| StatusRow::File { side, index }),
                );
            }
        }
        rows
    }

    /// Whether anything is staged (a commit can be made).
    pub fn has_staged(&self) -> bool {
        self.files_on(DiffSide::Staged).next().is_some()
    }
}

/// The status of the repository whose root is `root`. Runs `git`: call it off the UI thread.
pub fn read_status(root: &Path) -> Result<RepoStatus, GitError> {
    let output = execute(
        root,
        &[
            "status",
            "--porcelain=v2",
            "-z",
            "--branch",
            "--untracked-files=all",
            "--ignore-submodules=dirty",
        ],
        None,
        Access::Read,
        Instant::now() + STATUS_TIMEOUT,
    )?
    .into_result()?;
    Ok(parse_status(&output))
}

/// `git status --porcelain=v2 -z --branch` output. Unknown records are skipped.
fn parse_status(output: &[u8]) -> RepoStatus {
    let mut status = RepoStatus {
        has_commits: true,
        ..RepoStatus::default()
    };
    let mut records = output
        .split(|&byte| byte == 0)
        .map(|record| String::from_utf8_lossy(record).into_owned());
    while let Some(record) = records.next() {
        if let Some(header) = record.strip_prefix("# ") {
            read_header(header, &mut status);
            continue;
        }
        let file = match record.as_bytes().first() {
            Some(b'1') => changed_entry(&record),
            // A rename or copy: the path it came from is the next record.
            Some(b'2') => changed_entry(&record).map(|file| FileStatus {
                orig_path: records.next(),
                ..file
            }),
            Some(b'u') => unmerged_entry(&record),
            Some(b'?') => record.strip_prefix("? ").map(untracked_entry),
            _ => None,
        };
        let Some(file) = file else {
            continue;
        };
        if status.files.len() == MAX_STATUS_FILES {
            status.truncated = true;
            break;
        }
        status.files.push(file);
    }
    status
}

fn read_header(header: &str, status: &mut RepoStatus) {
    if let Some(oid) = header.strip_prefix("branch.oid ") {
        status.has_commits = oid != INITIAL_OID;
    } else if let Some(head) = header.strip_prefix("branch.head ") {
        status.branch = (head != DETACHED_HEAD).then(|| head.to_string());
    }
}

/// `1 XY sub mH mI mW hH hI path`, or `2 XY sub mH mI mW hH hI Xscore path`.
fn changed_entry(record: &str) -> Option<FileStatus> {
    let fields = if record.starts_with('2') { 10 } else { 9 };
    let mut parts = record.splitn(fields, ' ');
    let xy = parts.nth(1)?.as_bytes();
    let path = parts.nth(fields - 3)?;
    Some(FileStatus {
        path: path.to_string(),
        orig_path: None,
        staged: Change::from_code(*xy.first()?),
        unstaged: Change::from_code(*xy.get(1)?),
    })
}

/// `u XY sub m1 m2 m3 mW h1 h2 h3 path`: a merge conflict.
fn unmerged_entry(record: &str) -> Option<FileStatus> {
    let path = record.splitn(11, ' ').nth(10)?;
    Some(FileStatus {
        path: path.to_string(),
        orig_path: None,
        staged: None,
        unstaged: Some(Change::Conflicted),
    })
}

fn untracked_entry(path: &str) -> FileStatus {
    FileStatus {
        path: path.to_string(),
        orig_path: None,
        staged: None,
        unstaged: Some(Change::Untracked),
    }
}

#[cfg(test)]
mod tests;
