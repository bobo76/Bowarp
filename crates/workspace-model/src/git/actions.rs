//! The repositories the tabs are in, and what the Git page changes in one: staging and
//! unstaging (whole files, or a patch of some lines) and committing. Nothing here touches the
//! working tree.

use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use super::runner::{Access, GitError, execute};
use super::status::{DiffSide, FileStatus};

/// How long staging and unstaging wait for `git`.
const STAGE_TIMEOUT: Duration = Duration::from_secs(30);
/// How long a commit waits for `git`, its hooks included (a pre-commit hook may run tests).
const COMMIT_TIMEOUT: Duration = Duration::from_secs(300);
/// How long finding a folder's repository waits for `git rev-parse`.
const ROOT_TIMEOUT: Duration = Duration::from_secs(5);
/// At most this many lines of a failed commit's message are shown (a hook may print a lot).
const MAX_COMMIT_ERROR_LINES: usize = 20;
/// What git prints when it doesn't know who commits (`user.name` / `user.email` unset).
const MISSING_IDENTITY: [&str; 4] = [
    "Please tell me who you are",
    "Author identity unknown",
    "unable to auto-detect email address",
    "empty ident name",
];
/// Added to a missing-identity failure whose message doesn't already say how to fix it.
const IDENTITY_HINT: &str = "Set them with `git config --global user.name \"Your Name\"` and \
                             `git config --global user.email you@example.com`.";

/// A repository one or more tabs are in.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RepoEntry {
    /// The work tree's top folder, as git prints it.
    pub root: PathBuf,
    /// Its git directory (`.git`, or a linked worktree's own folder under the main one),
    /// where `index` and `HEAD` change.
    pub git_dir: PathBuf,
    /// The top folder's name.
    pub name: String,
}

/// The repositories `folders` are in, each once, in the order of the first folder in it (so
/// the selected tab's folder, given first, gives the first entry). Folders outside a work tree
/// are left out. Runs `git`: call it off the UI thread.
pub fn repos_of(folders: &[PathBuf]) -> Vec<RepoEntry> {
    let mut repos: Vec<RepoEntry> = Vec::new();
    for (index, folder) in folders.iter().enumerate() {
        // Tabs often share a folder; a folder inside a known repository may still be in a
        // nested one (a submodule), so only exact repeats are skipped.
        if folders[..index].contains(folder) {
            continue;
        }
        let Some(repo) = locate(folder) else {
            continue;
        };
        if !repos.iter().any(|known| known.root == repo.root) {
            repos.push(repo);
        }
    }
    repos
}

/// The repository whose work tree contains `dir`.
fn locate(dir: &Path) -> Option<RepoEntry> {
    let output = execute(
        dir,
        &["rev-parse", "--show-toplevel", "--absolute-git-dir"],
        None,
        Access::Read,
        Instant::now() + ROOT_TIMEOUT,
    )
    .ok()?
    .into_result()
    .ok()?;
    let output = String::from_utf8(output).ok()?;
    let mut lines = output
        .lines()
        .map(str::trim_end)
        .filter(|line| !line.is_empty());
    let root = PathBuf::from(lines.next()?);
    let git_dir = PathBuf::from(lines.next()?);
    let name = root.file_name().map_or_else(
        || root.display().to_string(),
        |name| name.to_string_lossy().into_owned(),
    );
    Some(RepoEntry {
        root,
        git_dir,
        name,
    })
}

/// Stage `files` whole, deletions included. The old path of a rename is already staged (it is
/// gone from the index): naming it would fail as a pathspec matching nothing.
pub fn stage(root: &Path, files: &[&FileStatus]) -> Result<(), GitError> {
    let mut args = vec!["--literal-pathspecs", "add", "--all", "--"];
    args.extend(files.iter().map(|file| file.path.as_str()));
    run_write(root, &args, None, STAGE_TIMEOUT)
}

/// Unstage `files` whole (both paths of a rename), keeping their working-tree changes.
/// `has_commits`: HEAD has a commit (in a new repository files are taken out of the index).
pub fn unstage(root: &Path, files: &[&FileStatus], has_commits: bool) -> Result<(), GitError> {
    let paths = paths_of(files);
    let mut args = vec!["--literal-pathspecs"];
    args.extend(unstage_command(has_commits));
    args.push("--");
    args.extend(paths.iter().map(String::as_str));
    run_write(root, &args, None, STAGE_TIMEOUT)
}

