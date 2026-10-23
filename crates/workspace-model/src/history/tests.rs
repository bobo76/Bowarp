use super::*;
use crate::git::tests::TempDir;

fn entry(command: &str, time: u64) -> HistoryEntry {
    HistoryEntry::new(command, Some("/tmp".into()), time)
}

fn commands(history: &CommandHistory) -> Vec<&str> {
    history
        .entries()
        .iter()
        .map(|entry| entry.command.as_str())
        .collect()
}

#[test]
fn latest_use_wins() {
    let mut history = CommandHistory::new(10);
    history.add(entry("ls", 1));
    history.add(entry("cd src", 2));
    history.add(HistoryEntry::new("ls", Some("/home".into()), 3));
    assert_eq!(commands(&history), ["cd src", "ls"]);
    assert_eq!(history.entries()[1].dir, Some("/home".into()));
    assert_eq!(history.entries()[1].time, 3);
}

#[test]
fn oldest_entries_beyond_the_cap_are_dropped() {
    let mut history = CommandHistory::new(3);
    for (time, command) in ["a", "b", "c", "d", "e"].into_iter().enumerate() {
        history.add(entry(command, time as u64));
    }
    assert_eq!(commands(&history), ["c", "d", "e"]);
}

#[test]
fn unrecordable_commands_are_refused() {
    let mut history = CommandHistory::new(10);
    for command in ["", "   ", " secret", "a\nb", "echo \x1b[31m", "tab\there"] {
        assert!(!history.add(entry(command, 0)), "{command:?}");
    }
    assert!(history.add(entry("echo ok ", 0)));
    assert!(!history.is_empty() && history.len() == 1);
}

#[test]
fn suggestion_is_the_rest_of_the_newest_prefix_match() {
    let mut history = CommandHistory::new(10);
    history.add(entry("clear", 1));
    history.add(entry("cargo test", 2));
    history.add(entry("cargo build", 3));
    assert_eq!(history.suggestion("c"), Some("argo build"));
    assert_eq!(history.suggestion("cl"), Some("ear"));
    assert_eq!(history.suggestion("cargo t"), Some("est"));
    assert_eq!(history.suggestion("clear"), None);
    assert_eq!(history.suggestion("Cl"), None);
    assert_eq!(history.suggestion(""), None);
    assert_eq!(history.suggestion("x"), None);
}

#[test]
fn matching_is_a_case_insensitive_substring_oldest_first() {
    let mut history = CommandHistory::new(10);
    history.add(entry("cd vi", 1));
    history.add(entry("ls", 2));
    history.add(entry("cd VI_opl", 3));
    history.add(entry("cd vi_afp/", 4));
    let found: Vec<&str> = history
        .matching(" vi ")
        .iter()
        .map(|entry| entry.command.as_str())
        .collect();
    assert_eq!(found, ["cd vi", "cd VI_opl", "cd vi_afp/"]);
    assert_eq!(history.matching("").len(), 4);
    assert!(history.matching("zzz").is_empty());
    history.add(entry("echo \u{c9}t\u{c9}", 5));
    assert_eq!(history.matching("\u{e9}t\u{e9}").len(), 1);
    assert_eq!(history.matching("O T").len(), 0);
    assert_eq!(history.matching("ECHO").len(), 1);
}

#[test]
fn jsonl_round_trip_dedups_and_caps() {
    let text = concat!(
        "{\"cmd\":\"ls\",\"dir\":\"/a\",\"time\":1}\n",
        "not json\n",
        "{\"cmd\":\"pwd\",\"time\":2}\n",
        "\n",
        "{\"cmd\":\"ls\",\"dir\":\"/b\",\"time\":3}\n",
        "{\"cmd\":\"a\\nb\",\"time\":4}\n",
        "{\"cmd\":\"make\",\"time\":5}\n",
    );
    let (history, lines) = CommandHistory::from_jsonl(text, 10);
    assert_eq!(lines, 6);
    assert_eq!(commands(&history), ["pwd", "ls", "make"]);
    assert_eq!(history.entries()[1].dir, Some("/b".into()));
    let (again, _) = CommandHistory::from_jsonl(&history.to_jsonl(), 10);
    assert_eq!(again, history);
    let (capped, _) = CommandHistory::from_jsonl(text, 2);
    assert_eq!(commands(&capped), ["ls", "make"]);
}

