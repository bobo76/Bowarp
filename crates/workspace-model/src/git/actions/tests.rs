use std::fs;

use super::*;
use crate::git::status::{Change, read_status};
use crate::git::tests::{TempDir, git, index_content, init_repo};

fn committed_repo() -> Option<TempDir> {
    let repo = init_repo()?;
    fs::write(repo.0.join("kept.txt"), "kept\n").unwrap();
    fs::write(repo.0.join("old.txt"), "old\n").unwrap();
    assert!(git(&repo.0, &["add", "."]));
    assert!(git(&repo.0, &["commit", "-q", "-m", "initial"]));
    Some(repo)
}

#[test]
fn lists_each_repository_once_selected_first() {
    let Some(first) = committed_repo() else {
        return;
    };
    let Some(second) = committed_repo() else {
        return;
    };
    let outside = TempDir::new();
    let nested = first.0.join("src");
    fs::create_dir_all(&nested).unwrap();

    let repos = repos_of(&[
        second.0.clone(),
        nested,
        outside.0.clone(),
        first.0.clone(),
        second.0.clone(),
    ]);

    let names: Vec<_> = repos.iter().map(|repo| repo.name.as_str()).collect();
    let expected = |dir: &TempDir| dir.0.file_name().unwrap().to_string_lossy().into_owned();
    assert_eq!(names, [expected(&second), expected(&first)]);
    assert!(repos[0].git_dir.join("HEAD").is_file());
    assert!(repos_of(std::slice::from_ref(&outside.0)).is_empty());
}

#[test]
fn a_linked_worktree_has_its_own_git_dir() {
    let Some(main) = committed_repo() else {
        return;
    };
    let linked = TempDir::new();
    let target = linked.0.join("wt");
    assert!(git(
        &main.0,
        &[
            "worktree",
            "add",
            "-q",
            "-b",
            "topic",
            target.to_str().unwrap()
        ]
    ));

    let repos = repos_of(&[target.clone(), main.0.clone()]);

    assert_eq!(repos.len(), 2);
    assert_eq!(repos[0].name, "wt");
    assert!(repos[0].git_dir.join("index").is_file());
    assert_ne!(repos[0].git_dir, repos[1].git_dir);
    assert!(repos[0].git_dir.starts_with(&repos[1].git_dir));
}

#[test]
fn stages_and_unstages_whole_files() {
    let Some(repo) = committed_repo() else {
        return;
    };
    fs::write(repo.0.join("kept.txt"), "changed\n").unwrap();
    fs::remove_file(repo.0.join("old.txt")).unwrap();
    fs::write(repo.0.join("new file.txt"), "new\n").unwrap();
    let status = read_status(&repo.0).unwrap();
    let all: Vec<&FileStatus> = status.files.iter().collect();

    stage(&repo.0, &all).unwrap();
    let status = read_status(&repo.0).unwrap();
    let staged: Vec<_> = status.files.iter().map(|file| file.staged).collect();
    assert_eq!(
        staged,
        [
            Some(Change::Modified),
            Some(Change::Added),
            Some(Change::Deleted)
        ]
    );
    assert!(status.files.iter().all(|file| file.unstaged.is_none()));

    let kept = status
        .files
        .iter()
        .find(|file| file.path == "kept.txt")
        .unwrap();
    unstage(&repo.0, &[kept], status.head).unwrap();
    assert_eq!(index_content(&repo.0, "kept.txt").unwrap(), b"kept\n");
    assert_eq!(
        fs::read_to_string(repo.0.join("kept.txt")).unwrap(),
        "changed\n"
    );

    unstage_all(&repo.0, Head::Born).unwrap();
    let status = read_status(&repo.0).unwrap();
    assert!(!status.has_staged());
    stage_all(&repo.0).unwrap();
    assert_eq!(
        read_status(&repo.0)
            .unwrap()
            .files_on(DiffSide::Unstaged)
            .count(),
        0
    );
}

#[test]
fn unstages_both_paths_of_a_rename() {
    let Some(repo) = committed_repo() else {
        return;
    };
    assert!(git(&repo.0, &["mv", "old.txt", "renamed.txt"]));
    let status = read_status(&repo.0).unwrap();
    let renamed = &status.files[0];
    assert_eq!(renamed.staged, Some(Change::Renamed));

    unstage(&repo.0, &[renamed], Head::Born).unwrap();
    assert_eq!(index_content(&repo.0, "old.txt").unwrap(), b"old\n");
    assert_eq!(index_content(&repo.0, "renamed.txt"), None);
}

