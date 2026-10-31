use workspace_model::{ActivityPolicy, AgentId, HookSemantic, ProgressState};

use crate::AgentMode;

/// What an agent session is doing, as reported by its hooks or progress reports.
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

/// A status-changing hook event.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum HookEvent {
    PromptSubmitted,
    ToolStarted,
    ToolFinished,
    /// A permission dialog, question or other prompt is waiting for the user.
    InputRequested(WaitReason),
    /// The turn ended or the agent reported it is waiting for input.
    Stopped,
}

impl HookEvent {
    pub fn from_agent(agent: AgentId, name: &str, detail: Option<&str>) -> Option<Self> {
        match agent.decode_hook_event(name, detail)? {
            HookSemantic::PromptSubmitted => Some(Self::PromptSubmitted),
            HookSemantic::ToolStarted => Some(Self::ToolStarted),
            HookSemantic::ToolFinished => Some(Self::ToolFinished),
            HookSemantic::PermissionRequested => Some(Self::InputRequested(WaitReason::Permission)),
            HookSemantic::ReplyRequested => Some(Self::InputRequested(WaitReason::Reply)),
            HookSemantic::Stopped => Some(Self::Stopped),
        }
    }

    /// Apply a provider's declared automatic-approval policy to permission requests.
    pub fn for_mode(
        self,
        mode: Option<AgentMode>,
        autopilot_approves_permissions: bool,
    ) -> Option<Self> {
        if autopilot_approves_permissions
            && mode == Some(AgentMode::Autopilot)
            && self == Self::InputRequested(WaitReason::Permission)
        {
            return None;
        }
        Some(self)
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

    /// Apply a hook according to the provider's declared source of activity status.
    pub fn apply_hook(&mut self, event: HookEvent, at: u64, policy: ActivityPolicy) {
        let applies = match policy {
            ActivityPolicy::FullHooks => true,
            ActivityPolicy::ProgressWithAttentionHooks => match event {
                HookEvent::InputRequested(_) => true,
                HookEvent::ToolStarted | HookEvent::ToolFinished => {
                    self.status == AgentStatus::NeedsAttention
                }
                HookEvent::PromptSubmitted | HookEvent::Stopped => false,
            },
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
