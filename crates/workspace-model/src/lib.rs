//! What the workspace knows about its tabs beyond the terminal itself: where each session
//! works, which git branch that folder is on, what the user set on each tab (name, color), and
//! how tabs are laid out (pinned, groups, order), which tabs a sidebar search matches and which
//! tab wants attention next, how long a session has been in its status, and where the window
//! was when it closed. No UI dependency.

mod attention;
mod git;
mod lap;
pub mod layout;
mod location;
mod search;
mod tab;
mod window_state;

pub use attention::{Attention, next_attention, waiting_count};
pub use git::{GitHead, lines_added};
pub use lap::{Lap, format_lap, until_next_second};
pub use layout::{DropTarget, Group, GroupId, Item, Layout, TabId};
pub use location::{Location, display_path};
pub use search::{SearchedTab, TabQuery};
pub use tab::{TabColor, TabMeta};
pub use window_state::{Rect, WindowMode, WindowState};
