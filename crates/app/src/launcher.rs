use std::ffi::OsStr;
use std::path::{Path, PathBuf};

use term_core::{LaunchSpec, SessionKind};
#[cfg(any(windows, test))]
use workspace_model::distro_icon;
use workspace_model::{LauncherIcon, shell_icon};

/// A program the new-tab menu offers.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Launcher {
    pub label: String,
    pub kind: SessionKind,
    /// Resolved path, or `None` when the program is not on `PATH`.
    pub program: Option<PathBuf>,
    pub args: Vec<String>,
    pub icon: LauncherIcon,
}

impl Launcher {
    pub fn spec(&self, cwd: Option<PathBuf>) -> Option<LaunchSpec> {
        let program = self.program.as_ref()?.to_string_lossy().into_owned();
        Some(LaunchSpec::new(
            self.kind,
            Some(program),
            self.args.clone(),
            cwd,
        ))
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
];

/// WSL distros that only back other tools and have no usable shell.
#[cfg(any(windows, test))]
const HIDDEN_DISTROS: &[&str] = &["docker-desktop", "docker-desktop-data"];
const WSL_DISTRO_FLAG: &str = "-d";

/// Interactive login shell, as Git's own "Git Bash" shortcut starts it.
#[cfg(any(windows, test))]
const GIT_BASH_ARGS: &[&str] = &["-i", "-l"];

/// Agent CLIs: (program, label, kind, icon).
const AGENTS: &[(&str, &str, SessionKind, LauncherIcon)] = &[
    (
        "claude",
        "Claude Code",
        SessionKind::ClaudeCode,
        LauncherIcon::Claude,
    ),
    (
        "copilot",
        "Copilot CLI",
        SessionKind::CopilotCli,
        LauncherIcon::Copilot,
    ),
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
                args: Vec::new(),
                icon: shell_icon(name),
            })
        })
        .collect();
    #[cfg(windows)]
    launchers.extend(windows::extra_shells(&path));
    if let Some(index) = default_shell
        .as_deref()
        .and_then(|shell| default_shell_index(&launchers, shell))
    {
        launchers[..=index].rotate_right(1);
    }
    launchers.extend(AGENTS.iter().map(|&(name, label, kind, icon)| Launcher {
        label: label.into(),
        kind,
        program: find_program(name, &path),
        args: Vec::new(),
        icon,
    }));
    launchers
}

/// The launcher the menu's "Terminal" entry (and later Ctrl+Shift+T) opens: Git Bash on
/// Windows, else the first installed shell, which is `$SHELL` when `discover` found it.
pub fn default_shell(launchers: &[Launcher]) -> Option<&Launcher> {
    preferred_shell(launchers, cfg!(windows))
}

fn preferred_shell(launchers: &[Launcher], prefer_git_bash: bool) -> Option<&Launcher> {
    let mut installed = launchers
        .iter()
        .filter(|launcher| launcher.kind == SessionKind::Shell && launcher.program.is_some());
    let first = installed.clone().next();
    if prefer_git_bash {
        return installed
            .find(|launcher| launcher.icon == LauncherIcon::GitBash)
            .or(first);
    }
    first
}

/// The `+` menu's entries: the default shell (shown as "Terminal"), then the shells, then the
/// agents.
pub fn menu_sections(
    launchers: &[Launcher],
) -> (Option<&Launcher>, Vec<&Launcher>, Vec<&Launcher>) {
    let (shells, agents) = launchers
        .iter()
        .partition(|launcher| launcher.kind == SessionKind::Shell);
    (default_shell(launchers), shells, agents)
}

fn default_shell_index(launchers: &[Launcher], default_shell: &Path) -> Option<usize> {
    let name = default_shell.file_stem()?;
    launchers
        .iter()
        .position(|launcher| launcher.program.as_deref().and_then(Path::file_stem) == Some(name))
}

/// Name shown for a session until its program sets a title.
pub fn default_name(spec: &LaunchSpec) -> String {
    if let Some(&(_, label, ..)) = AGENTS.iter().find(|&&(_, _, kind, _)| kind == spec.kind) {
        return label.into();
    }
    if let Some(distro) = wsl_distro(spec) {
        return distro.into();
    }
    spec.program
        .as_deref()
        .map(PathBuf::from)
        .or_else(|| std::env::var_os("SHELL").map(PathBuf::from))
        .as_deref()
        .and_then(Path::file_stem)
        .map_or_else(
            || "Shell".into(),
            |name| name.to_string_lossy().into_owned(),
        )
}

