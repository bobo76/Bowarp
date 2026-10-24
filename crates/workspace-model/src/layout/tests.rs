use super::*;

/// Tabs 1..=6: 1 pinned, then 2, group A [3, 4], 5, group B [6].
fn sample() -> (Layout, GroupId, GroupId) {
    let mut layout = Layout::default();
    for id in 1..=6 {
        layout.add(id, None);
    }
    layout.toggle_pin(1);
    let a = layout.new_group_with(3, "A").unwrap();
    layout.move_to_group(4, a);
    let b = layout.new_group_with(6, "B").unwrap();
    (layout, a, b)
}

#[test]
fn selection_survives_save_and_restore() {
    let (layout, _, _) = sample();
    // Pinned (1), loose (2, 5), in a group (3, 4, 6).
    for selected in 1..=6 {
        let (saved, index) = layout.save_selected(Some(selected), |id| Some(id * 10));
        assert_eq!(
            index,
            saved.tabs().iter().position(|&&id| id == selected * 10)
        );
        let (restored, picked) = saved.filter_map_selected(index, |id| Some(id / 10));
        assert_eq!(picked, Some(selected));
        assert_eq!(Layout::restore(restored).tabs(), layout.tabs());
    }
}

#[test]
fn dropped_tabs_before_the_selection_are_not_counted() {
    let (layout, _, _) = sample();
    // Saving drops 1 (pinned) and 3 (grouped), both before 4.
    let (saved, index) = layout.save_selected(Some(4), |id| (id != 1 && id != 3).then_some(id));
    assert_eq!(saved.tabs(), vec![&2, &4, &5, &6]);
    assert_eq!(index, Some(1));
    // Reopening drops 2, before the selected one.
    let (_, picked) = saved.filter_map_selected(index, |id| (id != 2).then_some(id));
    assert_eq!(picked, Some(4));
}

#[test]
fn a_dropped_selected_tab_gives_no_selection() {
    let (layout, _, _) = sample();
    let (saved, index) = layout.save_selected(Some(3), |id| (id != 3).then_some(id));
    assert_eq!(index, None);
    let (_, picked) = saved.filter_map_selected(Some(1), |id| (id != 2).then_some(id));
    assert_eq!(picked, None);
    let (_, picked) = layout.save(Some).filter_map_selected(None, Some);
    assert_eq!(picked, None);
}

#[test]
fn save_then_restore_keeps_pins_groups_and_order() {
    let (mut layout, a, _) = sample();
    {
        let group = layout.group_mut(a).unwrap();
        group.color = Some(TabColor::Green);
        group.collapsed = true;
    }
    let empty = layout.new_group_with(5, "Empty").unwrap();
    layout.remove_from_group(5);
    assert!(layout.group(empty).unwrap().tabs().is_empty());

    // Through JSON, with ids standing in for the saved tab data and renumbered on reopen.
    let json = serde_json::to_string(&layout.save(|id| Some(id * 10))).unwrap();
    let saved: SavedLayout<TabId> = serde_json::from_str(&json).unwrap();
    let restored = Layout::restore(saved.filter_map(|id| Some(id / 10)));

    assert_eq!(restored.tabs(), layout.tabs());
    assert_eq!(restored.pinned(), layout.pinned());
    let groups = |layout: &Layout| -> Vec<(String, Option<TabColor>, bool, Vec<TabId>)> {
        layout
            .groups()
            .map(|group| {
                let tabs = group.tabs().to_vec();
                (group.name.clone(), group.color, group.collapsed, tabs)
            })
            .collect()
    };
    assert_eq!(groups(&restored), groups(&layout));
    assert_eq!(restored.save(Some), layout.save(Some));
}

#[test]
fn restored_groups_get_distinct_ids_and_new_groups_dont_clash() {
    let (layout, _, _) = sample();
    let mut restored = Layout::restore(layout.save(Some));
    let ids: Vec<GroupId> = restored.groups().map(|group| group.id).collect();
    assert_eq!(ids.len(), 2);
    assert_ne!(ids[0], ids[1]);
    let new = restored.new_group_with(2, "C").unwrap();
    assert!(!ids.contains(&new));
}

#[test]
fn save_and_filter_map_leave_out_dropped_tabs_but_keep_groups() {
    let (layout, _, _) = sample();
    let saved = layout.save(|id| (id % 2 == 0).then_some(id));
    assert_eq!(saved.tabs(), vec![&2, &4, &6]);
    let restored = Layout::restore(saved.filter_map(|id| (id != 6).then_some(id)));
    assert_eq!(restored.tabs(), vec![2, 4]);
    assert_eq!(restored.groups().count(), 2);
    assert!(
        restored
            .groups()
            .any(|group| group.name == "B" && group.tabs().is_empty())
    );
}

