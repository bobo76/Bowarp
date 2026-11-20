//! Switching branches from the Git page, on the workspace's side: before the switch, a
//! question when a tab working in that working tree is busy (as the close question counts it)
//! or a file viewer holds unsaved edits of a file in it; after it, the tabs re-read their
//! branch, which they otherwise do only on their own output, title or hook events.

use std::cell::Cell;
use std::path::{Path, PathBuf};
use std::rc::Rc;

use gpui_kit::component::WindowExt;
use gpui_kit::{App, AppContext, Context, Entity, Window};
use workspace_model::git;
use workspace_model::{BusyTab, CloseWarning, switch_warning};

use super::super::WorkspaceView;
use super::super::busy_close::description;
use crate::git_page::{GitPage, GitPageEvent, SwitchRequest};

/// What a switch would change files under, with the folder it is in.
enum InUse {
    Tab(BusyTab),
    /// A file viewer's unsaved file, by name.
    File(String),
}

impl WorkspaceView {
    pub(super) fn on_git_page_event(
        &mut self,
        page: &Entity<GitPage>,
        event: &GitPageEvent,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        match event {
            GitPageEvent::SwitchAsked(request) => {
                self.check_before_switch(page.clone(), request.clone(), window, cx);
            }
            GitPageEvent::Switched => self.refresh_tab_branches(cx),
        }
    }

    /// Switch now when nothing local is busy or unsaved; else find what of it is in the
    /// request's working tree (`git`, off the UI thread) and ask about that first.
    fn check_before_switch(
        &mut self,
        page: Entity<GitPage>,
        request: SwitchRequest,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        // A newer request replaces a check still going.
        self.git.switch_check = None;
        let in_use = self.in_use(cx);
        if in_use.is_empty() {
            page.update(cx, |page, cx| page.start_switch(request, window, cx));
            return;
        }
        let root = request.root.clone();
        let check = cx.background_spawn(async move { in_working_tree(in_use, &root) });
        self.git.switch_check = Some(cx.spawn_in(window, async move |this, cx| {
            let in_use = check.await;
            let _ = this.update_in(cx, |workspace, window, cx| {
                workspace.confirm_switch(page, request, in_use, window, cx);
            });
        }));
    }

    /// The busy local tabs (not remote ones) and the files with unsaved edits, with the folder
    /// each is in.
    fn in_use(&self, cx: &App) -> Vec<(PathBuf, InUse)> {
        let mut in_use = Vec::new();
        for tab in &self.tabs {
            let busy = Self::busy_tab(tab, cx).filter(|_| tab.spec.host.is_none());
            if let Some(busy) = busy
                && let Some(folder) = tab.view.read(cx).current_folder()
            {
                in_use.push((folder, InUse::Tab(busy)));
            }
            let viewer = tab.viewer.as_ref().map(|viewer| viewer.view.read(cx));
            let unsaved = viewer.filter(|viewer| viewer.is_dirty());
            if let Some((viewer, folder)) =
                unsaved.and_then(|viewer| Some((viewer, viewer.path()?.parent()?)))
            {
                in_use.push((folder.to_path_buf(), InUse::File(viewer.file_name())));
            }
        }
        in_use
    }

    /// Ask about `in_use` (what is in the working tree) before switching, or switch now.
    fn confirm_switch(
        &mut self,
        page: Entity<GitPage>,
        request: SwitchRequest,
        in_use: Vec<InUse>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.git.switch_check = None;
        let (mut busy, mut files) = (Vec::new(), Vec::new());
        for item in in_use {
            match item {
                InUse::Tab(tab) => busy.push(tab),
                InUse::File(name) => files.push(name),
            }
        }
        match switch_warning(&request.branch, &busy, &files) {
            None => page.update(cx, |page, cx| page.start_switch(request, window, cx)),
            Some(warning) => ask_before_switch(&page, request, warning, window, cx),
        }
    }

    /// Every local tab re-reads its folder and branch (a tab outside the working tree finds
    /// it unchanged; a remote tab's branch is on its host).
    fn refresh_tab_branches(&mut self, cx: &mut Context<Self>) {
        for tab in self.tabs.iter().filter(|tab| tab.spec.host.is_none()) {
            tab.view.update(cx, |view, cx| view.refresh_branch(cx));
        }
    }
}

/// What of `in_use` is in the working tree whose top folder is `root`: not in another
/// repository or a linked worktree under it. Runs `git`: off the UI thread.
fn in_working_tree(in_use: Vec<(PathBuf, InUse)>, root: &Path) -> Vec<InUse> {
    in_use
        .into_iter()
        .filter(|(folder, _)| git::locate(folder).is_some_and(|repo| repo.root == root))
        .map(|(_, item)| item)
        .collect()
}

/// "Switch to x anyway?" listing what is in use: Switch runs the switch, Cancel (or Esc)
/// leaves everything as it is.
fn ask_before_switch(
    page: &Entity<GitPage>,
    request: SwitchRequest,
    warning: CloseWarning,
    window: &mut Window,
    cx: &mut App,
) {
    let page = page.downgrade();
    let pending = Rc::new(Cell::new(Some(request)));
    let warning = Rc::new(warning);
    window.open_alert_dialog(cx, move |alert, _, _| {
        let (page, pending) = (page.clone(), pending.clone());
        alert
            .title(warning.title.clone())
            .description(description(&warning))
            .ok_text(warning.ok_text)
            .show_cancel(true)
            .on_ok(move |_, window, cx| {
                if let Some(request) = pending.take() {
                    let _ = page.update(cx, |page, cx| page.start_switch(request, window, cx));
                }
                true
            })
    });
}

#[cfg(test)]
mod tests;
