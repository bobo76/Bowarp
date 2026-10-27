use super::*;

use std::time::{Duration, Instant};

/// Let the PTY reader finish before GPUI's deterministic executor polls the event receiver; a
/// later wake from that thread fails the test.
fn wait_for_exit(session: &term_core::Session) {
    let deadline = Instant::now() + Duration::from_secs(30);
    while !session.has_exited() {
        assert!(Instant::now() < deadline, "test child did not exit");
        std::thread::sleep(Duration::from_millis(10));
    }
}

#[test]
fn default_bindings_name_workspace_actions() {
    let bindings = default_bindings();
    assert_eq!(bindings.len(), 19);
    assert!(
        bindings
            .iter()
            .all(|shortcut| shortcut.action.starts_with("workspace::"))
    );
}

#[cfg(unix)]
#[gpui::test]
fn last_command_events_and_history_deletion_are_flushed_with_the_final_snapshot(
    cx: &mut gpui_kit::TestAppContext,
) {
    use gpui_kit::VisualTestContext;
    use gpui_kit::component::Root;
    use std::cell::RefCell;
    use std::sync::mpsc;
    use term_core::{ExecutedCommand, SessionEvent};

    cx.update(|cx| {
        gpui_kit::init(cx);
        settings_file::init(cx);
        crate::terminal_element::init(cx);
        crate::terminal_view::init(cx);
        crate::history::init_in_memory(cx);
        init(cx);
    });
    let (saved, snapshots) = mpsc::channel();
    let writer = Rc::new(tabs_file::TabsWriter::for_test(move |state| {
        saved.send(state).unwrap();
    }));
    let closing_writer = writer.clone();
    let captured = Rc::new(RefCell::new(None));
    let capture = captured.clone();
    let window = cx.add_window(move |window, cx| {
        let (_, messages) = futures::channel::mpsc::unbounded();
        let workspace =
            cx.new(|cx| WorkspaceView::new(Rc::from([]), None, messages, writer, window, cx));
        workspace.update(cx, |workspace, cx| {
            let mut spec = LaunchSpec::new(
                SessionKind::Shell,
                Some("/bin/sh".into()),
                vec!["-c".into(), "exit 0".into()],
                None,
            );
            spec.mark_token = Some("test".into());
            for _ in 0..2 {
                workspace.open_tab(spec.clone(), window, cx).unwrap();
                wait_for_exit(&workspace.tabs.last().unwrap().view.read(cx).session);
            }
        });
        *capture.borrow_mut() = Some(workspace.clone());
        Root::new(workspace, window, cx)
    });
    let workspace = captured.borrow().clone().unwrap();
    let mut cx = VisualTestContext::from_window(window.into(), cx);
    cx.run_until_parked();
    cx.update(|_, cx| {
        workspace.update(cx, |workspace, cx| {
            for tab in &workspace.tabs {
                tab.view.update(cx, |view, cx| {
                    view.dispatch_test_event(
                        SessionEvent::CommandExecuted(ExecutedCommand {
                            command: "cargo test".into(),
                            folder: None,
                        }),
                        cx,
                    );
                });
            }
        });
    });
    cx.run_until_parked();
    for _ in 0..2 {
        let state = snapshots.recv_timeout(Duration::from_secs(10)).unwrap();
        assert_eq!(state.layout.tabs().len(), 2);
        assert!(
            state
                .layout
                .tabs()
                .iter()
                .all(|tab| { tab.last_command.as_deref() == Some("cargo test") })
        );
    }

    // "Restore previous tabs" off: no snapshot is queued; on again: saving resumes.
    let set_restore = |cx: &mut VisualTestContext, on: bool| {
        cx.update(|_, cx| {
            settings_file::preview(cx, |settings| {
                settings.restore_tabs = workspace_model::FeatureSwitch::new(on);
            });
        });
    };
    let save = |cx: &mut VisualTestContext| {
        cx.update(|_, cx| workspace.update(cx, |workspace, cx| workspace.save_tabs(cx)));
        cx.run_until_parked();
    };
    set_restore(&mut cx, false);
    save(&mut cx);
    assert!(snapshots.recv_timeout(Duration::from_millis(300)).is_err());
    set_restore(&mut cx, true);
    save(&mut cx);
    assert!(snapshots.recv_timeout(Duration::from_secs(10)).is_ok());

    cx.update(|_, cx| crate::history::forget("cargo test", cx));
    cx.run_until_parked();
    cx.update(|_, cx| {
        workspace.update(cx, |workspace, cx| {
            workspace.tabs[0].meta.custom_name = Some("final name".into());
            workspace.save_tabs_and_flush(cx);
        });
    });
    assert!(!closing_writer.is_active());
    let states: Vec<_> = snapshots.try_iter().collect();
    assert!(states.len() >= 2, "deletion must queue saves before close");
    let final_state = states.last().unwrap();
    assert!(
        final_state
            .layout
            .tabs()
            .iter()
            .all(|tab| tab.last_command.is_none())
    );
    assert_eq!(
        final_state.layout.tabs()[0].meta.custom_name.as_deref(),
        Some("final name")
    );
    // A retained workspace must not keep writing after the lock can change owners.
    workspace.read_with(&cx, |workspace, cx| workspace.save_tabs(cx));
    assert!(snapshots.try_recv().is_err());
}

