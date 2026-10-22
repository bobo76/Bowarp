//! The settings page, shown in place of the selected tab's terminal, and its row in the
//! sidebar.

use gpui_kit::component::button::{Button, ButtonVariants};
use gpui_kit::component::notification::Notification;
use gpui_kit::component::{IconName, Selectable, Sizable, WindowExt};
use gpui_kit::{AnyElement, Context, FocusHandle, IntoElement, Subscription, Window};

use super::WorkspaceView;
use crate::file_viewer::FilePlace;
use crate::settings_file::{self, AppSettings};
use crate::sidebar;

pub(super) struct SettingsPane {
    /// Whether the sidebar shows the Settings row (at most one, not saved in `tabs.json`).
    open: bool,
    /// Whether the page shows in place of the selected tab's terminal (the Settings row is
    /// selected). The workspace's `active` keeps the tab shown again when the page goes.
    shown: bool,
    /// The keyboard goes here while the page shows, not to the hidden terminal.
    focus: FocusHandle,
    _changes: Subscription,
}

impl SettingsPane {
    pub fn new(cx: &mut Context<WorkspaceView>) -> Self {
        // A change (from the page, or the file reloaded after a hand edit) redraws the page or
        // the terminal: both read the settings when drawn, and the terminal measures its cells
        // and resizes its pty then (a hidden tab does when it shows).
        let changes = cx.observe_global::<AppSettings>(|_, cx| cx.notify());
        Self {
            open: false,
            shown: false,
            focus: cx.focus_handle(),
            _changes: changes,
        }
    }

    /// Whether the page hides the selected tab's terminal.
    pub fn shows_page(&self) -> bool {
        self.shown
    }

    /// Give the selected tab its place back; the Settings row stays.
    pub fn hide_page(&mut self) {
        self.shown = false;
    }

    pub fn focus(&self) -> &FocusHandle {
        &self.focus
    }
}

impl WorkspaceView {
    fn toggle_settings(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.settings.shown && self.settings.focus.contains_focused(window, cx) {
            self.close_settings(window, cx);
        } else {
            self.open_settings(window, cx);
        }
    }

    /// Show the settings page in place of the selected tab's terminal, adding the sidebar's
    /// Settings row if it isn't there (the title bar button, or selecting the row).
    fn open_settings(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        // The file may have been edited by hand since: the page shows what it holds now.
        settings_file::reload(cx);
        self.settings.open = true;
        if !self.settings.shown {
            self.settings.shown = true;
            if let Some(tab) = self.active_tab() {
                tab.view.update(cx, |view, _| view.hide());
            }
        }
        window.focus(&self.settings.focus, cx);
        cx.notify();
    }

    /// Remove the Settings row; if its page showed, the tab selected before shows again.
    pub(super) fn close_settings(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.settings.open = false;
        if self.settings.shown {
            self.activate(self.active, window, cx);
        }
        cx.notify();
    }

    /// "Open settings file": settings.json (created with the current settings if missing) in
    /// the selected tab's file viewer, beside the settings page.
    pub(super) fn open_settings_file(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(path) = settings_file::file_to_open(cx) else {
            // A failed write shows in the page's banner.
            if settings_file::file(cx).path().is_none() {
                let message = "There is no config folder (no home folder) to keep settings in";
                window.push_notification(Notification::error(message), cx);
            }
            return;
        };
        if let Some(id) = self.active_tab().map(|tab| tab.id) {
            self.open_file_in(id, path, FilePlace::default(), window, cx);
        }
    }

    pub(super) fn settings_button(&self, cx: &mut Context<Self>) -> impl IntoElement {
        Button::new("open-settings")
            .icon(IconName::Settings)
            .ghost()
            .small()
            .selected(self.settings.shown)
            .tooltip("Settings")
            .on_click(cx.listener(|workspace, _, window, cx| workspace.toggle_settings(window, cx)))
    }

    /// The sidebar's Settings row, while it is open.
    pub(super) fn render_settings_row(&self, cx: &mut Context<Self>) -> Option<AnyElement> {
        if !self.settings.open {
            return None;
        }
        let row = sidebar::settings_row(
            self.settings.shown,
            cx.listener(|workspace, _, window, cx| workspace.open_settings(window, cx)),
            cx.listener(|workspace, _, window, cx| {
                cx.stop_propagation();
                workspace.close_settings(window, cx);
            }),
            cx,
        );
        Some(row.into_any_element())
    }
}

#[cfg(test)]
mod tests {
    use std::cell::RefCell;
    use std::rc::Rc;

    use futures::channel::mpsc;
    use gpui_kit::component::Root;
    use gpui_kit::{AppContext, Focusable, TestAppContext, VisualTestContext};
    use term_core::{LaunchSpec, SessionKind};

    use super::*;

    #[gpui::test]
    fn settings_toggle_focuses_then_closes_and_restores_the_terminal(cx: &mut TestAppContext) {
        cx.update(|cx| {
            gpui_kit::init(cx);
            settings_file::init(cx);
            crate::terminal_element::init(cx);
        });
        let captured = Rc::new(RefCell::new(None));
        let capture = captured.clone();
        let window = cx.add_window(move |window, cx| {
            let (_, messages) = mpsc::unbounded();
            let workspace =
                cx.new(|cx| WorkspaceView::new(Rc::from([]), None, messages, false, window, cx));
            workspace.update(cx, |workspace, cx| {
                #[cfg(windows)]
                let (program, args) = ("cmd", vec!["/C", "exit 0"]);
                #[cfg(not(windows))]
                let (program, args) = ("/bin/sh", vec!["-c", "exit 0"]);
                let spec = LaunchSpec::new(
                    SessionKind::Shell,
                    Some(program.into()),
                    args.into_iter().map(String::from).collect(),
                    None,
                );
                workspace.open_tab(spec, window, cx).unwrap();
                workspace.toggle_settings(window, cx);
            });
            *capture.borrow_mut() = Some(workspace.clone());
            Root::new(workspace, window, cx)
        });
        let workspace = captured.borrow().clone().unwrap();
        let mut cx = VisualTestContext::from_window(window.into(), cx);
        cx.run_until_parked();
        cx.update(|window, cx| {
            window.draw(cx).clear(cx);
            workspace.update(cx, |workspace, cx| {
                assert!(workspace.settings.shown);
                assert!(workspace.settings.open);
                assert!(workspace.settings.focus.contains_focused(window, cx));
                let terminal = workspace.tabs[0].view.focus_handle(cx);
                window.focus(&terminal, cx);
                workspace.toggle_settings(window, cx);
                assert!(workspace.settings.shown);
                assert!(workspace.settings.open);
                assert!(workspace.settings.focus.is_focused(window));
                workspace.toggle_settings(window, cx);
                assert!(!workspace.settings.shown);
                assert!(!workspace.settings.open);
                assert!(terminal.is_focused(window));
            });
        });
    }
}
