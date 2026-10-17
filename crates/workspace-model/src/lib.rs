//! What the workspace knows about its tabs beyond the terminal itself: where each session
//! works, which git branch that folder is on, what the user set on each tab (name, color), and
//! how tabs are laid out (pinned, groups, order), which tabs a sidebar search matches and which
//! tab wants attention next. No UI dependency.

mod attention;
mod git;
pub mod layout;
mod location;
mod search;
mod tab;

pub use attention::{Attention, next_attention};
pub use git::{GitHead, lines_added};
pub use layout::{DropTarget, Group, GroupId, Item, Layout, TabId};
pub use location::{Location, display_path};
pub use search::TabQuery;
pub use tab::{TabColor, TabMeta};
