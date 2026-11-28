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
    /// Attach a remote tab whose connection was lost to its session again.
    Reconnect,
    /// Start an exited tab's program again.
    Restart,
    /// Show an idle agent tab as Ready for review again, until it is selected next.
    MarkReady,
    Duplicate,
    TogglePin,
    NewGroup,
    MoveToGroup(GroupId),
    RemoveFromGroup,
    CopyBranch,
    CopyTitle,
    CopyFolder,
    Rename,
    /// Drop the custom name, back to the program's title.
    ResetName,
    Move(Direction),
    Close,
    CloseOthers,
    CloseBelow,
    SetColor(Option<TabColor>),
}

/// What the menu shows about its tab, read when it opens.
pub struct TabMenuState {
    /// A remote tab lost its connection: "Reconnect" comes first.
    pub can_reconnect: bool,
    /// The tab's program exited: "Restart" comes first.
    pub can_restart: bool,
    /// An idle agent tab not Ready for review: "Mark as ready for review" is offered.
    pub can_mark_ready: bool,
    pub pinned: bool,
    /// The tab has a custom name, so "Reset name" is offered.
    pub has_custom_name: bool,
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

impl TabMenuState {
    /// Whether the menu offers `command` to run: shown, and not greyed out.
    pub fn offers(&self, command: TabCommand) -> bool {
        match command {
            TabCommand::Reconnect => self.can_reconnect,
            TabCommand::Restart => self.can_restart,
            TabCommand::MarkReady => self.can_mark_ready,
            TabCommand::MoveToGroup(id) => self.other_groups.iter().any(|(group, _)| *group == id),
            TabCommand::RemoveFromGroup => self.in_group,
            TabCommand::CopyBranch => self.has_branch,
            TabCommand::CopyFolder => self.has_folder,
            TabCommand::ResetName => self.has_custom_name,
            TabCommand::Move(Direction::Up) => self.can_move_up,
            TabCommand::Move(Direction::Down) => self.can_move_down,
            TabCommand::CloseOthers => self.has_others_to_close,
            TabCommand::CloseBelow => self.has_below_to_close,
            TabCommand::Duplicate
            | TabCommand::TogglePin
            | TabCommand::NewGroup
            | TabCommand::CopyTitle
            | TabCommand::Rename
            | TabCommand::Close
            | TabCommand::SetColor(_) => true,
        }
    }

    /// The commands the menu offers, in its order (the "Move to group" submenu in place), the
    /// color swatches left out.
    pub fn offered(&self) -> Vec<TabCommand> {
        let move_to_groups = self
            .other_groups
            .iter()
            .map(|(id, _)| TabCommand::MoveToGroup(*id));
        [
            TabCommand::Reconnect,
            TabCommand::Restart,
            TabCommand::MarkReady,
            TabCommand::Duplicate,
            TabCommand::TogglePin,
            TabCommand::NewGroup,
        ]
        .into_iter()
        .chain(move_to_groups)
        .chain([
            TabCommand::RemoveFromGroup,
            TabCommand::CopyBranch,
            TabCommand::CopyTitle,
            TabCommand::CopyFolder,
            TabCommand::Rename,
            TabCommand::ResetName,
            TabCommand::Move(Direction::Up),
            TabCommand::Move(Direction::Down),
            TabCommand::Close,
            TabCommand::CloseOthers,
            TabCommand::CloseBelow,
        ])
        .filter(|command| self.offers(*command))
        .collect()
    }

    /// The command's label in the menu; a group's name in the "Move to group" submenu.
    pub fn label(&self, command: TabCommand) -> SharedString {
        match command {
            TabCommand::Reconnect => "Reconnect".into(),
            TabCommand::Restart => "Restart".into(),
            TabCommand::MarkReady => "Mark as ready for review".into(),
            TabCommand::Duplicate => "Duplicate tab".into(),
            TabCommand::TogglePin if self.pinned => "Unpin tab".into(),
            TabCommand::TogglePin => "Pin tab".into(),
            TabCommand::NewGroup => "New group with tab".into(),
            TabCommand::MoveToGroup(id) => self.group_name(id),
            TabCommand::RemoveFromGroup => "Remove from group".into(),
            TabCommand::CopyBranch => "Copy branch name".into(),
            TabCommand::CopyTitle => "Copy pane title".into(),
            TabCommand::CopyFolder => "Copy working directory".into(),
            TabCommand::Rename => "Rename tab".into(),
            TabCommand::ResetName => "Reset name".into(),
            TabCommand::Move(Direction::Up) => "Move tab up".into(),
            TabCommand::Move(Direction::Down) => "Move tab down".into(),
            TabCommand::Close => "Close tab".into(),
            TabCommand::CloseOthers => "Close other tabs".into(),
            TabCommand::CloseBelow => "Close tabs below".into(),
            TabCommand::SetColor(color) => color.map_or("No color", TabColor::label).into(),
        }
    }

    fn group_name(&self, id: GroupId) -> SharedString {
        self.other_groups
            .iter()
            .find(|(group, _)| *group == id)
            .map(|(_, name)| name.clone())
            .unwrap_or_default()
    }
}

pub fn build(
    menu: PopupMenu,
    state: TabMenuState,
    run: RunCommand,
    window: &mut Window,
    cx: &mut Context<PopupMenu>,
) -> PopupMenu {
    // Greyed out when not offered, except the ones shown only then (`when`).
    let item = |command: TabCommand| {
        command_item(
            state.label(command),
            command,
            state.offers(command),
            run.clone(),
        )
    };
    let with_leading_item = |menu: PopupMenu, command: TabCommand| {
        menu.when(state.offers(command), |menu| {
            menu.item(item(command)).separator()
        })
    };
    let remove_from_group = (
        state.label(TabCommand::RemoveFromGroup),
        state.offers(TabCommand::RemoveFromGroup),
    );
    let groups_menu = group_choices(state.other_groups.clone(), remove_from_group, run.clone());
    let pick_color = run.clone();
    let duplicate = run.clone();
    let first = [
        TabCommand::Reconnect,
        TabCommand::Restart,
        TabCommand::MarkReady,
    ];
    first
        .into_iter()
        .fold(
            menu.min_w(px(crate::ui_dimensions::MENU_WIDTH)),
            with_leading_item,
        )
        .item(
            PopupMenuItem::element(|_, cx| {
                shortcuts::menu_label_with_hint("duplicate-tab", "Duplicate tab", &DuplicateTab, cx)
            })
            .on_click(move |_, window, cx| duplicate(TabCommand::Duplicate, window, cx)),
        )
        .item(item(TabCommand::TogglePin))
        .item(item(TabCommand::NewGroup))
        .submenu("Move to group", window, cx, move |menu, _, _| {
            groups_menu(menu)
        })
        .separator()
        .item(item(TabCommand::CopyBranch))
        .item(item(TabCommand::CopyTitle))
        .item(item(TabCommand::CopyFolder))
        .separator()
        .item(item(TabCommand::Rename))
        .when(state.offers(TabCommand::ResetName), |menu| {
            menu.item(item(TabCommand::ResetName))
        })
        .item(item(TabCommand::Move(Direction::Up)))
        .item(item(TabCommand::Move(Direction::Down)))
        .separator()
        .item(item(TabCommand::Close))
        .item(item(TabCommand::CloseOthers))
        .item(item(TabCommand::CloseBelow))
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
    remove_from_group: (SharedString, bool),
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
        let (label, enabled) = remove_from_group.clone();
        menu.separator().item(command_item(
            label,
            TabCommand::RemoveFromGroup,
            enabled,
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

#[cfg(test)]
mod tests;
