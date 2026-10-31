use super::*;
use crate::{SavedKind, Settings};

#[test]
fn test_provider_has_an_independent_capability_combination() {
    let agent = AgentId::from_cli_selector("test-agent").unwrap();
    let session = SessionKind::Agent(agent);

    assert!(session.has_activity_hooks());
    assert!(!session.has_attention_hooks());
    assert!(session.has_progress_status());
    assert!(!session.has_mode_reporting());
    assert!(!session.has_working_spinner());
    assert!(!session.has_shell_fallback());
    assert!(!session.unavailable_without_hooks());
    assert!(!session.hooks_precede_bell());
    assert_eq!(agent.mode_from_args(&[]), None);
    assert_eq!(agent.parse_mode("interactive"), None);
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
