use std::rc::Rc;

use gpui_kit::component::menu::{PopupMenu, PopupMenuItem};
use gpui_kit::component::tooltip::Tooltip;
use gpui_kit::component::{ActiveTheme, Icon, IconName};
use gpui_kit::prelude::FluentBuilder;
use gpui_kit::{
    App, Context, InteractiveElement, IntoElement, ParentElement, SharedString,
    StatefulInteractiveElement, Styled, Window, div, px,
};
use workspace_model::GroupId;
use workspace_model::TabColor;
use workspace_model::layout::Direction;

use crate::shortcuts;
use crate::sidebar::tab_color;
use crate::workspace_view::DuplicateTab;

/// What the tab menu (`⋮` button or right-click) does to its tab.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TabCommand {
    Duplicate,
    TogglePin,
    NewGroup,
    MoveToGroup(GroupId),
    RemoveFromGroup,
    CopyBranch,
    CopyTitle,
    CopyFolder,
    Rename,
    Move(Direction),
    Close,
    CloseOthers,
    CloseBelow,
    SetColor(Option<TabColor>),
}

/// What the menu shows about its tab, read when it opens.
pub struct TabMenuState {
    pub pinned: bool,
    /// Groups the tab can move to: every group but its own.
    pub other_groups: Vec<(GroupId, SharedString)>,
    pub in_group: bool,
    pub has_branch: bool,
    pub has_folder: bool,
    pub can_move_up: bool,
    pub can_move_down: bool,
    pub has_others_to_close: bool,
    pub has_below_to_close: bool,
    pub color: Option<TabColor>,
}

/// Runs a menu command.
pub type Run<C> = Rc<dyn Fn(C, &mut Window, &mut App)>;

pub type RunCommand = Run<TabCommand>;

pub fn build(
    menu: PopupMenu,
    state: TabMenuState,
    run: RunCommand,
    window: &mut Window,
    cx: &mut Context<PopupMenu>,
) -> PopupMenu {
    let item = |label: &'static str, command: TabCommand, enabled: bool| {
        command_item(label, command, enabled, run.clone())
    };
    let pin_label = if state.pinned { "Unpin tab" } else { "Pin tab" };
    let groups_menu = group_choices(state.other_groups, state.in_group, run.clone());
    let pick_color = run.clone();
    let duplicate = run.clone();
    menu.min_w(px(crate::ui_dimensions::MENU_WIDTH))
        .item(
            PopupMenuItem::element(|_, cx| {
                shortcuts::menu_label_with_hint("duplicate-tab", "Duplicate tab", &DuplicateTab, cx)
            })
            .on_click(move |_, window, cx| duplicate(TabCommand::Duplicate, window, cx)),
        )
        .item(item(pin_label, TabCommand::TogglePin, true))
        .item(item("New group with tab", TabCommand::NewGroup, true))
        .submenu("Move to group", window, cx, move |menu, _, _| {
            groups_menu(menu)
        })
        .separator()
        .item(item(
            "Copy branch name",
            TabCommand::CopyBranch,
            state.has_branch,
        ))
        .item(item("Copy pane title", TabCommand::CopyTitle, true))
        .item(item(
            "Copy working directory",
            TabCommand::CopyFolder,
            state.has_folder,
        ))
        .separator()
        .item(item("Rename tab", TabCommand::Rename, true))
        .item(item(
            "Move tab up",
            TabCommand::Move(Direction::Up),
            state.can_move_up,
        ))
        .item(item(
            "Move tab down",
            TabCommand::Move(Direction::Down),
            state.can_move_down,
        ))
        .separator()
        .item(item("Close tab", TabCommand::Close, true))
        .item(item(
            "Close other tabs",
            TabCommand::CloseOthers,
            state.has_others_to_close,
        ))
        .item(item(
            "Close tabs below",
            TabCommand::CloseBelow,
            state.has_below_to_close,
        ))
        .separator()
        .item(color_swatches(
            state.color,
            Rc::new(move |color, window, cx| pick_color(TabCommand::SetColor(color), window, cx)),
        ))
}

fn command_item<C: Copy + 'static>(
    label: impl Into<SharedString>,
    command: C,
    enabled: bool,
    run: Run<C>,
) -> PopupMenuItem {
    PopupMenuItem::new(label)
        .disabled(!enabled)
        .on_click(move |_, window, cx| run(command, window, cx))
}

