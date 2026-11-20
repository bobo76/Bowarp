//! Search in the text a file viewer shows (Ctrl+F): literal matches, with the case and
//! whole-word options, the match to show first and stepping through them with wrap-around.

use std::ops::Range;

use crate::layout::Direction;

/// The search bar's toggles. Both off: case is ignored and a match may sit inside a word.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct SearchOptions {
    pub match_case: bool,
    pub whole_word: bool,
}

/// Every match of `query` in `text`, as byte ranges in order, none overlapping. Case is
/// compared one character at a time (`İ`, which lowercases to two, matches only itself).
pub fn find_matches(text: &str, query: &str, options: SearchOptions) -> Vec<Range<usize>> {
    let mut matches = Vec::new();
    if query.is_empty() {
        return matches;
    }
    let mut next_free = 0;
    for (start, _) in text.char_indices() {
        if start < next_free {
            continue;
        }
        let Some(end) = match_end(text, start, query, options.match_case) else {
            continue;
        };
        if options.whole_word && !is_whole_word(text, start..end) {
            continue;
        }
        matches.push(start..end);
        next_free = end;
    }
    matches
}

/// Where `query` ends when it is found at `start` of `text`.
fn match_end(text: &str, start: usize, query: &str, match_case: bool) -> Option<usize> {
    let mut rest = text[start..].char_indices();
    for wanted in query.chars() {
        let (_, found) = rest.next()?;
        if !same_char(found, wanted, match_case) {
            return None;
        }
    }
    Some(rest.next().map_or(text.len(), |(offset, _)| start + offset))
}

fn same_char(a: char, b: char, match_case: bool) -> bool {
    if a == b {
        return true;
    }
    if match_case {
        return false;
    }
    if a.is_ascii() && b.is_ascii() {
        return a.eq_ignore_ascii_case(&b);
    }
    a.to_lowercase().eq(b.to_lowercase())
}

/// Whether `range` of `text` has no word character right before or right after it.
fn is_whole_word(text: &str, range: Range<usize>) -> bool {
    let before = text[..range.start].chars().next_back();
    let after = text[range.end..].chars().next();
    !before.is_some_and(is_word_char) && !after.is_some_and(is_word_char)
}

fn is_word_char(c: char) -> bool {
    c.is_alphanumeric() || c == '_'
}

/// The first match starting at or after `offset`, else the first one (the search wraps).
pub fn first_match_from(matches: &[Range<usize>], offset: usize) -> Option<usize> {
    if matches.is_empty() {
        return None;
    }
    Some(matches.partition_point(|found| found.start < offset) % matches.len())
}

/// The match starting nearest to `offset` (the earlier one on a tie): where the current match
/// was before the text changed.
pub fn nearest_match(matches: &[Range<usize>], offset: usize) -> Option<usize> {
    (0..matches.len()).min_by_key(|&index| matches[index].start.abs_diff(offset))
}

/// The match after (`Down`) or before (`Up`) `current`, wrapping around; with none current,
/// the first or the last.
pub fn step_match(current: Option<usize>, count: usize, direction: Direction) -> Option<usize> {
    if count == 0 {
        return None;
    }
    Some(match (current, direction) {
        (None, Direction::Down) => 0,
        (None, Direction::Up) => count - 1,
        (Some(index), Direction::Down) => (index + 1) % count,
        (Some(index), Direction::Up) => (index + count - 1) % count,
    })
}

/// The count beside the field: blank for an empty query, "0 matches", else "N of M".
pub fn match_count_label(query: &str, current: Option<usize>, total: usize) -> String {
    match (query.is_empty(), current) {
        (true, _) => String::new(),
        (false, Some(index)) => format!("{} of {total}", index + 1),
        (false, None) if total == 0 => "0 matches".to_owned(),
        (false, None) => format!("{total} matches"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const ANY_CASE: SearchOptions = SearchOptions {
        match_case: false,
        whole_word: false,
    };

    fn found<'a>(text: &'a str, query: &str, options: SearchOptions) -> Vec<&'a str> {
        let matches = find_matches(text, query, options);
        matches.into_iter().map(|range| &text[range]).collect()
    }

    #[test]
    fn matches_are_literal_and_ignore_case_by_default() {
        assert_eq!(found("Foo foo FOO", "foo", ANY_CASE), ["Foo", "foo", "FOO"]);
        assert_eq!(found("a.b axb", "a.b", ANY_CASE), ["a.b"]);
        assert_eq!(found("Ärger ärger", "ÄRGER", ANY_CASE), ["Ärger", "ärger"]);
        assert!(found("text", "", ANY_CASE).is_empty());
        assert!(found("", "x", ANY_CASE).is_empty());
    }

    #[test]
    fn match_case_compares_exactly() {
        let options = SearchOptions {
            match_case: true,
            ..ANY_CASE
        };
        assert_eq!(found("Foo foo FOO", "foo", options), ["foo"]);
    }

    #[test]
    fn matches_do_not_overlap_and_are_byte_ranges() {
        assert_eq!(find_matches("aaaa", "aa", ANY_CASE), [0..2, 2..4]);
        assert_eq!(find_matches("é é", "é", ANY_CASE), [0..2, 3..5]);
        assert_eq!(find_matches("ab", "abc", ANY_CASE), []);
    }

    #[test]
    fn whole_word_needs_no_word_character_around_the_match() {
        let options = SearchOptions {
            whole_word: true,
            ..ANY_CASE
        };
        assert_eq!(
            find_matches("cat concat cat_x (cat) cats cat", "cat", options),
            [0..3, 18..21, 28..31]
        );
        // A rejected match inside a word doesn't hide a later one that overlaps it.
        assert_eq!(found("aaa aa", "aa", options), ["aa"]);
        assert_eq!(found("été", "t", options), Vec::<&str>::new());
    }

    #[test]
    fn the_first_match_is_at_or_after_the_offset_and_wraps() {
        let matches = [2..4, 10..12, 20..22];
        assert_eq!(first_match_from(&matches, 0), Some(0));
        assert_eq!(first_match_from(&matches, 10), Some(1));
        assert_eq!(first_match_from(&matches, 11), Some(2));
        assert_eq!(first_match_from(&matches, 21), Some(0));
        assert_eq!(first_match_from(&[], 0), None);
    }

    #[test]
    fn the_nearest_match_follows_a_changed_text() {
        let matches = [2..4, 10..12, 20..22];
        assert_eq!(nearest_match(&matches, 11), Some(1));
        assert_eq!(nearest_match(&matches, 15), Some(1));
        assert_eq!(nearest_match(&matches, 16), Some(2));
        assert_eq!(nearest_match(&matches, 100), Some(2));
        assert_eq!(nearest_match(&[], 3), None);
    }

    #[test]
    fn stepping_wraps_silently() {
        assert_eq!(step_match(Some(2), 3, Direction::Down), Some(0));
        assert_eq!(step_match(Some(0), 3, Direction::Up), Some(2));
        assert_eq!(step_match(Some(1), 3, Direction::Down), Some(2));
        assert_eq!(step_match(None, 3, Direction::Down), Some(0));
        assert_eq!(step_match(None, 3, Direction::Up), Some(2));
        assert_eq!(step_match(None, 0, Direction::Down), None);
    }

    #[test]
    fn the_count_is_blank_for_an_empty_query() {
        assert_eq!(match_count_label("", None, 0), "");
        assert_eq!(match_count_label("x", None, 0), "0 matches");
        assert_eq!(match_count_label("x", Some(1), 7), "2 of 7");
    }
}
