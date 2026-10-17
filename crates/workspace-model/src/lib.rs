//! What the workspace knows about its tabs beyond the terminal itself: where each session
//! works, which git branch that folder is on, what the user set on each tab (name, color), and
//! how tabs are laid out (pinned, groups, order). No UI dependency.

mod git;
pub mod layout;
mod location;
mod tab;

pub use git::GitHead;
pub use layout::{DropTarget, Group, GroupId, Item, Layout, TabId};
pub use location::{Location, display_path};
pub use tab::{TabColor, TabMeta};
