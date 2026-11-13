//! Restarting an exited tab (`RestartTab`): its launch starts again in its last folder, in
//! place, so the tab keeps its name, color, group and place; the scrollback starts empty.

use gpui_kit::component::WindowExt;
use gpui_kit::component::notification::Notification;
use gpui_kit::{App, Context, Window};
use term_core::LaunchSpec;

use super::WorkspaceView;
use crate::launcher::launch_name;

impl WorkspaceView {
    /// Whether tab `tab_id`'s program exited and it can start again.
    pub(super) fn can_restart(&self, tab_id: u64, cx: &App) -> bool {
        self.tab_index(tab_id)
            .is_some_and(|index| self.tabs[index].view.read(cx).can_restart())
    }

    /// Start tab `tab_id`'s launch again: on its host for a remote tab (off this thread),
    /// here in its last folder otherwise. Nothing unless it exited.
    pub(super) fn restart_tab(&mut self, tab_id: u64, window: &mut Window, cx: &mut Context<Self>) {
        let Some(index) = self.tab_index(tab_id) else {
            return;
        };
        if self.tabs[index].spec.host.is_some() {
            self.restart_on_host(index, window, cx);
            return;
        }
        self.restart_here(index, window, cx);
    }

    /// A remote tab's restart runs off this thread; the tab shows "Starting … again" until then.
    fn restart_on_host(&mut self, index: usize, window: &mut Window, cx: &mut Context<Self>) {
        let tab_id = self.tabs[index].id;
        let view = self.tabs[index].view.clone();
        if view.update(cx, |view, cx| view.begin_restart(cx)) {
            self.start_again_on_host(tab_id, window, cx);
        }
    }

    fn restart_here(&mut self, index: usize, window: &mut Window, cx: &mut Context<Self>) {
        if !self.tabs[index].view.read(cx).can_restart() {
            return;
        }
        let spec = self.restart_spec(index, cx);
        let name = launch_name(&spec).to_owned();
        if let Err(err) = self.replace_tab_session(index, spec, window, cx) {
            log::error!("failed to restart {name}: {err}");
            let message = format!("Failed to restart {name}: {err}");
            window.push_notification(Notification::error(message), cx);
            return;
        }
        // Its folder may have changed.
        self.save_tabs(cx);
    }

    /// Tab `index`'s launch, with the saved arguments only, in the folder it was last in.
    fn restart_spec(&self, index: usize, cx: &App) -> LaunchSpec {
        let tab = &self.tabs[index];
        let mut spec = tab.spec.clone();
        spec.cwd = tab.view.read(cx).current_folder().or(spec.cwd);
        spec
    }
}
