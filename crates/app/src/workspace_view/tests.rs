use super::*;
use workspace_model::agents::CLAUDE;

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
    assert_eq!(bindings.len(), 20);
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

    cx.update(|_, cx| {
        crate::history::forget("cargo test", &workspace_model::HistoryScope::All, cx)
    });
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
        Self::with_launchers(name, Rc::from([]), cx)
    }

    /// Like `new`, with these launchers for the `+` menu and the empty state.
    fn with_launchers(
        name: &str,
        launchers: Rc<[Launcher]>,
        cx: &mut gpui_kit::TestAppContext,
    ) -> Self {
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
                    launchers,
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
fn reset_name_is_offered_for_a_custom_name_and_shows_the_program_title(
    cx: &mut gpui_kit::TestAppContext,
) {
    let mut h = PanesHarness::new("reset-name", cx);
    let workspace = h.workspace.clone();
    h.cx.update(|window, cx| {
        workspace.update(cx, |workspace, cx| {
            let id = workspace.tabs[0].id;
            let offered = |workspace: &WorkspaceView, cx: &App| {
                workspace.tab_menu_state(id, cx).unwrap().has_custom_name
            };
            let title = workspace.tabs[0].view.read(cx).name().to_string();
            assert!(!offered(workspace, cx));
            workspace.tabs[0].meta.rename("build");
            assert!(offered(workspace, cx));
            assert_eq!(WorkspaceView::tab_name(&workspace.tabs[0], cx), "build");

            workspace.run_tab_command(id, TabCommand::ResetName, window, cx);
            assert_eq!(workspace.tabs[0].meta.custom_name, None);
            assert!(!offered(workspace, cx));
            assert_eq!(WorkspaceView::tab_name(&workspace.tabs[0], cx), title);

            // An open name editor is dropped, so it can't put the old name back later.
            workspace.tabs[0].meta.rename("build");
            workspace.start_rename(RenameTarget::Tab(id), RenameOrigin::Menu, window, cx);
            workspace.run_tab_command(id, TabCommand::ResetName, window, cx);
            assert!(workspace.renaming.is_none());
            workspace.commit_rename(window, cx);
            assert_eq!(workspace.tabs[0].meta.custom_name, None);
        })
    });
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
#[cfg_attr(not(debug_assertions), ignore = "debug_bounds needs debug_assertions")]
fn panel_opacity_leaves_file_viewer_and_settings_opaque(cx: &mut gpui_kit::TestAppContext) {
    use gpui_kit::component::ActiveTheme;
    use workspace_model::{UiTheme, WindowOpacity};

    let mut h = PanesHarness::new("panel-opacity", cx);
    h.toggle_explorer();
    h.open_file();
    for theme in [UiTheme::Dark, UiTheme::Light, UiTheme::Sepia] {
        for percent in [50., 100.] {
            h.cx.update(|_, cx| {
                settings_file::preview(cx, |settings| {
                    settings.ui_theme = theme;
                    settings.window_opacity = WindowOpacity::new(percent);
                });
            });
            h.cx.run_until_parked();
            h.cx.update(|window, cx| window.draw(cx).clear(cx));
            let sidebar = h.cx.debug_bounds("sidebar-header").unwrap();
            let tab = h.cx.debug_bounds("sidebar-tab-0").unwrap();
            let explorer = h.cx.debug_bounds("explorer-header").unwrap();
            let viewer = h.cx.debug_bounds("file-viewer").unwrap();
            h.cx.update(|window, cx| {
                let expected = crate::window_background::panel_background(cx.theme().sidebar, cx);
                let accent =
                    crate::window_background::panel_background(cx.theme().sidebar_accent, cx);
                for (bounds, color, name) in [
                    (sidebar, expected, "sidebar"),
                    (explorer, expected, "explorer"),
                    (tab, accent, "selected tab"),
                    (viewer, cx.theme().background, "opaque viewer"),
                ] {
                    assert_background_at(window, bounds, color, name);
                }
            });
            let row = h.cx.debug_bounds("explorer-row-0").unwrap();
            h.cx.simulate_mouse_move(row.center(), None, Default::default());
            h.cx.update(|window, cx| {
                window.draw(cx).clear(cx);
                assert_background_at(
                    window,
                    row,
                    sidebar::hover_background(cx),
                    "hovered explorer row",
                );
            });
            h.cx.simulate_mouse_move(viewer.center(), None, Default::default());
        }
    }
    let workspace = h.workspace.clone();
    h.cx.update(|window, cx| {
        workspace.update(cx, |workspace, cx| {
            workspace.open_settings(window, cx);
        });
    });
    for theme in [UiTheme::Dark, UiTheme::Light, UiTheme::Sepia] {
        for percent in [50., 100.] {
            h.cx.update(|_, cx| {
                settings_file::preview(cx, |settings| {
                    settings.ui_theme = theme;
                    settings.window_opacity = WindowOpacity::new(percent);
                });
            });
            h.cx.run_until_parked();
            h.cx.update(|window, cx| window.draw(cx).clear(cx));
            let settings = h.cx.debug_bounds("settings-background").unwrap();
            let row = h.cx.debug_bounds("settings-row").unwrap();
            h.cx.update(|window, cx| {
                assert_background_at(window, settings, cx.theme().background, "opaque Settings");
                assert_background_at(
                    window,
                    row,
                    crate::window_background::panel_background(cx.theme().sidebar_accent, cx),
                    "selected Settings row",
                );
            });
        }
    }
}

fn assert_background_at(
    window: &Window,
    bounds: gpui_kit::Bounds<gpui_kit::Pixels>,
    expected: gpui_kit::Hsla,
    name: &str,
) {
    let painted_bounds = bounds.scale(window.scale_factor());
    let backgrounds: Vec<_> = window
        .painted_quads()
        .iter()
        .filter(|quad| quad.bounds == painted_bounds)
        .filter_map(|quad| quad.background.as_solid())
        .filter(|color| color.a > 0.)
        .collect();
    assert!(!backgrounds.is_empty(), "{name} has no fill at {bounds:?}");
    assert!(
        backgrounds.iter().all(|color| *color == expected),
        "{name} at {bounds:?}: {backgrounds:?}, expected {expected:?}"
    );
    if expected.a < 1. {
        assert_eq!(
            backgrounds.len(),
            1,
            "{name} must not stack translucent fills"
        );
    }
}

#[gpui::test]
#[cfg_attr(not(debug_assertions), ignore = "debug_bounds needs debug_assertions")]
fn explorer_opacity_fill_does_not_take_space_from_a_long_listing(
    cx: &mut gpui_kit::TestAppContext,
) {
    let mut h = PanesHarness::new("explorer-opacity-height", cx);
    for index in 0..80 {
        std::fs::write(h.folder.join(format!("file-{index}.txt")), "").unwrap();
    }
    h.toggle_explorer();
    let folder = h.folder.clone();
    let explorer = h
        .workspace
        .read_with(&h.cx, |workspace, _| workspace.explorer.clone());
    h.cx.update(|_, cx| {
        explorer.update(cx, |explorer, cx| {
            explorer.reread(std::collections::HashSet::from([folder]), &[], cx);
        });
    });
    h.cx.run_until_parked();
    h.cx.update(|window, cx| window.draw(cx).clear(cx));
    let panel = h.cx.debug_bounds("file-explorer").unwrap();
    let rows = h.cx.debug_bounds("explorer-rows").unwrap();
    assert_eq!(
        rows.size.height,
        panel.size.height - px(crate::ui_dimensions::HEADER_HEIGHT),
        "a long folder must use all space below the header"
    );
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
    let editor_point = h.cx.update(|window, cx| {
        window.draw(cx).clear(cx);
        gpui_kit::point(
            window.viewport_size().width - px(100.),
            window.viewport_size().height - px(100.),
        )
    });
    h.cx.simulate_mouse_down(
        editor_point,
        MouseButton::Left,
        gpui_kit::Modifiers::default(),
    );
    h.cx.simulate_mouse_up(
        editor_point,
        MouseButton::Left,
        gpui_kit::Modifiers::default(),
    );
    let workspace = h.workspace.clone();
    let terminal_focused = |h: &mut PanesHarness| {
        h.cx.update(|window, cx| {
            workspace.read(cx).tabs[0]
                .view
                .focus_handle(cx)
                .is_focused(window)
        })
    };
    assert!(
        !terminal_focused(&mut h),
        "the viewer starts with the keyboard"
    );

    h.set_panes(false);
    assert!(h.pending());
    // Discard is the dialog's confirm button.
    h.cx.simulate_keystrokes("enter");
    h.cx.run_until_parked();
    let (on, explorer, none) = h.state();
    assert!(!on && !explorer && none.is_none());
    assert!(!h.switch_on() && !h.pending());
    assert!(viewer.upgrade().is_none(), "the viewer must be dropped");
    h.cx.update(|window, cx| window.draw(cx).clear(cx));
    assert!(
        terminal_focused(&mut h),
        "Discard must return the keyboard to the terminal"
    );
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

#[gpui::test]
fn explorer_dims_what_git_ignores_until_it_hides(cx: &mut gpui_kit::TestAppContext) {
    let mut h = PanesHarness::new("explorer-ignored", cx);
    let initialized = std::process::Command::new("git")
        .args(["init", "-q"])
        .current_dir(&h.folder)
        .status()
        .is_ok_and(|status| status.success());
    if !initialized {
        eprintln!("git not available, skipping");
        return;
    }
    std::fs::write(h.folder.join(".gitignore"), "target/\n*.log\n").unwrap();
    let debug = h.folder.join("target").join("debug");
    std::fs::create_dir_all(&debug).unwrap();
    std::fs::write(debug.join("app"), "").unwrap();
    std::fs::write(h.folder.join("a.log"), "").unwrap();
    // (dimmed row names, folders holding git state)
    let dimmed = |h: &mut PanesHarness| {
        h.cx.run_until_parked();
        h.workspace.read_with(&h.cx, |workspace, cx| {
            workspace.explorer.read(cx).ignored_for_test()
        })
    };
    h.toggle_explorer();
    assert_eq!(
        dimmed(&mut h),
        (vec![".git".into(), "target".into(), "a.log".into()], 1),
        "git's answer for the root dims its ignored entries, not the sources"
    );
    let workspace = h.workspace.clone();
    h.cx.update(|_, cx| {
        let explorer = workspace.read(cx).explorer.clone();
        assert!(explorer.update(cx, |explorer, cx| explorer.reveal(&debug, cx)));
    });
    assert_eq!(
        dimmed(&mut h),
        (
            vec![
                ".git".into(),
                "target".into(),
                "debug".into(),
                "app".into(),
                "a.log".into()
            ],
            3
        ),
        "what an ignored folder holds is dimmed"
    );
    // The watcher is unavailable in tests: report the in-place `.gitignore` edit by hand.
    std::fs::write(h.folder.join(".gitignore"), "target/\n").unwrap();
    let (root, gitignore) = (h.folder.clone(), h.folder.join(".gitignore"));
    h.cx.update(|_, cx| {
        let explorer = workspace.read(cx).explorer.clone();
        explorer.update(cx, |explorer, cx| {
            explorer.reread([root].into(), &[gitignore], cx)
        });
    });
    assert_eq!(
        dimmed(&mut h).0,
        vec![
            ".git".to_string(),
            "target".into(),
            "debug".into(),
            "app".into()
        ],
        "a changed `.gitignore` asks git again, so `a.log` is no longer dimmed"
    );
    h.toggle_explorer();
    assert_eq!(
        dimmed(&mut h),
        (Vec::new(), 0),
        "hiding the explorer drops git's answers"
    );
}

#[gpui::test]
fn agent_status_off_warning_stays_once_beside_the_fallback_warning(
    cx: &mut gpui_kit::TestAppContext,
) {
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
            let warning = "Agent status is off: agentws-hook not found next to the app.";
            workspace.warn_agent_status_off(warning.into(), window, cx);
            workspace.warn_agent_status_off(warning.into(), window, cx);
            workspace.warn_shell_fallback("Could not start broken-shell".into(), window, cx);
        });
        Root::new(workspace, window, cx)
    });
    let mut cx = VisualTestContext::from_window(window.into(), cx);
    cx.run_until_parked();
    cx.update(|window, cx| assert_eq!(window.notifications(cx).len(), 2));
    cx.executor().advance_clock(Duration::from_secs(60));
    cx.run_until_parked();
    cx.update(|window, cx| {
        assert_eq!(window.notifications(cx).len(), 2, "both persist");
    });
    assert!(cx.shown_system_notifications().is_empty());
}

