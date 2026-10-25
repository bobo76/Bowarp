//! What the workspace knows about its tabs beyond the terminal itself: where each session
//! works, which git branch that folder is on, what the user set on each tab (name, color), and
//! how tabs are laid out (pinned, groups, order), which tabs a sidebar search matches and which
//! tab wants attention next, how long a session has been in its status, and where the window
//! and its tabs were when it closed, which running instance owns that save, which icon each
//! new-tab launcher shows, the file explorer's tree, how the file viewer shows a file and how
//! wide it is, the links in terminal output and documents, the
//! shell commands run in the app (history), the shell integration marks (OSC 133) in a shell's
//! output, the progress reports (OSC 9;4) in a program's output, how a running command is
//! labeled and badged, which tabs closing would interrupt, which clipboard writes wait for the
//! user, how far the mouse wheel scrolls,
//! the About dialog's version line, the user's settings file and keyboard shortcuts, and the
//! window background's opacity and blur. No UI
//! dependency.

// Long test scenarios are exempt from the function length limit (docs/code-quality.md).
#![cfg_attr(test, allow(clippy::too_many_lines))]

mod about;
mod attention;
mod clipboard_guard;
mod close_warning;
mod file_pane_features;
pub mod file_tree;
pub mod file_view;
mod git;
mod history;
mod history_features;
mod instance_lock;
mod keymap;
mod lap;
mod last_command;
mod launcher_icon;
pub mod layout;
pub mod links;
mod location;
mod notification_features;
mod osc133;
pub mod pane_width;
mod platform;
mod progress;
mod running;
mod search;
mod settings;
mod tab;
mod tabs_state;
mod time;
mod wheel;
mod window_background;
mod window_state;
mod workspace_features;

pub use about::version_line;
pub use attention::{Attention, next_attention, waiting_count};
pub use clipboard_guard::{
    CLIPBOARD_PREVIEW_CHARS, ClipboardAnswer, ClipboardGuard, ClipboardNotice, ClipboardOffer,
    ClipboardPolicy, ClipboardTarget, ClipboardWrites,
};
pub use close_warning::{
    AgentActivity, BusyReason, BusyTab, CloseTarget, CloseWarning, TabActivity, close_warning,
};
pub use file_pane_features::{
    FileLinkAction, FilePaneFeatures, PaneChange, PaneEntry, pane_change,
};
pub use git::{GitHead, lines_added};
pub use history::{
    CommandHistory, HistoryEntry, HistorySelection, append_entry, clear_history_file, entry_line,
    is_recordable, remove_command,
};
pub use history_features::{HistoryFeatures, HistoryKey};
pub use instance_lock::{held_by_other, lock_pid, lock_text};
pub use keymap::{
    HeldModifiers, KeybindingOverlay, Keymap, KeymapProblem, KeymapReason, Shortcut, action_label,
    is_terminal_input, short_name,
};
pub use lap::{Lap, format_lap, until_next_second};
pub use last_command::LastCommand;
pub use launcher_icon::{LauncherIcon, distro_icon, shell_icon};
pub use layout::{
    DropTarget, Group, GroupId, Item, Layout, SavedGroup, SavedItem, SavedLayout, TabId,
};
pub use location::{Location, display_path};
pub use notification_features::{NotificationFeatures, ToastKind, exit_toast_kind};
pub use osc133::{CommandLine, Osc133Scanner, PromptMark, new_mark_token};
pub use platform::Platform;
pub use progress::{ProgressScanner, ProgressState};
pub use running::{
    COMMAND_LABEL_CHARS, RUNNING_AFTER, RUNNING_PULSE_FOR, RunningBadge, command_label,
    format_clock, running_badge,
};
pub use search::{SearchedTab, TabQuery, step_match};
pub use settings::{
    DependentState, FeatureSwitch, FontSize, Settings, SettingsFile, SettingsLoad, TabCardField,
    TabCardFields, UiTheme, installed_font_family, settings_backup_path,
};
pub use tab::{TabColor, TabMeta};
pub use tabs_state::{SavedKind, SavedTab, TabsState};
pub use wheel::{WheelScroll, notch_lines};
pub use window_background::{
    BLUR_NEEDS_OPACITY, BLUR_UNAVAILABLE, Backdrop, BlurControl, BlurSupport, MICA_FALLBACK,
    MICA_MIN_WINDOWS_BUILD, WindowBlur, WindowOpacity, WindowsSwitch, blur_options, shown_blur,
};
pub use window_state::{Rect, WindowMode, WindowState};
pub use workspace_features::WorkspaceFeatures;
