//! Where the window was when it closed (position, size, maximized), saved as a small JSON file
//! so the next start reopens it there.

use std::fs;
use std::io;
use std::path::Path;

use serde::{Deserialize, Serialize};

/// Narrowest and shortest saved window size taken as real; anything smaller is treated as a
/// damaged file.
const MIN_SIZE: f32 = 100.;

/// Widest and tallest saved window size taken as real; anything larger is treated as a damaged
/// file (a window that big can fail to get a drawing surface at every start).
const MAX_SIZE: f32 = 16384.;

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

/// The window's bounds and mode. For a maximized or fullscreen window the bounds are the size
/// to restore to (see `keeping_saved_bounds`).
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

    /// Write the state to `path`, creating its folder (see `write_atomic`).
    pub fn save(&self, path: &Path) -> io::Result<()> {
        write_atomic(path, &self.to_json())
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
        let usable = |side: f32| (MIN_SIZE..=MAX_SIZE).contains(&side);
        (finite && usable(width) && usable(height)).then_some(state)
    }

    /// The state to save over `saved` (what the file holds now). A maximized or fullscreen
    /// window keeps the saved bounds and changes only the mode: some platforms (X11) report the
    /// maximized size for it, which would leave a screen-sized window after un-maximizing at
    /// the next start. A maximized window can't move, so the saved restore bounds are still
    /// the right ones. Without saved bounds, the reported ones are used.
    pub fn keeping_saved_bounds(self, saved: Option<WindowState>) -> Self {
        match (self.mode, saved) {
            (WindowMode::Windowed, _) | (_, None) => self,
            (mode, Some(saved)) => Self {
                bounds: saved.bounds,
                mode,
            },
        }
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

/// Write `text` to `path`, creating its folder, through a temporary file next to it so a crash
/// mid-write never leaves a truncated file.
pub(crate) fn write_atomic(path: &Path, text: &str) -> io::Result<()> {
    if let Some(dir) = path.parent() {
        fs::create_dir_all(dir)?;
    }
    // One temporary file per process, so two instances closing at once don't clash.
    let temp = temp_path(path);
    fs::write(&temp, text)?;
    fs::rename(&temp, path)
}

pub(crate) fn temp_path(path: &Path) -> std::path::PathBuf {
    path.with_extension(format!("json.{}.tmp", std::process::id()))
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
        let huge = r#"{"bounds":{"x":0,"y":0,"width":1e9,"height":600}}"#;
        assert_eq!(WindowState::from_json(huge), None);
        let tallest = r#"{"bounds":{"x":0,"y":0,"width":800,"height":16384}}"#;
        assert!(WindowState::from_json(tallest).is_some());
        let too_tall = r#"{"bounds":{"x":0,"y":0,"width":800,"height":16385}}"#;
        assert_eq!(WindowState::from_json(too_tall), None);
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
    fn on_display_limit_is_exactly_min_visible() {
        let display = [rect(0., 0., 1920., 1080.)];
        // 48 px of the window's width on screen counts, 47.9 px does not.
        assert!(windowed(rect(1872., 100., 800., 600.)).is_on_any_display(&display));
        assert!(!windowed(rect(1872.1, 100., 800., 600.)).is_on_any_display(&display));
        // Same for the height, off the bottom edge.
        assert!(windowed(rect(100., 1032., 800., 600.)).is_on_any_display(&display));
        assert!(!windowed(rect(100., 1032.1, 800., 600.)).is_on_any_display(&display));
    }

    #[test]
    fn maximized_keeps_the_saved_bounds() {
        let saved = windowed(rect(30., 40., 900., 700.));
        let screen = rect(0., 0., 1920., 1080.);
        for mode in [WindowMode::Maximized, WindowMode::Fullscreen] {
            let closing = WindowState {
                bounds: screen,
                mode,
            };
            let expected = WindowState {
                bounds: saved.bounds,
                mode,
            };
            assert_eq!(closing.keeping_saved_bounds(Some(saved)), expected);
            // Nothing saved yet: what the platform reports is all there is.
            assert_eq!(closing.keeping_saved_bounds(None), closing);
        }
    }

    #[test]
    fn windowed_replaces_the_saved_bounds() {
        let saved = WindowState {
            bounds: rect(30., 40., 900., 700.),
            mode: WindowMode::Maximized,
        };
        let closing = windowed(rect(200., 100., 1200., 800.));
        assert_eq!(closing.keeping_saved_bounds(Some(saved)), closing);
        assert_eq!(closing.keeping_saved_bounds(None), closing);
    }

    #[test]
    fn save_then_load() {
        let dir = std::env::temp_dir().join(format!("agentws-window-{}", std::process::id()));
        let path = dir.join("nested").join("window.json");
        let state = windowed(rect(30., 40., 900., 700.));
        state.save(&path).unwrap();
        assert_eq!(WindowState::load(&path), Some(state));
        assert!(!temp_path(&path).exists());
        fs::write(&path, "not json").unwrap();
        assert_eq!(WindowState::load(&path), None);
        assert_eq!(WindowState::load(&dir.join("missing.json")), None);
        fs::remove_dir_all(&dir).unwrap();
    }
}
