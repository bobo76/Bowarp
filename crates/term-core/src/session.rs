use std::borrow::Cow;
use std::collections::HashMap;
use std::io;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::mpsc::{self, RecvTimeoutError};
use std::sync::{Arc, Mutex, OnceLock};
use std::thread::{self, JoinHandle};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use alacritty_terminal::event::{Event, EventListener, WindowSize};
use alacritty_terminal::event_loop::{EventLoop, EventLoopSender, Msg, State};
use alacritty_terminal::grid::{Dimensions, Row, Scroll};
use alacritty_terminal::index::{Column, Line, Point, Side};
use alacritty_terminal::selection::{Selection, SelectionType};
use alacritty_terminal::sync::FairMutex;
use alacritty_terminal::term::cell::{Cell, Flags};
use alacritty_terminal::term::{self, ClipboardType, Term, TermMode};
use alacritty_terminal::tty;
use alacritty_terminal::vte::ansi::{ClearMode, Handler, Processor, StdSyncHandler};
use parking_lot::MutexGuard;
use workspace_model::ProgressState;

use crate::launch::LaunchSpec;
use crate::palette::Palette;
use crate::prompt::{
    ExecutedCommand, MarkedPty, OutputTap, PromptEvent, PromptInput, PromptTracker,
};
use workspace_model::{AgentId, SessionKind, apply_environment};

/// How long a closed session's child has to exit after SIGHUP before it is killed.
const EXIT_GRACE: Duration = Duration::from_secs(2);

/// Notifications delivered from the session's I/O thread.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum SessionEvent {
    /// New terminal content is available. Not repeated until [`Session::ack_wakeup`].
    Wakeup,
    /// The program set (`Some`) or reset (`None`) the terminal title.
    TitleChanged(Option<String>),
    Bell,
    /// The child process exited, with its exit code when one is available.
    Exited(Option<i32>),
    /// A remote session lost the connection to its server, for this reason: the program may
    /// still run there ([`Session::remote_id`]). Sent instead of [`SessionEvent::Exited`].
    Disconnected(String),
    /// A shell with integration started a command, with its command line when the shell
    /// reported it.
    CommandStarted(Option<String>),
    /// A shell with integration is about to run this command line typed at its prompt.
    CommandExecuted(ExecutedCommand),
    /// The command a shell with integration ran finished: the prompt is back, with the
    /// command's exit status when the shell sent it. Also sent when the shell reports a new
    /// folder at a prompt without a command before it.
    CommandFinished(Option<i32>),
    /// The running command is this agent, started with the tab's hooks (`N` mark).
    AgentStarted(AgentId),
    /// The agent the `N` mark named exited (`X` mark), with its exit status; the command it
    /// ran in may go on until `CommandFinished`.
    AgentFinished(Option<i32>),
    /// The program reported progress (OSC 9;4), including repeated states.
    ProgressChanged {
        state: ProgressState,
        /// I/O observation time, in microseconds since the Unix epoch.
        at: u64,
    },
    /// The program wrote text to the clipboard or the primary selection (OSC 52).
    ClipboardStore(ClipboardType, String),
    /// A remote session's program is in another folder on its host now
    /// ([`Session::remote_folder`]).
    RemoteFolderChanged,
}

/// Called on the session's I/O thread, sometimes while it holds the terminal lock: it must
/// not call [`Session::lock_term`] or anything else that locks the terminal.
pub type EventCallback = Arc<dyn Fn(SessionEvent) + Send + Sync>;

/// Terminal grid size in cells, plus the cell size in pixels reported to the pty.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct TermDimensions {
    pub columns: u16,
    pub lines: u16,
    pub cell_width: u16,
    pub cell_height: u16,
}

impl TermDimensions {
    pub fn new(columns: u16, lines: u16) -> Self {
        Self {
            columns,
            lines,
            cell_width: 1,
            cell_height: 1,
        }
    }

    fn window_size(self) -> WindowSize {
        WindowSize {
            num_lines: self.lines,
            num_cols: self.columns,
            cell_width: self.cell_width,
            cell_height: self.cell_height,
        }
    }
}

impl Dimensions for TermDimensions {
    fn total_lines(&self) -> usize {
        self.screen_lines()
    }

    fn screen_lines(&self) -> usize {
        self.lines as usize
    }

    fn columns(&self) -> usize {
        self.columns as usize
    }
}

