//! The tabs open when the window closed (what each runs, its last folder, its name and color),
//! where they sat in the sidebar and which one was selected, saved as a small JSON file so the
//! next start reopens them. Sessions start fresh: nothing of a program's own state is kept.

use std::fs;
use std::io;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::TabMeta;
use crate::layout::SavedLayout;
use crate::window_state::write_atomic;

/// What a saved tab runs, as the new-tab menu tells them apart.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum SavedKind {
    Shell,
    ClaudeCode,
    CopilotCli,
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

    /// Write the state to `path`, creating its folder (see `write_atomic`).
    pub fn save(&self, path: &Path) -> io::Result<()> {
        write_atomic(path, &self.to_json())
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
