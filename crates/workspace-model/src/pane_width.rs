//! The file viewer's width beside the terminal: a share of the window, so it follows window
//! resizes, kept within limits that leave both panes usable.

/// The viewer's share of the window width until it is dragged, and after a double-click on its
/// divider.
pub const DEFAULT_VIEWER_FRACTION: f32 = 0.45;
/// The narrowest the viewer gets, in pixels.
pub const MIN_VIEWER_WIDTH: f32 = 240.;
/// The narrowest the terminal gets, in pixels.
pub const MIN_TERMINAL_WIDTH: f32 = 240.;

/// The viewer's width for `fraction` of a `row` of panes `fixed` of which go to the sidebar and
/// the explorer. The terminal keeps [`MIN_TERMINAL_WIDTH`] first, then the viewer
/// [`MIN_VIEWER_WIDTH`] (when the window is too narrow for both, the terminal shrinks).
pub fn viewer_width(fraction: f32, row: f32, fixed: f32) -> f32 {
    let widest = row - fixed - MIN_TERMINAL_WIDTH;
    (fraction * row).min(widest).max(MIN_VIEWER_WIDTH)
}

/// The share of the `row` the viewer gets when its divider is dragged to `x` (the viewer runs
/// from `x` to the row's right edge), within [`viewer_width`]'s limits.
pub fn fraction_at(x: f32, row: f32, fixed: f32) -> f32 {
    if row <= 0. {
        return DEFAULT_VIEWER_FRACTION;
    }
    viewer_width((row - x) / row, row, fixed) / row
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_width_follows_the_row() {
        assert_eq!(viewer_width(0.5, 1000., 240.), 500.);
        assert_eq!(viewer_width(0.5, 2000., 240.), 1000.);
    }

    #[test]
    fn both_panes_keep_a_minimum() {
        // The terminal keeps its minimum.
        assert_eq!(viewer_width(0.9, 1000., 240.), 520.);
        // The viewer keeps its minimum.
        assert_eq!(viewer_width(0.05, 1000., 240.), MIN_VIEWER_WIDTH);
        // Too narrow for both: the viewer keeps its minimum.
        assert_eq!(viewer_width(0.5, 500., 240.), MIN_VIEWER_WIDTH);
    }

    #[test]
    fn dragging_sets_a_clamped_fraction() {
        assert_eq!(fraction_at(600., 1000., 0.), 0.4);
        assert_eq!(fraction_at(0., 1000., 240.), 0.52);
        assert_eq!(fraction_at(990., 1000., 0.), 0.24);
        assert_eq!(fraction_at(10., 0., 0.), DEFAULT_VIEWER_FRACTION);
    }
}
