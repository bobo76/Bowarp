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
        .map(|rect| (rect.span.column, rect.span.cells, rect.color))
        .collect();
    assert_eq!(
        colors,
        [(0, 3, ACTIVE_SEARCH_BACKGROUND), (4, 3, SEARCH_BACKGROUND)]
    );
    search.step(workspace_model::layout::Direction::Down);
    search.refresh(&mut term);
    let painted = snapshot(term.renderable_content(), &palette, false, &search);
    let colors: Vec<_> = painted
        .backgrounds
        .iter()
        .map(|rect| (rect.span.column, rect.span.cells, rect.color))
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
        flags: Flags::empty(),
    }
}

fn underlined(foreground: Rgb) -> CellStyle {
    CellStyle {
        flags: Flags::UNDERLINE,
        ..plain(foreground)
    }
}

/// Push single-width cells on line 0 starting at column 0.
fn push_line(cells: &[(char, CellStyle)]) -> Vec<TextBatch> {
    let mut batches = Vec::new();
    for (column, &(character, style)) in cells.iter().enumerate() {
        push_cell(
            &mut batches,
            CellSpan::new(0, column, 1),
            character,
            None,
            style,
        );
    }
    batches
}

/// `(column, cells, text)` of each batch.
fn layout(batches: &[TextBatch]) -> Vec<(usize, usize, &str)> {
    batches
        .iter()
        .map(|batch| (batch.span.column, batch.span.cells, batch.text.as_str()))
        .collect()
}

/// `(line, column, cells)` of each rect.
fn rects(backgrounds: &BackgroundRects) -> Vec<(usize, usize, usize)> {
    backgrounds
        .iter()
        .map(|rect| (rect.span.line, rect.span.column, rect.span.cells))
        .collect()
}

#[test]
fn adjacent_backgrounds_with_same_color_merge() {
    let mut backgrounds = BackgroundRects::default();
    backgrounds.push(CellSpan::new(0, 0, 1), RED);
    backgrounds.push(CellSpan::new(0, 1, 2), RED);
    backgrounds.push(CellSpan::new(0, 3, 1), RED);
    assert_eq!(rects(&backgrounds), [(0, 0, 4)]);
}

#[test]
fn background_color_change_starts_a_new_rect() {
    let mut backgrounds = BackgroundRects::default();
    backgrounds.push(CellSpan::new(0, 0, 1), RED);
    backgrounds.push(CellSpan::new(0, 1, 1), BLUE);
    assert_eq!(rects(&backgrounds), [(0, 0, 1), (0, 1, 1)]);
}

#[test]
fn background_gap_or_new_line_starts_a_new_rect() {
    let mut backgrounds = BackgroundRects::default();
    backgrounds.push(CellSpan::new(0, 0, 1), RED);
    backgrounds.push(CellSpan::new(0, 2, 1), RED);
    backgrounds.push(CellSpan::new(1, 3, 1), RED);
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
    push_cell(&mut batches, CellSpan::new(0, 0, 1), 'a', None, plain(RED));
    push_cell(&mut batches, CellSpan::new(0, 2, 1), 'b', None, plain(RED));
    push_cell(&mut batches, CellSpan::new(1, 3, 1), 'c', None, plain(RED));
    assert_eq!(layout(&batches), [(0, 1, "a"), (2, 1, "b"), (3, 1, "c")]);
}

#[test]
fn wide_char_gets_its_own_batch() {
    let mut batches = Vec::new();
    push_cell(&mut batches, CellSpan::new(0, 0, 1), 'a', None, plain(RED));
    push_cell(&mut batches, CellSpan::new(0, 1, 2), '中', None, plain(RED));
    push_cell(&mut batches, CellSpan::new(0, 3, 1), 'b', None, plain(RED));
    assert_eq!(layout(&batches), [(0, 1, "a"), (1, 2, "中"), (3, 1, "b")]);
}

#[test]
fn ascii_with_combining_mark_does_not_merge() {
    let mut batches = Vec::new();
    push_cell(&mut batches, CellSpan::new(0, 0, 1), 'a', None, plain(RED));
    push_cell(
        &mut batches,
        CellSpan::new(0, 1, 1),
        'e',
        Some(&['\u{301}']),
        plain(RED),
    );
    push_cell(&mut batches, CellSpan::new(0, 2, 1), 'b', None, plain(RED));
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