/// A launcher that starts `/bin/sh -c "exit 0"` (`None`: not installed). Shells started with
/// their own arguments get no integration script, so nothing is written to a config folder.
#[cfg(unix)]
fn test_launcher(id: &str, kind: SessionKind, installed: bool) -> Launcher {
    Launcher {
        id: id.into(),
        label: id.to_uppercase(),
        kind,
        program: installed.then(|| "/bin/sh".into()),
        args: vec!["-c".into(), "exit 0".into()],
        icon: workspace_model::LauncherIcon::Shell,
    }
}

#[cfg(unix)]
#[gpui::test]
fn closing_the_last_tab_keeps_the_window_its_groups_and_the_explorer_state(
    cx: &mut gpui_kit::TestAppContext,
) {
    let launchers: Rc<[Launcher]> = Rc::from([
        test_launcher("one", SessionKind::Shell, true),
        test_launcher("two", SessionKind::Shell, true),
        test_launcher("agent", SessionKind::Agent(CLAUDE), false),
    ]);
    let mut h = PanesHarness::with_launchers("last-tab", launchers, cx);
    let workspace = h.workspace.clone();
    let first = workspace.read_with(&h.cx, |workspace, _| workspace.tabs[0].id);
    h.cx.update(|_, cx| {
        workspace.update(cx, |workspace, cx| {
            let group = workspace.layout.new_group_with(first, "G").unwrap();
            workspace.layout.group_mut(group).unwrap().color = Some(TabColor::Green);
            workspace.sync_order(cx);
        })
    });
    h.toggle_explorer();
    h.cx.update(|window, cx| window.draw(cx).clear(cx));
    h.cx.simulate_keystrokes("ctrl-shift-w");
    h.cx.run_until_parked();
    // (tabs, groups, explorer remembered on, explorer shown)
    let state = |h: &mut PanesHarness| {
        h.workspace.read_with(&h.cx, |workspace, _| {
            (
                workspace.tabs.len(),
                workspace.layout.groups().count(),
                workspace.explorer_visible,
                workspace.explorer_shown(),
            )
        })
    };
    assert_eq!(
        state(&mut h),
        (0, 1, true, false),
        "the window and its group stay"
    );
    let saved = workspace.read_with(&h.cx, |workspace, _| {
        workspace.layout.save_selected(None, |_| Some(()))
    });
    assert!(matches!(
        &saved.0.items[..],
        [workspace_model::SavedItem::Group(group)]
            if group.name == "G" && group.color == Some(TabColor::Green) && group.tabs.is_empty()
    ));
    let focus = |h: &mut PanesHarness| {
        h.cx.update(|window, cx| {
            let empty = workspace.read(cx).empty_focus.clone();
            (empty.contains_focused(window, cx), window.focused(cx))
        })
    };
    // The default shell's button takes the keyboard once drawn; Tab skips the disabled agent.
    h.cx.update(|window, cx| window.draw(cx).clear(cx));
    h.cx.run_until_parked();
    let (inside, default_button) = focus(&mut h);
    let default_button = default_button.unwrap();
    assert!(inside);
    assert!(workspace.read_with(&h.cx, |workspace, _| default_button
        != workspace.empty_focus));
    h.cx.simulate_keystrokes("tab");
    let (inside, second) = focus(&mut h);
    assert!(inside && second.as_ref() != Some(&default_button));
    h.cx.simulate_keystrokes("tab");
    assert_eq!(
        focus(&mut h),
        (true, Some(default_button.clone())),
        "Tab stays in the empty state"
    );

    // The tab keys do nothing without a tab.
    for keys in [
        "ctrl-shift-w",
        "ctrl-tab",
        "ctrl-shift-tab",
        "ctrl-1",
        "ctrl-shift-a",
    ] {
        h.cx.simulate_keystrokes(keys);
        h.cx.run_until_parked();
        assert_eq!(state(&mut h).0, 0, "{keys}");
    }
    h.cx.simulate_keystrokes("ctrl-shift-d");
    h.cx.run_until_parked();
    assert_eq!(state(&mut h).0, 0);

    // Enter opens the default shell in the last tab's folder, ungrouped at the end.
    h.cx.update(|window, cx| window.draw(cx).clear(cx));
    assert_eq!(focus(&mut h), (true, Some(default_button)));
    // A button clicks on the key's release (`simulate_keystrokes` sends only presses).
    let enter = gpui_kit::Keystroke::parse("enter").unwrap();
    h.cx.simulate_event(gpui_kit::KeyDownEvent {
        keystroke: enter.clone(),
        is_held: false,
        prefer_character_input: false,
    });
    h.cx.simulate_event(gpui_kit::KeyUpEvent { keystroke: enter });
    h.cx.run_until_parked();
    let folder = h.folder.clone();
    h.cx.update(|window, cx| {
        let workspace = workspace.read(cx);
        let tab = workspace.active_tab().expect("a tab opened");
        wait_for_exit(&tab.view.read(cx).session);
        assert_eq!(tab.spec.cwd, Some(folder));
        assert_eq!(workspace.layout.group_of(tab.id), None);
        assert!(matches!(
            workspace.layout.items(),
            [workspace_model::Item::Group(_), workspace_model::Item::Tab(id)] if *id == tab.id
        ));
        assert!(workspace.last_closed_folder.is_none());
        assert!(tab.view.focus_handle(cx).is_focused(window));
    });
    assert_eq!(state(&mut h), (1, 1, true, true), "the explorer comes back");
}

