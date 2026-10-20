//! Commands run in the app's shell tabs, for the prompt's inline suggestion and the history
//! popup. Each entry keeps the folder it ran in and when. A command is kept once, at its latest
//! use, and only the newest [`CommandHistory::DEFAULT_CAP`] are kept.
//!
//! Saved as JSON lines (`history.jsonl` in the config folder): each run appends one line, so
//! saving costs no rewrite and two instances can append to the same file. Loading replays the
//! lines, oldest first, and the file is rewritten compacted once it holds many more lines than
//! entries ([`CommandHistory::needs_compaction`]).

use std::collections::HashSet;
use std::fs::{self, File, OpenOptions};
use std::io::{self, Read, Seek, SeekFrom, Write};
use std::ops::Range;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

/// One command as it ran.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct HistoryEntry {
    #[serde(rename = "cmd")]
    pub command: String,
    /// The folder it ran in; `None` when unknown or not UTF-8 (JSON holds only UTF-8 paths).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub dir: Option<PathBuf>,
    /// Seconds since the Unix epoch.
    #[serde(default)]
    pub time: u64,
}

impl HistoryEntry {
    pub fn new(command: impl Into<String>, dir: Option<PathBuf>, time: u64) -> Self {
        Self {
            command: command.into(),
            dir: dir.filter(|dir| dir.to_str().is_some()),
            time,
        }
    }
}

/// Whether `command` belongs in the history: not blank, a single line without control
/// characters (it is typed back into a prompt), and not starting with a space, which shells
/// (`HISTCONTROL=ignorespace`, zsh `HIST_IGNORE_SPACE`) use to keep a command out of history.
pub fn is_recordable(command: &str) -> bool {
    !command.trim().is_empty()
        && !command.starts_with(' ')
        && !command.chars().any(char::is_control)
}

/// The commands, oldest first, each command at most once.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CommandHistory {
    entries: Vec<HistoryEntry>,
    cap: usize,
}

impl Default for CommandHistory {
    fn default() -> Self {
        Self::new(Self::DEFAULT_CAP)
    }
}

impl CommandHistory {
    pub const DEFAULT_CAP: usize = 10_000;

    pub fn new(cap: usize) -> Self {
        Self {
            entries: Vec::new(),
            cap: cap.max(1),
        }
    }

    /// Replay JSON lines, oldest first; damaged or unrecordable lines are skipped. Returns the
    /// history and how many lines the text held.
    pub fn from_jsonl(text: &str, cap: usize) -> (Self, usize) {
        let lines: Vec<&str> = text
            .lines()
            .filter(|line| !line.trim().is_empty())
            .collect();
        let mut seen = HashSet::new();
        let mut newest_first = Vec::new();
        for line in lines.iter().rev() {
            if newest_first.len() >= cap.max(1) {
                break;
            }
            let Ok(entry) = serde_json::from_str::<HistoryEntry>(line) else {
                continue;
            };
            if is_recordable(&entry.command) && seen.insert(entry.command.clone()) {
                newest_first.push(entry);
            }
        }
        newest_first.reverse();
        let history = Self {
            entries: newest_first,
            cap: cap.max(1),
        };
        (history, lines.len())
    }

    /// All entries as JSON lines, oldest first.
    pub fn to_jsonl(&self) -> String {
        self.entries.iter().filter_map(entry_line).collect()
    }

    /// Whether a file of `lines` lines holding this history should be rewritten: it has grown
    /// past half again the cap.
    pub fn needs_compaction(&self, lines: usize) -> bool {
        lines > self.cap + self.cap / 2
    }

    /// Record `entry` as the latest use of its command, dropping its earlier use and the
    /// oldest entry beyond the cap. Returns false (nothing recorded) when the command is not
    /// recordable.
    pub fn add(&mut self, entry: HistoryEntry) -> bool {
        if !is_recordable(&entry.command) {
            return false;
        }
        self.entries.retain(|old| old.command != entry.command);
        self.entries.push(entry);
        if self.entries.len() > self.cap {
            let excess = self.entries.len() - self.cap;
            self.entries.drain(..excess);
        }
        true
    }

    /// Oldest first.
    pub fn entries(&self) -> &[HistoryEntry] {
        &self.entries
    }

    pub fn len(&self) -> usize {
        self.entries.len()
    }

    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    /// What the most recent command starting with `typed` adds to it, for the inline
    /// suggestion. Case-sensitive, since accepting it types the rest after what is there.
    /// `None` for an empty `typed` or when no longer command starts with it.
    pub fn suggestion(&self, typed: &str) -> Option<&str> {
        if typed.is_empty() {
            return None;
        }
        self.entries.iter().rev().find_map(|entry| {
            entry
                .command
                .strip_prefix(typed)
                .filter(|rest| !rest.is_empty())
        })
    }

