//! Shortcut hints in tooltips and menus, read from the keymap in use (`keymap`), so they show
//! the keys the settings file's `"keybindings"` give an action (none while nothing runs it).
//! Also the default Ctrl+Shift tab keys, shared by the views' default bindings.

use gpui_kit::component::kbd::Kbd;
use gpui_kit::{
    Action, App, Div, InteractiveElement, IntoElement, Keystroke, ParentElement, SharedString,
    Stateful, StatefulInteractiveElement, Styled, div, transparent_white,
};

use crate::keymap;

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

/// `strokes` as text: `Ctrl+Shift+T`, a sequence as `Ctrl+K Ctrl+C`.
pub fn keys_text(strokes: &[Keystroke]) -> String {
    strokes
        .iter()
        .map(Kbd::format)
        .collect::<Vec<_>>()
        .join(" ")
}

/// The text of `action`'s first key in the keymap in use, if it has one.
pub fn shortcut_text(action: &str, cx: &App) -> Option<String> {
    keymap::hint(cx, action).map(|strokes| keys_text(&strokes))
}

/// `label (Ctrl+Shift+KEY)` with `action`'s key, for tooltips; `label` alone while no key
/// runs it.
pub fn with_shortcut(label: &str, action: &dyn Action, cx: &App) -> SharedString {
    match shortcut_text(action.name(), cx) {
        Some(keys) => format!("{label} ({keys})").into(),
        None => SharedString::from(label.to_owned()),
    }
}

/// `strokes` drawn like the hints gpui-component's menu shows for bound actions.
pub fn shortcut_hint(strokes: Vec<Keystroke>) -> impl IntoElement {
    div()
        .flex()
        .flex_none()
        .items_center()
        .gap_1()
        .children(strokes.into_iter().map(|stroke| {
            Kbd::new(stroke)
                .p_0()
                .flex_nowrap()
                .border_0()
                .bg(transparent_white())
        }))
}

/// A menu entry's label with `action`'s shortcut on the right (none while no key runs it),
/// for `PopupMenuItem::element`: the menu draws hints only when the item itself carries an
/// action.
pub fn menu_label_with_hint(
    id: &'static str,
    label: impl Into<SharedString>,
    action: &dyn Action,
    cx: &App,
) -> Stateful<Div> {
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
        .children(keymap::hint(cx, action.name()).map(shortcut_hint))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[cfg(not(target_os = "macos"))]
    #[test]
    fn keys_read_as_text() {
        let strokes = |keys: &str| -> Vec<Keystroke> {
            keys.split_whitespace()
                .map(|stroke| Keystroke::parse(stroke).unwrap())
                .collect()
        };
        assert_eq!(keys_text(&strokes("ctrl-shift-t")), "Ctrl+Shift+T");
        assert_eq!(keys_text(&strokes("ctrl-k ctrl-c")), "Ctrl+K Ctrl+C");
    }
}
