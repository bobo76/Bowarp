use super::*;
use crate::agents::{CLAUDE, COPILOT};

const HIDDEN: TabAtEnd = TabAtEnd {
    visible: false,
    finished_unseen: false,
};
const HIDDEN_UNREAD: TabAtEnd = TabAtEnd {
    visible: false,
    finished_unseen: true,
};
const SELECTED: TabAtEnd = TabAtEnd {
    visible: true,
    finished_unseen: false,
};

fn running(agent: AgentId, title: Option<&str>) -> InPlaceAgent {
    let mut in_place = InPlaceAgent::default();
    in_place.start(agent, title.map(str::to_owned));
    in_place
}

#[test]
fn the_agent_mark_switches_the_tab_to_agent_status_until_the_d_mark() {
    let mut in_place = InPlaceAgent::default();
    assert_eq!(in_place.status_kind(SessionKind::Shell), SessionKind::Shell);
    in_place.start(COPILOT, Some("bash".into()));
    assert_eq!(in_place.agent(), Some(COPILOT));
    assert_eq!(
        in_place.status_kind(SessionKind::Shell),
        SessionKind::Agent(COPILOT)
    );
    let outcome = in_place.end(RunEnd::CommandFinished(Some(0)), SELECTED);
    assert_eq!(
        outcome,
        Some(RunEndOutcome {
            agent: COPILOT,
            title: Some("bash".into()),
            exit_toast: None,
            leftover: Leftover::Nothing,
        })
    );
    assert_eq!(in_place.status_kind(SessionKind::Shell), SessionKind::Shell);
    // A D mark without a run (any other command) changes nothing.
    assert_eq!(in_place.end(RunEnd::CommandFinished(Some(1)), HIDDEN), None);
}

#[test]
fn an_agent_killed_with_ctrl_c_in_the_background_gets_the_exit_toast() {
    let mut in_place = running(CLAUDE, None);
    let outcome = in_place
        .end(RunEnd::CommandFinished(Some(130)), HIDDEN_UNREAD)
        .unwrap();
    assert_eq!(outcome.exit_toast, Some(Some(130)));
    // The unread turn is still on screen.
    assert_eq!(outcome.leftover, Leftover::ReadyForReview);
    assert_eq!(in_place.agent(), None);
}

#[test]
fn a_clean_exit_with_a_turn_to_review_gets_no_toast() {
    let outcome = running(CLAUDE, None)
        .end(RunEnd::CommandFinished(Some(0)), HIDDEN_UNREAD)
        .unwrap();
    assert_eq!(outcome.exit_toast, None);
    assert_eq!(outcome.leftover, Leftover::ReadyForReview);
    let outcome = running(CLAUDE, None)
        .end(RunEnd::CommandFinished(Some(0)), HIDDEN)
        .unwrap();
    assert_eq!(outcome.exit_toast, Some(Some(0)));
    assert_eq!(outcome.leftover, Leftover::Nothing);
}

#[test]
fn the_selected_tab_gets_no_toast() {
    for code in [Some(0), Some(1), None] {
        let outcome = running(CLAUDE, None)
            .end(RunEnd::CommandFinished(code), SELECTED)
            .unwrap();
        assert_eq!(outcome.exit_toast, None);
    }
}

#[test]
fn a_d_mark_without_a_status_toasts_without_a_code() {
    let outcome = running(COPILOT, None)
        .end(RunEnd::CommandFinished(None), HIDDEN)
        .unwrap();
    assert_eq!(outcome.exit_toast, Some(None));
}

#[test]
fn a_shell_exiting_mid_run_ends_as_a_plain_exited_shell() {
    let mut in_place = running(CLAUDE, Some("bash"));
    let outcome = in_place
        .end(RunEnd::ShellExited(Some(1)), HIDDEN_UNREAD)
        .unwrap();
    assert_eq!(outcome.exit_toast, Some(Some(1)));
    // Exited wins over Ready for review.
    assert_eq!(outcome.leftover, Leftover::Nothing);
    assert_eq!(outcome.title, Some("bash".into()));
    assert_eq!(in_place.status_kind(SessionKind::Shell), SessionKind::Shell);
}

#[test]
fn a_second_mark_keeps_the_title_from_before_the_first() {
    let mut in_place = running(CLAUDE, Some("bash"));
    in_place.start(COPILOT, Some("✳ Claude Code".into()));
    assert_eq!(in_place.agent(), Some(COPILOT));
    let outcome = in_place
        .end(RunEnd::CommandFinished(Some(0)), SELECTED)
        .unwrap();
    assert_eq!(outcome.title, Some("bash".into()));
}

#[test]
fn the_kind_line_names_the_agent_and_its_shell() {
    assert_eq!(in_place_kind_label(CLAUDE, "bash"), "Claude Code in bash");
}
