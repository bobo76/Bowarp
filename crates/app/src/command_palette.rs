//! The command palette (Ctrl+Shift+P): an overlay under the title bar with a search field over
//! the tabs (by name, "go to tab") and the Keyboard page's actions with their keys. The
//! workspace builds the rows when it opens the palette and runs the chosen one
//! (`workspace_view::palette`); the palette is its own entity, redrawn only while it is used.
//!
//! An empty query shows two sections, Tabs then Actions; a typed one a single list ranked by
//! `workspace_model::rank_matches`. Up/Down (the list's keys), PageUp/PageDown, Enter and a
//! click choose; Esc, a click outside or the keyboard leaving it close it ([`PaletteEvent`]),
//! except to a dialog, which the palette waits under.

use gpui_kit::component::list::{List, ListDelegate, ListEvent, ListItem, ListState};
use gpui_kit::component::{ActiveTheme, Icon, IconName, IndexPath, Sizable, WindowExt};
use gpui_kit::{
    App, AppContext, Context, Entity, EventEmitter, FocusHandle, FontWeight, HighlightStyle,
    InteractiveElement, IntoElement, KeyBinding, Keystroke, ParentElement, Render, SharedString,
    Styled, StyledText, Subscription, Task, Window, actions, div, px,
};
use workspace_model::{
    Candidate, FuzzyMatch, SessionKind, highlight_ranges, rank_matches, short_name,
};

use crate::shortcuts;
use crate::sidebar::{self, attention_color};

/// The palette's key context, around its list.
const PALETTE_CONTEXT: &str = "CommandPalette";
/// The palette's width; a narrower window shrinks it.
const PALETTE_WIDTH: f32 = 560.;
/// About ten rows, then the list scrolls.
const PALETTE_LIST_HEIGHT: f32 = 360.;
/// The rows PageUp and PageDown move by.
const PAGE_ROWS: usize = 10;
const PLACEHOLDER: &str = "Search actions and tabs";
const NO_MATCH: &str = "No action or tab matches.";
const TABS_TITLE: &str = "Tabs";
const ACTIONS_TITLE: &str = "Actions";

actions!(
    command_palette,
    [
        /// PageUp in the field: the selection moves up by a page.
        SelectPageUp,
        /// PageDown in the field.
        SelectPageDown,
        /// Tab and Shift+Tab: the keyboard stays in the field.
        KeepFocus
    ]
);

/// The palette's own keys; Up, Down, Enter and Esc are the list's.
pub fn init(cx: &mut App) {
    let context = Some(PALETTE_CONTEXT);
    cx.bind_keys([
        KeyBinding::new("pageup", SelectPageUp, context),
        KeyBinding::new("pagedown", SelectPageDown, context),
        KeyBinding::new("tab", KeepFocus, context),
        KeyBinding::new("shift-tab", KeepFocus, context),
    ]);
}

/// What a row runs.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PaletteChoice {
    /// Select this tab (its id), as a sidebar click does.
    Tab(u64),
    /// Run this action (its full keymap name).
    Action(&'static str),
}

/// A tab's row, in sidebar order.
pub struct TabRow {
    pub id: u64,
    pub name: String,
    pub group: Option<String>,
    pub kind: SessionKind,
    pub needs_attention: bool,
    /// The tab selected in the sidebar: checked.
    pub selected: bool,
}

/// An action's row, in Keyboard page order.
pub struct ActionRow {
    pub action: &'static str,
    pub label: String,
    /// Its first key, shown as chips; `None` while it has none.
    pub keys: Option<Vec<Keystroke>>,
}

/// What the palette tells the workspace.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PaletteEvent {
    /// A row was chosen: close the palette, keyboard back, then run it.
    Run(PaletteChoice),
    /// Esc, a click outside, or the keyboard going elsewhere (a shortcut): close it,
    /// the keyboard back where it was unless it went elsewhere.
    Dismiss,
}

pub struct CommandPalette {
    list: Entity<ListState<PaletteDelegate>>,
    /// Around the field and the list: the palette has the keyboard while this contains it.
    focus: FocusHandle,
    _subscriptions: [Subscription; 2],
}

impl EventEmitter<PaletteEvent> for CommandPalette {}

