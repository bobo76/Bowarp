use super::*;
use workspace_model::agents::{CLAUDE, COPILOT};

#[cfg(unix)]
mod unix;

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

#[cfg(windows)]
#[test]
fn finds_git_bash_from_cmd_and_mingw_git() {
    use std::fs;

    use super::windows::git_bash_near;

    let root = std::env::temp_dir().join(format!("agentws-git-{}", std::process::id()));
    let _ = fs::remove_dir_all(&root);
    fs::create_dir_all(root.join("bin")).unwrap();
    fs::write(root.join(r"bin\bash.exe"), "").unwrap();

    let bash = Some(root.join(r"bin\bash.exe"));
    assert_eq!(git_bash_near(&root.join(r"cmd\git.exe")), bash);
    assert_eq!(git_bash_near(&root.join(r"mingw64\bin\git.exe")), bash);
    fs::remove_dir_all(root).unwrap();
}

#[test]
fn quoted_or_escaped_windows_paths_survive_agent_argument_parsing() {
    for arguments in [
        r"--settings 'C:\Users\my name\settings.json'",
        r#"--settings C:\\Users\\my\ name\\settings.json"#,
    ] {
        let settings = Settings {
            claude_args: arguments.into(),
            copilot_args: arguments.into(),
            ..Settings::default()
        };
        for kind in [SessionKind::Agent(CLAUDE), SessionKind::Agent(COPILOT)] {
            let launcher = Launcher {
                id: "agent".into(),
                label: "Agent".into(),
                kind,
                program: Some("agent".into()),
                args: Vec::new(),
                icon: LauncherIcon::Agent(COPILOT),
            };
            let spec = launcher.configured_spec(None, &settings).unwrap().unwrap();
            assert_eq!(
                spec.args,
                ["--settings", r"C:\Users\my name\settings.json"],
                "{kind:?}: {arguments}"
            );
        }
    }
}

#[test]
fn history_shell_key_prefers_the_matching_launcher() {
    let git_bash = git_bash_launcher(PathBuf::from(r"D:\Tools\PortableGit\bin\bash.exe"));
    let ubuntu = wsl_launcher(Path::new(r"C:\Windows\System32\wsl.exe"), "Ubuntu");
    let launchers = [git_bash.clone(), ubuntu.clone()];
    let none = || None;
    let key = |spec: &LaunchSpec| shell_key_with(&launchers, spec, none, true);
    assert_eq!(key(&git_bash.spec(None).unwrap()), "git-bash");
    assert_eq!(key(&ubuntu.spec(None).unwrap()), "wsl:Ubuntu");
    // The same bash without Git Bash's arguments is not that launcher: no `Git` folder here.
    let mut plain = git_bash.spec(None).unwrap();
    plain.args.clear();
    assert_eq!(key(&plain), "bash");
    // A restored tab whose distro is no longer listed.
    let mut debian = ubuntu.spec(None).unwrap();
    debian.args = vec!["-d".into(), "Debian".into()];
    assert_eq!(key(&debian), "wsl:Debian");
    // No program: the default shell, by the same rules.
    let spec = LaunchSpec::shell(None);
    let pwsh = || Some(r"C:\Program Files\PowerShell\7\pwsh.exe".to_string());
    assert_eq!(shell_key_with(&launchers, &spec, pwsh, true), "pwsh");
    assert_eq!(shell_key_with(&launchers, &spec, none, false), "shell");
}
