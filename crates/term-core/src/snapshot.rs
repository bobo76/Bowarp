//! A terminal's state as the bytes that recreate it: what a remote server sends a client that
//! attaches to a running session, so the client's own terminal shows the same scrollback,
//! screen, cursor and modes before the live output continues.
//!
//! Replayed into a fresh terminal of the same size, the snapshot gives the same cells (text,
//! colors, attributes, hyperlinks), the same scrollback (up to the receiver's history limit),
//! cursor position, cursor style and the modes programs set (keypad, mouse, bracketed paste,
//! focus reports). Not kept: the primary screen while the alternate screen is active (only
//! the alternate screen is replayed), the scroll region, tab stops, charsets, colors changed by
//! OSC 4/10/11 and the title (sent apart by the server).

use std::fmt::Write as _;

use alacritty_terminal::grid::Dimensions;
use alacritty_terminal::index::{Column, Line};
use alacritty_terminal::term::cell::{Cell, Flags};
use alacritty_terminal::term::{Term, TermMode};
use alacritty_terminal::vte::ansi::{Color, CursorShape, NamedColor};

/// The bytes that recreate `term`'s scrollback, screen, cursor and modes in a fresh terminal
/// of the same size.
pub fn snapshot<L>(term: &Term<L>) -> Vec<u8> {
    snapshot_within(term, usize::MAX)
}

/// As [`snapshot`], without the oldest scrollback lines that would make it longer than
/// `max_bytes`. The screen is always kept, so a huge screen may still exceed it.
pub fn snapshot_within<L>(term: &Term<L>, max_bytes: usize) -> Vec<u8> {
    let grid = term.grid();
    let mode = *term.mode();
    let mut out = String::new();
    if mode.contains(TermMode::ALT_SCREEN) {
        out.push_str("\x1b[?1049h");
    }
    let body_start = out.len();
    // Where a scrollback line, or the screen, starts in the default pen: the lines before
    // such a place can be dropped.
    let mut cuts = Vec::new();
    let mut pen = Pen::default();
    let history = grid.history_size() as i32;
    let screen = grid.screen_lines() as i32;
    let columns = grid.columns();
    for line in -history..screen {
        let row = &grid[Line(line)];
        let wraps = columns > 0 && row[Column(columns - 1)].flags.contains(Flags::WRAPLINE);
        let end = if wraps {
            columns
        } else {
            (0..columns)
                .rev()
                .find(|&col| !is_blank(&row[Column(col)]))
                .map_or(0, |col| col + 1)
        };
        for col in 0..end {
            let cell = &row[Column(col)];
            if cell
                .flags
                .intersects(Flags::WIDE_CHAR_SPACER | Flags::LEADING_WIDE_CHAR_SPACER)
            {
                continue;
            }
            pen.apply(cell, &mut out);
            out.push(cell.c);
            if let Some(zerowidth) = cell.zerowidth() {
                out.extend(zerowidth);
            }
        }
        if !wraps && line + 1 < screen {
            // The rest of the line is blank in the default colors, as on a fresh line.
            pen.reset(&mut out);
            out.push_str("\r\n");
            if line < 0 {
                cuts.push(out.len());
            }
        }
    }
    pen.reset(&mut out);
    let cursor = &grid.cursor;
    let _ = write!(
        out,
        "\x1b[{};{}H",
        cursor.point.line.0 + 1,
        cursor.point.column.0 + 1
    );
    if cursor.input_needs_wrap {
        // The cursor sits past the last column: rewrite the last cell to set the pending wrap.
        let cell = &grid[cursor.point];
        if !cell.flags.contains(Flags::WIDE_CHAR_SPACER) {
            pen.apply(cell, &mut out);
            out.push(cell.c);
        }
    }
    pen.apply(&cursor.template, &mut out);
    push_modes(mode, &mut out);
    push_cursor_style(term, &mut out);
    if out.len() > max_bytes {
        let excess = out.len() - max_bytes;
        let cut = cuts
            .iter()
            .copied()
            .find(|&cut| cut - body_start >= excess)
            .or(cuts.last().copied());
        if let Some(cut) = cut {
            out.replace_range(body_start..cut, "");
        }
    }
    out.into_bytes()
}

