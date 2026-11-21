//! The machines remote tabs can run on (docs/remote.md), as the `remote_hosts` setting: a
//! list of `{"name", "destination", "server_program"}` objects, edited on the settings page or
//! by hand.

use serde::{Deserialize, Deserializer, Serialize};
use serde_json::Value;

use crate::agents::{AgentId, SessionKind};

/// The program the server side runs when a host names none.
pub const DEFAULT_SERVER_PROGRAM: &str = "agentws-server";

/// `last_program` for a shell; an agent is saved by its CLI selector.
const SHELL_PROGRAM: &str = "shell";

/// One machine remote tabs can run on.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize)]
pub struct RemoteHost {
    /// Shown in menus and tab names; empty for the destination's host part.
    pub name: String,
    /// What `ssh` connects to: `[user@]host`, or a `Host` alias from the ssh config.
    pub destination: String,
    /// `agentws-server` on the host, when it is not on the `PATH` of non-interactive ssh
    /// logins (e.g. `~/.cargo/bin/agentws-server`); empty for the default.
    pub server_program: String,
    /// The folder "Open in folder…" last opened on the host; empty for its home folder.
    #[serde(skip_serializing_if = "String::is_empty")]
    pub last_folder: String,
    /// What "Open in folder…" last opened there: `shell` or an agent's CLI selector.
    #[serde(skip_serializing_if = "String::is_empty")]
    pub last_program: String,
}

/// A text field of a [`RemoteHost`], as the settings page edits it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RemoteHostField {
    Name,
    Destination,
    ServerProgram,
}

impl RemoteHost {
    /// The name shown for the host: its own, else the destination without the user.
    pub fn label(&self) -> &str {
        let name = self.name.trim();
        if name.is_empty() {
            host_part(self.destination.trim())
        } else {
            name
        }
    }

    /// The trimmed ssh destination; `None` while it is empty (the host can't be used).
    pub fn destination(&self) -> Option<&str> {
        Some(self.destination.trim()).filter(|destination| !destination.is_empty())
    }

    /// The server program to run there.
    pub fn server_program(&self) -> &str {
        let program = self.server_program.trim();
        if program.is_empty() {
            DEFAULT_SERVER_PROGRAM
        } else {
            program
        }
    }

    /// What "Open in folder…" last opened on the host: a shell when nothing (known) was.
    pub fn last_kind(&self) -> SessionKind {
        AgentId::from_cli_selector(self.last_program.trim())
            .map_or(SessionKind::Shell, SessionKind::Agent)
    }

    pub fn field(&self, field: RemoteHostField) -> &str {
        match field {
            RemoteHostField::Name => &self.name,
            RemoteHostField::Destination => &self.destination,
            RemoteHostField::ServerProgram => &self.server_program,
        }
    }

    fn field_mut(&mut self, field: RemoteHostField) -> &mut String {
        match field {
            RemoteHostField::Name => &mut self.name,
            RemoteHostField::Destination => &mut self.destination,
            RemoteHostField::ServerProgram => &mut self.server_program,
        }
    }

    /// Read one entry of the list: its string fields, `None` for anything but an object.
    /// A field that isn't a string is empty.
    fn from_value(value: &Value) -> Option<Self> {
        let entry = value.as_object()?;
        let text = |key: &str| {
            entry
                .get(key)
                .and_then(Value::as_str)
                .unwrap_or_default()
                .to_owned()
        };
        Some(Self {
            name: text("name"),
            destination: text("destination"),
            server_program: text("server_program"),
            last_folder: text("last_folder"),
            last_program: text("last_program"),
        })
    }
}

/// The `remote_hosts` setting, in the order written. Entries that aren't objects are dropped
/// (at the next save); entries without a destination are kept, since the settings page adds
/// a host before its fields are typed, but [`RemoteHosts::usable`] leaves them out.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize)]
#[serde(transparent)]
pub struct RemoteHosts(Vec<RemoteHost>);

impl<'de> Deserialize<'de> for RemoteHosts {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let value = Value::deserialize(deserializer)?;
        let entries = value.as_array().map(Vec::as_slice).unwrap_or_default();
        Ok(Self::new(
            entries.iter().filter_map(RemoteHost::from_value).collect(),
        ))
    }
}

impl RemoteHosts {
    pub fn new(hosts: Vec<RemoteHost>) -> Self {
        Self(hosts)
    }

    pub fn all(&self) -> &[RemoteHost] {
        &self.0
    }

    /// The hosts that name a destination, with it.
    pub fn usable(&self) -> impl Iterator<Item = (&RemoteHost, &str)> {
        self.0
            .iter()
            .filter_map(|host| Some((host, host.destination()?)))
    }

