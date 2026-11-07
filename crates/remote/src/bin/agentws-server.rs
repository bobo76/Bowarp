//! `agentws-server`: runs Agent Workspace sessions on this machine for the app on another.
//!
//! - `agentws-server proxy`: what the app runs through ssh. Connects to this user's server
//!   (starting it when none runs) and relays stdin and stdout to it.
//! - `agentws-server serve [--idle-exit <seconds>]`: run the server in the foreground. It
//!   exits after the given time (default 60, 0 = never) with no session and no connection.

use std::process::ExitCode;
use std::time::Duration;

use agentws_remote::transport;

const DEFAULT_IDLE_EXIT: Duration = Duration::from_secs(60);

const USAGE: &str = "usage: agentws-server proxy | serve [--idle-exit <seconds>] | --version";

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

fn usage() -> ExitCode {
    eprintln!("{USAGE}");
    ExitCode::from(2)
}
