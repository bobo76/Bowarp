use gpui_kit::component::TitleBar;
use gpui_kit::{
    App, Context, Entity, FocusHandle, Focusable, IntoElement, ParentElement, Render, Styled,
    Window, div,
};

use crate::terminal_view::TerminalView;

/// Window content: the title bar above the active terminal.
pub struct WorkspaceView {
    terminal: Entity<TerminalView>,
}

impl WorkspaceView {
    pub fn new(terminal: Entity<TerminalView>, cx: &mut Context<Self>) -> Self {
        // The title bar shows the terminal's title, so re-render when the terminal changes.
        cx.observe(&terminal, |_, _, cx| cx.notify()).detach();
        Self { terminal }
    }
}

impl Focusable for WorkspaceView {
    fn focus_handle(&self, cx: &App) -> FocusHandle {
        self.terminal.focus_handle(cx)
    }
}

impl Render for WorkspaceView {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let title = self.terminal.read(cx).display_title();
        div()
            .size_full()
            .flex()
            .flex_col()
            .child(TitleBar::new().child(div().text_sm().child(title)))
            .child(div().flex_1().min_h_0().child(self.terminal.clone()))
    }
}