/// A window with one tab, an in-memory settings file and a text file to open.
pub(super) struct PanesHarness {
    pub(super) cx: gpui_kit::VisualTestContext,
    pub(super) workspace: Entity<WorkspaceView>,
    folder: PathBuf,
    file: PathBuf,
}

impl PanesHarness {
    pub(super) fn new(name: &str, cx: &mut gpui_kit::TestAppContext) -> Self {
        use gpui_kit::VisualTestContext;
        use gpui_kit::component::Root;
        use std::cell::RefCell;

        cx.update(|cx| {
            gpui_kit::init(cx);
            settings_file::init_in_memory(cx);
            crate::terminal_element::init(cx);
            crate::terminal_view::init(cx);
            init(cx);
        });
        let folder = std::env::temp_dir().join(format!("agentws-{name}-{}", std::process::id()));
        std::fs::create_dir_all(&folder).unwrap();
        let file = folder.join("note.txt");
        std::fs::write(&file, "hello").unwrap();
        #[cfg(windows)]
        let (program, args) = ("cmd", vec!["/C", "exit 0"]);
        #[cfg(not(windows))]
        let (program, args) = ("/bin/sh", vec!["-c", "exit 0"]);
        let spec = LaunchSpec::new(
            SessionKind::Shell,
            Some(program.into()),
            args.into_iter().map(String::from).collect(),
            Some(folder.clone()),
        );
        let captured = Rc::new(RefCell::new(None));
        let capture = captured.clone();
        let window = cx.add_window(move |window, cx| {
            let (_, messages) = futures::channel::mpsc::unbounded();
            let workspace = cx.new(|cx| {
                WorkspaceView::new(
                    Rc::from([]),
                    None,
                    messages,
                    Rc::new(tabs_file::TabsWriter::new(false)),
                    window,
                    cx,
                )
            });
            workspace.update(cx, |workspace, cx| {
                workspace.open_tab(spec, window, cx).unwrap();
                wait_for_exit(&workspace.tabs[0].view.read(cx).session);
            });
            *capture.borrow_mut() = Some(workspace.clone());
            Root::new(workspace, window, cx)
        });
        let workspace = captured.borrow().clone().unwrap();
        let cx = VisualTestContext::from_window(window.into(), cx);
        cx.run_until_parked();
        Self {
            cx,
            workspace,
            folder,
            file,
        }
    }

    fn widths(&mut self) -> (ViewerFraction, ExplorerWidth) {
        self.cx.update(|_, cx| {
            let settings = settings_file::settings(cx);
            (settings.file_viewer_fraction, settings.file_explorer_width)
        })
    }

    /// Saves widths other than the defaults and returns them.
    fn save_widths(&mut self) -> (ViewerFraction, ExplorerWidth) {
        self.cx.update(|_, cx| {
            settings_file::preview(cx, |settings| {
                settings.file_viewer_fraction = ViewerFraction::new(0.4);
                settings.file_explorer_width = ExplorerWidth::new(321.);
            });
        });
        self.widths()
    }

    fn set_panes(&mut self, on: bool) {
        self.cx.update(|_, cx| {
            settings_file::update(cx, |settings| {
                settings.file_panes = FeatureSwitch::new(on);
            });
        });
        self.cx.run_until_parked();
    }

    fn open_file(&mut self) {
        let file = self.file.clone();
        let workspace = self.workspace.clone();
        self.cx.update(|window, cx| {
            workspace.update(cx, |workspace, cx| {
                let id = workspace.tabs[0].id;
                workspace.open_file_in(id, file, FilePlace::default(), window, cx);
            });
        });
        self.cx.run_until_parked();
    }

    fn toggle_explorer(&mut self) {
        let workspace = self.workspace.clone();
        self.cx
            .update(|_, cx| workspace.update(cx, |workspace, cx| workspace.toggle_explorer(cx)));
    }

    /// (panes on, explorer shown, the first tab's viewer).
    fn state(&mut self) -> (bool, bool, Option<WeakEntity<FileViewer>>) {
        self.workspace.read_with(&self.cx, |workspace, _| {
            (
                workspace.file_panes_on,
                workspace.explorer_visible,
                workspace.tabs[0]
                    .viewer
                    .as_ref()
                    .map(|v| v.view.downgrade()),
            )
        })
    }

    fn mark_dirty(&mut self, viewer: &WeakEntity<FileViewer>, dirty: bool) {
        self.cx.update(|_, cx| {
            viewer
                .upgrade()
                .unwrap()
                .update(cx, |viewer, _| viewer.mark_dirty_for_test(dirty));
        });
    }

