//! What the workspace knows about its tabs beyond the terminal itself: where each session
//! works, which git branch that folder is on, what the user set on each tab (name, color), and
//! how tabs are laid out (pinned, groups, order), which tabs a sidebar search matches and which
//! tab wants attention next, how long a session has been in its status, and where the window
//! and its tabs were when it closed, which running instance owns that save, which icon each
//! new-tab launcher shows, the file explorer's tree, how the file viewer shows a file, the
//! shell commands run in the app (history), the shell integration marks (OSC 133) in a shell's
//! output and the About dialog's version line. No UI dependency.

mod about;
mod attention;
pub mod file_tree;
pub mod file_view;
mod git;
mod history;
mod instance_lock;
mod lap;
mod launcher_icon;
pub mod layout;
mod location;
mod osc133;
mod search;
mod tab;
mod tabs_state;
mod window_state;

pub use about::version_line;
pub use attention::{Attention, next_attention, waiting_count};
pub use git::{GitHead, lines_added};
pub use history::{
    CommandHistory, HistoryEntry, HistorySelection, append_entry, entry_line, is_recordable,
};
pub use instance_lock::{held_by_other, lock_pid, lock_text};
pub use lap::{Lap, format_lap, until_next_second};
pub use launcher_icon::{LauncherIcon, distro_icon, shell_icon};
pub use layout::{
    DropTarget, Group, GroupId, Item, Layout, SavedGroup, SavedItem, SavedLayout, TabId,
};
pub use location::{Location, display_path};
pub use osc133::{CommandLine, Osc133Scanner, PromptMark, new_mark_token};
pub use search::{SearchedTab, TabQuery};
pub use tab::{TabColor, TabMeta};
pub use tabs_state::{SavedKind, SavedTab, TabsState};
pub use window_state::{Rect, WindowMode, WindowState};
