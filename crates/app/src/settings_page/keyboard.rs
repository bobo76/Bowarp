//! The settings page's Keyboard page: one row per action of the keymap (`keymap::current`)
//! with its keys. Clicking a key records a new one in its place, `+` records one more, `×`
//! removes a key and "Reset to default" gives the action its default keys back; each gesture
//! is one edit of the settings file's `"keybindings"` (`keymap::edit`), and the bindings and
//! hints follow when the settings change.
//!
//! While a row records ([`KeyRecorder`]), the next keystroke goes to it, caught before any
//! binding or the terminal sees it (`App::intercept_keystrokes`); plain Esc cancels. Recording
//! ends with that keystroke, Esc, or a click anywhere outside the recording chip, and is
//! dropped when a keystroke arrives while the chip doesn't have the keyboard.

use gpui_kit::component::button::{Button, ButtonVariant, ButtonVariants};
use gpui_kit::component::notification::Notification;
use gpui_kit::component::setting::{SettingField, SettingGroup, SettingItem};
use gpui_kit::component::{ActiveTheme, Disableable, IconName, Sizable, WindowExt};
use gpui_kit::prelude::FluentBuilder;
use gpui_kit::{
    AnyElement, App, FocusHandle, Global, InteractiveElement, IntoElement, Keystroke,
    KeystrokeEvent, ParentElement, SharedString, StatefulInteractiveElement, Styled, Window, div,
};
use workspace_model::{HeldModifiers, action_label, is_terminal_input, short_name};

use crate::keymap;
use crate::shortcuts::{self, keys_text};

/// The key context of the recording chip: a keystroke is recorded only while the focused
/// element is inside it.
const RECORDER_CONTEXT: &str = "KeyRecorder";

/// The key names GPUI gives a modifier pressed and released alone.
const MODIFIER_KEYS: &[&str] = &["shift", "control", "alt", "platform", "function"];

/// The row recording a keystroke.
struct Recording {
    action: &'static str,
    /// The key the recorded one replaces; `None`: it is added.
    replacing: Option<String>,
    /// Given to the recording chip; the keystroke is taken while it has the keyboard.
    focus: FocusHandle,
    /// What had the keyboard before (the settings page), given it back when recording ends.
    return_focus: Option<FocusHandle>,
}

/// Which row records a keystroke, if any. The workspace redraws when it changes.
#[derive(Default)]
pub struct KeyRecorder(Option<Recording>);

impl Global for KeyRecorder {}

/// Catch keystrokes for the recording row, app-wide.
pub fn init(cx: &mut App) {
    cx.set_global(KeyRecorder::default());
    cx.intercept_keystrokes(intercept).detach();
}

/// The Keyboard page: the terminal's actions, the workspace's, and a reset for them all.
pub fn keyboard_groups(cx: &App) -> Vec<SettingGroup> {
    let Some(keymap) = keymap::current(cx) else {
        return Vec::new();
    };
    let (terminal, workspace): (Vec<_>, Vec<_>) = keymap
        .actions()
        .partition(|(_, context)| *context == Some("Terminal"));
    let rows = |actions: Vec<(&'static str, Option<&'static str>)>| {
        actions
            .into_iter()
            .map(|(action, _)| action_row(action))
            .collect::<Vec<_>>()
    };
    vec![
        SettingGroup::new()
            .title("Terminal")
            .description(
                "Click a shortcut to record another in its place, + to add one, × to remove \
                 it. Esc cancels recording. Saved to \"keybindings\" in settings.json.",
            )
            .items(rows(terminal)),
        SettingGroup::new()
            .title("Workspace")
            .items(rows(workspace)),
        SettingGroup::new()
            .title("All shortcuts")
            .item(reset_all_row()),
    ]
}

fn action_row(action: &'static str) -> SettingItem {
    SettingItem::new(
        action_label(action),
        SettingField::render(move |_, window, cx| keys_field(action, window, cx)),
    )
    .description(format!("\"{}\" in settings.json.", short_name(action)))
    .keywords(["shortcut", "key", "keybinding", "hotkey"])
}

