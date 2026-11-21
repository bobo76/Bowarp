//! The repositories the tabs are in, and what the Git page changes in one: staging and
//! unstaging (whole files, or a patch of some lines), committing, and discarding a file's
//! unstaged changes, the only action here that touches the working tree.

use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use super::runner::{Access, GitError, execute};
use super::status::{Change, DiffSide, FileStatus, Head};

/// Commands that write (staging, discarding, committing) are never stopped: git killed
/// halfway leaves `.git/index.lock` behind, and every later git command fails until it is
/// removed by hand. A slow hook or LFS filter is waited for. This only keeps the deadline
/// arithmetic finite.
const WRITE_DEADLINE: Duration = Duration::from_secs(7 * 24 * 60 * 60);
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

/// The repository to show from those `found`: the `selected` tab's, else the one `shown`
/// when still listed, else the first. Pure: the caller finds `selected` with [`locate`].
pub fn choose_repo(
    found: &[RepoEntry],
    selected: Option<RepoEntry>,
    shown: Option<RepoEntry>,
) -> Option<RepoEntry> {
    let listed = |repo: &RepoEntry| found.iter().any(|entry| entry.root == repo.root);
    selected
        .filter(listed)
        .or_else(|| shown.filter(listed))
        .or_else(|| found.first().cloned())
}

/// The repository whose work tree contains `dir`. Runs `git`: call it off the UI thread.
pub fn locate(dir: &Path) -> Option<RepoEntry> {
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
    // `lines` drops the line ends (`\r\n` too); a folder name may end in spaces.
    let mut lines = output.lines().filter(|line| !line.is_empty());
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
    run_write(root, &args, None)
}

/// Unstage `files` whole (both paths of a rename), keeping their working-tree changes. With an
/// unborn `head` (a new repository) they are taken out of the index.
pub fn unstage(root: &Path, files: &[&FileStatus], head: Head) -> Result<(), GitError> {
    let paths = paths_of(files);
    let mut args = vec!["--literal-pathspecs"];
    args.extend(unstage_command(head));
    args.push("--");
    args.extend(paths.iter().map(String::as_str));
    run_write(root, &args, None)
}

/// Stage every change, untracked files included.
pub fn stage_all(root: &Path) -> Result<(), GitError> {
    run_write(root, &["add", "--all", "--", ":/"], None)
}

/// Unstage everything, keeping the working tree as it is.
pub fn unstage_all(root: &Path, head: Head) -> Result<(), GitError> {
    let mut args = unstage_command(head).to_vec();
    args.extend(["--", ":/"]);
    run_write(root, &args, None)
}

/// Apply a [`super::partial_patch`] of the diff on `side` to the index: staging its lines
/// (Unstaged) or unstaging them (Staged).
pub fn apply_patch(root: &Path, patch: Vec<u8>, side: DiffSide) -> Result<(), GitError> {
    let mut args = vec!["apply", "--cached", "--recount", "--whitespace=nowarn"];
    if side == DiffSide::Staged {
        args.push("--reverse");
    }
    args.push("-");
    run_write(root, &args, Some(patch))
}

/// Throw away the unstaged changes of `files`: a tracked file goes back to its staged content
/// (what is staged is kept), an untracked one is deleted. Merge conflicts are left alone
/// ([`FileStatus::discardable`]).
pub fn discard(root: &Path, files: &[&FileStatus]) -> Result<(), GitError> {
    let (untracked, tracked): (Vec<&FileStatus>, Vec<&FileStatus>) = files
        .iter()
        .copied()
        .filter(|file| file.discardable())
        .partition(|file| file.unstaged == Some(Change::Untracked));
    if !tracked.is_empty() {
        let mut args = vec!["--literal-pathspecs", "restore", "--worktree", "--"];
        args.extend(tracked.iter().map(|file| file.path.as_str()));
        run_write(root, &args, None)?;
    }
    if !untracked.is_empty() {
        let mut args = vec!["--literal-pathspecs", "clean", "-f", "-q", "--"];
        args.extend(untracked.iter().map(|file| file.path.as_str()));
        run_write(root, &args, None)?;
    }
    Ok(())
}

