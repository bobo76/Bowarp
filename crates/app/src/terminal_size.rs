//! The size the terminal last had on screen. Sessions start at it rather than at a placeholder
//! size: a tab resized once shown, while its shell already waits at the prompt, can lose the
//! next key (Git Bash drops it, so an accepted suggestion loses its first letter). It is saved
//! with the window bounds, so restored tabs start at it too.

use gpui_kit::{App, Global};
use term_core::TermDimensions;
use workspace_model::GridSize;

use crate::window_bounds;

/// The size before any terminal was shown and with none saved.
const DEFAULT: TermDimensions = TermDimensions {
    columns: 80,
    lines: 24,
    cell_width: 1,
    cell_height: 1,
};

struct LastTerminalSize(TermDimensions);

impl Global for LastTerminalSize {}

/// Start from the size saved when the window last closed.
pub fn init(cx: &mut App) {
    if let Some(saved) = window_bounds::saved_terminal() {
        cx.set_global(LastTerminalSize(from_grid(saved)));
    }
}

/// The size a new session starts at.
pub fn current(cx: &App) -> TermDimensions {
    cx.try_global::<LastTerminalSize>()
        .map_or(DEFAULT, |size| size.0)
}

/// Remember the size a terminal was just laid out at.
pub fn note(size: TermDimensions, cx: &mut App) {
    if current(cx) != size {
        cx.set_global(LastTerminalSize(size));
    }
}

/// The size to save with the window; none before any terminal was shown.
pub fn to_save(cx: &App) -> Option<GridSize> {
    cx.try_global::<LastTerminalSize>()
        .map(|size| to_grid(size.0))
}

fn from_grid(grid: GridSize) -> TermDimensions {
    TermDimensions {
        columns: grid.columns,
        lines: grid.lines,
        cell_width: grid.cell_width,
        cell_height: grid.cell_height,
    }
}

fn to_grid(size: TermDimensions) -> GridSize {
    GridSize {
        columns: size.columns,
        lines: size.lines,
        cell_width: size.cell_width,
        cell_height: size.cell_height,
    }
}

#[cfg(test)]
mod tests {
    use gpui_kit::TestAppContext;

    use super::*;

    #[gpui::test]
    fn new_sessions_start_at_the_last_laid_out_size(cx: &mut TestAppContext) {
        cx.update(|cx| {
            assert_eq!(current(cx), DEFAULT);
            assert_eq!(to_save(cx), None);
            let shown = TermDimensions {
                columns: 132,
                lines: 41,
                cell_width: 9,
                cell_height: 18,
            };
            note(shown, cx);
            assert_eq!(current(cx), shown);
            assert_eq!(to_save(cx).map(from_grid), Some(shown));
        });
    }
}