    fn switch_on(&mut self) -> bool {
        self.cx
            .update(|_, cx| settings_file::file_panes(cx).panes_enabled())
    }

    fn pending(&mut self) -> bool {
        self.workspace
            .read_with(&self.cx, |workspace, _| workspace.file_panes_off_pending)
    }

    /// Opens the file and the explorer, then makes the file's edits unsaved.
    fn open_dirty_viewer(&mut self) -> WeakEntity<FileViewer> {
        self.toggle_explorer();
        self.open_file();
        let (on, explorer, viewer) = self.state();
        assert!(on && explorer);
        let viewer = viewer.expect("the viewer opens while the panes are on");
        self.mark_dirty(&viewer, true);
        viewer
    }
}

impl Drop for PanesHarness {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.folder);
    }
}

#[gpui::test]
fn in_app_notifications_switch_gates_agent_toasts_live(cx: &mut gpui_kit::TestAppContext) {
    use gpui_kit::component::WindowExt;

    let mut h = PanesHarness::new("toasts-live", cx);
    let count = |h: &mut PanesHarness| h.cx.update(|window, cx| window.notifications(cx).len());
    let notify = |h: &mut PanesHarness, notice: AgentNotice| {
        let workspace = h.workspace.clone();
        h.cx.update(|window, cx| {
            workspace.update(cx, |workspace, cx| {
                let id = workspace.tabs[0].id;
                workspace.notify_agent(id, notice, window, cx);
            })
        });
        h.cx.run_until_parked();
    };
    let switch_to = |h: &mut PanesHarness, on: bool| {
        h.cx.update(|_, cx| {
            settings_file::update(cx, |settings| {
                settings.in_app_notifications = FeatureSwitch::new(on);
            });
        });
        h.cx.run_until_parked();
    };
    let turn_off = |h: &mut PanesHarness| switch_to(h, false);
    let turn_on = |h: &mut PanesHarness| switch_to(h, true);

    notify(&mut h, AgentNotice::NeedsAttention(None));
    assert_eq!(count(&mut h), 1, "on by default");
    turn_off(&mut h);
    assert_eq!(count(&mut h), 1, "turning off keeps what is shown");
    notify(&mut h, AgentNotice::Finished);
    notify(&mut h, AgentNotice::Exited(Some(0)));
    assert_eq!(count(&mut h), 1, "off: no agent status toasts");
    // Toasts are keyed by tab, so clear them to tell a new one from a replaced one.
    h.cx.update(|window, cx| window.clear_notifications(cx));
    h.cx.executor()
        .advance_clock(std::time::Duration::from_secs(1));
    h.cx.run_until_parked();
    assert_eq!(count(&mut h), 0);
    notify(&mut h, AgentNotice::Exited(Some(0)));
    assert_eq!(count(&mut h), 0, "off: a clean exit does not toast");
    notify(&mut h, AgentNotice::Exited(Some(1)));
    assert_eq!(count(&mut h), 1, "off: a failed exit still toasts");
    // A user-action toast is not an agent toast.
    h.cx.update(|window, cx| {
        window.push_notification(Notification::info("feedback"), cx);
    });
    assert_eq!(count(&mut h), 2);
    // Toasts are keyed by tab, so clear them to tell a new one from a replaced one.
    h.cx.update(|window, cx| window.clear_notifications(cx));
    h.cx.executor()
        .advance_clock(std::time::Duration::from_secs(1));
    h.cx.run_until_parked();
    assert_eq!(count(&mut h), 0);
    turn_on(&mut h);
    notify(&mut h, AgentNotice::Exited(Some(0)));
    assert_eq!(count(&mut h), 1, "on again: toasts return");
}

#[gpui::test]
fn file_panes_switch_refuses_off_with_unsaved_edits(cx: &mut gpui_kit::TestAppContext) {
    let mut h = PanesHarness::new("panes-refuse", cx);
    let viewer = h.open_dirty_viewer();

    h.set_panes(false);
    let (on, explorer, kept) = h.state();
    assert!(on && explorer && kept.is_some());
    assert!(h.switch_on());
    assert!(h.pending(), "the discard question waits for an answer");
    // A second "off" while the question is open starts no second chain.
    h.set_panes(false);
    assert!(h.switch_on() && h.pending());

    // Cancel ends the chain: nothing closes, and a later "off" asks again.
    h.cx.simulate_keystrokes("escape");
    h.cx.run_until_parked();
    assert!(!h.pending());
    assert!(h.state().0 && h.switch_on() && viewer.upgrade().is_some());
    h.set_panes(false);
    assert!(h.pending());
}

