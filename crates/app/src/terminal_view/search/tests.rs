use super::*;
use std::cell::RefCell;
use std::rc::Rc;
use std::sync::{Arc, mpsc};
use std::time::Duration;

use gpui_kit::component::Root;
use gpui_kit::{Task, TestAppContext, VisualTestContext};
use term_core::alacritty_terminal::index::{Column, Point};
use term_core::alacritty_terminal::vte::ansi::{Processor, StdSyncHandler};
use term_core::{LaunchSpec, Palette, Session, SessionEvent, SessionKind, TermDimensions};

fn draw(cx: &mut VisualTestContext) {
    cx.run_until_parked();
    cx.update(|window, cx| window.draw(cx).clear(cx));
}

#[gpui::test]
fn search_shortcuts_keep_input_out_of_the_program(cx: &mut TestAppContext) {
    cx.update(|cx| {
        gpui_kit::init(cx);
        crate::settings_file::init(cx);
        crate::terminal_element::init(cx);
        super::super::init(cx);
    });
    #[cfg(windows)]
    let (program, args) = (
        "powershell",
        vec!["-NoProfile", "-NonInteractive", "-Command", "exit 0"],
    );
    #[cfg(not(windows))]
    let (program, args) = ("/bin/sh", vec!["-c", "exit 0"]);
    let spec = LaunchSpec::new(
        SessionKind::Shell,
        Some(program.into()),
        args.into_iter().map(String::from).collect(),
        None,
    );
    let (exited, exit_events) = mpsc::channel();
    let session = Session::spawn(
        1,
        &spec,
        TermDimensions::new(80, 24),
        Palette::default(),
        Arc::new(move |event| {
            if let SessionEvent::Exited(_) = event {
                exited
                    .send(())
                    .expect("test must wait for the child to exit");
            }
        }),
    )
    .unwrap();
    // Drain ConPTY startup output before seeding a grid that only the test can mutate.
    exit_events
        .recv_timeout(Duration::from_secs(30))
        .expect("test child did not exit and drain its terminal output");
    Processor::<StdSyncHandler>::new().advance(&mut *session.lock_term(), b"\x1bcone one");
    let captured = Rc::new(RefCell::new(None));
    let capture = captured.clone();
    let window = cx.add_window(move |window, cx| {
        let view = cx.new(|cx| {
            TerminalView::new(
                session,
                &spec,
                super::super::StatusReporting::OutputActivity,
                Task::ready(()),
                cx,
            )
        });
        view.update(cx, |view, cx| window.focus(&view.focus_handle, cx));
        *capture.borrow_mut() = Some(view.clone());
        Root::new(view, window, cx)
    });
    let view = captured.borrow().clone().unwrap();
    let mut cx = VisualTestContext::from_window(window.into(), cx);
    draw(&mut cx);
    cx.simulate_keystrokes("ctrl-shift-f");
    draw(&mut cx);
    assert!(view.read_with(&cx, |view, _| view.search_bar.is_some()));
    cx.simulate_input("ONE");
    draw(&mut cx);
    view.read_with(&cx, |view, _| {
        assert_eq!(view.search.matches().len(), 2);
        assert_eq!(view.search.current(), Some(0));
        assert!(!view.session.awaiting_echo());
    });
    cx.simulate_keystrokes("enter");
    draw(&mut cx);
    assert_eq!(
        view.read_with(&cx, |view, _| view.search.current()),
        Some(1)
    );
    assert!(!view.read_with(&cx, |view, _| view.session.awaiting_echo()));
    cx.simulate_keystrokes("shift-enter");
    draw(&mut cx);
    assert_eq!(
        view.read_with(&cx, |view, _| view.search.current()),
        Some(0)
    );
    assert!(!view.read_with(&cx, |view, _| view.session.awaiting_echo()));
    cx.simulate_keystrokes("ctrl-shift-f");
    draw(&mut cx);
    cx.simulate_input("absent");
    draw(&mut cx);
    view.read_with(&cx, |view, cx| {
        let bar = view.search_bar.as_ref().unwrap();
        assert_eq!(bar.input.read(cx).value().as_ref(), "absent");
        assert!(view.search.matches().is_empty());
        assert!(!view.session.awaiting_echo());
    });
    cx.simulate_keystrokes("escape");
    draw(&mut cx);
    cx.update(|window, cx| {
        let view = view.read(cx);
        assert!(view.search_bar.is_none());
        assert!(view.search.matches().is_empty());
        assert!(view.focus_handle.is_focused(window));
        assert!(!view.session.awaiting_echo());
        assert_eq!(view.session.visible_text()[0], "one one");
        assert_eq!(
            view.session.lock_term().grid().cursor.point,
            Point::new(0.into(), Column(7)),
        );
    });
    cx.simulate_input("x");
    view.read_with(&cx, |view, _| {
        assert!(view.session.awaiting_echo());
    });
}
