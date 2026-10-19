//! Shell integration: bash, zsh and PowerShell tabs start with a small script that loads the
//! user's own startup files and then marks the prompt with OSC 133 (`term_core::PromptInput`),
//! which the command history, the Running status and (off Linux) the shell's folder need. Each
//! session gets a random token in `AGENTWS_MARK_TOKEN`, which the script reads (and removes
//! from the environment) and puts in every mark, so marks printed by programs are ignored. The
//! scripts are written to `shell-integration/` in the config folder at the first launch;
//! nothing is written to the user's own files. cmd.exe has no hooks: its `PROMPT` reports the
//! folder (`D` with `path=`) and puts it in the title. A WSL distro whose login shell is bash
//! gets the bash script (its token passed through `WSLENV`). fish, other WSL shells and shells
//! started with arguments (except Git Bash's login shell and `wsl -d <distro>`) run unchanged.

use std::fs;
use std::io;
use std::path::{Path, PathBuf};
use std::sync::OnceLock;

use term_core::{LaunchSpec, SessionKind};
use workspace_model::new_mark_token;

use crate::launcher::WSL_DISTRO_FLAG;
use crate::window_bounds::config_file;

const BASH_SCRIPT: &str = include_str!("../shell-integration/agentws.bash");
const ZSH_ENV: &str = include_str!("../shell-integration/zsh/.zshenv");
const PWSH_SCRIPT: &str = include_str!("../shell-integration/agentws.ps1");

const BASH_FILE: &str = "agentws.bash";
const ZSH_DIR: &str = "zsh";
const PWSH_FILE: &str = "agentws.ps1";
/// zsh reads its startup files from `$ZDOTDIR`.
const ZSH_DIR_ENV: &str = "ZDOTDIR";
/// The user's own `ZDOTDIR`, for the injected `.zshenv` (unset: `$HOME`).
const USER_ZDOTDIR_ENV: &str = "AGENTWS_USER_ZDOTDIR";
const MARK_TOKEN_ENV: &str = "AGENTWS_MARK_TOKEN";
/// Arguments of the one launch that has some and still gets a script: Git Bash's login shell.
const BASH_LOGIN_ARGS: [&str; 2] = ["-i", "-l"];
/// Tells the bash script to load the login files, since a login shell ignores `--rcfile`.
const BASH_LOGIN_ENV: &str = "AGENTWS_BASH_LOGIN";
/// cmd.exe's prompt, read from this variable; `$P$G` (folder and `>`) when unset.
const CMD_PROMPT_ENV: &str = "PROMPT";
const CMD_DEFAULT_PROMPT: &str = "$P$G";
/// The variables Windows hands to WSL processes.
const WSL_ENV: &str = "WSLENV";
/// Run inside the distro with the bash script's Windows path as `$1`: bash, as the login shell
/// it would have been, loading the script; any other login shell unchanged.
const WSL_LAUNCH: &str = concat!(
    r#"shell=$(getent passwd "$(id -un)" | cut -d: -f7); "#,
    r#"case "${shell##*/}" in bash) "#,
    r#"AGENTWS_BASH_LOGIN=1 exec "$shell" --rcfile "$(wslpath -u "$1")" -i;; esac; "#,
    r#"unset AGENTWS_MARK_TOKEN; exec "${shell:-/bin/sh}" -l"#,
);
/// Files earlier versions wrote, and the completion cache zsh wrote next to them.
const STALE_FILES: [&str; 3] = ["zsh/.zprofile", "zsh/.zshrc", "zsh/.zcompdump"];

/// The shells that get a script.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Shell {
    Bash,
    Zsh,
    Pwsh,
    Cmd,
    Wsl,
}

impl Shell {
    fn of_program(program: &str) -> Option<Self> {
        // Either separator: Windows paths are recognized on any platform.
        let name = program.rsplit(['/', '\\']).next()?.to_ascii_lowercase();
        match name.strip_suffix(".exe").unwrap_or(&name) {
            "bash" => Some(Self::Bash),
            "zsh" => Some(Self::Zsh),
            "pwsh" | "powershell" => Some(Self::Pwsh),
            "cmd" => Some(Self::Cmd),
            "wsl" => Some(Self::Wsl),
            _ => None,
        }
    }
}

