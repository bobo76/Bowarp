use std::rc::Rc;

use gpui_kit::component::button::{Button, ButtonVariants};
use gpui_kit::component::menu::{ContextMenuExt, DropdownMenu, PopupMenu};
use gpui_kit::component::{ActiveTheme, Icon, IconName, Sizable};
use gpui_kit::prelude::FluentBuilder;
use gpui_kit::{
    AnyElement, App, ClickEvent, Context, Div, Hsla, InteractiveElement, IntoElement,
    ParentElement, Render, SharedString, StatefulInteractiveElement, Styled, Window, div, px, rgb,
};
use term_core::SessionKind;
use workspace_model::{Attention, TabColor};

use crate::assets::{AGENT_SPARK_ICON, GIT_BRANCH_ICON, PIN_ICON};

pub const SIDEBAR_WIDTH: f32 = 240.;
/// Tabs selectable with Ctrl+1..9.
pub const NUMBERED_TABS: usize = 9;

const CLAUDE_ORANGE: u32 = 0xd97757;
/// Added lines in the hover card, as in a diff.
const ADDED_GREEN: u32 = 0x3fb950;

/// A tab's status, shown as a badge on its icon.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TabStatus {
    /// Claude Code is working on a prompt.
    Working,
    /// Claude Code waits for a permission decision or an answer.
    NeedsAttention,
    /// Claude Code finished a turn while the tab was hidden; shown until the tab is selected.
    Done,
    /// Claude Code finished and waits for the next prompt.
    Idle,
    /// A session without status hooks printed output recently.
    Active,
    /// A session without status hooks has been quiet for a while.
    Quiet,
    Exited,
}

impl TabStatus {
    fn color(self) -> Hsla {
        match self {
            Self::Working => rgb(0x4c9aff),
            Self::NeedsAttention => rgb(0xf5a524),
            Self::Done | Self::Idle => rgb(0x3fb950),
            Self::Active => rgb(0xc9d1d9),
            Self::Quiet | Self::Exited => rgb(0x545d68),
        }
        .into()
    }

    pub fn label(self) -> &'static str {
        match self {
            Self::Working => "Working",
            Self::NeedsAttention => "Needs attention",
            Self::Done => "Done",
            Self::Idle => "Idle",
            Self::Active => "Recent output",
            Self::Quiet => "No recent output",
            Self::Exited => "Exited",
        }
    }

    /// What "jump to the next tab needing attention" makes of this status.
    pub fn attention(self) -> Option<Attention> {
        match self {
            Self::NeedsAttention => Some(Attention::Needed),
            Self::Done => Some(Attention::Done),
            Self::Working | Self::Idle | Self::Active | Self::Quiet | Self::Exited => None,
        }
    }
}

/// The color of the Needs attention badge, warning triangle and chip.
pub fn attention_color() -> Hsla {
    TabStatus::NeedsAttention.color()
}

pub fn kind_icon(kind: SessionKind) -> Icon {
    match kind {
        SessionKind::Shell => Icon::new(IconName::SquareTerminal),
        SessionKind::ClaudeCode => Icon::empty()
            .path(AGENT_SPARK_ICON)
            .text_color(rgb(CLAUDE_ORANGE)),
        SessionKind::CopilotCli => Icon::new(IconName::Github),
    }
}

pub fn tab_color(color: TabColor) -> Hsla {
    match color {
        TabColor::Red => rgb(0xf87171),
        TabColor::Orange => rgb(0xfb923c),
        TabColor::Yellow => rgb(0xfacc15),
        TabColor::Green => rgb(0x4ade80),
        TabColor::Blue => rgb(0x60a5fa),
        TabColor::Pink => rgb(0xf472b6),
        TabColor::Purple => rgb(0xa78bfa),
    }
    .into()
}

/// Builds the tab menu, shown from the row's `⋮` button and on right-click.
pub type MenuBuilder = Rc<dyn Fn(PopupMenu, &mut Window, &mut Context<PopupMenu>) -> PopupMenu>;

/// The second line of a tab row.
pub enum TabDetail {
    /// The folder is in a git repository: its branch (or abbreviated commit).
    Branch(SharedString),
    /// The folder, with the home folder shortened to `~`.
    Folder(SharedString),
}