fn is_blank(cell: &Cell) -> bool {
    cell.c == ' '
        && cell.bg == Color::Named(NamedColor::Background)
        && !cell
            .flags
            .intersects(Flags::INVERSE | Flags::ALL_UNDERLINES | Flags::STRIKEOUT)
        && cell.hyperlink().is_none()
        && cell.zerowidth().is_none()
}

/// The graphic rendition last written, so a run of cells with the same look is written once.
#[derive(Default)]
struct Pen {
    style: Option<Style>,
    hyperlink: Option<(String, String)>,
}

#[derive(Clone, PartialEq)]
struct Style {
    fg: Color,
    bg: Color,
    underline: Option<Color>,
    flags: Flags,
}

const STYLE_FLAGS: Flags = Flags::BOLD
    .union(Flags::DIM)
    .union(Flags::ITALIC)
    .union(Flags::ALL_UNDERLINES)
    .union(Flags::INVERSE)
    .union(Flags::HIDDEN)
    .union(Flags::STRIKEOUT);

impl Pen {
    fn apply(&mut self, cell: &Cell, out: &mut String) {
        let style = Style {
            fg: cell.fg,
            bg: cell.bg,
            underline: cell.underline_color(),
            flags: cell.flags & STYLE_FLAGS,
        };
        if self.style.as_ref() != Some(&style) {
            push_sgr(&style, out);
            self.style = Some(style);
        }
        let hyperlink = cell
            .hyperlink()
            .map(|link| (link.id().to_string(), link.uri().to_string()));
        if self.hyperlink != hyperlink {
            match &hyperlink {
                Some((id, uri)) => {
                    let _ = write!(out, "\x1b]8;id={id};{uri}\x1b\\");
                }
                None => out.push_str("\x1b]8;;\x1b\\"),
            }
            self.hyperlink = hyperlink;
        }
    }

    fn reset(&mut self, out: &mut String) {
        if self.hyperlink.take().is_some() {
            out.push_str("\x1b]8;;\x1b\\");
        }
        let default = Style {
            fg: Color::Named(NamedColor::Foreground),
            bg: Color::Named(NamedColor::Background),
            underline: None,
            flags: Flags::empty(),
        };
        if self.style.as_ref() != Some(&default) {
            out.push_str("\x1b[0m");
            self.style = Some(default);
        }
    }
}

fn push_sgr(style: &Style, out: &mut String) {
    out.push_str("\x1b[0");
    let flags = style.flags;
    for (flag, code) in [
        (Flags::BOLD, "1"),
        (Flags::DIM, "2"),
        (Flags::ITALIC, "3"),
        (Flags::UNDERLINE, "4"),
        (Flags::DOUBLE_UNDERLINE, "4:2"),
        (Flags::UNDERCURL, "4:3"),
        (Flags::DOTTED_UNDERLINE, "4:4"),
        (Flags::DASHED_UNDERLINE, "4:5"),
        (Flags::INVERSE, "7"),
        (Flags::HIDDEN, "8"),
        (Flags::STRIKEOUT, "9"),
    ] {
        if flags.contains(flag) {
            out.push(';');
            out.push_str(code);
        }
    }
    push_color(style.fg, Layer::Foreground, out);
    push_color(style.bg, Layer::Background, out);
    if let Some(color) = style.underline {
        push_color(color, Layer::Underline, out);
    }
    out.push('m');
}

#[derive(Clone, Copy)]
enum Layer {
    Foreground,
    Background,
    Underline,
}

