//! The right of the Git page: the selected file's diff. A header bar (path, side, counts),
//! then its hunks in a virtualized list: each hunk's header, then its lines with old and new
//! line numbers, in the terminal's monospace font, added lines tinted green and removed ones
//! red (colors set on the settings page). Long lines scroll sideways. Hunk buttons, selected
//! lines and the selection bar are drawn here; what they do is in [`super::line_staging`].

use std::time::Duration;

use gpui_kit::component::ActiveTheme;
use gpui_kit::prelude::FluentBuilder;
use gpui_kit::{
    AnyElement, App, Context, Hsla, InteractiveElement, IntoElement, KeyDownEvent,
    ListHorizontalSizingBehavior, MouseButton, ParentElement, Pixels, SharedString, Styled,
    UniformListScrollHandle, Window, div, font, point, px, uniform_list,
};
use workspace_model::git::{
    self, DiffLine, DiffRow, DiffSide, FileDiff, LineKind, LineRef, TextDiff,
};

use super::{DiffRead, GitPage, HEADER_HEIGHT};
use crate::color::to_hsla;
use crate::settings_file;
use crate::terminal_element::font_family;

/// How long a diff is read before "Loading…" shows (a quick read shows no flash).
pub(super) const LOADING_DELAY: Duration = Duration::from_millis(200);
/// Bytes in a megabyte, to name the diff size limit.
const BYTES_PER_MB: usize = 1 << 20;
/// A diff row's height, from the font size.
const LINE_HEIGHT_FACTOR: f32 = 1.4;
/// A tab shows as this many spaces.
const TAB_WIDTH: usize = 4;
/// A character's width, from the font size, when the font gives none.
const FALLBACK_CHAR_WIDTH_FACTOR: f32 = 0.6;
/// Characters of padding in each line-number gutter.
const GUTTER_PADDING_CHARS: f32 = 2.;
/// Characters taken by the `+` / `-` prefix and the space after it.
const PREFIX_CHARS: f32 = 2.;
/// A selected added or removed line's background is this many times as opaque, and this much
/// lighter (so an opaque color still shows the selection).
const SELECTED_BG_OPACITY_FACTOR: f32 = 2.;
const SELECTED_BG_LIGHTEN: f32 = 0.12;
/// Width of the bar left of a selected line or the line cursor.
const LINE_BAR_WIDTH: f32 = 2.;
/// Background alpha of a hunk's header band.
const HUNK_BG_ALPHA: f32 = 0.08;
const NO_NEWLINE_TEXT: &str = "No newline at end of file";
/// The diff header's note on a file whose lines can't be staged one by one.
pub(super) const WHOLE_FILE_NOTE: &str = "Stage the whole file";

/// The diff the page shows, with what its view needs worked out once.
pub(super) struct ShownDiff {
    side: DiffSide,
    path: String,
    orig_path: Option<String>,
    /// Lines can be staged one by one: the file can, and its diff shows lines.
    by_line: bool,
    result: Result<FileDiff, String>,
    /// The rows of a text diff (else empty).
    rows: Vec<DiffRow>,
    /// The added and removed lines of a text diff, in order.
    changed: Vec<LineRef>,
    /// The row the list's width is measured from.
    widest_row: Option<usize>,
    /// Lines added and removed.
    counts: (usize, usize),
    /// Digits of the largest line number.
    number_digits: usize,
}

impl ShownDiff {
    pub(super) fn new(read: DiffRead) -> Self {
        let text = match &read.result {
            Ok(FileDiff::Text(diff)) => Some(diff),
            _ => None,
        };
        let rows = text.map(TextDiff::rows).unwrap_or_default();
        let changed = text.map(TextDiff::changed_lines).unwrap_or_default();
        let widest_row = text.and_then(|diff| diff.widest_row(&rows, TAB_WIDTH));
        let counts = text.map(TextDiff::counts).unwrap_or_default();
        let number_digits = text.map_or(1, |diff| diff.max_line_no().max(1).ilog10() as usize + 1);
        Self {
            side: read.side,
            path: read.path,
            orig_path: read.orig_path,
            by_line: read.by_line && !changed.is_empty(),
            result: read.result,
            rows,
            changed,
            widest_row,
            counts,
            number_digits,
        }
    }

