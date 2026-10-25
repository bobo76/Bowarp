use gpui_kit::{KeyContext, TestAppContext};
use serde_json::{Value, json};

use super::*;

fn init(cx: &mut TestAppContext) {
    cx.update(|cx| {
        gpui_kit::init(cx);
        settings_file::init_in_memory(cx);
        crate::terminal_view::init(cx);
        crate::workspace_view::init(cx);
    });
}

fn set_overlay(cx: &mut TestAppContext, overlay: Value) {
    let Value::Object(map) = overlay else {
        panic!("not an object");
    };
    cx.update(|cx| {
        settings_file::preview(cx, |settings| {
            settings.keybindings = KeybindingOverlay::new(map)
        });
    });
    cx.run_until_parked();
}

/// The actions `keys` runs in a terminal (inside the workspace, inside gpui-component's root).
fn actions_in_terminal(cx: &mut TestAppContext, keys: &str) -> Vec<&'static str> {
    actions_in(cx, &["Root", "Workspace", "Terminal"], keys)
}

/// The actions `keys` runs with `contexts` as the key context stack (outermost first).
fn actions_in(cx: &mut TestAppContext, contexts: &[&str], keys: &str) -> Vec<&'static str> {
    cx.update(|cx| {
        let contexts: Vec<KeyContext> = contexts
            .iter()
            .map(|context| KeyContext::parse(context).unwrap())
            .collect();
        let input = [Keystroke::parse(keys).unwrap()];
        let (bindings, _) = cx
            .key_bindings()
            .borrow()
            .bindings_for_input(&input, &contexts);
        bindings
            .iter()
            .map(|binding| binding.action().name())
            .collect()
    })
}

fn counts(cx: &mut TestAppContext) -> (usize, usize) {
    cx.update(|cx| {
        let keymap = cx.key_bindings();
        let keymap = keymap.borrow();
        let own = keymap
            .bindings()
            .filter(|binding| binding.meta() == Some(OWN_BINDING))
            .count();
        (own, keymap.bindings().len() - own)
    })
}

#[gpui::test]
fn every_default_binds(cx: &mut TestAppContext) {
    init(cx);
    let defaults = cx.update(|cx| cx.global::<AppKeymap>().defaults.len());
    assert_eq!(counts(cx).0, defaults);
    assert!(cx.update(|cx| problems(cx).is_empty()));
    assert_eq!(actions_in_terminal(cx, "ctrl-v"), ["terminal::PasteOrSend"]);
    // The pass-through keys reach the terminal: no action runs.
    assert!(actions_in_terminal(cx, "ctrl-c").is_empty());
}

#[gpui::test]
fn the_overlay_applies_live_and_keeps_other_bindings(cx: &mut TestAppContext) {
    init(cx);
    let (own, others) = counts(cx);
    set_overlay(
        cx,
        json!({
            "ctrl-c": "Copy",
            "ctrl-shift-v": "NewTab",
            "ctrl-v": null,
            "ctrl-z": "Nope",
        }),
    );
    // Nothing runs for Ctrl+V: the key goes to the terminal as ^V.
    assert!(actions_in_terminal(cx, "ctrl-v").is_empty());
    assert_eq!(
        actions_in_terminal(cx, "ctrl-shift-v"),
        ["workspace::NewTab"]
    );
    // A user binding outranks the pass-through key in the terminal's context.
    assert_eq!(actions_in_terminal(cx, "ctrl-c"), ["terminal::Copy"]);
    // One added (ctrl-c), one removed (ctrl-v), one replaced.
    assert_eq!(counts(cx), (own, others));
    let reasons: Vec<String> =
        cx.update(|cx| problems(cx).iter().map(ToString::to_string).collect());
    assert_eq!(
        reasons,
        ["\"ctrl-z\" is ignored: there is no action \"Nope\"."]
    );

    set_overlay(cx, json!({}));
    assert_eq!(actions_in_terminal(cx, "ctrl-v"), ["terminal::PasteOrSend"]);
    assert_eq!(actions_in_terminal(cx, "ctrl-shift-v"), ["terminal::Paste"]);
    assert_eq!(counts(cx), (own, others));
    assert!(cx.update(|cx| problems(cx).is_empty()));
}

#[gpui::test]
fn a_context_less_action_binds_anywhere(cx: &mut TestAppContext) {
    init(cx);
    set_overlay(cx, json!({ "f12": "ToggleFullScreen" }));
    assert_eq!(actions_in(cx, &[], "f12"), ["workspace::ToggleFullScreen"]);
}

#[gpui::test]
fn reordering_two_spellings_of_one_key_binds_again(cx: &mut TestAppContext) {
    init(cx);
    set_overlay(
        cx,
        json!({ "ctrl-shift-v": "NewTab", "shift-ctrl-v": "Find" }),
    );
    assert_eq!(actions_in_terminal(cx, "ctrl-shift-v"), ["terminal::Find"]);
    set_overlay(
        cx,
        json!({ "shift-ctrl-v": "Find", "ctrl-shift-v": "NewTab" }),
    );
    assert_eq!(
        actions_in_terminal(cx, "ctrl-shift-v"),
        ["workspace::NewTab"]
    );
}

#[gpui::test]
fn other_settings_changes_leave_the_bindings_alone(cx: &mut TestAppContext) {
    init(cx);
    let version = cx.update(|cx| cx.key_bindings().borrow().version());
    cx.update(|cx| {
        settings_file::preview(cx, |settings| {
            settings.extra.insert("unrelated".into(), Value::Null);
        });
    });
    cx.run_until_parked();
    assert!(cx.update(|cx| cx.key_bindings().borrow().version()) == version);
}

#[test]
fn normalize_uses_gpui_spelling_and_reports_bad_keystrokes() {
    assert_eq!(normalize("shift-ctrl-V").unwrap(), "ctrl-shift-v");
    assert_eq!(normalize("ctrl-k  ctrl-c").unwrap(), "ctrl-k ctrl-c");
    assert!(
        normalize("ctrl-x-y")
            .unwrap_err()
            .starts_with("Invalid keystroke")
    );
}
