//! The changed lines of a diff picked for staging, and the line cursor: what the Git page's
//! clicks and keys do to them. Every move goes over the changed lines only (a context line
//! can't be staged on its own), given in order by [`super::TextDiff::changed_lines`].

use std::collections::BTreeSet;

use super::patch::LineRef;
use crate::layout::Direction;

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct LineSelection {
    /// Where a Shift range starts.
    anchor: Option<LineRef>,
    /// The keyboard's line.
    cursor: Option<LineRef>,
    selected: BTreeSet<LineRef>,
}

impl LineSelection {
    pub fn cursor(&self) -> Option<LineRef> {
        self.cursor
    }

    pub fn selected(&self) -> &BTreeSet<LineRef> {
        &self.selected
    }

    pub fn contains(&self, line: LineRef) -> bool {
        self.selected.contains(&line)
    }

    /// The lines an action applies to: the selected ones, else the cursor's.
    pub fn targets(&self) -> BTreeSet<LineRef> {
        if self.selected.is_empty() {
            self.cursor.into_iter().collect()
        } else {
            self.selected.clone()
        }
    }

    /// Select only `line`.
    pub fn click(&mut self, line: LineRef) {
        self.anchor = Some(line);
        self.cursor = Some(line);
        self.selected = BTreeSet::from([line]);
    }

    /// Select the changed lines from the anchor to `line` (a drag does the same), replacing the
    /// selection; without an anchor, as [`Self::click`].
    pub fn shift_click(&mut self, line: LineRef, changed: &[LineRef]) {
        let Some(anchor) = self.anchor else {
            self.click(line);
            return;
        };
        self.cursor = Some(line);
        self.selected = range(anchor, line, changed);
    }

    /// Add `line` to the selection, or take it out; it becomes the anchor.
    pub fn toggle(&mut self, line: LineRef) {
        self.anchor = Some(line);
        self.cursor = Some(line);
        if !self.selected.remove(&line) {
            self.selected.insert(line);
        }
    }

    /// Move the cursor to the next changed line in `direction`; from no cursor, to the first
    /// (Down) or the last (Up). The selection is cleared. Returns the new cursor (`None`
    /// without changed lines).
    pub fn step(&mut self, direction: Direction, changed: &[LineRef]) -> Option<LineRef> {
        let next = self.next_line(direction, changed)?;
        self.anchor = Some(next);
        self.selected.clear();
        self.cursor = Some(next);
        Some(next)
    }

    /// As [`Self::step`], but the selection runs from the anchor to the new cursor.
    pub fn extend(&mut self, direction: Direction, changed: &[LineRef]) -> Option<LineRef> {
        let next = self.next_line(direction, changed)?;
        let anchor = *self.anchor.get_or_insert(self.cursor.unwrap_or(next));
        self.selected = range(anchor, next, changed);
        self.cursor = Some(next);
        Some(next)
    }

    /// The changed line a step in `direction` goes to.
    fn next_line(&self, direction: Direction, changed: &[LineRef]) -> Option<LineRef> {
        let at = self.cursor.and_then(|cursor| line_index(cursor, changed));
        let next = match (at, direction) {
            (None, Direction::Down) => changed.first(),
            (None, Direction::Up) => changed.last(),
            (Some(at), Direction::Down) => changed.get(at + 1).or(changed.get(at)),
            (Some(at), Direction::Up) => changed.get(at.saturating_sub(1)),
        };
        next.copied()
    }

    /// Unselect every line (the cursor stays and becomes the anchor). Returns whether any was
    /// selected.
    pub fn clear(&mut self) -> bool {
        self.anchor = self.cursor;
        !std::mem::take(&mut self.selected).is_empty()
    }

    /// After lines were staged or unstaged and the diff read again (`changed` its changed
    /// lines): nothing is selected and the cursor goes to the changed line at index `at` of
    /// the new lines (where the first applied line was), else to the last one.
    pub fn after_apply(&mut self, changed: &[LineRef], at: usize) {
        self.cursor = changed.get(at).or(changed.last()).copied();
        self.anchor = self.cursor;
        self.selected.clear();
    }
}

/// The index of `line` in `changed`.
pub fn line_index(line: LineRef, changed: &[LineRef]) -> Option<usize> {
    changed.binary_search(&line).ok()
}

/// The changed lines from `from` to `to`, either way round; none when one isn't a changed line.
fn range(from: LineRef, to: LineRef, changed: &[LineRef]) -> BTreeSet<LineRef> {
    let (Some(from), Some(to)) = (line_index(from, changed), line_index(to, changed)) else {
        return BTreeSet::new();
    };
    changed[from.min(to)..=from.max(to)]
        .iter()
        .copied()
        .collect()
}

#[cfg(test)]
mod tests;