/// What a sidebar row shows for one tab.
pub struct TabRow {
    pub index: usize,
    pub kind: SessionKind,
    pub name: SharedString,
    pub detail: Option<TabDetail>,
    pub status: TabStatus,
    pub selected: bool,
    pub pinned: bool,
    /// The tab's own color, else its group's.
    pub color: Option<TabColor>,
    /// Shown in place of the name while the tab is being renamed.
    pub name_editor: Option<AnyElement>,
}

/// Where a dragged tab would land, drawn on the row under the mouse.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DropMarker {
    Above,
    Below,
    /// Into the group whose header is under the mouse.
    Inside,
}

/// The value carried while a tab row is dragged; it also renders the row that follows the
/// mouse.
#[derive(Clone)]
pub struct DraggedTab {
    pub id: u64,
    pub kind: SessionKind,
    pub name: SharedString,
}

impl Render for DraggedTab {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = cx.theme();
        div()
            .flex()
            .items_center()
            .gap_2()
            .w(px(SIDEBAR_WIDTH - 24.))
            .h(px(32.))
            .px_2()
            .rounded_md()
            .border_1()
            .border_color(theme.drag_border)
            .bg(theme.sidebar_accent)
            .text_color(theme.sidebar_accent_foreground)
            .text_sm()
            .opacity(0.9)
            .child(kind_icon(self.kind).small())
            .child(single_line(self.name.clone()))
    }
}

/// Wraps a sidebar row so it can show where a dragged tab would land.
pub fn drop_slot(row: impl IntoElement, marker: Option<DropMarker>, cx: &App) -> Div {
    let theme = cx.theme();
    // The line sits in the 2px gap between rows.
    let line = || {
        div()
            .absolute()
            .left_1()
            .right_1()
            .h(px(2.))
            .rounded_full()
            .bg(theme.drag_border)
    };
    let slot = div().relative().child(row);
    match marker {
        None => slot,
        Some(DropMarker::Above) => slot.child(line().top(px(-2.))),
        Some(DropMarker::Below) => slot.child(line().bottom(px(-2.))),
        Some(DropMarker::Inside) => slot.child(
            div()
                .absolute()
                .inset_0()
                .rounded_md()
                .border_1()
                .border_color(theme.drag_border)
                .bg(theme.drop_target),
        ),
    }
}

/// The row's background, which the badge ring copies so the badge reads as cut out of the
/// icon: `normal`, or `hovered` while the row (group `row_group`) is hovered.
struct RowBackground {
    normal: Hsla,
    hovered: Hsla,
    row_group: SharedString,
}

/// The badge on the icon's bottom-right corner. Quiet sessions get none.
fn status_badge(status: TabStatus, background: &RowBackground) -> Option<AnyElement> {
    let ring = background.normal;
    let hovered = background.hovered;
    let badge = div()
        .absolute()
        .right(px(-3.))
        .bottom(px(-3.))
        .rounded_full()
        .border_2()
        .border_color(ring)
        .group_hover(background.row_group.clone(), |style| {
            style.border_color(hovered)
        });
    let badge = match status {
        TabStatus::Quiet => return None,
        TabStatus::Done => badge
            .flex()
            .items_center()
            .justify_center()
            .size(px(14.))
            .bg(status.color())
            .child(Icon::new(IconName::Check).size(px(8.)).text_color(ring)),
        TabStatus::Exited => badge
            .size(px(12.))
            .bg(ring)
            .group_hover(background.row_group.clone(), |style| style.bg(hovered))
            .child(
                div()
                    .size_full()
                    .rounded_full()
                    .border_1()
                    .border_color(status.color()),
            ),
        _ => badge.size(px(12.)).bg(status.color()),
    };
    Some(badge.into_any_element())
}

/// The session icon in a rounded square, with the status badge.
fn session_icon(
    kind: SessionKind,
    status: TabStatus,
    background: &RowBackground,
    cx: &App,
) -> impl IntoElement {
    let theme = cx.theme();
    div()
        .relative()
        .flex()
        .flex_none()
        .items_center()
        .justify_center()
        .size(px(28.))
        .rounded_md()
        .bg(theme.secondary)
        .child(kind_icon(kind).small())
        .children(status_badge(status, background))
}

