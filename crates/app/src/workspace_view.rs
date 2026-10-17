use std::io;
use std::path::{Path, PathBuf};
use std::rc::Rc;

use agent_status::HookMessage;
use futures::StreamExt;
use futures::channel::mpsc::UnboundedReceiver;
use gpui_kit::component::button::{Button, ButtonVariants};
use gpui_kit::component::input::{Escape, Input, InputEvent, InputState};
use gpui_kit::component::menu::{DropdownMenu, PopupMenu, PopupMenuItem};
use gpui_kit::component::notification::Notification;
use gpui_kit::component::{ActiveTheme, IconName, Sizable, TitleBar, WindowExt};
use gpui_kit::prelude::FluentBuilder;
use gpui_kit::{
    AnyElement, App, AppContext, ClipboardItem, Context, DragMoveEvent, Entity, FocusHandle,
    Focusable, InteractiveElement, IntoElement, KeyDownEvent, Keystroke, MouseButton,
    ParentElement, Render, SharedString, StatefulInteractiveElement, Styled, Subscription, Task,
    WeakEntity, Window, div, px,
};
use term_core::LaunchSpec;
use workspace_model::layout::Direction;
use workspace_model::{
    DropTarget, Group, GroupId, Item, Layout, Location, TabColor, TabMeta, display_path,
};

use crate::hooks::ClaudeHooks;
use crate::launcher::Launcher;
use crate::sidebar::{
    self, DraggedTab, DropMarker, GroupRow, GroupSummary, MenuBuilder, SIDEBAR_WIDTH, TabDetail,
    TabRow,
};
use crate::tab_menu::{self, GroupCommand, GroupMenuState, TabCommand, TabMenuState};
use crate::terminal_view::{TabChanged, TerminalView};

const NEW_GROUP_NAME: &str = "New group";

struct Tab {
    /// The session id, which stays with the tab when tabs move.
    id: u64,
    view: Entity<TerminalView>,
    /// The launch before status hooks were attached, for opening another tab like it.
    spec: LaunchSpec,
    meta: TabMeta,
    _changed: Subscription,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum RenameTarget {
    Tab(u64),
    Group(GroupId),
}

/// A tab or group name being edited in the sidebar.
struct Rename {
    target: RenameTarget,
    input: Entity<InputState>,
    _events: Subscription,
}

/// Window content: the title bar above the tab sidebar and the selected terminal.
pub struct WorkspaceView {
    /// In sidebar order (`layout.tabs()`), so indices match Ctrl+1..9.
    tabs: Vec<Tab>,
    layout: Layout,
    active: usize,
    sidebar_visible: bool,
    renaming: Option<Rename>,
    /// Where the tab being dragged would land, from the last drag move over the sidebar.
    drop_hint: Option<DropTarget>,
    /// For shortening folders to `~` in the sidebar.
    home: Option<PathBuf>,
    next_session_id: u64,
    launchers: Rc<[Launcher]>,
    hooks: Option<Rc<ClaudeHooks>>,
    _hook_messages: Task<()>,
}

impl WorkspaceView {
    pub fn new(
        launchers: Rc<[Launcher]>,
        hooks: Option<Rc<ClaudeHooks>>,
        mut hook_messages: UnboundedReceiver<HookMessage>,
        cx: &mut Context<Self>,
    ) -> Self {
        let hook_task = cx.spawn(async move |this, cx| {
            while let Some(message) = hook_messages.next().await {
                if this
                    .update(cx, |workspace, cx| workspace.on_hook_message(message, cx))
                    .is_err()
                {
                    break;
                }
            }
        });
        Self {
            tabs: Vec::new(),
            layout: Layout::default(),
            active: 0,
            sidebar_visible: true,
            renaming: None,
            drop_hint: None,
            home: std::env::home_dir(),
            next_session_id: 1,
            launchers,
            hooks,
            _hook_messages: hook_task,
        }
    }