/// State shared between the [`Session`] handle and its I/O thread.
struct Shared {
    callback: EventCallback,
    /// Replaced when the user picks other terminal colors (`Session::set_palette`).
    palette: Mutex<Palette>,
    sender: OnceLock<EventLoopSender>,
    /// Set for a session whose program runs elsewhere ([`Session::remote`]).
    link: Option<Box<dyn RemoteLink>>,
    size: Mutex<TermDimensions>,
    wakeup_pending: AtomicBool,
    started_at: Instant,
    /// Milliseconds after `started_at` of the latest output; 0 means no output yet.
    last_output_ms: AtomicU64,
    exited: AtomicBool,
    exit_code: Mutex<Option<i32>>,
    prompt: Arc<PromptTracker>,
    /// Writes to the program so far, and how many of them came before the latest output.
    inputs: AtomicU64,
    inputs_before_output: AtomicU64,
    /// A remote session's folder on its host, as its server last reported it.
    remote_folder: Mutex<Option<String>>,
}

impl Shared {
    fn new(
        size: TermDimensions,
        palette: Palette,
        callback: EventCallback,
        link: Option<Box<dyn RemoteLink>>,
    ) -> Self {
        let prompt_callback = callback.clone();
        let prompt = Arc::new(PromptTracker::new(move |event| {
            prompt_callback(match event {
                PromptEvent::Started(command) => SessionEvent::CommandStarted(command),
                PromptEvent::Executed(command) => SessionEvent::CommandExecuted(command),
                PromptEvent::Finished(exit) => SessionEvent::CommandFinished(exit),
                PromptEvent::AgentStarted(agent) => SessionEvent::AgentStarted(agent),
                PromptEvent::AgentFinished(exit) => SessionEvent::AgentFinished(exit),
            });
        }));
        Self {
            callback,
            palette: Mutex::new(palette),
            sender: OnceLock::new(),
            link,
            size: Mutex::new(size),
            wakeup_pending: AtomicBool::new(false),
            started_at: Instant::now(),
            last_output_ms: AtomicU64::new(0),
            exited: AtomicBool::new(false),
            exit_code: Mutex::new(None),
            prompt,
            inputs: AtomicU64::new(0),
            inputs_before_output: AtomicU64::new(0),
            remote_folder: Mutex::new(None),
        }
    }

    fn record_output(&self) {
        let inputs = self.inputs.load(Ordering::Acquire);
        self.inputs_before_output.store(inputs, Ordering::Release);
        let elapsed = self.started_at.elapsed().as_millis().max(1) as u64;
        self.last_output_ms.store(elapsed, Ordering::Relaxed);
    }

    fn wake(&self) {
        if !self.wakeup_pending.swap(true, Ordering::AcqRel) {
            (self.callback)(SessionEvent::Wakeup);
        }
    }

    fn write(&self, bytes: Cow<'static, [u8]>) {
        if let Some(link) = &self.link {
            link.write(bytes);
            return;
        }
        let Some(sender) = self.sender.get() else {
            return;
        };
        if let Err(err) = sender.send(Msg::Input(bytes)) {
            log::debug!("pty write dropped: {err}");
        }
    }

    /// The terminal's answer to a query in the output. A remote session's server already
    /// answered it from its own terminal.
    fn reply(&self, bytes: String) {
        if self.link.is_none() {
            self.write(Cow::Owned(bytes.into_bytes()));
        }
    }

    /// The program exited: report it after its output.
    fn exit(&self) {
        let code = *self.exit_code.lock().unwrap();
        self.end(SessionEvent::Exited(code));
    }

    /// No more output comes: report why after the output, `Exited` or `Disconnected`.
    fn end(&self, event: SessionEvent) {
        self.wake();
        (self.callback)(event);
        // After the callback: `has_exited()` then means no event is left to deliver.
        self.exited.store(true, Ordering::Release);
    }
}

/// Receives events from alacritty's I/O thread and forwards the ones the UI cares about.
#[derive(Clone)]
pub struct SessionListener(Arc<Shared>);