/// The distro a `wsl -d <distro>` launch opens.
fn wsl_distro(spec: &LaunchSpec) -> Option<&str> {
    let program = Path::new(spec.program.as_deref()?);
    if !program.file_stem()?.eq_ignore_ascii_case("wsl") {
        return None;
    }
    let flag = spec.args.iter().position(|arg| arg == WSL_DISTRO_FLAG)?;
    spec.args.get(flag + 1).map(String::as_str)
}

#[cfg(any(windows, test))]
fn wsl_launcher(wsl: &Path, distro: &str) -> Launcher {
    Launcher {
        label: distro.into(),
        kind: SessionKind::Shell,
        program: Some(wsl.into()),
        args: vec![WSL_DISTRO_FLAG.into(), distro.into()],
        icon: distro_icon(distro),
    }
}

#[cfg(any(windows, test))]
fn git_bash_launcher(program: PathBuf) -> Launcher {
    Launcher {
        label: "Git Bash".into(),
        kind: SessionKind::Shell,
        program: Some(program),
        args: GIT_BASH_ARGS.iter().map(|&arg| arg.into()).collect(),
        icon: LauncherIcon::GitBash,
    }
}

#[cfg(any(windows, test))]
fn shows_distro(distro: &str) -> bool {
    !HIDDEN_DISTROS
        .iter()
        .any(|hidden| hidden.eq_ignore_ascii_case(distro))
}

