#![cfg(unix)]

use std::sync::Arc;
use std::sync::mpsc::{self, Receiver};
use std::time::{Duration, Instant};

use term_core::{LaunchSpec, Session, SessionEvent, SessionKind, TermDimensions};

const TIMEOUT: Duration = Duration::from_secs(10);

fn spawn_sh(script: &str, size: TermDimensions) -> (Session, Receiver<SessionEvent>) {
    let spec = LaunchSpec::new(
        SessionKind::Shell,
        Some("sh".into()),
        vec!["-c".into(), script.into()],
        None,
    );
    let (tx, rx) = mpsc::channel();
    let callback = Arc::new(move |event| {
        let _ = tx.send(event);
    });
    let session = Session::spawn(1, &spec, size, callback).expect("spawn sh");
    (session, rx)
}

/// Collect events until the child exits, returning everything received.
fn wait_for_exit(rx: &Receiver<SessionEvent>) -> Vec<SessionEvent> {
    let deadline = Instant::now() + TIMEOUT;
    let mut events = Vec::new();
    loop {
        let remaining = deadline.saturating_duration_since(Instant::now());
        let event = rx.recv_timeout(remaining).expect("child did not exit in time");
        let exited = matches!(event, SessionEvent::Exited(_));
        events.push(event);
        if exited {
            return events;
        }
    }
}

#[test]
fn output_reaches_the_grid() {
    let (session, rx) = spawn_sh("printf 'hello\\nworld'", TermDimensions::new(80, 24));
    wait_for_exit(&rx);

    let text = session.visible_text();
    assert_eq!(text[0], "hello");
    assert_eq!(text[1], "world");
    assert!(session.last_output().is_some());
}

#[test]
fn exit_code_is_reported() {
    let (session, rx) = spawn_sh("exit 3", TermDimensions::new(80, 24));
    let events = wait_for_exit(&rx);

    assert_eq!(events.last(), Some(&SessionEvent::Exited(Some(3))));
    assert!(session.has_exited());
    assert_eq!(session.exit_code(), Some(3));
}

#[test]
fn resize_reaches_the_pty_and_grid() {
    let (session, rx) = spawn_sh("read _; stty size", TermDimensions::new(80, 24));
    session.resize(TermDimensions::new(100, 30));
    session.write(&b"\r"[..]);
    wait_for_exit(&rx);

    assert!(session.visible_text().iter().any(|line| line == "30 100"));
    let term = session.lock_term();
    use term_core::alacritty_terminal::grid::Dimensions;
    assert_eq!((term.columns(), term.screen_lines()), (100, 30));
}

#[test]
fn wakeups_are_coalesced_until_acknowledged() {
    let (session, rx) = spawn_sh("seq 1 5000", TermDimensions::new(80, 24));
    let events = wait_for_exit(&rx);

    let wakeups = events.iter().filter(|event| **event == SessionEvent::Wakeup).count();
    assert_eq!(wakeups, 1, "burst of output must produce a single wakeup");
    drop(session);
}

#[test]
fn ack_rearms_wakeup() {
    let (session, rx) = spawn_sh("printf a; read _; printf b", TermDimensions::new(80, 24));
    let deadline = Instant::now() + TIMEOUT;
    while rx.recv_timeout(deadline - Instant::now()).expect("first wakeup") != SessionEvent::Wakeup {}

    session.ack_wakeup();
    session.write(&b"\r"[..]);
    let events = wait_for_exit(&rx);

    assert!(events.contains(&SessionEvent::Wakeup));
}

#[test]
fn title_changes_are_forwarded() {
    let (_session, rx) = spawn_sh("printf '\\033]0;agent title\\007'", TermDimensions::new(80, 24));
    let events = wait_for_exit(&rx);

    assert!(events.contains(&SessionEvent::TitleChanged(Some("agent title".into()))));
}
