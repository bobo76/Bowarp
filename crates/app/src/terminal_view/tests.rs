use super::*;

#[test]
fn only_a_hidden_finished_turn_is_ready_for_review() {
    use AgentStatus::*;
    assert!(finished_unseen(Working, Idle, false));
    assert!(!finished_unseen(Working, Idle, true));
    assert!(!finished_unseen(NeedsAttention, Idle, false));
    assert!(!finished_unseen(Working, NeedsAttention, false));
    assert!(!finished_unseen(Working, Exited, false));
}

#[gpui::test]
fn unread_completions_are_acknowledged_and_replaced_for_both_agents(
    cx: &mut gpui_kit::TestAppContext,
) {
    use agent_status::HookEvent;

    for kind in [SessionKind::ClaudeCode, SessionKind::CopilotCli] {
        #[cfg(windows)]
        let (program, args) = ("cmd", vec!["/C", "exit 0"]);
        #[cfg(not(windows))]
        let (program, args) = ("/bin/sh", vec!["-c", "exit 0"]);
        let spec = LaunchSpec::new(
            kind,
            Some(program.into()),
            args.into_iter().map(String::from).collect(),
            None,
        );
        let session = Session::spawn(
            1,
            &spec,
            TermDimensions::new(80, 24),
            Palette::default(),
            Arc::new(|_| {}),
        )
        .unwrap();
        let view = cx.new(|cx| {
            TerminalView::new(session, &spec, StatusReporting::Hooks, Task::ready(()), cx)
        });
        view.update(cx, |view, cx| {
            let mut at = 0;
            let mut report = |view: &mut TerminalView, working, cx: &mut Context<TerminalView>| {
                at += 1;
                match kind {
                    SessionKind::ClaudeCode => view.apply_hook_event(
                        AgentEvent::Hook(if working {
                            HookEvent::PromptSubmitted
                        } else {
                            HookEvent::Stopped
                        }),
                        at,
                        cx,
                    ),
                    SessionKind::CopilotCli => view.apply_progress(
                        if working {
                            ProgressState::Indeterminate
                        } else {
                            ProgressState::Hidden
                        },
                        at,
                        cx,
                    ),
                    SessionKind::Shell => unreachable!(),
                }
            };
            report(view, true, cx);
            report(view, false, cx);
            assert_eq!(view.tab_status(), TabStatus::Idle);

            view.hide();
            report(view, true, cx);
            report(view, false, cx);
            assert_eq!(view.tab_status(), TabStatus::ReadyForReview);
            report(view, false, cx);
            view.on_session_event(SessionEvent::TitleChanged(Some("Agent".into())), cx);
            view.on_session_event(SessionEvent::Wakeup, cx);
            view.apply_hook_event(
                AgentEvent::ModeChanged(agent_status::AgentMode::Plan),
                0,
                cx,
            );
            assert_eq!(view.tab_status(), TabStatus::ReadyForReview);

            view.show(cx);
            assert_eq!(view.tab_status(), TabStatus::Idle);
            view.hide();
            report(view, false, cx);
            assert_eq!(view.tab_status(), TabStatus::Idle);

            report(view, true, cx);
            report(view, false, cx);
            report(view, true, cx);
            assert_eq!(view.tab_status(), TabStatus::Working);
            assert!(!view.finished_unseen);
            report(view, false, cx);
            assert_eq!(view.tab_status(), TabStatus::ReadyForReview);
            view.apply_hook_event(
                AgentEvent::Hook(HookEvent::InputRequested),
                agent_status::now_micros(),
                cx,
            );
            assert_eq!(view.tab_status(), TabStatus::NeedsAttention);
            assert!(!view.finished_unseen);
        });
    }
}

#[test]
fn agent_notices_classify_status_transitions() {
    use AgentStatus::*;
    for before in [Idle, Working, NeedsAttention, Exited] {
        for after in [Idle, Working, NeedsAttention, Exited] {
            let expected = match (before, after) {
                (Working, Idle) => Some(AgentNotice::Finished),
                (before, NeedsAttention) if before != NeedsAttention => {
                    Some(AgentNotice::NeedsAttention)
                }
                _ => None,
            };
            assert_eq!(AgentNotice::for_change(before, after), expected);
        }
    }
}

