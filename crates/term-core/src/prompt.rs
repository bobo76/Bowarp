//! Shell integration: where the prompt's command line starts and which commands run, from the
//! OSC 133 marks the injected shell scripts print.
//!
//! `alacritty_terminal` ignores OSC 133, and its parser runs on the I/O thread in batches, so
//! the grid position of a mark is caught like this: [`MarkedPty`] wraps the pty and reads
//! through it. When it finds a mark it ends the bytes it hands to the parser right after the
//! mark, followed by a DECRQM query with a tag number (`CSI <tag> $ p`). Parsing that query makes
//! the terminal answer with a `PtyWrite` event, which the session's listener hands to
//! [`PromptTracker::on_tag_reply`] instead of the pty: the mark has been parsed. Until the mark
//! is resolved the wrapper reports `WouldBlock`, so nothing after it is parsed; the terminal
//! state is then exactly the one at the mark, and whoever gets the terminal lock first (the
//! I/O thread on its next read, or the UI) records the cursor position.
//!
//! Only the `A` and `B` marks need a position. The command that runs comes with its `C` mark,
//! and zsh reports the command line as it is edited (`L`); for bash it is read from the grid,
//! from the `B` mark to the cursor. Marks inside a synchronized update are not held: the
//! parser keeps those bytes back until the update ends, so that prompt is not tracked.
//!
//! While output is held back the event loop must be woken to read it again ([`Wake`]): on Unix
//! a socket registered under the pty's key stays readable, on Windows a completion packet for
//! the pty's key is posted to the event loop's poller, the way alacritty's own pipe reader
//! wakes it.

#[cfg(windows)]
use polling::os::iocp::{CompletionPacket, PollerIocpExt};
#[cfg(unix)]
use std::io::Write;
use std::io::{self, ErrorKind, Read};
#[cfg(unix)]
use std::os::unix::net::UnixStream;
use std::path::PathBuf;
use std::sync::{Arc, Mutex, MutexGuard, OnceLock};
use std::thread;
use std::time::{Duration, Instant};

use alacritty_terminal::event::{EventListener, OnResize, WindowSize};
use alacritty_terminal::grid::Dimensions;
use alacritty_terminal::index::{Column, Line, Point};
use alacritty_terminal::sync::FairMutex;
use alacritty_terminal::term::Term;
use alacritty_terminal::term::cell::Flags;
use alacritty_terminal::tty::{ChildEvent, EventedPty, EventedReadWrite};
use polling::{Event as PollingEvent, PollMode, Poller};
use workspace_model::{CommandLine, Osc133Scanner, ProgressScanner, ProgressState, PromptMark};

/// DECRQM mode numbers used as tags: `TAG_BASE..TAG_BASE + TAG_COUNT`, far from real modes.
const TAG_BASE: u16 = 61_000;
const TAG_COUNT: u16 = 1_000;
/// A mark not parsed this long after it was handed over is dropped, so output never stalls
/// on it.
const MARK_TIMEOUT: Duration = Duration::from_millis(200);
/// How long a read waits before reporting `WouldBlock` while the parser has not reached a
/// handed-over mark yet, so the I/O thread doesn't spin meanwhile.
const PARSE_WAIT: Duration = Duration::from_millis(1);
const READ_CHUNK: usize = 64 * 1024;
/// A real pty's output is held after `A` and `B` marks only where the event loop can be woken
/// to read the held output (`Wake`).
const HOLDS_PROMPTS: bool = cfg!(any(unix, windows));
/// Ctrl+E (end of line) in bash, zsh and PowerShell's emacs mode.
const END_OF_LINE: char = '\x05';
const BACKSPACE: char = '\x7f';

/// A command line the shell is about to run.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ExecutedCommand {
    pub command: String,
    /// The shell's folder when it ran, when it could be read.
    pub folder: Option<PathBuf>,
}

/// What the tracker reports about the commands the shell runs.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum PromptEvent {
    /// A command started (`C` mark), with its command line when the shell reported it.
    Started(Option<String>),
    /// A command line typed at a tracked prompt is about to run; sent after its `Started`.
    Executed(ExecutedCommand),
    /// The running command finished (`D` mark): the shell is back at its prompt. Also sent
    /// for a `D` mark that reports a new folder without a command before it.
    Finished,
}

/// What the user typed at the prompt, as the shell reported it (zsh) or read from the grid.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PromptInput {
    /// From the command line's start to the cursor.
    pub before_cursor: String,
    /// The whole command line; read from the grid, trailing blanks are trimmed.
    pub text: String,
    /// Nothing but blanks after the cursor, and no text the shell shows there itself.
    pub at_end: bool,
    /// Grid line where the prompt starts (the `A` mark, else the command line's own line).
    pub prompt_line: i32,
}

impl PromptInput {
    /// The keys that replace the command line with `command`: end of line, one Backspace per
    /// typed character, then `command`.
    pub fn replace_keys(&self, command: &str) -> String {
        // `text` read from the grid drops trailing blanks; the cursor may sit after typed
        // spaces.
        let typed = self
            .text
            .chars()
            .count()
            .max(self.before_cursor.chars().count());
        let mut keys = String::from(END_OF_LINE);
        keys.extend(std::iter::repeat_n(BACKSPACE, typed));
        keys.push_str(command);
        keys
    }
}

