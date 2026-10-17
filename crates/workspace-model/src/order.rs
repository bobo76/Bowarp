//! Tab ordering rules. Pinned tabs form a block at the top of the list; moves stay inside a
//! tab's block. Functions take the pinned flag of every tab, in order.

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Direction {
    Up,
    Down,
}

/// The index the tab at `index` swaps with when moved, or `None` at the edge of its block.
pub fn move_target(pinned: &[bool], index: usize, direction: Direction) -> Option<usize> {
    let target = match direction {
        Direction::Up => index.checked_sub(1)?,
        Direction::Down => index + 1,
    };
    let same_block = pinned.get(target)? == pinned.get(index)?;
    same_block.then_some(target)
}

/// Where the tab at `index` goes when its pinned flag flips: the end of the pinned block when
/// pinning, the start of the unpinned block when unpinning. The result is the index to move it
/// to with [`index_after_move`] semantics (remove, then insert).
pub fn pin_target(pinned: &[bool], index: usize) -> usize {
    let pinned_count = pinned.iter().filter(|&&pinned| pinned).count();
    if pinned[index] {
        pinned_count - 1
    } else {
        pinned_count
    }
}

/// Where the tab at `index` ends up after the tab at `from` is removed and inserted at `to`.
pub fn index_after_move(index: usize, from: usize, to: usize) -> usize {
    if index == from {
        to
    } else if from < index && index <= to {
        index - 1
    } else if to <= index && index < from {
        index + 1
    } else {
        index
    }
}

/// Tabs closed by "close other tabs" on `index`: every other unpinned tab.
pub fn others_to_close(pinned: &[bool], index: usize) -> Vec<usize> {
    (0..pinned.len())
        .filter(|&other| other != index && !pinned[other])
        .collect()
}

/// Tabs closed by "close tabs below" on `index`: the unpinned tabs after it.
pub fn below_to_close(pinned: &[bool], index: usize) -> Vec<usize> {
    (index + 1..pinned.len())
        .filter(|&other| !pinned[other])
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    const P: bool = true;
    const U: bool = false;

    #[test]
    fn moves_stay_inside_the_block() {
        let pinned = [P, P, U, U];
        assert_eq!(move_target(&pinned, 0, Direction::Up), None);
        assert_eq!(move_target(&pinned, 0, Direction::Down), Some(1));
        assert_eq!(move_target(&pinned, 1, Direction::Down), None);
        assert_eq!(move_target(&pinned, 2, Direction::Up), None);
        assert_eq!(move_target(&pinned, 2, Direction::Down), Some(3));
        assert_eq!(move_target(&pinned, 3, Direction::Down), None);
    }

    #[test]
    fn pinning_goes_to_the_end_of_the_pinned_block() {
        assert_eq!(pin_target(&[P, U, U, U], 3), 1);
        assert_eq!(pin_target(&[U, U], 1), 0);
    }

    #[test]
    fn unpinning_goes_to_the_start_of_the_unpinned_block() {
        // Removing the first of three pinned tabs and inserting at 2 puts it after the others.
        assert_eq!(pin_target(&[P, P, P, U], 0), 2);
        assert_eq!(pin_target(&[P, U], 0), 0);
    }

    #[test]
    fn indices_follow_a_move() {
        // Move 1 -> 3: [a b c d] becomes [a c d b].
        assert_eq!(index_after_move(1, 1, 3), 3);
        assert_eq!(index_after_move(2, 1, 3), 1);
        assert_eq!(index_after_move(3, 1, 3), 2);
        assert_eq!(index_after_move(0, 1, 3), 0);
        // Move 3 -> 1: [a b c d] becomes [a d b c].
        assert_eq!(index_after_move(1, 3, 1), 2);
        assert_eq!(index_after_move(2, 3, 1), 3);
        assert_eq!(index_after_move(3, 3, 1), 1);
    }

    #[test]
    fn closing_spares_pinned_tabs() {
        let pinned = [P, U, U, P, U];
        assert_eq!(others_to_close(&pinned, 2), vec![1, 4]);
        assert_eq!(others_to_close(&pinned, 0), vec![1, 2, 4]);
        assert_eq!(below_to_close(&pinned, 1), vec![2, 4]);
        assert_eq!(below_to_close(&pinned, 4), Vec::<usize>::new());
    }
}
