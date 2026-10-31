use workspace_model::{AgentId, AgentMode};

/// Mode reports can arrive out of order, just like status hooks.
#[derive(Default)]
pub struct AgentModeState {
    mode: Option<AgentMode>,
    last_event_at: u64,
}

impl AgentModeState {
    /// Seed the mode declared by the provider's launch-argument adapter.
    pub fn from_agent_args(agent: AgentId, args: &[String]) -> Self {
        let mut state = Self::default();
        if let Some(mode) = agent.mode_from_args(args) {
            state.apply(mode, 0);
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
    use workspace_model::agents::COPILOT;

    #[test]
    fn recognizes_only_supported_modes() {
        for (value, mode, label) in [
            ("interactive", AgentMode::Interactive, "Interactive"),
            ("plan", AgentMode::Plan, "Plan"),
            ("autopilot", AgentMode::Autopilot, "Autopilot"),
        ] {
            assert_eq!(COPILOT.parse_mode(value), Some(mode));
            assert_eq!(mode.label(), label);
        }
        assert_eq!(COPILOT.parse_mode("unknown"), None);
        assert_eq!(COPILOT.parse_mode(""), None);
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
            let mut state = AgentModeState::from_agent_args(COPILOT, &args);
            assert_eq!(state.mode(), expected, "{args:?}");
            state.apply(AgentMode::Interactive, 1);
            assert_eq!(state.mode(), Some(AgentMode::Interactive));
        }
    }

    #[test]
    fn repeated_modes_use_the_last_value_but_plan_selects_the_initial_mode() {
        for (args, expected) in [
            (
                vec!["--mode", "plan", "--mode=autopilot"],
                AgentMode::Autopilot,
            ),
            (vec!["--mode=autopilot", "--mode", "plan"], AgentMode::Plan),
            (
                vec!["--mode", "plan", "--mode=interactive"],
                AgentMode::Interactive,
            ),
            (vec!["--mode=plan", "--mode=plan"], AgentMode::Plan),
            (vec!["--plan", "--mode=autopilot"], AgentMode::Plan),
            (vec!["--mode", "autopilot", "--plan"], AgentMode::Plan),
            (vec!["--plan", "--plan"], AgentMode::Plan),
            (vec!["--autopilot", "--autopilot"], AgentMode::Autopilot),
            (vec!["--plan", "--", "--mode=autopilot"], AgentMode::Plan),
        ] {
            let args: Vec<String> = args.into_iter().map(String::from).collect();
            assert_eq!(
                AgentModeState::from_agent_args(COPILOT, &args).mode(),
                Some(expected),
                "{args:?}"
            );
        }
    }

    #[test]
    fn cli_rejected_mode_combinations_do_not_seed_a_mode() {
        for args in [
            vec!["--autopilot", "--plan"],
            vec!["--plan", "--autopilot"],
            vec!["--mode", "interactive", "--autopilot"],
            vec!["--autopilot", "--mode=autopilot"],
            vec!["--plan", "--mode=interactive"],
            vec!["--mode=plan", "--plan"],
        ] {
            let args: Vec<String> = args.into_iter().map(String::from).collect();
            assert_eq!(
                AgentModeState::from_agent_args(COPILOT, &args).mode(),
                None,
                "{args:?}"
            );
        }
    }
}
