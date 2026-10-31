//! The sidebar's tab list: rows, group headers, menus and drag & drop.

use std::path::Path;
use std::rc::Rc;

use gpui_kit::component::button::{Button, ButtonVariants};
use gpui_kit::component::hover_card::HoverCard;
use gpui_kit::component::menu::{DropdownMenu, PopupMenu, PopupMenuItem};
use gpui_kit::component::{ActiveTheme, IconName, Sizable};
use gpui_kit::{
    Anchor, AnyElement, App, AppContext, ClickEvent, Context, Div, DragMoveEvent,
    InteractiveElement, IntoElement, ParentElement, SharedString, Stateful,
    StatefulInteractiveElement, Styled, WeakEntity, Window, div, px,
};
use workspace_model::{DropTarget, Group, GroupId, Item, Location, display_path};

use super::{RenameTarget, WorkspaceView};
use crate::launcher::{self, Launcher};
use crate::open_menu::OpenMenu;
use crate::settings_file;
use crate::shortcuts;
use crate::sidebar::{
    self, DraggedTab, DropMarker, GroupRow, GroupSummary, MenuBuilder, RunningPulse, SIDEBAR_WIDTH,
    TabDetail, TabRow,
};
use crate::tab_menu;
use crate::workspace_view::{NewTab, NextAttention};

#[derive(Clone, Copy, PartialEq, Eq)]
enum GroupPresentation {
    Layout,
    SearchResults,
}

impl WorkspaceView {
    pub(super) fn render_sidebar(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let workspace = cx.entity().downgrade();
        let new_tab_button = self.new_tab_button(workspace.clone(), cx);
        let rows = self.sidebar_rows(cx);
        let attention_button = self.attention_button(cx);
        let theme = cx.theme();
        div()
            .id("sidebar")
            .flex()
            .flex_col()
            .flex_none()
            .w(px(SIDEBAR_WIDTH))
            .h_full()
            .border_r_1()
            .border_color(theme.sidebar_border)
            .child(
                div()
                    .debug_selector(|| "sidebar-header".into())
                    .flex()
                    .items_center()
                    .gap_1()
                    .h(px(crate::ui_dimensions::HEADER_HEIGHT))
                    .bg(crate::window_background::panel_background(
                        theme.sidebar,
                        cx,
                    ))
                    .pl_2()
                    .pr_1()
                    .child(self.render_search(cx))
                    .children(attention_button)
                    .child(new_tab_button),
            )
            .child(
                div()
                    .id("tabs")
                    .flex()
                    .flex_col()
                    .flex_1()
                    .overflow_y_scroll()
                    .on_drag_move(cx.listener(
                        |workspace, event: &DragMoveEvent<DraggedTab>, _, cx| {
                            if !event.bounds.contains(&event.event.position) {
                                workspace.set_drop_hint(None, cx);
                            }
                        },
                    ))
                    .on_drop(cx.listener(|workspace, dragged: &DraggedTab, _, cx| {
                        workspace.drop_dragged(dragged.id, cx)
                    }))
                    .children(
                        rows.into_iter()
                            .flat_map(|row| [row, sidebar::row_divider(cx).into_any_element()]),
                    )
                    .child(self.render_end_drop_zone(cx)),
            )
    }

    fn sidebar_rows(&self, cx: &mut Context<Self>) -> Vec<AnyElement> {
        // While searching, only matching tabs show, and groups holding one show expanded.
        let matches = self.search_matches(cx);
        let shown = |id: &u64| matches.as_ref().is_none_or(|matches| matches.contains(id));
        let mut rows = Vec::new();
        for id in self.layout.pinned().iter().filter(|id| shown(id)) {
            rows.extend(
                self.render_tab_row(*id, cx)
                    .map(IntoElement::into_any_element),
            );
        }
        for (slot, item) in self.layout.items().iter().enumerate() {
            match item {
                Item::Tab(id) if shown(id) => rows.extend(
                    self.render_tab_row(*id, cx)
                        .map(IntoElement::into_any_element),
                ),
                Item::Tab(_) => {}
                Item::Group(group) => {
                    rows.extend(self.sidebar_group_rows(slot, group, matches.as_ref(), cx))
                }
            }
        }
        if matches.is_some() && rows.is_empty() {
            rows.push(sidebar::no_matches_hint(cx).into_any_element());
        }
        rows.extend(self.render_settings_row(cx));
        rows
    }