    /// Whether it is the diff of `path` on `side`.
    pub(super) fn is(&self, side: DiffSide, path: &str) -> bool {
        self.side == side && self.path == path
    }

    pub(super) fn text(&self) -> Option<&TextDiff> {
        match &self.result {
            Ok(FileDiff::Text(diff)) => Some(diff),
            _ => None,
        }
    }

    pub(super) fn side(&self) -> DiffSide {
        self.side
    }

    pub(super) fn path(&self) -> &str {
        &self.path
    }

    /// Whether its lines and hunks can be staged (or unstaged) one by one.
    pub(super) fn stages_by_line(&self) -> bool {
        self.by_line
    }

    pub(super) fn changed_lines(&self) -> &[LineRef] {
        &self.changed
    }

    /// The index of `line` among the changed lines.
    pub(super) fn line_index(&self, line: LineRef) -> Option<usize> {
        git::line_index(line, &self.changed)
    }

    /// The index in the rows of `line`.
    pub(super) fn row_of(&self, line: LineRef) -> Option<usize> {
        self.rows.iter().position(|row| *row == DiffRow::Line(line))
    }

    /// Whether only the whole file can be staged, though it shows (untracked, conflicted,
    /// binary, too large).
    fn whole_file_only(&self) -> bool {
        match &self.result {
            Ok(FileDiff::Text(diff)) => !diff.hunks.is_empty() && !self.by_line,
            Ok(_) => true,
            Err(_) => false,
        }
    }

    /// Whether `other` shows the same lines (a selection of one still fits the other).
    pub(super) fn same_lines(&self, other: &Self) -> bool {
        self.text() == other.text()
    }

    /// Why no line shows, if none does.
    pub(super) fn message(&self) -> Option<String> {
        match &self.result {
            Err(err) => Some(format!("Couldn't read the diff: {err}")),
            Ok(FileDiff::Binary) => Some("Binary file, not shown. It can be staged whole.".into()),
            Ok(FileDiff::TooLarge) => Some(format!(
                "This diff is too large to show (over {} MB). The file can be staged whole.",
                git::MAX_DIFF_BYTES / BYTES_PER_MB
            )),
            Ok(FileDiff::Text(diff)) if diff.hunks.is_empty() => {
                Some("No changes in the content (mode or rename only).".into())
            }
            Ok(FileDiff::Text(_)) => None,
        }
    }

    #[cfg(test)]
    pub(super) fn result(&self) -> &Result<FileDiff, String> {
        &self.result
    }
}

/// How a diff's rows look: their sizes in the current font, and the colors set.
struct Metrics {
    family: SharedString,
    font_size: Pixels,
    line_height: Pixels,
    char_width: Pixels,
    colors: Colors,
}

/// The diff colors from the settings; `None` for the theme's.
struct Colors {
    background: Option<Hsla>,
    text: Option<Hsla>,
    added: Hsla,
    removed: Hsla,
}

impl Colors {
    fn new(cx: &App) -> Self {
        let colors = &settings_file::settings(cx).git_diff_colors;
        Self {
            background: colors.background().map(to_hsla),
            text: colors.text().map(to_hsla),
            added: to_hsla(colors.added()),
            removed: to_hsla(colors.removed()),
        }
    }
}

impl Metrics {
    fn new(window: &Window, cx: &App) -> Self {
        let family = font_family(cx);
        let font_size = px(settings_file::settings(cx).font_size.points());
        let text_system = window.text_system();
        let font_id = text_system.resolve_font(&font(family.clone()));
        let char_width = text_system
            .advance(font_id, font_size, 'm')
            .map(|advance| advance.width)
            .unwrap_or(font_size * FALLBACK_CHAR_WIDTH_FACTOR);
        Self {
            family,
            font_size,
            line_height: (font_size * LINE_HEIGHT_FACTOR).round(),
            char_width,
            colors: Colors::new(cx),
        }
    }

