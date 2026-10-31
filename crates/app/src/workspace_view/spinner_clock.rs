//! The one timer behind every working spinner (Copilot, renamed Claude Code). Per-element
//! animations each run their own timer, and timers firing either side of a frame cost a window
//! repaint each; one shared tick repaints once for any number of spinners.

use std::cell::Cell;

use gpui_kit::{Context, Task};

use super::WorkspaceView;
use crate::sidebar::{WORKING_SPIN_STEPS, WORKING_SPIN_TICK};

#[derive(Default)]
pub(super) struct SpinnerClock {
    /// Quarter turns, `0..WORKING_SPIN_STEPS`.
    step: u8,
    /// Whether the frame being rendered draws a turning spinner.
    drawn: Cell<bool>,
    /// The next tick, scheduled by a frame that drew a turning spinner. A tick only follows a
    /// frame, so nothing runs while no frame is drawn (window hidden, spinner scrolled off).
    tick: Option<Task<()>>,
}

impl SpinnerClock {
    /// Called before rendering the sidebar: no turning spinner drawn yet.
    pub(super) fn begin_frame(&self) {
        self.drawn.set(false);
    }

    /// The step a turning spinner draws at; marks the frame as needing the next tick.
    pub(super) fn drawn_step(&self) -> u8 {
        self.drawn.set(true);
        self.step
    }

    /// Called after rendering the sidebar: schedules the next tick while a turning spinner is
    /// drawn, cancels it otherwise.
    pub(super) fn end_frame(&mut self, cx: &mut Context<WorkspaceView>) {
        if !self.drawn.get() {
            self.tick = None;
            return;
        }
        if self.tick.is_some() {
            return;
        }
        self.tick = Some(cx.spawn(async move |this, cx| {
            cx.background_executor().timer(WORKING_SPIN_TICK).await;
            let _ = this.update(cx, |workspace, cx| {
                let clock = &mut workspace.spinner_clock;
                clock.tick = None;
                clock.step = next_step(clock.step);
                cx.notify();
            });
        }));
    }
}

fn next_step(step: u8) -> u8 {
    (step + 1) % WORKING_SPIN_STEPS
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn steps_wrap_after_a_full_turn() {
        let steps: Vec<u8> = std::iter::successors(Some(0), |&step| Some(next_step(step)))
            .take(6)
            .collect();
        assert_eq!(steps, [0, 1, 2, 3, 0, 1]);
    }
}
