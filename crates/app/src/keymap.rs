//! The app's keyboard shortcuts: the defaults of each view (`terminal_view`,
//! `workspace_view`) with the settings file's `"keybindings"` overlay applied
//! (`workspace_model::Keymap`). The bindings are registered again whenever the overlay
//! changes, so a hand edit applies once the settings file is read again (the settings page
//! opened, the file saved in the file viewer, any change made on the settings page). The
//! entries skipped are listed on the settings page ([`problems`]).

use std::rc::Rc;

use gpui_kit::{
    App, DummyKeyboardMapper, Global, KeyBinding, KeyBindingContextPredicate, KeyBindingMetaIndex,
    Keystroke,
};
use workspace_model::{KeybindingOverlay, Keymap, KeymapProblem, Shortcut};

use crate::settings_file::{self, AppSettings};

/// Marks the bindings registered here, so that binding again replaces only them. Bindings
/// without a meta count as 0 where GPUI compares metas (`NoAction` precedence), so the mark
/// changes nothing else.
const OWN_BINDING: KeyBindingMetaIndex = KeyBindingMetaIndex(0);

struct AppKeymap {
    defaults: Vec<Shortcut>,
    /// The overlay the bindings were built from.
    overlay: KeybindingOverlay,
    keymap: Keymap,
}

impl Global for AppKeymap {}

/// Add a view's default shortcuts (with the overlay applied) and register the bindings.
pub fn add_defaults(cx: &mut App, defaults: Vec<Shortcut>) {
    if !cx.has_global::<AppKeymap>() {
        let overlay = settings_file::keybindings(cx).cloned().unwrap_or_default();
        cx.set_global(AppKeymap {
            defaults: Vec::new(),
            overlay,
            keymap: Keymap::default(),
        });
        cx.observe_global::<AppSettings>(rebind_if_changed).detach();
    }
    cx.global_mut::<AppKeymap>().defaults.extend(defaults);
    bind(cx);
}

/// The overlay entries skipped, for the settings page.
pub fn problems(cx: &App) -> &[KeymapProblem] {
    cx.try_global::<AppKeymap>()
        .map_or(&[], |state| state.keymap.problems())
}

fn rebind_if_changed(cx: &mut App) {
    let Some(overlay) = settings_file::keybindings(cx) else {
        return;
    };
    if *overlay == cx.global::<AppKeymap>().overlay {
        return;
    }
    let overlay = overlay.clone();
    cx.global_mut::<AppKeymap>().overlay = overlay;
    bind(cx);
}

/// Build the keymap from the defaults and the overlay and register it in place of the one
/// registered before.
fn bind(cx: &mut App) {
    let state = cx.global::<AppKeymap>();
    let keymap = Keymap::new(state.defaults.clone(), &state.overlay, normalize);
    let own = own_bindings(&keymap, cx);
    replace_own_bindings(cx, own);
    cx.global_mut::<AppKeymap>().keymap = keymap;
}

/// `keymap`'s bindings as GPUI bindings, marked as ours; one that can't be built is left out.
fn own_bindings(keymap: &Keymap, cx: &App) -> Vec<KeyBinding> {
    keymap
        .bindings()
        .iter()
        .filter_map(|shortcut| match key_binding(shortcut, cx) {
            Ok(binding) => Some(binding.with_meta(OWN_BINDING)),
            Err(err) => {
                eprintln!("agent-workspace: can't bind {}: {err}", shortcut.keys);
                None
            }
        })
        .collect()
}

/// Register `own` in place of the bindings marked as ours, keeping every other binding
/// (gpui-component's, the terminal's pass-through keys) in its order.
fn replace_own_bindings(cx: &mut App, own: Vec<KeyBinding>) {
    let others: Vec<KeyBinding> = cx
        .key_bindings()
        .borrow()
        .bindings()
        .filter(|binding| binding.meta() != Some(OWN_BINDING))
        .cloned()
        .collect();
    cx.clear_key_bindings();
    cx.bind_keys(others);
    cx.bind_keys(own);
}

fn key_binding(shortcut: &Shortcut, cx: &App) -> Result<KeyBinding, String> {
    let action = cx
        .build_action(shortcut.action, None)
        .map_err(|err| err.to_string())?;
    let context = shortcut
        .context
        .map(|context| KeyBindingContextPredicate::parse(context).map(Rc::new))
        .transpose()
        .map_err(|err| err.to_string())?;
    KeyBinding::load(
        &shortcut.keys,
        action,
        context,
        false,
        None,
        &DummyKeyboardMapper,
    )
    .map_err(|err| err.to_string())
}

/// `keys` in GPUI's spelling (`shift-ctrl-V` is `ctrl-shift-v`), each keystroke of a
/// sequence on its own, or GPUI's reason it can't be read.
fn normalize(keys: &str) -> Result<String, String> {
    keys.split_whitespace()
        .map(|stroke| {
            Keystroke::parse(stroke)
                .map(|keystroke| keystroke.unparse())
                .map_err(|err| err.to_string())
        })
        .collect::<Result<Vec<_>, _>>()
        .map(|strokes| strokes.join(" "))
}

#[cfg(test)]
mod tests;
