//! An agent session's status: from Claude Code hooks, or from Copilot CLI's progress reports
//! once it sent one (or a permission hook requested attention).

use std::time::{Duration, Instant};

use agent_status::{AgentState, AgentStatus, WaitReason};
use workspace_model::Lap;

/// A status change.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) struct StatusChange {
    pub before: AgentStatus,
    pub after: AgentStatus,
}

/// The agent status and when it last changed, for the hover card's lap time.
pub(super) struct AgentTracker {
    state: AgentState,
    lap: Lap<AgentStatus>,
}

impl AgentTracker {
    pub fn new(now: Instant) -> Self {
        let state = AgentState::default();
        Self {
            lap: Lap::new(state.status(), now),
            state,
        }
    }

    pub fn status(&self) -> AgentStatus {
        self.state.status()
    }

    /// What the agent waits for, while it needs attention.
    pub fn wait_reason(&self) -> Option<WaitReason> {
        self.state.wait_reason()
    }

    /// How long the agent has been working on the current turn, or `None` when it is not
    /// working. Tool calls keep the lap running; waiting for a permission decision ends it.
    pub fn working_for(&self, now: Instant) -> Option<Duration> {
        (self.status() == AgentStatus::Working).then(|| self.lap.elapsed(now))
    }

    /// Change the state as of `now`. Returns the status change, `None` when the status stayed.
    pub fn update(
        &mut self,
        change: impl FnOnce(&mut AgentState),
        now: Instant,
    ) -> Option<StatusChange> {
        let before = self.status();
        change(&mut self.state);
        let after = self.status();
        self.lap
            .set(after, now)
            .then_some(StatusChange { before, after })
    }

    /// The process ended.
    pub fn exit(&mut self, now: Instant) {
        self.state.exit();
        self.lap.set(self.status(), now);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use workspace_model::ProgressState;

    #[test]
    fn a_status_change_starts_a_new_lap() {
        let start = Instant::now();
        let mut agent = AgentTracker::new(start);
        assert_eq!(agent.working_for(start), None);

        let later = start + Duration::from_secs(3);
        let change = agent.update(
            |state| state.apply_progress(ProgressState::Indeterminate, 1),
            later,
        );
        assert_eq!(
            change,
            Some(StatusChange {
                before: AgentStatus::Idle,
                after: AgentStatus::Working,
            })
        );
        assert_eq!(
            agent.working_for(later + Duration::from_secs(2)),
            Some(Duration::from_secs(2))
        );
    }

    #[test]
    fn the_same_status_again_is_no_change() {
        let now = Instant::now();
        let mut agent = AgentTracker::new(now);
        agent.update(
            |state| state.apply_progress(ProgressState::Indeterminate, 1),
            now,
        );
        assert_eq!(
            agent.update(
                |state| state.apply_progress(ProgressState::Indeterminate, 2),
                now
            ),
            None
        );
    }

    #[test]
    fn exit_stops_the_working_lap() {
        let now = Instant::now();
        let mut agent = AgentTracker::new(now);
        agent.update(
            |state| state.apply_progress(ProgressState::Indeterminate, 1),
            now,
        );
        agent.exit(now);
        assert_eq!(agent.status(), AgentStatus::Exited);
        assert_eq!(agent.working_for(now), None);
    }
}
