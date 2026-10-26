use super::*;

#[cfg(unix)]
#[gpui::test]
fn restored_command_is_one_shot_and_never_accepted_by_tab_or_enter(
    cx: &mut gpui_kit::TestAppContext,
) {
    use gpui_kit::AppContext;
    use std::sync::{Arc, mpsc};
    use std::time::{Duration, Instant};
    use term_core::{ExecutedCommand, LaunchSpec, Palette, Session, SessionEvent, TermDimensions};

    cx.update(history::init_in_memory);
    let mut spec = LaunchSpec::new(
        SessionKind::Shell,
        Some("sh".into()),
        vec![
            "-c".into(),
            r"printf '\033]133;A;aw=tok\007$ \033]133;B;aw=tok\007'; read line".into(),
        ],
        None,
    );
    spec.mark_token = Some("tok".into());
    let (sender, events) = mpsc::channel();
    let session = Session::spawn(
        1,
        &spec,
        TermDimensions::new(80, 24),
        Palette::default(),
        Arc::new(move |event| {
            let _ = sender.send(event);
        }),
    )
    .unwrap();
    let view = cx.new(|cx| TerminalView::from_test_session(session, &spec, cx));
    view.update(cx, |view, cx| {
        view.restore_last_command(Some("cargo test".into()), cx);
        assert!(!view.handle_history_key(&Keystroke::parse("escape").unwrap(), cx));
        assert!(view.restored_suggestion(cx).is_none());
    });
    let deadline = Instant::now() + Duration::from_secs(5);
    while !view.read_with(cx, |view, _| view.prompt_input().is_some()) {
        view.update(cx, |view, cx| {
            for event in events.try_iter() {
                view.dispatch_test_event(event, cx);
            }
        });
        assert!(Instant::now() < deadline, "first prompt did not arrive");
        std::thread::sleep(Duration::from_millis(10));
    }
    view.update(cx, |view, cx| {
        for command in ["cargo build", "cd project", " secret"] {
            view.dispatch_test_event(
                SessionEvent::CommandExecuted(ExecutedCommand {
                    command: command.into(),
                    folder: None,
                }),
                cx,
            );
            assert_eq!(view.last_command(), Some("cargo build"));
        }
    });
    for key in [
        "tab",
        "enter",
        "ctrl-c",
        "escape",
        "backspace",
        "up",
        "a->a",
    ] {
        view.update(cx, |view, cx| {
            view.close_history_popup(cx);
            view.restore_last_command(Some("git push".into()), cx);
            assert_eq!(view.history_suggestion(cx), Some("git push".into()));
            let consumed = view.handle_history_key(&Keystroke::parse(key).unwrap(), cx);
            assert_eq!(consumed, key == "up", "{key}");
            assert!(view.restored_suggestion(cx).is_none(), "{key}");
            assert!(!view.session.awaiting_echo(), "{key} sent the command");
            assert_eq!(view.last_command(), Some("git push"));
        });
    }
    view.update(cx, |view, cx| {
        view.close_history_popup(cx);
        view.restore_last_command(Some("git push".into()), cx);
    });
    cx.update(|cx| history::forget("git push", cx));
    cx.run_until_parked();
    view.read_with(cx, |view, _| assert_eq!(view.last_command(), None));
    view.update(cx, |view, cx| {
        view.restore_last_command(Some("git push".into()), cx);
        assert!(view.handle_history_key(&Keystroke::parse("right").unwrap(), cx));
        assert!(view.restored_suggestion(cx).is_none());
    });
    // The tty echoes typed bytes at once, so `awaiting_echo` may already be false here: wait
    // for the command to reach the screen instead.
    let deadline = Instant::now() + Duration::from_secs(5);
    while !view.read_with(cx, |view, _| {
        view.session.visible_text()[0].contains("git push")
    }) {
        assert!(Instant::now() < deadline, "accepted command was not typed");
        std::thread::sleep(Duration::from_millis(10));
    }
    for program in ["pwsh", "powershell", "cmd"] {
        let mut excluded = spec.clone();
        excluded.program = Some(program.into());
        let session = Session::spawn(
            2,
            &spec,
            TermDimensions::new(80, 24),
            Palette::default(),
            Arc::new(|_| {}),
        )
        .unwrap();
        let excluded = cx.new(|cx| TerminalView::from_test_session(session, &excluded, cx));
        excluded.update(cx, |view, cx| {
            view.restore_last_command(Some("git push".into()), cx);
            assert_eq!(view.last_command(), None, "{program}");
        });
    }
}

