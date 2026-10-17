use gpui_kit::component::button::{Button, ButtonVariants};
use gpui_kit::component::tooltip::Tooltip;
use gpui_kit::component::{ActiveTheme, Icon, IconName, Sizable};
use gpui_kit::prelude::FluentBuilder;
use gpui_kit::{
    App, ClickEvent, Hsla, InteractiveElement, IntoElement, ParentElement, SharedString,
    StatefulInteractiveElement, Styled, Window, div, px, rgb,
};
use term_core::SessionKind;

pub const SIDEBAR_WIDTH: f32 = 220.;

/// The dot next to a tab's name.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TabStatus {
    /// Claude Code is working on a prompt.
    Working,
    /// Claude Code waits for a permission decision or an answer.
    NeedsAttention,
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
            Self::Idle => rgb(0x3fb950),
            Self::Active => rgb(0xc9d1d9),
            Self::Quiet | Self::Exited => rgb(0x545d68),
        }
        .into()
    }

    pub fn label(self) -> &'static str {
        match self {
            Self::Working => "Working",
            Self::NeedsAttention => "Needs attention",
            Self::Idle => "Idle",
            Self::Active => "Recent output",
            Self::Quiet => "No recent output",
            Self::Exited => "Exited",
        }
    }
}

pub fn kind_icon(kind: SessionKind) -> IconName {
    match kind {
        SessionKind::Shell => IconName::SquareTerminal,
        SessionKind::ClaudeCode => IconName::Bot,
        SessionKind::CopilotCli => IconName::Github,
    }
}

/// What a sidebar row shows for one tab.
pub struct TabRow {
    pub index: usize,
    pub kind: SessionKind,
    pub name: SharedString,
    pub status: TabStatus,
    pub selected: bool,
}

/// A dot; exited sessions get a hollow one.
fn status_dot(status: TabStatus) -> impl IntoElement {
    let dot = div().size(px(8.)).flex_none().rounded_full();
    match status {
        TabStatus::Exited => dot.border_1().border_color(status.color()),
        _ => dot.bg(status.color()),
    }
}

pub fn tab_row(
    row: TabRow,
    on_select: impl Fn(&ClickEvent, &mut Window, &mut App) + 'static,
    on_close: impl Fn(&ClickEvent, &mut Window, &mut App) + 'static,
    cx: &App,
) -> impl IntoElement {
    let theme = cx.theme();
    let group = SharedString::from(format!("tab-{}", row.index));
    let (background, foreground) = if row.selected {
        (theme.sidebar_accent, theme.sidebar_accent_foreground)
    } else {
        (theme.sidebar, theme.sidebar_foreground)
    };
    let shortcut = (row.index < 9).then(|| format!("Ctrl+{}", row.index + 1));
    let tooltip = match shortcut {
        Some(shortcut) => format!("{} · {}", row.status.label(), shortcut),
        None => row.status.label().to_string(),
    };

    div()
        .id(("tab", row.index))
        .group(group.clone())
        .flex()
        .items_center()
        .gap_2()
        .h(px(30.))
        .px_2()
        .rounded_md()
        .bg(background)
        .text_color(foreground)
        .hover(|style| style.bg(theme.sidebar_accent))
        .on_click(on_select)
        .child(Icon::new(kind_icon(row.kind)).small().flex_none())
        .child(
            div()
                .flex_1()
                .min_w_0()
                .overflow_hidden()
                .text_ellipsis()
                .whitespace_nowrap()
                .text_sm()
                .child(row.name),
        )
        .child(
            div()
                .id(("tab-status", row.index))
                .tooltip(move |window, cx| Tooltip::new(tooltip.clone()).build(window, cx))
                .p_1()
                .child(status_dot(row.status)),
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
