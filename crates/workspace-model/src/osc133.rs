//! Finding shell integration marks (OSC 133, "FinalTerm" semantic prompts) in a shell's output.
//!
//! The shell scripts the app injects print `ESC ] 133 ; X ; aw=<token> ... BEL` around the
//! prompt: `A` where the prompt starts, `B` where the command line starts (right after the
//! prompt), `C` when a command is about to run (with the command line, `cmd=<hex>`) and `D`
//! when it finished (with the shell's folder, `cwd=<hex>`). zsh also reports the command line while it is edited: `L` with
//! `cur=<characters before the cursor>;buf=<hex>`, plus `post=1` when zsh shows text after it
//! (zsh-autosuggestions). The token is random per session ([`new_mark_token`]) and only the
//! injected script knows it, so marks in a program's output (a `cat`ed file, a remote shell's
//! integration over ssh) are ignored.
//!
//! The terminal emulator ignores these, so the pty reader looks for them with
//! [`Osc133Scanner`], which keeps its state across reads so a mark split over two reads is
//! still found. It also follows synchronized updates (`CSI ? 2026 h` ... `l`), during which
//! the parser holds output back.

use std::hash::{BuildHasher, Hasher, RandomState};
use std::path::PathBuf;
use std::time::{SystemTime, UNIX_EPOCH};

/// The command line being edited, as the shell reported it.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct CommandLine {
    pub text: String,
    /// Characters before the cursor.
    pub cursor: usize,
    /// The shell shows text of its own after the command line (zsh-autosuggestions).
    pub shell_suggests: bool,
}

/// One of the OSC 133 marks.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum PromptMark {
    /// `A`: the prompt is about to be printed.
    PromptStart,
    /// `B`: the prompt ended; what follows is the command line the user edits.
    CommandStart,
    /// `C`: the command line was accepted and is about to run. `None` when the shell did not
    /// send it (too long, or left out of the shell's own history).
    CommandExecuted(Option<String>),
    /// `D`: the command finished (or the prompt was redrawn without running one), with the
    /// shell's folder when the script sent it.
    CommandFinished(Option<PathBuf>),
    /// `L` (zsh): the command line as it is edited; `None` when it was too long to send.
    CommandLine(Option<CommandLine>),
}

const ESC: u8 = 0x1b;
const BEL: u8 = 0x07;
/// CAN and SUB abort an escape sequence.
const CAN: u8 = 0x18;
const SUB: u8 = 0x1a;
const MARK_PREFIX: &[u8] = b"133;";
/// Kept of a mark's payload: the scripts send command lines of up to 4096 bytes, hex-encoded.
const MAX_PAYLOAD: usize = 8192 + 256;
/// Kept of a CSI's parameters: enough for `?2026`.
const MAX_CSI: usize = 8;
const SYNC_MODE: &[u8] = b"?2026";

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
enum State {
    #[default]
    Ground,
    Escape,
    Csi,
    Osc,
    /// ESC inside an OSC: `\` ends it (ST), anything else aborts it and starts a new escape.
    OscEscape,
}

/// Finds OSC 133 marks carrying the session's token in a byte stream fed in arbitrary pieces.
#[derive(Clone, Debug)]
pub struct Osc133Scanner {
    token: Vec<u8>,
    state: State,
    payload: Vec<u8>,
    /// The payload was longer than [`MAX_PAYLOAD`].
    truncated: bool,
    csi: Vec<u8>,
    in_sync: bool,
}

impl Osc133Scanner {
    /// A scanner for marks carrying `token`.
    pub fn new(token: &str) -> Self {
        Self {
            token: token.as_bytes().to_vec(),
            state: State::Ground,
            payload: Vec::new(),
            truncated: false,
            csi: Vec::new(),
            in_sync: false,
        }
    }

    /// Whether the bytes scanned so far end inside a synchronized update.
    pub fn in_sync(&self) -> bool {
        self.in_sync
    }

