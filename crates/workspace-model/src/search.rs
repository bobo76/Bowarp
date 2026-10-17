//! Sidebar search: which tabs match what the user typed.

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

#[cfg(test)]
mod tests {
    use super::*;

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
}