#[gpui::test]
fn repeated_hooks_titles_and_output_do_not_repeat_agent_notices(cx: &mut gpui_kit::TestAppContext) {
    use std::cell::RefCell;
    use std::rc::Rc;

    use agent_status::HookEvent;

    #[cfg(windows)]
    let (program, args) = ("cmd", vec!["/C", "exit 0"]);
    #[cfg(not(windows))]
    let (program, args) = ("/bin/sh", vec!["-c", "exit 0"]);
    let spec = LaunchSpec::new(
        SessionKind::ClaudeCode,
        Some(program.into()),
        args.into_iter().map(String::from).collect(),
        None,
    );
    let session = Session::spawn(
        1,
        &spec,
        TermDimensions::new(80, 24),
        Palette::default(),
        Arc::new(|_| {}),
    )
    .unwrap();
    let view =
        cx.new(|cx| TerminalView::new(session, &spec, StatusReporting::Hooks, Task::ready(()), cx));
    let notices = Rc::new(RefCell::new(Vec::new()));
    let captured = notices.clone();
    let _events = cx.update(|cx| {
        cx.subscribe(&view, move |_, notice: &AgentNotice, _| {
            captured.borrow_mut().push(*notice);
        })
    });
    view.update(cx, |view, cx| {
        for event in [
            HookEvent::PromptSubmitted,
            HookEvent::InputRequested,
            HookEvent::ToolStarted,
            HookEvent::Stopped,
        ] {
            view.apply_hook_event(AgentEvent::Hook(event), 0, cx);
        }
        assert!(
            notices.borrow().is_empty(),
            "foreground tabs must not notify"
        );
        view.hide();
        for (event, at) in [
            (HookEvent::PromptSubmitted, 1),
            (HookEvent::InputRequested, 2),
            (HookEvent::InputRequested, 3),
            (HookEvent::ToolStarted, 4),
            (HookEvent::Stopped, 5),
            (HookEvent::Stopped, 6),
        ] {
            view.apply_hook_event(AgentEvent::Hook(event), at, cx);
            view.on_session_event(SessionEvent::TitleChanged(Some("Agent".into())), cx);
            view.on_session_event(SessionEvent::Wakeup, cx);
        }
        // A clean exit must not repeat the already reported finished turn.
        view.on_session_event(SessionEvent::Exited(Some(0)), cx);
    });
    cx.run_until_parked();
    assert_eq!(
        *notices.borrow(),
        [AgentNotice::NeedsAttention, AgentNotice::Finished]
    );
}

#[test]
fn default_bindings_name_terminal_actions() {
    let bindings = default_bindings();
    assert_eq!(bindings.len(), 9);
    assert!(
        bindings
            .iter()
            .all(|shortcut| shortcut.action.starts_with("terminal::"))
    );
}

#[gpui::test]
fn copilot_permission_hooks_add_waiting_but_only_progress_controls_working_and_idle(
    cx: &mut gpui_kit::TestAppContext,
) {
    use agent_status::HookEvent;

    #[cfg(windows)]
    let (program, args) = ("cmd", vec!["/C", "exit 0"]);
    #[cfg(not(windows))]
    let (program, args) = ("/bin/sh", vec!["-c", "exit 0"]);
    let spec = LaunchSpec::new(
        SessionKind::CopilotCli,
        Some(program.into()),
        args.into_iter().map(String::from).collect(),
        None,
    );
    let session = Session::spawn(
        2,
        &spec,
        TermDimensions::new(80, 24),
        Palette::default(),
        Arc::new(|_| {}),
    )
    .unwrap();
    let view = cx.new(|cx| {
        TerminalView::new(
            session,
            &spec,
            StatusReporting::OutputActivity,
            Task::ready(()),
            cx,
        )
    });
    view.update(cx, |view, cx| {
        assert!(view.agent.is_none());
        view.apply_hook_event(AgentEvent::Hook(HookEvent::ToolStarted), 0, cx);
        assert!(view.agent.is_none(), "Copilot ignores activity hooks");
        view.apply_hook_event(AgentEvent::Hook(HookEvent::InputRequested), 0, cx);
        assert_eq!(
            view.agent.as_ref().unwrap().status(),
            AgentStatus::NeedsAttention
        );
        view.apply_hook_event(AgentEvent::Hook(HookEvent::Stopped), 1, cx);
        assert_eq!(
            view.agent.as_ref().unwrap().status(),
            AgentStatus::NeedsAttention
        );
        view.on_session_event(
            SessionEvent::ProgressChanged {
                state: ProgressState::Indeterminate,
                at: 2,
            },
            cx,
        );
        assert_eq!(view.agent.as_ref().unwrap().status(), AgentStatus::Working);
        view.apply_hook_event(AgentEvent::Hook(HookEvent::InputRequested), 3, cx);
        view.on_session_event(
            SessionEvent::ProgressChanged {
                state: ProgressState::Hidden,
                at: 2,
            },
            cx,
        );
        assert_eq!(
            view.agent.as_ref().unwrap().status(),
            AgentStatus::NeedsAttention,
            "queued older progress cannot clear a newer permission request"
        );
        view.on_session_event(
            SessionEvent::ProgressChanged {
                state: ProgressState::Indeterminate,
                at: 4,
            },
            cx,
        );
        assert_eq!(
            view.agent.as_ref().unwrap().status(),
            AgentStatus::Working,
            "repeated progress clears Waiting"
        );
        view.apply_progress(ProgressState::Hidden, 5, cx);
        assert_eq!(view.agent.as_ref().unwrap().status(), AgentStatus::Idle);
        view.apply_hook_event(AgentEvent::Hook(HookEvent::InputRequested), 0, cx);
        assert_eq!(
            view.agent.as_ref().unwrap().status(),
            AgentStatus::Idle,
            "a delayed permission hook cannot override completed progress"
        );
        view.on_session_event(SessionEvent::Exited(Some(0)), cx);
        view.apply_hook_event(
            AgentEvent::Hook(HookEvent::InputRequested),
            agent_status::now_micros(),
            cx,
        );
        assert_eq!(view.agent.as_ref().unwrap().status(), AgentStatus::Exited);
    });
}

