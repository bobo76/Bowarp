use serde_json::json;

use super::*;

/// Stands in for GPUI's parser: modifiers sorted, the key lowercased; an empty key fails.
fn normalize(keys: &str) -> Result<String, String> {
    keys.split_whitespace()
        .map(|stroke| {
            let mut parts: Vec<String> = stroke.split('-').map(str::to_lowercase).collect();
            let key = parts.pop().unwrap_or_default();
            if key.is_empty() || parts.iter().any(String::is_empty) {
                return Err(format!("Invalid keystroke \"{stroke}\"."));
            }
            parts.sort();
            parts.push(key);
            Ok(parts.join("-"))
        })
        .collect::<Result<Vec<_>, _>>()
        .map(|strokes| strokes.join(" "))
}

fn defaults() -> Vec<Shortcut> {
    vec![
        Shortcut::new("ctrl-shift-v", "terminal::Paste", Some("Terminal")),
        Shortcut::new("shift-insert", "terminal::Paste", Some("Terminal")),
        Shortcut::new("ctrl-v", "terminal::PasteOrSend", Some("Terminal")),
        Shortcut::new("ctrl-shift-t", "workspace::NewTab", Some("Workspace")),
        Shortcut::new("f11", "workspace::ToggleFullScreen", None),
    ]
}

fn keymap(overlay: Value) -> Keymap {
    let Value::Object(map) = overlay else {
        panic!("not an object");
    };
    Keymap::new(defaults(), &KeybindingOverlay::new(map), normalize)
}

fn bound(keymap: &Keymap) -> Vec<(&str, &str)> {
    keymap
        .bindings()
        .iter()
        .map(|shortcut| (shortcut.keys.as_str(), shortcut.action))
        .collect()
}

#[test]
fn without_an_overlay_the_defaults_apply() {
    let keymap = keymap(json!({}));
    assert_eq!(keymap.bindings().len(), defaults().len());
    assert!(keymap.problems().is_empty());
    assert_eq!(
        keymap.keys("terminal::Paste").collect::<Vec<_>>(),
        ["ctrl-shift-v", "shift-insert"]
    );
}

#[test]
fn null_removes_a_default_and_a_name_adds_or_replaces() {
    // In sorted order, as the overlay's map keeps it with or without `preserve_order`.
    let keymap = keymap(json!({
        "ctrl-alt-n": "NewTab",
        "ctrl-shift-t": "Paste",
        "ctrl-v": null,
    }));
    assert!(keymap.problems().is_empty(), "{:?}", keymap.problems());
    assert_eq!(
        bound(&keymap),
        [
            ("ctrl-shift-v", "terminal::Paste"),
            ("shift-insert", "terminal::Paste"),
            ("f11", "workspace::ToggleFullScreen"),
            ("alt-ctrl-n", "workspace::NewTab"),
            ("ctrl-shift-t", "terminal::Paste"),
        ]
    );
    // The action's own context, not the replaced default's.
    let paste = &keymap.bindings()[4];
    assert_eq!(paste.context, Some("Terminal"));
    assert_eq!(keymap.keys("terminal::PasteOrSend").count(), 0);
    assert_eq!(
        keymap
            .default_keys("terminal::PasteOrSend")
            .collect::<Vec<_>>(),
        ["ctrl-v"]
    );
}

#[test]
fn keys_match_the_defaults_whatever_the_spelling() {
    let keymap = keymap(json!({ "Shift-Ctrl-V": null }));
    assert!(keymap.problems().is_empty());
    assert_eq!(
        keymap.keys("terminal::Paste").collect::<Vec<_>>(),
        ["shift-insert"]
    );
}

