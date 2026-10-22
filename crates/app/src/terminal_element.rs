//! Paints a session's visible grid.
//!
//! The terminal lock is held only while copying the visible cells into plain batches; text
//! shaping and painting happen afterwards so the I/O thread is never blocked on rendering.

use core::panic::Location;
use std::sync::Arc;

use gpui_kit::{
    App, BorderStyle, Bounds, CursorStyle, DispatchPhase, Element, ElementId, ElementInputHandler,
    Entity, FocusHandle, Font, FontStyle, FontWeight, Global, GlobalElementId, Hitbox,
    HitboxBehavior, Hsla, InspectorElementId, IntoElement, LayoutId, MouseDownEvent,
    MouseMoveEvent, MouseUpEvent, PaintQuad, Pixels, Point, ShapedLine, SharedString,
    StrikethroughStyle, Style, TextAlign, TextRun, UnderlineStyle, Window, WindowTextSystem, fill,
    font, outline, point, px, relative, size,
};
use term_core::alacritty_terminal::grid::Dimensions;
use term_core::alacritty_terminal::term::cell::Flags;
use term_core::alacritty_terminal::term::{RenderableContent, TermMode};
use term_core::alacritty_terminal::vte::ansi::{CursorShape, NamedColor, Rgb};
use term_core::{Palette, TermDimensions, TerminalSearch, dim};
use unicode_width::UnicodeWidthChar;
use workspace_model::installed_font_family;

use crate::scrollbar::{ScrollMetrics, ScrollbarLayout, ScrollbarState};
use crate::settings_file::{self, AppSettings};
use crate::terminal_view::{GridGeometry, TerminalView};

