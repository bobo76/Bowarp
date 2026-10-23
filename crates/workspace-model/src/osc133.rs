//! Finding shell integration marks (OSC 133, "FinalTerm" semantic prompts) in a shell's output.
//!
//! The shell scripts the app injects print `ESC ] 133 ; X ; aw=<token> ... BEL` around the
//! prompt: `A` where the prompt starts, `B` where the command line starts (right after the
//! prompt), `C` when a command is about to run (with the command line, `cmd=<hex>`) and `D`
//! when it finished (with the shell's folder, `cwd=<hex>`, or for cmd.exe, which cannot print
//! hex, `path=<text>` as the last parameter, a Windows path possibly holding `;`). zsh also
//! reports the command line while it is edited: `L` with `cur=<characters before the
//! cursor>;buf=<hex>`, plus `post=1` when zsh shows text after it (zsh-autosuggestions). The
//! token is random per session ([`new_mark_token`]) and only the injected script knows it, so
//! marks in a program's output (a `cat`ed file, a remote shell's integration over ssh) are
//! ignored.
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
/// The folder as text: the last parameter, everything after it being the path.
const PATH_PARAM: &[u8] = b";path=";
const TOKEN_KEY: &[u8] = b"aw";
const COMMAND_KEY: &[u8] = b"cmd";
const FOLDER_KEY: &[u8] = b"cwd";
const CURSOR_KEY: &[u8] = b"cur";
const BUFFER_KEY: &[u8] = b"buf";
const POST_KEY: &[u8] = b"post";
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
            State::Csi => self.advance_csi(byte),
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

    fn advance_csi(&mut self, byte: u8) {
        match byte {
            ESC => self.state = State::Escape,
            CAN | SUB => self.state = State::Ground,
            0x20..=0x3f => {
                if self.csi.len() <= MAX_CSI {
                    self.csi.push(byte);
                }
            }
            0x40..=0x7e => {
                self.state = State::Ground;
                if self.csi == SYNC_MODE && matches!(byte, b'h' | b'l') {
                    self.in_sync = byte == b'h';
                }
            }
            // Other C0 controls run inside a CSI without ending it.
            0x00..=0x1f => {}
            _ => self.state = State::Ground,
        }
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
        let params = Params::parse(after)?;
        if self.token.is_empty() || params.token != Some(self.token.as_slice()) {
            return None;
        }
        params.into_mark(letter, self.truncated)
    }
}

#[derive(Default)]
struct Params<'a> {
    token: Option<&'a [u8]>,
    command: Option<&'a [u8]>,
    folder: Option<&'a [u8]>,
    cursor: Option<&'a [u8]>,
    buffer: Option<&'a [u8]>,
    path: Option<&'a [u8]>,
    post: bool,
}

impl<'a> Params<'a> {
    fn parse(after: &'a [u8]) -> Option<Self> {
        let (after, path) = match after
            .windows(PATH_PARAM.len())
            .position(|window| window == PATH_PARAM)
        {
            Some(at) => (&after[..at], Some(&after[at + PATH_PARAM.len()..])),
            None => (after, None),
        };
        let mut params = Self {
            path,
            ..Self::default()
        };
        if !after.is_empty() {
            let after = after.strip_prefix(b";")?;
            for param in after.split(|&byte| byte == b';') {
                let (key, value) = match param.iter().position(|&byte| byte == b'=') {
                    Some(at) => (&param[..at], &param[at + 1..]),
                    None => (param, &b""[..]),
                };
                params.set(key, value);
            }
        }
        Some(params)
    }

    fn set(&mut self, key: &[u8], value: &'a [u8]) {
        match key {
            TOKEN_KEY => self.token = Some(value),
            COMMAND_KEY => self.command = Some(value),
            FOLDER_KEY => self.folder = Some(value),
            CURSOR_KEY => self.cursor = Some(value),
            BUFFER_KEY => self.buffer = Some(value),
            POST_KEY => self.post = value == b"1",
            _ => {}
        }
    }

    fn into_mark(self, letter: u8, truncated: bool) -> Option<PromptMark> {
        // A cut payload may end in the middle of the text.
        let text = |value: Option<&[u8]>| {
            value
                .filter(|_| !truncated)
                .and_then(decode_hex)
                .and_then(|bytes| String::from_utf8(bytes).ok())
        };
        Some(match letter {
            b'A' => PromptMark::PromptStart,
            b'B' => PromptMark::CommandStart,
            b'C' => PromptMark::CommandExecuted(text(self.command)),
            b'D' => {
                let path = self
                    .path
                    .filter(|path| !truncated && !path.is_empty())
                    .and_then(|path| String::from_utf8(path.to_vec()).ok());
                PromptMark::CommandFinished(text(self.folder).or(path).map(PathBuf::from))
            }
            b'L' => {
                let cursor = self
                    .cursor
                    .and_then(|value| std::str::from_utf8(value).ok()?.parse().ok());
                let line = text(self.buffer)
                    .zip(cursor)
                    .map(|(text, cursor)| CommandLine {
                        text,
                        cursor,
                        shell_suggests: self.post,
                    });
                PromptMark::CommandLine(line)
            }
            _ => return None,
        })
    }
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
mod tests;