#[test]
fn actions_are_named_in_full_or_short_and_case_is_a_fallback() {
    let keymap = keymap(json!({
        "ctrl-a": "terminal::PasteOrSend",
        "ctrl-b": "toggleFULLscreen",
        "ctrl-c": " NewTab ",
    }));
    assert!(keymap.problems().is_empty());
    let tail: Vec<_> = keymap.bindings()[defaults().len()..]
        .iter()
        .map(|shortcut| (shortcut.action, shortcut.context))
        .collect();
    assert_eq!(
        tail,
        [
            ("terminal::PasteOrSend", Some("Terminal")),
            ("workspace::ToggleFullScreen", None),
            ("workspace::NewTab", Some("Workspace")),
        ]
    );
}

#[test]
fn bad_entries_are_skipped_with_a_reason_and_the_rest_apply() {
    let keymap = keymap(json!({
        "": "Paste",
        "ctrl-": "Paste",
        "ctrl-q": null,
        "ctrl-shift-t": "Explode",
        "ctrl-v": null,
        "ctrl-x": ["Paste", {}],
        "ctrl-y": 3,
    }));
    let reasons: Vec<_> = keymap
        .problems()
        .iter()
        .map(|problem| (problem.keys.as_str(), problem.reason.clone()))
        .collect();
    assert_eq!(
        reasons,
        [
            ("", KeymapReason::BadKeystroke("no keystroke.".into())),
            (
                "ctrl-",
                KeymapReason::BadKeystroke("Invalid keystroke \"ctrl-\".".into())
            ),
            ("ctrl-q", KeymapReason::NothingToRemove),
            (
                "ctrl-shift-t",
                KeymapReason::UnknownAction("Explode".into())
            ),
            ("ctrl-x", KeymapReason::NotAnAction),
            ("ctrl-y", KeymapReason::NotAnAction),
        ]
    );
    // The bad name keeps the default; the good null still applies.
    assert_eq!(
        keymap.keys("workspace::NewTab").collect::<Vec<_>>(),
        ["ctrl-shift-t"]
    );
    assert_eq!(keymap.keys("terminal::PasteOrSend").count(), 0);
}

#[test]
fn two_spellings_of_one_key_keep_the_later_entry() {
    let keymap = keymap(json!({ "ctrl-shift-v": null, "shift-ctrl-v": "NewTab" }));
    assert_eq!(
        keymap.problems(),
        [KeymapProblem {
            keys: "ctrl-shift-v".into(),
            reason: KeymapReason::SameKeysAs("shift-ctrl-v".into()),
        }]
    );
    assert_eq!(
        keymap.keys("workspace::NewTab").collect::<Vec<_>>(),
        ["ctrl-shift-t", "ctrl-shift-v"]
    );
}

#[test]
fn actions_are_listed_once_in_the_defaults_order() {
    let keymap = keymap(json!({ "ctrl-v": null }));
    let actions: Vec<_> = keymap.actions().map(|(action, _)| action).collect();
    assert_eq!(
        actions,
        [
            "terminal::Paste",
            "terminal::PasteOrSend",
            "workspace::NewTab",
            "workspace::ToggleFullScreen",
        ]
    );
}

#[test]
fn problems_read_as_sentences() {
    let unknown = KeymapProblem {
        keys: "ctrl-q".into(),
        reason: KeymapReason::UnknownAction("Quit".into()),
    };
    assert_eq!(
        unknown.to_string(),
        "\"ctrl-q\" is ignored: there is no action \"Quit\"."
    );
}

#[test]
fn overlay_round_trips_entries_it_cannot_use() {
    let text = r#"{"ctrl-v":null,"x":3,"ctrl-shift-v":"Paste"}"#;
    let overlay: KeybindingOverlay = serde_json::from_str(text).unwrap();
    let again: KeybindingOverlay =
        serde_json::from_str(&serde_json::to_string(&overlay).unwrap()).unwrap();
    assert_eq!(again, overlay);
    assert_eq!(overlay.entries().len(), 3);
}

#[test]
fn short_name_drops_the_namespace() {
    assert_eq!(short_name("terminal::Paste"), "Paste");
    assert_eq!(short_name("Paste"), "Paste");
}

