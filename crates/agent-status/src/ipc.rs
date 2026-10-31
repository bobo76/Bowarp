//! Hook IPC: `agentws-hook` opens one local socket connection per hook event (a Unix socket on
//! Linux, a named pipe on Windows) and sends a single line:
//! `<session id> <hook start time in µs since the Unix epoch> <hook_event_name> [notification_type]`.
//! Provider-neutral transport preserves the event and detail for the destination session to
//! interpret. Copilot mode reports use `CopilotMode` with the interaction mode as
//! `notification_type`.

use std::io::{self, BufRead, BufReader, Write};
use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::thread;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use interprocess::local_socket::prelude::*;
use interprocess::local_socket::{GenericNamespaced, ListenerOptions, Stream};
use serde_json::Value;

use workspace_model::{AgentId, HookNamePolicy};

use crate::{AgentMode, status::HookEvent};

/// Environment variable holding the session id a hook reports for.
pub const SESSION_ENV: &str = "AGENTWS_SESSION";
/// Environment variable holding the socket name of the app's [`HookServer`].
pub const PIPE_ENV: &str = "AGENTWS_PIPE";
/// Environment variable holding the registered agent id for provider protocol decoding.
pub const AGENT_ENV: &str = "AGENTWS_AGENT";

/// Longest line a hook may send; anything longer is not a hook message.
const MAX_LINE: u64 = 512;
/// How long a connection may take to send its line; a hook sends it right after connecting.
const READ_TIMEOUT: Duration = Duration::from_secs(5);
/// Most connections read at once. A hook sends its line right after connecting, so only a
/// misbehaving peer keeps one open; past this, new connections are dropped.
const MAX_READERS: usize = 16;

/// A report decoded by the session's agent integration.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AgentEvent {
    Hook(HookEvent),
    ModeChanged(AgentMode),
}

/// A report as it arrives over the wire: the transport is provider-neutral, so only the
/// destination session's agent knows what it means (see [`AgentReport::decode`]).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct AgentReport {
    pub event_name: String,
    pub detail: Option<String>,
}

impl AgentReport {
    /// What the report means to `agent`: its mode report, or a status-changing hook. Reports
    /// that change neither map to `None`.
    pub fn decode(&self, agent: AgentId) -> Option<AgentEvent> {
        if agent.mode_event_name() == Some(self.event_name.as_str()) {
            return agent
                .parse_mode(self.detail.as_deref()?)
                .map(AgentEvent::ModeChanged);
        }
        HookEvent::from_agent(agent, &self.event_name, self.detail.as_deref()).map(AgentEvent::Hook)
    }
}

/// A raw report for one session.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct HookMessage {
    pub session: u64,
    /// When the hook started, in microseconds since the Unix epoch.
    pub at: u64,
    pub report: AgentReport,
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

/// Forward a hook's JSON input (its stdin) to the server listening on `pipe_name`. `agent` is
/// `None` when the launching instance did not name one (e.g. an older app version still running
/// sessions after an upgrade): the event name is then read leniently.
pub fn forward_hook(
    agent: Option<AgentId>,
    pipe_name: &str,
    session: u64,
    at: u64,
    input: &[u8],
) -> io::Result<()> {
    let line = hook_line(agent, session, at, input)?;
    let mut stream = Stream::connect(pipe_name.to_ns_name::<GenericNamespaced>()?)?;
    stream.write_all(line.as_bytes())?;
    stream.flush()
}

/// The wire line for a hook's JSON input, newline included.
fn hook_line(agent: Option<AgentId>, session: u64, at: u64, input: &[u8]) -> io::Result<String> {
    let payload: Value = serde_json::from_slice(input)?;
    let field = |name| payload.get(name).and_then(Value::as_str);
    let policy = agent.map_or(HookNamePolicy::PascalCaseFallback, |agent| {
        agent.definition().hooks.name_policy
    });
    let event_name = policy
        .event_name(field("hook_event_name"), field("hookName"))
        .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidData, "missing hook_event_name"))?;
    let mut line = format!("{session} {at} {event_name}");
    if let Some(kind) = field("notification_type") {
        line.push(' ');
        line.push_str(kind);
    }
    line.push('\n');
    Ok(line)
}

