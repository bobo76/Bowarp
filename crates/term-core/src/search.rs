//! Search the terminal grid, including scrollback and soft-wrapped lines.

use alacritty_terminal::event::EventListener;
use alacritty_terminal::grid::Dimensions;
use alacritty_terminal::index::{Column, Direction, Point};
use alacritty_terminal::term::Term;
use alacritty_terminal::term::search::{Match, RegexIter, RegexSearch};
use workspace_model::layout::Direction as SearchDirection;

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

    pub fn step(&mut self, direction: SearchDirection) {
        if self.matches.is_empty() {
            return;
        }
        let len = self.matches.len();
        self.current = Some(match (self.current, direction) {
            (Some(index), SearchDirection::Down) => (index + 1) % len,
            (Some(index), SearchDirection::Up) => (index + len - 1) % len,
            (None, SearchDirection::Down) => 0,
            (None, SearchDirection::Up) => len - 1,
        });
        self.reveal = true;
    }

    /// Recompute only on output, query changes, or resize. Reveal only on explicit search
    /// navigation, so incoming output does not pull the viewport away from the reader.
    pub fn refresh<T: EventListener>(&mut self, term: &mut Term<T>) -> bool {
        if self.regex.is_none() {
            return false;
        }
        let dimensions = (term.columns(), term.screen_lines(), term.total_lines());
        let refresh = self.dirty || self.dimensions != Some(dimensions);
        if refresh {
            self.dirty = false;
            self.dimensions = Some(dimensions);
            self.rebuild_matches(term);
        }
        self.reveal_current(term);
        refresh
    }

    fn rebuild_matches<T: EventListener>(&mut self, term: &Term<T>) {
        let previous = self
            .current
            .and_then(|index| self.matches.get(index))
            .cloned();
        let Some(regex) = &mut self.regex else {
            return;
        };
        self.matches = RegexIter::new(
            Point::new(term.topmost_line(), Column(0)),
            Point::new(term.bottommost_line(), term.last_column()),
            Direction::Right,
            term,
            regex,
        )
        .collect();
        self.current = self.current_after_rebuild(previous.as_ref(), term.grid().display_offset());
    }

    fn current_after_rebuild(
        &self,
        previous: Option<&Match>,
        display_offset: usize,
    ) -> Option<usize> {
        if self.matches.is_empty() {
            return None;
        }
        if let Some(previous) = previous {
            return Some(
                self.matches
                    .binary_search_by_key(previous.start(), |range| *range.start())
                    .unwrap_or_else(|_| self.current.unwrap_or(0).min(self.matches.len() - 1)),
            );
        }
        let top = -(display_offset as i32);
        Some(
            self.matches
                .iter()
                .position(|range| range.start().line.0 >= top)
                .unwrap_or(self.matches.len() - 1),
        )
    }

    fn reveal_current<T: EventListener>(&mut self, term: &mut Term<T>) {
        if self.reveal {
            self.reveal = false;
            if let Some(index) = self.current {
                let range = &self.matches[index];
                term.scroll_to_point(*range.end());
                term.scroll_to_point(*range.start());
            }
        }
    }
}

#[cfg(test)]
mod tests;
