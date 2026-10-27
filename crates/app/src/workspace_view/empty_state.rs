//! What the terminal area shows once the last tab is closed: the `+` menu's launchers as
//! buttons, in the same order (the default shell first, as the primary button).

use gpui_kit::component::button::{Button, ButtonVariants};
use gpui_kit::component::{ActiveTheme, Disableable, FocusTrapElement};
use gpui_kit::{
    Action, AnyElement, Context, InteractiveElement, IntoElement, ParentElement,
    StatefulInteractiveElement, Styled, Window, div, px,
};

use super::{NewTab, WorkspaceView};
use crate::launcher::{self, Launcher};
use crate::settings_file;
use crate::shortcuts;
use crate::sidebar;

/// The buttons' column never gets wider than this, so the labels stay readable.
const COLUMN_WIDTH: f32 = 280.;

impl WorkspaceView {
    /// Give the keyboard to the empty state; its default button takes it once drawn.
    pub(super) fn focus_empty_state(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        window.focus(&self.empty_focus, cx);
        self.focus_empty_default = true;
        cx.notify();
    }

    /// The heading, a button per launcher and the new-tab key. Tab and Shift+Tab stay among
    /// the enabled buttons; the column scrolls when the window is too short for it.
    pub(super) fn render_empty_state(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        self.defer_default_button_focus(window, cx);
        let (default, shells, agents) =
            launcher::menu_sections(&self.launchers, settings_file::settings(cx).default_shell());
        let hint = default.and_then(|shell| {
            shortcuts::shortcut_text(NewTab.name(), cx)
                .map(|keys| format!("{keys} opens {}", shell.label))
        });
        let buttons = self.launcher_buttons(default, shells, agents, cx);
        let theme = cx.theme();
        div()
            .size_full()
            .bg(theme.background)
            .child(
                div()
                    .id("empty-state")
                    .size_full()
                    .overflow_y_scroll()
                    .child(
                        div()
                            .flex()
                            .flex_col()
                            .items_center()
                            .justify_center()
                            .min_h_full()
                            .px_4()
                            .py_8()
                            .gap_3()
                            .child(
                                div()
                                    .text_lg()
                                    .text_color(theme.foreground)
                                    .child("No open tabs"),
                            )
                            .child(
                                div()
                                    .flex()
                                    .flex_col()
                                    .gap_2()
                                    .w_full()
                                    .max_w(px(COLUMN_WIDTH))
                                    .children(buttons),
                            )
                            .children(hint.map(|hint| {
                                div()
                                    .max_w_full()
                                    .truncate()
                                    .text_sm()
                                    .text_color(theme.muted_foreground)
                                    .child(hint)
                            })),
                    ),
            )
            .focus_trap("empty-state-trap", &self.empty_focus)
            .into_any_element()
    }

    /// Once this frame is drawn, give the keyboard to the first enabled button, unless it went
    /// elsewhere meanwhile. With no button, the keyboard stays on the empty state (Ctrl+Shift+T
    /// still works).
    fn defer_default_button_focus(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let has_button = self
            .launchers
            .iter()
            .any(|launcher| launcher.program.is_some());
        if !std::mem::take(&mut self.focus_empty_default) || !has_button {
            return;
        }
        // The buttons keep their focus handles in element state, so they can only be reached
        // once this frame has drawn them (deferred work runs after the draw).
        let container = self.empty_focus.clone();
        window.defer(cx, move |window, cx| {
            if container.is_focused(window) {
                window.focus_next(cx);
            }
        });
    }

    /// The default shell as the primary button, then the other shells and the agents.
    fn launcher_buttons(
        &self,
        default: Option<&Launcher>,
        shells: Vec<&Launcher>,
        agents: Vec<&Launcher>,
        cx: &mut Context<Self>,
    ) -> Vec<AnyElement> {
        let primary = default.map(|shell| self.launcher_button(0, shell, cx).primary());
        let first_other = usize::from(primary.is_some());
        let others = shells
            .into_iter()
            .chain(agents)
            .enumerate()
            .map(|(index, launcher)| {
                self.launcher_button(first_other + index, launcher, cx)
                    .outline()
            });
        primary
            .into_iter()
            .chain(others)
            .map(IntoElement::into_any_element)
            .collect()
    }

    /// A launcher's button, as its `+` menu entry: disabled with the menu's wording when its
    /// program is not installed (and then skipped by Tab).
    fn launcher_button(&self, index: usize, launcher: &Launcher, cx: &mut Context<Self>) -> Button {
        let installed = launcher.program.is_some();
        let opened = launcher.clone();
        Button::new(("empty-state-launcher", index))
            .icon(sidebar::launcher_icon(launcher.icon))
            .label(launcher.display_label())
            .w_full()
            .disabled(!installed)
            .tab_stop(installed)
            .on_click(cx.listener(move |workspace, _, window, cx| {
                workspace.open_launcher(&opened, window, cx);
            }))
    }
}