/// Stage every change, untracked files included.
pub fn stage_all(root: &Path) -> Result<(), GitError> {
    run_write(root, &["add", "--all", "--", ":/"], None, STAGE_TIMEOUT)
}

/// Unstage everything, keeping the working tree as it is.
pub fn unstage_all(root: &Path, has_commits: bool) -> Result<(), GitError> {
    let mut args = unstage_command(has_commits).to_vec();
    args.extend(["--", ":/"]);
    run_write(root, &args, None, STAGE_TIMEOUT)
}

/// Apply a [`super::partial_patch`] of the diff on `side` to the index: staging its lines
/// (Unstaged) or unstaging them (Staged).
pub fn apply_patch(root: &Path, patch: Vec<u8>, side: DiffSide) -> Result<(), GitError> {
    let mut args = vec!["apply", "--cached", "--recount", "--whitespace=nowarn"];
    if side == DiffSide::Staged {
        args.push("--reverse");
    }
    args.push("-");
    run_write(root, &args, Some(patch), STAGE_TIMEOUT)
}

/// Commit what is staged with `message`, running the repository's hooks. Returns git's
/// summary (`[main 1a2b3c4] message`).
pub fn commit(root: &Path, message: &str) -> Result<String, GitError> {
    let output = execute(
        root,
        &["commit", "--file=-"],
        Some(message.as_bytes().to_vec()),
        Access::Write,
        Instant::now() + COMMIT_TIMEOUT,
    )?
    .into_result()?;
    let output = String::from_utf8_lossy(&output);
    Ok(output.lines().next().unwrap_or_default().to_string())
}

/// Whether `message` can be committed: it has text besides whitespace. Git cleans it up the
/// way `--cleanup=whitespace` does (`#` lines are kept, as VS Code keeps them).
pub fn commit_message_ready(message: &str) -> bool {
    !message.trim().is_empty()
}

/// Whether a failed commit's `message` says git doesn't know the author's name or email.
pub fn missing_identity(message: &str) -> bool {
    MISSING_IDENTITY.iter().any(|text| message.contains(text))
}

/// What to tell the user of a failed commit: git's message, cut at
/// [`MAX_COMMIT_ERROR_LINES`] lines (a hook may print a lot), with how to set a missing
/// identity when git's message doesn't say it.
pub fn commit_failure(error: &GitError) -> String {
    let message = error.to_string();
    let lines: Vec<&str> = message.lines().collect();
    let mut text = if lines.len() > MAX_COMMIT_ERROR_LINES {
        let mut kept = lines[..MAX_COMMIT_ERROR_LINES - 1].join("\n");
        kept.push_str("\n…");
        kept
    } else {
        message.clone()
    };
    if missing_identity(&message) && !message.contains("git config") {
        text.push('\n');
        text.push_str(IDENTITY_HINT);
    }
    text
}

/// `git restore --staged`, or `git rm --cached` before the first commit (there is no HEAD to
/// restore from). `-f`: without it git refuses a file changed since it was staged; with
/// `--cached` it only lets the staged content go, the working tree is never touched.
fn unstage_command(has_commits: bool) -> &'static [&'static str] {
    if has_commits {
        &["restore", "--staged"]
    } else {
        &["rm", "--cached", "-r", "-q", "-f"]
    }
}

/// The paths of `files`, the old path of a rename included.
fn paths_of(files: &[&FileStatus]) -> Vec<String> {
    files
        .iter()
        .flat_map(|file| std::iter::once(&file.path).chain(&file.orig_path))
        .cloned()
        .collect()
}

fn run_write(
    root: &Path,
    args: &[&str],
    input: Option<Vec<u8>>,
    timeout: Duration,
) -> Result<(), GitError> {
    execute(root, args, input, Access::Write, Instant::now() + timeout)?
        .into_result()
        .map(drop)
}

#[cfg(test)]
mod tests;
