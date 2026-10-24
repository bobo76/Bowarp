//! Agent session status: Claude Code from hooks, Copilot CLI from progress plus permission hooks.
//! Copilot's optional extension also reports its interaction mode over the hook socket.
//!
//! Claude Code sessions are launched with settings that run `agentws-hook` on status-changing
//! events. The hook forwards each event over a local socket to the app's [`HookServer`], and
//! [`AgentState`] turns the events into an [`AgentStatus`].

mod ipc;
mod mode;
mod settings;
mod status;

pub use ipc::{
    AgentEvent, HOOK_ENV, HookMessage, HookServer, PIPE_ENV, SESSION_ENV, default_pipe_name,
    forward_hook, now_micros,
};
pub use mode::{AgentMode, AgentModeState};
pub use settings::{HOOK_EVENTS, claude_settings, copilot_hooks};
pub use status::{AgentState, AgentStatus, HookEvent};