fn parse_line(line: &str) -> Option<HookMessage> {
    let mut fields = line.split_whitespace();
    let session = fields.next()?.parse().ok()?;
    let at = fields.next()?.parse().ok()?;
    let event_name = fields.next()?;
    let detail = fields.next().map(str::to_owned);
    Some(HookMessage {
        session,
        at,
        report: AgentReport {
            event_name: event_name.to_owned(),
            detail,
        },
    })
}

/// Whether the peer runs as the same user as this process. On Linux the socket is in the
/// abstract namespace, which has no file permissions, so any local user could connect and
/// change a tab's status.
#[cfg(unix)]
fn from_same_user(stream: &Stream) -> bool {
    // SAFETY: `geteuid` has no preconditions and cannot fail.
    let own = unsafe { libc::geteuid() };
    stream.peer_creds().ok().and_then(|creds| creds.euid()) == Some(own)
}

/// Named pipes are created with a security descriptor that lets only the owner (and
/// administrators) write to them.
#[cfg(not(unix))]
fn from_same_user(_stream: &Stream) -> bool {
    true
}

/// One of the [`MAX_READERS`] reader slots, given back on drop.
struct ReaderSlot(Arc<AtomicUsize>);

impl ReaderSlot {
    fn take(readers: &Arc<AtomicUsize>) -> Option<Self> {
        if readers.fetch_add(1, Ordering::AcqRel) >= MAX_READERS {
            readers.fetch_sub(1, Ordering::AcqRel);
            return None;
        }
        Some(Self(readers.clone()))
    }
}

impl Drop for ReaderSlot {
    fn drop(&mut self) {
        self.0.fetch_sub(1, Ordering::AcqRel);
    }
}

fn read_message(stream: Stream) -> Option<HookMessage> {
    match stream.set_recv_timeout(Some(READ_TIMEOUT)) {
        Ok(()) => {}
        // Named pipes have no I/O timeouts; there a silent peer holds one reader slot.
        Err(err) if err.kind() == io::ErrorKind::Unsupported => {}
        Err(_) => return None,
    }
    let mut line = String::new();
    BufReader::new(io::Read::take(stream, MAX_LINE))
        .read_line(&mut line)
        .ok()?;
    parse_line(&line)
}