    /// The host `wanted` names: by destination first, else by name (`--remote <name>`).
    pub fn find(&self, wanted: &str) -> Option<&RemoteHost> {
        let wanted = wanted.trim();
        self.usable()
            .find(|(_, destination)| *destination == wanted)
            .or_else(|| self.usable().find(|(host, _)| host.name.trim() == wanted))
            .map(|(host, _)| host)
    }

    /// The name to show for a tab on `host` (a launch's host: a destination, or a host's name
    /// from `--remote`): the label of the host it matches, else `host` itself, trimmed.
    pub fn label_for<'a>(&'a self, host: &'a str) -> &'a str {
        self.find(host).map_or(host.trim(), RemoteHost::label)
    }

    /// The destination `host` (a launch's host) connects to: the configured host's when it
    /// names one, else `host` itself, trimmed.
    pub fn destination_of<'a>(&'a self, host: &'a str) -> &'a str {
        self.find(host)
            .and_then(RemoteHost::destination)
            .unwrap_or(host.trim())
    }

    /// Whether two launch hosts (destinations, or a host's name from `--remote`) are the
    /// same configured machine.
    pub fn same_host(&self, a: &str, b: &str) -> bool {
        self.destination_of(a) == self.destination_of(b)
    }

    /// Where a launch on `destination` starts, given the selected tab's host and its folder
    /// there: that folder when the tab runs on the same host, else `None` (the home folder).
    pub fn folder_on<'a>(
        &self,
        destination: &str,
        selected: Option<(&str, Option<&'a str>)>,
    ) -> Option<&'a str> {
        let (host, folder) = selected?;
        if self.same_host(host, destination) {
            folder
        } else {
            None
        }
    }

    /// The folder "Open in folder…" offers on `destination`: the selected tab's there (see
    /// [`Self::folder_on`]), else the last one opened there, else empty (the home folder).
    pub fn offered_folder(
        &self,
        destination: &str,
        selected: Option<(&str, Option<&str>)>,
    ) -> String {
        if let Some(folder) = self.folder_on(destination, selected) {
            return folder.to_owned();
        }
        self.find(destination)
            .map(|host| host.last_folder.trim().to_owned())
            .unwrap_or_default()
    }

    /// Remember what "Open in folder…" opened on `destination`, offered there next time.
    /// Nothing for a host that isn't in the list.
    pub fn remember_launch(&mut self, destination: &str, folder: &str, kind: SessionKind) {
        let destination = destination.trim();
        let Some(host) = self
            .0
            .iter_mut()
            .find(|host| host.destination() == Some(destination))
        else {
            return;
        };
        host.last_folder = folder.trim().to_owned();
        host.last_program = kind
            .agent()
            .map_or(SHELL_PROGRAM, AgentId::cli_selector)
            .to_owned();
    }

    /// Append an empty host, for the settings page to fill in.
    pub fn add(&mut self) {
        self.0.push(RemoteHost::default());
    }

    /// Remove the host shown as `shown` at `index` (see [`Self::locate`]); nothing when it is
    /// gone.
    pub fn remove(&mut self, index: usize, shown: &RemoteHost) {
        if let Some(index) = self.locate(index, shown, None) {
            self.0.remove(index);
        }
    }

    /// Set `field` of the host shown as `shown` at `index` (see [`Self::locate`]); `shown` may
    /// already hold the new value. Nothing when the host is gone.
    pub fn set_field(
        &mut self,
        index: usize,
        shown: &RemoteHost,
        field: RemoteHostField,
        value: String,
    ) {
        if let Some(index) = self.locate(index, shown, Some(field)) {
            *self.0[index].field_mut(field) = value;
        }
    }

    /// Where the host the settings page showed as `shown` at `index` is now: the page edits a
    /// list re-read from the file, which another instance or a hand edit may have reordered.
    /// `index` when the host there matches `shown` (all fields but `except`), else the first
    /// one that does.
    fn locate(
        &self,
        index: usize,
        shown: &RemoteHost,
        except: Option<RemoteHostField>,
    ) -> Option<usize> {
        let fields = [
            RemoteHostField::Name,
            RemoteHostField::Destination,
            RemoteHostField::ServerProgram,
        ];
        let matches = |host: &RemoteHost| {
            fields
                .iter()
                .all(|&field| Some(field) == except || host.field(field) == shown.field(field))
        };
        if self.0.get(index).is_some_and(matches) {
            return Some(index);
        }
        self.0.iter().position(matches)
    }
}

/// `host` from an ssh destination `[user@]host`: how tabs and menus name it.
pub fn host_part(destination: &str) -> &str {
    destination
        .rsplit_once('@')
        .map_or(destination, |(_, host)| host)
}

#[cfg(test)]
mod tests;
