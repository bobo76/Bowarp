//! The Git page's place in the workspace: shown in place of the selected tab (terminal and file
//! viewer), like the settings page, with its row in the sidebar while open.

use std::path::PathBuf;

use gpui_kit::component::button::{Button, ButtonVariants};
use gpui_kit::component::{Icon, Selectable, Sizable};
use gpui_kit::{
    AnyElement, App, AppContext, Context, Entity, FocusHandle, IntoElement, SharedString,
    Subscription, Window,
};

use super::{ToggleGitPage, WorkspaceView};
use crate::assets::GIT_BRANCH_ICON;
use crate::git_page::GitPage;
use crate::settings_file::{self, AppSettings};
use crate::{shortcuts, sidebar};

#[derive(Default)]
pub(super) struct GitPane {
    /// The "Git page" switch as last applied: off, nothing opens the page.
    enabled: bool,
    /// Whether the sidebar shows the Git row (at most one, not saved in `tabs.json`).
    open: bool,
    /// Whether the page shows in place of the selected tab.
    shown: bool,
    /// Made the first time the page opens (it needs a window), then kept with its repository
    /// and folded groups.
    page: Option<Entity<GitPage>>,
    /// Redraws the Git row when the page changes repository.
    _page_changes: Option<Subscription>,
    /// Follows the "Git page" switch (`apply_git_page_setting`).
    _switch: Option<Subscription>,
}

impl GitPane {
    pub fn new(window: &mut Window, cx: &mut Context<WorkspaceView>) -> Self {
        let switch =
            cx.observe_global_in::<AppSettings>(window, WorkspaceView::apply_git_page_setting);
        Self {
            enabled: settings_file::git_page(cx),
            _switch: Some(switch),
            ..Self::default()
        }
    }

    /// Whether the "Git page" switch is on (the title-bar button shows).
    pub fn enabled(&self) -> bool {
        self.enabled
    }

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

    /// The key: back to the tab while the page shows, else open it.
    pub(super) fn toggle_git_page(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.git.shown {
            self.close_git(window, cx);
        } else {
            self.open_git(window, cx);
        }
    }

    /// Show the Git page on the selected tab's repository (when it is in one), from the key
    /// or the title-bar button.
    pub(super) fn open_git(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        // No tab: no selected folder (`tab_folder` would give the app's working directory),
        // so the repository shown stays.
        let selected = self
            .active_tab()
            .and_then(|_| self.tab_folder(self.active, cx));
        self.show_git(selected, window, cx);
    }

    /// Show the Git page in place of the selected tab (or the settings page), adding the
    /// sidebar's Git row if it isn't there, and read the repositories of the open tabs again,
    /// choosing `selected`'s repository when given, else keeping the one shown. Nothing while
    /// the switch is off.
    fn show_git(&mut self, selected: Option<PathBuf>, window: &mut Window, cx: &mut Context<Self>) {
        if !self.git.enabled {
            return;
        }
        let page = self.git_page_entity(window, cx);
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
        page.update(cx, |page, cx| page.show(folders, selected, window, cx));
        let focus = page.read(cx).focus_handle().clone();
        window.focus(&focus, cx);
        cx.notify();
    }

    /// The page, made the first time it opens and kept (with its commit drafts) after.
    fn git_page_entity(&mut self, window: &mut Window, cx: &mut Context<Self>) -> Entity<GitPage> {
        if let Some(page) = &self.git.page {
            return page.clone();
        }
        let page = cx.new(|cx| GitPage::new(window, cx));
        self.git._page_changes = Some(cx.observe(&page, |_, _, cx| cx.notify()));
        self.git.page = Some(page.clone());
        page
    }

    /// Settings changed: follow the "Git page" switch. Off closes the page and its row like
    /// Ctrl+Shift+W on the row; the page entity stays, hidden (not watching).
    pub(super) fn apply_git_page_setting(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let wanted = settings_file::git_page(cx);
        if wanted == self.git.enabled {
            return;
        }
        self.git.enabled = wanted;
        if !wanted {
            self.close_git(window, cx);
        }
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
            .tooltip(shortcuts::with_shortcut("Git", &ToggleGitPage, cx))
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
            // Back to the page: the repository it shows stays.
            cx.listener(|workspace, _, window, cx| workspace.show_git(None, window, cx)),
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
