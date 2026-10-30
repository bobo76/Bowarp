/// Copilot CLI's interaction mode, independent of whether it is currently working.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AgentMode {
    Interactive,
    Plan,
    Autopilot,
}

impl AgentMode {
    pub fn parse(value: &str) -> Option<Self> {
        match value {
            "interactive" => Some(Self::Interactive),
            "plan" => Some(Self::Plan),
            "autopilot" => Some(Self::Autopilot),
            _ => None,
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            Self::Interactive => "Interactive",
            Self::Plan => "Plan",
            Self::Autopilot => "Autopilot",
        }
    }
}

/// Mode reports can arrive out of order, just like status hooks.
#[derive(Default)]
pub struct AgentModeState {
    mode: Option<AgentMode>,
    last_event_at: u64,
}

impl AgentModeState {
    /// Seed the initial mode before the optional extension reports it. Permission flags such
    /// as `--yolo` do not select an interaction mode.
    pub fn from_args(args: &[String]) -> Self {
        let mut state = Self::default();
        let mut args = args.iter().map(String::as_str);
        while let Some(arg) = args.next() {
            let mode = match arg {
                "--" => break,
                "--autopilot" => Some(AgentMode::Autopilot),
                "--plan" => Some(AgentMode::Plan),
                "--mode" => args.next().and_then(AgentMode::parse),
                _ => arg.strip_prefix("--mode=").and_then(AgentMode::parse),
            };
            if let Some(mode) = mode {
                state.apply(mode, 0);
            }
        }
        state
    }

    pub fn mode(&self) -> Option<AgentMode> {
        self.mode
    }

    /// Returns whether the displayed mode changed.
    pub fn apply(&mut self, mode: AgentMode, at: u64) -> bool {
        if at < self.last_event_at {
            return false;
        }
        self.last_event_at = at;
        let changed = self.mode != Some(mode);
        self.mode = Some(mode);
        changed
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn recognizes_only_supported_modes() {
        for (value, mode, label) in [
            ("interactive", AgentMode::Interactive, "Interactive"),
            ("plan", AgentMode::Plan, "Plan"),
            ("autopilot", AgentMode::Autopilot, "Autopilot"),
        ] {
            assert_eq!(AgentMode::parse(value), Some(mode));
            assert_eq!(mode.label(), label);
        }
        assert_eq!(AgentMode::parse("unknown"), None);
        assert_eq!(AgentMode::parse(""), None);
    }

    #[test]
    fn mode_is_unknown_until_reported_and_ignores_late_events() {
        let mut state = AgentModeState::default();
        assert_eq!(state.mode(), None);
        assert!(state.apply(AgentMode::Autopilot, 3));
        assert!(!state.apply(AgentMode::Interactive, 2));
        assert_eq!(state.mode(), Some(AgentMode::Autopilot));
        assert!(!state.apply(AgentMode::Autopilot, 5));
        assert!(!state.apply(AgentMode::Plan, 4));
        assert!(state.apply(AgentMode::Plan, 5));
        assert_eq!(state.mode(), Some(AgentMode::Plan));
    }

    #[test]
    fn launch_mode_is_seeded_without_confusing_permissions_or_positional_arguments() {
        for (args, expected) in [
            (vec!["--mode", "autopilot"], Some(AgentMode::Autopilot)),
            (vec!["--mode=plan"], Some(AgentMode::Plan)),
            (vec!["--autopilot"], Some(AgentMode::Autopilot)),
            (vec!["--plan"], Some(AgentMode::Plan)),
            (vec!["--yolo"], None),
            (vec!["--", "--autopilot"], None),
            (vec!["--mode"], None),
            (vec!["--mode", "unknown"], None),
        ] {
            let args: Vec<String> = args.into_iter().map(String::from).collect();
            let mut state = AgentModeState::from_args(&args);
            assert_eq!(state.mode(), expected, "{args:?}");
            state.apply(AgentMode::Interactive, 1);
            assert_eq!(state.mode(), Some(AgentMode::Interactive));
        }
    }
}
