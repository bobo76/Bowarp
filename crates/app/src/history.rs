//! The command history shared by every shell tab: commands run with shell integration, with
//! their folder and time, kept in `history.jsonl` in the config folder
//! (`workspace_model::CommandHistory`). Each instance loads the file at start and appends each
//! command it records.

use std::path::PathBuf;
use std::time::{SystemTime, UNIX_EPOCH};

use gpui_kit::{App, Global};
use term_core::ExecutedCommand;
use workspace_model::{CommandHistory, HistoryEntry, append_entry, remove_command};

use crate::window_bounds::config_file;

pub struct History {
    commands: CommandHistory,
    path: Option<PathBuf>,
    forgotten: Option<String>,
}

impl Global for History {}

impl History {
    pub fn commands(&self) -> &CommandHistory {
        &self.commands
    }

    pub fn forgotten(&self) -> Option<&str> {
        self.forgotten.as_deref()
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
    });
}

#[cfg(test)]
pub fn init_in_memory(cx: &mut App) {
    cx.set_global(History {
        commands: CommandHistory::default(),
        path: None,
        forgotten: None,
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
