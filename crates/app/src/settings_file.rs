//! The user's settings (`workspace_model::SettingsFile`), read from `settings.json` in the
//! config folder at start and held as a global; a change re-reads the file and is saved at
//! once. A broken file is reported and left as it is until the first change moves it to
//! `settings.json.bak`.

use std::fs;
use std::path::{Path, PathBuf};

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

pub fn settings(cx: &App) -> &Settings {
    cx.global::<AppSettings>().0.settings()
}

/// The settings file, for its `load_error` and `save_error` (shown on the settings page).
pub fn file(cx: &App) -> &SettingsFile {
    &cx.global::<AppSettings>().0
}

/// Re-read the file, change the settings and save them if that changed anything (see
/// `SettingsFile::update`; observers of `AppSettings` are notified either way, so changes
/// made on disk apply too). A failed save is reported and kept in `save_error`: the change
/// still applies for this run. Each change writes the file: for a dragged or stepped value,
/// call this once when the gesture ends, not on every move.
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

/// Change the settings for this run without saving them (see `SettingsFile::set_unsaved`),
/// notifying observers so the change applies live: for each step of a gesture (a slider
/// moved). Call `update` with the final value when the gesture ends.
pub fn preview(cx: &mut App, change: impl FnOnce(&mut Settings)) {
    cx.global_mut::<AppSettings>().0.set_unsaved(change);
}

/// Read the file again (see `SettingsFile::reload`) and notify observers: it may have been
/// edited by hand (the settings page's load error goes once the file is fixed).
pub fn reload(cx: &mut App) {
    cx.global_mut::<AppSettings>().0.reload();
}

/// Whether `path` is the settings file (as opened from the settings page, or another
/// spelling of it).
pub fn is_file(cx: &App, path: &Path) -> bool {
    let Some(file) = file(cx).path() else {
        return false;
    };
    file == path
        || fs::canonicalize(file)
            .is_ok_and(|file| fs::canonicalize(path).is_ok_and(|path| path == file))
}

/// The settings file to open in the file viewer, first written with the current settings
/// when there is none yet. `None` without a config folder, or when that write failed (the
/// error is kept in `save_error`, shown on the settings page).
pub fn file_to_open(cx: &mut App) -> Option<PathBuf> {
    let file = &mut cx.global_mut::<AppSettings>().0;
    let path = file.path()?.to_path_buf();
    if let Err(err) = file.create_if_missing() {
        eprintln!(
            "agent-workspace: failed to create the settings file {}: {err}",
            path.display()
        );
        return None;
    }
    Some(path)
}