/// Where a mark was: the cursor then, the scrollback size then, and the text left of the
/// cursor on that line, to find the line again after the screen scrolled.
#[derive(Clone, Debug, PartialEq, Eq)]
struct Anchor {
    line: i32,
    column: usize,
    history: usize,
    prefix: String,
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
enum Phase {
    /// No mark seen: the shell has no integration.
    #[default]
    Unknown,
    /// Between a command's end and the next command line.
    Idle,
    /// Editing a command line.
    Typing {
        prompt: Option<Anchor>,
        start: Anchor,
        /// The shell's folder while it waits at this prompt.
        folder: Option<PathBuf>,
    },
    Running,
}

#[derive(Debug)]
struct Pending {
    mark: PromptMark,
    tag: u16,
    parsed: bool,
    /// When the parser was handed the tag.
    handed_over: Option<Instant>,
}

#[derive(Debug, Default)]
struct TrackerState {
    phase: Phase,
    /// The `A` mark of the prompt being printed, until its `B` mark.
    prompt: Option<Anchor>,
    pending: Option<Pending>,
    next_tag: u16,
    /// The shell reports its command line (`L` marks): without a report since the last
    /// command, the line is empty.
    reports_line: bool,
    /// The latest report, until a command runs or finishes.
    line: Option<CommandLine>,
    /// The folder the shell sent with its latest `D` mark.
    folder: Option<PathBuf>,
}

/// The shell integration state of one session, shared by the I/O thread, the terminal's event
/// listener and the UI. Lock order: the terminal first, then this.
pub struct PromptTracker {
    state: Mutex<TrackerState>,
    child_pid: OnceLock<u32>,
    on_event: Box<dyn Fn(PromptEvent) + Send + Sync>,
}

impl PromptTracker {
    pub fn new(on_event: impl Fn(PromptEvent) + Send + Sync + 'static) -> Self {
        Self {
            state: Mutex::default(),
            child_pid: OnceLock::new(),
            on_event: Box::new(on_event),
        }
    }

    /// The shell's process, for reading its folder when a command runs.
    pub fn set_child_pid(&self, pid: u32) {
        let _ = self.child_pid.set(pid);
    }

    fn state(&self) -> MutexGuard<'_, TrackerState> {
        self.state
            .lock()
            .unwrap_or_else(|poison| poison.into_inner())
    }

    /// A mark was found in the output. Returns the tag to append after it when the mark needs
    /// the terminal state at that point (`A` and `B`); the others are applied at once.
    /// `in_sync`: the mark is inside a synchronized update, whose bytes the parser holds back
    /// until it ends, so a tag would not be answered in time; the prompt is not tracked then.
    fn mark(&self, mark: PromptMark, in_sync: bool) -> Option<u16> {
        let (events, tag) = {
            let mut state = self.state();
            match mark {
                PromptMark::PromptStart | PromptMark::CommandStart => {
                    // A prompt without a D mark before it (bash whose PROMPT_COMMAND was
                    // replaced): the command finished anyway. The pending mark's `apply` sets
                    // the phase again once it resolves.
                    let mut events = Vec::new();
                    if state.phase == Phase::Running {
                        state.phase = Phase::Idle;
                        state.line = None;
                        events.push(PromptEvent::Finished);
                    }
                    if in_sync {
                        (events, None)
                    } else {
                        let tag = TAG_BASE + state.next_tag;
                        state.next_tag = (state.next_tag + 1) % TAG_COUNT;
                        state.pending = Some(Pending {
                            mark,
                            tag,
                            parsed: false,
                            handed_over: None,
                        });
                        (events, Some(tag))
                    }
                }
                PromptMark::CommandExecuted(command) => {
                    state.line = None;
                    let mut events = vec![PromptEvent::Started(command.clone())];
                    if let Phase::Typing { folder, .. } =
                        std::mem::replace(&mut state.phase, Phase::Running)
                        && let Some(command) = command
                    {
                        events.push(PromptEvent::Executed(ExecutedCommand { command, folder }));
                    }
                    (events, None)
                }
                PromptMark::CommandFinished(folder) => {
                    // A new folder at the first prompt (a startup file's `cd`) is news too.
                    let moved = folder.is_some() && folder != state.folder;
                    if moved {
                        state.folder = folder;
                    }
                    let was_running = std::mem::replace(&mut state.phase, Phase::Idle);
                    state.line = None;
                    if was_running == Phase::Running || moved {
                        (vec![PromptEvent::Finished], None)
                    } else {
                        (Vec::new(), None)
                    }
                }
                PromptMark::CommandLine(line) => {
                    // A line too long to report is read from the grid.
                    state.reports_line = line.is_some();
                    state.line = line;
                    (Vec::new(), None)
                }
            }
        };
        for event in events {
            (self.on_event)(event);
        }
        tag
    }

    /// The parser has been handed everything up to the pending mark's tag.
    fn handed_over(&self) {
        if let Some(pending) = self.state().pending.as_mut() {
            pending.handed_over.get_or_insert_with(Instant::now);
        }
    }

    fn has_pending(&self) -> bool {
        self.state().pending.is_some()
    }

    /// The pending mark was handed to the parser, which has not answered its tag yet.
    fn waiting_for_parser(&self) -> bool {
        self.state()
            .pending
            .as_ref()
            .is_some_and(|pending| !pending.parsed && pending.handed_over.is_some())
    }

    /// A `PtyWrite` the terminal produced. Returns true when it is the answer to one of our
    /// tags, which must not reach the program.
    pub fn on_tag_reply(&self, text: &str) -> bool {
        let Some(tag) = parse_tag_reply(text) else {
            return false;
        };
        if let Some(pending) = self.state().pending.as_mut()
            && pending.tag == tag
        {
            pending.parsed = true;
        }
        true
    }

    /// Apply the pending mark if it has been parsed, reading the terminal at that point.
    /// Returns false while a mark still waits; a mark not parsed within [`MARK_TIMEOUT`] is
    /// dropped.
    pub fn resolve<L: EventListener>(&self, term: &Term<L>) -> bool {
        let mut state = self.state();
        let Some(pending) = state.pending.take_if(|pending| pending.parsed) else {
            return state.pending.is_none() || drop_if_stale(&mut state);
        };
        // Read while the shell waits at its prompt: once the command runs, `cd` may have
        // changed it.
        let folder = (pending.mark == PromptMark::CommandStart)
            .then(|| self.child_folder(&state))
            .flatten();
        apply(&mut state, &pending.mark, term, folder);
        true
    }