#[gpui::test]
fn saving_over_a_file_changed_on_disk_asks_first(cx: &mut gpui_kit::TestAppContext) {
    let mut h = PanesHarness::new("viewer-conflict", cx);
    h.open_file();
    let viewer = h.state().2.expect("viewer").upgrade().unwrap();
    let save = |h: &mut PanesHarness| {
        let viewer = viewer.clone();
        h.cx.update(|window, cx| viewer.update(cx, |viewer, cx| viewer.save_for_test(window, cx)));
        h.cx.run_until_parked();
    };
    h.cx.update(|window, cx| {
        viewer.update(cx, |viewer, cx| viewer.edit_for_test("mine", window, cx))
    });
    // An agent writes the file meanwhile.
    std::fs::write(&h.file, "agent").unwrap();
    h.cx.run_until_parked();

    save(&mut h);
    assert_eq!(std::fs::read_to_string(&h.file).unwrap(), "agent");
    // Cancel keeps both: the file on disk and the unsaved edits.
    h.cx.simulate_keystrokes("escape");
    h.cx.run_until_parked();
    assert_eq!(std::fs::read_to_string(&h.file).unwrap(), "agent");
    assert!(h.cx.update(|_, cx| viewer.read(cx).is_dirty()));

    save(&mut h);
    // Overwrite is the dialog's confirm button.
    h.cx.simulate_keystrokes("enter");
    h.cx.run_until_parked();
    assert_eq!(std::fs::read_to_string(&h.file).unwrap(), "mine");
    assert!(!h.cx.update(|_, cx| viewer.read(cx).is_dirty()));

    // Unchanged on disk since: saves at once.
    h.cx.update(|window, cx| {
        viewer.update(cx, |viewer, cx| viewer.edit_for_test("again", window, cx))
    });
    save(&mut h);
    assert_eq!(std::fs::read_to_string(&h.file).unwrap(), "again");
}

#[gpui::test]
fn file_panes_switch_off_after_discarding_every_edit(cx: &mut gpui_kit::TestAppContext) {
    let mut h = PanesHarness::new("panes-discard", cx);
    let viewer = h.open_dirty_viewer();

    h.set_panes(false);
    assert!(h.pending());
    // Discard is the dialog's confirm button.
    h.cx.simulate_keystrokes("enter");
    h.cx.run_until_parked();
    let (on, explorer, none) = h.state();
    assert!(!on && !explorer && none.is_none());
    assert!(!h.switch_on() && !h.pending());
    assert!(viewer.upgrade().is_none(), "the viewer must be dropped");
}

#[gpui::test]
fn file_panes_switch_hides_both_panes_and_keeps_widths(cx: &mut gpui_kit::TestAppContext) {
    let mut h = PanesHarness::new("panes-widths", cx);
    let saved_widths = h.save_widths();

    // On: the explorer toggles and a file opens in the viewer.
    h.toggle_explorer();
    h.open_file();
    let (on, explorer, viewer) = h.state();
    assert!(on && explorer);
    let viewer = viewer.expect("the viewer opens while the panes are on");

    // Edits resolved: both panes go, the viewer entity (and its watcher) is dropped.
    h.mark_dirty(&viewer, false);
    h.set_panes(false);
    let (on, explorer, none) = h.state();
    assert!(!on && !explorer && none.is_none());
    assert!(viewer.upgrade().is_none(), "the viewer must be dropped");
    assert_eq!(h.widths(), saved_widths);

    // Off: the toggle does nothing, a file link opens no viewer.
    h.toggle_explorer();
    h.open_file();
    let (_, explorer, none) = h.state();
    assert!(!explorer && none.is_none());
    assert_eq!(h.widths(), saved_widths);

    // On again: the toggle works, the widths are what they were.
    h.set_panes(true);
    h.toggle_explorer();
    let (on, explorer, none) = h.state();
    assert!(on && explorer && none.is_none());
    assert_eq!(h.widths(), saved_widths);
    h.open_file();
    assert!(h.state().2.is_some());
}

