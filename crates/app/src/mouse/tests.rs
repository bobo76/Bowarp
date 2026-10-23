use super::*;

const CELL: Cell = Cell { column: 4, line: 9 };

fn none() -> Modifiers {
    Modifiers::default()
}

fn with(modifiers: impl FnOnce(&mut Modifiers)) -> Modifiers {
    let mut result = Modifiers::default();
    modifiers(&mut result);
    result
}

fn sgr(mode: TermMode) -> TermMode {
    mode | TermMode::SGR_MOUSE
}

#[test]
fn sgr_press_release_and_modifiers() {
    let mode = sgr(TermMode::MOUSE_REPORT_CLICK);
    let press = report(ReportButton::Left, MouseAction::Press, CELL, &none(), mode);
    assert_eq!(press.as_deref(), Some(&b"\x1b[<0;5;10M"[..]));
    let release = report(
        ReportButton::Right,
        MouseAction::Release,
        CELL,
        &none(),
        mode,
    );
    assert_eq!(release.as_deref(), Some(&b"\x1b[<2;5;10m"[..]));
    let ctrl_alt = with(|m| {
        m.control = true;
        m.alt = true;
    });
    let press = report(
        ReportButton::Middle,
        MouseAction::Press,
        CELL,
        &ctrl_alt,
        mode,
    );
    assert_eq!(press.as_deref(), Some(&b"\x1b[<25;5;10M"[..]));
}

#[test]
fn motion_follows_tracking_mode() {
    let click_only = sgr(TermMode::MOUSE_REPORT_CLICK);
    assert_eq!(
        report(
            ReportButton::Left,
            MouseAction::Motion,
            CELL,
            &none(),
            click_only
        ),
        None
    );

    let drag = sgr(TermMode::MOUSE_DRAG);
    let dragged = report(ReportButton::Left, MouseAction::Motion, CELL, &none(), drag);
    assert_eq!(dragged.as_deref(), Some(&b"\x1b[<32;5;10M"[..]));
    assert_eq!(
        report(ReportButton::None, MouseAction::Motion, CELL, &none(), drag),
        None
    );

    let any = sgr(TermMode::MOUSE_MOTION);
    let hover = report(ReportButton::None, MouseAction::Motion, CELL, &none(), any);
    assert_eq!(hover.as_deref(), Some(&b"\x1b[<35;5;10M"[..]));
}

#[test]
fn no_reports_without_tracking() {
    let mode = TermMode::SGR_MOUSE;
    assert_eq!(
        report(ReportButton::Left, MouseAction::Press, CELL, &none(), mode),
        None
    );
}

#[test]
fn legacy_encoding() {
    let mode = TermMode::MOUSE_REPORT_CLICK;
    let press = report(ReportButton::Left, MouseAction::Press, CELL, &none(), mode);
    assert_eq!(press, Some(vec![0x1b, b'[', b'M', 32, 32 + 5, 32 + 10]));
    let release = report(
        ReportButton::Right,
        MouseAction::Release,
        CELL,
        &none(),
        mode,
    );
    assert_eq!(
        release,
        Some(vec![0x1b, b'[', b'M', 32 + 3, 32 + 5, 32 + 10])
    );

    let far = Cell {
        column: 300,
        line: 0,
    };
    assert_eq!(
        report(ReportButton::Left, MouseAction::Press, far, &none(), mode),
        None
    );
    let utf8 = mode | TermMode::UTF8_MOUSE;
    let press = report(ReportButton::Left, MouseAction::Press, far, &none(), utf8).unwrap();
    assert_eq!(&press[4..], "\u{14d}!".as_bytes());
}

#[test]
fn wheel_reports_when_program_tracks_mouse() {
    // Claude Code: alternate screen, any-motion tracking, SGR encoding.
    let mode = TermMode::ALT_SCREEN
        | TermMode::ALTERNATE_SCROLL
        | sgr(TermMode::MOUSE_REPORT_CLICK | TermMode::MOUSE_DRAG | TermMode::MOUSE_MOTION);
    assert_eq!(
        wheel_action(2, CELL, &none(), mode),
        WheelAction::Program(b"\x1b[<64;5;10M\x1b[<64;5;10M".to_vec())
    );
    assert_eq!(
        wheel_action(-1, CELL, &none(), mode),
        WheelAction::Program(b"\x1b[<65;5;10M".to_vec())
    );
    let shift = with(|m| m.shift = true);
    assert_eq!(
        wheel_action(3, CELL, &shift, mode),
        WheelAction::Scrollback(3)
    );
}

#[test]
fn wheel_sends_arrows_on_alternate_screen_without_tracking() {
    let mode = TermMode::ALT_SCREEN | TermMode::ALTERNATE_SCROLL;
    assert_eq!(
        wheel_action(2, CELL, &none(), mode),
        WheelAction::Program(b"\x1b[A\x1b[A".to_vec())
    );
    assert_eq!(
        wheel_action(-1, CELL, &none(), mode | TermMode::APP_CURSOR),
        WheelAction::Program(b"\x1bOB".to_vec())
    );
}

#[test]
fn wheel_scrolls_history_on_primary_screen() {
    let mode = TermMode::ALTERNATE_SCROLL;
    assert_eq!(
        wheel_action(-4, CELL, &none(), mode),
        WheelAction::Scrollback(-4)
    );
}
