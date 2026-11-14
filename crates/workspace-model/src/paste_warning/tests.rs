use super::*;

#[test]
fn text_without_a_line_break_needs_no_question() {
    assert_eq!(paste_warning(""), None);
    assert_eq!(paste_warning("echo hi"), None);
}

#[test]
fn a_trailing_line_break_counts_as_one_line() {
    let warning = paste_warning("ls\n").unwrap();
    assert_eq!(warning.title, "Paste 1 line?");
    assert_eq!(warning.description, "It will run as a command.");
    assert_eq!(warning.preview, ["ls"]);
    assert_eq!(warning.more, None);
}

#[test]
fn lines_are_counted_without_the_trailing_break() {
    let warning = paste_warning("a\nb\n").unwrap();
    assert_eq!(warning.title, "Paste 2 lines?");
    assert_eq!(warning.description, "They will run as commands.");
    assert_eq!(warning.preview, ["a", "b"]);
    assert_eq!(paste_warning("a\nb").unwrap().title, "Paste 2 lines?");
}

#[test]
fn crlf_and_lone_cr_each_end_one_line() {
    assert_eq!(paste_warning("a\r\nb\r\n").unwrap().preview, ["a", "b"]);
    assert_eq!(paste_warning("a\rb").unwrap().preview, ["a", "b"]);
    assert_eq!(paste_warning("a\n\nb").unwrap().preview, ["a", "", "b"]);
}

#[test]
fn a_lone_line_break_is_one_empty_line() {
    let warning = paste_warning("\n").unwrap();
    assert_eq!(warning.title, "Paste 1 line?");
    assert_eq!(warning.preview, [""]);
}

#[test]
fn lines_past_the_preview_are_summed_up() {
    let text: String = (1..=17).map(|n| format!("line {n}\n")).collect();
    let warning = paste_warning(&text).unwrap();
    assert_eq!(warning.title, "Paste 17 lines?");
    assert_eq!(warning.preview.len(), PASTE_PREVIEW_LINES);
    assert_eq!(warning.preview[0], "line 1");
    assert_eq!(warning.more.as_deref(), Some("… and 12 more lines"));
}

#[test]
fn one_line_past_the_preview_is_singular() {
    let text = "1\n2\n3\n4\n5\n6";
    assert_eq!(
        paste_warning(text).unwrap().more.as_deref(),
        Some("… and 1 more line")
    );
}

#[test]
fn long_lines_are_cut() {
    assert_eq!(preview_line("abcdef", 4), "abc…");
    assert_eq!(preview_line("abcd", 4), "abcd");
    let line = "x".repeat(200);
    let warning = paste_warning(&format!("{line}\n")).unwrap();
    assert_eq!(warning.preview[0].chars().count(), PASTE_PREVIEW_CHARS);
}

#[test]
fn control_characters_show_as_text() {
    assert_eq!(preview_line("\x1b[31mred\x07", 80), "^[[31mred^G");
    assert_eq!(preview_line("a\tb\x7f", 80), "a b^?");
    assert_eq!(preview_line("a\u{9b}b", 80), "a\u{fffd}b");
    assert_eq!(preview_line("ab\u{202e}cd", 80), "abcd");
}
