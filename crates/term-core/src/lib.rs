//! Terminal session core: a pty-backed child process feeding an alacritty `Term`.
//!
//! This crate has no UI dependency. A renderer locks the terminal to read the grid and is
//! notified through a [`SessionEvent`] callback; wakeups are coalesced so a burst of output
//! produces a single [`SessionEvent::Wakeup`] until the renderer calls [`Session::ack_wakeup`].

mod launch;
mod palette;
mod prompt;
mod session;

pub use alacritty_terminal;
pub use launch::{LaunchSpec, SessionKind};
pub use palette::{Palette, dim};
pub use prompt::{ExecutedCommand, PromptInput};
pub use session::{EventCallback, Session, SessionEvent, SessionListener, TermDimensions};
