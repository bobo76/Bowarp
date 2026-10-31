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

/// A session is either a shell or a registered agent integration.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum SessionKind {
    Shell,
    Agent(AgentId),
}

impl SessionKind {
    pub fn agent(self) -> Option<AgentId> {
        match self {
            Self::Shell => None,
            Self::Agent(agent) => Some(agent),
        }
    }

    pub fn is_shell(self) -> bool {
        matches!(self, Self::Shell)
    }

    /// Hooks report every status change (no output-activity fallback while they run).
    pub fn has_activity_hooks(self) -> bool {
        self.agent()
            .is_some_and(|agent| agent.activity() == ActivityPolicy::FullHooks)
    }

    /// Terminal progress reports drive Working/Idle; hooks only add and end Waiting.
    pub fn has_progress_status(self) -> bool {
        self.agent()
            .is_some_and(|agent| agent.activity() == ActivityPolicy::ProgressWithAttentionHooks)
    }

    pub fn has_mode_reporting(self) -> bool {
        self.agent().is_some_and(|agent| agent.mode().is_some())
    }

    /// Whether a working tab shows the sidebar spinner; `title_hidden` is whether a custom name
    /// replaces the program's title.
    pub fn shows_working_spinner(self, title_hidden: bool) -> bool {
        self.agent().is_some_and(|agent| {
            agent
                .definition()
                .presentation
                .working_spinner
                .shows(title_hidden)
        })
    }

    pub fn has_shell_fallback(self) -> bool {
        self.agent()
            .is_some_and(|agent| agent.definition().launch.shell_after_normal_exit)
    }

    pub fn unavailable_without_hooks(self) -> bool {
        self.agent().is_some_and(|agent| {
            agent.definition().status.without_status == NoStatusPolicy::Unavailable
        })
    }

    pub fn hooks_precede_bell(self) -> bool {
        self.agent()
            .is_some_and(|agent| agent.definition().status.bell == BellPrecedence::Hooks)
    }
}

/// Validated identity for one statically registered agent integration.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct AgentId(&'static str);

impl AgentId {
    pub fn from_cli_selector(selector: &str) -> Option<Self> {
        Self::find(|identity| identity.cli_selector == selector)
    }

    pub fn from_saved_kind(kind: &str) -> Option<Self> {
        Self::find(|identity| identity.saved_kind == kind)
    }

    fn find(matches: impl Fn(&AgentIdentity) -> bool) -> Option<Self> {
        AGENTS
            .iter()
            .find(|agent| matches(&agent.identity))
            .map(|agent| Self(agent.identity.cli_selector))
    }

    pub fn definition(self) -> &'static AgentDefinition {
        AGENTS
            .iter()
            .copied()
            .find(|agent| agent.identity.cli_selector == self.0)
            .expect("AgentId is created only from the static registry")
    }

    pub fn cli_selector(self) -> &'static str {
        self.0
    }

    pub fn saved_kind(self) -> &'static str {
        self.definition().identity.saved_kind
    }

    pub fn label(self) -> &'static str {
        self.definition().identity.label
    }

    pub fn executable(self) -> &'static str {
        self.definition().launch.executable
    }

    pub fn argument_setting(self) -> &'static ArgumentSetting {
        &self.definition().launch.argument_setting
    }

    pub fn inherited_markers(self) -> &'static [&'static str] {
        self.definition().launch.inherited_markers
    }

    pub fn environment(self) -> &'static [(&'static str, &'static str)] {
        self.definition().launch.environment
    }

    pub fn activity(self) -> ActivityPolicy {
        self.definition().status.activity
    }

    /// Hook configuration serialized according to the provider's declared format.
    pub fn hook_artifact(self, hook_program: &Path) -> io::Result<Vec<ArtifactFile>> {
        let artifact = &self.definition().hooks.artifact;
        (artifact.generate)(hook_program, artifact.events, artifact.timeout_secs)
    }

    pub fn hook_artifact_plan(self) -> &'static HookArtifactPlan {
        &self.definition().hooks.artifact
    }

    pub fn hook_artifact_kind(self) -> HookArtifactKind {
        self.hook_artifact_plan().kind
    }

    pub fn event_name(self, direct_name: Option<&str>, hook_name: Option<&str>) -> Option<String> {
        self.definition()
            .hooks
            .name_policy
            .event_name(direct_name, hook_name)
    }

    pub fn decode_hook_event(self, name: &str, detail: Option<&str>) -> Option<HookSemantic> {
        (self.definition().hooks.decode)(name, detail)
    }

    pub fn mode(self) -> Option<&'static ModeReporting> {
        self.definition().mode.as_ref()
    }

    pub fn mode_event_name(self) -> Option<&'static str> {
        self.mode().map(|mode| mode.event_name)
    }

    pub fn mode_from_args(self, args: &[String]) -> Option<AgentMode> {
        (self.mode()?.from_args)(args)
    }

    pub fn parse_mode(self, value: &str) -> Option<AgentMode> {
        (self.mode()?.from_report)(value)
    }

    pub fn autopilot_approves_permissions(self) -> bool {
        self.mode()
            .is_some_and(|mode| mode.autopilot_approves_permissions)
    }
}

