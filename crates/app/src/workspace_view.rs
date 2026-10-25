mod busy_close;
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
use gpui_kit::component::notification::{Notification, NotificationDelivery};
use gpui_kit::component::{ActiveTheme, Icon, IconName, Selectable, Sizable, TitleBar, WindowExt};
use gpui_kit::prelude::FluentBuilder;
use gpui_kit::{
    Action, AnyElement, App, AppContext, ClipboardItem, Context, CursorStyle, DragMoveEvent, Empty,
    Entity, FocusHandle, Focusable, InteractiveElement, IntoElement, MouseButton, MouseDownEvent,
    ParentElement, Render, StatefulInteractiveElement, Styled, Subscription, Task, WeakEntity,
    Window, actions, div, px,
};
use term_core::{LaunchSpec, SessionKind};
use workspace_model::layout::Direction;
use workspace_model::pane_width::{self, DEFAULT_VIEWER_FRACTION, ExplorerWidth, ViewerFraction};
use workspace_model::{
    Attention, DropTarget, FeatureSwitch, FileLinkAction, GroupId, Layout, PaneChange, PaneEntry,
    Shortcut, TabColor, TabId, TabMeta, TabsState, ToastKind, exit_toast_kind, next_attention,
    pane_change, waiting_count,
};

use self::divider_drag::DividerDrag;
use self::settings_pane::SettingsPane;
use self::tab_card::TabCards;
use self::tab_search::TabSearch;
use crate::about;
use crate::assets::FILES_ICON;
use crate::file_explorer::{CdAvailability, ExplorerEvent, FileExplorer};
use crate::file_viewer::{FilePlace, FileViewer, ViewerEvent};
use crate::history;
use crate::hooks::ClaudeHooks;
use crate::keymap;
use crate::launcher::{self, Launcher, launch_name};
use crate::settings_file;
use crate::settings_page;
use crate::shell_integration;
use crate::shortcuts::{
    self, ATTENTION_KEY, CLOSE_TAB_KEY, DUPLICATE_TAB_KEY, EXPLORER_KEY, NEW_TAB_KEY, SEARCH_KEY,
    SIDEBAR_KEY,
};
use crate::sidebar::{self, SIDEBAR_WIDTH};
use crate::tab_menu::{GroupCommand, GroupMenuState, TabCommand, TabMenuState};
use crate::tabs_file;
use crate::terminal_view::{
    AgentExited, AgentNotice, LastCommandChanged, OpenFileRequest, StatusReporting, TabChanged,
    TerminalView,
};
use crate::window_bounds;

const NEW_GROUP_NAME: &str = "New group";
const KEY_CONTEXT: &str = "Workspace";

/// The width of the strip on the file viewer's left edge that resizes it.
const DIVIDER_WIDTH: f32 = 6.;

actions!(
    workspace,
    [
        ToggleFullScreen,
        NextTab,
        PreviousTab,
        NewTab,
        DuplicateTab,
        CloseTab,
        ToggleSidebar,
        ToggleExplorer,
        SearchTabs,
        NextAttention,
        SelectTab1,
        SelectTab2,
        SelectTab3,
        SelectTab4,
        SelectTab5,
        SelectTab6,
        SelectTab7,
        SelectTab8,
        SelectTab9,
    ]
);