    fn sidebar_group_rows(
        &self,
        slot: usize,
        group: &Group,
        matches: Option<&std::collections::HashSet<u64>>,
        cx: &mut Context<Self>,
    ) -> Vec<AnyElement> {
        let Some(matches) = matches else {
            return self.render_group(slot, group, None, cx);
        };
        let tabs: Vec<_> = group
            .tabs()
            .iter()
            .copied()
            .filter(|id| matches.contains(id))
            .collect();
        if tabs.is_empty() {
            return Vec::new();
        }
        self.render_group(slot, group, Some(&tabs), cx)
    }

    /// Shown while a tab needs attention: the count; clicking jumps to the next one.
    fn attention_button(&self, cx: &mut Context<Self>) -> Option<AnyElement> {
        let count = self.attention_count(cx);
        if count == 0 {
            return None;
        }
        let label = match count {
            1 => "1 tab needs attention: go to it".to_string(),
            count => format!("{count} tabs need attention: go to the next"),
        };
        let tooltip = shortcuts::with_shortcut(&label, &NextAttention, cx);
        let button = Button::new("next-attention")
            .icon(IconName::TriangleAlert)
            .label(count.to_string())
            .ghost()
            .xsmall()
            .text_color(sidebar::attention_color(cx))
            .tooltip(tooltip)
            .on_click(
                cx.listener(|workspace, _, window, cx| workspace.jump_to_attention(window, cx)),
            );
        Some(button.into_any_element())
    }

    /// A group's header, then its indented tabs unless it is collapsed. While searching,
    /// `only` lists the matching tabs, shown even when the group is collapsed.
    fn render_group(
        &self,
        slot: usize,
        group: &Group,
        only: Option<&[u64]>,
        cx: &mut Context<Self>,
    ) -> Vec<AnyElement> {
        let presentation = if only.is_some() {
            GroupPresentation::SearchResults
        } else {
            GroupPresentation::Layout
        };
        let mut rows = vec![self.render_group_header(slot, group, presentation, cx)];
        if group.collapsed && only.is_none() {
            return rows;
        }
        if group.tabs().is_empty() {
            rows.push(self.render_empty_group(group.id, cx));
        }
        for &id in only.unwrap_or(group.tabs()) {
            rows.extend(
                self.render_tab_row(id, cx)
                    .map(|row| row.pl_3().into_any_element()),
            );
        }
        rows
    }

    /// The space below the last row: drops there go ungrouped to the end.
    fn render_end_drop_zone(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let marker = self.drop_marker(DropTarget::End, cx);
        sidebar::drop_slot(
            div()
                .h_full()
                .bg(crate::window_background::panel_background(
                    cx.theme().sidebar,
                    cx,
                )),
            marker,
            cx,
        )
        .id("tabs-end")
        .flex_1()
        .min_h(px(32.))
        .on_drag_move(cx.listener(
            |workspace, event: &DragMoveEvent<DraggedTab>, _, cx| {
                if event.bounds.contains(&event.event.position) {
                    workspace.set_drop_hint(Some(DropTarget::End), cx);
                }
            },
        ))
    }

