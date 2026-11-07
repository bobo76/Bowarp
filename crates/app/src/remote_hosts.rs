//! Connections to the machines remote tabs run on (docs/remote.md): one ssh connection to
//! `agentws-server` per host, opened when the host's first tab starts and again when a tab
//! starts after the connection was lost.
//!
//! Connecting runs ssh and waits for the server's hello on the UI thread (bounded by ssh's
//! connect timeout); starting a session on a connected host waits for one round trip.

use std::collections::HashMap;
use std::io;

use agentws_remote::{Client, LocalView, SpawnSpec};
use gpui_kit::{App, Global};
use term_core::{EventCallback, LaunchSpec, Palette, Session, TermDimensions};

/// How the remote shell finds `agentws-server` when it is not on its `PATH`.
pub const SERVER_PROGRAM_ENV: &str = "AGENTWS_REMOTE_SERVER";
const DEFAULT_SERVER_PROGRAM: &str = "agentws-server";

type Connector = Box<dyn Fn(&str) -> io::Result<Client>>;

pub struct RemoteHosts {
    clients: HashMap<String, Client>,
    connect: Connector,
}

impl Global for RemoteHosts {}

impl Default for RemoteHosts {
    fn default() -> Self {
        Self::with_connector(Box::new(connect_over_ssh))
    }
}

impl RemoteHosts {
    fn with_connector(connect: Connector) -> Self {
        Self {
            clients: HashMap::new(),
            connect,
        }
    }

    /// The connection to `host`, opened (again) when there is none.
    fn client(&mut self, host: &str) -> io::Result<&Client> {
        if !self.clients.get(host).is_some_and(Client::is_connected) {
            let client = (self.connect)(host).map_err(|err| {
                io::Error::new(err.kind(), format!("could not connect to {host}: {err}"))
            })?;
            self.clients.insert(host.to_owned(), client);
        }
        Ok(&self.clients[host])
    }

    /// Start `spec` on its host as session `id`.
    fn spawn(
        &mut self,
        id: u64,
        spec: &LaunchSpec,
        size: TermDimensions,
        palette: Palette,
        callback: EventCallback,
    ) -> io::Result<Session> {
        let host = spec.host.as_deref().ok_or_else(|| {
            io::Error::new(io::ErrorKind::InvalidInput, "the launch names no host")
        })?;
        let view = LocalView {
            id,
            palette,
            callback,
        };
        let remote = self
            .client(host)?
            .spawn(SpawnSpec::from_launch(spec), size, view)
            .map_err(|err| io::Error::new(err.kind(), format!("{host}: {err}")))?;
        Ok(remote.session)
    }
}

/// Start `spec` (which names a host) on that host as session `id`.
pub fn spawn(
    id: u64,
    spec: &LaunchSpec,
    size: TermDimensions,
    palette: Palette,
    callback: EventCallback,
    cx: &mut App,
) -> io::Result<Session> {
    cx.default_global::<RemoteHosts>()
        .spawn(id, spec, size, palette, callback)
}

fn connect_over_ssh(host: &str) -> io::Result<Client> {
    let program = std::env::var(SERVER_PROGRAM_ENV)
        .ok()
        .filter(|program| !program.trim().is_empty())
        .unwrap_or_else(|| DEFAULT_SERVER_PROGRAM.to_owned());
    let command = agentws_remote::ssh_command(host, &program)?;
    let name = host.to_owned();
    Client::over_command(
        command,
        Box::new(move |reason| log::warn!("connection to {name} ended: {reason}")),
    )
}

#[cfg(all(test, unix))]
mod tests;
