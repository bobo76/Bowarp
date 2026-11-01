use super::*;

#[test]
fn shells_are_recognized_by_program_name() {
    assert_eq!(Shell::of_program("/usr/bin/bash"), Some(Shell::Bash));
    assert_eq!(Shell::of_program("zsh"), Some(Shell::Zsh));
    assert_eq!(
        Shell::of_program(r"C:\Program Files\PowerShell\7\pwsh.exe"),
        Some(Shell::Pwsh)
    );
    assert_eq!(Shell::of_program("powershell.exe"), Some(Shell::Pwsh));
    assert_eq!(Shell::of_program("fish"), None);
    assert_eq!(
        Shell::of_program(r"C:\Windows\System32\cmd.exe"),
        Some(Shell::Cmd)
    );
    assert_eq!(Shell::of_program("wsl.exe"), Some(Shell::Wsl));
}

#[test]
fn cmd_prompt_reports_the_folder_before_the_users_prompt() {
    assert_eq!(
        cmd_prompt("t0k", "$P$G"),
        r"$e]133;D;aw=t0k;path=$P$e\$e]0;$P$e\$P$G"
    );
}

#[test]
fn hidden_cd_is_only_supported_by_zsh_and_powershell() {
    for shell in [Shell::Bash, Shell::Cmd, Shell::Wsl] {
        assert!(!shell.has_hidden_cd());
    }
    for shell in [Shell::Zsh, Shell::Pwsh] {
        assert!(shell.has_hidden_cd());
    }
}

#[test]
fn bash_configuration_preserves_login_startup() {
    let mut spec = LaunchSpec::new(
        SessionKind::Shell,
        Some("bash".into()),
        BASH_LOGIN_ARGS.map(String::from).to_vec(),
        None,
    );
    Shell::Bash.configure(&mut spec, Path::new("scripts"), "token");
    assert_eq!(
        spec.args,
        [
            "--rcfile",
            &Path::new("scripts").join(BASH_FILE).to_string_lossy(),
            "-i"
        ]
    );
    assert_eq!(spec.env.get(BASH_LOGIN_ENV).map(String::as_str), Some("1"));
}

#[test]
fn wsl_gets_the_script_for_a_distro_or_the_default_one() {
    let wsl = |args: &[&str]| {
        LaunchSpec::new(
            SessionKind::Shell,
            Some("wsl.exe".into()),
            args.iter().map(|&arg| arg.into()).collect(),
            None,
        )
    };
    let shell = |spec: &LaunchSpec| target(spec).map(|(shell, _)| shell);
    assert_eq!(shell(&wsl(&["-d", "Ubuntu"])), Some(Shell::Wsl));
    assert_eq!(shell(&wsl(&[])), Some(Shell::Wsl));
    assert_eq!(shell(&wsl(&["-e", "top"])), None);
    assert_eq!(shell(&wsl(&["-d", "Ubuntu", "-e", "top"])), None);
}

#[test]
fn other_launches_are_left_alone() {
    let mut claude = LaunchSpec::agent(
        workspace_model::AgentId::from_cli_selector("claude").unwrap(),
        None,
    );
    assert!(!attach(&mut claude));
    assert!(claude.args.is_empty() && claude.env.is_empty());
    let mut bash_script = LaunchSpec::new(
        SessionKind::Shell,
        Some("bash".into()),
        vec!["build.sh".into()],
        None,
    );
    assert!(!attach(&mut bash_script));
    assert_eq!(bash_script.args, ["build.sh"]);
    let mut fish = LaunchSpec::new(SessionKind::Shell, Some("fish".into()), vec![], None);
    assert!(!attach(&mut fish));
    assert_eq!(fish.program.as_deref(), Some("fish"));
}

