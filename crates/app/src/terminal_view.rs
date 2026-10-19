use std::borrow::Cow;
use std::fs;
use std::io;
use std::ops::Range;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::{Duration, Instant};

use agent_status::{AgentState, AgentStatus, HookEvent};

use futures::StreamExt;
use futures::channel::mpsc;
use gpui_kit::component::ActiveTheme;
use gpui_kit::{
    AnyElement, App, AppContext, Bounds, ClipboardItem, Context, CursorStyle, Entity,
    EntityInputHandler, EventEmitter, FocusHandle, Focusable, InteractiveElement, IntoElement,
    KeyBinding, KeyDownEvent, Keystroke, Modifiers, ModifiersChangedEvent, MouseButton,
    MouseDownEvent, MouseMoveEvent, MouseUpEvent, NoAction, ParentElement, Pixels, Point, Render,
    ScrollDelta, ScrollWheelEvent, Styled, Task, UTF16Selection, Window, actions, div, size,
};
use term_core::alacritty_terminal::grid::Scroll;
use term_core::alacritty_terminal::index::{Column, Line, Point as GridPoint, Side};
use term_core::alacritty_terminal::selection::SelectionType;
use term_core::alacritty_terminal::term::TermMode;
use term_core::{
    EventCallback, LaunchSpec, Palette, Session, SessionEvent, SessionKind, TermDimensions,
};
use workspace_model::file_tree::{self, ShellSyntax, TypeError, cd_command};
use workspace_model::{
    COMMAND_LABEL_CHARS, Lap, Location, ProgressState, RUNNING_AFTER, RunningBadge, WheelScroll,
    command_label, notch_lines, running_badge,
};

use crate::file_viewer::FilePlace;
use crate::history;
use crate::history_popup::HistoryPopup;
use crate::keys;
use crate::launcher;
use crate::mouse::{self, Cell, MouseAction, ReportButton, WheelAction};
use crate::scrollbar::{self, ScrollbarLayout, ScrollbarState};
use crate::shell_integration;
use crate::sidebar::TabStatus;
use crate::terminal_element::{TerminalElement, to_hsla};
use crate::terminal_links::{self, LinkTarget, TerminalLink};

const KEY_CONTEXT: &str = "Terminal";

/// How long after its last output a session without status hooks still counts as active.
const ACTIVITY_WINDOW: Duration = Duration::from_secs(2);

/// Keys bound by the window root (focus traversal, copy) that terminal programs need: Tab for
/// completion, Shift+Tab for Claude Code's mode cycling, Ctrl+C for interrupt.
const PASSTHROUGH_KEYS: &[&str] = &["tab", "shift-tab", "ctrl-c"];

actions!(terminal, [Copy, Paste]);

pub fn init(cx: &mut App) {
    cx.bind_keys(
        PASSTHROUGH_KEYS
            .iter()
            .map(|key| KeyBinding::new(key, NoAction, Some(KEY_CONTEXT))),
    );
    cx.bind_keys(default_bindings());
}

/// Terminal shortcuts. Bindings match before `on_key_down`, so these keys never reach the
/// program. Kept as data so a user keymap can replace them later.
fn default_bindings() -> Vec<KeyBinding> {
    vec![
        KeyBinding::new("ctrl-shift-c", Copy, Some(KEY_CONTEXT)),
        KeyBinding::new("ctrl-insert", Copy, Some(KEY_CONTEXT)),
        KeyBinding::new("ctrl-shift-v", Paste, Some(KEY_CONTEXT)),
        KeyBinding::new("shift-insert", Paste, Some(KEY_CONTEXT)),
    ]
}

