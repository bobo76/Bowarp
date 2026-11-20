//! Search in the rendered Markdown view (Ctrl+F): a bar under the header with the query, the
//! match count, previous / next, the case and whole-word toggles, and every match highlighted
//! in the page. The matching runs off the UI thread (`workspace_model::text_search`) on the
//! text the page renders, again whenever that text changes (a reload).

use std::ops::Range;

use gpui_kit::component::button::{Button, ButtonVariants};
use gpui_kit::component::input::{Escape, Input, InputEvent, InputState};
use gpui_kit::component::text::{RangeHighlight, RenderedText, TextViewState};
use gpui_kit::component::{ActiveTheme, IconName, Selectable, Sizable};
use gpui_kit::{
    AnyElement, AppContext, Bounds, Context, Entity, Focusable, Hsla, InteractiveElement,
    IntoElement, KeyDownEvent, ParentElement, Pixels, ScrollHandle, Styled, Subscription, Task,
    Window, div, point, px,
};
use workspace_model::layout::Direction;
use workspace_model::text_search::{self, SearchOptions};

use super::FileViewer;
use crate::settings_file;
use crate::terminal_element::to_hsla;

/// How strongly the terminal palette's search colors tint the page behind other matches and
/// behind the current one: the page paints a background only, so an opaque color would leave
/// light text unreadable on a dark theme.
const MATCH_TINT: f32 = 0.35;
const CURRENT_MATCH_TINT: f32 = 0.7;

/// The search field's narrowest width, toggles included.
const SEARCH_FIELD_MIN_WIDTH: f32 = 120.;

/// The search of one viewer. The query and the toggles stay while the viewer lives (never
/// saved), so Ctrl+F reopens with the last query.
#[derive(Default)]
pub(super) struct ViewerSearch {
    /// The search field, made on the first Ctrl+F.
    input: Option<Entity<InputState>>,
    options: SearchOptions,
    /// The bar shows.
    shown: bool,
    matches: Vec<Range<usize>>,
    current: Option<usize>,
    /// The rendered text `matches` index, and the one being searched now.
    searched: Option<RenderedText>,
    searching: Option<RenderedText>,
    /// The search running off the UI thread; replacing or dropping it cancels it.
    running: Option<Task<()>>,
    _input_events: Option<Subscription>,
}

/// Which match becomes the current one once a search ends.
#[derive(Clone, Copy)]
enum Pick {
    /// The first at or after this offset, scrolled into view (a new query or toggle).
    From(usize),
    /// The nearest to this offset, without scrolling (the text changed under the search).
    Nearest(usize),
}

/// Which toggle of the field.
#[derive(Clone, Copy)]
enum Toggle {
    MatchCase,
    WholeWord,
}

impl FileViewer {
    /// Ctrl+F: show the bar (a one-line selection in the page becomes the query) and focus its
    /// field with the query selected. `false` when the view has no search (only rendered
    /// Markdown has one so far).
    pub(super) fn open_search(&mut self, window: &mut Window, cx: &mut Context<Self>) -> bool {
        let Some(page) = self.markdown_page() else {
            return false;
        };
        let page_state = page.read(cx);
        let selection = one_line(page_state.selected_text());
        // Start at the selected occurrence (the top of the page without a selection).
        let selection_start = page_state
            .selected_source_range()
            .and_then(|source| page_state.rendered_text().range_for_source(source))
            .map_or(0, |range| range.start);
        let input = self.search_input(window, cx);
        if let Some(selection) = &selection {
            input.update(cx, |input, cx| {
                input.set_value(selection.clone(), window, cx)
            });
        }
        input.update(cx, |input, cx| {
            input.select_all(window, cx);
            input.focus(window, cx);
        });
        if !self.search.shown || selection.is_some() {
            self.search.shown = true;
            self.run_search(Pick::From(selection_start), cx);
        }
        cx.notify();
        true
    }

    /// The search field, made with its event handlers the first time.
    fn search_input(&mut self, window: &mut Window, cx: &mut Context<Self>) -> Entity<InputState> {
        if let Some(input) = &self.search.input {
            return input.clone();
        }
        let input = cx.new(|cx| InputState::new(window, cx).placeholder("Search file..."));
        let events = cx.subscribe_in(&input, window, |viewer, _, event, window, cx| match event {
            InputEvent::Change => viewer.run_search(Pick::From(viewer.current_start()), cx),
            InputEvent::PressEnter { shift, .. } => {
                let direction = if *shift {
                    Direction::Up
                } else {
                    Direction::Down
                };
                viewer.step_search(direction, window, cx);
            }
            _ => {}
        });
        self.search.input = Some(input.clone());
        self.search._input_events = Some(events);
        input
    }

