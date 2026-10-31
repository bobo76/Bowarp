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
fn configured_agent_arguments_are_shell_words_after_existing_arguments() {
    let mut settings = Settings {
        claude_args: r#"--model "opus" --settings 'my settings.json' --tag "" escaped\ word"#
            .into(),
        copilot_args: "--mode autopilot --yolo".into(),
        ..Settings::default()
    };
    let mut agent = shell("/usr/bin/claude");
    agent.kind = SessionKind::ClaudeCode;
    agent.args = vec!["own-argument".into()];
    let spec = agent
        .configured_spec(Some("/repo".into()), &settings)
        .unwrap()
        .unwrap();
    assert_eq!(
        spec.args,
        [
            "own-argument",
            "--model",
            "opus",
            "--settings",
            "my settings.json",
            "--tag",
            "",
            "escaped word",
        ]
    );
    assert_eq!(spec.cwd, Some("/repo".into()));
    assert_eq!(agent.args, ["own-argument"]);
    settings.claude_args.clear();
    assert_eq!(
        agent
            .configured_spec(None, &settings)
            .unwrap()
            .unwrap()
            .args,
        agent.args
    );
    agent.kind = SessionKind::CopilotCli;
    assert_eq!(
        agent
            .configured_spec(None, &settings)
            .unwrap()
            .unwrap()
            .args,
        ["own-argument", "--mode", "autopilot", "--yolo"]
    );
}

#[test]
fn invalid_agent_arguments_are_reported_but_never_affect_shells() {
    let settings = Settings {
        claude_args: "'unterminated".into(),
        copilot_args: "\"unterminated".into(),
        ..Settings::default()
    };
    let mut launcher = shell("/bin/bash");
    assert!(
        launcher
            .configured_spec(None, &settings)
            .unwrap()
            .unwrap()
            .args
            .is_empty()
    );
    for kind in [SessionKind::ClaudeCode, SessionKind::CopilotCli] {
        launcher.kind = kind;
        assert!(launcher.configured_spec(None, &settings).is_err());
    }
    launcher.program = None;
    assert!(launcher.configured_spec(None, &settings).unwrap().is_none());
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
    assert_eq!(
        program(Platform::Windows),
        Some(Path::new("/git/bin/bash.exe"))
    );
    assert_eq!(program(Platform::Linux), Some(Path::new("/usr/bin/pwsh")));
    assert_eq!(
        preferred_shell(&launchers[..1], Platform::Windows)
            .and_then(|shell| shell.program.as_deref()),
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
    let label = |platform, login: Option<&str>| {
        automatic_shell_on(&launchers, platform, login.map(Path::new))
            .map(|shell| shell.label.as_str())
    };
    assert_eq!(label(Platform::Linux, Some("/bin/bash")), Some("Bash"));
    // `$SHELL` isn't listed (or unset): the first tab runs it, Ctrl+Shift+T opens Bash.
    assert_eq!(label(Platform::Linux, Some("/usr/bin/tcsh")), None);
    assert_eq!(label(Platform::Linux, None), None);
    assert_eq!(label(Platform::Windows, None), Some("Bash"));
    let windows = windows_launchers();
    assert_eq!(
        automatic_shell_on(&windows, Platform::Windows, None).map(|shell| shell.id.as_str()),
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
        let spec = startup_shell_on(&launchers, setting, Platform::Linux, cwd.clone());
        assert_eq!(spec.program, None, "{setting:?}");
        assert_eq!(spec.cwd, cwd);
    }
    let spec = startup_shell_on(&launchers, Some("Zsh"), Platform::Linux, cwd.clone());
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
    let spec = startup_shell_on(&launchers, None, Platform::Windows, cwd.clone());
    assert_eq!(spec.program.as_deref(), Some("/git/bin/bash.exe"));
    assert_eq!(spec.args, GIT_BASH_ARGS);
    assert_eq!(spec.cwd, cwd);
    let spec = startup_shell_on(&launchers, Some("pwsh"), Platform::Windows, cwd.clone());
    assert_eq!(spec.program.as_deref(), Some("/pwsh.exe"));
    // No shell found: alacritty's default.
    let spec = startup_shell_on(&[], None, Platform::Windows, cwd.clone());
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
    let claude = LaunchSpec::agent(
        workspace_model::AgentId::from_cli_selector("claude").unwrap(),
        None,
    );
    assert_eq!(default_name(&claude), "Claude Code");
    let bash = LaunchSpec::new(
        SessionKind::Shell,
        Some("/usr/bin/bash".into()),
        vec![],
        None,
    );
    assert_eq!(default_name(&bash), "bash");
}
