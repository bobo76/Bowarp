//! Mouse reports for programs that enable xterm mouse tracking (modes 1000/1002/1003), and the
//! wheel decision between reports, arrow keys and local scrollback.

use gpui_kit::{Modifiers, MouseButton};
use term_core::alacritty_terminal::term::TermMode;

/// Added to the button code when the event is motion rather than a press or release.
const MOTION_FLAG: u8 = 32;
const SHIFT_FLAG: u8 = 4;
const ALT_FLAG: u8 = 8;
const CONTROL_FLAG: u8 = 16;
/// The low bits of a button code that select the button.
const BUTTON_BITS: u8 = 0b11;
/// Legacy button value for "some button was released".
const LEGACY_RELEASE: u8 = 3;
/// Legacy values are offset so they stay printable.
const LEGACY_OFFSET: usize = 32;
/// First value UTF-8 mouse mode encodes as more than one byte.
const UTF8_TWO_BYTE_START: usize = 0x80;
/// UTF-8 mouse mode only allows values encoded in at most two bytes.
const UTF8_MOUSE_LIMIT: usize = 0x800;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ReportButton {
    Left,
    Middle,
    Right,
    WheelUp,
    WheelDown,
    /// Motion without a pressed button.
    None,
}

impl ReportButton {
    pub fn from_gpui(button: MouseButton) -> Option<Self> {
        match button {
            MouseButton::Left => Some(Self::Left),
            MouseButton::Middle => Some(Self::Middle),
            MouseButton::Right => Some(Self::Right),
            MouseButton::Navigate(_) => None,
        }
    }

    fn code(self) -> u8 {
        match self {
            Self::Left => 0,
            Self::Middle => 1,
            Self::Right => 2,
            Self::None => 3,
            Self::WheelUp => 64,
            Self::WheelDown => 65,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MouseAction {
    Press,
    Release,
    Motion,
}

/// Zero-based cell in the visible screen.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Cell {
    pub column: usize,
    pub line: usize,
}

/// Whether pointer events go to the program instead of driving selection and scrollback.
/// Holding Shift keeps local selection available, as in other terminals.
pub fn is_reporting_to_program(mode: TermMode, modifiers: &Modifiers) -> bool {
    mode.intersects(TermMode::MOUSE_MODE) && !modifiers.shift
}

/// The bytes reporting a mouse event, or `None` when the active tracking mode does not ask for
/// it or the cell cannot be encoded.
pub fn report(
    button: ReportButton,
    action: MouseAction,
    cell: Cell,
    modifiers: &Modifiers,
    mode: TermMode,
) -> Option<Vec<u8>> {
    if !is_wanted(button, action, mode) {
        return None;
    }
    let code = button_code(button, action, modifiers);
    if mode.contains(TermMode::SGR_MOUSE) {
        return Some(encode_sgr(code, action, cell));
    }
    encode_legacy(
        code,
        button,
        action,
        cell,
        mode.contains(TermMode::UTF8_MOUSE),
    )
}

fn is_wanted(button: ReportButton, action: MouseAction, mode: TermMode) -> bool {
    match (action, button) {
        (MouseAction::Motion, ReportButton::None) => mode.contains(TermMode::MOUSE_MOTION),
        (MouseAction::Motion, _) => mode.intersects(TermMode::MOUSE_DRAG | TermMode::MOUSE_MOTION),
        _ => mode.intersects(TermMode::MOUSE_MODE),
    }
}

fn button_code(button: ReportButton, action: MouseAction, modifiers: &Modifiers) -> u8 {
    let motion = if action == MouseAction::Motion {
        MOTION_FLAG
    } else {
        0
    };
    button.code()
        + motion
        + SHIFT_FLAG * modifiers.shift as u8
        + ALT_FLAG * modifiers.alt as u8
        + CONTROL_FLAG * modifiers.control as u8
}

fn encode_sgr(code: u8, action: MouseAction, cell: Cell) -> Vec<u8> {
    let suffix = if action == MouseAction::Release {
        'm'
    } else {
        'M'
    };
    let (column, line) = (cell.column + 1, cell.line + 1);
    format!("\x1b[<{code};{column};{line}{suffix}").into_bytes()
}

fn encode_legacy(
    code: u8,
    button: ReportButton,
    action: MouseAction,
    cell: Cell,
    utf8: bool,
) -> Option<Vec<u8>> {
    let mut code = code;
    // The legacy encoding cannot say which button was released.
    if action == MouseAction::Release {
        if matches!(button, ReportButton::WheelUp | ReportButton::WheelDown) {
            return None;
        }
        code = (code & !BUTTON_BITS) | LEGACY_RELEASE;
    }
    let mut bytes = b"\x1b[M".to_vec();
    push_legacy_value(&mut bytes, code as usize, false)?;
    push_legacy_value(&mut bytes, cell.column + 1, utf8)?;
    push_legacy_value(&mut bytes, cell.line + 1, utf8)?;
    Some(bytes)
}

/// Legacy values are offset by 32; plain bytes stop at 255, UTF-8 (mode 1005) at 2047.
fn push_legacy_value(bytes: &mut Vec<u8>, value: usize, utf8: bool) -> Option<()> {
    let value = value + LEGACY_OFFSET;
    if utf8 && value >= UTF8_TWO_BYTE_START {
        let ch = char::from_u32(u32::try_from(value).ok()?).filter(|_| value < UTF8_MOUSE_LIMIT)?;
        bytes.extend_from_slice(ch.encode_utf8(&mut [0; 4]).as_bytes());
    } else {
        bytes.push(u8::try_from(value).ok()?);
    }
    Some(())
}

#[derive(Debug, PartialEq, Eq)]
pub enum WheelAction {
    /// Bytes for the program: mouse reports or arrow keys.
    Program(Vec<u8>),
    /// Lines to move the local scrollback; positive scrolls up into history.
    Scrollback(i32),
}

/// What a wheel movement of `lines` (positive = up) over `cell` does.
pub fn wheel_action(lines: i32, cell: Cell, modifiers: &Modifiers, mode: TermMode) -> WheelAction {
    let count = lines.unsigned_abs() as usize;
    if is_reporting_to_program(mode, modifiers) {
        let button = if lines > 0 {
            ReportButton::WheelUp
        } else {
            ReportButton::WheelDown
        };
        let bytes = report(button, MouseAction::Press, cell, modifiers, mode)
            .map(|bytes| bytes.repeat(count))
            .unwrap_or_default();
        return WheelAction::Program(bytes);
    }
    let alternate_scroll = mode.contains(TermMode::ALT_SCREEN | TermMode::ALTERNATE_SCROLL)
        && !mode.intersects(TermMode::MOUSE_MODE);
    if alternate_scroll {
        // Full-screen programs without mouse support get arrow keys instead of scrollback.
        let arrow: &[u8] = match (lines > 0, mode.contains(TermMode::APP_CURSOR)) {
            (true, true) => b"\x1bOA",
            (true, false) => b"\x1b[A",
            (false, true) => b"\x1bOB",
            (false, false) => b"\x1b[B",
        };
        return WheelAction::Program(arrow.repeat(count));
    }
    WheelAction::Scrollback(lines)
}

#[cfg(test)]
mod tests {
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
}
