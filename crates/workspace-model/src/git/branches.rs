//! The repository's branches for the Git page's branch picker, and switching to one: a local
//! branch, or a remote-tracking one checked out as a new local branch tracking it. Switching
//! needs a clean working tree, which the page checks itself: git would carry changes that
//! don't conflict over to the other branch.

use std::path::Path;
use std::time::{Duration, Instant};

use super::runner::{Access, GitError, execute};
use super::status::RepoStatus;

/// How long reading the branches waits for `git`.
const LIST_TIMEOUT: Duration = Duration::from_secs(20);
/// A switch is never stopped: a checkout killed halfway leaves a half-switched working tree.
/// This only keeps the deadline arithmetic finite.
const SWITCH_DEADLINE: Duration = Duration::from_secs(7 * 24 * 60 * 60);
/// At most this many lines of git's message are shown (a post-checkout hook may print a lot).
const MAX_MESSAGE_LINES: usize = 20;
/// The prefix of a local branch's full reference.
const LOCAL_PREFIX: &str = "refs/heads/";
/// The prefix of a remote-tracking branch's full reference.
const REMOTE_PREFIX: &str = "refs/remotes/";
/// What `git for-each-ref` prints per branch: its reference, upstream, `*` when checked
/// out here, the worktree it is checked out in, and the target of a symbolic reference.
const REF_FORMAT: &str =
    "--format=%(refname)%00%(upstream)%00%(HEAD)%00%(worktreepath)%00%(symref)";

/// A local branch.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct LocalBranch {
    pub name: String,
    /// The full reference of the branch it tracks (`refs/remotes/origin/main`), if any.
    pub upstream: Option<String>,
    /// Checked out in this working tree.
    pub current: bool,
    /// Checked out in another worktree: git refuses to switch to it here.
    pub elsewhere: bool,
}

/// A remote-tracking branch no local branch tracks.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RemoteBranch {
    /// As shown: `origin/feature`.
    pub name: String,
    /// Its full reference: `refs/remotes/origin/feature`.
    pub reference: String,
    /// The branch on the remote: `feature`, the name of the local branch made from it.
    pub branch: String,
}

/// The branches the picker lists: local ones, then remote-tracking ones (without the
/// remotes' `HEAD` and the branches a local branch already tracks), each by name.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Branches {
    pub local: Vec<LocalBranch>,
    pub remote: Vec<RemoteBranch>,
}

/// A branch picked to switch to.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum BranchChoice {
    Local(String),
    Remote(RemoteBranch),
}

impl BranchChoice {
    /// The branch checked out after the switch: a remote branch's local name.
    pub fn local_name(&self) -> &str {
        match self {
            Self::Local(name) => name,
            Self::Remote(remote) => &remote.branch,
        }
    }
}

/// What switching to a picked branch does.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum SwitchPlan {
    /// It is the branch checked out: nothing to do.
    Nothing,
    /// Run `git` with these arguments.
    Run(Vec<String>),
    /// Nothing runs; why, for the user.
    Refuse(String),
}

/// The branches of the repository at `root`. Runs `git`: call it off the UI thread.
pub fn read_branches(root: &Path) -> Result<Branches, GitError> {
    let remotes = read_lines(root, &["remote"])?;
    let refs = read_lines(
        root,
        &["for-each-ref", REF_FORMAT, "refs/heads", "refs/remotes"],
    )?;
    let remotes: Vec<&str> = remotes.lines().collect();
    Ok(parse_branches(&refs, &remotes))
}

fn read_lines(root: &Path, args: &[&str]) -> Result<String, GitError> {
    let deadline = Instant::now() + LIST_TIMEOUT;
    let output = execute(root, args, None, Access::Read, deadline)?.into_result()?;
    Ok(String::from_utf8_lossy(&output).into_owned())
}

/// `git for-each-ref` output in [`REF_FORMAT`], `remotes` being the remotes' names.
fn parse_branches(output: &str, remotes: &[&str]) -> Branches {
    let mut branches = Branches::default();
    let mut remote_refs = Vec::new();
    for line in output.lines() {
        let fields: Vec<&str> = line.split('\0').collect();
        let [reference, upstream, head, worktree, symref] = fields[..] else {
            continue;
        };
        if let Some(name) = reference.strip_prefix(LOCAL_PREFIX) {
            branches
                .local
                .push(local_branch(name, upstream, head, worktree));
        } else if symref.is_empty() && !reference.ends_with("/HEAD") {
            remote_refs.extend(remote_branch(reference, remotes));
        }
    }
    let tracked = |remote: &RemoteBranch| {
        branches
            .local
            .iter()
            .any(|local| local.upstream.as_deref() == Some(remote.reference.as_str()))
    };
    remote_refs.retain(|remote| !tracked(remote));
    branches.remote = remote_refs;
    branches
}

