//! Connections to the machines remote tabs run on (docs/remote.md): one ssh connection to
//! `agentws-server` per host, opened when the host's first tab starts and again when a tab
//! starts after the connection was lost.
//!
//! Connecting (ssh, then the server's hello) and starting the session there run on a thread
//! of their own ([`start`]), so the UI never waits on the network; so does attaching again
//! to sessions whose connection was lost ([`reattach`]).

use std::collections::HashMap;
use std::io;
use std::sync::{Arc, Mutex};
use std::thread;

use agentws_remote::{Client, LocalView, SpawnSpec};
use futures::channel::oneshot;
use gpui_kit::{App, Global};
use term_core::{EventCallback, LaunchSpec, Palette, Session, TermDimensions};

use crate::settings_file;

/// How the remote shell finds `agentws-server` for a host the settings don't list.
pub const SERVER_PROGRAM_ENV: &str = "AGENTWS_REMOTE_SERVER";

/// Where to connect: the ssh destination and the server program to run there.
#[derive(Clone, Debug, PartialEq, Eq)]
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
    Ok(receiver)
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
    Ok(receiver)
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