/// The row's keys as chips, the recording chip, `+`, and "Reset to default" while the keys
/// differ from the defaults. Ids carry the action: the field renders outside the row's id
/// scope.
fn keys_field(action: &'static str, window: &mut Window, cx: &mut App) -> AnyElement {
    let Some(keymap) = keymap::current(cx) else {
        return div().into_any_element();
    };
    let keys: Vec<String> = keymap.keys(action).map(str::to_owned).collect();
    let changed = keymap.is_changed(action);
    let recording = cx
        .global::<KeyRecorder>()
        .0
        .as_ref()
        .filter(|recording| recording.action == action && recording.focus.is_focused(window))
        .map(|recording| (recording.replacing.clone(), recording.focus.clone()));
    let adding = matches!(recording, Some((None, _)));
    let mut chips = div()
        .flex()
        .flex_wrap()
        .justify_end()
        .items_center()
        .gap_1();
    for (index, key) in keys.iter().enumerate() {
        chips = match &recording {
            Some((Some(replacing), focus)) if replacing == key => {
                chips.child(recording_chip(focus, cx))
            }
            _ => chips.child(key_chip(action, index, key.clone(), cx)),
        };
    }
    if keys.is_empty() && !adding {
        chips = chips.child(
            div()
                .text_sm()
                .text_color(cx.theme().muted_foreground)
                .child("Not bound"),
        );
    }
    chips = match &recording {
        Some((None, focus)) => chips.child(recording_chip(focus, cx)),
        _ => chips.child(
            Button::new(SharedString::from(format!("{action}-add")))
                .icon(IconName::Plus)
                .ghost()
                .xsmall()
                .tooltip("Add a shortcut")
                .on_click(move |_, window, cx| start(action, None, window, cx)),
        ),
    };
    div()
        .flex()
        .flex_col()
        .items_end()
        .gap_1()
        .child(chips)
        .when(changed, |column| {
            column.child(
                Button::new(SharedString::from(format!("{action}-reset")))
                    .label("Reset to default")
                    .ghost()
                    .xsmall()
                    .on_click(move |_, _, cx| {
                        keymap::edit(cx, |overlay, defaults, normalize| {
                            overlay.reset_action(defaults, action, normalize);
                        });
                    }),
            )
        })
        .into_any_element()
}

/// One key of an action: click to record another in its place, × to remove it.
fn key_chip(action: &'static str, index: usize, key: String, cx: &App) -> impl IntoElement + use<> {
    let theme = cx.theme();
    let strokes = key
        .split_whitespace()
        .filter_map(|stroke| gpui_kit::Keystroke::parse(stroke).ok())
        .collect::<Vec<_>>();
    let label = keys_text(&strokes);
    let removed = key.clone();
    div()
        .flex()
        .items_center()
        .rounded_md()
        .border_1()
        .border_color(theme.border)
        .child(
            div()
                .id(SharedString::from(format!("{action}-key-{index}")))
                .aria_label(SharedString::from(format!("Change {label}")))
                .pl_1p5()
                .py_0p5()
                .rounded_l_md()
                .cursor_pointer()
                .hover(|style| style.bg(theme.secondary_hover))
                .child(shortcuts::shortcut_hint(strokes))
                .on_click(move |_, window, cx| start(action, Some(key.clone()), window, cx)),
        )
        .child(
            Button::new(SharedString::from(format!("{action}-remove-{index}")))
                .icon(IconName::Close)
                .ghost()
                .xsmall()
                .tooltip(format!("Remove {label}"))
                .on_click(move |_, _, cx| {
                    keymap::edit(cx, |overlay, defaults, normalize| {
                        overlay.set_key(defaults, &removed, None, normalize);
                    });
                }),
        )
}

/// Takes the keyboard while it shows; a click outside it ends recording.
fn recording_chip(focus: &FocusHandle, cx: &App) -> impl IntoElement + use<> {
    let theme = cx.theme();
    div()
        .id("key-recorder")
        .track_focus(focus)
        .key_context(RECORDER_CONTEXT)
        .px_2()
        .py_0p5()
        .rounded_md()
        .border_1()
        .border_color(theme.ring)
        .text_xs()
        .child("Press a shortcut (Esc cancels)")
        .on_mouse_down_out(|_, window, cx| stop(window, cx))
}

fn reset_all_row() -> SettingItem {
    SettingItem::new(
        "Reset all shortcuts",
        SettingField::render(|_, _, cx| {
            let changed = keymap::current(cx).is_some_and(|keymap| {
                keymap
                    .actions()
                    .any(|(action, _)| keymap.is_changed(action))
            });
            Button::new("reset-all-shortcuts")
                .label("Reset all")
                .outline()
                .small()
                .disabled(!changed)
                .on_click(|_, window, cx| {
                    window.open_alert_dialog(cx, |alert, _, _| {
                        alert
                            .title("Reset all shortcuts?")
                            .description(
                                "Every action gets its default shortcuts back. Entries the \
                                 app can't read stay in the settings file.",
                            )
                            .ok_text("Reset")
                            .ok_variant(ButtonVariant::Danger)
                            .show_cancel(true)
                            .on_ok(|_, _, cx| {
                                keymap::edit(cx, |overlay, defaults, normalize| {
                                    overlay.reset_all(defaults, normalize);
                                });
                                true
                            })
                    });
                })
        }),
    )
    .description("Give every action its default shortcuts back.")
    .keywords(["shortcut", "key", "keybinding", "default"])
}

