//! Restarting an exited tab: the exit bar's Restart button, Enter on the dead terminal and the
//! tab menu ask; the workspace starts the tab's launch again in its last folder, in place.

use gpui_kit::component::Sizable;
use gpui_kit::component::button::{Button, ButtonVariants};
use gpui_kit::{AnyElement, Context, IntoElement, KeyDownEvent};
use term_core::SessionKind;

use super::{ProcessState, TerminalView};

/// Asked from an exited tab: the workspace starts its program again in the tab.
pub struct RestartTab;

impl TerminalView {
    /// The program exited and no restart is under way.
    pub fn can_restart(&self) -> bool {
        matches!(self.process, ProcessState::Exited(_)) && !self.restarting
    }

    /// The program exited: the grid no longer takes input, so it shows no cursor.
    pub fn has_exited(&self) -> bool {
        matches!(self.process, ProcessState::Exited(_))
    }

    /// Marks a restart that runs off this thread (a remote tab's) as under way; `false`
    /// (nothing done) unless [`Self::can_restart`].
    pub fn begin_restart(&mut self, cx: &mut Context<Self>) -> bool {
        if !self.can_restart() {
            return false;
        }
        self.restarting = true;
        cx.notify();
        true
    }

    /// The restart failed: the tab can be restarted again.
    pub fn end_restart(&mut self, cx: &mut Context<Self>) {
        if self.restarting {
            self.restarting = false;
            cx.notify();
        }
    }

    /// A fresh press of Enter on an exited tab restarts it (a held key's repeats do not);
    /// `false` for any other key or state.
    pub(super) fn restart_on_enter(&self, event: &KeyDownEvent, cx: &mut Context<Self>) -> bool {
        let keystroke = &event.keystroke;
        let pressed_enter =
            keystroke.key == "enter" && !keystroke.modifiers.modified() && !event.is_held;
        if !(pressed_enter && self.can_restart()) {
            return false;
        }
        cx.emit(RestartTab);
        true
    }

    /// The exit bar's Restart button; `None` while a restart is under way.
    pub(super) fn restart_button(&self, cx: &mut Context<Self>) -> Option<AnyElement> {
        if self.restarting {
            return None;
        }
        let new_conversation = if matches!(self.kind(), SessionKind::Agent(_)) {
            ", in a new conversation"
        } else {
            ""
        };
        let tooltip = format!(
            "Start {} again in the same folder{new_conversation} (Enter). Clears the scrollback.",
            self.name
        );
        let button = Button::new("restart-tab")
            .label("Restart")
            .small()
            .primary()
            .tooltip(tooltip)
            .on_click(cx.listener(|view, _, _, cx| {
                if view.can_restart() {
                    cx.emit(RestartTab);
                }
            }));
        Some(button.into_any_element())
    }
}
