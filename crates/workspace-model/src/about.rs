//! The About dialog's version line, which users copy into bug reports.

/// `name version (commit)`, e.g. "agent-workspace 0.1.0 (abc1234)"; without the commit when the
/// build had none (built from a source tarball, or git missing).
pub fn version_line(name: &str, version: &str, commit: Option<&str>) -> String {
    match commit.map(str::trim).filter(|commit| !commit.is_empty()) {
        Some(commit) => format!("{name} {version} ({commit})"),
        None => format!("{name} {version}"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn version_line_names_the_commit_when_there_is_one() {
        assert_eq!(
            version_line("agent-workspace", "0.1.0", Some("abc1234")),
            "agent-workspace 0.1.0 (abc1234)"
        );
        assert_eq!(
            version_line("agent-workspace", "0.1.0", None),
            "agent-workspace 0.1.0"
        );
        assert_eq!(
            version_line("agent-workspace", "0.1.0", Some(" ")),
            "agent-workspace 0.1.0"
        );
    }
}
