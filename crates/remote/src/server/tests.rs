//! A server and clients in this process, over socket pairs.
#![cfg(unix)]

use std::os::unix::net::UnixStream;
use std::sync::mpsc::Receiver;

use super::*;
use crate::client::{Client, RemoteSession};

const TIMEOUT: Duration = Duration::from_secs(10);

fn wait_until(what: &str, mut condition: impl FnMut() -> bool) {
    let deadline = Instant::now() + TIMEOUT;
    while !condition() {
        assert!(Instant::now() < deadline, "timed out waiting for {what}");
        thread::sleep(Duration::from_millis(10));
    }
}

/// A client connected to `server`, and a receiver of its disconnect reason.
fn connect(server: &Server) -> (Client, Receiver<String>) {
    let (client_end, server_end) = UnixStream::pair().unwrap();
    let server = server.clone();
    thread::spawn(move || {
        let reader = server_end.try_clone().unwrap();
        server.serve_connection(reader, server_end);
    });
    let (disconnected, reason) = mpsc::channel();
    let client = Client::connect(
        client_end.try_clone().unwrap(),
        client_end,
        Box::new(move |why| drop(disconnected.send(why))),
    )
    .unwrap();
    (client, reason)
}

fn shell(script: Option<&str>) -> SpawnSpec {
    SpawnSpec {
        program: Some("sh".into()),
        args: script
            .map(|script| vec!["-c".into(), script.into()])
            .unwrap_or_default(),
        // A plain prompt, whatever the user's rc files say.
        env: vec![("PS1".into(), "$ ".into()), ("ENV".into(), String::new())],
        ..SpawnSpec::default()
    }
}

fn size(columns: u16, lines: u16) -> TermDimensions {
    TermDimensions::new(columns, lines)
}

fn screen_contains(session: &RemoteSession, text: &str) -> bool {
    session
        .mirror()
        .visible_text()
        .iter()
        .any(|line| line.contains(text))
}

fn events() -> (EventCallbackRecorder, Arc<Mutex<Vec<SessionEvent>>>) {
    let seen = Arc::new(Mutex::new(Vec::new()));
    let recorder = seen.clone();
    (
        Arc::new(move |event| recorder.lock().unwrap().push(event)),
        seen,
    )
}

type EventCallbackRecorder = term_core::EventCallback;

#[test]
fn a_spawned_shell_runs_what_is_typed() {
    let server = Server::new();
    let (client, _) = connect(&server);
    let (callback, seen) = events();
    let session = client.spawn(shell(None), size(40, 6), callback).unwrap();
    session.write(b"echo $((6 * 7))\n".to_vec());
    wait_until("the answer", || screen_contains(&session, "42"));
    assert!(seen.lock().unwrap().contains(&SessionEvent::Wakeup));
    assert!(!session.has_exited());
}

#[test]
fn a_session_outlives_its_client_and_reattaches_with_its_screen() {
    let server = Server::new();
    let id = {
        let (client, _) = connect(&server);
        let session = client
            .spawn(shell(None), size(40, 6), Arc::new(|_| {}))
            .unwrap();
        session.write(b"echo before-$((1 + 1))\n".to_vec());
        wait_until("the output", || screen_contains(&session, "before-2"));
        session.id()
        // The client and the session are dropped: detached, still running.
    };
    let (client, _) = connect(&server);
    let sessions = client.list().unwrap();
    assert_eq!(sessions.len(), 1);
    assert_eq!(sessions[0].id, id);
    assert_eq!(sessions[0].exit, None);
    let session = client.attach(id, size(50, 8), Arc::new(|_| {})).unwrap();
    // The snapshot shows what was there before.
    assert!(screen_contains(&session, "before-2"));
    assert_eq!(session.mirror().size(), size(50, 8));
    session.write(b"echo after-$((2 + 2))\n".to_vec());
    wait_until("new output", || screen_contains(&session, "after-4"));
}

