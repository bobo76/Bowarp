//! The command history shared by every shell tab: commands run with shell integration, with
//! their folder and time, kept in `history.jsonl` in the config folder
//! (`workspace_model::CommandHistory`). Each instance loads the file at start and appends each
//! command it records.

use std::path::PathBuf;
use std::time::{SystemTime, UNIX_EPOCH};

use gpui_kit::{App, Global};
use term_core::ExecutedCommand;
use workspace_model::{
    CommandHistory, HistoryEntry, append_entry, clear_history_file, remove_command,
};

use crate::window_bounds::config_file;

pub struct History {
    commands: CommandHistory,
    path: Option<PathBuf>,
    forgotten: Option<String>,
    /// Counts "Clear saved session commands" requests; each tab compares it with the count it
    /// has seen (see `TerminalView::new`).
    saved_cleared: u32,
    /// Counts `forget` calls, so an observer can tell a new one from `forgotten` left over.
    forgets: u32,
}

impl Global for History {}

impl History {
    pub fn commands(&self) -> &CommandHistory {
        &self.commands
    }

    pub fn forgotten(&self) -> Option<&str> {
        self.forgotten.as_deref()
    }

    pub fn saved_cleared(&self) -> u32 {
        self.saved_cleared
    }

    pub fn forgets(&self) -> u32 {
        self.forgets
    }
}

pub fn init(cx: &mut App) {
    let path = config_file("history.jsonl");
    let commands = path
        .as_deref()
        .map_or_else(CommandHistory::default, |path| {
            CommandHistory::load(path, CommandHistory::DEFAULT_CAP)
        });
    cx.set_global(History {
        commands,
        path,
        forgotten: None,
        saved_cleared: 0,
        forgets: 0,
    });
}

#[cfg(test)]
pub fn init_in_memory(cx: &mut App) {
    cx.set_global(History {
        commands: CommandHistory::default(),
        path: None,
        forgotten: None,
        saved_cleared: 0,
        forgets: 0,
    });
}

/// Record a command a shell ran. A failed save is only reported: the command stays in this
/// run's history.
pub fn record(command: ExecutedCommand, cx: &mut App) {
    let time = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |since| since.as_secs());
    let entry = HistoryEntry::new(command.command, command.folder, time);
    let history = cx.global_mut::<History>();
    history.forgotten = None;
    if !history.commands.add(entry.clone()) {
        return;
    }
    let Some(path) = &history.path else {
        return;
    };
    if let Err(err) = append_entry(path, &entry) {
        eprintln!(
            "agent-workspace: failed to save command history to {}: {err}",
            path.display()
        );
    }
}

/// Forget `command`, in this run and in the history file. A failed save is only reported: the
/// command is gone from this run's history.
pub fn forget(command: &str, cx: &mut App) {
    let history = cx.global_mut::<History>();
    history.forgotten = Some(command.to_owned());
    history.forgets += 1;
    history.commands.remove(command);
    let Some(path) = &history.path else {
        return;
    };
    if let Err(err) = remove_command(path, command) {
        eprintln!(
            "agent-workspace: failed to remove a command from {}: {err}",
            path.display()
        );
    }
}

/// "Clear history": forget every command, in this run and in the history file. When the file
/// can't be cleared the commands are gone for this run only, and the error says so.
pub fn clear(cx: &mut App) -> Result<(), String> {
    let history = cx.global_mut::<History>();
    history.forgotten = None;
    history.commands.clear();
    let Some(path) = &history.path else {
        return Ok(());
    };
    clear_history_file(path).map_err(|err| {
        format!(
            "Couldn't clear {}: {err}. The commands return on the next start.",
            path.display()
        )
    })
}

/// "Clear saved session commands": every tab drops its saved last command and the tab
/// snapshot is saved again through the ordered writer. Nothing is rerun.
pub fn clear_saved_session_commands(cx: &mut App) -> Result<(), String> {
    cx.global_mut::<History>().saved_cleared += 1;
    Ok(())
}

#[cfg(test)]
mod tests {
    use std::fs;
    use std::path::PathBuf;

    use super::*;

    fn temp_folder(name: &str) -> PathBuf {
        let folder = std::env::temp_dir().join(format!("aw-history-{name}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&folder);
        fs::create_dir_all(&folder).unwrap();
        folder
    }

    fn set_history(cx: &mut gpui_kit::TestAppContext, path: PathBuf, text: &str) {
        fs::write(&path, text).unwrap();
        cx.update(|cx| {
            cx.set_global(History {
                commands: CommandHistory::load(&path, CommandHistory::DEFAULT_CAP),
                path: Some(path),
                forgotten: None,
                saved_cleared: 0,
                forgets: 0,
            });
        });
    }

    #[gpui::test]
    fn clear_empties_memory_and_file(cx: &mut gpui_kit::TestAppContext) {
        let folder = temp_folder("clear");
        let path = folder.join("history.jsonl");
        set_history(cx, path.clone(), "{\"cmd\":\"ls\",\"time\":1}\n");
        assert_eq!(cx.update(|cx| cx.global::<History>().commands().len()), 1);
        assert_eq!(cx.update(clear), Ok(()));
        assert_eq!(cx.update(|cx| cx.global::<History>().commands().len()), 0);
        assert_eq!(fs::read_to_string(&path).unwrap(), "");
        let _ = fs::remove_dir_all(&folder);
    }

    #[gpui::test]
    fn clear_reports_a_file_that_cannot_be_cleared(cx: &mut gpui_kit::TestAppContext) {
        let folder = temp_folder("clear-error");
        let blocker = folder.join("blocker");
        fs::write(&blocker, "").unwrap();
        // The folder of the history file is a regular file: locking it fails.
        cx.update(|cx| {
            cx.set_global(History {
                commands: CommandHistory::default(),
                path: Some(blocker.join("history.jsonl")),
                forgotten: None,
                saved_cleared: 0,
                forgets: 0,
            });
        });
        let error = cx.update(clear).unwrap_err();
        assert!(error.contains("Couldn't clear"), "{error}");
        let _ = fs::remove_dir_all(&folder);
    }

    #[gpui::test]
    fn clear_saved_session_commands_counts_requests(cx: &mut gpui_kit::TestAppContext) {
        cx.update(init_in_memory);
        assert_eq!(cx.update(clear_saved_session_commands), Ok(()));
        assert_eq!(cx.update(|cx| cx.global::<History>().saved_cleared()), 1);
    }
}
