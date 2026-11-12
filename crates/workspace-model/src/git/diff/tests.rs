use super::*;
use crate::git::tests::{TempDir, git, write};

const MODIFIED: &str = "\
diff --git a/src/lib.rs b/src/lib.rs
index 1111111..2222222 100644
--- a/src/lib.rs
+++ b/src/lib.rs
@@ -1,3 +1,3 @@ fn main() {
 one
-two
+deux
 three
@@ -10,2 +10,3 @@
 ten
 eleven
+twelve
\\ No newline at end of file
";

fn text(diff: FileDiff) -> TextDiff {
    match diff {
        FileDiff::Text(diff) => diff,
        other => panic!("not a text diff: {other:?}"),
    }
}

#[test]
fn parses_hunks_lines_and_numbers() {
    let diff = text(parse_diff(MODIFIED.as_bytes()));

    assert!(diff.header.starts_with(b"diff --git"));
    assert!(diff.header.ends_with(b"+++ b/src/lib.rs\n"));
    assert_eq!(diff.hunks.len(), 2);
    let first = &diff.hunks[0];
    assert_eq!(
        (
            first.old_start,
            first.old_len,
            first.new_start,
            first.new_len
        ),
        (1, 3, 1, 3)
    );
    assert_eq!(first.section, "fn main() {");
    let summary: Vec<_> = first
        .lines
        .iter()
        .map(|line| {
            (
                line.kind,
                line.text().into_owned(),
                line.old_no,
                line.new_no,
            )
        })
        .collect();
    assert_eq!(
        summary,
        [
            (LineKind::Context, "one".into(), Some(1), Some(1)),
            (LineKind::Removed, "two".into(), Some(2), None),
            (LineKind::Added, "deux".into(), None, Some(2)),
            (LineKind::Context, "three".into(), Some(3), Some(3)),
        ]
    );
    let last = diff.hunks[1].lines.last().unwrap();
    assert_eq!(last.text(), "twelve");
    assert_eq!(last.new_no, Some(12));
    assert!(last.no_newline);
    assert!(!diff.hunks[1].lines[0].no_newline);
}

/// What `git diff` prints for a symlink that became a regular file.
const TYPE_CHANGE: &str = "\
diff --git a/f b/f
deleted file mode 120000
index 1de5659..0000000
--- a/f
+++ /dev/null
@@ -1 +0,0 @@
-target
\\ No newline at end of file
diff --git a/f b/f
new file mode 100644
index 0000000..273a402
--- /dev/null
+++ b/f
@@ -0,0 +1 @@
+body
";

#[test]
fn a_type_change_shows_the_removal_and_the_addition() {
    let diff = text(parse_diff(TYPE_CHANGE.as_bytes()));

    assert!(
        diff.header
            .starts_with(b"diff --git a/f b/f\ndeleted file mode")
    );
    assert!(diff.header.ends_with(b"+++ /dev/null\n"));
    let lines: Vec<_> = diff
        .hunks
        .iter()
        .flat_map(|hunk| &hunk.lines)
        .map(|line| (line.kind, line.text().into_owned(), line.no_newline))
        .collect();
    assert_eq!(
        lines,
        [
            (LineKind::Removed, "target".into(), true),
            (LineKind::Added, "body".into(), false),
        ]
    );
    assert_eq!(diff.counts(), (1, 1));
}

#[test]
fn a_second_section_that_is_binary_makes_the_diff_binary() {
    let output = TYPE_CHANGE.replace(
        "@@ -0,0 +1 @@\n+body\n",
        "Binary files /dev/null and b/f differ\n",
    );
    assert_eq!(parse_diff(output.as_bytes()), FileDiff::Binary);
}

#[test]
fn a_hunk_ends_at_a_line_that_is_no_diff_line() {
    let diff = text(parse_diff(
        b"@@ -1 +1 @@\n-a\n+b\nwarning: trailing junk\n@@ -9 +9 @@\n-z\n",
    ));
    assert_eq!(diff.hunks.len(), 1);
    assert_eq!(diff.hunks[0].lines.len(), 2);
}

#[test]
fn a_missing_length_means_one() {
    let hunk = parse_hunk_header(b"@@ -5 +5,0 @@\n").unwrap();
    assert_eq!(
        (hunk.old_start, hunk.old_len, hunk.new_start, hunk.new_len),
        (5, 1, 5, 0)
    );
    assert_eq!(hunk.section, "");
    assert!(parse_hunk_header(b"@@ garbage").is_none());
}

#[test]
fn keeps_crlf_and_shows_it_without_cr() {
    let diff = text(parse_diff(
        b"--- a/w.txt\n+++ b/w.txt\n@@ -1 +1 @@\n-old\r\n+new\r\n",
    ));
    let added = &diff.hunks[0].lines[1];
    assert_eq!(added.content, b"new\r");
    assert_eq!(added.text(), "new");
}

#[test]
fn binary_and_empty_diffs() {
    let binary =
        b"diff --git a/a.png b/a.png\nindex 1..2 100644\nBinary files a/a.png and b/a.png differ\n";
    assert_eq!(parse_diff(binary), FileDiff::Binary);
    assert_eq!(parse_diff(b""), FileDiff::Text(TextDiff::default()));
}

#[test]
fn reads_staged_unstaged_and_untracked_diffs() {
    let repo = TempDir::new();
    if !git(&repo.0, &["init", "-q"]) {
        eprintln!("git not available, skipping");
        return;
    }
    write(&repo.0.join("a.txt"), "one\ntwo\n");
    assert!(git(&repo.0, &["add", "."]));
    assert!(git(&repo.0, &["commit", "-q", "-m", "initial"]));
    write(&repo.0.join("a.txt"), "one\n2\n");
    assert!(git(&repo.0, &["add", "a.txt"]));
    write(&repo.0.join("a.txt"), "one\n2\nthree\n");
    write(&repo.0.join("new file.txt"), "x\ny\n");
    let file = |path: &str, staged, unstaged| FileStatus {
        path: path.to_string(),
        orig_path: None,
        staged,
        unstaged,
    };
    let a = file("a.txt", Some(Change::Modified), Some(Change::Modified));

    let staged = text(read_diff(&repo.0, &a, DiffSide::Staged).unwrap());
    let staged: Vec<_> = staged.hunks[0].lines.iter().map(DiffLine::text).collect();
    assert_eq!(staged, ["one", "two", "2"]);

    let unstaged = text(read_diff(&repo.0, &a, DiffSide::Unstaged).unwrap());
    assert_eq!(unstaged.hunks[0].lines.last().unwrap().text(), "three");
    assert_eq!(
        unstaged.hunks[0].lines.last().unwrap().kind,
        LineKind::Added
    );

    let untracked = file("new file.txt", None, Some(Change::Untracked));
    let untracked = text(read_diff(&repo.0, &untracked, DiffSide::Unstaged).unwrap());
    let kinds: Vec<_> = untracked.hunks[0].lines.iter().map(|l| l.kind).collect();
    assert_eq!(kinds, [LineKind::Added, LineKind::Added]);
}

#[test]
fn a_diff_over_the_limit_reads_as_too_large() {
    let repo = TempDir::new();
    if !git(&repo.0, &["init", "-q"]) {
        eprintln!("git not available, skipping");
        return;
    }
    let line = "a line of an untracked file far too large to show\n";
    write(
        &repo.0.join("big.log"),
        &line.repeat(MAX_DIFF_BYTES / line.len() + 1),
    );
    let big = FileStatus {
        path: "big.log".to_string(),
        orig_path: None,
        staged: None,
        unstaged: Some(Change::Untracked),
    };

    assert_eq!(
        read_diff(&repo.0, &big, DiffSide::Unstaged),
        Ok(FileDiff::TooLarge)
    );
}

#[test]
fn rows_list_hunk_headers_lines_and_no_newline_markers() {
    let diff = text(parse_diff(MODIFIED.as_bytes()));
    let line = |hunk, line| LineRef { hunk, line };

    let rows = diff.rows();

    assert_eq!(
        rows,
        [
            DiffRow::Hunk(0),
            DiffRow::Line(line(0, 0)),
            DiffRow::Line(line(0, 1)),
            DiffRow::Line(line(0, 2)),
            DiffRow::Line(line(0, 3)),
            DiffRow::Hunk(1),
            DiffRow::Line(line(1, 0)),
            DiffRow::Line(line(1, 1)),
            DiffRow::Line(line(1, 2)),
            DiffRow::NoNewline(line(1, 2)),
        ]
    );
    assert!(TextDiff::default().rows().is_empty());
}

#[test]
fn counts_added_and_removed_lines() {
    let diff = text(parse_diff(MODIFIED.as_bytes()));

    assert_eq!(diff.counts(), (2, 1));
    assert_eq!(diff.max_line_no(), 12);
    assert_eq!(TextDiff::default().counts(), (0, 0));
    assert_eq!(TextDiff::default().max_line_no(), 0);
}

#[test]
fn hunk_header_text_shows_ranges_and_section() {
    let diff = text(parse_diff(MODIFIED.as_bytes()));

    assert_eq!(diff.hunks[0].header_text(), "@@ -1,3 +1,3 @@ fn main() {");
    assert_eq!(diff.hunks[1].header_text(), "@@ -10,2 +10,3 @@");
}

#[test]
fn display_text_expands_tabs() {
    let diff = text(parse_diff(b"@@ -1 +1 @@\n-\tx\r\n+a\tb\n"));
    let removed = &diff.hunks[0].lines[0];

    assert_eq!(removed.display_text(4), "    x");
    assert_eq!(diff.hunks[0].lines[1].display_text(2), "a  b");
    assert_eq!(removed.display_width(4), 5);
}

#[test]
fn widest_row_counts_tabs_and_hunk_headers() {
    let diff = text(parse_diff(b"@@ -1 +1 @@\n-\t\tx\n+abcdefghijklmnopq\n"));
    let rows = diff.rows();

    // The header `@@ -1,1 +1,1 @@` has 15 characters, the added line 17, the removed one 9.
    assert_eq!(diff.widest_row(&rows, 4), Some(2));
    // With wide tabs the removed line is wider.
    assert_eq!(diff.widest_row(&rows, 10), Some(1));
    assert_eq!(TextDiff::default().widest_row(&[], 4), None);
}

#[test]
fn lists_the_changed_lines_in_order_and_by_hunk() {
    let diff = text(parse_diff(MODIFIED.as_bytes()));
    let at = |hunk, line| LineRef { hunk, line };

    assert_eq!(diff.changed_lines(), [at(0, 1), at(0, 2), at(1, 2)]);
    assert_eq!(diff.hunk_lines(0), BTreeSet::from([at(0, 1), at(0, 2)]));
    assert_eq!(diff.hunk_lines(1), BTreeSet::from([at(1, 2)]));
    assert!(diff.hunk_lines(2).is_empty());
}