/// The "Move to group" submenu: the other groups, then "Remove from group".
fn group_choices(
    groups: Vec<(GroupId, SharedString)>,
    in_group: bool,
    run: RunCommand,
) -> impl Fn(PopupMenu) -> PopupMenu + 'static {
    move |mut menu| {
        if groups.is_empty() {
            menu = menu.item(PopupMenuItem::new("No other groups").disabled(true));
        }
        for (id, name) in &groups {
            menu = menu.item(command_item(
                name.clone(),
                TabCommand::MoveToGroup(*id),
                true,
                run.clone(),
            ));
        }
        menu.separator().item(command_item(
            "Remove from group",
            TabCommand::RemoveFromGroup,
            in_group,
            run.clone(),
        ))
    }
}

/// What the group menu (`⋮` button or right-click on a group header) does to its group.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum GroupCommand {
    Rename,
    ToggleCollapsed,
    Move(Direction),
    CloseTabs,
    /// Delete the group, keeping its tabs ungrouped.
    Delete,
    SetColor(Option<TabColor>),
}

pub struct GroupMenuState {
    pub collapsed: bool,
    pub tab_count: usize,
    pub can_move_up: bool,
    pub can_move_down: bool,
    pub color: Option<TabColor>,
}

pub type RunGroupCommand = Run<GroupCommand>;

pub fn build_group(menu: PopupMenu, state: GroupMenuState, run: RunGroupCommand) -> PopupMenu {
    let item = |label: &'static str, command: GroupCommand, enabled: bool| {
        command_item(label, command, enabled, run.clone())
    };
    let collapse_label = if state.collapsed {
        "Expand group"
    } else {
        "Collapse group"
    };
    let has_tabs = state.tab_count > 0;
    let delete_label = if has_tabs {
        "Ungroup tabs"
    } else {
        "Delete group"
    };
    let pick_color = run.clone();
    menu.min_w(px(crate::ui_dimensions::MENU_WIDTH))
        .item(item("Rename group", GroupCommand::Rename, true))
        .item(item(collapse_label, GroupCommand::ToggleCollapsed, true))
        .separator()
        .item(item(
            "Move group up",
            GroupCommand::Move(Direction::Up),
            state.can_move_up,
        ))
        .item(item(
            "Move group down",
            GroupCommand::Move(Direction::Down),
            state.can_move_down,
        ))
        .separator()
        .item(item(
            "Close tabs in group",
            GroupCommand::CloseTabs,
            has_tabs,
        ))
        .item(item(delete_label, GroupCommand::Delete, true))
        .separator()
        .item(color_swatches(
            state.color,
            Rc::new(move |color, window, cx| pick_color(GroupCommand::SetColor(color), window, cx)),
        ))
}

type PickColor = Run<Option<TabColor>>;

/// One row of color swatches, "no color" first. A swatch click sets the color; the click then
/// reaches the menu item, which closes the menu.
fn color_swatches(current: Option<TabColor>, pick: PickColor) -> PopupMenuItem {
    PopupMenuItem::element(move |_, cx| {
        let choices = std::iter::once(None).chain(TabColor::ALL.map(Some));
        div().flex().items_center().gap_1p5().py_1().children(
            choices
                .enumerate()
                .map(|(index, color)| swatch(index, color, current, pick.clone(), cx)),
        )
    })
}

fn swatch(
    index: usize,
    color: Option<TabColor>,
    current: Option<TabColor>,
    pick: PickColor,
    cx: &App,
) -> impl IntoElement {
    let theme = cx.theme();
    let label = color.map_or("No color", TabColor::label);
    let ring = if color == current {
        theme.foreground
    } else {
        theme.transparent
    };
    div()
        .id(("tab-color", index))
        .flex()
        .items_center()
        .justify_center()
        .size(px(20.))
        .rounded_full()
        .border_2()
        .border_color(ring)
        .tooltip(move |window, cx| Tooltip::new(label).build(window, cx))
        .on_click(move |_, window, cx| pick(color, window, cx))
        .child(
            div()
                .flex()
                .items_center()
                .justify_center()
                .size(px(14.))
                .rounded_full()
                .map(|dot| match color {
                    Some(color) => dot.bg(tab_color(color)),
                    None => dot.child(
                        Icon::new(IconName::Ban)
                            .size(px(14.))
                            .text_color(theme.muted_foreground),
                    ),
                }),
        )
}