#[test]
fn selection_scrolls_to_stay_in_view() {
    let mut selection = HistorySelection::default();
    assert_eq!(selection.visible(25, 10), 15..25);
    assert_eq!(selection.index(25), Some(24));
    for _ in 0..9 {
        selection.older(1, 25, 10);
    }
    assert_eq!(selection.visible(25, 10), 15..25);
    selection.older(1, 25, 10);
    assert_eq!((selection.selected, selection.hidden_below), (10, 1));
    assert_eq!(selection.visible(25, 10), 14..24);
    assert_eq!(selection.index(25), Some(14));
    selection.older(100, 25, 10);
    assert_eq!(selection.visible(25, 10), 0..10);
    assert_eq!(selection.index(25), Some(0));
    selection.newer(10, 25, 10);
    assert_eq!(selection.visible(25, 10), 1..11);
    assert_eq!(selection.index(25), Some(10));
    selection.newer(100, 25, 10);
    assert_eq!(selection, HistorySelection::default());
}

#[test]
fn selection_follows_fewer_matches() {
    let mut selection = HistorySelection {
        selected: 20,
        hidden_below: 15,
    };
    selection.keep_in_view(3, 10);
    assert_eq!(selection.visible(3, 10), 0..3);
    assert_eq!(selection.index(3), Some(0));
    selection.keep_in_view(0, 10);
    assert_eq!(selection.visible(0, 10), 0..0);
    assert_eq!(selection.index(0), None);
}

#[test]
fn compaction_once_lines_pass_half_again_the_cap() {
    let history = CommandHistory::new(10);
    assert!(!history.needs_compaction(15));
    assert!(history.needs_compaction(16));
}

#[cfg(unix)]
#[test]
fn non_utf8_folders_are_dropped() {
    use std::ffi::OsStr;
    use std::os::unix::ffi::OsStrExt;

    let bad = PathBuf::from(OsStr::from_bytes(b"/tmp/\xff"));
    let entry = HistoryEntry::new("ls", Some(bad), 1);
    assert_eq!(entry.dir, None);
    assert!(entry_line(&entry).is_some());
}

#[test]
fn appended_lines_load_back_and_compact() {
    let dir = std::env::temp_dir().join(format!("agentws-history-{}", std::process::id()));
    let path = dir.join("history.jsonl");
    let _ = fs::remove_dir_all(&dir);
    for time in 0..8 {
        append_entry(&path, &entry(&format!("cmd {}", time % 3), time)).unwrap();
    }
    let history = CommandHistory::load(&path, 2);
    assert_eq!(commands(&history), ["cmd 0", "cmd 1"]);
    // 8 lines > 2 + 1: the file was rewritten with just the kept entries.
    let text = fs::read_to_string(&path).unwrap();
    assert_eq!(text.lines().count(), 2);
    assert_eq!(CommandHistory::load(&dir.join("missing"), 5).len(), 0);
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let mode = fs::metadata(&path).unwrap().permissions().mode();
        assert_eq!(mode & 0o777, 0o600, "compacted file is owner-only");
    }
    fs::remove_dir_all(&dir).unwrap();
}

