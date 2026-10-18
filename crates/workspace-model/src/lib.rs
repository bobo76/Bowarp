//! What the workspace knows about its tabs beyond the terminal itself: where each session
//! works, which git branch that folder is on, what the user set on each tab (name, color), and
//! how tabs are laid out (pinned, groups, order), which tabs a sidebar search matches and which
//! tab wants attention next, how long a session has been in its status, and where the window
//! and its tabs were when it closed, which running instance owns that save, which icon each
//! new-tab launcher shows, and the file explorer's tree. No UI dependency.

mod attention;
pub mod file_tree;
mod git;
mod instance_lock;
mod lap;
mod launcher_icon;
pub mod layout;
mod location;
mod search;
mod tab;
mod tabs_state;
mod window_state;

pub use attention::{Attention, next_attention, waiting_count};
pub use git::{GitHead, lines_added};
pub use instance_lock::{held_by_other, lock_pid, lock_text};
pub use lap::{Lap, format_lap, until_next_second};
pub use launcher_icon::{LauncherIcon, distro_icon, shell_icon};
pub use layout::{
    DropTarget, Group, GroupId, Item, Layout, SavedGroup, SavedItem, SavedLayout, TabId,
};
pub use location::{Location, display_path};
pub use search::{SearchedTab, TabQuery};
pub use tab::{TabColor, TabMeta};
pub use tabs_state::{SavedKind, SavedTab, TabsState};
pub use window_state::{Rect, WindowMode, WindowState};
