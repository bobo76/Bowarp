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