/// The terminal font when the settings name none, or one that isn't installed.
#[cfg(target_os = "windows")]
pub(crate) const DEFAULT_FONT_FAMILY: &str = "Cascadia Mono";
#[cfg(not(target_os = "windows"))]
pub(crate) const DEFAULT_FONT_FAMILY: &str = "DejaVu Sans Mono";
const LINE_HEIGHT_FACTOR: f32 = 1.2;
/// Space around the grid. The scroll bar sits in the right padding.
const PADDING_X: f32 = 8.0;
const PADDING_Y: f32 = 4.0;
const SCROLLBAR_THUMB_WIDTH: f32 = 4.0;
const SCROLLBAR_ACTIVE_THUMB_WIDTH: f32 = 6.0;
const SELECTION_BACKGROUND: Rgb = Rgb {
    r: 0x26,
    g: 0x4f,
    b: 0x78,
};
const SEARCH_BACKGROUND: Rgb = Rgb {
    r: 0xe6,
    g: 0xc0,
    b: 0x70,
};
const ACTIVE_SEARCH_BACKGROUND: Rgb = Rgb {
    r: 0xff,
    g: 0x96,
    b: 0x40,
};
const SEARCH_FOREGROUND: Rgb = Rgb {
    r: 0x18,
    g: 0x18,
    b: 0x18,
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

/// Cell backgrounds, with adjacent same-colored cells on a line merged into one rect.
#[derive(Default)]
struct BackgroundRects(Vec<BackgroundRect>);

impl BackgroundRects {
    /// Extend the last rect when this cell continues it, or start a new rect.
    fn push(&mut self, line: usize, column: usize, cells: usize, color: Rgb) {
        match self.0.last_mut() {
            Some(rect)
                if rect.line == line
                    && rect.column + rect.cells == column
                    && rect.color == color =>
            {
                rect.cells += cells;
            }
            _ => self.0.push(BackgroundRect {
                line,
                column,
                cells,
                color,
            }),
        }
    }

    fn iter(&self) -> impl Iterator<Item = &BackgroundRect> {
        self.0.iter()
    }
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
    backgrounds: BackgroundRects,
    cursor: Option<CursorCell>,
}

pub struct PrepaintState {
    background: Hsla,
    backgrounds: Vec<PaintQuad>,
    lines: Vec<(Point<Pixels>, ShapedLine)>,
    cursor: Option<PaintQuad>,
    marked_text: Option<(PaintQuad, Point<Pixels>, ShapedLine)>,
    ghost_text: Option<(Point<Pixels>, ShapedLine)>,
    geometry: GridGeometry,
    scrollbar: Option<PreparedScrollbar>,
    /// The underline of the link under a held Ctrl, and the hitboxes giving it a hand cursor.
    link: Vec<(PaintQuad, Hitbox)>,
}

struct PreparedScrollbar {
    layout: ScrollbarLayout,
    hitbox: Hitbox,
    /// Track and thumb, or nothing while the bar is hidden.
    quads: Vec<PaintQuad>,
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
        // In a column with the exit banner, give way to it.
        style.flex_shrink = 1.;
        style.min_size.height = px(0.).into();
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
        // Read at each frame: a font change re-measures the cells and resizes the pty below.
        let shaper = TextShaper::new(window, cx);
        let cell_width = shaper.cell_width();
        let line_height = px((f32::from(shaper.font_size) * LINE_HEIGHT_FACTOR).round());
        let grid_bounds = inset(bounds, px(PADDING_X), px(PADDING_Y));
        let (columns, screen_lines) = fit_grid(grid_bounds, cell_width, line_height);
        let focused = self.focus_handle.is_focused(window);

        let (snapshot, scroll_metrics) = self.view.update(cx, |view, cx| {
            view.session.resize(TermDimensions {
                columns,
                lines: screen_lines,
                cell_width: f32::from(cell_width) as u16,
                cell_height: f32::from(line_height) as u16,
            });
            take_snapshot(view, focused, cx)
        });
        let view = self.view.read(cx);
        let palette = view.session.palette();
        let grid = CellGrid {
            origin: grid_bounds.origin,
            cell_width,
            line_height,
        };

        let backgrounds = snapshot
            .backgrounds
            .iter()
            .map(|rect| {
                fill(
                    grid.bounds(rect.line, rect.column, rect.cells),
                    to_hsla(rect.color),
                )
            })
            .collect();
        let (cursor, cursor_origin) = prepare_cursor(
            snapshot.cursor.as_ref(),
            grid,
            to_hsla(snapshot.cursor_color),
        );
        // Only next to a visible cursor, cut at the line's end.
        let ghost_text = snapshot.cursor.as_ref().and_then(|cursor| {
            let room = (columns as usize).saturating_sub(cursor.column);
            let text = fit_columns(view.ghost_text.as_deref()?, room);
            let style = ghost_text_style(palette.foreground, snapshot.background);
            (!text.is_empty()).then(|| (cursor_origin, shaper.shape(text, style)))
        });
        let marked_text = view.marked_text.clone().map(|text| {
            let shaped = shaper.shape(text, marked_text_style(palette.foreground));
            marked_text_overlay(shaped, cursor_origin, line_height, snapshot.background)
        });
        let scrollbar = prepare_scrollbar(
            bounds,
            scroll_metrics,
            &view.scrollbar,
            to_hsla(palette.foreground),
            window,
        );
        let link_color = to_hsla(palette.foreground);
        let link = view
            .link_hover
            .link
            .iter()
            .flat_map(|link| &link.rows)
            .map(|row| {
                let cells = grid.bounds(row.line, row.column, row.cells);
                let underline = Bounds::new(
                    point(cells.origin.x, cells.bottom() - px(2.)),
                    size(cells.size.width, px(1.)),
                );
                let hitbox = window.insert_hitbox(cells, HitboxBehavior::Normal);
                (fill(underline, link_color), hitbox)
            })
            .collect();

        PrepaintState {
            background: to_hsla(snapshot.background),
            backgrounds,
            lines: shape_lines(snapshot.batches, grid, &shaper),
            cursor,
            marked_text,
            ghost_text,
            geometry: GridGeometry {
                origin: grid.origin,
                cell_width,
                line_height,
                columns: columns as usize,
                lines: screen_lines as usize,
                cursor_origin,
            },
            scrollbar,
            link,
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
        if let Some((origin, line)) = prepaint.ghost_text.take()
            && let Err(err) = line.paint(origin, line_height, TextAlign::Left, None, window, cx)
        {
            log::warn!("failed to paint the history suggestion: {err}");
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

        for (underline, hitbox) in prepaint.link.drain(..) {
            window.paint_quad(underline);
            window.set_cursor_style(CursorStyle::PointingHand, &hitbox);
        }
        let view = self.view.clone();
        // Window-wide, so the underline goes when the pointer leaves the terminal.
        window.on_mouse_event(move |event: &MouseMoveEvent, phase, _, cx| {
            if phase == DispatchPhase::Bubble {
                view.update(cx, |view, cx| {
                    view.update_link_hover(event.position, &event.modifiers, cx);
                });
            }
        });

        let scrollbar = prepaint.scrollbar.take();
        if let Some(scrollbar) = &scrollbar {
            for quad in &scrollbar.quads {
                window.paint_quad(quad.clone());
            }
            window.set_cursor_style(CursorStyle::Arrow, &scrollbar.hitbox);
            self.register_scrollbar_listeners(scrollbar.layout, window);
        }

        let geometry = prepaint.geometry;
        let scrollable = scrollbar.is_some();
        self.view.update(cx, |view, cx| {
            view.geometry = Some(geometry);
            view.set_scrollable(scrollable, cx);
        });
    }
}

impl TerminalElement {
    /// Window-level listeners so a thumb drag keeps working when the pointer leaves the
    /// terminal. They run before the terminal's own handlers, which they stop when they act.
    fn register_scrollbar_listeners(&self, layout: ScrollbarLayout, window: &mut Window) {
        let view = self.view.clone();
        window.on_mouse_event(move |event: &MouseDownEvent, phase, window, cx| {
            if phase == DispatchPhase::Bubble
                && view.update(cx, |view, cx| {
                    view.scrollbar_mouse_down(event, layout, window, cx)
                })
            {
                cx.stop_propagation();
            }
        });
        let view = self.view.clone();
        window.on_mouse_event(move |event: &MouseMoveEvent, phase, _, cx| {
            if phase == DispatchPhase::Bubble
                && view.update(cx, |view, cx| view.scrollbar_mouse_move(event, layout, cx))
            {
                cx.stop_propagation();
            }
        });
        let view = self.view.clone();
        window.on_mouse_event(move |_: &MouseUpEvent, phase, _, cx| {
            if phase == DispatchPhase::Bubble
                && view.update(cx, |view, cx| view.scrollbar_mouse_up(cx))
            {
                cx.stop_propagation();
            }
        });
    }
}

/// Shapes runs of terminal text in the terminal font.
struct TextShaper {
    text_system: Arc<WindowTextSystem>,
    font: Font,
    font_size: Pixels,
}

impl TextShaper {
    /// In the font of the settings.
    fn new(window: &Window, cx: &App) -> Self {
        let text_system = window.text_system().clone();
        Self {
            font: font(font_family(cx)),
            font_size: px(settings_file::settings(cx).font_size.points()),
            text_system,
        }
    }

    /// The width of one cell: the advance of a glyph in the monospace font.
    fn cell_width(&self) -> Pixels {
        let font_id = self.text_system.resolve_font(&self.font);
        self.text_system
            .advance(font_id, self.font_size, 'm')
            .map(|advance| advance.width)
            .unwrap_or(self.font_size * 0.6)
    }

    fn shape(&self, text: String, style: CellStyle) -> ShapedLine {
        let run = text_run(&self.font, text.len(), style);
        self.text_system
            .shape_line(text.into(), self.font_size, &[run], None)
    }
}

/// The terminal's font family, worked out from the settings when their family changes (not
/// on each frame): the settings' one when it is installed, else the built-in one. GPUI would
/// draw a missing family in the first font of its own fallback list that loads (often a
/// proportional one), which breaks the grid.
struct TerminalFont {
    /// The settings' family it was worked out from (trimmed, empty for none).
    requested: String,
    family: SharedString,
}

impl Global for TerminalFont {}

pub fn init(cx: &mut App) {
    refresh_font_family(cx);
    cx.observe_global::<AppSettings>(refresh_font_family)
        .detach();
}

/// Checks the settings' family against the installed names (never `resolve_font`, which
/// falls back or panics for a missing one) when it differs from the one last checked.
fn refresh_font_family(cx: &mut App) {
    let requested = settings_file::settings(cx)
        .font_family()
        .unwrap_or_default()
        .to_owned();
    if cx
        .try_global::<TerminalFont>()
        .is_some_and(|font| font.requested == requested)
    {
        return;
    }
    let family = if requested.is_empty() {
        DEFAULT_FONT_FAMILY.into()
    } else {
        let installed = cx.text_system().all_font_names();
        // DirectWrite matches family names in any case; cosmic-text (Linux) only exactly.
        installed_font_family(&installed, &requested, cfg!(target_os = "windows")).map_or_else(
            || DEFAULT_FONT_FAMILY.into(),
            |family| family.to_owned().into(),
        )
    };
    cx.set_global(TerminalFont { requested, family });
}

/// The terminal's font family (see `TerminalFont`).
pub(crate) fn font_family(cx: &App) -> SharedString {
    cx.try_global::<TerminalFont>()
        .map_or_else(|| DEFAULT_FONT_FAMILY.into(), |font| font.family.clone())
}

/// Maps grid cells to window positions.
#[derive(Clone, Copy)]
struct CellGrid {
    origin: Point<Pixels>,
    cell_width: Pixels,
    line_height: Pixels,
}

impl CellGrid {
    fn origin(&self, line: usize, column: usize) -> Point<Pixels> {
        point(
            self.origin.x + self.cell_width * column as f32,
            self.origin.y + self.line_height * line as f32,
        )
    }

    fn bounds(&self, line: usize, column: usize, cells: usize) -> Bounds<Pixels> {
        Bounds::new(
            self.origin(line, column),
            size(self.cell_width * cells as f32, self.line_height),
        )
    }
}

/// Columns and lines that fit in `bounds`, at least 2 columns and 1 line.
fn fit_grid(bounds: Bounds<Pixels>, cell_width: Pixels, line_height: Pixels) -> (u16, u16) {
    let columns = (f32::from(bounds.size.width) / f32::from(cell_width))
        .floor()
        .max(2.0) as u16;
    let lines = (f32::from(bounds.size.height) / f32::from(line_height))
        .floor()
        .max(1.0) as u16;
    (columns, lines)
}

/// Refresh search and copy the visible grid under one lock, keeping match coordinates aligned.
fn take_snapshot(
    view: &mut TerminalView,
    focused: bool,
    cx: &mut gpui_kit::Context<TerminalView>,
) -> (GridSnapshot, ScrollMetrics) {
    // Re-arm before reading so output arriving mid-frame schedules another frame.
    view.session.ack_wakeup();
    let mut term = view.session.lock_term();
    if view.search.refresh(&mut term) {
        cx.notify();
    }
    let grid = term.grid();
    let scroll_metrics = ScrollMetrics {
        history: grid.history_size(),
        display_offset: grid.display_offset(),
        screen_lines: grid.screen_lines(),
    };
    let snapshot = snapshot(
        term.renderable_content(),
        view.session.palette(),
        focused,
        &view.search,
    );
    (snapshot, scroll_metrics)
}

fn shape_lines(
    batches: Vec<TextBatch>,
    grid: CellGrid,
    shaper: &TextShaper,
) -> Vec<(Point<Pixels>, ShapedLine)> {
    batches
        .into_iter()
        .map(|batch| {
            let origin = grid.origin(batch.line, batch.column);
            (origin, shaper.shape(batch.text, batch.style))
        })
        .collect()
}

/// The cursor quad, and the top-left of the cursor cell (the grid origin without a cursor).
fn prepare_cursor(
    cursor: Option<&CursorCell>,
    grid: CellGrid,
    color: Hsla,
) -> (Option<PaintQuad>, Point<Pixels>) {
    let Some(cursor) = cursor else {
        return (None, grid.origin);
    };
    let cell = grid.bounds(cursor.line, cursor.column, cursor.cells);
    (
        cursor_quad(cursor.shape, cell, color),
        grid.origin(cursor.line, cursor.column),
    )
}

/// The scroll bar in the right padding of `element_bounds`, when there is history to scroll.
fn prepare_scrollbar(
    element_bounds: Bounds<Pixels>,
    metrics: ScrollMetrics,
    state: &ScrollbarState,
    color: Hsla,
    window: &mut Window,
) -> Option<PreparedScrollbar> {
    if metrics.history == 0 {
        return None;
    }
    let layout = ScrollbarLayout {
        track: Bounds::new(
            point(element_bounds.right() - px(PADDING_X), element_bounds.top()),
            size(px(PADDING_X), element_bounds.size.height),
        ),
        metrics,
    };
    let quads = if state.is_visible() {
        scrollbar_quads(layout, state.is_active(), color)
    } else {
        Vec::new()
    };
    Some(PreparedScrollbar {
        layout,
        hitbox: window.insert_hitbox(layout.track, HitboxBehavior::Normal),
        quads,
    })
}

/// Track (only while hovered or dragged) and thumb, right-aligned in the track.
fn scrollbar_quads(layout: ScrollbarLayout, active: bool, color: Hsla) -> Vec<PaintQuad> {
    let Some((top, height)) = layout.thumb() else {
        return Vec::new();
    };
    let track = layout.track;
    let width = px(if active {
        SCROLLBAR_ACTIVE_THUMB_WIDTH
    } else {
        SCROLLBAR_THUMB_WIDTH
    });
    let thumb = Bounds::new(
        point(track.right() - width - px(1.), track.top() + px(top)),
        size(width, px(height)),
    );
    let thumb_color = color.opacity(if active { 0.6 } else { 0.35 });
    let thumb = fill(thumb, thumb_color).corner_radii(width / 2.);
    if !active {
        return vec![thumb];
    }
    vec![fill(track, color.opacity(0.08)), thumb]
}

fn inset(bounds: Bounds<Pixels>, x: Pixels, y: Pixels) -> Bounds<Pixels> {
    Bounds::new(
        point(bounds.origin.x + x, bounds.origin.y + y),
        size(
            (bounds.size.width - x * 2.).max(px(0.)),
            (bounds.size.height - y * 2.).max(px(0.)),
        ),
    )
}

/// Copy the visible cells into paint batches. Runs under the terminal lock: no shaping here.
fn snapshot(
    content: RenderableContent<'_>,
    palette: &Palette,
    focused: bool,
    search: &TerminalSearch,
) -> GridSnapshot {
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
        backgrounds: BackgroundRects::default(),
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

        let selected = content
            .selection
            .is_some_and(|selection| selection.contains(indexed.point));
        let (mut foreground, mut background) = cell_colors(
            cell.flags,
            palette.resolve(cell.fg, colors),
            palette.resolve(cell.bg, colors),
            selected,
        );
        if !selected && let Some(active) = search.highlight(indexed.point) {
            background = if active {
                ACTIVE_SEARCH_BACKGROUND
            } else {
                SEARCH_BACKGROUND
            };
            foreground = if cell.flags.contains(Flags::HIDDEN) {
                background
            } else {
                SEARCH_FOREGROUND
            };
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
            snapshot.backgrounds.push(line, column, cells, background);
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

/// A cell's foreground and background after its DIM, INVERSE and HIDDEN attributes and the
/// selection are applied.
fn cell_colors(flags: Flags, foreground: Rgb, background: Rgb, selected: bool) -> (Rgb, Rgb) {
    let foreground = if flags.contains(Flags::DIM) {
        dim(foreground)
    } else {
        foreground
    };
    let (foreground, background) = if flags.contains(Flags::INVERSE) {
        (background, foreground)
    } else {
        (foreground, background)
    };
    let foreground = if flags.contains(Flags::HIDDEN) {
        background
    } else {
        foreground
    };
    let background = if selected {
        SELECTION_BACKGROUND
    } else {
        background
    };
    (foreground, background)
}

/// The cursor drawn over the cell, or `None` when there is nothing to draw on top.
fn cursor_quad(shape: CursorShape, cell: Bounds<Pixels>, color: Hsla) -> Option<PaintQuad> {
    let thickness = px(2.);
    match shape {
        // Focused block cursors are drawn by inverting the cell colors in `snapshot`.
        CursorShape::Block | CursorShape::Hidden => None,
        CursorShape::HollowBlock => Some(outline(cell, color, BorderStyle::Solid)),
        CursorShape::Beam => Some(fill(
            Bounds::new(cell.origin, size(thickness, cell.size.height)),
            color,
        )),
        CursorShape::Underline => Some(fill(
            Bounds::new(
                point(cell.origin.x, cell.bottom() - thickness),
                size(cell.size.width, thickness),
            ),
            color,
        )),
    }
}

/// IME composition text is shown underlined in the default foreground.
fn marked_text_style(foreground: Rgb) -> CellStyle {
    CellStyle {
        foreground,
        bold: false,
        italic: false,
        underline: true,
        undercurl: false,
        strikeout: false,
    }
}

/// The history suggestion after the cursor: the foreground halfway to the background.
fn ghost_text_style(foreground: Rgb, background: Rgb) -> CellStyle {
    let mix = |a: u8, b: u8| ((u16::from(a) + u16::from(b)) / 2) as u8;
    CellStyle {
        foreground: Rgb {
            r: mix(foreground.r, background.r),
            g: mix(foreground.g, background.g),
            b: mix(foreground.b, background.b),
        },
        bold: false,
        italic: false,
        underline: false,
        undercurl: false,
        strikeout: false,
    }
}

/// Composition text at the cursor, on a backdrop that hides the cells beneath it.
fn marked_text_overlay(
    shaped: ShapedLine,
    origin: Point<Pixels>,
    line_height: Pixels,
    background: Rgb,
) -> (PaintQuad, Point<Pixels>, ShapedLine) {
    let backdrop = fill(
        Bounds::new(origin, size(shaped.width, line_height)),
        to_hsla(background),
    );
    (backdrop, origin, shaped)
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

/// The start of `text` that fits in `columns` cells (wide characters take two).
fn fit_columns(text: &str, columns: usize) -> String {
    let mut used = 0;
    text.chars()
        .take_while(|&c| {
            used += c.width().unwrap_or(0);
            used <= columns
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use term_core::alacritty_terminal::event::VoidListener;
    use term_core::alacritty_terminal::index::{Column, Point as GridPoint, Side};
    use term_core::alacritty_terminal::selection::{Selection, SelectionType};
    use term_core::alacritty_terminal::term::{Config, Term};
    use term_core::alacritty_terminal::vte::ansi::{Processor, StdSyncHandler};

    fn searchable_term(text: &str) -> (Term<VoidListener>, TerminalSearch) {
        let mut term = Term::new(Config::default(), &TermDimensions::new(30, 3), VoidListener);
        Processor::<StdSyncHandler>::new().advance(&mut term, text.as_bytes());
        let mut search = TerminalSearch::default();
        search.set_query("one").unwrap();
        search.refresh(&mut term);
        (term, search)
    }

    #[test]
    fn search_paints_all_matches_and_distinguishes_the_active_match() {
        let (mut term, mut search) = searchable_term("one one");
        let palette = Palette::default();
        let painted = snapshot(term.renderable_content(), &palette, false, &search);
        let colors: Vec<_> = painted
            .backgrounds
            .iter()
            .map(|rect| (rect.column, rect.cells, rect.color))
            .collect();
        assert_eq!(
            colors,
            [(0, 3, ACTIVE_SEARCH_BACKGROUND), (4, 3, SEARCH_BACKGROUND)]
        );
        search.step(true);
        search.refresh(&mut term);
        let painted = snapshot(term.renderable_content(), &palette, false, &search);
        let colors: Vec<_> = painted
            .backgrounds
            .iter()
            .map(|rect| (rect.column, rect.cells, rect.color))
            .collect();
        assert_eq!(
            colors,
            [(0, 3, SEARCH_BACKGROUND), (4, 3, ACTIVE_SEARCH_BACKGROUND)]
        );
        let painted = snapshot(
            term.renderable_content(),
            &palette,
            false,
            &TerminalSearch::default(),
        );
        assert!(painted.backgrounds.0.is_empty());
    }

    #[test]
    fn selection_takes_precedence_over_search_highlights() {
        let (mut term, search) = searchable_term("one one");
        let mut selection = Selection::new(
            SelectionType::Simple,
            GridPoint::new(0.into(), Column(0)),
            Side::Left,
        );
        selection.update(GridPoint::new(0.into(), Column(2)), Side::Right);
        term.selection = Some(selection);
        let painted = snapshot(
            term.renderable_content(),
            &Palette::default(),
            false,
            &search,
        );
        assert_eq!(painted.backgrounds.0[0].color, SELECTION_BACKGROUND);
        assert_eq!(painted.backgrounds.0[1].color, SEARCH_BACKGROUND);
    }

    #[test]
    fn search_does_not_reveal_hidden_text() {
        let (term, search) = searchable_term("\x1b[8mone\x1b[0m");
        let painted = snapshot(
            term.renderable_content(),
            &Palette::default(),
            false,
            &search,
        );
        let matched = &painted.batches[0];
        assert_eq!(matched.text.trim_end(), "one");
        assert_eq!(matched.style.foreground, ACTIVE_SEARCH_BACKGROUND);
    }

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

    /// `(line, column, cells)` of each rect.
    fn rects(backgrounds: &BackgroundRects) -> Vec<(usize, usize, usize)> {
        backgrounds
            .iter()
            .map(|rect| (rect.line, rect.column, rect.cells))
            .collect()
    }

    #[test]
    fn adjacent_backgrounds_with_same_color_merge() {
        let mut backgrounds = BackgroundRects::default();
        backgrounds.push(0, 0, 1, RED);
        backgrounds.push(0, 1, 2, RED);
        backgrounds.push(0, 3, 1, RED);
        assert_eq!(rects(&backgrounds), [(0, 0, 4)]);
    }

    #[test]
    fn background_color_change_starts_a_new_rect() {
        let mut backgrounds = BackgroundRects::default();
        backgrounds.push(0, 0, 1, RED);
        backgrounds.push(0, 1, 1, BLUE);
        assert_eq!(rects(&backgrounds), [(0, 0, 1), (0, 1, 1)]);
    }

    #[test]
    fn background_gap_or_new_line_starts_a_new_rect() {
        let mut backgrounds = BackgroundRects::default();
        backgrounds.push(0, 0, 1, RED);
        backgrounds.push(0, 2, 1, RED);
        backgrounds.push(1, 3, 1, RED);
        assert_eq!(rects(&backgrounds), [(0, 0, 1), (0, 2, 1), (1, 3, 1)]);
    }

    #[test]
    fn plain_cell_keeps_its_colors() {
        assert_eq!(cell_colors(Flags::empty(), RED, BLUE, false), (RED, BLUE));
    }

    #[test]
    fn dim_darkens_the_foreground() {
        assert_eq!(cell_colors(Flags::DIM, RED, BLUE, false), (dim(RED), BLUE));
    }

    #[test]
    fn inverse_swaps_after_dim() {
        assert_eq!(
            cell_colors(Flags::DIM | Flags::INVERSE, RED, BLUE, false),
            (BLUE, dim(RED))
        );
    }

    #[test]
    fn hidden_text_takes_the_background_color() {
        assert_eq!(cell_colors(Flags::HIDDEN, RED, BLUE, false), (BLUE, BLUE));
        assert_eq!(
            cell_colors(Flags::HIDDEN | Flags::INVERSE, RED, BLUE, false),
            (RED, RED)
        );
    }

    #[test]
    fn selection_replaces_the_background() {
        assert_eq!(
            cell_colors(Flags::INVERSE, RED, BLUE, true),
            (BLUE, SELECTION_BACKGROUND)
        );
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

    #[test]
    fn ghost_text_is_cut_by_cell_width() {
        assert_eq!(fit_columns("status", 3), "sta");
        assert_eq!(fit_columns("\u{4f60}\u{597d}x", 3), "\u{4f60}");
        assert_eq!(fit_columns("\u{4f60}\u{597d}x", 5), "\u{4f60}\u{597d}x");
        assert_eq!(fit_columns("ab", 0), "");
    }
}
