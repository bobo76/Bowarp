mod divider_drag;
mod settings_pane;
mod sidebar_rows;
mod tab_card;
mod tab_search;

use std::io;
use std::path::PathBuf;
use std::rc::Rc;

use agent_status::HookMessage;
use futures::StreamExt;
use futures::channel::mpsc::UnboundedReceiver;
use gpui_kit::component::button::{Button, ButtonVariants};
use gpui_kit::component::input::{Escape, Input, InputEvent, InputState};
use gpui_kit::component::notification::Notification;
use gpui_kit::component::{ActiveTheme, Icon, IconName, Selectable, Sizable, TitleBar, WindowExt};
use gpui_kit::prelude::FluentBuilder;
use gpui_kit::{
    AnyElement, App, AppContext, ClipboardItem, Context, CursorStyle, DragMoveEvent, Empty, Entity,
    FocusHandle, Focusable, InteractiveElement, IntoElement, KeyBinding, KeyDownEvent, Keystroke,
    MouseButton, MouseDownEvent, ParentElement, Render, StatefulInteractiveElement, Styled,
    Subscription, Task, Window, actions, div, px,
};
use term_core::{LaunchSpec, SessionKind};
use workspace_model::layout::Direction;
use workspace_model::pane_width::{self, DEFAULT_VIEWER_FRACTION, ExplorerWidth, ViewerFraction};
use workspace_model::{
    Attention, DropTarget, GroupId, Layout, TabColor, TabId, TabMeta, TabsState, next_attention,
    waiting_count,
};

use self::divider_drag::DividerDrag;
use self::settings_pane::SettingsPane;
use self::tab_card::TabCards;
use self::tab_search::TabSearch;
use crate::about;
use crate::assets::FILES_ICON;
use crate::file_explorer::{ExplorerEvent, FileExplorer};
use crate::file_viewer::{FilePlace, FileViewer, ViewerEvent};
use crate::hooks::ClaudeHooks;
use crate::launcher::{self, Launcher, launch_name};
use crate::settings_file;
use crate::settings_page;
use crate::shell_integration;
use crate::shortcuts::{
    self, ATTENTION_KEY, CLOSE_TAB_KEY, DUPLICATE_TAB_KEY, EXPLORER_KEY, NEW_TAB_KEY, SEARCH_KEY,
    SIDEBAR_KEY,
};
use crate::sidebar::{self, NUMBERED_TABS, SIDEBAR_WIDTH};
use crate::tab_menu::{GroupCommand, GroupMenuState, TabCommand, TabMenuState};
use crate::tabs_file;
use crate::terminal_view::{AgentExited, OpenFileRequest, TabChanged, TerminalView};
use crate::window_bounds;

const NEW_GROUP_NAME: &str = "New group";

/// The width of the strip on the file viewer's left edge that resizes it.
const DIVIDER_WIDTH: f32 = 6.;

actions!(workspace, [ToggleFullScreen]);

/// F11 toggles full screen. Bound for the whole window, so it works whichever pane has the
/// keyboard: bindings match before the terminal's key handler, so F11 is not sent to the
/// program.
pub fn init(cx: &mut App) {
    cx.bind_keys([KeyBinding::new("f11", ToggleFullScreen, None)]);
    // A global listener, so F11 also works while a dialog or notification has the keyboard
    // (they are not inside the workspace view) or nothing does.
    cx.on_action(|_: &ToggleFullScreen, cx| {
        // X11 without a window manager may never report the focused window; the app has one.
        let Some(window) = cx
            .active_window()
            .or_else(|| cx.windows().into_iter().next())
        else {
            return;
        };
        // The window is being updated while it dispatches the action, so it can't be updated
        // again until the dispatch is over.
        cx.defer(move |cx| {
            let _ = window.update(cx, |_, window, _| window.toggle_fullscreen());
        });
    });
}

struct Tab {
    /// The first session's id, which stays with the tab when tabs move (and when an agent's
    /// tab goes on as a shell, a new session).
    id: u64,
    view: Entity<TerminalView>,
    /// The launch before status hooks were attached, for opening another tab like it.
    spec: LaunchSpec,
    meta: TabMeta,
    /// The file the tab shows beside its terminal; `None` when the viewer is closed.
    viewer: Option<TabViewer>,
    _view_events: [Subscription; 3],
}

struct TabViewer {
    view: Entity<FileViewer>,
    _events: Subscription,
}

/// Dragged when the file viewer's divider is: the drag moves the divider, nothing follows the
/// pointer.
struct ViewerDivider;

impl Render for ViewerDivider {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        Empty
    }
}

/// Dragged when the file explorer's right edge is.
struct ExplorerDivider;

impl Render for ExplorerDivider {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        Empty
    }
}