/// Cell geometry from the last paint, used to map pointer positions to grid cells.
#[derive(Clone, Copy, Debug)]
pub struct GridGeometry {
    pub origin: Point<Pixels>,
    pub cell_width: Pixels,
    pub line_height: Pixels,
    pub columns: usize,
    pub lines: usize,
    /// Top-left of the cursor cell, for placing IME candidate windows.
    pub cursor_origin: Point<Pixels>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum ProcessState {
    Running,
    /// The child exited, with its exit code when one is available.
    Exited(Option<i32>),
}

/// A command started at a shell's prompt (its `C` mark) that has not finished (`D` mark).
struct RunningCommand {
    /// The command line as the hover card shows it, when the shell reported it.
    label: Option<String>,
    started: Instant,
    /// Tells the sidebar once the command has run for [`RUNNING_AFTER`]; dropped with the
    /// command, so one that finishes sooner never shows.
    _reveal: Task<()>,
}

impl RunningCommand {
    fn badge(&self) -> RunningBadge {
        running_badge(self.started.elapsed())
    }
}

/// The key the shell integration scripts bind to their hidden cd (Ctrl+Alt+Shift+F12, which
/// PowerShell reads as a key chord).
const HIDDEN_CD_KEYS: &[u8] = b"\x1b[24;8~";

/// Emitted when what the sidebar and title bar show changes: title, status, activity, folder
/// or branch.
pub struct TabChanged;

/// Emitted when a Claude Code or Copilot CLI session's program exited: the tab goes on as a
/// shell.
pub struct AgentExited;

/// Emitted when a file path is Ctrl+clicked: the file is to be shown in the viewer.
pub struct OpenFileRequest {
    pub path: PathBuf,
    pub place: FilePlace,
}

/// One terminal session and its interaction state.
pub struct TerminalView {
    pub(crate) session: Session,
    /// Shown until the program sets a title.
    name: String,
    /// Folder the session was started in, used while the child's own cannot be read.
    launch_cwd: Option<PathBuf>,
    location: Option<Location>,
    /// How the program parses typed paths.
    shell: ShellSyntax,
    /// Status from Claude Code hooks, or from Copilot CLI's progress reports once it sent
    /// one; `None` for sessions without either.
    agent: Option<AgentState>,
    /// When the agent status last changed, for the hover card's lap time.
    agent_lap: Lap<AgentStatus>,
    /// The agent finished a turn while the tab was hidden; cleared when it is shown.
    finished_unseen: bool,
    /// The program rang the bell (BEL) while the tab was hidden; cleared when it is shown.
    bell_unseen: bool,
    /// Hidden views do not paint, so they acknowledge wakeups themselves.
    visible: bool,
    /// Runs while the session counts as active, to report when it goes quiet.
    activity_task: Option<Task<()>>,
    pub(crate) focus_handle: FocusHandle,
    pub(crate) geometry: Option<GridGeometry>,
    /// IME composition text not yet committed.
    pub(crate) marked_text: Option<String>,
    /// The rest of a history command matching the typed text, drawn dimmed after the cursor.
    pub(crate) ghost_text: Option<String>,
    pub(crate) history_popup: Option<HistoryPopup>,
    title: Option<String>,
    process: ProcessState,
    /// Set by the shell integration marks; agent sessions keep their hook status instead.
    running_command: Option<RunningCommand>,
    /// Where [`Self::change_dir`] leaves the folder for the shell's hidden cd key binding;
    /// `None` when the shell has none.
    cd_file: Option<PathBuf>,
    wheel: WheelScroll,
    selecting: bool,
    /// Button whose press was reported to the program; its release and drags are reported too.
    reported_button: Option<ReportButton>,
    /// Last cell a motion report was sent for, so motion is reported once per cell.
    last_motion_cell: Option<Cell>,
    pub(crate) scrollbar: ScrollbarState,
    /// The link under the pointer while Ctrl is held, drawn underlined with a hand cursor.
    pub(crate) hovered_link: Option<TerminalLink>,
    /// Where the pointer was when Ctrl was last seen held over the terminal, so new output
    /// under it is looked at again; `None` once Ctrl is released.
    link_pointer: Option<Point<Pixels>>,
    /// The cell (and scroll offset) `hovered_link` was last looked up for while Ctrl was held,
    /// so moving within a cell does not look again; cleared when output arrives.
    link_cell: Option<(Cell, usize)>,
    /// For links to `~/` paths.
    home: Option<PathBuf>,
    _session_events: Task<()>,
}

impl TerminalView {
    /// Start `spec` as session `id`. `reports_status` says whether the session was set up
    /// with Claude Code status hooks.
    pub fn spawn(
        id: u64,
        spec: &LaunchSpec,
        reports_status: bool,
        cx: &mut App,
    ) -> io::Result<Entity<Self>> {
        let (sender, mut receiver) = mpsc::unbounded();
        let callback: EventCallback = Arc::new(move |event| {
            let _ = sender.unbounded_send(event);
        });
        let session = Session::spawn(
            id,
            spec,
            TermDimensions::new(80, 24),
            Palette::default(),
            callback,
        )?;

        Ok(cx.new(|cx| {
            let session_events = cx.spawn(async move |this, cx| {
                while let Some(event) = receiver.next().await {
                    if this
                        .update(cx, |view: &mut Self, cx| view.on_session_event(event, cx))
                        .is_err()
                    {
                        break;
                    }
                }
            });
            Self::new(session, spec, reports_status, session_events, cx)
        }))
    }

    fn new(
        session: Session,
        spec: &LaunchSpec,
        reports_status: bool,
        session_events: Task<()>,
        cx: &mut Context<Self>,
    ) -> Self {
        Self {
            session,
            name: launcher::default_name(spec),
            launch_cwd: spec.cwd.clone(),
            location: spec.cwd.clone().map(Location::read),
            shell: launcher::shell_syntax(spec),
            agent: reports_status.then(AgentState::default),
            agent_lap: Lap::new(AgentState::default().status(), Instant::now()),
            finished_unseen: false,
            bell_unseen: false,
            visible: true,
            activity_task: None,
            focus_handle: cx.focus_handle(),
            geometry: None,
            marked_text: None,
            ghost_text: None,
            history_popup: None,
            title: None,
            process: ProcessState::Running,
            running_command: None,
            cd_file: spec
                .env
                .get(shell_integration::CD_FILE_ENV)
                .map(PathBuf::from),
            wheel: WheelScroll::default(),
            selecting: false,
            reported_button: None,
            last_motion_cell: None,
            scrollbar: ScrollbarState::default(),
            hovered_link: None,
            link_pointer: None,
            link_cell: None,
            home: std::env::home_dir(),
            _session_events: session_events,
        }
    }

    pub fn id(&self) -> u64 {
        self.session.id()
    }

    pub fn kind(&self) -> SessionKind {
        self.session.kind()
    }

