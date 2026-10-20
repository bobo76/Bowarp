//! Links in terminal output and in the file viewer's Markdown: a URL or a file path (with an
//! optional `:line:col`) under the pointer in a line of text, and where a link written in a
//! file points. Whether a path exists is left to the caller; nothing here touches the file
//! system.

use std::ops::Range;
use std::path::{Path, PathBuf};

/// URL schemes recognized in plain text.
const SCHEMES: &[&str] = &["https", "http", "ftp", "file"];

/// URL schemes a terminal hyperlink (OSC 8) may open: its text need not show where it goes,
/// so a link to any other registered handler (`smb:`, `vscode:`) is not a link.
const HYPERLINK_SCHEMES: &[&str] = &["https", "http", "ftp", "mailto", "file"];

/// A link found in a line of text.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct FoundLink {
    /// Where the link is in the line, in chars.
    pub range: Range<usize>,
    pub target: LinkText,
}

/// What a link in text says.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum LinkText {
    Url(String),
    /// A file path, to be checked: most words that look like one are not.
    Path(PathRef),
}

/// A file path with the place in the file it names, as compilers and agents print it
/// (`src/main.rs:42:7`).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PathRef {
    pub path: String,
    /// 1-based.
    pub line: Option<u32>,
    /// 1-based.
    pub column: Option<u32>,
}

/// The link at char `index` of `line`, if any. A URL starts with one of [`SCHEMES`]; a path
/// holds a `/`, `\`, `.` or a leading `~`. Punctuation ending a sentence and unbalanced
/// brackets around the link are left out.
pub fn find_link(line: &[char], index: usize) -> Option<FoundLink> {
    if !line.get(index).is_some_and(|&c| is_link_char(c)) {
        return None;
    }
    let start = line[..index]
        .iter()
        .rposition(|&c| !is_link_char(c))
        .map_or(0, |at| at + 1);
    let end = line[index..]
        .iter()
        .position(|&c| !is_link_char(c))
        .map_or(line.len(), |at| index + at);
    let word = &line[start..end];

    if let Some(url_start) = url_start(word) {
        let url = trim_end(&word[url_start..]);
        let range = start + url_start..start + url_start + url.len();
        return range.contains(&index).then(|| FoundLink {
            range,
            target: LinkText::Url(url.iter().collect()),
        });
    }

    // A URL with an unknown scheme, or nothing after the scheme.
    if word.windows(3).any(|three| three == [':', '/', '/']) {
        return None;
    }
    let (lead, path) = trim_start(word);
    let path = trim_end(path);
    let range = start + lead..start + lead + path.len();
    if !range.contains(&index) {
        return None;
    }
    let text: String = path.iter().collect();
    let path_ref = parse_path_ref(&text);
    let looks_like_path = path_ref.path.starts_with('~')
        || path_ref.path.chars().any(|c| matches!(c, '/' | '\\' | '.'));
    (looks_like_path && !path_ref.path.is_empty()).then_some(FoundLink {
        range,
        target: LinkText::Path(path_ref),
    })
}

/// Chars a URL or a path may hold. Quotes, backticks, angle brackets, pipes and box drawing
/// end one.
fn is_link_char(c: char) -> bool {
    c.is_alphanumeric()
        || matches!(
            c,
            '-' | '.'
                | '_'
                | '~'
                | ':'
                | '/'
                | '?'
                | '#'
                | '['
                | ']'
                | '@'
                | '!'
                | '$'
                | '&'
                | '*'
                | '+'
                | ','
                | ';'
                | '='
                | '%'
                | '('
                | ')'
                | '\\'
        )
}

