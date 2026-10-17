//! Claude Code session status from hooks.
//!
//! Claude Code sessions are launched with settings that run `agentws-hook` on status-changing
//! events. The hook forwards each event over a local socket to the app's [`HookServer`], and
//! [`AgentState`] turns the events into an [`AgentStatus`].

mod ipc;
mod settings;
mod status;

pub use ipc::{
    HookMessage, HookServer, PIPE_ENV, SESSION_ENV, default_pipe_name, forward_hook, now_micros,
};
pub use settings::{HOOK_EVENTS, claude_settings};
pub use status::{AgentState, AgentStatus, HookEvent};