#[gpui::test]
fn duplicate_shortcut_does_not_send_eof_to_the_original_terminal(
    cx: &mut gpui_kit::TestAppContext,
) {
    use gpui_kit::VisualTestContext;
    use gpui_kit::component::Root;
    use std::cell::RefCell;

    cx.update(|cx| {
        gpui_kit::init(cx);
        settings_file::init(cx);
        crate::terminal_element::init(cx);
        crate::terminal_view::init(cx);
        init(cx);
    });
    #[cfg(windows)]
    let (program, args) = ("cmd", vec!["/C", "exit 0"]);
    #[cfg(not(windows))]
    let (program, args) = ("/bin/sh", vec!["-c", "exit 0"]);
    let spec = LaunchSpec::new(
        SessionKind::Shell,
        Some(program.into()),
        args.into_iter().map(String::from).collect(),
        None,
    );
    let captured = Rc::new(RefCell::new(None));
    let capture = captured.clone();
    let window = cx.add_window(move |window, cx| {
        let (_, messages) = futures::channel::mpsc::unbounded();
        let workspace = cx.new(|cx| {
            WorkspaceView::new(
                Rc::from([]),
                None,
                messages,
                Rc::new(tabs_file::TabsWriter::new(false)),
                window,
                cx,
            )
        });
        workspace.update(cx, |workspace, cx| {
            workspace.open_tab(spec, window, cx).unwrap();
            // Drain startup output so any later input remains observable through
            // awaiting_echo, independently of the child process's response.
            wait_for_exit(&workspace.tabs[0].view.read(cx).session);
        });
        *capture.borrow_mut() = Some(workspace.clone());
        Root::new(workspace, window, cx)
    });
    let workspace = captured.borrow().clone().unwrap();
    let mut cx = VisualTestContext::from_window(window.into(), cx);
    cx.run_until_parked();
    cx.update(|window, cx| window.draw(cx).clear(cx));
    let original = workspace.read_with(&cx, |workspace, _| workspace.tabs[0].view.clone());
    // Dispatch synchronously so the new child's output drains before GPUI's
    // deterministic executor starts polling its event receiver.
    cx.update(|window, cx| {
        window.dispatch_keystroke(gpui_kit::Keystroke::parse("ctrl-shift-d").unwrap(), cx);
    });
    workspace.read_with(&cx, |workspace, cx| {
        assert_eq!(workspace.tabs.len(), 2);
        assert_eq!(workspace.active, 1);
        assert_eq!(workspace.tabs[0].view, original);
        assert_eq!(
            workspace.tabs[1].spec.program,
            workspace.tabs[0].spec.program
        );
        assert_eq!(workspace.tabs[1].spec.args, workspace.tabs[0].spec.args);
        assert!(
            !original.read(cx).session.awaiting_echo(),
            "the duplicate shortcut must not send Ctrl+D to the original terminal"
        );
        wait_for_exit(&workspace.tabs[1].view.read(cx).session);
    });
    cx.run_until_parked();
    cx.update(|window, cx| window.draw(cx).clear(cx));
    cx.simulate_keystrokes("ctrl-1");
    cx.run_until_parked();
    cx.update(|window, cx| window.draw(cx).clear(cx));
    for shortcut in [
        "ctrl-shift-b",
        "ctrl-shift-b",
        "ctrl-|",
        "ctrl-|",
        "ctrl-shift-a",
    ] {
        cx.simulate_keystrokes(shortcut);
        assert!(
            !original.read_with(&cx, |view, _| view.session.awaiting_echo()),
            "{shortcut} must not reach the terminal"
        );
        cx.run_until_parked();
        cx.update(|window, cx| window.draw(cx).clear(cx));
    }
    assert_eq!(workspace.read_with(&cx, |workspace, _| workspace.active), 0);
    cx.simulate_keystrokes("ctrl-d");
    assert!(
        original.read_with(&cx, |view, _| view.session.awaiting_echo()),
        "plain Ctrl+D must still reach the terminal"
    );
}

#[gpui::test]
fn tab_actions_capture_shortcuts_while_search_has_focus(cx: &mut gpui_kit::TestAppContext) {
    use gpui_kit::VisualTestContext;
    use gpui_kit::component::Root;
    use std::cell::RefCell;

    cx.update(|cx| {
        gpui_kit::init(cx);
        settings_file::init(cx);
        init(cx);
    });
    let captured = Rc::new(RefCell::new(None));
    let capture = captured.clone();
    let window = cx.add_window(move |window, cx| {
        let (_, messages) = futures::channel::mpsc::unbounded();
        let workspace = cx.new(|cx| {
            WorkspaceView::new(
                Rc::from([]),
                None,
                messages,
                Rc::new(tabs_file::TabsWriter::new(false)),
                window,
                cx,
            )
        });
        workspace.update(cx, |workspace, cx| workspace.focus_search(window, cx));
        *capture.borrow_mut() = Some(workspace.clone());
        Root::new(workspace, window, cx)
    });
    let workspace = captured.borrow().clone().unwrap();
    let mut cx = VisualTestContext::from_window(window.into(), cx);
    cx.run_until_parked();
    cx.update(|window, cx| window.draw(cx).clear(cx));
    cx.simulate_keystrokes("ctrl-alt-shift-b");
    assert!(workspace.read_with(&cx, |workspace, _| workspace.sidebar_visible));
    cx.simulate_keystrokes("ctrl-b");
    assert!(workspace.read_with(&cx, |workspace, _| workspace.sidebar_visible));
    cx.simulate_keystrokes("ctrl-shift-b");
    assert!(!workspace.read_with(&cx, |workspace, _| workspace.sidebar_visible));
    assert!(workspace.read_with(&cx, |workspace, cx| workspace.search.query(cx).is_empty()));
}

#[gpui::test]
fn startup_fallback_warning_is_deferred_until_the_root_exists(cx: &mut gpui_kit::TestAppContext) {
    use gpui_kit::VisualTestContext;
    use gpui_kit::component::Root;

    cx.update(gpui_kit::init);
    cx.update(settings_file::init);
    let window = cx.add_window(move |window, cx| {
        let (_, messages) = futures::channel::mpsc::unbounded();
        let workspace = cx.new(|cx| {
            WorkspaceView::new(
                Rc::from([]),
                None,
                messages,
                Rc::new(tabs_file::TabsWriter::new(false)),
                window,
                cx,
            )
        });
        workspace.update(cx, |workspace, cx| {
            workspace.warn_shell_fallback(
                "Could not start broken-shell: executable not found".into(),
                window,
                cx,
            );
        });
        Root::new(workspace, window, cx)
    });
    let mut cx = VisualTestContext::from_window(window.into(), cx);
    cx.run_until_parked();
    assert_eq!(cx.window_title().as_deref(), Some(about::APP_NAME));
    cx.update(|window, cx| assert_eq!(window.notifications(cx).len(), 1));
    assert!(cx.shown_system_notifications().is_empty());
}