/// Where a URL starts in `word`: the first known scheme followed by `://`.
fn url_start(word: &[char]) -> Option<usize> {
    let text: String = word.iter().collect();
    let lower = text.to_ascii_lowercase();
    let mut earliest: Option<usize> = None;
    for scheme in SCHEMES {
        let pattern = format!("{scheme}://");
        let mut from = 0;
        while let Some(found) = lower[from..].find(&pattern) {
            let at = from + found;
            // `xhttps://` is not a URL start; `(https://` is.
            let preceded_by_letter = lower[..at]
                .chars()
                .next_back()
                .is_some_and(|c| c.is_ascii_alphanumeric());
            if !preceded_by_letter {
                earliest = Some(earliest.map_or(at, |earliest| earliest.min(at)));
                break;
            }
            from = at + 1;
        }
    }
    // A byte offset in ASCII-lowered text; count chars up to it.
    let at = earliest?;
    let chars = text[..at].chars().count();
    // Nothing after the scheme: not a link.
    (word.len() > chars + 3 + scheme_len(&lower[at..])).then_some(chars)
}

fn scheme_len(text: &str) -> usize {
    text.find("://").unwrap_or(0)
}

/// Drop sentence punctuation and closing brackets without an opening one from the end.
fn trim_end(mut text: &[char]) -> &[char] {
    loop {
        let Some(&last) = text.last() else {
            return text;
        };
        let drop = match last {
            '.' | ',' | ';' | ':' | '!' | '?' => true,
            ')' => count(text, '(') < count(text, ')'),
            ']' => count(text, '[') < count(text, ']'),
            _ => false,
        };
        if !drop {
            return text;
        }
        text = &text[..text.len() - 1];
    }
}

/// Drop opening brackets without a closing one from the start; returns how many chars went.
fn trim_start(text: &[char]) -> (usize, &[char]) {
    let mut lead = 0;
    let mut end = text.len();
    // `(src/a.rs:3)`: the brackets around the whole word.
    while end - lead >= 2 && matches!((text[lead], text[end - 1]), ('(', ')') | ('[', ']')) {
        lead += 1;
        end -= 1;
    }
    let text = &text[..end];
    while let Some(&first) = text.get(lead) {
        let rest = &text[lead..];
        let drop = match first {
            '(' => count(rest, '(') > count(rest, ')'),
            '[' => count(rest, '[') > count(rest, ']'),
            ':' | ',' | ';' => true,
            _ => false,
        };
        if !drop {
            break;
        }
        lead += 1;
    }
    (lead, &text[lead..])
}

fn count(text: &[char], c: char) -> usize {
    text.iter().filter(|&&other| other == c).count()
}

/// Split `src/main.rs:42:7` into the path, line and column. A part that isn't a number stays
/// in the path.
pub fn parse_path_ref(text: &str) -> PathRef {
    let number = |part: &str| -> Option<u32> {
        (!part.is_empty() && part.bytes().all(|b| b.is_ascii_digit()))
            .then(|| part.parse().ok())
            .flatten()
    };
    let mut path = text;
    let mut numbers = Vec::new();
    while numbers.len() < 2 {
        let Some((rest, last)) = path.rsplit_once(':') else {
            break;
        };
        let Some(value) = number(last) else {
            break;
        };
        numbers.push(value);
        path = rest;
    }
    let (line, column) = match numbers[..] {
        [column, line] => (Some(line), Some(column)),
        [line] => (Some(line), None),
        _ => (None, None),
    };
    PathRef {
        path: path.to_string(),
        line: line.filter(|&line| line > 0),
        column: column.filter(|&column| column > 0),
    }
}

/// Where a path written in terminal output points: `~/` from `home`, a relative path from
/// `folder` (the tab's). `None` when that folder (or home) is not known.
pub fn resolve_path(path: &str, folder: Option<&Path>, home: Option<&Path>) -> Option<PathBuf> {
    if path == "~" {
        return home.map(Path::to_path_buf);
    }
    if let Some(rest) = path.strip_prefix("~/") {
        return home.map(|home| home.join(rest));
    }
    let path = Path::new(path);
    if path.is_absolute() {
        return Some(path.to_path_buf());
    }
    folder.map(|folder| folder.join(path))
}

