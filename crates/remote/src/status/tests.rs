use super::*;
use crate::protocol::{ServerMessage, write_frame};

fn running(sessions: usize) -> ServerStatus {
    ServerStatus {
        version: "0.2.59".into(),
        protocol: Some(PROTOCOL_VERSION),
        daemon: DaemonState::Running,
        sessions: Some(sessions),
    }
}

fn exited(exit: i32, stdout: &str, stderr: &str) -> Option<CommandOutput> {
    Some(CommandOutput {
        exit: Some(exit),
        stdout: stdout.into(),
        stderr: stderr.into(),
    })
}

fn done(outcome: CheckOutcome) -> HostCheck {
    HostCheck::Done(outcome)
}

#[test]
fn status_lines_read_back() {
    let not_running = ServerStatus {
        daemon: DaemonState::NotRunning,
        sessions: None,
        ..running(0)
    };
    for status in [running(3), not_running] {
        assert_eq!(ServerStatus::parse(&status.lines()), Some(status));
    }
    assert_eq!(
        running(2).lines(),
        format!("version=0.2.59\nprotocol={PROTOCOL_VERSION}\ndaemon=running\nsessions=2\n")
    );
}

#[test]
fn parsing_skips_unknown_keys_and_needs_a_daemon_state() {
    let status = ServerStatus::parse("motd\nuptime=5\ndaemon=incompatible\r\nversion=1.0\n");
    assert_eq!(
        status,
        Some(ServerStatus {
            version: "1.0".into(),
            protocol: None,
            daemon: DaemonState::Incompatible,
            sessions: None,
        })
    );
    assert_eq!(ServerStatus::parse("version=1.0\n"), None);
    assert_eq!(ServerStatus::parse("daemon=sleeping\n"), None);
}

#[test]
fn a_run_is_classified_by_its_exit_and_output() {
    assert_eq!(classify(None), CheckOutcome::NoAnswer);
    assert_eq!(
        classify(exited(0, &running(1).lines(), "")),
        CheckOutcome::Status(running(1))
    );
    assert_eq!(
        classify(exited(127, "", "sh: 1: agentws-server: not found")),
        CheckOutcome::NotFound
    );
    let refused = "Warning: banner\nssh: connect to host box port 22: Connection refused\n";
    assert_eq!(
        classify(exited(255, "", refused)),
        CheckOutcome::Unreachable {
            stderr: refused.trim().into()
        }
    );
    let usage = "usage: agentws-server proxy | serve [--idle-exit <seconds>] | list";
    assert_eq!(classify(exited(2, "", usage)), CheckOutcome::TooOld);
    assert_eq!(
        classify(exited(1, "", "agentws-server: broken pipe\n")),
        CheckOutcome::Failed {
            stderr: "agentws-server: broken pipe".into()
        }
    );
    assert_eq!(
        classify(exited(0, "hello\n", "")),
        CheckOutcome::Failed {
            stderr: "not a status answer: hello".into()
        }
    );
    assert_eq!(
        classify(exited(255, "", "")),
        CheckOutcome::Unreachable {
            stderr: "ssh exited with 255".into()
        }
    );
}

#[test]
fn the_status_line_names_the_state_then_the_version() {
    let line = |check: Option<&HostCheck>| status_line_for(false, check, PROTOCOL_VERSION);
    assert_eq!(line(None).text, "Not checked");
    assert_eq!(line(Some(&HostCheck::Checking)).text, "Checking…");
    let one = line(Some(&done(CheckOutcome::Status(running(1)))));
    assert_eq!(one.text, "Server running, 1 session · version 0.2.59");
    assert_eq!(one.tone, StatusTone::Fine);
    let stopped = ServerStatus {
        daemon: DaemonState::NotRunning,
        sessions: None,
        ..running(0)
    };
    let stopped = line(Some(&done(CheckOutcome::Status(stopped))));
    assert_eq!(stopped.text, "Server not running · version 0.2.59");
    assert_eq!(stopped.tone, StatusTone::Neutral);
}

#[test]
fn problems_say_what_went_wrong() {
    let line = |outcome| status_line_for(false, Some(&done(outcome)), PROTOCOL_VERSION);
    let unreachable = line(CheckOutcome::Unreachable {
        stderr: "banner\nssh: Could not resolve hostname box".into(),
    });
    assert_eq!(
        unreachable.text,
        "Unreachable: ssh: Could not resolve hostname box"
    );
    assert_eq!(
        unreachable.details.as_deref(),
        Some("banner\nssh: Could not resolve hostname box")
    );
    assert_eq!(unreachable.tone, StatusTone::Problem);
    let missing = line(CheckOutcome::NotFound);
    assert_eq!(
        (missing.text.as_str(), missing.hint),
        ("agentws-server not found", Some("set Server program"))
    );
    assert_eq!(
        line(CheckOutcome::TooOld).text,
        "Server too old to report status: update it"
    );
    assert_eq!(
        line(CheckOutcome::NoAnswer).text,
        "No answer from the server"
    );
}

#[test]
fn a_protocol_mismatch_is_flagged_after_the_version() {
    let line = status_line_for(false, Some(&done(CheckOutcome::Status(running(0)))), 9);
    assert_eq!(
        line.text,
        format!(
            "Server running, 0 sessions · version 0.2.59 · speaks protocol {PROTOCOL_VERSION}, \
             this app 9: remote tabs can't open"
        )
    );
    assert_eq!(line.tone, StatusTone::Problem);
}

