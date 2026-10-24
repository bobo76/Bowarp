//! What the two Workspace switches allow, together with the history switches they depend on:
//! "Restore previous tabs" governs the automatic tab snapshot (`tabs.json`), and "Remember
//! last shell command" needs it as well as command history. A dependent switch keeps its
//! stored value while a parent is off but does nothing.

use crate::{HistoryFeatures, Settings};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct WorkspaceFeatures {
    restore_tabs: bool,
    remember_last_command: bool,
    history: HistoryFeatures,
}

impl Default for WorkspaceFeatures {
    fn default() -> Self {
        Self::from_settings(&Settings::default())
    }
}

impl WorkspaceFeatures {
    pub fn from_settings(settings: &Settings) -> Self {
        Self {
            restore_tabs: settings.restore_tabs.is_on(),
            remember_last_command: settings.remember_last_command.is_on(),
            history: HistoryFeatures::from_settings(settings),
        }
    }

    /// The saved tabs are read at start. Off: a fresh default tab, `tabs.json` untouched.
    pub fn loads_tabs(self) -> bool {
        self.restore_tabs
    }

    /// Snapshots are written to `tabs.json`. Off: the file is left as it is.
    pub fn saves_tabs(self) -> bool {
        self.restore_tabs
    }

    /// New last commands are captured (and so saved). Saved ones are retained either way.
    pub fn captures_last_command(self) -> bool {
        self.restore_tabs && self.remember_last_command && self.history.records()
    }

    /// A saved last command is offered as ghost text: capture plus inline suggestions.
    pub fn offers_last_command(self) -> bool {
        self.captures_last_command() && self.history.suggestions()
    }

    pub fn history(self) -> HistoryFeatures {
        self.history
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::FeatureSwitch;

    fn features(change: impl FnOnce(&mut Settings)) -> WorkspaceFeatures {
        let mut settings = Settings::default();
        change(&mut settings);
        WorkspaceFeatures::from_settings(&settings)
    }

    #[test]
    fn defaults_are_all_on() {
        let all = WorkspaceFeatures::default();
        assert!(all.loads_tabs() && all.saves_tabs());
        assert!(all.captures_last_command() && all.offers_last_command());
    }

    #[test]
    fn every_combination_follows_the_parent_child_rule() {
        for bits in 0..16u8 {
            let (history, ghost, restore, remember) =
                (bits & 1 != 0, bits & 2 != 0, bits & 4 != 0, bits & 8 != 0);
            let f = features(|settings| {
                settings.command_history = FeatureSwitch::new(history);
                settings.inline_suggestions = FeatureSwitch::new(ghost);
                settings.restore_tabs = FeatureSwitch::new(restore);
                settings.remember_last_command = FeatureSwitch::new(remember);
            });
            assert_eq!(f.loads_tabs(), restore, "{bits}");
            assert_eq!(f.saves_tabs(), restore, "{bits}");
            let captures = history && restore && remember;
            assert_eq!(f.captures_last_command(), captures, "{bits}");
            assert_eq!(f.offers_last_command(), captures && ghost, "{bits}");
        }
    }

    #[test]
    fn the_child_value_is_kept_while_a_parent_is_off() {
        let restore_off = features(|settings| settings.restore_tabs = FeatureSwitch::new(false));
        assert!(!restore_off.captures_last_command());
        assert!(features(|_| {}).captures_last_command());
        let history_off = features(|settings| settings.command_history = FeatureSwitch::new(false));
        assert!(!history_off.captures_last_command());
    }
}