    /// Whether the shell waits at its prompt: no command runs per the shell integration marks
    /// (always, without them) and the process is alive.
    pub fn at_prompt(&self) -> bool {
        self.running_command.is_none() && self.process == ProcessState::Running
    }

    /// Make the shell change to `folder`: through its hidden key binding when it has one and
    /// reports its prompt (the typed line is kept and the prompt redrawn), else by typing a
    /// `cd` line.
    pub fn change_dir(&mut self, folder: &Path, cx: &mut Context<Self>) -> Result<(), TypeError> {
        if let Some(file) = &self.cd_file
            && self.session.prompt_input().is_some()
        {
            let text = file_tree::typeable(folder)?;
            if fs::write(file, text).is_ok() {
                self.send_input(Cow::Borrowed(HIDDEN_CD_KEYS), cx);
                return Ok(());
            }
        }
        let command = cd_command(folder, self.shell)?;
        self.type_text(command, cx);
        Ok(())
    }

    /// Where the session works now, as of the last tab change.
    pub fn location(&self) -> Option<&Location> {
        self.location.as_ref()
    }

    /// The child's folder now, else the last one known, else the one it started in.
    pub fn current_folder(&self) -> Option<PathBuf> {
        self.session
            .current_dir()
            .or_else(|| self.location.as_ref().map(|location| location.dir.clone()))
            .or_else(|| self.launch_cwd.clone())
    }

    /// The program's title, or the session's default name.
    pub fn name(&self) -> &str {
        self.title.as_deref().unwrap_or(&self.name)
    }

    pub fn tab_status(&self) -> TabStatus {
        // A bell asks for the user like a waiting prompt, for programs without status hooks.
        if self.bell_unseen && self.process == ProcessState::Running {
            return TabStatus::NeedsAttention;
        }
        if let Some(agent) = self.agent {
            return match agent.status() {
                AgentStatus::Idle if self.finished_unseen => TabStatus::Done,
                AgentStatus::Idle => TabStatus::Idle,
                AgentStatus::Working => TabStatus::Working,
                AgentStatus::NeedsAttention => TabStatus::NeedsAttention,
                AgentStatus::Exited => TabStatus::Exited,
            };
        }
        match self.process {
            ProcessState::Exited(_) => TabStatus::Exited,
            ProcessState::Running
                if self
                    .running_command
                    .as_ref()
                    .is_some_and(|running| running.badge() != RunningBadge::Hidden) =>
            {
                TabStatus::Running
            }
            ProcessState::Running if self.activity_task.is_some() => TabStatus::Active,
            ProcessState::Running => TabStatus::Quiet,
        }
    }

    /// The running command's line for the hover card, while the status is
    /// [`TabStatus::Running`] and the shell reported it.
    pub fn running_label(&self) -> Option<&str> {
        if self.tab_status() != TabStatus::Running {
            return None;
        }
        self.running_command.as_ref()?.label.as_deref()
    }

    /// How long the running command has run (from its `C` mark), while the status is
    /// [`TabStatus::Running`].
    pub fn running_for(&self, now: Instant) -> Option<Duration> {
        if self.tab_status() != TabStatus::Running {
            return None;
        }
        let running = self.running_command.as_ref()?;
        Some(now.saturating_duration_since(running.started))
    }

    /// Whether the Running badge pulses: only for the first seconds of a command.
    pub fn running_pulses(&self) -> bool {
        self.tab_status() == TabStatus::Running
            && self
                .running_command
                .as_ref()
                .is_some_and(|running| running.badge() == RunningBadge::Pulsing)
    }

    /// How long the agent has been working on the current turn, or `None` when it is not
    /// working. Tool calls keep the lap running; waiting for a permission decision ends it.
    pub fn working_for(&self, now: Instant) -> Option<Duration> {
        let working = self.agent?.status() == AgentStatus::Working;
        working.then(|| self.agent_lap.elapsed(now))
    }

    /// Stop painting the view; only the selected tab is painted.
    pub fn hide(&mut self) {
        self.visible = false;
        self.history_popup = None;
        self.hovered_link = None;
        self.link_pointer = None;
        self.link_cell = None;
        // A wakeup sent before the next paint would never be acknowledged by `prepaint`.
        self.session.ack_wakeup();
    }

    /// Paint the view again, clearing a finished turn or a bell it showed while hidden.
    pub fn show(&mut self, cx: &mut Context<Self>) {
        self.visible = true;
        let was_done = std::mem::take(&mut self.finished_unseen);
        let rang = std::mem::take(&mut self.bell_unseen);
        if self.refresh_location() || was_done || rang {
            cx.emit(TabChanged);
        }
        cx.notify();
    }

    /// Re-read the child's folder and its git branch. Returns whether either changed. When the
    /// folder cannot be read (exited child, unsupported platform) the last known one is kept.
    fn refresh_location(&mut self) -> bool {
        let Some(dir) = self.current_folder() else {
            return false;
        };
        let location = Some(Location::read(dir));
        if location == self.location {
            return false;
        }
        self.location = location;
        true
    }

    /// Tell the sidebar something changed, refreshing the folder and branch first: a shell's
    /// title changes and output bursts end around `cd` and `git checkout`, and hook events
    /// follow Claude Code's commands, so the location is read then instead of polled.
    fn emit_tab_changed(&mut self, cx: &mut Context<Self>) {
        self.refresh_location();
        cx.emit(TabChanged);
    }

