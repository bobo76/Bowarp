use super::*;
use crate::git::tests::TempDir;

fn entry(command: &str, time: u64) -> HistoryEntry {
    HistoryEntry::new(command, Some("/tmp".into()), time).in_shell("bash")
}

const ALL: HistoryScope = HistoryScope::All;

fn shell(key: &str) -> HistoryScope {
    HistoryScope::Shell(key.into())
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
    history.add(HistoryEntry::new("ls", Some("/home".into()), 3).in_shell("bash"));
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
    let suggest = |typed| history.suggestion(typed, "bash");
    assert_eq!(suggest("c"), Some("argo build"));
    assert_eq!(suggest("cl"), Some("ear"));
    assert_eq!(suggest("cargo t"), Some("est"));
    assert_eq!(suggest("clear"), None);
    assert_eq!(suggest("Cl"), None);
    assert_eq!(suggest(""), None);
    assert_eq!(suggest("x"), None);
}

#[test]
fn matching_is_a_case_insensitive_substring_oldest_first() {
    let mut history = CommandHistory::new(10);
    history.add(entry("cd vi", 1));
    history.add(entry("ls", 2));
    history.add(entry("cd VI_opl", 3));
    history.add(entry("cd vi_afp/", 4));
    let found: Vec<&str> = history
        .matching(" vi ", &ALL)
        .iter()
        .map(|entry| entry.command.as_str())
        .collect();
    assert_eq!(found, ["cd vi", "cd VI_opl", "cd vi_afp/"]);
    assert_eq!(history.matching("", &ALL).len(), 4);
    assert!(history.matching("zzz", &ALL).is_empty());
    history.add(entry("echo \u{c9}t\u{c9}", 5));
    assert_eq!(history.matching("\u{e9}t\u{e9}", &ALL).len(), 1);
    assert_eq!(history.matching("O T", &ALL).len(), 0);
    assert_eq!(history.matching("ECHO", &shell("bash")).len(), 1);
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
    assert!(history.remove("ls", &ALL));
    assert!(!history.remove("ls", &ALL));
    assert_eq!(commands(&history), ["cd src"]);

    let dir = std::env::temp_dir().join(format!("agentws-remove-{}", std::process::id()));
    let path = dir.join("history.jsonl");
    let _ = fs::remove_dir_all(&dir);
    remove_command(&path, "ls", &ALL).unwrap();
    // Earlier uses of the command are in the file too; a damaged line stays.
    for (time, command) in [(1, "ls"), (2, "cd src"), (3, "ls")] {
        append_entry(&path, &entry(command, time)).unwrap();
    }
    fs::write(
        &path,
        [fs::read(&path).unwrap(), b"{\"cmd\":\"pw\n".to_vec()].concat(),
    )
    .unwrap();
    remove_command(&path, "ls", &ALL).unwrap();
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
                    remove_command(path, "remove me", &ALL).unwrap();
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

#[test]
fn clearing_empties_memory_and_the_file_and_tolerates_a_missing_file() {
    let root = TempDir::new();
    let path = root.0.join("history.jsonl");
    clear_history_file(&path).unwrap();
    assert!(!path.exists());
    append_entry(&path, &entry("make", 1)).unwrap();
    append_entry(&path, &entry("cargo test", 2)).unwrap();
    let mut history = CommandHistory::load(&path, 10);
    assert_eq!(history.len(), 2);
    history.clear();
    assert!(history.is_empty());
    clear_history_file(&path).unwrap();
    assert!(CommandHistory::load(&path, 10).is_empty());
    append_entry(&path, &entry("after", 3)).unwrap();
    assert_eq!(commands(&CommandHistory::load(&path, 10)), ["after"]);
}

#[test]
fn entries_saved_before_shells_load_without_one_and_round_trip() {
    let text = concat!(
        "{\"cmd\":\"ls\",\"time\":1}\n",
        "{\"cmd\":\"dir\",\"time\":2,\"shell\":\"pwsh\"}\n",
    );
    let (history, _) = CommandHistory::from_jsonl(text, 10);
    assert_eq!(history.entries()[0].shell, None);
    assert_eq!(history.entries()[1].shell.as_deref(), Some("pwsh"));
    assert_eq!(history.shells().collect::<Vec<_>>(), [None, Some("pwsh")]);
    // No shell writes no field: the line reads the same in an older version.
    assert_eq!(history.to_jsonl(), text);
    assert_eq!(
        entry_line(&HistoryEntry::new("ls", None, 1).in_shell("wsl:Ubuntu")).unwrap(),
        "{\"cmd\":\"ls\",\"time\":1,\"shell\":\"wsl:Ubuntu\"}\n"
    );
    // Commands without a shell show only under "All" and are never suggested.
    assert!(
        history
            .matching("", &shell("pwsh"))
            .iter()
            .all(|e| e.command == "dir")
    );
    assert_eq!(history.matching("", &ALL).len(), 2);
    assert_eq!(history.suggestion("l", "pwsh"), None);
}

#[test]
fn commands_are_kept_once_per_shell_and_capped_per_shell() {
    let run = |command: &str, key: &str, time| HistoryEntry::new(command, None, time).in_shell(key);
    let mut history = CommandHistory::new(2);
    history.add(run("ls", "bash", 1));
    history.add(run("ls", "pwsh", 2));
    history.add(run("ls", "bash", 3));
    assert_eq!(commands(&history), ["ls", "ls"]);
    assert_eq!(history.count_in(&shell("bash")), 1);
    // A busy shell drops its own oldest commands, not the other shell's.
    for (time, command) in ["a", "b", "c"].into_iter().enumerate() {
        history.add(run(command, "bash", 10 + time as u64));
    }
    assert_eq!(commands(&history), ["ls", "b", "c"]);
    assert_eq!(history.count_in(&shell("pwsh")), 1);
    assert_eq!(history.count_in(&shell("bash")), 2);
    assert_eq!(history.count_in(&shell("zsh")), 0);
    assert_eq!(history.count_in(&ALL), 3);

    let (reloaded, _) = CommandHistory::from_jsonl(&history.to_jsonl(), 2);
    assert_eq!(reloaded, history);

    // The file keeps the same rules: once per shell, the newest of each shell up to the cap.
    let text = concat!(
        "{\"cmd\":\"x\",\"shell\":\"pwsh\"}\n",
        "{\"cmd\":\"a\",\"shell\":\"bash\"}\n",
        "{\"cmd\":\"b\",\"shell\":\"bash\"}\n",
        "{\"cmd\":\"a\",\"shell\":\"pwsh\"}\n",
        "{\"cmd\":\"c\",\"shell\":\"bash\"}\n",
        "{\"cmd\":\"b\",\"shell\":\"bash\"}\n",
    );
    let (loaded, _) = CommandHistory::from_jsonl(text, 2);
    let pairs: Vec<(&str, Option<&str>)> = loaded
        .entries()
        .iter()
        .map(|entry| (entry.command.as_str(), entry.shell.as_deref()))
        .collect();
    assert_eq!(
        pairs,
        [
            ("x", Some("pwsh")),
            ("a", Some("pwsh")),
            ("c", Some("bash")),
            ("b", Some("bash"))
        ]
    );
    // Compaction counts what is kept, not one shell's cap.
    assert!(!loaded.needs_compaction(5));
    assert!(loaded.needs_compaction(6));
}

#[test]
fn suggestions_come_from_the_own_shell_and_all_lists_a_command_once() {
    let run = |command: &str, key: &str, time| {
        HistoryEntry::new(command, Some(format!("/{key}").into()), time).in_shell(key)
    };
    let mut history = CommandHistory::new(10);
    history.add(run("git status", "bash", 1));
    history.add(run("Get-ChildItem", "pwsh", 2));
    history.add(run("git stash", "pwsh", 3));
    history.add(run("git status", "pwsh", 4));
    assert_eq!(history.suggestion("g", "bash"), Some("it status"));
    assert_eq!(history.suggestion("G", "bash"), None);
    assert_eq!(history.suggestion("G", "pwsh"), Some("et-ChildItem"));
    assert_eq!(history.suggestion("git st", "pwsh"), Some("atus"));
    assert_eq!(history.suggestion("git", "zsh"), None);

    let listed = |scope: &HistoryScope| -> Vec<(String, String)> {
        history
            .matching("git", scope)
            .iter()
            .map(|entry| {
                let dir = entry.dir.as_deref().unwrap().display().to_string();
                (entry.command.clone(), dir)
            })
            .collect()
    };
    let pair = |command: &str, dir: &str| (command.to_string(), dir.to_string());
    assert_eq!(listed(&shell("bash")), [pair("git status", "/bash")]);
    // "All": the newest use and its folder.
    assert_eq!(
        listed(&ALL),
        [pair("git stash", "/pwsh"), pair("git status", "/pwsh")]
    );
}

#[test]
fn removing_in_a_shell_keeps_the_others() {
    let run = |command: &str, key: &str, time| HistoryEntry::new(command, None, time).in_shell(key);
    let mut history = CommandHistory::new(10);
    history.add(run("ls", "bash", 1));
    history.add(run("ls", "pwsh", 2));
    history.add(HistoryEntry::new("ls", None, 0));
    assert!(history.remove("ls", &shell("bash")));
    assert!(!history.remove("ls", &shell("bash")));
    let (reloaded, _) = CommandHistory::from_jsonl(&history.to_jsonl(), 10);
    assert_eq!(reloaded, history);
    assert_eq!(history.shells().collect::<Vec<_>>(), [None, Some("pwsh")]);
    assert!(history.remove("ls", &ALL));
    assert!(history.is_empty() && history.shells().next().is_none());

    let root = TempDir::new();
    let path = root.0.join("history.jsonl");
    for entry in [
        run("ls", "bash", 1),
        run("ls", "pwsh", 2),
        HistoryEntry::new("ls", None, 3),
    ] {
        append_entry(&path, &entry).unwrap();
    }
    remove_command(&path, "ls", &shell("pwsh")).unwrap();
    let left = CommandHistory::load(&path, 10);
    assert_eq!(left.shells().collect::<Vec<_>>(), [None, Some("bash")]);
    remove_command(&path, "ls", &ALL).unwrap();
    assert!(CommandHistory::load(&path, 10).is_empty());
}
