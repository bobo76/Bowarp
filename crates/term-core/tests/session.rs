#![cfg(unix)]

use std::sync::Arc;
use std::sync::mpsc::{self, Receiver};
use std::time::{Duration, Instant};

use term_core::alacritty_terminal::index::{Column, Line, Point, Side};
use term_core::alacritty_terminal::selection::SelectionType;
use term_core::{LaunchSpec, Palette, Session, SessionEvent, SessionKind, TermDimensions};

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
    let session = Session::spawn(1, &spec, size, Palette::default(), callback).expect("spawn sh");
    (session, rx)
}

/// Collect events until the child exits, returning everything received.
fn wait_for_exit(rx: &Receiver<SessionEvent>) -> Vec<SessionEvent> {
    let deadline = Instant::now() + TIMEOUT;
    let mut events = Vec::new();
    loop {
        let remaining = deadline.saturating_duration_since(Instant::now());
        let event = rx
            .recv_timeout(remaining)
            .expect("child did not exit in time");
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
fn selection_covers_grid_text() {
    let (session, rx) = spawn_sh("printf 'hello world'", TermDimensions::new(80, 24));
    wait_for_exit(&rx);

    let start = Point::new(Line(0), Column(0));
    session.start_selection(SelectionType::Simple, start, Side::Left);
    session.update_selection(Point::new(Line(0), Column(4)), Side::Right);
    assert_eq!(session.selection_text().as_deref(), Some("hello"));
    assert_eq!(session.finish_selection().as_deref(), Some("hello"));

    session.clear_selection();
    assert_eq!(session.selection_text(), None);
}

#[test]
fn empty_selection_is_dropped_when_finished() {
    let (session, rx) = spawn_sh("printf hello", TermDimensions::new(80, 24));
    wait_for_exit(&rx);

    let point = Point::new(Line(0), Column(2));
    session.start_selection(SelectionType::Simple, point, Side::Left);
    assert_eq!(session.finish_selection(), None);
    assert_eq!(session.selection_text(), None);
}

#[test]
fn scroll_to_moves_through_history_and_clamps() {
    let (session, rx) = spawn_sh("seq 1 100", TermDimensions::new(80, 24));
    wait_for_exit(&rx);

    session.scroll_to(10);
    assert_eq!(session.display_offset(), 10);
    session.scroll_to(3);
    assert_eq!(session.display_offset(), 3);
    session.scroll_to(10_000);
    let history = 100 + 1 - 24;
    assert_eq!(session.display_offset(), history);
    session.scroll_to(0);
    assert_eq!(session.display_offset(), 0);
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

    let wakeups = events
        .iter()
        .filter(|event| **event == SessionEvent::Wakeup)
        .count();
    assert_eq!(wakeups, 1, "burst of output must produce a single wakeup");
    drop(session);
}

#[test]
fn ack_rearms_wakeup() {
    let (session, rx) = spawn_sh("printf a; read _; printf b", TermDimensions::new(80, 24));
    let deadline = Instant::now() + TIMEOUT;
    while rx
        .recv_timeout(deadline - Instant::now())
        .expect("first wakeup")
        != SessionEvent::Wakeup
    {}

    session.ack_wakeup();
    session.write(&b"\r"[..]);
    let events = wait_for_exit(&rx);

    assert!(events.contains(&SessionEvent::Wakeup));
}

#[test]
fn background_color_query_is_answered() {
    // Raw mode keeps the reply off the screen; `tr` makes ESC and BEL visible.
    let script = "stty raw -echo; printf '\\033]11;?\\007'; dd bs=1 count=24 2>/dev/null | tr '\\033\\007' 'EB'";
    let (session, rx) = spawn_sh(script, TermDimensions::new(80, 24));
    wait_for_exit(&rx);

    assert_eq!(session.visible_text()[0], "E]11;rgb:1e1e/1e1e/1e1eB");
}

#[test]
fn title_changes_are_forwarded() {
    let (_session, rx) = spawn_sh(
        "printf '\\033]0;agent title\\007'",
        TermDimensions::new(80, 24),
    );
    let events = wait_for_exit(&rx);

    assert!(events.contains(&SessionEvent::TitleChanged(Some("agent title".into()))));
}

#[cfg(target_os = "linux")]
#[test]
fn current_dir_follows_cd() {
    let target = std::env::temp_dir().canonicalize().unwrap();
    let script = format!("cd '{}' && printf ready && sleep 10", target.display());
    let (session, rx) = spawn_sh(&script, TermDimensions::new(80, 24));

    let deadline = Instant::now() + TIMEOUT;
    while session.visible_text()[0] != "ready" {
        assert!(Instant::now() < deadline, "shell did not print in time");
        let _ = rx.recv_timeout(Duration::from_millis(50));
    }
    assert_eq!(session.current_dir(), Some(target));
}

#[test]
fn dropping_kills_a_child_that_ignores_hangup() {
    let (session, rx) = spawn_sh(
        "trap '' HUP; echo $$; exec sleep 30",
        TermDimensions::new(80, 24),
    );
    let deadline = Instant::now() + TIMEOUT;
    let pid = loop {
        let _ = rx.recv_timeout(Duration::from_millis(50));
        session.ack_wakeup();
        if let Ok(pid) = session.visible_text()[0].parse::<u32>() {
            break pid;
        }
        assert!(Instant::now() < deadline, "child did not print its pid");
    };

    let started = Instant::now();
    drop(session);
    assert!(
        started.elapsed() < Duration::from_millis(500),
        "drop blocked on the child"
    );

    let deadline = Instant::now() + TIMEOUT;
    while process_exists(pid) {
        assert!(Instant::now() < deadline, "child {pid} was not killed");
        std::thread::sleep(Duration::from_millis(50));
    }
}

fn process_exists(pid: u32) -> bool {
    std::process::Command::new("kill")
        .args(["-0", &pid.to_string()])
        .status()
        .is_ok_and(|status| status.success())
}
