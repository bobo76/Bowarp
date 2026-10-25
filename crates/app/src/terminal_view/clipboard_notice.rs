//! The clipboard notice: a program's clipboard write (OSC 52) waits above the terminal for
//! Allow, Deny or "Always for this tab" (`workspace_model::ClipboardGuard`).

use gpui_kit::component::button::{Button, ButtonVariants};
use gpui_kit::component::{ActiveTheme, Icon, IconName, Sizable};
use gpui_kit::{
    AnyElement, App, ClipboardItem, Context, IntoElement, ParentElement, Styled, Window, div,
};
use term_core::alacritty_terminal::term::ClipboardType;
use workspace_model::{ClipboardAnswer, ClipboardGuard, ClipboardOffer, ClipboardTarget};

use super::{AllowClipboard, DenyClipboard, ProcessState, TabChanged, TerminalView};
use crate::shortcuts;
use crate::terminal_element::font_family;

impl TerminalView {
    /// A program wrote the clipboard or the primary selection: apply it when the tab allows
    /// all writes, else hold it and show the notice (the sidebar shows Needs attention).
    pub(super) fn on_clipboard_store(
        &mut self,
        clipboard: ClipboardType,
        text: String,
        cx: &mut Context<Self>,
    ) {
        let Some(target) = clipboard_target(clipboard) else {
            return;
        };
        if self.process != ProcessState::Running {
            return;
        }
        match self.clipboard.offer(target, text) {
            ClipboardOffer::Apply(target, text) => store_clipboard(target, text, cx),
            ClipboardOffer::Held { first } => {
                if first {
                    cx.emit(TabChanged);
                }
                cx.notify();
            }
        }
    }

    /// Apply or drop the held writes. Does nothing when none are held.
    pub(crate) fn answer_clipboard(&mut self, answer: ClipboardAnswer, cx: &mut Context<Self>) {
        let Some(writes) = self.clipboard.answer(answer) else {
            return;
        };
        for (target, text) in writes.into_writes() {
            store_clipboard(target, text, cx);
        }
        cx.emit(TabChanged);
        cx.notify();
    }

    #[cfg(test)]
    pub(crate) fn clipboard_always(&self) -> bool {
        self.clipboard.always()
    }

    /// The guard for this tab's next session: it keeps "Always for this tab".
    pub fn clipboard_carry_over(&self) -> ClipboardGuard {
        self.clipboard.carry_over()
    }

    pub fn set_clipboard_guard(&mut self, guard: ClipboardGuard) {
        self.clipboard = guard;
    }

    #[cfg(test)]
    pub(crate) fn clipboard_pending(&self) -> bool {
        self.clipboard.pending().is_some()
    }

    /// Ctrl+Shift+Y; without a held write the key goes on to the program.
    pub(super) fn on_allow_clipboard(
        &mut self,
        _: &AllowClipboard,
        _: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.answer_held_or_propagate(ClipboardAnswer::Allow, cx);
    }

    /// Ctrl+Shift+N; without a held write the key goes on to the program.
    pub(super) fn on_deny_clipboard(
        &mut self,
        _: &DenyClipboard,
        _: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.answer_held_or_propagate(ClipboardAnswer::Deny, cx);
    }

    fn answer_held_or_propagate(&mut self, answer: ClipboardAnswer, cx: &mut Context<Self>) {
        if self.clipboard.pending().is_none() {
            cx.propagate();
            return;
        }
        self.answer_clipboard(answer, cx);
    }

    /// The notice above the terminal while a write is held.
    pub(super) fn clipboard_banner(&self, cx: &mut Context<Self>) -> Option<AnyElement> {
        let notice = self.clipboard.notice()?;
        let theme = cx.theme();
        let button = |id: &'static str, label: &'static str, answer: ClipboardAnswer| {
            Button::new(id).label(label).small().on_click(cx.listener(
                move |view, _, window, cx| {
                    view.answer_clipboard(answer, cx);
                    window.focus(&view.focus_handle, cx);
                },
            ))
        };
        let preview = notice.preview.clone().map(|preview| {
            div()
                .flex_1()
                .min_w_0()
                .overflow_hidden()
                .whitespace_nowrap()
                .text_ellipsis()
                .font_family(font_family(cx))
                .text_color(theme.muted_foreground)
                .child(preview)
        });
        let banner = div()
            .flex_none()
            .flex()
            .items_center()
            .gap_2()
            .px_3()
            .py_1()
            .text_sm()
            .bg(theme.warning.opacity(0.15))
            .border_b_1()
            .border_color(theme.warning)
            .text_color(theme.foreground)
            .child(Icon::new(IconName::TriangleAlert).text_color(theme.warning))
            .child(div().flex_none().child(notice.message.clone()))
            .children(preview)
            .child(div().flex_1())
            .child(
                button("clipboard-allow", "Allow", ClipboardAnswer::Allow)
                    .primary()
                    .tooltip(shortcuts::with_shortcut("Allow", &AllowClipboard, cx)),
            )
            .child(
                button("clipboard-deny", "Deny", ClipboardAnswer::Deny)
                    .outline()
                    .tooltip(shortcuts::with_shortcut("Deny", &DenyClipboard, cx)),
            )
            .child(
                button(
                    "clipboard-always",
                    "Always for this tab",
                    ClipboardAnswer::Always,
                )
                .ghost()
                .tooltip("Allow this and later clipboard writes until the tab closes"),
            );
        Some(banner.into_any_element())
    }
}

/// The primary selection only exists on Linux; elsewhere writes to it are dropped unasked.
fn clipboard_target(clipboard: ClipboardType) -> Option<ClipboardTarget> {
    match clipboard {
        ClipboardType::Clipboard => Some(ClipboardTarget::Clipboard),
        ClipboardType::Selection if cfg!(target_os = "linux") => Some(ClipboardTarget::Selection),
        ClipboardType::Selection => None,
    }
}

/// Put text a program copied (OSC 52) on the clipboard, once allowed. Copilot CLI on Windows
/// sends it and also runs `clip.exe`, which exits with code 1 in this app's tabs, so its
/// Ctrl+C and "Copy to clipboard" rely on it.
fn store_clipboard(target: ClipboardTarget, text: String, cx: &mut App) {
    match target {
        ClipboardTarget::Clipboard => cx.write_to_clipboard(ClipboardItem::new_string(text)),
        #[cfg(target_os = "linux")]
        ClipboardTarget::Selection => cx.write_to_primary(ClipboardItem::new_string(text)),
        #[cfg(not(target_os = "linux"))]
        ClipboardTarget::Selection => {}
    }
}
