//! Command history per shell: the key a tab records its commands under, and the HISTORY
//! popup's shell chips ("All", then one per shell) with their Alt+letter keys.

use crate::history::{CommandHistory, HistoryScope};
use crate::keymap::HeldModifiers;
use crate::launcher_icon::LauncherIcon;

/// The WSL launcher's distro flag (`wsl.exe -d Ubuntu`).
const WSL_DISTRO_FLAGS: &[&str] = &["-d", "--distribution"];

/// Menu names of the shells the app knows, by launcher id, for a chip whose shell has no
/// discovered launcher (uninstalled since).
const SHELL_LABELS: &[(&str, &str)] = &[
    ("bash", "Bash"),
    ("zsh", "Zsh"),
    ("fish", "Fish"),
    ("pwsh", "PowerShell"),
    ("powershell", "Windows PowerShell"),
    ("cmd", "Command Prompt"),
    ("git-bash", "Git Bash"),
    ("wsl", "WSL"),
    ("shell", "Shell"),
];

/// The key a shell run as `program` with `args` records its history under when it matches no
/// discovered launcher: `wsl:<distro>` for `wsl -d <distro>` (`wsl` without one), `git-bash`
/// for Git for Windows' bash, `wsl` for Windows' own `bash.exe`, else the program's lowercased
/// file stem (`bash`, `zsh`, `pwsh`). `program` is a name or a path, `/` or `\\` separated;
/// pass the default shell for a session started without one. `shell` when there is none.
pub fn shell_key(program: Option<&str>, args: &[String], windows: bool) -> String {
    let Some(program) = program else {
        return "shell".into();
    };
    let mut parts: Vec<&str> = program.split(['/', '\\']).collect();
    let name = parts.pop().unwrap_or(program);
    let stem = match name.rsplit_once('.') {
        Some((stem, extension)) if extension.eq_ignore_ascii_case("exe") => stem,
        _ => name,
    };
    let stem = stem.to_lowercase();
    match stem.as_str() {
        "" => "shell".into(),
        "wsl" => args
            .iter()
            .position(|arg| WSL_DISTRO_FLAGS.contains(&arg.as_str()))
            .and_then(|flag| args.get(flag + 1))
            .map_or_else(|| "wsl".into(), |distro| format!("wsl:{distro}")),
        "bash" if windows && in_folder(&parts, "git") => "git-bash".into(),
        "bash" if windows && in_folder(&parts, "system32") => "wsl".into(),
        _ => stem,
    }
}

fn in_folder(parts: &[&str], folder: &str) -> bool {
    parts.iter().any(|part| part.eq_ignore_ascii_case(folder))
}

/// A shell the new-tab menu offers, in menu order: its launcher id, menu name and icon.
#[derive(Clone, Copy, Debug)]
pub struct KnownShell<'a> {
    pub id: &'a str,
    pub label: &'a str,
    pub icon: LauncherIcon,
}

/// The menu name and icon of shell `key`: its launcher's, else a known shell name, the distro
/// name of `wsl:<distro>` or the key itself, with the generic shell icon.
pub fn shell_name(key: &str, known: &[KnownShell]) -> (String, LauncherIcon) {
    if let Some(shell) = known.iter().find(|shell| shell.id == key) {
        return (shell.label.into(), shell.icon);
    }
    if let Some(&(_, label)) = SHELL_LABELS.iter().find(|&&(id, _)| id == key) {
        return (label.into(), LauncherIcon::Shell);
    }
    if let Some(distro) = key.strip_prefix("wsl:").filter(|distro| !distro.is_empty()) {
        return (distro.into(), LauncherIcon::Shell);
    }
    (key.into(), LauncherIcon::Shell)
}

/// One of the HISTORY popup's chips.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ShellChip {
    pub scope: HistoryScope,
    pub label: String,
    /// `None` for "All".
    pub icon: Option<LauncherIcon>,
}

impl ShellChip {
    /// The letter Alt switches to it with: its label's first, lowercased.
    pub fn letter(&self) -> Option<char> {
        self.label.chars().next().map(lowercase)
    }
}

fn lowercase(letter: char) -> char {
    letter.to_lowercase().next().unwrap_or(letter)
}

/// The popup's chips for a tab of shell `own` with `selected` chosen: "All", then the shells
/// that have commands, `own` and `selected` in menu order (`known`), then the others by label.
/// Empty (no chips shown) when every command was run in `own`.
pub fn history_chips(
    history: &CommandHistory,
    own: &str,
    selected: &HistoryScope,
    known: &[KnownShell],
) -> Vec<ShellChip> {
    let mut keys: Vec<&str> = Vec::new();
    let mut shellless = false;
    for shell in history.shells() {
        match shell {
            Some(key) => keys.push(key),
            None => shellless = true,
        }
    }
    keys.push(own);
    if let HistoryScope::Shell(key) = selected {
        keys.push(key);
    }
    keys.sort_unstable();
    keys.dedup();
    if !shellless && keys == [own] {
        return Vec::new();
    }
    let mut shells: Vec<(Option<usize>, String, LauncherIcon, &str)> = keys
        .into_iter()
        .map(|key| {
            let position = known.iter().position(|shell| shell.id == key);
            let (label, icon) = shell_name(key, known);
            (position, label, icon, key)
        })
        .collect();
    // Menu order first (`None` sorts before `Some`, hence the flag), then by label.
    shells.sort_by_cached_key(|(position, label, ..)| {
        (position.is_none(), *position, label.to_lowercase())
    });
    let all = ShellChip {
        scope: HistoryScope::All,
        label: "All".into(),
        icon: None,
    };
    std::iter::once(all)
        .chain(shells.into_iter().map(|(_, label, icon, key)| ShellChip {
            scope: HistoryScope::Shell(key.into()),
            label,
            icon: Some(icon),
        }))
        .collect()
}

/// The chip Alt+`key` switches to from `selected`: the first chip whose letter is `key`
/// (ignoring case, Shift allowed), or the next one with that letter after the selected chip,
/// wrapping. `None` without Alt, with Ctrl (AltGr on Windows), Super or Fn, or when no chip
/// has that letter.
pub fn switch_chip(
    chips: &[ShellChip],
    selected: &HistoryScope,
    key: &str,
    held: HeldModifiers,
) -> Option<usize> {
    if !held.alt || held.control || held.platform || held.function {
        return None;
    }
    let mut chars = key.chars();
    let (Some(letter), None) = (chars.next(), chars.next()) else {
        return None;
    };
    let letter = lowercase(letter);
    let matching: Vec<usize> = chips
        .iter()
        .enumerate()
        .filter(|(_, chip)| chip.letter() == Some(letter))
        .map(|(index, _)| index)
        .collect();
    let current = matching
        .iter()
        .position(|&index| chips[index].scope == *selected);
    match current {
        Some(position) => matching.get((position + 1) % matching.len()).copied(),
        None => matching.first().copied(),
    }
}

#[cfg(test)]
mod tests;
