mod agent_tracker;
mod clipboard_notice;
mod link_hover;
mod pointer;
mod search;

use std::borrow::Cow;
use std::fs;
use std::io;
use std::ops::Range;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::{Duration, Instant};

use agent_status::{AgentEvent, AgentMode, AgentModeState, AgentState, AgentStatus, WaitReason};

use futures::StreamExt;
use futures::channel::mpsc;
use gpui_kit::component::ActiveTheme;
use gpui_kit::prelude::FluentBuilder;
use gpui_kit::{
    Action, AnyElement, App, AppContext, Bounds, ClipboardItem, Context, CursorStyle, Entity,
    EntityInputHandler, EventEmitter, FocusHandle, Focusable, InteractiveElement, IntoElement,
    KeyBinding, KeyDownEvent, Keystroke, Modifiers, MouseButton, MouseDownEvent, MouseMoveEvent,
    NoAction, ParentElement, Pixels, Point, Render, Styled, Task, UTF16Selection, Window, actions,
    div, size,
};
use term_core::alacritty_terminal::grid::Scroll;
use term_core::alacritty_terminal::term::TermMode;
use term_core::{
    EventCallback, ExecutedCommand, LaunchSpec, Palette, Session, SessionEvent, SessionKind,
    TermDimensions, TerminalSearch,
};
use workspace_model::file_tree::{self, ShellSyntax, TypeError, cd_command};
use workspace_model::{
    AgentActivity, BusyReason, COMMAND_LABEL_CHARS, ClipboardGuard, Location, ProgressState,
    RUNNING_AFTER, RunningBadge, Shortcut, TabActivity, WorkspaceFeatures, command_label,
    running_badge,
};

use self::agent_tracker::AgentTracker;
use self::link_hover::LinkHover;
use self::pointer::PointerState;
use crate::file_viewer::FilePlace;
use crate::history;
use crate::history_popup::HistoryPopup;
use crate::keymap;
use crate::keys;
use crate::launcher;
use crate::scrollbar::{self, ScrollbarLayout, ScrollbarState};
use crate::settings_file::{self, AppSettings};
use crate::shell_integration;
use crate::shortcuts::{self, ALLOW_CLIPBOARD_KEY, DENY_CLIPBOARD_KEY};
use crate::sidebar::TabStatus;
use crate::terminal_element::{TerminalElement, to_hsla};
use crate::workspace_view::CloseTab;

const KEY_CONTEXT: &str = "Terminal";

/// How long after its last output a session without status hooks still counts as active.
const ACTIVITY_WINDOW: Duration = Duration::from_secs(2);

/// Keys bound by the window root (focus traversal, copy) that terminal programs need: Tab for
/// completion, Shift+Tab for Claude Code's mode cycling, Ctrl+C for interrupt.
const PASSTHROUGH_KEYS: &[&str] = &["tab", "shift-tab", "ctrl-c"];

actions!(
    terminal,
    [
        Copy,
        CopyOrSend,
        Paste,
        PasteOrSend,
        Find,
        CloseFind,
        AllowClipboard,
        DenyClipboard
    ]
);

pub fn init(cx: &mut App) {
    cx.bind_keys(
        PASSTHROUGH_KEYS
            .iter()
            .map(|key| KeyBinding::new(key, NoAction, Some(KEY_CONTEXT))),
    );
    keymap::add_defaults(cx, default_bindings());
}

