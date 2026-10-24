//! The user's settings (`workspace_model::SettingsFile`), read from `settings.json` in the
//! config folder at start and held as a global; a change re-reads the file and is saved at
//! once. A broken file is reported and left as it is until the first change moves it to
//! `settings.json.bak` (a layout change, `save_layout`, doesn't).

use std::fs;
use std::io;
use std::path::{Path, PathBuf};

use gpui_kit::component::{ActiveTheme, Theme, ThemeMode};
use gpui_kit::{App, Global};
use workspace_model::{HistoryFeatures, Settings, SettingsFile, UiTheme, WorkspaceFeatures};

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
    apply_theme(cx);
    cx.observe_global::<AppSettings>(apply_theme).detach();
}

fn apply_theme(cx: &mut App) {
    let mode = match settings(cx).ui_theme {
        UiTheme::Light => ThemeMode::Light,
        UiTheme::Dark => ThemeMode::Dark,
    };
    if cx.theme().mode != mode {
        Theme::change(mode, None, cx);
    }
}

pub fn settings(cx: &App) -> &Settings {
    cx.global::<AppSettings>().0.settings()
}

/// Whether activity indicators animate (the "Activity animations" switch). Animation code
/// asks this at draw time and starts no timer or animation while it is false.
pub fn activity_animations(cx: &App) -> bool {
    settings(cx).activity_animations.is_on()
}

/// Default settings that touch no file, for tests that change switches with `preview`.
#[cfg(test)]
pub fn init_in_memory(cx: &mut App) {
    cx.set_global(AppSettings(SettingsFile::default()));
}

/// Which history features are active (the "Command history", "History popup" and "Inline
/// suggestions" switches). All on while settings aren't loaded (unit tests).
pub fn history_features(cx: &App) -> HistoryFeatures {
    cx.try_global::<AppSettings>()
        .map_or_else(HistoryFeatures::default, |settings| {
            HistoryFeatures::from_settings(settings.0.settings())
        })
}

/// What the "Restore previous tabs" and "Remember last shell command" switches allow, with the
/// history switches they need. All on while settings aren't loaded (unit tests).
pub fn workspace_features(cx: &App) -> WorkspaceFeatures {
    cx.try_global::<AppSettings>()
        .map_or_else(WorkspaceFeatures::default, |settings| {
            WorkspaceFeatures::from_settings(settings.0.settings())
        })
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
    let result = file.update(change);
    report_save_error(file, result);
}

/// `update` for a layout change (a pane resized), which the user makes without looking at the
/// settings: while the file is broken, the change applies for this run only and the file is
/// left as it is (see `SettingsFile::update_unless_broken`), instead of being moved to
/// `settings.json.bak` while the user may be fixing it in an editor.
pub fn save_layout(cx: &mut App, change: impl FnOnce(&mut Settings)) {
    let file = &mut cx.global_mut::<AppSettings>().0;
    let result = file.update_unless_broken(change);
    report_save_error(file, result);
}

fn report_save_error(file: &SettingsFile, result: io::Result<bool>) {
    if let Err(err) = result {
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

#[cfg(test)]
mod tests {
    use gpui_kit::TestAppContext;

    use super::*;

    #[gpui::test]
    fn theme_changes_apply_live_without_restarting(cx: &mut TestAppContext) {
        cx.update(|cx| {
            gpui_kit::init(cx);
            init(cx);
            cx.set_global(AppSettings(SettingsFile::default()));
        });
        cx.run_until_parked();
        cx.update(|cx| {
            assert_eq!(cx.theme().mode, ThemeMode::Dark);
            update(cx, |settings| settings.ui_theme = UiTheme::Light);
        });
        cx.run_until_parked();
        cx.update(|cx| {
            assert_eq!(cx.theme().mode, ThemeMode::Light);
            preview(cx, |settings| settings.ui_theme = UiTheme::Dark);
        });
        cx.run_until_parked();
        cx.update(|cx| assert_eq!(cx.theme().mode, ThemeMode::Dark));
    }

    #[gpui::test]
    fn feature_switches_apply_live_and_come_back(cx: &mut TestAppContext) {
        use std::cell::Cell;
        use std::rc::Rc;

        use workspace_model::FeatureSwitch;

        cx.update(|cx| {
            gpui_kit::init(cx);
            cx.set_global(AppSettings(SettingsFile::default()));
        });
        let notified = Rc::new(Cell::new(0));
        let count = notified.clone();
        cx.update(|cx| {
            assert!(activity_animations(cx));
            assert!(settings(cx).group_hover_cards.is_on());
            cx.observe_global::<AppSettings>(move |_| count.set(count.get() + 1))
                .detach();
            preview(cx, |settings| {
                settings.activity_animations = FeatureSwitch::new(false);
                settings.group_hover_cards = FeatureSwitch::new(false);
            });
        });
        cx.run_until_parked();
        cx.update(|cx| {
            assert!(!activity_animations(cx));
            assert!(!settings(cx).group_hover_cards.is_on());
            preview(cx, |settings| {
                settings.activity_animations = FeatureSwitch::new(true);
            });
        });
        cx.update(|cx| {
            assert!(activity_animations(cx));
            assert!(!settings(cx).group_hover_cards.is_on());
        });
        assert_eq!(notified.get(), 2);
    }
}