    /// Apply a status event from this session's Claude Code hooks.
    pub fn apply_hook_event(&mut self, event: HookEvent, at: u64, cx: &mut Context<Self>) {
        self.update_agent(|agent| agent.apply(event, at), cx);
    }

    /// Apply a progress report from Copilot CLI. Its first one switches the tab from output
    /// activity to agent status, so a Copilot that never reports progress keeps the former.
    fn apply_progress(&mut self, state: ProgressState, cx: &mut Context<Self>) {
        if self.kind() != SessionKind::CopilotCli {
            return;
        }
        let adopted = self.agent.is_none();
        if adopted {
            self.agent = Some(AgentState::default());
            self.activity_task = None;
        }
        let changed = self.update_agent(|agent| agent.apply_progress(state.is_shown()), cx);
        if adopted && !changed {
            cx.emit(TabChanged);
        }
    }

    /// Change the agent status and tell the sidebar when it changed. Returns whether it did.
    fn update_agent(
        &mut self,
        change: impl FnOnce(&mut AgentState),
        cx: &mut Context<Self>,
    ) -> bool {
        let Some(agent) = self.agent.as_mut() else {
            return false;
        };
        let before = agent.status();
        change(agent);
        let after = agent.status();
        if !self.agent_lap.set(after, Instant::now()) {
            return false;
        }
        self.finished_unseen = finished_unseen(before, after, self.visible);
        self.emit_tab_changed(cx);
        true
    }

    fn on_session_event(&mut self, event: SessionEvent, cx: &mut Context<Self>) {
        match event {
            SessionEvent::Wakeup => {
                self.track_activity(cx);
                // The text under a held Ctrl may have changed.
                if let Some(pointer) = self.link_pointer {
                    self.link_cell = self.link_key(pointer);
                    self.set_hovered_link(self.link_under(pointer), cx);
                }
                if !self.visible {
                    // Nothing paints, so re-arm here to keep hearing about output.
                    self.session.ack_wakeup();
                    return;
                }
            }
            SessionEvent::TitleChanged(title) => {
                self.title = title;
                self.emit_tab_changed(cx);
            }
            SessionEvent::Bell => {
                // Claude Code's hooks already tell when it finished or waits.
                let hooked = self.agent.is_some() && self.kind() == SessionKind::ClaudeCode;
                if self.visible || self.bell_unseen || hooked {
                    return;
                }
                self.bell_unseen = true;
                cx.emit(TabChanged);
            }
            SessionEvent::CommandStarted(command) => {
                let label =
                    command.and_then(|command| command_label(&command, COMMAND_LABEL_CHARS));
                match self.running_command.as_mut() {
                    // A second C mark before a D mark: the same run goes on, with the new line.
                    Some(running) => {
                        running.label = label;
                        if self.agent.is_none() {
                            cx.emit(TabChanged);
                        }
                    }
                    None => {
                        self.running_command = Some(RunningCommand {
                            label,
                            started: Instant::now(),
                            _reveal: self.reveal_running(cx),
                        });
                        // Not at the prompt any more: the explorer disables its cd entries.
                        cx.emit(TabChanged);
                    }
                }
            }
            SessionEvent::CommandExecuted(command) => {
                self.history_popup = None;
                history::record(command, cx);
            }
            SessionEvent::CommandFinished => {
                self.running_command = None;
                if self.agent.is_none() {
                    // The command may have changed the folder or the branch.
                    self.emit_tab_changed(cx);
                }
            }
            SessionEvent::ProgressChanged(state) => self.apply_progress(state, cx),
            SessionEvent::Exited(code) => {
                self.process = ProcessState::Exited(code);
                self.running_command = None;
                if let Some(agent) = self.agent.as_mut() {
                    agent.exit();
                    self.agent_lap.set(agent.status(), Instant::now());
                }
                cx.emit(TabChanged);
                // A failed agent keeps its last screen, so its error can be read.
                if self.kind() != SessionKind::Shell && code == Some(0) {
                    cx.emit(AgentExited);
                }
            }
        }
        cx.notify();
    }

    /// Tell the sidebar once the running command has run for [`RUNNING_AFTER`], when its status
    /// becomes Running.
    fn reveal_running(&self, cx: &mut Context<Self>) -> Task<()> {
        let started = Instant::now();
        cx.spawn(async move |this, cx| {
            cx.background_executor().timer(RUNNING_AFTER).await;
            // Should the timer fire a little early, the badge would stay hidden.
            let left = RUNNING_AFTER.saturating_sub(started.elapsed());
            if !left.is_zero() {
                cx.background_executor().timer(left).await;
            }
            let _ = this.update(cx, |view, cx| {
                if view.agent.is_none() {
                    cx.emit(TabChanged);
                }
            });
        })
    }

