//! The width of the Git page's list of changes: the strip on its right edge drags it (kept in
//! memory while dragged, saved once on release) and a double-click resets it. The diff keeps
//! [`MIN_GIT_DIFF_WIDTH`]: on a narrow page the list shrinks first, down to its minimum.

use gpui_kit::component::ActiveTheme;
use gpui_kit::{
    App, AppContext, Context, CursorStyle, Div, DragMoveEvent, Empty, InteractiveElement,
    IntoElement, MouseButton, MouseDownEvent, Render, Stateful, StatefulInteractiveElement, Styled,
    Window, div, px,
};
use workspace_model::pane_width::GitListWidth;
#[cfg(doc)]
use workspace_model::pane_width::MIN_GIT_DIFF_WIDTH;

use super::GitPage;
use crate::divider_drag::DIVIDER_WIDTH;
use crate::settings_file;

/// The share of the list's width a file's name may take in its row (its folder takes the rest).
const FILE_NAME_SHARE: f32 = 0.6;

/// Dragged when the list's right edge is.
pub(super) struct ListDivider;

impl Render for ListDivider {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        Empty
    }
}

impl GitPage {
    /// The list's width: the one being dragged, else the saved one.
    pub(super) fn list_width(&self, cx: &App) -> GitListWidth {
        self.list_drag
            .current(|| settings_file::settings(cx).git_list_width)
    }

    /// The widest a file's name gets in its row.
    pub(super) fn file_name_max_width(&self, cx: &App) -> f32 {
        self.list_width(cx).get() * FILE_NAME_SHARE
    }

    /// Follow a drag of the list's edge in memory only; `event.bounds` is the row of list and
    /// diff. Saved by [`Self::end_list_drag`].
    pub(super) fn drag_list_edge(
        &mut self,
        event: &DragMoveEvent<ListDivider>,
        _: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let row = event.bounds;
        let x = f32::from(event.event.position.x - row.origin.x);
        let width = GitListWidth::at(x, f32::from(row.size.width));
        if self.list_drag.follow(width, self.list_width(cx)) {
            cx.notify();
        }
    }

    /// On any mouse button release: save the width a drag left, once per drag.
    pub(super) fn end_list_drag(&mut self, cx: &mut Context<Self>) {
        self.list_drag.finish(cx);
    }

    pub(super) fn save_list_width(width: GitListWidth, cx: &mut App) {
        settings_file::save_layout(cx, |settings| settings.git_list_width = width);
    }

    /// The strip on the list's right edge (in a `relative` list column): drag it to resize the
    /// list, double-click it for the default width.
    pub(super) fn render_list_divider(&self, cx: &mut Context<Self>) -> Stateful<Div> {
        let hovered = cx.theme().border;
        div()
            .id("git-list-divider")
            // Keeps presses on the strip from reaching the list rows under it.
            .occlude()
            .absolute()
            .top_0()
            .right_0()
            .h_full()
            .w(px(DIVIDER_WIDTH))
            .hover(move |style| style.bg(hovered))
            .cursor(CursorStyle::ResizeLeftRight)
            .on_drag(ListDivider, |_, _, _, cx| cx.new(|_| ListDivider))
            .on_mouse_down(
                MouseButton::Left,
                cx.listener(|page, event: &MouseDownEvent, _, cx| {
                    if event.click_count == 2 {
                        page.list_drag.reset(GitListWidth::default(), cx);
                        cx.notify();
                    }
                }),
            )
    }
}
