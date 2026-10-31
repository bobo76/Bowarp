use std::io::Write;
use std::process::{Command, Output, Stdio};
use std::sync::mpsc::{self, Receiver};
use std::time::Duration;

use agent_status::{
    AGENT_ENV, AgentEvent, AgentId, AgentMode, HookEvent, HookMessage, HookServer, PIPE_ENV,
    SESSION_ENV, WaitReason, now_micros,
};

const TIMEOUT: Duration = Duration::from_secs(5);

fn start_server() -> (HookServer, Receiver<HookMessage>) {
    let pipe_name = format!("agentws-hook-test-{}.sock", now_micros());
    let (sender, receiver) = mpsc::channel();
    let server = HookServer::start(pipe_name, move |message| {
        let _ = sender.send(message);
    })
    .expect("start server");
    (server, receiver)
}

fn run_hook(env: &[(&str, &str)], input: &str) -> Output {
    run_hook_with_args(env, input, &[])
}

fn run_hook_with_args(env: &[(&str, &str)], input: &str, args: &[&str]) -> Output {
    let mut child = Command::new(env!("CARGO_BIN_EXE_agentws-hook"))
        .args(args)
        .env_remove(AGENT_ENV)
        .env_remove(PIPE_ENV)
        .env_remove(SESSION_ENV)
        .env(AGENT_ENV, "claude")
        .envs(env.iter().copied())
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("spawn hook");
    child
        .stdin
        .take()
        .unwrap()
        .write_all(input.as_bytes())
        .unwrap();
    child.wait_with_output().expect("hook output")
}

fn decoded_event(message: &HookMessage, agent_id: &str) -> Option<AgentEvent> {
    let AgentEvent::RawReport { event_name, detail } = &message.event else {
        return Some(message.event.clone());
    };
    let agent = AgentId::from_cli_selector(agent_id)?;
    if agent.mode_event_name() == Some(event_name.as_str()) {
        return agent
            .parse_mode(detail.as_deref()?)
            .map(AgentEvent::ModeChanged);
    }
    HookEvent::from_agent(agent, event_name, detail.as_deref()).map(AgentEvent::Hook)
}

fn assert_silent_success(output: &Output) {
    assert!(output.status.success(), "{output:?}");
    assert!(output.stdout.is_empty(), "{output:?}");
}

#[test]
fn forwards_event_to_server() {
    let (server, receiver) = start_server();
    let before = now_micros();
    let output = run_hook(
        &[(PIPE_ENV, server.pipe_name()), (SESSION_ENV, "12")],
        r#"{"session_id":"abc","hook_event_name":"PreToolUse","tool_name":"Bash","tool_input":{"command":"ls"}}"#,
    );
    assert_silent_success(&output);

    let message = receiver.recv_timeout(TIMEOUT).expect("message");
    assert_eq!(message.session, 12);
    assert_eq!(
        decoded_event(&message, "claude"),
        Some(AgentEvent::Hook(HookEvent::ToolStarted))
    );
    assert!(message.at >= before && message.at <= now_micros());
}

#[test]
fn forwards_copilot_mode_to_server() {
    let (server, receiver) = start_server();
    let output = run_hook_with_args(
        &[
            (AGENT_ENV, "copilot"),
            (PIPE_ENV, server.pipe_name()),
            (SESSION_ENV, "12"),
        ],
        r#"{"hook_event_name":"CopilotMode","notification_type":"autopilot"}"#,
        &["--report-errors"],
    );
    assert_silent_success(&output);
    let message = receiver.recv_timeout(TIMEOUT).expect("mode message");
    assert_eq!(message.session, 12);
    assert_eq!(
        decoded_event(&message, "copilot"),
        Some(AgentEvent::ModeChanged(AgentMode::Autopilot))
    );
}

#[test]
fn copilot_permission_request_is_silent_and_isolated_to_its_tab() {
    let (server, receiver) = start_server();
    for session in ["12", "13"] {
        let output = run_hook(
            &[
                (AGENT_ENV, "copilot"),
                (PIPE_ENV, server.pipe_name()),
                (SESSION_ENV, session),
            ],
            r#"{"hook_event_name":"PermissionRequest","session_id":"copilot-session","tool_name":"shell","tool_input":{"command":"git status"}}"#,
        );
        assert_silent_success(&output);
        assert!(output.stderr.is_empty(), "{output:?}");
        let message = receiver.recv_timeout(TIMEOUT).expect("permission message");
        assert_eq!(message.session.to_string(), session);
        assert_eq!(
            decoded_event(&message, "copilot"),
            Some(AgentEvent::Hook(HookEvent::InputRequested(
                WaitReason::Permission
            )))
        );
    }
}

#[test]
fn reports_delivery_errors_only_when_requested() {
    let output = run_hook_with_args(
        &[
            (PIPE_ENV, "agentws-hook-test-missing.sock"),
            (SESSION_ENV, "1"),
        ],
        r#"{"hook_event_name":"CopilotMode","notification_type":"autopilot"}"#,
        &["--report-errors"],
    );
    assert!(!output.status.success(), "{output:?}");
    assert!(output.stdout.is_empty(), "{output:?}");
    assert!(!output.stderr.is_empty(), "{output:?}");
}

#[test]
fn exits_quietly_without_server() {
    let output = run_hook(
        &[
            (PIPE_ENV, "agentws-hook-test-missing.sock"),
            (SESSION_ENV, "1"),
        ],
        r#"{"hook_event_name":"Stop"}"#,
    );
    assert_silent_success(&output);
}

#[test]
fn exits_quietly_outside_a_session() {
    assert_silent_success(&run_hook(&[], r#"{"hook_event_name":"Stop"}"#));
}

#[test]
fn ignores_malformed_input() {
    let (server, receiver) = start_server();
    let output = run_hook(
        &[(PIPE_ENV, server.pipe_name()), (SESSION_ENV, "1")],
        "not json",
    );
    assert_silent_success(&output);
    assert!(receiver.recv_timeout(Duration::from_millis(200)).is_err());
}