#[cfg(unix)]
#[gpui::test]
fn history_switches_apply_live_pass_keys_through_and_keep_data(cx: &mut gpui_kit::TestAppContext) {
    use std::sync::{Arc, mpsc};
    use std::time::{Duration, Instant};

    use gpui_kit::AppContext;
    use term_core::{ExecutedCommand, LaunchSpec, Palette, Session, SessionEvent, TermDimensions};
    use workspace_model::FeatureSwitch;

    fn executed(command: &str) -> SessionEvent {
        SessionEvent::CommandExecuted(ExecutedCommand {
            command: command.into(),
            folder: None,
        })
    }
    let up = || Keystroke::parse("up").unwrap();
    let tab = || Keystroke::parse("tab").unwrap();

    cx.update(|cx| {
        crate::settings_file::init_in_memory(cx);
        history::init_in_memory(cx);
        history::record(
            ExecutedCommand {
                command: "cargo test".into(),
                folder: None,
            },
            cx,
        );
    });
    let mut spec = LaunchSpec::new(
        SessionKind::Shell,
        Some("sh".into()),
        vec![
            "-c".into(),
            r"printf '\033]133;A;aw=tok\007$ \033]133;B;aw=tok\007'; read line".into(),
        ],
        None,
    );
    spec.mark_token = Some("tok".into());
    let (sender, events) = mpsc::channel();
    let session = Session::spawn(
        1,
        &spec,
        TermDimensions::new(80, 24),
        Palette::default(),
        Arc::new(move |event| {
            let _ = sender.send(event);
        }),
    )
    .unwrap();
    let view = cx.new(|cx| TerminalView::from_test_session(session, &spec, cx));
    let deadline = Instant::now() + Duration::from_secs(5);
    while !view.read_with(cx, |view, _| view.prompt_input().is_some()) {
        view.update(cx, |view, cx| {
            for event in events.try_iter() {
                view.dispatch_test_event(event, cx);
            }
        });
        assert!(Instant::now() < deadline, "first prompt did not arrive");
        std::thread::sleep(Duration::from_millis(10));
    }
    let set = |cx: &mut gpui_kit::TestAppContext, master: bool, popup: bool, ghost: bool| {
        cx.update(|cx| {
            crate::settings_file::preview(cx, |settings| {
                settings.command_history = FeatureSwitch::new(master);
                settings.history_popup = FeatureSwitch::new(popup);
                settings.inline_suggestions = FeatureSwitch::new(ghost);
            });
        });
    };
    let history_len = |cx: &mut gpui_kit::TestAppContext| {
        cx.update(|cx| cx.global::<history::History>().commands().len())
    };

    // Popup off: Up goes to the shell, recording and suggestions go on.
    set(cx, true, false, true);
    view.update(cx, |view, cx| {
        assert!(!view.handle_history_key(&up(), cx));
        assert!(view.history_popup.is_none());
        view.restore_last_command(Some("git push".into()), cx);
        assert_eq!(view.history_suggestion(cx), Some("git push".into()));
        view.dispatch_test_event(executed("make"), cx);
    });
    assert_eq!(history_len(cx), 2);

    // Master off while the popup is open: it closes at once, nothing is used or collected,
    // stored data stays.
    set(cx, true, true, true);
    view.update(cx, |view, cx| {
        assert!(view.handle_history_key(&up(), cx));
        assert!(view.history_popup.is_some());
    });
    set(cx, false, true, true);
    cx.run_until_parked();
    view.update(cx, |view, cx| {
        assert!(view.history_popup.is_none());
        view.restore_last_command(Some("git push".into()), cx);
        assert_eq!(view.history_suggestion(cx), None);
        assert_eq!(view.restored_suggestion(cx), None);
        assert!(!view.handle_history_key(&up(), cx));
        assert!(!view.handle_history_key(&tab(), cx));
        view.dispatch_test_event(executed("cargo build"), cx);
        assert_eq!(view.last_command(), Some("git push"));
    });
    assert_eq!(history_len(cx), 2);

    // Master on again: the children come back as they were.
    set(cx, true, true, true);
    view.update(cx, |view, cx| {
        view.restore_last_command(Some("git push".into()), cx);
        assert_eq!(view.history_suggestion(cx), Some("git push".into()));
        assert!(view.handle_history_key(&up(), cx));
        view.close_history_popup(cx);
    });

    // Suggestions off: ghost text gone and Tab goes to the shell; the popup still works.
    set(cx, true, true, false);
    view.update(cx, |view, cx| {
        view.restore_last_command(Some("git push".into()), cx);
        assert_eq!(view.history_suggestion(cx), None);
        assert!(!view.handle_history_key(&tab(), cx));
        assert!(view.handle_history_key(&up(), cx));
    });

    // Remember last shell command off (or no tab restoration): history still records, but the
    // saved last command is neither captured nor offered; it is kept and comes back.
    set(cx, true, true, true);
    let remember = |cx: &mut gpui_kit::TestAppContext, restore: bool, remember: bool| {
        cx.update(|cx| {
            crate::settings_file::preview(cx, |settings| {
                settings.restore_tabs = FeatureSwitch::new(restore);
                settings.remember_last_command = FeatureSwitch::new(remember);
            });
        });
    };
    let changes = std::rc::Rc::new(std::cell::Cell::new(0));
    let counted = changes.clone();
    cx.update(|cx| {
        cx.subscribe(
            &view,
            move |_, _: &crate::terminal_view::LastCommandChanged, _| {
                counted.set(counted.get() + 1);
            },
        )
        .detach();
    });
    for (restore, remember_on) in [(true, false), (false, true), (false, false)] {
        view.update(cx, |view, cx| {
            view.restore_last_command(Some("git push".into()), cx);
        });
        remember(cx, restore, remember_on);
        cx.run_until_parked();
        let before = history_len(cx);
        view.update(cx, |view, cx| {
            assert_eq!(
                view.restored_suggestion(cx),
                None,
                "{restore} {remember_on}"
            );
            view.dispatch_test_event(executed("brand new command"), cx);
            assert_eq!(view.last_command(), Some("git push"));
        });
        assert_eq!(history_len(cx), before + 1);
        assert_eq!(
            changes.get(),
            0,
            "{restore} {remember_on}: no LastCommandChanged"
        );
        cx.update(|cx| crate::history::forget("brand new command", cx));
        cx.run_until_parked();
    }
    remember(cx, true, true);
    view.update(cx, |view, cx| {
        view.dispatch_test_event(executed("brand new command"), cx);
        assert_eq!(view.last_command(), Some("brand new command"));
    });
    cx.update(|cx| crate::history::forget("brand new command", cx));
    cx.run_until_parked();
    set(cx, true, true, false);

    // The clear actions.
    set(cx, false, true, true);
    view.update(cx, |view, cx| {
        view.restore_last_command(Some("git push".into()), cx)
    });
    assert_eq!(cx.update(history::clear_saved_session_commands), Ok(()));
    cx.run_until_parked();
    view.read_with(cx, |view, _| assert_eq!(view.last_command(), None));
    assert_eq!(history_len(cx), 2);
    assert_eq!(cx.update(history::clear), Ok(()));
    assert_eq!(history_len(cx), 0);
}

