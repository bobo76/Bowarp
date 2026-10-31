//! The keyboard in the sidebar's tab list: Down or Tab in the "Search tabs..." field moves it
//! there as one focus stop, the list keys select visible tabs, Enter, Escape or Tab end the
//! search and hand it to the terminal. Its keys are bound in the list's own key context only,
//! so the terminal never loses them; they are fixed navigation keys, not on the Keyboard
//! settings page. F2 (`RenameTab`, rebindable, see `workspace_view::default_bindings`) renames
//! the selected tab and gives the keyboard back to the list afterwards.

use std::cell::Cell;

use gpui_kit::{
    Action, App, Context, Div, FocusHandle, InteractiveElement, KeyBinding, ScrollHandle, Stateful,
    StatefulInteractiveElement, Window, actions,
};
use workspace_model::{ListMove, list_entry, list_step, listed_tabs};

use super::{RenameOrigin, RenameTab, RenameTarget, WorkspaceView};

/// The tab list's key context, below the workspace's.
pub(super) const KEY_CONTEXT: &str = "TabList";

actions!(
    tab_list,
    [
        SelectPrevious,
        SelectNext,
        SelectFirst,
        SelectLast,
        FocusTerminal,
        FocusSearch
    ]
);

pub(super) fn init(cx: &mut App) {
    cx.bind_keys([
        KeyBinding::new("up", SelectPrevious, Some(KEY_CONTEXT)),
        KeyBinding::new("down", SelectNext, Some(KEY_CONTEXT)),
        KeyBinding::new("home", SelectFirst, Some(KEY_CONTEXT)),
        KeyBinding::new("end", SelectLast, Some(KEY_CONTEXT)),
        KeyBinding::new("enter", FocusTerminal, Some(KEY_CONTEXT)),
        KeyBinding::new("escape", FocusTerminal, Some(KEY_CONTEXT)),
        // Bound here, the window's Tab focus cycling never steps into the rows' buttons.
        KeyBinding::new("tab", FocusTerminal, Some(KEY_CONTEXT)),
        KeyBinding::new("shift-tab", FocusSearch, Some(KEY_CONTEXT)),
    ]);
}

pub(super) struct TabList {
    /// Focused while the list has the keyboard; read at render for the focus ring.
    pub focus: FocusHandle,
    pub scroll: ScrollHandle,
    /// The next sidebar draw scrolls the selected row into view (set by the list keys).
    pub scroll_to_selected: Cell<bool>,
}

impl TabList {
    pub fn new(cx: &mut App) -> Self {
        Self {
            focus: cx.focus_handle(),
            scroll: ScrollHandle::new(),
            scroll_to_selected: Cell::new(false),
        }
    }
}

impl WorkspaceView {
    /// For each tab (sidebar order), whether the sidebar shows its row.
    fn listed_tabs(&self, cx: &App) -> Vec<bool> {
        let order: Vec<u64> = self.tabs.iter().map(|tab| tab.id).collect();
        listed_tabs(&self.layout, &order, self.search_matches(cx).as_ref())
    }

    /// Down or Tab in the search field: move the keyboard into the tab list, on the selected
    /// tab or, when the sidebar does not show it, the first row. With no row (no tab open, or
    /// a search matching none) the keyboard stays where it is.
    pub(super) fn enter_tab_list(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(index) = list_entry(&self.listed_tabs(cx), Some(self.active)) else {
            return;
        };
        window.focus(&self.tab_list.focus, cx);
        if self.settings.shows_page() {
            // The Settings row is the selected entry: the page stays until a key moves off it.
            cx.notify();
            return;
        }
        self.select_from_list(index, window, cx);
    }

    /// A list key: select the row it moves to, keeping the keyboard in the list.
    fn step_tab_list(&mut self, step: ListMove, window: &mut Window, cx: &mut Context<Self>) {
        let listed = self.listed_tabs(cx);
        // From the Settings row, Up / Down pick the tab entering the list would have picked.
        let from_settings =
            self.settings.shows_page() && matches!(step, ListMove::Previous | ListMove::Next);
        let index = if from_settings {
            list_entry(&listed, Some(self.active))
        } else {
            list_step(&listed, Some(self.active), step)
        };
        if let Some(index) = index {
            self.select_from_list(index, window, cx);
        }
    }