impl CommandPalette {
    /// The palette over `tabs` and `actions`, the first row selected and the keyboard in its
    /// empty field.
    pub fn new(
        tabs: Vec<TabRow>,
        actions: Vec<ActionRow>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        let delegate = PaletteDelegate::new(tabs, actions);
        let list = cx.new(|cx| ListState::new(delegate, window, cx).searchable(true));
        list.update(cx, |list, cx| {
            let first = list.delegate().first_row();
            list.set_selected_index(first, window, cx);
            list.focus(window, cx);
        });
        let focus = cx.focus_handle();
        let list_events = cx.subscribe_in(&list, window, Self::on_list_event);
        // Not when the window goes inactive (its focus path then reads as empty), nor to a
        // dialog a shortcut opened over it: the palette stays open, and the dialog gives the
        // keyboard back to its field on close (gpui-component records it as the previous focus).
        let focus_out = cx.on_focus_out(&focus, window, |_, _, window, cx| {
            if window.is_window_active() && !window.has_active_dialog(cx) {
                cx.emit(PaletteEvent::Dismiss);
            }
        });
        Self {
            list,
            focus,
            _subscriptions: [list_events, focus_out],
        }
    }

    /// Whether the keyboard is in the palette.
    pub fn has_focus(&self, window: &Window, cx: &App) -> bool {
        self.focus.contains_focused(window, cx)
    }

    /// The rows shown, in order.
    #[cfg(test)]
    pub fn shown_choices(&self, cx: &App) -> Vec<PaletteChoice> {
        let delegate = self.list.read(cx).delegate();
        delegate
            .row_paths()
            .into_iter()
            .filter_map(|ix| delegate.choice(ix))
            .collect()
    }

    /// The row selected, if any.
    #[cfg(test)]
    pub fn selected_choice(&self, cx: &App) -> Option<PaletteChoice> {
        let list = self.list.read(cx);
        list.delegate().choice(list.selected_index()?)
    }

    fn on_list_event(
        &mut self,
        list: &Entity<ListState<PaletteDelegate>>,
        event: &ListEvent,
        _: &mut Window,
        cx: &mut Context<Self>,
    ) {
        match event {
            ListEvent::Confirm(ix) => {
                if let Some(choice) = list.read(cx).delegate().choice(*ix) {
                    cx.emit(PaletteEvent::Run(choice));
                }
            }
            ListEvent::Cancel => cx.emit(PaletteEvent::Dismiss),
            ListEvent::Select(_) => {}
        }
    }

    /// PageUp / PageDown: move the selection by a page, staying on the first or last row.
    fn page(&mut self, direction: PageDirection, window: &mut Window, cx: &mut Context<Self>) {
        self.list.update(cx, |list, cx| {
            let target = list.delegate().page_from(list.selected_index(), direction);
            if target.is_some() {
                list.set_selected_index(target, window, cx);
                list.scroll_to_selected_item(window, cx);
            }
        });
    }
}

impl Render for CommandPalette {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = cx.theme();
        div()
            .id("command-palette")
            .key_context(PALETTE_CONTEXT)
            .track_focus(&self.focus)
            .occlude()
            .on_action(|_: &KeepFocus, _, _| {})
            .on_action(cx.listener(|palette, _: &SelectPageUp, window, cx| {
                palette.page(PageDirection::Up, window, cx);
            }))
            .on_action(cx.listener(|palette, _: &SelectPageDown, window, cx| {
                palette.page(PageDirection::Down, window, cx);
            }))
            .on_mouse_down_out(cx.listener(|_, _, _, cx| cx.emit(PaletteEvent::Dismiss)))
            .w(px(PALETTE_WIDTH))
            .max_w_full()
            .bg(theme.popover)
            .text_color(theme.popover_foreground)
            .border_1()
            .border_color(theme.border)
            .rounded_md()
            .shadow_lg()
            .overflow_hidden()
            .child(
                List::new(&self.list)
                    .search_placeholder(PLACEHOLDER)
                    .max_h(px(PALETTE_LIST_HEIGHT)),
            )
    }
}

/// The palette placed over the workspace's content: centered at its top, over the sidebar
/// and the terminal. Only the palette takes the pointer; the strip around it lets it through.
pub fn overlay(palette: Entity<CommandPalette>) -> impl IntoElement {
    div()
        .absolute()
        .top_0()
        .left_0()
        .right_0()
        .px_4()
        .pt_1()
        .flex()
        .justify_center()
        .child(palette)
}

