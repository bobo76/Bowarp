//! Clipboard writes from programs (OSC 52) wait for the user's answer. A held write is applied
//! on Allow and dropped on Deny; a newer write to the same target replaces it. "Always for
//! this tab" applies the held write and every later one until the tab closes. This is a
//! safeguard, not a notification: no switch turns it off.

use crate::running::command_label;

/// Characters of held text the notice shows; a longer text is cut with `…`.
pub const CLIPBOARD_PREVIEW_CHARS: usize = 60;

/// Where a program asked to put text.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ClipboardTarget {
    Clipboard,
    /// The primary selection (X11/Wayland), pasted with the middle button.
    Selection,
}

/// Held text per target: a program may write both the clipboard and the primary selection
/// for one copy, and both wait for the same answer.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct ClipboardWrites {
    pub clipboard: Option<String>,
    pub selection: Option<String>,
}

impl ClipboardWrites {
    fn is_empty(&self) -> bool {
        self.clipboard.is_none() && self.selection.is_none()
    }

    fn slot(&mut self, target: ClipboardTarget) -> &mut Option<String> {
        match target {
            ClipboardTarget::Clipboard => &mut self.clipboard,
            ClipboardTarget::Selection => &mut self.selection,
        }
    }

    /// The notice for these writes. The program chooses the text, so the preview has no
    /// control characters. Only the start of a long text is looked at for the preview.
    fn notice(&self) -> ClipboardNotice {
        let (clipboard, selection) = (self.clipboard.as_deref(), self.selection.as_deref());
        match (clipboard, selection) {
            (Some(clip), Some(sel)) if clip != sel => ClipboardNotice {
                message: format!(
                    "A program wants to copy {} to the clipboard and {} to the primary selection",
                    size_label(clip),
                    size_label(sel)
                ),
                preview: split_preview(clip, sel),
            },
            _ => {
                let target = match (clipboard, selection) {
                    (Some(_), Some(_)) => "the clipboard and the primary selection",
                    (None, Some(_)) => "the primary selection",
                    _ => "the clipboard",
                };
                let text = clipboard.or(selection).unwrap_or_default();
                ClipboardNotice {
                    message: format!("A program wants to copy {} to {target}", size_label(text)),
                    preview: preview(text, CLIPBOARD_PREVIEW_CHARS),
                }
            }
        }
    }

    /// The writes to apply, clipboard first.
    pub fn into_writes(self) -> impl Iterator<Item = (ClipboardTarget, String)> {
        let clipboard = self
            .clipboard
            .map(|text| (ClipboardTarget::Clipboard, text));
        let selection = self
            .selection
            .map(|text| (ClipboardTarget::Selection, text));
        clipboard.into_iter().chain(selection)
    }
}

/// What the user answered.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ClipboardAnswer {
    Allow,
    Always,
    Deny,
}

/// What the notice says about the held writes.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ClipboardNotice {
    /// For example "A program wants to copy 12 characters (2 lines) to the clipboard".
    pub message: String,
    /// The text on one line, shortened to [`CLIPBOARD_PREVIEW_CHARS`]; `None` when it is blank.
    /// When the two targets get different texts it shows both, clipboard first, split by `|`.
    pub preview: Option<String>,
}

/// What to do with a write a program just sent.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ClipboardOffer {
    /// Allowed for the tab: write it now.
    Apply(ClipboardTarget, String),
    /// Held for the user's answer. `first` is true when nothing was held before, so the
    /// notice just appeared.
    Held { first: bool },
}

/// The tab's held clipboard write and whether the user allowed all of them.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct ClipboardGuard {
    pending: ClipboardWrites,
    /// The notice for `pending`, made once per held write (not on every frame).
    notice: Option<ClipboardNotice>,
    always: bool,
}

impl ClipboardGuard {
    /// A fresh guard for the same tab's next session: it keeps "Always for this tab" (an
    /// agent's tab going on as a shell is still the same tab) and drops any held write.
    pub fn carry_over(&self) -> Self {
        Self {
            always: self.always,
            ..Self::default()
        }
    }

    /// Take a write from the program: apply it when the tab allows all writes, else hold it,
    /// replacing an earlier held write to the same target.
    pub fn offer(&mut self, target: ClipboardTarget, text: String) -> ClipboardOffer {
        if self.always {
            return ClipboardOffer::Apply(target, text);
        }
        let first = self.pending.is_empty();
        *self.pending.slot(target) = Some(text);
        self.notice = Some(self.pending.notice());
        ClipboardOffer::Held { first }
    }