#[gpui::test]
fn new_tabs_open_below_the_selected_tab(cx: &mut gpui_kit::TestAppContext) {
    let mut h = PanesHarness::new("new-tab-below", cx);
    let workspace = h.workspace.clone();
    // Open a tab the way the `+` menu, Ctrl+Shift+T and the explorer do; returns its id.
    let open = |h: &mut PanesHarness, select: u64| -> u64 {
        h.cx.update(|window, cx| {
            workspace.update(cx, |workspace, cx| {
                let index = workspace.tab_index(select).unwrap();
                workspace.activate(index, window, cx);
                let spec = workspace.tabs[0].spec.clone();
                workspace.open_tab_or_notify(spec, window, cx);
                let tab = workspace.active_tab().unwrap();
                wait_for_exit(&tab.view.read(cx).session);
                tab.id
            })
        })
    };
    let order = |h: &mut PanesHarness| {
        workspace.read_with(&h.cx, |workspace, _| {
            let ids: Vec<u64> = workspace.tabs.iter().map(|tab| tab.id).collect();
            assert_eq!(ids, workspace.layout.tabs());
            ids
        })
    };
    let first = order(&mut h)[0];
    let second = open(&mut h, first);
    let third = open(&mut h, second);
    // Selected first of three: the new tab is second.
    let below_first = open(&mut h, first);
    assert_eq!(order(&mut h), vec![first, below_first, second, third]);
    // In a group: right below the selected tab, in the group.
    let group = h.cx.update(|_, cx| {
        workspace.update(cx, |workspace, cx| {
            let group = workspace.layout.new_group_with(second, "G").unwrap();
            workspace.layout.move_to_group(third, group);
            workspace.sync_order(cx);
            group
        })
    });
    let in_group = open(&mut h, second);
    assert_eq!(
        order(&mut h),
        vec![first, below_first, second, in_group, third]
    );
    workspace.read_with(&h.cx, |workspace, _| {
        assert_eq!(
            workspace.layout.group(group).unwrap().tabs(),
            &[second, in_group, third]
        );
    });
    // Below a pinned tab: ungrouped, first after the pinned ones.
    h.cx.update(|_, cx| {
        workspace.update(cx, |workspace, cx| {
            workspace.layout.toggle_pin(first);
            workspace.layout.toggle_pin(third);
            workspace.sync_order(cx);
        })
    });
    let below_pinned = open(&mut h, first);
    assert_eq!(
        order(&mut h),
        vec![first, third, below_pinned, below_first, second, in_group]
    );
    workspace.read_with(&h.cx, |workspace, _| {
        assert_eq!(workspace.layout.group_of(below_pinned), None);
        assert_eq!(workspace.active_tab().unwrap().id, below_pinned);
    });
    // From the settings page: below the tab under the page, and the page leaves.
    let under_page = h.cx.update(|window, cx| {
        workspace.update(cx, |workspace, cx| {
            let index = workspace.tab_index(second).unwrap();
            workspace.activate(index, window, cx);
            workspace.open_settings(window, cx);
            let spec = workspace.tabs[0].spec.clone();
            workspace.open_tab_or_notify(spec, window, cx);
            assert!(!workspace.settings.shows_page());
            let tab = workspace.active_tab().unwrap();
            wait_for_exit(&tab.view.read(cx).session);
            tab.id
        })
    });
    let expected = [
        first,
        third,
        below_pinned,
        below_first,
        second,
        under_page,
        in_group,
    ];
    assert_eq!(order(&mut h), expected);
}

