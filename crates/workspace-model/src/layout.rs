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

    /// Like `filter_map`, plus the new value of the tab at index `selected` in `tabs()`, when
    /// it was kept.
    pub fn filter_map_selected<U: Clone>(
        self,
        selected: Option<usize>,
        mut f: impl FnMut(T) -> Option<U>,
    ) -> (SavedLayout<U>, Option<U>) {
        let mut index = 0;
        let mut picked = None;
        let layout = self.filter_map(|tab| {
            let this_index = index;
            index += 1;
            let new = f(tab)?;
            if selected == Some(this_index) {
                picked = Some(new.clone());
            }
            Some(new)
        });
        (layout, picked)
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

    /// Add a new tab ungrouped at the end of the list.
    pub fn add(&mut self, id: TabId) {
        self.items.push(Item::Tab(id));
    }

    /// Add a new tab right below `after`, in its group or among the ungrouped tabs. Below a
    /// pinned tab it goes ungrouped right after the pinned tabs; below an unknown tab,
    /// ungrouped at the end of the list, like `add`.
    pub fn add_after(&mut self, id: TabId, after: TabId) {
        match self.place(after) {
            Some(Place::Loose(index)) => self.items.insert(index + 1, Item::Tab(id)),
            Some(Place::Grouped { item, index }) => {
                if let Item::Group(group) = &mut self.items[item] {
                    group.tabs.insert(index + 1, id);
                }
            }
            Some(Place::Pinned(_)) => self.items.insert(0, Item::Tab(id)),
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
        let Some(before) = self.place(id) else {
            return false;
        };
        if !self.accepts_drop(id, target) {
            return false;
        }
        self.take(id);
        self.insert_drop(id, target);
        Some(before) != self.place(id)
    }

    fn accepts_drop(&self, id: TabId, target: DropTarget) -> bool {
        match target {
            DropTarget::Before(other) | DropTarget::After(other) => {
                id != other && self.place(other).is_some()
            }
            DropTarget::BeforeGroup(group) | DropTarget::IntoGroup(group) => {
                self.group(group).is_some()
            }
            DropTarget::End => true,
        }
    }

    fn insert_drop(&mut self, id: TabId, target: DropTarget) {
        match target {
            DropTarget::Before(other) | DropTarget::After(other) => {
                let after = usize::from(matches!(target, DropTarget::After(_)));
                self.insert_next_to(id, other, after);
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
    }

    fn insert_next_to(&mut self, id: TabId, other: TabId, after: usize) {
        match self.place(other).expect("drop target was checked") {
            Place::Pinned(index) => self.pinned.insert(index + after, id),
            Place::Loose(index) => self.items.insert(index + after, Item::Tab(id)),
            Place::Grouped { item, index } => {
                let Item::Group(group) = &mut self.items[item] else {
                    unreachable!("grouped place must point at a group");
                };
                group.tabs.insert(index + after, id);
            }
        }
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

    /// Like `save`, plus the index of `selected` in the saved layout's `tabs()`, when it was
    /// kept. Tabs left out are not counted.
    pub fn save_selected<T>(
        &self,
        selected: Option<TabId>,
        mut f: impl FnMut(TabId) -> Option<T>,
    ) -> (SavedLayout<T>, Option<usize>) {
        let mut kept = 0;
        let mut index = None;
        let layout = self.save(|id| {
            let saved = f(id)?;
            if selected == Some(id) {
                index = Some(kept);
            }
            kept += 1;
            Some(saved)
        });
        (layout, index)
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
mod tests;
