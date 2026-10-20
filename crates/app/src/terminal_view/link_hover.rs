//! Links under the pointer while Ctrl is held: drawn underlined with a hand cursor, opened on
//! a click.

use gpui_kit::{
    Context, Modifiers, ModifiersChangedEvent, MouseButton, MouseDownEvent, Pixels, Point, Window,
};

use super::{OpenFileRequest, TerminalView};
use crate::mouse::Cell;
use crate::terminal_links::{self, LinkTarget, TerminalLink};

#[derive(Default)]
pub(crate) struct LinkHover {
    /// The link under the pointer while Ctrl is held, drawn underlined with a hand cursor.
    pub(crate) link: Option<TerminalLink>,
    /// Where the pointer was when Ctrl was last seen held over the terminal, so new output
    /// under it is looked at again; `None` once Ctrl is released.
    pointer: Option<Point<Pixels>>,
    /// The cell (and scroll offset) `link` was last looked up for while Ctrl was held, so
    /// moving within a cell does not look again; cleared when output arrives.
    cell: Option<(Cell, usize)>,
}

impl TerminalView {
    /// The visible cell under `position`, `None` outside the grid.
    fn cell_under(&self, position: Point<Pixels>) -> Option<Cell> {
        let geometry = self.geometry?;
        let x = f32::from(position.x - geometry.origin.x);
        let y = f32::from(position.y - geometry.origin.y);
        if x < 0.0 || y < 0.0 {
            return None;
        }
        let column = (x / f32::from(geometry.cell_width)) as usize;
        let line = (y / f32::from(geometry.line_height)) as usize;
        (column < geometry.columns && line < geometry.lines).then_some(Cell { column, line })
    }

    /// The link under `position`, if any.
    fn link_under(&self, position: Point<Pixels>) -> Option<TerminalLink> {
        let cell = self.cell_under(position)?;
        let folder = self.current_folder();
        terminal_links::link_at(&self.session, cell, folder.as_deref(), self.home.as_deref())
    }

    fn set_hovered_link(&mut self, link: Option<TerminalLink>, cx: &mut Context<Self>) {
        if link != self.link_hover.link {
            self.link_hover.link = link;
            cx.notify();
        }
    }

    /// Underline the link under the pointer while Ctrl is held (and nothing else is).
    pub(crate) fn update_link_hover(
        &mut self,
        position: Point<Pixels>,
        modifiers: &Modifiers,
        cx: &mut Context<Self>,
    ) {
        if !is_link_modifier(modifiers) || !self.visible {
            self.link_hover.pointer = None;
            self.link_hover.cell = None;
            self.set_hovered_link(None, cx);
            return;
        }
        let key = self.link_key(position);
        let unchanged =
            self.link_hover.pointer.is_some() && key.is_some() && key == self.link_hover.cell;
        self.link_hover.pointer = Some(position);
        if unchanged {
            return;
        }
        self.link_hover.cell = key;
        self.set_hovered_link(self.link_under(position), cx);
    }

    /// Look again at the link under a held Ctrl: the text there may have changed.
    pub(super) fn refresh_link_hover(&mut self, cx: &mut Context<Self>) {
        if let Some(pointer) = self.link_hover.pointer {
            self.link_hover.cell = self.link_key(pointer);
            self.set_hovered_link(self.link_under(pointer), cx);
        }
    }

    /// The cell under `position` with the scroll offset: the link there stays the same while
    /// these do and no output arrives.
    fn link_key(&self, position: Point<Pixels>) -> Option<(Cell, usize)> {
        let cell = self.cell_under(position)?;
        Some((cell, self.session.display_offset()))
    }

    pub(super) fn on_modifiers_changed(
        &mut self,
        event: &ModifiersChangedEvent,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.update_link_hover(window.mouse_position(), &event.modifiers, cx);
    }

    /// Open the link a Ctrl+click is on. Returns whether there was one: the click is then
    /// neither reported to the program nor starts a selection.
    pub(super) fn open_clicked_link(
        &mut self,
        event: &MouseDownEvent,
        cx: &mut Context<Self>,
    ) -> bool {
        if event.button != MouseButton::Left || !is_link_modifier(&event.modifiers) {
            return false;
        }
        let Some(link) = self.link_under(event.position) else {
            return false;
        };
        self.link_hover.link = None;
        self.link_hover.cell = None;
        self.open_link(link.target, cx);
        cx.notify();
        true
    }

    /// Open a Ctrl+clicked link: a URL in the browser, a file in the viewer.
    fn open_link(&mut self, target: LinkTarget, cx: &mut Context<Self>) {
        match target {
            LinkTarget::Url(url) => cx.open_url(&url),
            LinkTarget::File { path, place } => cx.emit(OpenFileRequest { path, place }),
        }
    }
}

/// Ctrl (Cmd on macOS) alone: held over a link, it shows and opens the link.
fn is_link_modifier(modifiers: &Modifiers) -> bool {
    modifiers.secondary() && !modifiers.shift && !modifiers.alt
}
