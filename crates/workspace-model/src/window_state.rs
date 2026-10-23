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
    /// the right ones. Without saved bounds, the reported ones are used. Full screen is saved
    /// as maximized, so the next start is not full screen (without a window manager's title
    /// bar, only F11 would leave it).
    pub fn keeping_saved_bounds(self, saved: Option<WindowState>) -> Self {
        let mode = match self.mode {
            WindowMode::Fullscreen => WindowMode::Maximized,
            mode => mode,
        };
        match (mode, saved) {
            (WindowMode::Windowed, _) => self,
            (mode, None) => Self { mode, ..self },
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

/// Tabs may contain commands with secrets; secure the temporary file before writing any.
pub(crate) fn write_private_atomic(path: &Path, text: &str) -> io::Result<()> {
    use std::io::Write;
    #[cfg(unix)]
    use std::os::unix::fs::OpenOptionsExt;

    if let Some(dir) = path.parent() {
        fs::create_dir_all(dir)?;
    }
    let temp = temp_path(path);
    match fs::remove_file(&temp) {
        Ok(()) => {}
        Err(err) if err.kind() == io::ErrorKind::NotFound => {}
        Err(err) => return Err(err),
    }
    let mut options = fs::OpenOptions::new();
    options.write(true).create_new(true);
    #[cfg(unix)]
    options.mode(0o600);
    let mut file = options.open(&temp)?;
    file.write_all(text.as_bytes())?;
    drop(file);
    fs::rename(&temp, path)
}

pub(crate) fn temp_path(path: &Path) -> std::path::PathBuf {
    path.with_extension(format!("json.{}.tmp", std::process::id()))
}

#[cfg(test)]
mod tests;
