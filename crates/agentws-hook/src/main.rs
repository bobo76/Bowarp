//! Forwards hook JSON to the Agent Workspace session that started the agent. Claude Code hooks
//! always exit 0 and print nothing; the Copilot extension requests errors with `--report-errors`.

use std::ffi::OsStr;
use std::io::{self, Read};

use agent_status::{PIPE_ENV, SESSION_ENV, forward_hook, now_micros};

fn main() {
    let result = run();
    if std::env::args_os().nth(1).as_deref() == Some(OsStr::new("--report-errors"))
        && let Err(error) = result
    {
        eprintln!("agentws-hook: {error}");
        std::process::exit(1);
    }
}

fn run() -> io::Result<()> {
    let at = now_micros();
    // Read the input even when there is nothing to forward: exiting first would make the
    // writer's still-pending write fail with a broken pipe.
    let mut input = Vec::new();
    io::stdin().read_to_end(&mut input)?;
    let pipe_name = std::env::var(PIPE_ENV)
        .map_err(|error| io::Error::new(io::ErrorKind::InvalidInput, error))?;
    let session = std::env::var(SESSION_ENV)
        .map_err(|error| io::Error::new(io::ErrorKind::InvalidInput, error))?
        .parse()
        .map_err(|error| io::Error::new(io::ErrorKind::InvalidInput, error))?;
    forward_hook(&pipe_name, session, at, &input)
}
