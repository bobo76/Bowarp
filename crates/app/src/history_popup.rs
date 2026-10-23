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

use crate::history::{self, History};
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
const FOLDER_MAX_WIDTH: f32 = 220.;
const PROMPT_GAP: f32 = 2.;

/// The open popup.
#[derive(Default)]
pub(crate) struct HistoryPopup {
    selection: HistorySelection,
    /// The typed text the matches were last listed for; the selection resets when it changes.
    query: Option<String>,
}

impl HistoryPopup {
    fn visible_selection(&self, query: &str, total: usize, rows: usize) -> HistorySelection {
        let mut selection = if self.query.as_deref() == Some(query) {
            self.selection
        } else {
            HistorySelection::default()
        };
        selection.keep_in_view(total, rows);
        selection
    }
}

struct HistoryRow {
    index: usize,
    command: String,
    folder: Option<String>,
    selected: bool,
}

struct PopupPlacement {
    rows: usize,
    width: Pixels,
    position: gpui_kit::Point<Pixels>,
    anchor: Anchor,
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
    /// Remove the selected command from the history.
    Forget,
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
        "delete" if plain => PopupKey::Forget,
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
        if is_plain(keystroke)
            && matches!(keystroke.key.as_str(), "right" | "end")
            && self.restored_suggestion().is_some()
            && self.accept_suggestion(cx)
        {
            return true;
        }
        self.last_command.dismiss();
        if self.history_popup.is_some() {
            return self.handle_popup_key(keystroke, cx);
        }
        if !is_plain(keystroke) {
            return false;
        }
        match keystroke.key.as_str() {
            "up" if self.prompt_input().is_some() => {
                self.history_popup = Some(HistoryPopup::default());
                self.refresh_history(cx);
                cx.notify();
                true
            }
            "right" | "end" | "tab" => self.accept_suggestion(cx),
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
        let rows = self
            .popup_placement(&input)
            .map_or(1, |placement| placement.rows);
        self.refresh_history(cx);
        let Some(popup) = self.history_popup.as_mut() else {
            return false;
        };
        popup.selection.keep_in_view(total, rows);
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
            PopupKey::Forget => {
                let command = popup.selection.index(total).and_then(|index| {
                    let history = cx.global::<History>().commands();
                    let matches = history.matching(&input.text);
                    matches.get(index).map(|entry| entry.command.clone())
                });
                if let Some(command) = command {
                    history::forget(&command, cx);
                    self.refresh_history(cx);
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
            self.refresh_history(cx);
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
        if let Some(command) = self.restored_suggestion() {
            return Some(command.to_owned());
        }
        let history = cx.global::<History>().commands();
        history.suggestion(&input.before_cursor).map(str::to_string)
    }

    pub(crate) fn history_suggestion(&self, cx: &App) -> Option<String> {
        if self.history_popup.is_some() {
            return None;
        }
        self.suggestion(cx)
    }

    pub(crate) fn restored_suggestion(&self) -> Option<&str> {
        let input = self.prompt_input()?;
        if !input.before_cursor.is_empty() {
            return None;
        }
        self.last_command.suggestion(&input.text, input.at_end)
    }

    /// Reconcile history state after a prompt, history or popup event, never during rendering.
    pub(crate) fn refresh_history(&mut self, cx: &mut Context<Self>) {
        let input = self.prompt_input();
        if input
            .as_ref()
            .is_some_and(|input| !input.text.is_empty() || !input.before_cursor.is_empty())
        {
            self.last_command.dismiss();
        }
        let rows = input
            .as_ref()
            .and_then(|input| self.popup_placement(input))
            .map_or(MAX_ROWS, |placement| placement.rows);
        if input.is_none() {
            self.history_popup = None;
        }
        if let (Some(popup), Some(input)) = (&mut self.history_popup, &input) {
            if popup.query.as_deref() != Some(input.text.as_str()) {
                popup.query = Some(input.text.clone());
                popup.selection = HistorySelection::default();
            }
            let total = cx
                .global::<History>()
                .commands()
                .matching(&input.text)
                .len();
            popup.selection.keep_in_view(total, rows);
        }
    }

    fn popup_placement(&self, input: &PromptInput) -> Option<PopupPlacement> {
        let geometry = self.geometry?;
        let display_offset = self.session.display_offset() as i32;
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
        let width = (geometry.cell_width * geometry.columns as f32).min(px(MAX_WIDTH));
        let (position, anchor) = if above {
            (
                point(geometry.origin.x, prompt_top - px(PROMPT_GAP)),
                Anchor::BottomLeft,
            )
        } else {
            (
                point(geometry.origin.x, below_top + px(PROMPT_GAP)),
                Anchor::TopLeft,
            )
        };
        Some(PopupPlacement {
            rows,
            width,
            position,
            anchor,
        })
    }

    pub(crate) fn render_history_popup(&self, cx: &mut Context<Self>) -> Option<AnyElement> {
        let input = self.prompt_input()?;
        let popup = self.history_popup.as_ref()?;
        let placement = self.popup_placement(&input)?;
        let matches = cx.global::<History>().commands().matching(&input.text);
        let total = matches.len();
        // Resizing changes the visible range, not the stored navigation state.
        let selection = popup.visible_selection(&input.text, total, placement.rows);
        let selected = selection.index(total);
        let visible = selection
            .visible(total, placement.rows)
            .map(|index| (index, matches[index].clone()))
            .collect();
        let panel = self.history_panel(visible, selected, placement.width, cx);
        Some(
            deferred(
                anchored()
                    .position(placement.position)
                    .anchor(placement.anchor)
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
                    .map(|folder| display_path(folder, home.as_deref()));
                self.history_row(
                    HistoryRow {
                        index,
                        command: entry.command,
                        folder,
                        selected: selected == Some(index),
                    },
                    family.clone(),
                    cx,
                )
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
                    .child(key_chip("del", cx))
                    .child(div().mr_2().child("to remove"))
                    .child(key_chip("esc", cx))
                    .child("to dismiss"),
            )
    }

    fn history_row(
        &self,
        row: HistoryRow,
        family: SharedString,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let theme = cx.theme();
        let HistoryRow {
            index,
            command,
            folder,
            selected,
        } = row;
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
                        .max_w(px(FOLDER_MAX_WIDTH))
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
mod tests;
