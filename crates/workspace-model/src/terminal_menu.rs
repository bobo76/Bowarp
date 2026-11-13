//! The terminal's right-click menu: which entries it shows, in which order, and which of them
//! can be chosen.

/// One line of the terminal's right-click menu.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TerminalMenuEntry {
    OpenLink,
    CopyLink,
    Separator,
    Copy,
    Paste,
    SelectAll,
    Search,
    ClearScrollback,
}

impl TerminalMenuEntry {
    pub fn label(self) -> &'static str {
        match self {
            Self::OpenLink => "Open link",
            Self::CopyLink => "Copy link",
            Self::Separator => "",
            Self::Copy => "Copy",
            Self::Paste => "Paste",
            Self::SelectAll => "Select all",
            Self::Search => "Search",
            Self::ClearScrollback => "Clear scrollback",
        }
    }

    /// Copy needs a selection; Clear scrollback needs the main screen (the alternate screen has
    /// no scrollback to clear); every other entry can always be chosen.
    pub fn is_enabled(self, state: TerminalMenuState) -> bool {
        match self {
            Self::Copy => state.has_selection,
            Self::ClearScrollback => !state.alt_screen,
            _ => true,
        }
    }
}

/// What the terminal looks like when the menu opens, to enable its entries.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct TerminalMenuState {
    pub has_selection: bool,
    /// A full-screen program shows (alternate screen).
    pub alt_screen: bool,
}

/// The menu's entries: the link's own first when the click was on a link.
pub fn terminal_menu(on_link: bool) -> Vec<TerminalMenuEntry> {
    use TerminalMenuEntry::*;
    let link = [OpenLink, CopyLink, Separator];
    let terminal = [Copy, Paste, SelectAll, Search, ClearScrollback];
    link.into_iter()
        .filter(|_| on_link)
        .chain(terminal)
        .collect()
}

#[cfg(test)]
mod tests {
    use super::TerminalMenuEntry::*;
    use super::*;

    #[test]
    fn a_link_puts_its_entries_first_then_a_separator() {
        let terminal = [Copy, Paste, SelectAll, Search, ClearScrollback];
        assert_eq!(terminal_menu(false), terminal);
        let on_link = terminal_menu(true);
        assert_eq!(on_link[..3], [OpenLink, CopyLink, Separator]);
        assert_eq!(on_link[3..], terminal);
    }

    fn disabled(state: TerminalMenuState) -> Vec<TerminalMenuEntry> {
        let entries = terminal_menu(true);
        let disabled = entries.into_iter().filter(|entry| !entry.is_enabled(state));
        disabled.collect()
    }

    #[test]
    fn only_copy_needs_a_selection() {
        let selected = TerminalMenuState {
            has_selection: true,
            alt_screen: false,
        };
        assert_eq!(disabled(TerminalMenuState::default()), [Copy]);
        assert!(disabled(selected).is_empty());
    }

    #[test]
    fn clear_scrollback_is_disabled_on_the_alternate_screen() {
        let full_screen = TerminalMenuState {
            has_selection: true,
            alt_screen: true,
        };
        assert_eq!(disabled(full_screen), [ClearScrollback]);
        let none = TerminalMenuState {
            has_selection: false,
            alt_screen: true,
        };
        assert_eq!(disabled(none), [Copy, ClearScrollback]);
    }

    #[test]
    fn entries_have_labels() {
        assert_eq!(OpenLink.label(), "Open link");
        assert_eq!(ClearScrollback.label(), "Clear scrollback");
        assert_eq!(SelectAll.label(), "Select all");
    }
}
