//! The machines remote tabs can run on (docs/remote.md), as the `remote_hosts` setting: a
//! list of `{"name", "destination", "server_program"}` objects, edited on the settings page or
//! by hand.

use serde::{Deserialize, Deserializer, Serialize};
use serde_json::Value;

/// The program the server side runs when a host names none.
pub const DEFAULT_SERVER_PROGRAM: &str = "agentws-server";

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

    /// Append an empty host, for the settings page to fill in.
    pub fn add(&mut self) {
        self.0.push(RemoteHost::default());
    }

    /// Remove host `index`; nothing when there is none (the list changed meanwhile).
    pub fn remove(&mut self, index: usize) {
        if index < self.0.len() {
            self.0.remove(index);
        }
    }

    /// Set a field of host `index`; nothing when there is none.
    pub fn set_field(&mut self, index: usize, field: RemoteHostField, value: String) {
        if let Some(host) = self.0.get_mut(index) {
            *host.field_mut(field) = value;
        }
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
