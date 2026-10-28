//! The command history of the shell tabs: commands run with shell integration, with their
//! shell, folder and time, kept per shell in `history.jsonl` in the config folder
//! (`workspace_model::CommandHistory`). Each instance loads the file at start and appends each
//! command it records.

use std::io;
use std::path::{Path, PathBuf};
use std::sync::mpsc::{self, SendError, Sender};
use std::thread::{self, JoinHandle};
use std::time::{SystemTime, UNIX_EPOCH};

use gpui_kit::{App, Global};
use term_core::ExecutedCommand;
use workspace_model::{
    CommandHistory, HistoryEntry, HistoryScope, append_entry, clear_history_file, remove_command,
};

use crate::window_bounds::config_file;

pub struct History {
    commands: CommandHistory,
    /// `None` keeps the history in memory only.
    writer: Option<HistoryWriter>,
    /// The command last forgotten, and in which shells.
    forgotten: Option<(String, HistoryScope)>,
    /// Counts "Clear saved session commands" requests; each tab compares it with the count it
    /// has seen (see `TerminalView::new`).
    saved_cleared: u32,
    /// Counts `forget` calls, so an observer can tell a new one from `forgotten` left over.
    forgets: u32,
}

impl Global for History {}

impl History {
    fn new(commands: CommandHistory, path: Option<PathBuf>) -> Self {
        Self {
            commands,
            writer: path.map(HistoryWriter::new),
            forgotten: None,
            saved_cleared: 0,
            forgets: 0,
        }
    }

    pub fn commands(&self) -> &CommandHistory {
        &self.commands
    }

    pub fn forgotten(&self) -> Option<(&str, &HistoryScope)> {
        self.forgotten
            .as_ref()
            .map(|(command, scope)| (command.as_str(), scope))
    }

    pub fn saved_cleared(&self) -> u32 {
        self.saved_cleared
    }

    pub fn forgets(&self) -> u32 {
        self.forgets
    }
}

/// A change to the history file, in the order it was made.
enum Job {
    Append(HistoryEntry),
    Remove(String, HistoryScope),
    Clear(Sender<io::Result<()>>),
}

/// Writes the history file on its own thread, in order: another instance may hold the file's
/// lock (while compacting it), and the UI must not wait for it.
struct HistoryWriter {
    path: PathBuf,
    /// `None` once finished, or when the thread could not start: jobs then run in place.
    worker: Option<Worker>,
}

struct Worker {
    sender: Sender<Job>,
    thread: JoinHandle<()>,
}

impl HistoryWriter {
    fn new(path: PathBuf) -> Self {
        let (sender, receiver) = mpsc::channel();
        let worker_path = path.clone();
        let worker = thread::Builder::new()
            .name("history-writer".into())
            .spawn(move || {
                for job in receiver {
                    apply(&worker_path, job);
                }
            })
            .inspect_err(|err| eprintln!("agent-workspace: failed to start history writer: {err}"))
            .ok()
            .map(|thread| Worker { sender, thread });
        Self { path, worker }
    }

    fn queue(&self, job: Job) {
        let job = match &self.worker {
            Some(worker) => match worker.sender.send(job) {
                Ok(()) => return,
                Err(SendError(job)) => job,
            },
            None => job,
        };
        apply(&self.path, job);
    }

    /// Empty the file behind the changes queued before, and wait for the result.
    fn clear(&self) -> io::Result<()> {
        let (reply, result) = mpsc::channel();
        self.queue(Job::Clear(reply));
        result
            .recv()
            .unwrap_or_else(|_| Err(io::Error::other("the history writer stopped")))
    }

    /// Write every queued change before returning.
    fn finish(&mut self) {
        if let Some(Worker { sender, thread }) = self.worker.take() {
            drop(sender);
            if thread.join().is_err() {
                eprintln!("agent-workspace: history writer panicked; commands may be unsaved");
            }
        }
    }
}

impl Drop for HistoryWriter {
    fn drop(&mut self) {
        self.finish();
    }
}

fn apply(path: &Path, job: Job) {
    match job {
        Job::Append(entry) => {
            if let Err(err) = append_entry(path, &entry) {
                eprintln!(
                    "agent-workspace: failed to save command history to {}: {err}",
                    path.display()
                );
            }
        }
        Job::Remove(command, scope) => {
            if let Err(err) = remove_command(path, &command, &scope) {
                eprintln!(
                    "agent-workspace: failed to remove a command from {}: {err}",
                    path.display()
                );
            }
        }
        // The caller waits for the answer; it may be gone only if it panicked.
        Job::Clear(reply) => {
            let _ = reply.send(clear_history_file(path));
        }
    }
}