    /// Mark a session without status hooks active on output, and quiet again once no output
    /// arrived for [`ACTIVITY_WINDOW`].
    fn track_activity(&mut self, cx: &mut Context<Self>) {
        if self.agent.is_some() || self.activity_task.is_some() {
            return;
        }
        self.activity_task = Some(cx.spawn(async move |this, cx| {
            loop {
                let Ok(Some(last_output)) =
                    this.read_with(cx, |view, _| view.session.last_output())
                else {
                    return;
                };
                let quiet_at = last_output + ACTIVITY_WINDOW;
                let now = Instant::now();
                if now >= quiet_at {
                    break;
                }
                cx.background_executor().timer(quiet_at - now).await;
            }
            let _ = this.update(cx, |view, cx| {
                view.activity_task = None;
                view.emit_tab_changed(cx);
            });
        }));
        cx.emit(TabChanged);
    }

    /// Type `text` into the program as if the user had.
    pub fn type_text(&mut self, text: String, cx: &mut Context<Self>) {
        self.send_input(Cow::Owned(text.into_bytes()), cx);
    }

    /// Send user input to the program, snapping the view back to the live screen.
    fn send_input(&mut self, bytes: Cow<'static, [u8]>, cx: &mut Context<Self>) {
        self.session.scroll_display(Scroll::Bottom);
        self.session.clear_selection();
        self.session.write(bytes);
        cx.notify();
    }

    fn mode(&self) -> TermMode {
        self.session.mode()
    }

    fn on_copy(&mut self, _: &Copy, _: &mut Window, cx: &mut Context<Self>) {
        let text = self.session.selection_text();
        if let Some(text) = text.filter(|text| !text.is_empty()) {
            cx.write_to_clipboard(ClipboardItem::new_string(text));
        }
    }

    fn paste(&mut self, text: Option<String>, cx: &mut Context<Self>) {
        let Some(text) = text.filter(|text| !text.is_empty()) else {
            return;
        };
        let bytes = keys::paste_bytes(&text, self.mode());
        self.send_input(bytes.into(), cx);
    }

    fn on_key_down(&mut self, event: &KeyDownEvent, _: &mut Window, cx: &mut Context<Self>) {
        let consumed = self.handle_history_key(&event.keystroke, cx)
            || self.handle_scrollback_shortcut(&event.keystroke, cx)
            || self.forward_key(event, cx);
        if consumed {
            cx.stop_propagation();
        }
    }

    /// Shift+PageUp and Shift+PageDown move through the scrollback history.
    fn handle_scrollback_shortcut(
        &mut self,
        keystroke: &Keystroke,
        cx: &mut Context<Self>,
    ) -> bool {
        if !is_shift_only(&keystroke.modifiers) {
            return false;
        }
        let scroll = match keystroke.key.as_str() {
            "pageup" => Scroll::PageUp,
            "pagedown" => Scroll::PageDown,
            _ => return false,
        };
        self.scroll_scrollback(scroll, cx);
        true
    }

    /// Move through the scrollback on the user's request, showing the scroll bar.
    fn scroll_scrollback(&mut self, scroll: Scroll, cx: &mut Context<Self>) {
        self.session.scroll_display(scroll);
        self.reveal_scrollbar(cx);
        cx.notify();
    }

    /// Show the scroll bar until it has been left alone for [`scrollbar::LINGER`].
    fn reveal_scrollbar(&mut self, cx: &mut Context<Self>) {
        self.scrollbar.revealed = true;
        self.scrollbar.hide_task = Some(cx.spawn(async move |this, cx| {
            cx.background_executor().timer(scrollbar::LINGER).await;
            let _ = this.update(cx, |view, cx| {
                view.scrollbar.revealed = false;
                cx.notify();
            });
        }));
    }

    /// Drop hover and drag state once there is no history to scroll (e.g. on the alternate
    /// screen), since the bar and its listeners are gone.
    pub(crate) fn set_scrollable(&mut self, scrollable: bool, cx: &mut Context<Self>) {
        if !scrollable && self.scrollbar.is_active() {
            self.scrollbar.hovered = false;
            self.scrollbar.drag_grab = None;
            cx.notify();
        }
    }