    /// Like [`Self::resolve`], for when the terminal lock could not be taken.
    fn resolve_unlocked(&self) -> bool {
        drop_if_stale(&mut self.state())
    }

    /// What the user typed at the prompt, when the shell is at its command line. `None` without
    /// integration, while a command runs, or when the command line spans several lines.
    pub fn input<L: EventListener>(&self, term: &Term<L>) -> Option<PromptInput> {
        let (prompt, start, line) = {
            let state = self.state();
            let Phase::Typing { prompt, start, .. } = &state.phase else {
                return None;
            };
            let line = state
                .reports_line
                .then(|| state.line.clone().unwrap_or_default());
            (prompt.clone(), start.clone(), line)
        };
        prompt_input(term, prompt.as_ref(), &start, line)
    }

    /// The shell's folder now: read from the process on Linux, else the one its script last
    /// reported (at each prompt).
    pub fn folder(&self) -> Option<PathBuf> {
        self.child_folder(&self.state())
    }

    fn child_folder(&self, state: &TrackerState) -> Option<PathBuf> {
        if cfg!(target_os = "linux")
            && let Some(pid) = self.child_pid.get()
        {
            return std::fs::read_link(format!("/proc/{pid}/cwd")).ok();
        }
        state.folder.clone()
    }
}

fn drop_if_stale(state: &mut TrackerState) -> bool {
    let stale = state.pending.as_ref().is_some_and(|pending| {
        !pending.parsed
            && pending
                .handed_over
                .is_some_and(|at| at.elapsed() >= MARK_TIMEOUT)
    });
    if stale {
        log::debug!("shell integration mark dropped: not parsed in time");
        state.pending = None;
    }
    stale
}

/// Apply an `A` or `B` mark with the terminal as it was right after it; `folder` is the
/// shell's folder at a `B` mark.
fn apply<L: EventListener>(
    state: &mut TrackerState,
    mark: &PromptMark,
    term: &Term<L>,
    folder: Option<PathBuf>,
) {
    match mark {
        PromptMark::PromptStart => {
            state.prompt = Some(anchor_at_cursor(term));
            state.phase = Phase::Idle;
        }
        PromptMark::CommandStart => {
            state.phase = Phase::Typing {
                prompt: state.prompt.take(),
                start: anchor_at_cursor(term),
                folder,
            };
        }
        // Applied when found (`PromptTracker::mark`).
        _ => {}
    }
}

/// `ESC [ <tag> $ p`: DECRQM for an unknown ANSI mode, answered `ESC [ <tag> ; 0 $ y`.
fn tag_query(tag: u16) -> String {
    format!("\x1b[{tag}$p")
}

fn parse_tag_reply(text: &str) -> Option<u16> {
    let body = text.strip_prefix("\x1b[")?.strip_suffix("$y")?;
    let (mode, _) = body.split_once(';')?;
    let tag: u16 = mode.parse().ok()?;
    (TAG_BASE..TAG_BASE + TAG_COUNT)
        .contains(&tag)
        .then_some(tag)
}

fn anchor_at_cursor<L: EventListener>(term: &Term<L>) -> Anchor {
    let grid = term.grid();
    let point = grid.cursor.point;
    Anchor {
        line: point.line.0,
        column: point.column.0,
        history: grid.history_size(),
        prefix: line_prefix(term, point.line, point.column.0),
    }
}

fn line_prefix<L: EventListener>(term: &Term<L>, line: Line, columns: usize) -> String {
    let row = &term.grid()[line];
    (0..columns).map(|column| row[Column(column)].c).collect()
}

/// Where `anchor` is now. The screen may have scrolled since: by the growth of the scrollback
/// when it wasn't full yet, else found by looking upward for the line that still starts with
/// the text that was left of the cursor.
fn locate<L: EventListener>(term: &Term<L>, anchor: &Anchor) -> Option<Point> {
    let grid = term.grid();
    if anchor.column >= grid.columns() {
        return None;
    }
    let history = grid.history_size();
    let topmost = -(history as i32);
    let first = history.saturating_sub(anchor.history);
    for drift in first..=first + grid.screen_lines() {
        let line = anchor.line - drift as i32;
        if line < topmost {
            break;
        }
        if line >= grid.screen_lines() as i32 {
            continue;
        }
        if line_prefix(term, Line(line), anchor.column) == anchor.prefix {
            return Some(Point::new(Line(line), Column(anchor.column)));
        }
    }
    None
}

/// The text from `start` up to `end` (exclusive). Lines that wrapped are joined; other line
/// ends become `\n` with the line's trailing blanks dropped.
fn text_between<L: EventListener>(term: &Term<L>, start: Point, end: Point) -> String {
    let grid = term.grid();
    let columns = grid.columns();
    let mut text = String::new();
    for line in start.line.0..=end.line.0 {
        let row = &grid[Line(line)];
        let from = if line == start.line.0 {
            start.column.0
        } else {
            0
        };
        let to = if line == end.line.0 {
            end.column.0.min(columns)
        } else {
            columns
        };
        let mut segment = String::new();
        for column in from..to {
            let cell = &row[Column(column)];
            if cell
                .flags
                .intersects(Flags::WIDE_CHAR_SPACER | Flags::LEADING_WIDE_CHAR_SPACER)
            {
                continue;
            }
            segment.push(cell.c);
            if let Some(extra) = cell.zerowidth() {
                segment.extend(extra);
            }
        }
        let wrapped = row[Column(columns - 1)].flags.contains(Flags::WRAPLINE);
        if line == end.line.0 || wrapped {
            text.push_str(&segment);
        } else {
            text.push_str(segment.trim_end());
            text.push('\n');
        }
    }
    text
}

