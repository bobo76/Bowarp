use std::rc::Rc;
use std::sync::OnceLock;
use std::time::Duration;

use agent_status::AgentMode;
use gpui_kit::component::button::{Button, ButtonVariants};
use gpui_kit::component::menu::{ContextMenuExt, DropdownMenu, PopupMenu};
use gpui_kit::component::{ActiveTheme, Icon, IconName, Sizable};
use gpui_kit::prelude::FluentBuilder;
use gpui_kit::{
    Animation, AnimationExt, AnyElement, App, ClickEvent, Context, Div, Hsla, InteractiveElement,
    IntoElement, ParentElement, Render, SharedString, StatefulInteractiveElement, Styled,
    Transformation, Window, div, percentage, pulsating_between, px, rgb,
};
use term_core::SessionKind;
use workspace_model::{Attention, LauncherIcon, TabColor, format_clock, format_lap};

use crate::assets::{
    AGENT_SPARK_ICON, BASH_ICON, COPILOT_ICON, DEBIAN_ICON, FISH_ICON, GIT_BRANCH_ICON, GIT_ICON,
    LINUX_ICON, PIN_ICON, POWERSHELL_ICON, PROMPT_ICON, UBUNTU_ICON, ZSH_ICON,
};
use crate::light_theme;
use crate::shortcuts;
use crate::workspace_view::{CloseTab, select_tab_action};

pub const SIDEBAR_WIDTH: f32 = 240.;
/// Tab rows are tall enough for the name, the detail line and a large session icon.
const TAB_ROW_HEIGHT: f32 = 56.;
const GROUP_ROW_HEIGHT: f32 = 40.;
const GROUP_CARD_WIDTH: f32 = 240.;
const TAB_CARD_WIDTH: f32 = 280.;
const ICON_TILE_SIZE: f32 = 32.;
const SESSION_ICON_SIZE: f32 = 18.;
const DETAIL_ICON_SIZE: f32 = 11.;
const KIND_ICON_SIZE: f32 = 12.;
const ATTENTION_ICON_SIZE: f32 = 13.;
const GROUP_CHEVRON_SIZE: f32 = 14.;
const GROUP_COLOR_SIZE: f32 = 8.;
const STATUS_DOT_SIZE: f32 = 7.;
const BADGE_SIZE: f32 = 12.;
const DONE_BADGE_SIZE: f32 = 16.;
const BADGE_OFFSET: f32 = -3.;
const CHECK_MARK_SIZE: f32 = 8.;
const HINT_ROW_HEIGHT: f32 = 28.;
const DRAG_PREVIEW_INSET: f32 = 24.;
const DRAG_PREVIEW_HEIGHT: f32 = 32.;
const COLOR_STRIPE_INSET: f32 = 12.;
const COLOR_STRIPE_WIDTH: f32 = 3.;
const COLOR_STRIPE_OFFSET: f32 = 2.;
const DROP_LINE_HEIGHT: f32 = 2.;
const SECTION_HEADING_HEIGHT: f32 = 24.;
const CLAUDE_ORANGE: u32 = 0xd97757;
/// Added lines in the hover card, as in a diff.
const ADDED_GREEN: u32 = 0x3fb950;
/// One breath of the Running badge, between these opacities. It repaints at most
/// `RUNNING_PULSE_FPS` times a second, only while a pulsing badge is drawn, and only for the
/// first `RUNNING_PULSE_FOR` of a command.
const RUNNING_PULSE: Duration = Duration::from_millis(2000);
const RUNNING_PULSE_OPACITY: (f32, f32) = (0.35, 1.);
const RUNNING_PULSE_FPS: f32 = 10.;
/// How often working Copilot tabs' spinners turn a step, all on one shared tick (the
/// workspace's `SpinnerClock`), only while one is drawn. Each repaint redraws the whole
/// window, so the rate is kept low.
pub const WORKING_SPIN_TICK: Duration = Duration::from_millis(250);
/// The spinner's positions in one turn (90 degrees apart).
pub const WORKING_SPIN_STEPS: u8 = 4;