impl EventListener for SessionListener {
    fn send_event(&self, event: Event) {
        let shared = &self.0;
        match event {
            Event::Wakeup => {
                shared.record_output();
                shared.wake();
            }
            Event::Title(title) => (shared.callback)(SessionEvent::TitleChanged(Some(title))),
            Event::ResetTitle => (shared.callback)(SessionEvent::TitleChanged(None)),
            Event::Bell => (shared.callback)(SessionEvent::Bell),
            // `ChildExit` arrives before the remaining output is drained, and output read by
            // that drain is only announced by a `Wakeup` sent after `Exit`. Report the exit on
            // `Exit` and wake first, so the renderer sees all output before `Exited`.
            Event::ChildExit(status) => *shared.exit_code.lock().unwrap() = status.code(),
            Event::Exit => shared.exit(),
            Event::PtyWrite(text) => {
                if !shared.prompt.on_tag_reply(&text) {
                    shared.reply(text);
                }
            }
            Event::TextAreaSizeRequest(format) => {
                let size = *shared.size.lock().unwrap();
                shared.reply(format(size.window_size()));
            }
            // Answered from the palette defaults: this runs under the terminal lock, so runtime
            // overrides in `Term::colors` cannot be read here.
            Event::ColorRequest(index, format) => {
                let color = shared.palette.lock().unwrap().default_color(index);
                shared.reply(format(color));
            }
            Event::ClipboardStore(clipboard, text) => {
                (shared.callback)(SessionEvent::ClipboardStore(clipboard, text));
            }
            // Clipboard reads (OSC 52 `?`) are refused by `Osc52::OnlyCopy`, the default.
            other => log::trace!("unhandled terminal event: {other:?}"),
        }
    }
}

/// Where a remote session sends what the user does: the connection to the server that runs
/// its program. Called on the UI thread: it must not block on the network.
pub trait RemoteLink: Send + Sync {
    fn write(&self, bytes: Cow<'static, [u8]>);
    fn resize(&self, size: TermDimensions);
    /// The session was dropped (its tab closed): end the program.
    fn close(&self);
    /// The program's session id on its server.
    fn remote_id(&self) -> u64;
}

/// Brings a remote session's output and exit, from the connection to its server.
pub struct SessionFeed {
    term: Arc<FairMutex<Term<SessionListener>>>,
    parser: Mutex<Processor<StdSyncHandler>>,
    shared: Arc<Shared>,
}

impl SessionFeed {
    /// Show output of the program.
    pub fn output(&self, bytes: &[u8]) {
        if bytes.is_empty() {
            return;
        }
        {
            let mut parser = self.parser.lock().unwrap();
            let mut term = self.term.lock();
            parser.advance(&mut *term, bytes);
        }
        self.shared.record_output();
        self.shared.wake();
    }

    /// The server resized the program's pty (another client did): follow it.
    pub fn resized(&self, size: TermDimensions) {
        {
            let mut current = self.shared.size.lock().unwrap();
            if *current == size {
                return;
            }
            *current = size;
        }
        self.term.lock().resize(size);
        self.shared.wake();
    }

    /// The program exited (`None`: without a code). Only the first call to this or
    /// [`Self::disconnected`] counts.
    pub fn exited(&self, code: Option<i32>) {
        {
            let mut exit_code = self.shared.exit_code.lock().unwrap();
            if self.shared.exited.load(Ordering::Acquire) {
                return;
            }
            *exit_code = code;
        }
        self.shared.exit();
    }

    /// The connection to the server was lost: no more output or input, like an exit, but
    /// reported as [`SessionEvent::Disconnected`]. Only the first call to this or
    /// [`Self::exited`] counts.
    pub fn disconnected(&self, reason: String) {
        if self.shared.exited.load(Ordering::Acquire) {
            return;
        }
        self.shared.end(SessionEvent::Disconnected(reason));
    }

    pub fn has_exited(&self) -> bool {
        self.shared.exited.load(Ordering::Acquire)
    }

    /// The program is in `folder` on the server now: reported as
    /// [`SessionEvent::RemoteFolderChanged`] when it differs from the last one.
    pub fn folder_changed(&self, folder: String) {
        {
            let mut current = self.shared.remote_folder.lock().unwrap();
            if current.as_deref() == Some(folder.as_str()) {
                return;
            }
            *current = Some(folder);
        }
        (self.shared.callback)(SessionEvent::RemoteFolderChanged);
    }
}

/// Reads a session's current folder from any thread, e.g. from its output tap.
#[derive(Clone)]
pub struct FolderProbe(Arc<Shared>);

impl FolderProbe {
    /// As [`Session::current_dir`].
    pub fn folder(&self) -> Option<PathBuf> {
        current_dir(&self.0)
    }
}

fn current_dir(shared: &Shared) -> Option<PathBuf> {
    // The pid of an exited child may already belong to another process.
    if shared.exited.load(Ordering::Acquire) {
        return None;
    }
    shared.prompt.folder()
}

/// A running child process attached to a pty and a terminal emulator.
pub struct Session {
    id: u64,
    kind: SessionKind,
    /// Process id of the program started in the pty, when the platform reports one.
    child_pid: Option<u32>,
    term: Arc<FairMutex<Term<SessionListener>>>,
    shared: Arc<Shared>,
    io_thread: Option<JoinHandle<(EventLoop<SessionPty, SessionListener>, State)>>,
}

type SessionPty = MarkedPty<tty::Pty, SessionListener>;

impl Session {
    pub fn spawn(
        id: u64,
        spec: &LaunchSpec,
        size: TermDimensions,
        palette: Palette,
        callback: EventCallback,
    ) -> io::Result<Self> {
        Self::spawn_with_tap(id, spec, size, palette, callback, None)
    }

