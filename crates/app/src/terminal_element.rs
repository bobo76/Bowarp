//! Paints a session's visible grid.
//!
//! The terminal lock is held only while copying the visible cells into plain batches; text
//! shaping and painting happen afterwards so the I/O thread is never blocked on rendering.

use core::panic::Location;

use gpui_kit::{
    App, BorderStyle, Bounds, Element, ElementId, ElementInputHandler, Entity, FocusHandle, Font,
    FontStyle, FontWeight, GlobalElementId, Hsla, InspectorElementId, IntoElement, LayoutId,
    PaintQuad, Pixels, Point, ShapedLine, StrikethroughStyle, Style, TextAlign, TextRun,
    UnderlineStyle, Window, fill, font, outline, point, px, relative, size,
};
use term_core::alacritty_terminal::term::cell::Flags;
use term_core::alacritty_terminal::term::{RenderableContent, TermMode};
use term_core::alacritty_terminal::vte::ansi::{CursorShape, NamedColor, Rgb};
use term_core::{Palette, TermDimensions, dim};

use crate::terminal_view::{GridGeometry, TerminalView};

#[cfg(target_os = "windows")]
const FONT_FAMILY: &str = "Cascadia Mono";
#[cfg(not(target_os = "windows"))]
const FONT_FAMILY: &str = "DejaVu Sans Mono";
const FONT_SIZE: f32 = 14.0;
const LINE_HEIGHT_FACTOR: f32 = 1.2;
const SELECTION_BACKGROUND: Rgb = Rgb {
    r: 0x26,
    g: 0x4f,
    b: 0x78,
};

pub struct TerminalElement {
    view: Entity<TerminalView>,
    focus_handle: FocusHandle,
}

impl TerminalElement {
    pub fn new(view: Entity<TerminalView>, focus_handle: FocusHandle) -> Self {
        Self { view, focus_handle }
    }
}

/// A run of cells on one line sharing a style, painted as one shaped line.
struct TextBatch {
    line: usize,
    column: usize,
    cells: usize,
    text: String,
    style: CellStyle,
}

#[derive(Clone, Copy, PartialEq, Eq)]
struct CellStyle {
    foreground: Rgb,
    bold: bool,
    italic: bool,
    underline: bool,
    undercurl: bool,
    strikeout: bool,
}

struct BackgroundRect {
    line: usize,
    column: usize,
    cells: usize,
    color: Rgb,
}

struct CursorCell {
    line: usize,
    column: usize,
    cells: usize,
    shape: CursorShape,
}

/// Plain copy of everything needed to paint one frame.
struct GridSnapshot {
    background: Rgb,
    cursor_color: Rgb,
    batches: Vec<TextBatch>,
    backgrounds: Vec<BackgroundRect>,
    cursor: Option<CursorCell>,
}

pub struct PrepaintState {
    background: Hsla,
    backgrounds: Vec<PaintQuad>,
    lines: Vec<(Point<Pixels>, ShapedLine)>,
    cursor: Option<PaintQuad>,
    marked_text: Option<(PaintQuad, Point<Pixels>, ShapedLine)>,
    geometry: GridGeometry,
}

impl IntoElement for TerminalElement {
    type Element = Self;

    fn into_element(self) -> Self::Element {
        self
    }
}

impl Element for TerminalElement {
    type RequestLayoutState = ();
    type PrepaintState = PrepaintState;

    fn id(&self) -> Option<ElementId> {
        None
    }

    fn source_location(&self) -> Option<&'static Location<'static>> {
        None
    }

    fn request_layout(
        &mut self,
        _: Option<&GlobalElementId>,
        _: Option<&InspectorElementId>,
        window: &mut Window,
        cx: &mut App,
    ) -> (LayoutId, Self::RequestLayoutState) {
        let mut style = Style::default();
        style.size.width = relative(1.).into();
        style.size.height = relative(1.).into();
        (window.request_layout(style, [], cx), ())
    }