/// Registers the workspace shortcuts (see `default_bindings`).
pub fn init(cx: &mut App) {
    keymap::add_defaults(cx, default_bindings());
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

/// Workspace shortcuts, changed by the settings file's `"keybindings"` (see `keymap`). F11
/// toggles full screen. It is bound for the whole window, so it works whichever pane has the
/// keyboard: bindings match before the terminal's key handler, so F11 is not sent to the
/// program.
fn default_bindings() -> Vec<Shortcut> {
    let bind =
        |keys: &str, action: &dyn Action| Shortcut::new(keys, action.name(), Some(KEY_CONTEXT));
    let mut bindings = vec![
        Shortcut::new("f11", ToggleFullScreen.name(), None),
        bind("ctrl-tab", &NextTab),
        bind("ctrl-shift-tab", &PreviousTab),
        bind(&format!("ctrl-{EXPLORER_KEY}"), &ToggleExplorer),
        bind("ctrl-1", &SelectTab1),
        bind("ctrl-2", &SelectTab2),
        bind("ctrl-3", &SelectTab3),
        bind("ctrl-4", &SelectTab4),
        bind("ctrl-5", &SelectTab5),
        bind("ctrl-6", &SelectTab6),
        bind("ctrl-7", &SelectTab7),
        bind("ctrl-8", &SelectTab8),
        bind("ctrl-9", &SelectTab9),
    ];
    for (key, action) in [
        (NEW_TAB_KEY, &NewTab as &dyn Action),
        (DUPLICATE_TAB_KEY, &DuplicateTab),
        (CLOSE_TAB_KEY, &CloseTab),
        (SIDEBAR_KEY, &ToggleSidebar),
        (SEARCH_KEY, &SearchTabs),
        (ATTENTION_KEY, &NextAttention),
    ] {
        bindings.push(bind(&format!("ctrl-shift-{key}"), action));
    }
    bindings
}

/// The action that selects the tab at `index` (Ctrl+1 to Ctrl+9 by default), if any.
pub fn select_tab_action(index: usize) -> Option<&'static str> {
    let actions: [&dyn Action; 9] = [
        &SelectTab1,
        &SelectTab2,
        &SelectTab3,
        &SelectTab4,
        &SelectTab5,
        &SelectTab6,
        &SelectTab7,
        &SelectTab8,
        &SelectTab9,
    ];
    actions.get(index).map(|action| action.name())
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TabOwnership {
    Owner,
    Guest,
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
    _view_events: [Subscription; 5],
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
    /// GPU capability is fixed for the lifetime of this window.
    pulse_running: bool,
    explorer: Entity<FileExplorer>,
    /// The "close busy tabs?" question is open (it is never asked twice at once).
    close_question_open: bool,
    explorer_visible: bool,
    /// Whether the file explorer and viewer panes work: the "File explorer and viewer" switch
    /// as last applied (it stays on while unsaved edits wait for an answer).
    file_panes_on: bool,
    /// Set while the discard questions for turning the panes off wait for answers, so a second
    /// "off" cannot start a second set. Cleared by Cancel and by the end of the chain.
    file_panes_off_pending: bool,
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
    tabs_writer: Rc<tabs_file::TabsWriter>,
    next_session_id: u64,
    launchers: Rc<[Launcher]>,
    hooks: Option<Rc<ClaudeHooks>>,
    settings: SettingsPane,
    _hook_messages: Task<()>,
    _explorer_events: Subscription,
    _file_panes_observer: Subscription,
    /// `History::saved_cleared` / `forgets` as last handled by `scrub_retained_commands`.
    history_seen: (u32, u32),
    _history_observer: Subscription,
}

fn history_counts(cx: &App) -> (u32, u32) {
    cx.try_global::<history::History>()
        .map_or((0, 0), |history| {
            (history.saved_cleared(), history.forgets())
        })
}

impl WorkspaceView {
    pub fn warn_shell_fallback(
        &self,
        failure: String,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let message = format!("{failure}. Started a fallback shell instead.");
        // Startup runs before the notification layer's root exists.
        cx.defer_in(window, move |_, window, cx| {
            window.push_notification(Notification::warning(message).autohide(false), cx);
        });
    }

    pub fn new(
        launchers: Rc<[Launcher]>,
        hooks: Option<Rc<ClaudeHooks>>,
        mut hook_messages: UnboundedReceiver<HookMessage>,
        tabs_writer: Rc<tabs_file::TabsWriter>,
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
        window.set_window_title(about::APP_NAME);
        let file_panes_observer = cx.observe_global_in::<settings_file::AppSettings>(
            window,
            Self::apply_file_panes_setting,
        );
        let history_seen = history_counts(cx);
        let history_observer = cx.observe_global::<history::History>(Self::scrub_retained_commands);
        Self {
            tabs: Vec::new(),
            layout: Layout::default(),
            active: 0,
            sidebar_visible: true,
            pulse_running: sidebar::pulse_allowed(window),
            explorer,
            close_question_open: false,
            explorer_visible: false,
            file_panes_on: settings_file::file_panes(cx).panes_enabled(),
            file_panes_off_pending: false,
            viewer_drag: DividerDrag::new(Self::save_viewer_fraction),
            explorer_drag: DividerDrag::new(Self::save_explorer_width),
            renaming: None,
            search: TabSearch::new(window, cx),
            cards: TabCards::default(),
            drop_hint: None,
            home: std::env::home_dir(),
            tabs_writer,
            next_session_id: 1,
            launchers,
            hooks,
            settings: SettingsPane::new(cx),
            _hook_messages: hook_task,
            _explorer_events: explorer_events,
            _file_panes_observer: file_panes_observer,
            history_seen,
            _history_observer: history_observer,
        }
    }

    /// "Restore previous tabs" off keeps `tabs.json` as it is, so a cleared or forgotten
    /// command would stay in it and return once restoring is turned on. Remove just the last
    /// commands from that file, through the ordered writer (never on this thread, never from a
    /// guest instance).
    fn scrub_retained_commands(&mut self, cx: &mut Context<Self>) {
        let seen = history_counts(cx);
        let (cleared_seen, forgets_seen) = std::mem::replace(&mut self.history_seen, seen);
        if !self.tabs_writer.is_active() || settings_file::workspace_features(cx).saves_tabs() {
            return;
        }
        if seen.0 != cleared_seen {
            self.tabs_writer.scrub_last_commands(None);
        }
        if seen.1 != forgets_seen
            && let Some(command) = cx.global::<history::History>().forgotten()
        {
            self.tabs_writer
                .scrub_last_commands(Some(command.to_owned()));
        }
    }

    /// Start `spec` in a new tab and select it. The tab joins the selected tab's group.
    pub fn open_tab(
        &mut self,
        spec: LaunchSpec,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> io::Result<()> {
        let group = self.active_group();
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
    ) -> io::Result<(Entity<TerminalView>, [Subscription; 5])> {
        let session_id = self.next_session_id;
        let mut launch = spec.clone();
        let reporting = if self
            .hooks
            .as_ref()
            .is_some_and(|hooks| hooks.attach(session_id, &mut launch))
        {
            StatusReporting::Hooks
        } else {
            StatusReporting::OutputActivity
        };
        shell_integration::attach(&mut launch);
        let view = TerminalView::spawn(session_id, &launch, reporting, cx).inspect_err(|_| {
            if let Some(hooks) = &self.hooks {
                hooks.remove_session(session_id);
            }
        })?;
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
        let notice = cx.subscribe_in(
            &view,
            window,
            move |workspace, _, notice: &AgentNotice, window, cx| {
                workspace.notify_agent(tab_id, *notice, window, cx);
            },
        );
        let last_command = cx.subscribe(&view, |workspace, _, _: &LastCommandChanged, cx| {
            workspace.save_tabs(cx);
        });
        Ok((view, [changed, exited, open_file, notice, last_command]))
    }

    fn notify_agent(
        &self,
        tab_id: u64,
        notice: AgentNotice,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let kind = match notice {
            AgentNotice::Exited(code) => exit_toast_kind(code),
            AgentNotice::NeedsAttention | AgentNotice::Finished => ToastKind::AgentStatus,
        };
        if !settings_file::notifications(cx).shows(kind) {
            return;
        }
        let Some(index) = self.tab_index(tab_id) else {
            return;
        };
        let name = Self::tab_name(&self.tabs[index], cx);
        let notification = match notice {
            AgentNotice::NeedsAttention => Notification::warning("Needs attention"),
            AgentNotice::Finished => Notification::info(sidebar::TabStatus::ReadyForReview.label()),
            AgentNotice::Exited(Some(0)) => Notification::success("Agent finished"),
            AgentNotice::Exited(Some(code)) => {
                Notification::error(format!("Agent exited with code {code}"))
            }
            AgentNotice::Exited(None) => Notification::error("Agent exited without an exit code"),
        };
        let workspace = cx.entity().downgrade();
        window.push_notification(
            notification
                .id1::<AgentNotice>(("agent-notice", tab_id as usize))
                .title(name)
                .delivery(NotificationDelivery::InApp)
                .on_click(move |_, window, cx| {
                    let _ = workspace.update(cx, |workspace, cx| {
                        if let Some(index) = workspace.tab_index(tab_id) {
                            workspace.activate(index, window, cx);
                        }
                    });
                }),
            cx,
        );
    }

    /// Go on with the default shell in tab `tab_id` once its agent exited, in the agent's last
    /// folder. The new session starts on an empty screen; the tab keeps its name, color, place
    /// and open file, and is saved as a shell from now on.
    fn fall_back_to_shell(&mut self, tab_id: u64, window: &mut Window, cx: &mut Context<Self>) {
        let Some(index) = self.tab_index(tab_id) else {
            return;
        };
        if let Some(hooks) = &self.hooks {
            hooks.remove_session(self.tabs[index].view.read(cx).id());
        }
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
        if let Err(err) = self.replace_tab_session(index, spec.clone(), window, cx) {
            log::error!("failed to start {}: {err}", launch_name(&spec));
            window.push_notification(
                Notification::error(format!("Failed to start {}: {err}", launch_name(&spec))),
                cx,
            );
        }
    }

    fn replace_tab_session(
        &mut self,
        index: usize,
        spec: LaunchSpec,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> io::Result<()> {
        let tab_id = self.tabs[index].id;
        let (view, view_events) = self.start_session(tab_id, &spec, window, cx)?;
        let had_focus = self.tabs[index]
            .view
            .read(cx)
            .focus_handle
            .is_focused(window);
        // "Always for this tab" lasts until the tab closes, across its sessions.
        let clipboard = self.tabs[index].view.read(cx).clipboard_carry_over();
        view.update(cx, |view, _| view.set_clipboard_guard(clipboard));
        let tab = &mut self.tabs[index];
        if let Some(hooks) = &self.hooks {
            hooks.remove_session(tab.view.read(cx).id());
        }
        tab.view = view.clone();
        tab.spec = spec;
        tab._view_events = view_events;
        self.cards.forget(tab_id);
        if index != self.active || self.settings.shows_page() {
            view.update(cx, |view, _| view.hide());
            self.sync_explorer(cx);
            cx.notify();
            return Ok(());
        }
        view.update(cx, |view, cx| view.show(cx));
        if had_focus {
            window.focus(&view.focus_handle(cx), cx);
        }
        self.sync_explorer(cx);
        cx.notify();
        Ok(())
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
        if let FileLinkAction::OpenExternally(path) = settings_file::file_panes(cx).file_link(&path)
        {
            Self::offer_external_open(path, window, cx);
            return;
        }
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
                Ok(id) => {
                    if let Some(index) = self.tab_index(id) {
                        self.tabs[index].view.update(cx, |view, cx| {
                            view.restore_last_command(saved.last_command, cx);
                        });
                    }
                    Some(id)
                }
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
    /// another instance owns the tabs (`instance_lock`) or "Restore previous tabs" is off.
    pub fn save_tabs(&self, cx: &App) {
        // "Restore previous tabs" off: tabs.json stays as it is (checked at every save, so
        // the switch applies at once and turning it on saves again from the next change).
        if !self.tabs_writer.is_active() || !settings_file::workspace_features(cx).saves_tabs() {
            return;
        }
        let active = self.active_tab().map(|tab| tab.id);
        let (layout, selected) = self.layout.save_selected(active, |id| {
            let tab = self.tabs.iter().find(|tab| tab.id == id)?;
            let folder = tab.view.read(cx).current_folder();
            let mut saved = tabs_file::saved_tab(&tab.spec, folder, tab.meta.clone());
            saved.last_command = tab.view.read(cx).last_command().map(str::to_owned);
            Some(saved)
        });
        self.tabs_writer.save(TabsState { layout, selected });
    }

    /// Queue the final UI snapshot behind earlier saves, then wait for the worker to stop.
    pub fn save_tabs_and_flush(&self, cx: &App) {
        self.save_tabs(cx);
        self.tabs_writer.finish();
    }

    /// Close the window from inside the app (closing the last tab, or the title bar button on
    /// Linux), saving the tabs and the window bounds first.
    fn close_window(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.save_tabs_and_flush(cx);
        window_bounds::close(window);
    }

    pub fn open_tab_or_notify(
        &mut self,
        spec: LaunchSpec,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let group = self.active_group();
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
    fn new_tab_folder(&self, cx: &App) -> Option<PathBuf> {
        self.tab_folder(self.active, cx)
    }

    /// The tab's current folder, else the app's working directory.
    fn tab_folder(&self, index: usize, cx: &App) -> Option<PathBuf> {
        self.tabs
            .get(index)
            .and_then(|tab| tab.view.read(cx).current_folder())
            .or_else(|| std::env::current_dir().ok())
    }

    fn open_launcher(&mut self, launcher: &Launcher, window: &mut Window, cx: &mut Context<Self>) {
        let folder = self.new_tab_folder(cx);
        self.open_launcher_in(launcher, folder, window, cx);
    }

    fn open_launcher_in(
        &mut self,
        launcher: &Launcher,
        folder: Option<PathBuf>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        match launcher.configured_spec(folder, settings_file::settings(cx)) {
            Ok(Some(spec)) => self.open_tab_or_notify(spec, window, cx),
            Ok(None) => {}
            Err(error) => window.push_notification(
                Notification::error(format!("{} arguments: {error}", launcher.label)),
                cx,
            ),
        }
    }

    /// Open the default shell (the `+` menu's first entry) in the selected tab's folder.
    fn open_default_shell(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.open_default_shell_in(self.new_tab_folder(cx), window, cx);
    }

    fn open_default_shell_in(
        &mut self,
        folder: Option<PathBuf>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(shell) =
            launcher::default_shell(&self.launchers, settings_file::settings(cx).default_shell())
                .cloned()
        else {
            window.push_notification(Notification::error("No shell found"), cx);
            return;
        };
        if let Some(spec) = shell.spec(folder) {
            self.open_tab_or_notify(spec, window, cx);
        }
    }

    /// Open another tab running what the tab runs, in its current folder and group.
    fn duplicate_tab(&mut self, index: usize, window: &mut Window, cx: &mut Context<Self>) {
        let Some(tab) = self.tabs.get(index) else {
            return;
        };
        let (mut spec, id) = (tab.spec.clone(), tab.id);
        spec.cwd = self.tab_folder(index, cx);
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
    /// file viewer, if any. Closing the last tab closes the window. Busy tabs were asked about
    /// already (`close_tab`).
    fn close_tab_checking_edits(
        &mut self,
        index: usize,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
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
                Self::defer_remove_tab(workspace.clone(), tab_id, window, cx);
            });
        });
    }

    fn defer_remove_tab(
        workspace: WeakEntity<Self>,
        tab_id: u64,
        window: &mut Window,
        cx: &mut App,
    ) {
        // Closing a tab updates its viewer, so wait until the viewer's update has ended.
        window.defer(cx, move |window, cx| {
            let _ = workspace.update(cx, |workspace, cx| {
                if let Some(index) = workspace.tab_index(tab_id) {
                    workspace.remove_tab(index, window, cx);
                }
            });
        });
    }

    fn remove_tab(&mut self, index: usize, window: &mut Window, cx: &mut Context<Self>) {
        let tab = self.tabs.remove(index);
        if let Some(hooks) = &self.hooks {
            hooks.remove_session(tab.view.read(cx).id());
        }
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

    fn active_tab(&self) -> Option<&Tab> {
        self.tabs.get(self.active)
    }

    fn active_group(&self) -> Option<GroupId> {
        self.active_tab()
            .and_then(|tab| self.layout.group_of(tab.id))
            .map(|group| group.id)
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
                    copy_text(location.folder.display().to_string(), cx);
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

    fn cycle(&mut self, direction: Direction, window: &mut Window, cx: &mut Context<Self>) {
        let count = self.tabs.len();
        if count == 0 {
            return;
        }
        let next = match direction {
            Direction::Down => (self.active + 1) % count,
            Direction::Up => (self.active + count - 1) % count,
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

    fn toggle_sidebar(&mut self, cx: &mut Context<Self>) {
        self.sidebar_visible = !self.sidebar_visible;
        cx.notify();
    }

    fn toggle_explorer(&mut self, cx: &mut Context<Self>) {
        if !settings_file::file_panes(cx).allows(PaneEntry::ToggleExplorer) {
            return;
        }
        self.explorer_visible = !self.explorer_visible;
        if self.explorer_visible {
            self.sync_explorer(cx);
        } else {
            // Drop the watches while hidden; showing it again reads the folders afresh.
            self.explorer.update(cx, |explorer, cx| {
                explorer.set_root(None, CdAvailability::Unavailable, cx)
            });
        }
        cx.notify();
    }

    /// Point the explorer at the selected tab's folder, as of its last tab change (switch,
    /// title change, end of an output burst: around `cd`), while it shows.
    fn sync_explorer(&mut self, cx: &mut Context<Self>) {
        if !self.explorer_visible {
            return;
        }
        let (root, cd) = match self.active_tab() {
            Some(tab) => {
                let view = tab.view.read(cx);
                let root = view
                    .location()
                    .map(|location| location.folder.clone())
                    .or_else(|| view.current_folder());
                // Typing `cd` into a running program would feed it keys.
                let cd = if view.kind() == SessionKind::Shell && view.at_prompt() {
                    CdAvailability::Available
                } else {
                    CdAvailability::Unavailable
                };
                (root, cd)
            }
            None => (None, CdAvailability::Unavailable),
        };
        self.explorer
            .update(cx, |explorer, cx| explorer.set_root(root, cd, cx));
    }

    /// With the panes off, a file link asks whether to open the file with the system's
    /// handler (a program or script there may run, so it is not done unasked).
    fn offer_external_open(path: PathBuf, window: &mut Window, cx: &mut App) {
        let name = format!("{path:?}");
        window.open_alert_dialog(cx, move |alert, _, _| {
            let path = path.clone();
            alert
                .title("Open externally?")
                .description(format!(
                    "The file explorer and viewer are off. Open {name} with the system's \
                     default application?"
                ))
                .ok_text("Open externally")
                .show_cancel(true)
                .on_ok(move |_, _, cx| {
                    cx.open_with_system(&path);
                    true
                })
        });
    }

    /// The ids of tabs whose viewer shows a file with unsaved edits.
    fn unsaved_viewer_tabs(&self, cx: &App) -> Vec<u64> {
        self.tabs
            .iter()
            .filter(|tab| {
                tab.viewer
                    .as_ref()
                    .is_some_and(|viewer| viewer.view.read(cx).is_dirty())
            })
            .map(|tab| tab.id)
            .collect()
    }

    /// Settings changed: show or drop the file panes to match the switch. Turning it off with
    /// unsaved edits puts the switch back on and asks (see `refuse_file_panes_off`).
    fn apply_file_panes_setting(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let wanted = settings_file::file_panes(cx).panes_enabled();
        let shown = self.file_panes_on;
        let unsaved = if shown && !wanted {
            self.unsaved_viewer_tabs(cx)
        } else {
            Vec::new()
        };
        match pane_change(shown, wanted, unsaved.len()) {
            PaneChange::Unchanged => {}
            PaneChange::Show => {
                self.file_panes_on = true;
                cx.notify();
            }
            PaneChange::Hide => self.drop_file_panes(window, cx),
            PaneChange::ResolveEdits => self.refuse_file_panes_off(unsaved, window, cx),
        }
    }

    /// The page's switch (or a hand edit) already wrote "off": write "on" back (settings.json
    /// is off for a moment, see PLAN.md) and start the discard questions, unless a chain of
    /// them is already waiting for answers.
    fn refuse_file_panes_off(
        &mut self,
        unsaved: Vec<u64>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        settings_file::update(cx, |settings| {
            settings.file_panes = FeatureSwitch::new(true);
        });
        if self.file_panes_off_pending {
            return;
        }
        self.file_panes_off_pending = true;
        Self::confirm_next(cx.entity().downgrade(), unsaved, window, cx);
    }

    fn confirm_next(
        workspace: WeakEntity<Self>,
        unsaved: Vec<u64>,
        window: &mut Window,
        cx: &mut App,
    ) {
        window.defer(cx, move |window, cx| {
            Self::confirm_file_panes_off(workspace, unsaved, window, cx);
        });
    }

    /// Ask, with the viewer's own discard prompt, about each unsaved file in turn. Cancel
    /// anywhere leaves everything as it was; after the last answer the switch goes off.
    fn confirm_file_panes_off(
        workspace: WeakEntity<Self>,
        mut unsaved: Vec<u64>,
        window: &mut Window,
        cx: &mut App,
    ) {
        let Some(tab_id) = unsaved.pop() else {
            Self::finish_file_panes_off(&workspace, window, cx);
            return;
        };
        let _ = workspace.update(cx, |workspace, cx| {
            workspace.ask_to_discard(tab_id, unsaved, window, cx);
        });
    }

    /// Every edit may go: drop the panes and write the switch off. The chain is over.
    fn finish_file_panes_off(workspace: &WeakEntity<Self>, window: &mut Window, cx: &mut App) {
        let alive = workspace.update(cx, |workspace, cx| {
            workspace.file_panes_off_pending = false;
            workspace.drop_file_panes(window, cx);
        });
        if alive.is_err() {
            return;
        }
        settings_file::update(cx, |settings| {
            settings.file_panes = FeatureSwitch::new(false);
        });
    }

    /// One step of the chain: the tab's discard prompt, then the next file. A tab closed
    /// meanwhile is skipped.
    fn ask_to_discard(
        &mut self,
        tab_id: u64,
        rest: Vec<u64>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let weak = cx.entity().downgrade();
        let viewer = self
            .tab_index(tab_id)
            .and_then(|index| self.tabs[index].viewer.as_ref())
            .map(|viewer| viewer.view.clone());
        let Some(viewer) = viewer else {
            Self::confirm_next(weak, rest, window, cx);
            return;
        };
        let cancelled = weak.clone();
        viewer.update(cx, |viewer, cx| {
            viewer.discard_then_or(
                window,
                cx,
                move |_, window, cx| {
                    Self::confirm_next(weak.clone(), rest.clone(), window, cx);
                },
                move |cx| {
                    let _ = cancelled.update(cx, |workspace, _| {
                        workspace.file_panes_off_pending = false;
                    });
                },
            );
        });
    }

    /// Hide both panes and stop their work: the explorer loses its folder (and watches), the
    /// viewers go with their files and watches. Widths stay in the settings.
    fn drop_file_panes(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.file_panes_on = false;
        self.explorer_visible = false;
        self.explorer.update(cx, |explorer, cx| {
            explorer.set_root(None, CdAvailability::Unavailable, cx)
        });
        for tab in &mut self.tabs {
            if let Some(viewer) = tab.viewer.take() {
                viewer
                    .view
                    .update(cx, |viewer, cx| viewer.forget_image(window, cx));
            }
        }
        if !self.settings.shows_page() {
            self.focus_active(window, cx);
        }
        cx.notify();
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
                self.change_active_folder(folder, window, cx);
            }
            ExplorerEvent::OpenTab(folder, agent) => {
                let Some(agent) = agent else {
                    self.open_default_shell_in(Some(folder.clone()), window, cx);
                    return;
                };
                self.open_launcher_in(agent, Some(folder.clone()), window, cx);
            }
            ExplorerEvent::EditEnded => self.focus_active(window, cx),
            ExplorerEvent::OpenFile(path) => {
                if let Some(id) = self.active_tab().map(|tab| tab.id) {
                    self.open_file_in(id, path.clone(), FilePlace::default(), window, cx);
                }
            }
        }
    }

    fn change_active_folder(
        &mut self,
        folder: &std::path::Path,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(tab) = self.active_tab() else {
            self.focus_active(window, cx);
            return;
        };
        // A command may have started since the explorer last synchronized.
        if !tab.view.read(cx).at_prompt() {
            return;
        }
        if let Err(err) = tab.view.update(cx, |view, cx| view.change_dir(folder, cx)) {
            window.push_notification(Notification::error(err.to_string()), cx);
            return;
        }
        self.focus_active(window, cx);
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

    fn close_active(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.settings.shows_page() {
            self.close_settings(window, cx);
            return;
        }
        self.close_tab(self.active, window, cx);
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
            .tooltip(shortcuts::with_shortcut(label, &ToggleSidebar, cx))
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
            .tooltip(shortcuts::with_shortcut(label, &ToggleExplorer, cx))
            .on_click(cx.listener(|workspace, _, _, cx| workspace.toggle_explorer(cx)))
    }
}

fn copy_text(text: String, cx: &mut App) {
    cx.write_to_clipboard(ClipboardItem::new_string(text));
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
    #[expect(clippy::too_many_lines, reason = "split when next changed")]
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let active = self
            .active_tab()
            .filter(|_| !self.settings.shows_page())
            .map(|tab| tab.view.clone());
        let file_panes = settings_file::file_panes(cx);
        let viewer = self.active_viewer(cx);
        let viewer_width = pane_width::viewer_width(
            self.viewer_fraction(cx),
            f32::from(window.viewport_size().width),
            self.fixed_panes_width(cx),
        );
        div()
            .size_full()
            .flex()
            .flex_col()
            .key_context(KEY_CONTEXT)
            // Capture handlers propagate by default; consume workspace shortcuts before
            // the terminal can translate them into control bytes.
            .capture_action(cx.listener(|workspace, _: &NextTab, window, cx| {
                workspace.cycle(Direction::Down, window, cx);
                cx.stop_propagation();
            }))
            .capture_action(cx.listener(|workspace, _: &PreviousTab, window, cx| {
                workspace.cycle(Direction::Up, window, cx);
                cx.stop_propagation();
            }))
            .capture_action(cx.listener(|workspace, _: &NewTab, window, cx| {
                workspace.open_default_shell(window, cx);
                cx.stop_propagation();
            }))
            .capture_action(cx.listener(|workspace, _: &DuplicateTab, window, cx| {
                workspace.duplicate_tab(workspace.active, window, cx);
                cx.stop_propagation();
            }))
            .capture_action(cx.listener(|workspace, _: &CloseTab, window, cx| {
                workspace.close_active(window, cx);
                cx.stop_propagation();
            }))
            .capture_action(cx.listener(|workspace, _: &ToggleSidebar, _, cx| {
                workspace.toggle_sidebar(cx);
                cx.stop_propagation();
            }))
            .capture_action(cx.listener(|workspace, _: &ToggleExplorer, _, cx| {
                workspace.toggle_explorer(cx);
                cx.stop_propagation();
            }))
            .capture_action(cx.listener(|workspace, _: &SearchTabs, window, cx| {
                workspace.focus_search(window, cx);
                cx.stop_propagation();
            }))
            .capture_action(cx.listener(|workspace, _: &NextAttention, window, cx| {
                workspace.jump_to_attention(window, cx);
                cx.stop_propagation();
            }))
            .capture_action(cx.listener(|workspace, _: &SelectTab1, window, cx| {
                workspace.activate(0, window, cx);
                cx.stop_propagation();
            }))
            .capture_action(cx.listener(|workspace, _: &SelectTab2, window, cx| {
                workspace.activate(1, window, cx);
                cx.stop_propagation();
            }))
            .capture_action(cx.listener(|workspace, _: &SelectTab3, window, cx| {
                workspace.activate(2, window, cx);
                cx.stop_propagation();
            }))
            .capture_action(cx.listener(|workspace, _: &SelectTab4, window, cx| {
                workspace.activate(3, window, cx);
                cx.stop_propagation();
            }))
            .capture_action(cx.listener(|workspace, _: &SelectTab5, window, cx| {
                workspace.activate(4, window, cx);
                cx.stop_propagation();
            }))
            .capture_action(cx.listener(|workspace, _: &SelectTab6, window, cx| {
                workspace.activate(5, window, cx);
                cx.stop_propagation();
            }))
            .capture_action(cx.listener(|workspace, _: &SelectTab7, window, cx| {
                workspace.activate(6, window, cx);
                cx.stop_propagation();
            }))
            .capture_action(cx.listener(|workspace, _: &SelectTab8, window, cx| {
                workspace.activate(7, window, cx);
                cx.stop_propagation();
            }))
            .capture_action(cx.listener(|workspace, _: &SelectTab9, window, cx| {
                workspace.activate(8, window, cx);
                cx.stop_propagation();
            }))
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
                    .on_close_window(cx.listener(|workspace, _, window, cx| {
                        workspace.request_close_window(window, cx);
                    }))
                    .child(
                        // Occluding keeps the title bar's drag area from taking the
                        // buttons' clicks (Windows answers HTCAPTION over them).
                        div()
                            .flex()
                            .items_center()
                            .gap_1()
                            .occlude()
                            .child(self.sidebar_toggle(cx))
                            .when(file_panes.panes_enabled(), |buttons| {
                                buttons.child(self.explorer_toggle(cx))
                            })
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
mod tests;
