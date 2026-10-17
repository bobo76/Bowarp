use std::borrow::Cow;
use std::collections::HashMap;
use std::io;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex, OnceLock};
use std::thread::JoinHandle;
use std::time::{Duration, Instant};

use alacritty_terminal::event::{Event, EventListener, WindowSize};
use alacritty_terminal::event_loop::{EventLoop, EventLoopSender, Msg, State};
use alacritty_terminal::grid::{Dimensions, Scroll};
use alacritty_terminal::index::{Column, Line, Point, Side};
use alacritty_terminal::selection::{Selection, SelectionType};
use alacritty_terminal::sync::FairMutex;
use alacritty_terminal::term::{self, Term, TermMode};
use alacritty_terminal::tty;
use parking_lot::MutexGuard;

use crate::launch::{LaunchSpec, SessionKind};
use crate::palette::Palette;

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
    palette: Palette,
    sender: OnceLock<EventLoopSender>,
    size: Mutex<TermDimensions>,
    wakeup_pending: AtomicBool,
    started_at: Instant,
    /// Milliseconds after `started_at` of the latest output; 0 means no output yet.
    last_output_ms: AtomicU64,
    exited: AtomicBool,
    exit_code: Mutex<Option<i32>>,
}

impl Shared {
    fn record_output(&self) {
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
            Event::PtyWrite(text) => shared.write(Cow::Owned(text.into_bytes())),
            Event::TextAreaSizeRequest(format) => {
                let size = *shared.size.lock().unwrap();
                shared.write(Cow::Owned(format(size.window_size()).into_bytes()));
            }
            // Answered from the palette defaults: this runs under the terminal lock, so runtime
            // overrides in `Term::colors` cannot be read here.
            Event::ColorRequest(index, format) => {
                let color = shared.palette.default_color(index);
                shared.write(Cow::Owned(format(color).into_bytes()));
            }
            // Clipboard (OSC 52) requests are wired up together with the renderer.
            other => log::trace!("unhandled terminal event: {other:?}"),
        }
    }
}

/// A running child process attached to a pty and a terminal emulator.
pub struct Session {
    id: u64,
    kind: SessionKind,
    term: Arc<FairMutex<Term<SessionListener>>>,
    shared: Arc<Shared>,
    io_thread: Option<JoinHandle<(EventLoop<tty::Pty, SessionListener>, State)>>,
}

impl Session {
    pub fn spawn(
        id: u64,
        spec: &LaunchSpec,
        size: TermDimensions,
        palette: Palette,
        callback: EventCallback,
    ) -> io::Result<Self> {
        let shared = Arc::new(Shared {
            callback,
            palette,
            sender: OnceLock::new(),
            size: Mutex::new(size),
            wakeup_pending: AtomicBool::new(false),
            started_at: Instant::now(),
            last_output_ms: AtomicU64::new(0),
            exited: AtomicBool::new(false),
            exit_code: Mutex::new(None),
        });
        let listener = SessionListener(shared.clone());
        let term = Arc::new(FairMutex::new(Term::new(
            term::Config::default(),
            &size,
            listener.clone(),
        )));

        // Windows has an extra `escape_args` field; the struct update fills it there.
        #[allow(clippy::needless_update)]
        let options = tty::Options {
            shell: spec.shell_command(),
            working_directory: spec.cwd.clone(),
            drain_on_exit: true,
            env: session_env(&spec.env),
            ..Default::default()
        };
        let pty = tty::new(&options, size.window_size(), id)?;
        let event_loop = EventLoop::new(term.clone(), listener, pty, options.drain_on_exit, false)?;
        let _ = shared.sender.set(event_loop.channel());
        let io_thread = event_loop.spawn();

        Ok(Self {
            id,
            kind: spec.kind,
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

    pub fn palette(&self) -> &Palette {
        &self.shared.palette
    }

    /// Lock the terminal for reading or modifying its state. Keep the lock short; the I/O
    /// thread blocks on it while parsing output.
    pub fn lock_term(&self) -> MutexGuard<'_, Term<SessionListener>> {
        self.term.lock()
    }

    /// The terminal modes the program has set.
    pub fn mode(&self) -> TermMode {
        *self.term.lock().mode()
    }

    /// Move the visible screen through the scrollback history.
    pub fn scroll_display(&self, scroll: Scroll) {
        self.term.lock().scroll_display(scroll);
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
        self.shared.write(bytes.into());
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
        if let Some(io_thread) = self.io_thread.take() {
            let _ = io_thread.join();
        }
    }
}

/// Environment for every child: advertise a capable terminal without mutating our own
/// process environment (alacritty's `tty::setup_env` does that, which is not thread-safe).
fn session_env(extra: &HashMap<String, String>) -> HashMap<String, String> {
    let mut env = HashMap::from([
        ("TERM".to_string(), "xterm-256color".to_string()),
        ("COLORTERM".to_string(), "truecolor".to_string()),
        ("TERM_PROGRAM".to_string(), "agent-workspace".to_string()),
    ]);
    env.extend(
        extra
            .iter()
            .map(|(key, value)| (key.clone(), value.clone())),
    );
    env
}