    /// Like [`Session::spawn`], and `tap` sees everything the program writes, as it is read
    /// from the pty: what a remote server streams to its clients.
    pub fn spawn_with_tap(
        id: u64,
        spec: &LaunchSpec,
        size: TermDimensions,
        palette: Palette,
        callback: EventCallback,
        tap: Option<OutputTap>,
    ) -> io::Result<Self> {
        let shared = Arc::new(Shared::new(size, palette, callback, None));
        let listener = SessionListener(shared.clone());
        let term = Arc::new(FairMutex::new(Term::new(
            term::Config::default(),
            &size,
            listener.clone(),
        )));

        let options = pty_options(spec);
        let pty = tty::new(&options, size.window_size(), id)?;
        let child_pid = child_pid(&pty);
        if let Some(pid) = child_pid {
            shared.prompt.set_child_pid(pid);
        }
        let progress_callback = shared.callback.clone();
        let pty = MarkedPty::new(
            pty,
            term.clone(),
            shared.prompt.clone(),
            spec.mark_token.as_deref(),
            Box::new(move |state| {
                let at = SystemTime::now()
                    .duration_since(UNIX_EPOCH)
                    .map_or(0, |elapsed| elapsed.as_micros() as u64);
                progress_callback(SessionEvent::ProgressChanged { state, at });
            }),
            tap,
        );
        let event_loop = EventLoop::new(term.clone(), listener, pty, options.drain_on_exit, false)?;
        let _ = shared.sender.set(event_loop.channel());
        let io_thread = event_loop.spawn();

        Ok(Self {
            id,
            kind: spec.kind,
            child_pid,
            term,
            shared,
            io_thread: Some(io_thread),
        })
    }

    /// A session whose program runs elsewhere, on a remote server: what the user types and
    /// resizes goes to `link`, and the returned [`SessionFeed`] brings the program's output
    /// and exit. Terminal queries in the output are not answered here: the server answers them.
    pub fn remote(
        id: u64,
        kind: SessionKind,
        size: TermDimensions,
        palette: Palette,
        callback: EventCallback,
        link: Box<dyn RemoteLink>,
    ) -> (Self, SessionFeed) {
        let shared = Arc::new(Shared::new(size, palette, callback, Some(link)));
        let term = Arc::new(FairMutex::new(Term::new(
            term::Config::default(),
            &size,
            SessionListener(shared.clone()),
        )));
        let feed = SessionFeed {
            term: term.clone(),
            parser: Mutex::new(Processor::new()),
            shared: shared.clone(),
        };
        let session = Self {
            id,
            kind,
            child_pid: None,
            term,
            shared,
            io_thread: None,
        };
        (session, feed)
    }

    pub fn id(&self) -> u64 {
        self.id
    }

    pub fn kind(&self) -> SessionKind {
        self.kind
    }

    /// The child's current working directory, which follows `cd` in a shell. `None` when it
    /// cannot be known: the child exited, or (off Linux) the shell has no integration script
    /// to report it.
    pub fn current_dir(&self) -> Option<PathBuf> {
        current_dir(&self.shared)
    }

    /// A handle reading [`Session::current_dir`] without the session.
    pub fn folder_probe(&self) -> FolderProbe {
        FolderProbe(self.shared.clone())
    }

    /// A remote session's folder on its host, as its server last reported it; `None` for a
    /// local session.
    pub fn remote_folder(&self) -> Option<String> {
        self.shared.remote_folder.lock().unwrap().clone()
    }

    /// A remote session's id on its server, to attach to it again; `None` for a local
    /// session.
    pub fn remote_id(&self) -> Option<u64> {
        self.shared.link.as_ref().map(|link| link.remote_id())
    }

    pub fn palette(&self) -> Palette {
        *self.shared.palette.lock().unwrap()
    }

