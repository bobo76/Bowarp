use std::fs;
use std::time::Duration;

use workspace_model::TabColor;
use workspace_model::agents::CLAUDE;

use super::*;

const WAIT: Duration = Duration::from_secs(10);

struct TestDir(PathBuf);

impl TestDir {
    fn new(name: &str) -> Self {
        let path = std::env::temp_dir().join(format!(
            "agentws-tabs-writer-{name}-{}-{}",
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
        LaunchSpec::agent(
            workspace_model::AgentId::from_cli_selector("claude").unwrap(),
            None,
        ),
        LaunchSpec::agent(
            workspace_model::AgentId::from_cli_selector("copilot").unwrap(),
            None,
        ),
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
        kind: SessionKind::Agent(CLAUDE),
        program: Some("claude".into()),
        args: vec![],
        icon: LauncherIcon::Agent(CLAUDE),
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
