use std::cell::Cell;
use std::rc::Rc;

use gpui_kit::component::Root;
use gpui_kit::{
    AppContext, Context, KeyContext, Keystroke, Render, TestAppContext, VisualTestContext,
};
use serde_json::{Value, json};

use super::*;
use crate::settings_file;

/// Stands in for the settings page: the recording chip inside the workspace's key context.
struct Page {
    focus: FocusHandle,
}

impl Render for Page {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let focus = cx
            .global::<KeyRecorder>()
            .0
            .as_ref()
            .map(|recording| recording.focus.clone());
        div()
            .key_context("Workspace")
            .track_focus(&self.focus)
            .size_full()
            .children(focus.map(|focus| recording_chip(&focus, cx)))
    }
}

fn open(cx: &mut TestAppContext) -> VisualTestContext {
    open_with_page_focus(cx).0
}

/// The window, and the page's focus handle (focused at the start).
fn open_with_page_focus(cx: &mut TestAppContext) -> (VisualTestContext, FocusHandle) {
    let page_focus = Cell::new(None);
    cx.update(|cx| {
        gpui_kit::init(cx);
        settings_file::init_in_memory(cx);
        crate::terminal_view::init(cx);
        crate::workspace_view::init(cx);
        init(cx);
    });
    let window = cx.add_window(|window, cx| {
        let page = cx.new(|cx| {
            cx.observe_global::<KeyRecorder>(|_, cx| cx.notify())
                .detach();
            Page {
                focus: cx.focus_handle(),
            }
        });
        let focus = page.read(cx).focus.clone();
        window.focus(&focus, cx);
        page_focus.set(Some(focus));
        Root::new(page, window, cx)
    });
    let cx = VisualTestContext::from_window(window.into(), cx);
    cx.run_until_parked();
    let page_focus = page_focus.take().expect("page focus");
    (cx, page_focus)
}

fn record(cx: &mut VisualTestContext, action: &'static str, replacing: Option<&str>) {
    let replacing = replacing.map(str::to_owned);
    cx.update(|window, cx| start(action, replacing, window, cx));
    cx.run_until_parked();
}

fn overlay(cx: &mut VisualTestContext) -> Value {
    cx.update(|_, cx| {
        Value::Object(
            settings_file::keybindings(cx)
                .map(|overlay| overlay.entries().clone())
                .unwrap_or_default(),
        )
    })
}

fn recording(cx: &mut VisualTestContext) -> bool {
    cx.update(|_, cx| cx.global::<KeyRecorder>().0.is_some())
}

