//! Reopening the window where it was: the bounds and maximized state are saved to
//! `window.json` in the config folder when the window closes and read at the next start.
//!
//! They are read from the window just before it closes rather than tracked while it moves:
//! GPUI reports no bounds change when only the maximized state changes (X11 sets it from a
//! property), and the window is gone once `on_window_closed` runs.

use std::path::PathBuf;

use directories::ProjectDirs;
use gpui_kit::{App, Bounds, Pixels, Window, WindowBounds, point, px, size};
use workspace_model::{Rect, WindowMode, WindowState};

/// Size of the window when nothing usable was saved.
const DEFAULT_SIZE: (f32, f32) = (1000., 650.);

/// `name` in the app's config folder (`~/.config/agent-workspace` on Linux,
/// `%APPDATA%\agent-workspace\config` on Windows); `None` without a home folder.
pub fn config_file(name: &str) -> Option<PathBuf> {
    let dirs = ProjectDirs::from("", "", "agent-workspace")?;
    Some(dirs.config_dir().join(name))
}

fn state_path() -> Option<PathBuf> {
    config_file("window.json")
}

/// The saved bounds when they are usable and on a connected display, else the default size
/// centered on the primary display.
pub fn initial(cx: &App) -> WindowBounds {
    let displays: Vec<Rect> = cx
        .displays()
        .iter()
        .map(|display| to_rect(display.bounds()))
        .collect();
    state_path()
        .and_then(|path| WindowState::load(&path))
        .filter(|state| state.is_on_any_display(&displays))
        .map(to_window_bounds)
        .unwrap_or_else(|| {
            let (width, height) = DEFAULT_SIZE;
            WindowBounds::Windowed(Bounds::centered(None, size(px(width), px(height)), cx))
        })
}

/// Save `bounds` for the next start. A maximized or fullscreen window keeps the bounds already
/// saved and updates only the mode (see `WindowState::keeping_saved_bounds`). A failure is only
/// reported: it costs the user nothing but the default window position next time.
pub fn save(bounds: WindowBounds) {
    let Some(path) = state_path() else {
        return;
    };
    let state = to_state(bounds).keeping_saved_bounds(WindowState::load(&path));
    if let Err(err) = state.save(&path) {
        eprintln!(
            "agent-workspace: failed to save window bounds to {}: {err}",
            path.display()
        );
    }
}

/// Close the window from inside the app, saving its bounds first. Callers save the tabs first
/// (`WorkspaceView::close_window`).
pub fn close(window: &mut Window) {
    save(window.window_bounds());
    window.remove_window();
}

fn to_rect(bounds: Bounds<Pixels>) -> Rect {
    Rect {
        x: f32::from(bounds.origin.x),
        y: f32::from(bounds.origin.y),
        width: f32::from(bounds.size.width),
        height: f32::from(bounds.size.height),
    }
}

fn to_bounds(rect: Rect) -> Bounds<Pixels> {
    Bounds::new(
        point(px(rect.x), px(rect.y)),
        size(px(rect.width), px(rect.height)),
    )
}

fn to_state(bounds: WindowBounds) -> WindowState {
    let (mode, bounds) = match bounds {
        WindowBounds::Windowed(bounds) => (WindowMode::Windowed, bounds),
        WindowBounds::Maximized(bounds) => (WindowMode::Maximized, bounds),
        WindowBounds::Fullscreen(bounds) => (WindowMode::Fullscreen, bounds),
    };
    WindowState {
        bounds: to_rect(bounds),
        mode,
    }
}

fn to_window_bounds(state: WindowState) -> WindowBounds {
    let bounds = to_bounds(state.bounds);
    match state.mode {
        WindowMode::Windowed => WindowBounds::Windowed(bounds),
        WindowMode::Maximized => WindowBounds::Maximized(bounds),
        WindowMode::Fullscreen => WindowBounds::Fullscreen(bounds),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn state_round_trip_keeps_mode_and_bounds() {
        let bounds = Bounds::new(point(px(-1200.), px(40.5)), size(px(1000.), px(650.)));
        for window_bounds in [
            WindowBounds::Windowed(bounds),
            WindowBounds::Maximized(bounds),
            WindowBounds::Fullscreen(bounds),
        ] {
            assert_eq!(to_window_bounds(to_state(window_bounds)), window_bounds);
        }
        let state = to_state(WindowBounds::Maximized(bounds));
        assert_eq!(state.mode, WindowMode::Maximized);
        assert_eq!(
            state.bounds,
            Rect {
                x: -1200.,
                y: 40.5,
                width: 1000.,
                height: 650.
            }
        );
    }
}
