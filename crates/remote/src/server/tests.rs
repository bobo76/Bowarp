//! A server and clients in this process, over socket pairs.
#![cfg(unix)]

use std::net::Shutdown;
use std::os::unix::net::UnixStream;
use std::path::Path;
use std::sync::mpsc::Receiver;

use super::*;
use crate::client::{Client, LocalView, RemoteSession};
use term_core::EventCallback;

const TIMEOUT: Duration = Duration::from_secs(10);

fn wait_until(what: &str, mut condition: impl FnMut() -> bool) {
    let deadline = Instant::now() + TIMEOUT;
    while !condition() {
        assert!(Instant::now() < deadline, "timed out waiting for {what}");
        thread::sleep(Duration::from_millis(10));
    }
}

/// A client connected to `server`, its end of the connection (to cut it), and a receiver of
/// its disconnect reason.
fn connect(server: &Server) -> (Client, UnixStream, Receiver<String>) {
    let (client_end, server_end) = UnixStream::pair().unwrap();
    let server = server.clone();
    thread::spawn(move || {
        let reader = server_end.try_clone().unwrap();
        server.serve_connection(reader, server_end);
    });
    let (disconnected, reason) = mpsc::channel();
    let cut = client_end.try_clone().unwrap();
    let client = Client::connect(
        client_end.try_clone().unwrap(),
        client_end,
        Box::new(move |why| drop(disconnected.send(why))),
    )
    .unwrap();
    (client, cut, reason)
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

fn view(callback: EventCallback) -> LocalView {
    LocalView {
        id: 100,
        palette: Palette::default(),
        callback,
    }
}

fn quiet() -> LocalView {
    view(Arc::new(|_| {}))
}

fn screen_contains(remote: &RemoteSession, text: &str) -> bool {
    remote
        .session
        .visible_text()
        .iter()
        .any(|line| line.contains(text))
}

fn events() -> (EventCallback, Arc<Mutex<Vec<SessionEvent>>>) {
    let seen = Arc::new(Mutex::new(Vec::new()));
    let recorder = seen.clone();
    (
        Arc::new(move |event| recorder.lock().unwrap().push(event)),
        seen,
    )
}

#[test]
fn a_spawned_shell_runs_what_is_typed() {
    let server = Server::new();
    let (client, _, _) = connect(&server);
    let (callback, seen) = events();
    let remote = client
        .spawn(shell(None), size(40, 6), view(callback))
        .unwrap();
    assert_eq!(remote.session.id(), 100);
    assert_eq!(remote.session.kind(), SessionKind::Shell);
    remote.session.write(b"echo $((6 * 7))\n".to_vec());
    wait_until("the answer", || screen_contains(&remote, "42"));
    assert!(seen.lock().unwrap().contains(&SessionEvent::Wakeup));
    assert!(!remote.session.has_exited());
}

#[test]
fn a_session_outlives_its_connection_and_reattaches_with_its_screen() {
    let server = Server::new();
    let (client, cut, reason) = connect(&server);
    let (callback, seen) = events();
    let remote = client
        .spawn(shell(None), size(40, 6), view(callback))
        .unwrap();
    assert_eq!(remote.session.remote_id(), Some(remote.info.id));
    remote.session.write(b"echo before-$((1 + 1))\n".to_vec());
    wait_until("the output", || screen_contains(&remote, "before-2"));
    // The network goes away.
    cut.shutdown(Shutdown::Both).unwrap();
    let why = reason.recv_timeout(TIMEOUT).unwrap();
    assert!(!why.is_empty());
    wait_until("the tab to see it", || remote.session.has_exited());
    // Disconnected, not exited: the tab can attach again.
    let ends: Vec<_> = seen
        .lock()
        .unwrap()
        .iter()
        .filter(|event| {
            matches!(
                event,
                SessionEvent::Exited(_) | SessionEvent::Disconnected(_)
            )
        })
        .cloned()
        .collect();
    assert_eq!(ends, [SessionEvent::Disconnected(why)]);
    assert!(!client.is_connected());
    // Dropping the tab now cannot end the session: it is out of reach.
    let id = remote.info.id;
    drop(remote);

    let (client, _, _) = connect(&server);
    let sessions = client.list().unwrap();
    assert_eq!(sessions.len(), 1);
    assert_eq!(sessions[0].id, id);
    assert_eq!(sessions[0].exit, None);
    let again = client
        .reattach(id, size(50, 8), quiet())
        .unwrap()
        .expect("the session is still there");
    // The snapshot shows what was there before.
    assert!(screen_contains(&again, "before-2"));
    assert_eq!(again.session.size(), size(50, 8));
    again.session.write(b"echo after-$((2 + 2))\n".to_vec());
    wait_until("new output", || screen_contains(&again, "after-4"));
}

#[test]
fn reattaching_to_a_session_closed_meanwhile_finds_it_gone() {
    let server = Server::new();
    let (client, cut, reason) = connect(&server);
    let remote = client.spawn(shell(None), size(40, 6), quiet()).unwrap();
    let id = remote.info.id;
    cut.shutdown(Shutdown::Both).unwrap();
    reason.recv_timeout(TIMEOUT).unwrap();
    drop(remote);
    // Closed on the server (`agentws-server close`) while this app was away.
    let (other, _, _) = connect(&server);
    other.close(id).unwrap();
    wait_until("the close", || other.list().unwrap().is_empty());
    assert!(other.reattach(id, size(40, 6), quiet()).unwrap().is_none());
}

#[test]
fn a_session_that_exited_while_away_reattaches_as_exited() {
    let server = Server::new();
    let (client, cut, reason) = connect(&server);
    let script = "read line; echo done-$line; exit 4";
    let remote = client
        .spawn(shell(Some(script)), size(40, 6), quiet())
        .unwrap();
    let id = remote.info.id;
    cut.shutdown(Shutdown::Both).unwrap();
    reason.recv_timeout(TIMEOUT).unwrap();
    drop(remote);
    // Another client makes it finish while this one is away.
    let (other, _, _) = connect(&server);
    let helper = other.attach(id, size(40, 6), quiet()).unwrap();
    helper.session.write(b"x\n".to_vec());
    wait_until("the exit", || helper.session.has_exited());
    let (callback, seen) = events();
    let (client, _, _) = connect(&server);
    let again = client
        .reattach(id, size(40, 6), view(callback))
        .unwrap()
        .expect("an exited session stays listed");
    assert!(again.session.has_exited());
    assert_eq!(again.session.exit_code(), Some(4));
    assert!(screen_contains(&again, "done-x"));
    assert!(
        seen.lock()
            .unwrap()
            .contains(&SessionEvent::Exited(Some(4)))
    );
}

#[test]
fn an_exit_is_reported_to_attached_clients_and_late_ones() {
    let server = Server::new();
    let (client, _, _) = connect(&server);
    let (callback, seen) = events();
    let remote = client
        .spawn(shell(Some("echo bye; exit 3")), size(30, 4), view(callback))
        .unwrap();
    wait_until("the exit", || remote.session.has_exited());
    assert_eq!(remote.session.exit_code(), Some(3));
    assert!(screen_contains(&remote, "bye"));
    assert!(
        seen.lock()
            .unwrap()
            .contains(&SessionEvent::Exited(Some(3)))
    );
    // An exited session stays listed until it is closed.
    assert_eq!(client.list().unwrap()[0].exit, Some(Some(3)));
    let (other, _, _) = connect(&server);
    let late = other.attach(remote.info.id, size(30, 4), quiet()).unwrap();
    assert!(late.session.has_exited());
    assert_eq!(late.session.exit_code(), Some(3));
    assert!(screen_contains(&late, "bye"));
    // Closing the tab ends the session for everyone.
    drop(remote);
    wait_until("the session to leave the list", || {
        client.list().unwrap().is_empty()
    });
}

#[test]
fn clients_follow_each_others_resizes_and_closes() {
    let server = Server::new();
    let (first, _, _) = connect(&server);
    let (second, _, _) = connect(&server);
    let one = first.spawn(shell(None), size(40, 6), quiet()).unwrap();
    let (callback, seen) = events();
    let two = second
        .attach(one.info.id, size(40, 6), view(callback))
        .unwrap();
    one.session.write(b"echo shared\n".to_vec());
    wait_until("output on the other client", || {
        screen_contains(&two, "shared")
    });
    one.session.resize(size(60, 10));
    wait_until("the resize", || two.session.size() == size(60, 10));
    drop(one);
    wait_until("the close", || two.session.has_exited());
    assert!(seen.lock().unwrap().contains(&SessionEvent::Exited(None)));
    assert!(second.list().unwrap().is_empty());
}

#[test]
fn bad_requests_fail_without_ending_the_connection() {
    let server = Server::new();
    let (client, _, _) = connect(&server);
    let unknown_kind = SpawnSpec {
        kind: "no-such-agent".into(),
        ..shell(None)
    };
    let err = client
        .spawn(unknown_kind, size(20, 4), quiet())
        .err()
        .unwrap();
    assert!(err.to_string().contains("no-such-agent"), "{err}");
    let err = client.attach(999, size(20, 4), quiet()).err().unwrap();
    assert!(err.to_string().contains("no session 999"), "{err}");
    assert!(client.is_connected());
    assert!(client.list().unwrap().is_empty());
}

#[test]
fn a_launch_becomes_a_spawn_spec() {
    let mut launch = LaunchSpec::agent(
        workspace_model::agents::CLAUDE,
        Some(PathBuf::from("/srv/project")),
    );
    launch.args = vec!["--continue".into()];
    launch.env.insert("B".into(), "2".into());
    launch.env.insert("A".into(), "1".into());
    let spec = SpawnSpec::from_launch(&launch);
    assert_eq!(spec.kind, "claude");
    assert_eq!(spec.program.as_deref(), Some("claude"));
    assert_eq!(spec.args, ["--continue"]);
    assert_eq!(spec.cwd.as_deref(), Some("/srv/project"));
    assert_eq!(
        spec.env,
        [("A".into(), "1".into()), ("B".into(), "2".into())]
    );
    assert_eq!(SpawnSpec::from_launch(&LaunchSpec::shell(None)).kind, "");
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
    let (client, cut, _) = connect(&server);
    wait_until("the connection", || server.idle_for().is_none());
    let remote = client.spawn(shell(None), size(20, 4), quiet()).unwrap();
    cut.shutdown(Shutdown::Both).unwrap();
    wait_until("the disconnect", || !client.is_connected());
    // The session keeps the server busy.
    thread::sleep(Duration::from_millis(50));
    assert!(server.idle_for().is_none());
    drop(remote);
}

/// Reading a program's folder needs `/proc`.
#[cfg(target_os = "linux")]
#[test]
fn clients_hear_the_folder_a_shell_moves_to() {
    let base = std::env::temp_dir().join(format!("agentws-folder-{}", std::process::id()));
    let (start, next) = (base.join("start"), base.join("next"));
    std::fs::create_dir_all(&start).unwrap();
    std::fs::create_dir_all(&next).unwrap();
    let (start, next) = (start.canonicalize().unwrap(), next.canonicalize().unwrap());
    let text = |path: &Path| path.to_string_lossy().into_owned();

    let server = Server::new();
    let (client, _, _) = connect(&server);
    let (callback, seen) = events();
    let spec = SpawnSpec {
        cwd: Some(text(&start)),
        ..shell(None)
    };
    let remote = client.spawn(spec, size(80, 6), view(callback)).unwrap();
    assert_eq!(remote.info.folder, Some(text(&start)));
    assert_eq!(remote.session.remote_folder(), Some(text(&start)));

    remote
        .session
        .write(format!("cd '{}' && echo moved\n", text(&next)).into_bytes());
    wait_until("the new folder", || {
        remote.session.remote_folder() == Some(text(&next))
    });
    assert!(
        seen.lock()
            .unwrap()
            .contains(&SessionEvent::RemoteFolderChanged)
    );
    // A client attaching later starts with it.
    let (other, _, _) = connect(&server);
    let again = other.attach(remote.info.id, size(80, 6), quiet()).unwrap();
    assert_eq!(again.session.remote_folder(), Some(text(&next)));
    drop((remote, again));
    let _ = std::fs::remove_dir_all(&base);
}

#[test]
fn a_closed_session_frees_its_terminal_copy() {
    let server = Server::new();
    let hosted = server.spawn(&shell(None), size(20, 4)).unwrap();
    let (id, fanout) = (hosted.id, Arc::downgrade(&hosted.fanout));
    drop(hosted);
    server.close(id);
    // The program's output reader lets go once the hang-up ends it.
    wait_until("the fan-out to be freed", || fanout.strong_count() == 0);
}