#[test]
fn git_bash_and_windows_powershell_get_a_script() {
    let git_bash = LaunchSpec::new(
        SessionKind::Shell,
        Some(r"C:\Program Files\Git\bin\bash.exe".into()),
        vec!["-i".into(), "-l".into()],
        None,
    );
    assert_eq!(target(&git_bash).map(|(shell, _)| shell), Some(Shell::Bash));
    let login_zsh = LaunchSpec::new(
        SessionKind::Shell,
        Some("zsh".into()),
        vec!["-i".into(), "-l".into()],
        None,
    );
    assert_eq!(target(&login_zsh), None);
    let powershell = LaunchSpec::new(
        SessionKind::Shell,
        Some("powershell.exe".into()),
        vec![],
        None,
    );
    assert_eq!(
        target(&powershell).map(|(shell, _)| shell),
        Some(Shell::Pwsh)
    );
}

/// Runs `program` interactively on `input` (no terminal: no line editor) and returns the
/// marks it printed; `None` when the shell is not installed.
#[cfg(unix)]
fn run_shell(
    program: &str,
    args: &[&str],
    env: &[(&str, &Path)],
    input: &str,
) -> Option<(Vec<workspace_model::PromptMark>, String)> {
    use std::io::Write;
    use std::process::{Command, Stdio};

    Command::new(program).arg("-c").arg("exit").status().ok()?;
    // One pipe for both: the prompt goes to stderr, the hooks print to stdout.
    let mut command = Command::new("sh");
    command
        .args(["-c", "exec \"$@\" 2>&1", "sh", program])
        .args(args)
        .env(MARK_TOKEN_ENV, "t0k")
        // With a real terminal type (xterm-256color from the caller's shell), bash's
        // readline loses the prompt when stdin is a pipe, and with it the A and B marks.
        .env("TERM", "dumb")
        // Run inside WSL, the bash script would report Windows folders (`wslpath -w`).
        .env_remove("WSL_DISTRO_NAME")
        .env_remove(ZSH_DIR_ENV)
        .env_remove(USER_ZDOTDIR_ENV)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    for (key, value) in env {
        command.env(key, value);
    }
    let mut child = command.spawn().ok()?;
    child
        .stdin
        .take()
        .unwrap()
        .write_all(input.as_bytes())
        .unwrap();
    let text = child.wait_with_output().unwrap().stdout;
    let mut scanner = workspace_model::Osc133Scanner::new("t0k");
    let mut marks = Vec::new();
    let mut rest = &text[..];
    while let Some((end, mark)) = scanner.scan(rest) {
        marks.push(mark);
        rest = &rest[end..];
    }
    Some((marks, String::from_utf8_lossy(&text).into_owned()))
}

#[cfg(unix)]
fn scratch(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("agentws-{name}-{}", std::process::id()));
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(dir.join("home")).unwrap();
    write_scripts(&dir.join("scripts")).unwrap();
    dir
}

/// What every script must print for `input`: a marked prompt, the command with its C mark,
/// no command for a planted mark, and no token in the command's environment.
#[cfg(unix)]
fn check_marks(marks: &[workspace_model::PromptMark], output: &str) {
    use workspace_model::PromptMark::*;
    let commands: Vec<&str> = marks
        .iter()
        .filter_map(|mark| match mark {
            CommandExecuted(command) => Some(command.as_deref().unwrap_or("")),
            _ => None,
        })
        .collect();
    assert_eq!(
        commands[0], "echo token=${AGENTWS_MARK_TOKEN-unset}",
        "{output}"
    );
    assert!(output.contains("token=unset"), "{output}");
    let start = marks.iter().position(|mark| *mark == PromptStart).unwrap();
    assert_eq!(marks[start + 1], CommandStart, "{marks:?}");
    assert!(
        matches!(marks[start + 2], CommandExecuted(Some(_))),
        "{marks:?}"
    );
    assert!(
        marks.iter().any(
            |mark| matches!(mark, CommandFinished { folder: Some(folder), .. }
                if folder.is_absolute())
        ),
        "{marks:?}"
    );
    // The planted marks have the wrong token; only the printf commands themselves count.
    assert_eq!(commands.len(), 4, "{marks:?}");
    assert!(commands[1].starts_with("printf"), "{marks:?}");
    assert!(
        marks.contains(&AgentStarted(workspace_model::agents::CLAUDE)),
        "{marks:?}"
    );
    assert!(
        marks
            .iter()
            .any(|mark| matches!(mark, CommandFinished { exit: Some(3), .. })),
        "{marks:?}"
    );
}