    /// Paints the terminal with `palette` from the next frame on; colors a program set at
    /// runtime (OSC 4/10/11) still win. A program asking for a color gets the new one.
    pub fn set_palette(&self, palette: Palette) {
        *self.shared.palette.lock().unwrap() = palette;
    }

    /// Lock the terminal for reading or modifying its state. Keep the lock short; the I/O
    /// thread blocks on it while parsing output.
    pub fn lock_term(&self) -> MutexGuard<'_, Term<SessionListener>> {
        self.term.lock()
    }

    /// What the user typed at the shell's prompt, when the shell reports its prompt (shell
    /// integration) and is waiting at it on the normal screen.
    pub fn prompt_input(&self) -> Option<PromptInput> {
        let term = self.term.lock();
        self.shared.prompt.resolve(&term);
        if term.mode().contains(TermMode::ALT_SCREEN) {
            return None;
        }
        self.shared.prompt.input(&term)
    }

    /// The terminal modes the program has set.
    pub fn mode(&self) -> TermMode {
        *self.term.lock().mode()
    }

    /// Move the visible screen through the scrollback history.
    pub fn scroll_display(&self, scroll: Scroll) {
        self.term.lock().scroll_display(scroll);
    }

    /// Scroll the visible screen to `display_offset` lines back, clamped to the history.
    pub fn scroll_to(&self, display_offset: usize) {
        let mut term = self.term.lock();
        let delta = display_offset as i32 - term.grid().display_offset() as i32;
        if delta != 0 {
            term.scroll_display(Scroll::Delta(delta));
        }
    }

    /// How many lines the visible screen is scrolled back into history.
    pub fn display_offset(&self) -> usize {
        self.term.lock().grid().display_offset()
    }

    /// Replace any selection with a new one anchored at `point`.
    pub fn start_selection(&self, selection_type: SelectionType, point: Point, side: Side) {
        self.term.lock().selection = Some(Selection::new(selection_type, point, side));
    }

    /// Extend the current selection to `point`, if there is one.
    pub fn update_selection(&self, point: Point, side: Side) {
        if let Some(selection) = self.term.lock().selection.as_mut() {
            selection.update(point, side);
        }
    }

    /// End a selection gesture, dropping the selection if it covers nothing. Returns the
    /// selected text.
    pub fn finish_selection(&self) -> Option<String> {
        let mut term = self.term.lock();
        if term.selection.as_ref().is_some_and(Selection::is_empty) {
            term.selection = None;
        }
        term.selection_to_string()
    }

    pub fn selection_text(&self) -> Option<String> {
        self.term.lock().selection_to_string()
    }

    /// Whether a selection covers at least one cell, without building its text.
    pub fn has_selection(&self) -> bool {
        let term = self.term.lock();
        term.selection
            .as_ref()
            .is_some_and(|selection| !selection.is_empty())
    }

    pub fn clear_selection(&self) {
        self.term.lock().selection = None;
    }

    /// Select the scrollback and the screen down to its last line with text (or the cursor's,
    /// if lower): the empty lines below are left out. Returns the selected text.
    pub fn select_all(&self) -> Option<String> {
        let mut term = self.term.lock();
        let start = Point::new(term.topmost_line(), Column(0));
        let end = Point::new(last_used_line(&term), term.last_column());
        let mut selection = Selection::new(SelectionType::Simple, start, Side::Left);
        selection.update(end, Side::Right);
        term.selection = Some(selection);
        term.selection_to_string()
    }

    /// Drop the lines scrolled off the screen, as `ESC [ 3 J` does: the view goes back to the
    /// bottom and a selection in them goes.
    pub fn clear_scrollback(&self) {
        self.term.lock().clear_screen(ClearMode::Saved);
    }

    /// Re-arm wakeup notifications. Call before reading the terminal so output arriving
    /// during the read triggers a new [`SessionEvent::Wakeup`].
    pub fn ack_wakeup(&self) {
        self.shared.wakeup_pending.store(false, Ordering::Release);
    }

    pub fn write(&self, bytes: impl Into<Cow<'static, [u8]>>) {
        self.shared.inputs.fetch_add(1, Ordering::AcqRel);
        self.shared.write(bytes.into());
    }

    /// Whether input was written since the latest output: the screen may not show what was
    /// typed yet.
    pub fn awaiting_echo(&self) -> bool {
        self.shared.inputs.load(Ordering::Acquire)
            > self.shared.inputs_before_output.load(Ordering::Acquire)
    }

