//! Resuming a restored agent tab's conversation: the tab asks first in a bar above the
//! terminal ("Resume" / "Start new"), and the workspace restarts the agent with the resume
//! flag when the user agrees (see `workspace_model::Conversation`).

use agent_status::AgentReport;
use gpui_kit::component::button::{Button, ButtonVariants};
use gpui_kit::component::{ActiveTheme, Icon, IconName, Sizable};
use gpui_kit::{AnyElement, Context, InteractiveElement, IntoElement, ParentElement, Styled, div};
use workspace_model::{AgentId, Conversation, HookSemantic};

use super::TerminalView;

/// The user asked to resume this conversation: the workspace restarts the agent with it.
pub struct ResumeConversation(pub String);

/// The agent started to resume a conversation exited at once: it no longer has it.
pub struct ResumeFailed;

impl TerminalView {
    /// The conversation to save with the tab, when its agent can resume one.
    pub fn conversation(&self) -> Option<&str> {
        self.conversation.id()
    }

    /// A restored tab had conversation `id`: ask whether to resume it.
    pub fn offer_resume(&mut self, id: String, cx: &mut Context<Self>) {
        if self.resumable_agent().is_some() {
            self.conversation = Conversation::restored(id);
            cx.notify();
        }
    }

    /// This session was started to resume conversation `id`.
    pub fn mark_resumed(&mut self, id: String) {
        self.conversation = Conversation::resumed(id);
    }

    /// The agent the tab was started as, when it resumes conversations. An agent typed in a
    /// shell tab is left out: the tab reopens as a shell.
    fn resumable_agent(&self) -> Option<AgentId> {
        self.kind()
            .agent()
            .filter(|agent| agent.definition().launch.resume_flag.is_some())
    }

    /// Follow the main agent's conversation through its hooks.
    pub(super) fn track_conversation(&mut self, report: &AgentReport, cx: &mut Context<Self>) {
        let Some(agent) = self.resumable_agent() else {
            return;
        };
        if report.subagent.is_some() {
            return;
        }
        let prompt = agent.decode_hook_event(&report.event_name, report.detail.as_deref())
            == Some(HookSemantic::PromptSubmitted);
        if self
            .conversation
            .on_hook(report.conversation.as_deref(), prompt)
        {
            cx.notify();
        }
    }

    /// Whether the agent exiting with `code` was its resume failing.
    pub(super) fn resume_failed(&self, code: Option<i32>) -> bool {
        self.conversation.resume_failed(code)
    }

    #[cfg(all(test, unix))]
    pub(crate) fn resume_offer_for_test(&self) -> Option<&str> {
        self.conversation.offer()
    }

    pub(crate) fn answer_resume(&mut self, resume: bool, cx: &mut Context<Self>) {
        let Some(id) = self.conversation.offer().map(str::to_owned) else {
            return;
        };
        self.conversation.dismiss();
        cx.notify();
        if resume {
            cx.emit(ResumeConversation(id));
        }
    }

    /// The bar asking whether to resume the restored conversation, while it does.
    pub(super) fn resume_banner(&self, cx: &mut Context<Self>) -> Option<AnyElement> {
        self.conversation.offer()?;
        let theme = cx.theme();
        let button = |id: &'static str, label: &'static str, resume: bool| {
            Button::new(id).label(label).small().on_click(cx.listener(
                move |view, _, window, cx| {
                    view.answer_resume(resume, cx);
                    window.focus(&view.focus_handle, cx);
                },
            ))
        };
        let banner = div()
            .id("resume-banner")
            .flex_none()
            .flex()
            .items_center()
            .gap_2()
            .px_3()
            .py_1()
            .text_sm()
            .bg(theme.info.opacity(0.15))
            .border_b_1()
            .border_color(theme.info)
            .text_color(theme.foreground)
            .child(Icon::new(IconName::Info).text_color(theme.info))
            .child(
                div()
                    .flex_1()
                    .min_w_0()
                    .child("This tab had a conversation when the window closed."),
            )
            .child(
                button("resume-conversation", "Resume it", true)
                    .primary()
                    .tooltip("Restart the agent on that conversation"),
            )
            .child(
                button("resume-start-new", "Start new", false)
                    .outline()
                    .tooltip("Keep this new conversation; typing a prompt does the same"),
            );
        // Its tint is translucent: on the theme background, not on a see-through terminal.
        Some(
            div()
                .flex_none()
                .bg(theme.background)
                .child(banner)
                .into_any_element(),
        )
    }
}