#[cfg(unix)]
const SCRIPT_INPUT: &str = concat!(
    "echo token=${AGENTWS_MARK_TOKEN-unset}\n",
    "printf '\\033]133;C;aw=bad;cmd=6c73\\007\\033]133;C;cmd=6c73\\007\\n'\n",
    // An agent's mark, printed by hand with the token the script keeps.
    "printf '\\033]133;N;aw=%s;agent=claude\\007' \"$__agentws_token\"\n",
    "(exit 3)\n",
);

#[cfg(unix)]
#[test]
fn bash_script_marks_the_prompt() {
    let dir = scratch("bash");
    let script = dir.join("scripts").join(BASH_FILE);
    let script = script.to_str().unwrap();
    let home = dir.join("home");
    let Some((marks, output)) = run_shell(
        "bash",
        &["--rcfile", script, "-i"],
        &[("HOME", &home)],
        SCRIPT_INPUT,
    ) else {
        return;
    };
    check_marks(&marks, &output);
    fs::remove_dir_all(&dir).unwrap();
}

#[cfg(unix)]
#[test]
fn zsh_script_marks_the_prompt_and_restores_zdotdir() {
    let dir = scratch("zsh");
    let zdotdir = dir.join("scripts").join(ZSH_DIR);
    let home = dir.join("home");
    fs::write(home.join(".zshrc"), "echo user-zshrc=$+ZDOTDIR\n").unwrap();
    let Some((marks, output)) = run_shell(
        "zsh",
        &["-i"],
        &[("HOME", &home), (ZSH_DIR_ENV, &zdotdir)],
        SCRIPT_INPUT,
    ) else {
        return;
    };
    check_marks(&marks, &output);
    // The user's own .zshrc ran, with ZDOTDIR unset again.
    assert!(output.contains("user-zshrc=0"), "{output}");
    let written: Vec<_> = fs::read_dir(&zdotdir)
        .unwrap()
        .map(|entry| entry.unwrap().file_name())
        .collect();
    assert_eq!(written, [".zshenv"]);
    fs::remove_dir_all(&dir).unwrap();
}

/// Runs `command` (a `sh -c` line) in a terminal (`script` from util-linux) typing `input`,
/// and returns the marks and the output; `None` without `script` or `shell`.
#[cfg(unix)]
fn run_in_terminal(
    shell: &str,
    command: &str,
    env: &[(&str, &Path)],
    input: &str,
) -> Option<(Vec<workspace_model::PromptMark>, String)> {
    use std::io::Write;
    use std::process::{Command, Stdio};

    Command::new(shell).arg("-c").arg("exit").status().ok()?;
    Command::new("script").arg("-V").output().ok()?;
    let mut command_line = Command::new("script");
    command_line
        .args(["-qec", command, "/dev/null"])
        .env("SHELL", "/bin/sh")
        .env(MARK_TOKEN_ENV, "t0k")
        .env("TERM", "dumb")
        .env("AGENTWS_CLAUDE_SETTINGS", "/hooks/settings.json")
        .env("AGENTWS_COPILOT_PLUGIN", "/hooks/plugin")
        .env_remove("WSL_DISTRO_NAME")
        .env_remove(ZSH_DIR_ENV)
        .env_remove(USER_ZDOTDIR_ENV)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    for (key, value) in env {
        command_line.env(key, value);
    }
    let mut child = command_line.spawn().ok()?;
    child
        .stdin
        .take()
        .unwrap()
        .write_all(input.as_bytes())
        .unwrap();
    let text = child.wait_with_output().unwrap().stdout;
    let mut scanner = workspace_model::Osc133Scanner::new("t0k");
    let mut marks = Vec::new();
    let mut rest = &text[..];
    while let Some((end, mark)) = scanner.scan(rest) {
        marks.push(mark);
        rest = &rest[end..];
    }
    Some((marks, String::from_utf8_lossy(&text).into_owned()))
}

