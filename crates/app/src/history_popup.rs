//! Command history at a shell prompt (shell integration only): the inline suggestion drawn
//! dimmed after the cursor, accepted with Right or End, and the "HISTORY" popup above the
//! prompt, opened with Up, listing past commands that contain the typed text. Both come from
//! the tab's own shell; the popup's chips (Alt+letter or a click) show another shell's commands
//! or "All".

use gpui_kit::component::{ActiveTheme, Icon, IconName, Sizable};
use gpui_kit::prelude::FluentBuilder;
use gpui_kit::{Anchor, SharedString};
use gpui_kit::{
    AnyElement, App, Context, FontWeight, InteractiveElement, IntoElement, Keystroke, MouseButton,
    MouseDownEvent, ParentElement, Pixels, Styled, anchored, deferred, div, point, px,
};
use term_core::{PromptInput, SessionKind};
use workspace_model::{
    HeldModifiers, HistoryEntry, HistoryKey, HistoryScope, HistorySelection, ShellChip,
    display_path, history_chips, shell_name, switch_chip,
};

use crate::history::{self, History};
use crate::launcher;
use crate::settings_file;
use crate::sidebar::launcher_icon;
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
/// The narrowest an unselected chip's name gets: room for the ellipsis.
const CHIP_NAME_MIN_WIDTH: f32 = 10.;
/// The narrowest an unselected chip gets, besides its name: padding and the underlined letter,
/// plus the icon and its gap. Text alone would never let a chip shrink.
const CHIP_MIN_WIDTH: f32 = 8. + 12.;
const CHIP_ICON_WIDTH: f32 = 12. + 4.;

/// The open popup.
pub(crate) struct HistoryPopup {
    selection: HistorySelection,
    /// The typed text the matches were last listed for; the selection resets when it changes.
    query: Option<String>,
    /// The selected chip: the tab's own shell when the popup opens.
    scope: HistoryScope,
}

impl HistoryPopup {
    pub(crate) fn new(shell: &str) -> Self {
        Self {
            selection: HistorySelection::default(),
            query: None,
            scope: HistoryScope::Shell(shell.into()),
        }
    }

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

/// The popup's header and empty state, worked out before rendering.
struct HistoryHeader {
    chips: Vec<ShellChip>,
    /// The commands listed.
    scope: HistoryScope,
    /// Why no command is listed, when none is.
    empty_text: Option<String>,
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

fn held_modifiers(keystroke: &Keystroke) -> HeldModifiers {
    let modifiers = &keystroke.modifiers;
    HeldModifiers {
        control: modifiers.control,
        alt: modifiers.alt,
        shift: modifiers.shift,
        platform: modifiers.platform,
        function: modifiers.function,
    }
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
        let features = settings_file::history_features(cx);
        if is_plain(keystroke)
            && matches!(keystroke.key.as_str(), "right" | "end")
            && self.restored_suggestion(cx).is_some()
            && self.accept_suggestion(cx)
        {
            return true;
        }
        self.last_command.dismiss();
        if self.history_popup.is_some() {
            if !features.popup() {
                self.close_history_popup(cx);
                return false;
            }
            return self.handle_popup_key(keystroke, cx);
        }
        if !is_plain(keystroke) {
            return false;
        }
        match keystroke.key.as_str() {
            "up" if features.intercepts(HistoryKey::Up) && self.prompt_input().is_some() => {
                self.history_popup = Some(HistoryPopup::new(&self.shell_key));
                self.refresh_history(cx);
                cx.notify();
                true
            }
            "right" | "end" | "tab" if features.intercepts(HistoryKey::Accept) => {
                self.accept_suggestion(cx)
            }
            _ => false,
        }
    }

