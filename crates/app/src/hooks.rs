use std::collections::HashMap;
use std::fmt;
use std::fs::{self, OpenOptions};
use std::io::{self, Write};
#[cfg(unix)]
use std::os::unix::fs::{DirBuilderExt, OpenOptionsExt};
use std::path::{Path, PathBuf};

use agent_status::{AGENT_ENV, HookMessage, HookServer, PIPE_ENV, SESSION_ENV, default_pipe_name};
use term_core::LaunchSpec;
use workspace_model::{ActivityPolicy, AgentId, HookArtifactKind, agents};

use crate::processes;

/// Hook binary installed next to the app executable.
const HOOK_PROGRAM: &str = "agentws-hook";
/// Why the status hooks could not start.
#[derive(Debug)]
pub enum HooksError {
    /// The hook program is not installed next to the app executable (its expected path).
    HookProgramMissing(PathBuf),
    /// The socket or named pipe the hooks report to could not be opened.
    Server(io::Error),
    /// An agent's hook artifact could not be written.
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

/// Receives status and mode reports from registered agent integrations.
pub struct AgentHooks {
    server: HookServer,
    hook_program: PathBuf,
    artifacts: HashMap<AgentId, PathBuf>,
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct HookAttachment {
    pub reporting: HookReporting,
    pub diagnostics: Vec<String>,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum HookReporting {
    #[default]
    None,
    FullActivity,
    Supplemental,
}

impl HookAttachment {
    pub fn reports_full_activity(&self) -> bool {
        self.reporting == HookReporting::FullActivity
    }
}

impl AgentHooks {
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
            log::warn!("could not sweep stale agent hook artifacts: {error}");
        }
        let mut artifacts = HashMap::new();
        for agent in agents() {
            let artifact_path = artifact_path(&settings_dir, agent);
            if agent.hook_artifact_kind() == HookArtifactKind::SettingsFile {
                let files = agent
                    .hook_artifact(&hook_program)
                    .map_err(HooksError::Other)?;
                let [file] = files.as_slice() else {
                    return Err(HooksError::Other(io::Error::new(
                        io::ErrorKind::InvalidData,
                        format!("{} settings must be one file", agent.label()),
                    )));
                };
                if let Err(error) = write_private(&artifact_path, file.contents.as_bytes()) {
                    return Err(HooksError::SettingsWrite {
                        path: artifact_path,
                        error,
                    });
                }
            }
            artifacts.insert(agent, artifact_path);
        }
        Ok(Self {
            server,
            hook_program,
            artifacts,
        })
    }

    /// Attach the provider's declared reporting artifact without changing user arguments.
    pub fn attach(&self, session: u64, spec: &mut LaunchSpec) -> HookAttachment {
        let Some(agent) = spec.kind.agent() else {
            return HookAttachment::default();
        };
        spec.env.insert(SESSION_ENV.into(), session.to_string());
        spec.env
            .insert(AGENT_ENV.into(), agent.cli_selector().into());
        spec.env
            .insert(PIPE_ENV.into(), self.server.pipe_name().into());
        if let Some(name) = agent.mode().and_then(|mode| mode.hook_program_env) {
            spec.env.insert(
                name.into(),
                self.hook_program.to_string_lossy().into_owned(),
            );
        }
        let artifact = match agent.hook_artifact_kind() {
            HookArtifactKind::SettingsFile => Ok(self.artifacts[&agent].clone()),
            HookArtifactKind::PluginDirectory => self.plugin(agent, session),
        };
        let path = match artifact {
            Ok(path) => path,
            Err(error) => {
                self.remove_session(session);
                let fallback = match agent.activity() {
                    ActivityPolicy::FullHooks => "using output activity",
                    ActivityPolicy::ProgressWithAttentionHooks => "using progress",
                };
                return HookAttachment {
                    reporting: HookReporting::None,
                    diagnostics: vec![format!(
                        "{} hooks unavailable; {fallback}: {error}",
                        agent.label()
                    )],
                };
            }
        };
        spec.args.splice(
            0..0,
            [
                agent.hook_artifact_plan().launch_flag.into(),
                path.to_string_lossy().into_owned(),
            ],
        );
        let reporting = match agent.activity() {
            ActivityPolicy::FullHooks => HookReporting::FullActivity,
            ActivityPolicy::ProgressWithAttentionHooks => HookReporting::Supplemental,
        };
        HookAttachment {
            reporting,
            diagnostics: Vec::new(),
        }
    }