/// A tab's status, shown as a badge on its icon.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TabStatus {
    /// An agent is working on a prompt.
    Working,
    /// An agent waits for a permission decision or an answer.
    NeedsAttention,
    /// An unread completed turn, not a claim that the agent's work is correct.
    ReadyForReview,
    /// An agent finished and waits for the next prompt.
    Idle,
    /// A command typed at a shell's prompt runs (shell integration).
    Running,
    /// A session without agent status printed output recently.
    Active,
    /// A session without agent status has been quiet for a while.
    Quiet,
    /// A quiet Claude Code session while status hooks are off: its silence says nothing.
    Unavailable,
    Exited,
}

impl TabStatus {
    /// The status color for the current theme: the light-mode themes use darker tones, which
    /// stay readable on their tinted sidebars.
    fn color(self, cx: &App) -> Hsla {
        let Some(palette) = light_theme::active_palette(cx) else {
            return self.dark_color();
        };
        light_theme::hsla(palette.status[self.palette_slot()])
    }

    fn dark_color(self) -> Hsla {
        match self {
            Self::Working => rgb(0x4c9aff),
            Self::NeedsAttention => rgb(0xf5a524),
            Self::ReadyForReview | Self::Idle => rgb(0x3fb950),
            Self::Running => rgb(0x2dd4bf),
            Self::Active => rgb(0xc9d1d9),
            Self::Quiet | Self::Unavailable | Self::Exited => rgb(0x545d68),
        }
        .into()
    }

    /// Index into `light_theme::Palette::status`.
    fn palette_slot(self) -> usize {
        match self {
            Self::Working => 0,
            Self::NeedsAttention => 1,
            Self::ReadyForReview | Self::Idle => 2,
            Self::Running => 3,
            Self::Active => 4,
            Self::Quiet | Self::Unavailable | Self::Exited => 5,
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            Self::Working => "Working",
            Self::NeedsAttention => "Needs attention",
            Self::ReadyForReview => "Ready for review",
            Self::Idle => "Idle",
            Self::Running => "Running",
            Self::Active => "Recent output",
            Self::Quiet => "No recent output",
            Self::Unavailable => "Status unavailable",
            Self::Exited => "Exited",
        }
    }

    /// What "jump to the next tab needing attention" makes of this status.
    pub fn attention(self) -> Option<Attention> {
        match self {
            Self::NeedsAttention => Some(Attention::Needed),
            Self::ReadyForReview => Some(Attention::Done),
            Self::Working
            | Self::Idle
            | Self::Running
            | Self::Active
            | Self::Quiet
            | Self::Unavailable
            | Self::Exited => None,
        }
    }
}

/// The color of the Needs attention badge, warning triangle and chip.
pub fn attention_color(cx: &App) -> Hsla {
    TabStatus::NeedsAttention.color(cx)
}

pub fn kind_icon(kind: SessionKind) -> Icon {
    match kind {
        SessionKind::Shell => Icon::new(IconName::SquareTerminal),
        SessionKind::ClaudeCode => Icon::empty()
            .path(AGENT_SPARK_ICON)
            .text_color(rgb(CLAUDE_ORANGE)),
        SessionKind::CopilotCli => Icon::empty().path(COPILOT_ICON),
    }
}