/// Make a shell launch load the integration script. Other launches, shells started with their
/// own arguments, and launches when the scripts could not be written are left unchanged;
/// returns whether the script was attached.
pub fn attach(spec: &mut LaunchSpec) -> bool {
    let Some((shell, program)) = target(spec) else {
        return false;
    };
    let Some(dir) = scripts_dir() else {
        return false;
    };
    let token = new_mark_token();
    match shell {
        Shell::Bash => {
            let login = !spec.args.is_empty();
            spec.args = vec![
                "--rcfile".into(),
                dir.join(BASH_FILE).to_string_lossy().into_owned(),
            ];
            if login {
                spec.args.push("-i".into());
                spec.env.insert(BASH_LOGIN_ENV.into(), "1".into());
            }
        }
        Shell::Zsh => {
            if let Ok(user_zdotdir) = std::env::var(ZSH_DIR_ENV) {
                spec.env.insert(USER_ZDOTDIR_ENV.into(), user_zdotdir);
            }
            spec.env.insert(
                ZSH_DIR_ENV.into(),
                dir.join(ZSH_DIR).to_string_lossy().into_owned(),
            );
        }
        Shell::Pwsh => {
            let script = dir.join(PWSH_FILE).to_string_lossy().into_owned();
            spec.args = vec![
                "-NoExit".into(),
                "-Command".into(),
                powershell_load(&script),
            ];
        }
        Shell::Cmd => {
            let prompt = std::env::var(CMD_PROMPT_ENV)
                .ok()
                .filter(|prompt| !prompt.is_empty());
            spec.env.insert(
                CMD_PROMPT_ENV.into(),
                cmd_prompt(&token, prompt.as_deref().unwrap_or(CMD_DEFAULT_PROMPT)),
            );
        }
        Shell::Wsl => {
            let script = dir.join(BASH_FILE).to_string_lossy().into_owned();
            spec.args
                .extend(["-e", "sh", "-c", WSL_LAUNCH, "sh", script.as_str()].map(String::from));
            let wsl_env = match std::env::var(WSL_ENV) {
                Ok(names) if !names.is_empty() => format!("{names}:{MARK_TOKEN_ENV}"),
                _ => MARK_TOKEN_ENV.into(),
            };
            spec.env.insert(WSL_ENV.into(), wsl_env);
        }
    }
    spec.env.insert(MARK_TOKEN_ENV.into(), token.clone());
    spec.mark_token = Some(token);
    spec.program = Some(program);
    true
}

/// The shell a launch runs and its program, when it gets a script.
fn target(spec: &LaunchSpec) -> Option<(Shell, String)> {
    if spec.kind != SessionKind::Shell {
        return None;
    }
    // Without a program alacritty starts the login shell (`$SHELL` here) on Unix, PowerShell
    // on Windows.
    let program = match &spec.program {
        Some(program) => program.clone(),
        None if cfg!(unix) => std::env::var("SHELL")
            .ok()
            .filter(|shell| !shell.is_empty())?,
        None => return None,
    };
    let shell = Shell::of_program(&program)?;
    let plain_args = spec.args.is_empty()
        || match shell {
            Shell::Bash => spec.args == BASH_LOGIN_ARGS,
            Shell::Wsl => spec.args.len() == 2 && spec.args[0] == WSL_DISTRO_FLAG,
            _ => false,
        };
    plain_args.then_some((shell, program))
}

/// cmd.exe's `PROMPT`: a `D` mark with the folder (`$P`), the folder as the title, then
/// `prompt` itself. `$e` is ESC, and `$e\` ends a sequence.
fn cmd_prompt(token: &str, prompt: &str) -> String {
    format!(r"$e]133;D;aw={token};path=$P$e\$e]0;$P$e\{prompt}")
}

/// The `-Command` that loads `script`. Its text runs as a script block rather than as a
/// script file, which the default execution policy of Windows PowerShell (Restricted) blocks.
fn powershell_load(script: &str) -> String {
    format!(
        ". ([ScriptBlock]::Create([IO.File]::ReadAllText({})))",
        powershell_quote(script)
    )
}

/// `text` as a PowerShell single-quoted string.
fn powershell_quote(text: &str) -> String {
    format!("'{}'", text.replace('\'', "''"))
}

/// The folder holding the scripts, written once per run; `None` (no integration) when it
/// cannot be written.
fn scripts_dir() -> Option<&'static Path> {
    static DIR: OnceLock<Option<PathBuf>> = OnceLock::new();
    DIR.get_or_init(|| {
        let dir = config_file("shell-integration")?;
        match write_scripts(&dir) {
            Ok(()) => Some(dir),
            Err(err) => {
                eprintln!(
                    "agent-workspace: shell integration disabled, cannot write {}: {err}",
                    dir.display()
                );
                None
            }
        }
    })
    .as_deref()
}

fn write_scripts(dir: &Path) -> io::Result<()> {
    let files = [
        (BASH_FILE, BASH_SCRIPT),
        ("zsh/.zshenv", ZSH_ENV),
        (PWSH_FILE, PWSH_SCRIPT),
    ];
    fs::create_dir_all(dir.join(ZSH_DIR))?;
    for name in STALE_FILES {
        let _ = fs::remove_file(dir.join(name));
    }
    for (name, contents) in files {
        let path = dir.join(name);
        // Leave an up-to-date file alone: a running shell may be reading it.
        if fs::read_to_string(&path).is_ok_and(|current| current == contents) {
            continue;
        }
        fs::write(&path, contents)?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
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
        let mut claude = LaunchSpec::claude_code(None);
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
            marks
                .iter()
                .any(|mark| matches!(mark, CommandFinished(Some(folder)) if folder.is_absolute())),
            "{marks:?}"
        );
        // The planted marks have the wrong token; only the printf command itself counts.
        assert_eq!(commands.len(), 2, "{marks:?}");
        assert!(commands[1].starts_with("printf"), "{marks:?}");
    }

    #[cfg(unix)]
    const SCRIPT_INPUT: &str = concat!(
        "echo token=${AGENTWS_MARK_TOKEN-unset}\n",
        "printf '\\033]133;C;aw=bad;cmd=6c73\\007\\033]133;C;cmd=6c73\\007\\n'\n",
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

    #[test]
    fn powershell_paths_are_single_quoted() {
        assert_eq!(powershell_quote(r"C:\a b\x.ps1"), r"'C:\a b\x.ps1'");
        assert_eq!(powershell_quote("it's"), "'it''s'");
        assert_eq!(
            powershell_load(r"C:\x.ps1"),
            r". ([ScriptBlock]::Create([IO.File]::ReadAllText('C:\x.ps1')))"
        );
    }
}