/// The actions `keys` runs in the workspace (outside a terminal).
fn actions_for(cx: &mut VisualTestContext, keys: &str) -> Vec<&'static str> {
    cx.update(|_, cx| {
        let contexts = [
            KeyContext::parse("Root").unwrap(),
            KeyContext::parse("Workspace").unwrap(),
        ];
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

#[gpui::test]
fn a_recorded_keystroke_replaces_the_key_and_runs_nothing(cx: &mut TestAppContext) {
    let mut cx = open(cx);
    let (duplicated, new_tabs) = (Rc::new(Cell::new(0)), Rc::new(Cell::new(0)));
    let (count, new_count) = (duplicated.clone(), new_tabs.clone());
    cx.update(|_, cx| {
        cx.on_action(move |_: &crate::workspace_view::DuplicateTab, _| count.set(count.get() + 1));
        cx.on_action(move |_: &crate::workspace_view::NewTab, _| {
            new_count.set(new_count.get() + 1);
        });
    });
    cx.simulate_keystrokes("ctrl-shift-d");
    assert_eq!(
        duplicated.get(),
        1,
        "the binding runs while nothing records"
    );
    record(&mut cx, "workspace::NewTab", Some("ctrl-shift-t"));
    assert!(recording(&mut cx));
    // Ctrl+Shift+D is Duplicate tab's: the keystroke is taken, not run, and moves over.
    cx.simulate_keystrokes("ctrl-shift-d");
    assert!(!recording(&mut cx));
    assert_eq!(duplicated.get(), 1);
    // The page has the keyboard again.
    cx.simulate_keystrokes("ctrl-shift-d");
    assert_eq!(
        (duplicated.get(), new_tabs.get()),
        (1, 1),
        "Ctrl+Shift+D is New tab's now"
    );
    assert_eq!(
        overlay(&mut cx),
        json!({ "ctrl-shift-t": null, "ctrl-shift-d": "NewTab" })
    );
    assert_eq!(actions_for(&mut cx, "ctrl-shift-d"), ["workspace::NewTab"]);
    assert!(actions_for(&mut cx, "ctrl-shift-t").is_empty());
    cx.update(|_, cx| {
        assert_eq!(
            shortcuts::shortcut_text("workspace::NewTab", cx).as_deref(),
            Some("Ctrl+Shift+D")
        );
        assert_eq!(
            shortcuts::shortcut_text("workspace::DuplicateTab", cx),
            None
        );
    });
}

#[gpui::test]
fn escape_cancels_and_typing_keys_are_refused(cx: &mut TestAppContext) {
    let mut cx = open(cx);
    record(&mut cx, "terminal::Paste", None);
    cx.simulate_keystrokes("a");
    assert!(recording(&mut cx));
    // Terminal keys are refused too: Ctrl+D is EOF, Ctrl+C an interrupt.
    cx.simulate_keystrokes("ctrl-d ctrl-c alt-b");
    assert!(recording(&mut cx));
    assert_eq!(overlay(&mut cx), json!({}));
    cx.simulate_keystrokes("escape");
    assert!(!recording(&mut cx));
    assert_eq!(overlay(&mut cx), json!({}));
    // Added as one more key, written in GPUI's spelling.
    record(&mut cx, "terminal::Paste", None);
    cx.simulate_keystrokes("alt-shift-p");
    assert_eq!(overlay(&mut cx), json!({ "alt-shift-p": "Paste" }));
}

#[gpui::test]
fn a_keystroke_away_from_the_chip_ends_recording_and_goes_on(cx: &mut TestAppContext) {
    let (mut cx, page_focus) = open_with_page_focus(cx);
    let new_tabs = Rc::new(Cell::new(0));
    let count = new_tabs.clone();
    cx.update(|_, cx| {
        cx.on_action(move |_: &crate::workspace_view::NewTab, _| count.set(count.get() + 1));
    });
    record(&mut cx, "terminal::Paste", None);
    // The page has the keyboard, not the chip: the key ends recording and runs its action.
    cx.update(|window, cx| window.focus(&page_focus, cx));
    cx.simulate_keystrokes("ctrl-shift-t");
    assert!(!recording(&mut cx));
    assert_eq!(new_tabs.get(), 1);
    assert_eq!(overlay(&mut cx), json!({}));
}

#[gpui::test]
fn removing_and_resetting_edit_the_overlay_once_each(cx: &mut TestAppContext) {
    let mut cx = open(cx);
    cx.update(|_, cx| {
        keymap::edit(cx, |overlay, defaults, normalize| {
            overlay.set_key(defaults, "ctrl-v", None, normalize);
        });
    });
    cx.run_until_parked();
    assert_eq!(overlay(&mut cx), json!({ "ctrl-v": null }));
    assert!(actions_for(&mut cx, "ctrl-v").is_empty());
    cx.update(|_, cx| {
        keymap::edit(cx, |overlay, defaults, normalize| {
            overlay.reset_action(defaults, "terminal::PasteOrSend", normalize);
        });
    });
    cx.run_until_parked();
    assert_eq!(overlay(&mut cx), json!({}));
}

#[gpui::test]
fn edits_keep_the_other_entries_in_their_order(cx: &mut TestAppContext) {
    let mut cx = open(cx);
    cx.update(|_, cx| {
        settings_file::update(cx, |settings| {
            let entries = json!({ "f9": "NewTab", "alt-f1": "Paste", "f2": "Find" });
            let entries = entries.as_object().unwrap().clone();
            settings.keybindings = workspace_model::KeybindingOverlay::new(entries);
        });
        keymap::edit(cx, |overlay, defaults, normalize| {
            overlay.set_key(defaults, "alt-f1", None, normalize);
        });
    });
    let keys: Vec<String> = cx.update(|_, cx| {
        settings_file::keybindings(cx)
            .unwrap()
            .entries()
            .keys()
            .cloned()
            .collect()
    });
    assert_eq!(keys, ["f9", "f2"]);
}
