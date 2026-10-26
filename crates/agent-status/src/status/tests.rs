use super::*;

const PERMISSION: HookEvent = HookEvent::InputRequested(WaitReason::Permission);

fn run(events: &[(HookEvent, u64)]) -> AgentStatus {
    let mut state = AgentState::default();
    for &(event, at) in events {
        state.apply(event, at);
    }
    state.status()
}

#[test]
fn starts_idle() {
    assert_eq!(AgentState::default().status(), AgentStatus::Idle);
}

#[test]
fn follows_a_turn_with_a_permission_prompt() {
    use HookEvent::*;
    let mut state = AgentState::default();
    let steps = [
        (PromptSubmitted, AgentStatus::Working),
        (ToolStarted, AgentStatus::Working),
        (PERMISSION, AgentStatus::NeedsAttention),
        (ToolFinished, AgentStatus::Working),
        (Stopped, AgentStatus::Idle),
    ];
    for (at, (event, expected)) in steps.into_iter().enumerate() {
        state.apply(event, at as u64 + 1);
        assert_eq!(state.status(), expected, "after {event:?}");
    }
}

#[test]
fn late_event_does_not_override_newer_one() {
    use HookEvent::*;
    // `ToolFinished` from a slow hook arrives after `Stopped`.
    assert_eq!(
        run(&[(PromptSubmitted, 1), (Stopped, 3), (ToolFinished, 2)]),
        AgentStatus::Idle
    );
    assert_eq!(
        run(&[(ToolStarted, 1), (PERMISSION, 3), (ToolStarted, 2)]),
        AgentStatus::NeedsAttention
    );
}

#[test]
fn events_with_the_same_time_apply_in_arrival_order() {
    use HookEvent::*;
    assert_eq!(
        run(&[(ToolStarted, 5), (ToolFinished, 5), (Stopped, 5)]),
        AgentStatus::Idle
    );
}

#[test]
fn exited_is_final() {
    use HookEvent::*;
    let mut state = AgentState::default();
    state.apply(PromptSubmitted, 1);
    state.exit();
    state.apply(ToolStarted, 2);
    assert_eq!(state.status(), AgentStatus::Exited);
}

#[test]
fn progress_shown_means_working_until_removed() {
    let mut state = AgentState::default();
    state.apply_progress(ProgressState::Indeterminate, 1);
    assert_eq!(state.status(), AgentStatus::Working);
    state.apply_progress(ProgressState::Hidden, 2);
    assert_eq!(state.status(), AgentStatus::Idle);
}

#[test]
fn progress_after_exit_is_ignored() {
    let mut state = AgentState::default();
    state.exit();
    state.apply_progress(ProgressState::Indeterminate, 1);
    assert_eq!(state.status(), AgentStatus::Exited);
}

#[test]
fn copilot_permission_waits_until_progress_resumes_or_finishes() {
    for progress in [ProgressState::Indeterminate, ProgressState::Hidden] {
        let mut state = AgentState::default();
        state.apply_progress(ProgressState::Indeterminate, 1);
        state.apply(PERMISSION, 2);
        assert_eq!(state.status(), AgentStatus::NeedsAttention);
        state.apply_progress(progress, 3);
        assert_eq!(
            state.status(),
            if progress.is_shown() {
                AgentStatus::Working
            } else {
                AgentStatus::Idle
            }
        );
    }
}

#[test]
fn delayed_copilot_permission_hook_cannot_override_newer_progress() {
    let mut state = AgentState::default();
    state.apply_progress(ProgressState::Hidden, 2);
    state.apply(PERMISSION, 1);
    assert_eq!(state.status(), AgentStatus::Idle);
}

#[test]
fn queued_progress_does_not_hide_a_newer_permission_request() {
    for progress in [ProgressState::Indeterminate, ProgressState::Hidden] {
        let mut state = AgentState::default();
        state.apply_progress(progress, 1);
        state.apply(PERMISSION, 2);
        assert_eq!(state.status(), AgentStatus::NeedsAttention);

        let mut state = AgentState::default();
        state.apply(PERMISSION, 2);
        state.apply_progress(progress, 1);
        assert_eq!(state.status(), AgentStatus::NeedsAttention);
    }
}

#[test]
fn maps_hook_names() {
    let cases = [
        ("UserPromptSubmit", None, Some(HookEvent::PromptSubmitted)),
        ("PreToolUse", None, Some(HookEvent::ToolStarted)),
        ("PostToolUse", None, Some(HookEvent::ToolFinished)),
        ("PostToolUseFailure", None, Some(HookEvent::ToolFinished)),
        ("PermissionRequest", None, Some(PERMISSION)),
        ("Stop", None, Some(HookEvent::Stopped)),
        ("StopFailure", None, Some(HookEvent::Stopped)),
        ("SessionEnd", None, None),
        ("SessionStart", None, None),
    ];
    for (name, kind, expected) in cases {
        assert_eq!(HookEvent::from_hook(name, kind), expected, "{name}");
    }
}

#[test]
fn maps_notification_types() {
    let notification = |kind| HookEvent::from_hook("Notification", kind);
    assert_eq!(notification(Some("permission_prompt")), Some(PERMISSION));
    for kind in [
        "elicitation_dialog",
        "elicitation_url_dialog",
        "agent_needs_input",
    ] {
        assert_eq!(
            notification(Some(kind)),
            Some(HookEvent::InputRequested(WaitReason::Reply)),
            "{kind}"
        );
    }
    assert_eq!(notification(Some("idle_prompt")), Some(HookEvent::Stopped));
    assert_eq!(notification(Some("auth_success")), None);
    assert_eq!(notification(None), None);
}

#[test]
fn keeps_what_a_waiting_agent_asks_for_until_the_status_moves_on() {
    let reply = HookEvent::InputRequested(WaitReason::Reply);
    let mut state = AgentState::default();
    assert_eq!(state.wait_reason(), None);
    state.apply(PERMISSION, 1);
    assert_eq!(state.wait_reason(), Some(WaitReason::Permission));
    state.apply(reply, 2);
    assert_eq!(state.wait_reason(), Some(WaitReason::Reply));
    // A late hook leaves the newer request in place.
    state.apply(PERMISSION, 1);
    assert_eq!(state.wait_reason(), Some(WaitReason::Reply));
    state.apply(HookEvent::ToolFinished, 3);
    assert_eq!(state.wait_reason(), None);

    state.apply(reply, 4);
    state.apply_progress(ProgressState::Indeterminate, 5);
    assert_eq!(state.wait_reason(), None);

    state.apply(PERMISSION, 6);
    state.exit();
    assert_eq!(state.wait_reason(), None);
}

#[test]
fn wait_reasons_read_as_toast_text() {
    assert_eq!(WaitReason::Permission.label(), "Waiting for permission");
    assert_eq!(WaitReason::Reply.label(), "Waiting for your reply");
}