/// A folder with stand-ins for `claude` and `copilot` that print their arguments and
/// `ConEmuANSI`, then exit with status 3, and `PATH` with it first.
#[cfg(unix)]
fn fake_agents(dir: &Path) -> (PathBuf, std::ffi::OsString) {
    use std::os::unix::fs::PermissionsExt;

    let bin = dir.join("bin");
    fs::create_dir_all(&bin).unwrap();
    for agent in ["claude", "copilot"] {
        let path = bin.join(agent);
        fs::write(
            &path,
            format!("#!/bin/sh\necho \"{agent}-argv:$*|conemu:${{ConEmuANSI-unset}}\"\nexit 3\n"),
        )
        .unwrap();
        fs::set_permissions(&path, fs::Permissions::from_mode(0o755)).unwrap();
    }
    let mut path = bin.clone().into_os_string();
    path.push(":/usr/bin:/bin");
    (bin, path)
}

#[cfg(unix)]
const AGENT_INPUT: &str = concat!(
    "claude --resume x; echo after=$?\n",
    "copilot\n",
    // One-shot runs and the escapes run the program unchanged.
    "claude -p hi\n",
    "copilot --version\n",
    "claude mcp list\n",
    "claude | cat\n",
    "\\claude a\n",
    "command claude b\n",
    "unset AGENTWS_CLAUDE_SETTINGS; claude c\n",
    // Not on PATH: the shell's own message.
    "PATH=/nonexistent; copilot d\n",
    "exit\n",
);

/// What the agent functions must do for [`AGENT_INPUT`]: marks and hooks around the two
/// interactive runs only.
#[cfg(unix)]
fn check_agent_runs(marks: &[workspace_model::PromptMark], output: &str) {
    use workspace_model::PromptMark::*;
    use workspace_model::agents::{CLAUDE, COPILOT};
    let agent_marks: Vec<_> = marks
        .iter()
        .filter(|mark| matches!(mark, AgentStarted(_) | AgentFinished(_)))
        .collect();
    assert_eq!(
        agent_marks,
        [
            &AgentStarted(CLAUDE),
            &AgentFinished(Some(3)),
            &AgentStarted(COPILOT),
            &AgentFinished(Some(3))
        ],
        "{output}"
    );
    for expected in [
        "claude-argv:--settings /hooks/settings.json --resume x|conemu:unset",
        "after=3",
        "copilot-argv:--plugin-dir /hooks/plugin|conemu:ON",
        "claude-argv:-p hi|",
        "copilot-argv:--version|",
        "claude-argv:mcp list|",
        "claude-argv:|",
        "claude-argv:a|",
        "claude-argv:b|",
        "claude-argv:c|",
        "not found",
    ] {
        assert!(output.contains(expected), "{expected}: {output}");
    }
}

/// The user's own `claude` alias and `copilot` function, from their rc file.
#[cfg(unix)]
const USER_AGENTS: &str = "alias claude='echo user-claude'\ncopilot() { echo user-copilot; }\n";

#[cfg(unix)]
const USER_AGENT_INPUT: &str = "claude\ncopilot\nexit\n";

#[cfg(unix)]
fn check_user_agents_kept(marks: &[workspace_model::PromptMark], output: &str) {
    use workspace_model::PromptMark::*;
    assert!(output.contains("user-claude"), "{output}");
    assert!(output.contains("user-copilot"), "{output}");
    assert!(
        !marks
            .iter()
            .any(|mark| matches!(mark, AgentStarted(_) | AgentFinished(_))),
        "{marks:?}"
    );
}

/// User aliases that start with the agent (wrapped), one with `command`.
#[cfg(unix)]
const USER_WRAPPED: &str =
    "alias claude='claude --user-flag'\nalias copilot='command copilot --banner'\n";

#[cfg(unix)]
const USER_WRAPPED_INPUT: &str = "claude x\ncopilot\nexit\n";