/// A row shown: which tab or action, and its label's matched characters.
struct Shown {
    row: RowRef,
    positions: Vec<usize>,
}

/// Which way PageUp and PageDown move the selection.
#[derive(Clone, Copy)]
enum PageDirection {
    Up,
    Down,
}

#[derive(Clone, Copy)]
enum RowRef {
    Tab(usize),
    Action(usize),
}

struct Section {
    /// `None` for a typed query's single list.
    title: Option<&'static str>,
    rows: Vec<Shown>,
}

/// The palette's list: every row, and the ones the query shows.
struct PaletteDelegate {
    tabs: Vec<TabRow>,
    actions: Vec<ActionRow>,
    sections: Vec<Section>,
}

impl PaletteDelegate {
    fn new(tabs: Vec<TabRow>, actions: Vec<ActionRow>) -> Self {
        let mut delegate = Self {
            tabs,
            actions,
            sections: Vec::new(),
        };
        delegate.sections = delegate.all_sections();
        delegate
    }

    /// No query: Tabs (none with no tab), then Actions.
    fn all_sections(&self) -> Vec<Section> {
        let unmatched = |row| Shown {
            row,
            positions: Vec::new(),
        };
        let tabs = (0..self.tabs.len()).map(|index| unmatched(RowRef::Tab(index)));
        let actions = (0..self.actions.len()).map(|index| unmatched(RowRef::Action(index)));
        [
            (TABS_TITLE, tabs.collect()),
            (ACTIONS_TITLE, actions.collect()),
        ]
        .into_iter()
        .filter(|(_, rows): &(_, Vec<Shown>)| !rows.is_empty())
        .map(|(title, rows)| Section {
            title: Some(title),
            rows,
        })
        .collect()
    }

    /// A typed query: one list, best match first, ties to the tabs (listed first).
    fn ranked(&self, query: &str) -> Vec<Section> {
        let tabs = self.tabs.iter().map(|tab| Candidate {
            label: &tab.name,
            alias: tab.group.as_deref(),
        });
        let actions = self.actions.iter().map(|action| Candidate {
            label: &action.label,
            alias: Some(short_name(action.action)),
        });
        let candidates: Vec<Candidate<'_>> = tabs.chain(actions).collect();
        let rows: Vec<Shown> = rank_matches(query, &candidates)
            .into_iter()
            .map(|(index, FuzzyMatch { positions, .. })| Shown {
                row: self.row_ref(index),
                positions,
            })
            .collect();
        vec![Section { title: None, rows }]
    }

    /// The row at `index` of the tabs followed by the actions.
    fn row_ref(&self, index: usize) -> RowRef {
        match index.checked_sub(self.tabs.len()) {
            Some(action) => RowRef::Action(action),
            None => RowRef::Tab(index),
        }
    }

    fn shown(&self, ix: IndexPath) -> Option<&Shown> {
        self.sections.get(ix.section)?.rows.get(ix.row)
    }

    fn choice(&self, ix: IndexPath) -> Option<PaletteChoice> {
        Some(match self.shown(ix)?.row {
            RowRef::Tab(index) => PaletteChoice::Tab(self.tabs.get(index)?.id),
            RowRef::Action(index) => PaletteChoice::Action(self.actions.get(index)?.action),
        })
    }

    /// Every row shown, in order.
    fn row_paths(&self) -> Vec<IndexPath> {
        self.sections
            .iter()
            .enumerate()
            .flat_map(|(section, shown)| {
                (0..shown.rows.len()).map(move |row| IndexPath::new(row).section(section))
            })
            .collect()
    }

    fn first_row(&self) -> Option<IndexPath> {
        self.row_paths().first().copied()
    }

    /// The row a page before or after `from` (the first row when none is selected).
    fn page_from(&self, from: Option<IndexPath>, direction: PageDirection) -> Option<IndexPath> {
        let paths = self.row_paths();
        let last = paths.len().checked_sub(1)?;
        let at = from
            .and_then(|from| paths.iter().position(|path| path.eq_row(from)))
            .unwrap_or(0);
        let target = match direction {
            PageDirection::Down => (at + PAGE_ROWS).min(last),
            PageDirection::Up => at.saturating_sub(PAGE_ROWS),
        };
        paths.get(target).copied()
    }

