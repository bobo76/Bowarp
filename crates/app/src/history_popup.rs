//! Command history at a shell prompt (shell integration only): the inline suggestion drawn
//! dimmed after the cursor, accepted with Right or End, and the "HISTORY" popup above the
//! prompt, opened with Up, listing past commands that contain the typed text.

use gpui_kit::component::{ActiveTheme, Icon, IconName, Sizable};
use gpui_kit::prelude::FluentBuilder;
use gpui_kit::{Anchor, SharedString};
use gpui_kit::{
    AnyElement, App, Context, InteractiveElement, IntoElement, Keystroke, MouseButton,
    MouseDownEvent, ParentElement, Pixels, Styled, anchored, deferred, div, point, px,
};
use term_core::{PromptInput, SessionKind};
use workspace_model::{HistoryEntry, HistorySelection, display_path};

use crate::history::History;
use crate::terminal_element;
use crate::terminal_view::TerminalView;

/// Rows shown at most; more scroll.
const MAX_ROWS: usize = 10;
/// Fewer rows than this above the prompt puts the popup below it.
const MIN_ROWS_ABOVE: usize = 3;
const ROW_HEIGHT: f32 = 26.;
const HEADER_HEIGHT: f32 = 28.;
const FOOTER_HEIGHT: f32 = 32.;
const MAX_WIDTH: f32 = 720.;

/// The open popup.
#[derive(Default)]
pub(crate) struct HistoryPopup {
    selection: HistorySelection,
    /// The typed text the matches were last listed for; the selection resets when it changes.
    query: Option<String>,
    /// Rows shown at the last render, for Page Up / Page Down.
    rows: usize,
}

fn is_plain(keystroke: &Keystroke) -> bool {
    let modifiers = &keystroke.modifiers;
    !(modifiers.control
        || modifiers.alt
        || modifiers.shift
        || modifiers.platform
        || modifiers.function)
}

/// What a key does while the popup is open.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum PopupKey {
    Older(Rows),
    Newer(Rows),
    /// Put the selected command on the prompt.
    Pick,
    Dismiss,
    /// Typing and Backspace go to the shell; the list refilters on the new command line.
    Edit,
    /// Anything else closes the popup and goes to the shell.
    Other,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Rows {
    One,
    Page,
}

fn popup_key(keystroke: &Keystroke) -> PopupKey {
    let plain = is_plain(keystroke);
    let modifiers = &keystroke.modifiers;
    let typing = keystroke.key_char.is_some()
        && !(modifiers.control || modifiers.alt || modifiers.platform || modifiers.function);
    match keystroke.key.as_str() {
        "up" if plain => PopupKey::Older(Rows::One),
        "down" if plain => PopupKey::Newer(Rows::One),
        "pageup" if plain => PopupKey::Older(Rows::Page),
        "pagedown" if plain => PopupKey::Newer(Rows::Page),
        "enter" | "tab" if plain => PopupKey::Pick,
        "escape" if plain => PopupKey::Dismiss,
        "backspace" => PopupKey::Edit,
        _ if typing => PopupKey::Edit,
        _ => PopupKey::Other,
    }
}

impl TerminalView {
    /// What the user typed at a shell's prompt, when the shell has integration.
    pub(crate) fn prompt_input(&self) -> Option<PromptInput> {
        if self.kind() != SessionKind::Shell {
            return None;
        }
        self.session.prompt_input()
    }

    /// History keys at the prompt. Returns whether the key was used (and must not reach the
    /// shell).
    pub(crate) fn handle_history_key(
        &mut self,
        keystroke: &Keystroke,
        cx: &mut Context<Self>,
    ) -> bool {
        if self.history_popup.is_some() {
            return self.handle_popup_key(keystroke, cx);
        }
        if !is_plain(keystroke) {
            return false;
        }
        match keystroke.key.as_str() {
            "up" if self.prompt_input().is_some() => {
                self.history_popup = Some(HistoryPopup::default());
                cx.notify();
                true
            }
            "right" | "end" => self.accept_suggestion(cx),
            _ => false,
        }
    }

    fn handle_popup_key(&mut self, keystroke: &Keystroke, cx: &mut Context<Self>) -> bool {
        let Some(input) = self.prompt_input() else {
            self.close_history_popup(cx);
            return false;
        };
        let total = cx
            .global::<History>()
            .commands()
            .matching(&input.text)
            .len();
        let Some(popup) = self.history_popup.as_mut() else {
            return false;
        };
        let rows = popup.rows.max(1);
        let by = |step: Rows| match step {
            Rows::One => 1,
            Rows::Page => rows,
        };
        match popup_key(keystroke) {
            PopupKey::Older(step) => popup.selection.older(by(step), total, rows),
            PopupKey::Newer(step) => popup.selection.newer(by(step), total, rows),
            PopupKey::Pick => {
                let index = popup.selection.index(total);
                self.close_history_popup(cx);
                if let Some(index) = index {
                    self.put_history_command(&input, index, cx);
                }
            }
            PopupKey::Dismiss => self.close_history_popup(cx),
            PopupKey::Edit => return false,
            PopupKey::Other => {
                self.close_history_popup(cx);
                return false;
            }
        }
        cx.notify();
        true
    }

