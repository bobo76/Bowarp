//! "Jump to the next tab needing attention": which tab that is.

/// Why a tab wants the user, least urgent first.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum Attention {
    /// The agent finished a turn the user has not looked at yet.
    Done,
    /// The agent waits for a permission decision or an answer.
    Needed,
}

/// The tab to jump to, given each tab's attention in sidebar order and the selected tab. The
/// selected tab is not a candidate (the user is already looking at it); among the others only
/// the most urgent kind present counts (a waiting prompt before a finished turn), and of those
/// the first one after `current`, wrapping around, so repeating the jump cycles through them.
/// `None` when no other tab wants attention.
pub fn next_attention(tabs: &[Option<Attention>], current: Option<usize>) -> Option<usize> {
    let start = current.map_or(0, |current| current + 1);
    let mut candidates = (0..tabs.len())
        .map(|offset| (start + offset) % tabs.len())
        .filter(|&index| Some(index) != current)
        .filter_map(|index| Some((index, tabs[index]?)));
    let most_urgent = candidates.clone().map(|(_, attention)| attention).max()?;
    candidates
        .find(|&(_, attention)| attention == most_urgent)
        .map(|(index, _)| index)
}

#[cfg(test)]
mod tests {
    use super::*;

    const NEEDED: Option<Attention> = Some(Attention::Needed);
    const DONE: Option<Attention> = Some(Attention::Done);

    #[test]
    fn nothing_to_jump_to() {
        assert_eq!(next_attention(&[], None), None);
        assert_eq!(next_attention(&[None, None], Some(0)), None);
        assert_eq!(next_attention(&[None, NEEDED], Some(1)), None);
    }

    #[test]
    fn cycles_through_tabs_needing_attention_after_the_selected_one() {
        let tabs = [NEEDED, None, NEEDED, None];
        assert_eq!(next_attention(&tabs, Some(0)), Some(2));
        assert_eq!(next_attention(&tabs, Some(1)), Some(2));
        assert_eq!(next_attention(&tabs, Some(2)), Some(0));
        assert_eq!(next_attention(&tabs, Some(3)), Some(0));
        assert_eq!(next_attention(&[None, NEEDED], None), Some(1));
        assert_eq!(next_attention(&tabs, None), Some(0));
    }

    #[test]
    fn a_waiting_prompt_comes_before_a_finished_turn() {
        let tabs = [None, DONE, None, NEEDED];
        assert_eq!(next_attention(&tabs, Some(0)), Some(3));
        // Already on the only waiting prompt: the finished turn is next.
        assert_eq!(next_attention(&tabs, Some(3)), Some(1));
        assert_eq!(next_attention(&[DONE, None, DONE], Some(0)), Some(2));
    }
}
