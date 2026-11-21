use std::fs;
use std::path::PathBuf;

use super::*;
use crate::git::status::read_status;
use crate::git::tests::{TempDir, git, init_repo};

fn line(fields: [&str; 5]) -> String {
    fields.join("\0")
}

fn local(name: &str, upstream: Option<&str>, current: bool) -> LocalBranch {
    LocalBranch {
        name: name.to_string(),
        upstream: upstream.map(str::to_string),
        current,
        elsewhere: false,
    }
}

fn remote(remote: &str, branch: &str) -> RemoteBranch {
    RemoteBranch {
        name: format!("{remote}/{branch}"),
        reference: format!("refs/remotes/{remote}/{branch}"),
        branch: branch.to_string(),
    }
}

#[test]
fn lists_local_branches_then_remote_ones_no_local_branch_tracks() {
    let output = [
        line([
            "refs/heads/main",
            "refs/remotes/origin/main",
            "*",
            "/repo",
            "",
        ]),
        line(["refs/heads/topic", "", " ", "/elsewhere", ""]),
        line(["refs/heads/wip", "", " ", "", ""]),
        line([
            "refs/remotes/origin/HEAD",
            "",
            " ",
            "",
            "refs/remotes/origin/main",
        ]),
        line(["refs/remotes/origin/main", "", " ", "", ""]),
        line(["refs/remotes/origin/feature/x", "", " ", "", ""]),
        line(["refs/remotes/my/fork/fix", "", " ", "", ""]),
    ]
    .join("\n");

    let branches = parse_branches(&output, &["origin", "my/fork"]);

    let topic = LocalBranch {
        elsewhere: true,
        ..local("topic", None, false)
    };
    assert_eq!(
        branches.local,
        [
            local("main", Some("refs/remotes/origin/main"), true),
            topic,
            local("wip", None, false)
        ]
    );
    assert_eq!(
        branches.remote,
        [remote("origin", "feature/x"), remote("my/fork", "fix")]
    );
}

#[test]
fn a_removed_remotes_branch_splits_at_its_first_slash() {
    let output = line(["refs/remotes/gone/topic/a", "", " ", "", ""]);
    assert_eq!(
        parse_branches(&output, &[]).remote,
        [remote("gone", "topic/a")]
    );
}

#[test]
fn switching_to_the_current_branch_does_nothing() {
    let branches = Branches {
        local: vec![local("main", None, true), local("topic", None, false)],
        remote: Vec::new(),
    };
    let plan = |name: &str| switch_plan(&branches, &BranchChoice::Local(name.to_string()));
    assert_eq!(plan("main"), SwitchPlan::Nothing);
    assert_eq!(
        plan("topic"),
        SwitchPlan::Run(vec![
            "switch".into(),
            "--no-guess".into(),
            "--".into(),
            "topic".into()
        ])
    );
}

#[test]
fn a_remote_branch_becomes_a_local_branch_tracking_it() {
    let feature = remote("origin", "feature");
    let mut branches = Branches::default();
    let plan = |branches: &Branches| switch_plan(branches, &BranchChoice::Remote(feature.clone()));

    let create = ["switch", "--no-guess", "--create", "feature", "--track"];
    let mut expected: Vec<String> = create.iter().map(|arg| arg.to_string()).collect();
    expected.push("refs/remotes/origin/feature".into());
    assert_eq!(plan(&branches), SwitchPlan::Run(expected));

    branches.local = vec![local("feature", Some("refs/remotes/origin/feature"), false)];
    assert_eq!(
        plan(&branches),
        SwitchPlan::Run(vec![
            "switch".into(),
            "--no-guess".into(),
            "--".into(),
            "feature".into()
        ])
    );

    branches.local = vec![local("feature", Some("refs/remotes/fork/feature"), false)];
    let SwitchPlan::Refuse(message) = plan(&branches) else {
        panic!("a local branch tracking something else is not switched to");
    };
    assert!(message.contains("\"feature\""), "{message}");
    assert!(message.contains("origin/feature"), "{message}");
}

#[test]
fn any_listed_file_blocks_a_switch() {
    let mut status = RepoStatus::default();
    assert_eq!(switch_blocker(&status), None);
    status.files = read_files(1);
    assert_eq!(
        switch_blocker(&status).unwrap(),
        "Commit or discard your 1 changed file first: switching branches needs a clean \
         working tree."
    );
    status.files = read_files(3);
    assert!(
        switch_blocker(&status)
            .unwrap()
            .contains("your 3 changed files")
    );
    status.truncated = true;
    assert!(
        switch_blocker(&status)
            .unwrap()
            .contains("more than 3 changed files")
    );
}

fn read_files(count: usize) -> Vec<crate::git::FileStatus> {
    (0..count)
        .map(|index| crate::git::FileStatus {
            path: format!("f{index}"),
            orig_path: None,
            staged: None,
            unstaged: Some(crate::git::Change::Untracked),
        })
        .collect()
}

#[test]
fn the_message_drops_hints_and_is_cut() {
    let printed = b"Warning: you are leaving 1 commit behind:\n\nhint: If you want to keep it\n\
                    hint: git branch <name>\nSwitched to branch 'main'\n";
    assert_eq!(
        switch_message(printed),
        "Warning: you are leaving 1 commit behind:\nSwitched to branch 'main'"
    );
    let long: String = (0..30).map(|index| format!("line {index}\n")).collect();
    let cut = switch_message(long.as_bytes());
    assert_eq!(cut.lines().count(), MAX_MESSAGE_LINES);
    assert!(cut.ends_with('…'));
}

