use std::io;
use std::path::Path;

use serde_json::json;

use super::{
    ActivityPolicy, AgentDefinition, AgentId, AgentIdentity, ArgumentSetting, ArtifactFile,
    BellPrecedence, HookArtifactKind, HookArtifactPlan, HookNamePolicy, HookProtocol, LaunchPlan,
    NoStatusPolicy, Presentation, StatusPolicy, SubagentHooks, WorkingSpinner,
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
    "SubagentStart",
    "SubagentStop",
];
const HOOK_TIMEOUT_SECS: u64 = 5;

pub const DEFINITION: AgentDefinition = AgentDefinition {
    identity: AgentIdentity {
        cli_selector: "claude",
        saved_kind: "claude-code",
        label: "Claude Code",
    },
    presentation: Presentation {
        icon_asset: "icons/agentws/agent-spark.svg",
        tint_rgb: Some(0xd97757),
        working_spinner: WorkingSpinner::WhenTitleHidden,
    },
    launch: LaunchPlan {
        executable: "claude",
        argument_setting: ArgumentSetting {
            key: "claude_args",
            caption: "Claude Code arguments",
            placeholder: "--model opus",
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
        shell_after_normal_exit: true,
    },
    hooks: HookProtocol {
        artifact: HookArtifactPlan {
            kind: HookArtifactKind::SettingsFile,
            launch_flag: "--settings",
            shell_env: "AGENTWS_CLAUDE_SETTINGS",
            resource_suffix: "-claude-settings.json",
            events: HOOK_EVENTS,
            timeout_secs: HOOK_TIMEOUT_SECS,
            generate: settings,
        },
        name_policy: HookNamePolicy::DirectOnly,
        decode: decode_hook_event,
        subagents: SubagentHooks {
            spawn_tools: &["Agent", "Task"],
            unnamed_start: false,
        },
    },
    mode: None,
    status: StatusPolicy {
        activity: ActivityPolicy::FullHooks,
        without_status: NoStatusPolicy::Unavailable,
        bell: BellPrecedence::Hooks,
    },
};
pub const CLAUDE: AgentId = AgentId(DEFINITION.identity.cli_selector);

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
) -> io::Result<Vec<ArtifactFile>> {
    let command = hook_command(hook_program)?;
    let handler = json!([{
        "hooks": [{ "type": "command", "command": command, "timeout": timeout_secs }]
    }]);
    let contents = serde_json::to_string_pretty(&claude_settings_json(events, handler))
        .expect("settings serialize");
    Ok(vec![ArtifactFile {
        name: "settings.json",
        contents,
    }])
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
