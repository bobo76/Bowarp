use std::sync::atomic::{AtomicUsize, Ordering};

use super::*;

/// A fresh empty folder under the system temp dir, removed on drop.
pub struct TempDir(pub PathBuf);

impl TempDir {
    pub fn new() -> Self {
        static NEXT: AtomicUsize = AtomicUsize::new(0);
        let name = format!(
            "workspace-model-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        );
        let path = std::env::temp_dir().join(name);
        fs::create_dir_all(&path).unwrap();
        Self(path)
    }
}

impl Drop for TempDir {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

fn write(path: &Path, content: &str) {
    fs::create_dir_all(path.parent().unwrap()).unwrap();
    fs::write(path, content).unwrap();
}

#[test]
fn parses_branch_and_detached_heads() {
    assert_eq!(
        GitHead::parse("ref: refs/heads/main\n"),
        Some(GitHead::Branch("main".into()))
    );
    assert_eq!(
        GitHead::parse("ref: refs/heads/feature/sidebar"),
        Some(GitHead::Branch("feature/sidebar".into()))
    );
    assert_eq!(
        GitHead::parse("d608fd1c0ffee0000000000000000000000000ab\n"),
        Some(GitHead::Detached("d608fd1".into()))
    );
    assert_eq!(GitHead::parse("garbage"), None);
    assert_eq!(GitHead::parse(""), None);
}

#[test]
fn reads_head_from_a_subfolder() {
    let repo = TempDir::new();
    write(&repo.0.join(".git/HEAD"), "ref: refs/heads/m2-sidebar\n");
    let nested = repo.0.join("crates/app/src");
    fs::create_dir_all(&nested).unwrap();

    assert_eq!(
        GitHead::read(&nested),
        Some(GitHead::Branch("m2-sidebar".into()))
    );
}

#[test]
fn follows_a_worktree_gitdir_file() {
    let root = TempDir::new();
    write(
        &root.0.join("main/.git/worktrees/wt/HEAD"),
        "ref: refs/heads/topic\n",
    );
    write(
        &root.0.join("wt/.git"),
        "gitdir: ../main/.git/worktrees/wt\n",
    );

    assert_eq!(
        GitHead::read(&root.0.join("wt")),
        Some(GitHead::Branch("topic".into()))
    );
}

#[test]
fn numstat_counts_per_path_with_binary_files_unknown() {
    let numstat = concat!(
        "12\t3\tcrates/app/src/sidebar.rs\0",
        "-\t-\tdocs/shot.png\0",
        "1\t0\tnew\tfile.txt\0",
    );
    let counts = numstat_counts(numstat);
    assert_eq!(counts["crates/app/src/sidebar.rs"], (Some(12), Some(3)));
    assert_eq!(counts["docs/shot.png"], (None, None));
    assert_eq!(
        counts["new\tfile.txt"],
        (Some(1), Some(0)),
        "a tab in a name is kept"
    );
    assert!(numstat_counts("").is_empty());
}

#[test]
fn status_entries_read_each_kind_of_change() {
    let status = " M a.rs\0M  b.rs\0A  c.rs\0 D d.rs\0?? e f.rs\0UU g.rs\0AA h.rs\0!! i\0AM j.rs\0";
    assert_eq!(
        status_entries(status),
        vec![
            ("a.rs".to_string(), FileChange::Modified),
            ("b.rs".to_string(), FileChange::Modified),
            ("c.rs".to_string(), FileChange::Added),
            ("d.rs".to_string(), FileChange::Deleted),
            ("e f.rs".to_string(), FileChange::Untracked),
            ("g.rs".to_string(), FileChange::Conflicted),
            ("h.rs".to_string(), FileChange::Conflicted),
            ("j.rs".to_string(), FileChange::Added),
        ]
    );
}

#[test]
fn marks_name_changed_files_and_the_folders_holding_them() {
    let file = |path: &str, change| ChangedFile {
        path: PathBuf::from(path),
        change,
        added: None,
        removed: None,
    };
    let changes = WorkingChanges {
        files: vec![
            file("/repo/src/a/new.rs", FileChange::Untracked),
            file("/repo/src/b.rs", FileChange::Modified),
            file("/repo/gone.rs", FileChange::Deleted),
        ],
    };
    let marks = changes.marks();
    assert_eq!(
        marks[Path::new("/repo/src/a/new.rs")],
        FileChange::Untracked
    );
    assert_eq!(marks[Path::new("/repo/src/a")], FileChange::Modified);
    assert_eq!(marks[Path::new("/repo/src")], FileChange::Modified);
    assert!(!marks.contains_key(Path::new("/repo/gone.rs")));
}

#[test]
fn counts_lines_of_text_files_only() {
    assert_eq!(count_lines(b""), 0);
    assert_eq!(count_lines(b"one\ntwo\n"), 2);
    assert_eq!(count_lines(b"one\ntwo"), 2);
    assert_eq!(count_lines(b"\n"), 1);
    assert_eq!(count_lines(b"text\0with a nul\n"), 0);
}

fn git(dir: &Path, args: &[&str]) -> bool {
    Command::new("git")
        .args(["-c", "user.name=test", "-c", "user.email=test@example.com"])
        .args([
            "-c",
            "commit.gpgsign=false",
            "-c",
            "core.hooksPath=/dev/null",
        ])
        .args(args)
        .current_dir(dir)
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .is_ok_and(|status| status.success())
}

#[test]
fn counts_tracked_changes_and_untracked_files() {
    let repo = TempDir::new();
    if !git(&repo.0, &["init", "-q"]) {
        eprintln!("git not available, skipping");
        return;
    }
    write(&repo.0.join("tracked.txt"), "one\n");
    write(&repo.0.join(".gitignore"), "ignored.txt\n");
    assert!(git(&repo.0, &["add", "."]));
    assert!(git(&repo.0, &["commit", "-q", "-m", "initial"]));
    let nested = repo.0.join("src");
    fs::create_dir_all(&nested).unwrap();
    assert_eq!(lines_added(&nested), Some(0));

    write(&repo.0.join("tracked.txt"), "one\ntwo\nthree\n");
    write(&repo.0.join("new.txt"), "a\nb\nc");
    write(&nested.join("new.rs"), "fn main() {}\n");
    write(&repo.0.join("ignored.txt"), "not counted\n");
    fs::write(repo.0.join("blob.bin"), b"\0\x01\n").unwrap();

    // 2 added to tracked.txt, 3 in new.txt, 1 in src/new.rs; from any folder of the repo.
    assert_eq!(lines_added(&nested), Some(6));
    assert_eq!(lines_added(&repo.0), Some(6));

    let changes = working_changes(&nested).unwrap();
    let top = fs::canonicalize(&repo.0).unwrap();
    let found: Vec<(PathBuf, FileChange, Option<u64>, Option<u64>)> = changes
        .files
        .iter()
        .map(|file| {
            let path = fs::canonicalize(&file.path).unwrap();
            (path, file.change, file.added, file.removed)
        })
        .collect();
    assert_eq!(
        found,
        vec![
            (
                top.join("tracked.txt"),
                FileChange::Modified,
                Some(2),
                Some(0)
            ),
            (top.join("blob.bin"), FileChange::Untracked, None, None),
            (top.join("new.txt"), FileChange::Untracked, Some(3), Some(0)),
            (
                top.join("src/new.rs"),
                FileChange::Untracked,
                Some(1),
                Some(0)
            ),
        ]
    );
}

#[test]
fn no_diff_stat_outside_a_repository() {
    let folder = TempDir::new();
    assert_eq!(lines_added(&folder.0), None);
}

#[test]
fn no_head_outside_a_repository() {
    let folder = TempDir::new();
    assert_eq!(GitHead::read(&folder.0), None);
}

fn entries(names: &[&str]) -> Vec<DirEntry> {
    names
        .iter()
        .map(|name| DirEntry::new(*name, crate::file_tree::EntryKind::File))
        .collect()
}

#[test]
fn check_ignore_input_shields_names_from_pathspec_magic() {
    assert_eq!(
        check_ignore_input(&entries(&["a.log", ":(glob)*"])),
        b"./a.log\0./:(glob)*\0"
    );
}

#[test]
fn ignored_names_come_from_the_output_and_git_dir() {
    let listing = entries(&[".git", "a.log", "nl\nx.log", "src", "target"]);
    let ignored = ignored_from_output(b"./a.log\0./nl\nx.log\0./target\0./unknown\0", &listing);
    let expected: HashSet<String> = [".git", "a.log", "nl\nx.log", "target"]
        .into_iter()
        .map(String::from)
        .collect();
    assert_eq!(ignored, expected);
    assert!(ignored_from_output(b"", &entries(&["src"])).is_empty());
}

#[test]
fn ignored_names_ask_git_for_the_folder() {
    let repo = TempDir::new();
    if !git(&repo.0, &["init", "-q"]) {
        eprintln!("git not available, skipping");
        return;
    }
    write(&repo.0.join(".gitignore"), "target/\n*.log\n");
    write(&repo.0.join("target/debug/app"), "");
    write(&repo.0.join("src/main.rs"), "");
    write(&repo.0.join("a.log"), "");
    write(&repo.0.join("tracked.log"), "");
    // `:` and `*` are not allowed in Windows file names, so pathspec magic can't come from one.
    let magic = (!cfg!(windows)).then_some(":(glob)*");
    if let Some(name) = magic {
        write(&repo.0.join(name), "");
    }
    assert!(git(&repo.0, &["add", "-f", "tracked.log"]));
    let folder = |names: &[&str]| -> Vec<DirEntry> {
        names
            .iter()
            .map(|name| {
                let kind = if repo.0.join(name).is_dir() {
                    crate::file_tree::EntryKind::Folder
                } else {
                    crate::file_tree::EntryKind::File
                };
                DirEntry::new(*name, kind)
            })
            .collect()
    };
    let names = [
        ".git",
        ".gitignore",
        "a.log",
        "src",
        "target",
        "tracked.log",
    ];
    let root = folder(&[&names[..], magic.as_slice()].concat());
    let expected: HashSet<String> = [".git", "a.log", "target"]
        .into_iter()
        .map(String::from)
        .collect();
    assert_eq!(ignored_names(&repo.0, &root), Some(expected));
    assert_eq!(
        ignored_names(&repo.0.join("src"), &entries(&["main.rs"])),
        Some(HashSet::new())
    );
    // Inside an ignored folder, git reports every entry.
    assert_eq!(
        ignored_names(&repo.0.join("target/debug"), &entries(&["app"])),
        Some(HashSet::from(["app".to_string()]))
    );
    // Inside `.git` there is no work tree.
    assert_eq!(
        ignored_names(&repo.0.join(".git"), &entries(&["HEAD"])),
        None
    );
}

#[test]
fn nothing_is_ignored_outside_a_repository() {
    let folder = TempDir::new();
    write(&folder.0.join("a.log"), "");
    assert_eq!(ignored_names(&folder.0, &entries(&["a.log"])), None);
}