/// Throw away every unstaged change: tracked files go back to their staged content, and the
/// untracked ones among `files` (those the user was told about) are deleted; an untracked
/// file that appeared since is kept, as are ignored ones. Merge conflicts are left alone.
/// `files` paths are relative to the work tree's top.
pub fn discard_all(root: &Path, files: &[&FileStatus]) -> Result<(), GitError> {
    let restore = ["restore", "--worktree", "--ignore-unmerged", "--", ":/"];
    run_write(root, &restore, None)?;
    let untracked: Vec<String> = files
        .iter()
        .filter(|file| file.discardable() && file.unstaged == Some(Change::Untracked))
        .map(|file| format!(":(top,literal){}", file.path))
        .collect();
    for chunk in arg_chunks(&untracked, MAX_PATHS_LEN) {
        let mut args = vec!["clean", "-f", "-q", "--"];
        args.extend(chunk.iter().map(String::as_str));
        run_write(root, &args, None)?;
    }
    Ok(())
}

/// How many bytes of paths one git command line takes: Windows refuses a command line over
/// 32K UTF-16 units, and a path's UTF-8 length is never less than its UTF-16 one.
const MAX_PATHS_LEN: usize = 16 * 1024;

/// `args` split into runs whose lengths add up to at most `max_len` (a longer single one gets
/// a run of its own).
fn arg_chunks(args: &[String], max_len: usize) -> Vec<&[String]> {
    let mut chunks = Vec::new();
    let (mut start, mut len) = (0, 0);
    for (index, arg) in args.iter().enumerate() {
        let arg_len = arg.len() + 1;
        if index > start && len + arg_len > max_len {
            chunks.push(&args[start..index]);
            (start, len) = (index, 0);
        }
        len += arg_len;
    }
    if start < args.len() {
        chunks.push(&args[start..]);
    }
    chunks
}

/// What the confirmation before discarding `files` says, naming what is lost.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DiscardPrompt {
    pub title: String,
    pub description: String,
    /// The confirming button's label.
    pub action: &'static str,
}

/// The confirmation before discarding the [`FileStatus::discardable`] ones of `files`; `None`
/// when there is none.
pub fn discard_prompt(files: &[&FileStatus]) -> Option<DiscardPrompt> {
    let files: Vec<&FileStatus> = files
        .iter()
        .copied()
        .filter(|file| file.discardable())
        .collect();
    let untracked = files
        .iter()
        .filter(|file| file.unstaged == Some(Change::Untracked))
        .count();
    let tracked = files.len() - untracked;
    let prompt = match (files.as_slice(), tracked) {
        ([], _) => return None,
        ([file], 0) => DiscardPrompt {
            title: format!("Delete \"{}\"?", file.path),
            description: format!("It isn't tracked by git: the file is deleted. {UNDONE}"),
            action: "Delete",
        },
        ([file], _) => DiscardPrompt {
            title: format!("Discard the changes in \"{}\"?", file.path),
            description: format!("Its unstaged changes are lost; what is staged is kept. {UNDONE}"),
            action: "Discard",
        },
        _ => {
            let mut lost = Vec::new();
            if tracked > 0 {
                lost.push(format!(
                    "the unstaged changes in {} are lost",
                    files_count(tracked)
                ));
            }
            if untracked > 0 {
                lost.push(format!(
                    "{} not tracked by git {} deleted",
                    files_count(untracked),
                    if untracked == 1 { "is" } else { "are" }
                ));
            }
            let mut description = lost.join(", and ");
            description[..1].make_ascii_uppercase();
            DiscardPrompt {
                title: format!("Discard all changes in {}?", files_count(files.len())),
                description: format!("{description}; what is staged is kept. {UNDONE}"),
                action: "Discard All",
            }
        }
    };
    Some(prompt)
}

/// Ends every discard confirmation.
const UNDONE: &str = "This can't be undone.";

/// "1 file", "3 files".
fn files_count(count: usize) -> String {
    if count == 1 {
        "1 file".to_string()
    } else {
        format!("{count} files")
    }
}

/// Commit what is staged with `message`, running the repository's hooks. Returns git's
/// summary (`[main 1a2b3c4] message`).
pub fn commit(root: &Path, message: &str) -> Result<String, GitError> {
    let output = execute(
        root,
        &["commit", "--file=-"],
        Some(message.as_bytes().to_vec()),
        Access::Write,
        Instant::now() + WRITE_DEADLINE,
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
fn unstage_command(head: Head) -> &'static [&'static str] {
    match head {
        Head::Born => &["restore", "--staged"],
        Head::Unborn => &["rm", "--cached", "-r", "-q", "-f"],
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

fn run_write(root: &Path, args: &[&str], input: Option<Vec<u8>>) -> Result<(), GitError> {
    execute(
        root,
        args,
        input,
        Access::Write,
        Instant::now() + WRITE_DEADLINE,
    )?
    .into_result()
    .map(drop)
}

#[cfg(test)]
mod tests;
