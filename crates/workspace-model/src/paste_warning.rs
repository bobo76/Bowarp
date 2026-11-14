//! The question asked before pasting text with a line break while the program has not turned
//! on bracketed paste: each line break then reaches it as Enter, so the lines run at once. The
//! caller checks the mode; this module only reads the text.

#[cfg(test)]
mod tests;

use crate::running::is_invisible_format;

/// Lines of pasted text the question shows; the rest are counted.
pub const PASTE_PREVIEW_LINES: usize = 5;
/// Characters of a pasted line the question shows; a longer line is cut with `…`.
pub const PASTE_PREVIEW_CHARS: usize = 80;

/// What the question shows.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PasteWarning {
    /// "Paste 4 lines?"
    pub title: String,
    /// "They will run as commands."
    pub description: &'static str,
    /// The first lines, made safe to show (see [`preview_line`]).
    pub preview: Vec<String>,
    /// "… and 12 more lines" when the preview leaves lines out.
    pub more: Option<String>,
}

/// The question for pasting `text`; `None` when it has no line break, so nothing runs by
/// itself. A trailing line break counts: "a\n" is one line that runs at once.
pub fn paste_warning(text: &str) -> Option<PasteWarning> {
    let lines = pasted_lines(text)?;
    let count = lines.len();
    let preview = lines.iter().take(PASTE_PREVIEW_LINES);
    Some(PasteWarning {
        title: plural(count, "Paste 1 line?", &format!("Paste {count} lines?")),
        description: if count == 1 {
            "It will run as a command."
        } else {
            "They will run as commands."
        },
        preview: preview
            .map(|line| preview_line(line, PASTE_PREVIEW_CHARS))
            .collect(),
        more: more_lines(count.saturating_sub(PASTE_PREVIEW_LINES)),
    })
}

/// The lines of `text` split at `\r\n`, `\n` or `\r` (each reaches the program as Enter), a
/// trailing break ending the last line; `None` without a line break.
fn pasted_lines(text: &str) -> Option<Vec<&str>> {
    if !text.contains(['\n', '\r']) {
        return None;
    }
    let mut lines: Vec<&str> = text
        .split("\r\n")
        .flat_map(|part| part.split(['\n', '\r']))
        .collect();
    if lines.last() == Some(&"") {
        lines.pop();
    }
    Some(lines)
}

/// "… and 1 more line" / "… and 12 more lines"; `None` for none.
fn more_lines(count: usize) -> Option<String> {
    (count > 0).then(|| {
        plural(
            count,
            "… and 1 more line",
            &format!("… and {count} more lines"),
        )
    })
}

fn plural(count: usize, one: &str, many: &str) -> String {
    if count == 1 { one } else { many }.to_owned()
}

/// `line` as the preview shows it: a tab becomes a space, other control characters their caret
/// form (ESC is `^[`, so an escape sequence shows as text), invisible format characters are
/// dropped, and a line longer than `max_chars` is cut with `…`.
fn preview_line(line: &str, max_chars: usize) -> String {
    let shown: String = line
        .chars()
        .filter(|&c| !is_invisible_format(c))
        .flat_map(visible_char)
        .collect();
    if shown.chars().count() <= max_chars {
        return shown;
    }
    let kept: String = shown.chars().take(max_chars.saturating_sub(1)).collect();
    format!("{kept}…")
}

/// `c` as shown: a tab as a space, C0 controls and DEL in caret form, C1 controls as `�`.
fn visible_char(c: char) -> Vec<char> {
    match c {
        '\t' => vec![' '],
        '\u{0}'..='\u{1f}' | '\u{7f}' => vec!['^', char::from(c as u8 ^ 0x40)],
        '\u{80}'..='\u{9f}' => vec!['\u{fffd}'],
        _ => vec![c],
    }
}
