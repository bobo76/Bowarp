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
fn sums_added_lines_skipping_binary_files() {
    let numstat = "12\t3\tcrates/app/src/sidebar.rs\n-\t-\tdocs/shot.png\n0\t5\tREADME.md\n1\t0\tnew file.txt\n";
    assert_eq!(numstat_added(numstat), 13);
    assert_eq!(numstat_added(""), 0);
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