/// Receives raw agent reports from `agentws-hook` processes.
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
        let readers = Arc::new(AtomicUsize::new(0));
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
                    if !from_same_user(&stream) {
                        log::warn!("hook connection from another user refused");
                        continue;
                    }
                    let Some(slot) = ReaderSlot::take(&readers) else {
                        log::warn!("too many open hook connections, dropping one");
                        continue;
                    };
                    // A connection that never sends must not hold up the others.
                    let on_message = on_message.clone();
                    thread::spawn(move || {
                        let _slot = slot;
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
    use workspace_model::agents::{CLAUDE, COPILOT};

    use crate::WaitReason;

    fn report(event_name: &str, detail: Option<&str>) -> AgentReport {
        AgentReport {
            event_name: event_name.into(),
            detail: detail.map(Into::into),
        }
    }

    #[test]
    fn parses_lines() {
        assert_eq!(
            parse_line("7 1000 PreToolUse\n"),
            Some(HookMessage {
                session: 7,
                at: 1000,
                report: report("PreToolUse", None),
            })
        );
        assert_eq!(
            parse_line("7 1000 Notification permission_prompt\n").map(|m| m.report),
            Some(report("Notification", Some("permission_prompt")))
        );
        assert_eq!(
            parse_line("7 1000 CopilotMode autopilot\n").map(|m| m.report),
            Some(report("CopilotMode", Some("autopilot")))
        );
        assert_eq!(
            parse_line("7 1000 SessionStart\n").map(|m| m.report),
            Some(report("SessionStart", None)),
            "the transport keeps events it can't interpret"
        );
        assert_eq!(parse_line("x 1000 Stop\n"), None);
        assert_eq!(parse_line("7 Stop\n"), None);
        assert_eq!(parse_line(""), None);
    }

    #[test]
    fn reports_decode_by_the_destination_agent() {
        let permission = Some(AgentEvent::Hook(HookEvent::InputRequested(
            WaitReason::Permission,
        )));
        let reply = Some(AgentEvent::Hook(HookEvent::InputRequested(
            WaitReason::Reply,
        )));
        for agent in [CLAUDE, COPILOT] {
            let notification = |kind| report("Notification", Some(kind)).decode(agent);
            assert_eq!(notification("permission_prompt"), permission);
            assert_eq!(notification("elicitation_dialog"), reply);
            assert_eq!(
                report("PreToolUse", None).decode(agent),
                Some(AgentEvent::Hook(HookEvent::ToolStarted))
            );
            assert_eq!(report("SessionStart", None).decode(agent), None);
        }
        assert_eq!(
            report("CopilotMode", Some("autopilot")).decode(COPILOT),
            Some(AgentEvent::ModeChanged(AgentMode::Autopilot))
        );
        assert_eq!(report("CopilotMode", Some("unknown")).decode(COPILOT), None);
        assert_eq!(report("CopilotMode", None).decode(COPILOT), None);
        assert_eq!(
            report("CopilotMode", Some("autopilot")).decode(CLAUDE),
            None,
            "only the agent that declares the mode event reads it"
        );
        assert_eq!(
            report("Notification", Some("idle_prompt")).decode(CLAUDE),
            Some(AgentEvent::Hook(HookEvent::Stopped))
        );
        assert_eq!(
            report("UserPromptSubmit", None).decode(COPILOT),
            None,
            "Copilot's progress reports Working"
        );
    }

    #[test]
    fn builds_hook_lines() {
        let claude = Some(CLAUDE);
        let copilot = Some(COPILOT);
        assert_eq!(
            hook_line(claude, 7, 1000, br#"{"hook_event_name":"Stop"}"#).unwrap(),
            "7 1000 Stop\n"
        );
        assert_eq!(
            hook_line(
                claude,
                7,
                1000,
                br#"{"hook_event_name":"Notification","notification_type":"idle_prompt"}"#
            )
            .unwrap(),
            "7 1000 Notification idle_prompt\n"
        );
        let camel_case = br#"{"hookName":"permissionRequest","sessionId":"abc","toolName":"bash"}"#;
        assert_eq!(
            hook_line(copilot, 7, 1000, camel_case).unwrap(),
            "7 1000 PermissionRequest\n",
            "Copilot's camelCase payload"
        );
        assert!(
            hook_line(claude, 7, 1000, camel_case).is_err(),
            "Claude names every event"
        );
        assert!(hook_line(copilot, 7, 1000, br#"{"hookName":""}"#).is_err());
        assert!(hook_line(claude, 7, 1000, br#"{"session_id":"abc"}"#).is_err());
        assert!(hook_line(claude, 7, 1000, b"not json").is_err());
    }

    #[test]
    fn hook_lines_without_an_agent_read_either_name() {
        assert_eq!(
            hook_line(None, 7, 1000, br#"{"hook_event_name":"Stop"}"#).unwrap(),
            "7 1000 Stop\n"
        );
        assert_eq!(
            hook_line(None, 7, 1000, br#"{"hookName":"permissionRequest"}"#).unwrap(),
            "7 1000 PermissionRequest\n"
        );
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
        forward_hook(Some(CLAUDE), &pipe_name, 3, 42, input).expect("forward");

        let message = receiver
            .recv_timeout(Duration::from_secs(5))
            .expect("message");
        assert_eq!(
            message,
            HookMessage {
                session: 3,
                at: 42,
                report: report("Notification", Some("idle_prompt")),
            }
        );
    }

    #[test]
    fn reader_slots_are_bounded_and_given_back() {
        let readers = Arc::new(AtomicUsize::new(0));
        let slots: Vec<_> = (0..MAX_READERS)
            .map(|_| ReaderSlot::take(&readers).expect("free slot"))
            .collect();
        assert!(ReaderSlot::take(&readers).is_none());
        drop(slots);
        assert_eq!(readers.load(Ordering::Acquire), 0);
        assert!(ReaderSlot::take(&readers).is_some());
    }

    #[test]
    fn forward_fails_without_server() {
        let input = br#"{"hook_event_name":"Stop"}"#;
        assert!(forward_hook(Some(CLAUDE), "agentws-test-no-server.sock", 1, 1, input).is_err());
    }
}
