//! The sidebar's tab list: rows, group headers, menus and drag & drop.

use std::path::Path;
use std::rc::Rc;

use gpui_kit::component::button::{Button, ButtonVariants};
use gpui_kit::component::menu::{DropdownMenu, PopupMenu, PopupMenuItem};
use gpui_kit::component::{ActiveTheme, IconName, Sizable};
use gpui_kit::{
    AnyElement, App, AppContext, ClickEvent, Context, Div, DragMoveEvent, InteractiveElement,
    IntoElement, ParentElement, SharedString, Stateful, StatefulInteractiveElement, Styled,
    WeakEntity, Window, div, px,
};
use workspace_model::{DropTarget, Group, GroupId, Item, Location, display_path};

use super::{RenameTarget, WorkspaceView};
use crate::launcher::{self, Launcher};
use crate::settings_file;
use crate::shortcuts::{self, NEW_TAB_KEY};
use crate::sidebar::{
    self, DraggedTab, DropMarker, GroupRow, GroupSummary, MenuBuilder, SIDEBAR_WIDTH, TabDetail,
    TabRow,
};
use crate::tab_menu;

impl WorkspaceView {
    pub(super) fn render_sidebar(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let workspace = cx.entity().downgrade();
        let new_tab_button = self.new_tab_button(workspace.clone());
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
                Item::Group(group) => match &matches {
                    None => rows.extend(self.render_group(slot, group, None, cx)),
                    Some(matches) => {
                        let tabs: Vec<u64> = group
                            .tabs()
                            .iter()
                            .copied()
                            .filter(|id| matches.contains(id))
                            .collect();
                        if !tabs.is_empty() {
                            rows.extend(self.render_group(slot, group, Some(&tabs), cx));
                        }
                    }
                },
            }
        }
        if matches.is_some() && rows.is_empty() {
            rows.push(sidebar::no_matches_hint(cx).into_any_element());
        }
        rows.extend(self.render_settings_row(cx));
        let attention_button = self.attention_button(cx);
        let theme = cx.theme();
        div()
            .id("sidebar")
            .flex()
            .flex_col()
            .flex_none()
            .w(px(SIDEBAR_WIDTH))
            .h_full()
            .bg(theme.sidebar)
            .border_r_1()
            .border_color(theme.sidebar_border)
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap_1()
                    .h(px(40.))
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
                    // Leaving the list hides the marker; a drop applies the last marker shown.
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
                    // A divider under every row, the last one included.
                    .children(
                        rows.into_iter()
                            .flat_map(|row| [row, sidebar::row_divider(cx).into_any_element()]),
                    )
                    .child(self.render_end_drop_zone(cx)),
            )
    }

    /// Shown while a tab needs attention: the count; clicking jumps to the next one.
    fn attention_button(&self, cx: &mut Context<Self>) -> Option<AnyElement> {
        let count = self.attention_count(cx);
        if count == 0 {
            return None;
        }
        let tooltip = match count {
            1 => "1 tab needs attention: go to it (Ctrl+Shift+A)".to_string(),
            count => format!("{count} tabs need attention: go to the next (Ctrl+Shift+A)"),
        };
        let button = Button::new("next-attention")
            .icon(IconName::TriangleAlert)
            .label(count.to_string())
            .ghost()
            .xsmall()
            .text_color(sidebar::attention_color())
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
        let mut rows = vec![self.render_group_header(slot, group, only.is_some(), cx)];
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
        sidebar::drop_slot(div().h_full(), marker, cx)
            .id("tabs-end")
            .flex_1()
            .min_h(px(32.))
            .on_drag_move(
                cx.listener(|workspace, event: &DragMoveEvent<DraggedTab>, _, cx| {
                    if event.bounds.contains(&event.event.position) {
                        workspace.set_drop_hint(Some(DropTarget::End), cx);
                    }
                }),
            )
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
        let row = TabRow {
            index,
            kind,
            name: name.clone(),
            detail: view
                .location()
                .map(|location| tab_detail(location, self.home.as_deref())),
            status: view.tab_status(),
            pulse: self.pulse_running && view.running_pulses(),
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
        searching: bool,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let workspace = cx.entity().downgrade();
        let id = group.id;
        // The settings page hides the selected tab: no group holds the selection then.
        let active_id = self
            .active_tab()
            .filter(|_| !self.settings.shows_page())
            .map(|tab| tab.id);
        let statuses = group.tabs().iter().filter_map(|&tab| {
            let index = self.tab_index(tab)?;
            Some(self.tabs[index].view.read(cx).tab_status())
        });
        let row = GroupRow {
            slot,
            name: group.name.clone().into(),
            color: group.color,
            collapsed: group.collapsed && !searching,
            tab_count: group.tabs().len(),
            summary: GroupSummary::of(statuses),
            holds_selected: group.collapsed
                && !searching
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
    fn new_tab_button(&self, workspace: WeakEntity<Self>) -> impl IntoElement {
        let launchers = self.launchers.clone();
        Button::new("new-tab")
            .icon(IconName::Plus)
            .ghost()
            .xsmall()
            .tooltip(shortcuts::with_shortcut("New tab", NEW_TAB_KEY))
            .dropdown_menu(move |mut menu, _, cx| {
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
                        PopupMenuItem::element(move |_, _| {
                            shortcuts::menu_label_with_hint(
                                "new-tab-default-shell",
                                label.clone(),
                                NEW_TAB_KEY,
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
                        let label = if launcher.program.is_some() {
                            launcher.label.clone()
                        } else {
                            format!("{} (not installed)", launcher.label)
                        };
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

    fn tab_menu_builder(workspace: WeakEntity<Self>, id: u64) -> MenuBuilder {
        Rc::new(move |menu: PopupMenu, window, cx| {
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
        None => TabDetail::Folder(display_path(&location.dir, home).into()),
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
            dir: home.join("dev/app"),
            head: Some(GitHead::Branch("main".into())),
        };
        let plain = Location {
            dir: home.join(".config"),
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
