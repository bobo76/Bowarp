//! The user's settings (`workspace_model::SettingsFile`), read from `settings.json` in the
//! config folder at start and held as a global; a change re-reads the file and is saved at
//! once. A broken file is reported and left as it is until the first change moves it to
//! `settings.json.bak`.

use gpui_kit::{App, Global};
use workspace_model::{Settings, SettingsFile};

use crate::window_bounds::config_file;

pub struct AppSettings(SettingsFile);

impl Global for AppSettings {}

pub fn init(cx: &mut App) {
    let file = match config_file("settings.json") {
        Some(path) => SettingsFile::load(path),
        None => SettingsFile::default(),
    };
    if let (Some(path), Some(error)) = (file.path(), file.load_error()) {
        eprintln!(
            "agent-workspace: ignoring broken settings file {} (using defaults): {error}",
            path.display()
        );
    }
    cx.set_global(AppSettings(file));
}

#[expect(dead_code, reason = "read by the settings the next steps add")]
pub fn settings(cx: &App) -> &Settings {
    cx.global::<AppSettings>().0.settings()
}

/// The settings file, for its `load_error` and `save_error` (shown on the settings page).
#[expect(dead_code, reason = "read by the settings page the next steps add")]
pub fn file(cx: &App) -> &SettingsFile {
    &cx.global::<AppSettings>().0
}

/// Re-read the file, change the settings and save them if that changed anything (see
/// `SettingsFile::update`; observers of `AppSettings` are notified either way, so changes
/// made on disk apply too). A failed save is reported and kept in `save_error`: the change
/// still applies for this run. Each change writes the file: for a dragged or stepped value,
/// call this once when the gesture ends, not on every move.
#[expect(dead_code, reason = "called by the settings page the next steps add")]
pub fn update(cx: &mut App, change: impl FnOnce(&mut Settings)) {
    let file = &mut cx.global_mut::<AppSettings>().0;
    if let Err(err) = file.update(change) {
        let path = file.path().map(|path| path.display().to_string());
        eprintln!(
            "agent-workspace: failed to save settings to {}: {err}",
            path.unwrap_or_default()
        );
    }
}