    fn prepaint(
        &mut self,
        _: Option<&GlobalElementId>,
        _: Option<&InspectorElementId>,
        bounds: Bounds<Pixels>,
        _: &mut Self::RequestLayoutState,
        window: &mut Window,
        cx: &mut App,
    ) -> Self::PrepaintState {
        let base_font = font(FONT_FAMILY);
        let font_size = px(FONT_SIZE);
        let text_system = window.text_system().clone();
        let font_id = text_system.resolve_font(&base_font);
        let cell_width = text_system
            .advance(font_id, font_size, 'm')
            .map(|advance| advance.width)
            .unwrap_or(px(FONT_SIZE * 0.6));
        let line_height = px((FONT_SIZE * LINE_HEIGHT_FACTOR).round());

        let columns = (f32::from(bounds.size.width) / f32::from(cell_width))
            .floor()
            .max(2.0) as u16;
        let screen_lines = (f32::from(bounds.size.height) / f32::from(line_height))
            .floor()
            .max(1.0) as u16;
        let focused = self.focus_handle.is_focused(window);

        let view = self.view.read(cx);
        let session = &view.session;
        session.resize(TermDimensions {
            columns,
            lines: screen_lines,
            cell_width: f32::from(cell_width) as u16,
            cell_height: f32::from(line_height) as u16,
        });
        // Re-arm before reading so output arriving mid-frame schedules another frame.
        session.ack_wakeup();
        let snapshot = {
            let term = session.lock_term();
            snapshot(term.renderable_content(), session.palette(), focused)
        };
        let marked_text = view.marked_text.clone();

        let cell_origin = |line: usize, column: usize| {
            point(
                bounds.origin.x + cell_width * column as f32,
                bounds.origin.y + line_height * line as f32,
            )
        };
        let cell_bounds = |line: usize, column: usize, cells: usize| {
            Bounds::new(
                cell_origin(line, column),
                size(cell_width * cells as f32, line_height),
            )
        };

        let backgrounds = snapshot
            .backgrounds
            .iter()
            .map(|rect| {
                fill(
                    cell_bounds(rect.line, rect.column, rect.cells),
                    to_hsla(rect.color),
                )
            })
            .collect();

        let lines = snapshot
            .batches
            .into_iter()
            .map(|batch| {
                let run = text_run(&base_font, batch.text.len(), batch.style);
                let shaped = text_system.shape_line(batch.text.into(), font_size, &[run], None);
                (cell_origin(batch.line, batch.column), shaped)
            })
            .collect();

        let cursor_color = to_hsla(snapshot.cursor_color);
        let cursor = snapshot.cursor.as_ref().and_then(|cursor| {
            let cell = cell_bounds(cursor.line, cursor.column, cursor.cells);
            let thickness = px(2.);
            match cursor.shape {
                // Focused block cursors are drawn by inverting the cell colors in `snapshot`.
                CursorShape::Block | CursorShape::Hidden => None,
                CursorShape::HollowBlock => Some(outline(cell, cursor_color, BorderStyle::Solid)),
                CursorShape::Beam => Some(fill(
                    Bounds::new(cell.origin, size(thickness, line_height)),
                    cursor_color,
                )),
                CursorShape::Underline => Some(fill(
                    Bounds::new(
                        point(cell.origin.x, cell.bottom() - thickness),
                        size(cell.size.width, thickness),
                    ),
                    cursor_color,
                )),
            }
        });
        let cursor_origin = snapshot.cursor.as_ref().map_or(bounds.origin, |cursor| {
            cell_origin(cursor.line, cursor.column)
        });

        let marked_text = marked_text.map(|text| {
            let style = CellStyle {
                foreground: session.palette().foreground,
                bold: false,
                italic: false,
                underline: true,
                undercurl: false,
                strikeout: false,
            };
            let run = text_run(&base_font, text.len(), style);
            let shaped = text_system.shape_line(text.into(), font_size, &[run], None);
            let backdrop = fill(
                Bounds::new(cursor_origin, size(shaped.width, line_height)),
                to_hsla(snapshot.background),
            );
            (backdrop, cursor_origin, shaped)
        });

        PrepaintState {
            background: to_hsla(snapshot.background),
            backgrounds,
            lines,
            cursor,
            marked_text,
            geometry: GridGeometry {
                origin: bounds.origin,
                cell_width,
                line_height,
                columns: columns as usize,
                lines: screen_lines as usize,
                cursor_origin,
            },
        }
    }

