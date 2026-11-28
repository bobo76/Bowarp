use workspace_model::layout::Layout;

use super::*;

/// Two groups, so their ids are real ones.
fn group_ids() -> (GroupId, GroupId) {
    let mut layout = Layout::default();
    layout.add(1);
    layout.add(2);
    let first = layout.new_group_with(1, "Backend").unwrap();
    let second = layout.new_group_with(2, "Docs").unwrap();
    (first, second)
}

/// A plain tab: not grouped, nothing to reconnect, restart or reset, alone in the window.
fn plain() -> TabMenuState {
    TabMenuState {
        can_reconnect: false,
        can_restart: false,
        can_mark_ready: false,
        pinned: false,
        has_custom_name: false,
        other_groups: Vec::new(),
        in_group: false,
        has_branch: false,
        has_folder: false,
        can_move_up: false,
        can_move_down: false,
        has_others_to_close: false,
        has_below_to_close: false,
        color: None,
    }
}

#[test]
fn a_plain_tab_is_offered_only_the_commands_that_always_apply() {
    assert_eq!(
        plain().offered(),
        [
            TabCommand::Duplicate,
            TabCommand::TogglePin,
            TabCommand::NewGroup,
            TabCommand::CopyTitle,
            TabCommand::Rename,
            TabCommand::Close,
        ]
    );
}

#[test]
fn every_command_that_applies_is_offered_in_menu_order() {
    let (backend, docs) = group_ids();
    let state = TabMenuState {
        can_reconnect: true,
        can_restart: true,
        can_mark_ready: true,
        pinned: true,
        has_custom_name: true,
        other_groups: vec![(backend, "Backend".into()), (docs, "Docs".into())],
        in_group: true,
        has_branch: true,
        has_folder: true,
        can_move_up: true,
        can_move_down: true,
        has_others_to_close: true,
        has_below_to_close: true,
        color: None,
    };
    assert_eq!(
        state.offered(),
        [
            TabCommand::Reconnect,
            TabCommand::Restart,
            TabCommand::MarkReady,
            TabCommand::Duplicate,
            TabCommand::TogglePin,
            TabCommand::NewGroup,
            TabCommand::MoveToGroup(backend),
            TabCommand::MoveToGroup(docs),
            TabCommand::RemoveFromGroup,
            TabCommand::CopyBranch,
            TabCommand::CopyTitle,
            TabCommand::CopyFolder,
            TabCommand::Rename,
            TabCommand::ResetName,
            TabCommand::Move(Direction::Up),
            TabCommand::Move(Direction::Down),
            TabCommand::Close,
            TabCommand::CloseOthers,
            TabCommand::CloseBelow,
        ]
    );
}

#[test]
fn a_group_is_offered_only_while_the_tab_can_move_to_it() {
    let (backend, docs) = group_ids();
    let state = TabMenuState {
        other_groups: vec![(docs, "Docs".into())],
        in_group: true,
        ..plain()
    };
    assert!(state.offers(TabCommand::MoveToGroup(docs)));
    assert!(
        !state.offers(TabCommand::MoveToGroup(backend)),
        "its own group"
    );
    assert!(state.offers(TabCommand::RemoveFromGroup));
    assert!(!plain().offers(TabCommand::RemoveFromGroup));
}

#[test]
fn labels_follow_the_tab() {
    let (backend, _) = group_ids();
    let state = TabMenuState {
        other_groups: vec![(backend, "Backend".into())],
        ..plain()
    };
    assert_eq!(state.label(TabCommand::TogglePin), "Pin tab");
    assert_eq!(state.label(TabCommand::MoveToGroup(backend)), "Backend");
    assert_eq!(state.label(TabCommand::CopyTitle), "Copy pane title");
    let pinned = TabMenuState {
        pinned: true,
        ..plain()
    };
    assert_eq!(pinned.label(TabCommand::TogglePin), "Unpin tab");
}