    /// A left press on the track starts a thumb drag; off the thumb it first centers the thumb
    /// on the pointer. Returns whether the press was consumed.
    pub(crate) fn scrollbar_mouse_down(
        &mut self,
        event: &MouseDownEvent,
        layout: ScrollbarLayout,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> bool {
        if event.button != MouseButton::Left || !layout.track.contains(&event.position) {
            return false;
        }
        let Some((top, height)) = layout.thumb() else {
            return false;
        };
        window.focus(&self.focus_handle, cx);
        let y = f32::from(event.position.y - layout.track.origin.y);
        let grab = if (top..top + height).contains(&y) {
            y - top
        } else {
            height / 2.0
        };
        self.scrollbar.drag_grab = Some(grab);
        self.session
            .scroll_to(layout.offset_for_pointer(event.position.y, grab));
        cx.notify();
        true
    }

    /// Drags the thumb, or tracks hover over the track. Returns whether the move was consumed.
    pub(crate) fn scrollbar_mouse_move(
        &mut self,
        event: &MouseMoveEvent,
        layout: ScrollbarLayout,
        cx: &mut Context<Self>,
    ) -> bool {
        if let Some(grab) = self.scrollbar.drag_grab {
            self.session
                .scroll_to(layout.offset_for_pointer(event.position.y, grab));
            cx.notify();
            return true;
        }
        let hovered = layout.track.contains(&event.position);
        if hovered != self.scrollbar.hovered {
            self.scrollbar.hovered = hovered;
            if !hovered {
                self.reveal_scrollbar(cx);
            }
            cx.notify();
        }
        false
    }

    /// Ends a thumb drag. Returns whether the release was consumed.
    pub(crate) fn scrollbar_mouse_up(&mut self, cx: &mut Context<Self>) -> bool {
        if self.scrollbar.drag_grab.take().is_none() {
            return false;
        }
        self.reveal_scrollbar(cx);
        cx.notify();
        true
    }

    fn forward_key(&mut self, event: &KeyDownEvent, cx: &mut Context<Self>) -> bool {
        let keystroke = &event.keystroke;
        let Some(bytes) = keys::key_to_bytes(keystroke, event.prefer_character_input, self.mode())
        else {
            return false;
        };
        self.send_input(bytes, cx);
        true
    }

    fn on_paste(&mut self, _: &Paste, _: &mut Window, cx: &mut Context<Self>) {
        self.paste(cx.read_from_clipboard().and_then(|item| item.text()), cx);
    }

    fn on_scroll(&mut self, event: &ScrollWheelEvent, _: &mut Window, cx: &mut Context<Self>) {
        let Some(geometry) = self.geometry else {
            return;
        };
        let lines = match event.delta {
            ScrollDelta::Lines(delta) => notch_lines(delta.y, cfg!(target_os = "linux")),
            ScrollDelta::Pixels(delta) => f32::from(delta.y) / f32::from(geometry.line_height),
        };
        let whole = self.wheel.add(lines);
        if whole == 0 {
            return;
        }

        let Some((cell, _)) = self.viewport_cell(event.position) else {
            return;
        };
        match mouse::wheel_action(whole, cell, &event.modifiers, self.mode()) {
            WheelAction::Program(bytes) => self.session.write(bytes),
            WheelAction::Scrollback(lines) => self.scroll_scrollback(Scroll::Delta(lines), cx),
        }
        cx.notify();
        cx.stop_propagation();
    }

    /// The visible cell under `position`, clamped to the grid, and which half of it was hit.
    fn viewport_cell(&self, position: Point<Pixels>) -> Option<(Cell, Side)> {
        let geometry = self.geometry?;
        let cell_width = f32::from(geometry.cell_width);
        let line_height = f32::from(geometry.line_height);
        let x = f32::from(position.x - geometry.origin.x).max(0.0);
        let y = f32::from(position.y - geometry.origin.y).max(0.0);
        let column = ((x / cell_width) as usize).min(geometry.columns.saturating_sub(1));
        let line = ((y / line_height) as usize).min(geometry.lines.saturating_sub(1));
        let side = if x - column as f32 * cell_width < cell_width / 2.0 {
            Side::Left
        } else {
            Side::Right
        };
        Some((Cell { column, line }, side))
    }

    /// The visible cell under `position`, `None` outside the grid.
    fn cell_under(&self, position: Point<Pixels>) -> Option<Cell> {
        let geometry = self.geometry?;
        let x = f32::from(position.x - geometry.origin.x);
        let y = f32::from(position.y - geometry.origin.y);
        if x < 0.0 || y < 0.0 {
            return None;
        }
        let column = (x / f32::from(geometry.cell_width)) as usize;
        let line = (y / f32::from(geometry.line_height)) as usize;
        (column < geometry.columns && line < geometry.lines).then_some(Cell { column, line })
    }

    /// The link under `position`, if any.
    fn link_under(&self, position: Point<Pixels>) -> Option<TerminalLink> {
        let cell = self.cell_under(position)?;
        let folder = self.current_folder();
        terminal_links::link_at(&self.session, cell, folder.as_deref(), self.home.as_deref())
    }

    fn set_hovered_link(&mut self, link: Option<TerminalLink>, cx: &mut Context<Self>) {
        if link != self.hovered_link {
            self.hovered_link = link;
            cx.notify();
        }
    }

    /// Underline the link under the pointer while Ctrl is held (and nothing else is).
    pub(crate) fn update_link_hover(
        &mut self,
        position: Point<Pixels>,
        modifiers: &Modifiers,
        cx: &mut Context<Self>,
    ) {
        if !is_link_modifier(modifiers) || !self.visible {
            self.link_pointer = None;
            self.link_cell = None;
            self.set_hovered_link(None, cx);
            return;
        }
        let key = self.link_key(position);
        let unchanged = self.link_pointer.is_some() && key.is_some() && key == self.link_cell;
        self.link_pointer = Some(position);
        if unchanged {
            return;
        }
        self.link_cell = key;
        self.set_hovered_link(self.link_under(position), cx);
    }

    /// The cell under `position` with the scroll offset: the link there stays the same while
    /// these do and no output arrives.
    fn link_key(&self, position: Point<Pixels>) -> Option<(Cell, usize)> {
        let cell = self.cell_under(position)?;
        Some((cell, self.session.display_offset()))
    }

    fn on_modifiers_changed(
        &mut self,
        event: &ModifiersChangedEvent,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.update_link_hover(window.mouse_position(), &event.modifiers, cx);
    }

    /// Open a Ctrl+clicked link: a URL in the browser, a file in the viewer.
    fn open_link(&mut self, target: LinkTarget, cx: &mut Context<Self>) {
        match target {
            LinkTarget::Url(url) => cx.open_url(&url),
            LinkTarget::File { path, place } => cx.emit(OpenFileRequest { path, place }),
        }
    }

    fn grid_point(&self, position: Point<Pixels>) -> Option<(GridPoint, Side)> {
        let (cell, side) = self.viewport_cell(position)?;
        let display_offset = self.session.display_offset();
        Some((
            GridPoint::new(
                Line(cell.line as i32 - display_offset as i32),
                Column(cell.column),
            ),
            side,
        ))
    }

    fn send_mouse_report(
        &mut self,
        button: ReportButton,
        action: MouseAction,
        position: Point<Pixels>,
        modifiers: &Modifiers,
    ) {
        let Some((cell, _)) = self.viewport_cell(position) else {
            return;
        };
        if action == MouseAction::Motion {
            if self.last_motion_cell == Some(cell) {
                return;
            }
            self.last_motion_cell = Some(cell);
        }
        if let Some(bytes) = mouse::report(button, action, cell, modifiers, self.mode()) {
            self.session.write(bytes);
        }
    }

    fn on_mouse_down(
        &mut self,
        event: &MouseDownEvent,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        window.focus(&self.focus_handle, cx);
        self.close_history_popup(cx);
        // A link takes the click before the program's mouse reports and the selection.
        if event.button == MouseButton::Left
            && is_link_modifier(&event.modifiers)
            && let Some(link) = self.link_under(event.position)
        {
            self.hovered_link = None;
            self.link_cell = None;
            self.open_link(link.target, cx);
            cx.notify();
            return;
        }
        let Some(button) = ReportButton::from_gpui(event.button) else {
            return;
        };
        if mouse::is_reporting_to_program(self.mode(), &event.modifiers) {
            self.send_mouse_report(button, MouseAction::Press, event.position, &event.modifiers);
            self.reported_button = Some(button);
            return;
        }
        match event.button {
            MouseButton::Left => self.start_selection(event, cx),
            #[cfg(target_os = "linux")]
            MouseButton::Middle => {
                self.paste(cx.read_from_primary().and_then(|item| item.text()), cx);
            }
            _ => {}
        }
    }

    fn start_selection(&mut self, event: &MouseDownEvent, cx: &mut Context<Self>) {
        let Some((point, side)) = self.grid_point(event.position) else {
            return;
        };
        let selection_type = match event.click_count {
            2 => SelectionType::Semantic,
            3.. => SelectionType::Lines,
            _ => SelectionType::Simple,
        };
        self.session.start_selection(selection_type, point, side);
        self.selecting = true;
        cx.notify();
    }

    fn on_mouse_move(&mut self, event: &MouseMoveEvent, _: &mut Window, cx: &mut Context<Self>) {
        if self.reported_button.is_some()
            || mouse::is_reporting_to_program(self.mode(), &event.modifiers)
        {
            let button = self.reported_button.unwrap_or(ReportButton::None);
            self.send_mouse_report(
                button,
                MouseAction::Motion,
                event.position,
                &event.modifiers,
            );
            return;
        }
        if !self.selecting || event.pressed_button != Some(MouseButton::Left) {
            return;
        }
        let Some((point, side)) = self.grid_point(event.position) else {
            return;
        };
        self.session.update_selection(point, side);
        cx.notify();
    }

    fn on_mouse_up(&mut self, event: &MouseUpEvent, _: &mut Window, cx: &mut Context<Self>) {
        let button = ReportButton::from_gpui(event.button);
        if button.is_some() && button == self.reported_button {
            self.reported_button = None;
            self.send_mouse_report(
                button.unwrap_or(ReportButton::None),
                MouseAction::Release,
                event.position,
                &event.modifiers,
            );
            return;
        }
        if !self.selecting || event.button != MouseButton::Left {
            return;
        }
        self.selecting = false;
        let text = self.session.finish_selection();
        #[cfg(target_os = "linux")]
        if let Some(text) = text.filter(|text| !text.is_empty()) {
            cx.write_to_primary(ClipboardItem::new_string(text));
        }
        #[cfg(not(target_os = "linux"))]
        let _ = text;
        cx.notify();
    }
}

fn is_shift_only(modifiers: &Modifiers) -> bool {
    modifiers.shift && !modifiers.control && !modifiers.alt
}

/// Ctrl (Cmd on macOS) alone: held over a link, it shows and opens the link.
fn is_link_modifier(modifiers: &Modifiers) -> bool {
    modifiers.secondary() && !modifiers.shift && !modifiers.alt
}

impl EventEmitter<TabChanged> for TerminalView {}
impl EventEmitter<AgentExited> for TerminalView {}
impl EventEmitter<OpenFileRequest> for TerminalView {}

impl Focusable for TerminalView {
    fn focus_handle(&self, _: &App) -> FocusHandle {
        self.focus_handle.clone()
    }
}

impl Drop for TerminalView {
    fn drop(&mut self) {
        // Left behind when the shell never read it.
        if let Some(file) = &self.cd_file {
            let _ = fs::remove_file(file);
        }
    }
}

impl Render for TerminalView {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let background = self.session.palette().background;
        let view = div()
            .size_full()
            .bg(to_hsla(background))
            .key_context(KEY_CONTEXT)
            .track_focus(&self.focus_handle)
            .cursor(CursorStyle::IBeam)
            .on_action(cx.listener(Self::on_copy))
            .on_action(cx.listener(Self::on_paste))
            .on_key_down(cx.listener(Self::on_key_down))
            .on_modifiers_changed(cx.listener(Self::on_modifiers_changed))
            .on_scroll_wheel(cx.listener(Self::on_scroll))
            .on_any_mouse_down(cx.listener(Self::on_mouse_down))
            .on_mouse_move(cx.listener(Self::on_mouse_move))
            .on_mouse_up(MouseButton::Left, cx.listener(Self::on_mouse_up))
            .on_mouse_up(MouseButton::Middle, cx.listener(Self::on_mouse_up))
            .on_mouse_up(MouseButton::Right, cx.listener(Self::on_mouse_up))
            .on_mouse_up_out(MouseButton::Left, cx.listener(Self::on_mouse_up))
            .on_mouse_up_out(MouseButton::Middle, cx.listener(Self::on_mouse_up))
            .on_mouse_up_out(MouseButton::Right, cx.listener(Self::on_mouse_up));