/// The end of the screen line `line` and the lines it wraps into.
fn logical_line_end<L: EventListener>(term: &Term<L>, line: Line) -> Point {
    let grid = term.grid();
    let last_column = grid.columns() - 1;
    let bottom = grid.screen_lines() as i32 - 1;
    let mut line = line.0;
    while line < bottom
        && grid[Line(line)][Column(last_column)]
            .flags
            .contains(Flags::WRAPLINE)
    {
        line += 1;
    }
    Point::new(Line(line), Column(grid.columns()))
}

/// The command line from `start`: `line` when the shell reported it, else read from the grid.
fn prompt_input<L: EventListener>(
    term: &Term<L>,
    prompt: Option<&Anchor>,
    start: &Anchor,
    line: Option<CommandLine>,
) -> Option<PromptInput> {
    let start_point = locate(term, start)?;
    let cursor = term.grid().cursor.point;
    if cursor < start_point {
        return None;
    }
    let (before_cursor, text, at_end) = match line {
        Some(line) => {
            if line.text.chars().any(char::is_control) {
                return None;
            }
            let before_cursor: String = line.text.chars().take(line.cursor).collect();
            let at_end = before_cursor.len() == line.text.len() && !line.shell_suggests;
            (before_cursor, line.text, at_end)
        }
        None => {
            let before_cursor = text_between(term, start_point, cursor);
            if before_cursor.contains('\n') {
                return None;
            }
            let after = text_between(term, cursor, logical_line_end(term, cursor.line));
            let at_end = after.trim().is_empty();
            let text = format!("{before_cursor}{after}").trim_end().to_string();
            (before_cursor, text, at_end)
        }
    };
    // The prompt moved by as much as the command line did since both were recorded.
    let prompt_line = prompt
        .filter(|prompt| prompt.history == start.history && prompt.line <= start.line)
        .map_or(start_point.line.0, |prompt| {
            start_point.line.0 - (start.line - prompt.line)
        });
    Some(PromptInput {
        before_cursor,
        text,
        at_end,
        prompt_line,
    })
}

/// Finds marks in what it reads and holds back what follows each one until it is resolved.
struct MarkFilter<L: EventListener> {
    term: Arc<FairMutex<Term<L>>>,
    tracker: Arc<PromptTracker>,
    scanner: Osc133Scanner,
    chunk: Box<[u8]>,
    /// Read from the pty, not yet scanned.
    input: Vec<u8>,
    input_pos: usize,
    /// Scanned, not yet handed to the parser (ends with a tag query after a mark).
    output: Vec<u8>,
    output_pos: usize,
    /// Output after an `A` or `B` mark is held back until the mark is resolved; without it the
    /// prompt's position is not tracked.
    holds_prompts: bool,
}

impl<L: EventListener> MarkFilter<L> {
    fn new(
        term: Arc<FairMutex<Term<L>>>,
        tracker: Arc<PromptTracker>,
        token: &str,
        holds_prompts: bool,
    ) -> Self {
        Self {
            term,
            tracker,
            holds_prompts,
            scanner: Osc133Scanner::new(token),
            chunk: vec![0; READ_CHUNK].into_boxed_slice(),
            input: Vec::new(),
            input_pos: 0,
            output: Vec::new(),
            output_pos: 0,
        }
    }

    fn read(&mut self, inner: &mut impl Read, buf: &mut [u8]) -> io::Result<usize> {
        if buf.is_empty() {
            return Ok(0);
        }
        loop {
            if self.output_pos < self.output.len() {
                let count = buf.len().min(self.output.len() - self.output_pos);
                buf[..count].copy_from_slice(&self.output[self.output_pos..][..count]);
                self.output_pos += count;
                if self.output_pos == self.output.len() {
                    self.tracker.handed_over();
                }
                return Ok(count);
            }
            if self.tracker.has_pending() && !self.try_resolve() {
                if self.tracker.waiting_for_parser() {
                    thread::sleep(PARSE_WAIT);
                }
                return Err(ErrorKind::WouldBlock.into());
            }
            if self.input_pos >= self.input.len() {
                let count = inner.read(&mut self.chunk)?;
                if count == 0 {
                    return Ok(0);
                }
                self.input.clear();
                self.input.extend_from_slice(&self.chunk[..count]);
                self.input_pos = 0;
            }
            self.scan_input();
        }
    }

    /// Whether output read from the pty waits behind an unresolved mark.
    #[cfg(unix)]
    fn holds_input(&self) -> bool {
        self.input_pos < self.input.len()
    }

    /// Move scanned input to the output, stopping after the first mark.
    fn scan_input(&mut self) {
        let rest = &self.input[self.input_pos..];
        self.output.clear();
        self.output_pos = 0;
        match self.scanner.scan(rest) {
            Some((end, mark)) => {
                self.output.extend_from_slice(&rest[..end]);
                self.input_pos += end;
                let untracked = self.scanner.in_sync() || !self.holds_prompts;
                if let Some(tag) = self.tracker.mark(mark, untracked) {
                    self.output.extend_from_slice(tag_query(tag).as_bytes());
                }
            }
            None => {
                self.output.extend_from_slice(rest);
                self.input_pos = self.input.len();
            }
        }
    }

    /// The I/O thread's own parse loop may hold the terminal lock between reads; then this
    /// waits for the next read (or for the UI to resolve the mark).
    fn try_resolve(&self) -> bool {
        match self.term.try_lock_unfair() {
            Some(term) => self.tracker.resolve(&term),
            None => self.tracker.resolve_unlocked(),
        }
    }
}

