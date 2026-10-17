use std::rc::Rc;

use gpui_kit::component::menu::{PopupMenu, PopupMenuItem};
use gpui_kit::component::tooltip::Tooltip;
use gpui_kit::component::{ActiveTheme, Icon, IconName};
use gpui_kit::prelude::FluentBuilder;
use gpui_kit::{
    App, InteractiveElement, IntoElement, ParentElement, StatefulInteractiveElement, Styled,
    Window, div, px,
};
use workspace_model::TabColor;
use workspace_model::order::Direction;

use crate::sidebar::tab_color;

/// What the tab menu (`⋮` button or right-click) does to its tab.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TabCommand {
    TogglePin,
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
    pub has_branch: bool,
    pub has_folder: bool,
    pub can_move_up: bool,
    pub can_move_down: bool,
    pub has_others_to_close: bool,
    pub has_below_to_close: bool,
    pub color: Option<TabColor>,
}

/// Runs a command on the menu's tab.
pub type RunCommand = Rc<dyn Fn(TabCommand, &mut Window, &mut App)>;

pub fn build(menu: PopupMenu, state: TabMenuState, run: RunCommand) -> PopupMenu {
    let item = |label: &'static str, command: TabCommand, enabled: bool| {
        let run = run.clone();
        PopupMenuItem::new(label)
            .disabled(!enabled)
            .on_click(move |_, window, cx| run(command, window, cx))
    };
    let pin_label = if state.pinned { "Unpin tab" } else { "Pin tab" };
    menu.min_w(px(200.))
        .item(item(pin_label, TabCommand::TogglePin, true))
        .separator()
        .item(item(
            "Copy branch",
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
        .item(color_swatches(state.color, run))
}

/// One row of color swatches, "no color" first. A swatch click sets the color; the click then
/// reaches the menu item, which closes the menu.
fn color_swatches(current: Option<TabColor>, run: RunCommand) -> PopupMenuItem {
    PopupMenuItem::element(move |_, cx| {
        let choices = std::iter::once(None).chain(TabColor::ALL.map(Some));
        div().flex().items_center().gap_1p5().py_1().children(
            choices
                .enumerate()
                .map(|(index, color)| swatch(index, color, current, run.clone(), cx)),
        )
    })
}

fn swatch(
    index: usize,
    color: Option<TabColor>,
    current: Option<TabColor>,
    run: RunCommand,
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
        .on_click(move |_, window, cx| run(TabCommand::SetColor(color), window, cx))
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
