use super::*;

#[cfg(unix)]
fn over_shell(script: &str) -> io::Result<Client> {
    let mut command = Command::new("sh");
    command.args(["-c", script]);
    Client::over_command(command, None, Box::new(|_| {}))
}

#[cfg(unix)]
#[test]
fn a_silent_server_times_out() {
    let started = std::time::Instant::now();
    let err = over_shell("exec sleep 30").err().unwrap();
    assert_eq!(err.kind(), ErrorKind::TimedOut, "{err}");
    assert!(started.elapsed() < Duration::from_secs(10));
}

#[cfg(unix)]
#[test]
fn a_process_left_holding_stderr_does_not_stall_the_error() {
    let started = std::time::Instant::now();
    let err = over_shell("sleep 30 >/dev/null & echo 'ssh: refused' >&2; exit 255")
        .err()
        .unwrap();
    assert!(err.to_string().contains("ssh: refused"), "{err}");
    assert!(started.elapsed() < Duration::from_secs(10));
}