    /// F2 in the list: edit the selected tab's name. Nothing while the ring sits on the
    /// Settings row (the selected tab is hidden under the page) or the selected row isn't shown.
    fn rename_selected(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.settings.shows_page() {
            return;
        }
        let Some(id) = self.active_tab().map(|tab| tab.id) else {
            return;
        };
        if !self.selected_tab_is_listed(cx) {
            return;
        }
        self.tab_list.scroll_to_selected.set(true);
        self.start_rename(RenameTarget::Tab(id), RenameOrigin::TabList, window, cx);
    }

    /// The selected tab has a row in the list (not hidden by the search or a collapsed group).
    fn selected_tab_is_listed(&self, cx: &App) -> bool {
        self.listed_tabs(cx).get(self.active) == Some(&true)
    }

    /// A rename started from the list ended: the list gets the keyboard back while the selected
    /// tab has a row, else the terminal (or the page) does, as when the list is left.
    pub(super) fn return_to_tab_list(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.sidebar_visible && self.selected_tab_is_listed(cx) {
            window.focus(&self.tab_list.focus, cx);
        } else {
            self.focus_active(window, cx);
        }
    }

    fn select_from_list(&mut self, index: usize, window: &mut Window, cx: &mut Context<Self>) {
        if self.select_tab(index, window, cx).is_some() {
            self.search.set_cursor(self.tabs[index].id);
            self.tab_list.scroll_to_selected.set(true);
        }
    }

    /// Wraps the sidebar's rows: the list's key context, focus and scrolling.
    pub(super) fn tab_list_keys(
        &self,
        list: Stateful<Div>,
        cx: &mut Context<Self>,
    ) -> Stateful<Div> {
        list.key_context(KEY_CONTEXT)
            .track_focus(&self.tab_list.focus)
            .track_scroll(&self.tab_list.scroll)
            .on_action(list_key::<SelectPrevious>(cx, |workspace, window, cx| {
                workspace.step_tab_list(ListMove::Previous, window, cx);
            }))
            .on_action(list_key::<SelectNext>(cx, |workspace, window, cx| {
                workspace.step_tab_list(ListMove::Next, window, cx);
            }))
            .on_action(list_key::<SelectFirst>(cx, |workspace, window, cx| {
                workspace.step_tab_list(ListMove::First, window, cx);
            }))
            .on_action(list_key::<SelectLast>(cx, |workspace, window, cx| {
                workspace.step_tab_list(ListMove::Last, window, cx);
            }))
            .on_action(list_key::<FocusTerminal>(cx, |workspace, window, cx| {
                workspace.end_search(window, cx);
            }))
            .on_action(list_key::<FocusSearch>(cx, |workspace, window, cx| {
                workspace.focus_search(window, cx);
            }))
            .on_action(list_key::<RenameTab>(cx, |workspace, window, cx| {
                workspace.rename_selected(window, cx);
            }))
            // A click never gives the list the keyboard: clicking a row still focuses the
            // terminal, and clicking a header or the empty space leaves the keyboard alone.
            // The rows' own buttons and the name editor handle the press first.
            .on_any_mouse_down(|_, window, _| window.prevent_default())
    }
}

/// A listener for one of the list's keys, run only while the list itself has the keyboard.
/// A key pressed in the name editor inside it (a single-line input lets Enter, Tab, Up and
/// Down through) goes on to the bindings below, as if the list didn't bind it.
fn list_key<A: Action>(
    cx: &mut Context<WorkspaceView>,
    run: fn(&mut WorkspaceView, &mut Window, &mut Context<WorkspaceView>),
) -> impl Fn(&A, &mut Window, &mut App) + 'static {
    cx.listener(move |workspace, _: &A, window, cx| {
        if workspace.tab_list.focus.is_focused(window) {
            run(workspace, window, cx);
        } else {
            cx.propagate();
        }
    })
}