#[test]
fn stages_the_edits_of_a_renamed_file() {
    let Some(repo) = committed_repo() else {
        return;
    };
    assert!(git(&repo.0, &["mv", "old.txt", "renamed.txt"]));
    fs::write(repo.0.join("renamed.txt"), "old\nedited\n").unwrap();
    let status = read_status(&repo.0).unwrap();
    let renamed = &status.files[0];
    assert_eq!(renamed.staged, Some(Change::Renamed));
    assert_eq!(renamed.unstaged, Some(Change::Modified));
    assert_eq!(renamed.orig_path.as_deref(), Some("old.txt"));

    stage(&repo.0, &[renamed]).unwrap();
    assert_eq!(
        index_content(&repo.0, "renamed.txt").unwrap(),
        b"old\nedited\n"
    );
    assert_eq!(index_content(&repo.0, "old.txt"), None);
    let status = read_status(&repo.0).unwrap();
    assert_eq!(status.files_on(DiffSide::Unstaged).count(), 0);
}

#[test]
fn unstages_before_the_first_commit_a_file_edited_since_staged() {
    let Some(repo) = init_repo() else {
        return;
    };
    fs::write(repo.0.join("a.txt"), "a\n").unwrap();
    fs::write(repo.0.join("b.txt"), "b\n").unwrap();
    stage_all(&repo.0).unwrap();
    fs::write(repo.0.join("a.txt"), "a\nedited\n").unwrap();
    fs::write(repo.0.join("b.txt"), "b\nedited\n").unwrap();
    let status = read_status(&repo.0).unwrap();
    assert_eq!(status.head, Head::Unborn);
    let a = status
        .files
        .iter()
        .find(|file| file.path == "a.txt")
        .unwrap();
    assert_eq!(a.staged, Some(Change::Added));
    assert_eq!(a.unstaged, Some(Change::Modified));

    unstage(&repo.0, &[a], status.head).unwrap();
    assert_eq!(index_content(&repo.0, "a.txt"), None);
    assert_eq!(
        fs::read_to_string(repo.0.join("a.txt")).unwrap(),
        "a\nedited\n"
    );

    unstage_all(&repo.0, status.head).unwrap();
    assert_eq!(index_content(&repo.0, "b.txt"), None);
    assert_eq!(
        fs::read_to_string(repo.0.join("b.txt")).unwrap(),
        "b\nedited\n"
    );
}

#[test]
fn unstages_before_the_first_commit() {
    let Some(repo) = init_repo() else {
        return;
    };
    fs::write(repo.0.join("a.txt"), "a\n").unwrap();
    stage_all(&repo.0).unwrap();
    let status = read_status(&repo.0).unwrap();
    assert_eq!(status.head, Head::Unborn);
    assert!(status.has_staged());

    unstage(&repo.0, &[&status.files[0]], status.head).unwrap();
    assert_eq!(index_content(&repo.0, "a.txt"), None);
    assert!(repo.0.join("a.txt").exists());
}

#[test]
fn commits_what_is_staged() {
    let Some(repo) = committed_repo() else {
        return;
    };
    fs::write(repo.0.join("kept.txt"), "changed\n").unwrap();
    fs::write(repo.0.join("other.txt"), "not staged\n").unwrap();
    stage_all(&repo.0).unwrap();
    let status = read_status(&repo.0).unwrap();
    let other = status
        .files
        .iter()
        .find(|file| file.path == "other.txt")
        .unwrap();
    unstage(&repo.0, &[other], Head::Born).unwrap();

    let summary = commit(&repo.0, "Change kept\n\nWith a body.").unwrap();
    assert!(summary.contains("Change kept"), "summary: {summary}");
    let status = read_status(&repo.0).unwrap();
    assert!(!status.has_staged());
    assert_eq!(status.files.len(), 1);
    assert_eq!(status.files[0].unstaged, Some(Change::Untracked));
}