#[gpui::test]
fn busy_tabs_are_asked_about_before_closing(cx: &mut gpui_kit::TestAppContext) {
    use gpui_kit::component::WindowExt;
    use term_core::SessionEvent;

    let mut h = PanesHarness::new("busy-close", cx);
    let workspace = h.workspace.clone();
    h.cx.update(|window, cx| {
        workspace.update(cx, |workspace, cx| {
            for _ in 0..3 {
                let spec = workspace.tabs[0].spec.clone();
                workspace.open_tab(spec, window, cx).unwrap();
                wait_for_exit(&workspace.tabs.last().unwrap().view.read(cx).session);
            }
        })
    });
    h.cx.run_until_parked();
    let ids: Vec<u64> = workspace.read_with(&h.cx, |workspace, _| {
        workspace.tabs.iter().map(|tab| tab.id).collect()
    });
    assert_eq!(ids.len(), 4);
    // The middle tab runs a command (`C` mark without `D`).
    h.cx.update(|_, cx| {
        workspace.update(cx, |workspace, cx| {
            workspace.tabs[1].view.update(cx, |view, cx| {
                view.revive_for_test();
                view.dispatch_test_event(SessionEvent::CommandStarted(Some("sleep 9".into())), cx);
            });
        })
    });
    h.cx.run_until_parked();

    let dialog = |h: &mut PanesHarness| h.cx.update(|window, cx| window.has_active_dialog(cx));
    let tab_count = |h: &mut PanesHarness| {
        h.workspace
            .read_with(&h.cx, |workspace, _| workspace.tabs.len())
    };
    // Closes the open dialogs with Esc, top first, and returns how many there were.
    let dismiss_all = |h: &mut PanesHarness| {
        let mut count = 0;
        while h.cx.update(|window, cx| window.has_active_dialog(cx)) {
            assert!(count < 5, "dialogs did not close");
            h.cx.simulate_keystrokes("escape");
            h.cx.run_until_parked();
            count += 1;
        }
        count
    };
    let close_tab = |h: &mut PanesHarness, id: u64| {
        let workspace = h.workspace.clone();
        h.cx.update(|window, cx| {
            workspace.update(cx, |workspace, cx| {
                let index = workspace.tab_index(id).unwrap();
                workspace.close_tab(index, window, cx);
            })
        });
        h.cx.run_until_parked();
    };
    let allow_close_or_ask = |h: &mut PanesHarness| {
        let workspace = h.workspace.clone();
        h.cx.update(|window, cx| {
            workspace.update(cx, |workspace, cx| workspace.allow_close_or_ask(window, cx))
        })
    };

    // A busy tab asks; Cancel (Esc) keeps it.
    close_tab(&mut h, ids[1]);
    assert!(dialog(&mut h));
    assert_eq!(tab_count(&mut h), 4);
    h.cx.simulate_keystrokes("escape");
    h.cx.run_until_parked();
    assert!(!dialog(&mut h));
    assert_eq!(tab_count(&mut h), 4);

    // The window manager's close asks once, however often it is repeated; Cancel clears that.
    assert!(!allow_close_or_ask(&mut h));
    assert!(!allow_close_or_ask(&mut h));
    assert_eq!(dismiss_all(&mut h), 1);
    assert!(!allow_close_or_ask(&mut h));
    assert_eq!(dismiss_all(&mut h), 1);

    // Another dialog is open: the window manager's close still asks, on top of it, once.
    let open_other_dialog = |h: &mut PanesHarness| {
        h.cx.update(|window, cx| {
            window.open_alert_dialog(cx, |alert, _, _| alert.title("Other").show_cancel(true))
        });
        h.cx.run_until_parked();
    };
    open_other_dialog(&mut h);
    assert!(!allow_close_or_ask(&mut h));
    assert!(!allow_close_or_ask(&mut h));
    assert_eq!(dismiss_all(&mut h), 2);
    // Cancelling the question leaves the other dialog, and a new close asks again.
    open_other_dialog(&mut h);
    assert!(!allow_close_or_ask(&mut h));
    h.cx.simulate_keystrokes("escape");
    h.cx.run_until_parked();
    assert!(dialog(&mut h));
    assert!(!allow_close_or_ask(&mut h));
    assert_eq!(dismiss_all(&mut h), 2);

    // An idle tab closes silently.
    close_tab(&mut h, ids[3]);
    assert!(!dialog(&mut h));
    assert_eq!(tab_count(&mut h), 3);

    // Several tabs, one busy and one idle: one question; Cancel closes nothing.
    let close_busy_and_idle = |h: &mut PanesHarness| {
        let workspace = h.workspace.clone();
        h.cx.update(|window, cx| {
            workspace.update(cx, |workspace, cx| {
                workspace.close_tabs(vec![ids[1], ids[2]], window, cx)
            })
        });
        h.cx.run_until_parked();
    };
    close_busy_and_idle(&mut h);
    assert_eq!(dismiss_all(&mut h), 1);
    assert_eq!(tab_count(&mut h), 3);

    // Its Close button (Enter) closes both.
    close_busy_and_idle(&mut h);
    assert!(dialog(&mut h));
    h.cx.simulate_keystrokes("enter");
    h.cx.run_until_parked();
    assert!(!dialog(&mut h));
    assert_eq!(tab_count(&mut h), 1);

    // Nothing busy: the window may close without a question.
    assert!(allow_close_or_ask(&mut h));
    assert!(!dialog(&mut h));
}

