//! Overlay scroll bar for the scrollback history.
//!
//! The bar sits in the terminal's right padding, so it costs no column. It is shown while the
//! user scrolls, hovers or drags it, and hidden when there is no history: the alternate screen
//! (Claude Code, full-screen programs) has none, and the terminal cannot know how those
//! programs scroll their own content.

use std::time::Duration;

use gpui_kit::{Bounds, Pixels, Task};

const MIN_THUMB_HEIGHT: f32 = 20.0;
/// How long the bar stays visible after the last scroll.
pub const LINGER: Duration = Duration::from_millis(1200);

/// Scroll position of the visible screen within the history, in lines.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ScrollMetrics {
    /// Lines of history above the screen.
    pub history: usize,
    /// Lines the screen is scrolled back into history; 0 is the live screen.
    pub display_offset: usize,
    pub screen_lines: usize,
}

impl ScrollMetrics {
    /// Top and height of the thumb within a track `track_height` tall, or `None` when there is
    /// no history to scroll.
    pub fn thumb(&self, track_height: f32) -> Option<(f32, f32)> {
        if self.history == 0 {
            return None;
        }
        let total = (self.history + self.screen_lines) as f32;
        let height = (track_height * self.screen_lines as f32 / total)
            .max(MIN_THUMB_HEIGHT)
            .min(track_height);
        let travel = track_height - height;
        let top = travel * (self.history - self.display_offset) as f32 / self.history as f32;
        Some((top, height))
    }

    /// The display offset that puts the thumb's top at `thumb_top`, clamped to the history.
    pub fn offset_for_thumb_top(&self, track_height: f32, thumb_top: f32) -> usize {
        let Some((_, height)) = self.thumb(track_height) else {
            return 0;
        };
        let travel = track_height - height;
        if travel <= 0.0 {
            return self.display_offset;
        }
        let fraction = (thumb_top / travel).clamp(0.0, 1.0);
        self.history - (fraction * self.history as f32).round() as usize
    }
}

/// Where the bar was painted, for hit-testing pointer events.
#[derive(Clone, Copy, Debug)]
pub struct ScrollbarLayout {
    pub track: Bounds<Pixels>,
    pub metrics: ScrollMetrics,
}

impl ScrollbarLayout {
    /// Thumb top and height relative to the track top.
    pub fn thumb(&self) -> Option<(f32, f32)> {
        self.metrics.thumb(f32::from(self.track.size.height))
    }

    /// The display offset that puts the thumb's top `grab` pixels above the pointer at `y`.
    pub fn offset_for_pointer(&self, y: Pixels, grab: f32) -> usize {
        let thumb_top = f32::from(y - self.track.origin.y) - grab;
        self.metrics
            .offset_for_thumb_top(f32::from(self.track.size.height), thumb_top)
    }
}

/// Interaction state of a terminal's scroll bar.
#[derive(Default)]
pub struct ScrollbarState {
    pub hovered: bool,
    /// Distance from the thumb's top to the pointer while the thumb is dragged.
    pub drag_grab: Option<f32>,
    /// Shown after a scroll until `hide_task` fires.
    pub revealed: bool,
    pub hide_task: Option<Task<()>>,
}

impl ScrollbarState {
    pub fn is_visible(&self) -> bool {
        self.revealed || self.hovered || self.drag_grab.is_some()
    }

    pub fn is_active(&self) -> bool {
        self.hovered || self.drag_grab.is_some()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn metrics(history: usize, display_offset: usize) -> ScrollMetrics {
        ScrollMetrics {
            history,
            display_offset,
            screen_lines: 25,
        }
    }

    #[test]
    fn no_history_has_no_thumb() {
        assert_eq!(metrics(0, 0).thumb(500.0), None);
    }

    #[test]
    fn thumb_height_is_the_screen_share_of_all_lines() {
        let (_, height) = metrics(75, 0).thumb(400.0).unwrap();
        assert_eq!(height, 100.0);
    }

    #[test]
    fn thumb_has_a_minimum_height() {
        let (_, height) = metrics(100_000, 0).thumb(400.0).unwrap();
        assert_eq!(height, MIN_THUMB_HEIGHT);
    }

    #[test]
    fn live_screen_puts_the_thumb_at_the_bottom() {
        assert_eq!(metrics(75, 0).thumb(400.0), Some((300.0, 100.0)));
    }

    #[test]
    fn oldest_history_puts_the_thumb_at_the_top() {
        assert_eq!(metrics(75, 75).thumb(400.0), Some((0.0, 100.0)));
    }

    #[test]
    fn thumb_top_maps_back_to_the_offset() {
        let metrics = metrics(75, 0);
        for offset in [0, 1, 30, 74, 75] {
            let (top, _) = ScrollMetrics {
                display_offset: offset,
                ..metrics
            }
            .thumb(400.0)
            .unwrap();
            assert_eq!(metrics.offset_for_thumb_top(400.0, top), offset);
        }
    }

    #[test]
    fn thumb_top_outside_the_track_is_clamped() {
        let metrics = metrics(75, 10);
        assert_eq!(metrics.offset_for_thumb_top(400.0, -50.0), 75);
        assert_eq!(metrics.offset_for_thumb_top(400.0, 1000.0), 0);
    }
}