/// `overlay` after `edit`, and the keymap it gives.
fn edited(overlay: Value, edit: impl FnOnce(&mut KeybindingOverlay)) -> (Value, Keymap) {
    let Value::Object(map) = overlay else {
        panic!("not an object");
    };
    let mut overlay = KeybindingOverlay::new(map);
    edit(&mut overlay);
    let keymap = Keymap::new(defaults(), &overlay, normalize);
    (Value::Object(overlay.entries().clone()), keymap)
}

fn keys_of<'a>(keymap: &'a Keymap, action: &'a str) -> Vec<&'a str> {
    keymap.keys(action).collect()
}

#[test]
fn a_new_key_is_written_once_and_takes_the_key_from_its_action() {
    let (overlay, keymap) = edited(json!({ "Ctrl-Alt-P": "NewTab", "x": 1 }), |overlay| {
        overlay.set_key(
            &defaults(),
            "alt-ctrl-p",
            Some("terminal::Paste"),
            &normalize,
        );
        overlay.set_key(
            &defaults(),
            "alt-ctrl-v",
            Some("terminal::PasteOrSend"),
            &normalize,
        );
    });
    // Every spelling of the key goes, unusable values too; unrelated entries stay.
    assert_eq!(
        overlay,
        json!({ "alt-ctrl-p": "Paste", "alt-ctrl-v": "PasteOrSend", "x": 1 })
    );
    assert_eq!(keymap.action_for("alt-ctrl-p"), Some("terminal::Paste"));
    assert!(
        keymap
            .keys("workspace::NewTab")
            .all(|keys| keys != "alt-ctrl-p")
    );
}

#[test]
fn a_default_key_given_to_another_action_and_back_leaves_no_entry() {
    let (overlay, keymap) = edited(json!({}), |overlay| {
        overlay.set_key(&defaults(), "ctrl-v", Some("terminal::Paste"), &normalize);
    });
    assert_eq!(overlay, json!({ "ctrl-v": "Paste" }));
    assert!(keys_of(&keymap, "terminal::PasteOrSend").is_empty());
    let (overlay, keymap) = edited(overlay, |overlay| {
        overlay.set_key(
            &defaults(),
            "ctrl-v",
            Some("terminal::PasteOrSend"),
            &normalize,
        );
    });
    assert_eq!(overlay, json!({}));
    assert!(!keymap.is_changed("terminal::PasteOrSend"));
}

#[test]
fn removing_a_key_writes_null_for_a_default_and_drops_an_added_one() {
    let (overlay, keymap) = edited(json!({ "ctrl-alt-v": "Paste" }), |overlay| {
        overlay.set_key(&defaults(), "shift-insert", None, &normalize);
        overlay.set_key(&defaults(), "alt-ctrl-v", None, &normalize);
        // Nothing binds it: nothing to write.
        overlay.set_key(&defaults(), "ctrl-q", None, &normalize);
    });
    assert_eq!(overlay, json!({ "shift-insert": null }));
    assert_eq!(keys_of(&keymap, "terminal::Paste"), ["ctrl-shift-v"]);
    assert!(keymap.is_changed("terminal::Paste"));
}

#[test]
fn replacing_a_key_moves_the_binding() {
    let (overlay, keymap) = edited(json!({}), |overlay| {
        overlay.replace_key(
            &defaults(),
            "workspace::NewTab",
            Some("ctrl-shift-t"),
            "alt-n",
            &normalize,
        );
        // A key the action doesn't have is left alone; `None` adds.
        overlay.replace_key(
            &defaults(),
            "workspace::NewTab",
            Some("ctrl-v"),
            "f2",
            &normalize,
        );
    });
    assert_eq!(
        overlay,
        json!({ "ctrl-shift-t": null, "alt-n": "NewTab", "f2": "NewTab" })
    );
    assert_eq!(keys_of(&keymap, "workspace::NewTab"), ["alt-n", "f2"]);
    assert_eq!(keymap.action_for("ctrl-v"), Some("terminal::PasteOrSend"));
}

