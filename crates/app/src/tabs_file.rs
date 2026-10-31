//! Reopening the tabs: what each tab runs, its folder, name, color and place in the sidebar are
//! saved to `tabs.json` when the window closes or a last command changes, and reopened at the next
//! start (`workspace_model::TabsState`). Sessions start fresh; a Claude Code tab starts a new
//! conversation in its folder.

use std::cell::RefCell;
use std::io;
use std::path::{Path, PathBuf};
use std::sync::mpsc::{self, Sender};
use std::thread::{self, JoinHandle};

use term_core::{LaunchSpec, SessionKind};
use workspace_model::{SavedKind, SavedTab, TabMeta, TabsState, WorkspaceFeatures};

use crate::window_bounds::config_file;

fn state_path() -> Option<PathBuf> {
    config_file("tabs.json")
}

/// The tabs saved at the last close; `None` when nothing usable was saved, or when
/// "Restore previous tabs" is off (the file is then left unread and unchanged).
pub fn load(features: WorkspaceFeatures) -> Option<TabsState> {
    load_from(&state_path()?, features)
}

fn load_from(path: &Path, features: WorkspaceFeatures) -> Option<TabsState> {
    if !features.loads_tabs() {
        return None;
    }
    TabsState::load(path)
}

/// One FIFO writer for the owning instance. Snapshots contain no GPUI entities; serialization
/// and private atomic replacement happen only on the worker.
pub struct TabsWriter {
    worker: RefCell<Option<Worker>>,
}

/// What the worker does, in queue order.
enum Job {
    Save(TabsState),
    /// Remove saved last commands from the retained `tabs.json` ("Restore previous tabs" off:
    /// the file is otherwise left alone). `None` removes all, `Some` the tabs holding it.
    Scrub(Option<String>),
}

struct Worker {
    sender: Sender<Job>,
    thread: JoinHandle<()>,
}

impl TabsWriter {
    pub fn new(owns_tabs: bool) -> Self {
        if owns_tabs && let Some(path) = state_path() {
            match Self::start(path, |message| eprintln!("{message}")) {
                Ok(writer) => return writer,
                Err(err) => eprintln!("agent-workspace: failed to start tabs writer: {err}"),
            }
        }
        Self {
            worker: RefCell::new(None),
        }
    }

    fn start(path: PathBuf, report: impl Fn(String) + Send + 'static) -> io::Result<Self> {
        let failure = move |action: &str, path: &Path, err: io::Error| {
            report(format!(
                "agent-workspace: failed to {action} {}: {err}",
                path.display()
            ));
        };
        Self::spawn(move |job| match job {
            Job::Save(state) => {
                if let Err(err) = state.save(&path) {
                    failure("save tabs to", &path, err);
                }
            }
            Job::Scrub(only) => {
                if let Err(err) = TabsState::scrub_file(&path, only.as_deref()) {
                    failure("remove saved commands from", &path, err);
                }
            }
        })
    }

    fn spawn(work: impl Fn(Job) + Send + 'static) -> io::Result<Self> {
        let (sender, receiver) = mpsc::channel();
        let thread = thread::Builder::new()
            .name("tabs-writer".into())
            .spawn(move || {
                for job in receiver {
                    work(job);
                }
            })?;
        Ok(Self {
            worker: RefCell::new(Some(Worker { sender, thread })),
        })
    }

    #[cfg(test)]
    pub(crate) fn for_test(save: impl Fn(TabsState) + Send + 'static) -> Self {
        Self::for_test_scrubbing(save, |_| {})
    }

    #[cfg(test)]
    pub(crate) fn for_test_scrubbing(
        save: impl Fn(TabsState) + Send + 'static,
        scrub: impl Fn(Option<String>) + Send + 'static,
    ) -> Self {
        Self::spawn(move |job| match job {
            Job::Save(state) => save(state),
            Job::Scrub(only) => scrub(only),
        })
        .unwrap()
    }

    pub fn is_active(&self) -> bool {
        self.worker.borrow().is_some()
    }

    /// Queue immediately, without doing storage work or waiting for earlier snapshots.
    pub fn save(&self, state: TabsState) {
        self.queue(Job::Save(state));
    }

    /// Queue the removal of saved last commands from the retained file, behind earlier saves.
    pub fn scrub_last_commands(&self, only: Option<String>) {
        self.queue(Job::Scrub(only));
    }

    fn queue(&self, job: Job) {
        if let Some(worker) = self.worker.borrow().as_ref()
            && worker.sender.send(job).is_err()
        {
            eprintln!("agent-workspace: failed to queue tabs: writer stopped");
        }
    }

    /// Stop accepting snapshots and drain every queued write before returning. Call before
    /// releasing the instance lock, even if GPUI still retains the workspace entity.
    pub fn finish(&self) {
        if let Some(Worker { sender, thread }) = self.worker.borrow_mut().take() {
            drop(sender);
            if thread.join().is_err() {
                eprintln!("agent-workspace: tabs writer panicked; tabs may not have been saved");
            }
        }
    }
}

impl Drop for TabsWriter {
    fn drop(&mut self) {
        self.finish();
    }
}

/// The saved form of a tab started from `spec`, now in `folder`. JSON holds only UTF-8 paths:
/// a folder that isn't one is replaced by the launch folder, else left out (the tab then
/// reopens in the app's folder).
pub fn saved_tab(spec: &LaunchSpec, folder: Option<PathBuf>, meta: TabMeta) -> SavedTab {
    let utf8 = |folder: &PathBuf| folder.to_str().is_some();
    SavedTab {
        kind: saved_kind(spec.kind),
        program: spec.program.clone(),
        args: spec.args.clone(),
        folder: folder
            .filter(utf8)
            .or_else(|| spec.cwd.clone().filter(utf8)),
        last_command: None,
        meta,
    }
}

/// The launch reopening a saved tab, in its folder or the nearest parent that still exists.
pub fn launch_spec(tab: &SavedTab) -> LaunchSpec {
    LaunchSpec::new(
        session_kind(tab.kind),
        tab.program.clone(),
        tab.args.clone(),
        tab.launch_folder(),
    )
}

fn saved_kind(kind: SessionKind) -> SavedKind {
    kind.agent().map_or(SavedKind::Shell, SavedKind::Agent)
}

fn session_kind(kind: SavedKind) -> SessionKind {
    match kind {
        SavedKind::Shell => SessionKind::Shell,
        SavedKind::Agent(agent) => SessionKind::Agent(agent),
    }
}

#[cfg(test)]
mod tests;