#[gpui::test]
fn settings_page_stays_after_the_last_tab_and_closing_it_shows_the_empty_state(
    cx: &mut gpui_kit::TestAppContext,
) {
    let mut h = PanesHarness::new("last-tab-settings", cx);
    let workspace = h.workspace.clone();
    h.cx.update(|window, cx| {
        workspace.update(cx, |workspace, cx| {
            workspace.open_settings(window, cx);
            workspace.close_tab(0, window, cx);
        })
    });
    h.cx.run_until_parked();
    h.cx.update(|window, cx| {
        let workspace = workspace.read(cx);
        assert!(workspace.tabs.is_empty() && workspace.settings.shows_page());
        assert!(workspace.settings.focus().is_focused(window));
    });
    // Ctrl+Shift+W still closes the page.
    h.cx.update(|window, cx| window.draw(cx).clear(cx));
    h.cx.simulate_keystrokes("ctrl-shift-w");
    h.cx.run_until_parked();
    // No launcher here, so no button: the keyboard stays on the empty state once drawn.
    h.cx.update(|window, cx| window.draw(cx).clear(cx));
    h.cx.run_until_parked();
    h.cx.update(|window, cx| {
        let workspace = workspace.read(cx);
        assert!(!workspace.settings.shows_page());
        assert!(workspace.empty_focus.is_focused(window));
    });
}

