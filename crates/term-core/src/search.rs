//! Search the terminal grid, including scrollback and soft-wrapped lines.

use std::time::{Duration, Instant};

use alacritty_terminal::event::EventListener;
use alacritty_terminal::grid::Dimensions;
use alacritty_terminal::index::{Column, Direction, Point};
use alacritty_terminal::term::Term;
use alacritty_terminal::term::search::{Match, RegexIter, RegexSearch};
use workspace_model::layout::Direction as SearchDirection;

/// While output keeps coming, the matches are rebuilt at most this often: each rebuild scans
/// the whole scrollback under the terminal lock on the UI thread.
const OUTPUT_REBUILD_INTERVAL: Duration = Duration::from_millis(200);

#[derive(Default)]
pub struct TerminalSearch {
    query: Option<String>,
    regex: Option<RegexSearch>,
    matches: Vec<Match>,
    current: Option<usize>,
    /// The matches may no longer match the grid.
    stale: bool,
    /// When the matches were last rebuilt; `None` makes the next rebuild immediate.
    last_rebuild: Option<Instant>,
    reveal: bool,
    /// (columns, screen lines): a resize rebuilds at once.
    layout: Option<(usize, usize)>,
    total_lines: Option<usize>,
}

/// What [`TerminalSearch::refresh`] did.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct SearchRefresh {
    /// The match count or the active match changed, so the search bar must be drawn again.
    pub changed: bool,
    /// Output arrived too soon after the last rebuild: refresh again after this long.
    pub retry_in: Option<Duration>,
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
        self.stale = true;
        self.last_rebuild = None;
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

    /// New output: rebuild at the next refresh, or once [`OUTPUT_REBUILD_INTERVAL`] has passed
    /// since the last rebuild.
    pub fn invalidate(&mut self) {
        self.stale = true;
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

    /// Recompute only on output, query changes, or resize; output at most once per
    /// [`OUTPUT_REBUILD_INTERVAL`]. Reveal only on explicit search navigation, so incoming
    /// output does not pull the viewport away from the reader.
    pub fn refresh<T: EventListener>(&mut self, term: &mut Term<T>) -> SearchRefresh {
        self.refresh_at(term, Instant::now())
    }

    /// [`Self::refresh`] without waiting for the rebuild interval: navigation must land on the
    /// grid's current matches.
    pub fn refresh_now<T: EventListener>(&mut self, term: &mut Term<T>) -> SearchRefresh {
        self.last_rebuild = None;
        self.refresh(term)
    }

    fn refresh_at<T: EventListener>(&mut self, term: &mut Term<T>, now: Instant) -> SearchRefresh {
        if self.regex.is_none() {
            return SearchRefresh::default();
        }
        let layout = (term.columns(), term.screen_lines());
        if self.layout != Some(layout) {
            self.layout = Some(layout);
            self.stale = true;
            self.last_rebuild = None;
        }
        let total_lines = term.total_lines();
        if self.total_lines != Some(total_lines) {
            self.total_lines = Some(total_lines);
            self.stale = true;
        }
        let wait = self
            .last_rebuild
            .map(|at| (at + OUTPUT_REBUILD_INTERVAL).saturating_duration_since(now))
            .filter(|wait| !wait.is_zero());
        let mut changed = false;
        if self.stale && wait.is_none() {
            let before = (self.matches.len(), self.current);
            self.stale = false;
            self.last_rebuild = Some(now);
            self.rebuild_matches(term);
            changed = before != (self.matches.len(), self.current);
        }
        self.reveal_current(term);
        SearchRefresh {
            changed,
            retry_in: wait.filter(|_| self.stale),
        }
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