    /// The entries containing `query` (ignoring case), oldest first; all of them for a blank
    /// query.
    pub fn matching(&self, query: &str) -> Vec<&HistoryEntry> {
        let query = query.trim();
        if query.is_empty() {
            return self.entries.iter().collect();
        }
        // Called on every frame while the popup is open: no allocation per entry for the usual
        // ASCII query.
        if query.is_ascii() {
            let query = query.as_bytes();
            return self
                .entries
                .iter()
                .filter(|entry| {
                    entry
                        .command
                        .as_bytes()
                        .windows(query.len())
                        .any(|window| window.eq_ignore_ascii_case(query))
                })
                .collect();
        }
        let query = query.to_lowercase();
        self.entries
            .iter()
            .filter(|entry| entry.command.to_lowercase().contains(&query))
            .collect()
    }

    /// Load the history saved at `path`, rewriting the file compacted when it has grown too
    /// long. A missing or unreadable file gives an empty history.
    pub fn load(path: &Path, cap: usize) -> Self {
        let Ok(bytes) = fs::read(path) else {
            return Self::new(cap);
        };
        // A line that is not UTF-8 (damaged) is skipped; it must not hide the rest.
        let mut text = String::with_capacity(bytes.len());
        for line in bytes.split(|&byte| byte == b'\n') {
            if let Ok(line) = std::str::from_utf8(line) {
                text.push_str(line);
                text.push('\n');
            }
        }
        let (history, lines) = Self::from_jsonl(&text, cap);
        if history.needs_compaction(lines)
            && let Err(err) = compact(path, &history.to_jsonl(), bytes.len() as u64)
        {
            eprintln!(
                "agent-workspace: failed to compact {}: {err}",
                path.display()
            );
        }
        history
    }
}

/// The history popup's selection, counted from the newest match (0), and how many of the
/// newest matches are scrolled out below the visible rows. The list shows the newest match at
/// the bottom, next to the prompt.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct HistorySelection {
    pub selected: usize,
    pub hidden_below: usize,
}

impl HistorySelection {
    /// Move `by` rows up, toward older matches, scrolling to keep the selection in view.
    pub fn older(&mut self, by: usize, total: usize, rows: usize) {
        self.selected = (self.selected + by).min(total.saturating_sub(1));
        self.keep_in_view(total, rows);
    }

    /// Move `by` rows down, toward the newest match.
    pub fn newer(&mut self, by: usize, total: usize, rows: usize) {
        self.selected = self.selected.saturating_sub(by);
        self.keep_in_view(total, rows);
    }

    /// Bring the selection and the scroll back in range after the matches changed.
    pub fn keep_in_view(&mut self, total: usize, rows: usize) {
        let rows = rows.max(1);
        self.selected = self.selected.min(total.saturating_sub(1));
        if self.selected < self.hidden_below {
            self.hidden_below = self.selected;
        } else if self.selected >= self.hidden_below + rows {
            self.hidden_below = self.selected + 1 - rows;
        }
        self.hidden_below = self.hidden_below.min(total.saturating_sub(rows));
    }

    /// The visible matches as indices into the oldest-first list, top to bottom.
    pub fn visible(&self, total: usize, rows: usize) -> Range<usize> {
        let end = total.saturating_sub(self.hidden_below);
        end.saturating_sub(rows.min(total))..end
    }

    /// The selected match as an index into the oldest-first list.
    pub fn index(&self, total: usize) -> Option<usize> {
        total.checked_sub(self.selected + 1)
    }
}

/// `entry` as one JSON line, newline included.
pub fn entry_line(entry: &HistoryEntry) -> Option<String> {
    let mut line = serde_json::to_string(entry).ok()?;
    line.push('\n');
    Some(line)
}

/// Options creating the history file (or its compacted copy) readable by the owner only:
/// command lines may hold secrets.
fn private_file() -> OpenOptions {
    let mut options = OpenOptions::new();
    options.create(true);
    #[cfg(unix)]
    std::os::unix::fs::OpenOptionsExt::mode(&mut options, 0o600);
    options
}

/// Rewrite the history file at `path` with `text`, unless it is no longer `read_len` bytes
/// long: another instance appended a line since it was read, which the rewrite would lose.
fn compact(path: &Path, text: &str, read_len: u64) -> io::Result<()> {
    let temp = path.with_extension(format!("jsonl.{}.tmp", std::process::id()));
    // A leftover from a crash keeps its mode when reopened.
    let _ = fs::remove_file(&temp);
    private_file()
        .write(true)
        .truncate(true)
        .open(&temp)?
        .write_all(text.as_bytes())?;
    if fs::metadata(path)?.len() != read_len {
        return fs::remove_file(&temp);
    }
    fs::rename(&temp, path)
}