fn detail_line(detail: TabDetail, color: Hsla) -> impl IntoElement {
    let (icon, text) = match detail {
        TabDetail::Branch(branch) => (Some(Icon::empty().path(GIT_BRANCH_ICON)), branch),
        TabDetail::Folder(folder) => (None, folder),
    };
    div()
        .flex()
        .items_center()
        .gap_1()
        .min_w_0()
        .text_xs()
        .text_color(color)
        .children(icon.map(|icon| icon.size(px(11.)).text_color(color)))
        .child(single_line(text))
}

/// Text cut with an ellipsis when it does not fit.
fn single_line(text: SharedString) -> impl IntoElement {
    div()
        .flex_1()
        .min_w_0()
        .overflow_hidden()
        .text_ellipsis()
        .whitespace_nowrap()
        .child(text)
}

/// The first line: attention triangle, name and pin, or the name editor while renaming.
fn name_line(row: &mut TabRow, muted: Hsla) -> impl IntoElement {
    let line = div().flex().items_center().gap_1().min_w_0().text_sm();
    if let Some(editor) = row.name_editor.take() {
        return line.child(editor);
    }
    line.when(row.status == TabStatus::NeedsAttention, |line| {
        line.child(
            Icon::new(IconName::TriangleAlert)
                .size(px(13.))
                .text_color(TabStatus::NeedsAttention.color()),
        )
    })
    .child(single_line(row.name.clone()))
    .when(row.pinned, |line| {
        line.child(Icon::empty().path(PIN_ICON).size(px(11.)).text_color(muted))
    })
}

pub fn tab_row(
    mut row: TabRow,
    on_select: impl Fn(&ClickEvent, &mut Window, &mut App) + 'static,
    on_close: impl Fn(&ClickEvent, &mut Window, &mut App) + 'static,
    menu: MenuBuilder,
    cx: &App,
) -> impl IntoElement {
    let theme = cx.theme();
    let group = SharedString::from(format!("tab-{}", row.index));
    let (background, foreground, border) = if row.selected {
        (
            theme.sidebar_accent,
            theme.sidebar_accent_foreground,
            theme.sidebar_border,
        )
    } else {
        (theme.sidebar, theme.sidebar_foreground, theme.sidebar)
    };
    let muted = theme.muted_foreground;
    let row_background = RowBackground {
        normal: background,
        hovered: theme.sidebar_accent,
        row_group: group.clone(),
    };

    let context_menu = menu.clone();

    div()
        .id(("tab", row.index))
        .group(group.clone())
        .relative()
        .flex()
        .items_center()
        .gap_2p5()
        .h(px(46.))
        .px_2()
        .rounded_md()
        .border_1()
        .border_color(border)
        .bg(background)
        .text_color(foreground)
        .hover(|style| style.bg(theme.sidebar_accent))
        .on_click(on_select)
        .when_some(row.color, |tab, color| {
            tab.child(
                div()
                    .absolute()
                    .left(px(1.))
                    .top(px(9.))
                    .bottom(px(9.))
                    .w(px(3.))
                    .rounded_full()
                    .bg(tab_color(color)),
            )
        })
        .child(session_icon(row.kind, row.status, &row_background, cx))
        .child(
            div()
                .flex()
                .flex_col()
                .flex_1()
                .min_w_0()
                .child(name_line(&mut row, muted))
                .children(row.detail.take().map(|detail| detail_line(detail, muted))),
        )
        .child(
            // The menu and close buttons show on the selected row and on hover.
            div()
                .flex()
                .items_center()
                .when(!row.selected, |buttons| {
                    buttons
                        .invisible()
                        .group_hover(group, |style| style.visible())
                })
                .child(
                    Button::new(("tab-menu", row.index))
                        .icon(IconName::EllipsisVertical)
                        .ghost()
                        .xsmall()
                        .tooltip("Tab menu")
                        .dropdown_menu(move |popup, window, cx| menu(popup, window, cx)),
                )
                .child(
                    Button::new(("close-tab", row.index))
                        .icon(IconName::Close)
                        .ghost()
                        .xsmall()
                        .tooltip("Close (Ctrl+Shift+W)")
                        .on_click(on_close),
                ),
        )
        .context_menu(move |popup, window, cx| context_menu(popup, window, cx))
}

/// The Ctrl+number shortcut that selects the tab at `index`, if it has one.
pub fn tab_shortcut(index: usize) -> Option<String> {
    (index < NUMBERED_TABS).then(|| format!("Ctrl+{}", index + 1))
}

