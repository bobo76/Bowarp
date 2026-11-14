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

/// How many tabs other than the selected one wait for the user ([`Attention::Needed`]): the
/// count on the "next attention" button. The selected tab is left out as in [`next_attention`],
/// so the button never offers to jump to the tab already shown.
pub fn waiting_count(tabs: &[Option<Attention>], current: Option<usize>) -> usize {
    tabs.iter()
        .enumerate()
        .filter(|&(index, attention)| {
            Some(index) != current && *attention == Some(Attention::Needed)
        })
        .count()
}

/// What the window shows, named in its title.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TitleSubject<'a> {
    /// No tab is open.
    Nothing,
    /// A page (Settings, Git) replaces the selected tab.
    Page(&'a str),
    /// The selected tab, by the name the sidebar shows.
    Tab(&'a str),
}

/// The window's title: "<tab or page> — <app>", plain "<app>" with nothing to name, prefixed
/// "(N) " while `waiting` tabs wait for the user (every tab counts, the selected one too).
pub fn window_title(app_name: &str, subject: TitleSubject, waiting: usize) -> String {
    let prefix = if waiting > 0 {
        format!("({waiting}) ")
    } else {
        String::new()
    };
    let name = match subject {
        TitleSubject::Nothing => None,
        TitleSubject::Page(name) | TitleSubject::Tab(name) => Some(name.trim()),
    };
    match name.filter(|name| !name.is_empty()) {
        Some(name) => format!("{prefix}{name} \u{2014} {app_name}"),
        None => format!("{prefix}{app_name}"),
    }
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

    #[test]
    fn waiting_count_leaves_out_the_selected_tab() {
        let tabs = [NEEDED, DONE, NEEDED, None];
        assert_eq!(waiting_count(&tabs, None), 2);
        assert_eq!(waiting_count(&tabs, Some(1)), 2);
        assert_eq!(waiting_count(&tabs, Some(2)), 1);
        assert_eq!(waiting_count(&[], None), 0);
    }

    #[test]
    fn only_the_selected_tab_waiting_counts_nothing() {
        // The button hides; the jump still goes on to the finished turn.
        let tabs = [NEEDED, DONE];
        assert_eq!(waiting_count(&tabs, Some(0)), 0);
        assert_eq!(next_attention(&tabs, Some(0)), Some(1));
        // With a waiting prompt elsewhere, the count and the jump agree on it.
        let tabs = [NEEDED, DONE, NEEDED];
        assert_eq!(waiting_count(&tabs, Some(0)), 1);
        assert_eq!(next_attention(&tabs, Some(0)), Some(2));
    }

    #[test]
    fn window_title_names_the_tab_or_page() {
        let app = "Agent Workspace";
        assert_eq!(window_title(app, TitleSubject::Nothing, 0), app);
        assert_eq!(
            window_title(app, TitleSubject::Tab("~/src"), 0),
            "~/src \u{2014} Agent Workspace"
        );
        assert_eq!(
            window_title(app, TitleSubject::Page("Settings"), 0),
            "Settings \u{2014} Agent Workspace"
        );
        // A blank program title names nothing.
        assert_eq!(window_title(app, TitleSubject::Tab("  "), 0), app);
    }

    #[test]
    fn window_title_counts_the_waiting_tabs() {
        let app = "Agent Workspace";
        assert_eq!(
            window_title(app, TitleSubject::Tab("build"), 2),
            "(2) build \u{2014} Agent Workspace"
        );
        assert_eq!(
            window_title(app, TitleSubject::Page("Git"), 1),
            "(1) Git \u{2014} Agent Workspace"
        );
        assert_eq!(
            window_title(app, TitleSubject::Nothing, 3),
            "(3) Agent Workspace"
        );
    }
}
