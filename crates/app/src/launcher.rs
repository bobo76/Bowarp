use std::ffi::OsStr;
use std::path::{Path, PathBuf};

use term_core::{LaunchSpec, SessionKind};
#[cfg(any(windows, test))]
use workspace_model::distro_icon;
use workspace_model::file_tree::ShellSyntax;
use workspace_model::{LauncherIcon, shell_icon};

/// A program the new-tab menu offers.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Launcher {
    /// Stable key that settings.json saves (`default_shell`; later per-launcher settings): the
    /// program name of a `SHELLS`/`AGENTS` entry (`bash`, `zsh`, `fish`, `pwsh`, `powershell`,
    /// `cmd`, `claude`, `copilot`), `git-bash` for Git Bash (its program is `bash.exe` too) and
    /// `wsl:<distro>` for a WSL distro. Unique in a [`discover`] list; `label` is only shown.
    pub id: String,
    pub label: String,
    pub kind: SessionKind,
    /// Resolved path, or `None` when the program is not on `PATH`.
    pub program: Option<PathBuf>,
    pub args: Vec<String>,
    pub icon: LauncherIcon,
}

impl Launcher {
    /// The launcher name shown in menus, including why it cannot currently be opened.
    pub fn display_label(&self) -> String {
        if self.program.is_some() {
            self.label.clone()
        } else {
            format!("{} (not installed)", self.label)
        }
    }

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

/// Shells offered when found on `PATH`: (program, label). The program name is also the
/// launcher's [`Launcher::id`], saved in settings.json: don't rename it.
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
const HIDDEN_DISTROS: &[&str] = &["docker-desktop", "docker-desktop-data", "wsl-vpnkit"];
pub(crate) const WSL_DISTRO_FLAG: &str = "-d";

/// Interactive login shell, as Git's own "Git Bash" shortcut starts it.
#[cfg(any(windows, test))]
const GIT_BASH_ARGS: &[&str] = &["-i", "-l"];

/// Agent CLIs: (program, label, kind, icon). The program name is the [`Launcher::id`].
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
                id: name.into(),
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
        id: name.into(),
        label: label.into(),
        kind,
        program: find_program(name, &path),
        args: Vec::new(),
        icon,
    }));
    launchers
}

/// The launcher the menu's first entry and Ctrl+Shift+T open: the installed shell `setting`
/// (the `default_shell` setting) names, else Git Bash on Windows, else the first installed
/// shell, which is `$SHELL` when `discover` found it.
pub fn default_shell<'a>(launchers: &'a [Launcher], setting: Option<&str>) -> Option<&'a Launcher> {
    setting
        .and_then(|name| configured_shell(launchers, name))
        .or_else(|| preferred_shell(launchers, cfg!(windows)))
}

/// The session the first tab (no saved tabs) or a `shell` tab named on the command line starts
/// in `cwd`: the default shell (see [`default_shell`]), except that with no shell set (or
/// none installed by that name) outside Windows it is the user's `$SHELL` as is
/// (`LaunchSpec::shell`), even one the menu doesn't list. On Windows `LaunchSpec::shell` would
/// be alacritty's Windows PowerShell, unlike Ctrl+Shift+T; it stays the fallback when no shell
/// is found.
pub fn startup_shell(
    launchers: &[Launcher],
    setting: Option<&str>,
    cwd: Option<PathBuf>,
) -> LaunchSpec {
    startup_shell_on(launchers, setting, cfg!(windows), cwd)
}

fn startup_shell_on(
    launchers: &[Launcher],
    setting: Option<&str>,
    windows: bool,
    cwd: Option<PathBuf>,
) -> LaunchSpec {
    setting
        .and_then(|name| configured_shell(launchers, name))
        .or_else(|| windows.then(|| preferred_shell(launchers, true)).flatten())
        .and_then(|shell| shell.spec(cwd.clone()))
        .unwrap_or_else(|| LaunchSpec::shell(cwd))
}