#[gpui::test]
fn restoring_no_tab_keeps_the_saved_groups(cx: &mut gpui_kit::TestAppContext) {
    use workspace_model::{SavedGroup, SavedItem, SavedLayout};

    let mut h = PanesHarness::new("restore-groups", cx);
    let workspace = h.workspace.clone();
    let opened = h.cx.update(|window, cx| {
        workspace.update(cx, |workspace, cx| {
            workspace.close_tab(0, window, cx);
            let layout = SavedLayout {
                pinned: Vec::new(),
                items: vec![SavedItem::Group(SavedGroup {
                    name: "Kept".into(),
                    color: Some(TabColor::Blue),
                    collapsed: true,
                    tabs: Vec::new(),
                })],
            };
            workspace.restore(
                TabsState {
                    layout,
                    selected: None,
                },
                window,
                cx,
            )
        })
    });
    assert!(!opened, "no tab reopened: the first tab opens as usual");
    workspace.read_with(&h.cx, |workspace, _| {
        let groups: Vec<_> = workspace
            .layout
            .groups()
            .map(|group| (group.name.clone(), group.color, group.collapsed))
            .collect();
        assert_eq!(groups, vec![("Kept".into(), Some(TabColor::Blue), true)]);
    });
}

#[gpui::test]
fn the_tab_list_takes_the_keyboard_from_the_search_field(cx: &mut gpui_kit::TestAppContext) {
    let mut h = PanesHarness::new("tab-list-keys", cx);
    let workspace = h.workspace.clone();
    // Four tabs, named so a search can find some; the first stays selected.
    let ids = h.cx.update(|window, cx| {
        workspace.update(cx, |workspace, cx| {
            for _ in 0..3 {
                let spec = workspace.tabs[0].spec.clone();
                workspace.open_tab_or_notify(spec, window, cx);
                wait_for_exit(&workspace.active_tab().unwrap().view.read(cx).session);
            }
            workspace.activate(0, window, cx);
            let ids: Vec<u64> = workspace.tabs.iter().map(|tab| tab.id).collect();
            for (tab, name) in workspace
                .tabs
                .iter_mut()
                .zip(["one", "two", "three", "four"])
            {
                tab.meta.custom_name = Some(name.into());
            }
            ids
        })
    });
    let selected = |h: &mut PanesHarness| {
        workspace.read_with(&h.cx, |workspace, _| workspace.active_tab().unwrap().id)
    };
    let list_focused = |h: &mut PanesHarness| {
        h.cx.update(|window, cx| workspace.read(cx).tab_list.focus.is_focused(window))
    };
    let terminal_focused = |h: &mut PanesHarness| {
        h.cx.update(|window, cx| {
            let workspace = workspace.read(cx);
            let tab = workspace.active_tab().unwrap();
            tab.view.focus_handle(cx).is_focused(window)
        })
    };
    let press = |h: &mut PanesHarness, keys: &str| {
        h.cx.update(|window, cx| window.draw(cx).clear(cx));
        h.cx.simulate_keystrokes(keys);
    };
    let search = |h: &mut PanesHarness| {
        h.cx.update(|window, cx| {
            workspace.update(cx, |workspace, cx| workspace.focus_search(window, cx));
        });
    };

    // Down with nothing typed enters the list on the selected tab.
    search(&mut h);
    press(&mut h, "down");
    assert!(list_focused(&mut h));
    assert_eq!(selected(&mut h), ids[0]);
    // The list keys select tabs, keeping the keyboard in the list; Up/Down wrap.
    press(&mut h, "down");
    assert_eq!(selected(&mut h), ids[1]);
    press(&mut h, "end");
    assert_eq!(selected(&mut h), ids[3]);
    press(&mut h, "down");
    assert_eq!(selected(&mut h), ids[0]);
    press(&mut h, "up");
    assert_eq!(selected(&mut h), ids[3]);
    press(&mut h, "home");
    assert_eq!(selected(&mut h), ids[0]);
    assert!(list_focused(&mut h));
    // Escape gives the keyboard to the selected terminal.
    press(&mut h, "escape");
    assert!(!list_focused(&mut h));
    assert!(terminal_focused(&mut h));

    // A collapsed group's tabs are skipped.
    h.cx.update(|_, cx| {
        workspace.update(cx, |workspace, cx| {
            let group = workspace.layout.new_group_with(ids[1], "G").unwrap();
            workspace.layout.move_to_group(ids[2], group);
            workspace.layout.group_mut(group).unwrap().collapsed = true;
            workspace.sync_order(cx);
        })
    });
    search(&mut h);
    press(&mut h, "tab");
    assert!(list_focused(&mut h), "Tab enters the list too");
    press(&mut h, "down");
    assert_eq!(selected(&mut h), ids[3]);
    press(&mut h, "up");
    assert_eq!(selected(&mut h), ids[0]);
    // Shift+Tab goes back to the field; Tab (one focus stop) or Enter in the list to the
    // terminal.
    press(&mut h, "shift-tab");
    assert!(!list_focused(&mut h));
    press(&mut h, "tab");
    press(&mut h, "tab");
    assert!(
        terminal_focused(&mut h),
        "Tab leaves the list for the terminal"
    );
    search(&mut h);
    press(&mut h, "tab");
    press(&mut h, "enter");
    assert!(terminal_focused(&mut h));

    // While searching: Down steps through the matches in the field, Tab enters the list on
    // the first match when the selected tab is hidden, and the list keeps to the matches,
    // collapsed group included.
    search(&mut h);
    // ("o": not in the test folder's path, which the search also matches.)
    h.cx.simulate_input("o");
    press(&mut h, "down");
    press(&mut h, "down");
    assert!(
        !list_focused(&mut h),
        "Down steps through matches while a query is typed"
    );
    assert_eq!(selected(&mut h), ids[1]);
    h.cx.update(|window, cx| {
        workspace.update(cx, |workspace, cx| {
            let index = workspace.tab_index(ids[2]).unwrap();
            workspace.select_tab(index, window, cx);
            workspace.focus_search(window, cx);
        })
    });
    h.cx.update(|window, cx| window.draw(cx).clear(cx));
    h.cx.simulate_input("wo");
    press(&mut h, "tab");
    assert!(list_focused(&mut h));
    assert_eq!(selected(&mut h), ids[1], "\"wo\" matches only \"two\"");
    press(&mut h, "down");
    assert_eq!(selected(&mut h), ids[1]);
    // Shift+Tab keeps the search; Escape in the list ends it, as in the field.
    let query_empty = |h: &mut PanesHarness| {
        workspace.read_with(&h.cx, |workspace, cx| workspace.search.query(cx).is_empty())
    };
    press(&mut h, "shift-tab");
    assert!(!query_empty(&mut h));
    press(&mut h, "tab");
    press(&mut h, "escape");
    assert!(terminal_focused(&mut h));
    assert!(query_empty(&mut h));

    // Hiding the sidebar while the list has the keyboard hands it to the terminal.
    search(&mut h);
    press(&mut h, "down");
    assert!(list_focused(&mut h));
    press(&mut h, "ctrl-shift-b");
    assert!(terminal_focused(&mut h));
}