fn key(source: &str) -> PopupKey {
    popup_key(&Keystroke::parse(source).unwrap())
}

#[test]
fn popup_keys_navigate_pick_or_go_to_the_shell() {
    assert_eq!(key("up"), PopupKey::Older(Rows::One));
    assert_eq!(key("pagedown"), PopupKey::Newer(Rows::Page));
    assert_eq!(key("enter"), PopupKey::Pick);
    assert_eq!(key("tab"), PopupKey::Pick);
    // Shift+Tab is not Tab: it goes to the shell.
    assert_eq!(key("shift-tab"), PopupKey::Other);
    assert_eq!(key("delete"), PopupKey::Forget);
    assert_eq!(key("shift-delete"), PopupKey::Other);
    assert_eq!(key("escape"), PopupKey::Dismiss);
    // Typing refilters: printable keys, with Shift too, and Backspace.
    assert_eq!(key("a->a"), PopupKey::Edit);
    assert_eq!(key("shift-a->A"), PopupKey::Edit);
    assert_eq!(key("space-> "), PopupKey::Edit);
    assert_eq!(key("backspace"), PopupKey::Edit);
    assert_eq!(key("ctrl-backspace"), PopupKey::Edit);
    // Shortcuts and cursor keys close it.
    assert_eq!(key("ctrl-c"), PopupKey::Other);
    assert_eq!(key("alt-b->b"), PopupKey::Other);
    assert_eq!(key("left"), PopupKey::Other);
    assert_eq!(key("shift-up"), PopupKey::Other);
}