/// Key alacritty's event loop reads the pty on (`PTY_READ_WRITE_TOKEN`, crate-private in
/// `alacritty_terminal/src/tty/unix.rs`; the version is pinned for this, and
/// `session::tests::output_after_a_mark_reaches_the_terminal` fails when it is wrong).
#[cfg(unix)]
const READ_WRITE_TOKEN: usize = 0;

/// Makes the event loop read again while the filter holds output back: the pty is polled
/// level-triggered, so once its kernel buffer is empty nothing would call the reader for the
/// output already read past a mark. A socket registered under the pty's key is kept readable
/// for as long as that lasts.
#[cfg(unix)]
struct Wake {
    sender: UnixStream,
    receiver: UnixStream,
    set: bool,
}

#[cfg(unix)]
impl Wake {
    fn new() -> io::Result<Self> {
        let (sender, receiver) = UnixStream::pair()?;
        sender.set_nonblocking(true)?;
        receiver.set_nonblocking(true)?;
        Ok(Self {
            sender,
            receiver,
            set: false,
        })
    }

    fn set(&mut self) {
        if !self.set {
            self.set = (&self.sender).write(&[1]).is_ok();
        }
    }

    fn clear(&mut self) {
        if self.set {
            let mut drain = [0u8; 16];
            while matches!((&self.receiver).read(&mut drain), Ok(count) if count > 0) {}
            self.set = false;
        }
    }
}

/// Makes the event loop read again while the filter holds output back. alacritty's Windows
/// pipe reader only wakes the poller for new output once a read found its pipe empty, so the
/// output already read past a mark (or still in that pipe) would wait for the next output.
/// Each read that blocks posts one readable event for the pty's key instead.
#[cfg(windows)]
struct Wake {
    poller: Arc<Poller>,
    event: PollingEvent,
}

#[cfg(windows)]
impl Wake {
    fn set(&self) {
        if let Err(err) = self.poller.post(CompletionPacket::new(self.event)) {
            log::warn!("shell integration: could not wake the pty reader: {err}");
        }
    }
}

/// Called on the I/O thread with each new progress state the program reports.
pub(crate) type ProgressCallback = Box<dyn Fn(ProgressState) + Send>;

/// A pty whose output is read through a [`MarkFilter`]. Without shell integration (or without
/// a [`Wake`]) output passes through unchanged and no marks are tracked. Progress reports are
/// looked for in all output.
pub(crate) struct MarkedPty<T, L: EventListener> {
    pty: T,
    filter: MarkFilter<L>,
    progress: ProgressScanner,
    /// The last progress state reported, so a program repeating it is not reported again.
    last_progress: Option<ProgressState>,
    on_progress: ProgressCallback,
    #[cfg(unix)]
    wake: Option<Wake>,
    /// Set when the event loop registers the pty, for shells with integration.
    #[cfg(windows)]
    wake: Option<Wake>,
    #[cfg(windows)]
    scans: bool,
}

impl<T, L: EventListener> MarkedPty<T, L> {
    /// `token`: the one the shell's marks carry; `None` passes output through.
    pub(crate) fn new(
        pty: T,
        term: Arc<FairMutex<Term<L>>>,
        tracker: Arc<PromptTracker>,
        token: Option<&str>,
        on_progress: ProgressCallback,
    ) -> Self {
        #[cfg(unix)]
        let wake = token.and_then(|_| {
            Wake::new()
                .inspect_err(|err| log::warn!("shell integration disabled: {err}"))
                .ok()
        });
        Self {
            pty,
            filter: MarkFilter::new(term, tracker, token.unwrap_or_default(), HOLDS_PROMPTS),
            progress: ProgressScanner::new(),
            last_progress: None,
            on_progress,
            #[cfg(unix)]
            wake,
            #[cfg(windows)]
            wake: None,
            #[cfg(windows)]
            scans: token.is_some(),
        }
    }
}

impl<T: EventedReadWrite, L: EventListener> MarkedPty<T, L> {
    fn report_progress(&mut self, output: &[u8]) {
        let Some(state) = self.progress.scan(output) else {
            return;
        };
        if self.last_progress.replace(state) != Some(state) {
            (self.on_progress)(state);
        }
    }

    #[cfg(unix)]
    fn read_marked(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        let Some(wake) = self.wake.as_mut() else {
            return self.pty.reader().read(buf);
        };
        wake.clear();
        let result = self.filter.read(self.pty.reader(), buf);
        let blocked = matches!(&result, Err(err) if err.kind() == ErrorKind::WouldBlock);
        if blocked && self.filter.holds_input() {
            wake.set();
        }
        result
    }

    #[cfg(windows)]
    fn read_marked(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        let Some(wake) = self.wake.as_ref() else {
            return self.pty.reader().read(buf);
        };
        let result = self.filter.read(self.pty.reader(), buf);
        if matches!(&result, Err(err) if err.kind() == ErrorKind::WouldBlock) {
            wake.set();
        }
        result
    }
}

impl<T: EventedReadWrite, L: EventListener> Read for MarkedPty<T, L> {
    fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        let count = self.read_marked(buf)?;
        self.report_progress(&buf[..count]);
        Ok(count)
    }
}

impl<T: EventedReadWrite, L: EventListener> EventedReadWrite for MarkedPty<T, L> {
    type Reader = Self;
    type Writer = T::Writer;

    unsafe fn register(
        &mut self,
        poll: &Arc<Poller>,
        interest: PollingEvent,
        mode: PollMode,
    ) -> io::Result<()> {
        // SAFETY: the caller's guarantee for `self` covers the pty and the wake socket it owns.
        unsafe {
            self.pty.register(poll, interest, mode)?;
            #[cfg(unix)]
            if let Some(wake) = &self.wake {
                poll.add_with_mode(
                    &wake.receiver,
                    PollingEvent::readable(READ_WRITE_TOKEN),
                    PollMode::Level,
                )?;
            }
        }
        #[cfg(windows)]
        if self.scans {
            self.wake = Some(Wake {
                poller: poll.clone(),
                event: PollingEvent::readable(alacritty_terminal::tty::PTY_READ_WRITE_TOKEN),
            });
        }
        Ok(())
    }

