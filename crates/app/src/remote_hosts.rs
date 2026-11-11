//! Connections to the machines remote tabs run on (docs/remote.md): one ssh connection to
//! `agentws-server` per host, opened when the host's first tab starts and again when a tab
//! starts after the connection was lost.
//!
//! Connecting (ssh, then the server's hello) and starting the session there run on a thread
//! of their own ([`start`]), so the UI never waits on the network; so does attaching again
//! to sessions whose connection was lost ([`reattach`]).
//!
//! The settings page checks a host's server on request ([`check`]): `agentws-server status`
//! over ssh, on a thread of its own; the results ([`HostChecks`]) last for the app run.

use std::collections::HashMap;
use std::io;
use std::sync::{Arc, Mutex};
use std::thread;

use agentws_remote::status::{self, CheckOutcome, HostCheck};
use agentws_remote::{Client, LocalView, SpawnSpec};
use futures::channel::oneshot;
use gpui_kit::{App, Global};
use term_core::{EventCallback, LaunchSpec, Palette, Session, TermDimensions};

use crate::settings_file;

/// How the remote shell finds `agentws-server` for a host the settings don't list.
pub const SERVER_PROGRAM_ENV: &str = "AGENTWS_REMOTE_SERVER";

/// Where to connect: the ssh destination and the server program to run there.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct Target {
    pub destination: String,
    pub program: String,
}

type Connector = Arc<dyn Fn(&Target) -> io::Result<Client> + Send + Sync>;

/// A host's connection; `None` until the first tab there connects.
type Connection = Arc<Mutex<Option<Arc<Client>>>>;

pub struct RemoteHosts {
    connections: HashMap<String, Connection>,
    connect: Connector,
}

impl Global for RemoteHosts {}

impl Default for RemoteHosts {
    fn default() -> Self {
        Self::with_connector(Arc::new(connect_over_ssh))
    }
}

type Checker = Arc<dyn Fn(&Target) -> CheckOutcome + Send + Sync>;

/// The server checks of this app run, by target: a host edited since shows "Not checked" for
/// its new target, and a check still running when the page closes shows when it opens again.
/// Observers (the settings pane) redraw on each change.
pub struct HostChecks {
    checks: HashMap<Target, HostCheck>,
    checker: Checker,
}

impl Global for HostChecks {}

impl Default for HostChecks {
    fn default() -> Self {
        Self::with_checker(Arc::new(|target| {
            status::check(&target.destination, &target.program, status::CHECK_TIMEOUT)
        }))
    }
}

impl HostChecks {
    fn with_checker(checker: Checker) -> Self {
        Self {
            checks: HashMap::new(),
            checker,
        }
    }
}

/// A host's connection, opened on first use, from any thread.
struct HostLink {
    connection: Connection,
    connect: Connector,
    target: Target,
}

/// A remote tab's start, run off the UI thread by [`RemoteStart::run`].
pub struct RemoteStart {
    host: HostLink,
    spawn: SpawnSpec,
    size: TermDimensions,
    view: LocalView,
}

/// A session to attach to again after its connection was lost: its id on the server, and
/// how it shows here.
pub struct Reattach {
    pub server_id: u64,
    pub size: TermDimensions,
    pub view: LocalView,
}

/// Tabs attaching again to their sessions on one host, run off the UI thread by
/// [`RemoteReattach::run`].
pub struct RemoteReattach {
    host: HostLink,
    sessions: Vec<Reattach>,
}

/// A session attached again: `None` when the server no longer has it (closed there, or the
/// server restarted), else the session or why attaching failed.
pub type Reattached = io::Result<Option<Session>>;

impl RemoteHosts {
    fn with_connector(connect: Connector) -> Self {
        Self {
            connections: HashMap::new(),
            connect,
        }
    }

    /// See [`is_connected`].
    fn is_connected(&self, destination: &str) -> bool {
        let Some(connection) = self.connections.get(destination) else {
            return false;
        };
        connection
            .try_lock()
            .ok()
            .and_then(|client| client.as_ref().map(|client| client.is_connected()))
            .unwrap_or(false)
    }

    fn host(&mut self, target: Target) -> HostLink {
        let connection = self
            .connections
            .entry(target.destination.clone())
            .or_default()
            .clone();
        HostLink {
            connection,
            connect: self.connect.clone(),
            target,
        }
    }

    /// What starting `spec` on `target` as session `id` needs.
    fn prepare(
        &mut self,
        target: Target,
        spec: &LaunchSpec,
        size: TermDimensions,
        view: LocalView,
    ) -> RemoteStart {
        RemoteStart {
            host: self.host(target),
            spawn: SpawnSpec::from_launch(spec),
            size,
            view,
        }
    }

