use super::*;
use crate::git::tests::{TempDir, git, write};

const HASH: &str = "0123456789012345678901234567890123456789";

fn ordinary(xy: &str, path: &str) -> String {
    format!("1 {xy} N... 100644 100644 100644 {HASH} {HASH} {path}")
}

#[test]
fn parses_branch_files_and_both_sides() {
    let output = [
        "# branch.oid ".to_string() + HASH,
        "# branch.head feature/git page".to_string(),
        ordinary("M.", "src/staged.rs"),
        ordinary(".M", "src/with space.rs"),
        ordinary("MM", "both.txt"),
        ordinary(".D", "gone.txt"),
        format!("2 R. N... 100644 100644 100644 {HASH} {HASH} R100 new name.txt"),
        "old name.txt".to_string(),
        format!("u UU N... 100644 100644 100644 100644 {HASH} {HASH} {HASH} merge.txt"),
        "? notes/todo.md".to_string(),
        String::new(),
    ]
    .join("\0");

    let status = parse_status(output.as_bytes());

    assert_eq!(status.branch.as_deref(), Some("feature/git page"));
    assert_eq!(status.head, Head::Born);
    assert!(!status.truncated);
    let file = |path: &str, staged, unstaged| FileStatus {
        path: path.to_string(),
        orig_path: None,
        staged,
        unstaged,
    };
    assert_eq!(
        status.files,
        [
            file("src/staged.rs", Some(Change::Modified), None),
            file("src/with space.rs", None, Some(Change::Modified)),
            file("both.txt", Some(Change::Modified), Some(Change::Modified)),
            file("gone.txt", None, Some(Change::Deleted)),
            FileStatus {
                orig_path: Some("old name.txt".to_string()),
                ..file("new name.txt", Some(Change::Renamed), None)
            },
            file("merge.txt", None, Some(Change::Conflicted)),
            file("notes/todo.md", None, Some(Change::Untracked)),
        ]
    );
    let staged: Vec<_> = status
        .files_on(DiffSide::Staged)
        .map(|file| file.path.as_str())
        .collect();
    assert_eq!(staged, ["src/staged.rs", "both.txt", "new name.txt"]);
    assert!(status.has_staged());
}

#[test]
fn initial_and_detached_heads() {
    let status = parse_status(b"# branch.oid (initial)\0# branch.head main\0");
    assert_eq!(status.head, Head::Unborn);
    assert_eq!(status.branch.as_deref(), Some("main"));
    assert!(!status.has_staged());

    let status =
        parse_status(format!("# branch.oid {HASH}\0# branch.head (detached)\0").as_bytes());
    assert_eq!(status.head, Head::Born);
    assert_eq!(status.branch, None);
}

#[test]
fn only_text_changes_stage_by_line() {
    let file = FileStatus {
        path: "a".to_string(),
        orig_path: None,
        staged: Some(Change::Added),
        unstaged: Some(Change::Untracked),
    };
    assert!(file.stages_by_line(DiffSide::Staged));
    assert!(!file.stages_by_line(DiffSide::Unstaged));
    let conflict = FileStatus {
        unstaged: Some(Change::Conflicted),
        ..file
    };
    assert!(!conflict.stages_by_line(DiffSide::Unstaged));
    let type_change = FileStatus {
        staged: Some(Change::TypeChanged),
        unstaged: Some(Change::TypeChanged),
        ..conflict
    };
    assert!(!type_change.stages_by_line(DiffSide::Staged));
    assert!(!type_change.stages_by_line(DiffSide::Unstaged));
}

#[test]
fn reads_a_real_repository() {
    let repo = TempDir::new();
    if !git(&repo.0, &["init", "-q", "-b", "main"]) {
        eprintln!("git not available, skipping");
        return;
    }
    write(&repo.0.join("new.txt"), "new\n");
    let status = read_status(&repo.0).unwrap();
    assert_eq!(status.head, Head::Unborn);
    assert_eq!(status.branch.as_deref(), Some("main"));
    assert_eq!(status.files[0].unstaged, Some(Change::Untracked));

    assert!(git(&repo.0, &["add", "."]));
    assert!(git(&repo.0, &["commit", "-q", "-m", "initial"]));
    write(&repo.0.join("new.txt"), "changed\n");
    let status = read_status(&repo.0).unwrap();
    assert_eq!(status.head, Head::Born);
    assert_eq!(status.files[0].unstaged, Some(Change::Modified));
}

#[test]
fn fails_outside_a_repository() {
    let folder = TempDir::new();
    assert!(matches!(read_status(&folder.0), Err(GitError::Failed(_))));
}

#[test]
fn rows_group_staged_then_unstaged_files() {
    let file = |path: &str, staged, unstaged| FileStatus {
        path: path.to_string(),
        orig_path: None,
        staged,
        unstaged,
    };
    let status = RepoStatus {
        files: vec![
            file("a", None, Some(Change::Modified)),
            file("b", Some(Change::Added), Some(Change::Modified)),
            file("c", Some(Change::Deleted), None),
        ],
        ..RepoStatus::default()
    };
    let (staged, unstaged) = (DiffSide::Staged, DiffSide::Unstaged);

    assert_eq!(
        status.rows(&HashSet::new()),
        [
            StatusRow::Header {
                side: staged,
                count: 2
            },
            StatusRow::File {
                side: staged,
                index: 1
            },
            StatusRow::File {
                side: staged,
                index: 2
            },
            StatusRow::Header {
                side: unstaged,
                count: 2
            },
            StatusRow::File {
                side: unstaged,
                index: 0
            },
            StatusRow::File {
                side: unstaged,
                index: 1
            },
        ]
    );
    assert_eq!(
        status.rows(&HashSet::from([staged])),
        [
            StatusRow::Header {
                side: staged,
                count: 2
            },
            StatusRow::Header {
                side: unstaged,
                count: 2
            },
            StatusRow::File {
                side: unstaged,
                index: 0
            },
            StatusRow::File {
                side: unstaged,
                index: 1
            },
        ]
    );
    assert!(RepoStatus::default().rows(&HashSet::new()).is_empty());
}