    fn chars(&self, count: f32) -> Pixels {
        self.char_width * count
    }
}

/// Scroll a diff back to its top-left corner.
pub(super) fn scroll_to_top(scroll: &UniformListScrollHandle) {
    let state = scroll.0.borrow();
    state.base_handle.set_offset(point(px(0.), px(0.)));
}

/// Where a key scrolls the diff.
#[derive(Clone, Copy)]
enum DiffScroll {
    LineUp,
    LineDown,
    PageUp,
    PageDown,
    Top,
    Bottom,
}

impl GitPage {
    /// The right of the page: the diff, a message, or the placeholder.
    pub(super) fn render_diff_pane(&self, window: &Window, cx: &mut Context<Self>) -> AnyElement {
        let shown = self.diff.as_ref().filter(|_| self.selected.is_some());
        let current = shown
            .zip(self.selected.as_ref())
            .is_some_and(|(diff, (side, path))| diff.is(*side, path));
        let pane = div()
            .id("git-diff-pane")
            .track_focus(&self.diff_focus)
            .on_key_down(cx.listener(Self::diff_key_down))
            .on_mouse_down(
                MouseButton::Left,
                cx.listener(|page, _, window, cx| window.focus(&page.diff_focus, cx)),
            )
            .on_mouse_up(
                MouseButton::Left,
                cx.listener(|page, _, _, _| page.dragging = false),
            )
            .flex()
            .flex_col()
            .flex_1()
            .min_w_0()
            .h_full();
        let body = match shown {
            None if self.selected.is_none() => {
                return pane
                    .child(centered("Select a file to see its changes.", cx))
                    .into_any_element();
            }
            _ if self.loading && !current => {
                return pane.child(centered("Loading…", cx)).into_any_element();
            }
            None => return pane.into_any_element(),
            Some(diff) => diff,
        };
        let content = match body.message() {
            Some(message) => centered(message, cx),
            None => self.render_diff_list(body, window, cx),
        };
        pane.child(render_diff_header(body, cx))
            .child(content)
            .into_any_element()
    }

    fn render_diff_list(
        &self,
        diff: &ShownDiff,
        window: &Window,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let metrics = Metrics::new(window, cx);
        let list = uniform_list(
            "git-diff",
            diff.rows.len(),
            cx.processor(|page, range: std::ops::Range<usize>, window, cx| {
                let Some(diff) = &page.diff else {
                    return Vec::new();
                };
                let metrics = Metrics::new(window, cx);
                let focused = page.diff_focus.is_focused(window);
                range
                    .map(|row| page.render_diff_row(diff, row, focused, &metrics, cx))
                    .collect::<Vec<_>>()
            }),
        )
        .with_width_from_item(diff.widest_row)
        .with_horizontal_sizing_behavior(ListHorizontalSizingBehavior::Unconstrained)
        .track_scroll(&self.diff_scroll)
        .debug_selector(|| "git-diff".into())
        .when_some(metrics.colors.background, |list, color| list.bg(color))
        .when_some(metrics.colors.text, |list, color| list.text_color(color))
        .font_family(metrics.family)
        .text_size(metrics.font_size)
        .flex_1();
        let selected = self.lines.selected().len();
        div()
            .relative()
            .flex()
            .flex_col()
            .flex_1()
            .min_h_0()
            .child(list)
            .when(selected > 0 && diff.stages_by_line(), |pane| {
                pane.child(self.render_selection_bar(diff.side(), selected, cx))
            })
            .into_any_element()
    }