/// What the hover card next to a tab row shows.
#[derive(Clone)]
pub struct TabCard {
    pub index: usize,
    pub status: TabStatus,
    /// The full folder, with the home folder shortened to `~`.
    pub folder: Option<SharedString>,
    pub branch: Option<SharedString>,
    /// The program's title, which a custom tab name hides in the row.
    pub title: SharedString,
    pub kind: SessionKind,
    /// The session kind, or the shell's name.
    pub kind_label: SharedString,
    /// Lines added in the working tree, once `git diff` has answered.
    pub lines_added: Option<u64>,
}

/// The status as a colored chip: mark and label.
fn status_chip(status: TabStatus) -> impl IntoElement {
    let color = status.color();
    let mark = match status {
        TabStatus::Done => Icon::new(IconName::Check).size(px(11.)).into_any_element(),
        TabStatus::NeedsAttention => Icon::new(IconName::TriangleAlert)
            .size(px(11.))
            .into_any_element(),
        _ => div()
            .size(px(7.))
            .rounded_full()
            .bg(color)
            .into_any_element(),
    };
    div()
        .flex()
        .items_center()
        .gap_1()
        .px_1p5()
        .py_0p5()
        .rounded_sm()
        .bg(color.opacity(0.15))
        .text_xs()
        .text_color(color)
        .child(mark)
        .child(status.label())
}

pub fn tab_card(card: TabCard, cx: &App) -> impl IntoElement {
    let theme = cx.theme();
    let muted = theme.muted_foreground;
    div()
        .flex()
        .flex_col()
        .gap_1()
        .w(px(280.))
        .text_sm()
        .child(
            div()
                .flex()
                .items_center()
                .justify_between()
                .mb_0p5()
                .child(status_chip(card.status))
                .children(
                    tab_shortcut(card.index)
                        .map(|shortcut| div().text_xs().text_color(muted).child(shortcut)),
                ),
        )
        // The whole folder, wrapped rather than cut: its end is the part that tells tabs apart.
        .children(card.folder.map(|folder| div().child(folder)))
        .children(
            card.branch
                .map(|branch| detail_line(TabDetail::Branch(branch), theme.foreground)),
        )
        .child(div().text_color(muted).child(single_line(card.title)))
        .child(
            div()
                .flex()
                .items_center()
                .gap_1p5()
                .text_xs()
                .text_color(muted)
                .child(kind_icon(card.kind).size(px(12.)))
                .child(single_line(card.kind_label))
                .children(card.lines_added.filter(|&added| added > 0).map(|added| {
                    div()
                        .flex_none()
                        .text_color(rgb(ADDED_GREEN))
                        .child(format!("+{added}"))
                })),
        )
}

/// Shown when the sidebar search matches no tab.
pub fn no_matches_hint(cx: &App) -> impl IntoElement {
    div()
        .flex()
        .items_center()
        .h(px(28.))
        .pl_3()
        .text_xs()
        .text_color(cx.theme().muted_foreground)
        .child("No matching tabs.")
}

/// What a collapsed group shows about its tabs.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct GroupSummary {
    pub needs_attention: usize,
    pub working: usize,
    pub done: usize,
}

impl GroupSummary {
    pub fn of(statuses: impl IntoIterator<Item = TabStatus>) -> Self {
        let mut summary = Self::default();
        for status in statuses {
            match status {
                TabStatus::NeedsAttention => summary.needs_attention += 1,
                TabStatus::Working => summary.working += 1,
                TabStatus::Done => summary.done += 1,
                _ => {}
            }
        }
        summary
    }

    /// (status, count) for each status present, most urgent first.
    fn chips(self) -> impl Iterator<Item = (TabStatus, usize)> {
        [
            (TabStatus::NeedsAttention, self.needs_attention),
            (TabStatus::Working, self.working),
            (TabStatus::Done, self.done),
        ]
        .into_iter()
        .filter(|(_, count)| *count > 0)
    }
}

/// What a sidebar row shows for one group.
pub struct GroupRow {
    /// Unique among the sidebar's group rows, for element ids.
    pub slot: usize,
    pub name: SharedString,
    pub color: Option<TabColor>,
    pub collapsed: bool,
    pub tab_count: usize,
    pub summary: GroupSummary,
    /// The group is collapsed and holds the selected tab.
    pub holds_selected: bool,
    /// Shown in place of the name while the group is being renamed.
    pub name_editor: Option<AnyElement>,
}