#[test]
fn restore_keeps_a_duplicated_tab_only_once() {
    let saved = SavedLayout {
        pinned: vec![1],
        items: vec![
            SavedItem::Tab(1),
            SavedItem::Group(SavedGroup {
                name: "G".into(),
                color: None,
                collapsed: false,
                tabs: vec![2, 2],
            }),
        ],
    };
    let layout = Layout::restore(saved);
    assert_eq!(layout.tabs(), vec![1, 2]);
    assert!(layout.is_pinned(1));
}

#[test]
fn sample_layout() {
    let (layout, a, b) = sample();
    assert_eq!(layout.tabs(), vec![1, 2, 3, 4, 5, 6]);
    assert_eq!(layout.pinned(), &[1]);
    assert_eq!(layout.group(a).unwrap().tabs(), &[3, 4]);
    assert_eq!(layout.group(b).unwrap().tabs(), &[6]);
    assert_eq!(layout.group_of(4).map(|group| group.id), Some(a));
    assert_eq!(layout.group_of(5), None);
}

#[test]
fn moves_stay_inside_the_block() {
    let (mut layout, a, _) = sample();
    assert!(!layout.can_move(1, Direction::Up));
    assert!(!layout.can_move(1, Direction::Down));
    assert!(!layout.can_move(3, Direction::Up));
    assert!(layout.can_move(3, Direction::Down));
    assert!(!layout.can_move(4, Direction::Down));

    layout.move_tab(3, Direction::Down);
    assert_eq!(layout.group(a).unwrap().tabs(), &[4, 3]);
}

#[test]
fn ungrouped_tabs_step_over_whole_groups() {
    let (mut layout, a, _) = sample();
    layout.move_tab(2, Direction::Down);
    assert_eq!(layout.tabs(), vec![1, 3, 4, 2, 5, 6]);
    assert_eq!(layout.group(a).unwrap().tabs(), &[3, 4]);
    assert_eq!(layout.group_of(2), None);
}

#[test]
fn pinning_leaves_the_group_and_unpinning_goes_to_the_top() {
    let (mut layout, a, _) = sample();
    layout.toggle_pin(3);
    assert_eq!(layout.pinned(), &[1, 3]);
    assert_eq!(layout.group(a).unwrap().tabs(), &[4]);

    layout.toggle_pin(1);
    assert_eq!(layout.pinned(), &[3]);
    assert_eq!(layout.tabs(), vec![3, 1, 2, 4, 5, 6]);
}

#[test]
fn new_group_is_created_where_the_tab_was() {
    let (mut layout, a, _) = sample();
    let c = layout.new_group_with(4, "C").unwrap();
    // After its old group.
    assert_eq!(layout.tabs(), vec![1, 2, 3, 4, 5, 6]);
    assert_eq!(layout.group(a).unwrap().tabs(), &[3]);
    assert_eq!(layout.group(c).unwrap().tabs(), &[4]);

    // A pinned tab's group goes to the top of the list.
    let d = layout.new_group_with(1, "D").unwrap();
    assert!(layout.pinned().is_empty());
    assert!(matches!(&layout.items()[0], Item::Group(group) if group.id == d));
}

#[test]
fn groups_stay_when_empty() {
    let (mut layout, a, _) = sample();
    layout.remove(3);
    layout.remove_from_group(4);
    let group = layout.group(a).unwrap();
    assert!(group.tabs().is_empty());
    // The tab went just below its old group.
    assert_eq!(layout.tabs(), vec![1, 2, 4, 5, 6]);
    assert!(matches!(layout.items()[2], Item::Tab(4)));
}

#[test]
fn new_tabs_join_the_given_group() {
    let (mut layout, a, _) = sample();
    layout.add(7, Some(a));
    layout.add(8, None);
    assert_eq!(layout.group(a).unwrap().tabs(), &[3, 4, 7]);
    assert_eq!(layout.tabs(), vec![1, 2, 3, 4, 7, 5, 6, 8]);
}

#[test]
fn tabs_added_after_another_go_right_below_it() {
    let (mut layout, a, b) = sample();
    // In a group, mid-group and at its end.
    layout.add_after(7, 3);
    layout.add_after(8, 6);
    assert_eq!(layout.group(a).unwrap().tabs(), &[3, 7, 4]);
    assert_eq!(layout.group(b).unwrap().tabs(), &[6, 8]);
    // Ungrouped: right below, above the next group.
    layout.add_after(9, 2);
    assert!(matches!(layout.items()[1], Item::Tab(9)));
    assert_eq!(layout.tabs(), vec![1, 2, 9, 3, 7, 4, 5, 6, 8]);
}

