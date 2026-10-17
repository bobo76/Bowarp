use std::ffi::OsStr;
use std::path::{Path, PathBuf};

use term_core::{LaunchSpec, SessionKind};

/// A program the new-tab menu offers.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Launcher {
    pub label: String,
    pub kind: SessionKind,
    /// Resolved path, or `None` when the program is not on `PATH`.
    pub program: Option<PathBuf>,
}

impl Launcher {
    pub fn spec(&self, cwd: Option<PathBuf>) -> Option<LaunchSpec> {
        let program = self.program.as_ref()?.to_string_lossy().into_owned();
        Some(LaunchSpec::new(self.kind, Some(program), Vec::new(), cwd))
    }
}

/// Shells offered when found on `PATH`: (program, label).
#[cfg(unix)]
const SHELLS: &[(&str, &str)] = &[
    ("bash", "Bash"),
    ("zsh", "Zsh"),
    ("fish", "Fish"),
    ("pwsh", "PowerShell"),
];
#[cfg(windows)]
const SHELLS: &[(&str, &str)] = &[
    ("pwsh", "PowerShell"),
    ("powershell", "Windows PowerShell"),
    ("cmd", "Command Prompt"),
    ("bash", "Bash"),
];

const AGENTS: &[(&str, &str, SessionKind)] = &[
    ("claude", "Claude Code", SessionKind::ClaudeCode),
    ("copilot", "Copilot CLI", SessionKind::CopilotCli),
];

/// Installed shells, the default shell first, followed by the agent CLIs (listed even when
/// missing, so the menu shows what could be launched).
pub fn discover() -> Vec<Launcher> {
    let path = std::env::var_os("PATH").unwrap_or_default();
    let default_shell = std::env::var_os("SHELL").map(PathBuf::from);
    let mut launchers: Vec<Launcher> = SHELLS
        .iter()
        .filter_map(|&(name, label)| {
            let program = find_program(name, &path)?;
            Some(Launcher {
                label: label.into(),
                kind: SessionKind::Shell,
                program: Some(program),
            })
        })
        .collect();
    if let Some(index) = default_shell
        .as_deref()
        .and_then(|shell| default_shell_index(&launchers, shell))
    {
        launchers[..=index].rotate_right(1);
    }
    launchers.extend(AGENTS.iter().map(|&(name, label, kind)| Launcher {
        label: label.into(),
        kind,
        program: find_program(name, &path),
    }));
    launchers
}

fn default_shell_index(launchers: &[Launcher], default_shell: &Path) -> Option<usize> {
    let name = default_shell.file_stem()?;
    launchers
        .iter()
        .position(|launcher| launcher.program.as_deref().and_then(Path::file_stem) == Some(name))
}

/// Name shown for a session until its program sets a title.
pub fn default_name(spec: &LaunchSpec) -> String {
    match spec.kind {
        SessionKind::ClaudeCode => "Claude Code".into(),
        SessionKind::CopilotCli => "Copilot CLI".into(),
        SessionKind::Shell => spec
            .program
            .as_deref()
            .map(PathBuf::from)
            .or_else(|| std::env::var_os("SHELL").map(PathBuf::from))
            .as_deref()
            .and_then(Path::file_stem)
            .map_or_else(
                || "Shell".into(),
                |name| name.to_string_lossy().into_owned(),
            ),
    }
}

#[cfg(unix)]
const EXTENSIONS: &[&str] = &[""];
#[cfg(windows)]
const EXTENSIONS: &[&str] = &["exe", "cmd", "bat", "com"];

/// First executable named `name` in the directories of `path` (a `PATH`-style list).
fn find_program(name: &str, path: &OsStr) -> Option<PathBuf> {
    std::env::split_paths(path)
        .flat_map(|dir| {
            EXTENSIONS
                .iter()
                .map(move |extension| dir.join(name).with_extension(extension))
        })
        .find(|candidate| is_executable(candidate))
}

#[cfg(unix)]
fn is_executable(path: &Path) -> bool {
    use std::os::unix::fs::PermissionsExt;
    path.metadata()
        .is_ok_and(|metadata| metadata.is_file() && metadata.permissions().mode() & 0o111 != 0)
}

#[cfg(windows)]
fn is_executable(path: &Path) -> bool {
    path.is_file()
}

#[cfg(all(test, unix))]
mod tests {
    use std::fs;
    use std::os::unix::fs::PermissionsExt;

    use super::*;

    fn temp_dir(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("agentws-{name}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        dir
    }

    fn write_file(path: &Path, mode: u32) {
        fs::write(path, "").unwrap();
        fs::set_permissions(path, fs::Permissions::from_mode(mode)).unwrap();
    }

    #[test]
    fn finds_first_executable_on_path() {
        let first = temp_dir("path-a");
        let second = temp_dir("path-b");
        write_file(&first.join("tool"), 0o644);
        write_file(&second.join("tool"), 0o755);
        let path = std::env::join_paths([&first, &second]).unwrap();

        assert_eq!(find_program("tool", &path), Some(second.join("tool")));
        assert_eq!(find_program("missing", &path), None);
        fs::remove_dir_all(first).unwrap();
        fs::remove_dir_all(second).unwrap();
    }

    fn shell(path: &str) -> Launcher {
        Launcher {
            label: path.into(),
            kind: SessionKind::Shell,
            program: Some(path.into()),
        }
    }

    #[test]
    fn finds_default_shell_by_name() {
        let launchers = [shell("/usr/bin/bash"), shell("/usr/bin/zsh")];
        assert_eq!(
            default_shell_index(&launchers, Path::new("/bin/zsh")),
            Some(1)
        );
        assert_eq!(
            default_shell_index(&launchers, Path::new("/bin/fish")),
            None
        );
    }

    #[test]
    fn default_names() {
        let claude = LaunchSpec::claude_code(None);
        assert_eq!(default_name(&claude), "Claude Code");
        let bash = LaunchSpec::new(
            SessionKind::Shell,
            Some("/usr/bin/bash".into()),
            vec![],
            None,
        );
        assert_eq!(default_name(&bash), "bash");
    }
}
