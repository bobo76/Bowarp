//! The right-click menu over the grid: Copy, Paste, Select all, Search, Clear scrollback, and
//! on a link Open link and Copy link first (`workspace_model::terminal_menu`). With mouse
//! reporting on, the right button goes to the program and Shift+right-click opens the menu.

use std::pin::pin;
use std::time::Duration;

use futures::future::{Either, select};
use gpui_kit::base::POPUP_PRIORITY;
use gpui_kit::component::menu::{PopupMenu, PopupMenuItem};
use gpui_kit::{
    Action, AnyElement, AppContext, ClipboardItem, Context, DismissEvent, Entity, Focusable,
    IntoElement, ParentElement, Pixels, Point, Subscription, Task, WeakEntity, Window, anchored,
    deferred, px,
};
use term_core::alacritty_terminal::term::TermMode;
use workspace_model::{TerminalMenuEntry, TerminalMenuState, terminal_menu};

use super::{ClearScrollback, Copy, Find, Paste, SelectAll, TerminalView};
use crate::shortcuts;
use crate::terminal_links::{LinkCandidate, LinkTarget, TerminalLink};

/// How long a right-click on a path waits for the file lookup before the menu opens without
/// the link's entries (a network mount that does not answer).
const LINK_LOOKUP_WAIT: Duration = Duration::from_millis(300);

#[derive(Default)]
pub(super) struct TerminalMenu {
    /// The menu showing: set when it opens, cleared on its `DismissEvent` (an entry chosen,
    /// Esc, a click outside), when another replaces it, or when the tab is hidden.
    open: Option<OpenMenu>,
    /// The file lookup of a right-clicked path before the menu opens; dropped (cancelling the
    /// opening) by another right-click or when the tab is hidden.
    opening: Option<Task<()>>,
}

struct OpenMenu {
    menu: Entity<PopupMenu>,
    position: Point<Pixels>,
    _dismissed: Subscription,
}

impl TerminalMenu {
    pub(super) fn is_open(&self) -> bool {
        self.open.is_some()
    }

    /// Drop a menu still waiting for its file lookup: the user clicked on since.
    pub(super) fn cancel_opening(&mut self) {
        self.opening = None;
    }
}

impl TerminalView {
    /// Open the menu at `position`, with the link's entries when a link is there. Called from
    /// the mouse-down handler: the menu opens after it, as the clicked terminal takes the
    /// keyboard once its handlers ran.
    pub(super) fn request_context_menu(
        &mut self,
        position: Point<Pixels>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(candidate) = self.link_candidate(position) else {
            return self.open_context_menu_later(position, None, window, cx);
        };
        let shown = self.link_hover.link.clone();
        let known = candidate
            .known()
            .or_else(|| shown.filter(|link| link.rows == candidate.rows()));
        if known.is_some() {
            return self.open_context_menu_later(position, known, window, cx);
        }
        let opening = self.open_context_menu_after_lookup(position, candidate, window, cx);
        self.context_menu.opening = Some(opening);
    }