    /// In the diff: PageUp / PageDown, Home / End scroll, and so do Up / Down where lines
    /// can't be staged (else they move the line cursor, see [`Self::line_key_down`]). Escape
    /// clears the selected lines, or with none gives the keyboard back to the list (as Tab
    /// does, see `FocusNext`).
    fn diff_key_down(&mut self, event: &KeyDownEvent, window: &mut Window, cx: &mut Context<Self>) {
        let keystroke = &event.keystroke;
        if keystroke.key == "escape" && !keystroke.modifiers.modified() {
            if self.lines.clear() {
                cx.notify();
            } else {
                window.focus(&self.focus, cx);
            }
            cx.stop_propagation();
            return;
        }
        if self.line_key_down(keystroke, window, cx) {
            cx.stop_propagation();
            return;
        }
        if keystroke.modifiers.modified() {
            return;
        }
        let scroll = match keystroke.key.as_str() {
            "up" => DiffScroll::LineUp,
            "down" => DiffScroll::LineDown,
            "pageup" => DiffScroll::PageUp,
            "pagedown" => DiffScroll::PageDown,
            "home" => DiffScroll::Top,
            "end" => DiffScroll::Bottom,
            // Space would stage the selected file from the list.
            "space" => DiffScroll::PageDown,
            _ => return,
        };
        self.scroll_diff(scroll, window, cx);
        cx.stop_propagation();
    }

    fn scroll_diff(&mut self, scroll: DiffScroll, window: &Window, cx: &mut Context<Self>) {
        let line = Metrics::new(window, cx).line_height;
        let state = self.diff_scroll.0.borrow();
        let handle = &state.base_handle;
        let (offset, max) = (handle.offset(), handle.max_offset());
        let page = (handle.bounds().size.height - line).max(line);
        let y = match scroll {
            DiffScroll::LineUp => offset.y + line,
            DiffScroll::LineDown => offset.y - line,
            DiffScroll::PageUp => offset.y + page,
            DiffScroll::PageDown => offset.y - page,
            DiffScroll::Top => px(0.),
            DiffScroll::Bottom => -max.y,
        };
        handle.set_offset(point(offset.x, y.clamp(-max.y, px(0.))));
        cx.notify();
    }
}

/// The bar above the diff: the path (`old → new` for a rename), the side and the counts.
fn render_diff_header(diff: &ShownDiff, cx: &App) -> AnyElement {
    let theme = cx.theme();
    let path = match &diff.orig_path {
        Some(orig) => format!("{orig} → {}", diff.path),
        None => diff.path.clone(),
    };
    let side = match diff.side {
        DiffSide::Staged => "Staged",
        DiffSide::Unstaged => "Changes",
    };
    let (added, removed) = diff.counts;
    div()
        .flex()
        .flex_none()
        .items_center()
        .gap_3()
        .h(px(HEADER_HEIGHT))
        .px_3()
        .border_b_1()
        .border_color(theme.border)
        .text_sm()
        .child(
            div()
                .min_w_0()
                .overflow_hidden()
                .whitespace_nowrap()
                .text_ellipsis()
                .child(path),
        )
        .child(
            div()
                .flex_none()
                .px_1p5()
                .rounded_sm()
                .bg(theme.secondary)
                .text_xs()
                .text_color(theme.muted_foreground)
                .child(side),
        )
        .when(diff.text().is_some(), |header| {
            header.child(
                div()
                    .flex()
                    .flex_none()
                    .gap_1()
                    .text_xs()
                    .child(div().text_color(theme.success).child(format!("+{added}")))
                    .child(div().text_color(theme.danger).child(format!("−{removed}"))),
            )
        })
        .when(diff.whole_file_only(), |header| {
            header.child(
                div()
                    .flex_none()
                    .text_xs()
                    .text_color(theme.muted_foreground)
                    .child(WHOLE_FILE_NOTE),
            )
        })
        .into_any_element()
}

/// How a changed line shows besides its kind.
#[derive(Clone, Copy)]
struct LineLook {
    selected: bool,
    /// It holds the line cursor and the diff has the keyboard.
    cursor: bool,
}

