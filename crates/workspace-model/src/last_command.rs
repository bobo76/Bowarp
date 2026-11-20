use crate::file_tree::ShellSyntax;
use crate::is_recordable;

const TRIVIAL_COMMANDS: &[&str] = &[
    "exit",
    "logout",
    "clear",
    "cls",
    "reset",
    "ls",
    "ll",
    "dir",
    "pwd",
    "cd",
    "history",
    // PowerShell names and aliases (the list is matched ignoring case, as PowerShell does).
    "clear-host",
    "set-location",
    "sl",
    "cd..",
    "cd\\",
    "get-childitem",
    "gci",
    "get-location",
    "gl",
    "get-history",
    "h",
];

/// A shell's saved command and its one-shot, never-executed restoration suggestion.
#[derive(Default)]
pub struct LastCommand {
    saved: Option<String>,
    restored: bool,
}

impl LastCommand {
    pub fn restore(&mut self, command: Option<String>) {
        self.saved = command.filter(|command| is_last_command(command));
        self.restored = self.saved.is_some();
    }

    pub fn saved(&self) -> Option<&str> {
        self.saved.as_deref()
    }

    pub fn suggestion(&self, line: &str, at_end: bool) -> Option<&str> {
        (self.restored && line.is_empty() && at_end)
            .then_some(self.saved.as_deref())
            .flatten()
    }

    pub fn dismiss(&mut self) {
        self.restored = false;
    }

    /// Trivial and private commands leave the previous saved command unchanged.
    pub fn record(&mut self, command: &str) -> bool {
        self.dismiss();
        if !is_last_command(command) || self.saved.as_deref() == Some(command) {
            return false;
        }
        self.saved = Some(command.to_owned());
        true
    }

    /// Drop the saved command ("Clear saved session commands"); true when there was one.
    pub fn clear(&mut self) -> bool {
        self.dismiss();
        self.saved.take().is_some()
    }

    pub fn forget(&mut self, command: &str) -> bool {
        if self.saved.as_deref() != Some(command) {
            return false;
        }
        self.saved = None;
        self.dismiss();
        true
    }
}

/// Whether an integrated tab running `shell` saves and offers its last command: the shells
/// whose `C` mark carries the command line (PowerShell's comes from PSReadLine's buffer, so
/// an inline prediction is not part of it). cmd has no integration.
pub fn saves_last_command(shell: ShellSyntax) -> bool {
    match shell {
        ShellSyntax::Posix | ShellSyntax::Wsl | ShellSyntax::PowerShell => true,
        ShellSyntax::Cmd => false,
    }
}

fn is_last_command(command: &str) -> bool {
    is_recordable(command)
        && command.split_whitespace().next().is_some_and(|word| {
            !TRIVIAL_COMMANDS
                .iter()
                .any(|trivial| trivial.eq_ignore_ascii_case(word))
        })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn trivial_or_private_commands_keep_the_last_useful_command() {
        let mut last = LastCommand::default();
        assert!(last.record("cargo test"));
        for command in TRIVIAL_COMMANDS
            .iter()
            .map(|word| format!("{word} argument"))
            .chain([
                " secret".into(),
                "bad\ncommand".into(),
                "".into(),
                "Clear-Host".into(),
                "EXIT 1".into(),
                "CD ..".into(),
            ])
        {
            assert!(!last.record(&command), "{command:?}");
            assert_eq!(last.saved(), Some("cargo test"));
        }
        assert!(last.record("lsof -i"));
        assert!(!last.record("lsof -i"));
    }

    #[test]
    fn integrated_shells_but_cmd_save_the_last_command() {
        for shell in [
            ShellSyntax::Posix,
            ShellSyntax::Wsl,
            ShellSyntax::PowerShell,
        ] {
            assert!(saves_last_command(shell), "{shell:?}");
        }
        assert!(!saves_last_command(ShellSyntax::Cmd));
    }

    #[test]
    fn clearing_drops_the_saved_command_and_its_suggestion() {
        let mut last = LastCommand::default();
        last.restore(Some("git push".into()));
        assert!(last.clear());
        assert_eq!(last.saved(), None);
        assert_eq!(last.suggestion("", true), None);
        assert!(!last.clear());
    }

    #[test]
    fn restoration_is_only_an_empty_line_suggestion_until_dismissed() {
        let mut last = LastCommand::default();
        last.restore(Some("git push".into()));
        assert_eq!(last.suggestion("", true), Some("git push"));
        assert_eq!(last.suggestion("g", true), None);
        assert_eq!(last.suggestion("", false), None);
        last.dismiss();
        assert_eq!(last.suggestion("", true), None);
        assert_eq!(last.saved(), Some("git push"));
    }

    #[test]
    fn forgetting_clears_the_saved_command_and_restoration() {
        let mut last = LastCommand::default();
        last.restore(Some("cargo test".into()));
        assert!(!last.forget("cargo build"));
        assert!(last.forget("cargo test"));
        assert_eq!(last.saved(), None);
        assert_eq!(last.suggestion("", true), None);
        for command in ["exit", " secret", "bad\rcommand"] {
            last.restore(Some(command.into()));
            assert_eq!(last.saved(), None);
        }
    }
}
