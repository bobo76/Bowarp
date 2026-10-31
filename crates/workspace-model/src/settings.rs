//! The user's settings, kept in `settings.json` in the config folder. Every field is optional
//! with a default, a field with a value of the wrong type gets its default, and fields this
//! version doesn't know (from a newer version, or misspelled) are kept and written back. A
//! file that can't be read or isn't a JSON object is left on disk untouched: the defaults
//! apply, and the first save moves the broken file to `settings.json.bak` so the user's text
//! is never lost. Every change re-reads the file first, so edits made by hand or by another
//! instance since the start are kept.

use std::fs;
use std::io;
use std::path::{Path, PathBuf};

use serde::de::DeserializeOwned;
use serde::{Deserialize, Deserializer, Serialize};
use serde_json::{Map, Value};

use crate::AgentId;
use crate::clipboard_guard::ClipboardPolicy;
use crate::keymap::KeybindingOverlay;
use crate::pane_width::{ExplorerWidth, ViewerFraction};
use crate::window_background::{WindowBlur, WindowOpacity};
use crate::window_state::write_atomic;

/// The settings. A new field gets a default (through `Default`, as `#[serde(default)]` asks
/// for), so a file without it still loads, and `#[serde(deserialize_with = "lenient")]`, so a
/// value of the wrong type falls back to `T::default()` instead of breaking the whole file.
/// A field whose default isn't its type's `Default` needs a type that has it (a newtype).
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Settings {
    /// The application UI theme.
    #[serde(deserialize_with = "lenient")]
    pub ui_theme: UiTheme,
    /// The terminal's colors: the UI theme's (the default) or a fixed scheme.
    #[serde(deserialize_with = "lenient")]
    pub terminal_colors: TerminalColors,
    /// The terminal's font family; empty (the default) for the built-in one. A family that
    /// isn't installed falls back to the built-in one too (the app checks).
    #[serde(deserialize_with = "lenient")]
    pub font_family: String,
    /// The terminal's font size.
    #[serde(deserialize_with = "lenient")]
    pub font_size: FontSize,
    /// The terminal background's opacity in percent (30-100); text stays opaque. See
    /// [`crate::Backdrop`].
    #[serde(deserialize_with = "lenient")]
    pub window_opacity: WindowOpacity,
    /// What shows through the translucent terminal background: as is, blurred or Mica.
    #[serde(deserialize_with = "lenient")]
    pub window_blur: WindowBlur,
    /// The shell the new-tab menu lists first and Ctrl+Shift+T opens, by its launcher id (`zsh`,
    /// `pwsh`, `powershell`, `git-bash`, `wsl:Ubuntu`; see the app's `Launcher::id`) or menu
    /// name (`Zsh`); empty (the default) for the automatic choice (`$SHELL`, Git Bash on
    /// Windows). A shell that isn't installed gets the automatic one too (the app checks).
    #[serde(deserialize_with = "lenient")]
    pub default_shell: String,
    /// Extra shell-style words for new Claude Code and Copilot CLI tabs.
    #[serde(deserialize_with = "lenient")]
    pub claude_args: String,
    #[serde(deserialize_with = "lenient")]
    pub copilot_args: String,
    /// What happens when a program writes the clipboard (OSC 52): ask (the default), allow
    /// or deny, in every tab.
    #[serde(deserialize_with = "lenient")]
    pub clipboard_writes: ClipboardPolicy,
    #[serde(deserialize_with = "lenient")]
    pub tab_card: TabCardFields,
    /// Whether group headers show a hover card (basic and extended); the collapsed-group
    /// status stays either way. Independent of `tab_card`.
    #[serde(deserialize_with = "lenient")]
    pub group_hover_cards: FeatureSwitch<true>,
    /// Whether activity indicators animate (the Running badge's pulse, and later Copilot's
    /// working animation); off gives static indicators and no animation timers.
    #[serde(deserialize_with = "lenient")]
    pub activity_animations: FeatureSwitch<true>,
    /// Master switch for Bowarp's command history: off stops collecting commands (memory and
    /// disk) and turns off the popup, inline suggestions and last-command capture and
    /// restoration. Stored data is kept. See [`crate::HistoryFeatures`].
    #[serde(deserialize_with = "lenient")]
    pub command_history: FeatureSwitch<true>,
    /// The HISTORY popup and its Up-key interception; needs `command_history`.
    #[serde(deserialize_with = "lenient")]
    pub history_popup: FeatureSwitch<true>,
    /// Ghost-text suggestions and their acceptance keys; needs `command_history`.
    #[serde(deserialize_with = "lenient")]
    pub inline_suggestions: FeatureSwitch<true>,
    /// Reopen the tabs saved at the last close. Off: start with a fresh tab and neither read
    /// nor write `tabs.json` (it is kept as it is). Read at start; saving stops at once.
    #[serde(deserialize_with = "lenient")]
    pub restore_tabs: FeatureSwitch<true>,
    /// Capture each tab's last shell command, save it with the tabs and offer it after a
    /// restart; needs `command_history` and `restore_tabs`. See [`crate::WorkspaceFeatures`].
    #[serde(deserialize_with = "lenient")]
    pub remember_last_command: FeatureSwitch<true>,
    /// The file explorer and file viewer panes and everything that scans, watches or loads
    /// files for them. Off hides both, closes the open files and sends file links to the
    /// system's handler; the pane widths below are kept. See [`crate::FilePaneFeatures`].
    #[serde(deserialize_with = "lenient")]
    pub file_panes: FeatureSwitch<true>,
    /// Toasts about agents (needs attention, ready for review, exited); status dots and
    /// badges stay either way. See [`crate::NotificationFeatures`].
    #[serde(deserialize_with = "lenient")]
    pub in_app_notifications: FeatureSwitch<true>,
    /// The file viewer's share of the window width, set by dragging its divider (not on the
    /// settings page).
    #[serde(deserialize_with = "lenient")]
    pub file_viewer_fraction: ViewerFraction,
    /// The file explorer's width in pixels (150-600), set by dragging its edge (not on the
    /// settings page).
    #[serde(deserialize_with = "lenient")]
    pub file_explorer_width: ExplorerWidth,
    /// Changes to the default keyboard shortcuts, key by key (see [`crate::Keymap`]): an
    /// action name binds the key, `null` unbinds it. Kept as written, unusable entries too.
    #[serde(deserialize_with = "lenient")]
    pub keybindings: KeybindingOverlay,
    /// Fields this version doesn't know, written back as they were.
    #[serde(flatten)]
    pub extra: Map<String, Value>,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum UiTheme {
    Light,
    #[default]
    Dark,
    Sepia,
}

impl UiTheme {
    /// The value as written in the settings file.
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Light => "light",
            Self::Dark => "dark",
            Self::Sepia => "sepia",
        }
    }

    /// The value written by [`UiTheme::as_str`]; anything else is `None`.
    pub fn from_name(name: &str) -> Option<Self> {
        [Self::Dark, Self::Light, Self::Sepia]
            .into_iter()
            .find(|theme| theme.as_str() == name)
    }
}

