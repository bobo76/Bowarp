use super::*;

fn at(hunk: usize, line: usize) -> LineRef {
    LineRef { hunk, line }
}

/// Changed lines of a two-hunk diff (the context lines between them are left out).
fn changed() -> Vec<LineRef> {
    vec![at(0, 1), at(0, 2), at(0, 4), at(1, 3), at(1, 4)]
}

fn selected(selection: &LineSelection) -> Vec<LineRef> {
    selection.selected().iter().copied().collect()
}

#[test]
fn click_selects_one_line() {
    let mut selection = LineSelection::default();
    selection.click(at(0, 2));
    selection.click(at(0, 4));

    assert_eq!(selected(&selection), [at(0, 4)]);
    assert_eq!(selection.cursor(), Some(at(0, 4)));
}

#[test]
fn shift_click_selects_a_range_across_hunks_either_way() {
    let changed = changed();
    let mut selection = LineSelection::default();
    selection.click(at(0, 2));

    selection.shift_click(at(1, 3), &changed);
    assert_eq!(selected(&selection), [at(0, 2), at(0, 4), at(1, 3)]);

    // The anchor stays: the range is replaced, not grown.
    selection.shift_click(at(0, 1), &changed);
    assert_eq!(selected(&selection), [at(0, 1), at(0, 2)]);
    assert_eq!(selection.cursor(), Some(at(0, 1)));
}

#[test]
fn shift_click_without_an_anchor_is_a_click() {
    let mut selection = LineSelection::default();
    selection.shift_click(at(1, 4), &changed());

    assert_eq!(selected(&selection), [at(1, 4)]);
}

#[test]
fn toggle_adds_and_removes_one_line() {
    let mut selection = LineSelection::default();
    selection.click(at(0, 1));
    selection.toggle(at(1, 3));
    assert_eq!(selected(&selection), [at(0, 1), at(1, 3)]);

    selection.toggle(at(0, 1));
    assert_eq!(selected(&selection), [at(1, 3)]);
    assert_eq!(selection.cursor(), Some(at(0, 1)));
}

#[test]
fn step_moves_over_changed_lines_and_clears_the_selection() {
    let changed = changed();
    let mut selection = LineSelection::default();

    assert_eq!(selection.step(Direction::Down, &changed), Some(at(0, 1)));
    selection.toggle(at(1, 4));
    selection.click(at(0, 2));
    // From (0, 2) the next changed line skips the context line (0, 3).
    assert_eq!(selection.step(Direction::Down, &changed), Some(at(0, 4)));
    assert!(selection.selected().is_empty());
    assert_eq!(selection.step(Direction::Down, &changed), Some(at(1, 3)));
    assert_eq!(selection.step(Direction::Up, &changed), Some(at(0, 4)));
}

#[test]
fn step_stays_at_the_ends() {
    let changed = changed();
    let mut selection = LineSelection::default();

    assert_eq!(selection.step(Direction::Up, &changed), Some(at(1, 4)));
    assert_eq!(selection.step(Direction::Down, &changed), Some(at(1, 4)));
    selection.click(at(0, 1));
    assert_eq!(selection.step(Direction::Up, &changed), Some(at(0, 1)));
    assert_eq!(LineSelection::default().step(Direction::Down, &[]), None);
}

#[test]
fn step_with_extend_grows_and_shrinks_from_the_anchor() {
    let changed = changed();
    let mut selection = LineSelection::default();
    selection.click(at(0, 2));

    selection.extend(Direction::Down, &changed);
    selection.extend(Direction::Down, &changed);
    assert_eq!(selected(&selection), [at(0, 2), at(0, 4), at(1, 3)]);

    selection.extend(Direction::Up, &changed);
    assert_eq!(selected(&selection), [at(0, 2), at(0, 4)]);
}

#[test]
fn step_with_extend_after_a_plain_move_starts_at_the_cursor() {
    let changed = changed();
    let mut selection = LineSelection::default();
    selection.step(Direction::Down, &changed);

    selection.extend(Direction::Down, &changed);

    assert_eq!(selected(&selection), [at(0, 1), at(0, 2)]);
}

#[test]
fn targets_are_the_selection_else_the_cursor() {
    let changed = changed();
    let mut selection = LineSelection::default();
    assert!(selection.targets().is_empty());

    selection.step(Direction::Down, &changed);
    assert_eq!(selection.targets(), BTreeSet::from([at(0, 1)]));

    selection.click(at(0, 4));
    selection.toggle(at(1, 4));
    assert_eq!(selection.targets(), BTreeSet::from([at(0, 4), at(1, 4)]));
}

#[test]
fn clear_keeps_the_cursor_as_the_anchor() {
    let changed = changed();
    let mut selection = LineSelection::default();
    selection.click(at(0, 1));
    selection.shift_click(at(0, 4), &changed);

    assert!(selection.clear());
    assert!(!selection.clear());
    assert_eq!(selection.cursor(), Some(at(0, 4)));
    selection.extend(Direction::Down, &changed);
    assert_eq!(selected(&selection), [at(0, 4), at(1, 3)]);
}

#[test]
fn after_apply_puts_the_cursor_where_the_applied_lines_were() {
    let mut selection = LineSelection::default();
    selection.click(at(0, 2));
    // The new diff lost two lines: index 1 is now the line after them.
    let left = [at(0, 1), at(0, 2), at(1, 1)];

    selection.after_apply(&left, 1);

    assert!(selection.selected().is_empty());
    assert_eq!(selection.cursor(), Some(at(0, 2)));
}

#[test]
fn after_apply_at_the_end_goes_to_the_last_line_or_nowhere() {
    let mut selection = LineSelection::default();
    selection.after_apply(&[at(0, 1), at(0, 2)], 4);
    assert_eq!(selection.cursor(), Some(at(0, 2)));

    selection.after_apply(&[], 0);
    assert_eq!(selection.cursor(), None);
    assert!(selection.targets().is_empty());
}

#[test]
fn a_context_line_gives_no_range() {
    let mut selection = LineSelection::default();
    selection.click(at(0, 1));

    selection.shift_click(at(0, 3), &changed());

    assert!(selection.selected().is_empty());
}
