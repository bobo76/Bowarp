//! Links under the pointer while Ctrl is held: drawn underlined with a hand cursor, opened on
//! a click. Whether a path names a file is looked up on a background thread
//! (`terminal_links::LinkCandidate`).

use gpui_kit::{
    AppContext, Context, Modifiers, ModifiersChangedEvent, MouseButton, MouseDownEvent, Pixels,
    Point, Task, Window,
};

use super::{OpenFileRequest, TerminalView};
use crate::mouse::Cell;
use crate::terminal_links::{self, LinkCandidate, LinkTarget, TerminalLink};

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
    /// The file system lookup for the text under the pointer, kept once done so the same text
    /// is not looked up again on each output.
    check: Option<LinkCheck>,
}

struct LinkCheck {
    candidate: LinkCandidate,
    /// Open the link once it is known to be one: it was clicked meanwhile.
    open: bool,
    /// The lookup while it runs; dropping it leaves its answer unused.
    task: Option<Task<()>>,
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

    /// What reads as a link under `position`, if anything.
    fn link_candidate(&self, position: Point<Pixels>) -> Option<LinkCandidate> {
        let cell = self.cell_under(position)?;
        let folder = self.current_folder();
        terminal_links::candidate_at(&self.session, cell, folder.as_deref(), self.home.as_deref())
    }

    fn set_hovered_link(&mut self, link: Option<TerminalLink>, cx: &mut Context<Self>) {
        if link != self.link_hover.link {
            self.link_hover.link = link;
            cx.notify();
        }
    }

    /// Show the link under `position`: at once when the text alone tells, else once the
    /// background lookup of its file answers.
    fn look_for_link(&mut self, position: Point<Pixels>, cx: &mut Context<Self>) {
        let Some(candidate) = self.link_candidate(position) else {
            self.link_hover.check = None;
            self.set_hovered_link(None, cx);
            return;
        };
        if let Some(link) = candidate.known() {
            self.link_hover.check = None;
            self.set_hovered_link(Some(link), cx);
            return;
        }
        self.check_link(candidate, cx);
    }

    /// Look up `candidate`'s file in the background, unless that was already asked.
    fn check_link(&mut self, candidate: LinkCandidate, cx: &mut Context<Self>) {
        let asked = &self.link_hover.check;
        if asked
            .as_ref()
            .is_some_and(|check| check.candidate == candidate)
        {
            return;
        }
        self.set_hovered_link(None, cx);
        let checked = candidate.clone();
        let task = cx.spawn(async move |this, cx| {
            let link = cx.background_spawn(async move { checked.check() }).await;
            let _ = this.update(cx, |view, cx| view.on_link_checked(link, cx));
        });
        self.link_hover.check = Some(LinkCheck {
            candidate,
            open: false,
            task: Some(task),
        });
    }

    fn on_link_checked(&mut self, link: Option<TerminalLink>, cx: &mut Context<Self>) {
        let Some(check) = &mut self.link_hover.check else {
            return;
        };
        check.task = None;
        match link {
            Some(link) if check.open => self.open_hovered_link(link, cx),
            link => self.set_hovered_link(link, cx),
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
            self.link_hover.check = None;
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
        self.look_for_link(position, cx);
    }

    /// Look again at the link under a held Ctrl: the text there may have changed.
    pub(super) fn refresh_link_hover(&mut self, cx: &mut Context<Self>) {
        if let Some(pointer) = self.link_hover.pointer {
            self.link_hover.cell = self.link_key(pointer);
            self.look_for_link(pointer, cx);
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
        let Some(candidate) = self.link_candidate(event.position) else {
            return false;
        };
        let shown = self.link_hover.link.clone();
        let link = candidate
            .known()
            .or_else(|| shown.filter(|link| link.rows == candidate.rows()));
        if let Some(link) = link {
            self.open_hovered_link(link, cx);
            return true;
        }
        // Not looked up yet: the click opens it when the lookup says it is a file.
        self.check_link(candidate, cx);
        match &mut self.link_hover.check {
            Some(check) if check.task.is_some() => {
                check.open = true;
                true
            }
            _ => false,
        }
    }

    fn open_hovered_link(&mut self, link: TerminalLink, cx: &mut Context<Self>) {
        self.link_hover.link = None;
        self.link_hover.cell = None;
        self.link_hover.check = None;
        self.open_link(link.target, cx);
        cx.notify();
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