    /// A draggable tab row. The upper half of the row drops above it, the lower half below.
    fn render_tab_row(&self, id: u64, cx: &mut Context<Self>) -> Option<Stateful<Div>> {
        let workspace = cx.entity().downgrade();
        let index = self.tab_index(id)?;
        let tab = &self.tabs[index];
        let name_editor = self.name_editor(RenameTarget::Tab(id), cx);
        let view = tab.view.read(cx);
        let name = SharedString::from(Self::tab_name(tab, cx));
        let kind = view.kind();
        let may_animate =
            self.pulse_running && self.window_visible && settings_file::activity_animations(cx);
        let status = view.tab_status();
        let row = TabRow {
            index,
            kind,
            name: name.clone(),
            detail: view
                .location()
                .map(|location| tab_detail(location, self.home.as_deref())),
            status,
            pulse: sidebar::running_pulse(may_animate, view.running_pulses()),
            spinner: sidebar::working_spinner(
                kind,
                status,
                tab.meta.custom_name.is_some(),
                sidebar::running_pulse(may_animate, true),
            )
            .map(|motion| match motion {
                RunningPulse::Animated => self.spinner_clock.drawn_step(),
                RunningPulse::Still => 0,
            }),
            selected: index == self.active && !self.settings.shows_page(),
            pinned: self.layout.is_pinned(id),
            color: self.tab_color(tab),
            name_editor,
        };
        let select = workspace.clone();
        let close = workspace.clone();
        let row = sidebar::tab_row(
            row,
            move |_, window, cx| {
                let _ = select.update(cx, |workspace, cx| {
                    if let Some(index) = workspace.tab_index(id) {
                        workspace.activate(index, window, cx);
                    }
                });
            },
            move |_, window, cx| {
                cx.stop_propagation();
                let _ = close.update(cx, |workspace, cx| {
                    if let Some(index) = workspace.tab_index(id) {
                        workspace.close_tab(index, window, cx);
                    }
                });
            },
            Self::tab_menu_builder(workspace, id),
            cx,
        );
        let row = self.with_hover_card(row.into_any_element(), id, cx);
        let marker = self.drop_marker(DropTarget::Before(id), cx);
        let dragged = DraggedTab { id, kind, name };
        let slot = sidebar::drop_slot(row, marker, cx)
            .id(("tab-slot", id as usize))
            .on_drag(dragged, |dragged, _, _, cx| cx.new(|_| dragged.clone()))
            .on_drag_move(cx.listener(
                move |workspace, event: &DragMoveEvent<DraggedTab>, _, cx| {
                    if !event.bounds.contains(&event.event.position) {
                        return;
                    }
                    // No marker on the dragged row itself: dropping there changes nothing.
                    let hint = (event.drag(cx).id != id).then(|| {
                        if event.event.position.y < event.bounds.center().y {
                            DropTarget::Before(id)
                        } else {
                            DropTarget::After(id)
                        }
                    });
                    workspace.set_drop_hint(hint, cx);
                },
            ));
        Some(slot)
    }

    /// A group header: the upper half drops above the group, the lower half into it. While
    /// searching it shows expanded, since its matching tabs are listed below it.
    fn render_group_header(
        &self,
        slot: usize,
        group: &Group,
        presentation: GroupPresentation,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let workspace = cx.entity().downgrade();
        let id = group.id;
        // The settings page hides the selected tab: no group holds the selection then.
        let active_id = self
            .active_tab()
            .filter(|_| !self.settings.shows_page())
            .map(|tab| tab.id);
        let row = GroupRow {
            slot,
            name: group.name.clone().into(),
            color: group.color,
            collapsed: group.collapsed && presentation == GroupPresentation::Layout,
            tab_count: group.tabs().len(),
            summary: self.group_summary(group, cx),
            holds_selected: group.collapsed
                && presentation == GroupPresentation::Layout
                && active_id.is_some_and(|active| group.tabs().contains(&active)),
            name_editor: self.name_editor(RenameTarget::Group(id), cx),
        };
        let toggle = workspace.clone();
        let header = sidebar::group_header(
            row,
            move |_, _, cx| {
                let _ = toggle.update(cx, |workspace, cx| workspace.toggle_group(id, cx));
            },
            Self::group_menu_builder(workspace, id),
            cx,
        );
        let header = header.into_any_element();
        let cards_on = settings_file::settings(cx).group_hover_cards.is_on();
        let header = if !cards_on {
            header
        } else {
            // Held back like the tab cards: hidden, but still wrapping the header.
            let held = self.cards.held || self.cards.group_suppressed == Some(slot);
            let workspace = cx.entity().downgrade();
            HoverCard::new(("group-card", slot))
                .anchor(Anchor::LeftCenter)
                .ml_2()
                .appearance(!held)
                .trigger(header)
                .content(move |_, _, cx| {
                    let card = workspace.upgrade().filter(|_| !held).and_then(|workspace| {
                        let workspace = workspace.read(cx);
                        let group = workspace.layout.group(id)?;
                        Some(
                            sidebar::group_card(
                                group.name.clone().into(),
                                group.tabs().len(),
                                workspace.group_summary(group, cx),
                                cx,
                            )
                            .into_any_element(),
                        )
                    });
                    card.unwrap_or_else(|| div().into_any_element())
                })
                .on_open_change({
                    let workspace = cx.entity().downgrade();
                    move |open, _, cx| {
                        let _ = workspace.update(cx, |workspace, cx| {
                            workspace.group_card_changed(slot, *open, cx)
                        });
                    }
                })
                .into_any_element()
        };
        let marker = self.drop_marker(DropTarget::IntoGroup(id), cx);
        sidebar::drop_slot(header, marker, cx)
            .id(("group-slot", slot))
            .on_drag_move(cx.listener(
                move |workspace, event: &DragMoveEvent<DraggedTab>, _, cx| {
                    if !event.bounds.contains(&event.event.position) {
                        return;
                    }
                    let hint = if event.event.position.y < event.bounds.center().y {
                        DropTarget::BeforeGroup(id)
                    } else {
                        DropTarget::IntoGroup(id)
                    };
                    workspace.set_drop_hint(Some(hint), cx);
                },
            ))
            .into_any_element()
    }

