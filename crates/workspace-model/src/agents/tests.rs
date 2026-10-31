use std::collections::HashSet;
use std::path::Path;

use super::*;
use crate::{SavedKind, Settings};

#[test]
fn test_provider_has_an_independent_capability_combination() {
    let agent = AgentId::from_cli_selector("test-agent").unwrap();
    let session = SessionKind::Agent(agent);

    assert!(session.has_activity_hooks());
    assert!(!session.has_progress_status());
    assert!(!session.has_mode_reporting());
    assert!(!session.has_working_spinner());
    assert!(!session.has_shell_fallback());
    assert!(!session.unavailable_without_hooks());
    assert!(!session.hooks_precede_bell());
    assert_eq!(agent.mode_from_args(&[]), None);
    assert_eq!(agent.parse_mode("interactive"), None);
    assert_eq!(agent.mode_event_name(), None);
    assert!(!agent.autopilot_approves_permissions());
}

#[test]
fn activity_policy_alone_decides_progress_and_hook_status() {
    for agent in agents() {
        let session = SessionKind::Agent(agent);
        assert_ne!(
            session.has_activity_hooks(),
            session.has_progress_status(),
            "{}",
            agent.cli_selector()
        );
    }
    assert!(SessionKind::Agent(CLAUDE).has_activity_hooks());
    assert!(SessionKind::Agent(COPILOT).has_progress_status());
    assert!(!SessionKind::Shell.has_activity_hooks());
    assert!(!SessionKind::Shell.has_progress_status());
}

#[test]
fn mode_reporting_is_declared_as_one_unit() {
    let mode = COPILOT.mode().expect("Copilot reports its mode");
    assert_eq!(COPILOT.mode_event_name(), Some(mode.event_name));
    assert_eq!(COPILOT.parse_mode("autopilot"), Some(AgentMode::Autopilot));
    assert!(COPILOT.autopilot_approves_permissions());
    assert_eq!(mode.hook_program_env, Some(HOOK_PROGRAM_ENV));
    assert!(CLAUDE.mode().is_none());
    assert_eq!(CLAUDE.parse_mode("autopilot"), None);
}