/// The shells found on this machine, in menu order: the choices for the `default_shell`
/// setting.
pub fn installed_shells(launchers: &[Launcher]) -> impl Iterator<Item = &Launcher> + Clone {
    launchers
        .iter()
        .filter(|launcher| launcher.kind == SessionKind::Shell && launcher.program.is_some())
}

/// The installed shell the `default_shell` setting `value` names: by [`Launcher::id`] (as the
/// settings page saves it), else by exact menu name (`PowerShell` is pwsh, as earlier versions
/// saved it, although its id ignoring case is Windows PowerShell's), else, for a hand-edited
/// file, by id and then by menu name, ignoring case (`ZSH`, `git bash`).
pub fn configured_shell<'a>(launchers: &'a [Launcher], value: &str) -> Option<&'a Launcher> {
    let value = value.trim();
    let matchers: [&dyn Fn(&Launcher) -> bool; 4] = [
        &|launcher| launcher.id == value,
        &|launcher| launcher.label == value,
        &|launcher| launcher.id.eq_ignore_ascii_case(value),
        &|launcher| launcher.label.eq_ignore_ascii_case(value),
    ];
    let installed = installed_shells(launchers);
    matchers
        .into_iter()
        .find_map(|matches| installed.clone().find(|launcher| matches(launcher)))
}

/// The shell "Automatic" (an empty `default_shell`) starts both for the first tab and for
/// Ctrl+Shift+T, for the settings page to name: Git Bash (else the first shell) on Windows; on
/// Unix the listed shell `$SHELL` names, `None` when it isn't listed (the first tab then runs
/// `$SHELL` as is while Ctrl+Shift+T opens the first listed shell).
pub fn automatic_shell(launchers: &[Launcher]) -> Option<&Launcher> {
    let login_shell = std::env::var_os("SHELL").map(PathBuf::from);
    automatic_shell_on(launchers, cfg!(windows), login_shell.as_deref())
}

fn automatic_shell_on<'a>(
    launchers: &'a [Launcher],
    windows: bool,
    login_shell: Option<&Path>,
) -> Option<&'a Launcher> {
    let shell = preferred_shell(launchers, windows)?;
    if windows {
        return Some(shell);
    }
    let stem = login_shell?.file_stem()?;
    let program_stem = shell.program.as_deref().and_then(Path::file_stem);
    (program_stem == Some(stem)).then_some(shell)
}

fn preferred_shell(launchers: &[Launcher], prefer_git_bash: bool) -> Option<&Launcher> {
    let mut installed = installed_shells(launchers);
    let first = installed.clone().next();
    if prefer_git_bash {
        return installed
            .find(|launcher| launcher.icon == LauncherIcon::GitBash)
            .or(first);
    }
    first
}

/// The `+` menu's entries: the default shell (with the shortcut hint, see [`default_shell`]),
/// then the other shells, then the agents.
pub fn menu_sections<'a>(
    launchers: &'a [Launcher],
    setting: Option<&str>,
) -> (Option<&'a Launcher>, Vec<&'a Launcher>, Vec<&'a Launcher>) {
    let default = default_shell(launchers, setting);
    let (shells, agents) = launchers
        .iter()
        .filter(|&launcher| default.is_none_or(|default| !std::ptr::eq(launcher, default)))
        .partition(|launcher| launcher.kind == SessionKind::Shell);
    (default, shells, agents)
}

fn default_shell_index(launchers: &[Launcher], default_shell: &Path) -> Option<usize> {
    let name = default_shell.file_stem()?;
    launchers
        .iter()
        .position(|launcher| launcher.program.as_deref().and_then(Path::file_stem) == Some(name))
}

