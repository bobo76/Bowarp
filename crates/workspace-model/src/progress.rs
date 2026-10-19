//! Finding progress reports (OSC 9;4, from ConEmu) in a program's output.
//!
//! A program reports progress with `ESC ] 9 ; 4 ; <state> [; <percent>] ST`; Windows Terminal
//! shows it as a spinner or bar on the tab. Copilot CLI shows progress while it works on a
//! prompt and is expected to clear it when the turn ends, so it stands in for the hooks Copilot
//! lacks. The terminal emulator ignores these, so the pty reader looks for them with
//! [`ProgressScanner`], which keeps its state across reads so a report split over two reads is
//! still found.

/// The state a progress report sets.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ProgressState {
    /// `0`, or no state: the progress is removed.
    Hidden,
    /// `1`: a percentage.
    Normal,
    /// `2`: a percentage, shown as failed.
    Error,
    /// `3`: busy without a percentage.
    Indeterminate,
    /// `4`: a percentage, shown as paused.
    Paused,
}

impl ProgressState {
    /// Whether the program shows progress, whatever its kind.
    pub fn is_shown(self) -> bool {
        self != Self::Hidden
    }

    fn from_param(param: &[u8]) -> Option<Self> {
        Some(match param {
            b"" | b"0" => Self::Hidden,
            b"1" => Self::Normal,
            b"2" => Self::Error,
            b"3" => Self::Indeterminate,
            b"4" => Self::Paused,
            _ => return None,
        })
    }
}

const ESC: u8 = 0x1b;
const BEL: u8 = 0x07;
/// CAN and SUB abort an escape sequence.
const CAN: u8 = 0x18;
const SUB: u8 = 0x1a;
const PROGRESS_PREFIX: &[u8] = b"9;4";
/// Kept of a report's payload: `9;4;<state>;<percent>` fits easily.
const MAX_PAYLOAD: usize = 16;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
enum State {
    #[default]
    Ground,
    Escape,
    Osc,
    /// ESC inside an OSC: `\` ends it (ST), anything else aborts it and starts a new escape.
    OscEscape,
}

/// Finds progress reports in a byte stream fed in arbitrary pieces.
#[derive(Clone, Debug, Default)]
pub struct ProgressScanner {
    state: State,
    payload: Vec<u8>,
    /// The payload is not a progress report, or longer than [`MAX_PAYLOAD`].
    skipped: bool,
}

impl ProgressScanner {
    pub fn new() -> Self {
        Self::default()
    }

    /// Scan `bytes`, continuing from the previous call. Returns the state of the last report
    /// whose terminator lies in `bytes`.
    pub fn scan(&mut self, bytes: &[u8]) -> Option<ProgressState> {
        bytes
            .iter()
            .fold(None, |last, &byte| self.advance(byte).or(last))
    }

    fn advance(&mut self, byte: u8) -> Option<ProgressState> {
        match self.state {
            State::Ground => {
                if byte == ESC {
                    self.state = State::Escape;
                }
            }
            State::Escape => self.escape(byte),
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

    /// Keep a payload byte, as long as the payload may still be a progress report.
    fn push_payload(&mut self, byte: u8) {
        if self.skipped {
            return;
        }
        let length = self.payload.len();
        let keep = match PROGRESS_PREFIX.get(length) {
            Some(&expected) => byte == expected,
            None => length < MAX_PAYLOAD,
        };
        if keep {
            self.payload.push(byte);
        } else {
            self.skipped = true;
        }
    }

    /// The byte after an ESC.
    fn escape(&mut self, byte: u8) {
        self.state = match byte {
            b']' => {
                self.payload.clear();
                self.skipped = false;
                State::Osc
            }
            ESC => State::Escape,
            _ => State::Ground,
        };
    }

    fn finish(&mut self) -> Option<ProgressState> {
        self.state = State::Ground;
        if self.skipped {
            return None;
        }
        let rest = self.payload.strip_prefix(PROGRESS_PREFIX)?;
        if rest.is_empty() {
            return Some(ProgressState::Hidden);
        }
        let params = rest.strip_prefix(b";")?;
        let state = params
            .split(|&byte| byte == b';')
            .next()
            .unwrap_or_default();
        ProgressState::from_param(state)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn scan_all(chunks: &[&[u8]]) -> Vec<Option<ProgressState>> {
        let mut scanner = ProgressScanner::new();
        chunks.iter().map(|chunk| scanner.scan(chunk)).collect()
    }

    #[test]
    fn reads_each_state_with_either_terminator() {
        let cases: [(&[u8], ProgressState); 6] = [
            (b"\x1b]9;4;0\x07", ProgressState::Hidden),
            (b"\x1b]9;4;1;40\x07", ProgressState::Normal),
            (b"\x1b]9;4;2;100\x1b\\", ProgressState::Error),
            (b"\x1b]9;4;3\x1b\\", ProgressState::Indeterminate),
            (b"\x1b]9;4;4;10\x07", ProgressState::Paused),
            (b"\x1b]9;4\x07", ProgressState::Hidden),
        ];
        for (output, expected) in cases {
            assert_eq!(
                scan_all(&[output]),
                vec![Some(expected)],
                "{}",
                output.escape_ascii()
            );
        }
    }

    #[test]
    fn returns_the_last_report_in_a_read() {
        let output = b"\x1b]9;4;3\x07working\x1b]9;4;0\x07done";
        assert_eq!(scan_all(&[output]), vec![Some(ProgressState::Hidden)]);
    }

    #[test]
    fn finds_a_report_split_over_reads() {
        assert_eq!(
            scan_all(&[b"text\x1b]9;", b"4;3", b"\x1b", b"\\more"]),
            vec![None, None, None, Some(ProgressState::Indeterminate)]
        );
    }

    #[test]
    fn ignores_other_sequences_and_unknown_states() {
        assert_eq!(scan_all(&[b"\x1b]0;title\x07"]), vec![None]);
        assert_eq!(scan_all(&[b"\x1b]9;1;notification\x07"]), vec![None]);
        assert_eq!(scan_all(&[b"\x1b]9;40\x07"]), vec![None]);
        assert_eq!(scan_all(&[b"\x1b]9;4;7\x07"]), vec![None]);
        assert_eq!(scan_all(&[b"\x1b]133;A;aw=t0k\x07"]), vec![None]);
        assert_eq!(scan_all(&[b"9;4;3\x07"]), vec![None]);
    }

    #[test]
    fn aborted_report_is_ignored() {
        assert_eq!(scan_all(&[b"\x1b]9;4;3\x18\x07"]), vec![None]);
        // ESC that is not ST aborts the report and starts a new sequence.
        assert_eq!(
            scan_all(&[b"\x1b]9;4;3\x1b]9;4;0\x07"]),
            vec![Some(ProgressState::Hidden)]
        );
    }

    #[test]
    fn overlong_payload_is_ignored() {
        assert_eq!(scan_all(&[b"\x1b]9;4;3;12345678901234567\x07"]), vec![None]);
    }
}