    fn handle_popup_key(&mut self, keystroke: &Keystroke, cx: &mut Context<Self>) -> bool {
        let Some(input) = self.prompt_input() else {
            self.close_history_popup(cx);
            return false;
        };
        let chips = self.history_chips(cx);
        let chip = self.history_popup.as_ref().and_then(|popup| {
            switch_chip(
                &chips,
                &popup.scope,
                &keystroke.key,
                held_modifiers(keystroke),
            )
        });
        if let Some(chip) = chip {
            self.select_history_chip(chips[chip].scope.clone(), cx);
            return true;
        }
        let scope = self.history_scope(&chips);
        let total = cx
            .global::<History>()
            .commands()
            .matching(&input.text, &scope)
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
                    self.put_history_command(&input, &scope, index, cx);
                }
            }
            PopupKey::Forget => {
                let command = popup.selection.index(total).and_then(|index| {
                    let history = cx.global::<History>().commands();
                    let matches = history.matching(&input.text, &scope);
                    matches.get(index).map(|entry| entry.command.clone())
                });
                if let Some(command) = command {
                    history::forget(&command, &scope, cx);
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

    /// The open popup's chips; none while only this tab's shell has commands.
    fn history_chips(&self, cx: &App) -> Vec<ShellChip> {
        let Some(popup) = &self.history_popup else {
            return Vec::new();
        };
        let history = cx.global::<History>().commands();
        history_chips(
            history,
            &self.shell_key,
            &popup.scope,
            &launcher::menu_shells(cx),
        )
    }

    /// The commands the popup lists: the selected chip's, the tab's shell's without chips.
    fn history_scope(&self, chips: &[ShellChip]) -> HistoryScope {
        match &self.history_popup {
            Some(popup) if !chips.is_empty() => popup.scope.clone(),
            _ => HistoryScope::Shell(self.shell_key.clone()),
        }
    }

    /// Show `scope`'s commands, from the newest.
    fn select_history_chip(&mut self, scope: HistoryScope, cx: &mut Context<Self>) {
        let Some(popup) = self.history_popup.as_mut() else {
            return;
        };
        popup.scope = scope;
        popup.selection = HistorySelection::default();
        self.refresh_history(cx);
        cx.notify();
    }

    /// Replace the command line with the `index`th (oldest first) command of `scope` matching
    /// `input`.
    fn put_history_command(
        &mut self,
        input: &PromptInput,
        scope: &HistoryScope,
        index: usize,
        cx: &mut Context<Self>,
    ) {
        let history = cx.global::<History>().commands();
        let Some(entry) = history.matching(&input.text, scope).get(index).copied() else {
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
        if !settings_file::history_features(cx).suggestions() {
            return None;
        }
        let input = self.prompt_input().filter(|input| input.at_end)?;
        if let Some(command) = self.restored_suggestion(cx) {
            return Some(command.to_owned());
        }
        let history = cx.global::<History>().commands();
        history
            .suggestion(&input.before_cursor, &self.shell_key)
            .map(str::to_string)
    }

    pub(crate) fn history_suggestion(&self, cx: &App) -> Option<String> {
        if self.history_popup.is_some() {
            return None;
        }
        self.suggestion(cx)
    }

    pub(crate) fn restored_suggestion(&self, cx: &App) -> Option<&str> {
        if !settings_file::workspace_features(cx).offers_last_command() {
            return None;
        }
        let input = self.prompt_input()?;
        if !input.before_cursor.is_empty() {
            return None;
        }
        self.last_command.suggestion(&input.text, input.at_end)
    }

    /// Reconcile history state after a prompt, history or popup event, never during rendering.
    pub(crate) fn refresh_history(&mut self, cx: &mut Context<Self>) {
        let features = settings_file::history_features(cx);
        if !features.popup() {
            self.history_popup = None;
        }
        if !features.suggestions() && !features.popup() {
            return;
        }
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
        let scope = self.history_scope(&self.history_chips(cx));
        if let (Some(popup), Some(input)) = (&mut self.history_popup, &input) {
            if popup.query.as_deref() != Some(input.text.as_str()) {
                popup.query = Some(input.text.clone());
                popup.selection = HistorySelection::default();
            }
            let total = cx
                .global::<History>()
                .commands()
                .matching(&input.text, &scope)
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
        let chips = self.history_chips(cx);
        let scope = self.history_scope(&chips);
        let history = cx.global::<History>().commands();
        let matches = history.matching(&input.text, &scope);
        let total = matches.len();
        // Resizing changes the visible range, not the stored navigation state.
        let selection = popup.visible_selection(&input.text, total, placement.rows);
        let selected = selection.index(total);
        let visible = selection
            .visible(total, placement.rows)
            .map(|index| (index, matches[index].clone()))
            .collect();
        let empty_text = (total == 0).then(|| self.empty_history_text(&chips, &scope, cx));
        let header = HistoryHeader {
            chips,
            scope,
            empty_text,
        };
        let panel = self.history_panel(header, visible, selected, placement.width, cx);
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

    /// Why the list is empty: the selected shell has no commands yet (pointing at "All" when
    /// it has some), or none matches the typed text.
    fn empty_history_text(&self, chips: &[ShellChip], scope: &HistoryScope, cx: &App) -> String {
        let history = cx.global::<History>().commands();
        if history.count_in(scope) > 0 {
            return "No matching commands".into();
        }
        let HistoryScope::Shell(key) = scope else {
            return "No commands yet".into();
        };
        let label = chips.iter().find(|chip| chip.scope == *scope).map_or_else(
            || shell_name(key, &launcher::menu_shells(cx)).0,
            |chip| chip.label.clone(),
        );
        let all_letter = chips.first().and_then(ShellChip::letter);
        match all_letter {
            Some(letter) if !history.is_empty() => format!(
                "No {label} commands yet · Alt+{}: all shells",
                letter.to_uppercase()
            ),
            _ => format!("No {label} commands yet"),
        }
    }

    fn history_panel(
        &self,
        header: HistoryHeader,
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
        let HistoryHeader {
            chips,
            scope,
            empty_text,
        } = header;
        let switches = !chips.is_empty();
        let chips: Vec<AnyElement> = chips
            .into_iter()
            .enumerate()
            .map(|(index, chip)| {
                let selected = chip.scope == scope;
                self.history_chip(index, chip, selected, cx)
            })
            .collect();
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
                    .gap_2()
                    .text_xs()
                    .text_color(muted)
                    .child(div().flex_none().child("HISTORY"))
                    .child(
                        div()
                            .flex_1()
                            .min_w_0()
                            .overflow_hidden()
                            .flex()
                            .items_center()
                            .gap_1()
                            .children(chips),
                    ),
            )
            .when_some(empty_text, |panel, text| {
                panel.child(
                    div()
                        .h(px(ROW_HEIGHT))
                        .px_3()
                        .flex()
                        .items_center()
                        .text_sm()
                        .text_color(muted)
                        .child(text),
                )
            })
            .children(rows)
            .child(history_footer(&scope, switches, cx))
    }

    /// A shell chip: icon, name with its Alt letter underlined. The selected one is filled and
    /// keeps its full name; the others shrink to their letter and an ellipsis.
    fn history_chip(
        &self,
        index: usize,
        chip: ShellChip,
        selected: bool,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let theme = cx.theme();
        let mut name = chip.label.chars();
        let letter: String = name.next().into_iter().collect();
        let rest: String = name.collect();
        let scope = chip.scope;
        let min_width = CHIP_MIN_WIDTH
            + if chip.icon.is_some() {
                CHIP_ICON_WIDTH
            } else {
                0.
            }
            + if rest.is_empty() {
                0.
            } else {
                CHIP_NAME_MIN_WIDTH
            };
        div()
            .id(("history-chip", index))
            .h(px(20.))
            .px_1()
            .flex()
            .items_center()
            .gap_1()
            .rounded_sm()
            .whitespace_nowrap()
            .when(selected, |chip| {
                chip.flex_none()
                    .bg(theme.primary)
                    .text_color(theme.primary_foreground)
                    .font_weight(FontWeight::BOLD)
            })
            .when(!selected, |chip| {
                chip.flex_shrink(1.)
                    .min_w(px(min_width))
                    .overflow_hidden()
                    .hover(|chip| chip.bg(theme.secondary))
                    .cursor_pointer()
            })
            .when_some(chip.icon, |chip, icon| {
                chip.child(launcher_icon(icon).xsmall().flex_none())
            })
            .child(
                div()
                    .flex()
                    .min_w_0()
                    .child(div().flex_none().underline().child(letter))
                    .when(!rest.is_empty(), |name| {
                        name.child(
                            div()
                                .when(!selected, |rest| {
                                    rest.flex_shrink(1.).min_w(px(CHIP_NAME_MIN_WIDTH))
                                })
                                .overflow_hidden()
                                .text_ellipsis()
                                .child(rest),
                        )
                    }),
            )
            .on_mouse_down(
                MouseButton::Left,
                cx.listener(move |view, _: &MouseDownEvent, _, cx| {
                    cx.stop_propagation();
                    view.select_history_chip(scope.clone(), cx);
                }),
            )
            .into_any_element()
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
        let scope = self.history_scope(&self.history_chips(cx));
        self.close_history_popup(cx);
        if let Some(input) = self.prompt_input() {
            self.put_history_command(&input, &scope, index, cx);
        }
    }
}

/// The keys: Delete's scope follows the selected chip; the Alt hint shows with the chips.
fn history_footer(scope: &HistoryScope, switches: bool, cx: &App) -> impl IntoElement {
    let theme = cx.theme();
    let remove_hint = if *scope == HistoryScope::All {
        "to remove from all shells"
    } else {
        "to remove"
    };
    div()
        .h(px(FOOTER_HEIGHT))
        .px_3()
        .flex()
        .items_center()
        .gap_1()
        .border_t_1()
        .border_color(theme.border)
        .text_xs()
        .text_color(theme.muted_foreground)
        .child(key_chip("↑", cx))
        .child(key_chip("↓", cx))
        .child(div().mr_2().child("to navigate"))
        .child(key_chip("enter", cx))
        .child(key_chip("tab", cx))
        .child(div().mr_2().child("to select"))
        .child(key_chip("del", cx))
        .child(div().mr_2().child(remove_hint))
        .child(key_chip("esc", cx))
        .child(div().mr_2().child("to dismiss"))
        .when(switches, |footer| {
            footer
                .child(key_chip("alt", cx))
                .child("letter to switch shell")
        })
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
