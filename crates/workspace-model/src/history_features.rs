//! Which of Bowarp's history features are active, from the three Terminal switches: "Command
//! history" is the master, "History popup" and "Inline suggestions" need it. A dependent
//! switch keeps its stored value while the master is off but does nothing.

use crate::Settings;

/// A key the history features may take from the shell.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum HistoryKey {
    /// Up at the prompt: opens the popup.
    Up,
    /// Right, End or Tab: accepts a suggestion.
    Accept,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct HistoryFeatures {
    command_history: bool,
    history_popup: bool,
    inline_suggestions: bool,
}

impl Default for HistoryFeatures {
    fn default() -> Self {
        Self::from_settings(&Settings::default())
    }
}

impl HistoryFeatures {
    pub fn from_settings(settings: &Settings) -> Self {
        Self {
            command_history: settings.command_history.is_on(),
            history_popup: settings.history_popup.is_on(),
            inline_suggestions: settings.inline_suggestions.is_on(),
        }
    }

    /// Commands are collected (memory and disk) and last commands captured.
    pub fn records(self) -> bool {
        self.command_history
    }

    pub fn popup(self) -> bool {
        self.command_history && self.history_popup
    }

    /// Ghost text (history and last-session) and its acceptance keys.
    pub fn suggestions(self) -> bool {
        self.command_history && self.inline_suggestions
    }

    /// Whether the key may be taken from the shell; false means it goes to the shell.
    pub fn intercepts(self, key: HistoryKey) -> bool {
        match key {
            HistoryKey::Up => self.popup(),
            HistoryKey::Accept => self.suggestions(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::FeatureSwitch;

    fn features(master: bool, popup: bool, suggestions: bool) -> HistoryFeatures {
        HistoryFeatures::from_settings(&Settings {
            command_history: FeatureSwitch::new(master),
            history_popup: FeatureSwitch::new(popup),
            inline_suggestions: FeatureSwitch::new(suggestions),
            ..Settings::default()
        })
    }

    #[test]
    fn defaults_are_all_on() {
        let all = HistoryFeatures::default();
        assert!(all.records() && all.popup() && all.suggestions());
        assert!(all.intercepts(HistoryKey::Up) && all.intercepts(HistoryKey::Accept));
    }

    #[test]
    fn every_combination_follows_the_parent_child_rule() {
        for bits in 0..8u8 {
            let (master, popup, suggestions) = (bits & 1 != 0, bits & 2 != 0, bits & 4 != 0);
            let f = features(master, popup, suggestions);
            assert_eq!(f.records(), master, "{bits}");
            assert_eq!(f.popup(), master && popup, "{bits}");
            assert_eq!(f.suggestions(), master && suggestions, "{bits}");
            assert_eq!(f.intercepts(HistoryKey::Up), master && popup, "{bits}");
            assert_eq!(
                f.intercepts(HistoryKey::Accept),
                master && suggestions,
                "{bits}"
            );
        }
    }

    #[test]
    fn popup_off_keeps_recording_and_suggestions() {
        let f = features(true, false, true);
        assert!(f.records() && f.suggestions() && !f.popup());
        assert!(!f.intercepts(HistoryKey::Up) && f.intercepts(HistoryKey::Accept));
    }

    #[test]
    fn re_enabling_the_master_restores_the_retained_children() {
        let off = features(false, true, false);
        assert!(!off.popup() && !off.suggestions() && !off.records());
        let on = features(true, true, false);
        assert!(on.popup() && !on.suggestions());
    }
}