/// Where `open_tab_in` puts a new tab.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum NewTabPlace {
    /// At the end of the group (`None`: of the top level).
    EndOf(Option<GroupId>),
    /// Right below this tab (duplicates).
    After(TabId),
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
    /// Whether Running badges may pulse in this window ([`sidebar::pulse_allowed`]), read each
    /// render.
    pulse_running: bool,
    explorer: Entity<FileExplorer>,
    explorer_visible: bool,
    /// The file viewer's share of the window width while its divider is dragged; otherwise
    /// the share saved in the settings (`viewer_fraction`), the same in every tab.
    viewer_drag: DividerDrag<f32>,
    /// The same for the explorer's width, in pixels (`explorer_width`).
    explorer_drag: DividerDrag<ExplorerWidth>,
    renaming: Option<Rename>,
    search: TabSearch,
    cards: TabCards,
    /// Where the tab being dragged would land, from the last drag move over the sidebar.
    drop_hint: Option<DropTarget>,
    /// For shortening folders to `~` in the sidebar.
    home: Option<PathBuf>,
    /// Whether this instance reopens and saves the tabs (it holds the instance lock).
    owns_tabs: bool,
    next_session_id: u64,
    launchers: Rc<[Launcher]>,
    hooks: Option<Rc<ClaudeHooks>>,
    settings: SettingsPane,
    _hook_messages: Task<()>,
    _explorer_events: Subscription,
}

impl WorkspaceView {
    pub fn new(
        launchers: Rc<[Launcher]>,
        hooks: Option<Rc<ClaudeHooks>>,
        mut hook_messages: UnboundedReceiver<HookMessage>,
        owns_tabs: bool,
        window: &mut Window,
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
        let agents = launchers
            .iter()
            .filter(|launcher| launcher.kind != SessionKind::Shell)
            .cloned()
            .collect();
        let explorer = cx.new(|cx| FileExplorer::new(agents, window, cx));
        let explorer_events = cx.subscribe_in(&explorer, window, Self::on_explorer_event);
        Self {
            tabs: Vec::new(),
            layout: Layout::default(),
            active: 0,
            sidebar_visible: true,
            pulse_running: false,
            explorer,
            explorer_visible: false,
            viewer_drag: DividerDrag::new(Self::save_viewer_fraction),
            explorer_drag: DividerDrag::new(Self::save_explorer_width),
            renaming: None,
            search: TabSearch::new(window, cx),
            cards: TabCards::default(),
            drop_hint: None,
            home: std::env::home_dir(),
            owns_tabs,
            next_session_id: 1,
            launchers,
            hooks,
            settings: SettingsPane::new(cx),
            _hook_messages: hook_task,
            _explorer_events: explorer_events,
        }
    }