    fn paint(
        &mut self,
        _: Option<&GlobalElementId>,
        _: Option<&InspectorElementId>,
        bounds: Bounds<Pixels>,
        _: &mut Self::RequestLayoutState,
        prepaint: &mut Self::PrepaintState,
        window: &mut Window,
        cx: &mut App,
    ) {
        window.handle_input(
            &self.focus_handle,
            ElementInputHandler::new(bounds, self.view.clone()),
            cx,
        );

        window.paint_quad(fill(bounds, prepaint.background));
        for quad in prepaint.backgrounds.drain(..) {
            window.paint_quad(quad);
        }
        let line_height = prepaint.geometry.line_height;
        for (origin, line) in &prepaint.lines {
            if let Err(err) = line.paint(*origin, line_height, TextAlign::Left, None, window, cx) {
                log::warn!("failed to paint terminal line: {err}");
            }
        }
        if let Some(cursor) = prepaint.cursor.take() {
            window.paint_quad(cursor);
        }
        if let Some((backdrop, origin, line)) = prepaint.marked_text.take() {
            window.paint_quad(backdrop);
            if let Err(err) = line.paint(origin, line_height, TextAlign::Left, None, window, cx) {
                log::warn!("failed to paint marked text: {err}");
            }
        }

        let geometry = prepaint.geometry;
        self.view
            .update(cx, |view, _| view.geometry = Some(geometry));
    }
}

/// Copy the visible cells into paint batches. Runs under the terminal lock: no shaping here.
fn snapshot(content: RenderableContent<'_>, palette: &Palette, focused: bool) -> GridSnapshot {
    let colors = content.colors;
    let display_offset = content.display_offset as i32;
    let default_background = palette.resolve_index(NamedColor::Background as usize, colors);
    let cursor_color = palette.resolve_index(NamedColor::Cursor as usize, colors);
    let cursor_point = content.cursor.point;
    let show_cursor =
        content.mode.contains(TermMode::SHOW_CURSOR) && content.cursor.shape != CursorShape::Hidden;
    let cursor_shape = match content.cursor.shape {
        CursorShape::Block if !focused => CursorShape::HollowBlock,
        shape => shape,
    };

    let mut snapshot = GridSnapshot {
        background: default_background,
        cursor_color,
        batches: Vec::new(),
        backgrounds: Vec::new(),
        cursor: None,
    };

    for indexed in content.display_iter {
        let cell = indexed.cell;
        if cell
            .flags
            .intersects(Flags::WIDE_CHAR_SPACER | Flags::LEADING_WIDE_CHAR_SPACER)
        {
            continue;
        }
        let line = (indexed.point.line.0 + display_offset) as usize;
        let column = indexed.point.column.0;
        let cells = if cell.flags.contains(Flags::WIDE_CHAR) {
            2
        } else {
            1
        };

        let mut foreground = palette.resolve(cell.fg, colors);
        let mut background = palette.resolve(cell.bg, colors);
        if cell.flags.contains(Flags::DIM) {
            foreground = dim(foreground);
        }
        if cell.flags.contains(Flags::INVERSE) {
            std::mem::swap(&mut foreground, &mut background);
        }
        if cell.flags.contains(Flags::HIDDEN) {
            foreground = background;
        }
        if content
            .selection
            .is_some_and(|selection| selection.contains(indexed.point))
        {
            background = SELECTION_BACKGROUND;
        }
        if show_cursor && indexed.point == cursor_point {
            if cursor_shape == CursorShape::Block {
                foreground = background;
                background = cursor_color;
            }
            snapshot.cursor = Some(CursorCell {
                line,
                column,
                cells,
                shape: cursor_shape,
            });
        }

        if background != default_background {
            match snapshot.backgrounds.last_mut() {
                Some(rect)
                    if rect.line == line
                        && rect.column + rect.cells == column
                        && rect.color == background =>
                {
                    rect.cells += cells;
                }
                _ => snapshot.backgrounds.push(BackgroundRect {
                    line,
                    column,
                    cells,
                    color: background,
                }),
            }
        }

        let style = CellStyle {
            foreground,
            bold: cell.flags.contains(Flags::BOLD),
            italic: cell.flags.contains(Flags::ITALIC),
            underline: cell.flags.intersects(Flags::ALL_UNDERLINES),
            undercurl: cell.flags.contains(Flags::UNDERCURL),
            strikeout: cell.flags.contains(Flags::STRIKEOUT),
        };
        push_cell(
            &mut snapshot.batches,
            line,
            column,
            cells,
            cell.c,
            cell.zerowidth(),
            style,
        );
    }
    snapshot
}