    /// Open the menu once the file lookup of `candidate` answers, off the UI thread, or after
    /// `LINK_LOOKUP_WAIT` without the link's entries.
    fn open_context_menu_after_lookup(
        &mut self,
        position: Point<Pixels>,
        candidate: LinkCandidate,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Task<()> {
        cx.spawn_in(window, async move |this, cx| {
            let lookup = cx.background_spawn(async move { candidate.check() });
            let timeout = cx.background_executor().timer(LINK_LOOKUP_WAIT);
            let link = match select(pin!(lookup), pin!(timeout)).await {
                Either::Left((link, _)) => link,
                Either::Right(_) => None,
            };
            let _ = this.update_in(cx, |view, window, cx| {
                view.context_menu.opening = None;
                view.open_context_menu(position, link, window, cx);
            });
        })
    }

    fn open_context_menu_later(
        &mut self,
        position: Point<Pixels>,
        link: Option<TerminalLink>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let view = cx.entity();
        window.defer(cx, move |window, cx| {
            view.update(cx, |view, cx| {
                view.open_context_menu(position, link, window, cx);
            });
        });
    }

    fn open_context_menu(
        &mut self,
        position: Point<Pixels>,
        link: Option<TerminalLink>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if !self.visible {
            return;
        }
        let state = TerminalMenuState {
            has_selection: self.session.has_selection(),
            alt_screen: self.mode().contains(TermMode::ALT_SCREEN),
        };
        let view = cx.entity().downgrade();
        let target = link.map(|link| link.target);
        let focus = self.focus_handle.clone();
        let menu = PopupMenu::build(window, cx, move |menu, _, _| {
            build_menu(menu, &view, target, state).action_context(focus)
        });
        let dismissed = cx.subscribe(&menu, |view, menu, _: &DismissEvent, cx| {
            let open = &view.context_menu.open;
            if open.as_ref().is_some_and(|open| open.menu == menu) {
                view.context_menu.open = None;
                cx.notify();
            }
        });
        menu.focus_handle(cx).focus(window, cx);
        self.context_menu.open = Some(OpenMenu {
            menu,
            position,
            _dismissed: dismissed,
        });
        cx.notify();
    }

    /// Hidden tabs don't paint: their menu goes, so it does not show again with the tab.
    pub(super) fn close_context_menu(&mut self) {
        self.context_menu = TerminalMenu::default();
    }

    pub(super) fn render_context_menu(&self) -> Option<AnyElement> {
        let open = self.context_menu.open.as_ref()?;
        let menu = anchored()
            .position(open.position)
            .snap_to_window_with_margin(px(8.))
            .child(open.menu.clone());
        Some(
            deferred(menu)
                .with_priority(POPUP_PRIORITY)
                .into_any_element(),
        )
    }

    /// Run a chosen entry. The menu then gives the keyboard back to the terminal, unless the
    /// entry moved it (Search's field).
    fn run_menu_entry(
        &mut self,
        entry: TerminalMenuEntry,
        link: Option<&LinkTarget>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        match entry {
            TerminalMenuEntry::OpenLink => {
                if let Some(target) = link {
                    self.open_link(target.clone(), cx);
                }
            }
            TerminalMenuEntry::CopyLink => {
                if let Some(target) = link {
                    cx.write_to_clipboard(ClipboardItem::new_string(link_text(target)));
                }
            }
            TerminalMenuEntry::Copy => {
                self.copy_selection(cx);
            }
            TerminalMenuEntry::Paste => self.paste_after_menu(window, cx),
            TerminalMenuEntry::SelectAll => self.select_all(cx),
            TerminalMenuEntry::Search => self.on_find(&Find, window, cx),
            TerminalMenuEntry::ClearScrollback => self.clear_scrollback(cx),
            TerminalMenuEntry::Separator => {}
        }
    }

    /// Paste once the menu has closed and given the keyboard back: the paste question's
    /// dialog must keep it.
    fn paste_after_menu(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let view = cx.entity();
        window.defer(cx, move |window, cx| {
            view.update(cx, |view, cx| view.paste_clipboard(window, cx));
        });
    }
}

/// The menu's entries; each runs on the view, if it is still there.
fn build_menu(
    menu: PopupMenu,
    view: &WeakEntity<TerminalView>,
    link: Option<LinkTarget>,
    state: TerminalMenuState,
) -> PopupMenu {
    let entries = terminal_menu(link.is_some());
    entries.into_iter().fold(
        menu.min_w(px(crate::ui_dimensions::MENU_WIDTH)),
        |menu, entry| {
            if entry == TerminalMenuEntry::Separator {
                return menu.separator();
            }
            menu.item(menu_item(entry, view.clone(), link.clone(), state))
        },
    )
}

fn menu_item(
    entry: TerminalMenuEntry,
    view: WeakEntity<TerminalView>,
    link: Option<LinkTarget>,
    state: TerminalMenuState,
) -> PopupMenuItem {
    let item = match entry_action(entry) {
        Some((id, action)) => PopupMenuItem::element(move |_, cx| {
            shortcuts::menu_label_with_hint(id, entry.label(), action, cx)
        }),
        None => PopupMenuItem::new(entry.label()),
    };
    item.disabled(!entry.is_enabled(state))
        .on_click(move |_, window, cx| {
            let _ = view.update(cx, |view, cx| {
                view.run_menu_entry(entry, link.as_ref(), window, cx);
            });
        })
}

/// The element id and the action whose key an entry shows; `None` for the link's entries,
/// which no key runs.
fn entry_action(entry: TerminalMenuEntry) -> Option<(&'static str, &'static dyn Action)> {
    Some(match entry {
        TerminalMenuEntry::Copy => ("terminal-menu-copy", &Copy),
        TerminalMenuEntry::Paste => ("terminal-menu-paste", &Paste),
        TerminalMenuEntry::SelectAll => ("terminal-menu-select-all", &SelectAll),
        TerminalMenuEntry::Search => ("terminal-menu-search", &Find),
        TerminalMenuEntry::ClearScrollback => ("terminal-menu-clear", &ClearScrollback),
        TerminalMenuEntry::OpenLink | TerminalMenuEntry::CopyLink => return None,
        TerminalMenuEntry::Separator => return None,
    })
}

/// What Copy link puts on the clipboard: the URL, or the file's path.
fn link_text(target: &LinkTarget) -> String {
    match target {
        LinkTarget::Url(url) => url.clone(),
        LinkTarget::File { path, .. } => path.display().to_string(),
    }
}
