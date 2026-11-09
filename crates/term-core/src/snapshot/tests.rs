use super::*;
use alacritty_terminal::event::VoidListener;
use alacritty_terminal::index::Point;
use alacritty_terminal::term::Config;
use alacritty_terminal::vte::ansi::{Processor, StdSyncHandler};

use crate::TermDimensions;

fn terminal(columns: u16, lines: u16) -> Term<VoidListener> {
    Term::new(
        Config::default(),
        &TermDimensions::new(columns, lines),
        VoidListener,
    )
}

fn fed(bytes: &[u8], columns: u16, lines: u16) -> Term<VoidListener> {
    let mut term = terminal(columns, lines);
    Processor::<StdSyncHandler>::new().advance(&mut term, bytes);
    term
}

/// Every cell (character, colors, attributes, link) of the scrollback and the screen.
fn cells(term: &Term<VoidListener>) -> Vec<String> {
    let grid = term.grid();
    let history = grid.history_size() as i32;
    (-history..grid.screen_lines() as i32)
        .map(|line| {
            let row = &grid[Line(line)];
            (0..grid.columns())
                .map(|col| {
                    let cell = &row[Column(col)];
                    format!(
                        "{:?}{:?}{:?}{:?}{:?}{:?}{:?}",
                        cell.c,
                        cell.fg,
                        cell.bg,
                        cell.flags,
                        cell.underline_color(),
                        cell.zerowidth(),
                        cell.hyperlink().map(|link| link.uri().to_string()),
                    )
                })
                .collect()
        })
        .collect()
}

/// Feed `bytes` to a terminal, replay its snapshot into a fresh one, and compare them.
fn assert_round_trip(bytes: &[u8], columns: u16, lines: u16) -> Term<VoidListener> {
    let original = fed(bytes, columns, lines);
    let replayed = fed(&snapshot(&original), columns, lines);
    assert_eq!(cells(&replayed), cells(&original));
    let (a, b) = (&original.grid().cursor, &replayed.grid().cursor);
    assert_eq!(a.point, b.point, "cursor position");
    assert_eq!(a.input_needs_wrap, b.input_needs_wrap, "pending wrap");
    assert_eq!(*original.mode(), *replayed.mode(), "modes");
    assert_eq!(original.cursor_style(), replayed.cursor_style());
    // The pen: what the next character is written with.
    let (a, b) = (&a.template, &b.template);
    assert_eq!((a.fg, a.bg, a.flags), (b.fg, b.bg, b.flags), "pen");
    replayed
}

#[test]
fn replays_plain_text_and_scrollback() {
    let mut text = String::new();
    for i in 0..30 {
        text.push_str(&format!("line {i}\r\n"));
    }
    text.push_str("$ ");
    let replayed = assert_round_trip(text.as_bytes(), 20, 5);
    assert_eq!(replayed.grid().history_size(), 26);
    assert_eq!(replayed.grid().cursor.point, Point::new(Line(4), Column(2)));
}

#[test]
fn replays_colors_and_attributes() {
    let text = "\x1b[1;31mbold red\x1b[0m \x1b[3;4:3;58;5;2mcurl\x1b[0m\r\n\
                \x1b[38;5;200;48;2;1;2;3mindexed on rgb\x1b[0m \x1b[7;9;2mdim\x1b[0m\r\n\
                \x1b[44m  blue bg  \x1b[0m\x1b[93;104mbright\x1b[0m\r\n\
                \x1b[32mpen stays green";
    assert_round_trip(text.as_bytes(), 40, 6);
}

#[test]
fn replays_soft_wraps_and_wide_characters() {
    // A line wrapped by the terminal, a wide character that did not fit at the end of a line,
    // and a combining mark.
    let text = "0123456789abcdef\r\nabcdefg\u{754c}x\r\ne\u{301}\u{754c}";
    let replayed = assert_round_trip(text.as_bytes(), 8, 5);
    assert!(
        replayed.grid()[Line(0)][Column(7)]
            .flags
            .contains(Flags::WRAPLINE)
    );
}

#[test]
fn replays_a_cursor_waiting_to_wrap() {
    assert_round_trip(b"abcdefgh", 8, 3);
}

#[test]
fn replays_cursor_position_and_modes() {
    let text = "\x1b[?1h\x1b=\x1b[?2004h\x1b[?1000h\x1b[?1006h\x1b[?1004h\x1b[?25l\
                \x1b[5 q\x1b[?7l\x1b[4hhello\x1b[2;3H";
    assert_round_trip(text.as_bytes(), 20, 4);
}

#[test]
fn replays_hyperlinks() {
    let text = "see \x1b]8;id=a;https://example.com\x1b\\here\x1b]8;;\x1b\\ done";
    assert_round_trip(text.as_bytes(), 30, 3);
}

#[test]
fn replays_the_alternate_screen() {
    let text = "shell output\r\n\x1b[?1049h\x1b[2J\x1b[Hfull screen app\x1b[3;1Hstatus";
    let replayed = assert_round_trip(text.as_bytes(), 20, 4);
    assert!(replayed.mode().contains(TermMode::ALT_SCREEN));
}

#[test]
fn keeps_colored_blank_cells() {
    assert_round_trip(b"\x1b[41m    \x1b[0m\r\nx\x1b[42m\x1b[K", 10, 3);
}

/// Rows of `columns` cells, each in its own truecolor: the densest common snapshot.
fn rainbow_rows(rows: usize, columns: usize) -> String {
    let mut text = String::new();
    for row in 0..rows {
        for col in 0..columns {
            let shade = (row * columns + col) % 256;
            text.push_str(&format!("\x1b[38;2;{shade};0;0;48;2;0;{shade};0mx"));
        }
        text.push_str(&format!("\x1b[0m\r\nrow {row}\r\n"));
    }
    text
}

#[test]
fn a_snapshot_within_a_limit_drops_the_oldest_scrollback_and_keeps_the_screen() {
    let original = fed(rainbow_rows(60, 20).as_bytes(), 20, 5);
    let full = snapshot(&original);
    let limit = full.len() / 3;

    let capped = snapshot_within(&original, limit);
    assert!(capped.len() <= limit, "{} > {limit}", capped.len());
    let replayed = fed(&capped, 20, 5);
    let (original_cells, replayed_cells) = (cells(&original), cells(&replayed));
    assert!(replayed.grid().history_size() < original.grid().history_size());
    // The newest lines, the screen among them, come back as they were.
    assert!(original_cells.ends_with(&replayed_cells));
    assert_eq!(replayed.grid().cursor.point, original.grid().cursor.point);
}

#[test]
fn a_snapshot_under_its_limit_is_whole() {
    let original = fed(rainbow_rows(10, 20).as_bytes(), 20, 5);
    assert_eq!(snapshot_within(&original, usize::MAX), snapshot(&original));
    let full = snapshot(&original);
    assert_eq!(snapshot_within(&original, full.len()), full);
}