    pub(crate) fn close_history_popup(&mut self, cx: &mut Context<Self>) {
        if self.history_popup.take().is_some() {
            cx.notify();
        }
    }

    /// Replace the command line with the `index`th (oldest first) command matching `input`.
    fn put_history_command(&mut self, input: &PromptInput, index: usize, cx: &mut Context<Self>) {
        let history = cx.global::<History>().commands();
        let Some(entry) = history.matching(&input.text).get(index).copied() else {
            return;
        };
        // Entries hold no control characters (`workspace_model::is_recordable`).
        self.type_text(input.replace_keys(&entry.command, self.shell), cx);
    }

    fn accept_suggestion(&mut self, cx: &mut Context<Self>) -> bool {
        // The screen may not show the last keys yet: the suggestion would be for older text.
        if self.session.awaiting_echo() {
            return false;
        }
        let Some(rest) = self.suggestion(cx) else {
            return false;
        };
        self.type_text(rest, cx);
        true
    }

    /// The rest of the newest command starting with what is typed, while the cursor is at the
    /// end of the command line.
    fn suggestion(&self, cx: &App) -> Option<String> {
        let input = self.prompt_input().filter(|input| input.at_end)?;
        let history = cx.global::<History>().commands();
        history.suggestion(&input.before_cursor).map(str::to_string)
    }

    /// Refresh the suggestion and the popup for the prompt as it is now; the popup closes when
    /// the shell left its prompt. Returns the popup element when it is open.
    pub(crate) fn update_history(&mut self, cx: &mut Context<Self>) -> Option<AnyElement> {
        let input = self.prompt_input();
        if input.is_none() {
            self.history_popup = None;
        }
        self.ghost_text = if self.history_popup.is_none() {
            self.suggestion(cx)
        } else {
            None
        };
        self.render_history_popup(input.as_ref()?, cx)
    }

    fn render_history_popup(
        &mut self,
        input: &PromptInput,
        cx: &mut Context<Self>,
    ) -> Option<AnyElement> {
        let geometry = self.geometry?;
        let display_offset = self.session.display_offset() as i32;
        let popup = self.history_popup.as_mut()?;

        let line_height = geometry.line_height;
        let prompt_line = (input.prompt_line + display_offset).max(0) as f32;
        let prompt_top = geometry.origin.y + line_height * prompt_line;
        let fit = |space: Pixels| {
            ((f32::from(space) - HEADER_HEIGHT - FOOTER_HEIGHT) / ROW_HEIGHT).max(0.) as usize
        };
        let rows_above = fit(prompt_top - geometry.origin.y);
        let below_top = geometry.cursor_origin.y + line_height;
        let rows_below = fit(geometry.origin.y + line_height * geometry.lines as f32 - below_top);
        let above = rows_above >= MIN_ROWS_ABOVE || rows_above >= rows_below;
        let rows = if above { rows_above } else { rows_below }.clamp(1, MAX_ROWS);
        popup.rows = rows;

        let history = cx.global::<History>().commands();
        let matches = history.matching(&input.text);
        let total = matches.len();
        if popup.query.as_deref() != Some(input.text.as_str()) {
            popup.query = Some(input.text.clone());
            popup.selection = HistorySelection::default();
        }
        popup.selection.keep_in_view(total, rows);
        let selected = popup.selection.index(total);
        let visible: Vec<(usize, HistoryEntry)> = popup
            .selection
            .visible(total, rows)
            .map(|index| (index, matches[index].clone()))
            .collect();

        let width = (geometry.cell_width * geometry.columns as f32).min(px(MAX_WIDTH));
        let (position, anchor) = if above {
            (
                point(geometry.origin.x, prompt_top - px(2.)),
                Anchor::BottomLeft,
            )
        } else {
            (
                point(geometry.origin.x, below_top + px(2.)),
                Anchor::TopLeft,
            )
        };
        let panel = self.history_panel(visible, selected, width, cx);
        Some(
            deferred(
                anchored()
                    .position(position)
                    .anchor(anchor)
                    .snap_to_window()
                    .child(panel),
            )
            .with_priority(1)
            .into_any_element(),
        )
    }

