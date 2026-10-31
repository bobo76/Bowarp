//! Agent session status from provider-declared hooks and progress reports. Hook events are
//! forwarded over a local socket to the app's [`HookServer`], and [`AgentState`] applies them
//! according to the provider's activity policy.

// Long test scenarios are exempt from the function length limit (docs/code-quality.md).
#![cfg_attr(test, allow(clippy::too_many_lines))]

mod ipc;
mod mode;
mod settings;
mod status;

pub use ipc::{
    AGENT_ENV, AgentEvent, HookMessage, HookServer, PIPE_ENV, SESSION_ENV, default_pipe_name,
    forward_hook, now_micros,
};
pub use mode::AgentModeState;
pub use settings::hook_artifact;
pub use status::{AgentState, AgentStatus, HookEvent, WaitReason};
pub use workspace_model::{AgentId, AgentMode};
