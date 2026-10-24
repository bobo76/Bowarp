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
        let settings_dir = settings_dir();
        if let Err(error) = sweep_stale_plugins(&settings_dir, process_is_running) {
            log::warn!("could not sweep stale Copilot plugins: {error}");
        }
        // A file rather than inline JSON: the Windows command line would need JSON quoting.
        let settings_path = settings_dir.join(format!(
            "agentws-{}-claude-settings.json",
            std::process::id()
        ));
        write_private(&settings_path, claude_settings(&hook_program).as_bytes())?;
        Ok(Self {
            server,
            settings_path,
            hook_program,
            copilot_plugins_dir: settings_dir.join(format!(
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
                    self.remove_session(session);
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

    /// Remove a session's plugin after its process exits or its tab closes.
    pub fn remove_session(&self, session: u64) {
        remove_plugins(&self.copilot_plugins_dir.join(session.to_string()));
    }

    /// Delete the app-owned settings and plugins when all sessions are shutting down.
    pub fn remove_settings(&self) {
        let _ = fs::remove_file(&self.settings_path);
        remove_plugins(&self.copilot_plugins_dir);
    }
}

fn remove_plugins(path: &Path) {
    if let Err(error) = fs::remove_dir_all(path)
        && error.kind() != io::ErrorKind::NotFound
    {
        log::warn!("could not remove Copilot plugin: {error}");
    }
}

fn plugin_owner(name: &str) -> Option<u32> {
    let name = name.strip_prefix("agentws-")?.strip_suffix("-copilot")?;
    let (pid, timestamp) = name.split_once('-')?;
    timestamp.parse::<u64>().ok()?;
    pid.parse::<u32>().ok().filter(|pid| *pid != 0)
}

fn sweep_stale_plugins(dir: &Path, is_running: impl Fn(u32) -> bool) -> io::Result<()> {
    for entry in fs::read_dir(dir)? {
        let entry = entry?;
        let Some(pid) = entry.file_name().to_str().and_then(plugin_owner) else {
            continue;
        };
        if !entry.file_type()?.is_dir() || is_running(pid) {
            continue;
        }
        #[cfg(unix)]
        {
            use std::os::unix::fs::MetadataExt;
            // The fallback runtime directory can be shared with other users.
            if entry.metadata()?.uid() != unsafe { libc::geteuid() } {
                continue;
            }
        }
        remove_plugins(&entry.path());
    }
    Ok(())
}

#[cfg(unix)]
fn process_is_running(pid: u32) -> bool {
    let Ok(pid) = libc::pid_t::try_from(pid) else {
        return true;
    };
    // Signal zero checks existence without sending a signal. Permission errors are not death.
    let result = unsafe { libc::kill(pid, 0) };
    result == 0 || io::Error::last_os_error().raw_os_error() != Some(libc::ESRCH)
}

#[cfg(windows)]
fn process_is_running(pid: u32) -> bool {
    use windows_sys::Win32::Foundation::{
        CloseHandle, ERROR_INVALID_PARAMETER, GetLastError, STILL_ACTIVE,
    };
    use windows_sys::Win32::System::Threading::{
        GetExitCodeProcess, OpenProcess, PROCESS_QUERY_LIMITED_INFORMATION,
    };

    // Access denied is inconclusive; only a nonexistent pid is safe to sweep.
    unsafe {
        let handle = OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, 0, pid);
        if handle.is_null() {
            return GetLastError() != ERROR_INVALID_PARAMETER;
        }
        let mut code = 0;
        let running = GetExitCodeProcess(handle, &mut code) == 0 || code == STILL_ACTIVE as u32;
        CloseHandle(handle);
        running
    }
}

fn create_private_dir(path: &Path) -> io::Result<()> {
    let builder = fs::DirBuilder::new();
    #[cfg(unix)]
    let mut builder = builder;
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
        let copilot_plugins_dir =
            std::env::temp_dir().join(format!("agentws-hooks-test-{}", agent_status::now_micros()));
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
        hooks.remove_session(12);
        assert!(!plugin.exists(), "closing a tab removes its plugin");
        assert!(
            hooks
                .copilot_plugins_dir
                .join("15")
                .join("hooks.json")
                .is_file()
        );
        hooks.remove_session(12);
        let partial = hooks.copilot_plugins_dir.join("16");
        create_private_dir(&partial).unwrap();
        fs::create_dir(partial.join("hooks.json")).unwrap();
        let mut failed = LaunchSpec::copilot_cli(None);
        assert!(!hooks.attach(16, &mut failed));
        assert!(failed.args.is_empty());
        assert!(
            !partial.exists(),
            "failed creation removes the partial plugin"
        );
        assert!(hooks.copilot_plugins_dir.join("15").exists());
        hooks.remove_settings();
        assert!(!hooks.copilot_plugins_dir.exists());
    }

    #[test]
    fn sweeps_only_plugin_directories_owned_by_dead_processes() {
        let root = std::env::temp_dir().join(format!(
            "agentws-sweep-test-{}-{}",
            std::process::id(),
            agent_status::now_micros()
        ));
        fs::create_dir(&root).unwrap();
        for name in [
            "agentws-7-100-copilot",
            "agentws-8-200-copilot",
            "agentws-8-invalid-copilot",
            "agentws-0-200-copilot",
            "agentws-8-200-copilot-other",
            "unrelated",
        ] {
            fs::create_dir(root.join(name)).unwrap();
            fs::write(root.join(name).join("hooks.json"), "{}").unwrap();
        }
        let file = root.join("agentws-8-300-copilot");
        fs::write(&file, "not a directory").unwrap();
        #[cfg(unix)]
        std::os::unix::fs::symlink(root.join("unrelated"), root.join("agentws-8-400-copilot"))
            .unwrap();

        sweep_stale_plugins(&root, |pid| pid == 7).unwrap();
        assert!(!root.join("agentws-8-200-copilot").exists());
        for name in [
            "agentws-7-100-copilot",
            "agentws-8-invalid-copilot",
            "agentws-0-200-copilot",
            "agentws-8-200-copilot-other",
            "unrelated",
        ] {
            assert!(root.join(name).join("hooks.json").is_file(), "{name}");
        }
        assert!(file.is_file());
        #[cfg(unix)]
        assert!(root.join("agentws-8-400-copilot").is_symlink());
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn process_check_preserves_live_owners_and_detects_exited_owners() {
        assert!(process_is_running(std::process::id()));
        #[cfg(unix)]
        let mut command = std::process::Command::new("sh");
        #[cfg(windows)]
        let mut command = std::process::Command::new("cmd");
        #[cfg(unix)]
        command.args(["-c", "exit 0"]);
        #[cfg(windows)]
        command.args(["/C", "exit 0"]);
        let mut child = command.spawn().unwrap();
        let pid = child.id();
        assert!(child.wait().unwrap().success());
        assert!(!process_is_running(pid));
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
