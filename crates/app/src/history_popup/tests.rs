use super::*;

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