#[test]
fn an_exit_is_reported_to_attached_clients_and_late_ones() {
    let server = Server::new();
    let (client, _) = connect(&server);
    let (callback, seen) = events();
    let session = client
        .spawn(shell(Some("echo bye; exit 3")), size(30, 4), callback)
        .unwrap();
    wait_until("the exit", || session.has_exited());
    assert_eq!(session.exit_code(), Some(3));
    assert!(screen_contains(&session, "bye"));
    assert!(
        seen.lock()
            .unwrap()
            .contains(&SessionEvent::Exited(Some(3)))
    );
    let id = session.id();
    drop(session);
    // An exited session stays listed until it is closed.
    assert_eq!(client.list().unwrap()[0].exit, Some(Some(3)));
    let again = client.attach(id, size(30, 4), Arc::new(|_| {})).unwrap();
    wait_until("the exit on attach", || again.has_exited());
    assert!(screen_contains(&again, "bye"));
    again.close();
    wait_until("the session to leave the list", || {
        client.list().unwrap().is_empty()
    });
}

#[test]
fn clients_follow_each_others_resizes_and_closes() {
    let server = Server::new();
    let (first, _) = connect(&server);
    let (second, _) = connect(&server);
    let one = first
        .spawn(shell(None), size(40, 6), Arc::new(|_| {}))
        .unwrap();
    let (callback, seen) = events();
    let two = second.attach(one.id(), size(40, 6), callback).unwrap();
    one.write(b"echo shared\n".to_vec());
    wait_until("output on the other client", || {
        screen_contains(&two, "shared")
    });
    one.resize(size(60, 10));
    wait_until("the resize", || two.mirror().size() == size(60, 10));
    one.close();
    wait_until("the close", || two.has_exited());
    assert!(seen.lock().unwrap().contains(&SessionEvent::Exited(None)));
    assert!(second.list().unwrap().is_empty());
}

#[test]
fn bad_requests_fail_without_ending_the_connection() {
    let server = Server::new();
    let (client, _) = connect(&server);
    let unknown_kind = SpawnSpec {
        kind: "no-such-agent".into(),
        ..shell(None)
    };
    let err = client
        .spawn(unknown_kind, size(20, 4), Arc::new(|_| {}))
        .err()
        .unwrap();
    assert!(err.to_string().contains("no-such-agent"), "{err}");
    let err = client
        .attach(999, size(20, 4), Arc::new(|_| {}))
        .err()
        .unwrap();
    assert!(err.to_string().contains("no session 999"), "{err}");
    assert!(client.is_connected());
    assert!(client.list().unwrap().is_empty());
}

#[test]
fn another_protocol_version_is_refused() {
    let server = Server::new();
    let (mut client_end, server_end) = UnixStream::pair().unwrap();
    let serving = server.clone();
    thread::spawn(move || {
        let reader = server_end.try_clone().unwrap();
        serving.serve_connection(reader, server_end);
    });
    let hello = ClientMessage::Hello {
        version: PROTOCOL_VERSION + 1,
    };
    write_frame(&mut client_end, &hello.encode()).unwrap();
    let frame = read_frame(&mut client_end).unwrap().unwrap();
    let ServerMessage::Failed { request, message } = ServerMessage::decode(&frame).unwrap() else {
        panic!("expected a failure");
    };
    assert_eq!(request, 0);
    assert!(message.contains("protocol"), "{message}");
}

#[test]
fn the_client_hears_when_the_server_goes_away() {
    let (client_end, server_end) = UnixStream::pair().unwrap();
    // A server that answers the hello, then hangs up.
    thread::spawn(move || {
        let mut stream = server_end;
        read_frame(&mut stream).unwrap();
        let hello = ServerMessage::Hello {
            version: PROTOCOL_VERSION,
        };
        write_frame(&mut stream, &hello.encode()).unwrap();
    });
    let (disconnected, reason) = mpsc::channel();
    let client = Client::connect(
        client_end.try_clone().unwrap(),
        client_end,
        Box::new(move |why| drop(disconnected.send(why))),
    )
    .unwrap();
    let why = reason.recv_timeout(TIMEOUT).unwrap();
    assert!(why.contains("closed"), "{why}");
    assert!(!client.is_connected());
    assert!(client.list().is_err());
}

#[test]
fn the_server_is_idle_only_without_connections_and_sessions() {
    let server = Server::new();
    assert!(server.idle_for().is_some());
    let (client, _) = connect(&server);
    wait_until("the connection", || server.idle_for().is_none());
    let session = client
        .spawn(shell(None), size(20, 4), Arc::new(|_| {}))
        .unwrap();
    drop(session);
    drop(client);
    // The client's socket is closed only once its reader thread sees the end; the session
    // keeps the server busy anyway.
    thread::sleep(Duration::from_millis(50));
    assert!(server.idle_for().is_none());
}
