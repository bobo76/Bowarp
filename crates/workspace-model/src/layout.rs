//! Where tabs sit in the sidebar: pinned tabs at the top, then ungrouped tabs and groups in the
//! user's order. Pinned tabs are never in a group; groups stay when their last tab leaves.
//! Tabs are addressed by session id, so callers never track indices across moves.

use std::collections::HashSet;

use serde::{Deserialize, Serialize};

use crate::TabColor;

pub type TabId = u64;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct GroupId(u64);

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Direction {
    Up,
    Down,
}

/// A named, collapsible group of tabs.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Group {
    pub id: GroupId,
    pub name: String,
    /// Shown by member tabs that have no color of their own.
    pub color: Option<TabColor>,
    pub collapsed: bool,
    tabs: Vec<TabId>,
}

impl Group {
    pub fn tabs(&self) -> &[TabId] {
        &self.tabs
    }

    /// Set the name from user input; blank input keeps the current name.
    pub fn rename(&mut self, input: &str) {
        let name = input.trim();
        if !name.is_empty() {
            self.name = name.to_string();
        }
    }
}

/// An entry below the pinned block.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Item {
    Tab(TabId),
    Group(Group),
}

/// Where a dragged tab lands.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DropTarget {
    /// Next to a tab, in that tab's block: pinned, a group, or ungrouped.
    Before(TabId),
    After(TabId),
    /// Ungrouped, just above the group's header.
    BeforeGroup(GroupId),
    /// At the end of the group.
    IntoGroup(GroupId),
    /// Ungrouped, at the end of the list.
    End,
}

/// A layout as saved to disk, with each tab's saved data (`T`) in its place instead of an id,
/// so a saved file cannot point at a tab it doesn't hold. Group ids are not saved.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(bound(deserialize = "T: Deserialize<'de>"))]
pub struct SavedLayout<T> {
    #[serde(default)]
    pub pinned: Vec<T>,
    #[serde(default)]
    pub items: Vec<SavedItem<T>>,
}

