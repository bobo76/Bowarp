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
    pub const DEFAULT_CAP: usize = 2_000;

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

    /// Forget `command`. Returns whether it was in the history.
    pub fn remove(&mut self, command: &str) -> bool {
        let before = self.entries.len();
        self.entries.retain(|entry| entry.command != command);
        self.entries.len() != before
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
        let _lock = match lock_history(path) {
            Ok(lock) => lock,
            Err(err) => {
                eprintln!("agent-workspace: failed to lock {}: {err}", path.display());
                return Self::new(cap);
            }
        };
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
            && let Err(err) = compact_locked(path, history.to_jsonl().as_bytes())
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

/// Lock a stable sibling, not the history inode that compaction replaces. Keep it open for the
/// entire read/modify/write operation; an appender must acquire it before opening the history.
fn lock_history(path: &Path) -> io::Result<File> {
    if let Some(folder) = path
        .parent()
        .filter(|folder| !folder.as_os_str().is_empty())
    {
        fs::create_dir_all(folder)?;
    }
    let file = private_file()
        .read(true)
        .write(true)
        .open(path.with_extension("lock"))?;
    file.lock()?;
    Ok(file)
}

/// Replace the history while the caller holds its sibling lock.
fn compact_locked(path: &Path, text: &[u8]) -> io::Result<()> {
    let temp = path.with_extension(format!("jsonl.{}.tmp", std::process::id()));
    // A leftover from a crash keeps its mode when reopened.
    let _ = fs::remove_file(&temp);
    private_file()
        .write(true)
        .truncate(true)
        .open(&temp)?
        .write_all(text)?;
    fs::rename(&temp, path)?;
    Ok(())
}

/// Drop every line of the history file at `path` that holds `command`, keeping the other lines
/// as they are (damaged ones too). A missing file is fine.
pub fn remove_command(path: &Path, command: &str) -> io::Result<()> {
    let _lock = lock_history(path)?;
    let bytes = match fs::read(path) {
        Ok(bytes) => bytes,
        Err(err) if err.kind() == io::ErrorKind::NotFound => return Ok(()),
        Err(err) => return Err(err),
    };
    let kept: Vec<&[u8]> = bytes
        .split(|&byte| byte == b'\n')
        .filter(|line| !line.is_empty() && !holds_command(line, command))
        .collect();
    let text = kept.iter().fold(Vec::new(), |mut text, line| {
        text.extend_from_slice(line);
        text.push(b'\n');
        text
    });
    compact_locked(path, &text)
}

fn holds_command(line: &[u8], command: &str) -> bool {
    serde_json::from_slice::<HistoryEntry>(line).is_ok_and(|entry| entry.command == command)
}

/// Append `entry` to the history file at `path`, creating it and its folder.
pub fn append_entry(path: &Path, entry: &HistoryEntry) -> io::Result<()> {
    let Some(line) = entry_line(entry) else {
        return Ok(());
    };
    let _lock = lock_history(path)?;
    let mut file = private_file().read(true).append(true).open(path)?;
    // After a crash in the middle of a write the last line has no end; start a new one, so
    // this line is not glued to it.
    let text = if ends_unterminated(&mut file)? {
        format!("\n{line}")
    } else {
        line
    };
    // The lock also keeps partial writes and repairs of unterminated lines together.
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
mod tests;