pub fn init(cx: &mut App) {
    let path = config_file("history.jsonl");
    let commands = path
        .as_deref()
        .map_or_else(CommandHistory::default, |path| {
            CommandHistory::load(path, CommandHistory::DEFAULT_CAP)
        });
    cx.set_global(History::new(commands, path));
}

#[cfg(test)]
pub fn init_in_memory(cx: &mut App) {
    cx.set_global(History::new(CommandHistory::default(), None));
}

/// Write the changes still queued for the history file; call before quitting.
pub fn finish(cx: &mut App) {
    if cx.has_global::<History>()
        && let Some(writer) = &mut cx.global_mut::<History>().writer
    {
        writer.finish();
    }
}

/// Record a command the shell with launcher id `shell` ran. The file is written on the writer's
/// thread; a failed save is only reported: the command stays in this run's history.
pub fn record(command: ExecutedCommand, shell: &str, cx: &mut App) {
    let time = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |since| since.as_secs());
    let entry = HistoryEntry::new(command.command, command.folder, time).in_shell(shell);
    let history = cx.global_mut::<History>();
    history.forgotten = None;
    if !history.commands.add(entry.clone()) {
        return;
    }
    if let Some(writer) = &history.writer {
        writer.queue(Job::Append(entry));
    }
}

/// Forget `command` in the shells `scope` covers, in this run and in the history file (written
/// on the writer's thread). A failed save is only reported: the command is gone from this run's
/// history.
pub fn forget(command: &str, scope: &HistoryScope, cx: &mut App) {
    let history = cx.global_mut::<History>();
    history.forgotten = Some((command.to_owned(), scope.clone()));
    history.forgets += 1;
    history.commands.remove(command, scope);
    if let Some(writer) = &history.writer {
        writer.queue(Job::Remove(command.to_owned(), scope.clone()));
    }
}

/// "Clear history": forget every command, in this run and in the history file. When the file
/// can't be cleared the commands are gone for this run only, and the error says so.
pub fn clear(cx: &mut App) -> Result<(), String> {
    let history = cx.global_mut::<History>();
    history.forgotten = None;
    history.commands.clear();
    let Some(writer) = &history.writer else {
        return Ok(());
    };
    writer.clear().map_err(|err| {
        format!(
            "Couldn't clear {}: {err}. The commands return on the next start.",
            writer.path.display()
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
            let commands = CommandHistory::load(&path, CommandHistory::DEFAULT_CAP);
            cx.set_global(History::new(commands, Some(path)));
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
    fn record_does_not_wait_for_another_instances_history_lock(cx: &mut gpui_kit::TestAppContext) {
        use std::time::{Duration, Instant};

        let folder = temp_folder("record-locked");
        let path = folder.join("history.jsonl");
        set_history(cx, path.clone(), "");
        // Another instance compacting its history holds the lock for a while.
        let lock = fs::File::create(path.with_extension("lock")).unwrap();
        lock.lock().unwrap();
        let holder = std::thread::spawn(move || {
            std::thread::sleep(Duration::from_secs(2));
            drop(lock);
        });

        let started = Instant::now();
        cx.update(|cx| {
            record(
                ExecutedCommand {
                    command: "cargo test".into(),
                    folder: None,
                },
                "bash",
                cx,
            )
        });
        assert!(
            started.elapsed() < Duration::from_secs(1),
            "record blocked the UI thread for {:?}",
            started.elapsed()
        );
        holder.join().unwrap();
        cx.update(finish);
        assert!(fs::read_to_string(&path).unwrap().contains("cargo test"));
        let _ = fs::remove_dir_all(&folder);
    }

    #[gpui::test]
    fn clear_reports_a_file_that_cannot_be_cleared(cx: &mut gpui_kit::TestAppContext) {
        let folder = temp_folder("clear-error");
        let blocker = folder.join("blocker");
        fs::write(&blocker, "").unwrap();
        // The folder of the history file is a regular file: locking it fails.
        cx.update(|cx| {
            let path = Some(blocker.join("history.jsonl"));
            cx.set_global(History::new(CommandHistory::default(), path));
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
