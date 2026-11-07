//! One file's diff, staged or unstaged, parsed into hunks and lines that can be shown and
//! staged one by one.

use std::borrow::Cow;
use std::collections::BTreeSet;
use std::iter::Peekable;
use std::path::Path;
use std::time::{Duration, Instant};

use super::patch::LineRef;
use super::runner::{Access, GitError, execute_capped};
use super::status::{Change, DiffSide, FileStatus};

/// How long [`read_diff`] waits for `git diff`.
const DIFF_TIMEOUT: Duration = Duration::from_secs(20);
/// A diff larger than this is not shown (the file can still be staged whole).
const MAX_DIFF_BYTES: usize = 8 << 20;
/// Lines of context around each change; [`super::patch`] relies on a fixed value.
const CONTEXT_ARG: &str = "-U3";
/// What `git diff --no-index` diffs an untracked file against.
const NULL_FILE: &str = "/dev/null";
/// Marks that the line before it has no newline at the end of the file.
const NO_NEWLINE_MARK: &[u8] = b"\\";
/// The first line of a file's section of a diff.
const SECTION_START: &[u8] = b"diff --git ";

/// A file's diff on one side.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum FileDiff {
    Text(TextDiff),
    /// Git shows no lines for it.
    Binary,
    /// Over [`MAX_DIFF_BYTES`]: not shown.
    TooLarge,
}

/// A text diff, its lines kept as git printed them (bytes: a patch must give them back
/// exactly, whatever their encoding or line ending).
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct TextDiff {
    /// The lines before the first hunk (`diff --git`, `index`, `---`, `+++`, ...), each ending
    /// with its newline.
    pub(super) header: Vec<u8>,
    pub hunks: Vec<Hunk>,
}

/// A `@@ -old_start,old_len +new_start,new_len @@` section.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Hunk {
    pub old_start: u32,
    pub old_len: u32,
    pub new_start: u32,
    pub new_len: u32,
    /// What git prints after the second `@@` (often the enclosing function), trimmed.
    pub section: String,
    pub lines: Vec<DiffLine>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum LineKind {
    Context,
    Added,
    Removed,
}

impl LineKind {
    pub(super) fn prefix(self) -> u8 {
        match self {
            Self::Context => b' ',
            Self::Added => b'+',
            Self::Removed => b'-',
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DiffLine {
    pub kind: LineKind,
    /// The line without its prefix or newline; a CR of a CRLF file stays.
    pub(super) content: Vec<u8>,
    /// Its 1-based number in the old file (not for added lines).
    pub old_no: Option<u32>,
    /// Its 1-based number in the new file (not for removed lines).
    pub new_no: Option<u32>,
    /// The file ends after this line without a newline.
    pub no_newline: bool,
}

impl DiffLine {
    /// The line as text to show: invalid UTF-8 replaced, a trailing CR dropped.
    pub fn text(&self) -> Cow<'_, str> {
        let content = self.content.strip_suffix(b"\r").unwrap_or(&self.content);
        String::from_utf8_lossy(content)
    }

    /// [`Self::text`] with each tab shown as `tab_width` spaces.
    pub fn display_text(&self, tab_width: usize) -> Cow<'_, str> {
        let text = self.text();
        if !text.contains('\t') {
            return text;
        }
        Cow::Owned(text.replace('\t', &" ".repeat(tab_width)))
    }

    /// How many characters [`Self::display_text`] has.
    fn display_width(&self, tab_width: usize) -> usize {
        self.text()
            .chars()
            .map(|c| if c == '\t' { tab_width } else { 1 })
            .sum()
    }
}

impl Hunk {
    /// Its header as git prints it: `@@ -12,7 +12,9 @@ fn section`.
    pub fn header_text(&self) -> String {
        let ranges = format!(
            "@@ -{},{} +{},{} @@",
            self.old_start, self.old_len, self.new_start, self.new_len
        );
        if self.section.is_empty() {
            ranges
        } else {
            format!("{ranges} {}", self.section)
        }
    }
}

/// A row of a diff's view: a hunk's header, a line, or the marker after a line that ends the
/// file without a newline.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DiffRow {
    /// The header of `hunks[i]`.
    Hunk(usize),
    Line(LineRef),
    /// "No newline at end of file", after the line it is about.
    NoNewline(LineRef),
}