#[test]
fn rendering_new_prompt_text_resets_only_the_presented_selection() {
    let popup = HistoryPopup {
        selection: HistorySelection {
            selected: 7,
            hidden_below: 4,
        },
        query: Some("old".into()),
    };
    assert_eq!(
        popup.visible_selection("new", 12, 3),
        HistorySelection::default()
    );
    assert_eq!(
        popup.selection,
        HistorySelection {
            selected: 7,
            hidden_below: 4
        }
    );
    assert_eq!(popup.query.as_deref(), Some("old"));
}

#[test]
fn resizing_keeps_the_selection_visible_without_mutating_navigation() {
    let popup = HistoryPopup {
        selection: HistorySelection {
            selected: 7,
            hidden_below: 4,
        },
        query: Some("same".into()),
    };
    assert_eq!(
        popup.visible_selection("same", 12, 1),
        HistorySelection {
            selected: 7,
            hidden_below: 7
        }
    );
    assert_eq!(popup.selection.hidden_below, 4);
}

#[cfg(unix)]
#[gpui::test]
fn prompt_events_and_history_changes_reconcile_the_open_popup(cx: &mut gpui_kit::TestAppContext) {
    use std::cell::RefCell;
    use std::rc::Rc;
    use std::sync::{Arc, mpsc};
    use std::time::{Duration, Instant};

    use gpui_kit::component::Root;
    use gpui_kit::{AppContext, VisualTestContext};
    use term_core::{ExecutedCommand, LaunchSpec, Palette, Session, SessionEvent, TermDimensions};

    fn draw(
        cx: &mut VisualTestContext,
        view: &gpui_kit::Entity<TerminalView>,
        events: &mpsc::Receiver<SessionEvent>,
    ) {
        cx.update(|_, cx| {
            view.update(cx, |view, cx| {
                for event in events.try_iter() {
                    view.dispatch_test_event(event, cx);
                }
            });
        });
        cx.run_until_parked();
        cx.update(|window, cx| window.draw(cx).clear(cx));
    }

    fn wait_for_input(
        cx: &mut VisualTestContext,
        view: &gpui_kit::Entity<TerminalView>,
        events: &mpsc::Receiver<SessionEvent>,
        text: &str,
    ) {
        let deadline = Instant::now() + Duration::from_secs(5);
        loop {
            draw(cx, view, events);
            if view.read_with(cx, |view, _| {
                view.prompt_input().is_some_and(|input| input.text == text)
                    && view
                        .history_popup
                        .as_ref()
                        .is_none_or(|popup| popup.query.as_deref() == Some(text))
            }) {
                return;
            }
            assert!(
                Instant::now() < deadline,
                "prompt did not settle to {text:?}: {:?}",
                view.read_with(cx, |view, _| (
                    view.prompt_input().map(|input| input.text),
                    view.session.visible_text(),
                    view.history_popup.as_ref().map(|popup| popup.query.clone()),
                ))
            );
            std::thread::sleep(Duration::from_millis(10));
        }
    }

    cx.update(|cx| {
        gpui_kit::init(cx);
        crate::settings_file::init(cx);
        crate::terminal_element::init(cx);
        crate::terminal_view::init(cx);
        history::init_in_memory(cx);
        for command in ["git status", "git stash", "cargo test"] {
            history::record(
                ExecutedCommand {
                    command: command.into(),
                    folder: None,
                },
                cx,
            );
        }
    });
    let mut spec = LaunchSpec::new(
        SessionKind::Shell,
        Some("sh".into()),
        vec![
            "-c".into(),
            r"printf '\033]133;A;aw=tok\007$ \033]133;B;aw=tok\007git'; read line".into(),
        ],
        None,
    );
    spec.mark_token = Some("tok".into());
    let (sender, events) = mpsc::channel();
    let session = Session::spawn(
        1,
        &spec,
        TermDimensions::new(80, 24),
        Palette::default(),
        Arc::new(move |event| {
            let _ = sender.send(event);
        }),
    )
    .unwrap();
    let captured = Rc::new(RefCell::new(None));
    let capture = captured.clone();
    let window = cx.add_window(move |window, cx| {
        let view = cx.new(|cx| TerminalView::from_test_session(session, &spec, cx));
        view.update(cx, |view, cx| window.focus(&view.focus_handle, cx));
        *capture.borrow_mut() = Some(view.clone());
        Root::new(view, window, cx)
    });
    let view = captured.borrow().clone().unwrap();
    let mut cx = VisualTestContext::from_window(window.into(), cx);
    wait_for_input(&mut cx, &view, &events, "git");
    assert_eq!(
        view.read_with(&cx, |view, cx| view.history_suggestion(cx)),
        Some(" stash".into()),
    );

    cx.simulate_keystrokes("up");
    draw(&mut cx, &view, &events);
    cx.simulate_keystrokes("up");
    draw(&mut cx, &view, &events);
    let navigation = view.read_with(&cx, |view, cx| {
        assert!(view.history_suggestion(cx).is_none());
        let popup = view.history_popup.as_ref().unwrap();
        assert_eq!(popup.selection.selected, 1);
        (popup.selection, popup.query.clone())
    });
    draw(&mut cx, &view, &events);
    draw(&mut cx, &view, &events);
    assert_eq!(
        view.read_with(&cx, |view, _| {
            let popup = view.history_popup.as_ref().unwrap();
            (popup.selection, popup.query.clone())
        }),
        navigation
    );

    cx.update(|window, cx| {
        for (key, text) in [("space", " "), ("s", "s")] {
            window.dispatch_keystroke(
                Keystroke {
                    modifiers: Default::default(),
                    key: key.into(),
                    key_char: Some(text.into()),
                },
                cx,
            );
        }
    });
    wait_for_input(&mut cx, &view, &events, "git s");
    assert_eq!(
        view.read_with(&cx, |view, _| {
            view.history_popup.as_ref().unwrap().selection.selected
        }),
        0
    );
    cx.simulate_keystrokes("up");
    cx.update(|_, cx| history::forget("git status", cx));
    draw(&mut cx, &view, &events);
    assert_eq!(
        view.read_with(&cx, |view, _| {
            view.history_popup.as_ref().unwrap().selection.selected
        }),
        0
    );
    cx.simulate_keystrokes("delete");
    draw(&mut cx, &view, &events);
    cx.update(|_, cx| assert!(cx.global::<History>().commands().matching("git").is_empty()));
    cx.simulate_keystrokes("escape");
    draw(&mut cx, &view, &events);
    view.read_with(&cx, |view, cx| {
        assert!(view.history_popup.is_none());
        assert!(view.history_suggestion(cx).is_none());
    });
}