#[test]
fn test_provider_uses_shared_settings_and_saved_kind_consumers() {
    let agent = AgentId::from_cli_selector("test-agent").unwrap();
    let mut settings = Settings::default();
    settings.set_agent_arguments(agent, "--example value".into());

    assert_eq!(settings.agent_arguments(agent), "--example value");
    let json = settings.to_json().unwrap();
    assert!(json.contains(r#""test_agent_args": "--example value""#));
    assert_eq!(
        Settings::from_json(&json).unwrap().agent_arguments(agent),
        "--example value"
    );

    let saved = serde_json::to_string(&SavedKind::Agent(agent)).unwrap();
    assert_eq!(saved, r#""test-agent-kind""#);
    assert_eq!(
        serde_json::from_str::<SavedKind>(&saved).unwrap(),
        SavedKind::Agent(agent)
    );
}

/// The hooks configuration in an agent's artifact: the settings file, or the file a plugin's
/// manifest names.
fn hook_json(agent: AgentId, hook_program: &Path) -> Value {
    let files = agent.hook_artifact(hook_program).unwrap();
    let parse = |file: &ArtifactFile| serde_json::from_str::<Value>(&file.contents).unwrap();
    let hooks = match agent.hook_artifact_kind() {
        HookArtifactKind::SettingsFile => {
            assert_eq!(files.len(), 1, "a settings artifact is one file");
            &files[0]
        }
        HookArtifactKind::PluginDirectory => {
            let manifest = files
                .iter()
                .find(|file| file.name == "plugin.json")
                .expect("plugin manifest");
            let hooks_name = parse(manifest)["hooks"].as_str().unwrap().to_owned();
            files
                .iter()
                .find(|file| file.name == hooks_name)
                .expect("the hooks file the manifest names")
        }
    };
    parse(hooks)
}

#[test]
fn artifact_files_are_plain_names_inside_the_artifact_folder() {
    for agent in agents() {
        let files = agent.hook_artifact(Path::new("agentws-hook")).unwrap();
        let names: HashSet<_> = files.iter().map(|file| file.name).collect();
        assert_eq!(names.len(), files.len(), "{}", agent.cli_selector());
        for name in names {
            assert!(
                !name.is_empty() && !name.contains(['/', '\\']) && name != ".." && name != ".",
                "{name}"
            );
        }
    }
}

#[test]
fn copilot_plugin_manifest_names_the_plugin_and_its_hooks() {
    let files = COPILOT.hook_artifact(Path::new("agentws-hook")).unwrap();
    let manifest = files
        .iter()
        .find(|file| file.name == "plugin.json")
        .unwrap();
    let manifest: Value = serde_json::from_str(&manifest.contents).unwrap();
    assert_eq!(manifest["name"], "bowarp-attention");
    assert_eq!(manifest["version"], "1.0.0");
    assert_eq!(manifest["hooks"], "hooks.json");
}

#[test]
fn providers_render_their_declared_hook_artifacts() {
    let program = Path::new("/opt/my apps/agentws-hook");
    for agent in agents() {
        let json = hook_json(agent, program);
        let plan = agent.hook_artifact_plan();
        let selector = agent.cli_selector();
        assert_eq!(
            json["hooks"].as_object().unwrap().len(),
            plan.events.len(),
            "{selector}"
        );
        for event in plan.events {
            match plan.kind {
                HookArtifactKind::SettingsFile => {
                    #[cfg(unix)]
                    let expected = "'/opt/my apps/agentws-hook'";
                    #[cfg(not(unix))]
                    let expected = "\"/opt/my apps/agentws-hook\"";
                    let handler = &json["hooks"][event][0]["hooks"][0];
                    assert_eq!(handler["type"], "command", "{selector} {event}");
                    assert_eq!(handler["command"], expected, "{selector} {event}");
                    assert_eq!(handler["timeout"], plan.timeout_secs, "{selector} {event}");
                }
                HookArtifactKind::PluginDirectory => {
                    assert_eq!(json["version"], 1, "{selector}");
                    let handler = &json["hooks"][event][0];
                    assert_eq!(handler["type"], "command", "{selector} {event}");
                    assert_eq!(
                        handler["exec"],
                        program.to_str().unwrap(),
                        "{selector} {event}"
                    );
                    assert_eq!(
                        handler["timeoutSec"], plan.timeout_secs,
                        "{selector} {event}"
                    );
                    assert!(handler.get("command").is_none(), "{selector} {event}");
                }
            }
        }
    }
}

#[test]
fn copilot_hooks_exec_windows_paths_without_shell_quoting() {
    let program = Path::new(r"C:\my apps\agentws-hook.exe");
    let json = hook_json(COPILOT, program);
    assert_eq!(
        json["hooks"]["PermissionRequest"][0]["exec"],
        r"C:\my apps\agentws-hook.exe"
    );
}

#[cfg(unix)]
#[test]
fn claude_hook_command_quotes_shell_metacharacters() {
    let json = hook_json(CLAUDE, Path::new("/opt/$HOME/a\"b`c/agentws-hook"));
    assert_eq!(
        json["hooks"]["Stop"][0]["hooks"][0]["command"],
        "'/opt/$HOME/a\"b`c/agentws-hook'"
    );
}

#[cfg(unix)]
#[test]
fn hook_artifacts_reject_non_utf8_paths_without_panicking() {
    use std::os::unix::ffi::OsStrExt;
    let path = Path::new(std::ffi::OsStr::from_bytes(b"/opt/\xff/agentws-hook"));
    for agent in agents() {
        let result = std::panic::catch_unwind(|| agent.hook_artifact(path)).expect("no panic");
        assert_eq!(
            result.unwrap_err().kind(),
            io::ErrorKind::InvalidInput,
            "{}",
            agent.cli_selector()
        );
    }
}

/// The app tells instances' leftover artifacts apart by suffix alone, so no suffix may end
/// another one, and ids, saved kinds and setting keys must be unique.
#[test]
fn registry_identifiers_are_unambiguous() {
    let definitions: Vec<_> = agents().map(AgentId::definition).collect();
    let suffixes: Vec<_> = agents()
        .map(|agent| agent.hook_artifact_plan().resource_suffix)
        .collect();
    for (index, suffix) in suffixes.iter().enumerate() {
        for other in &suffixes[index + 1..] {
            assert!(
                !suffix.ends_with(other) && !other.ends_with(suffix),
                "{suffix} / {other}"
            );
        }
    }
    for field in [
        |definition: &AgentDefinition| definition.identity.cli_selector,
        |definition: &AgentDefinition| definition.identity.saved_kind,
        |definition: &AgentDefinition| definition.launch.argument_setting.key,
    ] {
        let unique: HashSet<_> = definitions
            .iter()
            .map(|definition| field(definition))
            .collect();
        assert_eq!(unique.len(), definitions.len());
    }
}

#[test]
fn agent_constants_name_their_definitions() {
    assert_eq!(CLAUDE.saved_kind(), "claude-code");
    assert_eq!(COPILOT.saved_kind(), "copilot-cli");
}