    /// The held writes, while the notice shows.
    pub fn pending(&self) -> Option<&ClipboardWrites> {
        (!self.pending.is_empty()).then_some(&self.pending)
    }

    /// What the notice says about the held writes; `None` when none are held.
    pub fn notice(&self) -> Option<&ClipboardNotice> {
        self.notice.as_ref()
    }

    /// Apply the user's answer. `None` when nothing was held (nothing to do); otherwise the
    /// writes to apply now (empty after Deny).
    pub fn answer(&mut self, answer: ClipboardAnswer) -> Option<ClipboardWrites> {
        if self.pending.is_empty() {
            return None;
        }
        self.notice = None;
        let held = std::mem::take(&mut self.pending);
        match answer {
            ClipboardAnswer::Allow => Some(held),
            ClipboardAnswer::Always => {
                self.always = true;
                Some(held)
            }
            ClipboardAnswer::Deny => Some(ClipboardWrites::default()),
        }
    }

    /// Drop the held writes (the program exited). Returns whether any were held.
    pub fn deny(&mut self) -> bool {
        self.answer(ClipboardAnswer::Deny).is_some()
    }

    /// Whether the user chose "Always for this tab".
    pub fn always(&self) -> bool {
        self.always
    }
}

/// `text` on one line of at most `max_chars` characters. Only a bounded start of `text` is
/// read: a program can send megabytes, and the preview shows a few dozen characters.
fn preview(text: &str, max_chars: usize) -> Option<String> {
    let end = text.floor_char_boundary(max_chars * 8);
    command_label(&text[..end], max_chars)
}

/// Both texts, each with half the preview, split by `|`.
fn split_preview(clipboard: &str, selection: &str) -> Option<String> {
    let half = CLIPBOARD_PREVIEW_CHARS / 2;
    let clipboard = preview(clipboard, half).unwrap_or_default();
    let selection = preview(selection, half).unwrap_or_default();
    Some(format!("{clipboard} | {selection}"))
}

/// "1 character", "12 characters (2 lines)".
fn size_label(text: &str) -> String {
    let chars = text.chars().count();
    let mut label = match chars {
        1 => "1 character".to_string(),
        _ => format!("{chars} characters"),
    };
    let lines = text.trim_end_matches(['\n', '\r']).lines().count();
    if lines > 1 {
        label.push_str(&format!(" ({lines} lines)"));
    }
    label
}

#[cfg(test)]
mod tests {
    use super::ClipboardAnswer::{Allow, Always, Deny};
    use super::*;

    fn held(guard: &ClipboardGuard) -> ClipboardWrites {
        guard.pending().cloned().unwrap_or_default()
    }

    #[test]
    fn writes_are_held_until_allowed() {
        let mut guard = ClipboardGuard::default();
        assert_eq!(guard.pending(), None);
        assert_eq!(
            guard.offer(ClipboardTarget::Clipboard, "one".into()),
            ClipboardOffer::Held { first: true }
        );
        assert_eq!(
            guard.offer(ClipboardTarget::Clipboard, "two".into()),
            ClipboardOffer::Held { first: false },
            "a newer write replaces the held one"
        );
        assert_eq!(held(&guard).clipboard.as_deref(), Some("two"));
        assert_eq!(
            guard.notice().map(|notice| notice.message.as_str()),
            Some("A program wants to copy 3 characters to the clipboard")
        );
        let writes: Vec<_> = guard.answer(Allow).unwrap().into_writes().collect();
        assert_eq!(writes, [(ClipboardTarget::Clipboard, "two".to_string())]);
        assert_eq!(guard.pending(), None);
        assert_eq!(guard.notice(), None);
        assert!(!guard.always());
        assert_eq!(
            guard.offer(ClipboardTarget::Clipboard, "three".into()),
            ClipboardOffer::Held { first: true },
            "Allow is for one write"
        );
    }

    #[test]
    fn deny_drops_the_held_writes() {
        let mut guard = ClipboardGuard::default();
        assert!(!guard.deny());
        guard.offer(ClipboardTarget::Clipboard, "a".into());
        guard.offer(ClipboardTarget::Selection, "b".into());
        assert!(guard.deny());
        assert_eq!(guard.pending(), None);
        assert_eq!(guard.answer(Allow), None);
        assert_eq!(guard.notice(), None);
    }