    fn reregister(
        &mut self,
        poll: &Arc<Poller>,
        interest: PollingEvent,
        mode: PollMode,
    ) -> io::Result<()> {
        self.pty.reregister(poll, interest, mode)
    }

    fn deregister(&mut self, poll: &Arc<Poller>) -> io::Result<()> {
        #[cfg(unix)]
        if let Some(wake) = &self.wake {
            poll.delete(&wake.receiver)?;
        }
        #[cfg(windows)]
        {
            self.wake = None;
        }
        self.pty.deregister(poll)
    }

    fn reader(&mut self) -> &mut Self {
        self
    }

    fn writer(&mut self) -> &mut T::Writer {
        self.pty.writer()
    }
}

impl<T: EventedPty, L: EventListener> EventedPty for MarkedPty<T, L> {
    fn next_child_event(&mut self) -> Option<ChildEvent> {
        self.pty.next_child_event()
    }
}

impl<T: OnResize, L: EventListener> OnResize for MarkedPty<T, L> {
    fn on_resize(&mut self, window_size: WindowSize) {
        self.pty.on_resize(window_size);
    }
}

#[cfg(test)]
mod tests {
    use alacritty_terminal::event::Event;
    use alacritty_terminal::term::Config;
    use alacritty_terminal::vte::ansi::Processor;

    use super::*;
    use crate::TermDimensions;

    /// Answers tag queries like the session's listener.
    #[derive(Clone)]
    struct Listener(Arc<PromptTracker>);

    impl EventListener for Listener {
        fn send_event(&self, event: Event) {
            if let Event::PtyWrite(text) = event {
                assert!(self.0.on_tag_reply(&text), "unexpected reply {text:?}");
            }
        }
    }

    struct Harness {
        term: Arc<FairMutex<Term<Listener>>>,
        tracker: Arc<PromptTracker>,
        events: Arc<Mutex<Vec<PromptEvent>>>,
        filter: MarkFilter<Listener>,
        parser: Processor,
    }

    impl Harness {
        fn new(columns: u16, lines: u16) -> Self {
            let events = Arc::new(Mutex::new(Vec::new()));
            let sink = events.clone();
            let tracker = Arc::new(PromptTracker::new(move |event| {
                sink.lock().unwrap().push(event);
            }));
            let term = Arc::new(FairMutex::new(Term::new(
                Config::default(),
                &TermDimensions::new(columns, lines),
                Listener(tracker.clone()),
            )));
            let filter = MarkFilter::new(term.clone(), tracker.clone(), TOKEN, true);
            Self {
                term,
                tracker,
                events,
                filter,
                parser: Processor::new(),
            }
        }

        /// Feed `output` through the filter and the parser, as the event loop would: parse what
        /// each read returns, stop a batch at `WouldBlock`, start the next batch.
        fn feed(&mut self, output: &[u8]) {
            let mut source = output;
            let mut buf = [0u8; 7];
            let mut idle_batches = 0;
            loop {
                match self.filter.read(&mut source, &mut buf) {
                    Ok(0) => return,
                    Ok(count) => {
                        idle_batches = 0;
                        self.parser.advance(&mut *self.term.lock(), &buf[..count]);
                    }
                    Err(err) if err.kind() == ErrorKind::WouldBlock => {
                        idle_batches += 1;
                        assert!(idle_batches < 3, "reader stalled");
                    }
                    Err(err) => panic!("{err}"),
                }
            }
        }

        fn input(&self) -> Option<PromptInput> {
            let term = self.term.lock();
            self.tracker.resolve(&term);
            self.tracker.input(&term)
        }

        fn commands(&self) -> Vec<String> {
            let events = self.events.lock().unwrap();
            events
                .iter()
                .filter_map(|event| match event {
                    PromptEvent::Executed(done) => Some(done.command.clone()),
                    _ => None,
                })
                .collect()
        }

        /// The events other than `Executed`, taken.
        fn take_running_events(&self) -> Vec<PromptEvent> {
            let mut events = self.events.lock().unwrap();
            std::mem::take(&mut *events)
                .into_iter()
                .filter(|event| !matches!(event, PromptEvent::Executed(_)))
                .collect()
        }
    }

    const TOKEN: &str = "t0k";
    const PROMPT: &str =
        "\x1b]133;D;aw=t0k\x07\x1b]133;A;aw=t0k\x07user@host:~$ \x1b]133;B;aw=t0k\x07";

    /// Enter on `command`: the shell moves to a new line and sends the C mark.
    fn executed(command: &str) -> String {
        let hex: String = command.bytes().map(|byte| format!("{byte:02x}")).collect();
        format!("\r\n\x1b]133;C;aw=t0k;cmd={hex}\x07")
    }

    fn reported_line(text: &str, cursor: usize, post: bool) -> String {
        let hex: String = text.bytes().map(|byte| format!("{byte:02X}")).collect();
        let post = if post { ";post=1" } else { "" };
        format!("\x1b]133;L;aw=t0k;cur={cursor};buf={hex}{post}\x07")
    }

