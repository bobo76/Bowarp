use std::collections::HashMap;
use std::io;
use std::path::Path;

use serde_json::{Value, json};

mod claude;
mod copilot;
#[cfg(test)]
mod test_agent;
#[cfg(test)]
mod tests;

pub const HOOK_PROGRAM_ENV: &str = "AGENTWS_HOOK";

pub use claude::CLAUDE;
pub use copilot::COPILOT;

/// An agent's interaction mode, independent of whether it is currently working.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AgentMode {
    Interactive,
    Plan,
    Autopilot,
}

impl AgentMode {
    pub fn parse(value: &str) -> Option<Self> {
        match value {
            "interactive" => Some(Self::Interactive),
            "plan" => Some(Self::Plan),
            "autopilot" => Some(Self::Autopilot),
            _ => None,
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            Self::Interactive => "Interactive",
            Self::Plan => "Plan",
            Self::Autopilot => "Autopilot",
        }
    }
}

fn no_mode_from_args(_: &[String]) -> Option<AgentMode> {
    None
}

fn no_mode_from_report(_: &str) -> Option<AgentMode> {
    None
}

/// A session is either a shell or a registered agent integration.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum SessionKind {
    Shell,
    Agent(AgentId),
}

impl SessionKind {
    #[allow(non_upper_case_globals)]
    pub const ClaudeCode: Self = Self::Agent(CLAUDE);
    #[allow(non_upper_case_globals)]
    pub const CopilotCli: Self = Self::Agent(COPILOT);

    pub fn agent(self) -> Option<AgentId> {
        match self {
            Self::Shell => None,
            Self::Agent(agent) => Some(agent),
        }
    }

    pub fn is_shell(self) -> bool {
        matches!(self, Self::Shell)
    }

    pub fn has_activity_hooks(self) -> bool {
        self.agent()
            .is_some_and(|agent| agent.definition().activity == ActivityPolicy::FullHooks)
    }

    pub fn has_attention_hooks(self) -> bool {
        self.agent().is_some_and(|agent| {
            agent.definition().activity == ActivityPolicy::ProgressWithAttentionHooks
        })
    }

    pub fn has_progress_status(self) -> bool {
        self.agent()
            .is_some_and(|agent| agent.definition().progress_status)
    }

    pub fn has_mode_reporting(self) -> bool {
        self.agent()
            .is_some_and(|agent| agent.definition().mode_reporting)
    }

    pub fn has_working_spinner(self) -> bool {
        self.agent()
            .is_some_and(|agent| agent.definition().working_spinner)
    }

    pub fn has_shell_fallback(self) -> bool {
        self.agent()
            .is_some_and(|agent| agent.definition().normal_exit_shell_fallback)
    }

    pub fn unavailable_without_hooks(self) -> bool {
        self.agent()
            .is_some_and(|agent| agent.definition().no_status_policy == NoStatusPolicy::Unavailable)
    }

    pub fn hooks_precede_bell(self) -> bool {
        self.agent()
            .is_some_and(|agent| agent.definition().bell_precedence == BellPrecedence::Hooks)
    }
}

/// Validated identity for one statically registered agent integration.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct AgentId(&'static str);

impl AgentId {
    pub fn from_cli_selector(selector: &str) -> Option<Self> {
        AGENTS
            .iter()
            .copied()
            .find(|agent| agent.cli_selector == selector)
            .map(|agent| Self(agent.cli_selector))
    }

    pub fn from_saved_kind(kind: &str) -> Option<Self> {
        AGENTS
            .iter()
            .copied()
            .find(|agent| agent.saved_kind == kind)
            .map(|agent| Self(agent.cli_selector))
    }

    pub fn all() -> &'static [&'static AgentDefinition] {
        AGENTS
    }

    pub fn definition(self) -> &'static AgentDefinition {
        AGENTS
            .iter()
            .copied()
            .find(|agent| agent.cli_selector == self.0)
            .expect("AgentId is created only from the static registry")
    }

    pub fn cli_selector(self) -> &'static str {
        self.0
    }

    pub fn saved_kind(self) -> &'static str {
        self.definition().saved_kind
    }

    pub fn executable(self) -> &'static str {
        self.definition().executable
    }

    pub fn label(self) -> &'static str {
        self.definition().label
    }

    pub fn argument_setting(self) -> &'static ArgumentSetting {
        &self.definition().argument_setting
    }

    pub fn inherited_markers(self) -> &'static [&'static str] {
        self.definition().inherited_markers
    }

    pub fn environment(self) -> &'static [(&'static str, &'static str)] {
        self.definition().environment
    }

    pub fn hook_artifact(self, hook_program: &Path) -> io::Result<String> {
        let artifact = self.definition().hook_artifact;
        (artifact.generate)(hook_program, artifact.events, artifact.timeout_secs)
    }

    pub fn hook_artifact_kind(self) -> HookArtifactKind {
        self.definition().hook_artifact.kind
    }

    pub fn hook_launch_flag(self) -> &'static str {
        self.definition().hook_launch_flag
    }

    pub fn mode_event_name(self) -> Option<&'static str> {
        self.definition().mode_event_name
    }

    pub fn mode_from_args(self, args: &[String]) -> Option<AgentMode> {
        (self.definition().mode_from_args)(args)
    }

    pub fn parse_mode(self, value: &str) -> Option<AgentMode> {
        (self.definition().mode_from_report)(value)
    }

    pub fn event_name(self, direct_name: Option<&str>, hook_name: Option<&str>) -> Option<String> {
        if let Some(name) = direct_name {
            return Some(name.to_owned());
        }
        match self.definition().hook_name_policy {
            HookNamePolicy::DirectOnly => None,
            HookNamePolicy::PascalCaseFallback => {
                let mut chars = hook_name?.chars();
                let first = chars.next()?;
                Some(first.to_ascii_uppercase().to_string() + chars.as_str())
            }
        }
    }

    pub fn decode_hook_event(self, name: &str, detail: Option<&str>) -> Option<HookSemantic> {
        (self.definition().decode_hook_event)(name, detail)
    }
}

