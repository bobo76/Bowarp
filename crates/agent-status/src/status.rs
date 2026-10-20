/// What an agent session is doing, as reported by its hooks (Claude Code) or its progress
/// reports (Copilot CLI).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AgentStatus {
    Idle,
    Working,
    NeedsAttention,
    Exited,
}

/// A Claude Code hook event that changes the status.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum HookEvent {
    PromptSubmitted,
    ToolStarted,
    ToolFinished,
    /// A permission dialog, question or other prompt is waiting for the user.
    InputRequested,
    /// The turn ended, or Claude Code reported it has been waiting for input.
    Stopped,
}

/// `notification_type` values that mean the user has to answer before work continues.
const ATTENTION_NOTIFICATIONS: &[&str] = &[
    "permission_prompt",
    "elicitation_dialog",
    "elicitation_url_dialog",
    "agent_needs_input",
];

impl HookEvent {
    /// Map a hook's `hook_event_name` (and `notification_type` for `Notification`) to an
    /// event. Events that do not affect the status map to `None`.
    pub fn from_hook(event_name: &str, notification_type: Option<&str>) -> Option<Self> {
        match event_name {
            "UserPromptSubmit" => Some(Self::PromptSubmitted),
            "PreToolUse" => Some(Self::ToolStarted),
            "PostToolUse" | "PostToolUseFailure" | "PermissionDenied" => Some(Self::ToolFinished),
            "PermissionRequest" => Some(Self::InputRequested),
            // `Stop` does not fire when the user interrupts a turn; the idle prompt notification
            // that follows a minute later is what returns such a session to idle.
            "Stop" | "StopFailure" => Some(Self::Stopped),
            // `SessionEnd` also fires on `/clear`, when the process keeps running; a real exit is
            // reported by the terminal session instead.
            "Notification" => match notification_type? {
                "idle_prompt" => Some(Self::Stopped),
                kind if ATTENTION_NOTIFICATIONS.contains(&kind) => Some(Self::InputRequested),
                _ => None,
            },
            _ => None,
        }
    }

    fn status(self) -> AgentStatus {
        match self {
            Self::PromptSubmitted | Self::ToolStarted | Self::ToolFinished => AgentStatus::Working,
            Self::InputRequested => AgentStatus::NeedsAttention,
            Self::Stopped => AgentStatus::Idle,
        }
    }
}

/// Status of one session. Each hook runs in its own process, so a slow hook can deliver its
/// event after a later one; events are ordered by the time the hook started instead.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct AgentState {
    status: AgentStatus,
    /// Start time of the hook whose event set `status`, in microseconds since the Unix epoch.
    last_event_at: u64,
}

impl Default for AgentState {
    fn default() -> Self {
        Self {
            status: AgentStatus::Idle,
            last_event_at: 0,
        }
    }
}

impl AgentState {
    pub fn status(&self) -> AgentStatus {
        self.status
    }

    /// Apply an event whose hook started at `at` (microseconds since the Unix epoch). Events
    /// older than the last applied one are ignored, and an exited session stays exited.
    pub fn apply(&mut self, event: HookEvent, at: u64) {
        if self.status == AgentStatus::Exited || at < self.last_event_at {
            return;
        }
        self.status = event.status();
        self.last_event_at = at;
    }

    /// Apply a progress report from the program's output, for an agent without hooks: it
    /// works while it shows progress, and is idle once the progress is removed. Reports arrive
    /// in order, and an exited session stays exited.
    pub fn apply_progress(&mut self, shown: bool) {
        if self.status == AgentStatus::Exited {
            return;
        }
        self.status = if shown {
            AgentStatus::Working
        } else {
            AgentStatus::Idle
        };
    }

    /// The process ended.
    pub fn exit(&mut self) {
        self.status = AgentStatus::Exited;
    }
}

#[cfg(test)]
mod tests {
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
        state.apply_progress(true);
        assert_eq!(state.status(), AgentStatus::Working);
        state.apply_progress(false);
        assert_eq!(state.status(), AgentStatus::Idle);
    }

    #[test]
    fn progress_after_exit_is_ignored() {
        let mut state = AgentState::default();
        state.exit();
        state.apply_progress(true);
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
}