    #[test]
    fn typed_text_is_read_from_the_command_start() {
        let mut harness = Harness::new(40, 5);
        assert_eq!(harness.input(), None);
        harness.feed(format!("{PROMPT}git st").as_bytes());
        let input = harness.input().unwrap();
        assert_eq!(input.before_cursor, "git st");
        assert_eq!(input.text, "git st");
        assert!(input.at_end);
        assert_eq!(input.prompt_line, 0);
        // Cursor moved left over "st".
        harness.feed(b"\x08\x08");
        let input = harness.input().unwrap();
        assert_eq!(input.before_cursor, "git ");
        assert_eq!(input.text, "git st");
        assert!(!input.at_end);
    }

    #[test]
    fn executed_commands_are_reported_and_end_typing() {
        let mut harness = Harness::new(40, 5);
        harness.feed(format!("{PROMPT}ls -la{}output\r\n", executed("ls -la")).as_bytes());
        assert_eq!(harness.input(), None);
        // A C mark without a command (left out of the shell's history) records nothing.
        harness.feed(format!("{PROMPT} secret\r\n\x1b]133;C;aw=t0k\x07").as_bytes());
        harness.feed(PROMPT.as_bytes());
        assert_eq!(harness.input().unwrap().text, "");
        assert_eq!(harness.commands(), ["ls -la"]);
    }

    #[test]
    fn running_commands_are_reported_from_start_to_finish() {
        let mut harness = Harness::new(40, 5);
        // The first prompt's D mark ends no command.
        harness.feed(format!("{PROMPT}sleep 5{}", executed("sleep 5")).as_bytes());
        assert_eq!(
            harness.take_running_events(),
            [PromptEvent::Started(Some("sleep 5".into()))]
        );
        harness.feed(PROMPT.as_bytes());
        assert_eq!(harness.take_running_events(), [PromptEvent::Finished]);
        // A command left out of the history runs without its command line.
        harness.feed(b" secret\r\n\x1b]133;C;aw=t0k\x07");
        assert_eq!(harness.take_running_events(), [PromptEvent::Started(None)]);
        harness.feed(PROMPT.as_bytes());
        // Another D mark without a command in between, as after Ctrl+C at the prompt.
        harness.feed(PROMPT.as_bytes());
        assert_eq!(harness.take_running_events(), [PromptEvent::Finished]);
        // Two C marks (a pasted line, PowerShell's multi-line input), then one D mark.
        harness.feed(format!("{}{}", executed("a"), executed("b")).as_bytes());
        harness.feed(PROMPT.as_bytes());
        assert_eq!(
            harness.take_running_events(),
            [
                PromptEvent::Started(Some("a".into())),
                PromptEvent::Started(Some("b".into())),
                PromptEvent::Finished,
            ]
        );
    }

    #[test]
    fn a_command_before_the_first_prompt_runs_until_the_prompt() {
        let mut harness = Harness::new(40, 5);
        harness.feed(executed("make").as_bytes());
        assert_eq!(
            harness.take_running_events(),
            [PromptEvent::Started(Some("make".into()))]
        );
        harness.feed(PROMPT.as_bytes());
        assert_eq!(harness.take_running_events(), [PromptEvent::Finished]);
    }

    #[test]
    fn a_prompt_without_a_d_mark_finishes_the_command() {
        let mut harness = Harness::new(40, 5);
        // bash whose PROMPT_COMMAND was replaced: its prompts carry A and B but no D.
        let prompt = PROMPT.trim_start_matches("\x1b]133;D;aw=t0k\x07");
        harness.feed(format!("{PROMPT}sleep 5{}", executed("sleep 5")).as_bytes());
        harness.take_running_events();
        harness.feed(prompt.as_bytes());
        assert_eq!(harness.take_running_events(), [PromptEvent::Finished]);
        // The next command line is tracked as usual.
        harness.feed(b"ls");
        assert_eq!(harness.input().map(|input| input.text), Some("ls".into()));
    }

    #[test]
    fn the_folder_reported_at_each_prompt_is_kept() {
        let mut harness = Harness::new(40, 5);
        assert_eq!(harness.tracker.folder(), None);
        // "/home" at the first prompt: news, though no command ran.
        harness.feed(b"\x1b]133;D;aw=t0k;cwd=2f686f6d65\x07");
        assert_eq!(harness.tracker.folder(), Some(PathBuf::from("/home")));
        assert_eq!(harness.take_running_events(), [PromptEvent::Finished]);
        // The same folder again, or none sent: nothing to report, the folder stays.
        harness.feed(b"\x1b]133;D;aw=t0k;cwd=2f686f6d65\x07\x1b]133;D;aw=t0k\x07");
        assert!(harness.take_running_events().is_empty());
        assert_eq!(harness.tracker.folder(), Some(PathBuf::from("/home")));
    }

    #[test]
    fn without_the_hold_commands_are_reported_but_not_the_prompt() {
        let mut harness = Harness::new(40, 5);
        harness.filter.holds_prompts = false;
        harness.feed(format!("{PROMPT}sleep 5{}", executed("sleep 5")).as_bytes());
        assert_eq!(
            harness.take_running_events(),
            [PromptEvent::Started(Some("sleep 5".into()))]
        );
        harness.feed(PROMPT.as_bytes());
        assert_eq!(harness.take_running_events(), [PromptEvent::Finished]);
        harness.feed(b"ls");
        assert_eq!(harness.input(), None);
        assert!(harness.commands().is_empty());
    }