    fn plugin(&self, agent: AgentId, session: u64) -> io::Result<PathBuf> {
        let plugin_root = &self.artifacts[&agent];
        create_private_dir(plugin_root)?;
        let plugin = plugin_root.join(session.to_string());
        create_private_dir(&plugin)?;
        for file in agent.hook_artifact(&self.hook_program)? {
            write_private(&plugin.join(file.name), file.contents.as_bytes())?;
        }
        Ok(plugin)
    }

    /// Remove a session's plugin after its process exits or its tab closes.
    pub fn remove_session(&self, session: u64) {
        for (agent, path) in &self.artifacts {
            if agent.hook_artifact_kind() == HookArtifactKind::PluginDirectory {
                remove_artifact(&path.join(session.to_string()));
            }
        }
    }

    /// Delete the app-owned settings and plugins when all sessions are shutting down.
    pub fn remove_settings(&self) {
        for (agent, path) in &self.artifacts {
            match agent.hook_artifact_kind() {
                HookArtifactKind::PluginDirectory => remove_artifact(path),
                HookArtifactKind::SettingsFile => {
                    if let Err(error) = fs::remove_file(path)
                        && error.kind() != io::ErrorKind::NotFound
                    {
                        log::warn!("could not remove agent hook settings: {error}");
                    }
                }
            }
        }
    }
}

fn artifact_path(dir: &Path, agent: AgentId) -> PathBuf {
    let plan = agent.hook_artifact_plan();
    let name = match plan.kind {
        HookArtifactKind::SettingsFile => {
            format!(
                "agentws-{}{suffix}",
                std::process::id(),
                suffix = plan.resource_suffix
            )
        }
        HookArtifactKind::PluginDirectory => format!(
            "agentws-{}-{}{suffix}",
            std::process::id(),
            agent_status::now_micros(),
            suffix = plan.resource_suffix
        ),
    };
    dir.join(name)
}

fn remove_artifact(path: &Path) {
    if let Err(error) = fs::remove_dir_all(path)
        && error.kind() != io::ErrorKind::NotFound
    {
        log::warn!("could not remove agent hook artifact: {error}");
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct OwnedEntry {
    pid: u32,
    kind: HookArtifactKind,
}

fn owned_entry(name: &str) -> Option<OwnedEntry> {
    let name = name.strip_prefix("agentws-")?;
    let valid_pid = |pid: &str| pid.parse::<u32>().ok().filter(|pid| *pid != 0);
    agents().find_map(|agent| {
        let plan = agent.hook_artifact_plan();
        match plan.kind {
            HookArtifactKind::SettingsFile => name
                .strip_suffix(plan.resource_suffix)
                .and_then(valid_pid)
                .map(|pid| OwnedEntry {
                    pid,
                    kind: HookArtifactKind::SettingsFile,
                }),
            HookArtifactKind::PluginDirectory => {
                let (pid, timestamp) = name.strip_suffix(plan.resource_suffix)?.split_once('-')?;
                timestamp.parse::<u64>().ok()?;
                valid_pid(pid).map(|pid| OwnedEntry {
                    pid,
                    kind: HookArtifactKind::PluginDirectory,
                })
            }
        }
    })
}

/// Remove the hook artifacts of instances that are gone (crashed).
fn sweep_stale_files(dir: &Path, is_running: impl Fn(u32) -> bool) -> io::Result<()> {
    for entry in fs::read_dir(dir)? {
        let entry = entry?;
        let Some(owned) = entry.file_name().to_str().and_then(owned_entry) else {
            continue;
        };
        let file_type = entry.file_type()?;
        let expected_type = match owned.kind {
            HookArtifactKind::PluginDirectory => file_type.is_dir(),
            HookArtifactKind::SettingsFile => file_type.is_file(),
        };
        if !expected_type || is_running(owned.pid) {
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
        match owned.kind {
            HookArtifactKind::PluginDirectory => remove_artifact(&entry.path()),
            HookArtifactKind::SettingsFile => {
                if let Err(error) = fs::remove_file(entry.path()) {
                    log::warn!("could not remove stale agent settings: {error}");
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
