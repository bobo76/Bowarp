use std::fs::{self, OpenOptions};
use std::io::{self, Write};
#[cfg(unix)]
use std::os::unix::fs::{DirBuilderExt, OpenOptionsExt};
use std::path::{Path, PathBuf};

use agent_status::{
    HOOK_ENV, HookMessage, HookServer, PIPE_ENV, SESSION_ENV, claude_settings, copilot_hooks,
    default_pipe_name,
};
use term_core::{LaunchSpec, SessionKind};

/// Hook binary installed next to the app executable.
const HOOK_PROGRAM: &str = "agentws-hook";

/// Receives Claude Code status hooks, Copilot permission hooks and extension mode reports.
pub struct ClaudeHooks {
    server: HookServer,
    settings_path: PathBuf,
    hook_program: PathBuf,
    copilot_plugins_dir: PathBuf,
}

impl ClaudeHooks {
    /// Start the hook server and write the settings. `on_message` runs on a server thread.
    pub fn start(on_message: impl Fn(HookMessage) + Send + Sync + 'static) -> io::Result<Self> {
        let hook_program = std::env::current_exe()?
            .with_file_name(HOOK_PROGRAM)
            .with_extension(std::env::consts::EXE_EXTENSION);
        if !hook_program.is_file() {
            return Err(io::Error::new(
                io::ErrorKind::NotFound,
                format!("{} not found", hook_program.display()),
            ));
        }
        let server = HookServer::start(default_pipe_name(), on_message)?;
        // A file rather than inline JSON: the Windows command line would need JSON quoting.
        let settings_path = settings_dir().join(format!(
            "agentws-{}-claude-settings.json",
            std::process::id()
        ));
        write_private(&settings_path, claude_settings(&hook_program).as_bytes())?;
        Ok(Self {
            server,
            settings_path,
            hook_program,
            copilot_plugins_dir: settings_dir().join(format!(
                "agentws-{}-{}-copilot",
                std::process::id(),
                agent_status::now_micros()
            )),
        })
    }

    /// Attach status hooks to Claude Code or the mode bridge to Copilot. Only Claude Code
    /// reports activity through hooks; Copilot adds only permission requests and keeps using
    /// terminal progress for Working/Idle. The result selects Claude's hooks-only reporting.
    pub fn attach(&self, session: u64, spec: &mut LaunchSpec) -> bool {
        if spec.kind == SessionKind::Shell {
            return false;
        }
        spec.env.insert(SESSION_ENV.into(), session.to_string());
        spec.env
            .insert(PIPE_ENV.into(), self.server.pipe_name().into());
        if spec.kind == SessionKind::CopilotCli {
            spec.env.insert(
                HOOK_ENV.into(),
                self.hook_program.to_string_lossy().into_owned(),
            );
            match self.copilot_plugin(session) {
                Ok(plugin) => {
                    spec.args.splice(
                        0..0,
                        ["--plugin-dir".into(), plugin.to_string_lossy().into_owned()],
                    );
                }
                Err(error) => {
                    eprintln!("Copilot permission hooks unavailable; using progress: {error}");
                }
            }
            return false;
        }
        let settings = self.settings_path.to_string_lossy().into_owned();
        spec.args.splice(0..0, ["--settings".to_string(), settings]);
        true
    }

    fn copilot_plugin(&self, session: u64) -> io::Result<PathBuf> {
        create_private_dir(&self.copilot_plugins_dir)?;
        let plugin = self.copilot_plugins_dir.join(session.to_string());
        create_private_dir(&plugin)?;
        write_private(
            &plugin.join("plugin.json"),
            br#"{"name":"bowarp-attention","version":"1.0.0","hooks":"hooks.json"}"#,
        )?;
        write_private(
            &plugin.join("hooks.json"),
            copilot_hooks(&self.hook_program).as_bytes(),
        )?;
        Ok(plugin)
    }

    /// Delete the app-owned settings and plugins when all sessions are shutting down.
    pub fn remove_settings(&self) {
        let _ = fs::remove_file(&self.settings_path);
        let _ = fs::remove_dir_all(&self.copilot_plugins_dir);
    }
}

fn create_private_dir(path: &Path) -> io::Result<()> {
    let mut builder = fs::DirBuilder::new();
    #[cfg(unix)]
    builder.mode(0o700);
    match builder.create(path) {
        Err(error) if error.kind() == io::ErrorKind::AlreadyExists && path.is_dir() => Ok(()),
        result => result,
    }
}

