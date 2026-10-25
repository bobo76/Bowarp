use super::*;
use alacritty_terminal::event::VoidListener;
use alacritty_terminal::term::Config;
use alacritty_terminal::vte::ansi::{Processor, StdSyncHandler};

use crate::TermDimensions;

fn terminal(text: &str, columns: u16, lines: u16) -> Term<VoidListener> {
    let mut term = Term::new(
        Config::default(),
        &TermDimensions::new(columns, lines),
        VoidListener,
    );
    Processor::<StdSyncHandler>::new().advance(&mut term, text.as_bytes());
    term
}

#[test]
fn finds_scrollback_and_wraps_in_both_directions() {
    let mut term = terminal("needle\r\nother\r\nneedle\r\nlast", 20, 2);
    let mut search = TerminalSearch::default();
    search.set_query("needle").unwrap();
    search.refresh(&mut term);
    assert_eq!(search.matches.len(), 2);
    assert_eq!(search.current(), Some(1));
    search.step(SearchDirection::Down);
    search.refresh(&mut term);
    assert_eq!(search.current(), Some(0));
    assert!(term.grid().display_offset() > 0);
    search.step(SearchDirection::Up);
    search.refresh(&mut term);
    assert_eq!(search.current(), Some(1));
}

#[test]
fn matches_soft_wrapped_wide_characters() {
    let mut term = terminal("abcd\u{754c}needle", 6, 4);
    let mut search = TerminalSearch::default();
    search.set_query("\u{754c}nee").unwrap();
    search.refresh(&mut term);
    assert_eq!(search.matches.len(), 1);
    let range = &search.matches[0];
    assert_eq!(*range.start(), Point::new(0.into(), Column(4)));
    assert_eq!(*range.end(), Point::new(1.into(), Column(2)));
    assert_eq!(
        search.highlight(Point::new(0.into(), Column(5))),
        Some(true)
    );
    assert_eq!(
        search.highlight(Point::new(1.into(), Column(2))),
        Some(true)
    );
    assert_eq!(search.highlight(Point::new(1.into(), Column(3))), None);
}

#[test]
fn empty_and_missing_queries_clear_results() {
    let mut term = terminal("needle", 20, 2);
    let mut search = TerminalSearch::default();
    for query in ["needle", "", "missing"] {
        search.set_query(query).unwrap();
        search.refresh(&mut term);
        assert_eq!(search.matches.len(), usize::from(query == "needle"));
        if query != "needle" {
            search.step(SearchDirection::Down);
            search.step(SearchDirection::Up);
            assert_eq!(search.current(), None);
        }
    }
}

#[test]
fn refreshes_on_output_resize_and_alternate_screen() {
    let mut term = terminal("needle", 20, 3);
    let mut search = TerminalSearch::default();
    search.set_query("needle").unwrap();
    assert!(search.refresh(&mut term));
    assert!(!search.refresh(&mut term));
    Processor::<StdSyncHandler>::new().advance(&mut term, b"\r\nneedle");
    search.invalidate();
    search.refresh(&mut term);
    assert_eq!(search.matches.len(), 2);
    term.resize(TermDimensions::new(4, 3));
    assert!(search.refresh(&mut term));
    assert_eq!(search.matches.len(), 2);
    Processor::<StdSyncHandler>::new().advance(&mut term, b"\x1b[?1049h");
    search.invalidate();
    search.refresh(&mut term);
    assert!(search.matches.is_empty());
    Processor::<StdSyncHandler>::new().advance(&mut term, b"\x1b[?1049l");
    search.invalidate();
    search.refresh(&mut term);
    assert_eq!(search.matches.len(), 2);
}

#[test]
fn literal_metacharacters_and_uppercase_queries_ignore_case() {
    let mut term = terminal("Needle needle NEEDLE [a.*] C:\\src\\foo.rs", 60, 2);
    let mut search = TerminalSearch::default();
    search.set_query("NEEDLE").unwrap();
    search.refresh(&mut term);
    assert_eq!(search.matches.len(), 3);
    assert_eq!(
        search.highlight(Point::new(0.into(), Column(7))),
        Some(false)
    );
    for query in ["[a.*]", "c:\\SRC\\foo.rs"] {
        search.set_query(query).unwrap();
        search.refresh(&mut term);
        assert_eq!(search.matches.len(), 1);
    }
}

#[test]
fn output_does_not_force_scrolling_back_to_the_match() {
    let mut term = terminal("needle\r\nother\r\nlast", 20, 2);
    let mut search = TerminalSearch::default();
    search.set_query("needle").unwrap();
    search.refresh(&mut term);
    term.scroll_display(alacritty_terminal::grid::Scroll::Bottom);
    Processor::<StdSyncHandler>::new().advance(&mut term, b"\r\nmore");
    search.invalidate();
    search.refresh(&mut term);
    assert_eq!(term.grid().display_offset(), 0);
}

#[test]
fn unchanged_input_events_preserve_the_current_match() {
    let mut term = terminal("one one", 20, 2);
    let mut search = TerminalSearch::default();
    search.set_query("one").unwrap();
    search.refresh(&mut term);
    search.step(SearchDirection::Down);
    search.refresh(&mut term);
    search.set_query("one").unwrap();
    assert!(!search.refresh(&mut term));
    assert_eq!(search.current(), Some(1));
}

#[test]
fn rebuilding_keeps_the_active_match_or_clamps_its_old_index() {
    let mut term = terminal("one one one", 20, 3);
    let mut search = TerminalSearch::default();
    search.set_query("one").unwrap();
    search.refresh(&mut term);
    search.step(SearchDirection::Down);
    search.refresh(&mut term);
    search.invalidate();
    search.refresh(&mut term);
    assert_eq!(search.current(), Some(1));

    Processor::<StdSyncHandler>::new().advance(&mut term, b"\x1b[Hone\x1b[K");
    search.invalidate();
    search.refresh(&mut term);
    assert_eq!(search.matches.len(), 1);
    assert_eq!(search.current(), Some(0));
}

#[test]
fn navigation_without_a_current_match_starts_at_either_end() {
    let mut term = terminal("one one", 20, 3);
    let mut search = TerminalSearch::default();
    search.set_query("one").unwrap();
    search.refresh(&mut term);
    search.current = None;
    search.step(SearchDirection::Up);
    assert_eq!(search.current(), Some(1));
    search.current = None;
    search.step(SearchDirection::Down);
    assert_eq!(search.current(), Some(0));
}