/// One agent integration, read by launch, settings, hook, status and presentation code.
#[derive(Debug)]
pub struct AgentDefinition {
    pub identity: AgentIdentity,
    pub presentation: Presentation,
    pub launch: LaunchPlan,
    pub hooks: HookProtocol,
    /// How the agent reports its interaction mode; `None` when it has no modes we track.
    pub mode: Option<ModeReporting>,
    pub status: StatusPolicy,
}

/// Names that identify the agent in the CLI, saved files and the UI.
#[derive(Debug)]
pub struct AgentIdentity {
    /// `agent-workspace <selector>`, the launcher id settings save and `AGENTWS_AGENT`.
    pub cli_selector: &'static str,
    /// The kind saved in `tabs.json`.
    pub saved_kind: &'static str,
    pub label: &'static str,
}

#[derive(Debug)]
pub struct Presentation {
    pub icon_asset: &'static str,
    pub tint_rgb: Option<u32>,
    /// When a working tab shows a turning spinner instead of the pulsing badge.
    pub working_spinner: WorkingSpinner,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WorkingSpinner {
    Never,
    Always,
    /// Only while a custom name hides the program's title, for an agent that animates its own
    /// title while it works (no double animation).
    WhenTitleHidden,
}

impl WorkingSpinner {
    fn shows(self, title_hidden: bool) -> bool {
        match self {
            Self::Never => false,
            Self::Always => true,
            Self::WhenTitleHidden => title_hidden,
        }
    }
}

#[derive(Debug)]
pub struct LaunchPlan {
    pub executable: &'static str,
    pub argument_setting: ArgumentSetting,
    /// Variables set in the agent's environment.
    pub environment: &'static [(&'static str, &'static str)],
    /// Variables a running agent sets on its children, removed when the app starts inside one.
    pub inherited_markers: &'static [&'static str],
    /// A clean exit replaces the tab with a shell in the same folder.
    pub shell_after_normal_exit: bool,
}

/// How the agent's hooks are installed and how their reports read.
#[derive(Debug)]
pub struct HookProtocol {
    pub artifact: HookArtifactPlan,
    pub name_policy: HookNamePolicy,
    pub decode: fn(&str, Option<&str>) -> Option<HookSemantic>,
}

#[derive(Debug)]
pub struct ModeReporting {
    /// Event name of mode reports on the hook socket; the mode is their detail.
    pub event_name: &'static str,
    /// The mode the launch arguments select, before the first report.
    pub from_args: fn(&[String]) -> Option<AgentMode>,
    pub from_report: fn(&str) -> Option<AgentMode>,
    /// Autopilot answers permission requests itself, so they don't make the tab wait.
    pub autopilot_approves_permissions: bool,
    /// Variable through which the agent's mode reporter finds `agentws-hook`.
    pub hook_program_env: Option<&'static str>,
}

#[derive(Debug)]
pub struct StatusPolicy {
    pub activity: ActivityPolicy,
    /// What the tab shows while running without any status reports.
    pub without_status: NoStatusPolicy,
    pub bell: BellPrecedence,
}

#[derive(Clone, Copy, Debug)]
pub struct ArgumentSetting {
    pub key: &'static str,
    pub caption: &'static str,
    pub placeholder: &'static str,
}

/// Where a tab's activity status comes from.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ActivityPolicy {
    /// Hooks report every status change.
    FullHooks,
    /// Terminal progress (OSC 9;4) reports Working and Idle; hooks only add and end Waiting.
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

#[derive(Debug)]
pub struct HookArtifactPlan {
    pub kind: HookArtifactKind,
    /// Flag that passes the artifact's path on the agent's command line.
    pub launch_flag: &'static str,
    /// Ends the artifact's name after `agentws-<pid>`; tells an instance's leftovers apart.
    pub resource_suffix: &'static str,
    pub events: &'static [&'static str],
    pub timeout_secs: u64,
    /// Every file of the artifact. A `SettingsFile` artifact is one file, written where the app
    /// chooses; a `PluginDirectory` artifact's files are written into a per-session folder.
    pub generate: fn(&Path, &[&str], u64) -> io::Result<Vec<ArtifactFile>>,
}

/// One file of a hook artifact, named relative to the artifact's folder.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ArtifactFile {
    pub name: &'static str,
    pub contents: String,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum HookNamePolicy {
    DirectOnly,
    PascalCaseFallback,
}

impl HookNamePolicy {
    /// The hook's event name: `direct_name` (`hook_event_name`) when present, otherwise, under
    /// `PascalCaseFallback`, `hook_name` (camelCase `hookName`) turned PascalCase.
    pub fn event_name(self, direct_name: Option<&str>, hook_name: Option<&str>) -> Option<String> {
        if let Some(name) = direct_name {
            return Some(name.to_owned());
        }
        match self {
            Self::DirectOnly => None,
            Self::PascalCaseFallback => {
                let mut chars = hook_name?.chars();
                let first = chars.next()?;
                Some(first.to_ascii_uppercase().to_string() + chars.as_str())
            }
        }
    }
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
        .map(|agent| AgentId(agent.identity.cli_selector))
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