/// The program a launch runs, for error messages.
pub fn launch_name(spec: &LaunchSpec) -> &str {
    spec.program.as_deref().unwrap_or("shell")
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

/// Shells `PATH` does not find on Windows: `bash` there is `System32\bash.exe`, the WSL
/// launcher (default distro), and Git only puts `Git\cmd` on `PATH`.
#[cfg(windows)]
mod windows {
    use std::ffi::OsStr;
    use std::path::{Path, PathBuf};

    use super::{Launcher, find_program, git_bash_launcher, shows_distro, wsl_launcher};

    const GIT_BASH: &str = r"bin\bash.exe";
    const LXSS_KEY: &str = r"Software\Microsoft\Windows\CurrentVersion\Lxss";
    const DISTRIBUTION_NAME: &str = "DistributionName";

    pub(super) fn extra_shells(path: &OsStr) -> Vec<Launcher> {
        let mut shells: Vec<Launcher> = git_bash(path).map(git_bash_launcher).into_iter().collect();
        if let Some(wsl) = find_program("wsl", path) {
            shells.extend(
                wsl_distros()
                    .iter()
                    .filter(|distro| shows_distro(distro))
                    .map(|distro| wsl_launcher(&wsl, distro)),
            );
        }
        shells
    }

    /// Git Bash of the Git on `PATH`, else of a default install.
    fn git_bash(path: &OsStr) -> Option<PathBuf> {
        let installs = [
            std::env::var_os("ProgramFiles").map(|dir| PathBuf::from(dir).join("Git")),
            std::env::var_os("LOCALAPPDATA").map(|dir| PathBuf::from(dir).join(r"Programs\Git")),
        ];
        find_program("git", path)
            .and_then(|git| git_bash_near(&git))
            .or_else(|| {
                installs
                    .into_iter()
                    .flatten()
                    .map(|root| root.join(GIT_BASH))
                    .find(|bash| bash.is_file())
            })
    }

    /// `git.exe` sits in `Git\cmd` or `Git\mingw64\bin`; Git Bash is `Git\bin\bash.exe`.
    pub(super) fn git_bash_near(git: &Path) -> Option<PathBuf> {
        git.ancestors()
            .skip(1)
            .map(|dir| dir.join(GIT_BASH))
            .find(|bash| bash.is_file())
    }

    /// Registered distros, read from the registry so no `wsl.exe` runs at startup.
    fn wsl_distros() -> Vec<String> {
        let Ok(lxss) = windows_registry::CURRENT_USER.open(LXSS_KEY) else {
            return Vec::new();
        };
        let Ok(ids) = lxss.keys() else {
            return Vec::new();
        };
        ids.filter_map(|id| lxss.open(id).ok()?.get_string(DISTRIBUTION_NAME).ok())
            .collect()
    }
}

#[cfg(test)]
mod wsl_tests {
    use super::*;

    #[test]
    fn wsl_launch_is_named_after_its_distro() {
        let launcher = wsl_launcher(Path::new("wsl.exe"), "Ubuntu");
        let spec = launcher.spec(None).unwrap();
        assert_eq!(spec.args, ["-d", "Ubuntu"]);
        assert_eq!(default_name(&spec), "Ubuntu");
        assert_eq!(launcher.icon, LauncherIcon::Ubuntu);
        let vpnkit = wsl_launcher(Path::new("wsl.exe"), "wsl-vpnkit");
        assert_eq!(vpnkit.icon, LauncherIcon::Linux);
    }

    #[test]
    fn git_bash_has_the_git_icon() {
        let launcher = git_bash_launcher(PathBuf::from(r"C:\Program Files\Git\bin\bash.exe"));
        assert_eq!(launcher.label, "Git Bash");
        assert_eq!(launcher.args, ["-i", "-l"]);
        assert_eq!(launcher.icon, LauncherIcon::GitBash);
    }

    #[test]
    fn hides_docker_backing_distros() {
        assert!(shows_distro("Ubuntu"));
        assert!(shows_distro("wsl-vpnkit"));
        assert!(!shows_distro("docker-desktop"));
        assert!(!shows_distro("Docker-Desktop-Data"));
    }
}

#[cfg(all(test, windows))]
mod windows_tests {
    use std::fs;

    use super::windows::git_bash_near;

    #[test]
    fn finds_git_bash_from_cmd_and_mingw_git() {
        let root = std::env::temp_dir().join(format!("agentws-git-{}", std::process::id()));
        let _ = fs::remove_dir_all(&root);
        fs::create_dir_all(root.join("bin")).unwrap();
        fs::write(root.join(r"bin\bash.exe"), "").unwrap();

        let bash = Some(root.join(r"bin\bash.exe"));
        assert_eq!(git_bash_near(&root.join(r"cmd\git.exe")), bash);
        assert_eq!(git_bash_near(&root.join(r"mingw64\bin\git.exe")), bash);
        fs::remove_dir_all(root).unwrap();
    }
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
            args: Vec::new(),
            icon: shell_icon(path),
        }
    }

    #[test]
    fn terminal_entry_opens_first_installed_shell() {
        let mut missing = shell("/usr/bin/zsh");
        missing.program = None;
        let mut claude = shell("/usr/bin/claude");
        claude.kind = SessionKind::ClaudeCode;
        let launchers = [claude, missing, shell("/usr/bin/bash")];
        assert_eq!(
            default_shell(&launchers).and_then(|shell| shell.program.as_deref()),
            Some(Path::new("/usr/bin/bash"))
        );
        assert_eq!(default_shell(&launchers[..2]), None);
    }

    #[test]
    fn windows_terminal_entry_prefers_git_bash() {
        let launchers = [
            shell("/usr/bin/pwsh"),
            git_bash_launcher("/git/bin/bash.exe".into()),
        ];
        let program =
            |prefer| preferred_shell(&launchers, prefer).and_then(|shell| shell.program.as_deref());
        assert_eq!(program(true), Some(Path::new("/git/bin/bash.exe")));
        assert_eq!(program(false), Some(Path::new("/usr/bin/pwsh")));
        assert_eq!(
            preferred_shell(&launchers[..1], true).and_then(|shell| shell.program.as_deref()),
            Some(Path::new("/usr/bin/pwsh"))
        );
    }

    #[test]
    fn menu_lists_shells_then_agents() {
        let mut claude = shell("/usr/bin/claude");
        claude.kind = SessionKind::ClaudeCode;
        let launchers = [shell("/usr/bin/zsh"), claude, shell("/usr/bin/bash")];
        let (default, shells, agents) = menu_sections(&launchers);
        let programs = |group: Vec<&Launcher>| {
            group
                .into_iter()
                .map(|launcher| launcher.program.clone().unwrap())
                .collect::<Vec<_>>()
        };
        assert_eq!(
            default.and_then(|shell| shell.program.as_deref()),
            Some(Path::new("/usr/bin/zsh"))
        );
        assert_eq!(
            programs(shells),
            [
                PathBuf::from("/usr/bin/zsh"),
                PathBuf::from("/usr/bin/bash")
            ]
        );
        assert_eq!(programs(agents), [PathBuf::from("/usr/bin/claude")]);
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
