//! The sidebar's "Search tabs..." field: it leaves the matching tabs in the sidebar, Up/Down
//! select the previous or next match, Enter selects the first one, Escape ends the search.

use std::collections::HashSet;

use gpui_kit::component::input::{Escape, Input, InputEvent, InputState, MoveDown, MoveUp};
use gpui_kit::component::{ActiveTheme, Icon, IconName, Sizable};
use gpui_kit::{
    AnyElement, App, AppContext, Context, Entity, InteractiveElement, IntoElement, ParentElement,
    Styled, Subscription, Window, div,
};
use workspace_model::{SearchedTab, TabQuery, step_match};

use super::{Tab, WorkspaceView};

pub(super) struct TabSearch {
    input: Entity<InputState>,
    /// The tab (its id) Up/Down moved to last, so closing or moving tabs does not shift it;
    /// cleared when the search changes or ends.
    cursor: Option<u64>,
    _events: Subscription,
}

impl TabSearch {
    pub fn new(window: &mut Window, cx: &mut Context<WorkspaceView>) -> Self {
        let input = cx.new(|cx| InputState::new(window, cx).placeholder("Search tabs..."));
        let events = cx.subscribe_in(
            &input,
            window,
            |workspace, _, event, window, cx| match event {
                InputEvent::Change => {
                    workspace.search.cursor = None;
                    cx.notify();
                }
                InputEvent::PressEnter { .. } => workspace.select_first_match(window, cx),
                _ => {}
            },
        );
        Self {
            input,
            cursor: None,
            _events: events,
        }
    }

    pub fn query(&self, cx: &App) -> TabQuery {
        TabQuery::parse(&self.input.read(cx).value())
    }
}

impl WorkspaceView {
    /// Whether the tab shows while searching (see [`TabQuery::matches_tab`]).
    fn tab_matches(&self, tab: &Tab, query: &TabQuery, cx: &App) -> bool {
        let view = tab.view.read(cx);
        let name = Self::tab_name(tab, cx);
        let searched = SearchedTab {
            name: &name,
            title: view.name(),
            location: view.location(),
            group: self
                .layout
                .group_of(tab.id)
                .map(|group| group.name.as_str()),
        };
        query.matches_tab(&searched, self.home.as_deref())
    }

    /// The tabs the search leaves in the sidebar; `None` when there is no search.
    pub(super) fn search_matches(&self, cx: &App) -> Option<HashSet<u64>> {
        let query = self.search.query(cx);
        if query.is_empty() {
            return None;
        }
        Some(
            self.tabs
                .iter()
                .filter(|tab| self.tab_matches(tab, &query, cx))
                .map(|tab| tab.id)
                .collect(),
        )
    }

    /// Clear the search when it hides tab `id`, about to be selected.
    pub(super) fn clear_search_hiding(&self, id: u64, window: &mut Window, cx: &mut App) {
        if self
            .search_matches(cx)
            .is_some_and(|matches| !matches.contains(&id))
        {
            self.search
                .input
                .update(cx, |search, cx| search.clean(window, cx));
        }
    }

    /// Put the keyboard in the search field, showing the sidebar if it is hidden.
    pub(super) fn focus_search(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.sidebar_visible = true;
        self.search.input.update(cx, |search, cx| {
            search.select_all(window, cx);
            search.focus(window, cx);
        });
        cx.notify();
    }

    /// Clear the search and give the keyboard back to the selected terminal.
    fn end_search(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.search.cursor = None;
        self.search
            .input
            .update(cx, |search, cx| search.clean(window, cx));
        self.focus_active(window, cx);
        cx.notify();
    }

    /// Enter in the search field: keep the match Up/Down moved to, else select the first tab
    /// shown, and end the search.
    fn select_first_match(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let query = self.search.query(cx);
        let first = (!query.is_empty())
            .then(|| {
                self.tabs
                    .iter()
                    .position(|tab| self.tab_matches(tab, &query, cx))
            })
            .flatten();
        if self.search.cursor.is_none()
            && let Some(index) = first
        {
            self.activate(index, window, cx);
        }
        self.end_search(window, cx);
    }

    /// Up/Down in the search field: select the previous or next tab the search shows, keeping
    /// the keyboard in the field to type on or move again.
    fn step_search(&mut self, forward: bool, window: &mut Window, cx: &mut Context<Self>) {
        let query = self.search.query(cx);
        if query.is_empty() {
            return;
        }
        let matching: Vec<bool> = self
            .tabs
            .iter()
            .map(|tab| self.tab_matches(tab, &query, cx))
            .collect();
        let cursor = self.search.cursor.and_then(|id| self.tab_index(id));
        let Some(index) = step_match(&matching, cursor, forward) else {
            return;
        };
        self.activate(index, window, cx);
        self.search.cursor = Some(self.tabs[index].id);
        self.search
            .input
            .update(cx, |search, cx| search.focus(window, cx));
    }

    /// The "Search tabs..." field: Up/Down select the previous or next match, Enter selects
    /// the first match (or keeps the one moved to), Escape clears it.
    pub(super) fn render_search(&self, cx: &mut Context<Self>) -> AnyElement {
        div()
            .flex_1()
            .min_w_0()
            .on_action(
                cx.listener(|workspace, _: &Escape, window, cx| workspace.end_search(window, cx)),
            )
            // Captured: the field itself would take Up/Down to move its caret.
            .capture_action(cx.listener(|workspace, _: &MoveUp, window, cx| {
                workspace.step_search(false, window, cx);
                cx.stop_propagation();
            }))
            .capture_action(cx.listener(|workspace, _: &MoveDown, window, cx| {
                workspace.step_search(true, window, cx);
                cx.stop_propagation();
            }))
            .child(
                Input::new(&self.search.input)
                    .small()
                    .cleanable(true)
                    .prefix(
                        Icon::new(IconName::Search)
                            .small()
                            .text_color(cx.theme().muted_foreground),
                    ),
            )
            .into_any_element()
    }
}
