//! A patch of some of a diff's lines, for `git apply --cached`: how VS Code stages or unstages
//! the selected lines of a file.
//!
//! Staging applies part of the unstaged diff (index to working tree) to the index, so the
//! patch's old side must match the index: an unselected removed line stays (as context) and an
//! unselected added line is left out. Unstaging applies part of the staged diff (HEAD to index)
//! in reverse, so the new side must match the index: an unselected added line stays (as
//! context) and an unselected removed line is left out.

use std::collections::BTreeSet;
use std::io::Write;

use super::diff::{DiffLine, Hunk, LineKind, TextDiff};
use super::status::DiffSide;

/// What git prints after a line that ends the file without a newline.
const NO_NEWLINE_LINE: &[u8] = b"\\ No newline at end of file\n";

/// A line of a [`TextDiff`]: its hunk, and its place in that hunk's lines.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct LineRef {
    pub hunk: usize,
    pub line: usize,
}

/// A patch staging (`side` Unstaged) or unstaging (`side` Staged, applied in reverse) the
/// `selected` added and removed lines of `diff`, the diff of `path` (relative to the repository
/// root), a file whose [`super::FileStatus::stages_by_line`] holds on `side`. Context lines in
/// `selected` are ignored. `None` when no changed line is selected.
///
/// Selecting every change gives the diff back as it is, so a new or deleted file is added or
/// removed. Otherwise the patch edits the file in place, except that unstaging part of a
/// staged deletion puts the file back in the index with those lines. Where the end of the file
/// has no newline, a selection that would leave the marker in the middle of a side takes in the
/// rest of that change, as git cannot apply it otherwise.
pub fn partial_patch(
    path: &str,
    diff: &TextDiff,
    selected: &BTreeSet<LineRef>,
    side: DiffSide,
) -> Option<Vec<u8>> {
    let changed = diff.changed_lines();
    let picked = changed
        .iter()
        .filter(|line| selected.contains(line))
        .count();
    if picked == 0 {
        return None;
    }
    // Unstaging part of a staged deletion recreates the file in the index: the patch must stay
    // a deletion (`+++ /dev/null`, its mode), as reversed it then creates the file.
    let recreates = side == DiffSide::Staged && diff.deletes_file();
    let mut patch = if picked == changed.len() || recreates {
        diff.header.clone()
    } else {
        file_header(path)
    };
    write_hunks(&mut patch, diff, selected, side);
    Some(patch)
}

/// Write each hunk of `diff` with a `selected` line to `patch`, keeping only those lines.
fn write_hunks(patch: &mut Vec<u8>, diff: &TextDiff, selected: &BTreeSet<LineRef>, side: DiffSide) {
    // How much longer the side being changed is than the side matching the index, so far.
    let mut delta = 0i64;
    for (index, hunk) in diff.hunks.iter().enumerate() {
        if diff.hunk_lines(index).is_disjoint(selected) {
            continue;
        }
        let lines = hunk_patch_lines(hunk, side, |line| {
            selected.contains(&LineRef { hunk: index, line })
        });
        write_hunk(patch, hunk, &lines, side, &mut delta);
    }
}

/// The lines of `hunk` its patch keeps: the `is_selected` ones, and the rest of a change at an
/// end without a newline when the selection would split it.
fn hunk_patch_lines(
    hunk: &Hunk,
    side: DiffSide,
    is_selected: impl Fn(usize) -> bool,
) -> Vec<(LineKind, &DiffLine)> {
    let lines = select_lines(hunk, side, &is_selected);
    if markers_at_ends(&lines) {
        return lines;
    }
    let tail = no_newline_changes(hunk);
    select_lines(hunk, side, |line| is_selected(line) || tail.contains(&line))
}

/// The lines of `hunk` the patch keeps, each with the kind it gets.
fn select_lines(
    hunk: &Hunk,
    side: DiffSide,
    is_selected: impl Fn(usize) -> bool,
) -> Vec<(LineKind, &DiffLine)> {
    // The kind whose unselected lines match the index and stay as context; the other kind's
    // unselected lines are left out.
    let kept = match side {
        DiffSide::Unstaged => LineKind::Removed,
        DiffSide::Staged => LineKind::Added,
    };
    hunk.lines
        .iter()
        .enumerate()
        .filter_map(|(index, line)| match line.kind {
            LineKind::Context => Some((LineKind::Context, line)),
            _ if is_selected(index) => Some((line.kind, line)),
            kind if kind == kept => Some((LineKind::Context, line)),
            _ => None,
        })
        .collect()
}