    #[test]
    fn an_answer_with_nothing_held_does_nothing() {
        let mut guard = ClipboardGuard::default();
        for answer in [Allow, Always, Deny] {
            assert_eq!(guard.answer(answer), None);
        }
        assert!(!guard.always());
        guard.offer(ClipboardTarget::Clipboard, "a".into());
        assert_eq!(guard.answer(Deny), Some(ClipboardWrites::default()));
    }

    #[test]
    fn both_targets_wait_for_one_answer() {
        let mut guard = ClipboardGuard::default();
        guard.offer(ClipboardTarget::Selection, "sel".into());
        assert_eq!(
            guard.offer(ClipboardTarget::Clipboard, "clip".into()),
            ClipboardOffer::Held { first: false }
        );
        let writes: Vec<_> = guard.answer(Allow).unwrap().into_writes().collect();
        assert_eq!(
            writes,
            [
                (ClipboardTarget::Clipboard, "clip".to_string()),
                (ClipboardTarget::Selection, "sel".to_string()),
            ]
        );
    }

    #[test]
    fn always_applies_the_held_write_and_later_ones() {
        let mut guard = ClipboardGuard::default();
        guard.offer(ClipboardTarget::Clipboard, "held".into());
        let writes: Vec<_> = guard.answer(Always).unwrap().into_writes().collect();
        assert_eq!(writes, [(ClipboardTarget::Clipboard, "held".to_string())]);
        assert!(guard.always());
        assert_eq!(
            guard.offer(ClipboardTarget::Selection, "later".into()),
            ClipboardOffer::Apply(ClipboardTarget::Selection, "later".into())
        );
        assert_eq!(guard.pending(), None);
        guard.always = false;
        guard.offer(ClipboardTarget::Clipboard, "held".into());
        assert!(!guard.carry_over().always());
        guard.always = true;
        let carried = guard.carry_over();
        assert!(carried.always());
        assert_eq!(carried.pending(), None);
    }

    #[test]
    fn notice_describes_size_target_and_a_safe_preview() {
        let notice = |clipboard: Option<&str>, selection: Option<&str>| {
            ClipboardWrites {
                clipboard: clipboard.map(String::from),
                selection: selection.map(String::from),
            }
            .notice()
        };
        assert_eq!(
            notice(Some("hello"), None),
            ClipboardNotice {
                message: "A program wants to copy 5 characters to the clipboard".into(),
                preview: Some("hello".into()),
            }
        );
        assert_eq!(
            notice(None, Some("x")).message,
            "A program wants to copy 1 character to the primary selection"
        );
        let same = notice(Some("ab"), Some("ab"));
        assert_eq!(
            same.message,
            "A program wants to copy 2 characters to the clipboard and the primary selection"
        );
        assert_eq!(same.preview.as_deref(), Some("ab"));
        let different = notice(Some("ab"), Some("rm -rf ~"));
        assert_eq!(
            different.message,
            "A program wants to copy 2 characters to the clipboard and 8 characters to the \
             primary selection"
        );
        assert_eq!(different.preview.as_deref(), Some("ab | rm -rf ~"));
        let multi = notice(Some("rm -rf ~\n\u{1b}[2Jecho hi\n"), None);
        assert_eq!(
            multi.message,
            "A program wants to copy 21 characters (2 lines) to the clipboard"
        );
        assert_eq!(multi.preview.as_deref(), Some("rm -rf ~ [2Jecho hi"));
        let blank = notice(Some(""), None);
        assert_eq!(
            blank.message,
            "A program wants to copy 0 characters to the clipboard"
        );
        assert_eq!(blank.preview, None);
        let long = notice(Some(&"y".repeat(500)), None);
        assert_eq!(
            long.preview.map(|preview| preview.chars().count()),
            Some(CLIPBOARD_PREVIEW_CHARS)
        );
        let huge = notice(Some(&"é".repeat(100_000)), None);
        assert_eq!(
            huge.message,
            "A program wants to copy 100000 characters to the clipboard"
        );
        assert_eq!(
            huge.preview.map(|preview| preview.chars().count()),
            Some(CLIPBOARD_PREVIEW_CHARS)
        );
        let bidi = notice(Some("abc\u{202E}fed"), None);
        assert_eq!(bidi.preview.as_deref(), Some("abc fed"));
    }
}
