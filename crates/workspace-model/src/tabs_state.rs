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
    /// exists. `None` (start in the app's folder) when there is none.
    pub fn launch_folder(&self) -> Option<PathBuf> {
        self.folder
            .as_deref()?
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
mod tests {
    use super::*;
    use crate::TabColor;
    use crate::git::tests::TempDir;
    use crate::layout::{SavedGroup, SavedItem};
    use crate::window_state::temp_path;

    fn tab(kind: SavedKind, folder: &str) -> SavedTab {
        SavedTab {
            kind,
            program: None,
            args: Vec::new(),
            folder: Some(folder.into()),
            meta: TabMeta::default(),
        }
    }

    fn sample() -> TabsState {
        let mut claude = tab(SavedKind::ClaudeCode, "/work/api");
        claude.program = Some("/usr/bin/claude".into());
        claude.meta.custom_name = Some("api".into());
        claude.meta.color = Some(TabColor::Pink);
        let mut wsl = tab(SavedKind::Shell, "/home/me");
        wsl.program = Some("wsl.exe".into());
        wsl.args = vec!["-d".into(), "Ubuntu".into()];
        TabsState {
            layout: SavedLayout {
                pinned: vec![claude],
                items: vec![
                    SavedItem::Tab(tab(SavedKind::Shell, "/tmp")),
                    SavedItem::Group(SavedGroup {
                        name: "Agents".into(),
                        color: Some(TabColor::Blue),
                        collapsed: true,
                        tabs: vec![tab(SavedKind::CopilotCli, "/work"), wsl],
                    }),
                ],
            },
            selected: Some(2),
        }
    }

    #[test]
    fn json_round_trip() {
        let state = sample();
        assert_eq!(TabsState::from_json(&state.to_json()), Some(state));
    }

    #[test]
    fn json_is_readable_and_leaves_out_unset_fields() {
        let json = sample().to_json();
        assert!(json.contains(r#""kind": "claude-code""#));
        assert!(json.contains(r#""custom_name": "api""#));
        assert!(json.contains(r#""color": "pink""#));
        assert!(json.contains(r#""tab": {"#));
        let plain = TabsState {
            layout: SavedLayout {
                pinned: Vec::new(),
                items: vec![SavedItem::Tab(tab(SavedKind::Shell, "/tmp"))],
            },
            selected: None,
        };
        let json = plain.to_json();
        for field in ["program", "args", "custom_name", "color", "selected"] {
            assert!(!json.contains(field), "{field} in {json}");
        }
    }

    #[test]
    fn missing_fields_take_defaults() {
        let state =
            TabsState::from_json(r#"{"layout": {"items": [{"tab": {"kind": "shell"}}]}}"#).unwrap();
        let tabs = state.layout.tabs();
        assert_eq!(tabs.len(), 1);
        assert_eq!(tabs[0].kind, SavedKind::Shell);
        assert_eq!(tabs[0].program, None);
        assert_eq!(tabs[0].folder, None);
        assert_eq!(tabs[0].meta, TabMeta::default());
        assert_eq!(state.selected, None);
        assert_eq!(TabsState::from_json("{}"), Some(TabsState::default()));
    }

    #[test]
    fn damaged_files_are_rejected() {
        for text in [
            "",
            "not json",
            r#"{"layout": {"items": [{"tab": {"kind": "vim"}}]}}"#,
            r#"{"layout": {"items": [{"tab": {"kind": "shell", "color": "teal"}}]}}"#,
            r#"{"layout": {"pinned": [{"kind": "shell"}], "items": "x"}}"#,
        ] {
            assert_eq!(TabsState::from_json(text), None, "{text}");
        }
    }

    #[test]
    fn launch_folder_falls_back_to_the_nearest_existing_parent() {
        let root = TempDir::new();
        let existing = root.0.join("project");
        fs::create_dir_all(&existing).unwrap();
        let mut saved = tab(SavedKind::Shell, "/");
        saved.folder = Some(existing.clone());
        assert_eq!(saved.launch_folder(), Some(existing.clone()));
        saved.folder = Some(existing.join("deleted").join("deeper"));
        assert_eq!(saved.launch_folder(), Some(existing));
        saved.folder = Some("relative-folder-that-does-not-exist".into());
        assert_eq!(saved.launch_folder(), None);
        saved.folder = None;
        assert_eq!(saved.launch_folder(), None);
    }

    #[test]
    fn save_then_load() {
        let root = TempDir::new();
        let path = root.0.join("nested").join("tabs.json");
        let state = sample();
        state.save(&path).unwrap();
        assert_eq!(TabsState::load(&path), Some(state));
        assert!(!temp_path(&path).exists());
        fs::write(&path, "{").unwrap();
        assert_eq!(TabsState::load(&path), None);
        assert_eq!(TabsState::load(&root.0.join("missing.json")), None);
    }
}
