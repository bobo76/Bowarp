use gpui_kit::component::button::{Button, ButtonVariants};
use gpui_kit::component::tooltip::Tooltip;
use gpui_kit::component::{ActiveTheme, Icon, IconName, Sizable};
use gpui_kit::prelude::FluentBuilder;
use gpui_kit::{
    AnyElement, App, ClickEvent, Hsla, InteractiveElement, IntoElement, ParentElement,
    SharedString, StatefulInteractiveElement, Styled, Window, div, px, rgb,
};
use term_core::SessionKind;

use crate::assets::{AGENT_SPARK_ICON, GIT_BRANCH_ICON};

pub const SIDEBAR_WIDTH: f32 = 240.;

const CLAUDE_ORANGE: u32 = 0xd97757;

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

pub fn tab_row(
    row: TabRow,
    on_select: impl Fn(&ClickEvent, &mut Window, &mut App) + 'static,
    on_close: impl Fn(&ClickEvent, &mut Window, &mut App) + 'static,
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
    let shortcut = (row.index < 9).then(|| format!("Ctrl+{}", row.index + 1));
    let tooltip = match shortcut {
        Some(shortcut) => format!("{} · {}", row.status.label(), shortcut),
        None => row.status.label().to_string(),
    };
    let needs_attention = row.status == TabStatus::NeedsAttention;
    let muted = theme.muted_foreground;
    let row_background = RowBackground {
        normal: background,
        hovered: theme.sidebar_accent,
        row_group: group.clone(),
    };

    div()
        .id(("tab", row.index))
        .group(group.clone())
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
        .child(
            div()
                .id(("tab-status", row.index))
                .tooltip(move |window, cx| Tooltip::new(tooltip.clone()).build(window, cx))
                .child(session_icon(row.kind, row.status, &row_background, cx)),
        )
        .child(
            div()
                .flex()
                .flex_col()
                .flex_1()
                .min_w_0()
                .child(
                    div()
                        .flex()
                        .items_center()
                        .gap_1()
                        .text_sm()
                        .when(needs_attention, |line| {
                            line.child(
                                Icon::new(IconName::TriangleAlert)
                                    .size(px(13.))
                                    .text_color(TabStatus::NeedsAttention.color()),
                            )
                        })
                        .child(single_line(row.name)),
                )
                .children(row.detail.map(|detail| detail_line(detail, muted))),
        )
        .child(
            // The close button shows on the selected row and on hover.
            div()
                .when(!row.selected, |button| {
                    button
                        .invisible()
                        .group_hover(group, |style| style.visible())
                })
                .child(
                    Button::new(("close-tab", row.index))
                        .icon(IconName::Close)
                        .ghost()
                        .xsmall()
                        .tooltip("Close (Ctrl+Shift+W)")
                        .on_click(on_close),
                ),
        )
}