/// The per-user runtime directory when there is one, else the shared temp directory.
fn settings_dir() -> PathBuf {
    std::env::var_os("XDG_RUNTIME_DIR")
        .map(PathBuf::from)
        .filter(|dir| dir.is_dir())
        .unwrap_or_else(std::env::temp_dir)
}

/// Write a new file only the user can read. A leftover file from a crashed run with the same pid
/// is replaced; one owned by another user cannot be removed, so creating the file fails.
fn write_private(path: &Path, contents: &[u8]) -> io::Result<()> {
    let _ = fs::remove_file(path);
    let mut options = OpenOptions::new();
    options.write(true).create_new(true);
    #[cfg(unix)]
    options.mode(0o600);
    options.open(path)?.write_all(contents)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn copilot_gets_a_per_tab_permission_plugin_before_user_arguments() {
        let server = HookServer::start(
            format!("agentws-attach-test-{}.sock", agent_status::now_micros()),
            |_| {},
        )
        .unwrap();
        let copilot_plugins_dir = std::env::current_dir()
            .unwrap()
            .join(format!("agentws-hooks-test-{}", agent_status::now_micros()));
        let hooks = ClaudeHooks {
            server,
            settings_path: copilot_plugins_dir.join("claude-settings.json"),
            hook_program: PathBuf::from("agentws-hook"),
            copilot_plugins_dir,
        };
        let mut copilot = LaunchSpec::copilot_cli(None);
        copilot.args = vec!["--mode".into(), "plan".into()];
        assert!(!hooks.attach(12, &mut copilot));
        let plugin = hooks.copilot_plugins_dir.join("12");
        assert_eq!(
            copilot.args,
            ["--plugin-dir", plugin.to_str().unwrap(), "--mode", "plan"]
        );
        assert!(plugin.join("plugin.json").is_file());
        assert!(plugin.join("hooks.json").is_file());
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            for path in [
                &hooks.copilot_plugins_dir,
                &plugin,
                &plugin.join("plugin.json"),
                &plugin.join("hooks.json"),
            ] {
                assert_eq!(fs::metadata(path).unwrap().permissions().mode() & 0o077, 0);
            }
        }
        assert!(!copilot.env.contains_key("COPILOT_HOME"));
        assert_eq!(copilot.env[SESSION_ENV], "12");
        assert_eq!(copilot.env[PIPE_ENV], hooks.server.pipe_name());
        assert_eq!(copilot.env[HOOK_ENV], "agentws-hook");
        let mut other = LaunchSpec::copilot_cli(None);
        assert!(!hooks.attach(15, &mut other));
        assert_ne!(other.args[1], copilot.args[1]);
        assert_eq!(other.env[SESSION_ENV], "15");

        let mut shell = LaunchSpec::shell(None);
        assert!(!hooks.attach(13, &mut shell));
        assert!(shell.env.is_empty());

        let mut claude = LaunchSpec::claude_code(None);
        claude.args = vec!["--settings".into(), "user.json".into()];
        assert!(hooks.attach(14, &mut claude));
        assert_eq!(
            claude.args,
            [
                "--settings",
                hooks.settings_path.to_str().unwrap(),
                "--settings",
                "user.json"
            ]
        );
        assert_eq!(claude.env[SESSION_ENV], "14");
        assert!(!claude.env.contains_key(HOOK_ENV));
        hooks.remove_settings();
        assert!(!plugin.exists());
    }

    #[test]
    fn unavailable_copilot_plugin_keeps_progress_and_the_mode_bridge() {
        let server = HookServer::start(
            format!("agentws-fallback-test-{}.sock", agent_status::now_micros()),
            |_| {},
        )
        .unwrap();
        let hooks = ClaudeHooks {
            server,
            settings_path: PathBuf::from("settings.json"),
            hook_program: PathBuf::from("agentws-hook"),
            copilot_plugins_dir: PathBuf::from("nonexistent-parent").join("plugins"),
        };
        let mut copilot = LaunchSpec::copilot_cli(None);
        copilot.args = vec!["--yolo".into()];
        assert!(!hooks.attach(12, &mut copilot));
        assert_eq!(copilot.args, ["--yolo"]);
        assert_eq!(copilot.env[HOOK_ENV], "agentws-hook");
    }
}