#[test]
fn reset_removes_only_the_actions_entries() {
    let overlay = json!({
        "ctrl-shift-v": "NewTab",
        "shift-insert": null,
        "ctrl-alt-v": "paste",
        "ctrl-v": null,
        "ctrl-": "Paste",
        "f5": "NoSuchAction",
    });
    let (overlay, keymap) = edited(overlay, |overlay| {
        overlay.reset_action(&defaults(), "terminal::Paste", &normalize);
    });
    // Kept: another action's key, an unreadable key, an unknown action.
    assert_eq!(
        overlay,
        json!({ "ctrl-v": null, "ctrl-": "Paste", "f5": "NoSuchAction" })
    );
    assert_eq!(
        keys_of(&keymap, "terminal::Paste"),
        ["ctrl-shift-v", "shift-insert"]
    );
    assert!(!keymap.is_changed("terminal::Paste"));
    assert!(keys_of(&keymap, "terminal::PasteOrSend").is_empty());
    let (overlay, keymap) = edited(overlay, |overlay| {
        overlay.reset_all(&defaults(), &normalize);
    });
    assert_eq!(overlay, json!({ "ctrl-": "Paste", "f5": "NoSuchAction" }));
    assert!(
        keymap
            .actions()
            .all(|(action, _)| !keymap.is_changed(action))
    );
}

#[test]
fn an_action_whose_short_name_is_taken_is_written_in_full() {
    let defaults = vec![
        Shortcut::new("ctrl-c", "terminal::Copy", Some("Terminal")),
        Shortcut::new("ctrl-k", "viewer::Copy", None),
    ];
    let mut overlay = KeybindingOverlay::default();
    overlay.set_key(&defaults, "f1", Some("viewer::Copy"), &normalize);
    overlay.set_key(&defaults, "f2", Some("terminal::Copy"), &normalize);
    assert_eq!(
        Value::Object(overlay.entries().clone()),
        json!({ "f1": "viewer::Copy", "f2": "Copy" })
    );
}

#[test]
fn plain_keys_are_terminal_input() {
    let held = |control, alt, shift| HeldModifiers {
        control,
        alt,
        shift,
        ..HeldModifiers::default()
    };
    for key in ["a", "A", "1", "|", "space", "enter", "left", "tab"] {
        assert!(is_terminal_input(held(false, false, false), key), "{key}");
        assert!(is_terminal_input(held(false, false, true), key), "{key}");
    }
    for key in ["f5", "insert", "escape", "pageup"] {
        assert!(!is_terminal_input(held(false, false, false), key), "{key}");
    }
}

#[test]
fn ctrl_or_alt_with_a_letter_is_terminal_input() {
    let held = |control, alt, shift| HeldModifiers {
        control,
        alt,
        shift,
        ..HeldModifiers::default()
    };
    for key in ["d", "c", "z", "b"] {
        assert!(
            is_terminal_input(held(true, false, false), key),
            "ctrl-{key}"
        );
        assert!(
            is_terminal_input(held(false, true, false), key),
            "alt-{key}"
        );
        assert!(
            !is_terminal_input(held(true, false, true), key),
            "ctrl-shift-{key}"
        );
        assert!(
            !is_terminal_input(held(true, true, false), key),
            "ctrl-alt-{key}"
        );
    }
    for key in ["f5", "1", "|", "tab", "left", "insert"] {
        assert!(
            !is_terminal_input(held(true, false, false), key),
            "ctrl-{key}"
        );
    }
    let super_d = HeldModifiers {
        platform: true,
        ..HeldModifiers::default()
    };
    assert!(!is_terminal_input(super_d, "d"));
}

#[test]
fn actions_have_readable_labels() {
    assert_eq!(action_label("workspace::NewTab"), "New tab");
    assert_eq!(action_label("terminal::Paste"), "Paste");
    assert_eq!(action_label("workspace::SelectTab3"), "Select tab 3");
    assert_eq!(action_label("workspace::SomethingElse"), "Something else");
}