    /// Esc in the bar or the page: close the bar and focus the page. `false` when it was
    /// closed.
    pub(super) fn close_search(&mut self, window: &mut Window, cx: &mut Context<Self>) -> bool {
        if !self.search.shown {
            return false;
        }
        self.hide_search(cx);
        if let Some(page) = self.markdown_page() {
            let handle = page.read(cx).focus_handle().clone();
            window.focus(&handle, cx);
        }
        true
    }

    /// Close the bar where focus stays (another file, the raw view): the matches and their
    /// highlights go, the query and toggles stay, the scroll position too.
    pub(super) fn hide_search(&mut self, cx: &mut Context<Self>) {
        if !self.search.shown {
            return;
        }
        let search = &mut self.search;
        search.shown = false;
        search.matches.clear();
        search.current = None;
        search.searched = None;
        search.searching = None;
        search.running = None;
        if let Some(page) = self.markdown_page() {
            page.update(cx, |page, cx| page.clear_range_highlights(cx));
        }
        cx.notify();
    }

    /// The page notified: search again when its text changed (a reload), keeping the current
    /// match as the nearest one, without scrolling.
    pub(super) fn on_page_changed(&mut self, cx: &mut Context<Self>) {
        let Some(page) = self.markdown_page().filter(|_| self.search.shown) else {
            return;
        };
        let text = page.read(cx).rendered_text();
        let latest = self
            .search
            .searching
            .as_ref()
            .or(self.search.searched.as_ref());
        if latest != Some(&text) {
            self.run_search(Pick::Nearest(self.current_start()), cx);
        }
    }

    /// Where the current match starts; the top of the page without one.
    fn current_start(&self) -> usize {
        let search = &self.search;
        search
            .current
            .and_then(|index| search.matches.get(index))
            .map_or(0, |found| found.start)
    }

    /// Search the page's text for the query off the UI thread, replacing a search running.
    fn run_search(&mut self, pick: Pick, cx: &mut Context<Self>) {
        let (Some(page), Some(input)) = (self.markdown_page(), &self.search.input) else {
            return;
        };
        let query = input.read(cx).value().to_string();
        let text = page.read(cx).rendered_text();
        // The index of the text is built on first use: let the background task do it.
        let haystack = text.clone();
        let options = self.search.options;
        self.search.searching = Some(text.clone());
        self.search.running = Some(cx.spawn(async move |this, cx| {
            let matches = cx
                .background_spawn(async move {
                    text_search::find_matches(haystack.as_str(), &query, options)
                })
                .await;
            let _ = this.update(cx, |viewer, cx| {
                viewer.finish_search(text, matches, pick, cx)
            });
        }));
    }

    /// Show the matches found in `text`, unless the page's text has changed since (the search
    /// it started replaces this one).
    fn finish_search(
        &mut self,
        text: RenderedText,
        matches: Vec<Range<usize>>,
        pick: Pick,
        cx: &mut Context<Self>,
    ) {
        let Some(page) = self.markdown_page() else {
            return;
        };
        if page.read(cx).rendered_text() != text {
            return;
        }
        let search = &mut self.search;
        search.current = match pick {
            Pick::From(offset) => text_search::first_match_from(&matches, offset),
            Pick::Nearest(offset) => text_search::nearest_match(&matches, offset),
        };
        search.matches = matches;
        search.searched = Some(text);
        search.searching = None;
        search.running = None;
        self.show_matches(cx);
        if matches!(pick, Pick::From(_)) {
            self.reveal_current(cx);
        }
    }

    /// Enter / Shift+Enter or the arrows: the next or previous match, wrapping around.
    fn step_search(&mut self, direction: Direction, window: &mut Window, cx: &mut Context<Self>) {
        let search = &mut self.search;
        search.current = text_search::step_match(search.current, search.matches.len(), direction);
        self.show_matches(cx);
        self.reveal_current(cx);
        if let Some(input) = &self.search.input {
            input.update(cx, |input, cx| input.focus(window, cx));
        }
    }

    /// Flip a toggle and search again from the current match.
    fn toggle_search_option(&mut self, toggle: Toggle, cx: &mut Context<Self>) {
        let options = &mut self.search.options;
        match toggle {
            Toggle::MatchCase => options.match_case = !options.match_case,
            Toggle::WholeWord => options.whole_word = !options.whole_word,
        }
        self.run_search(Pick::From(self.current_start()), cx);
        cx.notify();
    }

