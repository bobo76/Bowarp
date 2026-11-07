use std::fs;
use std::path::Path;

use super::*;
use crate::git::diff::{FileDiff, parse_diff, read_diff};
use crate::git::status::read_status;
use crate::git::tests::{git, index_content, init_repo};
use crate::git::{Change, apply_patch};

fn text_diff(diff: FileDiff) -> TextDiff {
    match diff {
        FileDiff::Text(diff) => diff,
        other => panic!("not a text diff: {other:?}"),
    }
}

/// The lines of `diff` whose text and kind match one of `wanted` (`+x`, `-x`).
fn pick(diff: &TextDiff, wanted: &[&str]) -> BTreeSet<LineRef> {
    let mut picked = BTreeSet::new();
    for (hunk, lines) in diff.hunks.iter().enumerate() {
        for (line, diff_line) in lines.lines.iter().enumerate() {
            let prefix = char::from(diff_line.kind.prefix());
            if wanted.contains(&format!("{prefix}{}", diff_line.text()).as_str()) {
                picked.insert(LineRef { hunk, line });
            }
        }
    }
    assert!(!picked.is_empty(), "no line matches {wanted:?}");
    picked
}

/// Commit `path` with `committed`, write `worktree` over it, and stage everything first when
/// `staged`.
fn repo_with_change(
    path: &str,
    committed: &str,
    worktree: Option<&str>,
    staged: bool,
) -> Option<crate::git::tests::TempDir> {
    let repo = init_repo()?;
    let file = repo.0.join(path);
    fs::write(&file, committed).unwrap();
    assert!(git(&repo.0, &["add", "."]));
    assert!(git(&repo.0, &["commit", "-q", "-m", "initial"]));
    match worktree {
        Some(content) => fs::write(&file, content).unwrap(),
        None => fs::remove_file(&file).unwrap(),
    }
    if staged {
        assert!(git(&repo.0, &["add", "--all"]));
    }
    Some(repo)
}

/// Apply the lines matching `wanted` of `path`'s diff on `side`, and return the index content.
fn apply_lines(repo: &Path, path: &str, side: DiffSide, wanted: &[&str]) -> Option<String> {
    let status = read_status(repo).unwrap();
    let file = status
        .files
        .iter()
        .find(|file| file.path == path)
        .expect("file has changes");
    let diff = text_diff(read_diff(repo, file, side).unwrap());
    let patch = partial_patch(path, &diff, &pick(&diff, wanted), side).unwrap();
    apply_patch(repo, patch, side).unwrap_or_else(|err| panic!("patch did not apply: {err}"));
    index_content(repo, path).map(|bytes| String::from_utf8(bytes).unwrap())
}

const COMMITTED: &str = "a\nb\nc\nd\n";
const EDITED: &str = "a\nB\nc\nD\nE\n";

#[test]
fn stages_only_the_selected_lines() {
    let cases: [(&[&str], &str); 4] = [
        (&["-b", "+B"], "a\nB\nc\nd\n"),
        (&["+E"], "a\nb\nc\nd\nE\n"),
        (&["-d"], "a\nb\nc\n"),
        (&["+B", "+D"], "a\nb\nB\nc\nd\nD\n"),
    ];
    for (wanted, index) in cases {
        let Some(repo) = repo_with_change("f.txt", COMMITTED, Some(EDITED), false) else {
            return;
        };
        let staged = apply_lines(&repo.0, "f.txt", DiffSide::Unstaged, wanted);
        assert_eq!(staged.as_deref(), Some(index), "staging {wanted:?}");
        assert_eq!(fs::read_to_string(repo.0.join("f.txt")).unwrap(), EDITED);
    }
}

#[test]
fn unstages_only_the_selected_lines() {
    let cases: [(&[&str], &str); 3] = [
        (&["+E"], "a\nB\nc\nD\n"),
        (&["-b"], "a\nb\nB\nc\nD\nE\n"),
        (&["-b", "+B", "-d", "+D"], "a\nb\nc\nd\nE\n"),
    ];
    for (wanted, index) in cases {
        let Some(repo) = repo_with_change("f.txt", COMMITTED, Some(EDITED), true) else {
            return;
        };
        let unstaged = apply_lines(&repo.0, "f.txt", DiffSide::Staged, wanted);
        assert_eq!(unstaged.as_deref(), Some(index), "unstaging {wanted:?}");
    }
}

/// Lines `1` to `20`, each replaced by what `edit` gives for its number.
fn numbered(edit: impl Fn(u32) -> Vec<String>) -> String {
    (1..=20)
        .flat_map(edit)
        .map(|line| {
            line + "
"
        })
        .collect()
}

#[test]
fn stages_lines_of_two_hunks_at_once() {
    let committed = numbered(|n| vec![n.to_string()]);
    let edited = numbered(|n| match n {
        2 => vec!["2a".into(), "2b".into(), "2c".into()],
        18 => vec!["eighteen".into()],
        n => vec![n.to_string()],
    });
    let Some(repo) = repo_with_change("n.txt", &committed, Some(&edited), false) else {
        return;
    };
    let staged = apply_lines(
        &repo.0,
        "n.txt",
        DiffSide::Unstaged,
        &["+2b", "-18", "+eighteen"],
    );
    let expected = numbered(|n| match n {
        2 => vec!["2".into(), "2b".into()],
        18 => vec!["eighteen".into()],
        n => vec![n.to_string()],
    });
    assert_eq!(staged, Some(expected));

    // Then a line of the later hunk alone, the earlier one left out.
    let Some(repo) = repo_with_change("n.txt", &committed, Some(&edited), false) else {
        return;
    };
    let staged = apply_lines(&repo.0, "n.txt", DiffSide::Unstaged, &["+eighteen"]);
    let expected = numbered(|n| match n {
        18 => vec!["18".into(), "eighteen".into()],
        n => vec![n.to_string()],
    });
    assert_eq!(staged, Some(expected));
}