/// Whether every line without a final newline is the last line of each side it is on.
fn markers_at_ends(lines: &[(LineKind, &DiffLine)]) -> bool {
    let last_on =
        |on_side: fn(LineKind) -> bool| lines.iter().rposition(|(kind, _)| on_side(*kind));
    let last_old = last_on(|kind| kind != LineKind::Added);
    let last_new = last_on(|kind| kind != LineKind::Removed);
    lines
        .iter()
        .enumerate()
        .filter(|(_, (_, line))| line.no_newline)
        .all(|(index, (kind, _))| {
            let old_ok = *kind == LineKind::Added || last_old == Some(index);
            let new_ok = *kind == LineKind::Removed || last_new == Some(index);
            old_ok && new_ok
        })
}

/// The changed lines of each run of changes in `hunk` that has a line without a final newline.
fn no_newline_changes(hunk: &Hunk) -> BTreeSet<usize> {
    let mut tail = BTreeSet::new();
    let mut run = Vec::new();
    let mut run_has_marker = false;
    for (index, line) in hunk.lines.iter().enumerate() {
        if line.kind == LineKind::Context {
            if run_has_marker {
                tail.extend(run.drain(..));
            }
            run.clear();
            run_has_marker = false;
            continue;
        }
        run.push(index);
        run_has_marker |= line.no_newline;
    }
    if run_has_marker {
        tail.extend(run);
    }
    tail
}

/// `diff --git a/<path> b/<path>` and the `---` / `+++` lines, the path quoted as git does when
/// it has to be.
fn file_header(path: &str) -> Vec<u8> {
    let old = quote_path(&format!("a/{path}"));
    let new = quote_path(&format!("b/{path}"));
    format!("diff --git {old} {new}\n--- {old}\n+++ {new}\n").into_bytes()
}

/// `path` as git writes it in a patch: in double quotes with C escapes when it has a quote, a
/// backslash or a control character, else as it is.
fn quote_path(path: &str) -> String {
    let needs_quotes = path
        .chars()
        .any(|c| c == '"' || c == '\\' || c.is_ascii_control());
    if !needs_quotes {
        return path.to_string();
    }
    let mut quoted = String::from("\"");
    for c in path.chars() {
        match c {
            '"' => quoted.push_str("\\\""),
            '\\' => quoted.push_str("\\\\"),
            '\t' => quoted.push_str("\\t"),
            '\n' => quoted.push_str("\\n"),
            '\r' => quoted.push_str("\\r"),
            c if c.is_ascii_control() => quoted.push_str(&format!("\\{:03o}", c as u32)),
            c => quoted.push(c),
        }
    }
    quoted.push('"');
    quoted
}

/// Write the hunk header and `lines` to `patch`. The side matching the index keeps the
/// original start; the other side's start moves by `delta`, which grows by this hunk's change.
fn write_hunk(
    patch: &mut Vec<u8>,
    hunk: &Hunk,
    lines: &[(LineKind, &DiffLine)],
    side: DiffSide,
    delta: &mut i64,
) {
    let count = |kind: LineKind| lines.iter().filter(|(k, _)| *k != kind).count() as u32;
    let (old_len, new_len) = (count(LineKind::Added), count(LineKind::Removed));
    let (old_start, new_start) = match side {
        DiffSide::Unstaged => (
            hunk.old_start,
            moved_start(hunk.old_start, hunk.old_len, new_len, *delta),
        ),
        DiffSide::Staged => (
            moved_start(hunk.new_start, hunk.new_len, old_len, *delta),
            hunk.new_start,
        ),
    };
    *delta += match side {
        DiffSide::Unstaged => i64::from(new_len) - i64::from(old_len),
        DiffSide::Staged => i64::from(old_len) - i64::from(new_len),
    };
    let _ = writeln!(patch, "@@ -{old_start},{old_len} +{new_start},{new_len} @@");
    for (kind, line) in lines {
        patch.push(kind.prefix());
        patch.extend_from_slice(&line.content);
        patch.push(b'\n');
        if line.no_newline {
            patch.extend_from_slice(NO_NEWLINE_LINE);
        }
    }
}

/// The start of the side being changed, given the start and length of the side matching the
/// index. A side with no line is numbered by the line before it.
fn moved_start(anchor_start: u32, anchor_len: u32, len: u32, delta: i64) -> u32 {
    let first = i64::from(anchor_start) + i64::from(anchor_len == 0) + delta;
    let start = if len == 0 { first - 1 } else { first };
    u32::try_from(start.max(0)).unwrap_or(0)
}

#[cfg(test)]
mod tests;