fn summary_chip(status: TabStatus, count: usize) -> impl IntoElement {
    let color = status.color();
    let mark = match status {
        TabStatus::NeedsAttention => Icon::new(IconName::TriangleAlert)
            .size(px(11.))
            .into_any_element(),
        TabStatus::Done => Icon::new(IconName::Check).size(px(11.)).into_any_element(),
        // The same dot as the Working badge on a tab icon.
        _ => div()
            .size(px(7.))
            .rounded_full()
            .bg(color)
            .into_any_element(),
    };
    div()
        .flex()
        .items_center()
        .gap_0p5()
        .text_xs()
        .text_color(color)
        .child(mark)
        .child(count.to_string())
}

pub fn group_header(
    mut row: GroupRow,
    on_toggle: impl Fn(&ClickEvent, &mut Window, &mut App) + 'static,
    menu: MenuBuilder,
    cx: &App,
) -> impl IntoElement {
    let theme = cx.theme();
    let group = SharedString::from(format!("group-{}", row.slot));
    let muted = theme.muted_foreground;
    let (background, border) = if row.holds_selected {
        (theme.sidebar_accent, theme.sidebar_border)
    } else {
        (theme.sidebar, theme.sidebar)
    };
    let chevron = if row.collapsed {
        IconName::ChevronRight
    } else {
        IconName::ChevronDown
    };
    let count = match row.tab_count {
        1 => "1 tab".to_string(),
        count => format!("{count} tabs"),
    };
    let context_menu = menu.clone();
    let name = match row.name_editor.take() {
        Some(editor) => editor,
        None => div()
            .flex()
            .flex_1()
            .min_w_0()
            .items_center()
            .gap_2()
            .child(single_line(row.name.clone()))
            .child(div().flex_none().text_xs().text_color(muted).child(count))
            .into_any_element(),
    };

    div()
        .id(("group", row.slot))
        .group(group.clone())
        .relative()
        .flex()
        .items_center()
        .gap_1()
        .h(px(30.))
        .pl_1()
        .pr_2()
        .rounded_md()
        .border_1()
        .border_color(border)
        .bg(background)
        .text_sm()
        .text_color(theme.sidebar_foreground)
        .hover(|style| style.bg(theme.sidebar_accent))
        .on_click(on_toggle)
        .child(Icon::new(chevron).size(px(14.)).text_color(muted))
        .when_some(row.color, |header, color| {
            header.child(
                div()
                    .flex_none()
                    .size(px(8.))
                    .rounded_full()
                    .bg(tab_color(color)),
            )
        })
        .child(name)
        .when(row.collapsed, |header| {
            header.child(
                div().flex().items_center().gap_1p5().children(
                    row.summary
                        .chips()
                        .map(|(status, count)| summary_chip(status, count)),
                ),
            )
        })
        .child(
            div()
                .invisible()
                .group_hover(group, |style| style.visible())
                .child(
                    Button::new(("group-menu", row.slot))
                        .icon(IconName::EllipsisVertical)
                        .ghost()
                        .xsmall()
                        .tooltip("Group menu")
                        .dropdown_menu(move |popup, window, cx| menu(popup, window, cx)),
                ),
        )
        .context_menu(move |popup, window, cx| context_menu(popup, window, cx))
}

/// Shown under an expanded group with no tabs.
pub fn empty_group_hint(cx: &App) -> impl IntoElement {
    div()
        .flex()
        .items_center()
        .h(px(28.))
        .pl_3()
        .text_xs()
        .text_color(cx.theme().muted_foreground)
        .child("No tabs. Drag one here.")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn summary_counts_the_statuses_worth_showing() {
        let summary = GroupSummary::of([
            TabStatus::Working,
            TabStatus::NeedsAttention,
            TabStatus::Working,
            TabStatus::Idle,
            TabStatus::Done,
            TabStatus::Quiet,
        ]);
        assert_eq!(
            summary,
            GroupSummary {
                needs_attention: 1,
                working: 2,
                done: 1,
            }
        );
        let chips: Vec<_> = summary.chips().collect();
        assert_eq!(
            chips,
            vec![
                (TabStatus::NeedsAttention, 1),
                (TabStatus::Working, 2),
                (TabStatus::Done, 1),
            ]
        );
        assert_eq!(GroupSummary::of([TabStatus::Idle]).chips().count(), 0);
    }
}
