//! What the "File explorer and viewer" switch allows. On, both panes work as always. Off,
//! neither pane shows, none of their entry points does anything, nothing is scanned, watched
//! or loaded for them, and a file link goes to the system's handler. Pane widths
//! (`file_viewer_fraction`, `file_explorer_width`) are not touched either way.

use std::path::{Path, PathBuf};

use crate::Settings;

/// A way into the explorer or the viewer.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PaneEntry {
    /// Ctrl+Shift+| and the title bar button.
    ToggleExplorer,
    /// A file chosen in the explorer.
    ExplorerFile,
    /// A Ctrl+clicked file path or `file://` link in a terminal.
    TerminalLink,
    /// "Open settings file" on the settings page.
    SettingsFile,
}

/// What a file link does.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum FileLinkAction {
    OpenInViewer(PathBuf),
    /// Offer to open `path` with the system's handler.
    OpenExternally(PathBuf),
}

/// What to do about the switch after a settings change.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PaneChange {
    /// The panes already match the setting.
    Unchanged,
    /// Show the panes now.
    Show,
    /// Drop the panes now.
    Hide,
    /// Turned off while open files have unsaved edits: ask first, and keep the panes (the
    /// switch goes back on) unless every edit may be discarded.
    ResolveEdits,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct FilePaneFeatures {
    on: bool,
}

impl Default for FilePaneFeatures {
    fn default() -> Self {
        Self::from_settings(&Settings::default())
    }
}

impl FilePaneFeatures {
    pub fn from_settings(settings: &Settings) -> Self {
        Self {
            on: settings.file_panes.is_on(),
        }
    }

    /// The panes may show, and folders may be scanned and watched and files loaded for them.
    pub fn panes_enabled(self) -> bool {
        self.on
    }

    /// Whether `entry` does anything. Off, only the settings file and terminal links still
    /// act, and then on the system's handler (see [`Self::file_link`]).
    pub fn allows(self, entry: PaneEntry) -> bool {
        match entry {
            PaneEntry::ToggleExplorer | PaneEntry::ExplorerFile => self.on,
            PaneEntry::TerminalLink | PaneEntry::SettingsFile => true,
        }
    }

    pub fn file_link(self, path: &Path) -> FileLinkAction {
        if self.on {
            FileLinkAction::OpenInViewer(path.to_owned())
        } else {
            FileLinkAction::OpenExternally(path.to_owned())
        }
    }
}

/// How the panes (`shown` now) follow the setting (`wanted`), given how many open files have
/// unsaved edits. Turning on never needs an answer; turning off needs one only for edits.
pub fn pane_change(shown: bool, wanted: bool, unsaved_files: usize) -> PaneChange {
    match (shown, wanted) {
        (true, false) if unsaved_files > 0 => PaneChange::ResolveEdits,
        (true, false) => PaneChange::Hide,
        (false, true) => PaneChange::Show,
        _ => PaneChange::Unchanged,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::FeatureSwitch;

    fn features(on: bool) -> FilePaneFeatures {
        FilePaneFeatures::from_settings(&Settings {
            file_panes: FeatureSwitch::new(on),
            ..Settings::default()
        })
    }

    #[test]
    fn on_by_default_with_every_entry_open() {
        let all = FilePaneFeatures::default();
        assert!(all.panes_enabled());
        for entry in [
            PaneEntry::ToggleExplorer,
            PaneEntry::ExplorerFile,
            PaneEntry::TerminalLink,
            PaneEntry::SettingsFile,
        ] {
            assert!(all.allows(entry), "{entry:?}");
        }
    }

    #[test]
    fn off_closes_the_pane_entries_and_keeps_the_external_ones() {
        let off = features(false);
        assert!(!off.panes_enabled());
        assert!(!off.allows(PaneEntry::ToggleExplorer));
        assert!(!off.allows(PaneEntry::ExplorerFile));
        assert!(off.allows(PaneEntry::TerminalLink) && off.allows(PaneEntry::SettingsFile));
    }

    #[test]
    fn file_links_open_in_the_viewer_only_while_on() {
        let path = Path::new("/tmp/a.rs");
        assert_eq!(
            features(true).file_link(path),
            FileLinkAction::OpenInViewer(path.into())
        );
        assert_eq!(
            features(false).file_link(path),
            FileLinkAction::OpenExternally(path.into())
        );
    }

    #[test]
    fn unsaved_edits_block_turning_off_until_resolved() {
        assert_eq!(pane_change(true, false, 1), PaneChange::ResolveEdits);
        assert_eq!(pane_change(true, false, 3), PaneChange::ResolveEdits);
        assert_eq!(pane_change(true, false, 0), PaneChange::Hide);
    }

    #[test]
    fn turning_on_and_no_change_never_ask() {
        assert_eq!(pane_change(false, true, 0), PaneChange::Show);
        assert_eq!(pane_change(false, true, 2), PaneChange::Show);
        assert_eq!(pane_change(true, true, 2), PaneChange::Unchanged);
        assert_eq!(pane_change(false, false, 0), PaneChange::Unchanged);
    }
}
