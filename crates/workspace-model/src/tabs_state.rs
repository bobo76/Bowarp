//! The tabs open when the window closed (what each runs, its last folder, its name and color),
//! where they sat in the sidebar and which one was selected, saved as a small JSON file so the
//! next start reopens them. Sessions start fresh: nothing of a program's own state is kept.

use std::fs;
use std::io;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Deserializer, Serialize, Serializer};

use crate::AgentId;
use crate::TabMeta;
use crate::layout::{SavedItem, SavedLayout};
use crate::window_state::write_private_atomic;

/// What a saved tab runs, as the new-tab menu tells them apart.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SavedKind {
    Shell,
    Agent(AgentId),
}

impl Serialize for SavedKind {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        let value = match self {
            Self::Shell => "shell",
            Self::Agent(agent) => agent.saved_kind(),
        };
        serializer.serialize_str(value)
    }
}

impl<'de> Deserialize<'de> for SavedKind {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let value = String::deserialize(deserializer)?;
        if value == "shell" {
            return Ok(Self::Shell);
        }
        AgentId::from_saved_kind(&value)
            .map(Self::Agent)
            .ok_or_else(|| serde::de::Error::custom("unknown saved session kind"))
    }
}

/// One tab as saved: its launch and what the user set on it.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct SavedTab {
    pub kind: SavedKind,
    /// `None` starts the user's default shell.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub program: Option<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub args: Vec<String>,
    /// The folder the session was in when the window closed.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub folder: Option<PathBuf>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub last_command: Option<String>,
    #[serde(flatten)]
    pub meta: TabMeta,
}

impl SavedTab {
    /// The folder to reopen the tab in: the saved one, else its nearest parent that still
    /// exists. `None` (start in the app's folder) when there is none. Linux names a deleted
    /// folder `"<path> (deleted)"`; when no folder has that name, `<path>` is tried instead.
    pub fn launch_folder(&self) -> Option<PathBuf> {
        let folder = self.folder.as_deref()?;
        let folder = match folder
            .to_str()
            .and_then(|text| text.strip_suffix(" (deleted)"))
        {
            Some(path) if !folder.is_dir() => Path::new(path),
            _ => folder,
        };
        folder
            .ancestors()
            .find(|dir| dir.is_dir())
            .map(Path::to_path_buf)
    }
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct TabsState {
    #[serde(default)]
    pub layout: SavedLayout<SavedTab>,
    /// Index of the selected tab in `layout.tabs()`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub selected: Option<usize>,
}

impl TabsState {
    /// The state saved at `path`; `None` when the file is missing, unreadable or not valid
    /// JSON.
    pub fn load(path: &Path) -> Option<Self> {
        Self::from_json(&fs::read_to_string(path).ok()?)
    }

    /// Write atomically, creating the folder and using owner-only permissions on Unix.
    pub fn save(&self, path: &Path) -> io::Result<()> {
        write_private_atomic(path, &self.to_json())
    }

    /// Remove the saved last command of every tab (`only: None`) or of the tabs whose last
    /// command is exactly `only`. Everything else stays as it is. True when something changed.
    pub fn scrub_last_commands(&mut self, only: Option<&str>) -> bool {
        let mut changed = false;
        let mut scrub = |tab: &mut SavedTab| {
            let matches = tab.last_command.is_some()
                && only.is_none_or(|command| tab.last_command.as_deref() == Some(command));
            if matches {
                tab.last_command = None;
                changed = true;
            }
        };
        let layout = &mut self.layout;
        layout.pinned.iter_mut().for_each(&mut scrub);
        for item in &mut layout.items {
            match item {
                SavedItem::Tab(tab) => scrub(tab),
                SavedItem::Group(group) => group.tabs.iter_mut().for_each(&mut scrub),
            }
        }
        changed
    }

    /// Scrub the last commands (see `scrub_last_commands`) from the file at `path` and write it
    /// back only when one was removed. A missing file is nothing to do; a file that exists but
    /// can't be read or parsed is an error and is left untouched.
    pub fn scrub_file(path: &Path, only: Option<&str>) -> io::Result<()> {
        let text = match fs::read_to_string(path) {
            Ok(text) => text,
            Err(err) if err.kind() == io::ErrorKind::NotFound => return Ok(()),
            Err(err) => return Err(err),
        };
        let Some(mut state) = Self::from_json(&text) else {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "not a valid tabs file",
            ));
        };
        if !state.scrub_last_commands(only) {
            return Ok(());
        }
        state.save(path)
    }

    pub fn to_json(&self) -> String {
        serde_json::to_string_pretty(self).expect("tabs state serializes")
    }

    pub fn from_json(text: &str) -> Option<Self> {
        serde_json::from_str(text).ok()
    }
}

#[cfg(test)]
mod tests;
