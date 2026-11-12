//! An agent tab's conversation, for resuming it after a restart: the id its hooks last
//! reported, the offer to resume it shown in a restored tab, and a resume that failed (the
//! agent no longer has that conversation).

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Conversation {
    id: Option<String>,
    /// The restored tab asks whether to resume `id`.
    offered: bool,
    /// Started with the resume flag, and no hook has reported since.
    resuming: bool,
}

impl Conversation {
    /// A restored tab that had conversation `id`: offer to resume it. The id is kept, so the
    /// offer comes back at the next start if the tab is left as it is.
    pub fn restored(id: String) -> Self {
        Self {
            id: Some(id),
            offered: true,
            resuming: false,
        }
    }

    /// The agent was started to resume conversation `id`.
    pub fn resumed(id: String) -> Self {
        Self {
            id: Some(id),
            offered: false,
            resuming: true,
        }
    }

    /// The conversation to save with the tab.
    pub fn id(&self) -> Option<&str> {
        self.id.as_deref()
    }

    /// The conversation the tab offers to resume, while it does.
    pub fn offer(&self) -> Option<&str> {
        self.id.as_deref().filter(|_| self.offered)
    }

    /// The user answered "Start new", or resumed: the offer goes.
    pub fn dismiss(&mut self) {
        self.offered = false;
    }

    /// A main-agent hook arrived, naming its `conversation` when it does. A submitted prompt
    /// starts the new conversation: the offer goes. Returns whether the offer went.
    pub fn on_hook(&mut self, conversation: Option<&str>, prompt_submitted: bool) -> bool {
        self.resuming = false;
        if let Some(id) = conversation
            && self.id.as_deref() != Some(id)
        {
            self.id = Some(id.to_owned());
        }
        let ended = self.offered && prompt_submitted;
        if ended {
            self.offered = false;
        }
        ended
    }

    /// The agent exited with `code`: whether that was the resume failing (it exits with an
    /// error before any hook when it no longer has the conversation).
    pub fn resume_failed(&self, code: Option<i32>) -> bool {
        self.resuming && code != Some(0)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_restored_offer_lasts_until_a_prompt_or_an_answer() {
        let mut conversation = Conversation::restored("a".into());
        assert_eq!(conversation.offer(), Some("a"));
        assert!(!conversation.on_hook(None, false), "other hooks keep it");
        assert!(conversation.on_hook(Some("b"), true));
        assert_eq!((conversation.offer(), conversation.id()), (None, Some("b")));

        let mut conversation = Conversation::restored("a".into());
        conversation.dismiss();
        assert_eq!((conversation.offer(), conversation.id()), (None, Some("a")));
    }

    #[test]
    fn hooks_follow_the_current_conversation() {
        let mut conversation = Conversation::default();
        assert_eq!(conversation.id(), None);
        conversation.on_hook(Some("a"), true);
        // `/clear` starts another one.
        conversation.on_hook(Some("b"), false);
        assert_eq!(conversation.id(), Some("b"));
        assert_eq!(conversation.offer(), None);
    }

    #[test]
    fn a_resume_fails_when_the_agent_errors_out_before_any_hook() {
        let conversation = Conversation::resumed("a".into());
        assert!(conversation.resume_failed(Some(1)));
        assert!(conversation.resume_failed(None));
        assert!(!conversation.resume_failed(Some(0)));

        let mut answered = Conversation::resumed("a".into());
        answered.on_hook(Some("a"), false);
        assert!(
            !answered.resume_failed(Some(1)),
            "it ran: a later failure is its own"
        );
        assert!(!Conversation::restored("a".into()).resume_failed(Some(1)));
    }
}