/// Icon of a new-tab menu entry.
pub fn launcher_icon(icon: LauncherIcon) -> Icon {
    let path = match icon {
        LauncherIcon::Claude => return kind_icon(SessionKind::ClaudeCode),
        LauncherIcon::Copilot => return kind_icon(SessionKind::CopilotCli),
        LauncherIcon::Shell | LauncherIcon::CommandPrompt => PROMPT_ICON,
        LauncherIcon::PowerShell => POWERSHELL_ICON,
        LauncherIcon::GitBash => GIT_ICON,
        LauncherIcon::Bash => BASH_ICON,
        LauncherIcon::Zsh => ZSH_ICON,
        LauncherIcon::Fish => FISH_ICON,
        LauncherIcon::Ubuntu => UBUNTU_ICON,
        LauncherIcon::Debian => DEBIAN_ICON,
        LauncherIcon::Linux => LINUX_ICON,
    };
    Icon::empty().path(path)
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
    /// Whether a Running badge pulses rather than stays still.
    pub pulse: RunningPulse,
    /// The spinner left of the name, at its step out of `WORKING_SPIN_STEPS` (0 when still);
    /// see [`working_spinner`].
    pub spinner: Option<u8>,
    pub selected: bool,
    pub pinned: bool,
    /// The tab's own color, else its group's.
    pub color: Option<TabColor>,
    /// Shown in place of the name while the tab is being renamed.
    pub name_editor: Option<AnyElement>,
}

/// Whether an activity indicator (the Running badge, the working spinner) moves.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RunningPulse {
    Animated,
    Still,
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
            .w(px(SIDEBAR_WIDTH - DRAG_PREVIEW_INSET))
            .h(px(DRAG_PREVIEW_HEIGHT))
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

/// The thin line between two sidebar rows, the full width of the sidebar.
pub fn row_divider(cx: &App) -> impl IntoElement {
    div().flex_none().h(px(1.)).bg(cx.theme().sidebar_border)
}

/// Wraps a sidebar row so it can show where a dragged tab would land.
pub fn drop_slot(row: impl IntoElement, marker: Option<DropMarker>, cx: &App) -> Div {
    let theme = cx.theme();
    // The line sits inside the row, against the divider: a line reaching past the row would be
    // painted over by the divider and the next row (children paint in order).
    let line = || {
        div()
            .absolute()
            .left_1()
            .right_1()
            .h(px(DROP_LINE_HEIGHT))
            .rounded_full()
            .bg(theme.drag_border)
    };
    let slot = div().relative().child(row);
    match marker {
        None => slot,
        Some(DropMarker::Above) => slot.child(line().top_0()),
        Some(DropMarker::Below) => slot.child(line().bottom_0()),
        Some(DropMarker::Inside) => slot.child(
            div()
                .absolute()
                .inset_0()
                .border_1()
                .border_color(theme.drag_border)
                .bg(theme.drop_target),
        ),
    }
}

/// A hovered row's background: weaker than the selected row's accent so the two stay apart.
/// Opaque, because the badge ring copies it.
pub fn hover_background(cx: &App) -> Hsla {
    let theme = cx.theme();
    theme.sidebar.blend(theme.sidebar_accent.opacity(0.5))
}

/// The row's background, which the badge ring copies so the badge reads as cut out of the
/// icon: `normal`, or `hovered` while the row (group `row_group`) is hovered.
struct RowBackground {
    normal: Hsla,
    hovered: Hsla,
    row_group: SharedString,
}

/// The Running badge's dot, pulsing or still.
fn running_dot(color: Hsla, pulse: RunningPulse, index: usize) -> AnyElement {
    let dot = div().size_full().rounded_full().bg(color);
    if pulse == RunningPulse::Still {
        return dot.into_any_element();
    }
    dot.with_animation(
        ("running-badge", index),
        Animation::new(RUNNING_PULSE)
            .repeat_synced()
            .with_easing(pulsating_between(
                RUNNING_PULSE_OPACITY.0,
                RUNNING_PULSE_OPACITY.1,
            ))
            .with_max_fps(RUNNING_PULSE_FPS),
        |dot, opacity| dot.opacity(opacity),
    )
    .into_any_element()
}

/// Whether a Running badge pulses: animation is allowed (a hardware GPU, see
/// [`pulse_allowed`], the window shown, and the "Activity animations" switch on) and the
/// command is within its first seconds.
pub fn running_pulse(may_animate: bool, pulsing: bool) -> RunningPulse {
    if may_animate && pulsing {
        RunningPulse::Animated
    } else {
        RunningPulse::Still
    }
}

