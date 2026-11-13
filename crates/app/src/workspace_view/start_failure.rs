//! Start-up problems the window shows instead of exiting: a first tab that would not start
//! and bad command-line arguments leave the window without a tab, and the empty state then
//! says why (`StartFailure`); with restored tabs, a toast says it instead.

use gpui_kit::component::WindowExt;
use gpui_kit::component::notification::Notification;
use gpui_kit::{Context, Window};
use term_core::{LaunchSpec, SessionKind};
use workspace_model::StartError;

use super::WorkspaceView;
use crate::launcher::launch_name;

/// Why the window has no tab after start-up, shown above the empty state's launchers until a
/// tab opens.
pub(super) struct StartFailure {
    pub(super) error: StartError,
    /// What Retry starts again; `None` when retrying cannot help (bad arguments).
    retry: Option<LaunchSpec>,
}

impl WorkspaceView {
    /// Open the first tab; when it would not start, the empty state says why and offers Retry,
    /// and the launchers open in its folder.
    pub fn open_first_tab(
        &mut self,
        spec: LaunchSpec,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let folder = spec.cwd.clone();
        if let Err(error) = self.start_with_fallback(spec.clone(), window, cx) {
            self.last_closed_folder = folder;
            self.show_start_failure(error, Some(spec), window, cx);
        }
    }

    /// Show `error` in the empty state (the window has no tab) and give it the keyboard.
    pub fn show_start_failure(
        &mut self,
        error: StartError,
        retry: Option<LaunchSpec>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.start_failure = Some(StartFailure { error, retry });
        self.focus_empty_state(window, cx);
    }

    /// Start `spec` in a new tab; an installed but broken shell falls back to the platform's
    /// automatic shell, with a warning. Each failure is also printed to stderr.
    fn start_with_fallback(
        &mut self,
        spec: LaunchSpec,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Result<(), StartError> {
        let fallback = (spec.kind == SessionKind::Shell && spec.program.is_some())
            .then(|| LaunchSpec::shell(spec.cwd.clone()));
        let program = launch_name(&spec).to_owned();
        let Err(err) = self.open_tab(spec, window, cx) else {
            return Ok(());
        };
        eprintln!("agent-workspace: failed to start {program}: {err}");
        let Some(fallback) = fallback else {
            return Err(StartError::launch(program, err.to_string()));
        };
        let fallback_name = launch_name(&fallback).to_owned();
        let Err(fallback_err) = self.open_tab(fallback, window, cx) else {
            self.warn_shell_fallback(format!("Could not start {program}: {err}"), window, cx);
            return Ok(());
        };
        eprintln!("agent-workspace: failed to start {fallback_name}: {fallback_err}");
        let both = format!("{err}. The fallback shell failed too: {fallback_err}");
        Err(StartError::launch(program, both))
    }

    /// Start the failed launch again; a new failure replaces the error shown and counts.
    pub(super) fn retry_start(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(spec) = self
            .start_failure
            .as_ref()
            .and_then(|failure| failure.retry.clone())
        else {
            return;
        };
        if let Err(latest) = self.start_with_fallback(spec, window, cx)
            && let Some(failure) = &mut self.start_failure
        {
            failure.error.retry_failed(latest);
            cx.notify();
        }
    }

    /// Say `error` in a toast until dismissed: tabs were restored, so the window has a tab.
    pub fn warn_start_error(
        &self,
        error: &StartError,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let notification = Notification::error(error.details().join("\n"))
            .title(error.heading())
            .autohide(false);
        // Startup runs before the notification layer's root exists.
        cx.defer_in(window, move |_, window, cx| {
            window.push_notification(notification, cx);
        });
    }

    /// A warning shown from start-up until dismissed.
    pub fn warn_until_dismissed(
        &self,
        message: String,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        // Startup runs before the notification layer's root exists.
        cx.defer_in(window, move |_, window, cx| {
            window.push_notification(Notification::warning(message).autohide(false), cx);
        });
    }
}