/// The terminal's color scheme: the UI theme's, or one of the themes' schemes whatever the UI.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TerminalColors {
    #[default]
    MatchTheme,
    Dark,
    Light,
    Sepia,
}

impl TerminalColors {
    /// The value as written in the settings file.
    pub fn as_str(self) -> &'static str {
        match self {
            Self::MatchTheme => "match_theme",
            Self::Dark => "dark",
            Self::Light => "light",
            Self::Sepia => "sepia",
        }
    }

    /// The value written by [`TerminalColors::as_str`]; anything else is `None`.
    pub fn from_name(name: &str) -> Option<Self> {
        [Self::MatchTheme, Self::Dark, Self::Light, Self::Sepia]
            .into_iter()
            .find(|colors| colors.as_str() == name)
    }

    /// The theme whose terminal colors apply while the UI uses `ui_theme`.
    pub fn scheme(self, ui_theme: UiTheme) -> UiTheme {
        match self {
            Self::MatchTheme => ui_theme,
            Self::Dark => UiTheme::Dark,
            Self::Light => UiTheme::Light,
            Self::Sepia => UiTheme::Sepia,
        }
    }
}

/// Optional pieces of a sidebar tab hover card. The name and shortcut are unconditional.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TabCardField {
    Status,
    Location,
    Changes,
    ProgramTitle,
    SessionKind,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct TabCardFields {
    #[serde(deserialize_with = "lenient_enabled")]
    pub enabled: bool,
    #[serde(deserialize_with = "lenient_enabled")]
    pub status: bool,
    #[serde(deserialize_with = "lenient_enabled")]
    pub location: bool,
    #[serde(deserialize_with = "lenient_enabled")]
    pub changes: bool,
    #[serde(deserialize_with = "lenient_enabled")]
    pub program_title: bool,
    #[serde(deserialize_with = "lenient_enabled")]
    pub session_kind: bool,
}

