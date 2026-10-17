use std::path::Path;

use serde_json::{Value, json};

/// Hook events `agentws-hook` is registered for. `Notification` is filtered by
/// `notification_type` on the app side.
pub const HOOK_EVENTS: &[&str] = &[
    "UserPromptSubmit",
    "PreToolUse",
    "PostToolUse",
    "PostToolUseFailure",
    "PermissionRequest",
    "PermissionDenied",
    "Notification",
    "Stop",
    "StopFailure",
    "SessionEnd",
];

/// Seconds Claude Code waits for the hook; it normally finishes in a few milliseconds.
const HOOK_TIMEOUT_SECS: u64 = 5;

/// Claude Code settings (for `claude --settings <file>`) that run `hook_program` on every
/// status-changing event. Claude Code adds these hooks to the user's own.
pub fn claude_settings(hook_program: &Path) -> String {
    let command = format!("\"{}\"", hook_program.display());
    let handlers = json!([{
        "hooks": [{ "type": "command", "command": command, "timeout": HOOK_TIMEOUT_SECS }]
    }]);
    let hooks: serde_json::Map<String, Value> = HOOK_EVENTS
        .iter()
        .map(|event| (event.to_string(), handlers.clone()))
        .collect();
    serde_json::to_string_pretty(&json!({ "hooks": hooks })).expect("settings serialize")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn registers_quoted_command_for_every_event() {
        let settings: Value =
            serde_json::from_str(&claude_settings(Path::new("/opt/my apps/agentws-hook"))).unwrap();
        for event in HOOK_EVENTS {
            let handler = &settings["hooks"][event][0]["hooks"][0];
            assert_eq!(handler["type"], "command", "{event}");
            assert_eq!(
                handler["command"], "\"/opt/my apps/agentws-hook\"",
                "{event}"
            );
        }
    }
}
