//! The command palette's pure logic: which keymap actions it lists given the window's state,
//! and the fuzzy matching that ranks actions and tabs as the user types (case-insensitive
//! subsequence, scored for word starts, consecutive runs and a prefix; space-separated terms
//! each match, in any order).

use std::ops::Range;

use crate::keymap::short_name;

/// Points for each matched character.
const MATCH: i32 = 1;
/// More for a character starting a word (`New tab`'s `t`, `ToggleExplorer`'s `E`).
const WORD_START: i32 = 8;
/// More for a character right after the one matched before it.
const CONSECUTIVE: i32 = 6;
/// More for a term's first character matching the text's first one.
const PREFIX: i32 = 4;
/// Lost for each character skipped between two matched ones.
const GAP: i32 = 1;

/// Actions never listed: duplicates of listed ones (`CopyOrSend`, `PasteOrSend`), a key with
/// nothing to pick (`CloseFind`), and the palette's own key.
const NEVER_LISTED: &[&str] = &[
    "CopyOrSend",
    "PasteOrSend",
    "CloseFind",
    "ToggleCommandPalette",
];

/// What the window shows when the palette opens, for [`palette_lists`].
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct PaletteState {
    /// A tab is open.
    pub has_tab: bool,
    /// The selected tab's terminal shows (no Settings or Git page, not the empty state).
    pub terminal_shown: bool,
    /// The selected terminal has a selection.
    pub has_selection: bool,
    /// A clipboard write waits for Allow or Deny in the selected tab.
    pub clipboard_held: bool,
    /// The selected tab's program exited.
    pub exited: bool,
    /// The "File explorer and viewer" switch is on.
    pub file_panes_on: bool,
    /// The "Git page" switch is on.
    pub git_page_on: bool,
}

/// Whether the palette lists `action` (a full keymap name, `terminal` when it is bound in
/// the terminal's context) in `state`: actions that can't apply now are left out, and so
/// are the select-tab keys (the palette lists the tabs by name).
pub fn palette_lists(action: &str, terminal: bool, state: &PaletteState) -> bool {
    let name = short_name(action);
    if NEVER_LISTED.contains(&name) || name.starts_with("SelectTab") {
        return false;
    }
    if terminal && !state.terminal_shown {
        return false;
    }
    match name {
        "Copy" => state.has_selection,
        "AllowClipboard" | "DenyClipboard" => state.clipboard_held,
        "Paste" => !state.exited,
        "ToggleExplorer" => state.file_panes_on,
        "ToggleGitPage" => state.git_page_on,
        "RenameTab" => state.has_tab,
        _ => true,
    }
}

/// A row the palette can match: the label it shows, and another name it is found by (an
/// action's settings-file name, a tab's group) whose matches are not drawn.
#[derive(Clone, Copy, Debug)]
pub struct Candidate<'a> {
    pub label: &'a str,
    pub alias: Option<&'a str>,
}

/// How well a candidate matches a query.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct FuzzyMatch {
    pub score: i32,
    /// The label's matched characters (char indices, ascending), drawn highlighted.
    pub positions: Vec<usize>,
}

/// Match every space-separated term of `query` in `candidate` (each in its label or its
/// alias, whichever scores more), ignoring case; `None` when a term matches neither.
pub fn fuzzy_match(query: &str, candidate: Candidate<'_>) -> Option<FuzzyMatch> {
    let mut total = FuzzyMatch::default();
    for term in query.split_whitespace() {
        let found = match_field(term, candidate)?;
        total.score += found.score;
        total.positions.extend(found.positions);
    }
    total.positions.sort_unstable();
    total.positions.dedup();
    Some(total)
}

