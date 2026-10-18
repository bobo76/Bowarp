//! Where the window was when it closed (position, size, maximized), saved as a small JSON file
//! so the next start reopens it there.

use std::fs;
use std::io;
use std::path::Path;

use serde::{Deserialize, Serialize};

/// Narrowest and shortest saved window size taken as real; anything smaller is treated as a
/// damaged file.
const MIN_SIZE: f32 = 100.;

/// How much of the window (in logical pixels, each way) must lie on a display for the saved
/// position to be used.
const MIN_VISIBLE: f32 = 48.;

/// A rectangle in logical pixels, the platform's global screen coordinates.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct Rect {
    pub x: f32,
    pub y: f32,
    pub width: f32,
    pub height: f32,
}

impl Rect {
    fn right(&self) -> f32 {
        self.x + self.width
    }

    fn bottom(&self) -> f32 {
        self.y + self.height
    }

    /// Width and height of the overlap with `other` (zero or negative when apart).
    fn overlap(&self, other: &Rect) -> (f32, f32) {
        (
            self.right().min(other.right()) - self.x.max(other.x),
            self.bottom().min(other.bottom()) - self.y.max(other.y),
        )
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum WindowMode {
    #[default]
    Windowed,
    Maximized,
    Fullscreen,
}

/// The window's bounds and mode. For a maximized or fullscreen window the bounds are what the
/// platform reports for it (the restore size where it knows one, else the maximized size).
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct WindowState {
    pub bounds: Rect,
    #[serde(default)]
    pub mode: WindowMode,
}

impl WindowState {
    /// The state saved at `path`; `None` when the file is missing, unreadable, not valid JSON
    /// or holds an unusable size.
    pub fn load(path: &Path) -> Option<Self> {
        Self::from_json(&fs::read_to_string(path).ok()?)
    }

    /// Write the state to `path`, creating its folder. Goes through a temporary file next to
    /// it, so a crash mid-write never leaves a truncated file.
    pub fn save(&self, path: &Path) -> io::Result<()> {
        if let Some(dir) = path.parent() {
            fs::create_dir_all(dir)?;
        }
        let temp = path.with_extension("json.tmp");
        fs::write(&temp, self.to_json())?;
        fs::rename(&temp, path)
    }

    pub fn to_json(&self) -> String {
        serde_json::to_string_pretty(self).expect("window state serializes")
    }

    pub fn from_json(text: &str) -> Option<Self> {
        let state: Self = serde_json::from_str(text).ok()?;
        let Rect {
            x,
            y,
            width,
            height,
        } = state.bounds;
        let finite = [x, y, width, height].iter().all(|value| value.is_finite());
        (finite && width >= MIN_SIZE && height >= MIN_SIZE).then_some(state)
    }

    /// Whether at least `MIN_VISIBLE` pixels each way of the window lie on one of `displays`,
    /// so the user can see and grab it. False with no displays.
    pub fn is_on_any_display(&self, displays: &[Rect]) -> bool {
        displays.iter().any(|display| {
            let (width, height) = self.bounds.overlap(display);
            width >= MIN_VISIBLE && height >= MIN_VISIBLE
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn rect(x: f32, y: f32, width: f32, height: f32) -> Rect {
        Rect {
            x,
            y,
            width,
            height,
        }
    }

    fn windowed(bounds: Rect) -> WindowState {
        WindowState {
            bounds,
            mode: WindowMode::Windowed,
        }
    }

    #[test]
    fn json_round_trip() {
        let state = WindowState {
            bounds: rect(-1200., 40.5, 1000., 650.),
            mode: WindowMode::Maximized,
        };
        assert_eq!(WindowState::from_json(&state.to_json()), Some(state));
        assert!(state.to_json().contains("\"maximized\""));
    }

    #[test]
    fn missing_mode_is_windowed() {
        let text = r#"{"bounds":{"x":10,"y":20,"width":800,"height":600}}"#;
        let state = WindowState::from_json(text).unwrap();
        assert_eq!(state, windowed(rect(10., 20., 800., 600.)));
    }

    #[test]
    fn rejects_damaged_or_unusable_files() {
        assert_eq!(WindowState::from_json(""), None);
        assert_eq!(WindowState::from_json("{\"bounds\":"), None);
        assert_eq!(WindowState::from_json("[1, 2]"), None);
        let tiny = r#"{"bounds":{"x":0,"y":0,"width":20,"height":600}}"#;
        assert_eq!(WindowState::from_json(tiny), None);
        let unknown_mode = r#"{"bounds":{"x":0,"y":0,"width":800,"height":600},"mode":"tiled"}"#;
        assert_eq!(WindowState::from_json(unknown_mode), None);
    }

    #[test]
    fn on_display_needs_a_grabbable_overlap() {
        let displays = [rect(0., 0., 1920., 1080.), rect(1920., 0., 1280., 1024.)];
        assert!(windowed(rect(100., 100., 1000., 650.)).is_on_any_display(&displays));
        // On the second display only.
        assert!(windowed(rect(2000., 300., 800., 600.)).is_on_any_display(&displays));
        // Mostly off the right edge of the second display, but a corner still shows.
        assert!(windowed(rect(3100., 900., 800., 600.)).is_on_any_display(&displays));
        // Only a 20 px sliver on screen.
        assert!(!windowed(rect(3180., 100., 800., 600.)).is_on_any_display(&displays));
        // Where an unplugged display on the left used to be.
        assert!(!windowed(rect(-1500., 100., 1000., 650.)).is_on_any_display(&displays));
        // Below every display.
        assert!(!windowed(rect(100., 1100., 1000., 650.)).is_on_any_display(&displays));
        assert!(!windowed(rect(100., 100., 1000., 650.)).is_on_any_display(&[]));
    }

    #[test]
    fn save_then_load() {
        let dir = std::env::temp_dir().join(format!("agentws-window-{}", std::process::id()));
        let path = dir.join("nested").join("window.json");
        let state = windowed(rect(30., 40., 900., 700.));
        state.save(&path).unwrap();
        assert_eq!(WindowState::load(&path), Some(state));
        assert!(!path.with_extension("json.tmp").exists());
        fs::write(&path, "not json").unwrap();
        assert_eq!(WindowState::load(&path), None);
        assert_eq!(WindowState::load(&dir.join("missing.json")), None);
        fs::remove_dir_all(&dir).unwrap();
    }
}
