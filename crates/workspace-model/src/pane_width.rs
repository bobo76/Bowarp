//! The file viewer's width beside the terminal: a share of the space the sidebar and the
//! explorer leave, so it follows window resizes, kept within limits that leave both panes
//! usable. The share is saved in the settings file as a [`ViewerFraction`]. The file
//! explorer's width ([`ExplorerWidth`]) is here too.

use serde::{Deserialize, Deserializer, Serialize};

/// The viewer's share of the free width until it is dragged, and after a double-click on its
/// divider.
pub const DEFAULT_VIEWER_FRACTION: f32 = 0.45;
/// The narrowest the viewer gets, in pixels.
pub const MIN_VIEWER_WIDTH: f32 = 240.;
/// The narrowest the terminal gets beside the viewer, in pixels: about 60 columns.
pub const MIN_TERMINAL_WIDTH: f32 = 480.;

/// The viewer's width for `fraction` of the free width of a `row` of panes, `fixed` of which go
/// to the sidebar and the explorer. The terminal keeps [`MIN_TERMINAL_WIDTH`] first, then the
/// viewer [`MIN_VIEWER_WIDTH`] (when the window is too narrow for both, the terminal shrinks).
pub fn viewer_width(fraction: f32, row: f32, fixed: f32) -> f32 {
    let free = row - fixed;
    (fraction * free)
        .min(free - MIN_TERMINAL_WIDTH)
        .max(MIN_VIEWER_WIDTH)
}

/// The share of the free width the viewer gets when its divider is dragged to `x` (the viewer
/// runs from `x` to the row's right edge), within [`viewer_width`]'s limits and
/// [`ViewerFraction`]'s range. `None` when the free width is below
/// `MIN_VIEWER_WIDTH + MIN_TERMINAL_WIDTH`: the divider can't move, so a drag keeps the saved
/// share.
pub fn fraction_at(x: f32, row: f32, fixed: f32) -> Option<f32> {
    let free = row - fixed;
    if free < MIN_VIEWER_WIDTH + MIN_TERMINAL_WIDTH {
        return None;
    }
    let fraction = viewer_width((row - x) / free, row, fixed) / free;
    Some(fraction.min(ViewerFraction::MAX))
}

/// The viewer's share of the free width (see [`viewer_width`]) as saved in the settings file,
/// kept within [`ViewerFraction::MIN`]..=[`ViewerFraction::MAX`]: a value outside is clamped
/// when read, a value of the wrong type gives [`DEFAULT_VIEWER_FRACTION`]. The pixel limits
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

/// The explorer's width in pixels until it is dragged, and after a double-click on its edge.
pub const DEFAULT_EXPLORER_WIDTH: f32 = 300.;

/// The explorer's width in pixels as saved in the settings file, kept within
/// [`ExplorerWidth::MIN`]..=[`ExplorerWidth::MAX`] (half to twice the default): a value outside
/// is clamped when read, a value of the wrong type gives [`DEFAULT_EXPLORER_WIDTH`].
#[derive(Clone, Copy, Debug, PartialEq, Serialize)]
#[serde(transparent)]
pub struct ExplorerWidth(f32);

impl ExplorerWidth {
    pub const MIN: f32 = DEFAULT_EXPLORER_WIDTH / 2.;
    pub const MAX: f32 = DEFAULT_EXPLORER_WIDTH * 2.;

    /// `width` clamped to the range; not a number gives the default.
    pub fn new(width: f32) -> Self {
        if width.is_nan() {
            return Self::default();
        }
        Self(width.clamp(Self::MIN, Self::MAX))
    }

    /// The width when the explorer's edge is dragged to `x`, measured from the left edge of the
    /// row of panes, with `left` pixels (the sidebar) before the explorer.
    pub fn at(x: f32, left: f32) -> Self {
        Self::new(x - left)
    }

    pub fn get(self) -> f32 {
        self.0
    }
}

impl Default for ExplorerWidth {
    fn default() -> Self {
        Self(DEFAULT_EXPLORER_WIDTH)
    }
}

impl<'de> Deserialize<'de> for ExplorerWidth {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        f32::deserialize(deserializer).map(Self::new)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_width_is_a_share_of_the_free_space() {
        // 2000 px less the 240 px sidebar and a 360 px explorer leave 1400.
        assert_eq!(viewer_width(0.5, 2000., 600.), 700.);
        assert_eq!(viewer_width(0.5, 2000., 0.), 1000.);
        assert_eq!(viewer_width(0.25, 3000., 600.), 600.);
    }

