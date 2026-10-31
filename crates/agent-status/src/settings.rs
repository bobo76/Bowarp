use std::io;
use std::path::Path;

use workspace_model::AgentId;

/// Render the hook artifact declared by an agent integration.
pub fn hook_artifact(agent: AgentId, hook_program: &Path) -> io::Result<String> {
    workspace_model::hook_artifact(agent, hook_program)
}

#[cfg(test)]
mod tests {
    use std::path::Path;

    use serde_json::Value;
    use workspace_model::agents;
    use workspace_model::agents::{ActivityPolicy, HookArtifactKind};

    use super::*;

    #[test]
    fn providers_render_their_declared_hook_artifacts() {
        for agent in agents() {
            let artifact = hook_artifact(agent, Path::new("agentws-hook")).unwrap();
            let json: Value = serde_json::from_str(&artifact).unwrap();
            let definition = agent.definition();
            match definition.hook_artifact.kind {
                HookArtifactKind::SettingsFile => {
                    #[cfg(unix)]
                    let expected = "'agentws-hook'";
                    #[cfg(not(unix))]
                    let expected = "\"agentws-hook\"";
                    for event in definition.hook_artifact.events {
                        assert_eq!(
                            json["hooks"][event][0]["hooks"][0]["command"],
                            expected,
                            "{}",
                            agent.cli_selector()
                        );
                    }
                    assert_eq!(definition.activity, ActivityPolicy::FullHooks);
                }
                HookArtifactKind::PluginDirectory => {
                    assert_eq!(json["version"], 1);
                    for event in definition.hook_artifact.events {
                        assert_eq!(
                            json["hooks"][event][0]["exec"],
                            "agentws-hook",
                            "{}",
                            agent.cli_selector()
                        );
                    }
                    assert_eq!(
                        definition.activity,
                        ActivityPolicy::ProgressWithAttentionHooks
                    );
                }
            }
        }
    }

    #[cfg(unix)]
    #[test]
    fn claude_hook_command_quotes_shell_metacharacters() {
        let agent = AgentId::from_cli_selector("claude").unwrap();
        let artifact = hook_artifact(agent, Path::new("/opt/$HOME/a\"b`c/agentws-hook")).unwrap();
        let json: Value = serde_json::from_str(&artifact).unwrap();
        assert_eq!(
            json["hooks"]["Stop"][0]["hooks"][0]["command"],
            "'/opt/$HOME/a\"b`c/agentws-hook'"
        );
    }

    #[cfg(unix)]
    #[test]
    fn hook_artifact_rejects_non_utf8_executable_paths() {
        use std::os::unix::ffi::OsStrExt;
        let path = Path::new(std::ffi::OsStr::from_bytes(b"/opt/\xff/agentws-hook"));
        let copilot = AgentId::from_cli_selector("copilot").unwrap();
        assert_eq!(
            hook_artifact(copilot, path).unwrap_err().kind(),
            io::ErrorKind::InvalidInput
        );
    }
}
