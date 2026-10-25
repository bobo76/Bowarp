use super::*;

fn chars(text: &str) -> Vec<char> {
    text.chars().collect()
}

/// The link at the first char of `needle` in `line`, as text.
fn link_at(line: &str, needle: &str) -> Option<(String, LinkText)> {
    let line = chars(line);
    let text: String = line.iter().collect();
    let byte = text.find(needle)?;
    let index = text[..byte].chars().count();
    let found = find_link(&line, index)?;
    Some((line[found.range].iter().collect(), found.target))
}

fn url(text: &str) -> LinkText {
    LinkText::Url(text.to_string())
}

fn path(path: &str, line: Option<u32>, column: Option<u32>) -> LinkText {
    LinkText::Path(PathRef {
        path: path.to_string(),
        line,
        column,
    })
}

#[test]
fn urls_are_found_without_trailing_punctuation() {
    let line = "See https://example.com/a?b=1#c. Done";
    assert_eq!(
        link_at(line, "example"),
        Some((
            "https://example.com/a?b=1#c".into(),
            url("https://example.com/a?b=1#c")
        ))
    );
    assert_eq!(link_at(line, "See"), None);
    assert_eq!(link_at(line, "Done"), None);
}

#[test]
fn brackets_around_a_url_are_left_out_unless_balanced() {
    let found = link_at("(see https://x.org/wiki/A_(b))", "x.org");
    assert_eq!(
        found,
        Some((
            "https://x.org/wiki/A_(b)".into(),
            url("https://x.org/wiki/A_(b)")
        ))
    );
    let found = link_at("[link](https://x.org/p)", "x.org");
    assert_eq!(
        found.map(|(text, _)| text),
        Some("https://x.org/p".to_string())
    );
    // Before the URL in the same word: not part of it.
    assert_eq!(link_at("[link](https://x.org/p)", "link"), None);
}

#[test]
fn quotes_and_box_drawing_end_a_link() {
    let found = link_at("│ \"http://localhost:3000/\" │", "localhost");
    assert_eq!(
        found,
        Some((
            "http://localhost:3000/".into(),
            url("http://localhost:3000/")
        ))
    );
}

#[test]
fn a_scheme_alone_or_inside_a_word_is_no_url() {
    assert_eq!(link_at("https:// alone", "https"), None);
    assert_eq!(link_at("xhttps://a.b", "a.b"), None);
    assert_eq!(link_at("ssh://host/x", "host"), None);
}

#[test]
fn paths_with_line_and_column() {
    assert_eq!(
        link_at("error at src/foo.rs:42:7: oops", "foo"),
        Some((
            "src/foo.rs:42:7".into(),
            path("src/foo.rs", Some(42), Some(7))
        ))
    );
    assert_eq!(
        link_at("Edited (src/foo.rs:42)", "src"),
        Some(("src/foo.rs:42".into(), path("src/foo.rs", Some(42), None)))
    );
    assert_eq!(
        link_at("Read README.md.", "README"),
        Some(("README.md".into(), path("README.md", None, None)))
    );
    assert_eq!(
        link_at("open ~/notes", "notes"),
        Some(("~/notes".into(), path("~/notes", None, None)))
    );
    assert_eq!(
        link_at(r"at C:\src\main.rs:3", "main"),
        Some((
            r"C:\src\main.rs:3".into(),
            path(r"C:\src\main.rs", Some(3), None)
        ))
    );
}

#[test]
fn plain_words_are_no_paths() {
    assert_eq!(link_at("hello world", "world"), None);
    assert_eq!(link_at("Makefile:12", "Makefile"), None);
    assert_eq!(link_at("a , b", ","), None);
}

#[test]
fn non_ascii_text_counts_chars() {
    let line = "→ é https://ä.example/ü ok";
    let found = link_at(line, "ä").unwrap();
    assert_eq!(found.0, "https://ä.example/ü");
    let index = chars(line).iter().position(|&c| c == 'h').unwrap();
    assert_eq!(find_link(&chars(line), index).unwrap().range, 4..23);
}