    #[test]
    fn both_panes_keep_a_minimum() {
        // The terminal keeps its minimum: 1400 free, at most 920 for the viewer.
        assert_eq!(viewer_width(0.9, 2000., 600.), 920.);
        assert_eq!(
            viewer_width(1., 1000., 240.),
            1000. - 240. - MIN_TERMINAL_WIDTH
        );
        // The viewer keeps its minimum.
        assert_eq!(viewer_width(0.05, 2000., 600.), MIN_VIEWER_WIDTH);
        // Too narrow for both (300 free): the viewer keeps its minimum.
        assert_eq!(viewer_width(0.5, 900., 600.), MIN_VIEWER_WIDTH);
        // Nothing free at all.
        assert_eq!(viewer_width(0.5, 500., 600.), MIN_VIEWER_WIDTH);
    }

    #[test]
    fn dragging_sets_a_clamped_fraction() {
        // The viewer from 1400 to 2000 is 600 of the 1200 free.
        assert_eq!(fraction_at(1400., 2000., 800.), Some(0.5));
        assert_eq!(fraction_at(600., 1000., 0.), Some(0.4));
        // Dragged to the far left: the terminal keeps its minimum.
        assert_eq!(fraction_at(0., 2000., 800.), Some(0.6));
        // Dragged to the far right: the viewer keeps its minimum.
        assert_eq!(fraction_at(1990., 2000., 800.), Some(0.2));
        // Just enough room for both panes.
        assert!(fraction_at(10., 1320., 600.).is_some());
    }

    #[test]
    fn a_drag_in_a_window_too_narrow_for_both_panes_changes_nothing() {
        // 719 px free, 200 px free, nothing free, less than nothing free.
        assert_eq!(fraction_at(500., 1319., 600.), None);
        assert_eq!(fraction_at(500., 800., 600.), None);
        assert_eq!(fraction_at(10., 600., 600.), None);
        assert_eq!(fraction_at(10., 0., 0.), None);
        assert_eq!(fraction_at(10., 500., 600.), None);
    }

    #[test]
    fn a_drag_is_the_inverse_of_the_width() {
        let (row, fixed) = (1600., 540.);
        for x in [1100., 1200., 1300.] {
            let fraction = fraction_at(x, row, fixed).unwrap();
            let width = viewer_width(fraction, row, fixed);
            assert!((width - (row - x)).abs() < 0.01, "{x}: {width}");
        }
    }

    #[test]
    fn grabbing_the_divider_does_not_move_it() {
        // Wide, and just enough free width for both panes.
        for (row, fixed) in [(1600., 540.), (1260., 540.)] {
            for fraction in [0., 0.3, DEFAULT_VIEWER_FRACTION, 0.8, 1.] {
                let width = viewer_width(fraction, row, fixed);
                let grabbed = fraction_at(row - width, row, fixed).unwrap();
                let grabbed = viewer_width(grabbed, row, fixed);
                assert!(
                    (grabbed - width).abs() < 0.01,
                    "{row} {fraction}: {grabbed}"
                );
            }
        }
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
        for row in [300., 800., 1920., 3840., 5120., 7680.] {
            for fixed in [0., 240., 840.] {
                for x in [0., row / 2., row] {
                    let Some(fraction) = fraction_at(x, row, fixed) else {
                        continue;
                    };
                    let saved = ViewerFraction::new(fraction).get();
                    assert_eq!(saved, fraction, "{row} {fixed} {x}");
                }
            }
        }
    }

    #[test]
    fn explorer_width_is_clamped() {
        assert_eq!(ExplorerWidth::MIN, 150.);
        assert_eq!(ExplorerWidth::MAX, 600.);
        assert_eq!(ExplorerWidth::new(400.).get(), 400.);
        assert_eq!(ExplorerWidth::new(10.).get(), 150.);
        assert_eq!(ExplorerWidth::new(-3.).get(), 150.);
        assert_eq!(ExplorerWidth::new(9000.).get(), 600.);
        assert_eq!(ExplorerWidth::new(f32::INFINITY).get(), 600.);
        assert_eq!(ExplorerWidth::new(f32::NAN), ExplorerWidth::default());
        assert_eq!(ExplorerWidth::default().get(), DEFAULT_EXPLORER_WIDTH);
    }

    #[test]
    fn dragging_the_explorer_edge_sets_its_width() {
        // The edge at 540 px with the 240 px sidebar before it.
        assert_eq!(ExplorerWidth::at(540., 240.).get(), 300.);
        assert_eq!(ExplorerWidth::at(640., 240.).get(), 400.);
        assert_eq!(ExplorerWidth::at(250., 240.).get(), 150.);
        assert_eq!(ExplorerWidth::at(2000., 240.).get(), 600.);
        // No sidebar.
        assert_eq!(ExplorerWidth::at(200., 0.).get(), 200.);
    }
}