    pub fn resize(&self, size: TermDimensions) {
        {
            let mut current = self.shared.size.lock().unwrap();
            if *current == size {
                return;
            }
            *current = size;
        }
        self.term.lock().resize(size);
        if let Some(link) = &self.shared.link {
            link.resize(size);
        } else if let Some(sender) = self.shared.sender.get() {
            let _ = sender.send(Msg::Resize(size.window_size()));
        }
    }

    pub fn size(&self) -> TermDimensions {
        *self.shared.size.lock().unwrap()
    }

    /// Time of the most recent output, if any. This is terminal activity, not agent status.
    pub fn last_output(&self) -> Option<Instant> {
        let ms = self.shared.last_output_ms.load(Ordering::Relaxed);
        (ms != 0).then(|| self.shared.started_at + Duration::from_millis(ms))
    }

    pub fn has_exited(&self) -> bool {
        self.shared.exited.load(Ordering::Acquire)
    }

    pub fn exit_code(&self) -> Option<i32> {
        *self.shared.exit_code.lock().unwrap()
    }

    /// The visible screen as text, one string per line with trailing blanks trimmed.
    pub fn visible_text(&self) -> Vec<String> {
        screen_text(&self.term.lock())
    }
}

/// The visible screen as text, one string per line with trailing blanks trimmed.
pub(crate) fn screen_text<L>(term: &Term<L>) -> Vec<String> {
    let grid = term.grid();
    (0..grid.screen_lines() as i32)
        .map(|line| row_text(&grid[Line(line)]))
        .collect()
}

/// The scrollback (oldest first) when `scrollback` is set, then the screen, as text: one
/// string per row as in [`screen_text`], without the trailing empty rows. A wrapped line stays
/// one string per row.
pub(crate) fn term_text<L>(term: &Term<L>, scrollback: bool) -> Vec<String> {
    let grid = term.grid();
    let first = if scrollback {
        -(grid.history_size() as i32)
    } else {
        0
    };
    let mut lines: Vec<String> = (first..grid.screen_lines() as i32)
        .map(|line| row_text(&grid[Line(line)]))
        .collect();
    while lines.last().is_some_and(String::is_empty) {
        lines.pop();
    }
    lines
}

/// A row's characters with trailing blanks trimmed: a wide character once (not its spacer),
/// with its combining characters.
fn row_text(row: &Row<Cell>) -> String {
    let mut text = String::new();
    for column in 0..row.len() {
        let cell = &row[Column(column)];
        if cell
            .flags
            .intersects(Flags::WIDE_CHAR_SPACER | Flags::LEADING_WIDE_CHAR_SPACER)
        {
            continue;
        }
        text.push(cell.c);
        if let Some(extra) = cell.zerowidth() {
            text.extend(extra);
        }
    }
    text.truncate(text.trim_end().len());
    text
}

impl Drop for Session {
    fn drop(&mut self) {
        if let Some(link) = &self.shared.link {
            link.close();
        }
        if let Some(sender) = self.shared.sender.get() {
            let _ = sender.send(Msg::Shutdown);
        }
        let Some(io_thread) = self.io_thread.take() else {
            return;
        };
        // Dropping the pty sends SIGHUP and then waits for the child with no time limit, so reap
        // it off the caller's (UI) thread, and kill a child that ignores the hangup.
        let (reaped_tx, reaped_rx) = mpsc::channel::<()>();
        thread::spawn(move || {
            drop(io_thread.join());
            drop(reaped_tx);
        });
        let child_pid = self.child_pid;
        thread::spawn(move || {
            if reaped_rx.recv_timeout(EXIT_GRACE) == Err(RecvTimeoutError::Timeout) {
                kill_child(child_pid);
            }
        });
    }
}

#[cfg(unix)]
fn kill_child(pid: Option<u32>) {
    // A pid that doesn't fit would wrap into another process group (or -1: every process).
    let Some(group) = pid
        .and_then(|pid| libc::pid_t::try_from(pid).ok())
        .filter(|&pid| pid > 0)
    else {
        return;
    };
    log::warn!("child {group} ignored SIGHUP, killing its process group");
    // SAFETY: `kill` has no memory-safety preconditions. The child is a session leader
    // (alacritty `setsid`s it), so its pid is also its process group id; signalling the group
    // also ends the tools it started there. The reaper thread may reap the child between the
    // grace timeout and this call. The group id cannot be reused while any member lives, so
    // only once the whole group is gone could a new process group take that id, a narrow
    // residual race (a pidfd taken at spawn would close it).
    unsafe {
        libc::kill(-group, libc::SIGKILL);
    }
}

