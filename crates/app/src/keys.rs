//! Translation of keystrokes and pasted text into the bytes a terminal program expects.

use std::borrow::Cow;

use gpui_kit::Keystroke;
use term_core::alacritty_terminal::term::TermMode;

/// xterm modifier parameter when no modifier is held.
const NO_MODIFIERS: u8 = 1;
/// CSI key code that is implied when omitted.
const DEFAULT_KEY_CODE: u8 = 1;

/// Bytes to send for a keystroke, or `None` when it should arrive as text input instead
/// (plain characters go through the platform input handler so IME composition works).
pub fn key_to_bytes(
    keystroke: &Keystroke,
    prefer_character_input: bool,
    mode: TermMode,
) -> Option<Cow<'static, [u8]>> {
    let modifiers = &keystroke.modifiers;
    if prefer_character_input || modifiers.platform {
        return None;
    }
    // xterm modifier parameter: 1 + shift(1) + alt(2) + ctrl(4).
    let modifier_code =
        1 + modifiers.shift as u8 + 2 * modifiers.alt as u8 + 4 * modifiers.control as u8;
    let key = keystroke.key.as_str();

    if let Some(sequence) = special_key_sequence(key, modifier_code, mode) {
        return Some(sequence.into_bytes().into());
    }

    let with_alt = |bytes: &[u8]| -> Cow<'static, [u8]> {
        if modifiers.alt {
            [b"\x1b", bytes].concat().into()
        } else {
            bytes.to_vec().into()
        }
    };
    match key {
        // Shift+Enter sends ESC CR, which Claude Code treats as a newline instead of submit.
        "enter" if modifiers.shift => return Some(Cow::Borrowed(b"\x1b\r")),
        "enter" => return Some(with_alt(b"\r")),
        "backspace" if modifiers.control => return Some(with_alt(b"\x08")),
        "backspace" => return Some(with_alt(b"\x7f")),
        "tab" if modifiers.shift => return Some(Cow::Borrowed(b"\x1b[Z")),
        "tab" => return Some(with_alt(b"\t")),
        "escape" => return Some(with_alt(b"\x1b")),
        "space" if modifiers.control => return Some(with_alt(b"\0")),
        _ => {}
    }

    if modifiers.control {
        let &[byte] = key.as_bytes() else { return None };
        return control_byte(byte).map(|control| with_alt(&[control]));
    }
    if modifiers.alt {
        let text = keystroke.key_char.as_deref().unwrap_or(key);
        if text.chars().count() == 1 {
            return Some(with_alt(text.as_bytes()));
        }
    }
    None
}

/// Text to send for a paste, honoring bracketed paste mode.
pub fn paste_bytes(text: &str, mode: TermMode) -> Vec<u8> {
    if mode.contains(TermMode::BRACKETED_PASTE) {
        // Strip ESC so pasted content cannot terminate the bracket early.
        let body = text.replace('\x1b', "");
        return format!("\x1b[200~{body}\x1b[201~").into_bytes();
    }
    text.replace("\r\n", "\r").replace('\n', "\r").into_bytes()
}

/// Escape sequence for cursor, editing and function keys.
fn special_key_sequence(key: &str, modifier_code: u8, mode: TermMode) -> Option<String> {
    let unmodified = modifier_code == NO_MODIFIERS;
    if let Some(letter) = cursor_key_letter(key) {
        if unmodified && mode.contains(TermMode::APP_CURSOR) {
            return Some(ss3_sequence(letter));
        }
        return Some(csi_sequence(DEFAULT_KEY_CODE, letter, modifier_code));
    }
    if let Some(code) = tilde_key_code(key) {
        return Some(csi_sequence(code, '~', modifier_code));
    }
    let letter = low_function_key_letter(key)?;
    if unmodified {
        return Some(ss3_sequence(letter));
    }
    Some(csi_sequence(DEFAULT_KEY_CODE, letter, modifier_code))
}

/// `CSI code ; modifier final`, dropping the modifier when none is held and the code too when
/// it is the default.
fn csi_sequence(code: u8, final_byte: char, modifier_code: u8) -> String {
    match (code, modifier_code) {
        (DEFAULT_KEY_CODE, NO_MODIFIERS) => format!("\x1b[{final_byte}"),
        (_, NO_MODIFIERS) => format!("\x1b[{code}{final_byte}"),
        _ => format!("\x1b[{code};{modifier_code}{final_byte}"),
    }
}

fn ss3_sequence(final_byte: char) -> String {
    format!("\x1bO{final_byte}")
}

fn cursor_key_letter(key: &str) -> Option<char> {
    Some(match key {
        "up" => 'A',
        "down" => 'B',
        "right" => 'C',
        "left" => 'D',
        "home" => 'H',
        "end" => 'F',
        _ => return None,
    })
}

fn tilde_key_code(key: &str) -> Option<u8> {
    Some(match key {
        "insert" => 2,
        "delete" => 3,
        "pageup" => 5,
        "pagedown" => 6,
        "f5" => 15,
        "f6" => 17,
        "f7" => 18,
        "f8" => 19,
        "f9" => 20,
        "f10" => 21,
        "f11" => 23,
        "f12" => 24,
        _ => return None,
    })
}

fn low_function_key_letter(key: &str) -> Option<char> {
    Some(match key {
        "f1" => 'P',
        "f2" => 'Q',
        "f3" => 'R',
        "f4" => 'S',
        _ => return None,
    })
}

/// The C0 control byte for Ctrl+key, following xterm's conventions.
fn control_byte(key: u8) -> Option<u8> {
    Some(match key {
        b'a'..=b'z' => key - b'a' + 1,
        b'@' | b'2' => 0x00,
        b'[' | b'3' => 0x1b,
        b'\\' | b'4' => 0x1c,
        b']' | b'5' => 0x1d,
        b'^' | b'6' => 0x1e,
        b'_' | b'-' | b'/' | b'7' => 0x1f,
        b'8' => 0x7f,
        _ => return None,
    })
}

#[cfg(test)]
mod tests;