#[test]
fn a_last_line_without_newline_takes_in_its_whole_change() {
    let Some(repo) = repo_with_change("e.txt", "a\nb", Some("a\nb\nc"), false) else {
        return;
    };
    // `-b` must stay as context but has no newline, so `+c` cannot follow it: the whole run
    // (`-b +b +c`) is staged.
    let staged = apply_lines(&repo.0, "e.txt", DiffSide::Unstaged, &["+c"]);
    assert_eq!(staged.as_deref(), Some("a\nb\nc"));
}

#[test]
fn keeps_a_missing_final_newline_when_staging_elsewhere() {
    let Some(repo) = repo_with_change("e.txt", "a\nb\nc", Some("A\nb\nC"), false) else {
        return;
    };
    let staged = apply_lines(&repo.0, "e.txt", DiffSide::Unstaged, &["-a", "+A"]);
    assert_eq!(staged.as_deref(), Some("A\nb\nc"));
}

#[test]
fn part_of_a_deleted_file_edits_it_instead() {
    let Some(repo) = repo_with_change("gone.txt", "x\ny\n", None, false) else {
        return;
    };
    let staged = apply_lines(&repo.0, "gone.txt", DiffSide::Unstaged, &["-x"]);
    assert_eq!(staged.as_deref(), Some("y\n"));
    let status = read_status(&repo.0).unwrap();
    assert_eq!(status.files[0].staged, Some(Change::Modified));
    assert_eq!(status.files[0].unstaged, Some(Change::Deleted));

    // All of what is left deletes it.
    let staged = apply_lines(&repo.0, "gone.txt", DiffSide::Unstaged, &["-y"]);
    assert_eq!(staged, None);
}

#[test]
fn unstaging_part_of_a_new_file_keeps_the_rest() {
    let Some(repo) = init_repo() else {
        return;
    };
    fs::write(repo.0.join("seed.txt"), "seed\n").unwrap();
    assert!(git(&repo.0, &["add", "."]));
    assert!(git(&repo.0, &["commit", "-q", "-m", "initial"]));
    fs::write(repo.0.join("with space.txt"), "p\nq\n").unwrap();
    assert!(git(&repo.0, &["add", "."]));

    let index = apply_lines(&repo.0, "with space.txt", DiffSide::Staged, &["+q"]);
    assert_eq!(index.as_deref(), Some("p\n"));
    let index = apply_lines(&repo.0, "with space.txt", DiffSide::Staged, &["+p"]);
    assert_eq!(
        index, None,
        "unstaging every line takes the file out of the index"
    );
}

#[test]
fn crlf_lines_stay_crlf() {
    let Some(repo) = repo_with_change("w.txt", "a\r\nb\r\n", Some("a\r\nB\r\nc\r\n"), false) else {
        return;
    };
    let staged = apply_lines(&repo.0, "w.txt", DiffSide::Unstaged, &["+c"]);
    assert_eq!(staged.as_deref(), Some("a\r\nb\r\nc\r\n"));
}

#[test]
fn nothing_selected_gives_no_patch() {
    let FileDiff::Text(diff) = parse_diff(b"--- a/f\n+++ b/f\n@@ -1,2 +1,2 @@\n x\n-y\n+z\n")
    else {
        panic!("text diff expected");
    };
    let context = BTreeSet::from([LineRef { hunk: 0, line: 0 }]);
    assert_eq!(
        partial_patch("f", &diff, &context, DiffSide::Unstaged),
        None
    );
    assert_eq!(
        partial_patch("f", &diff, &BTreeSet::new(), DiffSide::Staged),
        None
    );
}

#[test]
fn builds_the_expected_patch_text() {
    let FileDiff::Text(diff) =
        parse_diff(b"diff --git a/f b/f\n--- a/f\n+++ b/f\n@@ -1,2 +1,3 @@\n x\n-y\n+z\n+w\n")
    else {
        panic!("text diff expected");
    };
    let selected = BTreeSet::from([LineRef { hunk: 0, line: 3 }]);
    let patch = partial_patch("f", &diff, &selected, DiffSide::Unstaged).unwrap();
    assert_eq!(
        String::from_utf8(patch).unwrap(),
        "diff --git a/f b/f\n--- a/f\n+++ b/f\n@@ -1,2 +1,3 @@\n x\n y\n+w\n"
    );
    let patch = partial_patch("f", &diff, &selected, DiffSide::Staged).unwrap();
    assert_eq!(
        String::from_utf8(patch).unwrap(),
        "diff --git a/f b/f\n--- a/f\n+++ b/f\n@@ -1,2 +1,3 @@\n x\n z\n+w\n"
    );
}

#[test]
fn quotes_paths_only_when_git_would() {
    assert_eq!(quote_path("a/src/main.rs"), "a/src/main.rs");
    assert_eq!(quote_path("a/with space.txt"), "a/with space.txt");
    assert_eq!(quote_path("a/été.md"), "a/été.md");
    assert_eq!(quote_path("a/tab\there"), "\"a/tab\\there\"");
    assert_eq!(quote_path("a/q\"\\"), "\"a/q\\\"\\\\\"");
    assert_eq!(quote_path("a/bell\u{7}"), "\"a/bell\\007\"");
}

#[test]
fn moves_starts_by_the_earlier_hunks() {
    assert_eq!(moved_start(10, 3, 4, 2), 12);
    // An insertion at the top (`-0,0`) starts at line 1 on the other side.
    assert_eq!(moved_start(0, 0, 2, 0), 1);
    // A side left with no line is numbered by the line before it.
    assert_eq!(moved_start(1, 3, 0, 0), 0);
}