    /// Start `spec` in a new tab and select it. The tab joins the selected tab's group.
    pub fn open_tab(
        &mut self,
        spec: LaunchSpec,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> io::Result<()> {
        let id = self.next_session_id;
        let mut launch = spec.clone();
        let reports_status = self
            .hooks
            .as_ref()
            .is_some_and(|hooks| hooks.attach(id, &mut launch));
        let view = TerminalView::spawn(id, &launch, reports_status, cx)?;
        self.next_session_id += 1;
        let changed = cx.subscribe(&view, |_, _, _: &TabChanged, cx| cx.notify());
        let group = self
            .tabs
            .get(self.active)
            .and_then(|tab| self.layout.group_of(tab.id))
            .map(|group| group.id);
        self.tabs.push(Tab {
            id,
            view,
            spec,
            meta: TabMeta::default(),
            _changed: changed,
        });
        self.layout.add(id, group);
        self.sync_order(cx);
        if let Some(index) = self.tab_index(id) {
            self.activate(index, window, cx);
        }
        Ok(())
    }

    fn open_tab_or_notify(
        &mut self,
        spec: LaunchSpec,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let program = launch_name(&spec);
        if let Err(err) = self.open_tab(spec, window, cx) {
            log::error!("failed to start {program}: {err}");
            window.push_notification(
                Notification::error(format!("Failed to start {program}: {err}")),
                cx,
            );
        }
    }

    /// Folder for new tabs: the selected tab's current one, else the app's working directory.
    fn new_tab_cwd(&self, cx: &App) -> Option<PathBuf> {
        self.tabs
            .get(self.active)
            .and_then(|tab| {
                tab.view
                    .read(cx)
                    .location()
                    .map(|location| location.dir.clone())
            })
            .or_else(|| std::env::current_dir().ok())
    }

    fn open_launcher(&mut self, launcher: &Launcher, window: &mut Window, cx: &mut Context<Self>) {
        let cwd = self.new_tab_cwd(cx);
        if let Some(spec) = launcher.spec(cwd) {
            self.open_tab_or_notify(spec, window, cx);
        }
    }

    /// Open another tab running what the selected tab runs.
    fn duplicate_active(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(tab) = self.tabs.get(self.active) else {
            return;
        };
        let spec = tab.spec.clone();
        self.open_tab_or_notify(spec, window, cx);
    }

    /// Select a tab, expanding its group if it is collapsed.
    fn activate(&mut self, index: usize, window: &mut Window, cx: &mut Context<Self>) {
        let Some(tab) = self.tabs.get(index) else {
            return;
        };
        let view = tab.view.clone();
        self.layout.reveal(tab.id);
        if let Some(previous) = self.tabs.get(self.active).filter(|_| self.active != index) {
            previous
                .view
                .update(cx, |view, cx| view.set_visible(false, cx));
        }
        self.active = index;
        view.update(cx, |view, cx| view.set_visible(true, cx));
        window.focus(&view.focus_handle(cx), cx);
        cx.notify();
    }

    /// Close a tab, ending its process. Closing the last tab closes the window.
    fn close_tab(&mut self, index: usize, window: &mut Window, cx: &mut Context<Self>) {
        if index >= self.tabs.len() {
            return;
        }
        let tab = self.tabs.remove(index);
        self.layout.remove(tab.id);
        if self.tabs.is_empty() {
            window.remove_window();
            return;
        }
        if index < self.active || self.active == self.tabs.len() {
            self.active -= 1;
        }
        self.activate(self.active, window, cx);
    }

    fn close_tabs(&mut self, ids: Vec<u64>, window: &mut Window, cx: &mut Context<Self>) {
        for id in ids {
            if let Some(index) = self.tab_index(id) {
                self.close_tab(index, window, cx);
            }
        }
    }

    fn tab_index(&self, id: u64) -> Option<usize> {
        self.tabs.iter().position(|tab| tab.id == id)
    }

    /// Put `tabs` back in sidebar order after the layout changed, keeping the same tab
    /// selected.
    fn sync_order(&mut self, cx: &mut Context<Self>) {
        let active_id = self.tabs.get(self.active).map(|tab| tab.id);
        let order = self.layout.tabs();
        self.tabs
            .sort_by_key(|tab| order.iter().position(|&id| id == tab.id));
        if let Some(index) = active_id.and_then(|id| self.tab_index(id)) {
            self.active = index;
        }
        cx.notify();
    }

    /// The name shown in the sidebar: the custom name, else the program's title.
    fn tab_name(tab: &Tab, cx: &App) -> String {
        match &tab.meta.custom_name {
            Some(name) => name.clone(),
            None => tab.view.read(cx).name().to_string(),
        }
    }

    /// The tab's own color, else its group's.
    fn tab_color(&self, tab: &Tab) -> Option<TabColor> {
        tab.meta
            .color
            .or_else(|| self.layout.group_of(tab.id).and_then(|group| group.color))
    }

    fn tab_menu_state(&self, id: u64, cx: &App) -> Option<TabMenuState> {
        let tab = &self.tabs[self.tab_index(id)?];
        let location = tab.view.read(cx).location();
        let own_group = self.layout.group_of(id).map(|group| group.id);
        Some(TabMenuState {
            pinned: self.layout.is_pinned(id),
            other_groups: self
                .layout
                .groups()
                .filter(|group| Some(group.id) != own_group)
                .map(|group| (group.id, group.name.clone().into()))
                .collect(),
            in_group: own_group.is_some(),
            has_branch: location.is_some_and(|location| location.head.is_some()),
            has_folder: location.is_some(),
            can_move_up: self.layout.can_move(id, Direction::Up),
            can_move_down: self.layout.can_move(id, Direction::Down),
            has_others_to_close: !self.layout.others_to_close(id).is_empty(),
            has_below_to_close: !self.layout.below_to_close(id).is_empty(),
            color: tab.meta.color,
        })
    }

    fn run_tab_command(
        &mut self,
        id: u64,
        command: TabCommand,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(index) = self.tab_index(id) else {
            return;
        };
        let location = self.tabs[index].view.read(cx).location().cloned();
        match command {
            TabCommand::TogglePin => {
                self.layout.toggle_pin(id);
                self.sync_order(cx);
            }
            TabCommand::NewGroup => {
                if let Some(group) = self.layout.new_group_with(id, NEW_GROUP_NAME) {
                    self.sync_order(cx);
                    self.start_rename(RenameTarget::Group(group), window, cx);
                }
            }
            TabCommand::MoveToGroup(group) => {
                self.layout.move_to_group(id, group);
                self.sync_order(cx);
            }
            TabCommand::RemoveFromGroup => {
                self.layout.remove_from_group(id);
                self.sync_order(cx);
            }
            TabCommand::CopyBranch => {
                if let Some(head) = location.and_then(|location| location.head) {
                    copy_text(head.name().to_string(), cx);
                }
            }
            TabCommand::CopyTitle => {
                copy_text(self.tabs[index].view.read(cx).name().to_string(), cx)
            }
            TabCommand::CopyFolder => {
                if let Some(location) = location {
                    copy_text(location.dir.display().to_string(), cx);
                }
            }
            TabCommand::Rename => self.start_rename(RenameTarget::Tab(id), window, cx),
            TabCommand::Move(direction) => {
                self.layout.move_tab(id, direction);
                self.sync_order(cx);
            }
            TabCommand::Close => self.close_tab(index, window, cx),
            TabCommand::CloseOthers => {
                let others = self.layout.others_to_close(id);
                self.activate(index, window, cx);
                self.close_tabs(others, window, cx);
            }
            TabCommand::CloseBelow => {
                let below = self.layout.below_to_close(id);
                self.close_tabs(below, window, cx);
            }
            TabCommand::SetColor(color) => {
                self.tabs[index].meta.color = color;
                cx.notify();
            }
        }
    }

    fn group_menu_state(&self, id: GroupId) -> Option<GroupMenuState> {
        let group = self.layout.group(id)?;
        Some(GroupMenuState {
            collapsed: group.collapsed,
            tab_count: group.tabs().len(),
            can_move_up: self.layout.can_move_group(id, Direction::Up),
            can_move_down: self.layout.can_move_group(id, Direction::Down),
            color: group.color,
        })
    }

    fn run_group_command(
        &mut self,
        id: GroupId,
        command: GroupCommand,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(group) = self.layout.group_mut(id) else {
            return;
        };
        match command {
            GroupCommand::Rename => self.start_rename(RenameTarget::Group(id), window, cx),
            GroupCommand::ToggleCollapsed => {
                group.collapsed = !group.collapsed;
                cx.notify();
            }
            GroupCommand::Move(direction) => {
                self.layout.move_group(id, direction);
                self.sync_order(cx);
            }
            GroupCommand::CloseTabs => {
                let tabs = group.tabs().to_vec();
                self.close_tabs(tabs, window, cx);
            }
            GroupCommand::Delete => {
                self.layout.delete_group(id);
                self.sync_order(cx);
            }
            GroupCommand::SetColor(color) => {
                group.color = color;
                cx.notify();
            }
        }
    }

    fn toggle_group(&mut self, id: GroupId, cx: &mut Context<Self>) {
        if let Some(group) = self.layout.group_mut(id) {
            group.collapsed = !group.collapsed;
            cx.notify();
        }
    }

    /// Edit a name in place: Enter or clicking elsewhere keeps it, Escape cancels.
    fn start_rename(&mut self, target: RenameTarget, window: &mut Window, cx: &mut Context<Self>) {
        let name = match target {
            RenameTarget::Tab(id) => self
                .tab_index(id)
                .map(|index| Self::tab_name(&self.tabs[index], cx)),
            RenameTarget::Group(id) => self.layout.group(id).map(|group| group.name.clone()),
        };
        let Some(name) = name else {
            return;
        };
        let input = cx.new(|cx| InputState::new(window, cx).default_value(name));
        input.update(cx, |input, cx| {
            input.select_all(window, cx);
            input.focus(window, cx);
        });
        let events = cx.subscribe_in(&input, window, |workspace, _, event, window, cx| {
            if matches!(event, InputEvent::PressEnter { .. } | InputEvent::Blur) {
                workspace.finish_rename(true, window, cx);
            }
        });
        self.renaming = Some(Rename {
            target,
            input,
            _events: events,
        });
        cx.notify();
    }

    fn finish_rename(&mut self, keep: bool, window: &mut Window, cx: &mut Context<Self>) {
        let Some(rename) = self.renaming.take() else {
            return;
        };
        if keep {
            let name = rename.input.read(cx).value();
            match rename.target {
                RenameTarget::Tab(id) => {
                    if let Some(index) = self.tab_index(id) {
                        self.tabs[index].meta.rename(&name);
                    }
                }
                RenameTarget::Group(id) => {
                    if let Some(group) = self.layout.group_mut(id) {
                        group.rename(&name);
                    }
                }
            }
        }
        if let Some(tab) = self.tabs.get(self.active) {
            window.focus(&tab.view.focus_handle(cx), cx);
        }
        cx.notify();
    }

    /// The name editor for `target`, when it is being renamed.
    fn name_editor(&self, target: RenameTarget, cx: &mut Context<Self>) -> Option<AnyElement> {
        let rename = self
            .renaming
            .as_ref()
            .filter(|rename| rename.target == target)?;
        let editor = div()
            .flex_1()
            .min_w_0()
            // Keep clicks inside the editor from reaching the row, which would move focus.
            .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
            .on_action(cx.listener(|workspace, _: &Escape, window, cx| {
                workspace.finish_rename(false, window, cx)
            }))
            .child(Input::new(&rename.input).xsmall());
        Some(editor.into_any_element())
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

    fn cycle(&mut self, forward: bool, window: &mut Window, cx: &mut Context<Self>) {
        let count = self.tabs.len();
        if count == 0 {
            return;
        }
        let next = if forward {
            (self.active + 1) % count
        } else {
            (self.active + count - 1) % count
        };
        self.activate(next, window, cx);
    }

    fn on_hook_message(&mut self, message: HookMessage, cx: &mut Context<Self>) {
        let Some(tab) = self
            .tabs
            .iter()
            .find(|tab| tab.view.read(cx).id() == message.session)
        else {
            return;
        };
        tab.view.update(cx, |view, cx| {
            view.apply_hook_event(message.event, message.at, cx)
        });
    }

    /// Tab shortcuts run in the capture phase, before the terminal turns keys into input.
    fn on_key_down_capture(
        &mut self,
        event: &KeyDownEvent,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.handle_tab_shortcut(&event.keystroke, window, cx) {
            cx.stop_propagation();
        }
    }

    fn toggle_sidebar(&mut self, cx: &mut Context<Self>) {
        self.sidebar_visible = !self.sidebar_visible;
        cx.notify();
    }

    /// Ctrl+Tab / Ctrl+Shift+Tab cycle tabs, Ctrl+1..9 select one, Ctrl+Shift+T opens another
    /// tab like the selected one, Ctrl+Shift+W closes it, Ctrl+Shift+B shows or hides the
    /// sidebar.
    fn handle_tab_shortcut(
        &mut self,
        keystroke: &Keystroke,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> bool {
        let modifiers = &keystroke.modifiers;
        if !modifiers.control || modifiers.alt || modifiers.platform {
            return false;
        }
        match (keystroke.key.as_str(), modifiers.shift) {
            ("tab", shift) => self.cycle(!shift, window, cx),
            ("t", true) => self.duplicate_active(window, cx),
            ("w", true) => self.close_tab(self.active, window, cx),
            ("b", true) => self.toggle_sidebar(cx),
            (key, false) => match tab_number(key) {
                Some(number) => self.activate(number - 1, window, cx),
                None => return false,
            },
            _ => return false,
        }
        true
    }

    fn render_sidebar(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let workspace = cx.entity().downgrade();
        let new_tab_button = self.new_tab_button(workspace.clone());
        let mut rows = Vec::new();
        for &id in self.layout.pinned() {
            rows.extend(self.render_tab_row(id, false, cx));
        }
        for (slot, item) in self.layout.items().iter().enumerate() {
            match item {
                Item::Tab(id) => rows.extend(self.render_tab_row(*id, false, cx)),
                Item::Group(group) => {
                    rows.push(self.render_group_header(slot, group, cx));
                    if group.collapsed {
                        continue;
                    }
                    if group.tabs().is_empty() {
                        rows.push(self.render_empty_group(group.id, cx));
                    }
                    for &id in group.tabs() {
                        rows.extend(self.render_tab_row(id, true, cx));
                    }
                }
            }
        }
        let end_marker = self.drop_marker(DropTarget::End, cx);
        // The space below the last row: drops there go ungrouped to the end.
        let end_zone = sidebar::drop_slot(div().h_full(), end_marker, cx)
            .id("tabs-end")
            .flex_1()
            .min_h(px(32.))
            .on_drag_move(
                cx.listener(|workspace, event: &DragMoveEvent<DraggedTab>, _, cx| {
                    if event.bounds.contains(&event.event.position) {
                        workspace.set_drop_hint(Some(DropTarget::End), cx);
                    }
                }),
            );
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
                    .justify_between()
                    .h(px(34.))
                    .pl_3()
                    .pr_1()
                    .text_xs()
                    .text_color(theme.muted_foreground)
                    .child("SESSIONS")
                    .child(new_tab_button),
            )
            .child(
                div()
                    .id("tabs")
                    .flex()
                    .flex_col()
                    .gap_0p5()
                    .px_1()
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
                    .children(rows)
                    .child(end_zone),
            )
    }

    /// A draggable tab row. The upper half of the row drops above it, the lower half below.
    fn render_tab_row(
        &self,
        id: u64,
        indented: bool,
        cx: &mut Context<Self>,
    ) -> Option<AnyElement> {
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
            selected: index == self.active,
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
        let marker = self.drop_marker(DropTarget::Before(id), cx);
        let dragged = DraggedTab { id, kind, name };
        let slot = sidebar::drop_slot(row, marker, cx)
            .id(("tab-slot", id as usize))
            .when(indented, |slot| slot.pl_3())
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
        Some(slot.into_any_element())
    }

    /// A group header: the upper half drops above the group, the lower half into it.
    fn render_group_header(
        &self,
        slot: usize,
        group: &Group,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let workspace = cx.entity().downgrade();
        let id = group.id;
        let active_id = self.tabs.get(self.active).map(|tab| tab.id);
        let statuses = group.tabs().iter().filter_map(|&tab| {
            let index = self.tab_index(tab)?;
            Some(self.tabs[index].view.read(cx).tab_status())
        });
        let row = GroupRow {
            slot,
            name: group.name.clone().into(),
            color: group.color,
            collapsed: group.collapsed,
            tab_count: group.tabs().len(),
            summary: GroupSummary::of(statuses),
            holds_selected: group.collapsed
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

    fn sidebar_toggle(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let (icon, tooltip) = if self.sidebar_visible {
            (IconName::PanelLeftClose, "Hide sidebar (Ctrl+Shift+B)")
        } else {
            (IconName::PanelLeftOpen, "Show sidebar (Ctrl+Shift+B)")
        };
        Button::new("toggle-sidebar")
            .icon(icon)
            .ghost()
            .small()
            .tooltip(tooltip)
            .on_click(cx.listener(|workspace, _, _, cx| workspace.toggle_sidebar(cx)))
    }

    fn new_tab_button(&self, workspace: WeakEntity<Self>) -> impl IntoElement {
        let launchers = self.launchers.clone();
        Button::new("new-tab")
            .icon(IconName::Plus)
            .ghost()
            .xsmall()
            .tooltip("New tab")
            .dropdown_menu(move |mut menu, _, _| {
                for launcher in launchers.iter() {
                    let available = launcher.program.is_some();
                    let label = if available {
                        launcher.label.clone()
                    } else {
                        format!("{} (not installed)", launcher.label)
                    };
                    let launcher = launcher.clone();
                    let workspace = workspace.clone();
                    menu = menu.item(
                        PopupMenuItem::new(label)
                            .icon(sidebar::kind_icon(launcher.kind))
                            .disabled(!available)
                            .on_click(move |_, window, cx| {
                                let _ = workspace.update(cx, |workspace, cx| {
                                    workspace.open_launcher(&launcher, window, cx)
                                });
                            }),
                    );
                }
                menu
            })
    }
}

/// The branch when the folder is in a repository, else the folder.
fn tab_detail(location: &Location, home: Option<&Path>) -> TabDetail {
    match &location.head {
        Some(head) => TabDetail::Branch(SharedString::from(head.name().to_string())),
        None => TabDetail::Folder(display_path(&location.dir, home).into()),
    }
}

fn copy_text(text: String, cx: &mut App) {
    cx.write_to_clipboard(ClipboardItem::new_string(text));
}

/// The tab number for Ctrl+1..9.
fn tab_number(key: &str) -> Option<usize> {
    key.parse().ok().filter(|number| (1..=9).contains(number))
}

fn launch_name(spec: &LaunchSpec) -> String {
    spec.program.clone().unwrap_or_else(|| "shell".into())
}

impl Focusable for WorkspaceView {
    fn focus_handle(&self, cx: &App) -> FocusHandle {
        match self.tabs.get(self.active) {
            Some(tab) => tab.view.focus_handle(cx),
            None => cx.focus_handle(),
        }
    }
}

impl Render for WorkspaceView {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let active = self.tabs.get(self.active).map(|tab| tab.view.clone());
        let title = active.as_ref().map_or_else(
            || "Agent Workspace".into(),
            |view| view.read(cx).display_title(),
        );
        if window.window_title() != title {
            window.set_window_title(&title);
        }

        div()
            .size_full()
            .flex()
            .flex_col()
            .capture_key_down(cx.listener(Self::on_key_down_capture))
            .child(
                TitleBar::new().child(
                    div()
                        .flex()
                        .items_center()
                        .gap_2()
                        .child(self.sidebar_toggle(cx))
                        .child(div().text_sm().child(title)),
                ),
            )
            .child(
                div()
                    .flex()
                    .flex_1()
                    .min_h_0()
                    .when(self.sidebar_visible, |content| {
                        content.child(self.render_sidebar(cx))
                    })
                    .child(div().flex_1().min_w_0().h_full().children(active)),
            )
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

    #[test]
    fn tab_numbers() {
        assert_eq!(tab_number("1"), Some(1));
        assert_eq!(tab_number("9"), Some(9));
        assert_eq!(tab_number("0"), None);
        assert_eq!(tab_number("a"), None);
    }
}
