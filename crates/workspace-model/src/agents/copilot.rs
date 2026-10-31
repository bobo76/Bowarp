use std::io;
use std::path::Path;

use serde_json::json;

use super::{
    AgentDefinition, AgentId, AgentMode, ArgumentSetting, HookArtifactKind, HookArtifactPlan,
    copilot_hooks_json,
};

const HOOK_EVENTS: &[&str] = &[
    "PermissionRequest",
    "Notification",
    "PreToolUse",
    "PostToolUse",
    "PostToolUseFailure",
];
const HOOK_TIMEOUT_SECS: u64 = 5;

pub const DEFINITION: AgentDefinition = AgentDefinition {
    cli_selector: "copilot",
    saved_kind: "copilot-cli",
    executable: "copilot",
    label: "Copilot CLI",
    icon_asset: "icons/agentws/githubcopilot.svg",
    tint_rgb: None,
    argument_setting: ArgumentSetting {
        key: "copilot_args",
        caption: "Copilot CLI arguments",
        placeholder: "--model claude-sonnet-4.5",
    },
    environment: &[("ConEmuANSI", "ON")],
    inherited_markers: &[],
    activity: super::ActivityPolicy::ProgressWithAttentionHooks,
    progress_status: true,
    mode_reporting: true,
    mode_from_args,
    mode_from_report: parse_mode,
    autopilot_approves_permissions: true,
    working_spinner: true,
    normal_exit_shell_fallback: true,
    no_status_policy: super::NoStatusPolicy::Quiet,
    bell_precedence: super::BellPrecedence::Bell,
    hook_artifact: HookArtifactPlan {
        kind: HookArtifactKind::PluginDirectory,
        events: HOOK_EVENTS,
        timeout_secs: HOOK_TIMEOUT_SECS,
        generate: hooks,
    },
    decode_hook_event,
    hook_name_policy: super::HookNamePolicy::PascalCaseFallback,
    mode_event_name: Some("CopilotMode"),
    hook_launch_flag: "--plugin-dir",
    hook_program_env: Some(super::HOOK_PROGRAM_ENV),
    plugin_name: "bowarp-attention",
    resource_suffix: "-copilot",
};
pub const COPILOT: AgentId = AgentId("copilot");

fn parse_mode(value: &str) -> Option<AgentMode> {
    AgentMode::parse(value)
}

fn mode_from_args(args: &[String]) -> Option<AgentMode> {
    let mut mode = None;
    let mut plan = false;
    let mut autopilot = false;
    let mut explicit_mode = false;
    let mut args = args.iter().map(String::as_str);
    while let Some(arg) = args.next() {
        let parsed = match arg {
            "--" => break,
            "--autopilot" => {
                autopilot = true;
                None
            }
            "--plan" => {
                plan = true;
                None
            }
            "--mode" => {
                explicit_mode = true;
                args.next().and_then(parse_mode)
            }
            _ => arg.strip_prefix("--mode=").and_then(|value| {
                explicit_mode = true;
                parse_mode(value)
            }),
        };
        if parsed.is_some() {
            mode = parsed;
        }
    }
    if autopilot && (plan || explicit_mode) {
        return None;
    }
    if plan && explicit_mode && mode != Some(AgentMode::Autopilot) {
        return None;
    }
    if plan {
        Some(AgentMode::Plan)
    } else if autopilot {
        Some(AgentMode::Autopilot)
    } else {
        mode
    }
}

fn decode_hook_event(name: &str, detail: Option<&str>) -> Option<super::HookSemantic> {
    use super::HookSemantic as Semantic;

    match name {
        "PreToolUse" => Some(Semantic::ToolStarted),
        "PostToolUse" | "PostToolUseFailure" => Some(Semantic::ToolFinished),
        "PermissionRequest" => Some(Semantic::PermissionRequested),
        "Notification" => match detail? {
            "permission_prompt" => Some(Semantic::PermissionRequested),
            "elicitation_dialog" | "elicitation_url_dialog" | "agent_needs_input" => {
                Some(Semantic::ReplyRequested)
            }
            _ => None,
        },
        _ => None,
    }
}

fn hooks(hook_program: &Path, events: &[&str], timeout_secs: u64) -> io::Result<String> {
    let hook_program = hook_program.to_str().ok_or_else(|| {
        io::Error::new(
            io::ErrorKind::InvalidInput,
            format!("{} isn't valid UTF-8", hook_program.display()),
        )
    })?;
    let handler = json!([{
        "type": "command",
        "exec": hook_program,
        "timeoutSec": timeout_secs
    }]);
    Ok(
        serde_json::to_string_pretty(&copilot_hooks_json(events, handler))
            .expect("hooks serialize"),
    )
}
