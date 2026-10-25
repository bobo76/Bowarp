//! Which of a view's own menus is open, so the view can hold back popups (hover cards) that
//! would cover it. gpui-component keeps a menu's open state in its element, out of reach.

use gpui_kit::component::menu::PopupMenu;
use gpui_kit::{Context, DismissEvent, Entity, Subscription, WeakEntity};

/// The menu a view's menu builder last built, until it is dismissed or dropped.
#[derive(Default)]
pub struct OpenMenu {
    menu: Option<WeakEntity<PopupMenu>>,
    _watches: Option<[Subscription; 2]>,
}

impl OpenMenu {
    /// Note the menu `cx` builds as `owner`'s open menu. Call it from a menu builder: it runs
    /// once each time the menu opens. Opening takes the keyboard (`window.focus`), which
    /// redraws the window, so the owner needs no notify here.
    pub fn track<V: 'static>(
        owner: &WeakEntity<V>,
        slot: fn(&mut V) -> &mut OpenMenu,
        cx: &mut Context<PopupMenu>,
    ) {
        let menu = cx.entity();
        let _ = owner.update(cx, |owner, cx| {
            let dismissed = cx.subscribe(&menu, move |owner, menu, _: &DismissEvent, cx| {
                let open = slot(owner);
                if open.is(&menu) {
                    open.menu = None;
                    cx.notify();
                }
            });
            // A menu dropped without a dismissal (its row went) also redraws the owner.
            let released = cx.observe_release(&menu, |_, _, cx| cx.notify());
            *slot(owner) = OpenMenu {
                menu: Some(menu.downgrade()),
                _watches: Some([dismissed, released]),
            };
        });
    }

    /// Whether the tracked menu still shows: not dismissed, and its element (which owns it)
    /// still rendered.
    pub fn is_open(&self) -> bool {
        self.menu
            .as_ref()
            .is_some_and(|menu| menu.upgrade().is_some())
    }

    fn is(&self, menu: &Entity<PopupMenu>) -> bool {
        self.menu
            .as_ref()
            .is_some_and(|open| open.entity_id() == menu.entity_id())
    }
}
