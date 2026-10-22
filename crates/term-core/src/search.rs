//! Search the terminal grid, including scrollback and soft-wrapped lines.

use alacritty_terminal::event::EventListener;
use alacritty_terminal::grid::Dimensions;
use alacritty_terminal::index::{Column, Direction, Point};
use alacritty_terminal::term::Term;
use alacritty_terminal::term::search::{Match, RegexIter, RegexSearch};

#[derive(Default)]
pub struct TerminalSearch {
    query: Option<String>,
    regex: Option<RegexSearch>,
    matches: Vec<Match>,
    current: Option<usize>,
    dirty: bool,
    reveal: bool,
    dimensions: Option<(usize, usize, usize)>,
}

impl TerminalSearch {
    /// Search literal text, ignoring case even when the query contains uppercase letters.
    pub fn set_query(&mut self, query: &str) -> Result<(), String> {
        if self.query.as_deref() == Some(query) {
            return Ok(());
        }
        self.query = None;
        self.matches.clear();
        self.current = None;
        self.regex = None;
        self.invalidate();
        self.reveal = true;
        if !query.is_empty() {
            let mut pattern = String::from("(?i)");
            for character in query.chars() {
                if "\\.+*?()|[]{}^$#&-~".contains(character) {
                    pattern.push('\\');
                }
                pattern.push(character);
            }
            self.regex = Some(RegexSearch::new(&pattern).map_err(|error| error.to_string())?);
        }
        self.query = Some(query.to_owned());
        Ok(())
    }

    pub fn invalidate(&mut self) {
        self.dirty = true;
    }

    pub fn matches(&self) -> &[Match] {
        &self.matches
    }

    pub fn current(&self) -> Option<usize> {
        self.current
    }

    /// Whether a cell is a match, and whether it belongs to the active match.
    pub fn highlight(&self, point: Point) -> Option<bool> {
        let index = self.matches.partition_point(|range| *range.end() < point);
        self.matches
            .get(index)
            .filter(|range| range.contains(&point))
            .map(|_| self.current == Some(index))
    }

    pub fn step(&mut self, forward: bool) {
        if self.matches.is_empty() {
            return;
        }
        let len = self.matches.len();
        self.current = Some(match (self.current, forward) {
            (Some(index), true) => (index + 1) % len,
            (Some(index), false) => (index + len - 1) % len,
            (None, true) => 0,
            (None, false) => len - 1,
        });
        self.reveal = true;
    }

    /// Recompute only on output, query changes, or resize. Reveal only on explicit search
    /// navigation, so incoming output does not pull the viewport away from the reader.
    pub fn refresh<T: EventListener>(&mut self, term: &mut Term<T>) -> bool {
        let Some(regex) = &mut self.regex else {
            return false;
        };
        let dimensions = (term.columns(), term.screen_lines(), term.total_lines());
        let refresh = self.dirty || self.dimensions != Some(dimensions);
        if refresh {
            self.dirty = false;
            self.dimensions = Some(dimensions);
            let previous = self
                .current
                .and_then(|index| self.matches.get(index))
                .cloned();
            self.matches = RegexIter::new(
                Point::new(term.topmost_line(), Column(0)),
                Point::new(term.bottommost_line(), term.last_column()),
                Direction::Right,
                term,
                regex,
            )
            .collect();
            self.current = if self.matches.is_empty() {
                None
            } else if let Some(previous) = previous {
                Some(
                    self.matches
                        .binary_search_by_key(previous.start(), |range| *range.start())
                        .unwrap_or_else(|_| self.current.unwrap_or(0).min(self.matches.len() - 1)),
                )
            } else {
                let top = -(term.grid().display_offset() as i32);
                Some(
                    self.matches
                        .iter()
                        .position(|range| range.start().line.0 >= top)
                        .unwrap_or(self.matches.len() - 1),
                )
            };
        }
        if self.reveal {
            self.reveal = false;
            if let Some(index) = self.current {
                let range = &self.matches[index];
                term.scroll_to_point(*range.end());
                term.scroll_to_point(*range.start());
            }
        }
        refresh
    }
}

#[cfg(test)]
mod tests {
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
        search.step(true);
        search.refresh(&mut term);
        assert_eq!(search.current(), Some(0));
        assert!(term.grid().display_offset() > 0);
        search.step(false);
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
                search.step(true);
                search.step(false);
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
        search.step(true);
        search.refresh(&mut term);
        search.set_query("one").unwrap();
        assert!(!search.refresh(&mut term));
        assert_eq!(search.current(), Some(1));
    }
}
