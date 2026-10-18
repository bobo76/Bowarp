use std::fs::{self, OpenOptions};
use std::io::{self, Write};
#[cfg(unix)]
use std::os::unix::fs::OpenOptionsExt;
use std::path::{Path, PathBuf};

use agent_status::{
    HookMessage, HookServer, PIPE_ENV, SESSION_ENV, claude_settings, default_pipe_name,
};
use term_core::{LaunchSpec, SessionKind};

/// Hook binary installed next to the app executable.
const HOOK_PROGRAM: &str = "agentws-hook";

/// Lets Claude Code sessions report their status: the hook server plus the settings file that
/// registers `agentws-hook` with Claude Code.
pub struct ClaudeHooks {
    server: HookServer,
    settings_path: PathBuf,
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
        })
    }

    /// Make a Claude Code launch report its hook events as session `session`. Other session
    /// kinds are left unchanged; returns whether the session will report its status.
    pub fn attach(&self, session: u64, spec: &mut LaunchSpec) -> bool {
        if spec.kind != SessionKind::ClaudeCode {
            return false;
        }
        let settings = self.settings_path.to_string_lossy().into_owned();
        spec.args.splice(0..0, ["--settings".to_string(), settings]);
        spec.env.insert(SESSION_ENV.into(), session.to_string());
        spec.env
            .insert(PIPE_ENV.into(), self.server.pipe_name().into());
        true
    }

    /// Delete the settings file. Running Claude Code sessions have already read it.
    pub fn remove_settings(&self) {
        let _ = fs::remove_file(&self.settings_path);
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
