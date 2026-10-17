//! Hook IPC: `agentws-hook` opens one local socket connection per hook event (a Unix socket on
//! Linux, a named pipe on Windows) and sends a single line:
//! `<session id> <hook start time in µs since the Unix epoch> <hook_event_name> [notification_type]`.

use std::io::{self, BufRead, BufReader, Write};
use std::sync::Arc;
use std::thread;
use std::time::{SystemTime, UNIX_EPOCH};

use interprocess::local_socket::prelude::*;
use interprocess::local_socket::{GenericNamespaced, ListenerOptions, Stream};
use serde_json::Value;

use crate::status::HookEvent;

/// Environment variable holding the session id a hook reports for.
pub const SESSION_ENV: &str = "AGENTWS_SESSION";
/// Environment variable holding the socket name of the app's [`HookServer`].
pub const PIPE_ENV: &str = "AGENTWS_PIPE";

/// Longest line a hook may send; anything longer is not a hook message.
const MAX_LINE: u64 = 512;

/// A status-changing hook event for one session.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct HookMessage {
    pub session: u64,
    /// When the hook started, in microseconds since the Unix epoch.
    pub at: u64,
    pub event: HookEvent,
}

pub fn now_micros() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |elapsed| elapsed.as_micros() as u64)
}

/// A socket name unique to this process.
pub fn default_pipe_name() -> String {
    format!("agentws-{}.sock", std::process::id())
}

/// Forward a hook's JSON input (its stdin) to the server listening on `pipe_name`.
pub fn forward_hook(pipe_name: &str, session: u64, at: u64, input: &[u8]) -> io::Result<()> {
    let payload: Value = serde_json::from_slice(input)?;
    let field = |name| payload.get(name).and_then(Value::as_str);
    let event_name = field("hook_event_name")
        .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidData, "missing hook_event_name"))?;
    let mut line = format!("{session} {at} {event_name}");
    if let Some(kind) = field("notification_type") {
        line.push(' ');
        line.push_str(kind);
    }
    line.push('\n');

    let mut stream = Stream::connect(pipe_name.to_ns_name::<GenericNamespaced>()?)?;
    stream.write_all(line.as_bytes())?;
    stream.flush()
}

fn parse_line(line: &str) -> Option<HookMessage> {
    let mut fields = line.split_whitespace();
    let session = fields.next()?.parse().ok()?;
    let at = fields.next()?.parse().ok()?;
    let event_name = fields.next()?;
    let event = HookEvent::from_hook(event_name, fields.next())?;
    Some(HookMessage { session, at, event })
}

fn read_message(stream: Stream) -> Option<HookMessage> {
    let mut line = String::new();
    BufReader::new(io::Read::take(stream, MAX_LINE))
        .read_line(&mut line)
        .ok()?;
    parse_line(&line)
}

/// Receives hook events from `agentws-hook` processes.
///
/// The listener thread runs for the rest of the process; there is one server per app.
pub struct HookServer {
    pipe_name: String,
}

impl HookServer {
    /// Listen on `pipe_name`. `on_message` is called on a server thread for every event that
    /// affects a session's status.
    pub fn start(
        pipe_name: String,
        on_message: impl Fn(HookMessage) + Send + Sync + 'static,
    ) -> io::Result<Self> {
        let listener = ListenerOptions::new()
            .name(pipe_name.as_str().to_ns_name::<GenericNamespaced>()?)
            .create_sync()?;
        let on_message = Arc::new(on_message);
        thread::Builder::new()
            .name("agentws-hooks".into())
            .spawn(move || {
                for connection in listener.incoming() {
                    let stream = match connection {
                        Ok(stream) => stream,
                        Err(err) => {
                            log::warn!("hook connection failed: {err}");
                            continue;
                        }
                    };
                    // A connection that never sends must not hold up the others.
                    let on_message = on_message.clone();
                    thread::spawn(move || {
                        if let Some(message) = read_message(stream) {
                            on_message(message);
                        }
                    });
                }
            })?;
        Ok(Self { pipe_name })
    }

    pub fn pipe_name(&self) -> &str {
        &self.pipe_name
    }
}

#[cfg(test)]
mod tests {
    use std::sync::mpsc;
    use std::time::Duration;

    use super::*;

    #[test]
    fn parses_lines() {
        assert_eq!(
            parse_line("7 1000 PreToolUse\n"),
            Some(HookMessage {
                session: 7,
                at: 1000,
                event: HookEvent::ToolStarted
            })
        );
        assert_eq!(
            parse_line("7 1000 Notification permission_prompt\n").map(|m| m.event),
            Some(HookEvent::InputRequested)
        );
        assert_eq!(parse_line("7 1000 SessionStart\n"), None);
        assert_eq!(parse_line("x 1000 Stop\n"), None);
        assert_eq!(parse_line("7 Stop\n"), None);
        assert_eq!(parse_line(""), None);
    }

    #[test]
    fn forwards_hook_input_to_server() {
        let pipe_name = format!("agentws-test-{}.sock", now_micros());
        let (sender, receiver) = mpsc::channel();
        let _server = HookServer::start(pipe_name.clone(), move |message| {
            let _ = sender.send(message);
        })
        .expect("start server");

        let input = br#"{"session_id":"abc","hook_event_name":"Notification","notification_type":"idle_prompt","message":"waiting"}"#;
        forward_hook(&pipe_name, 3, 42, input).expect("forward");

        let message = receiver
            .recv_timeout(Duration::from_secs(5))
            .expect("message");
        assert_eq!(
            message,
            HookMessage {
                session: 3,
                at: 42,
                event: HookEvent::Stopped
            }
        );
    }

    #[test]
    fn forward_fails_without_server() {
        let input = br#"{"hook_event_name":"Stop"}"#;
        assert!(forward_hook("agentws-test-no-server.sock", 1, 1, input).is_err());
    }
}