#[gpui::test]
fn the_tab_list_keeps_the_settings_page_until_a_key_moves_off_it(
    cx: &mut gpui_kit::TestAppContext,
) {
    let mut h = PanesHarness::new("tab-list-settings", cx);
    let workspace = h.workspace.clone();
    let ids = h.cx.update(|window, cx| {
        workspace.update(cx, |workspace, cx| {
            for _ in 0..2 {
                let spec = workspace.tabs[0].spec.clone();
                workspace.open_tab_or_notify(spec, window, cx);
                wait_for_exit(&workspace.active_tab().unwrap().view.read(cx).session);
            }
            workspace.activate(1, window, cx);
            workspace.open_settings(window, cx);
            workspace.focus_search(window, cx);
            workspace
                .tabs
                .iter()
                .map(|tab| tab.id)
                .collect::<Vec<u64>>()
        })
    });
    let state = |h: &mut PanesHarness| {
        h.cx.update(|window, cx| {
            let workspace = workspace.read(cx);
            (
                workspace.tab_list.focus.is_focused(window),
                workspace.settings.shows_page(),
                workspace.active_tab().unwrap().id,
            )
        })
    };
    let press = |h: &mut PanesHarness, keys: &str| {
        h.cx.update(|window, cx| window.draw(cx).clear(cx));
        h.cx.simulate_keystrokes(keys);
    };

    // Entering moves only the keyboard: the page stays and the selected tab is unchanged.
    press(&mut h, "down");
    assert_eq!(state(&mut h), (true, true, ids[1]));
    // The first Down selects the tab entering would have selected, not the one after it.
    press(&mut h, "down");
    assert_eq!(state(&mut h), (true, false, ids[1]));
    press(&mut h, "down");
    assert_eq!(state(&mut h).2, ids[2]);

    // Escape from the list gives the keyboard to the Settings page while it shows.
    h.cx.update(|window, cx| {
        workspace.update(cx, |workspace, cx| {
            workspace.open_settings(window, cx);
            workspace.focus_search(window, cx);
        })
    });
    press(&mut h, "down");
    press(&mut h, "escape");
    h.cx.update(|window, cx| {
        let workspace = workspace.read(cx);
        assert!(workspace.settings.shows_page());
        assert!(workspace.settings.focus().is_focused(window));
    });

    // End selects the last tab and hides the page.
    h.cx.update(|window, cx| {
        workspace.update(cx, |workspace, cx| workspace.focus_search(window, cx))
    });
    press(&mut h, "down");
    press(&mut h, "end");
    assert_eq!(state(&mut h), (true, false, ids[2]));
}

