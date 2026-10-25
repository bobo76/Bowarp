//! Ctrl+Shift tab shortcuts, shared by the action bindings and the hints drawn in menus and
//! tooltips so the two can't drift apart.

use gpui_kit::component::kbd::Kbd;
use gpui_kit::{
    InteractiveElement, IntoElement, Keystroke, Modifiers, ParentElement, SharedString,
    StatefulInteractiveElement, Styled, div, transparent_white,
};

/// Ctrl+Shift+T: a new tab with the default shell.
pub const NEW_TAB_KEY: &str = "t";
/// Ctrl+Shift+D: another tab like the selected one.
pub const DUPLICATE_TAB_KEY: &str = "d";
/// Ctrl+Shift+W: close the selected tab.
pub const CLOSE_TAB_KEY: &str = "w";
/// Ctrl+Shift+B: show or hide the sidebar.
pub const SIDEBAR_KEY: &str = "b";
/// Ctrl+Shift+S: search tabs.
pub const SEARCH_KEY: &str = "s";
/// Ctrl+Shift+A: jump to the next tab needing attention.
pub const ATTENTION_KEY: &str = "a";
/// Ctrl+Shift+Y: allow the clipboard write waiting in the selected tab.
pub const ALLOW_CLIPBOARD_KEY: &str = "y";
/// Ctrl+Shift+N: deny it.
pub const DENY_CLIPBOARD_KEY: &str = "n";
/// Ctrl+Shift+|: show or hide the file explorer. Bound as the key that produces `|`, whatever
/// the layout; GPUI reports it without Shift (Ctrl+|).
pub const EXPLORER_KEY: &str = "|";

/// Ctrl+Shift+`key`.
pub fn tab_shortcut(key: &str) -> Keystroke {
    Keystroke {
        modifiers: Modifiers {
            control: true,
            shift: true,
            ..Modifiers::default()
        },
        key: key.into(),
        key_char: None,
    }
}

/// `label (Ctrl+Shift+KEY)`, for tooltips.
pub fn with_shortcut(label: &str, key: &str) -> SharedString {
    format!("{label} ({})", Kbd::format(&tab_shortcut(key))).into()
}

/// Ctrl+Shift+`key`, drawn like the hints gpui-component's menu shows for bound actions.
pub fn tab_shortcut_hint(key: &str) -> Kbd {
    Kbd::new(tab_shortcut(key))
        .p_0()
        .flex_nowrap()
        .border_0()
        .bg(transparent_white())
}

/// A menu entry's label with its shortcut on the right, for `PopupMenuItem::element`: the
/// menu draws hints only when the item itself carries an action.
pub fn menu_label_with_hint(
    id: &'static str,
    label: impl Into<SharedString>,
    key: &str,
) -> impl IntoElement {
    let label = label.into();
    div()
        .id(id)
        .aria_label(label.clone())
        .flex()
        .items_center()
        .w_full()
        .gap_3()
        .justify_between()
        .child(label)
        .child(tab_shortcut_hint(key))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[cfg(not(target_os = "macos"))]
    #[test]
    fn tooltip_names_the_shortcut() {
        assert_eq!(
            with_shortcut("New tab", NEW_TAB_KEY).as_ref(),
            "New tab (Ctrl+Shift+T)"
        );
        assert_eq!(
            Kbd::format(&tab_shortcut(DUPLICATE_TAB_KEY)),
            "Ctrl+Shift+D"
        );
    }
}