impl TextDiff {
    /// Its rows, in order: each hunk's header, then its lines, a marker after a line that
    /// ends the file without a newline.
    pub fn rows(&self) -> Vec<DiffRow> {
        let mut rows = Vec::new();
        for (hunk_index, hunk) in self.hunks.iter().enumerate() {
            rows.push(DiffRow::Hunk(hunk_index));
            for (line_index, line) in hunk.lines.iter().enumerate() {
                let line_ref = LineRef {
                    hunk: hunk_index,
                    line: line_index,
                };
                rows.push(DiffRow::Line(line_ref));
                if line.no_newline {
                    rows.push(DiffRow::NoNewline(line_ref));
                }
            }
        }
        rows
    }

    /// How many lines it adds and removes.
    pub fn counts(&self) -> (usize, usize) {
        let lines = self.hunks.iter().flat_map(|hunk| &hunk.lines);
        lines.fold((0, 0), |(added, removed), line| match line.kind {
            LineKind::Added => (added + 1, removed),
            LineKind::Removed => (added, removed + 1),
            LineKind::Context => (added, removed),
        })
    }

    /// The largest line number, old or new (0 without lines).
    pub fn max_line_no(&self) -> u32 {
        self.hunks
            .iter()
            .flat_map(|hunk| &hunk.lines)
            .flat_map(|line| [line.old_no, line.new_no])
            .flatten()
            .max()
            .unwrap_or(0)
    }

    /// The added and removed lines, in order (what a line cursor moves over).
    pub fn changed_lines(&self) -> Vec<LineRef> {
        (0..self.hunks.len())
            .flat_map(|hunk| self.hunk_changes(hunk))
            .collect()
    }

    /// The added and removed lines of `hunks[hunk]` (none when there is no such hunk).
    pub fn hunk_lines(&self, hunk: usize) -> BTreeSet<LineRef> {
        self.hunk_changes(hunk).collect()
    }

    fn hunk_changes(&self, hunk: usize) -> impl Iterator<Item = LineRef> + '_ {
        let lines = self.hunks.get(hunk).map_or(&[][..], |h| h.lines.as_slice());
        lines
            .iter()
            .enumerate()
            .filter(|(_, line)| line.kind != LineKind::Context)
            .map(move |(line, _)| LineRef { hunk, line })
    }

    /// The line `line` refers to.
    pub fn line(&self, line: LineRef) -> Option<&DiffLine> {
        self.hunks.get(line.hunk)?.lines.get(line.line)
    }

    /// The index in `rows` of the widest row (a hunk header or a line, tabs as `tab_width`
    /// characters): a list sized to it fits them all.
    pub fn widest_row(&self, rows: &[DiffRow], tab_width: usize) -> Option<usize> {
        let width = |row: &DiffRow| match *row {
            DiffRow::Hunk(index) => self.hunks[index].header_text().chars().count(),
            DiffRow::Line(line) => self
                .line(line)
                .map_or(0, |line| line.display_width(tab_width)),
            DiffRow::NoNewline(_) => 0,
        };
        rows.iter()
            .enumerate()
            .max_by_key(|(index, row)| (width(row), std::cmp::Reverse(*index)))
            .map(|(index, _)| index)
    }
}

/// The diff of `file` on `side`, in the repository whose root is `root`. An untracked file
/// shows as all added. Runs `git`: call it off the UI thread.
pub fn read_diff(root: &Path, file: &FileStatus, side: DiffSide) -> Result<FileDiff, GitError> {
    let untracked = side == DiffSide::Unstaged && file.unstaged == Some(Change::Untracked);
    let mut args = vec![
        "diff",
        "--no-color",
        "--no-ext-diff",
        "--no-textconv",
        CONTEXT_ARG,
        "--src-prefix=a/",
        "--dst-prefix=b/",
    ];
    if untracked {
        args.extend(["--no-index", "--", NULL_FILE, file.path.as_str()]);
    } else {
        if side == DiffSide::Staged {
            args.push("--cached");
        }
        args.push("--");
        args.push(&file.path);
        // Both paths, so a staged rename shows as one.
        if let Some(orig) = file
            .orig_path
            .as_deref()
            .filter(|_| side == DiffSide::Staged)
        {
            args.push(orig);
        }
    }
    let literal = ["--literal-pathspecs"];
    let args: Vec<&str> = literal.iter().copied().chain(args).collect();
    // One byte past the limit is enough to tell the diff is too large to show.
    let output = execute_capped(
        root,
        &args,
        None,
        Access::Read,
        Instant::now() + DIFF_TIMEOUT,
        MAX_DIFF_BYTES + 1,
    )?;
    // `--no-index` exits with 1 when the files differ, as they always do here.
    let output = if untracked && output.status.code() == Some(1) {
        output.stdout
    } else {
        output.into_result()?
    };
    Ok(parse_diff(&output))
}

