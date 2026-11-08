use workspace_model::{DEFAULT_SERVER_PROGRAM, RemoteHost};

use super::*;

#[cfg(unix)]
mod unix;

fn configured() -> workspace_model::RemoteHosts {
    workspace_model::RemoteHosts::new(vec![RemoteHost {
        name: "Build box".into(),
        destination: "me@box".into(),
        server_program: "~/.cargo/bin/agentws-server".into(),
    }])
}

#[test]
fn a_configured_host_is_found_by_destination_or_name_with_its_program() {
    let expected = Target {
        destination: "me@box".into(),
        program: "~/.cargo/bin/agentws-server".into(),
    };
    for host in ["me@box", "Build box"] {
        // The settings win over the environment variable.
        let target = resolve_target(&configured(), host, Some("/env/server".into()));
        assert_eq!(target, expected, "{host}");
    }
}

#[test]
fn an_unknown_host_is_the_destination_with_the_environment_or_default_program() {
    let target = resolve_target(&configured(), " other ", Some("/env/server".into()));
    assert_eq!(target.destination, "other");
    assert_eq!(target.program, "/env/server");
    let target = resolve_target(&configured(), "other", None);
    assert_eq!(target.program, DEFAULT_SERVER_PROGRAM);
}

#[test]
fn a_host_without_a_destination_has_no_target() {
    let mut host = RemoteHost::default();
    assert_eq!(host_target(&host), None);
    host.destination = " me@box ".into();
    assert_eq!(
        host_target(&host),
        Some(Target {
            destination: "me@box".into(),
            program: DEFAULT_SERVER_PROGRAM.into(),
        })
    );
}

#[gpui::test]
fn a_check_runs_once_at_a_time_and_its_outcome_is_kept_by_target(
    cx: &mut gpui_kit::TestAppContext,
) {
    use std::sync::atomic::{AtomicU32, Ordering};
    use std::sync::mpsc;

    let (release, released) = mpsc::channel::<()>();
    let released = Mutex::new(released);
    let calls = Arc::new(AtomicU32::new(0));
    let counted = calls.clone();
    let checker: Checker = Arc::new(move |_| {
        counted.fetch_add(1, Ordering::SeqCst);
        let _ = released.lock().unwrap().recv();
        CheckOutcome::NoAnswer
    });
    let target = Target {
        destination: "me@box".into(),
        program: "agentws-server".into(),
    };
    let edited = Target {
        program: "~/bin/agentws-server".into(),
        ..target.clone()
    };
    cx.update(|cx| {
        cx.set_global(HostChecks::with_checker(checker));
        check(target.clone(), cx);
        check(target.clone(), cx);
        assert_eq!(host_check(&target, cx), Some(HostCheck::Checking));
        assert_eq!(host_check(&edited, cx), None);
        assert!(!is_connected("me@box", cx));
    });
    release.send(()).unwrap();
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(10);
    while cx.update(|cx| host_check(&target, cx)) == Some(HostCheck::Checking) {
        assert!(
            std::time::Instant::now() < deadline,
            "the check never ended"
        );
        std::thread::sleep(std::time::Duration::from_millis(10));
        cx.run_until_parked();
    }
    assert_eq!(
        cx.update(|cx| host_check(&target, cx)),
        Some(HostCheck::Done(CheckOutcome::NoAnswer))
    );
    assert_eq!(calls.load(Ordering::SeqCst), 1);
}

#[gpui::test]
fn a_successful_connection_drops_the_hosts_check(cx: &mut gpui_kit::TestAppContext) {
    let target = Target {
        destination: "me@box".into(),
        program: "agentws-server".into(),
    };
    let other = Target {
        destination: "me@other".into(),
        ..target.clone()
    };
    let attempt = |outcome: io::Result<()>, cx: &mut gpui_kit::TestAppContext| {
        cx.update(|cx| {
            let checks = cx.default_global::<HostChecks>();
            for kept in [&target, &other] {
                let done = HostCheck::Done(CheckOutcome::NotFound);
                checks.checks.insert(kept.clone(), done);
            }
            let (sender, receiver) = oneshot::channel();
            let forwarded = forget_check_once_connected(receiver, target.clone(), cx);
            sender.send(outcome).unwrap();
            forwarded
        })
    };
    let failed = attempt(Err(io::Error::other("no route")), cx);
    cx.run_until_parked();
    assert!(futures::executor::block_on(failed).unwrap().is_err());
    assert!(cx.update(|cx| host_check(&target, cx)).is_some());
    let connected = attempt(Ok(()), cx);
    cx.run_until_parked();
    assert!(futures::executor::block_on(connected).unwrap().is_ok());
    assert_eq!(cx.update(|cx| host_check(&target, cx)), None);
    assert!(cx.update(|cx| host_check(&other, cx)).is_some());
}
