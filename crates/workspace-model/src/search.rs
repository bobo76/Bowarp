//! Sidebar search: which tabs match what the user typed.

use std::path::Path;

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
            .map(|location| display_path(&location.dir, home));
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

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use super::*;
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
            dir: home.join("dev").join("api"),
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
}
