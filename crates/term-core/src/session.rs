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
use alacritty_terminal::grid::{Dimensions, Scroll};
use alacritty_terminal::index::{Column, Line, Point, Side};
use alacritty_terminal::selection::{Selection, SelectionType};
use alacritty_terminal::sync::FairMutex;
use alacritty_terminal::term::{self, ClipboardType, Term, TermMode};
use alacritty_terminal::tty;
use parking_lot::MutexGuard;
use workspace_model::ProgressState;

use crate::launch::LaunchSpec;
use crate::palette::Palette;
use crate::prompt::{ExecutedCommand, MarkedPty, PromptEvent, PromptInput, PromptTracker};
use workspace_model::{SessionKind, apply_environment};

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
    /// A shell with integration started a command, with its command line when the shell
    /// reported it.
    CommandStarted(Option<String>),
    /// A shell with integration is about to run this command line typed at its prompt.
    CommandExecuted(ExecutedCommand),
    /// The command a shell with integration ran finished: the prompt is back. Also sent when
    /// the shell reports a new folder at a prompt without a command before it.
    CommandFinished,
    /// The program reported progress (OSC 9;4), including repeated states.
    ProgressChanged {
        state: ProgressState,
        /// I/O observation time, in microseconds since the Unix epoch.
        at: u64,
    },
    /// The program wrote text to the clipboard or the primary selection (OSC 52).
    ClipboardStore(ClipboardType, String),
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
}

impl Shared {
    fn new(size: TermDimensions, palette: Palette, callback: EventCallback) -> Self {
        let prompt_callback = callback.clone();
        let prompt = Arc::new(PromptTracker::new(move |event| {
            prompt_callback(match event {
                PromptEvent::Started(command) => SessionEvent::CommandStarted(command),
                PromptEvent::Executed(command) => SessionEvent::CommandExecuted(command),
                PromptEvent::Finished => SessionEvent::CommandFinished,
            });
        }));
        Self {
            callback,
            palette: Mutex::new(palette),
            sender: OnceLock::new(),
            size: Mutex::new(size),
            wakeup_pending: AtomicBool::new(false),
            started_at: Instant::now(),
            last_output_ms: AtomicU64::new(0),
            exited: AtomicBool::new(false),
            exit_code: Mutex::new(None),
            prompt,
            inputs: AtomicU64::new(0),
            inputs_before_output: AtomicU64::new(0),
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
        let Some(sender) = self.sender.get() else {
            return;
        };
        if let Err(err) = sender.send(Msg::Input(bytes)) {
            log::debug!("pty write dropped: {err}");
        }
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
            Event::Exit => {
                shared.wake();
                shared.exited.store(true, Ordering::Release);
                let code = *shared.exit_code.lock().unwrap();
                (shared.callback)(SessionEvent::Exited(code));
            }
            Event::PtyWrite(text) => {
                if !shared.prompt.on_tag_reply(&text) {
                    shared.write(Cow::Owned(text.into_bytes()));
                }
            }
            Event::TextAreaSizeRequest(format) => {
                let size = *shared.size.lock().unwrap();
                shared.write(Cow::Owned(format(size.window_size()).into_bytes()));
            }
            // Answered from the palette defaults: this runs under the terminal lock, so runtime
            // overrides in `Term::colors` cannot be read here.
            Event::ColorRequest(index, format) => {
                let color = shared.palette.lock().unwrap().default_color(index);
                shared.write(Cow::Owned(format(color).into_bytes()));
            }
            Event::ClipboardStore(clipboard, text) => {
                (shared.callback)(SessionEvent::ClipboardStore(clipboard, text));
            }
            // Clipboard reads (OSC 52 `?`) are refused by `Osc52::OnlyCopy`, the default.
            other => log::trace!("unhandled terminal event: {other:?}"),
        }
    }
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
        let shared = Arc::new(Shared::new(size, palette, callback));
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
        // The pid of an exited child may already belong to another process.
        if self.has_exited() {
            return None;
        }
        self.shared.prompt.folder()
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

    pub fn clear_selection(&self) {
        self.term.lock().selection = None;
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
        if let Some(sender) = self.shared.sender.get() {
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
        let term = self.term.lock();
        let grid = term.grid();
        (0..grid.screen_lines())
            .map(|line| {
                let row = &grid[Line(line as i32)];
                let text: String = (0..grid.columns()).map(|col| row[Column(col)].c).collect();
                text.trim_end().to_string()
            })
            .collect()
    }
}

impl Drop for Session {
    fn drop(&mut self) {
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
