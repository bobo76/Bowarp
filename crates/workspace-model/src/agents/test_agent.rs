use super::{
    ActivityPolicy, AgentDefinition, AgentIdentity, ArgumentSetting, BellPrecedence,
    HookArtifactKind, HookArtifactPlan, HookNamePolicy, HookProtocol, LaunchPlan, NoStatusPolicy,
    Presentation, StatusPolicy, WorkingSpinner,
};

pub(super) const DEFINITION: AgentDefinition = AgentDefinition {
    identity: AgentIdentity {
        cli_selector: "test-agent",
        saved_kind: "test-agent-kind",
        label: "Test Agent",
    },
    presentation: Presentation {
        icon_asset: "icons/agentws/agent-spark.svg",
        tint_rgb: None,
        working_spinner: WorkingSpinner::Never,
    },
    launch: LaunchPlan {
        executable: "test-agent",
        argument_setting: ArgumentSetting {
            key: "test_agent_args",
            caption: "Test Agent arguments",
            placeholder: "--example",
        },
        environment: &[],
        inherited_markers: &[],
        shell_after_normal_exit: false,
    },
    hooks: HookProtocol {
        artifact: HookArtifactPlan {
            kind: HookArtifactKind::SettingsFile,
            launch_flag: "--settings",
            resource_suffix: "-test-agent-settings.json",
            events: &["PreToolUse"],
            timeout_secs: 5,
            generate: super::claude::settings,
        },
        name_policy: HookNamePolicy::DirectOnly,
        decode: super::claude::decode_hook_event,
    },
    mode: None,
    status: StatusPolicy {
        activity: ActivityPolicy::FullHooks,
        without_status: NoStatusPolicy::Quiet,
        bell: BellPrecedence::Bell,
    },
};