/// The candidates matching `query` (their indexes, with the match), best first: by score,
/// then the shorter label, then the given order.
pub fn rank_matches(query: &str, candidates: &[Candidate<'_>]) -> Vec<(usize, FuzzyMatch)> {
    let mut ranked: Vec<(usize, FuzzyMatch)> = candidates
        .iter()
        .enumerate()
        .filter_map(|(index, candidate)| Some((index, fuzzy_match(query, *candidate)?)))
        .collect();
    let label_len = |index: usize| candidates[index].label.chars().count();
    ranked.sort_by(|(a, a_match), (b, b_match)| {
        (b_match.score.cmp(&a_match.score))
            .then_with(|| label_len(*a).cmp(&label_len(*b)))
            .then_with(|| a.cmp(b))
    });
    ranked
}

/// The byte ranges of `label` covering the characters at `positions` (char indices,
/// ascending), adjacent characters joined into one run, for drawing them highlighted.
pub fn highlight_ranges(label: &str, positions: &[usize]) -> Vec<Range<usize>> {
    let mut ranges: Vec<Range<usize>> = Vec::new();
    let mut wanted = positions.iter().peekable();
    for (index, (start, c)) in label.char_indices().enumerate() {
        if wanted.next_if_eq(&&index).is_none() {
            continue;
        }
        let end = start + c.len_utf8();
        match ranges.last_mut() {
            Some(last) if last.end == start => last.end = end,
            _ => ranges.push(start..end),
        }
    }
    ranges
}

/// One term's best match in the label or the alias; an alias match highlights nothing.
fn match_field(term: &str, candidate: Candidate<'_>) -> Option<FuzzyMatch> {
    let label = match_term(term, candidate.label);
    let alias = candidate
        .alias
        .and_then(|alias| match_term(term, alias))
        .map(|found| FuzzyMatch {
            positions: Vec::new(),
            ..found
        });
    match (label, alias) {
        (Some(label), Some(alias)) if alias.score > label.score => Some(alias),
        (Some(label), _) => Some(label),
        (None, alias) => alias,
    }
}

/// The best placement of `term`'s characters, in order, in `text`, ignoring case.
fn match_term(term: &str, text: &str) -> Option<FuzzyMatch> {
    let term: Vec<char> = term.chars().map(fold).collect();
    let chars: Vec<char> = text.chars().collect();
    let bonuses: Vec<i32> = (0..chars.len()).map(|at| start_bonus(&chars, at)).collect();
    let folded: Vec<char> = chars.into_iter().map(fold).collect();
    let (&first, rest) = term.split_first()?;
    let mut rows = vec![first_row(first, &folded, &bonuses)];
    for &wanted in rest {
        let row = next_row(rows.last()?, wanted, &folded, &bonuses);
        rows.push(row);
    }
    best_path(&rows)
}

/// For each text position: the best score with this term character matched there, and where
/// the term's previous character was matched.
type Row = Vec<Option<(i32, usize)>>;

fn first_row(wanted: char, text: &[char], bonuses: &[i32]) -> Row {
    text.iter()
        .enumerate()
        .map(|(at, &c)| {
            let prefix = if at == 0 { PREFIX } else { 0 };
            (c == wanted).then_some((MATCH + bonuses[at] + prefix, at))
        })
        .collect()
}

/// The next term character's row: after a gap (losing [`GAP`] per skipped character) or
/// right after the previous match (gaining [`CONSECUTIVE`]).
fn next_row(previous: &Row, wanted: char, text: &[char], bonuses: &[i32]) -> Row {
    let mut row = vec![None; text.len()];
    let mut gapped = Gapped::default();
    for at in 1..text.len() {
        if at >= 2 {
            gapped.offer(previous[at - 2], at - 2);
        }
        if text[at] == wanted {
            let gain = MATCH + bonuses[at];
            let adjacent = previous[at - 1].map(|(score, _)| (score + gain + CONSECUTIVE, at - 1));
            row[at] = higher(gapped.reach(at, gain), adjacent);
        }
    }
    row
}

/// The best previous match to jump from over a gap: the highest `score + GAP * at`, so a jump
/// to `to` loses [`GAP`] per character skipped whichever match it starts from.
#[derive(Default)]
struct Gapped(Option<(i32, usize)>);

impl Gapped {
    fn offer(&mut self, cell: Option<(i32, usize)>, at: usize) {
        let Some((score, _)) = cell else {
            return;
        };
        let value = score + GAP * as_score(at);
        if self.0.is_none_or(|(best, _)| value > best) {
            self.0 = Some((value, at));
        }
    }

    fn reach(&self, to: usize, gain: i32) -> Option<(i32, usize)> {
        self.0
            .map(|(value, from)| (value - GAP * as_score(to - 1) + gain, from))
    }
}

/// The higher-scoring cell, `adjacent` on a tie.
fn higher(gap: Option<(i32, usize)>, adjacent: Option<(i32, usize)>) -> Option<(i32, usize)> {
    match (gap, adjacent) {
        (Some(gap), Some(next)) if gap.0 > next.0 => Some(gap),
        (_, Some(next)) => Some(next),
        (gap, None) => gap,
    }
}

/// A character count as score points (capped far from overflow; texts are short).
fn as_score(n: usize) -> i32 {
    i32::try_from(n).unwrap_or(i32::MAX / 2)
}

/// The best score in the last row and the positions leading to it.
fn best_path(rows: &[Row]) -> Option<FuzzyMatch> {
    let last = rows.last()?;
    let (mut at, score) = last
        .iter()
        .enumerate()
        .filter_map(|(at, cell)| Some((at, cell.as_ref()?.0)))
        .fold(None, |best: Option<(usize, i32)>, (at, score)| match best {
            Some((_, top)) if top >= score => best,
            _ => Some((at, score)),
        })?;
    let mut positions = vec![at];
    for row in rows[1..].iter().rev() {
        at = row[at]?.1;
        positions.push(at);
    }
    positions.reverse();
    Some(FuzzyMatch { score, positions })
}

/// The bonus for a match at `at`: a word's start (the text's start, after a non-alphanumeric
/// character, or a capital after a lowercase letter).
fn start_bonus(chars: &[char], at: usize) -> i32 {
    let Some(before) = at.checked_sub(1).map(|back| chars[back]) else {
        return WORD_START;
    };
    let here = chars[at];
    let starts = !before.is_alphanumeric() || (before.is_lowercase() && here.is_uppercase());
    if starts { WORD_START } else { 0 }
}

/// `c` lowercased, kept one character so positions stay aligned.
fn fold(c: char) -> char {
    c.to_lowercase().next().unwrap_or(c)
}

#[cfg(test)]
mod tests;
