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
        let filter = MarkFilter::new(
            term.clone(),
            tracker.clone(),
            TOKEN,
            PromptTracking::Position,
        );
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
const PROMPT: &str = "\x1b]133;D;aw=t0k\x07\x1b]133;A;aw=t0k\x07user@host:~$ \x1b]133;B;aw=t0k\x07";

#[test]
fn repeated_progress_reports_are_forwarded() {
    let harness = Harness::new(40, 5);
    let reports = Arc::new(Mutex::new(Vec::new()));
    let captured = reports.clone();
    let mut pty = MarkedPty::new(
        (),
        harness.term,
        harness.tracker,
        None,
        Box::new(move |state| captured.lock().unwrap().push(state)),
    );
    pty.report_progress(b"\x1b]9;4;3\x07");
    pty.report_progress(b"ordinary output");
    pty.report_progress(b"\x1b]9;4;");
    pty.report_progress(b"3\x1b\\");
    pty.report_progress(b"\x1b]9;4;0\x07");
    pty.report_progress(b"\x1b]9;4;0\x07");
    assert_eq!(
        *reports.lock().unwrap(),
        [
            ProgressState::Indeterminate,
            ProgressState::Indeterminate,
            ProgressState::Hidden,
            ProgressState::Hidden,
        ]
    );
}

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
    assert_eq!(harness.take_running_events(), [PromptEvent::Finished(None)]);
    // A command left out of the history runs without its command line.
    harness.feed(b" secret\r\n\x1b]133;C;aw=t0k\x07");
    assert_eq!(harness.take_running_events(), [PromptEvent::Started(None)]);
    harness.feed(PROMPT.as_bytes());
    // Another D mark without a command in between, as after Ctrl+C at the prompt.
    harness.feed(PROMPT.as_bytes());
    assert_eq!(harness.take_running_events(), [PromptEvent::Finished(None)]);
    // Two C marks (a pasted line, PowerShell's multi-line input), then one D mark.
    harness.feed(format!("{}{}", executed("a"), executed("b")).as_bytes());
    harness.feed(PROMPT.as_bytes());
    assert_eq!(
        harness.take_running_events(),
        [
            PromptEvent::Started(Some("a".into())),
            PromptEvent::Started(Some("b".into())),
            PromptEvent::Finished(None),
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
    assert_eq!(harness.take_running_events(), [PromptEvent::Finished(None)]);
}

#[test]
fn a_prompt_without_a_d_mark_finishes_the_command() {
    let mut harness = Harness::new(40, 5);
    // bash whose PROMPT_COMMAND was replaced: its prompts carry A and B but no D.
    let prompt = PROMPT.trim_start_matches("\x1b]133;D;aw=t0k\x07");
    harness.feed(format!("{PROMPT}sleep 5{}", executed("sleep 5")).as_bytes());
    harness.take_running_events();
    harness.feed(prompt.as_bytes());
    assert_eq!(harness.take_running_events(), [PromptEvent::Finished(None)]);
    // The next command line is tracked as usual.
    harness.feed(b"ls");
    assert_eq!(harness.input().map(|input| input.text), Some("ls".into()));
}

#[test]
fn an_agent_mark_names_the_running_command_until_its_d_mark() {
    use workspace_model::agents::CLAUDE;
    const AGENT: &str = "\x1b]133;N;aw=t0k;agent=claude\x07";
    let mut harness = Harness::new(40, 5);
    // At the prompt no command runs: nothing to name.
    harness.feed(format!("{PROMPT}{AGENT}").as_bytes());
    assert!(harness.take_running_events().is_empty());
    harness.feed(format!("claude{}{AGENT}", executed("claude")).as_bytes());
    assert_eq!(
        harness.take_running_events(),
        [
            PromptEvent::Started(Some("claude".into())),
            PromptEvent::AgentStarted(CLAUDE),
        ]
    );
    // Ctrl+C: the shell reports the status with its D mark.
    harness.feed(b"\x1b]133;D;aw=t0k;exit=130\x07\x1b]133;A;aw=t0k\x07$ \x1b]133;B;aw=t0k\x07");
    assert_eq!(
        harness.take_running_events(),
        [PromptEvent::Finished(Some(130))]
    );
    // A status on a D mark that ends no command is not reported.
    harness.feed(b"\x1b]133;D;aw=t0k;exit=1\x07");
    assert!(harness.take_running_events().is_empty());
}

#[test]
fn the_folder_reported_at_each_prompt_is_kept() {
    let mut harness = Harness::new(40, 5);
    assert_eq!(harness.tracker.folder(), None);
    // "/home" at the first prompt: news, though no command ran.
    harness.feed(b"\x1b]133;D;aw=t0k;cwd=2f686f6d65\x07");
    assert_eq!(harness.tracker.folder(), Some(PathBuf::from("/home")));
    assert_eq!(harness.take_running_events(), [PromptEvent::Finished(None)]);
    // The same folder again, or none sent: nothing to report, the folder stays.
    harness.feed(b"\x1b]133;D;aw=t0k;cwd=2f686f6d65\x07\x1b]133;D;aw=t0k\x07");
    assert!(harness.take_running_events().is_empty());
    assert_eq!(harness.tracker.folder(), Some(PathBuf::from("/home")));
}

#[test]
fn without_the_hold_commands_are_reported_but_not_the_prompt() {
    let mut harness = Harness::new(40, 5);
    harness.filter.tracking = PromptTracking::EventsOnly;
    harness.feed(format!("{PROMPT}sleep 5{}", executed("sleep 5")).as_bytes());
    assert_eq!(
        harness.take_running_events(),
        [PromptEvent::Started(Some("sleep 5".into()))]
    );
    harness.feed(PROMPT.as_bytes());
    assert_eq!(harness.take_running_events(), [PromptEvent::Finished(None)]);
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
    let posix = ShellSyntax::Posix;
    assert_eq!(input("", "").replace_keys("ls", posix), "\x05ls");
    assert_eq!(
        input("gi", "git").replace_keys("ls", posix),
        "\x05\x7f\x7f\x7fls"
    );
    // Typed "git " read from the grid: the trailing space is trimmed from `text`.
    assert_eq!(
        input("git ", "git").replace_keys("git status", posix),
        "\x05\x7f\x7f\x7f\x7fgit status"
    );
    assert_eq!(
        input("\u{e9}", "\u{e9}t\u{e9}").replace_keys("x", posix),
        "\x05\x7f\x7f\x7fx"
    );
    assert_eq!(
        input("gi", "git").replace_keys("ls", ShellSyntax::Wsl),
        "\x05\x7f\x7f\x7fls"
    );
}

#[test]
fn powershell_goes_to_the_end_of_the_line_with_the_end_key() {
    // PSReadLine's Windows edit mode (pwsh and Windows PowerShell) has no Ctrl+E.
    let input = PromptInput {
        before_cursor: "gi".into(),
        text: "git".into(),
        at_end: false,
        prompt_line: 0,
    };
    assert_eq!(
        input.replace_keys("ls", ShellSyntax::PowerShell),
        "\x1b[F\x7f\x7f\x7fls"
    );
    let empty = PromptInput {
        before_cursor: String::new(),
        text: String::new(),
        at_end: true,
        prompt_line: 0,
    };
    assert_eq!(
        empty.replace_keys("Get-ChildItem", ShellSyntax::PowerShell),
        "\x1b[FGet-ChildItem"
    );
}

#[test]
fn powershell_erases_a_non_bmp_character_with_two_backspaces() {
    // PSReadLine deletes one UTF-16 unit per Backspace; other shells one character.
    let input = PromptInput {
        before_cursor: "a\u{1F600}".into(),
        text: "a\u{1F600}".into(),
        at_end: true,
        prompt_line: 0,
    };
    assert_eq!(
        input.replace_keys("ls", ShellSyntax::PowerShell),
        "\x1b[F\x7f\x7f\x7fls"
    );
    assert_eq!(
        input.replace_keys("ls", ShellSyntax::Posix),
        "\x05\x7f\x7fls"
    );
}

#[test]
fn unparsed_mark_is_dropped_after_the_timeout() {
    let tracker = PromptTracker::new(|_| {});
    let tag = tracker
        .mark(PromptMark::CommandStart, PromptTracking::Position)
        .unwrap();
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
