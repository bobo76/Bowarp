//! `agentws-server`: runs Agent Workspace sessions on this machine for the app on another.
//!
//! - `agentws-server proxy`: what the app runs through ssh. Connects to this user's server
//!   (starting it when none runs) and relays stdin and stdout to it.
//! - `agentws-server serve [--idle-exit <seconds>]`: run the server in the foreground. It
//!   exits after the given time (default 60, 0 = never) with no session and no connection.
//! - `agentws-server list`: the sessions this user's server runs, one per line: id, kind,
//!   state, attached clients, their machines and title, tab-separated.
//! - `agentws-server status`: whether this user's server runs, with this program's version and
//!   protocol and the server's session count, as `key=value` lines. Never starts the server.
//! - `agentws-server show <id> [--scrollback]`: print the session's screen as plain text, one
//!   line per row without the trailing empty rows, after its scrollback with `--scrollback`.
//!   Does not attach: the pty keeps its size. An exited session shows its last screen.
//! - `agentws-server close <id>`: end one of them.
//! - `agentws-server start`: start this user's server in the background, running until
//!   stopped (no idle exit); then print what `status` prints. A running server is left as it
//!   is.
//! - `agentws-server stop [--force]`: end every session and stop this user's server; then
//!   print what `status` prints. Without `--force`, while sessions run, it stops nothing,
//!   prints them as `list` does and exits with [`status::SESSIONS_RUNNING`].

use std::io::Write as _;
use std::process::ExitCode;
use std::time::Duration;

use agentws_remote::status::{self, DaemonState, ServerStatus};
use agentws_remote::{Client, transport};

const DEFAULT_IDLE_EXIT: Duration = Duration::from_secs(60);

const USAGE: &str = "usage: agentws-server proxy | serve [--idle-exit <seconds>] | list | \
                     status | show <id> [--scrollback] | start | stop [--force] | close <id> | \
                     --version";

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let args: Vec<&str> = args.iter().map(String::as_str).collect();
    let name = transport::socket_name();
    let result = match args.as_slice() {
        ["proxy"] => std::env::current_exe().and_then(|exe| transport::proxy(&name, &exe)),
        ["serve"] => transport::serve(&name, Some(DEFAULT_IDLE_EXIT)),
        ["serve", "--idle-exit", seconds] => match seconds.parse::<u64>() {
            Ok(0) => transport::serve(&name, None),
            Ok(seconds) => transport::serve(&name, Some(Duration::from_secs(seconds))),
            Err(_) => return usage(),
        },
        ["list"] => list(&name),
        ["status"] => status(&name),
        ["show", id] | ["show", id, "--scrollback"] => match id.parse::<u64>() {
            Ok(id) => show(&name, id, args.len() == 3),
            Err(_) => return usage(),
        },
        ["start"] => start(&name),
        ["stop"] => match stop(&name, false) {
            Ok(Stopped::Busy) => return ExitCode::from(status::SESSIONS_RUNNING as u8),
            other => other.map(|_| ()),
        },
        ["stop", "--force"] => stop(&name, true).map(|_| ()),
        ["close", id] => match id.parse::<u64>() {
            Ok(id) => close(&name, id),
            Err(_) => return usage(),
        },
        ["--version"] => {
            println!(
                "agentws-server {} (protocol {})",
                env!("CARGO_PKG_VERSION"),
                agentws_remote::protocol::PROTOCOL_VERSION
            );
            return ExitCode::SUCCESS;
        }
        _ => return usage(),
    };
    match result {
        Ok(()) => ExitCode::SUCCESS,
        Err(err) => {
            eprintln!("agentws-server: {err}");
            ExitCode::FAILURE
        }
    }
}

/// A client of the running server; an error when none runs.
fn client(name: &str) -> std::io::Result<Client> {
    let stream = transport::connect(name)
        .map_err(|err| std::io::Error::new(err.kind(), format!("no server running: {err}")))?;
    let (reader, writer) = transport::halves(stream);
    Client::connect(reader, writer, None, Box::new(|_| {}))
}

fn list(name: &str) -> std::io::Result<()> {
    print_sessions(&client(name)?.list()?);
    Ok(())
}

fn print_sessions(sessions: &[agentws_remote::SessionInfo]) {
    for info in sessions {
        let kind = if info.kind.is_empty() {
            "shell"
        } else {
            &info.kind
        };
        let state = match info.exit {
            None => "running".to_owned(),
            Some(Some(code)) => format!("exited ({code})"),
            Some(None) => "exited".to_owned(),
        };
        let title = info.title.as_deref().unwrap_or("");
        let clients = info.clients.len();
        let machines = machines(&info.clients);
        println!(
            "{}\t{kind}\t{state}\t{clients}\t{machines}\t{title}",
            info.id
        );
    }
}

/// The attached clients' machine names, comma-separated: `?` for one that gave no name, `-`
/// for none.
fn machines(clients: &[String]) -> String {
    if clients.is_empty() {
        return "-".to_owned();
    }
    let names: Vec<String> = clients.iter().map(|name| column_safe(name)).collect();
    names.join(",")
}