    /// Start `spec` in a new tab and select it. The tab joins the selected tab's group.
    pub fn open_tab(
        &mut self,
        spec: LaunchSpec,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> io::Result<()> {
        let group = self
            .active_tab()
            .and_then(|tab| self.layout.group_of(tab.id))
            .map(|group| group.id);
        self.open_tab_in(spec, NewTabPlace::EndOf(group), window, cx)
    }

    /// Start `spec` in a new tab at `place` and select it.
    fn open_tab_in(
        &mut self,
        spec: LaunchSpec,
        place: NewTabPlace,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> io::Result<()> {
        let id = self.spawn_tab(spec, TabMeta::default(), window, cx)?;
        match place {
            NewTabPlace::EndOf(group) => self.layout.add(id, group),
            NewTabPlace::After(tab) => self.layout.add_after(id, tab),
        }
        self.sync_order(cx);
        if let Some(index) = self.tab_index(id) {
            self.activate(index, window, cx);
        }
        Ok(())
    }

    /// Start `spec` in a new tab that is not in the layout yet. Returns its session id.
    fn spawn_tab(
        &mut self,
        spec: LaunchSpec,
        meta: TabMeta,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> io::Result<u64> {
        let id = self.next_session_id;
        let (view, view_events) = self.start_session(id, &spec, window, cx)?;
        self.tabs.push(Tab {
            id,
            view,
            spec,
            meta,
            viewer: None,
            _view_events: view_events,
        });
        Ok(id)
    }

    /// Start `spec` as a new session for tab `tab_id`, listening to it.
    fn start_session(
        &mut self,
        tab_id: u64,
        spec: &LaunchSpec,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> io::Result<(Entity<TerminalView>, [Subscription; 3])> {
        let session_id = self.next_session_id;
        let mut launch = spec.clone();
        let reports_status = self
            .hooks
            .as_ref()
            .is_some_and(|hooks| hooks.attach(session_id, &mut launch));
        shell_integration::attach(&mut launch);
        let view = TerminalView::spawn(session_id, &launch, reports_status, cx)?;
        self.next_session_id += 1;
        let changed = cx.subscribe(&view, |workspace, _, _: &TabChanged, cx| {
            workspace.sync_explorer(cx);
            cx.notify();
        });
        let exited = cx.subscribe_in(
            &view,
            window,
            move |workspace, _, _: &AgentExited, window, cx| {
                workspace.fall_back_to_shell(tab_id, window, cx);
            },
        );
        let open_file = cx.subscribe_in(
            &view,
            window,
            move |workspace, _, request: &OpenFileRequest, window, cx| {
                workspace.open_file_in(tab_id, request.path.clone(), request.place, window, cx);
            },
        );
        Ok((view, [changed, exited, open_file]))
    }

    /// Go on with the default shell in tab `tab_id` once its agent exited, in the agent's last
    /// folder. The new session starts on an empty screen; the tab keeps its name, color, place
    /// and open file, and is saved as a shell from now on.
    fn fall_back_to_shell(&mut self, tab_id: u64, window: &mut Window, cx: &mut Context<Self>) {
        let Some(index) = self.tab_index(tab_id) else {
            return;
        };
        let Some(shell) =
            launcher::default_shell(&self.launchers, settings_file::settings(cx).default_shell())
                .cloned()
        else {
            return;
        };
        // The child is gone: the last folder it reported, else the one it started in.
        let folder = self.tabs[index].view.read(cx).current_folder();
        let Some(spec) = shell.spec(folder) else {
            return;
        };
        let (view, view_events) = match self.start_session(tab_id, &spec, window, cx) {
            Ok(started) => started,
            Err(err) => {
                log::error!("failed to start {}: {err}", launch_name(&spec));
                window.push_notification(
                    Notification::error(format!("Failed to start {}: {err}", launch_name(&spec))),
                    cx,
                );
                return;
            }
        };
        let had_focus = self.tabs[index]
            .view
            .read(cx)
            .focus_handle
            .is_focused(window);
        let tab = &mut self.tabs[index];
        tab.view = view.clone();
        tab.spec = spec;
        tab._view_events = view_events;
        self.cards.forget(tab_id);
        if index == self.active && !self.settings.shows_page() {
            view.update(cx, |view, cx| view.show(cx));
            if had_focus {
                window.focus(&view.focus_handle(cx), cx);
            }
        } else {
            view.update(cx, |view, _| view.hide());
        }
        self.sync_explorer(cx);
        cx.notify();
    }

    /// Show `path` in tab `tab_id`'s file viewer, opening the viewer when it is closed.
    fn open_file_in(
        &mut self,
        tab_id: u64,
        path: PathBuf,
        place: FilePlace,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(index) = self.tab_index(tab_id) else {
            return;
        };
        let viewer = match &self.tabs[index].viewer {
            Some(viewer) => viewer.view.clone(),
            None => {
                let view = cx.new(|cx| FileViewer::new(window, cx));
                let events =
                    cx.subscribe_in(&view, window, move |workspace, _, event, window, cx| {
                        workspace.on_viewer_event(tab_id, event, window, cx);
                    });
                self.tabs[index].viewer = Some(TabViewer {
                    view: view.clone(),
                    _events: events,
                });
                view
            }
        };
        viewer.update(cx, |viewer, cx| viewer.open_at(path, place, window, cx));
        cx.notify();
    }

    /// The selected tab's file viewer, while it shows a file.
    fn active_viewer(&self, cx: &App) -> Option<Entity<FileViewer>> {
        let viewer = &self.active_tab()?.viewer.as_ref()?.view;
        viewer.read(cx).is_open().then(|| viewer.clone())
    }

    /// Reopen the tabs saved when the window last closed, each in its last folder, with the
    /// same layout and selection. A tab that fails to start is left out and reported. Returns
    /// whether any tab opened.
    pub fn restore(
        &mut self,
        state: TabsState,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> bool {
        let mut failures = Vec::new();
        let (layout, selected) = state.layout.filter_map_selected(state.selected, |saved| {
            let spec = tabs_file::launch_spec(&saved);
            match self.spawn_tab(spec.clone(), saved.meta, window, cx) {
                Ok(id) => Some(id),
                Err(err) => {
                    failures.push(format!("{}: {err}", launch_name(&spec)));
                    None
                }
            }
        });
        if !failures.is_empty() {
            let message = format!("Could not reopen {}", failures.join("; "));
            log::error!("{message}");
            // The notification layer is set up once the window's root view exists.
            cx.defer_in(window, move |_, window, cx| {
                window.push_notification(Notification::error(message), cx);
            });
        }
        if self.tabs.is_empty() {
            return false;
        }
        self.layout = Layout::restore(layout);
        self.sync_order(cx);
        let selected = selected.or_else(|| self.tabs.first().map(|tab| tab.id));
        for tab in self.tabs.iter().filter(|tab| Some(tab.id) != selected) {
            tab.view.update(cx, |view, _| view.hide());
        }
        if let Some(index) = selected.and_then(|id| self.tab_index(id)) {
            self.activate(index, window, cx);
        }
        true
    }

    /// The tabs as they are now, for reopening them at the next start. Nothing is saved when
    /// another instance owns the tabs (`instance_lock`).
    pub fn save_tabs(&self, cx: &App) {
        if !self.owns_tabs {
            return;
        }
        let active = self.active_tab().map(|tab| tab.id);
        let (layout, selected) = self.layout.save_selected(active, |id| {
            let tab = self.tabs.iter().find(|tab| tab.id == id)?;
            let folder = tab.view.read(cx).current_folder();
            Some(tabs_file::saved_tab(&tab.spec, folder, tab.meta.clone()))
        });
        tabs_file::save(&TabsState { layout, selected });
    }

    /// Close the window from inside the app (closing the last tab, or the title bar button on
    /// Linux), saving the tabs and the window bounds first.
    fn close_window(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.save_tabs(cx);
        window_bounds::close(window);
    }

    pub fn open_tab_or_notify(
        &mut self,
        spec: LaunchSpec,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let group = self
            .active_tab()
            .and_then(|tab| self.layout.group_of(tab.id))
            .map(|group| group.id);
        self.open_tab_in_or_notify(spec, NewTabPlace::EndOf(group), window, cx);
    }

    /// Open the tab named on the command line after the restored ones: at the end of the top
    /// level, since the command line names a folder, not a group.
    pub fn open_requested_tab(
        &mut self,
        spec: LaunchSpec,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.open_tab_in_or_notify(spec, NewTabPlace::EndOf(None), window, cx);
    }

    fn open_tab_in_or_notify(
        &mut self,
        spec: LaunchSpec,
        place: NewTabPlace,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let program = launch_name(&spec).to_string();
        if let Err(err) = self.open_tab_in(spec, place, window, cx) {
            log::error!("failed to start {program}: {err}");
            window.push_notification(
                Notification::error(format!("Failed to start {program}: {err}")),
                cx,
            );
        }
    }

    /// Folder for new tabs: the selected tab's current one, else the app's working directory.
    fn new_tab_cwd(&self, cx: &App) -> Option<PathBuf> {
        self.tab_cwd(self.active, cx)
    }

    /// The tab's current folder, else the app's working directory.
    fn tab_cwd(&self, index: usize, cx: &App) -> Option<PathBuf> {
        self.tabs
            .get(index)
            .and_then(|tab| tab.view.read(cx).current_folder())
            .or_else(|| std::env::current_dir().ok())
    }

    fn open_launcher(&mut self, launcher: &Launcher, window: &mut Window, cx: &mut Context<Self>) {
        let cwd = self.new_tab_cwd(cx);
        if let Some(spec) = launcher.spec(cwd) {
            self.open_tab_or_notify(spec, window, cx);
        }
    }

    /// Open the default shell (the `+` menu's first entry) in the selected tab's folder.
    fn open_default_shell(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(shell) =
            launcher::default_shell(&self.launchers, settings_file::settings(cx).default_shell())
                .cloned()
        else {
            window.push_notification(Notification::error("No shell found"), cx);
            return;
        };
        self.open_launcher(&shell, window, cx);
    }

    /// Open another tab running what the tab runs, in its current folder and group.
    fn duplicate_tab(&mut self, index: usize, window: &mut Window, cx: &mut Context<Self>) {
        let Some(tab) = self.tabs.get(index) else {
            return;
        };
        let (mut spec, id) = (tab.spec.clone(), tab.id);
        spec.cwd = self.tab_cwd(index, cx);
        self.open_tab_in_or_notify(spec, NewTabPlace::After(id), window, cx);
    }

    /// Select a tab, expanding its group if it is collapsed and ending a search that hides it.
    fn activate(&mut self, index: usize, window: &mut Window, cx: &mut Context<Self>) {
        let Some(tab) = self.tabs.get(index) else {
            return;
        };
        let (id, view) = (tab.id, tab.view.clone());
        self.settings.hide_page();
        self.layout.reveal(id);
        self.clear_search_hiding(id, window, cx);
        if let Some(previous) = self.active_tab().filter(|_| self.active != index) {
            previous.view.update(cx, |view, _| view.hide());
        }
        self.active = index;
        view.update(cx, |view, cx| view.show(cx));
        window.focus(&view.focus_handle(cx), cx);
        self.sync_explorer(cx);
        cx.notify();
    }

    /// Close a tab, ending its process, once the user agrees to drop the unsaved edits of its
    /// file viewer, if any. Closing the last tab closes the window.
    fn close_tab(&mut self, index: usize, window: &mut Window, cx: &mut Context<Self>) {
        let Some(tab) = self.tabs.get(index) else {
            return;
        };
        let dirty_viewer = tab
            .viewer
            .as_ref()
            .map(|viewer| viewer.view.clone())
            .filter(|viewer| viewer.read(cx).is_dirty());
        let Some(viewer) = dirty_viewer else {
            self.remove_tab(index, window, cx);
            return;
        };
        let (tab_id, workspace) = (tab.id, cx.entity().downgrade());
        viewer.update(cx, |viewer, cx| {
            viewer.discard_then(window, cx, move |_, window, cx| {
                // Run outside the viewer's update: closing the tab updates the viewer.
                let workspace = workspace.clone();
                window.defer(cx, move |window, cx| {
                    let _ = workspace.update(cx, |workspace, cx| {
                        if let Some(index) = workspace.tab_index(tab_id) {
                            workspace.remove_tab(index, window, cx);
                        }
                    });
                });
            });
        });
    }

    fn remove_tab(&mut self, index: usize, window: &mut Window, cx: &mut Context<Self>) {
        let tab = self.tabs.remove(index);
        // GPUI keeps a decoded image and its textures until told to drop them.
        if let Some(viewer) = &tab.viewer {
            viewer
                .view
                .update(cx, |viewer, cx| viewer.forget_image(window, cx));
        }
        self.layout.remove(tab.id);
        self.cards.forget(tab.id);
        if self.tabs.is_empty() {
            self.close_window(window, cx);
            return;
        }
        if index < self.active || self.active == self.tabs.len() {
            self.active -= 1;
        }
        if self.settings.shows_page() {
            // The page stays; the tab now selected under it (hidden, as every other tab) shows
            // when the page goes.
            self.sync_explorer(cx);
            self.focus_active(window, cx);
            cx.notify();
            return;
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

    fn active_tab(&self) -> Option<&Tab> {
        self.tabs.get(self.active)
    }

    fn tab_index(&self, id: u64) -> Option<usize> {
        self.tabs.iter().position(|tab| tab.id == id)
    }

    /// Put `tabs` back in sidebar order after the layout changed, keeping the same tab
    /// selected.
    fn sync_order(&mut self, cx: &mut Context<Self>) {
        let active_id = self.active_tab().map(|tab| tab.id);
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
            TabCommand::Duplicate => self.duplicate_tab(index, window, cx),
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

    /// Collapse or expand a group from its header. Not while searching: groups then show
    /// expanded whatever their state, so the change would only appear once the search ends.
    fn toggle_group(&mut self, id: GroupId, cx: &mut Context<Self>) {
        if !self.search.query(cx).is_empty() {
            return;
        }
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
                workspace.commit_rename(window, cx);
            }
        });
        self.renaming = Some(Rename {
            target,
            input,
            _events: events,
        });
        cx.notify();
    }

    fn commit_rename(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(rename) = self.renaming.take() else {
            return;
        };
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
        self.end_rename(window, cx);
    }

    fn cancel_rename(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.renaming.take().is_some() {
            self.end_rename(window, cx);
        }
    }

    /// Give the keyboard back to the selected terminal once the editor is gone.
    fn end_rename(&self, window: &mut Window, cx: &mut Context<Self>) {
        self.focus_active(window, cx);
        cx.notify();
    }

    /// The name editor for `target`, when it is being renamed.
    fn name_editor(&self, target: RenameTarget, cx: &mut Context<Self>) -> Option<AnyElement> {
        let rename = self
            .renaming
            .as_ref()
            .filter(|rename| rename.target == target)?;
        let editor =
            div()
                .flex_1()
                .min_w_0()
                // Keep clicks inside the editor from reaching the row, which would move focus.
                .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
                .on_action(cx.listener(|workspace, _: &Escape, window, cx| {
                    workspace.cancel_rename(window, cx)
                }))
                .child(Input::new(&rename.input).xsmall());
        Some(editor.into_any_element())
    }

    /// Each tab's attention, in sidebar order.
    fn attention(&self, cx: &App) -> Vec<Option<Attention>> {
        self.tabs
            .iter()
            .map(|tab| tab.view.read(cx).tab_status().attention())
            .collect()
    }

    /// The tab shown, left out of the attention count and jump: none while the settings page
    /// hides the selected tab.
    fn current_index(&self) -> Option<usize> {
        (!self.settings.shows_page() && self.active < self.tabs.len()).then_some(self.active)
    }

    /// Select the next other tab needing attention; with none, the next one with a finished turn.
    fn jump_to_attention(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if let Some(index) = next_attention(&self.attention(cx), self.current_index()) {
            self.activate(index, window, cx);
        }
    }

    /// How many other tabs than the selected one wait for a permission decision or an answer.
    fn attention_count(&self, cx: &App) -> usize {
        waiting_count(&self.attention(cx), self.current_index())
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

    fn toggle_explorer(&mut self, cx: &mut Context<Self>) {
        self.explorer_visible = !self.explorer_visible;
        if self.explorer_visible {
            self.sync_explorer(cx);
        } else {
            // Drop the watches while hidden; showing it again reads the folders afresh.
            self.explorer
                .update(cx, |explorer, cx| explorer.set_root(None, false, cx));
        }
        cx.notify();
    }

    /// Point the explorer at the selected tab's folder, as of its last tab change (switch,
    /// title change, end of an output burst: around `cd`), while it shows.
    fn sync_explorer(&mut self, cx: &mut Context<Self>) {
        if !self.explorer_visible {
            return;
        }
        let (root, can_cd) = match self.active_tab() {
            Some(tab) => {
                let view = tab.view.read(cx);
                let root = view
                    .location()
                    .map(|location| location.dir.clone())
                    .or_else(|| view.current_folder());
                // Typing `cd` into a running program would feed it keys.
                (root, view.kind() == SessionKind::Shell && view.at_prompt())
            }
            None => (None, false),
        };
        self.explorer
            .update(cx, |explorer, cx| explorer.set_root(root, can_cd, cx));
    }

    fn on_explorer_event(
        &mut self,
        _: &Entity<FileExplorer>,
        event: &ExplorerEvent,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        match event {
            ExplorerEvent::Close => self.toggle_explorer(cx),
            ExplorerEvent::CdTo(folder) => {
                if let Some(tab) = self.active_tab() {
                    // Stale when a command started since the explorer last synced.
                    if !tab.view.read(cx).at_prompt() {
                        return;
                    }
                    let changed = tab.view.update(cx, |view, cx| view.change_dir(folder, cx));
                    if let Err(err) = changed {
                        window.push_notification(Notification::error(err.to_string()), cx);
                        return;
                    }
                }
                self.focus_active(window, cx);
            }
            ExplorerEvent::OpenTab(folder, agent) => {
                let launcher = match agent {
                    Some(agent) => agent.clone(),
                    None => match launcher::default_shell(
                        &self.launchers,
                        settings_file::settings(cx).default_shell(),
                    ) {
                        Some(shell) => shell.clone(),
                        None => {
                            window.push_notification(Notification::error("No shell found"), cx);
                            return;
                        }
                    },
                };
                if let Some(spec) = launcher.spec(Some(folder.clone())) {
                    self.open_tab_or_notify(spec, window, cx);
                }
            }
            ExplorerEvent::EditEnded => self.focus_active(window, cx),
            ExplorerEvent::OpenFile(path) => {
                if let Some(id) = self.active_tab().map(|tab| tab.id) {
                    self.open_file_in(id, path.clone(), FilePlace::default(), window, cx);
                }
            }
        }
    }

    fn on_viewer_event(
        &mut self,
        tab_id: u64,
        event: &ViewerEvent,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        match event {
            ViewerEvent::Opened => cx.notify(),
            // The settings page reads the file again (its load error may be fixed).
            ViewerEvent::Saved(path) if settings_file::is_file(cx, path) => {
                settings_file::reload(cx);
            }
            ViewerEvent::Saved(_) => {}
            ViewerEvent::RevealFolder(folder) => {
                if !self.explorer_visible {
                    self.toggle_explorer(cx);
                }
                let shown = self
                    .explorer
                    .update(cx, |explorer, cx| explorer.reveal(folder, cx));
                if !shown {
                    let message = format!("{} is outside the explorer's folder", folder.display());
                    window.push_notification(Notification::info(message), cx);
                }
            }
            ViewerEvent::Closed => {
                // Only this tab's viewer closes; the next file opened gets a new one.
                if let Some(index) = self.tab_index(tab_id) {
                    self.tabs[index].viewer = None;
                }
                self.focus_active(window, cx);
                cx.notify();
            }
        }
    }

    /// The width the sidebar and the explorer take from the row of panes.
    fn fixed_panes_width(&self, cx: &App) -> f32 {
        let explorer = if self.explorer_visible {
            self.explorer_width(cx)
        } else {
            0.
        };
        self.sidebar_width() + explorer
    }

    fn sidebar_width(&self) -> f32 {
        if self.sidebar_visible {
            SIDEBAR_WIDTH
        } else {
            0.
        }
    }

    /// The file viewer's share of the window width: the one being dragged, else the saved one
    /// (which follows the settings file when it is reloaded).
    fn viewer_fraction(&self, cx: &App) -> f32 {
        self.viewer_drag
            .current(|| settings_file::settings(cx).file_viewer_fraction.get())
    }

    /// Follow a divider drag in memory only: [`Self::end_divider_drag`] saves it.
    fn drag_divider(&mut self, event: &DragMoveEvent<ViewerDivider>, cx: &mut Context<Self>) {
        let row = event.bounds;
        let x = f32::from(event.event.position.x - row.origin.x);
        let fraction =
            pane_width::fraction_at(x, f32::from(row.size.width), self.fixed_panes_width(cx));
        if self.viewer_drag.follow(fraction, self.viewer_fraction(cx)) {
            cx.notify();
        }
    }

    /// The explorer's width in pixels: the one being dragged, else the saved one.
    fn explorer_width(&self, cx: &App) -> f32 {
        self.explorer_drag
            .current(|| settings_file::settings(cx).file_explorer_width)
            .get()
    }

    /// Follow a drag of the explorer's edge in memory only, like [`Self::drag_divider`].
    fn drag_explorer_edge(
        &mut self,
        event: &DragMoveEvent<ExplorerDivider>,
        cx: &mut Context<Self>,
    ) {
        let x = f32::from(event.event.position.x - event.bounds.origin.x);
        let width = ExplorerWidth::at(x, self.sidebar_width());
        let shown = ExplorerWidth::new(self.explorer_width(cx));
        if self.explorer_drag.follow(width, shown) {
            cx.notify();
        }
    }

    /// On any mouse button release: save the width a divider drag left, once per drag.
    fn end_divider_drag(&mut self, cx: &mut Context<Self>) {
        self.viewer_drag.finish(cx);
        self.explorer_drag.finish(cx);
    }

    fn save_explorer_width(width: ExplorerWidth, cx: &mut App) {
        settings_file::save_layout(cx, |settings| settings.file_explorer_width = width);
    }

    /// The explorer in a box of its width, with a strip on its right edge: drag it to resize
    /// the explorer, double-click it for the default width.
    fn explorer_panel(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let hovered = cx.theme().border;
        div()
            .relative()
            .flex_none()
            .w(px(self.explorer_width(cx)))
            .h_full()
            .child(self.explorer.clone())
            .child(
                div()
                    .id("explorer-divider")
                    // Keeps presses on the strip from reaching the explorer rows under it.
                    .occlude()
                    .absolute()
                    .top_0()
                    .right_0()
                    .h_full()
                    .w(px(DIVIDER_WIDTH))
                    .hover(move |style| style.bg(hovered))
                    .cursor(CursorStyle::ResizeLeftRight)
                    .on_drag(ExplorerDivider, |_, _, _, cx| cx.new(|_| ExplorerDivider))
                    .on_mouse_down(
                        MouseButton::Left,
                        cx.listener(|workspace, event: &MouseDownEvent, _, cx| {
                            if event.click_count == 2 {
                                workspace.explorer_drag.reset(ExplorerWidth::default(), cx);
                            }
                        }),
                    ),
            )
    }

    fn save_viewer_fraction(fraction: f32, cx: &mut App) {
        settings_file::save_layout(cx, |settings| {
            settings.file_viewer_fraction = ViewerFraction::new(fraction);
        });
    }

    /// The strip on the viewer's left edge: drag it to resize the viewer, double-click it for
    /// the default width.
    fn viewer_divider(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = cx.theme();
        let (background, hovered) = (theme.background, theme.border);
        div()
            .id("viewer-divider")
            .flex_none()
            .w(px(DIVIDER_WIDTH))
            .h_full()
            .bg(background)
            .hover(move |style| style.bg(hovered))
            .cursor(CursorStyle::ResizeLeftRight)
            .on_drag(ViewerDivider, |_, _, _, cx| cx.new(|_| ViewerDivider))
            .on_mouse_down(
                MouseButton::Left,
                cx.listener(|workspace, event: &MouseDownEvent, _, cx| {
                    if event.click_count == 2 {
                        workspace.viewer_drag.reset(DEFAULT_VIEWER_FRACTION, cx);
                    }
                }),
            )
    }

    /// Give the keyboard to the selected tab's terminal, or to the settings page while it shows.
    fn focus_active(&self, window: &mut Window, cx: &mut Context<Self>) {
        if self.settings.shows_page() {
            window.focus(self.settings.focus(), cx);
        } else if let Some(tab) = self.active_tab() {
            window.focus(&tab.view.focus_handle(cx), cx);
        }
    }

    /// Ctrl+Tab / Ctrl+Shift+Tab cycle tabs, Ctrl+1..9 select one, Ctrl+Shift+T opens the
    /// default shell, Ctrl+Shift+D another tab like the selected one, Ctrl+Shift+W closes it,
    /// Ctrl+Shift+B shows or hides the sidebar, Ctrl+Shift+S searches tabs, Ctrl+Shift+A jumps
    /// to the next tab needing attention, Ctrl+Shift+| shows or hides the file explorer.
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
            (NEW_TAB_KEY, true) => self.open_default_shell(window, cx),
            (DUPLICATE_TAB_KEY, true) => self.duplicate_tab(self.active, window, cx),
            (CLOSE_TAB_KEY, true) if self.settings.shows_page() => self.close_settings(window, cx),
            (CLOSE_TAB_KEY, true) => self.close_tab(self.active, window, cx),
            (SIDEBAR_KEY, true) => self.toggle_sidebar(cx),
            // GPUI drops Shift from a shifted symbol: Ctrl+Shift+\ arrives as Ctrl+|.
            (EXPLORER_KEY, _) => self.toggle_explorer(cx),
            (SEARCH_KEY, true) => self.focus_search(window, cx),
            (ATTENTION_KEY, true) => self.jump_to_attention(window, cx),
            (key, false) => match tab_number(key) {
                Some(number) => self.activate(number - 1, window, cx),
                None => return false,
            },
            _ => return false,
        }
        true
    }

    fn sidebar_toggle(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let (icon, label) = if self.sidebar_visible {
            (IconName::PanelLeftClose, "Hide sidebar")
        } else {
            (IconName::PanelLeftOpen, "Show sidebar")
        };
        Button::new("toggle-sidebar")
            .icon(icon)
            .ghost()
            .small()
            .tooltip(shortcuts::with_shortcut(label, SIDEBAR_KEY))
            .on_click(cx.listener(|workspace, _, _, cx| workspace.toggle_sidebar(cx)))
    }

    fn explorer_toggle(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let label = if self.explorer_visible {
            "Hide explorer"
        } else {
            "Show explorer"
        };
        Button::new("toggle-explorer")
            .icon(Icon::empty().path(FILES_ICON))
            .ghost()
            .small()
            .selected(self.explorer_visible)
            .tooltip(shortcuts::with_shortcut(label, EXPLORER_KEY))
            .on_click(cx.listener(|workspace, _, _, cx| workspace.toggle_explorer(cx)))
    }
}

fn copy_text(text: String, cx: &mut App) {
    cx.write_to_clipboard(ClipboardItem::new_string(text));
}

/// The tab number for Ctrl+1..9.
fn tab_number(key: &str) -> Option<usize> {
    key.parse()
        .ok()
        .filter(|number| (1..=NUMBERED_TABS).contains(number))
}

impl Focusable for WorkspaceView {
    fn focus_handle(&self, cx: &App) -> FocusHandle {
        if self.settings.shows_page() {
            return self.settings.focus().clone();
        }
        match self.active_tab() {
            Some(tab) => tab.view.focus_handle(cx),
            None => cx.focus_handle(),
        }
    }
}

impl Render for WorkspaceView {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        self.pulse_running = sidebar::pulse_allowed(window);
        let active = self
            .active_tab()
            .filter(|_| !self.settings.shows_page())
            .map(|tab| tab.view.clone());
        let viewer = self.active_viewer(cx);
        let viewer_width = pane_width::viewer_width(
            self.viewer_fraction(cx),
            f32::from(window.viewport_size().width),
            self.fixed_panes_width(cx),
        );
        if window.window_title() != about::APP_NAME {
            window.set_window_title(about::APP_NAME);
        }

        div()
            .size_full()
            .flex()
            .flex_col()
            .capture_key_down(cx.listener(Self::on_key_down_capture))
            // Both halves of "any release": over this view, and outside it (an overlay, or
            // past the window's edge).
            .capture_any_mouse_up(cx.listener(|workspace, _, _, cx| {
                workspace.end_divider_drag(cx);
            }))
            .on_mouse_up_out(
                MouseButton::Left,
                cx.listener(|workspace, _, _, cx| workspace.end_divider_drag(cx)),
            )
            .child(
                TitleBar::new()
                    .on_close_window(
                        cx.listener(|workspace, _, window, cx| workspace.close_window(window, cx)),
                    )
                    .child(
                        // Occluding keeps the title bar's drag area from taking the
                        // buttons' clicks (Windows answers HTCAPTION over them).
                        div()
                            .flex()
                            .items_center()
                            .gap_1()
                            .occlude()
                            .child(self.sidebar_toggle(cx))
                            .child(self.explorer_toggle(cx))
                            .child(about::about_button())
                            .child(self.settings_button(cx)),
                    ),
            )
            .child(
                div()
                    .flex()
                    .flex_1()
                    .min_h_0()
                    .on_drag_move(cx.listener(
                        |workspace, event: &DragMoveEvent<ViewerDivider>, _, cx| {
                            workspace.drag_divider(event, cx);
                        },
                    ))
                    .on_drag_move(cx.listener(
                        |workspace, event: &DragMoveEvent<ExplorerDivider>, _, cx| {
                            workspace.drag_explorer_edge(event, cx);
                        },
                    ))
                    .when(self.sidebar_visible, |content| {
                        content.child(self.render_sidebar(cx))
                    })
                    .when(self.explorer_visible, |content| {
                        content.child(self.explorer_panel(cx))
                    })
                    .child(div().flex_1().min_w_0().h_full().children(active).when(
                        self.settings.shows_page(),
                        |area| {
                            area.track_focus(self.settings.focus())
                                .child(settings_page::render(
                                    self.launchers.clone(),
                                    cx.listener(|workspace, _, window, cx| {
                                        workspace.open_settings_file(window, cx)
                                    }),
                                    cx,
                                ))
                        },
                    ))
                    .when_some(viewer, |content, viewer| {
                        content.child(self.viewer_divider(cx)).child(
                            div()
                                .flex_none()
                                .w(px(viewer_width - DIVIDER_WIDTH))
                                .h_full()
                                .child(viewer),
                        )
                    }),
            )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tab_numbers() {
        assert_eq!(tab_number("1"), Some(1));
        assert_eq!(tab_number("9"), Some(9));
        assert_eq!(tab_number("0"), None);
        assert_eq!(tab_number("a"), None);
    }
}
