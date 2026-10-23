use std::path::{Path, PathBuf};

use crate::git::GitHead;

/// Where a session works: its folder and that folder's git HEAD.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Location {
    pub folder: PathBuf,
    pub head: Option<GitHead>,
}

impl Location {
    /// Read the git HEAD for `folder`. Touches only a few files, so it is cheap enough to call
    /// whenever a tab's state changes.
    pub fn read(folder: PathBuf) -> Self {
        let head = GitHead::read(&folder);
        Self { folder, head }
    }
}

/// `path` for display, with the home folder shortened to `~`.
pub fn display_path(path: &Path, home: Option<&Path>) -> String {
    let relative = home
        .filter(|home| !home.as_os_str().is_empty())
        .and_then(|home| path.strip_prefix(home).ok());
    match relative {
        Some(rest) if rest.as_os_str().is_empty() => "~".into(),
        Some(rest) => format!("~{}{}", std::path::MAIN_SEPARATOR, rest.display()),
        None => path.display().to_string(),
    }
}

#[cfg(test)]
mod tests {
    use std::fs;

    use super::*;
    use crate::git::tests::TempDir;

    #[test]
    fn reads_folder_and_branch() {
        let repo = TempDir::new();
        fs::create_dir_all(repo.0.join(".git")).unwrap();
        fs::write(repo.0.join(".git/HEAD"), "ref: refs/heads/main\n").unwrap();

        let location = Location::read(repo.0.clone());
        assert_eq!(location.folder, repo.0);
        assert_eq!(location.head, Some(GitHead::Branch("main".into())));
    }

    #[cfg(unix)]
    #[test]
    fn shortens_the_home_folder() {
        let home = Some(Path::new("/home/user"));
        assert_eq!(
            display_path(Path::new("/home/user/dev/app"), home),
            "~/dev/app"
        );
        assert_eq!(display_path(Path::new("/home/user"), home), "~");
        assert_eq!(
            display_path(Path::new("/home/username"), home),
            "/home/username"
        );
        assert_eq!(display_path(Path::new("/etc"), home), "/etc");
        assert_eq!(display_path(Path::new("/etc"), None), "/etc");
    }
}
