use super::*;

fn spawned(description: &str) -> SubagentEvent {
    SubagentEvent::Spawned {
        description: description.into(),
    }
}

fn started(id: &str, kind: &str) -> SubagentEvent {
    SubagentEvent::Started {
        id: id.into(),
        kind: Some(kind.into()),
    }
}

fn finished(id: &str) -> SubagentEvent {
    SubagentEvent::Finished { id: id.into() }
}

fn active(id: &str) -> SubagentEvent {
    SubagentEvent::Active { id: id.into() }
}

fn waiting(id: &str) -> SubagentEvent {
    SubagentEvent::Waiting { id: id.into() }
}

fn labels(subagents: &Subagents) -> Vec<&str> {
    subagents.running().iter().map(Subagent::label).collect()
}

#[test]
fn a_subagent_shows_from_its_start_to_its_stop_with_its_spawn_description() {
    let mut subagents = Subagents::default();
    assert!(!subagents.apply(spawned("Count rust files"), 1));
    assert!(subagents.apply(started("a1", "Explore"), 2));
    assert_eq!(labels(&subagents), ["Count rust files"]);
    assert!(!subagents.apply(active("a1"), 3), "already working");
    assert!(subagents.apply(finished("a1"), 4));
    assert!(subagents.is_empty());
}

#[test]
fn without_a_description_the_type_shows() {
    let mut subagents = Subagents::default();
    subagents.apply(started("a1", "Explore"), 1);
    assert_eq!(labels(&subagents), ["Explore"]);
    subagents.apply(active("a2"), 2);
    assert_eq!(labels(&subagents), ["Explore", "Subagent"]);
}

#[test]
fn descriptions_go_to_subagents_in_spawn_order() {
    let mut subagents = Subagents::default();
    subagents.apply(spawned("first"), 1);
    subagents.apply(spawned("second"), 2);
    subagents.apply(started("a1", "Explore"), 3);
    subagents.apply(started("a2", "Explore"), 4);
    assert_eq!(labels(&subagents), ["first", "second"]);
}

#[test]
fn a_spawn_arriving_after_its_start_still_names_it() {
    let mut subagents = Subagents::default();
    subagents.apply(started("a1", "Explore"), 5);
    assert!(subagents.apply(spawned("late"), 4));
    assert_eq!(labels(&subagents), ["late"]);
    // A spawn newer than every start waits for the next subagent.
    assert!(!subagents.apply(spawned("next"), 6));
    subagents.apply(started("a2", "Explore"), 7);
    assert_eq!(labels(&subagents), ["late", "next"]);
}

#[test]
fn a_stale_spawn_description_is_dropped() {
    let mut subagents = Subagents::default();
    subagents.apply(spawned("denied"), 1);
    subagents.apply(started("a1", "Explore"), 1 + PENDING_FOR_MICROS + 1);
    assert_eq!(labels(&subagents), ["Explore"]);
}

#[test]
fn a_tool_event_before_the_start_adds_the_subagent_and_the_start_fills_in_its_type() {
    let mut subagents = Subagents::default();
    subagents.apply(spawned("List files"), 1);
    assert!(subagents.apply(active("a1"), 3));
    assert_eq!(labels(&subagents), ["List files"]);
    assert!(subagents.apply(started("a1", "explore"), 2));
    assert_eq!(subagents.running()[0].kind.as_deref(), Some("explore"));
    assert_eq!(subagents.running().len(), 1);
}

#[test]
fn waiting_follows_the_newest_event() {
    let mut subagents = Subagents::default();
    subagents.apply(started("a1", "Explore"), 1);
    assert!(subagents.apply(waiting("a1"), 3));
    assert!(subagents.running()[0].waiting);
    // A late tool event from before the request leaves it waiting.
    assert!(!subagents.apply(active("a1"), 2));
    assert!(subagents.running()[0].waiting);
    assert!(subagents.apply(active("a1"), 4));
    assert!(!subagents.running()[0].waiting);
}

#[test]
fn a_late_event_does_not_bring_back_an_ended_subagent() {
    let mut subagents = Subagents::default();
    subagents.apply(started("a1", "Explore"), 1);
    subagents.apply(finished("a1"), 3);
    assert!(!subagents.apply(active("a1"), 2));
    assert!(!subagents.apply(started("a1", "Explore"), 1));
    assert!(subagents.is_empty());
    assert!(!subagents.apply(finished("unknown"), 4));
}

#[test]
fn clearing_reports_whether_any_ran() {
    let mut subagents = Subagents::default();
    assert!(!subagents.clear());
    subagents.apply(started("a1", "Explore"), 1);
    assert!(subagents.clear());
    assert!(subagents.is_empty());
}