/// Append a cell to the current batch when it continues it, or start a new batch.
///
/// Only ASCII batches are merged: their advance matches the cell width, so a whole batch stays
/// on the grid. Other characters (wide, fallback-font, combining) get a batch each and are
/// positioned at their own column.
fn push_cell(
    batches: &mut Vec<TextBatch>,
    line: usize,
    column: usize,
    cells: usize,
    character: char,
    zero_width: Option<&[char]>,
    style: CellStyle,
) {
    let decorated = |style: &CellStyle| style.underline || style.strikeout;
    let blank = (character == ' ' || character == '\t') && zero_width.is_none();
    // An undecorated blank only needs a position, so it may join any undecorated batch.
    let continues = |batch: &TextBatch| {
        batch.line == line
            && batch.column + batch.cells == column
            && batch.text.is_ascii()
            && (batch.style == style || blank && !decorated(&style) && !decorated(&batch.style))
    };

    if character.is_ascii() && zero_width.is_none() {
        if let Some(batch) = batches.last_mut().filter(|batch| continues(batch)) {
            batch.text.push(if blank { ' ' } else { character });
            batch.cells += cells;
            return;
        }
        if blank && !decorated(&style) {
            return;
        }
    }

    let mut text = String::from(character);
    text.extend(zero_width.into_iter().flatten());
    batches.push(TextBatch {
        line,
        column,
        cells,
        text,
        style,
    });
}

fn text_run(base: &Font, len: usize, style: CellStyle) -> TextRun {
    let color = to_hsla(style.foreground);
    TextRun {
        len,
        font: Font {
            weight: if style.bold {
                FontWeight::BOLD
            } else {
                FontWeight::NORMAL
            },
            style: if style.italic {
                FontStyle::Italic
            } else {
                FontStyle::Normal
            },
            ..base.clone()
        },
        color,
        background_color: None,
        underline: style.underline.then(|| UnderlineStyle {
            thickness: px(1.),
            color: Some(color),
            wavy: style.undercurl,
        }),
        strikethrough: style.strikeout.then(|| StrikethroughStyle {
            thickness: px(1.),
            color: Some(color),
        }),
    }
}

pub(crate) fn to_hsla(color: Rgb) -> Hsla {
    gpui_kit::rgb(((color.r as u32) << 16) | ((color.g as u32) << 8) | color.b as u32).into()
}

#[cfg(test)]
mod tests {
    use super::*;

    const RED: Rgb = Rgb {
        r: 0xff,
        g: 0,
        b: 0,
    };
    const BLUE: Rgb = Rgb {
        r: 0,
        g: 0,
        b: 0xff,
    };

    fn plain(foreground: Rgb) -> CellStyle {
        CellStyle {
            foreground,
            bold: false,
            italic: false,
            underline: false,
            undercurl: false,
            strikeout: false,
        }
    }

