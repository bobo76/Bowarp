//! An agent run in place in a shell tab. An agent started at a shell prompt with the tab's
//! hooks (the variables the tab's shell gets) announces itself with an `N` mark
//! ([`crate::PromptMark::AgentStarted`]) once its command runs. From then until the command's
//! `D` mark, or the shell exiting, the tab follows that agent's status policy like a tab
//! opened from the `+` menu; afterwards it is a shell tab again. The tab's saved kind stays the
//! shell, so restoring or duplicating it starts the shell.

use crate::{AgentId, SessionKind};

/// Whether an agent runs in place in a shell tab, and what to put back when it ends.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct InPlaceAgent {
    run: Option<Run>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct Run {
    agent: AgentId,
    /// The program title when the run started (`None`: none set), put back at its end.
    title_before: Option<String>,
}

/// How an in-place run ended.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RunEnd {
    /// The command's `D` mark (the agent exited, was interrupted or crashed), with the exit
    /// status the shell reported.
    CommandFinished(Option<i32>),
    /// The shell itself exited before the `D` mark, with its exit code.
    ShellExited(Option<i32>),
}

/// The tab when a run ends.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct TabAtEnd {
    /// The tab is the selected one, on screen.
    pub visible: bool,
    /// The agent finished a turn while the tab was hidden (Ready for review).
    pub finished_unseen: bool,
}

/// What the shell tab keeps of the agent's status once the run ended. Needs attention
/// (Waiting, a bell) never stays: nothing waits for the user any more.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Leftover {
    Nothing,
    /// The unread turn is still on screen: Ready for review until the tab is selected.
    ReadyForReview,
}

/// What ending a run changes on the tab.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RunEndOutcome {
    pub agent: AgentId,
    /// The title to put back.
    pub title: Option<String>,
    /// The agent exit toast to show, with its exit code, as an agent tab shows it.
    pub exit_toast: Option<Option<i32>>,
    pub leftover: Leftover,
}

impl InPlaceAgent {
    /// The agent running in place, if any.
    pub fn agent(&self) -> Option<AgentId> {
        self.run.as_ref().map(|run| run.agent)
    }

    /// The kind whose status policy, icon and notifications apply now: the agent while one
    /// runs in place, else `saved`, the kind the tab was started (and is saved) as.
    pub fn status_kind(&self, saved: SessionKind) -> SessionKind {
        self.agent().map_or(saved, SessionKind::Agent)
    }

    /// An agent's mark: from now on the tab shows `agent`'s status, starting over. `title` is
    /// the program title now; another mark during a run (one command line running two agents)
    /// keeps the title from before the first.
    pub fn start(&mut self, agent: AgentId, title: Option<String>) {
        let title_before = match self.run.take() {
            Some(run) => run.title_before,
            None => title,
        };
        self.run = Some(Run {
            agent,
            title_before,
        });
    }

    /// End the run, if one is going on: the tab is a shell tab again.
    pub fn end(&mut self, end: RunEnd, tab: TabAtEnd) -> Option<RunEndOutcome> {
        let run = self.run.take()?;
        let (code, leftover) = match end {
            RunEnd::CommandFinished(code) if tab.finished_unseen => {
                (code, Leftover::ReadyForReview)
            }
            RunEnd::CommandFinished(code) => (code, Leftover::Nothing),
            // An exited tab shows Exited, which wins over Ready for review.
            RunEnd::ShellExited(code) => (code, Leftover::Nothing),
        };
        Some(RunEndOutcome {
            agent: run.agent,
            title: run.title_before,
            exit_toast: shows_exit_toast(code, tab).then_some(code),
            leftover,
        })
    }
}

/// Whether an agent's exit (code `code`) gets a toast: only in a background tab, and not
/// after a clean exit while a Ready for review is pending, which already tells the user.
pub fn shows_exit_toast(code: Option<i32>, tab: TabAtEnd) -> bool {
    !tab.visible && (code != Some(0) || !tab.finished_unseen)
}

/// The hover card's kind line while an agent runs in place: "Claude Code in bash".
pub fn in_place_kind_label(agent: AgentId, shell_name: &str) -> String {
    format!("{} in {shell_name}", agent.label())
}

#[cfg(test)]
mod tests;
