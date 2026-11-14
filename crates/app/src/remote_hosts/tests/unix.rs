use std::net::Shutdown;
use std::os::unix::net::UnixStream;
use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::{Duration, Instant};

use agentws_remote::Server;
use term_core::SessionKind;

use super::*;

/// Each connection's socket, to cut it.
type Sockets = Arc<Mutex<Vec<UnixStream>>>;

/// Connects to an in-process server, counting the connections.
fn hosts(server: &Server) -> (RemoteHosts, Arc<AtomicU32>, Sockets) {
    let connects = Arc::new(AtomicU32::new(0));
    let sockets = Arc::new(Mutex::new(Vec::new()));
    let (counted, kept, server) = (connects.clone(), sockets.clone(), server.clone());
    let hosts = RemoteHosts::with_connector(Arc::new(move |target: &Target| {
        assert_eq!(target.destination, "box");
        counted.fetch_add(1, Ordering::SeqCst);
        let (client_end, server_end) = UnixStream::pair()?;
        let server = server.clone();
        thread::spawn(move || {
            let reader = server_end.try_clone().unwrap();
            server.serve_connection(reader, server_end);
        });
        kept.lock().unwrap().push(client_end.try_clone()?);
        Client::connect(client_end.try_clone()?, client_end, None, Box::new(|_| {}))
    }));
    (hosts, connects, sockets)
}

fn remote_shell() -> LaunchSpec {
    let mut spec = LaunchSpec::new(SessionKind::Shell, Some("sh".into()), Vec::new(), None);
    spec.env.insert("PS1".into(), "$ ".into());
    spec.env.insert("ENV".into(), String::new());
    spec.host = Some("box".into());
    spec
}

fn box_target() -> Target {
    Target {
        destination: "box".into(),
        program: "agentws-server".into(),
    }
}

/// Start a shell on "box" as session `id`, as [`start`] does on its thread.
fn spawn(hosts: &mut RemoteHosts, id: u64) -> io::Result<Session> {
    let view = LocalView {
        id,
        palette: Palette::default(),
        callback: Arc::new(|_| {}),
    };
    hosts
        .prepare(
            box_target(),
            &remote_shell(),
            TermDimensions::new(40, 5),
            view,
        )
        .run()
}

fn wait_until(what: &str, mut condition: impl FnMut() -> bool) {
    let deadline = Instant::now() + Duration::from_secs(10);
    while !condition() {
        assert!(Instant::now() < deadline, "timed out waiting for {what}");
        thread::sleep(Duration::from_millis(10));
    }
}

#[test]
fn tabs_on_one_host_share_a_connection_opened_again_once_lost() {
    let server = Server::new();
    let (mut hosts, connects, sockets) = hosts(&server);
    assert!(!hosts.is_connected("box"));
    let first = spawn(&mut hosts, 1).unwrap();
    let second = spawn(&mut hosts, 2).unwrap();
    assert_eq!(connects.load(Ordering::SeqCst), 1);
    assert!(hosts.is_connected("box"));
    assert!(!hosts.is_connected("other"));
    assert_eq!((first.id(), second.id()), (1, 2));
    second.write(b"echo hi-$((1 + 2))\n".as_slice());
    wait_until("the output", || {
        second
            .visible_text()
            .iter()
            .any(|line| line.contains("hi-3"))
    });
    sockets.lock().unwrap()[0].shutdown(Shutdown::Both).unwrap();
    wait_until("the tabs to see the loss", || first.has_exited());
    assert!(!hosts.is_connected("box"));
    let third = spawn(&mut hosts, 3).unwrap();
    assert_eq!(connects.load(Ordering::SeqCst), 2);
    assert!(!third.has_exited());
}

#[test]
fn tabs_starting_together_on_one_host_open_one_connection() {
    let server = Server::new();
    let (hosts, connects, _sockets) = hosts(&server);
    let hosts = Mutex::new(hosts);
    let starts: Vec<_> = (1..=3)
        .map(|id| {
            let view = LocalView {
                id,
                palette: Palette::default(),
                callback: Arc::new(|_| {}),
            };
            hosts.lock().unwrap().prepare(
                box_target(),
                &remote_shell(),
                TermDimensions::new(40, 5),
                view,
            )
        })
        .collect();
    let sessions: Vec<_> = starts
        .into_iter()
        .map(|start| thread::spawn(move || start.run()))
        .collect::<Vec<_>>()
        .into_iter()
        .map(|started| started.join().unwrap().unwrap())
        .collect();
    assert_eq!(sessions.len(), 3);
    assert_eq!(connects.load(Ordering::SeqCst), 1);
}

#[test]
fn a_failed_connection_names_the_host() {
    let mut hosts = RemoteHosts::with_connector(Arc::new(|_: &Target| {
        Err(io::Error::other("ssh: Could not resolve hostname box"))
    }));
    let err = spawn(&mut hosts, 1).err().unwrap();
    assert!(
        err.to_string().contains("could not connect to box"),
        "{err}"
    );
}

fn reattach(server_id: u64, id: u64) -> Reattach {
    Reattach {
        server_id,
        size: TermDimensions::new(40, 5),
        view: LocalView {
            id,
            palette: Palette::default(),
            callback: Arc::new(|_| {}),
        },
    }
}

#[test]
fn lost_tabs_reattach_over_one_new_connection_and_a_closed_one_is_gone() {
    let server = Server::new();
    let (mut hosts, connects, sockets) = hosts(&server);
    let first = spawn(&mut hosts, 1).unwrap();
    let second = spawn(&mut hosts, 2).unwrap();
    first.write(b"echo kept-$((2 + 3))\n".as_slice());
    wait_until("the output", || {
        first
            .visible_text()
            .iter()
            .any(|line| line.contains("kept-5"))
    });
    let ids = (first.remote_id().unwrap(), second.remote_id().unwrap());
    sockets.lock().unwrap()[0].shutdown(Shutdown::Both).unwrap();
    wait_until("the tabs to see the loss", || {
        first.has_exited() && second.has_exited()
    });
    // A new tab opens the host's connection again; the second session is closed on the
    // server meanwhile (`agentws-server close`).
    drop(spawn(&mut hosts, 9).unwrap());
    assert_eq!(connects.load(Ordering::SeqCst), 2);
    let client = hosts.connections["box"].lock().unwrap().clone().unwrap();
    client.close(ids.1).unwrap();
    wait_until("the close", || {
        client.list().unwrap().iter().all(|info| info.id != ids.1)
    });
    let results = hosts
        .prepare_reattach(box_target(), vec![reattach(ids.0, 4), reattach(ids.1, 5)])
        .run()
        .unwrap();
    assert_eq!(
        connects.load(Ordering::SeqCst),
        2,
        "the live connection is reused"
    );
    let [again, gone] = <[Reattached; 2]>::try_from(results).ok().unwrap();
    let again = again.unwrap().expect("still running there");
    assert_eq!(again.id(), 4);
    assert!(
        again
            .visible_text()
            .iter()
            .any(|line| line.contains("kept-5"))
    );
    assert!(gone.unwrap().is_none());
}

#[test]
fn an_unreachable_host_fails_the_whole_reattach() {
    let mut hosts = RemoteHosts::with_connector(Arc::new(|_: &Target| {
        Err(io::Error::other(
            "ssh: connect to host box port 22: No route to host",
        ))
    }));
    let err = hosts
        .prepare_reattach(box_target(), vec![reattach(1, 1)])
        .run()
        .err()
        .unwrap();
    assert!(err.to_string().contains("No route to host"), "{err}");
}
