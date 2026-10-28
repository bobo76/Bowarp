use std::io;
use std::path::Path;

use serde_json::{Value, json};
#[cfg(unix)]
use workspace_model::file_tree::{ShellSyntax, quote_path};

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
];

/// Hook events the Copilot plugin registers: its terminal progress reports Working and Idle, so
/// hooks only add Waiting (permission requests, questions) and the tool hooks that end it.
pub const COPILOT_HOOK_EVENTS: &[&str] = &[
    "PermissionRequest",
    "Notification",
    "PreToolUse",
    "PostToolUse",
    "PostToolUseFailure",
];

/// Seconds Claude Code waits for the hook; it normally finishes in a few milliseconds.
const HOOK_TIMEOUT_SECS: u64 = 5;

/// Claude Code settings (for `claude --settings <file>`) that run `hook_program` on every
/// status-changing event. Claude Code adds these hooks to the user's own. Fails for a path
/// that isn't UTF-8 or can't be quoted for the shell that runs the hook.
pub fn claude_settings(hook_program: &Path) -> io::Result<String> {
    let command = hook_command(hook_program)?;
    let handlers = json!([{
        "hooks": [{ "type": "command", "command": command, "timeout": HOOK_TIMEOUT_SECS }]
    }]);
    let hooks: serde_json::Map<String, Value> = HOOK_EVENTS
        .iter()
        .map(|event| (event.to_string(), handlers.clone()))
        .collect();
    Ok(serde_json::to_string_pretty(&json!({ "hooks": hooks })).expect("settings serialize"))
}

/// The hook command line: Claude Code runs it with `sh -c` on Unix, so the path is single
/// quoted there (double quotes would expand `$` and backticks).
#[cfg(unix)]
fn hook_command(hook_program: &Path) -> io::Result<String> {
    quote_path(hook_program, ShellSyntax::Posix)
        .map_err(|error| io::Error::new(io::ErrorKind::InvalidInput, error.to_string()))
}

/// Windows paths can't hold `"`, so double quotes keep the path one word.
#[cfg(not(unix))]
fn hook_command(hook_program: &Path) -> io::Result<String> {
    Ok(format!("\"{}\"", utf8(hook_program)?))
}

/// Serializing a `Path` that isn't UTF-8 fails (and `json!` would panic on it).
fn utf8(path: &Path) -> io::Result<&str> {
    path.to_str().ok_or_else(|| {
        io::Error::new(
            io::ErrorKind::InvalidInput,
            format!("{} isn't valid UTF-8", path.display()),
        )
    })
}

/// Copilot CLI plugin hooks (checked on 1.0.105). PascalCase selects the snake_case stdin
/// payload understood by `agentws-hook`, except for `PermissionRequest`, which Copilot sends
/// camelCase (see `hook_line`); `exec` avoids shell quoting on either platform. Fails for a path
/// that isn't UTF-8.
pub fn copilot_hooks(hook_program: &Path) -> io::Result<String> {
    let hook_program = utf8(hook_program)?;
    let handlers = json!([{
        "type": "command",
        "exec": hook_program,
        "timeoutSec": HOOK_TIMEOUT_SECS
    }]);
    let hooks: serde_json::Map<String, Value> = COPILOT_HOOK_EVENTS
        .iter()
        .map(|event| (event.to_string(), handlers.clone()))
        .collect();
    Ok(
        serde_json::to_string_pretty(&json!({ "version": 1, "hooks": hooks }))
            .expect("hooks serialize"),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn registers_quoted_command_for_every_event() {
        let settings: Value =
            serde_json::from_str(&claude_settings(Path::new("/opt/my apps/agentws-hook")).unwrap())
                .unwrap();
        #[cfg(unix)]
        let expected = "'/opt/my apps/agentws-hook'";
        #[cfg(not(unix))]
        let expected = "\"/opt/my apps/agentws-hook\"";
        for event in HOOK_EVENTS {
            let handler = &settings["hooks"][event][0]["hooks"][0];
            assert_eq!(handler["type"], "command", "{event}");
            assert_eq!(handler["command"], expected, "{event}");
        }
    }

    #[cfg(unix)]
    #[test]
    fn shell_characters_in_the_hook_path_stay_literal() {
        let settings: Value = serde_json::from_str(
            &claude_settings(Path::new("/opt/$HOME/a\"b`c/agentws-hook")).unwrap(),
        )
        .unwrap();
        assert_eq!(
            settings["hooks"]["Stop"][0]["hooks"][0]["command"],
            "'/opt/$HOME/a\"b`c/agentws-hook'"
        );
    }

    #[cfg(unix)]
    #[test]
    fn non_utf8_hook_path_is_refused_without_panicking() {
        use std::os::unix::ffi::OsStrExt;
        let program = Path::new(std::ffi::OsStr::from_bytes(b"/opt/\xff/agentws-hook"));
        let copilot = std::panic::catch_unwind(|| copilot_hooks(program)).expect("no panic");
        assert_eq!(copilot.unwrap_err().kind(), io::ErrorKind::InvalidInput);
        assert_eq!(
            claude_settings(program).unwrap_err().kind(),
            io::ErrorKind::InvalidInput
        );
    }

    #[test]
    fn copilot_registers_waiting_and_tool_events_without_shell_quoting() {
        let program = Path::new(r"C:\my apps\agentws-hook.exe");
        let settings: Value = serde_json::from_str(&copilot_hooks(program).unwrap()).unwrap();
        assert_eq!(settings["version"], 1);
        assert_eq!(
            settings["hooks"].as_object().unwrap().len(),
            COPILOT_HOOK_EVENTS.len()
        );
        for event in COPILOT_HOOK_EVENTS {
            let handler = &settings["hooks"][event][0];
            assert_eq!(handler["type"], "command", "{event}");
            assert_eq!(handler["exec"], program.to_str().unwrap(), "{event}");
            assert_eq!(handler["timeoutSec"], HOOK_TIMEOUT_SECS, "{event}");
            assert!(handler.get("command").is_none(), "{event}");
        }
    }
}
