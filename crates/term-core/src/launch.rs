use std::collections::HashMap;
use std::path::PathBuf;

use alacritty_terminal::tty::Shell;
use workspace_model::{AgentId, SessionKind};

/// Everything needed to start a session's child process.
#[derive(Clone, Debug)]
pub struct LaunchSpec {
    pub kind: SessionKind,
    /// `None` starts the user's default shell.
    pub program: Option<String>,
    pub args: Vec<String>,
    pub cwd: Option<PathBuf>,
    /// Extra environment variables, layered over the defaults set by [`crate::Session`].
    pub env: HashMap<String, String>,
    /// The token the injected shell integration script puts in its OSC 133 marks; `None`
    /// (no integration) reads the output unchanged.
    pub mark_token: Option<String>,
}

impl LaunchSpec {
    pub fn shell(cwd: Option<PathBuf>) -> Self {
        Self::new(SessionKind::Shell, None, Vec::new(), cwd)
    }

    pub fn agent(agent: AgentId, cwd: Option<PathBuf>) -> Self {
        Self::new(
            SessionKind::Agent(agent),
            Some(agent.executable().into()),
            Vec::new(),
            cwd,
        )
    }

    pub fn new(
        kind: SessionKind,
        program: Option<String>,
        args: Vec<String>,
        cwd: Option<PathBuf>,
    ) -> Self {
        Self {
            kind,
            program,
            args,
            cwd,
            env: HashMap::new(),
            mark_token: None,
        }
    }

    pub(crate) fn shell_command(&self) -> Option<Shell> {
        self.program
            .as_ref()
            .map(|program| Shell::new(program.clone(), self.args.clone()))
    }
}
