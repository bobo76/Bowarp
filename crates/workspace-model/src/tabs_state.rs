//! The tabs open when the window closed (what each runs, its last folder, its name and color),
//! where they sat in the sidebar and which one was selected, saved as a small JSON file so the
//! next start reopens them. Sessions start fresh: nothing of a program's own state is kept.

use std::fs;
use std::io;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Deserializer, Serialize, Serializer};
use serde_json::Value;

use crate::layout::{SavedGroup, SavedItem, SavedLayout};
use crate::window_state::write_private_atomic;
use crate::{AgentId, TabColor, TabMeta};

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
    /// The agent's conversation when the window closed, offered to resume at the next start.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub conversation: Option<String>,
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

    /// The state in `text`; `None` when it is not JSON or not shaped as a tabs file. A tab
    /// this version can't read (a kind or a color from a newer version) is dropped, or loses
    /// only its color, so the next save keeps every other tab.
    pub fn from_json(text: &str) -> Option<Self> {
        let lenient: LenientState = serde_json::from_str(text).ok()?;
        let mut tabs = TabReader::new(lenient.selected);
        let pinned = lenient
            .layout
            .pinned
            .into_iter()
            .filter_map(|tab| tabs.read(tab))
            .collect();
        let items = lenient
            .layout
            .items
            .into_iter()
            .filter_map(|item| tabs.read_item(item))
            .collect();
        Some(Self {
            layout: SavedLayout { pinned, items },
            selected: tabs.selected,
        })
    }
}

/// A tabs file read with each tab and item left as JSON, to read them one by one.
#[derive(Deserialize)]
struct LenientState {
    #[serde(default)]
    layout: LenientLayout,
    #[serde(default)]
    selected: Option<usize>,
}

#[derive(Default, Deserialize)]
struct LenientLayout {
    #[serde(default)]
    pinned: Vec<Value>,
    #[serde(default)]
    items: Vec<Value>,
}

/// Reads saved tabs in order, keeping `selected` on the same tab when earlier ones are
/// dropped (`None` when the selected one is).
struct TabReader {
    /// How many tabs of the file were read so far, dropped ones included.
    seen: usize,
    kept: usize,
    wanted: Option<usize>,
    selected: Option<usize>,
}

impl TabReader {
    fn new(wanted: Option<usize>) -> Self {
        Self {
            seen: 0,
            kept: 0,
            wanted,
            selected: None,
        }
    }

    /// The tab in `value`; without its color when only that is unknown.
    fn read(&mut self, mut value: Value) -> Option<SavedTab> {
        let tab = serde_json::from_value(value.clone()).ok().or_else(|| {
            value.as_object_mut()?.remove("color")?;
            serde_json::from_value(value).ok()
        });
        if tab.is_some() {
            if self.wanted == Some(self.seen) {
                self.selected = Some(self.kept);
            }
            self.kept += 1;
        }
        self.seen += 1;
        tab
    }

    /// A tab or a group of the sidebar's items; a group keeps the tabs it can read and drops
    /// a color it doesn't know. `None` for an item of an unknown kind.
    fn read_item(&mut self, mut value: Value) -> Option<SavedItem<SavedTab>> {
        if let Some(tab) = value.get_mut("tab") {
            return self.read(tab.take()).map(SavedItem::Tab);
        }
        let group = value.get_mut("group")?.as_object_mut()?;
        let tabs = match group.remove("tabs") {
            Some(Value::Array(tabs)) => tabs,
            _ => Vec::new(),
        };
        if group
            .get("color")
            .is_some_and(|color| serde_json::from_value::<TabColor>(color.clone()).is_err())
        {
            group.remove("color");
        }
        let mut group: SavedGroup<SavedTab> =
            serde_json::from_value(Value::Object(std::mem::take(group))).ok()?;
        group.tabs = tabs.into_iter().filter_map(|tab| self.read(tab)).collect();
        Some(SavedItem::Group(group))
    }
}

#[cfg(test)]
mod tests;
