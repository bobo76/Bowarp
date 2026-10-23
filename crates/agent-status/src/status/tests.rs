use super::*;

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
        (InputRequested, AgentStatus::NeedsAttention),
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
        run(&[(ToolStarted, 1), (InputRequested, 3), (ToolStarted, 2)]),
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
    state.apply_progress(ProgressState::Indeterminate);
    assert_eq!(state.status(), AgentStatus::Working);
    state.apply_progress(ProgressState::Hidden);
    assert_eq!(state.status(), AgentStatus::Idle);
}

#[test]
fn progress_after_exit_is_ignored() {
    let mut state = AgentState::default();
    state.exit();
    state.apply_progress(ProgressState::Indeterminate);
    assert_eq!(state.status(), AgentStatus::Exited);
}

#[test]
fn maps_hook_names() {
    let cases = [
        ("UserPromptSubmit", None, Some(HookEvent::PromptSubmitted)),
        ("PreToolUse", None, Some(HookEvent::ToolStarted)),
        ("PostToolUse", None, Some(HookEvent::ToolFinished)),
        ("PostToolUseFailure", None, Some(HookEvent::ToolFinished)),
        ("PermissionRequest", None, Some(HookEvent::InputRequested)),
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
    assert_eq!(
        notification(Some("permission_prompt")),
        Some(HookEvent::InputRequested)
    );
    assert_eq!(
        notification(Some("elicitation_dialog")),
        Some(HookEvent::InputRequested)
    );
    assert_eq!(notification(Some("idle_prompt")), Some(HookEvent::Stopped));
    assert_eq!(notification(Some("auth_success")), None);
    assert_eq!(notification(None), None);
}
