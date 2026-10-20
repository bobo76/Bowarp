//! The file viewer's width beside the terminal: a share of the window, so it follows window
//! resizes, kept within limits that leave both panes usable. The share is saved in the
//! settings file as a [`ViewerFraction`].

use serde::{Deserialize, Deserializer, Serialize};

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

/// The viewer's share of the window width as saved in the settings file, kept within
/// [`ViewerFraction::MIN`]..=[`ViewerFraction::MAX`]: a value outside is clamped when read, a
/// value of the wrong type gives [`DEFAULT_VIEWER_FRACTION`]. The pixel limits
/// ([`viewer_width`]) still apply when it is drawn, as they depend on the window's width.
#[derive(Clone, Copy, Debug, PartialEq, Serialize)]
#[serde(transparent)]
pub struct ViewerFraction(f32);

impl ViewerFraction {
    /// Any drag result fits, however wide the window: the pixel limits ([`viewer_width`])
    /// apply when it is drawn.
    pub const MIN: f32 = 0.0;
    pub const MAX: f32 = 1.0;

    /// `fraction` clamped to the range; not a number gives the default.
    pub fn new(fraction: f32) -> Self {
        if fraction.is_nan() {
            return Self::default();
        }
        Self(fraction.clamp(Self::MIN, Self::MAX))
    }

    pub fn get(self) -> f32 {
        self.0
    }
}

impl Default for ViewerFraction {
    fn default() -> Self {
        Self(DEFAULT_VIEWER_FRACTION)
    }
}

impl<'de> Deserialize<'de> for ViewerFraction {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        f32::deserialize(deserializer).map(Self::new)
    }
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

    #[test]
    fn saved_fraction_is_clamped() {
        assert_eq!(ViewerFraction::new(0.3).get(), 0.3);
        assert_eq!(ViewerFraction::new(0.).get(), ViewerFraction::MIN);
        assert_eq!(ViewerFraction::new(-2.).get(), ViewerFraction::MIN);
        assert_eq!(ViewerFraction::new(1.5).get(), ViewerFraction::MAX);
        assert_eq!(
            ViewerFraction::new(f32::INFINITY).get(),
            ViewerFraction::MAX
        );
        assert_eq!(ViewerFraction::new(f32::NAN), ViewerFraction::default());
        assert_eq!(ViewerFraction::default().get(), DEFAULT_VIEWER_FRACTION);
    }

    #[test]
    fn a_drag_on_a_wide_window_fits_the_saved_range() {
        for row in [800., 1920., 3840., 5120., 7680.] {
            for x in [0., row / 2., row] {
                let fraction = fraction_at(x, row, 0.);
                assert_eq!(ViewerFraction::new(fraction).get(), fraction, "{row} {x}");
            }
        }
    }
}
