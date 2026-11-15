//! Remote sessions: a headless server runs sessions on one machine, and the app on another
//! shows them, through ssh. Sessions go on running on the server when the app disconnects,
//! and the app can attach to them again. See docs/remote.md.
//!
//! - [`protocol`]: the messages and their encoding.
//! - [`Server`]: runs the sessions and streams them to attached clients.
//! - [`Client`]: the app's side, showing each session in a [`term_core::Session::remote`].
//! - [`transport`]: the per-user daemon socket, `agentws-server proxy` and the ssh command.
//! - [`ClientIdentity`]: who the app is to a server, kept per install.
//! - [`status`]: `agentws-server status` and the app's check of a host's server.

// Long test scenarios are exempt from the function length limit (docs/code-quality.md).
#![cfg_attr(test, allow(clippy::too_many_lines))]

mod client;
mod identity;
pub mod protocol;
mod server;
pub mod status;
pub mod transport;

pub use client::{Client, DisconnectCallback, LocalView, RemoteSession};
pub use protocol::{ClientIdentity, SessionInfo, SpawnSpec};
pub use server::Server;
pub use transport::ssh_command;