    fn group_summary(&self, group: &Group, cx: &App) -> GroupSummary {
        GroupSummary::of(group.tabs().iter().filter_map(|&id| {
            let index = self.tab_index(id)?;
            Some(self.tabs[index].view.read(cx).tab_status())
        }))
    }

    /// The placeholder under an expanded empty group; drops on it go into the group.
    fn render_empty_group(&self, id: GroupId, cx: &mut Context<Self>) -> AnyElement {
        div()
            .id((
                "empty-group",
                self.layout
                    .groups()
                    .position(|group| group.id == id)
                    .unwrap_or(0),
            ))
            .pl_3()
            .child(sidebar::empty_group_hint(cx))
            .on_drag_move(cx.listener(
                move |workspace, event: &DragMoveEvent<DraggedTab>, _, cx| {
                    if event.bounds.contains(&event.event.position) {
                        workspace.set_drop_hint(Some(DropTarget::IntoGroup(id)), cx);
                    }
                },
            ))
            .into_any_element()
    }

    /// The `+` menu: the default shell (with its shortcut), the other shells, then the agent
    /// CLIs.
    fn new_tab_button(&self, workspace: WeakEntity<Self>, cx: &App) -> impl IntoElement + use<> {
        let launchers = self.launchers.clone();
        Button::new("new-tab")
            .icon(IconName::Plus)
            .ghost()
            .xsmall()
            .tooltip(shortcuts::with_shortcut("New tab", &NewTab, cx))
            .dropdown_menu(move |mut menu, _, cx| {
                OpenMenu::track(&workspace, |workspace| &mut workspace.menu, cx);
                let open = |launcher: &Launcher| {
                    let launcher = launcher.clone();
                    let workspace = workspace.clone();
                    move |_: &ClickEvent, window: &mut Window, cx: &mut App| {
                        let _ = workspace.update(cx, |workspace, cx| {
                            workspace.open_launcher(&launcher, window, cx)
                        });
                    }
                };
                let (default, shells, agents) = launcher::menu_sections(
                    &launchers,
                    settings_file::settings(cx).default_shell(),
                );
                if let Some(default) = default {
                    let label = default.label.clone();
                    menu = menu.item(
                        PopupMenuItem::element(move |_, cx| {
                            shortcuts::menu_label_with_hint(
                                "new-tab-default-shell",
                                label.clone(),
                                &NewTab,
                                cx,
                            )
                        })
                        .icon(sidebar::launcher_icon(default.icon))
                        .on_click(open(default)),
                    );
                }
                for (index, group) in [shells, agents].into_iter().enumerate() {
                    if index > 0 {
                        menu = menu.separator();
                    }
                    for launcher in group {
                        let label = launcher.display_label();
                        menu = menu.item(
                            PopupMenuItem::new(label)
                                .icon(sidebar::launcher_icon(launcher.icon))
                                .disabled(launcher.program.is_none())
                                .on_click(open(launcher)),
                        );
                    }
                }
                menu
            })
    }

