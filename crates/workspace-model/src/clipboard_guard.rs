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
    always: bool,
}

impl ClipboardGuard {
    /// A guard that starts allowed when the tab already was (its agent's tab going on as a
    /// shell is still the same tab).
    pub fn new(always: bool) -> Self {
        Self {
            pending: ClipboardWrites::default(),
            always,
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
        ClipboardOffer::Held { first }
    }

    /// The held writes, while the notice shows.
    pub fn pending(&self) -> Option<&ClipboardWrites> {
        (!self.pending.is_empty()).then_some(&self.pending)
    }

    /// Allow: the held writes to apply now (empty when none were held).
    pub fn allow(&mut self) -> ClipboardWrites {
        std::mem::take(&mut self.pending)
    }

    /// "Always for this tab": allow the held writes and every later one.
    pub fn allow_always(&mut self) -> ClipboardWrites {
        self.always = true;
        self.allow()
    }

    /// Deny (also when the program exits): drop the held writes. Returns whether any were held.
    pub fn deny(&mut self) -> bool {
        !std::mem::take(&mut self.pending).is_empty()
    }

    /// Whether the user chose "Always for this tab".
    pub fn always(&self) -> bool {
        self.always
    }
}

/// What the notice says about the held writes.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ClipboardNotice {
    /// For example "A program wants to copy 12 characters (2 lines) to the clipboard".
    pub message: String,
    /// The text on one line, shortened to [`CLIPBOARD_PREVIEW_CHARS`]; `None` when it is blank.
    pub preview: Option<String>,
}

impl ClipboardWrites {
    /// The notice for these writes. The clipboard's text is described when both targets are
    /// written; the program chooses the text, so the preview has no control characters.
    pub fn notice(&self) -> ClipboardNotice {
        let target = match (&self.clipboard, &self.selection) {
            (Some(_), Some(_)) => "the clipboard and the primary selection",
            (None, Some(_)) => "the primary selection",
            _ => "the clipboard",
        };
        let text = self
            .clipboard
            .as_deref()
            .or(self.selection.as_deref())
            .unwrap_or_default();
        ClipboardNotice {
            message: format!("A program wants to copy {} to {target}", size_label(text)),
            preview: command_label(text, CLIPBOARD_PREVIEW_CHARS),
        }
    }
}

/// "1 character", "12 characters (2 lines)".
fn size_label(text: &str) -> String {
    let chars = text.chars().count();
    let mut label = if chars == 1 {
        "1 character".to_string()
    } else {
        format!("{chars} characters")
    };
    let lines = text.trim_end_matches(['\n', '\r']).lines().count();
    if lines > 1 {
        label.push_str(&format!(" ({lines} lines)"));
    }
    label
}

#[cfg(test)]
mod tests {
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
        let writes: Vec<_> = guard.allow().into_writes().collect();
        assert_eq!(writes, [(ClipboardTarget::Clipboard, "two".to_string())]);
        assert_eq!(guard.pending(), None);
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
        assert_eq!(guard.allow().into_writes().count(), 0);
    }

    #[test]
    fn both_targets_wait_for_one_answer() {
        let mut guard = ClipboardGuard::default();
        guard.offer(ClipboardTarget::Selection, "sel".into());
        assert_eq!(
            guard.offer(ClipboardTarget::Clipboard, "clip".into()),
            ClipboardOffer::Held { first: false }
        );
        let writes: Vec<_> = guard.allow().into_writes().collect();
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
        let writes: Vec<_> = guard.allow_always().into_writes().collect();
        assert_eq!(writes, [(ClipboardTarget::Clipboard, "held".to_string())]);
        assert!(guard.always());
        assert_eq!(
            guard.offer(ClipboardTarget::Selection, "later".into()),
            ClipboardOffer::Apply(ClipboardTarget::Selection, "later".into())
        );
        assert_eq!(guard.pending(), None);
        let inherited = ClipboardGuard::new(guard.always());
        assert!(inherited.always());
        assert!(!ClipboardGuard::new(false).always());
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
        assert_eq!(
            notice(Some("ab"), Some("other")).message,
            "A program wants to copy 2 characters to the clipboard and the primary selection"
        );
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
        let bidi = notice(Some("abc\u{202E}fed"), None);
        assert_eq!(bidi.preview.as_deref(), Some("abc fed"));
    }
}