    /// What attaching again to `sessions` on `target` needs.
    fn prepare_reattach(&mut self, target: Target, sessions: Vec<Reattach>) -> RemoteReattach {
        RemoteReattach {
            host: self.host(target),
            sessions,
        }
    }
}

impl RemoteStart {
    /// Connect when the host has no live connection, then start the session there. Blocks
    /// for up to ssh's connect timeout and one round trip.
    pub fn run(self) -> io::Result<Session> {
        let host = self.host.target.destination.clone();
        let client = self.host.client()?;
        let remote = client
            .spawn(self.spawn, self.size, self.view)
            .map_err(|err| io::Error::new(err.kind(), format!("{host}: {err}")))?;
        Ok(remote.session)
    }
}

impl RemoteReattach {
    /// Connect when the host has no live connection (the error: the host is unreachable),
    /// then attach to each session there, in order. Blocks like [`RemoteStart::run`], plus
    /// two round trips per session.
    pub fn run(self) -> io::Result<Vec<Reattached>> {
        let host = self.host.target.destination.clone();
        let client = self.host.client()?;
        Ok(self
            .sessions
            .into_iter()
            .map(|reattach| {
                let remote = client
                    .reattach(reattach.server_id, reattach.size, reattach.view)
                    .map_err(|err| io::Error::new(err.kind(), format!("{host}: {err}")))?;
                Ok(remote.map(|remote| remote.session))
            })
            .collect())
    }
}

impl HostLink {
    fn client(&self) -> io::Result<Arc<Client>> {
        // Held while connecting: a second tab on the host waits for this connection instead
        // of opening one of its own.
        let mut connection = self.connection.lock().unwrap();
        if let Some(client) = connection.as_ref().filter(|client| client.is_connected()) {
            return Ok(client.clone());
        }
        let client = (self.connect)(&self.target).map_err(|err| {
            let host = &self.target.destination;
            io::Error::new(err.kind(), format!("could not connect to {host}: {err}"))
        })?;
        let client = Arc::new(client);
        *connection = Some(client.clone());
        Ok(client)
    }
}

/// The session `spec` starts on its host, as session `id`: it arrives on the receiver once
/// connected and started there (a dropped sender means the attempt panicked).
pub fn start(
    id: u64,
    spec: &LaunchSpec,
    size: TermDimensions,
    palette: Palette,
    callback: EventCallback,
    cx: &mut App,
) -> io::Result<oneshot::Receiver<io::Result<Session>>> {
    let host = spec
        .host
        .as_deref()
        .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidInput, "the launch names no host"))?;
    let target = target(host, cx);
    let checked = target.clone();
    let view = LocalView {
        id,
        palette,
        callback,
    };
    let start = cx
        .default_global::<RemoteHosts>()
        .prepare(target, spec, size, view);
    let (sender, receiver) = oneshot::channel();
    thread::Builder::new()
        .name("agentws-remote-start".into())
        .spawn(move || {
            // The tab may be gone by now: the session then ends on the server as it drops.
            let _ = sender.send(start.run());
        })?;
    Ok(forget_check_once_connected(receiver, checked, cx))
}

/// Attach again to `sessions` on `host` (a launch's host), off this thread: their results
/// arrive on the receiver in order, or why the host could not be reached (a dropped sender
/// means the attempt panicked).
pub fn reattach(
    host: &str,
    sessions: Vec<Reattach>,
    cx: &mut App,
) -> io::Result<oneshot::Receiver<io::Result<Vec<Reattached>>>> {
    let target = target(host, cx);
    let checked = target.clone();
    let reattach = cx
        .default_global::<RemoteHosts>()
        .prepare_reattach(target, sessions);
    let (sender, receiver) = oneshot::channel();
    thread::Builder::new()
        .name("agentws-remote-reattach".into())
        .spawn(move || {
            // The tabs may be gone by now: the sessions then end on the server as they drop.
            let _ = sender.send(reattach.run());
        })?;
    Ok(forget_check_once_connected(receiver, checked, cx))
}