#[gpui::test]
fn clipboard_writes_wait_for_an_answer(cx: &mut gpui_kit::TestAppContext) {
    use term_core::alacritty_terminal::term::ClipboardType;

    use workspace_model::{ClipboardAnswer, ClipboardGuard};

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
    let session = Session::spawn(
        1,
        &spec,
        TermDimensions::new(80, 24),
        Palette::default(),
        Arc::new(|_| {}),
    )
    .unwrap();
    let view = cx.new(|cx| TerminalView::from_test_session(session, &spec, cx));
    let clipboard_text =
        |cx: &mut gpui_kit::TestAppContext| cx.read_from_clipboard().and_then(|item| item.text());
    let store = |text: &str| SessionEvent::ClipboardStore(ClipboardType::Clipboard, text.into());

    view.update(cx, |view, cx| {
        view.dispatch_test_event(store("first"), cx);
        view.dispatch_test_event(store("second"), cx);
        assert!(view.clipboard_pending());
        assert_eq!(view.tab_status(), TabStatus::NeedsAttention);
    });
    assert_eq!(clipboard_text(cx), None, "held until allowed");
    view.update(cx, |view, cx| {
        view.answer_clipboard(ClipboardAnswer::Allow, cx)
    });
    assert_eq!(clipboard_text(cx).as_deref(), Some("second"));

    view.update(cx, |view, cx| {
        assert!(!view.clipboard_pending());
        assert_ne!(view.tab_status(), TabStatus::NeedsAttention);
        view.dispatch_test_event(store("denied"), cx);
        view.answer_clipboard(ClipboardAnswer::Deny, cx);
        assert!(!view.clipboard_pending());
    });
    assert_eq!(clipboard_text(cx).as_deref(), Some("second"));

    view.update(cx, |view, cx| {
        view.dispatch_test_event(store("always"), cx);
        view.answer_clipboard(ClipboardAnswer::Always, cx);
        view.dispatch_test_event(store("later"), cx);
        assert!(!view.clipboard_pending());
        assert!(view.clipboard_always());
    });
    assert_eq!(clipboard_text(cx).as_deref(), Some("later"));

    let carried = view.read_with(cx, |view, _| view.clipboard_carry_over());
    view.update(cx, |view, cx| {
        view.set_clipboard_guard(carried);
        view.dispatch_test_event(store("carried"), cx);
        assert!(!view.clipboard_pending());
        assert!(view.clipboard_always());
    });
    assert_eq!(clipboard_text(cx).as_deref(), Some("carried"));

    #[cfg(target_os = "linux")]
    view.update(cx, |view, cx| {
        view.set_clipboard_guard(ClipboardGuard::default());
        let selection = ClipboardType::Selection;
        view.dispatch_test_event(SessionEvent::ClipboardStore(selection, "sel".into()), cx);
        assert!(
            view.clipboard_pending(),
            "a primary selection write is held too"
        );
        view.answer_clipboard(ClipboardAnswer::Deny, cx);
        assert!(!view.clipboard_pending());
    });

    view.update(cx, |view, cx| {
        view.set_clipboard_guard(ClipboardGuard::default());
        view.dispatch_test_event(store("at exit"), cx);
        view.dispatch_test_event(SessionEvent::Exited(Some(0)), cx);
        assert!(
            !view.clipboard_pending(),
            "an exited program's write is dropped"
        );
        view.dispatch_test_event(store("after exit"), cx);
        assert!(!view.clipboard_pending());
    });
    assert_eq!(clipboard_text(cx).as_deref(), Some("carried"));
}
