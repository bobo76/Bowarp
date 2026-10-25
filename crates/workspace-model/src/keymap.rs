//! The keyboard shortcuts: the app's default bindings with the user's `"keybindings"` overlay
//! from the settings file applied key by key. An overlay entry maps a keystroke (as GPUI
//! writes it: `ctrl-shift-v`, or a sequence `ctrl-k ctrl-c`) to an action name, which adds a
//! binding or replaces the defaults for that key, or to `null`, which removes them (the key
//! then goes to the terminal). An entry that can't be used is skipped, with the reason kept for
//! the settings page; the rest still apply.

use std::fmt;

use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};

/// The `"keybindings"` object of the settings file, kept exactly as written (entries that
/// can't be used too) so that saving the settings never loses the user's text. Boxed: with
/// serde_json's `preserve_order` (enabled in the build) the map is large inline.
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
#[serde(transparent)]
pub struct KeybindingOverlay(Box<Map<String, Value>>);

/// Equal when the entries are the same and in the same order: with `preserve_order` the order
/// decides which of two spellings of one key is used, and `Map`'s own `==` ignores it.
impl PartialEq for KeybindingOverlay {
    fn eq(&self, other: &Self) -> bool {
        self.0.iter().eq(other.0.iter())
    }
}

impl KeybindingOverlay {
    pub fn new(entries: Map<String, Value>) -> Self {
        Self(Box::new(entries))
    }

    /// Each keystroke as written, with its value.
    pub fn entries(&self) -> &Map<String, Value> {
        &self.0
    }
}

/// One binding: keystrokes to an action by its full GPUI name (`terminal::Paste`), matched in a
/// key context (`None`: anywhere in the window).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Shortcut {
    pub keys: String,
    pub action: &'static str,
    pub context: Option<&'static str>,
}

impl Shortcut {
    pub fn new(
        keys: impl Into<String>,
        action: &'static str,
        context: Option<&'static str>,
    ) -> Self {
        Self {
            keys: keys.into(),
            action,
            context,
        }
    }
}

/// Why an overlay entry is skipped.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum KeymapReason {
    /// The key can't be read as keystrokes; the parser's message.
    BadKeystroke(String),
    /// The value names no action the app has.
    UnknownAction(String),
    /// The value is neither an action name nor `null`.
    NotAnAction,
    /// `null` for a key no default uses: nothing to remove.
    NothingToRemove,
    /// Another entry spells the same keystrokes, and that one is used.
    SameKeysAs(String),
}

/// A skipped overlay entry: its key as written and why.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct KeymapProblem {
    pub keys: String,
    pub reason: KeymapReason,
}

impl fmt::Display for KeymapProblem {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "\"{}\" is ignored: ", self.keys)?;
        match &self.reason {
            KeymapReason::BadKeystroke(message) => write!(f, "{message}"),
            KeymapReason::UnknownAction(name) => write!(f, "there is no action \"{name}\"."),
            KeymapReason::NotAnAction => write!(f, "expected an action name or null."),
            KeymapReason::NothingToRemove => {
                write!(f, "no default shortcut uses it, so null removes nothing.")
            }
            KeymapReason::SameKeysAs(other) => {
                write!(f, "\"{other}\" is the same shortcut and is used instead.")
            }
        }
    }
}

/// The defaults with an overlay applied: the bindings to register, and the entries skipped.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Keymap {
    defaults: Vec<Shortcut>,
    bindings: Vec<Shortcut>,
    problems: Vec<KeymapProblem>,
}

/// What one usable overlay entry does to its key.
struct Override {
    keys: String,
    written: String,
    /// `None` removes the defaults.
    binding: Option<Shortcut>,
}

impl Keymap {
    /// `defaults` with `overlay` applied. The actions that can be bound are the ones the
    /// defaults use, each in its default's context. `normalize` reads a key as keystrokes and
    /// writes them in one spelling (`shift-ctrl-V` and `ctrl-shift-v` are one key), or says
    /// why it can't; defaults it can't read are kept as written.
    pub fn new(
        defaults: Vec<Shortcut>,
        overlay: &KeybindingOverlay,
        normalize: impl Fn(&str) -> Result<String, String>,
    ) -> Self {
        let defaults: Vec<Shortcut> = defaults
            .into_iter()
            .map(|shortcut| Shortcut {
                keys: normalize(&shortcut.keys).unwrap_or(shortcut.keys),
                ..shortcut
            })
            .collect();
        let mut problems = Vec::new();
        let mut overrides: Vec<Override> = Vec::new();
        for (written, value) in overlay.entries() {
            match parse_entry(&defaults, written, value, &normalize) {
                Ok(entry) => push_override(&mut overrides, entry, &mut problems),
                Err(reason) => problems.push(problem(written, reason)),
            }
        }
        let bindings = merge(&defaults, overrides);
        Self {
            defaults,
            bindings,
            problems,
        }
    }

