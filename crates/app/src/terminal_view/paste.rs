//! Pasting into the terminal. Outside bracketed paste mode every line break reaches the
//! program as Enter, so pasted lines run at once: a paste with a line break asks first
//! (`workspace_model::paste_warning`). The question is a safeguard: no setting turns it off.

use std::rc::Rc;

use gpui_kit::component::{ActiveTheme, WindowExt};
use gpui_kit::{App, Context, Div, ParentElement, Styled, Window, div, px};
use term_core::alacritty_terminal::term::TermMode;
use workspace_model::{PasteWarning, paste_warning};

use super::{ProcessState, TerminalView};
use crate::keys;
use crate::terminal_element::font_family;

/// Wide enough for a preview line of `PASTE_PREVIEW_CHARS` characters.
const QUESTION_WIDTH: f32 = 760.;

impl TerminalView {
    /// Send `text` as a paste, first asking when its lines would run as commands; false when
    /// there is no text. An open question counts as handled.
    pub(super) fn paste(
        &mut self,
        text: Option<String>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> bool {
        let Some(text) = text.filter(|text| !text.is_empty()) else {
            return false;
        };
        match self.paste_question(&text) {
            Some(warning) => self.ask_before_pasting(text, warning, window, cx),
            None => self.send_paste(&text, cx),
        }
        true
    }

    /// The question to ask before pasting `text`; `None` when its lines would not run: the
    /// program turned on bracketed paste, or nothing runs in the tab.
    fn paste_question(&self, text: &str) -> Option<PasteWarning> {
        let lines_run = self.process == ProcessState::Running
            && !self.mode().contains(TermMode::BRACKETED_PASTE);
        lines_run.then(|| paste_warning(text)).flatten()
    }

    /// Send `text` in the mode current now (it may have changed while the question was open).
    fn send_paste(&mut self, text: &str, cx: &mut Context<Self>) {
        let bytes = keys::paste_bytes(text, self.mode());
        self.send_input(bytes.into(), cx);
    }

    /// Ask `warning`'s question; Paste sends `text` (as read when asked), Cancel or Esc sends
    /// nothing. Does nothing while the question is already open.
    fn ask_before_pasting(
        &mut self,
        text: String,
        warning: PasteWarning,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        // `close_dialog` skips `on_close`: without an open dialog the flag is stale.
        if self.paste_question_open && window.has_active_dialog(cx) {
            return;
        }
        self.paste_question_open = true;
        let view = cx.entity().downgrade();
        let text: Rc<str> = text.into();
        window.open_alert_dialog(cx, move |alert, _, cx| {
            let (pasting_view, closed_view, text) = (view.clone(), view.clone(), text.clone());
            alert
                .title(warning.title.clone())
                .description(description(&warning, cx))
                .width(px(QUESTION_WIDTH))
                .ok_text("Paste")
                .show_cancel(true)
                .on_ok(move |_, window, cx| {
                    let _ = pasting_view.update(cx, |view, cx| {
                        view.paste_answered(&text, window, cx);
                    });
                    true
                })
                // Runs after every way out (Paste, Cancel, Esc; alert dialogs ignore clicks
                // outside them).
                .on_close(move |_, _, cx| {
                    let _ = closed_view.update(cx, |view, _| view.paste_question_open = false);
                })
        });
    }

    /// The question's Paste: send `text` and give the keyboard back to the terminal at once.
    fn paste_answered(&mut self, text: &str, window: &mut Window, cx: &mut Context<Self>) {
        self.send_paste(text, cx);
        // Paste closes the dialog animated, giving the focus back only after the animation:
        // typing on at once must reach the terminal.
        let focus = self.focus_handle.clone();
        window.defer(cx, move |window, cx| focus.focus(window, cx));
    }
}

/// "They will run as commands." over the preview of the first lines.
fn description(warning: &PasteWarning, cx: &App) -> Div {
    div()
        .flex()
        .flex_col()
        .gap_2()
        .child(warning.description)
        .child(preview(warning, cx))
        .children(warning.more.clone())
}

/// The first pasted lines in the terminal font, one row each (an empty line keeps its row).
fn preview(warning: &PasteWarning, cx: &App) -> Div {
    let rows = warning.preview.iter().map(|line| {
        let shown = if line.is_empty() { " " } else { line };
        div()
            .whitespace_nowrap()
            .overflow_hidden()
            .child(shown.to_owned())
    });
    div()
        .flex()
        .flex_col()
        .p_2()
        .rounded_md()
        .bg(cx.theme().muted)
        .text_color(cx.theme().foreground)
        .font_family(font_family(cx))
        .text_sm()
        .children(rows)
}
