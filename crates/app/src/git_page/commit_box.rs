//! The Git page's commit box, above the lists as in VS Code: a message that grows to a few
//! lines and a Commit button, enabled once something is staged and the message has text.
//! Ctrl+Enter (Cmd+Enter on macOS) commits; Enter adds a line. Each repository keeps its own
//! draft while the app runs. The repository's hooks run; a commit that fails keeps the
//! message and the staged files, and shows git's message.

use std::path::{Path, PathBuf};

use gpui_kit::component::button::{Button, ButtonVariants};
use gpui_kit::component::input::{Escape, InputEvent, Textarea, TextareaState};
use gpui_kit::component::notification::Notification;
use gpui_kit::component::{ActiveTheme, Disableable, Sizable, WindowExt};
use gpui_kit::prelude::FluentBuilder;
use gpui_kit::{
    AnyElement, AppContext, Context, Entity, Focusable, InteractiveElement, IntoElement,
    ParentElement, Styled, Subscription, Window, div,
};
use workspace_model::git::{self, RepoStatus};

use super::{Commit, GitPage, Op};

/// The commit box's key context: Ctrl+Enter, Tab and Shift+Tab are bound in it.
pub(super) const COMMIT_CONTEXT: &str = "GitCommit";
/// The message's height in lines: one when empty, growing up to this many, then scrolling.
const MAX_MESSAGE_LINES: usize = 6;
/// The commit button's tooltip while nothing is staged.
const NOTHING_STAGED: &str = "Stage changes to commit";

/// A commit that ran: in which repository, and git's summary when it succeeded.
pub(super) struct CommitDone {
    pub root: PathBuf,
    pub summary: Option<String>,
}

/// The message input, and the subscription that re-renders the page as it is typed in (the
/// button follows the text).
pub(super) fn new_message(
    window: &mut Window,
    cx: &mut Context<GitPage>,
) -> (Entity<TextareaState>, Subscription) {
    let message = cx.new(|cx| {
        let mut state = TextareaState::new(window, cx).placeholder(placeholder(None));
        state.set_auto_grow(1, MAX_MESSAGE_LINES, cx);
        state
    });
    let events = cx.subscribe(&message, |_, _, event: &InputEvent, cx| {
        if matches!(event, InputEvent::Change) {
            cx.notify();
        }
    });
    (message, events)
}

/// The message's placeholder, naming the branch committed on (none for a detached HEAD).
pub(super) fn placeholder(branch: Option<&str>) -> String {
    let keys = if cfg!(target_os = "macos") {
        "Cmd+Enter"
    } else {
        "Ctrl+Enter"
    };
    match branch {
        Some(branch) => format!("Message ({keys} to commit on {branch})"),
        None => format!("Message ({keys} to commit)"),
    }
}

impl GitPage {
    /// The message as typed.
    fn message_text(&self, cx: &Context<Self>) -> String {
        self.message.read(cx).value().to_string()
    }

    /// Whether Commit can run now: nothing committing, something staged, a message.
    pub(super) fn can_commit(&self, cx: &Context<Self>) -> bool {
        !self.committing
            && self.status().is_some_and(RepoStatus::has_staged)
            && git::commit_message_ready(&self.message_text(cx))
    }

    /// Commit what is staged in the repository shown, with the message.
    pub(super) fn commit(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if !self.can_commit(cx) {
            return;
        }
        let Some(target) = self.target() else {
            return;
        };
        self.committing = true;
        let message = self.message_text(cx);
        self.enqueue(Op::Commit(target, message), window, cx);
    }

    /// A commit ended: on success the message (or the draft of the repository it was for)
    /// empties and git's summary shows; on failure the message stays (the error shows).
    pub(super) fn finish_commit(
        &mut self,
        done: CommitDone,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.committing = false;
        let Some(summary) = done.summary else {
            return;
        };
        if self.shows(&done.root) {
            self.set_message(String::new(), window, cx);
        } else {
            self.drafts.remove(&done.root);
        }
        window.push_notification(Notification::info(summary), cx);
    }

    /// Another repository shows: keep the message as `from`'s draft and bring back `to`'s.
    pub(super) fn swap_draft(
        &mut self,
        from: Option<&Path>,
        to: Option<&Path>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let text = self.message_text(cx);
        if let Some(from) = from
            && !text.is_empty()
        {
            self.drafts.insert(from.to_path_buf(), text);
        }
        let draft = to.and_then(|to| self.drafts.remove(to)).unwrap_or_default();
        self.set_message(draft, window, cx);
    }

    /// Name the branch of the status shown in the placeholder.
    pub(super) fn sync_placeholder(&self, window: &mut Window, cx: &mut Context<Self>) {
        let branch = self.status().and_then(|status| status.branch.clone());
        self.message.update(cx, |message, cx| {
            message.set_placeholder(placeholder(branch.as_deref()), window, cx);
        });
    }

    fn set_message(&self, text: String, window: &mut Window, cx: &mut Context<Self>) {
        self.message
            .update(cx, |message, cx| message.set_value(text, window, cx));
    }

    /// Whether the repository at `root` shows.
    fn shows(&self, root: &Path) -> bool {
        self.repo.as_ref().is_some_and(|repo| repo.root == root)
    }

    /// The input's focus handle.
    pub(super) fn message_focus(&self, cx: &Context<Self>) -> gpui_kit::FocusHandle {
        self.message.read(cx).focus_handle(cx)
    }

    /// The message and the Commit button. Escape in the message gives the keyboard to the
    /// list.
    pub(super) fn render_commit_box(&self, cx: &mut Context<Self>) -> AnyElement {
        let nothing_staged = !self.status().is_some_and(RepoStatus::has_staged);
        let ready = self.can_commit(cx);
        div()
            .id("git-commit-box")
            .key_context(COMMIT_CONTEXT)
            .on_action(cx.listener(|page, _: &Escape, window, cx| {
                window.focus(&page.focus, cx);
            }))
            .on_action(cx.listener(|page, _: &Commit, window, cx| page.commit(window, cx)))
            .flex()
            .flex_col()
            .flex_none()
            .gap_2()
            .p_2()
            .border_b_1()
            .border_color(cx.theme().border)
            .child(
                Textarea::new(&self.message)
                    .small()
                    .readonly(self.committing),
            )
            .child(
                Button::new("git-commit")
                    .label("Commit")
                    .primary()
                    .small()
                    .w_full()
                    .loading(self.committing)
                    .disabled(!ready)
                    .when(nothing_staged, |button| button.tooltip(NOTHING_STAGED))
                    .on_click(cx.listener(|page, _, window, cx| page.commit(window, cx))),
            )
            .into_any_element()
    }
}

#[cfg(test)]
mod tests;
