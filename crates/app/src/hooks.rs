use std::fmt;
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

use crate::processes;

/// Hook binary installed next to the app executable.
const HOOK_PROGRAM: &str = "agentws-hook";
/// Ends the name of an instance's Claude settings file, after `agentws-<pid>`.
const CLAUDE_SETTINGS_SUFFIX: &str = "-claude-settings.json";

/// Why the status hooks could not start.
#[derive(Debug)]
pub enum HooksError {
    /// The hook program is not installed next to the app executable (its expected path).
    HookProgramMissing(PathBuf),
    /// The socket or named pipe the hooks report to could not be opened.
    Server(io::Error),
    /// The Claude Code settings file could not be written.
    SettingsWrite { path: PathBuf, error: io::Error },
    /// Anything else, e.g. the app's own path could not be read.
    Other(io::Error),
}

impl HooksError {
    /// The warning the user sees: the cause and, where there is one, the fix.
    pub fn warning(&self) -> String {
        let cause = match self {
            Self::HookProgramMissing(path) => {
                let name = path
                    .file_name()
                    .map_or(HOOK_PROGRAM.into(), |name| name.to_string_lossy());
                format!("{name} not found next to the app. Reinstall to fix.")
            }
            Self::Server(error) => {
                format!("could not start the status listener ({error}). Restart the app to retry.")
            }
            Self::SettingsWrite { path, error } => {
                format!("could not write {} ({error}).", path.display())
            }
            Self::Other(error) => format!("{error}."),
        };
        format!("Agent status is off: {cause}")
    }
}

impl fmt::Display for HooksError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::HookProgramMissing(path) => write!(f, "{} not found", path.display()),
            Self::Server(error) | Self::Other(error) => error.fmt(f),
            Self::SettingsWrite { path, error } => write!(f, "{}: {error}", path.display()),
        }
    }
}

/// Receives Claude Code status hooks, Copilot permission hooks and extension mode reports.
pub struct ClaudeHooks {
    server: HookServer,
    settings_path: PathBuf,
    hook_program: PathBuf,
    copilot_plugins_dir: PathBuf,
}

impl ClaudeHooks {
    /// Start the hook server and write the settings. `on_message` runs on a server thread.
    pub fn start(
        on_message: impl Fn(HookMessage) + Send + Sync + 'static,
    ) -> Result<Self, HooksError> {
        let hook_program = std::env::current_exe()
            .map_err(HooksError::Other)?
            .with_file_name(HOOK_PROGRAM)
            .with_extension(std::env::consts::EXE_EXTENSION);
        if !hook_program.is_file() {
            return Err(HooksError::HookProgramMissing(hook_program));
        }
        let server =
            HookServer::start(default_pipe_name(), on_message).map_err(HooksError::Server)?;
        let settings_dir = settings_dir();
        if let Err(error) = sweep_stale_files(&settings_dir, processes::is_running) {
            log::warn!("could not sweep stale hook settings and Copilot plugins: {error}");
        }
        // A file rather than inline JSON: the Windows command line would need JSON quoting.
        let settings_path = settings_dir.join(format!(
            "agentws-{}{CLAUDE_SETTINGS_SUFFIX}",
            std::process::id()
        ));
        let settings = claude_settings(&hook_program).map_err(HooksError::Other)?;
        if let Err(error) = write_private(&settings_path, settings.as_bytes()) {
            return Err(HooksError::SettingsWrite {
                path: settings_path,
                error,
            });
        }
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
            copilot_hooks(&self.hook_program)?.as_bytes(),
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

/// What an app instance leaves in the settings folder, by its pid.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum OwnedEntry {
    /// `agentws-<pid>-<µs>-copilot`, a folder.
    CopilotPlugins(u32),
    /// `agentws-<pid>-claude-settings.json`, a file.
    ClaudeSettings(u32),
}

fn owned_entry(name: &str) -> Option<OwnedEntry> {
    let name = name.strip_prefix("agentws-")?;
    let valid_pid = |pid: &str| pid.parse::<u32>().ok().filter(|pid| *pid != 0);
    if let Some(pid) = name.strip_suffix(CLAUDE_SETTINGS_SUFFIX) {
        return valid_pid(pid).map(OwnedEntry::ClaudeSettings);
    }
    let (pid, timestamp) = name.strip_suffix("-copilot")?.split_once('-')?;
    timestamp.parse::<u64>().ok()?;
    valid_pid(pid).map(OwnedEntry::CopilotPlugins)
}

/// Remove the hook settings and Copilot plugins of instances that are gone (crashed).
fn sweep_stale_files(dir: &Path, is_running: impl Fn(u32) -> bool) -> io::Result<()> {
    for entry in fs::read_dir(dir)? {
        let entry = entry?;
        let Some(owned) = entry.file_name().to_str().and_then(owned_entry) else {
            continue;
        };
        let file_type = entry.file_type()?;
        let (pid, expected_type) = match owned {
            OwnedEntry::CopilotPlugins(pid) => (pid, file_type.is_dir()),
            OwnedEntry::ClaudeSettings(pid) => (pid, file_type.is_file()),
        };
        if !expected_type || is_running(pid) {
            continue;
        }
        #[cfg(unix)]
        {
            use std::os::unix::fs::MetadataExt;
            // The fallback runtime directory can be shared with other users.
            if entry.metadata()?.uid() != processes::effective_uid() {
                continue;
            }
        }
        match owned {
            OwnedEntry::CopilotPlugins(_) => remove_plugins(&entry.path()),
            OwnedEntry::ClaudeSettings(_) => {
                if let Err(error) = fs::remove_file(entry.path()) {
                    log::warn!("could not remove stale Claude settings: {error}");
                }
            }
        }
    }
    Ok(())
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
mod tests;
