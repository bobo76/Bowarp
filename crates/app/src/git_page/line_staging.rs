//! Staging or unstaging single lines and hunks from the Git page's diff, as VS Code does: a
//! click selects a changed line, Shift extends from the anchor, Ctrl (Cmd) adds or removes one,
//! a drag selects a range; in the diff, Up / Down move a line cursor over the changed lines,
//! `S` / `U` stage or unstage the selection (else the cursor's line), Shift+`S` / Shift+`U` the
//! cursor's hunk. The patch is built here, on the UI thread, from the diff on screen, so it is
//! exactly what the user picked; `git apply` refuses it when the index changed since.

use std::collections::BTreeSet;

use gpui_kit::component::button::{Button, ButtonVariants};
use gpui_kit::component::menu::{ContextMenuExt, PopupMenu, PopupMenuItem};
use gpui_kit::component::{ActiveTheme, Disableable, IconName, Sizable};
use gpui_kit::prelude::FluentBuilder;
use gpui_kit::{
    AnyElement, Context, Div, InteractiveElement, IntoElement, Keystroke, Modifiers, MouseButton,
    MouseDownEvent, MouseMoveEvent, ParentElement, ScrollStrategy, SharedString, Styled, Window,
    div,
};
use workspace_model::git::{self, DiffSide, LineKind, LineRef};
use workspace_model::layout::Direction;

use super::diff_view::ShownDiff;
use super::{GitPage, IndexChange};

/// The words for an action on `side`'s diff: stage from Changes, unstage from Staged.
struct Words {
    verb: &'static str,
    hunk: &'static str,
    selected: &'static str,
}

fn words(side: DiffSide) -> Words {
    match side {
        DiffSide::Unstaged => Words {
            verb: "Stage",
            hunk: "Stage hunk",
            selected: "Stage selected lines",
        },
        DiffSide::Staged => Words {
            verb: "Unstage",
            hunk: "Unstage hunk",
            selected: "Unstage selected lines",
        },
    }
}

impl GitPage {
    /// Stage (`side` Unstaged) or unstage (`side` Staged) the selected lines, else the
    /// cursor's line, of the diff on `side`.
    pub(super) fn apply_selection(
        &mut self,
        side: DiffSide,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let lines = self.lines.targets();
        self.apply_lines(side, &lines, window, cx);
    }

    /// Stage or unstage hunk `hunk` of the diff on `side`.
    pub(super) fn apply_hunk(
        &mut self,
        side: DiffSide,
        hunk: usize,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(lines) = self
            .diff
            .as_ref()
            .and_then(|diff| diff.text())
            .map(|text| text.hunk_lines(hunk))
        else {
            return;
        };
        self.apply_lines(side, &lines, window, cx);
    }

    /// Queue the patch of `lines` when the selected file's diff on `side` shows and stages by
    /// line. Nothing while an op runs: the diff on screen may be about to change.
    fn apply_lines(
        &mut self,
        side: DiffSide,
        lines: &BTreeSet<LineRef>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.running {
            return;
        }
        let Some(diff) = self.diff.as_ref().filter(|diff| {
            diff.side() == side && diff.stages_by_line() && self.is_selected(side, diff.path())
        }) else {
            return;
        };
        let Some((text, first)) = diff.text().zip(lines.first()) else {
            return;
        };
        let Some(at) = diff.line_index(*first) else {
            return;
        };
        let Some(patch) = git::partial_patch(diff.path(), text, lines, side) else {
            return;
        };
        self.applied_at = Some(at);
        self.change_index(IndexChange::Apply { side, patch }, window, cx);
    }

