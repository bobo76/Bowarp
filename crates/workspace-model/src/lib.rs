//! What the workspace knows about its tabs beyond the terminal itself: where each session
//! works, which git branch that folder is on, what the user set on each tab (name, color), and
//! how tabs are laid out (pinned, groups, order), which tabs a sidebar search matches and which
//! tab wants attention next, how long a session has been in its status, and where the window
//! and its tabs were when it closed, which running instance owns that save, which icon each
//! new-tab launcher shows, the file explorer's tree, how the file viewer shows a file and how
//! wide it is, the links in terminal output and documents, the
//! shell commands run in the app (history), the shell integration marks (OSC 133) in a shell's
//! output, the progress reports (OSC 9;4) in a program's output, how a running command is
//! labeled and badged, how far the mouse wheel scrolls and the About dialog's version line. No
//! UI dependency.

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
pub mod links;
mod location;
mod osc133;
pub mod pane_width;
mod progress;
mod running;
mod search;
mod tab;
mod tabs_state;
mod wheel;
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
pub use progress::{ProgressScanner, ProgressState};
pub use running::{
    COMMAND_LABEL_CHARS, RUNNING_AFTER, RUNNING_PULSE_FOR, RunningBadge, command_label,
    format_clock, running_badge,
};
pub use search::{SearchedTab, TabQuery, step_match};
pub use tab::{TabColor, TabMeta};
pub use tabs_state::{SavedKind, SavedTab, TabsState};
pub use wheel::{WheelScroll, notch_lines};
pub use window_state::{Rect, WindowMode, WindowState};