/// Append `entry` to the history file at `path`, creating it and its folder.
pub fn append_entry(path: &Path, entry: &HistoryEntry) -> io::Result<()> {
    let Some(line) = entry_line(entry) else {
        return Ok(());
    };
    if let Some(dir) = path.parent() {
        fs::create_dir_all(dir)?;
    }
    let mut file = private_file().read(true).append(true).open(path)?;
    // After a crash in the middle of a write the last line has no end; start a new one, so
    // this line is not glued to it.
    let text = if ends_unterminated(&mut file)? {
        format!("\n{line}")
    } else {
        line
    };
    // One write per line, so lines appended by two instances don't interleave.
    file.write_all(text.as_bytes())
}

fn ends_unterminated(file: &mut File) -> io::Result<bool> {
    if file.metadata()?.len() == 0 {
        return Ok(false);
    }
    file.seek(SeekFrom::End(-1))?;
    let mut last = [0u8];
    file.read_exact(&mut last)?;
    Ok(last[0] != b'\n')
}

#[cfg(test)]
mod tests {
    use super::*;

    fn entry(command: &str, time: u64) -> HistoryEntry {
        HistoryEntry::new(command, Some("/tmp".into()), time)
    }

    fn commands(history: &CommandHistory) -> Vec<&str> {
        history
            .entries()
            .iter()
            .map(|entry| entry.command.as_str())
            .collect()
    }

    #[test]
    fn latest_use_wins() {
        let mut history = CommandHistory::new(10);
        history.add(entry("ls", 1));
        history.add(entry("cd src", 2));
        history.add(HistoryEntry::new("ls", Some("/home".into()), 3));
        assert_eq!(commands(&history), ["cd src", "ls"]);
        assert_eq!(history.entries()[1].dir, Some("/home".into()));
        assert_eq!(history.entries()[1].time, 3);
    }

    #[test]
    fn oldest_entries_beyond_the_cap_are_dropped() {
        let mut history = CommandHistory::new(3);
        for (time, command) in ["a", "b", "c", "d", "e"].into_iter().enumerate() {
            history.add(entry(command, time as u64));
        }
        assert_eq!(commands(&history), ["c", "d", "e"]);
    }

    #[test]
    fn unrecordable_commands_are_refused() {
        let mut history = CommandHistory::new(10);
        for command in ["", "   ", " secret", "a\nb", "echo \x1b[31m", "tab\there"] {
            assert!(!history.add(entry(command, 0)), "{command:?}");
        }
        assert!(history.add(entry("echo ok ", 0)));
        assert!(!history.is_empty() && history.len() == 1);
    }

    #[test]
    fn suggestion_is_the_rest_of_the_newest_prefix_match() {
        let mut history = CommandHistory::new(10);
        history.add(entry("clear", 1));
        history.add(entry("cargo test", 2));
        history.add(entry("cargo build", 3));
        assert_eq!(history.suggestion("c"), Some("argo build"));
        assert_eq!(history.suggestion("cl"), Some("ear"));
        assert_eq!(history.suggestion("cargo t"), Some("est"));
        assert_eq!(history.suggestion("clear"), None);
        assert_eq!(history.suggestion("Cl"), None);
        assert_eq!(history.suggestion(""), None);
        assert_eq!(history.suggestion("x"), None);
    }

    #[test]
    fn matching_is_a_case_insensitive_substring_oldest_first() {
        let mut history = CommandHistory::new(10);
        history.add(entry("cd vi", 1));
        history.add(entry("ls", 2));
        history.add(entry("cd VI_opl", 3));
        history.add(entry("cd vi_afp/", 4));
        let found: Vec<&str> = history
            .matching(" vi ")
            .iter()
            .map(|entry| entry.command.as_str())
            .collect();
        assert_eq!(found, ["cd vi", "cd VI_opl", "cd vi_afp/"]);
        assert_eq!(history.matching("").len(), 4);
        assert!(history.matching("zzz").is_empty());
        history.add(entry("echo \u{c9}t\u{c9}", 5));
        assert_eq!(history.matching("\u{e9}t\u{e9}").len(), 1);
        assert_eq!(history.matching("O T").len(), 0);
        assert_eq!(history.matching("ECHO").len(), 1);
    }

