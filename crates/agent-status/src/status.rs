use workspace_model::ProgressState;

use crate::AgentMode;

/// What an agent session is doing, as reported by its hooks (Claude Code) or its progress
/// reports (Copilot CLI).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AgentStatus {
    Idle,
    Working,
    NeedsAttention,
    Exited,
}

/// What a waiting agent asks the user for.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum WaitReason {
    /// A permission dialog: allow or deny a tool call.
    Permission,
    /// A question or other prompt the user answers.
    Reply,
}

impl WaitReason {
    /// What the attention toast says.
    pub fn label(self) -> &'static str {
        match self {
            Self::Permission => "Waiting for permission",
            Self::Reply => "Waiting for your reply",
        }
    }
}

/// A status-changing hook event (Copilot uses `InputRequested` and the tool events, see
/// [`AgentState::apply_copilot_hook`]).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum HookEvent {
    PromptSubmitted,
    ToolStarted,
    ToolFinished,
    /// A permission dialog, question or other prompt is waiting for the user.
    InputRequested(WaitReason),
    /// The turn ended, or Claude Code reported it has been waiting for input.
    Stopped,
}

/// What a `notification_type` asks the user for, if the user has to answer before work
/// continues.
fn notification_wait_reason(kind: &str) -> Option<WaitReason> {
    match kind {
        "permission_prompt" => Some(WaitReason::Permission),
        "elicitation_dialog" | "elicitation_url_dialog" | "agent_needs_input" => {
            Some(WaitReason::Reply)
        }
        _ => None,
    }
}

impl HookEvent {
    /// Autopilot approves permission requests itself, but can still ask a real question.
    pub fn for_copilot_mode(self, mode: Option<AgentMode>) -> Option<Self> {
        if mode == Some(AgentMode::Autopilot)
            && self == Self::InputRequested(WaitReason::Permission)
        {
            return None;
        }
        Some(self)
    }

    /// Map a hook's `hook_event_name` (and `notification_type` for `Notification`) to an
    /// event. Events that do not affect the status map to `None`.
    pub fn from_hook(event_name: &str, notification_type: Option<&str>) -> Option<Self> {
        match event_name {
            "UserPromptSubmit" => Some(Self::PromptSubmitted),
            "PreToolUse" => Some(Self::ToolStarted),
            "PostToolUse" | "PostToolUseFailure" | "PermissionDenied" => Some(Self::ToolFinished),
            "PermissionRequest" => Some(Self::InputRequested(WaitReason::Permission)),
            // `Stop` does not fire when the user interrupts a turn; the idle prompt notification
            // that follows a minute later is what returns such a session to idle.
            "Stop" | "StopFailure" => Some(Self::Stopped),
            // `SessionEnd` also fires on `/clear`, when the process keeps running; a real exit is
            // reported by the terminal session instead.
            "Notification" => match notification_type? {
                "idle_prompt" => Some(Self::Stopped),
                kind => notification_wait_reason(kind).map(Self::InputRequested),
            },
            _ => None,
        }
    }

    fn status(self) -> AgentStatus {
        match self {
            Self::PromptSubmitted | Self::ToolStarted | Self::ToolFinished => AgentStatus::Working,
            Self::InputRequested(_) => AgentStatus::NeedsAttention,
            Self::Stopped => AgentStatus::Idle,
        }
    }
}

/// Status of one session. Each hook runs in its own process, so a slow hook can deliver its
/// event after a later one; events are ordered by the time the hook started instead.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct AgentState {
    status: AgentStatus,
    /// What the agent waits for while `status` is `NeedsAttention`, else `None`.
    wait_reason: Option<WaitReason>,
    /// Hook start or progress observation time, in microseconds since the Unix epoch.
    last_event_at: u64,
}

impl Default for AgentState {
    fn default() -> Self {
        Self {
            status: AgentStatus::Idle,
            wait_reason: None,
            last_event_at: 0,
        }
    }
}

impl AgentState {
    pub fn status(&self) -> AgentStatus {
        self.status
    }

    /// What the agent waits for, while it needs attention.
    pub fn wait_reason(&self) -> Option<WaitReason> {
        self.wait_reason
    }

    /// Apply an event whose hook started at `at` (microseconds since the Unix epoch). Events
    /// older than the last applied one are ignored, and an exited session stays exited.
    pub fn apply(&mut self, event: HookEvent, at: u64) {
        if self.status == AgentStatus::Exited || at < self.last_event_at {
            return;
        }
        self.status = event.status();
        self.wait_reason = match event {
            HookEvent::InputRequested(reason) => Some(reason),
            _ => None,
        };
        self.last_event_at = at;
    }

    /// Apply a Copilot CLI hook. Its progress reports Working and Idle, so hooks only add
    /// Waiting, and a tool hook only ends it: the user answered and the turn moved on.
    pub fn apply_copilot_hook(&mut self, event: HookEvent, at: u64) {
        let applies = match event {
            HookEvent::InputRequested(_) => true,
            HookEvent::ToolStarted | HookEvent::ToolFinished => {
                self.status == AgentStatus::NeedsAttention
            }
            HookEvent::PromptSubmitted | HookEvent::Stopped => false,
        };
        if applies {
            self.apply(event, at);
        }
    }

    /// Apply Copilot's progress: it works while progress is shown, and is idle once removed.
    /// Copilot keeps reporting progress every 5 s while it waits for the user, so shown
    /// progress leaves Waiting in place; removed progress (the turn ended) clears it. Delayed
    /// older hooks cannot override a newer report. `at` is the I/O observation time, not the
    /// time a queued report is processed.
    pub fn apply_progress(&mut self, progress: ProgressState, at: u64) {
        if self.status == AgentStatus::Exited || at < self.last_event_at {
            return;
        }
        if self.status == AgentStatus::NeedsAttention && progress.is_shown() {
            return;
        }
        self.status = if progress.is_shown() {
            AgentStatus::Working
        } else {
            AgentStatus::Idle
        };
        self.wait_reason = None;
        self.last_event_at = at;
    }

    /// The process ended.
    pub fn exit(&mut self) {
        self.status = AgentStatus::Exited;
        self.wait_reason = None;
    }
}

#[cfg(test)]
mod tests;