    /// The diff's line keys; returns whether `keystroke` was one. Only in a diff whose lines
    /// stage one by one, and never with Ctrl, Alt or Cmd.
    pub(super) fn line_key_down(
        &mut self,
        keystroke: &Keystroke,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> bool {
        let Modifiers {
            control,
            alt,
            shift,
            platform,
            function,
        } = keystroke.modifiers;
        if control || alt || platform || function {
            return false;
        }
        if !self.diff.as_ref().is_some_and(ShownDiff::stages_by_line) {
            return false;
        }
        // `S` / `U` act on the diff of their side only (`apply_lines` checks it).
        match (keystroke.key.to_ascii_lowercase().as_str(), shift) {
            ("up", false) => self.step_line_cursor(Direction::Up, cx),
            ("down", false) => self.step_line_cursor(Direction::Down, cx),
            ("up", true) => self.extend_line_selection(Direction::Up, cx),
            ("down", true) => self.extend_line_selection(Direction::Down, cx),
            ("s", false) => self.apply_selection(DiffSide::Unstaged, window, cx),
            ("u", false) => self.apply_selection(DiffSide::Staged, window, cx),
            ("s", true) => self.apply_cursor_hunk(DiffSide::Unstaged, window, cx),
            ("u", true) => self.apply_cursor_hunk(DiffSide::Staged, window, cx),
            _ => return false,
        }
        true
    }

    /// Up / Down: move the line cursor, clearing the selection.
    fn step_line_cursor(&mut self, direction: Direction, cx: &mut Context<Self>) {
        let Some(diff) = &self.diff else {
            return;
        };
        let cursor = self.lines.step(direction, diff.changed_lines());
        self.reveal_line(cursor, cx);
    }

    /// Shift+Up / Shift+Down: move the line cursor, the selection running from the anchor.
    fn extend_line_selection(&mut self, direction: Direction, cx: &mut Context<Self>) {
        let Some(diff) = &self.diff else {
            return;
        };
        let cursor = self.lines.extend(direction, diff.changed_lines());
        self.reveal_line(cursor, cx);
    }

    /// Scroll the diff to show the line cursor, now at `cursor`.
    fn reveal_line(&mut self, cursor: Option<LineRef>, cx: &mut Context<Self>) {
        let row = cursor.and_then(|line| self.diff.as_ref()?.row_of(line));
        if let Some(row) = row {
            self.diff_scroll
                .scroll_to_item(row, ScrollStrategy::Nearest);
        }
        cx.notify();
    }

    /// Shift+`S` / Shift+`U`: stage or unstage the cursor's hunk.
    fn apply_cursor_hunk(&mut self, side: DiffSide, window: &mut Window, cx: &mut Context<Self>) {
        if let Some(cursor) = self.lines.cursor() {
            self.apply_hunk(side, cursor.hunk, window, cx);
        }
    }

    /// A row of a diff whose lines stage one by one, made clickable: a changed line selects,
    /// drags and opens the menu; a context line clears the selection.
    pub(super) fn interactive_line(
        &self,
        element: Div,
        row: usize,
        line: LineRef,
        kind: LineKind,
        side: DiffSide,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let element = element
            .id(("git-diff-row", row))
            .debug_selector(move || format!("git-diff-row-{row}"));
        if kind == LineKind::Context {
            return element
                .on_mouse_down(
                    MouseButton::Left,
                    cx.listener(|page, _, _, cx| {
                        if page.lines.clear() {
                            cx.notify();
                        }
                    }),
                )
                .into_any_element();
        }
        element
            .on_mouse_down(
                MouseButton::Left,
                cx.listener(move |page, event: &MouseDownEvent, window, cx| {
                    page.press_line(line, event.modifiers, window, cx);
                }),
            )
            .on_mouse_move(cx.listener(move |page, event: &MouseMoveEvent, _, cx| {
                page.drag_over(line, event, cx);
            }))
            // A right click outside the selection selects the line first, as in VS Code.
            .on_mouse_down(
                MouseButton::Right,
                cx.listener(move |page, _, _, cx| {
                    if !page.lines.contains(line) {
                        page.lines.click(line);
                        cx.notify();
                    }
                }),
            )
            .context_menu(self.line_menu(side, line.hunk, cx))
            .into_any_element()
    }

    fn press_line(
        &mut self,
        line: LineRef,
        modifiers: Modifiers,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(diff) = &self.diff else {
            return;
        };
        window.focus(&self.diff_focus, cx);
        if modifiers.shift {
            self.lines.shift_click(line, diff.changed_lines());
        } else if modifiers.secondary() {
            self.lines.toggle(line);
        } else {
            self.lines.click(line);
        }
        self.dragging = !modifiers.secondary();
        cx.notify();
    }

    /// While the button is held from a press on a changed line, the range runs to `line`.
    fn drag_over(&mut self, line: LineRef, event: &MouseMoveEvent, cx: &mut Context<Self>) {
        if !self.dragging {
            return;
        }
        if event.pressed_button != Some(MouseButton::Left) {
            // Released outside the diff.
            self.dragging = false;
            return;
        }
        let Some(diff) = &self.diff else {
            return;
        };
        if self.lines.cursor() != Some(line) {
            self.lines.shift_click(line, diff.changed_lines());
            cx.notify();
        }
    }

    fn line_menu(
        &self,
        side: DiffSide,
        hunk: usize,
        cx: &Context<Self>,
    ) -> impl Fn(PopupMenu, &mut Window, &mut Context<PopupMenu>) -> PopupMenu + 'static + use<>
    {
        let page = cx.entity().downgrade();
        move |menu, _, cx| {
            let running = page.upgrade().is_none_or(|page| page.read(cx).running);
            let words = words(side);
            let (lines_page, hunk_page) = (page.clone(), page.clone());
            menu.item(
                PopupMenuItem::new(words.selected)
                    .disabled(running)
                    .on_click(move |_, window, cx| {
                        let _ = lines_page.update(cx, |page, cx| {
                            page.apply_selection(side, window, cx);
                        });
                    }),
            )
            .item(PopupMenuItem::new(words.hunk).disabled(running).on_click(
                move |_, window, cx| {
                    let _ = hunk_page.update(cx, |page, cx| {
                        page.apply_hunk(side, hunk, window, cx);
                    });
                },
            ))
        }
    }

    /// The `+` / `−` button of hunk `hunk`'s header: shown on hover, and always while the hunk
    /// holds the line cursor.
    pub(super) fn render_hunk_button(
        &self,
        side: DiffSide,
        hunk: usize,
        group: SharedString,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let holds_cursor = self.lines.cursor().is_some_and(|line| line.hunk == hunk);
        let icon = match side {
            DiffSide::Unstaged => IconName::Plus,
            DiffSide::Staged => IconName::Minus,
        };
        div()
            .debug_selector(move || format!("git-hunk-apply-{hunk}"))
            .flex_none()
            .ml_4()
            .when(!holds_cursor, |button| {
                button
                    .invisible()
                    .group_hover(group, |style| style.visible())
            })
            .child(
                Button::new(("git-hunk-apply", hunk))
                    .icon(icon)
                    .ghost()
                    .xsmall()
                    .disabled(self.running)
                    .tooltip(words(side).hunk)
                    .on_click(cx.listener(move |page, _, window, cx| {
                        cx.stop_propagation();
                        page.apply_hunk(side, hunk, window, cx);
                    })),
            )
            .into_any_element()
    }

    /// The bar over the diff's top right while lines are selected: "Stage 3 lines" and
    /// "Clear".
    pub(super) fn render_selection_bar(
        &self,
        side: DiffSide,
        count: usize,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let theme = cx.theme();
        let noun = if count == 1 { "line" } else { "lines" };
        let label = format!("{} {count} {noun}", words(side).verb);
        div()
            .id("git-selection-bar")
            .debug_selector(|| "git-selection-bar".into())
            .occlude()
            .absolute()
            .top_2()
            .right_4()
            .flex()
            .items_center()
            .gap_1()
            .p_1()
            .rounded_md()
            .border_1()
            .border_color(theme.border)
            .bg(theme.popover)
            .shadow_md()
            .child(
                Button::new("git-apply-lines")
                    .label(label)
                    .primary()
                    .xsmall()
                    .disabled(self.running)
                    .on_click(cx.listener(move |page, _, window, cx| {
                        page.apply_selection(side, window, cx);
                    })),
            )
            .child(
                Button::new("git-clear-lines")
                    .label("Clear")
                    .ghost()
                    .xsmall()
                    .on_click(cx.listener(|page, _, _, cx| {
                        page.lines.clear();
                        cx.notify();
                    })),
            )
            .into_any_element()
    }
}

#[cfg(test)]
mod tests;