    fn underlined(foreground: Rgb) -> CellStyle {
        CellStyle {
            underline: true,
            ..plain(foreground)
        }
    }

    /// Push single-width cells on line 0 starting at column 0.
    fn push_line(cells: &[(char, CellStyle)]) -> Vec<TextBatch> {
        let mut batches = Vec::new();
        for (column, &(character, style)) in cells.iter().enumerate() {
            push_cell(&mut batches, 0, column, 1, character, None, style);
        }
        batches
    }

    /// `(column, cells, text)` of each batch.
    fn layout(batches: &[TextBatch]) -> Vec<(usize, usize, &str)> {
        batches
            .iter()
            .map(|batch| (batch.column, batch.cells, batch.text.as_str()))
            .collect()
    }

    #[test]
    fn adjacent_ascii_with_same_style_merges() {
        let batches = push_line(&[('a', plain(RED)), ('b', plain(RED)), ('c', plain(RED))]);
        assert_eq!(layout(&batches), [(0, 3, "abc")]);
    }

    #[test]
    fn style_change_starts_a_new_batch() {
        let batches = push_line(&[('a', plain(RED)), ('b', plain(BLUE))]);
        assert_eq!(layout(&batches), [(0, 1, "a"), (1, 1, "b")]);
    }

    #[test]
    fn leading_undecorated_blanks_are_skipped() {
        let batches = push_line(&[(' ', plain(RED)), ('\t', plain(RED)), ('a', plain(RED))]);
        assert_eq!(layout(&batches), [(2, 1, "a")]);
    }

    #[test]
    fn decorated_blank_starts_a_batch() {
        let batches = push_line(&[(' ', underlined(RED))]);
        assert_eq!(layout(&batches), [(0, 1, " ")]);
    }

    #[test]
    fn blank_between_styles_joins_the_earlier_batch() {
        let batches = push_line(&[('a', plain(RED)), ('\t', plain(BLUE)), ('b', plain(BLUE))]);
        assert_eq!(layout(&batches), [(0, 2, "a "), (2, 1, "b")]);
    }

    #[test]
    fn undecorated_blank_does_not_join_a_decorated_batch() {
        let batches = push_line(&[('a', underlined(RED)), (' ', plain(RED))]);
        assert_eq!(layout(&batches), [(0, 1, "a")]);
    }

    #[test]
    fn gap_in_columns_starts_a_new_batch() {
        let mut batches = Vec::new();
        push_cell(&mut batches, 0, 0, 1, 'a', None, plain(RED));
        push_cell(&mut batches, 0, 2, 1, 'b', None, plain(RED));
        push_cell(&mut batches, 1, 3, 1, 'c', None, plain(RED));
        assert_eq!(layout(&batches), [(0, 1, "a"), (2, 1, "b"), (3, 1, "c")]);
    }

    #[test]
    fn wide_char_gets_its_own_batch() {
        let mut batches = Vec::new();
        push_cell(&mut batches, 0, 0, 1, 'a', None, plain(RED));
        push_cell(&mut batches, 0, 1, 2, '中', None, plain(RED));
        push_cell(&mut batches, 0, 3, 1, 'b', None, plain(RED));
        assert_eq!(layout(&batches), [(0, 1, "a"), (1, 2, "中"), (3, 1, "b")]);
    }

    #[test]
    fn ascii_with_combining_mark_does_not_merge() {
        let mut batches = Vec::new();
        push_cell(&mut batches, 0, 0, 1, 'a', None, plain(RED));
        push_cell(&mut batches, 0, 1, 1, 'e', Some(&['\u{301}']), plain(RED));
        push_cell(&mut batches, 0, 2, 1, 'b', None, plain(RED));
        assert_eq!(
            layout(&batches),
            [(0, 1, "a"), (1, 1, "e\u{301}"), (2, 1, "b")]
        );
    }
}