    /// Scan `bytes`, continuing from the previous call. Returns the first mark whose
    /// terminator lies in `bytes`, with the index just past that terminator; the caller scans
    /// the rest of `bytes` with another call. `None` when no mark ends in `bytes`.
    pub fn scan(&mut self, bytes: &[u8]) -> Option<(usize, PromptMark)> {
        for (index, &byte) in bytes.iter().enumerate() {
            if let Some(mark) = self.advance(byte) {
                return Some((index + 1, mark));
            }
        }
        None
    }

    fn advance(&mut self, byte: u8) -> Option<PromptMark> {
        match self.state {
            State::Ground => {
                if byte == ESC {
                    self.state = State::Escape;
                }
            }
            State::Escape => self.escape(byte),
            State::Csi => match byte {
                ESC => self.state = State::Escape,
                CAN | SUB => self.state = State::Ground,
                0x20..=0x3f => {
                    if self.csi.len() <= MAX_CSI {
                        self.csi.push(byte);
                    }
                }
                0x40..=0x7e => {
                    self.state = State::Ground;
                    if self.csi == SYNC_MODE {
                        match byte {
                            b'h' => self.in_sync = true,
                            b'l' => self.in_sync = false,
                            _ => {}
                        }
                    }
                }
                // Other C0 controls run inside a CSI without ending it.
                0x00..=0x1f => {}
                _ => self.state = State::Ground,
            },
            State::Osc => match byte {
                BEL => return self.finish(),
                ESC => self.state = State::OscEscape,
                CAN | SUB => self.state = State::Ground,
                _ => self.push_payload(byte),
            },
            State::OscEscape => {
                if byte == b'\\' {
                    return self.finish();
                }
                self.escape(byte);
            }
        }
        None
    }

    /// Keep a payload byte, as long as the payload may still be a mark.
    fn push_payload(&mut self, byte: u8) {
        if self.truncated {
            return;
        }
        let length = self.payload.len();
        let keep = match MARK_PREFIX.get(length) {
            Some(&expected) => byte == expected,
            None => length < MAX_PAYLOAD,
        };
        if keep {
            self.payload.push(byte);
        } else {
            // Not a mark (the prefix differs), or too long to keep.
            self.truncated = true;
        }
    }

    /// The byte after an ESC.
    fn escape(&mut self, byte: u8) {
        self.state = match byte {
            b']' => {
                self.payload.clear();
                self.truncated = false;
                State::Osc
            }
            b'[' => {
                self.csi.clear();
                State::Csi
            }
            ESC => State::Escape,
            _ => State::Ground,
        };
    }

    fn finish(&mut self) -> Option<PromptMark> {
        self.state = State::Ground;
        let rest = self.payload.strip_prefix(MARK_PREFIX)?;
        let (&letter, after) = rest.split_first()?;
        let mut params = Params::default();
        if !after.is_empty() {
            let after = after.strip_prefix(b";")?;
            for param in after.split(|&byte| byte == b';') {
                let (key, value) = match param.iter().position(|&byte| byte == b'=') {
                    Some(at) => (&param[..at], &param[at + 1..]),
                    None => (param, &b""[..]),
                };
                match key {
                    b"aw" => params.token = Some(value),
                    b"cmd" => params.command = Some(value),
                    b"cwd" => params.folder = Some(value),
                    b"cur" => params.cursor = Some(value),
                    b"buf" => params.buffer = Some(value),
                    b"post" => params.post = value == b"1",
                    _ => {}
                }
            }
        }
        if self.token.is_empty() || params.token != Some(self.token.as_slice()) {
            return None;
        }
        // A cut payload may end in the middle of the text.
        let text = |value: Option<&[u8]>| {
            value
                .filter(|_| !self.truncated)
                .and_then(decode_hex)
                .and_then(|bytes| String::from_utf8(bytes).ok())
        };
        Some(match letter {
            b'A' => PromptMark::PromptStart,
            b'B' => PromptMark::CommandStart,
            b'C' => PromptMark::CommandExecuted(text(params.command)),
            b'D' => PromptMark::CommandFinished(text(params.folder).map(PathBuf::from)),
            b'L' => {
                let cursor = params
                    .cursor
                    .and_then(|value| std::str::from_utf8(value).ok()?.parse().ok());
                let line = text(params.buffer)
                    .zip(cursor)
                    .map(|(text, cursor)| CommandLine {
                        text,
                        cursor,
                        shell_suggests: params.post,
                    });
                PromptMark::CommandLine(line)
            }
            _ => return None,
        })
    }
}

