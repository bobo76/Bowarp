//! Invoked by Claude Code hooks: forwards the hook's JSON input to the Agent Workspace session
//! that started Claude Code. Always exits 0 and prints nothing, so it never affects Claude Code;
//! outside an Agent Workspace session it does nothing.

use std::io::{self, Read};

use agent_status::{PIPE_ENV, SESSION_ENV, forward_hook, now_micros};

fn main() {
    let at = now_micros();
    // Read the input even when there is nothing to forward: exiting first would make the
    // writer's still-pending write fail with a broken pipe.
    let mut input = Vec::new();
    if io::stdin().read_to_end(&mut input).is_err() {
        return;
    }
    let Ok(pipe_name) = std::env::var(PIPE_ENV) else {
        return;
    };
    let Some(session) = std::env::var(SESSION_ENV)
        .ok()
        .and_then(|id| id.parse().ok())
    else {
        return;
    };
    // The app may have quit or the payload may be unexpected: there is nobody to report to.
    let _ = forward_hook(&pipe_name, session, at, &input);
}