fn push_color(color: Color, layer: Layer, out: &mut String) {
    let extended = match layer {
        Layer::Foreground => 38,
        Layer::Background => 48,
        Layer::Underline => 58,
    };
    match color {
        Color::Spec(rgb) => {
            let _ = write!(out, ";{extended};2;{};{};{}", rgb.r, rgb.g, rgb.b);
        }
        Color::Indexed(index) => {
            let _ = write!(out, ";{extended};5;{index}");
        }
        Color::Named(named) => {
            let index = named as usize;
            let (base, bright) = match layer {
                Layer::Foreground => (30, 90),
                Layer::Background => (40, 100),
                Layer::Underline => {
                    if index < 16 {
                        let _ = write!(out, ";58;5;{index}");
                    }
                    return;
                }
            };
            match index {
                0..=7 => {
                    let _ = write!(out, ";{}", base + index);
                }
                8..=15 => {
                    let _ = write!(out, ";{}", bright + index - 8);
                }
                // `Dim*` colors come from the DIM flag, written above. Foreground, Background,
                // Cursor and the like are the default color, as reset by the leading `0`.
                _ => {
                    if let Some(base_index) = dim_base(named) {
                        let _ = write!(out, ";{}", base + base_index);
                    }
                }
            }
        }
    }
}

fn dim_base(named: NamedColor) -> Option<usize> {
    let base = match named {
        NamedColor::DimBlack => NamedColor::Black,
        NamedColor::DimRed => NamedColor::Red,
        NamedColor::DimGreen => NamedColor::Green,
        NamedColor::DimYellow => NamedColor::Yellow,
        NamedColor::DimBlue => NamedColor::Blue,
        NamedColor::DimMagenta => NamedColor::Magenta,
        NamedColor::DimCyan => NamedColor::Cyan,
        NamedColor::DimWhite => NamedColor::White,
        _ => return None,
    };
    Some(base as usize)
}

fn push_modes(mode: TermMode, out: &mut String) {
    // Private modes whose reset state is the default of a fresh terminal.
    for (flag, number) in [
        (TermMode::APP_CURSOR, 1),
        (TermMode::ORIGIN, 6),
        (TermMode::MOUSE_REPORT_CLICK, 1000),
        (TermMode::MOUSE_DRAG, 1002),
        (TermMode::MOUSE_MOTION, 1003),
        (TermMode::FOCUS_IN_OUT, 1004),
        (TermMode::SGR_MOUSE, 1006),
        (TermMode::BRACKETED_PASTE, 2004),
    ] {
        if mode.contains(flag) {
            let _ = write!(out, "\x1b[?{number}h");
        }
    }
    // Set in a fresh terminal.
    for (flag, number) in [
        (TermMode::SHOW_CURSOR, 25),
        (TermMode::LINE_WRAP, 7),
        (TermMode::ALTERNATE_SCROLL, 1007),
    ] {
        if !mode.contains(flag) {
            let _ = write!(out, "\x1b[?{number}l");
        }
    }
    if mode.contains(TermMode::URGENCY_HINTS) {
        out.push_str("\x1b[?1042h");
    }
    if mode.contains(TermMode::INSERT) {
        out.push_str("\x1b[4h");
    }
    if mode.contains(TermMode::LINE_FEED_NEW_LINE) {
        out.push_str("\x1b[20h");
    }
    if mode.contains(TermMode::APP_KEYPAD) {
        out.push_str("\x1b=");
    }
}

fn push_cursor_style<L>(term: &Term<L>, out: &mut String) {
    let style = term.cursor_style();
    let shape = match style.shape {
        CursorShape::Block | CursorShape::HollowBlock => 1,
        CursorShape::Underline => 3,
        CursorShape::Beam => 5,
        // Hidden comes from `SHOW_CURSOR`, written with the modes.
        CursorShape::Hidden => return,
    };
    // Even numbers are the steady variants.
    let number = if style.blinking { shape } else { shape + 1 };
    if number != 2 {
        let _ = write!(out, "\x1b[{number} q");
    }
}

#[cfg(test)]
mod tests;