#[gpui::test]
fn f2_in_the_tab_list_renames_the_selected_tab_and_gives_the_list_the_keyboard_back(
    cx: &mut gpui_kit::TestAppContext,
) {
    let mut h = PanesHarness::new("tab-list-rename", cx);
    let workspace = h.workspace.clone();
    // Focus-lost events (the editor's Blur) need an active window.
    h.cx.update(|window, _| window.activate_window());
    let ids = h.cx.update(|window, cx| {
        workspace.update(cx, |workspace, cx| {
            let spec = workspace.tabs[0].spec.clone();
            workspace.open_tab_or_notify(spec, window, cx);
            wait_for_exit(&workspace.active_tab().unwrap().view.read(cx).session);
            workspace.activate(0, window, cx);
            workspace
                .tabs
                .iter()
                .map(|tab| tab.id)
                .collect::<Vec<u64>>()
        })
    });
    let press = |h: &mut PanesHarness, keys: &str| {
        h.cx.update(|window, cx| window.draw(cx).clear(cx));
        h.cx.simulate_keystrokes(keys);
    };
    let enter_list = |h: &mut PanesHarness| {
        h.cx.update(|window, cx| {
            workspace.update(cx, |workspace, cx| workspace.focus_search(window, cx));
        });
        press(h, "down");
    };
    let list_focused = |h: &mut PanesHarness| {
        h.cx.update(|window, cx| workspace.read(cx).tab_list.focus.is_focused(window))
    };
    let terminal_focused = |h: &mut PanesHarness| {
        h.cx.update(|window, cx| {
            let workspace = workspace.read(cx);
            let tab = workspace.active_tab().unwrap();
            tab.view.focus_handle(cx).is_focused(window)
        })
    };
    let renaming = |h: &mut PanesHarness| {
        workspace.read_with(&h.cx, |workspace, _| {
            workspace
                .renaming
                .as_ref()
                .map(|rename| (rename.target, rename.origin))
        })
    };
    let name = |h: &mut PanesHarness, index: usize| {
        workspace.read_with(&h.cx, |workspace, _| {
            workspace.tabs[index].meta.custom_name.clone()
        })
    };

    // F2 opens the selected tab's name editor; Enter keeps the name and the list gets the
    // keyboard back, so Down moves the selection.
    enter_list(&mut h);
    press(&mut h, "down");
    press(&mut h, "f2");
    assert_eq!(
        renaming(&mut h),
        Some((RenameTarget::Tab(ids[1]), RenameOrigin::TabList))
    );
    assert!(!list_focused(&mut h), "the editor has the keyboard");
    h.cx.simulate_input("build");
    press(&mut h, "f2");
    assert_eq!(renaming(&mut h).unwrap().0, RenameTarget::Tab(ids[1]));
    press(&mut h, "enter");
    assert_eq!(renaming(&mut h), None);
    assert_eq!(name(&mut h, 1).as_deref(), Some("build"));
    assert!(list_focused(&mut h));
    press(&mut h, "down");
    assert_eq!(
        workspace.read_with(&h.cx, |workspace, _| workspace.active),
        0
    );

    // Escape keeps the old name and also gives the keyboard back to the list.
    press(&mut h, "f2");
    h.cx.simulate_input("other");
    press(&mut h, "escape");
    assert_eq!(renaming(&mut h), None);
    assert_eq!(name(&mut h, 0), None);
    assert!(list_focused(&mut h));

    // A click elsewhere commits and leaves the keyboard where the click put it.
    press(&mut h, "f2");
    h.cx.simulate_input("clicked");
    h.cx.update(|window, cx| {
        let terminal = workspace.read(cx).tabs[0].view.focus_handle(cx);
        window.focus(&terminal, cx);
    });
    h.cx.run_until_parked();
    h.cx.update(|window, cx| window.draw(cx).clear(cx));
    h.cx.run_until_parked();
    assert_eq!(renaming(&mut h), None);
    assert_eq!(name(&mut h, 0).as_deref(), Some("clicked"));
    assert!(terminal_focused(&mut h));

    // Hiding the sidebar while the editor has the keyboard: it commits and the terminal gets
    // the keyboard, the hidden list can't.
    enter_list(&mut h);
    press(&mut h, "f2");
    h.cx.update(|_, cx| {
        workspace.update(cx, |workspace, cx| {
            workspace.sidebar_visible = false;
            cx.notify();
        })
    });
    h.cx.update(|window, cx| window.draw(cx).clear(cx));
    h.cx.run_until_parked();
    assert_eq!(renaming(&mut h), None);
    assert!(terminal_focused(&mut h));
    h.cx.update(|_, cx| {
        workspace.update(cx, |workspace, cx| {
            workspace.sidebar_visible = true;
            cx.notify();
        })
    });

    // F2 in the terminal is left to the program.
    press(&mut h, "f2");
    assert_eq!(renaming(&mut h), None);
    assert!(terminal_focused(&mut h));

    // The tab menu's Rename keeps giving the keyboard back to the terminal.
    h.cx.update(|window, cx| {
        workspace.update(cx, |workspace, cx| {
            workspace.run_tab_command(ids[0], TabCommand::Rename, window, cx);
        })
    });
    assert_eq!(
        renaming(&mut h),
        Some((RenameTarget::Tab(ids[0]), RenameOrigin::Menu))
    );
    h.cx.simulate_input("menu");
    press(&mut h, "enter");
    assert_eq!(renaming(&mut h), None);
    assert_eq!(name(&mut h, 0).as_deref(), Some("menu"));
    assert!(terminal_focused(&mut h));

    // With the ring on the Settings row, F2 renames nothing (the selected tab is hidden).
    h.cx.update(|window, cx| {
        workspace.update(cx, |workspace, cx| workspace.open_settings(window, cx))
    });
    enter_list(&mut h);
    assert!(list_focused(&mut h));
    press(&mut h, "f2");
    assert_eq!(renaming(&mut h), None);
    assert!(list_focused(&mut h));
}

