//! Sidebar search: which tabs match what the user typed.

use std::path::Path;

use crate::layout::Direction;
use crate::location::{Location, display_path};

/// Whitespace-separated terms, each matched case-insensitively anywhere in a tab's fields
/// (name, folder, branch, group). A tab matches when every term is found in at least one field,
/// so `api main` finds the tab in an `api` folder on `main`.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct TabQuery {
    terms: Vec<String>,
}

impl TabQuery {
    pub fn parse(input: &str) -> Self {
        Self {
            terms: input.split_whitespace().map(str::to_lowercase).collect(),
        }
    }

    /// An empty query shows every tab.
    pub fn is_empty(&self) -> bool {
        self.terms.is_empty()
    }

    pub fn matches<'a>(&self, fields: impl IntoIterator<Item = &'a str>) -> bool {
        let fields: Vec<String> = fields.into_iter().map(str::to_lowercase).collect();
        self.terms
            .iter()
            .all(|term| fields.iter().any(|field| field.contains(term.as_str())))
    }
}

/// What a sidebar search looks at for one tab.
#[derive(Clone, Copy, Debug)]
pub struct SearchedTab<'a> {
    /// The name shown in the sidebar: the custom name, else the program's title.
    pub name: &'a str,
    /// The program's own title.
    pub title: &'a str,
    pub location: Option<&'a Location>,
    /// The name of the group holding the tab.
    pub group: Option<&'a str>,
}

impl TabQuery {
    /// Whether `tab` shows while searching: the query is looked up in its name, program title,
    /// folder (as shown, with `~`, so the home folder's path does not match every tab), branch
    /// and group name.
    pub fn matches_tab(&self, tab: &SearchedTab, home: Option<&Path>) -> bool {
        let folder = tab
            .location
            .map(|location| display_path(&location.folder, home));
        let branch = tab
            .location
            .and_then(|location| location.head.as_ref().map(|head| head.name()));
        let fields = [
            Some(tab.name),
            Some(tab.title),
            folder.as_deref(),
            branch,
            tab.group,
        ];
        self.matches(fields.into_iter().flatten())
    }
}

/// Up/Down in the search field: the match to move to from `cursor` (the match moved to last,
/// `None` before the first move), given which tabs match in sidebar order. Down goes to the
/// next match and Up to the previous one, wrapping around; the first move goes to the first
/// match (Down) or the last one (Up). `None` when nothing matches.
pub fn step_match(matching: &[bool], cursor: Option<usize>, direction: Direction) -> Option<usize> {
    let count = matching.len();
    let order: Vec<usize> = match (cursor.filter(|&cursor| cursor < count), direction) {
        (Some(cursor), Direction::Down) => (1..=count)
            .map(|offset| (cursor + offset) % count)
            .collect(),
        (Some(cursor), Direction::Up) => (1..=count)
            .map(|offset| (cursor + count - offset) % count)
            .collect(),
        (None, Direction::Down) => (0..count).collect(),
        (None, Direction::Up) => (0..count).rev().collect(),
    };
    order.into_iter().find(|&index| matching[index])
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use super::*;
    #[cfg(unix)]
    use crate::GitHead;

    #[test]
    fn empty_query_matches_everything() {
        let query = TabQuery::parse("   ");
        assert!(query.is_empty());
        assert!(query.matches(["anything"]));
        assert!(query.matches([]));
    }

    #[test]
    fn terms_match_case_insensitively_in_any_field() {
        let fields = ["PLAN.md remaining work", "~/dev/agent-workspace", "main"];
        assert!(TabQuery::parse("plan").matches(fields));
        assert!(TabQuery::parse("AGENT-work").matches(fields));
        assert!(TabQuery::parse("main").matches(fields));
        assert!(!TabQuery::parse("stock").matches(fields));
    }

    #[test]
    fn every_term_must_match_somewhere() {
        let fields = ["bash", "~/dev/api", "feature/login"];
        assert!(TabQuery::parse("api login").matches(fields));
        assert!(!TabQuery::parse("api logout").matches(fields));
    }

    #[cfg(unix)]
    #[test]
    fn looks_in_every_field_of_a_tab() {
        let home = Path::new("/home/user");
        let location = Location {
            folder: home.join("dev").join("api"),
            head: Some(GitHead::Branch("feature/login".into())),
        };
        let tab = SearchedTab {
            name: "backend",
            title: "vim server.rs",
            location: Some(&location),
            group: Some("Reviews"),
        };
        let matches = |input: &str| TabQuery::parse(input).matches_tab(&tab, Some(home));

        assert!(matches("backend"));
        assert!(matches("server.rs"));
        assert!(matches("~"));
        assert!(matches("dev"));
        assert!(matches("login"));
        assert!(matches("reviews api"));
        // The folder is matched as shown, so the home folder's path matches no tab.
        assert!(!matches("user"));
        assert!(!matches("frontend"));
    }

    #[test]
    fn a_tab_without_location_or_group_matches_on_its_names() {
        let tab = SearchedTab {
            name: "notes",
            title: "bash",
            location: None,
            group: None,
        };
        let home = PathBuf::from("/home/user");
        assert!(TabQuery::parse("bash").matches_tab(&tab, Some(&home)));
        assert!(!TabQuery::parse("main").matches_tab(&tab, Some(&home)));
    }

    #[test]
    fn up_and_down_step_through_the_matches() {
        let matching = [false, true, false, true, true];
        assert_eq!(step_match(&matching, None, Direction::Down), Some(1));
        assert_eq!(step_match(&matching, None, Direction::Up), Some(4));
        assert_eq!(step_match(&matching, Some(1), Direction::Down), Some(3));
        assert_eq!(step_match(&matching, Some(3), Direction::Up), Some(1));
        // Wrapping around both ends.
        assert_eq!(step_match(&matching, Some(4), Direction::Down), Some(1));
        assert_eq!(step_match(&matching, Some(1), Direction::Up), Some(4));
        // The only match stays put; no match, no move.
        assert_eq!(
            step_match(&[false, true], Some(1), Direction::Down),
            Some(1)
        );
        assert_eq!(step_match(&[false, false], None, Direction::Down), None);
        assert_eq!(step_match(&[], Some(0), Direction::Up), None);
        // A cursor past the end (a tab closed meanwhile) starts over.
        assert_eq!(step_match(&matching, Some(9), Direction::Down), Some(1));
    }
}