/// `git diff` output for one file. A type change (a symlink becoming a file) comes as two
/// sections, the old file's removal then the new file's addition: the hunks of both are kept,
/// the header of the first only.
pub(super) fn parse_diff(output: &[u8]) -> FileDiff {
    if output.len() > MAX_DIFF_BYTES {
        return FileDiff::TooLarge;
    }
    let mut lines = output.split_inclusive(|&byte| byte == b'\n').peekable();
    let mut diff = TextDiff::default();
    let mut header = Some(&mut diff.header);
    loop {
        while let Some(line) = lines.next_if(|line| !line.starts_with(b"@@")) {
            if line.starts_with(b"Binary files ") || line.starts_with(b"GIT binary patch") {
                return FileDiff::Binary;
            }
            if let Some(header) = header.as_deref_mut() {
                header.extend_from_slice(line);
            }
        }
        header = None;
        while let Some(line) = lines.next() {
            let Some(hunk) = parse_hunk(line, &mut lines) else {
                break;
            };
            diff.hunks.push(hunk);
            if !lines.peek().is_some_and(|line| line.starts_with(b"@@")) {
                break;
            }
        }
        if !lines
            .peek()
            .is_some_and(|line| line.starts_with(SECTION_START))
        {
            break;
        }
    }
    FileDiff::Text(diff)
}

/// The hunk whose header is `header`, with its lines taken from `lines` (up to the next hunk,
/// the next section, or a line that is no diff line); `None` when `header` is not one.
fn parse_hunk<'a>(
    header: &[u8],
    lines: &mut Peekable<impl Iterator<Item = &'a [u8]>>,
) -> Option<Hunk> {
    let mut hunk = parse_hunk_header(header)?;
    let (mut old_no, mut new_no) = (hunk.old_start, hunk.new_start);
    while let Some(line) = lines.next_if(|line| is_hunk_line(line)) {
        let line = line.strip_suffix(b"\n").unwrap_or(line);
        if line.starts_with(NO_NEWLINE_MARK) {
            if let Some(last) = hunk.lines.last_mut() {
                last.no_newline = true;
            }
            continue;
        }
        let (kind, content) = match line.split_first() {
            Some((b'+', rest)) => (LineKind::Added, rest),
            Some((b'-', rest)) => (LineKind::Removed, rest),
            // A space: `is_hunk_line` lets no other first byte through.
            Some((_, rest)) => (LineKind::Context, rest),
            // Some tools strip the space of an empty context line.
            None => (LineKind::Context, line),
        };
        let old = (kind != LineKind::Added).then(|| post_increment(&mut old_no));
        let new = (kind != LineKind::Removed).then(|| post_increment(&mut new_no));
        hunk.lines.push(DiffLine {
            kind,
            content: content.to_vec(),
            old_no: old,
            new_no: new,
            no_newline: false,
        });
    }
    Some(hunk)
}

/// Whether `line` (with its newline) is a line of a hunk: added, removed, context, the
/// no-newline marker, or an empty context line whose space was stripped.
fn is_hunk_line(line: &[u8]) -> bool {
    matches!(
        line.first(),
        Some(b'+' | b'-' | b' ' | b'\\' | b'\n') | None
    )
}

fn post_increment(number: &mut u32) -> u32 {
    let current = *number;
    *number += 1;
    current
}

/// `@@ -1,3 +1,4 @@ section`; a missing length is 1.
fn parse_hunk_header(line: &[u8]) -> Option<Hunk> {
    let line = String::from_utf8_lossy(line);
    let rest = line.strip_prefix("@@ -")?;
    let (ranges, section) = rest.split_once(" @@")?;
    let (old, new) = ranges.split_once(" +")?;
    let (old_start, old_len) = parse_range(old)?;
    let (new_start, new_len) = parse_range(new)?;
    Some(Hunk {
        old_start,
        old_len,
        new_start,
        new_len,
        section: section.trim().to_string(),
        lines: Vec::new(),
    })
}

fn parse_range(range: &str) -> Option<(u32, u32)> {
    match range.split_once(',') {
        Some((start, len)) => Some((start.parse().ok()?, len.parse().ok()?)),
        None => Some((range.parse().ok()?, 1)),
    }
}

#[cfg(test)]
mod tests;
