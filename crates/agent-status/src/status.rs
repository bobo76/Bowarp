use workspace_model::ProgressState;

/// What an agent session is doing, as reported by its hooks (Claude Code) or its progress
/// reports (Copilot CLI).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AgentStatus {
    Idle,
    Working,
    NeedsAttention,
    Exited,
}

/// A status-changing hook event (Copilot uses only `InputRequested`).
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
    /// Hook start or progress observation time, in microseconds since the Unix epoch.
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

    /// Apply Copilot's progress: it works while progress is shown, and is idle once removed.
    /// A later progress report also clears Waiting; delayed older hooks cannot override it.
    /// `at` is the I/O observation time, not the time a queued report is processed.
    pub fn apply_progress(&mut self, progress: ProgressState, at: u64) {
        if self.status == AgentStatus::Exited || at < self.last_event_at {
            return;
        }
        self.status = if progress.is_shown() {
            AgentStatus::Working
        } else {
            AgentStatus::Idle
        };
        self.last_event_at = at;
    }

    /// The process ended.
    pub fn exit(&mut self) {
        self.status = AgentStatus::Exited;
    }
}

#[cfg(test)]
mod tests;
