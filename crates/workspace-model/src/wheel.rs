//! How far the mouse wheel scrolls the terminal.

/// Lines a wheel notch scrolls on Linux.
const LINUX_NOTCH_LINES: f32 = 2.0;
/// What GPUI reports for a wheel notch on Linux (X11 and Wayland), whatever the system setting.
/// Windows reports the system's own "lines per notch", which is kept.
const GPUI_LINUX_NOTCH_LINES: f32 = 3.0;

/// The lines to scroll for a delta GPUI reports in lines: on Linux a notch scrolls
/// [`LINUX_NOTCH_LINES`] instead of GPUI's fixed three; elsewhere the delta is kept. On X11
/// touchpads (XInput2 smooth scrolling) also report lines, so they slow down by the same
/// factor; only Wayland's continuous scrolling comes in pixels. The result is also the number
/// of wheel reports (mouse reporting) or arrow keys (alternate scroll) sent to the program.
pub fn notch_lines(lines: f32, linux: bool) -> f32 {
    if !linux {
        return lines;
    }
    lines * LINUX_NOTCH_LINES / GPUI_LINUX_NOTCH_LINES
}

/// Adds up wheel deltas in lines (touchpads send fractions) and hands out whole lines.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct WheelScroll {
    remainder: f32,
}

impl WheelScroll {
    /// The whole lines to scroll now for `lines` more (positive: up, toward older output);
    /// the fraction left over waits for the next delta.
    pub fn add(&mut self, lines: f32) -> i32 {
        self.remainder += lines;
        let whole = self.remainder.trunc();
        self.remainder -= whole;
        whole as i32
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_notch_scrolls_two_lines_on_linux() {
        assert_eq!(notch_lines(3.0, true), 2.0);
        assert_eq!(notch_lines(-3.0, true), -2.0);
        assert_eq!(notch_lines(3.0, false), 3.0);
        let mut wheel = WheelScroll::default();
        for _ in 0..10 {
            assert_eq!(wheel.add(notch_lines(-3.0, true)), -2);
        }
    }

    #[test]
    fn fractions_add_up_to_whole_lines() {
        let mut wheel = WheelScroll::default();
        assert_eq!(wheel.add(0.4), 0);
        assert_eq!(wheel.add(0.4), 0);
        assert_eq!(wheel.add(0.4), 1);
        assert_eq!(wheel.add(-0.2), 0);
        assert_eq!(wheel.add(-1.5), -1);
    }
}