// Not derived: the derive would require `T: Default`.
impl<T> Default for SavedLayout<T> {
    fn default() -> Self {
        Self {
            pinned: Vec::new(),
            items: Vec::new(),
        }
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
#[serde(bound(deserialize = "T: Deserialize<'de>"))]
pub enum SavedItem<T> {
    Tab(T),
    Group(SavedGroup<T>),
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(bound(deserialize = "T: Deserialize<'de>"))]
pub struct SavedGroup<T> {
    pub name: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub color: Option<TabColor>,
    #[serde(default)]
    pub collapsed: bool,
    #[serde(default)]
    pub tabs: Vec<T>,
}

impl<T> SavedLayout<T> {
    /// Every tab, top to bottom (the order of `Layout::tabs`).
    pub fn tabs(&self) -> Vec<&T> {
        let mut tabs: Vec<&T> = self.pinned.iter().collect();
        for item in &self.items {
            match item {
                SavedItem::Tab(tab) => tabs.push(tab),
                SavedItem::Group(group) => tabs.extend(&group.tabs),
            }
        }
        tabs
    }

    /// Replace each tab by `f(tab)`, top to bottom; tabs it maps to `None` are left out. Groups
    /// stay, even when left empty.
    pub fn filter_map<U>(self, mut f: impl FnMut(T) -> Option<U>) -> SavedLayout<U> {
        let pinned = self.pinned.into_iter().filter_map(&mut f).collect();
        let items = self
            .items
            .into_iter()
            .filter_map(|item| match item {
                SavedItem::Tab(tab) => f(tab).map(SavedItem::Tab),
                SavedItem::Group(group) => Some(SavedItem::Group(SavedGroup {
                    name: group.name,
                    color: group.color,
                    collapsed: group.collapsed,
                    tabs: group.tabs.into_iter().filter_map(&mut f).collect(),
                })),
            })
            .collect();
        SavedLayout { pinned, items }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Place {
    Pinned(usize),
    Loose(usize),
    Grouped { item: usize, index: usize },
}

#[derive(Clone, Debug, Default)]
pub struct Layout {
    pinned: Vec<TabId>,
    items: Vec<Item>,
    next_group_id: u64,
}

impl Layout {
    pub fn pinned(&self) -> &[TabId] {
        &self.pinned
    }

    pub fn items(&self) -> &[Item] {
        &self.items
    }

    /// Every tab, top to bottom, including tabs in collapsed groups.
    pub fn tabs(&self) -> Vec<TabId> {
        let mut tabs = self.pinned.clone();
        for item in &self.items {
            match item {
                Item::Tab(id) => tabs.push(*id),
                Item::Group(group) => tabs.extend(&group.tabs),
            }
        }
        tabs
    }

    /// Add a new tab at the end of `group`, else ungrouped at the end of the list.
    pub fn add(&mut self, id: TabId, group: Option<GroupId>) {
        match group.and_then(|group| self.group_mut(group)) {
            Some(group) => group.tabs.push(id),
            None => self.items.push(Item::Tab(id)),
        }
    }

    /// Take a closed tab out. Its group stays, even when empty.
    pub fn remove(&mut self, id: TabId) {
        self.take(id);
    }

    pub fn is_pinned(&self, id: TabId) -> bool {
        self.pinned.contains(&id)
    }

    pub fn group(&self, id: GroupId) -> Option<&Group> {
        self.items.iter().find_map(|item| match item {
            Item::Group(group) if group.id == id => Some(group),
            _ => None,
        })
    }

    pub fn group_mut(&mut self, id: GroupId) -> Option<&mut Group> {
        self.items.iter_mut().find_map(|item| match item {
            Item::Group(group) if group.id == id => Some(group),
            _ => None,
        })
    }

    pub fn groups(&self) -> impl Iterator<Item = &Group> {
        self.items.iter().filter_map(|item| match item {
            Item::Group(group) => Some(group),
            Item::Tab(_) => None,
        })
    }

    pub fn group_of(&self, id: TabId) -> Option<&Group> {
        match self.place(id)? {
            Place::Grouped { item, .. } => match &self.items[item] {
                Item::Group(group) => Some(group),
                Item::Tab(_) => None,
            },
            _ => None,
        }
    }

    /// Whether the tab can move one step: it stays inside its block, and an ungrouped tab
    /// steps over a whole group.
    pub fn can_move(&self, id: TabId, direction: Direction) -> bool {
        let Some(place) = self.place(id) else {
            return false;
        };
        let (index, len) = match place {
            Place::Pinned(index) => (index, self.pinned.len()),
            Place::Loose(index) => (index, self.items.len()),
            Place::Grouped { item, index } => (index, self.group_len(item)),
        };
        step(index, len, direction).is_some()
    }

    pub fn move_tab(&mut self, id: TabId, direction: Direction) {
        let Some(place) = self.place(id) else {
            return;
        };
        match place {
            Place::Pinned(index) => swap_step(&mut self.pinned, index, direction),
            Place::Loose(index) => swap_step(&mut self.items, index, direction),
            Place::Grouped { item, index } => {
                if let Item::Group(group) = &mut self.items[item] {
                    swap_step(&mut group.tabs, index, direction);
                }
            }
        }
    }

    /// Pinning goes to the end of the pinned block (leaving any group), unpinning to the top of
    /// the ungrouped list.
    pub fn toggle_pin(&mut self, id: TabId) {
        match self.take(id) {
            Some(Place::Pinned(_)) => self.items.insert(0, Item::Tab(id)),
            Some(_) => self.pinned.push(id),
            None => {}
        }
    }

    /// Put the tab in a new group, created where the tab was (after its old group, or at the
    /// top of the list for a pinned tab).
    pub fn new_group_with(&mut self, id: TabId, name: &str) -> Option<GroupId> {
        let at = match self.take(id)? {
            Place::Pinned(_) => 0,
            Place::Loose(item) => item,
            Place::Grouped { item, .. } => item + 1,
        };
        let group_id = GroupId(self.next_group_id);
        self.next_group_id += 1;
        let group = Group {
            id: group_id,
            name: name.to_string(),
            color: None,
            collapsed: false,
            tabs: vec![id],
        };
        self.items.insert(at, Item::Group(group));
        Some(group_id)
    }

    pub fn move_to_group(&mut self, id: TabId, group: GroupId) {
        self.drop_tab(id, DropTarget::IntoGroup(group));
    }

    /// Take the tab out of its group, ungrouped just below the group.
    pub fn remove_from_group(&mut self, id: TabId) {
        let Some(Place::Grouped { item, .. }) = self.place(id) else {
            return;
        };
        self.take(id);
        self.items.insert(item + 1, Item::Tab(id));
    }

    /// Move the tab to `target`, joining the target's block. Returns whether anything moved.
    pub fn drop_tab(&mut self, id: TabId, target: DropTarget) -> bool {
        let target_tab = match target {
            DropTarget::Before(other) | DropTarget::After(other) => Some(other),
            _ => None,
        };
        let target_exists = match target {
            DropTarget::Before(other) | DropTarget::After(other) => self.place(other).is_some(),
            DropTarget::BeforeGroup(group) | DropTarget::IntoGroup(group) => {
                self.group(group).is_some()
            }
            DropTarget::End => true,
        };
        if target_tab == Some(id) || !target_exists || self.place(id).is_none() {
            return false;
        }
        let before = (self.pinned.clone(), self.items.clone());
        self.take(id);
        match target {
            DropTarget::Before(other) | DropTarget::After(other) => {
                let after = usize::from(matches!(target, DropTarget::After(_)));
                match self.place(other) {
                    Some(Place::Pinned(index)) => self.pinned.insert(index + after, id),
                    Some(Place::Loose(index)) => self.items.insert(index + after, Item::Tab(id)),
                    Some(Place::Grouped { item, index }) => {
                        if let Item::Group(group) = &mut self.items[item] {
                            group.tabs.insert(index + after, id);
                        }
                    }
                    None => unreachable!("the target tab was checked above"),
                }
            }
            DropTarget::BeforeGroup(group) => {
                let index = self.group_index(group).expect("checked above");
                self.items.insert(index, Item::Tab(id));
            }
            DropTarget::IntoGroup(group) => {
                self.group_mut(group).expect("checked above").tabs.push(id);
            }
            DropTarget::End => self.items.push(Item::Tab(id)),
        }
        // Dropping right next to itself in the same block changes nothing.
        before != (self.pinned.clone(), self.items.clone())
    }

    pub fn can_move_group(&self, id: GroupId, direction: Direction) -> bool {
        self.group_index(id)
            .is_some_and(|index| step(index, self.items.len(), direction).is_some())
    }

    pub fn move_group(&mut self, id: GroupId, direction: Direction) {
        if let Some(index) = self.group_index(id) {
            swap_step(&mut self.items, index, direction);
        }
    }

    /// Delete the group; its tabs stay, ungrouped, where the group was.
    pub fn delete_group(&mut self, id: GroupId) {
        let Some(index) = self.group_index(id) else {
            return;
        };
        let Item::Group(group) = self.items.remove(index) else {
            return;
        };
        let tabs = group.tabs.into_iter().map(Item::Tab);
        self.items.splice(index..index, tabs);
    }

    /// Expand the tab's group if it is collapsed, so the tab shows. Returns whether it changed.
    pub fn reveal(&mut self, id: TabId) -> bool {
        let Some(Place::Grouped { item, .. }) = self.place(id) else {
            return false;
        };
        match &mut self.items[item] {
            Item::Group(group) if group.collapsed => {
                group.collapsed = false;
                true
            }
            _ => false,
        }
    }

    /// Tabs closed by "close other tabs" on `id`: every other unpinned tab.
    pub fn others_to_close(&self, id: TabId) -> Vec<TabId> {
        self.tabs()
            .into_iter()
            .filter(|&other| other != id && !self.is_pinned(other))
            .collect()
    }

    /// Tabs closed by "close tabs below" on `id`: the unpinned tabs after it.
    pub fn below_to_close(&self, id: TabId) -> Vec<TabId> {
        self.tabs()
            .into_iter()
            .skip_while(|&other| other != id)
            .skip(1)
            .filter(|&other| !self.is_pinned(other))
            .collect()
    }

    /// The layout to save, with `f(id)` in each tab's place; tabs it maps to `None` are left
    /// out.
    pub fn save<T>(&self, mut f: impl FnMut(TabId) -> Option<T>) -> SavedLayout<T> {
        SavedLayout {
            pinned: self.pinned.iter().filter_map(|&id| f(id)).collect(),
            items: self
                .items
                .iter()
                .filter_map(|item| match item {
                    Item::Tab(id) => f(*id).map(SavedItem::Tab),
                    Item::Group(group) => Some(SavedItem::Group(SavedGroup {
                        name: group.name.clone(),
                        color: group.color,
                        collapsed: group.collapsed,
                        tabs: group.tabs.iter().filter_map(|&id| f(id)).collect(),
                    })),
                })
                .collect(),
        }
    }

    /// The layout of a saved one whose tabs were reopened as `TabId`s. A tab listed twice is
    /// kept only where it first appears.
    pub fn restore(saved: SavedLayout<TabId>) -> Self {
        let mut seen = HashSet::new();
        let saved = saved.filter_map(|id| seen.insert(id).then_some(id));
        let mut layout = Self {
            pinned: saved.pinned,
            ..Self::default()
        };
        for item in saved.items {
            let item = match item {
                SavedItem::Tab(id) => Item::Tab(id),
                SavedItem::Group(group) => {
                    let id = GroupId(layout.next_group_id);
                    layout.next_group_id += 1;
                    Item::Group(Group {
                        id,
                        name: group.name,
                        color: group.color,
                        collapsed: group.collapsed,
                        tabs: group.tabs,
                    })
                }
            };
            layout.items.push(item);
        }
        layout
    }

    fn place(&self, id: TabId) -> Option<Place> {
        if let Some(index) = self.pinned.iter().position(|&tab| tab == id) {
            return Some(Place::Pinned(index));
        }
        self.items
            .iter()
            .enumerate()
            .find_map(|(item_index, item)| match item {
                Item::Tab(tab) if *tab == id => Some(Place::Loose(item_index)),
                Item::Group(group) => {
                    group
                        .tabs
                        .iter()
                        .position(|&tab| tab == id)
                        .map(|index| Place::Grouped {
                            item: item_index,
                            index,
                        })
                }
                Item::Tab(_) => None,
            })
    }

    /// Remove the tab from wherever it is, returning where that was.
    fn take(&mut self, id: TabId) -> Option<Place> {
        let place = self.place(id)?;
        match place {
            Place::Pinned(index) => {
                self.pinned.remove(index);
            }
            Place::Loose(index) => {
                self.items.remove(index);
            }
            Place::Grouped { item, index } => {
                if let Item::Group(group) = &mut self.items[item] {
                    group.tabs.remove(index);
                }
            }
        }
        Some(place)
    }

    fn group_index(&self, id: GroupId) -> Option<usize> {
        self.items
            .iter()
            .position(|item| matches!(item, Item::Group(group) if group.id == id))
    }

    fn group_len(&self, item: usize) -> usize {
        match &self.items[item] {
            Item::Group(group) => group.tabs.len(),
            Item::Tab(_) => 0,
        }
    }
}

fn step(index: usize, len: usize, direction: Direction) -> Option<usize> {
    let target = match direction {
        Direction::Up => index.checked_sub(1)?,
        Direction::Down => index + 1,
    };
    (target < len).then_some(target)
}

fn swap_step<T>(list: &mut [T], index: usize, direction: Direction) {
    if let Some(target) = step(index, list.len(), direction) {
        list.swap(index, target);
    }
}

#[cfg(test)]
mod tests {
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
}