#[cfg(unix)]
fn check_user_aliases_wrapped(marks: &[workspace_model::PromptMark], output: &str) {
    use workspace_model::PromptMark::*;
    use workspace_model::agents::{CLAUDE, COPILOT};
    let agent_marks: Vec<_> = marks
        .iter()
        .filter(|mark| matches!(mark, AgentStarted(_) | AgentFinished(_)))
        .collect();
    assert_eq!(
        agent_marks,
        [
            &AgentStarted(CLAUDE),
            &AgentFinished(Some(3)),
            &AgentStarted(COPILOT),
            &AgentFinished(Some(3))
        ],
        "{output}"
    );
    for expected in [
        "claude-argv:--settings /hooks/settings.json --user-flag x|",
        "copilot-argv:--plugin-dir /hooks/plugin --banner|conemu:ON",
    ] {
        assert!(output.contains(expected), "{expected}: {output}");
    }
}

/// A user `--settings` of Claude (typed, or in an alias) runs unwrapped, without marks.
#[cfg(unix)]
const USER_SETTINGS: &str = "alias claude='claude --settings x.json'\n";

#[cfg(unix)]
const USER_SETTINGS_INPUT: &str = "claude y\nexit\n";

#[cfg(unix)]
const TYPED_SETTINGS_INPUT: &str = "claude --settings z.json\nclaude --settings=w.json\nexit\n";

#[cfg(unix)]
fn check_no_agent_marks(marks: &[workspace_model::PromptMark], output: &str, expected: &[&str]) {
    use workspace_model::PromptMark::*;
    assert!(
        !marks
            .iter()
            .any(|mark| matches!(mark, AgentStarted(_) | AgentFinished(_))),
        "{marks:?}"
    );
    for expected in expected {
        assert!(output.contains(expected), "{expected}: {output}");
    }
}

#[cfg(unix)]
#[test]
fn bash_agent_functions_run_interactive_agents_in_place() {
    let dir = scratch("bash-agents");
    let script = dir.join("scripts").join(BASH_FILE);
    let command = format!("bash --rcfile '{}' -i", script.display());
    let home = dir.join("home");
    let (_bin, path) = fake_agents(&dir);
    let path = Path::new(&path);
    let env = [("HOME", home.as_path()), ("PATH", path)];
    let Some((marks, output)) = run_in_terminal("bash", &command, &env, AGENT_INPUT) else {
        return;
    };
    check_agent_runs(&marks, &output);
    fs::write(home.join(".bashrc"), USER_AGENTS).unwrap();
    let (marks, output) = run_in_terminal("bash", &command, &env, USER_AGENT_INPUT).unwrap();
    check_user_agents_kept(&marks, &output);
    fs::write(home.join(".bashrc"), USER_WRAPPED).unwrap();
    let (marks, output) = run_in_terminal("bash", &command, &env, USER_WRAPPED_INPUT).unwrap();
    check_user_aliases_wrapped(&marks, &output);
    fs::write(home.join(".bashrc"), USER_SETTINGS).unwrap();
    let (marks, output) = run_in_terminal("bash", &command, &env, USER_SETTINGS_INPUT).unwrap();
    check_no_agent_marks(&marks, &output, &["claude-argv:--settings x.json y|"]);
    fs::write(home.join(".bashrc"), "").unwrap();
    let (marks, output) = run_in_terminal("bash", &command, &env, TYPED_SETTINGS_INPUT).unwrap();
    check_no_agent_marks(
        &marks,
        &output,
        &[
            "claude-argv:--settings z.json|",
            "claude-argv:--settings=w.json|",
        ],
    );
    fs::remove_dir_all(&dir).unwrap();
}

