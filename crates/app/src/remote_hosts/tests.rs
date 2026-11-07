use std::cell::Cell;
use std::net::Shutdown;
use std::os::unix::net::UnixStream;
use std::rc::Rc;
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::{Duration, Instant};

use agentws_remote::Server;
use term_core::SessionKind;

use super::*;

/// Each connection's socket, to cut it.
type Sockets = Arc<Mutex<Vec<UnixStream>>>;

/// Connects to an in-process server, counting the connections.
fn hosts(server: &Server) -> (RemoteHosts, Rc<Cell<u32>>, Sockets) {
    let connects = Rc::new(Cell::new(0));
    let sockets = Arc::new(Mutex::new(Vec::new()));
    let (counted, kept, server) = (connects.clone(), sockets.clone(), server.clone());
    let hosts = RemoteHosts::with_connector(Box::new(move |host| {
        assert_eq!(host, "box");
        counted.set(counted.get() + 1);
        let (client_end, server_end) = UnixStream::pair()?;
        let server = server.clone();
        thread::spawn(move || {
            let reader = server_end.try_clone().unwrap();
            server.serve_connection(reader, server_end);
        });
        kept.lock().unwrap().push(client_end.try_clone()?);
        Client::connect(client_end.try_clone()?, client_end, Box::new(|_| {}))
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

fn spawn(hosts: &mut RemoteHosts, id: u64) -> io::Result<Session> {
    hosts.spawn(
        id,
        &remote_shell(),
        TermDimensions::new(40, 5),
        Palette::default(),
        Arc::new(|_| {}),
    )
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
    let first = spawn(&mut hosts, 1).unwrap();
    let second = spawn(&mut hosts, 2).unwrap();
    assert_eq!(connects.get(), 1);
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
    let third = spawn(&mut hosts, 3).unwrap();
    assert_eq!(connects.get(), 2);
    assert!(!third.has_exited());
}

#[test]
fn a_failed_connection_names_the_host() {
    let mut hosts = RemoteHosts::with_connector(Box::new(|_| {
        Err(io::Error::other("ssh: Could not resolve hostname box"))
    }));
    let err = spawn(&mut hosts, 1).err().unwrap();
    assert!(
        err.to_string().contains("could not connect to box"),
        "{err}"
    );
    let mut local = remote_shell();
    local.host = None;
    let err = hosts
        .spawn(
            1,
            &local,
            TermDimensions::new(40, 5),
            Palette::default(),
            Arc::new(|_| {}),
        )
        .err()
        .unwrap();
    assert_eq!(err.kind(), io::ErrorKind::InvalidInput);
}