#[cfg(windows)]
fn kill_child(_pid: Option<u32>) {}

#[cfg(unix)]
fn child_pid(pty: &tty::Pty) -> Option<u32> {
    Some(pty.child().id())
}

#[cfg(windows)]
fn child_pid(pty: &tty::Pty) -> Option<u32> {
    pty.child_watcher().pid().map(|pid| pid.get())
}

fn pty_options(spec: &LaunchSpec) -> tty::Options {
    // Off Windows the struct update has nothing left to fill.
    #[allow(clippy::needless_update)]
    tty::Options {
        shell: spec.shell_command(),
        working_directory: spec.cwd.clone(),
        drain_on_exit: true,
        env: session_env(spec.kind, &spec.env),
        #[cfg(windows)]
        escape_args: true,
        ..Default::default()
    }
}

/// Environment for every child: advertise a capable terminal without mutating our own
/// process environment (alacritty's `tty::setup_env` does that, which is not thread-safe).
fn session_env(kind: SessionKind, extra: &HashMap<String, String>) -> HashMap<String, String> {
    let mut env = HashMap::from([
        ("TERM".to_string(), "xterm-256color".to_string()),
        ("COLORTERM".to_string(), "truecolor".to_string()),
        ("TERM_PROGRAM".to_string(), "agent-workspace".to_string()),
    ]);
    if let Some(agent) = kind.agent() {
        apply_environment(agent, &mut env);
    }
    env.extend(
        extra
            .iter()
            .map(|(key, value)| (key.clone(), value.clone())),
    );
    env
}

/// The lowest line with something on it, or the cursor's line if that is lower.
fn last_used_line<L: EventListener>(term: &Term<L>) -> Line {
    let grid = term.grid();
    let cursor = grid.cursor.point.line;
    let mut line = term.bottommost_line();
    while line > cursor && grid[line].is_clear() {
        line -= 1;
    }
    line
}

#[cfg(test)]
mod remote_tests {
    use super::*;

    /// What the session sent to its server.
    #[derive(Default)]
    struct FakeLink(Arc<Mutex<Vec<String>>>);

    impl RemoteLink for FakeLink {
        fn write(&self, bytes: Cow<'static, [u8]>) {
            let text = String::from_utf8_lossy(&bytes).into_owned();
            self.0.lock().unwrap().push(format!("write {text}"));
        }

        fn resize(&self, size: TermDimensions) {
            let entry = format!("resize {}x{}", size.columns, size.lines);
            self.0.lock().unwrap().push(entry);
        }

        fn close(&self) {
            self.0.lock().unwrap().push("close".into());
        }

        fn remote_id(&self) -> u64 {
            42
        }
    }

    #[test]
    fn a_remote_session_shows_its_feed_and_sends_input_to_its_link() {
        let sent = Arc::new(Mutex::new(Vec::new()));
        let events = Arc::new(Mutex::new(Vec::new()));
        let seen = events.clone();
        let (session, feed) = Session::remote(
            7,
            SessionKind::Shell,
            TermDimensions::new(20, 3),
            Palette::default(),
            Arc::new(move |event| seen.lock().unwrap().push(event)),
            Box::new(FakeLink(sent.clone())),
        );
        assert_eq!(session.id(), 7);
        // A device attributes query: the server answers it, not this terminal.
        feed.output(b"hello\x1b[c\r\nworld");
        assert_eq!(session.visible_text(), ["hello", "world", ""]);
        assert!(session.last_output().is_some());
        session.write(b"ls\r".as_slice());
        session.resize(TermDimensions::new(30, 4));
        // The server resizing it (for another client) is not sent back.
        feed.resized(TermDimensions::new(25, 5));
        assert_eq!(session.size(), TermDimensions::new(25, 5));
        assert_eq!(session.lock_term().columns(), 25);
        assert_eq!(*sent.lock().unwrap(), ["write ls\r", "resize 30x4"]);
        assert!(!session.has_exited());
        feed.exited(Some(2));
        feed.exited(None);
        assert!(session.has_exited());
        assert_eq!(session.exit_code(), Some(2));
        assert_eq!(session.current_dir(), None);
        let exits = events
            .lock()
            .unwrap()
            .iter()
            .filter(|event| matches!(event, SessionEvent::Exited(_)))
            .count();
        assert_eq!(exits, 1);
        drop(session);
        assert_eq!(sent.lock().unwrap().last().unwrap(), "close");
    }