impl Default for TabCardFields {
    fn default() -> Self {
        Self {
            enabled: true,
            status: true,
            location: true,
            changes: true,
            program_title: true,
            session_kind: true,
        }
    }
}

impl TabCardFields {
    pub fn active(self) -> bool {
        self.enabled
            && (self.status
                || self.location
                || self.changes
                || self.program_title
                || self.session_kind)
    }

    pub fn keeps(self, field: TabCardField) -> bool {
        self.active()
            && match field {
                TabCardField::Status => self.status,
                TabCardField::Location => self.location,
                TabCardField::Changes => self.changes,
                TabCardField::ProgramTitle => self.program_title,
                TabCardField::SessionKind => self.session_kind,
            }
    }

    /// Do not even compute a piece that is unchecked or whose card is disabled.
    pub fn keep<T>(self, field: TabCardField, value: impl FnOnce() -> T) -> Option<T> {
        self.keeps(field).then(value)
    }
}

fn lenient_enabled<'de, D: Deserializer<'de>>(deserializer: D) -> Result<bool, D::Error> {
    Ok(Value::deserialize(deserializer)?.as_bool().unwrap_or(true))
}

impl Settings {
    /// The font family the user set, trimmed; `None` for the built-in one.
    pub fn font_family(&self) -> Option<&str> {
        Some(self.font_family.trim()).filter(|family| !family.is_empty())
    }

    /// The default shell the user set, trimmed; `None` for the automatic one.
    pub fn default_shell(&self) -> Option<&str> {
        Some(self.default_shell.trim()).filter(|shell| !shell.is_empty())
    }

    pub fn agent_arguments(&self, agent: AgentId) -> &str {
        let key = agent.argument_setting().key;
        match key {
            "claude_args" => &self.claude_args,
            "copilot_args" => &self.copilot_args,
            _ => self.extra.get(key).and_then(Value::as_str).unwrap_or(""),
        }
    }

    pub fn set_agent_arguments(&mut self, agent: AgentId, value: String) {
        let key = agent.argument_setting().key;
        match key {
            "claude_args" => self.claude_args = value,
            "copilot_args" => self.copilot_args = value,
            _ => {
                self.extra.insert(key.into(), Value::String(value));
            }
        }
    }

    pub fn from_json(text: &str) -> serde_json::Result<Self> {
        serde_json::from_str(text)
    }

    /// Pretty JSON. Fails only for a value JSON can't hold (a non-UTF-8 path).
    pub fn to_json(&self) -> serde_json::Result<String> {
        serde_json::to_string_pretty(self)
    }
}

/// A persisted on/off switch whose default is `DEFAULT`: a missing value, or one that isn't a
/// JSON boolean, reads as `DEFAULT` (through `lenient`, since `Default` gives it) and is
/// written back as a plain boolean. The pattern for every feature switch.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(transparent)]
pub struct FeatureSwitch<const DEFAULT: bool>(bool);

impl<const DEFAULT: bool> FeatureSwitch<DEFAULT> {
    pub fn new(on: bool) -> Self {
        Self(on)
    }

    pub fn is_on(self) -> bool {
        self.0
    }
}

impl<const DEFAULT: bool> Default for FeatureSwitch<DEFAULT> {
    fn default() -> Self {
        Self(DEFAULT)
    }
}

/// How a switch that needs a parent option looks: it keeps its value but can't be changed
/// while the parent is off, and says which option it needs.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DependentState {
    pub enabled: bool,
    /// Shown beside the disabled control; `None` while enabled.
    pub caption: Option<String>,
}

impl DependentState {
    pub fn new(parent_on: bool, parent_name: &str) -> Self {
        Self::requiring(&[(parent_name, parent_on)])
    }

    /// A switch that needs every parent: disabled while any is off, the caption names the
    /// first one that is.
    pub fn requiring(parents: &[(&str, bool)]) -> Self {
        let unmet = parents.iter().find(|(_, on)| !on);
        Self {
            enabled: unmet.is_none(),
            caption: unmet.map(|(name, _)| format!("Needs \"{name}\" to be on.")),
        }
    }
}

/// The terminal's font size in points, kept within [`FontSize::MIN`]..=[`FontSize::MAX`]: a
/// value outside is clamped when read, a value of the wrong type gives [`FontSize::DEFAULT`].
#[derive(Clone, Copy, Debug, PartialEq, Serialize)]
#[serde(transparent)]
pub struct FontSize(f32);

