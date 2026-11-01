use std::io;
use std::path::Path;

use serde_json::json;

use super::{
    ActivityPolicy, AgentDefinition, AgentId, AgentIdentity, AgentMode, ArgumentSetting,
    ArtifactFile, BellPrecedence, HOOK_PROGRAM_ENV, HookArtifactKind, HookArtifactPlan,
    HookNamePolicy, HookProtocol, LaunchPlan, ModeReporting, NoStatusPolicy, Presentation,
    StatusPolicy, WorkingSpinner, copilot_hooks_json,
};

const HOOK_EVENTS: &[&str] = &[
    "PermissionRequest",
    "Notification",
    "PreToolUse",
    "PostToolUse",
    "PostToolUseFailure",
];
const HOOK_TIMEOUT_SECS: u64 = 5;
/// Name of the per-session Copilot plugin carrying the hooks.
const PLUGIN_NAME: &str = "bowarp-attention";
const HOOKS_FILE: &str = "hooks.json";

pub const DEFINITION: AgentDefinition = AgentDefinition {
    identity: AgentIdentity {
        cli_selector: "copilot",
        saved_kind: "copilot-cli",
        label: "Copilot CLI",
    },
    presentation: Presentation {
        icon_asset: "icons/agentws/githubcopilot.svg",
        tint_rgb: None,
        working_spinner: WorkingSpinner::Always,
    },
    launch: LaunchPlan {
        executable: "copilot",
        argument_setting: ArgumentSetting {
            key: "copilot_args",
            caption: "Copilot CLI arguments",
            placeholder: "--yolo",
        },
        // Copilot CLI only reports progress (OSC 9;4) to terminals it recognizes; ConEmu's
        // variable is the narrowest one that turns it on (checked with Copilot CLI 1.0.98).
        environment: &[("ConEmuANSI", "ON")],
        inherited_markers: &[],
        shell_after_normal_exit: true,
    },
    hooks: HookProtocol {
        artifact: HookArtifactPlan {
            kind: HookArtifactKind::PluginDirectory,
            launch_flag: "--plugin-dir",
            shell_env: "AGENTWS_COPILOT_PLUGIN",
            resource_suffix: "-copilot",
            events: HOOK_EVENTS,
            timeout_secs: HOOK_TIMEOUT_SECS,
            generate: hooks,
        },
        name_policy: HookNamePolicy::PascalCaseFallback,
        decode: decode_hook_event,
    },
    mode: Some(ModeReporting {
        event_name: "CopilotMode",
        from_args: mode_from_args,
        from_report: AgentMode::parse,
        autopilot_approves_permissions: true,
        hook_program_env: Some(HOOK_PROGRAM_ENV),
    }),
    status: StatusPolicy {
        activity: ActivityPolicy::ProgressWithAttentionHooks,
        without_status: NoStatusPolicy::Quiet,
        bell: BellPrecedence::Bell,
    },
};
pub const COPILOT: AgentId = AgentId(DEFINITION.identity.cli_selector);

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
                args.next().and_then(AgentMode::parse)
            }
            _ => arg.strip_prefix("--mode=").and_then(|value| {
                explicit_mode = true;
                AgentMode::parse(value)
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

/// A Copilot CLI plugin (`--plugin-dir`): its manifest and the hooks it names.
fn hooks(hook_program: &Path, events: &[&str], timeout_secs: u64) -> io::Result<Vec<ArtifactFile>> {
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
    let manifest = json!({ "name": PLUGIN_NAME, "version": "1.0.0", "hooks": HOOKS_FILE });
    Ok(vec![
        ArtifactFile {
            name: "plugin.json",
            contents: manifest.to_string(),
        },
        ArtifactFile {
            name: HOOKS_FILE,
            contents: serde_json::to_string_pretty(&copilot_hooks_json(events, handler))
                .expect("hooks serialize"),
        },
    ])
}