/// Terminal shortcuts. Bindings match before `on_key_down`, so these keys only reach the
/// program when their action propagates (`CopyOrSend`, `PasteOrSend` with nothing to do).
/// The settings file's `"keybindings"` change them (see `keymap`). The clipboard keys only
/// act while a write waits for an answer.
fn default_bindings() -> Vec<Shortcut> {
    let bind =
        |keys: &str, action: &dyn Action| Shortcut::new(keys, action.name(), Some(KEY_CONTEXT));
    vec![
        bind("ctrl-shift-c", &Copy),
        bind("ctrl-insert", &CopyOrSend),
        bind("ctrl-shift-v", &Paste),
        bind("shift-insert", &Paste),
        bind("ctrl-v", &PasteOrSend),
        bind("ctrl-shift-f", &Find),
        bind("escape", &CloseFind),
        bind(
            &format!("ctrl-shift-{ALLOW_CLIPBOARD_KEY}"),
            &AllowClipboard,
        ),
        bind(&format!("ctrl-shift-{DENY_CLIPBOARD_KEY}"), &DenyClipboard),
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

/// A background agent changed to a status the user should see.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AgentNotice {
    /// What the agent waits for, `None` when it did not say (a bell).
    NeedsAttention(Option<WaitReason>),
    Finished,
    Exited(Option<i32>),
}

impl AgentNotice {
    fn for_change(
        before: AgentStatus,
        after: AgentStatus,
        wait_reason: Option<WaitReason>,
    ) -> Option<Self> {
        if before == after {
            return None;
        }
        match after {
            AgentStatus::NeedsAttention => Some(Self::NeedsAttention(wait_reason)),
            AgentStatus::Idle if before == AgentStatus::Working => Some(Self::Finished),
            _ => None,
        }
    }
}

/// Emitted when a file path is Ctrl+clicked: the file is to be shown in the viewer.
pub struct OpenFileRequest {
    pub path: PathBuf,
    pub place: FilePlace,
}

pub struct LastCommandChanged;

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum StatusReporting {
    Hooks,
    OutputActivity,
}

const INITIAL_TERMINAL_COLUMNS: u16 = 80;
const INITIAL_TERMINAL_LINES: u16 = 24;

/// One terminal session and its interaction state.
pub struct TerminalView {
    pub(crate) session: Session,
    /// Shown until the program sets a title.
    name: String,
    /// Folder the session was started in, used while the child's own cannot be read.
    launch_folder: Option<PathBuf>,
    location: Option<Location>,
    /// How the program parses typed paths.
    pub(crate) shell: ShellSyntax,
    /// Status from Claude Code hooks, or from Copilot CLI's progress reports once it sent
    /// one; `None` for sessions without either.
    agent: Option<AgentTracker>,
    mode: AgentModeState,
    /// The agent finished a turn while the tab was hidden; cleared when it is shown.
    finished_unseen: bool,
    /// The program rang the bell (BEL) while the tab was hidden; cleared when it is shown.
    bell_unseen: bool,
    /// Clipboard writes (OSC 52) waiting for Allow or Deny, and "Always for this tab".
    clipboard: ClipboardGuard,
    /// Hidden views do not paint, so they acknowledge wakeups themselves.
    visible: bool,
    /// Runs while the session counts as active, to report when it goes quiet.
    activity_task: Option<Task<()>>,
    pub(crate) focus_handle: FocusHandle,
    pub(crate) geometry: Option<GridGeometry>,
    /// IME composition text not yet committed.
    pub(crate) marked_text: Option<String>,
    pub(crate) history_popup: Option<HistoryPopup>,
    pub(crate) last_command: workspace_model::LastCommand,
    last_command_supported: bool,
    /// How many "Clear saved session commands" requests this tab has handled.
    saved_cleared_seen: u32,
    /// The history switches last acted on; settings changes that leave them alone are ignored.
    workspace_features: WorkspaceFeatures,
    pub(crate) search: TerminalSearch,
    search_bar: Option<search::SearchBar>,
    title: Option<String>,
    process: ProcessState,
    /// Set by the shell integration marks; agent sessions keep their hook status instead.
    running_command: Option<RunningCommand>,
    /// Where [`Self::change_dir`] leaves the folder for the shell's hidden cd key binding;
    /// `None` when the shell has none.
    cd_file: Option<PathBuf>,
    pointer: PointerState,
    pub(crate) scrollbar: ScrollbarState,
    pub(crate) link_hover: LinkHover,
    /// For links to `~/` paths.
    home: Option<PathBuf>,
    _session_events: Task<()>,
}

impl TerminalView {
    /// Start `spec` as session `id`, tracking hook status when hooks were installed.
    pub fn spawn(
        id: u64,
        spec: &LaunchSpec,
        reporting: StatusReporting,
        cx: &mut App,
    ) -> io::Result<Entity<Self>> {
        let (sender, mut receiver) = mpsc::unbounded();
        let callback: EventCallback = Arc::new(move |event| {
            let _ = sender.unbounded_send(event);
        });
        let session = Session::spawn(
            id,
            spec,
            TermDimensions::new(INITIAL_TERMINAL_COLUMNS, INITIAL_TERMINAL_LINES),
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
            Self::new(session, spec, reporting, session_events, cx)
        }))
    }

    fn new(
        session: Session,
        spec: &LaunchSpec,
        reporting: StatusReporting,
        session_events: Task<()>,
        cx: &mut Context<Self>,
    ) -> Self {
        cx.observe_global::<history::History>(|view, cx| {
            if view.sync_last_command_with_history(cx) {
                cx.emit(LastCommandChanged);
            }
            view.refresh_history(cx);
            cx.notify();
        })
        .detach();
        // Turning a history or last-command switch off closes the popup and drops suggestions
        // at once.
        cx.observe_global::<AppSettings>(|view, cx| {
            let features = settings_file::workspace_features(cx);
            if features == view.workspace_features {
                return;
            }
            view.workspace_features = features;
            view.refresh_history(cx);
            cx.notify();
        })
        .detach();
        let saved_cleared_seen = cx
            .try_global::<history::History>()
            .map_or(0, history::History::saved_cleared);
        Self {
            saved_cleared_seen,
            workspace_features: settings_file::workspace_features(cx),
            session,
            name: launcher::default_name(spec),
            launch_folder: spec.cwd.clone(),
            location: spec.cwd.clone().map(Location::read),
            shell: launcher::shell_syntax(spec),
            agent: (reporting == StatusReporting::Hooks).then(|| AgentTracker::new(Instant::now())),
            mode: AgentModeState::default(),
            finished_unseen: false,
            bell_unseen: false,
            clipboard: ClipboardGuard::default(),
            visible: true,
            activity_task: None,
            focus_handle: cx.focus_handle(),
            geometry: None,
            marked_text: None,
            history_popup: None,
            last_command: workspace_model::LastCommand::default(),
            last_command_supported: spec.kind == SessionKind::Shell
                && spec.mark_token.is_some()
                && matches!(
                    launcher::shell_syntax(spec),
                    ShellSyntax::Posix | ShellSyntax::Wsl
                ),
            search: TerminalSearch::default(),
            search_bar: None,
            title: None,
            process: ProcessState::Running,
            running_command: None,
            cd_file: spec
                .env
                .get(shell_integration::CD_FILE_ENV)
                .map(PathBuf::from),
            pointer: PointerState::default(),
            scrollbar: ScrollbarState::default(),
            link_hover: LinkHover::default(),
            home: std::env::home_dir(),
            _session_events: session_events,
        }
    }

    #[cfg(test)]
    pub(crate) fn from_test_session(
        session: Session,
        spec: &LaunchSpec,
        cx: &mut Context<Self>,
    ) -> Self {
        Self::new(
            session,
            spec,
            StatusReporting::OutputActivity,
            Task::ready(()),
            cx,
        )
    }

    #[cfg(test)]
    pub(crate) fn dispatch_test_event(&mut self, event: SessionEvent, cx: &mut Context<Self>) {
        self.on_session_event(event, cx);
    }

    /// Count the (exited) test child as running again, so marks and hooks apply to it.
    #[cfg(test)]
    pub(crate) fn revive_for_test(&mut self) {
        self.process = ProcessState::Running;
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

    /// Why closing the tab would stop work (a running command, a working or waiting agent),
    /// or `None` when it can close without asking.
    pub fn busy_reason(&self) -> Option<BusyReason> {
        let agent = self.agent.as_ref().map(|agent| match agent.status() {
            AgentStatus::Working => AgentActivity::Working,
            AgentStatus::NeedsAttention => AgentActivity::Waiting,
            AgentStatus::Idle | AgentStatus::Exited => AgentActivity::Idle,
        });
        TabActivity {
            exited: self.process != ProcessState::Running,
            agent,
            command_running: self.running_command.is_some(),
            command: self
                .running_command
                .as_ref()
                .and_then(|running| running.label.clone()),
        }
        .busy_reason()
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
            .or_else(|| {
                self.location
                    .as_ref()
                    .map(|location| location.folder.clone())
            })
            .or_else(|| self.launch_folder.clone())
    }

    /// The program's title, or the session's default name.
    pub fn name(&self) -> &str {
        self.title.as_deref().unwrap_or(&self.name)
    }

    pub fn tab_status(&self) -> TabStatus {
        // A bell asks for the user like a waiting prompt, for programs without status hooks; so
        // does a clipboard write waiting for an answer.
        let asks = self.bell_unseen || self.clipboard.pending().is_some();
        if asks && self.process == ProcessState::Running {
            return TabStatus::NeedsAttention;
        }
        if let Some(agent) = &self.agent {
            return match agent.status() {
                AgentStatus::Idle if self.finished_unseen => TabStatus::ReadyForReview,
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
            // With hooks a Claude tab always has an agent: none means they are off.
            ProcessState::Running if self.kind() == SessionKind::ClaudeCode => {
                TabStatus::Unavailable
            }
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
        self.agent.as_ref()?.working_for(now)
    }

    /// Stop painting the view; only the selected tab is painted.
    pub fn hide(&mut self) {
        self.visible = false;
        self.history_popup = None;
        self.link_hover = LinkHover::default();
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
        self.refresh_history(cx);
        cx.notify();
    }

    /// Re-read the child's folder and its git branch. Returns whether either changed. When the
    /// folder cannot be read (exited child, unsupported platform) the last known one is kept.
    fn refresh_location(&mut self) -> bool {
        let Some(folder) = self.current_folder() else {
            return false;
        };
        let location = Some(Location::read(folder));
        if location == self.location {
            return false;
        }
        self.location = location;
        true
    }

    /// Tell the sidebar something changed, refreshing the folder and branch first: a shell's
    /// title changes and output bursts end around `cd` and `git checkout`, and hook events
    /// follow Claude Code's commands, so the location is read then instead of polled.
    fn refresh_location_and_notify(&mut self, cx: &mut Context<Self>) {
        self.refresh_location();
        cx.emit(TabChanged);
    }

    pub fn agent_mode(&self) -> Option<AgentMode> {
        self.mode.mode()
    }

    fn supports_last_command(&self) -> bool {
        self.last_command_supported
    }

    pub fn restore_last_command(&mut self, command: Option<String>, cx: &mut Context<Self>) {
        if self.supports_last_command() {
            self.last_command.restore(command);
            cx.notify();
        }
    }

    pub fn last_command(&self) -> Option<&str> {
        self.last_command.saved()
    }

    /// Mode reports must not change activity, lap time or the unseen-finished badge.
    pub fn apply_hook_event(&mut self, event: AgentEvent, at: u64, cx: &mut Context<Self>) {
        if self.process != ProcessState::Running {
            return;
        }
        match event {
            AgentEvent::Hook(event) if self.kind() == SessionKind::ClaudeCode => {
                self.update_agent(|agent| agent.apply(event, at), cx);
            }
            AgentEvent::Hook(event) if self.kind() == SessionKind::CopilotCli => {
                // Permission hooks can arrive before the first terminal progress report.
                if self.agent.is_none()
                    && matches!(event, agent_status::HookEvent::InputRequested(_))
                {
                    self.agent = Some(AgentTracker::new(Instant::now()));
                    self.activity_task = None;
                }
                self.update_agent(|agent| agent.apply_copilot_hook(event, at), cx);
            }
            AgentEvent::ModeChanged(mode)
                if self.kind() == SessionKind::CopilotCli && self.mode.apply(mode, at) =>
            {
                cx.emit(TabChanged);
            }
            _ => {}
        }
    }

    /// Apply a progress report from Copilot CLI. Its first one switches the tab from output
    /// activity to agent status, so a Copilot that never reports progress keeps the former.
    fn apply_progress(&mut self, state: ProgressState, at: u64, cx: &mut Context<Self>) {
        if self.kind() != SessionKind::CopilotCli {
            return;
        }
        let adopted = self.agent.is_none();
        if adopted {
            self.agent = Some(AgentTracker::new(Instant::now()));
            self.activity_task = None;
        }
        let changed = self.update_agent(|agent| agent.apply_progress(state, at), cx);
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
        let Some(change) = agent.update(change, Instant::now()) else {
            return false;
        };
        self.finished_unseen = finished_unseen(change.before, change.after, self.visible);
        if !self.visible
            && let Some(notice) =
                AgentNotice::for_change(change.before, change.after, agent.wait_reason())
        {
            cx.emit(notice);
        }
        self.refresh_location_and_notify(cx);
        true
    }

    fn on_session_event(&mut self, event: SessionEvent, cx: &mut Context<Self>) {
        match event {
            SessionEvent::Wakeup => {
                if !self.on_wakeup(cx) {
                    return;
                }
            }
            SessionEvent::TitleChanged(title) => {
                self.title = title;
                self.refresh_location_and_notify(cx);
            }
            SessionEvent::Bell => {
                if !self.on_bell(cx) {
                    return;
                }
            }
            SessionEvent::CommandStarted(command) => self.on_command_started(command, cx),
            SessionEvent::CommandExecuted(command) => {
                self.history_popup = None;
                self.record_executed(command, cx);
            }
            SessionEvent::CommandFinished => {
                self.running_command = None;
                if self.agent.is_none() {
                    // The command may have changed the folder or the branch.
                    self.refresh_location_and_notify(cx);
                }
            }
            SessionEvent::ProgressChanged { state, at } => self.apply_progress(state, at, cx),
            SessionEvent::ClipboardStore(clipboard, text) => {
                self.on_clipboard_store(clipboard, text, cx);
                return;
            }
            SessionEvent::Exited(code) => self.on_exited(code, cx),
        }
        self.refresh_history(cx);
        cx.notify();
    }

    fn on_wakeup(&mut self, cx: &mut Context<Self>) -> bool {
        self.search.invalidate();
        self.track_activity(cx);
        self.refresh_link_hover(cx);
        if !self.visible {
            // Nothing paints, so re-arm here to keep hearing about output.
            self.session.ack_wakeup();
            return false;
        }
        true
    }

    fn on_bell(&mut self, cx: &mut Context<Self>) -> bool {
        let hooked = self.agent.is_some() && self.kind() == SessionKind::ClaudeCode;
        if self.visible || self.bell_unseen || hooked {
            return false;
        }
        self.bell_unseen = true;
        if self.kind() != SessionKind::Shell {
            // A bell replaces the tab's toast: keep the reason a pending request gave.
            let wait_reason = self.agent.as_ref().and_then(AgentTracker::wait_reason);
            cx.emit(AgentNotice::NeedsAttention(wait_reason));
        }
        cx.emit(TabChanged);
        true
    }

    /// Command history off: nothing is collected or captured. "Remember last shell command" off
    /// (or "Restore previous tabs" off) only stops the capture.
    fn record_executed(&mut self, command: ExecutedCommand, cx: &mut Context<Self>) {
        let features = settings_file::workspace_features(cx);
        if !features.history().records() {
            self.last_command.dismiss();
            return;
        }
        if self.supports_last_command()
            && features.captures_last_command()
            && self.last_command.record(&command.command)
        {
            cx.emit(LastCommandChanged);
        }
        history::record(command, cx);
    }

    /// Apply a forgotten command and a "clear saved session commands" request to this tab's
    /// saved last command. Returns whether it changed.
    fn sync_last_command_with_history(&mut self, cx: &Context<Self>) -> bool {
        let history = cx.global::<history::History>();
        let forgotten = history
            .forgotten()
            .is_some_and(|command| self.last_command.forget(command));
        let cleared = history.saved_cleared() != self.saved_cleared_seen;
        self.saved_cleared_seen = history.saved_cleared();
        let cleared = cleared && self.last_command.clear();
        forgotten || cleared
    }

    fn on_command_started(&mut self, command: Option<String>, cx: &mut Context<Self>) {
        let label = command.and_then(|command| command_label(&command, COMMAND_LABEL_CHARS));
        if let Some(running) = self.running_command.as_mut() {
            running.label = label;
            if self.agent.is_none() {
                cx.emit(TabChanged);
            }
            return;
        }
        self.running_command = Some(RunningCommand {
            label,
            started: Instant::now(),
            _reveal: self.reveal_running(cx),
        });
        cx.emit(TabChanged);
    }

    fn on_exited(&mut self, code: Option<i32>, cx: &mut Context<Self>) {
        if self.kind() != SessionKind::Shell
            && !self.visible
            && (code != Some(0) || !self.finished_unseen)
        {
            cx.emit(AgentNotice::Exited(code));
        }
        self.process = ProcessState::Exited(code);
        self.running_command = None;
        // Nobody is left to ask on behalf of.
        self.clipboard.deny();
        if let Some(agent) = self.agent.as_mut() {
            agent.exit(Instant::now());
        }
        cx.emit(TabChanged);
        // A failed agent keeps its last screen, so its error can be read.
        if self.kind() != SessionKind::Shell && code == Some(0) {
            cx.emit(AgentExited);
        }
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
                view.refresh_location_and_notify(cx);
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
        self.last_command.dismiss();
        self.session.scroll_display(Scroll::Bottom);
        self.session.clear_selection();
        self.session.write(bytes);
        cx.notify();
    }

    fn mode(&self) -> TermMode {
        self.session.mode()
    }

    fn on_copy(&mut self, _: &Copy, _: &mut Window, cx: &mut Context<Self>) {
        self.copy_selection(cx);
    }

    /// Without a selection of ours, the key goes on to `on_key_down` and the program: programs
    /// with mouse reporting on (Copilot CLI) keep their own selection and copy it on Ctrl+Insert.
    /// Ctrl+Shift+C stays a plain `Copy`, since it would reach the program as `^C`.
    fn on_copy_or_send(&mut self, _: &CopyOrSend, _: &mut Window, cx: &mut Context<Self>) {
        if !self.copy_selection(cx) {
            cx.propagate();
        }
    }

    /// Copy the selection to the clipboard; false when there is nothing selected.
    fn copy_selection(&self, cx: &mut Context<Self>) -> bool {
        let Some(text) = self
            .session
            .selection_text()
            .filter(|text| !text.is_empty())
        else {
            return false;
        };
        cx.write_to_clipboard(ClipboardItem::new_string(text));
        true
    }

    /// Send text as a paste; false when there is no text to send.
    fn paste(&mut self, text: Option<String>, cx: &mut Context<Self>) -> bool {
        let Some(text) = text.filter(|text| !text.is_empty()) else {
            return false;
        };
        let bytes = keys::paste_bytes(&text, self.mode());
        self.send_input(bytes.into(), cx);
        true
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
        self.paste_clipboard(cx);
    }

    /// Without text on the clipboard, the key goes on to the program as `^V`: Claude Code then
    /// reads a copied image from the clipboard itself.
    fn on_paste_or_send(&mut self, _: &PasteOrSend, _: &mut Window, cx: &mut Context<Self>) {
        if !self.paste_clipboard(cx) {
            cx.propagate();
        }
    }

    fn paste_clipboard(&mut self, cx: &mut Context<Self>) -> bool {
        self.paste(cx.read_from_clipboard().and_then(|item| item.text()), cx)
    }
}

fn is_shift_only(modifiers: &Modifiers) -> bool {
    modifiers.shift && !modifiers.control && !modifiers.alt
}

impl EventEmitter<TabChanged> for TerminalView {}
impl EventEmitter<AgentExited> for TerminalView {}
impl EventEmitter<AgentNotice> for TerminalView {}
impl EventEmitter<OpenFileRequest> for TerminalView {}
impl EventEmitter<LastCommandChanged> for TerminalView {}

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
        // Below 100 % opacity the grid alone paints the translucent background: painted here
        // too, it would be twice as opaque.
        let opaque = settings_file::window_opacity(cx).is_opaque();
        let view = div()
            .size_full()
            .min_h_0()
            .when(opaque, |view| view.bg(to_hsla(background)))
            .track_focus(&self.focus_handle)
            .cursor(CursorStyle::IBeam)
            .on_action(cx.listener(Self::on_copy))
            .on_action(cx.listener(Self::on_copy_or_send))
            .on_action(cx.listener(Self::on_paste))
            .on_action(cx.listener(Self::on_paste_or_send))
            .on_action(cx.listener(Self::on_allow_clipboard))
            .on_action(cx.listener(Self::on_deny_clipboard))
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

        let history_popup = self.render_history_popup(cx);
        let exit_banner = self.exit_banner(cx);
        let search_bar = self.render_terminal_search(cx);
        let clipboard_banner = self.clipboard_banner(cx);
        // The banner sits below the grid, which shrinks by its height, so no output is hidden.
        div()
            .size_full()
            .key_context(KEY_CONTEXT)
            .on_action(cx.listener(Self::on_find))
            .on_action(cx.listener(Self::on_close_find))
            .flex()
            .flex_col()
            .children(clipboard_banner)
            .children(search_bar)
            .child(
                view.relative()
                    .flex()
                    .flex_col()
                    .child(TerminalElement::new(cx.entity(), self.focus_handle.clone()))
                    .children(exit_banner)
                    .children(history_popup),
            )
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
                    .child(shortcuts::with_shortcut("Close the tab", &CloseTab, cx)),
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
        self.last_command.dismiss();
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
/// at, which the sidebar shows as Ready for review until the tab is selected.
fn finished_unseen(before: AgentStatus, after: AgentStatus, visible: bool) -> bool {
    !visible && before == AgentStatus::Working && after == AgentStatus::Idle
}

#[cfg(test)]
mod tests;