impl FontSize {
    pub const MIN: f32 = 6.0;
    pub const MAX: f32 = 72.0;
    pub const DEFAULT: f32 = 14.0;

    /// `points` clamped to the range; not a number gives the default.
    pub fn new(points: f32) -> Self {
        if points.is_nan() {
            return Self::default();
        }
        Self(points.clamp(Self::MIN, Self::MAX))
    }

    pub fn points(self) -> f32 {
        self.0
    }
}

impl Default for FontSize {
    fn default() -> Self {
        Self(Self::DEFAULT)
    }
}

impl<'de> Deserialize<'de> for FontSize {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        f32::deserialize(deserializer).map(Self::new)
    }
}

/// `deserialize_with` for every `Settings` field: a value of the wrong type (or `null`) gives
/// `T::default()` and is dropped at the next save. Fails only on invalid JSON.
fn lenient<'de, D, T>(deserializer: D) -> Result<T, D::Error>
where
    D: Deserializer<'de>,
    T: DeserializeOwned + Default,
{
    let value = Value::deserialize(deserializer)?;
    Ok(serde_json::from_value(value).unwrap_or_default())
}

/// What reading the settings file found.
#[derive(Debug, PartialEq)]
pub enum SettingsLoad {
    /// No file yet: the defaults apply.
    Missing,
    Loaded(Settings),
    /// The file can't be read or isn't valid settings; the error says why.
    Broken(String),
}

impl SettingsLoad {
    pub fn read(path: &Path) -> Self {
        match fs::read_to_string(path) {
            Ok(text) => match Settings::from_json(&text) {
                Ok(settings) => Self::Loaded(settings),
                Err(err) => Self::Broken(err.to_string()),
            },
            Err(err) if err.kind() == io::ErrorKind::NotFound => Self::Missing,
            Err(err) => Self::Broken(err.to_string()),
        }
    }
}

/// Where a broken settings file is moved before the first save replaces it.
pub fn settings_backup_path(path: &Path) -> PathBuf {
    let mut name = path.file_name().unwrap_or_default().to_os_string();
    name.push(".bak");
    path.with_file_name(name)
}

/// The settings and the file they are saved to. Without a file (no config folder) they live
/// only in memory and saving does nothing.
#[derive(Debug, Default)]
pub struct SettingsFile {
    settings: Settings,
    path: Option<PathBuf>,
    /// Why the file read last is broken, until a save moves it aside.
    load_error: Option<String>,
    /// Why the last save failed, until one succeeds.
    save_error: Option<String>,
    /// Whether `set_unsaved` changed the settings since they were last read or saved.
    unsaved: bool,
}

impl SettingsFile {
    /// The settings saved at `path`, else the defaults. A broken file is left as it is, with
    /// the reason in `load_error`.
    pub fn load(path: PathBuf) -> Self {
        let mut file = Self {
            path: Some(path),
            ..Self::default()
        };
        file.reload();
        file
    }

    pub fn settings(&self) -> &Settings {
        &self.settings
    }

    pub fn path(&self) -> Option<&Path> {
        self.path.as_deref()
    }

    /// Why the file is ignored (broken: the defaults or the last good settings apply), until a
    /// save moves it to `settings_backup_path`.
    pub fn load_error(&self) -> Option<&str> {
        self.load_error.as_deref()
    }

    /// Why the last save failed (the change still applies in memory), until a save succeeds.
    pub fn save_error(&self) -> Option<&str> {
        self.save_error.as_deref()
    }

    /// Take the settings from the file again: a good file replaces them (and any change made
    /// with `set_unsaved`), a missing or broken one keeps them (a broken one sets
    /// `load_error`). For a file edited by hand while the app runs.
    pub fn reload(&mut self) {
        let Some(path) = &self.path else {
            return;
        };
        match SettingsLoad::read(path) {
            SettingsLoad::Missing => self.load_error = None,
            SettingsLoad::Loaded(settings) => {
                self.settings = settings;
                self.load_error = None;
                self.unsaved = false;
            }
            SettingsLoad::Broken(error) => self.load_error = Some(error),
        }
    }

