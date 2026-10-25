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
}