    /// The bindings to register, defaults first.
    pub fn bindings(&self) -> &[Shortcut] {
        &self.bindings
    }

    /// The overlay entries skipped, in the overlay's order.
    pub fn problems(&self) -> &[KeymapProblem] {
        &self.problems
    }

    /// Every action that can be bound, with its context, in the defaults' order.
    pub fn actions(&self) -> impl Iterator<Item = (&'static str, Option<&'static str>)> + '_ {
        self.defaults
            .iter()
            .enumerate()
            .filter_map(|(index, shortcut)| {
                let first = !self.defaults[..index]
                    .iter()
                    .any(|earlier| earlier.action == shortcut.action);
                first.then_some((shortcut.action, shortcut.context))
            })
    }

    /// The keys bound to `action` by default.
    pub fn default_keys<'a>(&'a self, action: &'a str) -> impl Iterator<Item = &'a str> + 'a {
        keys_for(&self.defaults, action)
    }

    /// The keys bound to `action` with the overlay applied.
    pub fn keys<'a>(&'a self, action: &'a str) -> impl Iterator<Item = &'a str> + 'a {
        keys_for(&self.bindings, action)
    }
}

fn keys_for<'a>(shortcuts: &'a [Shortcut], action: &'a str) -> impl Iterator<Item = &'a str> + 'a {
    shortcuts
        .iter()
        .filter(move |shortcut| shortcut.action == action)
        .map(|shortcut| shortcut.keys.as_str())
}

/// What the overlay entry `written: value` does, or why it is skipped.
fn parse_entry(
    defaults: &[Shortcut],
    written: &str,
    value: &Value,
    normalize: impl Fn(&str) -> Result<String, String>,
) -> Result<Override, KeymapReason> {
    let keys = normalize_keys(written, normalize).map_err(KeymapReason::BadKeystroke)?;
    let binding = match value {
        Value::Null if defaults.iter().any(|shortcut| shortcut.keys == keys) => None,
        Value::Null => return Err(KeymapReason::NothingToRemove),
        Value::String(name) => {
            let action = find_action(defaults, name)
                .ok_or_else(|| KeymapReason::UnknownAction(name.clone()))?;
            Some(Shortcut::new(keys.clone(), action.action, action.context))
        }
        _ => return Err(KeymapReason::NotAnAction),
    };
    Ok(Override {
        keys,
        written: written.to_owned(),
        binding,
    })
}

/// Add `entry`, replacing an earlier entry for the same keys (reported as skipped).
fn push_override(
    overrides: &mut Vec<Override>,
    entry: Override,
    problems: &mut Vec<KeymapProblem>,
) {
    if let Some(index) = overrides.iter().position(|old| old.keys == entry.keys) {
        let old = overrides.remove(index);
        let reason = KeymapReason::SameKeysAs(entry.written.clone());
        problems.push(problem(&old.written, reason));
    }
    overrides.push(entry);
}

/// The defaults whose keys no override uses, then the overrides' bindings.
fn merge(defaults: &[Shortcut], overrides: Vec<Override>) -> Vec<Shortcut> {
    let mut bindings: Vec<Shortcut> = defaults
        .iter()
        .filter(|shortcut| overrides.iter().all(|entry| entry.keys != shortcut.keys))
        .cloned()
        .collect();
    bindings.extend(overrides.into_iter().filter_map(|entry| entry.binding));
    bindings
}

fn problem(keys: &str, reason: KeymapReason) -> KeymapProblem {
    KeymapProblem {
        keys: keys.to_owned(),
        reason,
    }
}

fn normalize_keys(
    written: &str,
    normalize: impl Fn(&str) -> Result<String, String>,
) -> Result<String, String> {
    if written.trim().is_empty() {
        return Err("no keystroke.".to_owned());
    }
    normalize(written)
}

/// The default binding `name` stands for (its action and context): the action's full name
/// (`terminal::Paste`) or the part after the last `::` (`Paste`), exactly, else ignoring ASCII
/// case.
fn find_action<'a>(defaults: &'a [Shortcut], name: &str) -> Option<&'a Shortcut> {
    let name = name.trim();
    let names = |shortcut: &Shortcut| [shortcut.action, short_name(shortcut.action)];
    defaults
        .iter()
        .find(|shortcut| names(shortcut).contains(&name))
        .or_else(|| {
            defaults.iter().find(|shortcut| {
                names(shortcut)
                    .iter()
                    .any(|candidate| candidate.eq_ignore_ascii_case(name))
            })
        })
}

/// An action's name without its namespace: `Paste` for `terminal::Paste`.
pub fn short_name(action: &str) -> &str {
    action.rsplit("::").next().unwrap_or(action)
}

#[cfg(test)]
mod tests;
