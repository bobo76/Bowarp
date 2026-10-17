//! What the workspace knows about its tabs beyond the terminal itself: where each session
//! works and which git branch that folder is on. No UI dependency.

mod git;
mod location;

pub use git::GitHead;
pub use location::{Location, display_path};