    #[test]
    fn the_reported_command_line_wins_over_the_grid() {
        let mut harness = Harness::new(40, 5);
        // A right prompt and a zsh-autosuggestions suggestion are on the grid after the cursor.
        let right_prompt = "\x1b[s\x1b[1;30H12:03:11\x1b[u";
        harness.feed(format!("{PROMPT}{right_prompt}").as_bytes());
        assert!(!harness.input().unwrap().at_end);
        harness.feed(format!("{}git{right_prompt}", reported_line("git", 3, false)).as_bytes());
        let input = harness.input().unwrap();
        assert_eq!(
            (input.before_cursor.as_str(), input.text.as_str()),
            ("git", "git")
        );
        assert!(input.at_end);
        harness.feed(format!("{} status", reported_line("git", 3, true)).as_bytes());
        let input = harness.input().unwrap();
        assert_eq!(input.text, "git");
        assert!(!input.at_end, "zsh shows its own suggestion");
        harness.feed(format!("{}\x08\x08", reported_line("git ", 2, false)).as_bytes());
        let input = harness.input().unwrap();
        assert_eq!(
            (input.before_cursor.as_str(), input.text.as_str()),
            ("gi", "git ")
        );
        assert!(!input.at_end);
        harness.feed(executed("git").as_bytes());
        // The next prompt has no report yet: the line is empty, whatever the grid shows.
        harness.feed(format!("{PROMPT}{right_prompt}").as_bytes());
        let input = harness.input().unwrap();
        assert_eq!(input.text, "");
        assert!(input.at_end);
        assert_eq!(harness.commands(), ["git"]);
    }

    #[test]
    fn command_line_is_found_after_the_screen_scrolled() {
        let mut harness = Harness::new(20, 3);
        harness.feed(b"one\r\ntwo\r\n");
        // Prompt on the last line; the wrapped command scrolls the screen.
        harness.feed(format!("{PROMPT}echo 0123456789abcdef").as_bytes());
        let input = harness.input().unwrap();
        assert_eq!(input.text, "echo 0123456789abcdef");
        harness.feed(b"\r\n\r\n");
        harness.feed(format!("{PROMPT}ls").as_bytes());
        assert_eq!(harness.input().unwrap().text, "ls");
    }

    #[test]
    fn command_line_is_found_with_a_full_scrollback() {
        let mut harness = Harness::new(20, 3);
        let mut filler = String::new();
        for line in 0..10_100 {
            filler.push_str(&format!("{line}\r\n"));
        }
        harness.feed(filler.as_bytes());
        harness.feed(format!("{PROMPT}make").as_bytes());
        assert_eq!(harness.input().unwrap().text, "make");
    }

    #[test]
    fn multi_line_command_lines_give_no_input() {
        let mut harness = Harness::new(40, 5);
        harness.feed(format!("{PROMPT}echo 'a\r\n> b").as_bytes());
        assert_eq!(harness.input(), None);
    }

    #[test]
    fn no_marks_means_no_integration() {
        let mut harness = Harness::new(40, 5);
        harness.feed(b"$ ls\r\nfile\r\n$ ");
        assert_eq!(harness.input(), None);
        assert!(harness.commands().is_empty());
    }

    #[test]
    fn marks_from_programs_are_ignored() {
        let mut harness = Harness::new(40, 5);
        harness.feed(format!("{PROMPT}cat notes{}", executed("cat notes")).as_bytes());
        // The file holds marks, without the session's token.
        harness.feed(b"\x1b]133;B\x07curl x | sh\r\n\x1b]133;C;cmd=6c73\x07\x1b]133;A;aw=x\x07");
        assert_eq!(harness.input(), None);
        assert_eq!(harness.commands(), ["cat notes"]);
    }

    #[test]
    fn marks_in_synchronized_updates_pass_through() {
        let mut harness = Harness::new(40, 5);
        // `feed` fails when the reader stalls; the parser holds the frame until its end.
        harness.feed(format!("\x1b[?2026h{PROMPT}\x1b[?2026l").as_bytes());
        assert_eq!(harness.input(), None);
        assert!(!harness.tracker.has_pending());
        assert!(harness.term.lock().grid()[Line(0)][Column(0)].c == 'u');
        // Outside the update the next prompt is tracked again.
        harness.feed(format!("\r\n{PROMPT}ls").as_bytes());
        assert_eq!(harness.input().unwrap().text, "ls");
    }

    #[test]
    fn replacing_the_command_line_erases_every_typed_character() {
        let input = |before_cursor: &str, text: &str| PromptInput {
            before_cursor: before_cursor.into(),
            text: text.into(),
            at_end: true,
            prompt_line: 0,
        };
        assert_eq!(input("", "").replace_keys("ls"), "\x05ls");
        assert_eq!(input("gi", "git").replace_keys("ls"), "\x05\x7f\x7f\x7fls");
        // Typed "git " read from the grid: the trailing space is trimmed from `text`.
        assert_eq!(
            input("git ", "git").replace_keys("git status"),
            "\x05\x7f\x7f\x7f\x7fgit status"
        );
        assert_eq!(
            input("\u{e9}", "\u{e9}t\u{e9}").replace_keys("x"),
            "\x05\x7f\x7f\x7fx"
        );
    }

    #[test]
    fn unparsed_mark_is_dropped_after_the_timeout() {
        let tracker = PromptTracker::new(|_| {});
        let tag = tracker.mark(PromptMark::CommandStart, false).unwrap();
        assert!(!tracker.resolve_unlocked());
        tracker.handed_over();
        tracker.state().pending.as_mut().unwrap().handed_over = Some(Instant::now() - MARK_TIMEOUT);
        assert!(tracker.resolve_unlocked());
        assert!(!tracker.has_pending());
        // A late answer to the dropped tag is still kept from the program.
        assert!(tracker.on_tag_reply(&format!("\x1b[{tag};0$y")));
    }

    #[test]
    fn tag_replies_are_recognized() {
        assert_eq!(parse_tag_reply("\x1b[61000;0$y"), Some(61_000));
        assert_eq!(parse_tag_reply("\x1b[61999;0$y"), Some(61_999));
        assert_eq!(parse_tag_reply("\x1b[62000;0$y"), None);
        assert_eq!(parse_tag_reply("\x1b[4;2$y"), None);
        assert_eq!(parse_tag_reply("\x1b[12;40R"), None);
    }
}
