use super::*;

#[test]
fn only_a_hidden_finished_turn_is_done() {
    use AgentStatus::*;
    assert!(finished_unseen(Working, Idle, false));
    assert!(!finished_unseen(Working, Idle, true));
    assert!(!finished_unseen(NeedsAttention, Idle, false));
    assert!(!finished_unseen(Working, NeedsAttention, false));
    assert!(!finished_unseen(Working, Exited, false));
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
fn default_bindings_parse() {
    assert_eq!(default_bindings().len(), 7);
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