    /// The page's state when the matches were found in the text it shows now (not between a
    /// reload and the search of its new text).
    fn page_of_matches(&self, cx: &Context<Self>) -> Option<Entity<TextViewState>> {
        let page = self.markdown_page()?;
        let searched = self.search.searched.as_ref()?;
        (page.read(cx).rendered_text() == *searched).then_some(page)
    }

    /// Paint the matches in the page, the current one stronger.
    fn show_matches(&mut self, cx: &mut Context<Self>) {
        let Some(page) = self.page_of_matches(cx) else {
            return;
        };
        let highlights = self.match_highlights(cx);
        page.update(cx, |page, cx| {
            if let Err(err) = page.set_range_highlights(highlights, cx) {
                log::warn!("file viewer: search highlights rejected: {err}");
            }
        });
        cx.notify();
    }

    /// Scroll the page to the current match (only when it is off screen).
    fn reveal_current(&mut self, cx: &mut Context<Self>) {
        let Some(page) = self.page_of_matches(cx) else {
            return;
        };
        let Some(current) = self
            .search
            .current
            .map(|index| self.search.matches[index].clone())
        else {
            return;
        };
        page.update(cx, |page, cx| {
            if let Err(err) = page.reveal_range(current, cx) {
                log::warn!("file viewer: search match not revealed: {err}");
            }
        });
    }

    fn match_highlights(&self, cx: &Context<Self>) -> Vec<RangeHighlight> {
        let palette = settings_file::terminal_palette(cx);
        let tint = |color, alpha| Hsla {
            a: alpha,
            ..to_hsla(color)
        };
        let other = tint(palette.search, MATCH_TINT);
        let current = tint(palette.active_search, CURRENT_MATCH_TINT);
        let search = &self.search;
        search
            .matches
            .iter()
            .enumerate()
            .map(|(index, found)| {
                let color = if search.current == Some(index) {
                    current
                } else {
                    other
                };
                RangeHighlight::new(found.clone(), color)
            })
            .collect()
    }

    /// The page's state while the rendered Markdown view shows.
    fn markdown_page(&self) -> Option<Entity<TextViewState>> {
        let file = self.open_file()?;
        if file.mode != super::Mode::Rendered {
            return None;
        }
        match &file.rendered {
            Some(super::Rendered::Markdown(page)) => Some(page.state.clone()),
            _ => None,
        }
    }

    /// Whether the search field has focus.
    pub(super) fn search_has_focus(&self, window: &Window, cx: &gpui_kit::App) -> bool {
        self.search
            .input
            .as_ref()
            .is_some_and(|input| input.focus_handle(cx).is_focused(window))
    }

    /// Alt+C / Alt+W while the field has focus flip the toggles.
    fn on_search_key(&mut self, event: &KeyDownEvent, window: &mut Window, cx: &mut Context<Self>) {
        let keystroke = &event.keystroke;
        let modifiers = &keystroke.modifiers;
        let alt_only =
            modifiers.alt && !modifiers.control && !modifiers.shift && !modifiers.platform;
        let toggle = match keystroke.key.as_str() {
            "c" if alt_only => Toggle::MatchCase,
            "w" if alt_only => Toggle::WholeWord,
            _ => return,
        };
        if self.search_has_focus(window, cx) {
            self.toggle_search_option(toggle, cx);
            cx.stop_propagation();
        }
    }

    /// The bar, while it shows over rendered Markdown.
    pub(super) fn render_search_bar(&self, cx: &mut Context<Self>) -> Option<AnyElement> {
        let input = self.search.input.as_ref().filter(|_| self.search.shown)?;
        self.markdown_page()?;
        let query = input.read(cx).value();
        let search = &self.search;
        let count = text_search::match_count_label(&query, search.current, search.matches.len());
        let toggles = self.render_toggles(cx);
        let theme = cx.theme();
        Some(
            div()
                .flex_none()
                .flex()
                // A narrow viewer puts the count and the buttons on a second row.
                .flex_wrap()
                .items_center()
                .gap_x_2()
                .px_3()
                .py_1()
                .bg(theme.secondary)
                .border_b_1()
                .border_color(theme.border)
                .text_sm()
                .text_color(theme.secondary_foreground)
                .capture_key_down(cx.listener(Self::on_search_key))
                .on_action(cx.listener(|viewer, _: &Escape, window, cx| {
                    viewer.close_search(window, cx);
                }))
                .child(
                    div()
                        .flex_1()
                        // Room for a few characters beside the toggles in a narrow viewer.
                        .min_w(px(SEARCH_FIELD_MIN_WIDTH))
                        .child(Input::new(input).small().suffix(toggles)),
                )
                .child(
                    div()
                        .flex_none()
                        .whitespace_nowrap()
                        .text_color(theme.muted_foreground)
                        .child(count),
                )
                .child(self.step_button("viewer-previous-match", Direction::Up, cx))
                .child(self.step_button("viewer-next-match", Direction::Down, cx))
                .child(
                    Button::new("viewer-close-search")
                        .icon(IconName::Close)
                        .ghost()
                        .small()
                        .tooltip("Close search (Esc)")
                        .on_click(cx.listener(|viewer, _, window, cx| {
                            viewer.close_search(window, cx);
                        })),
                )
                .into_any_element(),
        )
    }