#[test]
fn removed_commands_leave_the_history_and_the_file() {
    let mut history = CommandHistory::new(10);
    history.add(entry("ls", 1));
    history.add(entry("cd src", 2));
    assert!(history.remove("ls"));
    assert!(!history.remove("ls"));
    assert_eq!(commands(&history), ["cd src"]);

    let dir = std::env::temp_dir().join(format!("agentws-remove-{}", std::process::id()));
    let path = dir.join("history.jsonl");
    let _ = fs::remove_dir_all(&dir);
    remove_command(&path, "ls").unwrap();
    // Earlier uses of the command are in the file too; a damaged line stays.
    for (time, command) in [(1, "ls"), (2, "cd src"), (3, "ls")] {
        append_entry(&path, &entry(command, time)).unwrap();
    }
    fs::write(
        &path,
        [fs::read(&path).unwrap(), b"{\"cmd\":\"pw\n".to_vec()].concat(),
    )
    .unwrap();
    remove_command(&path, "ls").unwrap();
    let text = fs::read_to_string(&path).unwrap();
    assert_eq!(text.lines().count(), 2, "{text}");
    assert!(text.contains("cd src") && text.contains("pw") && !text.contains("\"ls\""));
    assert_eq!(commands(&CommandHistory::load(&path, 10)), ["cd src"]);
    fs::remove_dir_all(&dir).unwrap();
}

#[test]
fn damaged_files_keep_their_other_lines() {
    let dir = std::env::temp_dir().join(format!("agentws-damaged-{}", std::process::id()));
    let path = dir.join("history.jsonl");
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(&dir).unwrap();
    // A non-UTF-8 line, and a last line cut off by a crash.
    fs::write(
        &path,
        b"{\"cmd\":\"ls\"}\n{\"cmd\":\"\xff\"}\n{\"cmd\":\"pw",
    )
    .unwrap();
    append_entry(&path, &entry("make", 1)).unwrap();
    let history = CommandHistory::load(&path, 10);
    assert_eq!(commands(&history), ["ls", "make"]);
    fs::remove_dir_all(&dir).unwrap();
}

#[test]
fn append_in_subprocess() {
    let Some(path) = std::env::var_os("AGENTWS_HISTORY_LOCK_TEST") else {
        return;
    };
    let path = PathBuf::from(path);
    fs::write(path.with_extension("ready"), "").unwrap();
    append_entry(&path, &entry("from child", 2)).unwrap();
}

#[test]
fn cross_process_append_waits_for_replacement() {
    use std::process::Command;
    use std::thread;
    use std::time::{Duration, Instant};

    let root = TempDir::new();
    let path = root.0.join("history.jsonl");
    append_entry(&path, &entry("old", 1)).unwrap();
    let lock = lock_history(&path).unwrap();
    let mut child = Command::new(std::env::current_exe().unwrap())
        .args([
            "--exact",
            "history::tests::append_in_subprocess",
            "--nocapture",
        ])
        .env("AGENTWS_HISTORY_LOCK_TEST", &path)
        .spawn()
        .unwrap();
    let deadline = Instant::now() + Duration::from_secs(10);
    while !path.with_extension("ready").exists() {
        assert!(Instant::now() < deadline, "child did not start");
        thread::sleep(Duration::from_millis(10));
    }
    thread::sleep(Duration::from_millis(100));
    assert!(
        child.try_wait().unwrap().is_none(),
        "append bypassed the lock"
    );
    compact_locked(
        &path,
        entry_line(&entry("replacement", 1)).unwrap().as_bytes(),
    )
    .unwrap();
    drop(lock);
    assert!(child.wait().unwrap().success());
    assert_eq!(
        commands(&CommandHistory::load(&path, 10)),
        ["replacement", "from child"]
    );
}

#[test]
fn concurrent_append_delete_and_compaction_preserve_other_commands() {
    use std::thread;

    let root = TempDir::new();
    let path = root.0.join("history.jsonl");
    thread::scope(|scope| {
        for worker in 0..4 {
            let path = &path;
            scope.spawn(move || {
                for index in 0..25 {
                    append_entry(
                        path,
                        &entry(&format!("worker {worker} command {index}"), index),
                    )
                    .unwrap();
                    append_entry(path, &entry("remove me", index)).unwrap();
                    remove_command(path, "remove me").unwrap();
                    CommandHistory::load(path, 100);
                }
            });
        }
    });
    let history = CommandHistory::load(&path, 100);
    assert_eq!(history.len(), 100);
    assert!(
        !history
            .entries()
            .iter()
            .any(|entry| entry.command == "remove me")
    );
}