fn local_branch(name: &str, upstream: &str, head: &str, worktree: &str) -> LocalBranch {
    let current = head == "*";
    LocalBranch {
        name: name.to_string(),
        upstream: (!upstream.is_empty()).then(|| upstream.to_string()),
        current,
        elsewhere: !current && !worktree.is_empty(),
    }
}

/// A remote-tracking reference split into its remote (the longest of `remotes` it starts
/// with, a remote's name may hold `/`) and branch; a remote since removed splits at its first
/// `/`.
fn remote_branch(reference: &str, remotes: &[&str]) -> Option<RemoteBranch> {
    let name = reference.strip_prefix(REMOTE_PREFIX)?;
    let remote = remotes
        .iter()
        .filter(|remote| name.starts_with(&format!("{remote}/")))
        .max_by_key(|remote| remote.len())
        .copied()
        .or_else(|| name.split_once('/').map(|(remote, _)| remote))?;
    let branch = name
        .get(remote.len() + 1..)
        .filter(|rest| !rest.is_empty())?;
    Some(RemoteBranch {
        name: name.to_string(),
        reference: reference.to_string(),
        branch: branch.to_string(),
    })
}

/// What switching to `choice` runs: `git switch` to a local branch; for a remote branch, a
/// new local branch of its name tracking it (never git's guess by name, ambiguous with several
/// remotes). A local branch of that name already there is switched to when it tracks this
/// remote branch, else nothing runs: switching to it would show other commits.
pub fn switch_plan(branches: &Branches, choice: &BranchChoice) -> SwitchPlan {
    let local = |name: &str| branches.local.iter().find(|local| local.name == name);
    match choice {
        BranchChoice::Local(name) if local(name).is_some_and(|local| local.current) => {
            SwitchPlan::Nothing
        }
        BranchChoice::Local(name) => SwitchPlan::Run(switch_args(&["--", name])),
        BranchChoice::Remote(remote) => match local(&remote.branch) {
            None => SwitchPlan::Run(switch_args(&[
                "--create",
                &remote.branch,
                "--track",
                &remote.reference,
            ])),
            Some(local) if local.upstream.as_deref() == Some(remote.reference.as_str()) => {
                switch_plan(branches, &BranchChoice::Local(local.name.clone()))
            }
            Some(local) => SwitchPlan::Refuse(format!(
                "A local branch \"{}\" already exists and doesn't track {}: switch to it under \
                 Local, or rename it to check out {} here.",
                local.name, remote.name, remote.name
            )),
        },
    }
}

fn switch_args(rest: &[&str]) -> Vec<String> {
    ["switch", "--no-guess"]
        .iter()
        .chain(rest)
        .map(|arg| arg.to_string())
        .collect()
}

/// Why the working tree in `status` can't switch branches: any staged, unstaged, untracked
/// or conflicted file (ignored ones aren't listed). `None` when it is clean.
pub fn switch_blocker(status: &RepoStatus) -> Option<String> {
    let count = status.files.len();
    let files = match (count, status.truncated) {
        (0, _) => return None,
        (1, _) => "1 changed file".to_string(),
        (_, false) => format!("{count} changed files"),
        (_, true) => format!("more than {count} changed files"),
    };
    Some(format!(
        "Commit or discard your {files} first: switching branches needs a clean working tree."
    ))
}

/// Run a [`SwitchPlan::Run`]'s `args` in `root`, the repository's hooks included, never
/// stopped. Returns what git printed ([`switch_message`]).
pub fn switch_branch(root: &Path, args: &[String]) -> Result<String, GitError> {
    let args: Vec<&str> = args.iter().map(String::as_str).collect();
    let deadline = Instant::now() + SWITCH_DEADLINE;
    let output = execute(root, &args, None, Access::Write, deadline)?;
    if !output.status.success() {
        let message = switch_message(&output.stderr);
        let message = match message.is_empty() {
            true => switch_message(&output.stdout),
            false => message,
        };
        return Err(GitError::Failed(message));
    }
    // "Switched to branch 'x'" comes on stderr, then how it compares with its upstream.
    let mut printed = output.stderr;
    printed.extend_from_slice(&output.stdout);
    Ok(switch_message(&printed))
}

/// What git printed for a switch, as the page shows it: without its `hint:` lines (a warning
/// such as "you are leaving 2 commits behind" stays), cut at [`MAX_MESSAGE_LINES`].
fn switch_message(printed: &[u8]) -> String {
    let text = String::from_utf8_lossy(printed);
    let lines: Vec<&str> = text
        .lines()
        .map(str::trim_end)
        .filter(|line| !line.is_empty() && !line.starts_with("hint:"))
        .collect();
    if lines.len() <= MAX_MESSAGE_LINES {
        return lines.join("\n");
    }
    let mut kept = lines[..MAX_MESSAGE_LINES - 1].join("\n");
    kept.push_str("\n…");
    kept
}

#[cfg(test)]
mod tests;
