//! The tabs the user selected, in order, for Back / Forward (like a browser's history) and
//! Last tab (switch with the tab selected before). Tabs are named by their id; a closed tab
//! leaves the history.

/// How many earlier tabs Back can reach; older ones are forgotten.
const MAX_BACK: usize = 50;

#[derive(Debug, Default, Clone)]
pub struct TabHistory {
    /// Earlier tabs, the most recent last.
    back: Vec<u64>,
    /// The selected tab.
    current: Option<u64>,
    /// Tabs left with Back, the next one last.
    forward: Vec<u64>,
}

impl TabHistory {
    /// Tab `id` was selected. Selecting another tab than Back or Forward would drops the
    /// Forward list, as in a browser; selecting the selected tab again changes nothing. After
    /// the selected tab closed, the tab selected in its place keeps both lists, minus itself.
    pub fn visit(&mut self, id: u64) {
        if self.current == Some(id) {
            return;
        }
        match self.current.replace(id) {
            Some(current) => {
                push_back(&mut self.back, current);
                self.forward.clear();
            }
            None => {
                for list in [&mut self.back, &mut self.forward] {
                    if list.last() == Some(&id) {
                        list.pop();
                    }
                }
            }
        }
    }

    /// The tab before the selected one, which becomes the selected one; `None` at the start.
    pub fn back(&mut self) -> Option<u64> {
        let id = self.back.pop()?;
        if let Some(current) = self.current.replace(id) {
            self.forward.push(current);
        }
        Some(id)
    }

    /// The tab left with Back, which becomes the selected one; `None` when there is none.
    pub fn forward(&mut self) -> Option<u64> {
        let id = self.forward.pop()?;
        if let Some(current) = self.current.replace(id) {
            push_back(&mut self.back, current);
        }
        Some(id)
    }

    /// The tab selected before the current one, for Last tab. Selecting it is a visit, so
    /// pressing Last tab again comes back: it switches between two tabs.
    pub fn last(&self) -> Option<u64> {
        self.back.last().copied()
    }

    pub fn can_go_back(&self) -> bool {
        !self.back.is_empty()
    }

    pub fn can_go_forward(&self) -> bool {
        !self.forward.is_empty()
    }

    /// Tab `id` closed: forget it. Its neighbours in the history that become equal merge, so
    /// Back never stays on the same tab. The caller then visits the tab selected instead.
    pub fn remove(&mut self, id: u64) {
        if self.current == Some(id) {
            self.current = None;
        }
        for list in [&mut self.back, &mut self.forward] {
            list.retain(|&other| other != id);
            list.dedup();
        }
    }
}

fn push_back(back: &mut Vec<u64>, id: u64) {
    if back.last() != Some(&id) {
        back.push(id);
    }
    if back.len() > MAX_BACK {
        back.remove(0);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn visited(ids: &[u64]) -> TabHistory {
        let mut history = TabHistory::default();
        for &id in ids {
            history.visit(id);
        }
        history
    }

    #[test]
    fn back_and_forward_walk_the_visits() {
        let mut history = visited(&[1, 2, 3]);
        assert!(history.can_go_back() && !history.can_go_forward());
        assert_eq!(history.back(), Some(2));
        assert_eq!(history.back(), Some(1));
        assert_eq!(history.back(), None);
        assert_eq!(history.forward(), Some(2));
        assert_eq!(history.forward(), Some(3));
        assert_eq!(history.forward(), None);
    }

    #[test]
    fn a_new_visit_drops_forward_and_a_repeat_is_ignored() {
        let mut history = visited(&[1, 2, 3]);
        history.back();
        history.visit(2);
        assert!(
            history.can_go_forward(),
            "selecting the selected tab changes nothing"
        );
        history.visit(4);
        assert!(!history.can_go_forward());
        assert_eq!(history.back(), Some(2));
        assert_eq!(history.back(), Some(1));
    }

    #[test]
    fn last_switches_between_two_tabs() {
        let mut history = visited(&[1, 2, 3]);
        assert_eq!(history.last(), Some(2));
        history.visit(2);
        assert_eq!(history.last(), Some(3));
        history.visit(3);
        assert_eq!(history.last(), Some(2));
        assert_eq!(TabHistory::default().last(), None);
    }

    #[test]
    fn closed_tabs_leave_the_history() {
        let mut history = visited(&[1, 2, 1, 3]);
        history.remove(1);
        assert_eq!(history.back(), Some(2), "1 is gone and 2, 2 merged");
        assert_eq!(history.back(), None);

        // The selected tab closes: the caller visits the one selected in its place.
        let mut history = visited(&[1, 2, 3]);
        history.remove(3);
        history.visit(2);
        assert_eq!(history.back(), Some(1), "Back doesn't stay on 2");
        assert_eq!(history.back(), None);
    }

    #[test]
    fn closing_a_tab_between_back_and_forward_does_not_leave_a_step_in_place() {
        let mut history = visited(&[1, 2, 3]);
        history.back(); // at 2, back [1], forward [3]
        history.remove(2);
        history.visit(1);
        assert_eq!(history.back(), None);
        assert_eq!(history.forward(), Some(3));
    }

    #[test]
    fn back_is_bounded() {
        let ids: Vec<u64> = (0..(MAX_BACK as u64 + 10)).collect();
        let mut history = visited(&ids);
        let mut steps = 0;
        while history.back().is_some() {
            steps += 1;
        }
        assert_eq!(steps, MAX_BACK);
    }
}
