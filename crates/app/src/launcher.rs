use std::ffi::OsStr;
use std::path::{Path, PathBuf};
use std::rc::Rc;

use gpui_kit::{App, Global};

use term_core::{LaunchSpec, SessionKind};
#[cfg(any(windows, test))]
use workspace_model::distro_icon;
use workspace_model::file_tree::ShellSyntax;
use workspace_model::{KnownShell, LauncherIcon, Platform, Settings, shell_icon};

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

    /// Current settings apply only to new launches. Saved and duplicated tabs use their spec.
    pub fn configured_spec(
        &self,
        cwd: Option<PathBuf>,
        settings: &Settings,
    ) -> Result<Option<LaunchSpec>, shell_words::ParseError> {
        let Some(mut spec) = self.spec(cwd) else {
            return Ok(None);
        };
        let arguments = match self.kind {
            SessionKind::ClaudeCode => &settings.claude_args,
            SessionKind::CopilotCli => &settings.copilot_args,
            SessionKind::Shell => return Ok(Some(spec)),
        };
        spec.args.extend(shell_words::split(arguments)?);
        Ok(Some(spec))
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
        .or_else(|| preferred_shell(launchers, Platform::CURRENT))
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
    startup_shell_on(launchers, setting, Platform::CURRENT, cwd)
}

fn startup_shell_on(
    launchers: &[Launcher],
    setting: Option<&str>,
    platform: Platform,
    cwd: Option<PathBuf>,
) -> LaunchSpec {
    setting
        .and_then(|name| configured_shell(launchers, name))
        .or_else(|| {
            (platform == Platform::Windows)
                .then(|| preferred_shell(launchers, platform))
                .flatten()
        })
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
    automatic_shell_on(launchers, Platform::CURRENT, login_shell.as_deref())
}

fn automatic_shell_on<'a>(
    launchers: &'a [Launcher],
    platform: Platform,
    login_shell: Option<&Path>,
) -> Option<&'a Launcher> {
    let shell = preferred_shell(launchers, platform)?;
    if platform == Platform::Windows {
        return Some(shell);
    }
    let stem = login_shell?.file_stem()?;
    let program_stem = shell.program.as_deref().and_then(Path::file_stem);
    (program_stem == Some(stem)).then_some(shell)
}

fn preferred_shell(launchers: &[Launcher], platform: Platform) -> Option<&Launcher> {
    let mut installed = installed_shells(launchers);
    let first = installed.clone().next();
    if platform == Platform::Windows {
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

/// The launchers found at start, for views that name shells (the history popup's chips).
pub struct Launchers(pub Rc<[Launcher]>);

impl Global for Launchers {}

/// The shells of the new-tab menu, in its order, for the history popup's chips; none before
/// [`Launchers`] is set (tests).
pub fn menu_shells(cx: &App) -> Vec<KnownShell<'_>> {
    let Some(Launchers(launchers)) = cx.try_global::<Launchers>() else {
        return Vec::new();
    };
    let setting = crate::settings_file::settings(cx).default_shell();
    let (default, shells, _) = menu_sections(launchers, setting);
    default
        .into_iter()
        .chain(shells)
        .filter(|launcher| launcher.kind == SessionKind::Shell)
        .map(|launcher| KnownShell {
            id: &launcher.id,
            label: &launcher.label,
            icon: launcher.icon,
        })
        .collect()
}

/// The shell a session started without a program runs: the user's shell (`$SHELL`), or
/// PowerShell on Windows (alacritty's default).
fn default_program() -> Option<String> {
    if cfg!(windows) {
        Some("powershell".to_string())
    } else {
        std::env::var("SHELL").ok()
    }
}

/// How the program `spec` starts parses typed paths (see [`default_program`] without one).
pub fn shell_syntax(spec: &LaunchSpec) -> ShellSyntax {
    spec.program
        .clone()
        .or_else(default_program)
        .map_or(ShellSyntax::Posix, |program| {
            ShellSyntax::of_program(&program)
        })
}

/// The key a session's commands are kept under in the history: the id of the launcher that
/// runs the same program and arguments, else one derived from them
/// (`workspace_model::shell_key`). Without a program, that of the default shell.
pub fn shell_key(launchers: &[Launcher], spec: &LaunchSpec) -> String {
    shell_key_with(launchers, spec, default_program, cfg!(windows))
}

fn shell_key_with(
    launchers: &[Launcher],
    spec: &LaunchSpec,
    default: impl FnOnce() -> Option<String>,
    windows: bool,
) -> String {
    let program = spec.program.clone().or_else(default);
    let launcher = program.as_deref().and_then(|program| {
        launchers.iter().find(|launcher| {
            launcher.kind == SessionKind::Shell
                && launcher.program.as_deref() == Some(Path::new(program))
                && launcher.args == spec.args
        })
    });
    match launcher {
        Some(launcher) => launcher.id.clone(),
        None => workspace_model::shell_key(program.as_deref(), &spec.args, windows),
    }
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
mod tests;
