//! Terminal session core: a pty-backed child process feeding an alacritty `Term`.
//!
//! This crate has no UI dependency. A renderer locks the terminal to read the grid and is
//! notified through a [`SessionEvent`] callback; wakeups are coalesced so a burst of output
//! produces a single [`SessionEvent::Wakeup`] until the renderer calls [`Session::ack_wakeup`].

// Long test scenarios are exempt from the function length limit (docs/code-quality.md).
#![cfg_attr(test, allow(clippy::too_many_lines))]

mod launch;
mod palette;
mod prompt;
mod search;
mod session;

pub use alacritty_terminal;
pub use launch::LaunchSpec;
pub use palette::{Palette, dim};
pub use prompt::{ExecutedCommand, PromptEvent, PromptInput};
pub use search::TerminalSearch;
pub use session::{EventCallback, Session, SessionEvent, SessionListener, TermDimensions};
pub use workspace_model::SessionKind;
