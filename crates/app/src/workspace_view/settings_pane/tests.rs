use std::cell::RefCell;
use std::rc::Rc;
use std::sync::{Arc, mpsc as sync_mpsc};
use std::time::Duration;

use futures::channel::mpsc;
use gpui_kit::component::Root;
use gpui_kit::{AppContext, Focusable, TestAppContext, VisualTestContext};
use term_core::{LaunchSpec, Palette, Session, SessionEvent, SessionKind, TermDimensions};
use workspace_model::TabMeta;

use super::*;
use crate::terminal_view::{TabChanged, TerminalView};

#[gpui::test]
fn settings_toggle_focuses_then_closes_and_restores_the_terminal(cx: &mut TestAppContext) {
    cx.update(|cx| {
        gpui_kit::init(cx);
        settings_file::init(cx);
        crate::terminal_element::init(cx);
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
    let (exited, exits) = sync_mpsc::channel();
    let session = Session::spawn(
        1,
        &spec,
        TermDimensions::new(80, 24),
        Palette::default(),
        Arc::new(move |event| {
            if matches!(event, SessionEvent::Exited(_)) {
                exited.send(()).unwrap();
            }
        }),
    )
    .unwrap();
    exits
        .recv_timeout(Duration::from_secs(30))
        .expect("test child did not exit");
    let captured = Rc::new(RefCell::new(None));
    let capture = captured.clone();
    let window = cx.add_window(move |window, cx| {
        let (_, messages) = mpsc::unbounded();
        let workspace = cx.new(|cx| {
            WorkspaceView::new(
                Rc::from([]),
                None,
                messages,
                super::super::TabOwnership::Guest,
                window,
                cx,
            )
        });
        workspace.update(cx, |workspace, cx| {
            let view = cx.new(|cx| TerminalView::from_test_session(session, &spec, cx));
            let events = std::array::from_fn(|_| cx.subscribe(&view, |_, _, _: &TabChanged, _| {}));
            workspace.tabs.push(super::super::Tab {
                id: 1,
                view,
                spec,
                meta: TabMeta::default(),
                viewer: None,
                _view_events: events,
            });
            workspace.layout.add(1, None);
            workspace.activate(0, window, cx);
            workspace.toggle_settings(window, cx);
        });
        *capture.borrow_mut() = Some(workspace.clone());
        Root::new(workspace, window, cx)
    });
    let workspace = captured.borrow().clone().unwrap();
    let mut cx = VisualTestContext::from_window(window.into(), cx);
    cx.run_until_parked();
    cx.update(|window, cx| {
        window.draw(cx).clear(cx);
        workspace.update(cx, |workspace, cx| {
            assert!(workspace.settings.shown);
            assert!(workspace.settings.open);
            assert!(workspace.settings.focus.contains_focused(window, cx));
            let terminal = workspace.tabs[0].view.focus_handle(cx);
            window.focus(&terminal, cx);
            workspace.toggle_settings(window, cx);
            assert!(workspace.settings.shown);
            assert!(workspace.settings.open);
            assert!(workspace.settings.focus.is_focused(window));
            workspace.toggle_settings(window, cx);
            assert!(!workspace.settings.shown);
            assert!(!workspace.settings.open);
            assert!(terminal.is_focused(window));
        });
    });
}