    fn fed_session(output: &[u8]) -> Session {
        let (session, feed) = Session::remote(
            1,
            SessionKind::Shell,
            TermDimensions::new(10, 3),
            Palette::default(),
            Arc::new(|_| {}),
            Box::new(FakeLink::default()),
        );
        feed.output(output);
        session
    }

    #[test]
    fn select_all_takes_the_scrollback_and_the_screen() {
        let session = fed_session(b"1\r\n2\r\n3\r\n4\r\n5");
        assert_eq!(session.select_all().as_deref(), Some("1\n2\n3\n4\n5"));
        assert_eq!(session.selection_text().as_deref(), Some("1\n2\n3\n4\n5"));
        assert!(session.has_selection());
        session.clear_selection();
        assert!(!session.has_selection());
    }

    #[test]
    fn select_all_stops_at_the_last_line_with_text_or_the_cursor() {
        let session = fed_session(b"1\r\n2\x1b[1;1H");
        assert_eq!(session.select_all().as_deref(), Some("1\n2"));
        let session = fed_session(b"1\r\n\r\n");
        assert_eq!(session.select_all().as_deref(), Some("1\n\n"));
    }

    #[test]
    fn clearing_the_scrollback_keeps_the_screen_and_scrolls_to_the_bottom() {
        let session = fed_session(b"1\r\n2\r\n3\r\n4\r\n5");
        session.scroll_display(Scroll::Top);
        assert_eq!(session.display_offset(), 2);
        session.select_all();
        session.clear_scrollback();
        assert_eq!(session.display_offset(), 0);
        assert_eq!(session.lock_term().history_size(), 0);
        assert_eq!(session.visible_text(), ["3", "4", "5"]);
        // The selection began in the lines that went.
        assert_eq!(session.selection_text(), None);
        assert_eq!(session.select_all().as_deref(), Some("3\n4\n5"));
    }

    #[test]
    fn a_lost_connection_ends_the_session_as_disconnected_not_exited() {
        let events = Arc::new(Mutex::new(Vec::new()));
        let seen = events.clone();
        let (session, feed) = Session::remote(
            1,
            SessionKind::Shell,
            TermDimensions::new(20, 3),
            Palette::default(),
            Arc::new(move |event| seen.lock().unwrap().push(event)),
            Box::new(FakeLink::default()),
        );
        assert_eq!(session.remote_id(), Some(42));
        feed.output(b"hi");
        feed.disconnected("ssh died".into());
        feed.disconnected("again".into());
        feed.exited(Some(0));
        assert!(session.has_exited());
        assert_eq!(session.exit_code(), None);
        let ends: Vec<_> = events
            .lock()
            .unwrap()
            .iter()
            .filter(|event| {
                matches!(
                    event,
                    SessionEvent::Exited(_) | SessionEvent::Disconnected(_)
                )
            })
            .cloned()
            .collect();
        assert_eq!(ends, [SessionEvent::Disconnected("ssh died".into())]);
    }
}

#[cfg(all(test, unix))]
mod tests {
    use super::*;
    use workspace_model::agents::{CLAUDE, COPILOT};

    #[test]
    fn only_copilot_sessions_get_the_progress_report_variable() {
        let copilot = session_env(SessionKind::Agent(COPILOT), &HashMap::new());
        assert_eq!(copilot.get("ConEmuANSI").map(String::as_str), Some("ON"));
        for kind in [SessionKind::Shell, SessionKind::Agent(CLAUDE)] {
            assert!(!session_env(kind, &HashMap::new()).contains_key("ConEmuANSI"));
        }
    }

    /// The real event loop over a real pty: output held back after a mark is read again once
    /// the pty has nothing new (the wake socket must be polled under alacritty's pty key).
    #[test]
    fn output_after_a_mark_reaches_the_terminal() {
        let script = r"printf '\033]133;A;aw=tok\007$ \033]133;B;aw=tok\007typed'; sleep 3";
        let mut spec = LaunchSpec::new(
            SessionKind::Shell,
            Some("sh".into()),
            vec!["-c".into(), script.into()],
            None,
        );
        spec.mark_token = Some("tok".into());
        let size = TermDimensions::new(40, 5);
        let session = Session::spawn(1, &spec, size, Palette::default(), Arc::new(|_| {})).unwrap();
        let deadline = Instant::now() + Duration::from_secs(5);
        loop {
            if session
                .prompt_input()
                .is_some_and(|input| input.text == "typed")
            {
                break;
            }
            assert!(
                Instant::now() < deadline,
                "output after the mark never arrived"
            );
            thread::sleep(Duration::from_millis(20));
        }
    }
}