    fn tab_item(&self, index: usize, positions: &[usize], cx: &App) -> Option<ListItem> {
        let tab = self.tabs.get(index)?;
        let muted = cx.theme().muted_foreground;
        let attention = tab.needs_attention.then(|| {
            Icon::new(IconName::TriangleAlert)
                .xsmall()
                .text_color(attention_color(cx))
        });
        let group = tab
            .group
            .clone()
            .map(|group| div().flex_none().text_xs().text_color(muted).child(group));
        let line = div()
            .flex()
            .items_center()
            .gap_2()
            .min_w_0()
            .child(sidebar::kind_icon(tab.kind).small())
            .children(attention)
            .child(
                div()
                    .min_w_0()
                    .truncate()
                    .child(highlighted(&tab.name, positions, cx)),
            )
            .children(group);
        Some(
            ListItem::new(SharedString::from(format!("palette-tab-{}", tab.id)))
                .accessibility_label(tab.name.clone())
                .check_icon(IconName::Check)
                .confirmed(tab.selected)
                .child(line),
        )
    }

    fn action_item(&self, index: usize, positions: &[usize], cx: &App) -> Option<ListItem> {
        let action = self.actions.get(index)?;
        let line = div()
            .flex()
            .items_center()
            .justify_between()
            .gap_3()
            .w_full()
            .child(
                div()
                    .min_w_0()
                    .truncate()
                    .child(highlighted(&action.label, positions, cx)),
            )
            .children(action.keys.clone().map(shortcuts::shortcut_hint));
        Some(
            ListItem::new(SharedString::from(format!("palette-action-{index}")))
                .accessibility_label(action.label.clone())
                .child(line),
        )
    }
}

/// `text` with the characters at `positions` bold in the theme's match color.
fn highlighted(text: &str, positions: &[usize], cx: &App) -> StyledText {
    let style = HighlightStyle {
        color: Some(cx.theme().list_highlight_foreground),
        font_weight: Some(FontWeight::BOLD),
        ..HighlightStyle::default()
    };
    let ranges = highlight_ranges(text, positions);
    StyledText::new(SharedString::from(text.to_owned()))
        .with_highlights(ranges.into_iter().map(|range| (range, style)))
}

impl ListDelegate for PaletteDelegate {
    type Item = ListItem;

    /// Show the rows for `query`, then select the first one (the list selects the first row
    /// of the first section, which may hold none).
    fn perform_search(
        &mut self,
        query: &str,
        window: &mut Window,
        cx: &mut Context<ListState<Self>>,
    ) -> Task<()> {
        self.sections = if query.trim().is_empty() {
            self.all_sections()
        } else {
            self.ranked(query)
        };
        cx.spawn_in(window, async move |list, cx| {
            let _ = list.update_in(cx, |list, window, cx| {
                let first = list.delegate().first_row();
                list.set_selected_index(first, window, cx);
                cx.notify();
            });
        })
    }

    fn sections_count(&self, _: &App) -> usize {
        self.sections.len()
    }

    fn items_count(&self, section: usize, _: &App) -> usize {
        self.sections
            .get(section)
            .map_or(0, |shown| shown.rows.len())
    }

    fn render_item(
        &mut self,
        ix: IndexPath,
        _: &mut Window,
        cx: &mut Context<ListState<Self>>,
    ) -> Option<ListItem> {
        let shown = self.shown(ix)?;
        match shown.row {
            RowRef::Tab(index) => self.tab_item(index, &shown.positions, cx),
            RowRef::Action(index) => self.action_item(index, &shown.positions, cx),
        }
    }

    fn render_section_header(
        &mut self,
        section: usize,
        _: &mut Window,
        cx: &mut Context<ListState<Self>>,
    ) -> Option<impl IntoElement> {
        let title = self.sections.get(section)?.title?;
        Some(
            div()
                .px_2()
                .pt_2()
                .pb_1()
                .text_xs()
                .text_color(cx.theme().muted_foreground)
                .child(title),
        )
    }

    fn render_empty(
        &mut self,
        _: &mut Window,
        cx: &mut Context<ListState<Self>>,
    ) -> impl IntoElement {
        div()
            .p_2()
            .text_sm()
            .text_color(cx.theme().muted_foreground)
            .child(NO_MATCH)
    }

    fn set_selected_index(
        &mut self,
        _: Option<IndexPath>,
        _: &mut Window,
        _: &mut Context<ListState<Self>>,
    ) {
    }
}

#[cfg(test)]
mod tests;