#[test]
fn an_incompatible_daemon_is_not_judged_by_the_installed_protocol() {
    let incompatible = ServerStatus {
        daemon: DaemonState::Incompatible,
        sessions: None,
        ..running(0)
    };
    let line = status_line_for(false, Some(&done(CheckOutcome::Status(incompatible))), 9);
    assert_eq!(
        line.text,
        "A server of another version is running: remote tabs can't open · version 0.2.59"
    );
    assert_eq!(line.tone, StatusTone::Problem);
}

#[test]
fn connected_comes_first() {
    let line = status_line_for(true, None, PROTOCOL_VERSION);
    assert_eq!(line.text, "Connected · Not checked");
    assert_eq!(line.tone, StatusTone::Fine);
    let line = status_line_for(true, Some(&done(CheckOutcome::NoAnswer)), PROTOCOL_VERSION);
    assert_eq!(line.text, "Connected · No answer from the server");
    assert_eq!(line.tone, StatusTone::Problem);
}

#[cfg(unix)]
#[test]
fn a_run_past_its_timeout_is_killed() {
    let started = Instant::now();
    let mut command = Command::new("sh");
    command.args(["-c", "exec sleep 30"]);
    assert_eq!(run(command, Duration::from_millis(200)).unwrap(), None);
    assert!(started.elapsed() < Duration::from_secs(5));
}

#[cfg(unix)]
#[test]
fn a_run_keeps_its_exit_and_output() {
    let mut command = Command::new("sh");
    command.args(["-c", "echo out; echo err >&2; exit 3"]);
    assert_eq!(
        run(command, Duration::from_secs(10)).unwrap(),
        Some(CommandOutput {
            exit: Some(3),
            stdout: "out\n".into(),
            stderr: "err\n".into(),
        })
    );
}

#[cfg(unix)]
#[test]
fn output_past_the_limit_is_cut_without_failing_the_writer() {
    let mut command = Command::new("sh");
    command.args(["-c", "head -c 200000 /dev/zero; exit 0"]);
    let output = run(command, Duration::from_secs(10)).unwrap().unwrap();
    assert_eq!(output.exit, Some(0));
    assert_eq!(output.stdout.len(), OUTPUT_LIMIT);
}

#[cfg(unix)]
#[test]
fn a_process_left_holding_the_pipes_does_not_hold_up_the_run() {
    let started = Instant::now();
    let mut command = Command::new("sh");
    command.args(["-c", "echo out; sleep 30 & exit 0"]);
    assert_eq!(
        run(command, Duration::from_millis(500)).unwrap(),
        Some(CommandOutput {
            exit: Some(0),
            stdout: "out\n".into(),
            stderr: String::new(),
        })
    );
    assert!(started.elapsed() < Duration::from_secs(5));
}

#[cfg(unix)]
#[test]
fn output_held_open_is_waited_for_briefly_not_until_the_timeout() {
    let started = Instant::now();
    let mut command = Command::new("sh");
    command.args(["-c", "echo out; sleep 30 & exit 0"]);
    let output = run(command, Duration::from_secs(20)).unwrap().unwrap();
    assert_eq!(output.stdout, "out\n");
    assert!(started.elapsed() < Duration::from_secs(5));
}

fn answer_with(message: ServerMessage) -> io::Cursor<Vec<u8>> {
    let mut frame = Vec::new();
    write_frame(&mut frame, &message.encode()).unwrap();
    io::Cursor::new(frame)
}

#[test]
fn a_daemon_refusing_the_hello_is_incompatible() {
    let refusal = ServerMessage::Failed {
        request: 0,
        message: "protocol 9 not supported".into(),
    };
    assert_eq!(
        daemon_state(answer_with(refusal), io::sink(), ASK_TIMEOUT).unwrap(),
        (DaemonState::Incompatible, None)
    );
}

#[test]
fn a_hello_in_another_version_is_an_error() {
    let hello = ServerMessage::Hello {
        version: PROTOCOL_VERSION + 1,
    };
    let err = daemon_state(answer_with(hello), io::sink(), ASK_TIMEOUT).unwrap_err();
    assert_eq!(err.kind(), io::ErrorKind::InvalidData);
}

/// A daemon that accepted the connection but never answers.
struct Silent;

impl Read for Silent {
    fn read(&mut self, _: &mut [u8]) -> io::Result<usize> {
        thread::sleep(Duration::from_secs(60));
        Ok(0)
    }
}

#[test]
fn a_daemon_that_never_answers_times_out() {
    let started = Instant::now();
    let err = daemon_state(Silent, io::sink(), Duration::from_millis(100)).unwrap_err();
    assert_eq!(err.kind(), io::ErrorKind::TimedOut);
    assert!(started.elapsed() < Duration::from_secs(5));
}

#[test]
fn only_a_missing_or_refusing_socket_means_no_daemon() {
    use io::ErrorKind::{ConnectionRefused, NotFound, PermissionDenied, TimedOut};
    for (kind, absent) in [
        (NotFound, true),
        (ConnectionRefused, true),
        (PermissionDenied, false),
        (TimedOut, false),
    ] {
        assert_eq!(daemon_absent(&io::Error::from(kind)), absent, "{kind:?}");
    }
}
