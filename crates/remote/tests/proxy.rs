//! The whole path but ssh: the client runs `agentws-server proxy`, which starts the daemon.
#![cfg(unix)]

use std::process::Command;
use std::sync::Arc;
use std::thread;
use std::time::{Duration, Instant};

use agentws_remote::transport::SOCKET_ENV;
use agentws_remote::{Client, LocalView, RemoteSession, SpawnSpec};
use term_core::{Palette, TermDimensions};

const SERVER: &str = env!("CARGO_BIN_EXE_agentws-server");

fn wait_until(what: &str, mut condition: impl FnMut() -> bool) {
    let deadline = Instant::now() + Duration::from_secs(10);
    while !condition() {
        assert!(Instant::now() < deadline, "timed out waiting for {what}");
        thread::sleep(Duration::from_millis(10));
    }
}

fn proxy(socket: &str) -> Client {
    let mut command = Command::new(SERVER);
    command.arg("proxy").env(SOCKET_ENV, socket);
    Client::over_command(command, Box::new(|_| {})).unwrap()
}

fn quiet(id: u64) -> LocalView {
    LocalView {
        id,
        palette: Palette::default(),
        callback: Arc::new(|_| {}),
    }
}

fn screen_contains(remote: &RemoteSession, text: &str) -> bool {
    remote
        .session
        .visible_text()
        .iter()
        .any(|line| line.contains(text))
}

#[test]
fn sessions_survive_the_proxy_and_reattach() {
    let socket = format!(
        "agentws-test-{}-{:?}.sock",
        std::process::id(),
        Instant::now()
    );
    let size = TermDimensions::new(60, 8);
    let spec = SpawnSpec {
        program: Some("sh".into()),
        env: vec![("PS1".into(), "$ ".into()), ("ENV".into(), String::new())],
        ..SpawnSpec::default()
    };
    let id = {
        let client = proxy(&socket);
        let remote = client.spawn(spec, size, quiet(1)).unwrap();
        remote.session.write(b"echo remote-$((40 + 2))\n".to_vec());
        wait_until("the output", || screen_contains(&remote, "remote-42"));
        // Killing the proxy is losing the connection: the daemon keeps the session.
        drop(client);
        wait_until("the disconnect", || remote.session.has_exited());
        remote.info.id
    };
    let client = proxy(&socket);
    let sessions = client.list().unwrap();
    assert_eq!(
        sessions.iter().map(|info| info.id).collect::<Vec<_>>(),
        [id]
    );
    let remote = client.attach(id, size, quiet(2)).unwrap();
    assert!(screen_contains(&remote, "remote-42"));
    drop(remote);
    wait_until("the close", || client.list().unwrap().is_empty());
}

#[test]
fn a_missing_server_program_is_reported() {
    let command = Command::new("/nonexistent/agentws-server");
    assert!(Client::over_command(command, Box::new(|_| {})).is_err());
}

#[test]
fn the_ssh_error_output_is_in_the_error() {
    let mut command = Command::new("sh");
    command.args([
        "-c",
        "echo 'ssh: Could not resolve hostname nowhere' >&2; exit 255",
    ]);
    let err = Client::over_command(command, Box::new(|_| {}))
        .err()
        .unwrap();
    assert!(err.to_string().contains("Could not resolve"), "{err}");
}