#[gpui::test]
fn a_name_editor_keeps_the_search_and_a_rename_that_hides_the_tab_sends_the_keyboard_on(
    cx: &mut gpui_kit::TestAppContext,
) {
    let mut h = PanesHarness::new("rename-with-search", cx);
    let workspace = h.workspace.clone();
    h.cx.update(|window, _| window.activate_window());
    let ids = h.cx.update(|window, cx| {
        workspace.update(cx, |workspace, cx| {
            let spec = workspace.tabs[0].spec.clone();
            workspace.open_tab_or_notify(spec, window, cx);
            wait_for_exit(&workspace.active_tab().unwrap().view.read(cx).session);
            for tab in &mut workspace.tabs {
                tab.meta.rename("quux");
            }
            workspace.activate(1, window, cx);
            workspace
                .tabs
                .iter()
                .map(|tab| tab.id)
                .collect::<Vec<u64>>()
        })
    });
    let press = |h: &mut PanesHarness, keys: &str| {
        h.cx.update(|window, cx| window.draw(cx).clear(cx));
        h.cx.simulate_keystrokes(keys);
    };
    let search = |h: &mut PanesHarness| {
        h.cx.update(|window, cx| {
            workspace.update(cx, |workspace, cx| workspace.focus_search(window, cx));
        });
        h.cx.simulate_input("quux");
    };
    let query_kept = |h: &mut PanesHarness| {
        workspace.read_with(&h.cx, |workspace, cx| {
            !workspace.search.query(cx).is_empty()
        })
    };

    // Enter in a tab name editor only commits: the list's own Enter (end the search) doesn't run.
    search(&mut h);
    h.cx.update(|window, cx| {
        workspace.update(cx, |workspace, cx| {
            workspace.run_tab_command(ids[0], TabCommand::Rename, window, cx);
        })
    });
    h.cx.simulate_input("quux-menu");
    press(&mut h, "enter");
    assert!(
        query_kept(&mut h),
        "Enter in the tab name editor keeps the search"
    );

    // The same in a group name editor.
    h.cx.update(|window, cx| {
        workspace.update(cx, |workspace, cx| {
            let group = workspace
                .layout
                .new_group_with(ids[0], "quux-group")
                .unwrap();
            workspace.run_group_command(group, GroupCommand::Rename, window, cx);
        })
    });
    h.cx.simulate_input("quux-team");
    press(&mut h, "enter");
    assert!(
        query_kept(&mut h),
        "Enter in the group name editor keeps the search"
    );

    // A rename from the list that hides the selected tab: the terminal gets the keyboard, as no
    // row could show the list's focus ring.
    h.cx.update(|window, cx| {
        workspace.update(cx, |workspace, cx| {
            workspace.select_tab(1, window, cx);
        })
    });
    search(&mut h);
    press(&mut h, "tab");
    assert!(h.cx.update(|window, cx| workspace.read(cx).tab_list.focus.is_focused(window)));
    press(&mut h, "f2");
    h.cx.simulate_input("zzz");
    press(&mut h, "enter");
    h.cx.run_until_parked();
    assert!(workspace.read_with(&h.cx, |workspace, _| workspace.renaming.is_none()));
    assert!(h.cx.update(|window, cx| {
        let workspace = workspace.read(cx);
        let tab = workspace.active_tab().unwrap();
        tab.view.focus_handle(cx).is_focused(window)
    }));
}

#[gpui::test]
fn the_tab_list_cannot_take_the_keyboard_without_a_tab(cx: &mut gpui_kit::TestAppContext) {
    let mut h = PanesHarness::new("tab-list-empty", cx);
    let workspace = h.workspace.clone();
    h.cx.update(|window, cx| {
        workspace.update(cx, |workspace, cx| {
            workspace.close_tab(0, window, cx);
            workspace.focus_search(window, cx);
        })
    });
    for key in ["down", "tab"] {
        h.cx.update(|window, cx| window.draw(cx).clear(cx));
        h.cx.simulate_keystrokes(key);
        h.cx.update(|window, cx| {
            let workspace = workspace.read(cx);
            assert!(!workspace.tab_list.focus.is_focused(window));
            assert!(
                !workspace.empty_focus.is_focused(window),
                "the field keeps it"
            );
        });
    }
}