#[test]
fn tabs_added_after_a_pinned_or_unknown_tab_go_to_the_end() {
    let (mut layout, _, _) = sample();
    layout.add_after(7, 1);
    layout.add_after(8, 42);
    assert_eq!(layout.pinned(), &[1]);
    assert_eq!(layout.tabs(), vec![1, 2, 3, 4, 5, 6, 7, 8]);
    assert_eq!(layout.group_of(7), None);
}

#[test]
fn drops_join_the_target_block() {
    let (mut layout, a, b) = sample();
    // Next to a grouped tab: joins that group.
    assert!(layout.drop_tab(2, DropTarget::After(3)));
    assert_eq!(layout.group(a).unwrap().tabs(), &[3, 2, 4]);
    // Next to a pinned tab: pinned.
    assert!(layout.drop_tab(5, DropTarget::Before(1)));
    assert_eq!(layout.pinned(), &[5, 1]);
    // Onto a group header: end of that group, unpinned.
    assert!(layout.drop_tab(1, DropTarget::IntoGroup(b)));
    assert_eq!(layout.group(b).unwrap().tabs(), &[6, 1]);
    assert_eq!(layout.pinned(), &[5]);
    // Above a group header and at the end: ungrouped.
    assert!(layout.drop_tab(4, DropTarget::BeforeGroup(a)));
    assert_eq!(layout.group_of(4), None);
    assert!(layout.drop_tab(3, DropTarget::End));
    assert_eq!(layout.group_of(3), None);
    assert_eq!(layout.tabs(), vec![5, 4, 2, 6, 1, 3]);
}

#[test]
fn drops_on_itself_or_missing_targets_do_nothing() {
    let (mut layout, _, _) = sample();
    assert!(!layout.drop_tab(3, DropTarget::Before(3)));
    assert!(!layout.drop_tab(3, DropTarget::After(99)));
    assert!(!layout.drop_tab(99, DropTarget::End));
    assert_eq!(layout.tabs(), vec![1, 2, 3, 4, 5, 6]);
}

#[test]
fn drop_change_detection_matches_the_resulting_layout() {
    let (initial, first_group, last_group) = sample();
    let targets: Vec<_> = (0..=7)
        .flat_map(|id| [DropTarget::Before(id), DropTarget::After(id)])
        .chain([
            DropTarget::BeforeGroup(first_group),
            DropTarget::BeforeGroup(last_group),
            DropTarget::IntoGroup(first_group),
            DropTarget::IntoGroup(last_group),
            DropTarget::End,
        ])
        .collect();
    for id in 0..=7 {
        for &target in &targets {
            let mut layout = initial.clone();
            let before = layout.save(Some);
            let changed = layout.drop_tab(id, target);
            assert_eq!(changed, before != layout.save(Some), "{id} -> {target:?}");
        }
    }
}

#[test]
fn groups_move_and_delete_in_place() {
    let (mut layout, a, b) = sample();
    assert!(!layout.can_move_group(b, Direction::Down));
    layout.move_group(a, Direction::Up);
    assert_eq!(layout.tabs(), vec![1, 3, 4, 2, 5, 6]);

    layout.delete_group(a);
    assert!(layout.group(a).is_none());
    assert_eq!(layout.tabs(), vec![1, 3, 4, 2, 5, 6]);
    assert_eq!(layout.group_of(3), None);
}

#[test]
fn reveal_expands_a_collapsed_group() {
    let (mut layout, a, _) = sample();
    layout.group_mut(a).unwrap().collapsed = true;
    assert!(!layout.reveal(2));
    assert!(layout.reveal(4));
    assert!(!layout.group(a).unwrap().collapsed);
    assert!(!layout.reveal(4));
}

#[test]
fn closing_spares_pinned_tabs() {
    let (layout, _, _) = sample();
    assert_eq!(layout.others_to_close(3), vec![2, 4, 5, 6]);
    assert_eq!(layout.below_to_close(4), vec![5, 6]);
    assert_eq!(layout.below_to_close(6), Vec::<TabId>::new());
    assert_eq!(layout.below_to_close(1), vec![2, 3, 4, 5, 6]);
}

#[test]
fn blank_group_names_keep_the_old_name() {
    let (mut layout, a, _) = sample();
    let group = layout.group_mut(a).unwrap();
    group.rename("  api  ");
    assert_eq!(group.name, "api");
    group.rename("  ");
    assert_eq!(group.name, "api");
}