    pub(super) fn tab_menu_builder(workspace: WeakEntity<Self>, id: u64) -> MenuBuilder {
        Rc::new(move |menu: PopupMenu, window, cx| {
            OpenMenu::track(&workspace, |workspace| &mut workspace.menu, cx);
            let Some(state) = workspace
                .upgrade()
                .and_then(|workspace| workspace.read(cx).tab_menu_state(id, cx))
            else {
                return menu;
            };
            let workspace = workspace.clone();
            tab_menu::build(
                menu,
                state,
                Rc::new(move |command, window, cx| {
                    let _ = workspace.update(cx, |workspace, cx| {
                        workspace.run_tab_command(id, command, window, cx)
                    });
                }),
                window,
                cx,
            )
        })
    }

    fn group_menu_builder(workspace: WeakEntity<Self>, id: GroupId) -> MenuBuilder {
        Rc::new(move |menu: PopupMenu, _, cx| {
            OpenMenu::track(&workspace, |workspace| &mut workspace.menu, cx);
            let Some(state) = workspace
                .upgrade()
                .and_then(|workspace| workspace.read(cx).group_menu_state(id))
            else {
                return menu;
            };
            let workspace = workspace.clone();
            tab_menu::build_group(
                menu,
                state,
                Rc::new(move |command, window, cx| {
                    let _ = workspace.update(cx, |workspace, cx| {
                        workspace.run_group_command(id, command, window, cx)
                    });
                }),
            )
        })
    }

    fn set_drop_hint(&mut self, hint: Option<DropTarget>, cx: &mut Context<Self>) {
        if self.drop_hint != hint {
            self.drop_hint = hint;
            cx.notify();
        }
    }

    fn drop_dragged(&mut self, id: u64, cx: &mut Context<Self>) {
        if let Some(target) = self.drop_hint.take()
            && self.layout.drop_tab(id, target)
        {
            self.sync_order(cx);
        }
        cx.notify();
    }

    /// The drop marker for the row `target` is shown on, while a tab is dragged.
    fn drop_marker(&self, row: DropTarget, cx: &App) -> Option<DropMarker> {
        if !cx.has_active_drag() {
            return None;
        }
        match (self.drop_hint?, row) {
            (DropTarget::Before(hint), DropTarget::Before(id)) if hint == id => {
                Some(DropMarker::Above)
            }
            (DropTarget::After(hint), DropTarget::Before(id)) if hint == id => {
                Some(DropMarker::Below)
            }
            (DropTarget::BeforeGroup(hint), DropTarget::IntoGroup(id)) if hint == id => {
                Some(DropMarker::Above)
            }
            (DropTarget::IntoGroup(hint), DropTarget::IntoGroup(id)) if hint == id => {
                Some(DropMarker::Inside)
            }
            (DropTarget::End, DropTarget::End) => Some(DropMarker::Above),
            _ => None,
        }
    }
}

/// The branch when the folder is in a repository, else the folder.
fn tab_detail(location: &Location, home: Option<&Path>) -> TabDetail {
    match &location.head {
        Some(head) => TabDetail::Branch(SharedString::from(head.name().to_string())),
        None => TabDetail::Folder(display_path(&location.folder, home).into()),
    }
}

#[cfg(test)]
mod tests {
    use workspace_model::GitHead;

    use super::*;

    #[test]
    fn detail_prefers_the_branch_over_the_folder() {
        let home = Path::new("/home/user");
        let in_repo = Location {
            folder: home.join("dev/app"),
            head: Some(GitHead::Branch("main".into())),
        };
        let plain = Location {
            folder: home.join(".config"),
            head: None,
        };

        assert!(matches!(
            tab_detail(&in_repo, Some(home)),
            TabDetail::Branch(branch) if branch == "main"
        ));
        assert!(matches!(
            tab_detail(&plain, Some(home)),
            TabDetail::Folder(folder) if folder == format!("~{}.config", std::path::MAIN_SEPARATOR)
        ));
    }
}