#[test]
fn a_failed_commit_reports_why() {
    let Some(repo) = committed_repo() else {
        return;
    };
    let err = commit(&repo.0, "nothing staged").unwrap_err();
    let GitError::Failed(message) = err else {
        panic!("expected a failure, got {err:?}");
    };
    assert!(!message.is_empty());
}

#[test]
fn a_rejecting_hook_shows_its_output() {
    let Some(repo) = committed_repo() else {
        return;
    };
    let hooks = repo.0.join(".no-hooks");
    fs::create_dir_all(&hooks).unwrap();
    let hook = hooks.join("pre-commit");
    fs::write(&hook, "#!/bin/sh\necho 'lint failed: fix it' >&2\nexit 1\n").unwrap();
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(&hook, fs::Permissions::from_mode(0o755)).unwrap();
    }
    fs::write(repo.0.join("kept.txt"), "changed\n").unwrap();
    stage_all(&repo.0).unwrap();

    let err = commit(&repo.0, "blocked").unwrap_err();
    assert_eq!(err, GitError::Failed("lint failed: fix it".into()));
}

#[test]
fn a_message_is_ready_with_text_besides_whitespace() {
    assert!(commit_message_ready("Fix it"));
    assert!(commit_message_ready("  \n# a comment line is text\n"));
    assert!(!commit_message_ready(""));
    assert!(!commit_message_ready(" \n\t\r\n"));
}

/// What git 2.4x prints when `user.name` and `user.email` are unset.
const NO_IDENTITY: &str = "Author identity unknown

*** Please tell me who you are.

Run

  git config --global user.email \"you@example.com\"
  git config --global user.name \"Your Name\"

to set your account's default identity.
Omit --global to set the identity only in this repository.

fatal: unable to auto-detect email address (got 'me@host.(none)')";

#[test]
fn recognizes_a_missing_identity() {
    assert!(missing_identity(NO_IDENTITY));
    assert!(missing_identity(
        "fatal: empty ident name (for <me@example.com>) not allowed"
    ));
    assert!(!missing_identity("lint failed: fix it"));
}

#[test]
fn a_failed_commit_tells_how_to_set_a_missing_identity() {
    // Git's own message already says how: it is shown as is.
    let failure = commit_failure(&GitError::Failed(NO_IDENTITY.into()));
    assert_eq!(failure, NO_IDENTITY);

    let failure = commit_failure(&GitError::Failed(
        "fatal: empty ident name (for <me@example.com>) not allowed".into(),
    ));
    assert_eq!(
        failure,
        "fatal: empty ident name (for <me@example.com>) not allowed\n\
         Set them with `git config --global user.name \"Your Name\"` and \
         `git config --global user.email you@example.com`."
    );
    assert_eq!(
        commit_failure(&GitError::Failed("lint failed: fix it".into())),
        "lint failed: fix it"
    );
}

#[test]
fn a_long_hook_output_is_cut() {
    let output: Vec<String> = (1..=50).map(|n| format!("line {n}")).collect();
    let failure = commit_failure(&GitError::Failed(output.join("\n")));
    let lines: Vec<&str> = failure.lines().collect();
    assert_eq!(lines.len(), MAX_COMMIT_ERROR_LINES);
    assert_eq!(lines[0], "line 1");
    assert_eq!(lines[MAX_COMMIT_ERROR_LINES - 2], "line 19");
    assert_eq!(lines[MAX_COMMIT_ERROR_LINES - 1], "…");

    let short: Vec<String> = (1..=MAX_COMMIT_ERROR_LINES)
        .map(|n| format!("line {n}"))
        .collect();
    let failure = commit_failure(&GitError::Failed(short.join("\n")));
    assert_eq!(failure, short.join("\n"));
}

#[test]
fn commits_in_a_new_repository() {
    let Some(repo) = init_repo() else {
        return;
    };
    fs::write(repo.0.join("first.txt"), "first\n").unwrap();
    stage_all(&repo.0).unwrap();
    assert_eq!(read_status(&repo.0).unwrap().head, Head::Unborn);

    let summary = commit(&repo.0, "First").unwrap();
    assert!(summary.contains("First"), "summary: {summary}");
    let status = read_status(&repo.0).unwrap();
    assert_eq!(status.head, Head::Born);
    assert!(status.branch.is_some());
    assert!(status.files.is_empty());
}
