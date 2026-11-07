//! Remote sessions: a headless server runs sessions on one machine, and the app on another
//! shows them, through ssh. Sessions go on running on the server when the app disconnects,
//! and the app can attach to them again. See docs/remote.md.
//!
//! - [`protocol`]: the messages and their encoding.
//! - [`Server`]: runs the sessions and streams them to attached clients.
//! - [`Client`] / [`RemoteSession`]: the app's side, showing each session in a
//!   [`term_core::Mirror`].
//! - [`transport`]: the per-user daemon socket, `agentws-server proxy` and the ssh command.

// Long test scenarios are exempt from the function length limit (docs/code-quality.md).
#![cfg_attr(test, allow(clippy::too_many_lines))]

mod client;
pub mod protocol;
mod server;
pub mod transport;

pub use client::{Client, DisconnectCallback, RemoteSession};
pub use protocol::{SessionInfo, SpawnSpec};
pub use server::Server;
pub use transport::ssh_command;