/// How the program `spec` starts parses typed paths. With no program, that is the user's
/// shell (`$SHELL`), or PowerShell on Windows (alacritty's default).
pub fn shell_syntax(spec: &LaunchSpec) -> ShellSyntax {
    let default = || {
        if cfg!(windows) {
            Some("powershell".to_string())
        } else {
            std::env::var("SHELL").ok()
        }
    };
    spec.program
        .clone()
        .or_else(default)
        .map_or(ShellSyntax::Posix, |program| {
            ShellSyntax::of_program(&program)
        })
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
        id: format!("wsl:{distro}"),
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
        id: "git-bash".into(),
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
    fn hides_helper_distros() {
        assert!(shows_distro("Ubuntu"));
        assert!(!shows_distro("WSL-VPNKit"));
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
        let stem = Path::new(path).file_stem().unwrap().to_string_lossy();
        Launcher {
            id: stem.to_lowercase(),
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
            default_shell(&launchers, None).and_then(|shell| shell.program.as_deref()),
            Some(Path::new("/usr/bin/bash"))
        );
        assert_eq!(default_shell(&launchers[..2], None), None);
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
    fn menu_lists_other_shells_then_agents() {
        let mut claude = shell("/usr/bin/claude");
        claude.kind = SessionKind::ClaudeCode;
        let launchers = [shell("/usr/bin/zsh"), claude, shell("/usr/bin/bash")];
        let (default, shells, agents) = menu_sections(&launchers, None);
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
        assert_eq!(programs(shells), [PathBuf::from("/usr/bin/bash")]);
        assert_eq!(programs(agents), [PathBuf::from("/usr/bin/claude")]);
    }

    fn program(launcher: Option<&Launcher>) -> Option<&Path> {
        launcher.and_then(|launcher| launcher.program.as_deref())
    }

    fn named(label: &str, path: &str) -> Launcher {
        Launcher {
            label: label.into(),
            ..shell(path)
        }
    }

    #[test]
    fn setting_picks_the_default_shell_by_id_or_menu_name() {
        let mut missing = named("Fish", "/usr/bin/fish");
        missing.program = None;
        let launchers = [
            named("Bash", "/usr/bin/bash"),
            named("Zsh", "/usr/bin/zsh"),
            missing,
        ];
        let zsh = Some(Path::new("/usr/bin/zsh"));
        let bash = Some(Path::new("/usr/bin/bash"));
        for value in ["zsh", " ZSH ", "Zsh", "zSH"] {
            assert_eq!(
                program(default_shell(&launchers, Some(value))),
                zsh,
                "{value}"
            );
        }
        // Not installed, unknown, or automatic: the first installed shell.
        assert_eq!(program(default_shell(&launchers, Some("fish"))), bash);
        assert_eq!(program(default_shell(&launchers, Some("tcsh"))), bash);
        assert_eq!(program(default_shell(&launchers, None)), bash);
    }

    fn windows_launchers() -> [Launcher; 6] {
        let wsl = Path::new("/windows/wsl.exe");
        [
            named("PowerShell", "/pwsh/pwsh.exe"),
            named("Windows PowerShell", "/windows/powershell.exe"),
            named("Command Prompt", "/windows/cmd.exe"),
            git_bash_launcher("/git/bin/bash.exe".into()),
            wsl_launcher(wsl, "Ubuntu"),
            wsl_launcher(wsl, "Debian"),
        ]
    }

    #[test]
    fn ids_tell_apart_shells_with_like_names_and_programs() {
        let launchers = windows_launchers();
        let label = |value| configured_shell(&launchers, value).map(|shell| shell.label.as_str());
        // A program name is the id, so `powershell` is Windows PowerShell, `pwsh` PowerShell 7.
        assert_eq!(label("powershell"), Some("Windows PowerShell"));
        assert_eq!(label("pwsh"), Some("PowerShell"));
        // The menu name, as earlier versions saved it, still picks PowerShell 7.
        assert_eq!(label("PowerShell"), Some("PowerShell"));
        assert_eq!(label("windows powershell"), Some("Windows PowerShell"));
        // Git Bash runs `bash.exe` too, but `bash` isn't its id.
        assert_eq!(label("git-bash"), Some("Git Bash"));
        assert_eq!(label("Git Bash"), Some("Git Bash"));
        assert_eq!(label("bash"), None);
        assert_eq!(label("CMD"), Some("Command Prompt"));
    }

    #[test]
    fn wsl_distros_sharing_wsl_exe_have_their_own_ids() {
        let launchers = windows_launchers();
        let label = |value| configured_shell(&launchers, value).map(|shell| shell.label.as_str());
        assert_eq!(launchers[4].id, "wsl:Ubuntu");
        assert_eq!(label("wsl:Debian"), Some("Debian"));
        assert_eq!(label("WSL:debian"), Some("Debian"));
        assert_eq!(label("ubuntu"), Some("Ubuntu"));
        // The program name alone names no distro.
        assert_eq!(label("wsl"), None);
    }

    #[test]
    fn listed_launcher_ids_are_unique() {
        let git_bash = git_bash_launcher(PathBuf::new());
        let mut ids: Vec<&str> = SHELLS
            .iter()
            .map(|&(name, _)| name)
            .chain(AGENTS.iter().map(|&(name, ..)| name))
            .collect();
        ids.push(&git_bash.id);
        let count = ids.len();
        ids.sort_unstable();
        ids.dedup();
        assert_eq!(ids.len(), count);
        assert!(ids.iter().all(|id| !id.starts_with("wsl:")));
    }

    #[test]
    fn automatic_names_the_shell_both_entry_points_start() {
        let launchers = [named("Bash", "/usr/bin/bash"), named("Zsh", "/usr/bin/zsh")];
        let label = |windows, login: Option<&str>| {
            automatic_shell_on(&launchers, windows, login.map(Path::new))
                .map(|shell| shell.label.as_str())
        };
        assert_eq!(label(false, Some("/bin/bash")), Some("Bash"));
        // `$SHELL` isn't listed (or unset): the first tab runs it, Ctrl+Shift+T opens Bash.
        assert_eq!(label(false, Some("/usr/bin/tcsh")), None);
        assert_eq!(label(false, None), None);
        assert_eq!(label(true, None), Some("Bash"));
        let windows = windows_launchers();
        assert_eq!(
            automatic_shell_on(&windows, true, None).map(|shell| shell.id.as_str()),
            Some("git-bash")
        );
    }

    #[test]
    fn menu_lists_the_configured_shell_first() {
        let launchers = [named("Bash", "/usr/bin/bash"), named("Zsh", "/usr/bin/zsh")];
        let (default, shells, _) = menu_sections(&launchers, Some("Zsh"));
        assert_eq!(program(default), Some(Path::new("/usr/bin/zsh")));
        assert_eq!(
            program(shells.first().copied()),
            Some(Path::new("/usr/bin/bash"))
        );
        assert_eq!(shells.len(), 1);
    }

    #[test]
    fn startup_shell_keeps_dollar_shell_outside_windows() {
        let launchers = [named("Bash", "/usr/bin/bash"), named("Zsh", "/usr/bin/zsh")];
        let cwd = Some(PathBuf::from("/work"));
        for setting in [None, Some("tcsh")] {
            let spec = startup_shell_on(&launchers, setting, false, cwd.clone());
            assert_eq!(spec.program, None, "{setting:?}");
            assert_eq!(spec.cwd, cwd);
        }
        let spec = startup_shell_on(&launchers, Some("Zsh"), false, cwd.clone());
        assert_eq!(spec.program.as_deref(), Some("/usr/bin/zsh"));
        assert_eq!(spec.cwd, cwd);
        assert_eq!(spec.kind, SessionKind::Shell);
    }

    #[test]
    fn windows_startup_shell_is_git_bash_unless_set() {
        let launchers = [
            named("PowerShell", "/pwsh.exe"),
            git_bash_launcher("/git/bin/bash.exe".into()),
        ];
        let cwd = Some(PathBuf::from("/work"));
        let spec = startup_shell_on(&launchers, None, true, cwd.clone());
        assert_eq!(spec.program.as_deref(), Some("/git/bin/bash.exe"));
        assert_eq!(spec.args, GIT_BASH_ARGS);
        assert_eq!(spec.cwd, cwd);
        let spec = startup_shell_on(&launchers, Some("pwsh"), true, cwd.clone());
        assert_eq!(spec.program.as_deref(), Some("/pwsh.exe"));
        // No shell found: alacritty's default.
        let spec = startup_shell_on(&[], None, true, cwd.clone());
        assert_eq!(
            (spec.kind, spec.program, spec.cwd),
            (SessionKind::Shell, None, cwd)
        );
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
