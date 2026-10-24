use super::*;

const TOKEN: &str = "t0k";

fn marks(chunks: &[&[u8]]) -> Vec<(usize, usize, PromptMark)> {
    let mut scanner = Osc133Scanner::new(TOKEN);
    let mut found = Vec::new();
    for (chunk_index, chunk) in chunks.iter().enumerate() {
        let mut offset = 0;
        while let Some((end, mark)) = scanner.scan(&chunk[offset..]) {
            offset += end;
            found.push((chunk_index, offset, mark));
        }
    }
    found
}

fn kinds(chunks: &[&[u8]]) -> Vec<PromptMark> {
    marks(chunks).into_iter().map(|(_, _, mark)| mark).collect()
}

#[test]
fn finds_marks_with_either_terminator() {
    let output = b"\x1b]133;A;aw=t0k\x07$ \x1b]133;B;aw=t0k\x1b\\ls\r\n\x1b]133;C;aw=t0k\x07";
    let found = marks(&[output]);
    assert_eq!(
        found,
        vec![
            (0, 15, PromptMark::PromptStart),
            (0, 33, PromptMark::CommandStart),
            (0, 52, PromptMark::CommandExecuted(None)),
        ]
    );
}

#[test]
fn marks_without_the_token_are_ignored() {
    assert!(marks(&[b"\x1b]133;A\x07"]).is_empty());
    assert!(marks(&[b"\x1b]133;B;aw=other\x07"]).is_empty());
    assert!(marks(&[b"\x1b]133;C;cmd=6c73;aw=t0kx\x07"]).is_empty());
    let mut scanner = Osc133Scanner::new("");
    assert_eq!(scanner.scan(b"\x1b]133;A;aw=\x07"), None);
}

#[test]
fn command_lines_are_decoded() {
    assert_eq!(
        kinds(&[b"\x1b]133;C;aw=t0k;cmd=6769742073C3A9\x07"]),
        [PromptMark::CommandExecuted(Some("git s\u{e9}".into()))]
    );
    // Not hex, odd length, not UTF-8: the mark counts, without its command.
    for bad in [&b"zz"[..], b"676", b"ff"] {
        let mut output = b"\x1b]133;C;aw=t0k;cmd=".to_vec();
        output.extend_from_slice(bad);
        output.push(BEL);
        assert_eq!(kinds(&[&output]), [PromptMark::CommandExecuted(None)]);
    }
    assert_eq!(
        kinds(&[b"\x1b]133;L;aw=t0k;cur=1;buf=6c73;post=1\x07"]),
        [PromptMark::CommandLine(Some(CommandLine {
            text: "ls".into(),
            cursor: 1,
            shell_suggests: true,
        }))]
    );
    assert_eq!(
        kinds(&[b"\x1b]133;L;aw=t0k\x07"]),
        [PromptMark::CommandLine(None)]
    );
}

#[test]
fn folders_are_decoded() {
    assert_eq!(
        kinds(&[b"\x1b]133;D;aw=t0k;cwd=433a5cc3a9\x07"]),
        [PromptMark::CommandFinished(Some(PathBuf::from(
            "C:\\\u{e9}"
        )))]
    );
    assert_eq!(
        kinds(&[b"\x1b]133;D;aw=t0k;cwd=zz\x07"]),
        [PromptMark::CommandFinished(None)]
    );
}

#[test]
fn text_paths_run_to_the_end_of_the_mark() {
    assert_eq!(
        kinds(&[b"\x1b]133;D;aw=t0k;path=C:\\a;b=c\x1b\\"]),
        [PromptMark::CommandFinished(Some(PathBuf::from(
            "C:\\a;b=c"
        )))]
    );
    // The token must come before the path.
    assert!(kinds(&[b"\x1b]133;D;path=C:\\;aw=t0k\x07"]).is_empty());
}

#[test]
fn mark_split_across_reads_is_found_where_it_ends() {
    let found = marks(&[b"text\x1b]13", b"3;B;aw=", b"t0k\x07rest"]);
    assert_eq!(found, vec![(2, 4, PromptMark::CommandStart)]);
    let found = marks(&[b"\x1b]133;D;aw=t0k\x1b", b"\\"]);
    assert_eq!(found, vec![(1, 1, PromptMark::CommandFinished(None))]);
}

#[test]
fn other_sequences_are_ignored() {
    assert!(marks(&[b"\x1b]0;133;A;aw=t0k\x07"]).is_empty());
    assert!(marks(&[b"\x1b]1337;A;aw=t0k\x07"]).is_empty());
    assert!(marks(&[b"\x1b]133;AB;aw=t0k\x07"]).is_empty());
    assert!(marks(&[b"\x1b]133;E;aw=t0k\x07"]).is_empty());
    assert!(marks(&[b"\x1b[133;A;aw=t0k\x07"]).is_empty());
    assert!(marks(&[b"]133;A;aw=t0k\x07"]).is_empty());
}

#[test]
fn aborted_osc_is_not_a_mark() {
    assert!(marks(&[b"\x1b]133;A;aw=t0k\x18\x07"]).is_empty());
    // ESC that is not ST aborts the OSC and starts a new sequence.
    let found = marks(&[b"\x1b]133;A\x1b]133;B;aw=t0k\x07"]);
    assert_eq!(found, vec![(0, 22, PromptMark::CommandStart)]);
}

#[test]
fn long_payloads_do_not_grow_the_buffer() {
    let mut scanner = Osc133Scanner::new(TOKEN);
    let mut long = b"\x1b]52;c;".to_vec();
    long.extend(std::iter::repeat_n(b'x', 100_000));
    assert_eq!(scanner.scan(&long), None);
    assert!(scanner.payload.len() <= MARK_PREFIX.len());
    assert_eq!(
        scanner.scan(b"\x07\x1b]133;B;aw=t0k\x07"),
        Some((16, PromptMark::CommandStart))
    );
    // A mark too long to keep still counts, without its text.
    let mut long = b"\x1b]133;C;aw=t0k;cmd=".to_vec();
    long.extend(std::iter::repeat_n(b'6', 100_000));
    long.push(BEL);
    assert_eq!(
        scanner.scan(&long),
        Some((long.len(), PromptMark::CommandExecuted(None)))
    );
    assert!(scanner.payload.len() <= MAX_PAYLOAD);
}

#[test]
fn synchronized_updates_are_followed() {
    let mut scanner = Osc133Scanner::new(TOKEN);
    assert_eq!(scanner.scan(b"\x1b[?2026h"), None);
    assert!(scanner.in_sync());
    assert!(scanner.scan(b"\x1b]133;A;aw=t0k\x07").is_some());
    assert!(scanner.in_sync());
    scanner.scan(b"\x1b[?20");
    assert!(scanner.in_sync());
    scanner.scan(b"26l");
    assert!(!scanner.in_sync());
    // Other modes and other final bytes leave it alone.
    scanner.scan(b"\x1b[?2026$p\x1b[?25h\x1b[2026h");
    assert!(!scanner.in_sync());
}

#[test]
fn tokens_are_random_hex() {
    let token = new_mark_token();
    assert_eq!(token.len(), 32);
    assert!(token.bytes().all(|byte| byte.is_ascii_hexdigit()));
    assert_ne!(token, new_mark_token());
}
