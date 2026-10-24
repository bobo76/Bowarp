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
    match kind {
        SessionKind::Shell => SavedKind::Shell,
        SessionKind::ClaudeCode => SavedKind::ClaudeCode,
        SessionKind::CopilotCli => SavedKind::CopilotCli,
    }
}

fn session_kind(kind: SavedKind) -> SessionKind {
    match kind {
        SavedKind::Shell => SessionKind::Shell,
        SavedKind::ClaudeCode => SessionKind::ClaudeCode,
        SavedKind::CopilotCli => SessionKind::CopilotCli,
    }
}

#[cfg(test)]
mod tests {
    use std::fs;
    use std::time::Duration;

    use workspace_model::TabColor;

    use super::*;

    const WAIT: Duration = Duration::from_secs(10);

    struct TestDir(PathBuf);

    impl TestDir {
        fn new(name: &str) -> Self {
            let path = PathBuf::from(format!(
                ".tabs-writer-{name}-{}-{}",
                std::process::id(),
                agent_status::now_micros()
            ));
            fs::create_dir(&path).unwrap();
            Self(path)
        }
    }

    impl Drop for TestDir {
        fn drop(&mut self) {
            fs::remove_dir_all(&self.0).unwrap();
        }
    }

    fn state(command: Option<&str>) -> TabsState {
        let mut tab = saved_tab(&LaunchSpec::shell(None), None, TabMeta::default());
        tab.last_command = command.map(str::to_owned);
        TabsState {
            layout: workspace_model::SavedLayout {
                pinned: vec![tab],
                items: vec![],
            },
            selected: Some(0),
        }
    }

    #[test]
    fn scrub_runs_on_the_worker_after_earlier_saves() {
        let ui_thread = thread::current().id();
        let (done, order) = mpsc::channel();
        let saved = done.clone();
        let writer = TabsWriter::for_test_scrubbing(
            move |state| saved.send(format!("save {:?}", state.selected)).unwrap(),
            move |only| {
                assert_ne!(thread::current().id(), ui_thread);
                done.send(format!("scrub {only:?}")).unwrap();
            },
        );
        writer.save(state(Some("cargo build")));
        writer.scrub_last_commands(Some("cargo build".into()));
        writer.scrub_last_commands(None);
        writer.finish();
        let seen: Vec<_> = order.try_iter().collect();
        assert_eq!(
            seen,
            ["save Some(0)", "scrub Some(\"cargo build\")", "scrub None"]
        );
    }