impl GitPage {
    fn render_diff_row(
        &self,
        diff: &ShownDiff,
        row: usize,
        focused: bool,
        metrics: &Metrics,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let theme = cx.theme();
        let Some(text) = diff.text() else {
            return div().into_any_element();
        };
        let base = div()
            .flex()
            .items_center()
            .min_w_full()
            .h(metrics.line_height)
            .font_family(metrics.family.clone())
            .text_size(metrics.font_size)
            .whitespace_nowrap();
        let gutters = metrics.chars(2. * (diff.number_digits as f32 + GUTTER_PADDING_CHARS));
        match diff.rows[row] {
            DiffRow::Hunk(index) => {
                let header = base
                    .debug_selector(move || format!("git-diff-hunk-{index}"))
                    .bg(theme.muted_foreground.opacity(HUNK_BG_ALPHA))
                    .text_color(theme.muted_foreground)
                    .child(
                        div()
                            .flex_none()
                            .w(gutters + metrics.chars(PREFIX_CHARS) + px(LINE_BAR_WIDTH)),
                    )
                    .child(div().flex_none().child(text.hunks[index].header_text()));
                if !diff.stages_by_line() {
                    return header.into_any_element();
                }
                let group = SharedString::from(format!("git-diff-hunk-{index}"));
                header
                    .id(("git-diff-row", row))
                    .group(group.clone())
                    .child(self.render_hunk_button(diff.side(), index, group, cx))
                    .into_any_element()
            }
            DiffRow::Line(line_ref) => {
                let Some(line) = text.line(line_ref) else {
                    return base.into_any_element();
                };
                let look = LineLook {
                    selected: self.lines.contains(line_ref),
                    cursor: focused && self.lines.cursor() == Some(line_ref),
                };
                let element = render_line(base, line, diff.number_digits, look, metrics, cx);
                if !diff.stages_by_line() {
                    return element.into_any_element();
                }
                self.interactive_line(element, row, line_ref, line.kind, diff.side(), cx)
            }
            DiffRow::NoNewline(_) => base
                .text_color(theme.muted_foreground)
                .italic()
                .child(
                    div()
                        .flex_none()
                        .w(gutters + metrics.chars(PREFIX_CHARS) + px(LINE_BAR_WIDTH)),
                )
                .child(div().flex_none().child(NO_NEWLINE_TEXT))
                .into_any_element(),
        }
    }
}

fn render_line(
    base: gpui_kit::Div,
    line: &DiffLine,
    digits: usize,
    look: LineLook,
    metrics: &Metrics,
    cx: &App,
) -> gpui_kit::Div {
    let theme = cx.theme();
    let tint = |color: Hsla| {
        Some(if look.selected {
            Hsla {
                l: (color.l + SELECTED_BG_LIGHTEN).min(1.),
                a: (color.a * SELECTED_BG_OPACITY_FACTOR).min(1.),
                ..color
            }
        } else {
            color
        })
    };
    let (prefix, background) = match line.kind {
        LineKind::Context => (" ", None),
        LineKind::Added => ("+", tint(metrics.colors.added)),
        LineKind::Removed => ("-", tint(metrics.colors.removed)),
    };
    let bar = if look.cursor {
        theme.foreground
    } else if look.selected {
        theme.primary
    } else {
        gpui_kit::transparent_black()
    };
    let gutter = |number: Option<u32>| {
        div()
            .flex_none()
            .w(metrics.chars(digits as f32 + GUTTER_PADDING_CHARS))
            .pr(metrics.chars(GUTTER_PADDING_CHARS / 2.))
            .text_right()
            .text_color(theme.muted_foreground)
            .child(number.map(|number| number.to_string()).unwrap_or_default())
    };
    base.when_some(background, |row, background| row.bg(background))
        .child(div().flex_none().w(px(LINE_BAR_WIDTH)).h_full().bg(bar))
        .child(gutter(line.old_no))
        .child(gutter(line.new_no))
        .child(
            div()
                .flex_none()
                .w(metrics.chars(PREFIX_CHARS))
                .text_color(theme.muted_foreground)
                .child(prefix),
        )
        .child(
            div()
                .flex_none()
                .child(line.display_text(TAB_WIDTH).into_owned()),
        )
}

fn centered(message: impl Into<SharedString>, cx: &App) -> AnyElement {
    div()
        .flex()
        .flex_1()
        .items_center()
        .justify_center()
        .p_4()
        .text_sm()
        .text_color(cx.theme().muted_foreground)
        .child(message.into())
        .into_any_element()
}
