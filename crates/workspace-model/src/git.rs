use std::fs;
use std::path::{Path, PathBuf};

/// Length of the abbreviated commit id shown for a detached HEAD.
const SHORT_COMMIT_LEN: usize = 7;

/// What a repository's HEAD points at.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum GitHead {
    Branch(String),
    /// A checked-out commit, abbreviated.
    Detached(String),
}

impl GitHead {
    /// HEAD of the repository containing `dir`, read from the files under `.git` (no `git`
    /// process). `None` when `dir` is not inside a repository or HEAD cannot be read.
    pub fn read(dir: &Path) -> Option<Self> {
        let git_dir = find_git_dir(dir)?;
        let head = fs::read_to_string(git_dir.join("HEAD")).ok()?;
        Self::parse(&head)
    }

    pub fn name(&self) -> &str {
        match self {
            Self::Branch(name) | Self::Detached(name) => name,
        }
    }

    fn parse(head: &str) -> Option<Self> {
        let head = head.trim();
        if let Some(reference) = head.strip_prefix("ref:") {
            let reference = reference.trim();
            let branch = reference.strip_prefix("refs/heads/").unwrap_or(reference);
            return Some(Self::Branch(branch.to_string()));
        }
        let is_commit =
            head.len() > SHORT_COMMIT_LEN && head.bytes().all(|b| b.is_ascii_hexdigit());
        is_commit.then(|| Self::Detached(head[..SHORT_COMMIT_LEN].to_string()))
    }
}

/// The git directory for `dir`: the nearest `.git` folder walking up, or the folder a `.git`
/// file points to (`gitdir: ...`, used by worktrees and submodules).
fn find_git_dir(dir: &Path) -> Option<PathBuf> {
    dir.ancestors().find_map(|folder| {
        let dot_git = folder.join(".git");
        if dot_git.is_dir() {
            return Some(dot_git);
        }
        let pointer = fs::read_to_string(&dot_git).ok()?;
        let target = pointer.trim().strip_prefix("gitdir:")?.trim();
        Some(folder.join(target))
    })
}

#[cfg(test)]
pub(crate) mod tests {
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
    fn no_head_outside_a_repository() {
        let folder = TempDir::new();
        assert_eq!(GitHead::read(&folder.0), None);
    }
}