    /// Re-read the file (keeping changes made on disk since), apply `change` and save when it
    /// changed anything or a `set_unsaved` preview is still unsaved; `Ok(true)` when it saved.
    /// The re-read alone may change the settings
    /// too, so `Ok(false)` doesn't mean they are as before. On a failed save the change is
    /// kept in memory and the error in `save_error`.
    ///
    /// Every call that changes something writes the file: for a value dragged or stepped
    /// continuously (slider, pane width), call it once when the gesture ends, not per frame.
    pub fn update(&mut self, change: impl FnOnce(&mut Settings)) -> io::Result<bool> {
        self.reload();
        self.change_and_save(change)
    }

    /// Like `update`, but while the file is broken the change applies in memory only
    /// (`Ok(false)`, not marked unsaved: a later `update` that changes nothing still leaves the
    /// broken file alone) and the file is left as it is. For layout changes (a pane
    /// resized): the user isn't looking at the settings then, so the broken file they may be
    /// fixing in an editor mustn't be moved to `settings_backup_path` under them. The change
    /// lasts until the file is read again; the next save of a still broken file keeps it.
    pub fn update_unless_broken(&mut self, change: impl FnOnce(&mut Settings)) -> io::Result<bool> {
        self.reload();
        if self.load_error.is_some() {
            change(&mut self.settings);
            return Ok(false);
        }
        self.change_and_save(change)
    }

    fn change_and_save(&mut self, change: impl FnOnce(&mut Settings)) -> io::Result<bool> {
        let before = self.settings.clone();
        change(&mut self.settings);
        // Without a good file to re-read (missing or broken), a previewed value is already in
        // memory: still save it.
        if self.settings == before && !self.unsaved {
            return Ok(false);
        }
        self.save()?;
        Ok(true)
    }

    /// Change the settings in memory only, without reading or writing the file: a value
    /// previewed while a gesture lasts (a slider being dragged). The `update` that ends the
    /// gesture re-reads the file and saves the final value, even when it equals the preview; a
    /// `reload` of a good file drops the unsaved change, a missing or broken file keeps it.
    pub fn set_unsaved(&mut self, change: impl FnOnce(&mut Settings)) {
        change(&mut self.settings);
        self.unsaved = true;
    }

    /// Save the settings when there is no file yet, so that it can be opened (in the file
    /// viewer); an existing file, broken or not, is left as it is. `Ok(true)` when it saved.
    pub fn create_if_missing(&mut self) -> io::Result<bool> {
        let Some(path) = &self.path else {
            return Ok(false);
        };
        if SettingsLoad::read(path) != SettingsLoad::Missing {
            return Ok(false);
        }
        self.save()?;
        Ok(true)
    }

    /// Write the settings to the file (see `write_atomic`), first moving it to
    /// `settings_backup_path` (replacing an older backup) if it is broken now. Overwrites
    /// changes made on disk since the last read: `update` re-reads first.
    pub fn save(&mut self) -> io::Result<()> {
        let result = self.write();
        self.save_error = result.as_ref().err().map(ToString::to_string);
        if result.is_ok() {
            self.unsaved = false;
        }
        result
    }

    fn write(&mut self) -> io::Result<()> {
        let Some(path) = &self.path else {
            return Ok(());
        };
        let text = self.settings.to_json().map_err(io::Error::other)?;
        // Checked again: another instance or the user may have fixed or broken it since.
        if let SettingsLoad::Broken(_) = SettingsLoad::read(path) {
            match fs::rename(path, settings_backup_path(path)) {
                Ok(()) => {}
                // Removed or moved meanwhile: nothing to keep.
                Err(err) if err.kind() == io::ErrorKind::NotFound => {}
                Err(err) => return Err(err),
            }
        }
        self.load_error = None;
        write_atomic(path, &text)
    }
}

/// The installed family to draw `requested` in: its name as listed in `installed` (the
/// platform's family names), or `None` when it isn't listed. Windows matches names ignoring
/// case. Names starting with `.` (GPUI's aliases, like `.SystemUIFont`) are
/// refused: they stand for proportional fonts.
pub fn installed_font_family<'a>(
    installed: &'a [String],
    requested: &str,
    platform: crate::Platform,
) -> Option<&'a str> {
    if requested.is_empty() || requested.starts_with('.') {
        return None;
    }
    if platform == crate::Platform::Windows {
        let requested_lower = requested.to_lowercase();
        return installed
            .iter()
            .find(|name| name.to_lowercase() == requested_lower)
            .map(String::as_str);
    }
    installed
        .iter()
        .find(|name| name.as_str() == requested)
        .map(String::as_str)
}

#[cfg(test)]
mod tests;
