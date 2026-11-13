//! Remote tabs whose connection was lost (docs/remote.md): Reconnect attaches again to their
//! sessions on the server, Start new runs the program there again once its session is gone.
//! Both run off the UI thread; the tab then shows the new session in place. Restarting an
//! exited remote tab takes the Start new path.

use std::io;
use std::path::PathBuf;

use gpui_kit::component::WindowExt;
use gpui_kit::component::notification::Notification;
use gpui_kit::{App, Context, Window};
use term_core::{LaunchSpec, Session};

use super::WorkspaceView;
use crate::launcher::{self, launch_name};
use crate::remote_hosts::{self, Reattached};
use crate::terminal_view::{RemoteAction, SessionEvents, StatusReporting, TerminalView};

/// A tab waiting for its session to come back: its id, the new session's events and its
/// launch.
struct Waiting {
    tab_id: u64,
    events: SessionEvents,
    spec: LaunchSpec,
}

impl WorkspaceView {
    pub(super) fn run_remote_action(
        &mut self,
        tab_id: u64,
        action: RemoteAction,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        match action {
            RemoteAction::Reconnect => self.reconnect(tab_id, window, cx),
            RemoteAction::StartNew => self.start_new_remote(tab_id, window, cx),
        }
    }

    /// Whether tab `tab_id` lost its connection and can reconnect.
    pub(super) fn can_reconnect(&self, tab_id: u64, cx: &App) -> bool {
        self.tab_index(tab_id)
            .is_some_and(|index| self.tabs[index].view.read(cx).can_reconnect())
    }

    /// Attach again to tab `tab_id`'s session on its host, with every other tab there that
    /// lost its connection: they shared it, so they come back over one new connection.
    pub(super) fn reconnect(&mut self, tab_id: u64, window: &mut Window, cx: &mut Context<Self>) {
        let Some(index) = self.tab_index(tab_id) else {
            return;
        };
        let Some(host) = self.tabs[index].spec.host.clone() else {
            return;
        };
        let mut waiting = Vec::new();
        let mut sessions = Vec::new();
        for tab in &self.tabs {
            if tab.spec.host.as_ref() != Some(&host) {
                continue;
            }
            let id = self.next_session_id;
            let started = tab
                .view
                .update(cx, |view, cx| view.begin_reconnect(id, &tab.spec, cx));
            let Some((events, reattach)) = started else {
                continue;
            };
            self.next_session_id += 1;
            sessions.push(reattach);
            waiting.push(Waiting {
                tab_id: tab.id,
                events,
                spec: tab.spec.clone(),
            });
        }
        if waiting.is_empty() {
            return;
        }
        let results = match remote_hosts::reattach(&host, sessions, cx) {
            Ok(results) => results,
            Err(err) => {
                self.reconnect_failed(&host, &waiting, &err, window, cx);
                return;
            }
        };
        cx.spawn_in(window, async move |workspace, cx| {
            let results = results
                .await
                .unwrap_or_else(|_| Err(io::Error::other("the reconnect attempt stopped")));
            let _ = workspace.update_in(cx, |workspace, window, cx| {
                workspace.finish_reconnect(&host, waiting, results, window, cx);
            });
        })
        .detach();
    }

    fn finish_reconnect(
        &mut self,
        host: &str,
        waiting: Vec<Waiting>,
        results: io::Result<Vec<Reattached>>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let results = match results {
            Ok(results) => results,
            Err(err) => {
                self.reconnect_failed(host, &waiting, &err, window, cx);
                return;
            }
        };
        for (tab, result) in waiting.into_iter().zip(results) {
            match result {
                Ok(Some(session)) => {
                    self.show_remote_session(
                        tab.tab_id, session, tab.events, tab.spec, true, window, cx,
                    );
                }
                Ok(None) => self.end_reconnect(tab.tab_id, true, cx),
                Err(err) => {
                    self.end_reconnect(tab.tab_id, false, cx);
                    log::error!("failed to reconnect to {host}: {err}");
                    window.push_notification(
                        Notification::error(format!("Failed to reconnect: {err}")).autohide(false),
                        cx,
                    );
                }
            }
        }
    }