    #[test]
    fn jsonl_round_trip_dedups_and_caps() {
        let text = concat!(
            "{\"cmd\":\"ls\",\"dir\":\"/a\",\"time\":1}\n",
            "not json\n",
            "{\"cmd\":\"pwd\",\"time\":2}\n",
            "\n",
            "{\"cmd\":\"ls\",\"dir\":\"/b\",\"time\":3}\n",
            "{\"cmd\":\"a\\nb\",\"time\":4}\n",
            "{\"cmd\":\"make\",\"time\":5}\n",
        );
        let (history, lines) = CommandHistory::from_jsonl(text, 10);
        assert_eq!(lines, 6);
        assert_eq!(commands(&history), ["pwd", "ls", "make"]);
        assert_eq!(history.entries()[1].dir, Some("/b".into()));
        let (again, _) = CommandHistory::from_jsonl(&history.to_jsonl(), 10);
        assert_eq!(again, history);
        let (capped, _) = CommandHistory::from_jsonl(text, 2);
        assert_eq!(commands(&capped), ["ls", "make"]);
    }

    #[test]
    fn selection_scrolls_to_stay_in_view() {
        let mut selection = HistorySelection::default();
        assert_eq!(selection.visible(25, 10), 15..25);
        assert_eq!(selection.index(25), Some(24));
        for _ in 0..9 {
            selection.older(1, 25, 10);
        }
        assert_eq!(selection.visible(25, 10), 15..25);
        selection.older(1, 25, 10);
        assert_eq!((selection.selected, selection.hidden_below), (10, 1));
        assert_eq!(selection.visible(25, 10), 14..24);
        assert_eq!(selection.index(25), Some(14));
        selection.older(100, 25, 10);
        assert_eq!(selection.visible(25, 10), 0..10);
        assert_eq!(selection.index(25), Some(0));
        selection.newer(10, 25, 10);
        assert_eq!(selection.visible(25, 10), 1..11);
        assert_eq!(selection.index(25), Some(10));
        selection.newer(100, 25, 10);
        assert_eq!(selection, HistorySelection::default());
    }

    #[test]
    fn selection_follows_fewer_matches() {
        let mut selection = HistorySelection {
            selected: 20,
            hidden_below: 15,
        };
        selection.keep_in_view(3, 10);
        assert_eq!(selection.visible(3, 10), 0..3);
        assert_eq!(selection.index(3), Some(0));
        selection.keep_in_view(0, 10);
        assert_eq!(selection.visible(0, 10), 0..0);
        assert_eq!(selection.index(0), None);
    }

    #[test]
    fn compaction_once_lines_pass_half_again_the_cap() {
        let history = CommandHistory::new(10);
        assert!(!history.needs_compaction(15));
        assert!(history.needs_compaction(16));
    }

    #[cfg(unix)]
    #[test]
    fn non_utf8_folders_are_dropped() {
        use std::ffi::OsStr;
        use std::os::unix::ffi::OsStrExt;

        let bad = PathBuf::from(OsStr::from_bytes(b"/tmp/\xff"));
        let entry = HistoryEntry::new("ls", Some(bad), 1);
        assert_eq!(entry.dir, None);
        assert!(entry_line(&entry).is_some());
    }

    #[test]
    fn appended_lines_load_back_and_compact() {
        let dir = std::env::temp_dir().join(format!("agentws-history-{}", std::process::id()));
        let path = dir.join("history.jsonl");
        let _ = fs::remove_dir_all(&dir);
        for time in 0..8 {
            append_entry(&path, &entry(&format!("cmd {}", time % 3), time)).unwrap();
        }
        let history = CommandHistory::load(&path, 2);
        assert_eq!(commands(&history), ["cmd 0", "cmd 1"]);
        // 8 lines > 2 + 1: the file was rewritten with just the kept entries.
        let text = fs::read_to_string(&path).unwrap();
        assert_eq!(text.lines().count(), 2);
        assert_eq!(CommandHistory::load(&dir.join("missing"), 5).len(), 0);
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let mode = fs::metadata(&path).unwrap().permissions().mode();
            assert_eq!(mode & 0o777, 0o600, "compacted file is owner-only");
        }
        fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn damaged_files_keep_their_other_lines() {
        let dir = std::env::temp_dir().join(format!("agentws-damaged-{}", std::process::id()));
        let path = dir.join("history.jsonl");
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        // A non-UTF-8 line, and a last line cut off by a crash.
        fs::write(
            &path,
            b"{\"cmd\":\"ls\"}\n{\"cmd\":\"\xff\"}\n{\"cmd\":\"pw",
        )
        .unwrap();
        append_entry(&path, &entry("make", 1)).unwrap();
        let history = CommandHistory::load(&path, 10);
        assert_eq!(commands(&history), ["ls", "make"]);
        fs::remove_dir_all(&dir).unwrap();
    }
}