#[derive(Default)]
struct Params<'a> {
    token: Option<&'a [u8]>,
    command: Option<&'a [u8]>,
    folder: Option<&'a [u8]>,
    cursor: Option<&'a [u8]>,
    buffer: Option<&'a [u8]>,
    post: bool,
}

/// Bytes from hex digits (either case); `None` when `hex` is not hex.
fn decode_hex(hex: &[u8]) -> Option<Vec<u8>> {
    if !hex.len().is_multiple_of(2) {
        return None;
    }
    let digit = |byte: u8| (byte as char).to_digit(16).map(|value| value as u8);
    hex.chunks(2)
        .map(|pair| Some(digit(pair[0])? << 4 | digit(pair[1])?))
        .collect()
}

/// A random token for a session's marks: 32 hex digits.
pub fn new_mark_token() -> String {
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |since| since.as_nanos());
    // Each `RandomState` has its own keys, seeded from the OS.
    (0..2u8)
        .map(|round| {
            let mut hasher = RandomState::new().build_hasher();
            hasher.write_u8(round);
            hasher.write_u128(nanos);
            hasher.write_u32(std::process::id());
            format!("{:016x}", hasher.finish())
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    const TOKEN: &str = "t0k";

    fn marks(chunks: &[&[u8]]) -> Vec<(usize, usize, PromptMark)> {
        let mut scanner = Osc133Scanner::new(TOKEN);
        let mut found = Vec::new();
        for (chunk_index, chunk) in chunks.iter().enumerate() {
            let mut offset = 0;
            while let Some((end, mark)) = scanner.scan(&chunk[offset..]) {
                offset += end;
                found.push((chunk_index, offset, mark));
            }
        }
        found
    }

    fn kinds(chunks: &[&[u8]]) -> Vec<PromptMark> {
        marks(chunks).into_iter().map(|(_, _, mark)| mark).collect()
    }

    #[test]
    fn finds_marks_with_either_terminator() {
        let output = b"\x1b]133;A;aw=t0k\x07$ \x1b]133;B;aw=t0k\x1b\\ls\r\n\x1b]133;C;aw=t0k\x07";
        let found = marks(&[output]);
        assert_eq!(
            found,
            vec![
                (0, 15, PromptMark::PromptStart),
                (0, 33, PromptMark::CommandStart),
                (0, 52, PromptMark::CommandExecuted(None)),
            ]
        );
    }

    #[test]
    fn marks_without_the_token_are_ignored() {
        assert!(marks(&[b"\x1b]133;A\x07"]).is_empty());
        assert!(marks(&[b"\x1b]133;B;aw=other\x07"]).is_empty());
        assert!(marks(&[b"\x1b]133;C;cmd=6c73;aw=t0kx\x07"]).is_empty());
        let mut scanner = Osc133Scanner::new("");
        assert_eq!(scanner.scan(b"\x1b]133;A;aw=\x07"), None);
    }

    #[test]
    fn command_lines_are_decoded() {
        assert_eq!(
            kinds(&[b"\x1b]133;C;aw=t0k;cmd=6769742073C3A9\x07"]),
            [PromptMark::CommandExecuted(Some("git s\u{e9}".into()))]
        );
        // Not hex, odd length, not UTF-8: the mark counts, without its command.
        for bad in [&b"zz"[..], b"676", b"ff"] {
            let mut output = b"\x1b]133;C;aw=t0k;cmd=".to_vec();
            output.extend_from_slice(bad);
            output.push(BEL);
            assert_eq!(kinds(&[&output]), [PromptMark::CommandExecuted(None)]);
        }
        assert_eq!(
            kinds(&[b"\x1b]133;L;aw=t0k;cur=1;buf=6c73;post=1\x07"]),
            [PromptMark::CommandLine(Some(CommandLine {
                text: "ls".into(),
                cursor: 1,
                shell_suggests: true,
            }))]
        );
        assert_eq!(
            kinds(&[b"\x1b]133;L;aw=t0k\x07"]),
            [PromptMark::CommandLine(None)]
        );
    }

    #[test]
    fn folders_are_decoded() {
        assert_eq!(
            kinds(&[b"\x1b]133;D;aw=t0k;cwd=433a5cc3a9\x07"]),
            [PromptMark::CommandFinished(Some(PathBuf::from(
                "C:\\\u{e9}"
            )))]
        );
        assert_eq!(
            kinds(&[b"\x1b]133;D;aw=t0k;cwd=zz\x07"]),
            [PromptMark::CommandFinished(None)]
        );
    }

    #[test]
    fn mark_split_across_reads_is_found_where_it_ends() {
        let found = marks(&[b"text\x1b]13", b"3;B;aw=", b"t0k\x07rest"]);
        assert_eq!(found, vec![(2, 4, PromptMark::CommandStart)]);
        let found = marks(&[b"\x1b]133;D;aw=t0k\x1b", b"\\"]);
        assert_eq!(found, vec![(1, 1, PromptMark::CommandFinished(None))]);
    }

    #[test]
    fn other_sequences_are_ignored() {
        assert!(marks(&[b"\x1b]0;133;A;aw=t0k\x07"]).is_empty());
        assert!(marks(&[b"\x1b]1337;A;aw=t0k\x07"]).is_empty());
        assert!(marks(&[b"\x1b]133;AB;aw=t0k\x07"]).is_empty());
        assert!(marks(&[b"\x1b]133;E;aw=t0k\x07"]).is_empty());
        assert!(marks(&[b"\x1b[133;A;aw=t0k\x07"]).is_empty());
        assert!(marks(&[b"]133;A;aw=t0k\x07"]).is_empty());
    }

    #[test]
    fn aborted_osc_is_not_a_mark() {
        assert!(marks(&[b"\x1b]133;A;aw=t0k\x18\x07"]).is_empty());
        // ESC that is not ST aborts the OSC and starts a new sequence.
        let found = marks(&[b"\x1b]133;A\x1b]133;B;aw=t0k\x07"]);
        assert_eq!(found, vec![(0, 22, PromptMark::CommandStart)]);
    }

    #[test]
    fn long_payloads_do_not_grow_the_buffer() {
        let mut scanner = Osc133Scanner::new(TOKEN);
        let mut long = b"\x1b]52;c;".to_vec();
        long.extend(std::iter::repeat_n(b'x', 100_000));
        assert_eq!(scanner.scan(&long), None);
        assert!(scanner.payload.len() <= MARK_PREFIX.len());
        assert_eq!(
            scanner.scan(b"\x07\x1b]133;B;aw=t0k\x07"),
            Some((16, PromptMark::CommandStart))
        );
        // A mark too long to keep still counts, without its text.
        let mut long = b"\x1b]133;C;aw=t0k;cmd=".to_vec();
        long.extend(std::iter::repeat_n(b'6', 100_000));
        long.push(BEL);
        assert_eq!(
            scanner.scan(&long),
            Some((long.len(), PromptMark::CommandExecuted(None)))
        );
        assert!(scanner.payload.len() <= MAX_PAYLOAD);
    }

    #[test]
    fn synchronized_updates_are_followed() {
        let mut scanner = Osc133Scanner::new(TOKEN);
        assert_eq!(scanner.scan(b"\x1b[?2026h"), None);
        assert!(scanner.in_sync());
        assert!(scanner.scan(b"\x1b]133;A;aw=t0k\x07").is_some());
        assert!(scanner.in_sync());
        scanner.scan(b"\x1b[?20");
        assert!(scanner.in_sync());
        scanner.scan(b"26l");
        assert!(!scanner.in_sync());
        // Other modes and other final bytes leave it alone.
        scanner.scan(b"\x1b[?2026$p\x1b[?25h\x1b[2026h");
        assert!(!scanner.in_sync());
    }

    #[test]
    fn tokens_are_random_hex() {
        let token = new_mark_token();
        assert_eq!(token.len(), 32);
        assert!(token.bytes().all(|byte| byte.is_ascii_hexdigit()));
        assert_ne!(token, new_mark_token());
    }
}