#[cfg(unix)]
#[test]
fn zsh_agent_functions_run_interactive_agents_in_place() {
    let dir = scratch("zsh-agents");
    let zdotdir = dir.join("scripts").join(ZSH_DIR);
    let home = dir.join("home");
    // Skip the system rc files: a distro's /etc/zsh/zshrc may run compinit, whose
    // "insecure directories" question would swallow the first typed line.
    fs::write(home.join(".zshenv"), "unsetopt GLOBAL_RCS\n").unwrap();
    let (_bin, path) = fake_agents(&dir);
    let path = Path::new(&path);
    let env = [
        ("HOME", home.as_path()),
        ("PATH", path),
        (ZSH_DIR_ENV, zdotdir.as_path()),
    ];
    let Some((marks, output)) = run_in_terminal("zsh", "zsh -i", &env, AGENT_INPUT) else {
        return;
    };
    check_agent_runs(&marks, &output);
    fs::write(home.join(".zshrc"), USER_AGENTS).unwrap();
    let (marks, output) = run_in_terminal("zsh", "zsh -i", &env, USER_AGENT_INPUT).unwrap();
    check_user_agents_kept(&marks, &output);
    fs::write(home.join(".zshrc"), USER_WRAPPED).unwrap();
    let (marks, output) = run_in_terminal("zsh", "zsh -i", &env, USER_WRAPPED_INPUT).unwrap();
    check_user_aliases_wrapped(&marks, &output);
    fs::write(home.join(".zshrc"), USER_SETTINGS).unwrap();
    let (marks, output) = run_in_terminal("zsh", "zsh -i", &env, USER_SETTINGS_INPUT).unwrap();
    check_no_agent_marks(&marks, &output, &["claude-argv:--settings x.json y|"]);
    fs::write(home.join(".zshrc"), "").unwrap();
    let (marks, output) = run_in_terminal("zsh", "zsh -i", &env, TYPED_SETTINGS_INPUT).unwrap();
    check_no_agent_marks(
        &marks,
        &output,
        &[
            "claude-argv:--settings z.json|",
            "claude-argv:--settings=w.json|",
        ],
    );
    fs::remove_dir_all(&dir).unwrap();
}

#[test]
fn powershell_paths_are_single_quoted() {
    assert_eq!(powershell_quote(r"C:\a b\x.ps1"), r"'C:\a b\x.ps1'");
    assert_eq!(powershell_quote("it's"), "'it''s'");
    assert_eq!(
        powershell_load(r"C:\x.ps1"),
        r". ([ScriptBlock]::Create([IO.File]::ReadAllText('C:\x.ps1')))"
    );
}

#[test]
fn powershell_script_keeps_windows_powershell_syntax_and_reports_exit_status() {
    let code: Vec<&str> = PWSH_SCRIPT
        .lines()
        .filter(|line| !line.trim_start().starts_with('#'))
        .collect();
    for operator in ["&&", "||", "??"] {
        assert!(
            !code.iter().any(|line| line.contains(operator)),
            "{operator} needs PowerShell 7"
        );
    }
    assert!(PWSH_SCRIPT.contains("133;D;aw=$global:__agentws_token;exit=$exit"));
    assert!(PWSH_SCRIPT.contains("function global:claude"));
    assert!(PWSH_SCRIPT.contains("function global:copilot"));
}

/// Stand-ins for `claude` and `copilot` as PowerShell finds them, printing their arguments and
/// `ConEmuANSI` and exiting with status 3: [`fake_agents`]'s scripts, batch files on Windows.
/// Returns their folder.
fn fake_powershell_agents(dir: &Path) -> PathBuf {
    #[cfg(unix)]
    {
        fake_agents(dir).0
    }
    #[cfg(windows)]
    {
        let bin = dir.join("bin");
        fs::create_dir_all(&bin).unwrap();
        for agent in ["claude", "copilot"] {
            fs::write(
                bin.join(format!("{agent}.cmd")),
                format!(
                    "@echo off\r\n\
                     if defined ConEmuANSI (set \"c=%ConEmuANSI%\") else (set \"c=unset\")\r\n\
                     echo {agent}-argv:%*^|conemu:%c%\r\n\
                     exit /b 3\r\n"
                ),
            )
            .unwrap();
        }
        bin
    }
}

