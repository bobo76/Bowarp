//! Closing tabs and the window, asking first when a tab is busy: a shell command runs or an
//! agent works or waits for the user (see `workspace_model::close_warning`). The question is a
//! safeguard: it does not depend on any settings switch.

use std::rc::Rc;

use gpui_kit::component::WindowExt;
use gpui_kit::component::button::ButtonVariant;
use gpui_kit::{App, Context, Div, ParentElement, Styled, Window, div};
use workspace_model::{
    BusyTab, COMMAND_LABEL_CHARS, CloseTarget, CloseWarning, close_warning, command_label,
};

use super::WorkspaceView;

impl WorkspaceView {
    /// Close a tab, ending its process: first asking when it is busy, then about unsaved edits
    /// in its file viewer. Closing the last tab leaves the empty state.
    pub(super) fn close_tab(&mut self, index: usize, window: &mut Window, cx: &mut Context<Self>) {
        let Some(id) = self.tabs.get(index).map(|tab| tab.id) else {
            return;
        };
        self.warn_if_busy(
            CloseTarget::Tab,
            &[id],
            window,
            cx,
            move |workspace, window, cx| workspace.close_tab_id_checking_edits(id, window, cx),
        );
    }

    /// Like `close_tab_checking_edits`, for a tab that may have closed since its id was taken.
    fn close_tab_id_checking_edits(
        &mut self,
        id: u64,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if let Some(index) = self.tab_index(id) {
            self.close_tab_checking_edits(index, window, cx);
        }
    }

    /// Close several tabs (a group's, other tabs, tabs below) after one question listing the
    /// busy ones. Unsaved edits are still asked about tab by tab.
    pub(super) fn close_tabs(
        &mut self,
        ids: Vec<u64>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let asked = ids.clone();
        self.warn_if_busy(
            CloseTarget::Tabs,
            &asked,
            window,
            cx,
            move |workspace, window, cx| {
                for &id in &ids {
                    workspace.close_tab_id_checking_edits(id, window, cx);
                }
            },
        );
    }

    /// Close the window from its title bar button (Linux), asking first when a tab is busy.
    pub(super) fn request_close_window(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let ids = self.tab_ids();
        self.warn_if_busy(
            CloseTarget::Window,
            &ids,
            window,
            cx,
            |workspace, window, cx| {
                workspace.close_window(window, cx);
            },
        );
    }

    /// The window manager asks whether the window may close (its close button, Alt+F4). Yes
    /// when no tab is busy; otherwise no, and the question is asked instead: its Close button
    /// closes the window. The question opens over any other dialog, but never twice.
    pub fn allow_close_or_ask(&mut self, window: &mut Window, cx: &mut Context<Self>) -> bool {
        if self.busy_tabs(&self.tab_ids(), cx).is_empty() {
            return true;
        }
        self.request_close_window(window, cx);
        false
    }

    fn tab_ids(&self) -> Vec<u64> {
        self.tabs.iter().map(|tab| tab.id).collect()
    }

    /// The busy tabs among `ids`, in that order.
    fn busy_tabs(&self, ids: &[u64], cx: &App) -> Vec<BusyTab> {
        ids.iter()
            .filter_map(|&id| self.tab_index(id))
            .filter_map(|index| {
                let tab = &self.tabs[index];
                let reason = tab.view.read(cx).busy_reason()?;
                // The name may be a title the program set: bound and clean it.
                let name = Self::tab_name(tab, cx);
                Some(BusyTab {
                    name: command_label(&name, COMMAND_LABEL_CHARS).unwrap_or(name),
                    reason,
                })
            })
            .collect()
    }

    /// Run `close` now when none of `ids` is busy, else once the user confirms.
    fn warn_if_busy(
        &mut self,
        target: CloseTarget,
        ids: &[u64],
        window: &mut Window,
        cx: &mut Context<Self>,
        close: impl Fn(&mut Self, &mut Window, &mut Context<Self>) + 'static,
    ) {
        match close_warning(target, &self.busy_tabs(ids, cx)) {
            None => close(self, window, cx),
            Some(warning) => self.ask_before_closing(warning, window, cx, close),
        }
    }

    /// Ask `warning`'s question; Close runs `close`, Cancel (or Esc) leaves everything open.
    /// Does nothing while this question is already open.
    fn ask_before_closing(
        &mut self,
        warning: CloseWarning,
        window: &mut Window,
        cx: &mut Context<Self>,
        close: impl Fn(&mut Self, &mut Window, &mut Context<Self>) + 'static,
    ) {
        if self.close_question_open {
            return;
        }
        self.close_question_open = true;
        let workspace = cx.entity().downgrade();
        let warning = Rc::new(warning);
        let close = Rc::new(close);
        window.open_alert_dialog(cx, move |alert, _, _| {
            let (workspace, close) = (workspace.clone(), close.clone());
            let closed_workspace = workspace.clone();
            alert
                .title(warning.title.clone())
                .description(description(&warning))
                .ok_text(warning.ok_text)
                .ok_variant(ButtonVariant::Danger)
                .show_cancel(true)
                .on_ok(move |_, window, cx| {
                    // Closing may ask about unsaved edits, a dialog of its own: wait until this
                    // one has closed, or that one would be taken for it.
                    let (workspace, close) = (workspace.clone(), close.clone());
                    window.defer(cx, move |window, cx| {
                        let _ = workspace.update(cx, |workspace, cx| close(workspace, window, cx));
                    });
                    true
                })
                // Runs after every way out (Close, Cancel, Esc, a click outside).
                .on_close(move |_, _, cx| {
                    let _ = closed_workspace.update(cx, |workspace, _| {
                        workspace.close_question_open = false;
                    });
                })
        });
    }
}

fn description(warning: &CloseWarning) -> Div {
    div()
        .flex()
        .flex_col()
        .gap_1()
        .child(warning.intro.clone())
        .children(
            warning
                .lines
                .iter()
                .map(|line| div().pl_2().child(format!("• {line}"))),
        )
}
