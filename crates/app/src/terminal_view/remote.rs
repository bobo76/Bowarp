//! A remote tab whose connection was lost (docs/remote.md): the program may still run on its
//! host. The banner below the terminal offers Reconnect, or Start new once the server no
//! longer has the session; the workspace does both ([`RemoteAction`]).

use gpui_kit::component::button::{Button, ButtonVariants};
use gpui_kit::component::{ActiveTheme, Sizable};
use gpui_kit::prelude::FluentBuilder;
use gpui_kit::{AnyElement, Context, IntoElement, ParentElement, Styled, div};
use term_core::LaunchSpec;

use super::{ProcessState, SessionEvents, TabChanged, TerminalView};
use crate::remote_hosts::Reattach;

/// Asked from a disconnected remote tab's banner, done by the workspace.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RemoteAction {
    /// Attach to the session on the server again.
    Reconnect,
    /// The server no longer has the session: start the same program there again.
    StartNew,
}

/// Why a remote tab's session stopped showing, and what is being done about it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Disconnect {
    /// Why the connection ended, as the client reported it.
    pub reason: String,
    pub phase: DisconnectPhase,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DisconnectPhase {
    /// Waiting for the user to reconnect.
    Lost,
    /// A reconnect is under way.
    Reconnecting,
    /// The server no longer has the session.
    Gone,
    /// After that, Start new is under way.
    Starting,
}

impl Disconnect {
    pub fn lost(reason: String) -> Self {
        Self {
            reason,
            phase: DisconnectPhase::Lost,
        }
    }

    /// The banner's text for a tab on `host`.
    pub fn message(&self, host: &str) -> String {
        match self.phase {
            DisconnectPhase::Lost => format!("Connection to {host} lost: {}", self.reason),
            DisconnectPhase::Reconnecting => format!("Reconnecting to {host}…"),
            DisconnectPhase::Gone => {
                format!("The session is gone from {host} (closed there, or its server restarted)")
            }
            DisconnectPhase::Starting => format!("Starting a new session on {host}…"),
        }
    }

    /// The banner's button, if any.
    pub fn action(&self) -> Option<(&'static str, RemoteAction)> {
        match self.phase {
            DisconnectPhase::Lost => Some(("Reconnect", RemoteAction::Reconnect)),
            DisconnectPhase::Reconnecting | DisconnectPhase::Starting => None,
            DisconnectPhase::Gone => Some(("Start new", RemoteAction::StartNew)),
        }
    }
}

impl TerminalView {
    /// The connection to the server was lost: the program may go on there.
    pub(super) fn on_disconnected(&mut self, reason: String, cx: &mut Context<Self>) {
        self.process = ProcessState::Disconnected(Disconnect::lost(reason));
        self.running_command = None;
        // Nobody is left to ask on behalf of.
        self.clipboard.deny();
        self.subagents.clear();
        cx.emit(TabChanged);
    }

    /// The tab lost its connection and waits for Reconnect (not while reconnecting, nor once
    /// its session is gone).
    pub fn can_reconnect(&self) -> bool {
        matches!(
            &self.process,
            ProcessState::Disconnected(Disconnect {
                phase: DisconnectPhase::Lost,
                ..
            })
        )
    }

    /// What attaching again to this tab's session (launched as `spec`) as session `id`
    /// needs, and its events; marks the tab as reconnecting. `None` unless
    /// [`Self::can_reconnect`].
    pub fn begin_reconnect(
        &mut self,
        id: u64,
        spec: &LaunchSpec,
        cx: &mut Context<Self>,
    ) -> Option<(SessionEvents, Reattach)> {
        if !self.can_reconnect() {
            return None;
        }
        let server_id = self.session.remote_id()?;
        let events = SessionEvents::new(spec, cx);
        let reattach = Reattach {
            server_id,
            // The old size: the snapshot then fits until the next layout resizes it.
            size: self.session.size(),
            view: events.local_view(id),
        };
        self.set_disconnect_phase(DisconnectPhase::Reconnecting, cx);
        Some((events, reattach))
    }

    /// Marks the tab as starting a new session, once its old one is gone; `false` (nothing
    /// done) otherwise.
    pub fn begin_start_new(&mut self, cx: &mut Context<Self>) -> bool {
        let gone = matches!(
            &self.process,
            ProcessState::Disconnected(Disconnect {
                phase: DisconnectPhase::Gone,
                ..
            })
        );
        if gone {
            self.set_disconnect_phase(DisconnectPhase::Starting, cx);
        }
        gone
    }

    /// The reconnect or new start failed (e.g. host unreachable: Reconnect again), or the
    /// server no longer has the session (`gone`: Start new).
    pub fn end_reconnect(&mut self, gone: bool, cx: &mut Context<Self>) {
        let phase = if gone {
            DisconnectPhase::Gone
        } else {
            DisconnectPhase::Lost
        };
        self.set_disconnect_phase(phase, cx);
    }

    fn set_disconnect_phase(&mut self, phase: DisconnectPhase, cx: &mut Context<Self>) {
        if let ProcessState::Disconnected(disconnect) = &mut self.process {
            disconnect.phase = phase;
            cx.emit(TabChanged);
            cx.notify();
        }
    }

    /// The program's title from the tab's previous session, until the program sets one.
    pub fn carry_title_over(&mut self, title: Option<String>) {
        if self.title.is_none() {
            self.title = title;
        }
    }

    pub fn title(&self) -> Option<String> {
        self.title.clone()
    }

    /// The bar below the terminal while disconnected, with Reconnect or Start new.
    pub(super) fn disconnect_banner(
        &self,
        disconnect: &Disconnect,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let host = self.host.as_deref().unwrap_or("the server");
        let theme = cx.theme();
        let button = disconnect.action().map(|(label, action)| {
            Button::new("remote-action")
                .label(label)
                .small()
                .primary()
                .on_click(cx.listener(move |_, _, _, cx| cx.emit(action)))
        });
        div()
            .flex_none()
            .flex()
            .items_center()
            .justify_between()
            .gap_3()
            .px_3()
            .py_1()
            .text_sm()
            .bg(theme.secondary)
            .border_t_1()
            .border_color(theme.border)
            .text_color(theme.secondary_foreground)
            .child(div().flex_1().min_w_0().child(disconnect.message(host)))
            .when_some(button, |banner, button| banner.child(button))
            .into_any_element()
    }
}
