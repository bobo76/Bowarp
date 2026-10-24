//! What the "In-app notifications" switch allows. On, every toast shows as always. Off, the
//! toasts about agents (Needs attention, Ready for review, clean exit) are not created; status
//! dots, attention badges and the sidebar status are separate and stay. Feedback for what the
//! user just did, an agent that exited with a failure and warnings that must not be missed
//! are safeguards, not notifications, and show either way. Sound is a separate switch and not
//! decided here.

use crate::Settings;

/// Why a toast is raised.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ToastKind {
    /// A background tab's agent needs attention, finished or exited cleanly.
    AgentStatus,
    /// A background tab's agent exited with a nonzero or unknown exit code. Not hidden by the
    /// switch: it is the only way to tell a crash from a clean exit without opening the tab.
    AgentFailure,
    /// The result of an action the user just took (an error saving a file, history cleared).
    UserFeedback,
    /// A warning that stays until dismissed (the fallback-shell warning at startup).
    PersistentWarning,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct NotificationFeatures {
    in_app: bool,
}

impl Default for NotificationFeatures {
    fn default() -> Self {
        Self::from_settings(&Settings::default())
    }
}

impl NotificationFeatures {
    pub fn from_settings(settings: &Settings) -> Self {
        Self {
            in_app: settings.in_app_notifications.is_on(),
        }
    }

    /// Whether a toast of this kind may be created now.
    pub fn shows(self, kind: ToastKind) -> bool {
        match kind {
            ToastKind::AgentStatus => self.in_app,
            ToastKind::AgentFailure | ToastKind::UserFeedback | ToastKind::PersistentWarning => {
                true
            }
        }
    }
}

/// The kind of toast for an agent that exited with `code` (`None`: no exit code known).
pub fn exit_toast_kind(code: Option<i32>) -> ToastKind {
    match code {
        Some(0) => ToastKind::AgentStatus,
        Some(_) | None => ToastKind::AgentFailure,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::FeatureSwitch;

    fn features(on: bool) -> NotificationFeatures {
        NotificationFeatures::from_settings(&Settings {
            in_app_notifications: FeatureSwitch::new(on),
            ..Settings::default()
        })
    }

    #[test]
    fn on_by_default_shows_every_kind() {
        let all = NotificationFeatures::default();
        for kind in [
            ToastKind::AgentStatus,
            ToastKind::AgentFailure,
            ToastKind::UserFeedback,
            ToastKind::PersistentWarning,
        ] {
            assert!(all.shows(kind), "{kind:?}");
        }
    }

    #[test]
    fn off_drops_only_agent_status_toasts() {
        let off = features(false);
        assert!(!off.shows(ToastKind::AgentStatus));
        assert!(off.shows(ToastKind::AgentFailure));
        assert!(off.shows(ToastKind::UserFeedback));
        assert!(off.shows(ToastKind::PersistentWarning));
        assert!(features(true).shows(ToastKind::AgentStatus));
    }

    #[test]
    fn nonzero_or_unknown_exit_toast_shown_when_off() {
        let off = features(false);
        for code in [Some(1), Some(-1), Some(137), None] {
            assert!(off.shows(exit_toast_kind(code)), "{code:?}");
        }
    }

    #[test]
    fn clean_exit_toast_suppressed_when_off_and_shown_when_on() {
        assert!(!features(false).shows(exit_toast_kind(Some(0))));
        assert!(features(true).shows(exit_toast_kind(Some(0))));
    }

    #[test]
    fn switch_persists_as_a_plain_bool_and_defaults_on() {
        let off: Settings = serde_json::from_str(r#"{"in_app_notifications": false}"#).unwrap();
        assert!(!off.in_app_notifications.is_on());
        for text in [
            r#"{}"#,
            r#"{"in_app_notifications": "no"}"#,
            r#"{"in_app_notifications": 0}"#,
        ] {
            let settings: Settings = serde_json::from_str(text).unwrap();
            assert!(settings.in_app_notifications.is_on(), "{text}");
        }
        let value = serde_json::to_value(&off).unwrap();
        assert_eq!(
            value["in_app_notifications"],
            serde_json::Value::Bool(false)
        );
        let back: Settings = serde_json::from_value(value).unwrap();
        assert!(!back.in_app_notifications.is_on());
    }
}
