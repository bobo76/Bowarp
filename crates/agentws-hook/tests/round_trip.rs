use std::io::Write;
use std::process::{Command, Output, Stdio};
use std::sync::mpsc::{self, Receiver};
use std::time::Duration;

use agent_status::{HookEvent, HookMessage, HookServer, PIPE_ENV, SESSION_ENV, now_micros};

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
    let mut child = Command::new(env!("CARGO_BIN_EXE_agentws-hook"))
        .env_remove(PIPE_ENV)
        .env_remove(SESSION_ENV)
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
    assert_eq!(message.event, HookEvent::ToolStarted);
    assert!(message.at >= before && message.at <= now_micros());
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
