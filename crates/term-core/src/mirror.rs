//! A terminal fed with bytes instead of a pty: the copy of a remote session's terminal.
//!
//! A remote server's real [`crate::Session`] answers the program's queries (device attributes,
//! colors, cursor position), so a mirror never writes anything back: it only shows what the
//! program wrote. The bytes come first from a [`crate::snapshot`] of the server's terminal,
//! then from the live output.

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};

use alacritty_terminal::event::{Event, EventListener};
use alacritty_terminal::sync::FairMutex;
use alacritty_terminal::term::{self, Term};
use alacritty_terminal::vte::ansi::{Processor, StdSyncHandler};
use parking_lot::MutexGuard;

use crate::session::{EventCallback, SessionEvent, TermDimensions, screen_text, term_text};

struct Shared {
    callback: EventCallback,
    size: Mutex<TermDimensions>,
    wakeup_pending: AtomicBool,
}

/// Forwards what the terminal reports about the output; queries are answered by the server.
#[derive(Clone)]
pub struct MirrorListener(Arc<Shared>);

impl EventListener for MirrorListener {
    fn send_event(&self, event: Event) {
        let callback = &self.0.callback;
        match event {
            Event::Title(title) => callback(SessionEvent::TitleChanged(Some(title))),
            Event::ResetTitle => callback(SessionEvent::TitleChanged(None)),
            Event::Bell => callback(SessionEvent::Bell),
            Event::ClipboardStore(clipboard, text) => {
                callback(SessionEvent::ClipboardStore(clipboard, text));
            }
            other => log::trace!("mirror ignores terminal event: {other:?}"),
        }
    }
}

/// A terminal showing the bytes it is fed.
pub struct Mirror {
    term: Arc<FairMutex<Term<MirrorListener>>>,
    parser: Mutex<Processor<StdSyncHandler>>,
    shared: Arc<Shared>,
}

impl Mirror {
    /// `callback` gets [`SessionEvent::Wakeup`] after new output (coalesced until
    /// [`Mirror::ack_wakeup`]), title changes, bells and clipboard stores. It is called on the
    /// thread that feeds the mirror, with the terminal locked: it must not lock it.
    pub fn new(size: TermDimensions, callback: EventCallback) -> Self {
        let shared = Arc::new(Shared {
            callback,
            size: Mutex::new(size),
            wakeup_pending: AtomicBool::new(false),
        });
        let listener = MirrorListener(shared.clone());
        Self {
            term: Arc::new(FairMutex::new(Term::new(
                term::Config::default(),
                &size,
                listener,
            ))),
            parser: Mutex::new(Processor::new()),
            shared,
        }
    }

    /// Parse output of the program.
    pub fn feed(&self, bytes: &[u8]) {
        if bytes.is_empty() {
            return;
        }
        {
            let mut parser = self.parser.lock().unwrap();
            let mut term = self.term.lock();
            parser.advance(&mut *term, bytes);
        }
        if !self.shared.wakeup_pending.swap(true, Ordering::AcqRel) {
            (self.shared.callback)(SessionEvent::Wakeup);
        }
    }

    pub fn lock_term(&self) -> MutexGuard<'_, Term<MirrorListener>> {
        self.term.lock()
    }

    pub fn ack_wakeup(&self) {
        self.shared.wakeup_pending.store(false, Ordering::Release);
    }

    /// Resize the terminal only: the server resizes the pty, and the program redraws.
    pub fn resize(&self, size: TermDimensions) {
        {
            let mut current = self.shared.size.lock().unwrap();
            if *current == size {
                return;
            }
            *current = size;
        }
        self.term.lock().resize(size);
    }

    pub fn size(&self) -> TermDimensions {
        *self.shared.size.lock().unwrap()
    }

    /// The bytes that recreate this terminal in a fresh one of the same size.
    pub fn snapshot(&self) -> Vec<u8> {
        self.snapshot_within(usize::MAX)
    }

    /// As [`Mirror::snapshot`], without the oldest scrollback lines beyond `max_bytes`
    /// ([`crate::snapshot::snapshot_within`]).
    pub fn snapshot_within(&self, max_bytes: usize) -> Vec<u8> {
        // Locks the parser too, so no output is half parsed.
        let _parser = self.parser.lock().unwrap();
        crate::snapshot::snapshot_within(&self.term.lock(), max_bytes)
    }

    /// The visible screen as text, one string per line with trailing blanks trimmed.
    pub fn visible_text(&self) -> Vec<String> {
        screen_text(&self.term.lock())
    }

    /// The screen as text, after the scrollback when `scrollback` is set: one string per row,
    /// oldest first, trailing blanks and trailing empty rows dropped. A wide character comes
    /// once, with its combining characters.
    pub fn text(&self, scrollback: bool) -> Vec<String> {
        // Locks the parser too, so no output is half parsed.
        let _parser = self.parser.lock().unwrap();
        term_text(&self.term.lock(), scrollback)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use alacritty_terminal::grid::Dimensions;

    #[test]
    fn shows_what_it_is_fed_and_ignores_queries() {
        let events = Arc::new(Mutex::new(Vec::new()));
        let seen = events.clone();
        let mirror = Mirror::new(
            TermDimensions::new(20, 3),
            Arc::new(move |event| seen.lock().unwrap().push(event)),
        );
        // A device attributes query would make a session write its answer to the pty.
        mirror.feed(b"\x1b]0;title\x07hello\x1b[c\r\nworld\x07");
        mirror.feed(b"!");
        assert_eq!(mirror.visible_text(), ["hello", "world!", ""]);
        assert_eq!(
            *events.lock().unwrap(),
            [
                SessionEvent::TitleChanged(Some("title".into())),
                SessionEvent::Bell,
                SessionEvent::Wakeup,
            ]
        );
        mirror.ack_wakeup();
        mirror.feed(b"?");
        assert_eq!(events.lock().unwrap().last(), Some(&SessionEvent::Wakeup));
    }

    #[test]
    fn a_snapshot_recreates_the_mirror() {
        let callback: EventCallback = Arc::new(|_| {});
        let size = TermDimensions::new(10, 2);
        let mirror = Mirror::new(size, callback.clone());
        mirror.feed(b"one\r\ntwo\r\n\x1b[31mthree");
        let copy = Mirror::new(size, callback);
        copy.feed(&mirror.snapshot());
        assert_eq!(copy.visible_text(), ["two", "three"]);
        assert_eq!(copy.lock_term().grid().history_size(), 1);
    }

    #[test]
    fn its_text_has_the_scrollback_on_demand_and_no_trailing_empty_rows() {
        let mirror = Mirror::new(TermDimensions::new(10, 3), Arc::new(|_| {}));
        assert!(mirror.text(true).is_empty());
        mirror.feed("one  \r\ntwo\r\n\x1b[1mあい\x1b[0m\r\ne\u{301}\r\n".as_bytes());
        assert_eq!(mirror.text(false), ["あい", "e\u{301}"]);
        assert_eq!(mirror.text(true), ["one", "two", "あい", "e\u{301}"]);
        // An empty row inside the text stays.
        mirror.feed(b"\r\nend");
        assert_eq!(mirror.text(false), ["e\u{301}", "", "end"]);
        // The alternate screen has no scrollback.
        mirror.feed(b"\x1b[?1049h\x1b[Hfull");
        assert_eq!(mirror.text(true), ["full"]);
    }
}