    /// The host could not be reached: the tabs stay disconnected, to try again.
    fn reconnect_failed(
        &mut self,
        host: &str,
        waiting: &[Waiting],
        err: &io::Error,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        for tab in waiting {
            self.end_reconnect(tab.tab_id, false, cx);
        }
        log::error!("failed to reconnect to {host}: {err}");
        window.push_notification(
            Notification::error(format!("Failed to reconnect to {host}: {err}")).autohide(false),
            cx,
        );
    }

    fn end_reconnect(&mut self, tab_id: u64, gone: bool, cx: &mut Context<Self>) {
        if let Some(index) = self.tab_index(tab_id) {
            self.tabs[index]
                .view
                .update(cx, |view, cx| view.end_reconnect(gone, cx));
        }
    }

    /// The server no longer has tab `tab_id`'s session: start the same program on the same
    /// host, in the folder it was last in there.
    fn start_new_remote(&mut self, tab_id: u64, window: &mut Window, cx: &mut Context<Self>) {
        let Some(index) = self.tab_index(tab_id) else {
            return;
        };
        if self.tabs[index]
            .view
            .update(cx, |view, cx| view.begin_start_new(cx))
        {
            self.start_again_on_host(tab_id, window, cx);
        }
    }

    /// Start tab `tab_id`'s launch again on its host, in the folder it was last in there, off
    /// this thread; the tab then shows the new session. The caller marked the tab as starting.
    pub(super) fn start_again_on_host(
        &mut self,
        tab_id: u64,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(index) = self.tab_index(tab_id) else {
            return;
        };
        let tab = &self.tabs[index];
        let mut spec = tab.spec.clone();
        if let Some(folder) = tab.view.read(cx).remote_folder() {
            spec.cwd = Some(PathBuf::from(folder));
        }
        let id = self.next_session_id;
        let (events, started) = match TerminalView::start_remote(id, &spec, cx) {
            Ok(started) => started,
            Err(err) => {
                self.start_new_failed(tab_id, &spec, &err, window, cx);
                return;
            }
        };
        self.next_session_id += 1;
        cx.spawn_in(window, async move |workspace, cx| {
            let started = started
                .await
                .unwrap_or_else(|_| Err(io::Error::other("the connection attempt stopped")));
            let _ = workspace.update_in(cx, |workspace, window, cx| match started {
                Ok(session) => {
                    workspace.show_remote_session(tab_id, session, events, spec, false, window, cx);
                }
                Err(err) => workspace.start_new_failed(tab_id, &spec, &err, window, cx),
            });
        })
        .detach();
    }

    fn start_new_failed(
        &mut self,
        tab_id: u64,
        spec: &LaunchSpec,
        err: &io::Error,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        // Back to Start new, or to Restart for an exited tab.
        self.end_reconnect(tab_id, true, cx);
        if let Some(index) = self.tab_index(tab_id) {
            self.tabs[index]
                .view
                .update(cx, |view, cx| view.end_restart(cx));
        }
        let program = launch_name(spec);
        let host = spec.host.as_deref().unwrap_or_default();
        log::error!("failed to start {program} on {host}: {err}");
        window.push_notification(
            Notification::error(format!("Failed to start {program} on {host}: {err}"))
                .autohide(false),
            cx,
        );
    }

    /// Show `session` in tab `tab_id` in place of its disconnected one; `same_program` keeps
    /// the old title until the program sets one. A tab closed meanwhile drops the session,
    /// which ends it on the server, as closing the tab would have.
    #[expect(
        clippy::too_many_arguments,
        reason = "a tab's new session and how it came"
    )]
    fn show_remote_session(
        &mut self,
        tab_id: u64,
        session: Session,
        events: SessionEvents,
        spec: LaunchSpec,
        same_program: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(index) = self.tab_index(tab_id) else {
            return;
        };
        let title = same_program
            .then(|| self.tabs[index].view.read(cx).title())
            .flatten();
        let shell_key = launcher::shell_key(&self.launchers, &spec);
        let view = TerminalView::with_session(
            session,
            events,
            &spec,
            StatusReporting::OutputActivity,
            shell_key,
            cx,
        );
        view.update(cx, |view, _| view.carry_title_over(title));
        let view_events = self.listen_to_tab(tab_id, &view, window, cx);
        self.swap_tab_view(index, view, view_events, spec, window, cx);
    }
}
