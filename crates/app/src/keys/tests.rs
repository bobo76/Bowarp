use gpui_kit::Modifiers;

use super::*;

fn keystroke(key: &str, modifiers: Modifiers, key_char: Option<&str>) -> Keystroke {
    Keystroke {
        modifiers,
        key: key.into(),
        key_char: key_char.map(Into::into),
    }
}

fn bytes(key: &str, modifiers: Modifiers, mode: TermMode) -> Option<Vec<u8>> {
    key_to_bytes(&keystroke(key, modifiers, None), false, mode).map(Cow::into_owned)
}

fn ctrl() -> Modifiers {
    Modifiers {
        control: true,
        ..Default::default()
    }
}

fn shift() -> Modifiers {
    Modifiers {
        shift: true,
        ..Default::default()
    }
}

fn alt() -> Modifiers {
    Modifiers {
        alt: true,
        ..Default::default()
    }
}

#[test]
fn plain_characters_go_to_text_input() {
    let plain = keystroke("a", Modifiers::default(), Some("a"));
    assert_eq!(key_to_bytes(&plain, false, TermMode::empty()), None);
}

#[test]
fn cursor_keys_respect_application_mode_and_modifiers() {
    let none = Modifiers::default();
    assert_eq!(bytes("up", none, TermMode::empty()).unwrap(), b"\x1b[A");
    assert_eq!(bytes("up", none, TermMode::APP_CURSOR).unwrap(), b"\x1bOA");
    assert_eq!(
        bytes("left", ctrl(), TermMode::APP_CURSOR).unwrap(),
        b"\x1b[1;5D"
    );
}

#[test]
fn control_combinations_produce_c0_bytes() {
    assert_eq!(bytes("c", ctrl(), TermMode::empty()).unwrap(), b"\x03");
    assert_eq!(bytes("d", ctrl(), TermMode::empty()).unwrap(), b"\x04");
    assert_eq!(bytes("space", ctrl(), TermMode::empty()).unwrap(), b"\0");
}

#[test]
fn agent_cli_keys() {
    assert_eq!(bytes("tab", shift(), TermMode::empty()).unwrap(), b"\x1b[Z");
    assert_eq!(
        bytes("enter", shift(), TermMode::empty()).unwrap(),
        b"\x1b\r"
    );
    assert_eq!(
        bytes("enter", Modifiers::default(), TermMode::empty()).unwrap(),
        b"\r"
    );
    assert_eq!(
        bytes("escape", Modifiers::default(), TermMode::empty()).unwrap(),
        b"\x1b"
    );
}

#[test]
fn alt_prefixes_escape() {
    let alt_b = keystroke("b", alt(), Some("b"));
    assert_eq!(
        key_to_bytes(&alt_b, false, TermMode::empty())
            .unwrap()
            .as_ref(),
        b"\x1bb"
    );
}

#[test]
fn character_preference_defers_to_text_input() {
    let alt_gr = keystroke(
        "q",
        Modifiers {
            control: true,
            alt: true,
            ..Default::default()
        },
        Some("@"),
    );
    assert_eq!(key_to_bytes(&alt_gr, true, TermMode::empty()), None);
}

#[test]
fn tilde_and_function_keys() {
    assert_eq!(
        bytes("delete", Modifiers::default(), TermMode::empty()).unwrap(),
        b"\x1b[3~"
    );
    assert_eq!(
        bytes("pageup", shift(), TermMode::empty()).unwrap(),
        b"\x1b[5;2~"
    );
    assert_eq!(
        bytes("f1", Modifiers::default(), TermMode::empty()).unwrap(),
        b"\x1bOP"
    );
    assert_eq!(
        bytes("f5", Modifiers::default(), TermMode::empty()).unwrap(),
        b"\x1b[15~"
    );
}

#[test]
fn modified_function_and_editing_keys() {
    assert_eq!(
        bytes("f1", shift(), TermMode::empty()).unwrap(),
        b"\x1b[1;2P"
    );
    assert_eq!(
        bytes("home", alt(), TermMode::empty()).unwrap(),
        b"\x1b[1;3H"
    );
    assert_eq!(
        bytes("end", Modifiers::default(), TermMode::empty()).unwrap(),
        b"\x1b[F"
    );
    assert_eq!(
        bytes("down", shift(), TermMode::APP_CURSOR).unwrap(),
        b"\x1b[1;2B"
    );
}

#[test]
fn paste_uses_brackets_when_enabled() {
    assert_eq!(
        paste_bytes("a\nb", TermMode::BRACKETED_PASTE),
        b"\x1b[200~a\nb\x1b[201~"
    );
    assert_eq!(
        paste_bytes("x\x1b[201~y", TermMode::BRACKETED_PASTE),
        b"\x1b[200~x[201~y\x1b[201~"
    );
}

#[test]
fn paste_converts_newlines_without_brackets() {
    assert_eq!(paste_bytes("a\r\nb\nc", TermMode::empty()), b"a\rb\rc");
}