/// Runs `test` (PowerShell) after loading the script the way a tab does, with PSReadLine
/// stood in for (a console host is needed for the real one), the agents of
/// [`fake_powershell_agents`] first on PATH, the hook variables set, and stdin and stdout
/// taken for the console. `user` runs before the script, as the profile would. Returns the
/// marks and the output (stdout, then stderr); `None` when `program` is not installed.
fn run_powershell(
    program: &str,
    name: &str,
    user: &str,
    test: &str,
) -> Option<(Vec<workspace_model::PromptMark>, String)> {
    use std::process::Command;

    let found = Command::new(program)
        .args(["-NoProfile", "-NonInteractive", "-Command", "exit"])
        .output()
        .is_ok();
    let required = cfg!(windows) && std::env::var_os("CI").is_some();
    assert!(found || !required, "{program} is missing on the Windows CI");
    if !found {
        return None;
    }
    let unique = format!("agentws-{name}-{program}-{}", std::process::id());
    let dir = std::env::temp_dir().join(unique);
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(&dir).unwrap();
    let scripts = dir.join("scripts");
    write_scripts(&scripts).unwrap();
    let bin = fake_powershell_agents(&dir);
    let script = scripts.join(PWSH_FILE);
    let test_file = dir.join("test.ps1");
    fs::write(
        &test_file,
        format!(
            "$env:PATH = {bin} + [IO.Path]::PathSeparator + $env:PATH\n\
             Remove-Item Env:ConEmuANSI -ErrorAction SilentlyContinue\n\
             $env:AGENTWS_CLAUDE_SETTINGS = 'hooks-settings'\n\
             $env:AGENTWS_COPILOT_PLUGIN = 'hooks-plugin'\n\
             function Get-Module {{ 'PSReadLine' }}\n\
             function Set-PSReadLineKeyHandler {{ }}\n\
             {user}\n\
             {load}\n\
             function global:__agentws_on_terminal {{ $true }}\n\
             {test}\n",
            bin = powershell_quote(&bin.to_string_lossy()),
            load = powershell_load(&script.to_string_lossy()),
        ),
    )
    .unwrap();
    let output = Command::new(program)
        .args([
            "-NoProfile",
            "-NonInteractive",
            "-ExecutionPolicy",
            "Bypass",
        ])
        .arg("-File")
        .arg(&test_file)
        .env(MARK_TOKEN_ENV, "t0k")
        .output()
        .unwrap();
    let mut scanner = workspace_model::Osc133Scanner::new("t0k");
    let mut marks = Vec::new();
    let mut rest = &output.stdout[..];
    while let Some((end, mark)) = scanner.scan(rest) {
        marks.push(mark);
        rest = &rest[end..];
    }
    let text = format!(
        "{}{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    fs::remove_dir_all(&dir).unwrap();
    Some((marks, text))
}

/// The PowerShells the tests run under: PowerShell 7 and, on Windows, Windows PowerShell 5.1.
const POWERSHELLS: &[&str] = if cfg!(windows) {
    &["pwsh", "powershell"]
} else {
    &["pwsh"]
};

const POWERSHELL_AGENT_INPUT: &str = concat!(
    "claude --resume x; \"after=$LASTEXITCODE\"\n",
    "copilot\n",
    "\"conemu-after=$env:ConEmuANSI|\"\n",
    // One-shot runs, pipes and the escape run the program unchanged.
    "claude -p hi\n",
    "copilot --version\n",
    "claude mcp list\n",
    "claude --settings z.json\n",
    "claude | Out-String\n",
    "'in' | claude a\n",
    "& (Get-Command claude -CommandType Application -TotalCount 1) b\n",
    "Remove-Item Env:AGENTWS_CLAUDE_SETTINGS; claude c\n",
    // The D mark's exit status: a program's, a success, a cmdlet's failure.
    "& (Get-Command claude -CommandType Application -TotalCount 1) d\n",
    "$p = prompt; [Console]::Write($p)\n",
    "$null = 1; $p = prompt; [Console]::Write($p)\n",
    "$global:LASTEXITCODE = 0\n",
    "Get-Item -LiteralPath 'agentws-missing' -ErrorAction SilentlyContinue\n",
    "$p = prompt; [Console]::Write($p)\n",
    // Not on PATH: PowerShell's own error.
    "$env:PATH = 'agentws-nonexistent'; copilot e\n",
);

#[test]
fn powershell_agent_functions_run_interactive_agents_in_place() {
    use workspace_model::PromptMark::*;
    use workspace_model::agents::{CLAUDE, COPILOT};

    for program in POWERSHELLS {
        let Some((marks, output)) =
            run_powershell(program, "pwsh-agents", "", POWERSHELL_AGENT_INPUT)
        else {
            continue;
        };
        let agent_marks: Vec<_> = marks
            .iter()
            .filter(|mark| matches!(mark, AgentStarted(_) | AgentFinished(_)))
            .collect();
        assert_eq!(
            agent_marks,
            [
                &AgentStarted(CLAUDE),
                &AgentFinished(Some(3)),
                &AgentStarted(COPILOT),
                &AgentFinished(Some(3))
            ],
            "{program}: {output}"
        );
        for expected in [
            "claude-argv:--settings hooks-settings --resume x|conemu:unset",
            "after=3",
            "copilot-argv:--plugin-dir hooks-plugin|conemu:ON",
            "conemu-after=|",
            "claude-argv:-p hi|",
            "copilot-argv:--version|",
            "claude-argv:mcp list|",
            "claude-argv:--settings z.json|",
            "claude-argv:|",
            "claude-argv:a|",
            "claude-argv:b|",
            "claude-argv:c|",
            "claude-argv:d|",
            "not recognized",
        ] {
            assert!(output.contains(expected), "{program}: {expected}: {output}");
        }
        let exits: Vec<_> = marks
            .iter()
            .filter_map(|mark| match mark {
                CommandFinished { exit, .. } => Some(*exit),
                _ => None,
            })
            .collect();
        assert_eq!(exits, [Some(3), Some(0), Some(1)], "{program}: {output}");
    }
}

#[test]
fn powershell_agent_functions_keep_the_users_own() {
    let user = concat!(
        "function global:claude { 'user-claude' }\n",
        "function global:__user_copilot { 'user-copilot' }\n",
        "Set-Alias -Name copilot -Value __user_copilot -Scope Global\n",
    );
    for program in POWERSHELLS {
        let Some((marks, output)) =
            run_powershell(program, "pwsh-user-agents", user, "claude\ncopilot\n")
        else {
            continue;
        };
        assert!(output.contains("user-claude"), "{program}: {output}");
        assert!(output.contains("user-copilot"), "{program}: {output}");
        assert!(
            !marks.iter().any(|mark| matches!(
                mark,
                workspace_model::PromptMark::AgentStarted(_)
                    | workspace_model::PromptMark::AgentFinished(_)
            )),
            "{program}: {marks:?}"
        );
    }
}

#[test]
fn powershell_agent_functions_wrap_an_alias_of_the_agent() {
    use workspace_model::PromptMark::*;
    use workspace_model::agents::CLAUDE;

    let user = concat!(
        "$target = (Get-Command claude -CommandType Application -TotalCount 1).Source\n",
        "Set-Alias -Name claude -Value $target -Scope Global\n",
    );
    // The alias's target is run, not a PATH lookup.
    let test = "$env:PATH = 'agentws-nonexistent'; claude x\n";
    for program in POWERSHELLS {
        let Some((marks, output)) = run_powershell(program, "pwsh-alias-agent", user, test) else {
            continue;
        };
        let expected = "claude-argv:--settings hooks-settings x|";
        assert!(output.contains(expected), "{program}: {output}");
        assert!(
            marks.contains(&AgentStarted(CLAUDE)),
            "{program}: {marks:?}"
        );
        assert!(
            marks.contains(&AgentFinished(Some(3))),
            "{program}: {marks:?}"
        );
    }
}

#[test]
fn powershell_prompt_gives_the_users_prompt_the_last_status() {
    let user = "function global:prompt { \"user-ok=$global:?|code=$global:LASTEXITCODE|\" }\n";
    let test = concat!(
        "& (Get-Command claude -CommandType Application -TotalCount 1) d\n",
        "$p = prompt; [Console]::Write($p)\n",
        "$null = 1; $p = prompt; [Console]::Write($p)\n",
    );
    for program in POWERSHELLS {
        let Some((_, output)) = run_powershell(program, "pwsh-user-prompt", user, test) else {
            continue;
        };
        assert!(
            output.contains("user-ok=False|code=3|"),
            "{program}: {output}"
        );
        assert!(
            output.contains("user-ok=True|code=3|"),
            "{program}: {output}"
        );
    }
}