    #[test]
    fn started_writer_scrubs_the_file_and_reports_unreadable_files() {
        let dir = temp_dir("scrub-writer");
        let path = dir.join("tabs.json");
        state(Some("cargo build")).save(&path).unwrap();
        let (reported, reports) = mpsc::channel();
        let writer = TabsWriter::start(path.clone(), move |message| {
            reported.send(message).unwrap();
        })
        .unwrap();
        writer.scrub_last_commands(None);
        writer.finish();
        assert_eq!(TabsState::load(&path), Some(state(None)));
        assert!(reports.try_recv().is_err());
        std::fs::write(&path, "garbage").unwrap();
        let (reported, reports) = mpsc::channel();
        let writer = TabsWriter::start(path.clone(), move |message| {
            reported.send(message).unwrap();
        })
        .unwrap();
        writer.scrub_last_commands(None);
        writer.finish();
        assert!(
            reports
                .try_recv()
                .unwrap()
                .contains("remove saved commands")
        );
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn writer_queues_without_waiting_and_close_drains_in_order() {
        let ui_thread = thread::current().id();
        let (entered, started) = mpsc::channel();
        let (release, gate) = mpsc::channel();
        let (saved, snapshots) = mpsc::channel();
        let writer = TabsWriter::for_test(move |state| {
            assert_ne!(thread::current().id(), ui_thread);
            if state == self::state(Some("cargo build")) {
                entered.send(()).unwrap();
                gate.recv_timeout(WAIT).unwrap();
            }
            saved.send(state).unwrap();
        });
        let old = state(Some("cargo build"));
        let newer = state(Some("cargo test"));
        let deleted = state(None);
        let mut final_snapshot = deleted.clone();
        final_snapshot.layout.pinned[0].meta.custom_name = Some("latest layout".into());
        writer.save(old.clone());
        started.recv_timeout(WAIT).unwrap();
        writer.save(newer.clone());
        writer.save(deleted.clone());
        writer.save(final_snapshot.clone());
        assert!(matches!(
            snapshots.try_recv(),
            Err(mpsc::TryRecvError::Empty)
        ));

        let (closed, finished) = mpsc::channel();
        let closer = thread::spawn(move || {
            writer.finish();
            assert!(!writer.is_active());
            writer.finish();
            writer.save(self::state(Some("must not reappear after close")));
            closed.send(()).unwrap();
        });
        assert!(matches!(
            finished.recv_timeout(Duration::from_millis(50)),
            Err(mpsc::RecvTimeoutError::Timeout)
        ));
        release.send(()).unwrap();
        finished.recv_timeout(WAIT).unwrap();
        closer.join().unwrap();
        assert_eq!(
            snapshots.into_iter().collect::<Vec<_>>(),
            [old, newer, deleted, final_snapshot]
        );
    }

    #[test]
    fn writer_drop_flushes_deletion_and_cannot_overwrite_the_next_owner() {
        let root = TestDir::new("drop");
        let path = root.0.join("tabs.json");
        let writer = TabsWriter::start(path.clone(), |message| panic!("{message}")).unwrap();
        writer.save(state(Some("cargo build")));
        writer.save(state(None));
        drop(writer);
        assert_eq!(TabsState::load(&path), Some(state(None)));

        let writer = TabsWriter::start(path.clone(), |message| panic!("{message}")).unwrap();
        writer.save(state(Some("old owner")));
        writer.finish();
        let next_owner = state(Some("new owner"));
        next_owner.save(&path).unwrap();
        writer.save(state(Some("must not resurrect")));
        drop(writer);
        assert_eq!(TabsState::load(&path), Some(next_owner));
    }

    #[test]
    fn writer_reports_storage_failure_and_keeps_processing() {
        let root = TestDir::new("failure");
        let path = root.0.join("tabs.json");
        fs::create_dir(&path).unwrap();
        let (reported, errors) = mpsc::channel();
        let writer = TabsWriter::start(path.clone(), move |message| {
            reported.send(message).unwrap();
        })
        .unwrap();
        writer.save(state(Some("cargo build")));
        let error = errors.recv_timeout(WAIT).unwrap();
        assert!(error.contains("failed to save tabs to"));
        assert!(error.contains(&path.display().to_string()));
        fs::remove_dir(&path).unwrap();
        writer.save(state(None));
        writer.finish();
        assert_eq!(TabsState::load(&path), Some(state(None)));
        assert!(errors.try_recv().is_err());
    }

    #[test]
    fn guest_writer_never_starts_or_accepts_snapshots() {
        let writer = TabsWriter::new(false);
        assert!(!writer.is_active());
        writer.save(state(Some("guest command")));
        writer.finish();
        assert!(!writer.is_active());
    }

    #[test]
    fn saved_tab_round_trip_keeps_the_launch() {
        let folder = std::env::temp_dir();
        let meta = TabMeta {
            custom_name: Some("api".into()),
            color: Some(TabColor::Green),
        };
        for spec in [
            LaunchSpec::shell(Some("/".into())),
            LaunchSpec::claude_code(None),
            LaunchSpec::copilot_cli(None),
            LaunchSpec::new(
                SessionKind::Shell,
                Some("wsl.exe".into()),
                vec!["-d".into(), "Ubuntu".into()],
                None,
            ),
        ] {
            let saved = saved_tab(&spec, Some(folder.clone()), meta.clone());
            assert_eq!(saved.meta, meta);
            let reopened = launch_spec(&saved);
            assert_eq!(reopened.kind, spec.kind);
            assert_eq!(reopened.program, spec.program);
            assert_eq!(reopened.args, spec.args);
            assert_eq!(reopened.cwd.as_ref(), Some(&folder));
        }
    }

    #[test]
    fn saved_tab_falls_back_to_the_launch_folder() {
        let spec = LaunchSpec::shell(Some("/".into()));
        let saved = saved_tab(&spec, None, TabMeta::default());
        assert_eq!(saved.folder, Some("/".into()));
    }

    #[test]
    fn restored_agent_keeps_original_arguments_not_new_launcher_settings() {
        use crate::launcher::Launcher;
        use workspace_model::{LauncherIcon, Settings};
        let launcher = Launcher {
            id: "claude".into(),
            label: "Claude Code".into(),
            kind: SessionKind::ClaudeCode,
            program: Some("claude".into()),
            args: vec![],
            icon: LauncherIcon::Claude,
        };
        let mut settings = Settings {
            claude_args: "--model 'opus' --settings 'user settings.json'".into(),
            ..Settings::default()
        };
        let spec = launcher.configured_spec(None, &settings).unwrap().unwrap();
        let saved = saved_tab(&spec, None, TabMeta::default());
        let state = TabsState {
            layout: workspace_model::SavedLayout {
                pinned: vec![saved],
                items: vec![],
            },
            selected: Some(0),
        };
        let state = TabsState::from_json(&state.to_json()).unwrap();
        let saved = &state.layout.pinned[0];
        settings.claude_args = "--model sonnet".into();
        assert_eq!(
            launch_spec(saved).args,
            ["--model", "opus", "--settings", "user settings.json"]
        );
        assert_eq!(
            launcher
                .configured_spec(None, &settings)
                .unwrap()
                .unwrap()
                .args,
            ["--model", "sonnet"]
        );
    }

    fn temp_dir(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("agentws-{name}-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn restore_off_neither_reads_nor_changes_the_tabs_file() {
        use workspace_model::{FeatureSwitch, SavedLayout, Settings};

        let dir = temp_dir("restore-off");
        let path = dir.join("tabs.json");
        let state = TabsState {
            layout: SavedLayout {
                pinned: vec![saved_tab(
                    &LaunchSpec::shell(None),
                    None,
                    TabMeta::default(),
                )],
                items: Vec::new(),
            },
            selected: Some(0),
        };
        state.save(&path).unwrap();
        let before = std::fs::read(&path).unwrap();
        let with = |restore: bool| {
            WorkspaceFeatures::from_settings(&Settings {
                restore_tabs: FeatureSwitch::new(restore),
                ..Settings::default()
            })
        };
        assert_eq!(load_from(&path, with(false)), None);
        assert_eq!(std::fs::read(&path).unwrap(), before);
        // Turned on again: the retained snapshot is restored.
        assert_eq!(load_from(&path, with(true)), Some(state));
        assert_eq!(std::fs::read(&path).unwrap(), before);
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[cfg(unix)]
    #[test]
    fn non_utf8_folders_are_not_saved() {
        use std::ffi::OsStr;
        use std::os::unix::ffi::OsStrExt;

        use workspace_model::SavedLayout;

        let bad = PathBuf::from(OsStr::from_bytes(b"/tmp/\xff"));
        let current = saved_tab(
            &LaunchSpec::shell(Some("/".into())),
            Some(bad.clone()),
            TabMeta::default(),
        );
        assert_eq!(current.folder, Some("/".into()));
        let both = saved_tab(
            &LaunchSpec::shell(Some(bad.clone())),
            Some(bad),
            TabMeta::default(),
        );
        assert_eq!(both.folder, None);
        let state = TabsState {
            layout: SavedLayout {
                pinned: vec![current, both],
                items: Vec::new(),
            },
            selected: Some(0),
        };
        assert_eq!(TabsState::from_json(&state.to_json()), Some(state));
    }
}