        let history_popup = self.update_history(cx);
        let exit_banner = self.exit_banner(cx);
        // The banner sits below the grid, which shrinks by its height, so no output is hidden.
        view.relative()
            .flex()
            .flex_col()
            .child(TerminalElement::new(cx.entity(), self.focus_handle.clone()))
            .children(exit_banner)
            .children(history_popup)
    }
}

impl TerminalView {
    /// A bar below the terminal once the program exited, so a finished tab is told apart from
    /// a quiet one without looking at the sidebar.
    fn exit_banner(&self, cx: &App) -> Option<AnyElement> {
        let ProcessState::Exited(code) = self.process else {
            return None;
        };
        let message = match code {
            Some(code) => format!("Process exited with code {code}"),
            None => "Process exited".to_string(),
        };
        let theme = cx.theme();
        let banner = div()
            .flex_none()
            .flex()
            .items_center()
            .justify_between()
            .gap_3()
            .px_3()
            .py_1()
            .text_sm()
            .bg(theme.secondary)
            .border_t_1()
            .border_color(theme.border)
            .text_color(theme.secondary_foreground)
            .child(message)
            .child(
                div()
                    .text_xs()
                    .text_color(theme.muted_foreground)
                    .child("Ctrl+Shift+W closes the tab"),
            );
        Some(banner.into_any_element())
    }
}

/// IME support: committed text is sent to the program; composition text is shown at the cursor.
impl EntityInputHandler for TerminalView {
    fn text_for_range(
        &mut self,
        _: Range<usize>,
        _: &mut Option<Range<usize>>,
        _: &mut Window,
        _: &mut Context<Self>,
    ) -> Option<String> {
        None
    }