/// Shared definitions consumed by launch, settings, hook, status and presentation code.
#[derive(Debug)]
pub struct AgentDefinition {
    pub cli_selector: &'static str,
    pub saved_kind: &'static str,
    pub executable: &'static str,
    pub label: &'static str,
    pub icon_asset: &'static str,
    pub tint_rgb: Option<u32>,
    pub argument_setting: ArgumentSetting,
    pub environment: &'static [(&'static str, &'static str)],
    pub inherited_markers: &'static [&'static str],
    pub activity: ActivityPolicy,
    pub progress_status: bool,
    pub mode_reporting: bool,
    pub mode_from_args: fn(&[String]) -> Option<AgentMode>,
    pub mode_from_report: fn(&str) -> Option<AgentMode>,
    pub autopilot_approves_permissions: bool,
    pub working_spinner: bool,
    pub normal_exit_shell_fallback: bool,
    pub no_status_policy: NoStatusPolicy,
    pub bell_precedence: BellPrecedence,
    pub hook_artifact: HookArtifactPlan,
    pub decode_hook_event: fn(&str, Option<&str>) -> Option<HookSemantic>,
    pub hook_name_policy: HookNamePolicy,
    pub mode_event_name: Option<&'static str>,
    pub hook_launch_flag: &'static str,
    pub hook_program_env: Option<&'static str>,
    pub plugin_name: &'static str,
    pub resource_suffix: &'static str,
}

#[derive(Clone, Copy, Debug)]
pub struct ArgumentSetting {
    pub key: &'static str,
    pub caption: &'static str,
    pub placeholder: &'static str,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ActivityPolicy {
    FullHooks,
    ProgressWithAttentionHooks,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum NoStatusPolicy {
    Quiet,
    Unavailable,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BellPrecedence {
    Hooks,
    Bell,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum HookArtifactKind {
    SettingsFile,
    PluginDirectory,
}

#[derive(Clone, Copy, Debug)]
pub struct HookArtifactPlan {
    pub kind: HookArtifactKind,
    pub events: &'static [&'static str],
    pub timeout_secs: u64,
    pub generate: fn(&Path, &[&str], u64) -> io::Result<String>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum HookNamePolicy {
    DirectOnly,
    PascalCaseFallback,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum HookSemantic {
    PromptSubmitted,
    ToolStarted,
    ToolFinished,
    PermissionRequested,
    ReplyRequested,
    Stopped,
}

static AGENTS: &[&AgentDefinition] = &[
    &claude::DEFINITION,
    &copilot::DEFINITION,
    #[cfg(test)]
    &test_agent::DEFINITION,
];

pub fn agents() -> impl Iterator<Item = AgentId> {
    AGENTS
        .iter()
        .copied()
        .map(|agent| AgentId(agent.cli_selector))
}

/// Hook configuration serialized according to the provider's declared format.
pub fn hook_artifact(agent: AgentId, hook_program: &Path) -> io::Result<String> {
    agent.hook_artifact(hook_program)
}

/// Apply provider-declared launch environment defaults.
pub fn apply_environment(agent: AgentId, environment: &mut HashMap<String, String>) {
    for (name, value) in agent.environment() {
        environment.insert((*name).into(), (*value).into());
    }
}

fn settings_json(events: &[&str], handler: Value) -> Value {
    let hooks: serde_json::Map<String, Value> = events
        .iter()
        .map(|event| ((*event).to_owned(), handler.clone()))
        .collect();
    json!({ "hooks": hooks })
}

pub(crate) fn claude_settings_json(events: &[&str], handler: Value) -> Value {
    settings_json(events, handler)
}

pub(crate) fn copilot_hooks_json(events: &[&str], handler: Value) -> Value {
    let mut value = settings_json(events, handler);
    value["version"] = Value::from(1);
    value
}