#[gpui::test]
fn settings_page_hides_the_viewer_and_opening_a_file_shows_the_tab(
    cx: &mut gpui_kit::TestAppContext,
) {
    let mut h = PanesHarness::new("settings-viewer", cx);
    let viewer = h.open_dirty_viewer();
    let workspace = h.workspace.clone();
    let shown = |h: &mut PanesHarness| {
        h.workspace.read_with(&h.cx, |workspace, cx| {
            (
                workspace.settings.shows_page(),
                workspace.active_viewer(cx).is_some(),
            )
        })
    };
    assert_eq!(shown(&mut h), (false, true));
    h.cx.update(|window, cx| {
        workspace.update(cx, |workspace, cx| workspace.open_settings(window, cx))
    });
    h.cx.run_until_parked();
    assert_eq!(shown(&mut h), (true, false), "the page hides the viewer");
    let (_, _, kept) = h.state();
    assert_eq!(kept, Some(viewer.clone()), "the tab keeps its viewer");
    h.cx.update(|_, cx| {
        let viewer = viewer.upgrade().unwrap();
        assert!(viewer.read(cx).is_open() && viewer.read(cx).is_dirty());
    });
    let page_focused =
        h.cx.update(|window, cx| workspace.read(cx).settings.focus().is_focused(window));
    assert!(page_focused, "the hidden viewer doesn't keep the keyboard");
    h.cx.update(|window, cx| {
        workspace.update(cx, |workspace, cx| workspace.close_settings(window, cx))
    });
    h.cx.run_until_parked();
    assert_eq!(
        shown(&mut h),
        (false, true),
        "the viewer comes back with the tab"
    );
    h.cx.update(|_, cx| assert!(viewer.upgrade().unwrap().read(cx).is_dirty()));
    h.cx.update(|window, cx| {
        workspace.update(cx, |workspace, cx| workspace.open_settings(window, cx))
    });
    h.cx.run_until_parked();
    // Opening a file in the selected tab gives the tab its place back.
    h.mark_dirty(&viewer, false);
    h.open_file();
    assert_eq!(shown(&mut h), (false, true), "the file opens in sight");
}

#[gpui::test]
fn settings_page_hides_the_explorer_and_keeps_it_as_it_was(cx: &mut gpui_kit::TestAppContext) {
    let mut h = PanesHarness::new("settings-explorer", cx);
    let sub = h.folder.join("sub");
    std::fs::create_dir_all(&sub).unwrap();
    std::fs::write(sub.join("inner.txt"), "x").unwrap();
    let width = h.save_widths().1.get();
    h.toggle_explorer();
    let workspace = h.workspace.clone();
    h.cx.update(|_, cx| {
        let explorer = workspace.read(cx).explorer.clone();
        assert!(explorer.update(cx, |explorer, cx| explorer.reveal(&sub, cx)));
    });
    // (page shows, explorer remembered on, on screen, width taken)
    let panes = |h: &mut PanesHarness| {
        h.workspace.read_with(&h.cx, |workspace, cx| {
            (
                workspace.settings.shows_page(),
                workspace.explorer_visible,
                workspace.explorer_shown(),
                workspace.fixed_panes_width(cx) - workspace.sidebar_width(),
            )
        })
    };
    // (root, expanded folders)
    let tree = |h: &mut PanesHarness| {
        h.workspace.read_with(&h.cx, |workspace, cx| {
            workspace.explorer.read(cx).expanded_for_test()
        })
    };
    assert_eq!(panes(&mut h), (false, true, true, width));
    let tree_before = tree(&mut h);
    assert!(tree_before.1.contains(&sub), "the folder is expanded");
    let page = |h: &mut PanesHarness| {
        let workspace = h.workspace.clone();
        h.cx.update(|window, cx| {
            workspace.update(cx, |workspace, cx| workspace.open_settings(window, cx))
        });
        h.cx.run_until_parked();
    };
    page(&mut h);
    assert_eq!(
        panes(&mut h),
        (true, true, false, 0.),
        "the page hides the explorer and gives its width back"
    );
    assert_eq!(
        tree(&mut h),
        tree_before,
        "hiding keeps the folder and expanded folders"
    );
    h.cx.update(|window, cx| {
        workspace.update(cx, |workspace, cx| workspace.close_settings(window, cx))
    });
    h.cx.run_until_parked();
    assert_eq!(panes(&mut h), (false, true, true, width));
    assert_eq!(
        tree(&mut h),
        tree_before,
        "the explorer comes back as it was"
    );

    // The shortcut (and the button, same method) on the page goes back to the tab with the
    // explorer shown.
    for remembered in [true, false] {
        if !remembered {
            h.toggle_explorer();
        }
        page(&mut h);
        h.cx.update(|window, cx| window.draw(cx).clear(cx));
        h.cx.simulate_keystrokes(&format!("ctrl-{EXPLORER_KEY}"));
        h.cx.run_until_parked();
        let (page_shows, _, explorer_shown, _) = panes(&mut h);
        assert_eq!(
            (page_shows, explorer_shown),
            (false, true),
            "remembered on: {remembered}"
        );
        let terminal_focused = h.cx.update(|window, cx| {
            workspace.read(cx).tabs[0]
                .view
                .focus_handle(cx)
                .is_focused(window)
        });
        assert!(terminal_focused, "the tab's terminal gets the keyboard");
    }
}