    fn selected_text_range(
        &mut self,
        _: bool,
        _: &mut Window,
        _: &mut Context<Self>,
    ) -> Option<UTF16Selection> {
        Some(UTF16Selection {
            range: 0..0,
            reversed: false,
        })
    }

    fn marked_text_range(&self, _: &mut Window, _: &mut Context<Self>) -> Option<Range<usize>> {
        self.marked_text
            .as_ref()
            .map(|text| 0..text.encode_utf16().count())
    }

    fn unmark_text(&mut self, _: &mut Window, cx: &mut Context<Self>) {
        self.marked_text = None;
        cx.notify();
    }

    fn replace_text_in_range(
        &mut self,
        _: Option<Range<usize>>,
        text: &str,
        _: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.marked_text = None;
        if !text.is_empty() {
            self.send_input(Cow::Owned(text.as_bytes().to_vec()), cx);
        }
    }

    fn replace_and_mark_text_in_range(
        &mut self,
        _: Option<Range<usize>>,
        text: &str,
        _: Option<Range<usize>>,
        _: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.marked_text = (!text.is_empty()).then(|| text.to_string());
        cx.notify();
    }

    fn bounds_for_range(
        &mut self,
        _: Range<usize>,
        _: Bounds<Pixels>,
        _: &mut Window,
        _: &mut Context<Self>,
    ) -> Option<Bounds<Pixels>> {
        let geometry = self.geometry?;
        Some(Bounds::new(
            geometry.cursor_origin,
            size(geometry.cell_width, geometry.line_height),
        ))
    }

    fn character_index_for_point(
        &mut self,
        _: Point<Pixels>,
        _: &mut Window,
        _: &mut Context<Self>,
    ) -> Option<usize> {
        None
    }
}

/// Whether a status change is a turn ending (Working -> Idle) on a tab the user is not looking
/// at, which the sidebar shows as Done until the tab is selected.
fn finished_unseen(before: AgentStatus, after: AgentStatus, visible: bool) -> bool {
    !visible && before == AgentStatus::Working && after == AgentStatus::Idle
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_a_hidden_finished_turn_is_done() {
        use AgentStatus::*;
        assert!(finished_unseen(Working, Idle, false));
        assert!(!finished_unseen(Working, Idle, true));
        assert!(!finished_unseen(NeedsAttention, Idle, false));
        assert!(!finished_unseen(Working, NeedsAttention, false));
        assert!(!finished_unseen(Working, Exited, false));
    }

    #[test]
    fn default_bindings_parse() {
        assert_eq!(default_bindings().len(), 4);
    }
}