/// `attempt` as it arrives, after `target`'s finished check is dropped when it succeeded: the
/// connection is live then and the page shows it as "Connected", not as an older check's result.
fn forget_check_once_connected<T: Send + 'static>(
    attempt: oneshot::Receiver<io::Result<T>>,
    target: Target,
    cx: &mut App,
) -> oneshot::Receiver<io::Result<T>> {
    let (sender, forwarded) = oneshot::channel();
    cx.spawn(async move |cx| {
        // A dropped sender (the attempt panicked) drops ours too, which reads the same.
        let Ok(result) = attempt.await else {
            return;
        };
        if result.is_ok() {
            cx.update(|cx| {
                let checks = &mut cx.default_global::<HostChecks>().checks;
                // A check still running stays, so Check stays disabled until it ends.
                if matches!(checks.get(&target), Some(HostCheck::Done(_))) {
                    checks.remove(&target);
                }
            });
        }
        let _ = sender.send(result);
    })
    .detach();
    forwarded
}

/// Where a host of the settings is; `None` without a destination.
pub fn host_target(host: &workspace_model::RemoteHost) -> Option<Target> {
    Some(Target {
        destination: host.destination()?.to_owned(),
        program: host.server_program().to_owned(),
    })
}

/// `target`'s last check this run, or the one running.
pub fn host_check(target: &Target, cx: &App) -> Option<HostCheck> {
    cx.try_global::<HostChecks>()?.checks.get(target).cloned()
}

/// Whether the app's connection to `destination` is live. Never waits: a connection being
/// opened (its lock held) counts as not connected.
pub fn is_connected(destination: &str, cx: &App) -> bool {
    cx.try_global::<RemoteHosts>()
        .is_some_and(|hosts| hosts.is_connected(destination))
}

/// Check `target`'s server off the UI thread, unless a check of it already runs: its state
/// is "Checking" until the outcome arrives. When the app quits first, the check's thread and
/// its ssh end with the process (ssh may outlive it until its own timeout).
pub fn check(target: Target, cx: &mut App) {
    let host_checks = cx.default_global::<HostChecks>();
    if host_checks.checks.get(&target) == Some(&HostCheck::Checking) {
        return;
    }
    host_checks
        .checks
        .insert(target.clone(), HostCheck::Checking);
    let receiver = match spawn_check(host_checks.checker.clone(), target.clone()) {
        Ok(receiver) => receiver,
        Err(err) => return finish_check(target, failed(&err.to_string()), cx),
    };
    cx.spawn(async move |cx| {
        let outcome = receiver
            .await
            .unwrap_or_else(|_| failed("the check stopped unexpectedly"));
        cx.update(|cx| finish_check(target, outcome, cx));
    })
    .detach();
}

/// Run `checker` for `target` on a thread of its own; its outcome arrives on the receiver.
fn spawn_check(checker: Checker, target: Target) -> io::Result<oneshot::Receiver<CheckOutcome>> {
    let (sender, receiver) = oneshot::channel();
    thread::Builder::new()
        .name("agentws-remote-check".into())
        .spawn(move || {
            let _ = sender.send(checker(&target));
        })?;
    Ok(receiver)
}

fn failed(message: &str) -> CheckOutcome {
    CheckOutcome::Failed {
        stderr: message.to_owned(),
    }
}

fn finish_check(target: Target, outcome: CheckOutcome, cx: &mut App) {
    cx.default_global::<HostChecks>()
        .checks
        .insert(target, HostCheck::Done(outcome));
}

/// Where `host` (a launch's host: a destination, or a host's name from the settings) is.
fn target(host: &str, cx: &App) -> Target {
    let settings = settings_file::settings(cx);
    let env_program = std::env::var(SERVER_PROGRAM_ENV)
        .ok()
        .filter(|program| !program.trim().is_empty());
    resolve_target(&settings.remote_hosts, host, env_program)
}

/// `host`'s entry in the settings when there is one; else `host` as the ssh destination, with
/// the server program from [`SERVER_PROGRAM_ENV`] or the default.
fn resolve_target(
    hosts: &workspace_model::RemoteHosts,
    host: &str,
    env_program: Option<String>,
) -> Target {
    if let Some(known) = hosts.find(host)
        && let Some(destination) = known.destination()
    {
        return Target {
            destination: destination.to_owned(),
            program: known.server_program().to_owned(),
        };
    }
    Target {
        destination: host.trim().to_owned(),
        program: env_program.unwrap_or_else(|| workspace_model::DEFAULT_SERVER_PROGRAM.to_owned()),
    }
}

fn connect_over_ssh(target: &Target) -> io::Result<Client> {
    let command = agentws_remote::ssh_command(&target.destination, &target.program)?;
    let name = target.destination.clone();
    Client::over_command(
        command,
        Box::new(move |reason| log::warn!("connection to {name} ended: {reason}")),
    )
}

#[cfg(test)]
mod tests;
