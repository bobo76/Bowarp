//! Shell integration: bash, zsh and PowerShell tabs start with a small script that loads the
//! user's own startup files and then marks the prompt with OSC 133 (`term_core::PromptInput`),
//! which the command history needs. Each session gets a random token in
//! `AGENTWS_MARK_TOKEN`, which the script reads (and removes from the environment) and puts in
//! every mark, so marks printed by programs are ignored. The scripts are written to
//! `shell-integration/` in the config folder at the first launch; nothing is written to the
//! user's own files. cmd.exe, fish, WSL, shells started with arguments, and PowerShell on
//! Windows (whose pty output is not scanned for marks yet) run unchanged.

use std::fs;
use std::io;
use std::path::{Path, PathBuf};
use std::sync::OnceLock;

use term_core::{LaunchSpec, SessionKind};
use workspace_model::new_mark_token;

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
/// Files earlier versions wrote, and the completion cache zsh wrote next to them.
const STALE_FILES: [&str; 3] = ["zsh/.zprofile", "zsh/.zshrc", "zsh/.zcompdump"];

/// The shells that get a script.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Shell {
    Bash,
    Zsh,
    Pwsh,
}

impl Shell {
    fn of_program(program: &str) -> Option<Self> {
        // Either separator: Windows paths are recognized on any platform.
        let name = program.rsplit(['/', '\\']).next()?.to_ascii_lowercase();
        match name.strip_suffix(".exe").unwrap_or(&name) {
            "bash" => Some(Self::Bash),
            "zsh" => Some(Self::Zsh),
            "pwsh" | "powershell" => Some(Self::Pwsh),
            _ => None,
        }
    }
}

/// Make a shell launch load the integration script. Other launches, shells started with their
/// own arguments, and launches when the scripts could not be written are left unchanged;
/// returns whether the script was attached.
pub fn attach(spec: &mut LaunchSpec) -> bool {
    if spec.kind != SessionKind::Shell || !spec.args.is_empty() {
        return false;
    }
    // Without a program alacritty starts the login shell (`$SHELL` here) on Unix, PowerShell
    // on Windows.
    let program = match &spec.program {
        Some(program) => program.clone(),
        None if cfg!(unix) => match std::env::var("SHELL") {
            Ok(shell) if !shell.is_empty() => shell,
            _ => return false,
        },
        None => return false,
    };
    let Some(shell) = Shell::of_program(&program) else {
        return false;
    };
    if shell == Shell::Pwsh && cfg!(windows) {
        return false;
    }
    let Some(dir) = scripts_dir() else {
        return false;
    };
    match shell {
        Shell::Bash => {
            spec.args = vec![
                "--rcfile".into(),
                dir.join(BASH_FILE).to_string_lossy().into_owned(),
            ];
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
                format!(". {}", powershell_quote(&script)),
            ];
        }
    }
    let token = new_mark_token();
    spec.env.insert(MARK_TOKEN_ENV.into(), token.clone());
    spec.mark_token = Some(token);
    spec.program = Some(program);
    true
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
        assert_eq!(Shell::of_program("cmd.exe"), None);
        assert_eq!(Shell::of_program("wsl.exe"), None);
    }

    #[test]
    fn other_launches_are_left_alone() {
        let mut claude = LaunchSpec::claude_code(None);
        assert!(!attach(&mut claude));
        assert!(claude.args.is_empty() && claude.env.is_empty());
        let mut git_bash = LaunchSpec::new(
            SessionKind::Shell,
            Some("bash".into()),
            vec!["-i".into(), "-l".into()],
            None,
        );
        assert!(!attach(&mut git_bash));
        assert_eq!(git_bash.args, ["-i", "-l"]);
        let mut fish = LaunchSpec::new(SessionKind::Shell, Some("fish".into()), vec![], None);
        assert!(!attach(&mut fish));
        assert_eq!(fish.program.as_deref(), Some("fish"));
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
        assert!(marks.contains(&CommandFinished));
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
    }
}