#[test]
fn path_refs_take_up_to_two_numbers() {
    assert_eq!(
        parse_path_ref("a.rs:1:2:3"),
        PathRef {
            path: "a.rs:1".into(),
            line: Some(2),
            column: Some(3)
        }
    );
    assert_eq!(
        parse_path_ref("a.rs:x"),
        PathRef {
            path: "a.rs:x".into(),
            line: None,
            column: None
        }
    );
    assert_eq!(parse_path_ref("a.rs:0").line, None);
}

#[test]
fn paths_resolve_from_the_folder_or_home() {
    let folder = Path::new("/work");
    let home = Path::new("/home/me");
    assert_eq!(
        resolve_path("src/a.rs", Some(folder), Some(home)),
        Some(PathBuf::from("/work/src/a.rs"))
    );
    assert_eq!(
        resolve_path("~/a", Some(folder), Some(home)),
        Some(PathBuf::from("/home/me/a"))
    );
    assert_eq!(resolve_path("src/a.rs", None, Some(home)), None);
    #[cfg(unix)]
    assert_eq!(
        resolve_path("/etc/hosts", None, None),
        Some(PathBuf::from("/etc/hosts"))
    );
}

#[test]
fn local_links_in_documents() {
    let folder = Path::new("/repo/docs");
    assert_eq!(
        local_link("FEATURES.md", folder),
        Some(PathBuf::from("/repo/docs/FEATURES.md"))
    );
    assert_eq!(
        local_link("../README.md#design-decisions", folder),
        Some(PathBuf::from("/repo/docs/../README.md"))
    );
    assert_eq!(
        local_link("my%20notes.md?x=1", folder),
        Some(PathBuf::from("/repo/docs/my notes.md"))
    );
    assert_eq!(
        local_link("file:///tmp/a.md", folder),
        Some(PathBuf::from("/tmp/a.md"))
    );
    assert_eq!(local_link("https://example.com/a.md", folder), None);
    assert_eq!(local_link("mailto:me@example.com", folder), None);
    assert_eq!(local_link("#section", folder), None);
    assert_eq!(local_link("", folder), None);
}

#[test]
fn hyperlink_schemes() {
    assert!(is_openable_hyperlink("https://example.com"));
    assert!(is_openable_hyperlink("HTTP://example.com"));
    assert!(is_openable_hyperlink("ftp://example.com/a"));
    assert!(is_openable_hyperlink("mailto:me@example.com"));
    assert!(is_openable_hyperlink("file:///tmp/a.rs"));
    assert!(!is_openable_hyperlink("smb://host/share"));
    assert!(!is_openable_hyperlink("vscode://file/x"));
    assert!(!is_openable_hyperlink("ms-officecmd:x"));
    assert!(!is_openable_hyperlink("javascript:alert(1)"));
    assert!(!is_openable_hyperlink("/tmp/a.rs"));
    assert!(!is_openable_hyperlink(""));
}

#[test]
fn link_rows_split_at_wraps() {
    let span = |row, column, cells| RowSpan { row, column, cells };
    // Within one row.
    assert_eq!(link_rows(2..5, 10), vec![span(0, 2, 3)]);
    assert_eq!(link_rows(12..15, 10), vec![span(1, 2, 3)]);
    // Ending at the row end.
    assert_eq!(link_rows(6..10, 10), vec![span(0, 6, 4)]);
    // Crossing one wrap.
    assert_eq!(link_rows(8..13, 10), vec![span(0, 8, 2), span(1, 0, 3)]);
    // Crossing two wraps.
    assert_eq!(
        link_rows(5..25, 10),
        vec![span(0, 5, 5), span(1, 0, 10), span(2, 0, 5)]
    );
    // A whole row.
    assert_eq!(link_rows(10..20, 10), vec![span(1, 0, 10)]);
    assert_eq!(link_rows(3..3, 10), vec![]);
    assert_eq!(link_rows(0..3, 0), vec![]);
}
