//! What the workspace knows about its tabs beyond the terminal itself: where each session
//! works, which git branch that folder is on, and what the user set on each tab (name, pin,
//! color, order). No UI dependency.

mod git;
mod location;
pub mod order;
mod tab;

pub use git::GitHead;
pub use location::{Location, display_path};
pub use tab::{TabColor, TabMeta};
