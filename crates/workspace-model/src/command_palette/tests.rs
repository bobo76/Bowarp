use super::*;

fn candidate(label: &str) -> Candidate<'_> {
    Candidate { label, alias: None }
}

fn positions(query: &str, label: &str) -> Vec<usize> {
    fuzzy_match(query, candidate(label)).unwrap().positions
}

fn score(query: &str, label: &str) -> i32 {
    fuzzy_match(query, candidate(label)).unwrap().score
}

#[test]
fn a_subsequence_matches_ignoring_case() {
    assert_eq!(positions("nt", "New tab"), [0, 4]);
    assert_eq!(positions("NEW", "new tab"), [0, 1, 2]);
    assert!(fuzzy_match("tn", candidate("New tab")).is_none());
    assert!(fuzzy_match("x", candidate("New tab")).is_none());
}

#[test]
fn an_empty_query_matches_everything_with_nothing_drawn() {
    assert_eq!(
        fuzzy_match("  ", candidate("New tab")),
        Some(FuzzyMatch::default())
    );
}

#[test]
fn word_starts_beat_letters_inside_words() {
    // "t" at the start of "tab" scores over the "t" inside "Duplicate".
    assert_eq!(positions("dt", "Duplicate tab"), [0, 10]);
    assert!(score("ct", "Close tab") > score("ct", "Select"));
}

#[test]
fn a_consecutive_run_beats_scattered_letters() {
    assert!(score("tab", "New tab") > score("tab", "New taxicab"));
    assert_eq!(positions("tab", "the tab"), [4, 5, 6]);
}

#[test]
fn a_prefix_beats_a_later_word_start() {
    assert!(score("n", "Next tab") > score("n", "Go next"));
}

#[test]
fn capitals_start_words_in_camel_case() {
    assert_eq!(positions("te", "ToggleExplorer"), [0, 6]);
}

#[test]
fn every_term_must_match_in_any_order() {
    assert_eq!(positions("tab new", "New tab"), [0, 1, 2, 4, 5, 6]);
    assert!(fuzzy_match("tab zzz", candidate("New tab")).is_none());
}

#[test]
fn a_term_may_match_the_alias_and_draws_nothing_there() {
    let action = Candidate {
        label: "Show or hide the file explorer",
        alias: Some("ToggleExplorer"),
    };
    let found = fuzzy_match("toggleexp", action).unwrap();
    assert!(found.positions.is_empty());
    let tab = Candidate {
        label: "server",
        alias: Some("Backend"),
    };
    assert_eq!(
        fuzzy_match("back serv", tab).unwrap().positions,
        [0, 1, 2, 3]
    );
}

#[test]
fn ranking_orders_by_score_then_shorter_label_then_given_order() {
    let candidates = [
        candidate("Close tab"),
        candidate("New tab"),
        candidate("Next tab"),
        candidate("Find in terminal"),
        candidate("New tab"),
    ];
    let order: Vec<usize> = rank_matches("new", &candidates)
        .into_iter()
        .map(|(index, _)| index)
        .collect();
    assert_eq!(order, [1, 4]);
    let order: Vec<usize> = rank_matches("tab", &candidates)
        .into_iter()
        .map(|(index, _)| index)
        .collect();
    // Same score: the shorter labels first, ties in the given order.
    assert_eq!(order, [1, 4, 2, 0]);
}

#[test]
fn highlights_join_adjacent_characters_in_byte_ranges() {
    assert_eq!(highlight_ranges("New tab", &[0, 1, 4]), [0..2, 4..5]);
    // "é" takes two bytes.
    assert_eq!(highlight_ranges("éte", &[0, 2]), [0..2, 3..4]);
    assert_eq!(highlight_ranges("éte", &[0, 1, 2]).first(), Some(&(0..4)));
    assert!(highlight_ranges("New tab", &[]).is_empty());
}

#[test]
fn the_palette_leaves_out_duplicates_and_select_tab_keys() {
    let state = PaletteState {
        has_tab: true,
        terminal_shown: true,
        has_selection: true,
        clipboard_held: true,
        exited: false,
        file_panes_on: true,
        git_page_on: true,
    };
    for action in [
        "terminal::CopyOrSend",
        "terminal::PasteOrSend",
        "terminal::CloseFind",
        "workspace::SelectTab1",
        "workspace::SelectTab9",
        "workspace::ToggleCommandPalette",
    ] {
        let terminal = action.starts_with("terminal");
        assert!(!palette_lists(action, terminal, &state), "{action}");
    }
    assert!(palette_lists("terminal::SelectAll", true, &state));
    assert!(palette_lists("workspace::NewTab", false, &state));
}

#[test]
fn terminal_actions_need_the_terminal_shown() {
    let state = PaletteState {
        has_tab: true,
        has_selection: true,
        ..PaletteState::default()
    };
    assert!(!palette_lists("terminal::Copy", true, &state));
    assert!(!palette_lists("terminal::Find", true, &state));
    assert!(palette_lists("workspace::CloseTab", false, &state));
}

#[test]
fn actions_hide_while_they_cannot_apply() {
    let shown = PaletteState {
        has_tab: true,
        terminal_shown: true,
        ..PaletteState::default()
    };
    assert!(!palette_lists("terminal::Copy", true, &shown));
    assert!(!palette_lists("terminal::AllowClipboard", true, &shown));
    assert!(!palette_lists("terminal::DenyClipboard", true, &shown));
    assert!(palette_lists("terminal::Paste", true, &shown));
    assert!(!palette_lists("workspace::ToggleExplorer", false, &shown));
    assert!(!palette_lists("workspace::ToggleGitPage", false, &shown));
    let exited = PaletteState {
        exited: true,
        ..shown
    };
    assert!(!palette_lists("terminal::Paste", true, &exited));
    assert!(!palette_lists(
        "workspace::RenameTab",
        false,
        &PaletteState::default()
    ));
    let on = PaletteState {
        has_selection: true,
        clipboard_held: true,
        file_panes_on: true,
        git_page_on: true,
        ..shown
    };
    assert!(palette_lists("terminal::Copy", true, &on));
    assert!(palette_lists("terminal::AllowClipboard", true, &on));
    assert!(palette_lists("workspace::ToggleExplorer", false, &on));
    assert!(palette_lists("workspace::ToggleGitPage", false, &on));
    assert!(palette_lists("workspace::RenameTab", false, &on));
}