/// The spinner a row shows left of its name: only a working Copilot tab has one (Claude Code
/// animates its own title). `motion` is whether it turns (see [`running_pulse`]).
pub fn working_spinner(
    kind: SessionKind,
    status: TabStatus,
    motion: RunningPulse,
) -> Option<RunningPulse> {
    (kind == SessionKind::CopilotCli && status == TabStatus::Working).then_some(motion)
}

/// The working spinner: a ring with a gap, turned `step` quarter turns.
fn spinner_icon(step: u8, cx: &App) -> impl IntoElement {
    let turn = f32::from(step) / f32::from(WORKING_SPIN_STEPS);
    Icon::new(IconName::LoaderCircle)
        .size(px(ATTENTION_ICON_SIZE))
        .text_color(TabStatus::Working.color(cx))
        .transform(Transformation::rotate(percentage(turn)))
}

/// Whether Running badges may pulse in `window`: not on a software-emulated GPU (a VM, remote
/// X, Xvfb), where every frame costs CPU. `AGENTWS_FORCE_PULSE` set makes debug builds pulse
/// anyway, to check the pulse under Xvfb.
pub fn pulse_allowed(window: &Window) -> bool {
    static FORCE: OnceLock<bool> = OnceLock::new();
    let forced = cfg!(debug_assertions)
        && *FORCE.get_or_init(|| std::env::var_os("AGENTWS_FORCE_PULSE").is_some());
    forced
        || !window
            .gpu_specs()
            .is_some_and(|specs| specs.is_software_emulated)
}

/// The badge on the icon's bottom-right corner. Quiet and Unavailable sessions get none. The
/// Running badge pulses when `pulse` is set; `index` tells the rows' animations apart.
fn status_badge(
    status: TabStatus,
    pulse: RunningPulse,
    index: usize,
    background: &RowBackground,
    cx: &App,
) -> Option<AnyElement> {
    let color = status.color(cx);
    let ring = background.normal;
    let hovered = background.hovered;
    let badge = div()
        .absolute()
        .right(px(BADGE_OFFSET))
        .bottom(px(BADGE_OFFSET))
        .rounded_full()
        .border_2()
        .border_color(ring)
        .group_hover(background.row_group.clone(), |style| {
            style.border_color(hovered)
        });
    let badge = match status {
        TabStatus::Quiet | TabStatus::Unavailable => return None,
        TabStatus::ReadyForReview => badge
            .flex()
            .items_center()
            .justify_center()
            .size(px(DONE_BADGE_SIZE))
            .bg(color)
            .child(
                Icon::new(IconName::Check)
                    .size(px(CHECK_MARK_SIZE))
                    .text_color(ring),
            ),
        // The dot fades over the ring color, so the icon never shows through it.
        TabStatus::Running => badge
            .size(px(BADGE_SIZE))
            .bg(ring)
            .group_hover(background.row_group.clone(), |style| style.bg(hovered))
            .child(running_dot(color, pulse, index)),
        TabStatus::Exited => badge
            .size(px(BADGE_SIZE))
            .bg(ring)
            .group_hover(background.row_group.clone(), |style| style.bg(hovered))
            .child(
                div()
                    .size_full()
                    .rounded_full()
                    .border_1()
                    .border_color(color),
            ),
        _ => badge.size(px(BADGE_SIZE)).bg(color),
    };
    Some(badge.into_any_element())
}

