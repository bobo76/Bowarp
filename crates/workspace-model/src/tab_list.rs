//! The keyboard in the sidebar's tab list: which tabs have a row, and the row a list key
//! selects. Rows follow sidebar order (`Layout::tabs`).

use std::collections::HashSet;

use crate::layout::{Direction, Layout, TabId};
use crate::step_match;

/// A key that moves the selection in the focused tab list.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ListMove {
    Previous,
    Next,
    First,
    Last,
}

/// For each tab of `order`, whether the sidebar shows its row: while searching (`matches`),
/// only the matching tabs, collapsed groups' included; otherwise every tab outside a collapsed
/// group.
pub fn listed_tabs(
    layout: &Layout,
    order: &[TabId],
    matches: Option<&HashSet<TabId>>,
) -> Vec<bool> {
    order
        .iter()
        .map(|id| match matches {
            Some(matches) => matches.contains(id),
            None => !layout.group_of(*id).is_some_and(|group| group.collapsed),
        })
        .collect()
}

/// The row the list starts on: the selected tab's when it has a row, else the first row;
/// `None` when no tab has one.
pub fn list_entry(listed: &[bool], selected: Option<usize>) -> Option<usize> {
    selected
        .filter(|&index| listed.get(index) == Some(&true))
        .or_else(|| listed.iter().position(|&shown| shown))
}

/// The row `step` selects, from the selected tab: Previous/Next wrap around like Ctrl+Tab,
/// First/Last go to the ends; tabs without a row are skipped.
pub fn list_step(listed: &[bool], selected: Option<usize>, step: ListMove) -> Option<usize> {
    match step {
        ListMove::Previous => step_match(listed, selected, Direction::Up),
        ListMove::Next => step_match(listed, selected, Direction::Down),
        ListMove::First => step_match(listed, None, Direction::Down),
        ListMove::Last => step_match(listed, None, Direction::Up),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Tabs 1..=5: 1 pinned, group A [2, 3], 4, group B [5].
    fn sample() -> Layout {
        let mut layout = Layout::default();
        for id in 1..=5 {
            layout.add(id);
        }
        layout.toggle_pin(1);
        let a = layout.new_group_with(2, "A").unwrap();
        layout.move_to_group(3, a);
        layout.new_group_with(5, "B").unwrap();
        layout
    }

    fn collapse(layout: &mut Layout, name: &str) {
        let id = layout.groups().find(|group| group.name == name).unwrap().id;
        layout.group_mut(id).unwrap().collapsed = true;
    }

    #[test]
    fn collapsed_groups_have_no_rows_unless_searching() {
        let mut layout = sample();
        collapse(&mut layout, "A");
        let order = layout.tabs();
        assert_eq!(order, vec![1, 2, 3, 4, 5]);
        assert_eq!(
            listed_tabs(&layout, &order, None),
            vec![true, false, false, true, true]
        );
        // A search lists its matches even inside a collapsed group, and only them.
        let matches = HashSet::from([3, 5]);
        assert_eq!(
            listed_tabs(&layout, &order, Some(&matches)),
            vec![false, false, true, false, true]
        );
    }

    #[test]
    fn the_list_starts_on_the_selected_row_or_the_first_one() {
        let listed = [false, true, false, true];
        assert_eq!(list_entry(&listed, Some(3)), Some(3));
        assert_eq!(list_entry(&listed, Some(2)), Some(1));
        assert_eq!(list_entry(&listed, None), Some(1));
        assert_eq!(list_entry(&listed, Some(9)), Some(1));
        assert_eq!(list_entry(&[false, false], Some(0)), None);
        assert_eq!(list_entry(&[], None), None);
    }

    #[test]
    fn previous_and_next_wrap_and_skip_hidden_tabs() {
        let listed = [true, false, true, true, false];
        assert_eq!(list_step(&listed, Some(0), ListMove::Next), Some(2));
        assert_eq!(list_step(&listed, Some(3), ListMove::Next), Some(0));
        assert_eq!(list_step(&listed, Some(0), ListMove::Previous), Some(3));
        assert_eq!(list_step(&listed, Some(2), ListMove::Previous), Some(0));
        // From a hidden tab: the nearest row in that direction.
        assert_eq!(list_step(&listed, Some(1), ListMove::Next), Some(2));
        assert_eq!(list_step(&listed, Some(4), ListMove::Previous), Some(3));
        // A single row stays selected.
        assert_eq!(list_step(&[false, true], Some(1), ListMove::Next), Some(1));
    }

    #[test]
    fn first_and_last_go_to_the_ends_of_the_rows() {
        let listed = [false, true, true, false];
        assert_eq!(list_step(&listed, Some(2), ListMove::First), Some(1));
        assert_eq!(list_step(&listed, Some(1), ListMove::Last), Some(2));
        assert_eq!(list_step(&[false], Some(0), ListMove::First), None);
    }
}
