use std::io;
use std::path::Path;

use serde_json::json;

use super::{
    AgentDefinition, AgentId, ArgumentSetting, HookArtifactKind, HookArtifactPlan,
    claude_settings_json,
};

const HOOK_EVENTS: &[&str] = &[
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
const HOOK_TIMEOUT_SECS: u64 = 5;

pub const DEFINITION: AgentDefinition = AgentDefinition {
    cli_selector: "claude",
    saved_kind: "claude-code",
    executable: "claude",
    label: "Claude Code",
    icon_asset: "icons/agentws/agent-spark.svg",
    tint_rgb: Some(0xd97757),
    argument_setting: ArgumentSetting {
        key: "claude_args",
        caption: "Claude Code arguments",
        placeholder: "--model sonnet",
    },
    environment: &[],
    inherited_markers: &[
        "AI_AGENT",
        "CLAUDECODE",
        "CLAUDE_CODE_CHILD_SESSION",
        "CLAUDE_CODE_ENTRYPOINT",
        "CLAUDE_CODE_EXECPATH",
        "CLAUDE_CODE_MESSAGING_SOCKET",
        "CLAUDE_CODE_MESSAGING_TOKEN",
        "CLAUDE_CODE_SESSION_ATTENDED",
        "CLAUDE_CODE_SESSION_ID",
        "CLAUDE_CODE_VERSION",
        "CLAUDE_EFFORT",
        "CLAUDE_PID",
    ],
    activity: super::ActivityPolicy::FullHooks,
    progress_status: false,
    mode_reporting: false,
    mode_from_args: super::no_mode_from_args,
    mode_from_report: super::no_mode_from_report,
    autopilot_approves_permissions: false,
    working_spinner: false,
    normal_exit_shell_fallback: true,
    no_status_policy: super::NoStatusPolicy::Unavailable,
    bell_precedence: super::BellPrecedence::Hooks,
    hook_artifact: HookArtifactPlan {
        kind: HookArtifactKind::SettingsFile,
        events: HOOK_EVENTS,
        timeout_secs: HOOK_TIMEOUT_SECS,
        generate: settings,
    },
    decode_hook_event,
    hook_name_policy: super::HookNamePolicy::DirectOnly,
    mode_event_name: None,
    hook_launch_flag: "--settings",
    hook_program_env: None,
    plugin_name: "",
    resource_suffix: "-claude-settings.json",
};
pub const CLAUDE: AgentId = AgentId("claude");

pub(super) fn decode_hook_event(name: &str, detail: Option<&str>) -> Option<super::HookSemantic> {
    use super::HookSemantic as Semantic;

    match name {
        "UserPromptSubmit" => Some(Semantic::PromptSubmitted),
        "PreToolUse" => Some(Semantic::ToolStarted),
        "PostToolUse" | "PostToolUseFailure" | "PermissionDenied" => Some(Semantic::ToolFinished),
        "PermissionRequest" => Some(Semantic::PermissionRequested),
        "Stop" | "StopFailure" => Some(Semantic::Stopped),
        "Notification" => match detail? {
            "idle_prompt" => Some(Semantic::Stopped),
            "permission_prompt" => Some(Semantic::PermissionRequested),
            "elicitation_dialog" | "elicitation_url_dialog" | "agent_needs_input" => {
                Some(Semantic::ReplyRequested)
            }
            _ => None,
        },
        _ => None,
    }
}

pub(super) fn settings(
    hook_program: &Path,
    events: &[&str],
    timeout_secs: u64,
) -> io::Result<String> {
    let command = hook_command(hook_program)?;
    let handler = json!([{
        "hooks": [{ "type": "command", "command": command, "timeout": timeout_secs }]
    }]);
    Ok(
        serde_json::to_string_pretty(&claude_settings_json(events, handler))
            .expect("settings serialize"),
    )
}

#[cfg(unix)]
fn hook_command(hook_program: &Path) -> io::Result<String> {
    use crate::file_tree::{ShellSyntax, quote_path};

    quote_path(hook_program, ShellSyntax::Posix)
        .map_err(|error| io::Error::new(io::ErrorKind::InvalidInput, error.to_string()))
}

#[cfg(not(unix))]
fn hook_command(hook_program: &Path) -> io::Result<String> {
    Ok(format!("\"{}\"", utf8(hook_program)?))
}

#[cfg(not(unix))]
fn utf8(path: &Path) -> io::Result<&str> {
    path.to_str().ok_or_else(|| {
        io::Error::new(
            io::ErrorKind::InvalidInput,
            format!("{} isn't valid UTF-8", path.display()),
        )
    })
}