    fn step_button(
        &self,
        id: &'static str,
        direction: Direction,
        cx: &mut Context<Self>,
    ) -> Button {
        let (icon, tooltip) = match direction {
            Direction::Up => (IconName::ChevronUp, "Previous match (Shift+Enter)"),
            Direction::Down => (IconName::ChevronDown, "Next match (Enter)"),
        };
        Button::new(id)
            .icon(icon)
            .ghost()
            .small()
            .tooltip(tooltip)
            .on_click(cx.listener(move |viewer, _, window, cx| {
                viewer.step_search(direction, window, cx);
            }))
    }

    /// "Aa" and "W" at the field's right end.
    fn render_toggles(&self, cx: &mut Context<Self>) -> AnyElement {
        let options = self.search.options;
        div()
            .flex()
            .items_center()
            .gap_0p5()
            .child(self.toggle_button(Toggle::MatchCase, options.match_case, cx))
            .child(self.toggle_button(Toggle::WholeWord, options.whole_word, cx))
            .into_any_element()
    }

    fn toggle_button(&self, toggle: Toggle, on: bool, cx: &mut Context<Self>) -> Button {
        let (id, label, name, key) = match toggle {
            Toggle::MatchCase => ("viewer-match-case", "Aa", "Match case", "Alt+C"),
            Toggle::WholeWord => ("viewer-whole-word", "W", "Whole word", "Alt+W"),
        };
        let state = if on { "on" } else { "off" };
        Button::new(id)
            .label(label)
            .ghost()
            .xsmall()
            .selected(on)
            .tooltip(format!("{name}: {state} ({key})"))
            .on_click(cx.listener(move |viewer, _, window, cx| {
                viewer.toggle_search_option(toggle, cx);
                if let Some(input) = &viewer.search.input {
                    input.update(cx, |input, cx| input.focus(window, cx));
                }
            }))
    }

    /// Focus the rendered Markdown page, as a click in it does.
    #[cfg(test)]
    pub(crate) fn focus_page_for_test(&self, window: &mut Window, cx: &mut gpui_kit::App) {
        let page = self.markdown_page().expect("rendered Markdown shows");
        let handle = page.read(cx).focus_handle().clone();
        window.focus(&handle, cx);
    }

    /// Whether the rendered Markdown page has focus.
    #[cfg(test)]
    pub(crate) fn page_focused_for_test(&self, window: &Window, cx: &gpui_kit::App) -> bool {
        self.markdown_page()
            .is_some_and(|page| page.read(cx).focus_handle().is_focused(window))
    }

    /// The count the bar shows; `None` while it is closed.
    #[cfg(test)]
    pub(crate) fn search_count_for_test(&self, cx: &gpui_kit::App) -> Option<String> {
        let input = self.search.input.as_ref().filter(|_| self.search.shown)?;
        let query = input.read(cx).value();
        let search = &self.search;
        Some(text_search::match_count_label(
            &query,
            search.current,
            search.matches.len(),
        ))
    }

    /// The highlighted ranges of the page's rendered text, the current one first.
    #[cfg(test)]
    pub(crate) fn search_matches_for_test(&self) -> (Option<Range<usize>>, Vec<Range<usize>>) {
        let search = &self.search;
        let current = search.current.map(|index| search.matches[index].clone());
        (current, search.matches.clone())
    }
}

/// `text` when it is a non-empty selection within one line.
fn one_line(text: String) -> Option<String> {
    (!text.is_empty() && !text.contains('\n')).then_some(text)
}

/// Scroll the page's container (`scroll`) so `line`, in window coordinates, shows a third of
/// the way down: the page lays out every block, so the container scrolls, not the page.
pub(super) fn scroll_line_into_view(scroll: &ScrollHandle, line: Bounds<Pixels>) {
    let viewport = scroll.bounds();
    let offset = scroll.offset();
    let target = viewport.top() + viewport.size.height / 3.;
    let y = (offset.y - (line.top() - target))
        .max(-scroll.max_offset().y)
        .min(px(0.));
    scroll.set_offset(point(offset.x, y));
}