/// Record the next keystroke for `action`, in place of `replacing` or as one more key.
fn start(action: &'static str, replacing: Option<String>, window: &mut Window, cx: &mut App) {
    let focus = cx.focus_handle();
    let return_focus = window.focused(cx);
    window.focus(&focus, cx);
    cx.set_global(KeyRecorder(Some(Recording {
        action,
        replacing,
        focus,
        return_focus,
    })));
}

/// End recording, giving the keyboard back if the chip still has it.
fn stop(window: &mut Window, cx: &mut App) {
    let Some(recording) = cx.global_mut::<KeyRecorder>().0.take() else {
        return;
    };
    if recording.focus.is_focused(window)
        && let Some(focus) = recording.return_focus
    {
        window.focus(&focus, cx);
    }
}

/// Runs before bindings and key handlers for every keystroke: while a row records, takes the
/// keystroke (`stop_propagation`: no action runs and no text is typed).
fn intercept(event: &KeystrokeEvent, window: &mut Window, cx: &mut App) {
    let Some((action, replacing)) = cx
        .try_global::<KeyRecorder>()
        .and_then(|state| state.0.as_ref())
        .map(|recording| (recording.action, recording.replacing.clone()))
    else {
        return;
    };
    let recording_has_keyboard = event
        .context_stack
        .iter()
        .any(|context| context.contains(RECORDER_CONTEXT));
    if !recording_has_keyboard {
        // The chip is gone or lost the keyboard: the key goes on as usual.
        cx.global_mut::<KeyRecorder>().0 = None;
        return;
    }
    let stroke = &event.keystroke;
    if MODIFIER_KEYS.contains(&stroke.key.as_str()) {
        return;
    }
    cx.stop_propagation();
    if stroke.key == "escape" && !stroke.modifiers.modified() {
        stop(window, cx);
        return;
    }
    if refuse_terminal_input(stroke, window, cx) {
        return;
    }
    let unparsed = stroke.unparse();
    let keys = keymap::normalize(&unparsed).unwrap_or(unparsed);
    stop(window, cx);
    save_key(action, replacing, keys, window, cx);
}

/// Warn and return true when `stroke` is input for terminal programs (typing, Ctrl+D): binding
/// it would take the key from them.
fn refuse_terminal_input(stroke: &Keystroke, window: &mut Window, cx: &mut App) -> bool {
    let modifiers = stroke.modifiers;
    let held = HeldModifiers {
        control: modifiers.control,
        alt: modifiers.alt,
        shift: modifiers.shift,
        platform: modifiers.platform,
        function: modifiers.function,
    };
    if !is_terminal_input(held, &stroke.key) {
        return false;
    }
    let message = format!(
        "{} belongs to the terminal program: a shortcut needs Ctrl+Shift, Super or a key like F5.",
        keys_text(std::slice::from_ref(stroke))
    );
    window.push_notification(Notification::warning(message), cx);
    true
}

/// Bind `keys` to `action` (in place of `replacing`), saying which action had them.
fn save_key(
    action: &'static str,
    replacing: Option<String>,
    keys: String,
    window: &mut Window,
    cx: &mut App,
) {
    let previous = keymap::current(cx)
        .and_then(|keymap| keymap.action_for(&keys))
        .filter(|previous| *previous != action);
    keymap::edit(cx, |overlay, defaults, normalize| {
        overlay.replace_key(defaults, action, replacing.as_deref(), &keys, normalize);
    });
    if let Some(previous) = previous {
        let strokes = keys
            .split_whitespace()
            .filter_map(|stroke| gpui_kit::Keystroke::parse(stroke).ok())
            .collect::<Vec<_>>();
        let message = format!(
            "{} now runs {} instead of {}.",
            keys_text(&strokes),
            action_label(action),
            action_label(previous)
        );
        window.push_notification(Notification::info(message), cx);
    }
}

#[cfg(test)]
mod tests;