    fn history_panel(
        &self,
        visible: Vec<(usize, HistoryEntry)>,
        selected: Option<usize>,
        width: Pixels,
        cx: &mut Context<Self>,
    ) -> impl IntoElement {
        let theme = cx.theme();
        let (muted, border) = (theme.muted_foreground, theme.border);
        let (background, foreground) = (theme.popover, theme.popover_foreground);
        let home = std::env::home_dir();
        let family = terminal_element::font_family(cx);
        let empty = visible.is_empty();
        let rows: Vec<AnyElement> = visible
            .into_iter()
            .map(|(index, entry)| {
                let folder = entry
                    .dir
                    .as_deref()
                    .map(|dir| display_path(dir, home.as_deref()));
                let selected = selected == Some(index);
                self.history_row(index, entry.command, folder, selected, family.clone(), cx)
            })
            .collect();
        div()
            .id("history-popup")
            .occlude()
            .w(width)
            .flex()
            .flex_col()
            .bg(background)
            .text_color(foreground)
            .border_1()
            .border_color(border)
            .rounded_md()
            .shadow_lg()
            .overflow_hidden()
            .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
            .child(
                div()
                    .h(px(HEADER_HEIGHT))
                    .px_3()
                    .flex()
                    .items_center()
                    .border_b_1()
                    .border_color(border)
                    .text_xs()
                    .text_color(muted)
                    .child("HISTORY"),
            )
            .when(empty, |panel| {
                panel.child(
                    div()
                        .h(px(ROW_HEIGHT))
                        .px_3()
                        .flex()
                        .items_center()
                        .text_sm()
                        .text_color(muted)
                        .child("No matching commands"),
                )
            })
            .children(rows)
            .child(
                div()
                    .h(px(FOOTER_HEIGHT))
                    .px_3()
                    .flex()
                    .items_center()
                    .gap_1()
                    .border_t_1()
                    .border_color(border)
                    .text_xs()
                    .text_color(muted)
                    .child(key_chip("↑", cx))
                    .child(key_chip("↓", cx))
                    .child(div().mr_2().child("to navigate"))
                    .child(key_chip("enter", cx))
                    .child(key_chip("tab", cx))
                    .child(div().mr_2().child("to select"))
                    .child(key_chip("esc", cx))
                    .child("to dismiss"),
            )
    }

    fn history_row(
        &self,
        index: usize,
        command: String,
        folder: Option<String>,
        selected: bool,
        family: SharedString,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let theme = cx.theme();
        let (muted, border, active) = (theme.muted_foreground, theme.border, theme.list_active);
        div()
            .id(("history-row", index))
            .h(px(ROW_HEIGHT))
            .px_2()
            .flex()
            .items_center()
            .gap_2()
            .when(selected, |row| row.bg(active))
            .child(
                Icon::new(IconName::SquareTerminal)
                    .small()
                    .text_color(muted),
            )
            .child(
                div()
                    .flex_1()
                    .min_w_0()
                    .overflow_hidden()
                    .whitespace_nowrap()
                    .text_ellipsis()
                    .font_family(family)
                    .text_sm()
                    .child(command),
            )
            .when_some(folder, |row, folder| {
                row.child(
                    div()
                        .flex_none()
                        .max_w(px(220.))
                        .px_1()
                        .flex()
                        .items_center()
                        .gap_1()
                        .border_1()
                        .border_color(border)
                        .rounded_sm()
                        .text_xs()
                        .text_color(muted)
                        .child(Icon::new(IconName::Folder).xsmall())
                        .child(
                            div()
                                .overflow_hidden()
                                .whitespace_nowrap()
                                .text_ellipsis()
                                .child(folder),
                        ),
                )
            })
            .on_mouse_down(
                MouseButton::Left,
                cx.listener(move |view, _: &MouseDownEvent, _, cx| {
                    cx.stop_propagation();
                    view.pick_history_row(index, cx);
                }),
            )
            .into_any_element()
    }

    /// A click on a row puts its command on the prompt.
    fn pick_history_row(&mut self, index: usize, cx: &mut Context<Self>) {
        self.close_history_popup(cx);
        if let Some(input) = self.prompt_input() {
            self.put_history_command(&input, index, cx);
        }
    }
}

fn key_chip(label: &'static str, cx: &App) -> impl IntoElement {
    div()
        .px_1()
        .border_1()
        .border_color(cx.theme().border)
        .rounded_sm()
        .bg(cx.theme().secondary)
        .child(SharedString::from(label))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn key(source: &str) -> PopupKey {
        popup_key(&Keystroke::parse(source).unwrap())
    }

    #[test]
    fn popup_keys_navigate_pick_or_go_to_the_shell() {
        assert_eq!(key("up"), PopupKey::Older(Rows::One));
        assert_eq!(key("pagedown"), PopupKey::Newer(Rows::Page));
        assert_eq!(key("enter"), PopupKey::Pick);
        assert_eq!(key("tab"), PopupKey::Pick);
        // Shift+Tab is not Tab: it goes to the shell.
        assert_eq!(key("shift-tab"), PopupKey::Other);
        assert_eq!(key("escape"), PopupKey::Dismiss);
        // Typing refilters: printable keys, with Shift too, and Backspace.
        assert_eq!(key("a->a"), PopupKey::Edit);
        assert_eq!(key("shift-a->A"), PopupKey::Edit);
        assert_eq!(key("space-> "), PopupKey::Edit);
        assert_eq!(key("backspace"), PopupKey::Edit);
        assert_eq!(key("ctrl-backspace"), PopupKey::Edit);
        // Shortcuts and cursor keys close it.
        assert_eq!(key("ctrl-c"), PopupKey::Other);
        assert_eq!(key("alt-b->b"), PopupKey::Other);
        assert_eq!(key("left"), PopupKey::Other);
        assert_eq!(key("shift-up"), PopupKey::Other);
    }
}
