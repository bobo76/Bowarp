//! `agentws-server`: runs Agent Workspace sessions on this machine for the app on another.
//!
//! - `agentws-server proxy`: what the app runs through ssh. Connects to this user's server
//!   (starting it when none runs) and relays stdin and stdout to it.
//! - `agentws-server serve [--idle-exit <seconds>]`: run the server in the foreground. It
//!   exits after the given time (default 60, 0 = never) with no session and no connection.
//! - `agentws-server list`: the sessions this user's server runs.
//! - `agentws-server status`: whether this user's server runs, with this program's version and
//!   protocol and the server's session count, as `key=value` lines. Never starts the server.
//! - `agentws-server close <id>`: end one of them.

use std::process::ExitCode;
use std::time::Duration;

use agentws_remote::status::{self, DaemonState, ServerStatus};
use agentws_remote::{Client, transport};

const DEFAULT_IDLE_EXIT: Duration = Duration::from_secs(60);

const USAGE: &str = "usage: agentws-server proxy | serve [--idle-exit <seconds>] | list | \
                     status | close <id> | --version";

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
    Client::connect(reader, writer, Box::new(|_| {}))
}

fn list(name: &str) -> std::io::Result<()> {
    for info in client(name)?.list()? {
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
        println!("{}\t{kind}\t{state}\t{title}", info.id);
    }
    Ok(())
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
