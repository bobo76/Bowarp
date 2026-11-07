//! The Git page's place in the workspace: shown in place of the selected tab (terminal and file
//! viewer), like the settings page, with its row in the sidebar while open.

use std::path::PathBuf;

use gpui_kit::component::button::{Button, ButtonVariants};
use gpui_kit::component::{Icon, Selectable, Sizable};
use gpui_kit::{
    AnyElement, App, AppContext, Context, Entity, FocusHandle, IntoElement, SharedString,
    Subscription, Window,
};

use super::WorkspaceView;
use crate::assets::GIT_BRANCH_ICON;
use crate::git_page::GitPage;
use crate::sidebar;

#[derive(Default)]
pub(super) struct GitPane {
    /// Whether the sidebar shows the Git row (at most one, not saved in `tabs.json`).
    open: bool,
    /// Whether the page shows in place of the selected tab.
    shown: bool,
    /// Made the first time the page opens (it needs a window), then kept with its repository
    /// and folded groups.
    page: Option<Entity<GitPage>>,
    /// Redraws the Git row when the page changes repository.
    _page_changes: Option<Subscription>,
}

impl GitPane {
    pub fn shows_page(&self) -> bool {
        self.shown
    }

    /// Give the selected tab its place back; the Git row stays. The page stops watching.
    pub fn hide_page(&mut self, cx: &mut App) {
        if !self.shown {
            return;
        }
        self.shown = false;
        if let Some(page) = &self.page {
            page.update(cx, |page, _| page.hide());
        }
    }

    /// Where the keyboard goes while the page shows.
    pub fn focus(&self, cx: &App) -> Option<FocusHandle> {
        let page = self.page.as_ref().filter(|_| self.shown)?;
        Some(page.read(cx).focus_handle().clone())
    }

    /// The page, while it shows.
    pub fn shown_page(&self) -> Option<Entity<GitPage>> {
        self.page.clone().filter(|_| self.shown)
    }
}

impl WorkspaceView {
    fn toggle_git(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let focused = self
            .git
            .focus(cx)
            .is_some_and(|focus| focus.contains_focused(window, cx));
        if focused {
            self.close_git(window, cx);
        } else {
            self.open_git(window, cx);
        }
    }

    /// Show the Git page in place of the selected tab (or the settings page), adding the
    /// sidebar's Git row if it isn't there, and read the repositories of the open tabs again.
    pub(super) fn open_git(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let page = match &self.git.page {
            Some(page) => page.clone(),
            None => {
                let page = cx.new(|cx| GitPage::new(window, cx));
                self.git._page_changes = Some(cx.observe(&page, |_, _, cx| cx.notify()));
                self.git.page = Some(page.clone());
                page
            }
        };
        self.git.open = true;
        if !self.git.shown {
            if self.settings.shows_page() {
                self.settings.hide_page();
            } else if let Some(tab) = self.active_tab() {
                tab.view.update(cx, |view, _| view.hide());
            }
            self.git.shown = true;
        }
        let folders = self.tab_folders(cx);
        page.update(cx, |page, cx| page.show(folders, window, cx));
        let focus = page.read(cx).focus_handle().clone();
        window.focus(&focus, cx);
        cx.notify();
    }

    /// Remove the Git row; if its page showed, the tab selected before shows again (the empty
    /// state while no tab is open).
    pub(super) fn close_git(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.git.open = false;
        if self.git.shown {
            if self.tabs.is_empty() {
                self.git.hide_page(cx);
                self.focus_active(window, cx);
            } else {
                self.activate(self.active, window, cx);
            }
        }
        cx.notify();
    }

    /// The tabs' folders, the selected tab's first, each once.
    fn tab_folders(&self, cx: &App) -> Vec<PathBuf> {
        let order = std::iter::once(self.active).chain(0..self.tabs.len());
        let mut folders: Vec<PathBuf> = Vec::new();
        for folder in order.filter_map(|index| self.tab_folder(index, cx)) {
            if !folders.contains(&folder) {
                folders.push(folder);
            }
        }
        folders
    }

    pub(super) fn git_button(&self, cx: &mut Context<Self>) -> impl IntoElement {
        Button::new("open-git")
            .icon(Icon::empty().path(GIT_BRANCH_ICON))
            .ghost()
            .small()
            .selected(self.git.shown)
            .tooltip("Git")
            .on_click(cx.listener(|workspace, _, window, cx| workspace.toggle_git(window, cx)))
    }

    /// The sidebar's Git row, while it is open, named after the page's repository.
    /// `list_focused`: the tab list has the keyboard, so the row shows the focus ring while
    /// the page shows.
    pub(super) fn render_git_row(
        &self,
        list_focused: bool,
        cx: &mut Context<Self>,
    ) -> Option<AnyElement> {
        if !self.git.open {
            return None;
        }
        let label: SharedString = self
            .git
            .page
            .as_ref()
            .and_then(|page| page.read(cx).repo_name().map(str::to_string))
            .unwrap_or_else(|| "Git".to_string())
            .into();
        let row = sidebar::page_row(
            sidebar::PageRow {
                id: "git-row",
                heading: "Git",
                label,
                icon: Icon::empty().path(GIT_BRANCH_ICON),
                selected: self.git.shown,
                focused: list_focused && self.git.shown,
            },
            cx.listener(|workspace, _, window, cx| workspace.open_git(window, cx)),
            cx.listener(|workspace, _, window, cx| {
                cx.stop_propagation();
                workspace.close_git(window, cx);
            }),
            cx,
        );
        Some(row.into_any_element())
    }
}

#[cfg(test)]
mod tests;
