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
use workspace_model::file_tree::ShellSyntax;
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
const PROMPT_TRACKING: PromptTracking = if cfg!(any(unix, windows)) {
    PromptTracking::Position
} else {
    PromptTracking::EventsOnly
};

#[derive(Clone, Copy, PartialEq, Eq)]
enum PromptTracking {
    Position,
    EventsOnly,
}
/// Ctrl+E: end of line in readline (bash) and zsh's emacs keymap.
const END_OF_LINE: &str = "\x05";
/// The End key as the terminal sends it (`keys::bytes`). PSReadLine's Windows edit mode, the
/// default on Windows, doesn't bind Ctrl+E; End is EndOfLine in all its edit modes.
const END_KEY: &str = "\x1b[F";
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
    /// The keys that replace the command line with `command` in a `shell` tab: end of line,
    /// one Backspace per typed character (UTF-16 unit in PowerShell), then `command`.
    pub fn replace_keys(&self, command: &str, shell: ShellSyntax) -> String {
        // PSReadLine's Backspace deletes one UTF-16 unit, so a non-BMP character takes two.
        let length = |text: &str| match shell {
            ShellSyntax::PowerShell => text.encode_utf16().count(),
            ShellSyntax::Posix | ShellSyntax::Wsl | ShellSyntax::Cmd => text.chars().count(),
        };
        // `text` read from the grid drops trailing blanks; the cursor may sit after typed
        // spaces.
        let typed = length(&self.text).max(length(&self.before_cursor));
        let end = match shell {
            ShellSyntax::PowerShell => END_KEY,
            ShellSyntax::Posix | ShellSyntax::Wsl | ShellSyntax::Cmd => END_OF_LINE,
        };
        let mut keys = String::from(end);
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

impl TrackerState {
    fn mark(
        &mut self,
        mark: PromptMark,
        tracking: PromptTracking,
    ) -> (Vec<PromptEvent>, Option<u16>) {
        match mark {
            PromptMark::PromptStart | PromptMark::CommandStart => self.start_prompt(mark, tracking),
            PromptMark::CommandExecuted(command) => (self.execute_command(command), None),
            PromptMark::CommandFinished(folder) => (self.finish_command(folder), None),
            PromptMark::CommandLine(line) => {
                self.reports_line = line.is_some();
                self.line = line;
                (Vec::new(), None)
            }
        }
    }

    fn start_prompt(
        &mut self,
        mark: PromptMark,
        tracking: PromptTracking,
    ) -> (Vec<PromptEvent>, Option<u16>) {
        let mut events = Vec::new();
        // A shell can replace PROMPT_COMMAND and omit D; the next prompt still ends the run.
        if self.phase == Phase::Running {
            self.phase = Phase::Idle;
            self.line = None;
            events.push(PromptEvent::Finished);
        }
        if tracking == PromptTracking::EventsOnly {
            return (events, None);
        }
        let tag = TAG_BASE + self.next_tag;
        self.next_tag = (self.next_tag + 1) % TAG_COUNT;
        self.pending = Some(Pending {
            mark,
            tag,
            parsed: false,
            handed_over: None,
        });
        (events, Some(tag))
    }

    fn execute_command(&mut self, command: Option<String>) -> Vec<PromptEvent> {
        self.line = None;
        let mut events = vec![PromptEvent::Started(command.clone())];
        if let Phase::Typing { folder, .. } = std::mem::replace(&mut self.phase, Phase::Running)
            && let Some(command) = command
        {
            events.push(PromptEvent::Executed(ExecutedCommand { command, folder }));
        }
        events
    }

    fn finish_command(&mut self, folder: Option<PathBuf>) -> Vec<PromptEvent> {
        let moved = folder.is_some() && folder != self.folder;
        if moved {
            self.folder = folder;
        }
        let was_running = std::mem::replace(&mut self.phase, Phase::Idle);
        self.line = None;
        if was_running == Phase::Running || moved {
            return vec![PromptEvent::Finished];
        }
        Vec::new()
    }
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
    /// EventsOnly avoids tag queries while synchronized output is held by the parser.
    fn mark(&self, mark: PromptMark, tracking: PromptTracking) -> Option<u16> {
        let (events, tag) = self.state().mark(mark, tracking);
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
    tracking: PromptTracking,
}

impl<L: EventListener> MarkFilter<L> {
    fn new(
        term: Arc<FairMutex<Term<L>>>,
        tracker: Arc<PromptTracker>,
        token: &str,
        tracking: PromptTracking,
    ) -> Self {
        Self {
            term,
            tracker,
            tracking,
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
                let tracking = if self.scanner.in_sync() {
                    PromptTracking::EventsOnly
                } else {
                    self.tracking
                };
                if let Some(tag) = self.tracker.mark(mark, tracking) {
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
            filter: MarkFilter::new(term, tracker, token.unwrap_or_default(), PROMPT_TRACKING),
            progress: ProgressScanner::new(),
            on_progress,
            #[cfg(unix)]
            wake,
            #[cfg(windows)]
            wake: None,
            #[cfg(windows)]
            scans: token.is_some(),
        }
    }

    fn report_progress(&mut self, output: &[u8]) {
        let Some(state) = self.progress.scan(output) else {
            return;
        };
        // A repeated state can resume work after a permission hook on the IPC channel.
        (self.on_progress)(state);
    }
}

impl<T: EventedReadWrite, L: EventListener> MarkedPty<T, L> {
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
mod tests;