/// `name` as one item of the machines column: a client sends any text, and a tab, newline or
/// comma in it would break the line's columns.
fn column_safe(name: &str) -> String {
    if name.is_empty() {
        return "?".to_owned();
    }
    name.chars()
        .map(|c| if c.is_control() || c == ',' { '?' } else { c })
        .collect()
}

/// Exits 0 whether or not the server runs; an error only when reaching or asking it failed.
fn status(name: &str) -> std::io::Result<()> {
    let (daemon, sessions) = match transport::connect(name) {
        Err(err) if status::daemon_absent(&err) => (DaemonState::NotRunning, None),
        Err(err) => {
            return Err(std::io::Error::new(
                err.kind(),
                format!("could not reach the server: {err}"),
            ));
        }
        Ok(stream) => {
            let (reader, writer) = transport::halves(stream);
            status::daemon_state(reader, writer, status::ASK_TIMEOUT)?
        }
    };
    let status = ServerStatus {
        version: env!("CARGO_PKG_VERSION").to_owned(),
        protocol: Some(agentws_remote::protocol::PROTOCOL_VERSION),
        daemon,
        sessions,
    };
    print!("{}", status.lines());
    Ok(())
}

fn show(name: &str, id: u64, scrollback: bool) -> std::io::Result<()> {
    let lines = client(name)?.show(id, scrollback)?;
    closed_pipe_is_done(print_lines(&lines))
}

fn print_lines(lines: &[String]) -> std::io::Result<()> {
    let mut out = std::io::stdout().lock();
    for line in lines {
        writeln!(out, "{line}")?;
    }
    out.flush()
}

/// A reader that quit early (`show 3 --scrollback | head`) is not an error: stop quietly, like
/// git and coreutils do.
fn closed_pipe_is_done(result: std::io::Result<()>) -> std::io::Result<()> {
    match result {
        Err(err) if err.kind() == std::io::ErrorKind::BrokenPipe => Ok(()),
        other => other,
    }
}

/// Start the daemon unless one listens, then report as `status` does.
fn start(name: &str) -> std::io::Result<()> {
    if transport::connect(name).is_err() {
        let exe = std::env::current_exe()?;
        transport::start_daemon(name, &exe, &["--idle-exit", "0"])
            .map_err(|err| std::io::Error::new(err.kind(), format!("could not start: {err}")))?;
    }
    status(name)
}

enum Stopped {
    Done,
    /// Sessions run and `--force` was not given: nothing stopped.
    Busy,
}

/// Stop the daemon (when one runs), then report as `status` does.
fn stop(name: &str, force: bool) -> std::io::Result<Stopped> {
    let stream = match transport::connect(name) {
        Err(err) if status::daemon_absent(&err) => {
            status(name)?;
            return Ok(Stopped::Done);
        }
        Err(err) => {
            return Err(std::io::Error::new(
                err.kind(),
                format!("could not reach the server: {err}"),
            ));
        }
        Ok(stream) => stream,
    };
    let (reader, writer) = transport::halves(stream);
    let client = Client::connect(reader, writer, None, Box::new(|_| {})).map_err(|err| {
        std::io::Error::new(
            err.kind(),
            format!("could not stop the server (another version?): {err}"),
        )
    })?;
    if !force {
        let running: Vec<_> = client
            .list()?
            .into_iter()
            .filter(|info| info.exit.is_none())
            .collect();
        if !running.is_empty() {
            print_sessions(&running);
            return Ok(Stopped::Busy);
        }
    }
    client.shutdown()?;
    drop(client);
    transport::wait_until_stopped(name)?;
    status(name)?;
    Ok(Stopped::Done)
}

fn close(name: &str, id: u64) -> std::io::Result<()> {
    let client = client(name)?;
    if !client.list()?.iter().any(|info| info.id == id) {
        return Err(std::io::Error::new(
            std::io::ErrorKind::NotFound,
            format!("no session {id}"),
        ));
    }
    client.close(id)?;
    // The list answers after the close: the close was handled.
    client.list()?;
    Ok(())
}

fn usage() -> ExitCode {
    eprintln!("{USAGE}");
    ExitCode::from(2)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn machines_are_comma_separated_with_placeholders() {
        assert_eq!(machines(&[]), "-");
        assert_eq!(machines(&["box".into()]), "box");
        assert_eq!(machines(&[String::new(), "a".into(), "b".into()]), "?,a,b");
    }

    #[test]
    fn a_machine_name_cannot_break_the_columns() {
        assert_eq!(machines(&["a\tb,c\nd".into()]), "a?b?c?d");
    }

    #[test]
    fn a_closed_pipe_ends_show_quietly_but_other_write_errors_stay() {
        use std::io::{Error, ErrorKind};
        assert!(closed_pipe_is_done(Err(Error::from(ErrorKind::BrokenPipe))).is_ok());
        assert!(closed_pipe_is_done(Ok(())).is_ok());
        let other = closed_pipe_is_done(Err(Error::from(ErrorKind::PermissionDenied)));
        assert_eq!(other.unwrap_err().kind(), ErrorKind::PermissionDenied);
    }
}