/// A repository with a commit on `main`, and `feature` one commit ahead.
fn upstream_repo() -> Option<TempDir> {
    let repo = init_repo()?;
    fs::write(repo.0.join("a.txt"), "a\n").unwrap();
    assert!(git(&repo.0, &["add", "."]));
    assert!(git(&repo.0, &["commit", "-q", "-m", "first"]));
    assert!(git(&repo.0, &["switch", "-q", "-c", "feature"]));
    fs::write(repo.0.join("b.txt"), "b\n").unwrap();
    assert!(git(&repo.0, &["add", "."]));
    assert!(git(&repo.0, &["commit", "-q", "-m", "second"]));
    assert!(git(&repo.0, &["switch", "-q", "main"]));
    Some(repo)
}

/// A clone of `upstream` (its `main` checked out, tracking `origin/main`).
fn clone_of(upstream: &TempDir) -> TempDir {
    let parent = TempDir::new();
    let target = parent.0.join("clone");
    assert!(git(
        &parent.0,
        &["clone", "-q", upstream.0.to_str().unwrap(), "clone"]
    ));
    let clone = TempDir::new();
    fs::remove_dir_all(&clone.0).unwrap();
    fs::rename(&target, &clone.0).unwrap();
    assert!(git(&clone.0, &["config", "core.hooksPath", ".no-hooks"]));
    clone
}

fn current_branch(root: &Path) -> String {
    read_status(root).unwrap().branch.unwrap_or_default()
}

#[test]
fn a_local_branch_named_like_an_option_is_switched_to() {
    let Some(repo) = upstream_repo() else {
        return;
    };
    assert!(git(&repo.0, &["update-ref", "refs/heads/-x", "HEAD"]));
    let branches = read_branches(&repo.0).unwrap();
    let SwitchPlan::Run(args) = switch_plan(&branches, &BranchChoice::Local("-x".into())) else {
        panic!("a listed branch is switched to");
    };
    switch_branch(&repo.0, &args).unwrap();
    assert_eq!(current_branch(&repo.0), "-x");
}

#[test]
fn reads_and_switches_the_branches_of_a_clone() {
    let Some(upstream) = upstream_repo() else {
        return;
    };
    let clone = clone_of(&upstream);

    let branches = read_branches(&clone.0).unwrap();
    assert_eq!(
        branches.local,
        [local("main", Some("refs/remotes/origin/main"), true)]
    );
    assert_eq!(branches.remote, [remote("origin", "feature")]);

    let choice = BranchChoice::Remote(branches.remote[0].clone());
    let SwitchPlan::Run(args) = switch_plan(&branches, &choice) else {
        panic!("a remote branch is checked out");
    };
    let summary = switch_branch(&clone.0, &args).unwrap();
    assert!(summary.contains("feature"), "{summary}");
    assert!(!summary.contains("hint:"), "{summary}");
    assert_eq!(current_branch(&clone.0), "feature");
    assert!(clone.0.join("b.txt").is_file());

    let after = read_branches(&clone.0).unwrap();
    assert!(
        after.remote.is_empty(),
        "the new branch tracks origin/feature"
    );
    let feature = after.local.iter().find(|branch| branch.name == "feature");
    assert_eq!(
        feature.and_then(|branch| branch.upstream.as_deref()),
        Some("refs/remotes/origin/feature")
    );

    let SwitchPlan::Run(args) = switch_plan(&after, &BranchChoice::Local("main".into())) else {
        panic!("main is switched to");
    };
    switch_branch(&clone.0, &args).unwrap();
    assert_eq!(current_branch(&clone.0), "main");
}

#[test]
fn a_branch_checked_out_in_another_worktree_is_marked_and_refused_by_git() {
    let Some(repo) = upstream_repo() else {
        return;
    };
    let linked = TempDir::new();
    let target: PathBuf = linked.0.join("wt");
    assert!(git(
        &repo.0,
        &["worktree", "add", "-q", target.to_str().unwrap(), "feature"]
    ));

    let branches = read_branches(&repo.0).unwrap();
    let feature = branches
        .local
        .iter()
        .find(|branch| branch.name == "feature");
    assert!(feature.is_some_and(|branch| branch.elsewhere && !branch.current));
    let in_linked = read_branches(&target).unwrap();
    let feature = in_linked
        .local
        .iter()
        .find(|branch| branch.name == "feature");
    assert!(feature.is_some_and(|branch| branch.current && !branch.elsewhere));

    let SwitchPlan::Run(args) = switch_plan(&branches, &BranchChoice::Local("feature".into()))
    else {
        panic!("git decides");
    };
    let Err(GitError::Failed(message)) = switch_branch(&repo.0, &args) else {
        panic!("git refuses a branch checked out elsewhere");
    };
    assert!(message.contains("feature"), "{message}");
    assert_eq!(current_branch(&repo.0), "main");
}

#[test]
fn a_detached_head_has_no_current_branch() {
    let Some(repo) = upstream_repo() else {
        return;
    };
    assert!(git(&repo.0, &["switch", "-q", "--detach", "feature"]));
    let branches = read_branches(&repo.0).unwrap();
    assert!(branches.local.iter().all(|branch| !branch.current));
}