/// Whether a terminal hyperlink to `uri` may be opened: its scheme is one of
/// [`HYPERLINK_SCHEMES`], in any case.
pub fn is_openable_hyperlink(uri: &str) -> bool {
    let Some((scheme, _)) = uri.trim_start().split_once(':') else {
        return false;
    };
    HYPERLINK_SCHEMES
        .iter()
        .any(|allowed| allowed.eq_ignore_ascii_case(scheme))
}

/// The cells of a link on one row of a grid.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct RowSpan {
    /// Zero-based row, counted from the row holding char 0.
    pub row: usize,
    pub column: usize,
    pub cells: usize,
}

/// The rows a link takes when `range` (chars of rows joined, one char per cell) is shown in
/// rows of `columns` cells: one span per row it touches.
pub fn link_rows(range: Range<usize>, columns: usize) -> Vec<RowSpan> {
    if columns == 0 || range.is_empty() {
        return Vec::new();
    }
    (range.start / columns..=(range.end - 1) / columns)
        .map(|row| {
            let row_start = row * columns;
            let start = range.start.max(row_start);
            let end = range.end.min(row_start + columns);
            RowSpan {
                row,
                column: start - row_start,
                cells: end - start,
            }
        })
        .collect()
}

/// The local file a link written in a document points to, resolved from the document's
/// `folder`: a relative or absolute path, or a `file://` URL, without its `#fragment` or
/// `?query`. `None` for links to elsewhere (`https:`, `mailto:`) and in-page anchors.
pub fn local_link(url: &str, folder: &Path) -> Option<PathBuf> {
    let url = url.trim();
    let without_fragment = url.split('#').next().unwrap_or_default();
    if let Some(rest) = strip_prefix_ignore_case(without_fragment, "file://") {
        // `file:///home/x` and `file://localhost/home/x`.
        let path = rest.strip_prefix("localhost").unwrap_or(rest);
        let path = percent_decode(path.split('?').next().unwrap_or_default())?;
        return (!path.is_empty()).then(|| file_url_path(&path));
    }
    if has_scheme(without_fragment) {
        return None;
    }
    let path = without_fragment.split('?').next().unwrap_or_default();
    if path.is_empty() {
        return None;
    }
    let path = percent_decode(path)?;
    Some(folder.join(path))
}

/// The path of a `file://` URL's path part: `/C:/x` is `C:/x` on Windows.
fn file_url_path(path: &str) -> PathBuf {
    let bytes = path.as_bytes();
    let drive = bytes.len() >= 3 && bytes[0] == b'/' && bytes[1].is_ascii_alphabetic();
    if cfg!(windows) && drive && bytes[2] == b':' {
        return PathBuf::from(&path[1..]);
    }
    PathBuf::from(path)
}

/// Whether `url` starts with a scheme (`https:`, `mailto:`): two or more letters, so a
/// Windows drive (`C:`) is a path.
fn has_scheme(url: &str) -> bool {
    let Some((scheme, _)) = url.split_once(':') else {
        return false;
    };
    scheme.len() >= 2
        && scheme.starts_with(|c: char| c.is_ascii_alphabetic())
        && scheme
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, '+' | '-' | '.'))
}

fn strip_prefix_ignore_case<'a>(text: &'a str, prefix: &str) -> Option<&'a str> {
    let head = text.get(..prefix.len())?;
    head.eq_ignore_ascii_case(prefix)
        .then(|| &text[prefix.len()..])
}

/// `%20` and the like decoded; `None` when the result is not UTF-8.
fn percent_decode(text: &str) -> Option<String> {
    let bytes = text.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        let hex = |b: u8| (b as char).to_digit(16);
        if bytes[i] == b'%'
            && let (Some(high), Some(low)) = (
                bytes.get(i + 1).copied().and_then(hex),
                bytes.get(i + 2).copied().and_then(hex),
            )
        {
            out.push((high * 16 + low) as u8);
            i += 3;
            continue;
        }
        out.push(bytes[i]);
        i += 1;
    }
    String::from_utf8(out).ok()
}

#[cfg(test)]
mod tests {
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
}