/// The session icon in a rounded square, with the status badge.
fn session_icon(row: &TabRow, background: &RowBackground, cx: &App) -> impl IntoElement + use<> {
    let theme = cx.theme();
    div()
        .relative()
        .flex()
        .flex_none()
        .items_center()
        .justify_center()
        .size(px(ICON_TILE_SIZE))
        .rounded_md()
        .bg(theme.secondary)
        .child(kind_icon(row.kind).size(px(SESSION_ICON_SIZE)))
        .children(status_badge(
            row.status, row.pulse, row.index, background, cx,
        ))
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
        .children(icon.map(|icon| icon.size(px(DETAIL_ICON_SIZE)).text_color(color)))
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

/// The first line: working spinner or attention triangle, name and pin, or the name editor
/// while renaming.
fn name_line(row: &mut TabRow, muted: Hsla, cx: &App) -> impl IntoElement {
    let line = div().flex().items_center().gap_1().min_w_0().text_sm();
    if let Some(editor) = row.name_editor.take() {
        return line.child(editor);
    }
    line.children(row.spinner.map(|step| spinner_icon(step, cx)))
        .when(row.status == TabStatus::NeedsAttention, |line| {
            line.child(
                Icon::new(IconName::TriangleAlert)
                    .size(px(ATTENTION_ICON_SIZE))
                    .text_color(attention_color(cx)),
            )
        })
        .child(single_line(row.name.clone()))
        .when(row.pinned, |line| {
            line.child(
                Icon::empty()
                    .path(PIN_ICON)
                    .size(px(DETAIL_ICON_SIZE))
                    .text_color(muted),
            )
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
    let (background, foreground, hovered) = row_colors(row.selected, cx);
    let muted = theme.muted_foreground;
    let row_background = RowBackground {
        normal: background,
        hovered,
        row_group: group.clone(),
    };

    let context_menu = menu.clone();

    div()
        .id(("tab", row.index))
        .group(group.clone())
        .relative()
        .flex()
        .items_center()
        .gap_3()
        .h(px(TAB_ROW_HEIGHT))
        .pl_3()
        .pr_2()
        .bg(background)
        .text_color(foreground)
        .hover(|style| style.bg(hovered))
        .on_click(on_select)
        .when_some(row.color, |tab, color| {
            tab.child(
                div()
                    .absolute()
                    .left(px(COLOR_STRIPE_OFFSET))
                    .top(px(COLOR_STRIPE_INSET))
                    .bottom(px(COLOR_STRIPE_INSET))
                    .w(px(COLOR_STRIPE_WIDTH))
                    .rounded_full()
                    .bg(tab_color(color)),
            )
        })
        .child(session_icon(&row, &row_background, cx))
        .child(
            div()
                .flex()
                .flex_col()
                .flex_1()
                .min_w_0()
                .child(name_line(&mut row, muted, cx))
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
                        .tooltip(shortcuts::with_shortcut("Close", &CloseTab, cx))
                        .on_click(on_close),
                ),
        )
        .context_menu(move |popup, window, cx| context_menu(popup, window, cx))
}

/// The Ctrl+number shortcut that selects the tab at `index`, if it has one.
pub fn tab_shortcut(index: usize, cx: &App) -> Option<String> {
    shortcuts::shortcut_text(select_tab_action(index)?, cx)
}

/// What the hover card next to a tab row shows.
#[derive(Clone)]
pub struct TabCard {
    pub index: usize,
    pub name: SharedString,
    pub status: Option<TabStatus>,
    pub mode: Option<AgentMode>,
    /// How long the agent has been working on the current turn, shown in the status chip.
    pub working_for: Option<Duration>,
    /// How long the shell's command has run, shown below the chip ("Running for 2:14").
    pub running_for: Option<Duration>,
    /// The command a shell runs, shown in the Running chip.
    pub running_label: Option<SharedString>,
    /// The full folder, with the home folder shortened to `~`.
    pub folder: Option<SharedString>,
    pub branch: Option<SharedString>,
    /// The program's title, which a custom tab name hides in the row.
    pub title: Option<SharedString>,
    pub kind: Option<SessionKind>,
    /// The session kind, or the shell's name.
    pub kind_label: Option<SharedString>,
    /// Lines added in the working tree, once `git diff` has answered.
    pub lines_added: Option<u64>,
}

/// The status as a colored chip: mark and label, with the lap time while working
/// ("Working · 2m 14s") or the command while running ("Running · make test").
fn status_chip(
    status: TabStatus,
    working_for: Option<Duration>,
    running_label: Option<SharedString>,
    cx: &App,
) -> impl IntoElement {
    let color = status.color(cx);
    let mark = status_mark(status, cx);
    div()
        .flex()
        .items_center()
        .gap_1()
        .min_w_0()
        .px_1p5()
        .py_0p5()
        .rounded_sm()
        .bg(color.opacity(0.15))
        .text_xs()
        .text_color(color)
        .child(div().flex_none().child(mark))
        .child(div().flex_none().child(status.label()))
        .children(working_for.map(|lap| format!("· {}", format_lap(lap))))
        .children(running_label.map(|command| single_line(format!("· {command}").into())))
}

pub fn tab_card(card: TabCard, cx: &App) -> impl IntoElement {
    let theme = cx.theme();
    let muted = theme.muted_foreground;
    div()
        .flex()
        .flex_col()
        .gap_1()
        .w(px(TAB_CARD_WIDTH))
        .text_sm()
        .child(
            div()
                .flex()
                .items_center()
                .justify_between()
                .mb_0p5()
                .gap_2()
                .child(single_line(card.name))
                .children(tab_shortcut(card.index, cx).map(|shortcut| {
                    div()
                        .flex_none()
                        .text_xs()
                        .text_color(muted)
                        .child(shortcut)
                })),
        )
        .children(
            card.status
                .map(|status| status_chip(status, card.working_for, card.running_label, cx)),
        )
        .children(card.mode.map(|mode| {
            div()
                .text_xs()
                .text_color(muted)
                .child(format!("Mode: {}", mode.label()))
        }))
        .children(card.running_for.map(|running| {
            div()
                .text_xs()
                .text_color(muted)
                .child(format!("Running for {}", format_clock(running)))
        }))
        // The whole folder, wrapped rather than cut: its end is the part that tells tabs apart.
        .children(card.folder.map(|folder| div().child(folder)))
        .children(
            card.branch
                .map(|branch| detail_line(TabDetail::Branch(branch), theme.foreground)),
        )
        .children(
            card.title
                .map(|title| div().text_color(muted).child(single_line(title))),
        )
        .child(
            div()
                .flex()
                .items_center()
                .gap_1p5()
                .text_xs()
                .text_color(muted)
                .children(
                    card.kind
                        .map(|kind| kind_icon(kind).size(px(KIND_ICON_SIZE))),
                )
                .children(card.kind_label.map(single_line))
                .children(card.lines_added.filter(|&added| added > 0).map(|added| {
                    div()
                        .flex_none()
                        .text_color(rgb(ADDED_GREEN))
                        .child(format!("+{added}"))
                })),
        )
}

/// The Settings row under the tabs, with a "Settings" heading: like a tab row, selected while
/// the settings page shows, with a close button on hover or while selected.
pub fn settings_row(
    selected: bool,
    on_select: impl Fn(&ClickEvent, &mut Window, &mut App) + 'static,
    on_close: impl Fn(&ClickEvent, &mut Window, &mut App) + 'static,
    cx: &App,
) -> impl IntoElement {
    let theme = cx.theme();
    let (background, foreground, hovered) = row_colors(selected, cx);
    let heading = div()
        .flex()
        .items_end()
        .h(px(SECTION_HEADING_HEIGHT))
        .pl_3()
        .pb_1()
        .text_xs()
        .text_color(theme.muted_foreground)
        .child("Settings");
    let row = div()
        .id("settings-row")
        .group("settings-row")
        .flex()
        .items_center()
        .gap_3()
        .h(px(TAB_ROW_HEIGHT))
        .pl_3()
        .pr_2()
        .bg(background)
        .text_color(foreground)
        .hover(|style| style.bg(hovered))
        .on_click(on_select)
        .child(
            div()
                .flex()
                .flex_none()
                .items_center()
                .justify_center()
                .size(px(ICON_TILE_SIZE))
                .rounded_md()
                .bg(theme.secondary)
                .child(Icon::new(IconName::Settings).size(px(SESSION_ICON_SIZE))),
        )
        .child(div().flex_1().min_w_0().text_sm().child("Settings"))
        .child(
            div()
                .when(!selected, |buttons| {
                    buttons
                        .invisible()
                        .group_hover("settings-row", |style| style.visible())
                })
                .child(
                    Button::new("close-settings")
                        .icon(IconName::Close)
                        .ghost()
                        .xsmall()
                        .tooltip(shortcuts::with_shortcut("Close", &CloseTab, cx))
                        .on_click(on_close),
                ),
        );
    div().flex().flex_col().child(heading).child(row)
}

/// Shown when the sidebar search matches no tab.
pub fn no_matches_hint(cx: &App) -> impl IntoElement {
    div()
        .flex()
        .items_center()
        .h(px(HINT_ROW_HEIGHT))
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
    pub running: usize,
    pub ready_for_review: usize,
}

impl GroupSummary {
    pub fn of(statuses: impl IntoIterator<Item = TabStatus>) -> Self {
        let mut summary = Self::default();
        for status in statuses {
            match status {
                TabStatus::NeedsAttention => summary.needs_attention += 1,
                TabStatus::Working => summary.working += 1,
                TabStatus::Running => summary.running += 1,
                TabStatus::ReadyForReview => summary.ready_for_review += 1,
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
            (TabStatus::Running, self.running),
            (TabStatus::ReadyForReview, self.ready_for_review),
        ]
        .into_iter()
        .filter(|(_, count)| *count > 0)
    }
}

pub fn group_card(
    name: SharedString,
    count: usize,
    summary: GroupSummary,
    cx: &App,
) -> impl IntoElement {
    div()
        .flex()
        .flex_col()
        .gap_1()
        .w(px(GROUP_CARD_WIDTH))
        .text_sm()
        .text_color(cx.theme().foreground)
        .child(single_line(name))
        .child(format!(
            "{count} {}",
            if count == 1 { "tab" } else { "tabs" }
        ))
        .child(format!("Working: {}", summary.working))
        .child(format!("Waiting: {}", summary.needs_attention))
        .child(format!(
            "{}: {}",
            TabStatus::ReadyForReview.label(),
            summary.ready_for_review
        ))
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

fn status_mark(status: TabStatus, cx: &App) -> AnyElement {
    let color = status.color(cx);
    match status {
        TabStatus::NeedsAttention => Icon::new(IconName::TriangleAlert)
            .size(px(DETAIL_ICON_SIZE))
            .into_any_element(),
        TabStatus::ReadyForReview => Icon::new(IconName::Check)
            .size(px(DETAIL_ICON_SIZE))
            .into_any_element(),
        // The same dot as the Working and Running badges on a tab icon (not pulsing here).
        _ => div()
            .size(px(STATUS_DOT_SIZE))
            .rounded_full()
            .bg(color)
            .into_any_element(),
    }
}

fn row_colors(selected: bool, cx: &App) -> (Hsla, Hsla, Hsla) {
    let theme = cx.theme();
    if selected {
        return (
            theme.sidebar_accent,
            theme.sidebar_accent_foreground,
            theme.sidebar_accent,
        );
    }
    (
        theme.sidebar,
        theme.sidebar_foreground,
        hover_background(cx),
    )
}

fn summary_chip(status: TabStatus, count: usize, cx: &App) -> impl IntoElement {
    let color = status.color(cx);
    div()
        .flex()
        .items_center()
        .gap_0p5()
        .text_xs()
        .text_color(color)
        .child(status_mark(status, cx))
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
    let (background, hovered) = if row.holds_selected {
        (theme.sidebar_accent, theme.sidebar_accent)
    } else {
        (theme.sidebar, hover_background(cx))
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
        .h(px(GROUP_ROW_HEIGHT))
        .pl_2()
        .pr_2()
        .bg(background)
        .text_sm()
        .text_color(theme.sidebar_foreground)
        .hover(|style| style.bg(hovered))
        .on_click(on_toggle)
        .child(
            Icon::new(chevron)
                .size(px(GROUP_CHEVRON_SIZE))
                .text_color(muted),
        )
        .when_some(row.color, |header, color| {
            header.child(
                div()
                    .flex_none()
                    .size(px(GROUP_COLOR_SIZE))
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
                        .map(|(status, count)| summary_chip(status, count, cx)),
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
        .h(px(HINT_ROW_HEIGHT))
        .pl_3()
        .text_xs()
        .text_color(cx.theme().muted_foreground)
        .child("No tabs. Drag one here.")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn running_badge_pulses_only_with_every_condition_including_the_switch() {
        assert_eq!(running_pulse(true, true), RunningPulse::Animated);
        for (may_animate, pulsing) in [(false, true), (true, false), (false, false)] {
            assert_eq!(running_pulse(may_animate, pulsing), RunningPulse::Still);
        }
    }

    #[test]
    fn only_a_working_copilot_tab_shows_the_spinner_turning_when_allowed() {
        let copilot = SessionKind::CopilotCli;
        assert_eq!(
            working_spinner(copilot, TabStatus::Working, RunningPulse::Animated),
            Some(RunningPulse::Animated)
        );
        assert_eq!(
            working_spinner(copilot, TabStatus::Working, RunningPulse::Still),
            Some(RunningPulse::Still),
            "animations off or not allowed: a still spinner, no timer"
        );
        for status in [
            TabStatus::NeedsAttention,
            TabStatus::ReadyForReview,
            TabStatus::Idle,
            TabStatus::Running,
            TabStatus::Active,
            TabStatus::Quiet,
            TabStatus::Unavailable,
            TabStatus::Exited,
        ] {
            assert_eq!(
                working_spinner(copilot, status, RunningPulse::Animated),
                None,
                "{status:?}"
            );
        }
        for kind in [SessionKind::ClaudeCode, SessionKind::Shell] {
            assert_eq!(
                working_spinner(kind, TabStatus::Working, RunningPulse::Animated),
                None
            );
        }
    }

    #[test]
    fn unread_completion_has_one_label_and_remains_an_attention_target() {
        assert_eq!(TabStatus::ReadyForReview.label(), "Ready for review");
        assert_eq!(TabStatus::ReadyForReview.attention(), Some(Attention::Done));
        assert_eq!(TabStatus::Idle.attention(), None);
    }

    #[test]
    fn summary_counts_the_statuses_worth_showing() {
        let summary = GroupSummary::of([
            TabStatus::Working,
            TabStatus::NeedsAttention,
            TabStatus::Working,
            TabStatus::Idle,
            TabStatus::Running,
            TabStatus::ReadyForReview,
            TabStatus::Quiet,
            TabStatus::Unavailable,
            TabStatus::Active,
        ]);
        assert_eq!(
            summary,
            GroupSummary {
                needs_attention: 1,
                working: 2,
                running: 1,
                ready_for_review: 1,
            }
        );
        let chips: Vec<_> = summary.chips().collect();
        assert_eq!(
            chips,
            vec![
                (TabStatus::NeedsAttention, 1),
                (TabStatus::Working, 2),
                (TabStatus::Running, 1),
                (TabStatus::ReadyForReview, 1),
            ]
        );
        assert_eq!(GroupSummary::of([TabStatus::Idle]).chips().count(), 0);
    }

    #[test]
    fn empty_groups_have_zero_totals() {
        assert_eq!(GroupSummary::of([]), GroupSummary::default());
    }
}
