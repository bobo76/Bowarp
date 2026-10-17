/// Colors a tab can be marked with.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TabColor {
    Red,
    Orange,
    Yellow,
    Green,
    Blue,
    Pink,
    Purple,
}

impl TabColor {
    pub const ALL: [Self; 7] = [
        Self::Red,
        Self::Orange,
        Self::Yellow,
        Self::Green,
        Self::Blue,
        Self::Pink,
        Self::Purple,
    ];

    pub fn label(self) -> &'static str {
        match self {
            Self::Red => "Red",
            Self::Orange => "Orange",
            Self::Yellow => "Yellow",
            Self::Green => "Green",
            Self::Blue => "Blue",
            Self::Pink => "Pink",
            Self::Purple => "Purple",
        }
    }
}

/// What the user set on a tab, as opposed to what its session reports.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct TabMeta {
    /// Replaces the program's title in the sidebar.
    pub custom_name: Option<String>,
    /// Pinned tabs stay at the top and are spared by "close other tabs" / "close tabs below".
    pub pinned: bool,
    pub color: Option<TabColor>,
}

impl TabMeta {
    /// Set the custom name from user input; blank input goes back to the program's title.
    pub fn rename(&mut self, input: &str) {
        let name = input.trim();
        self.custom_name = (!name.is_empty()).then(|| name.to_string());
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rename_trims_and_blank_resets() {
        let mut meta = TabMeta::default();
        meta.rename("  build  ");
        assert_eq!(meta.custom_name.as_deref(), Some("build"));
        meta.rename("   ");
        assert_eq!(meta.custom_name, None);
    }
}
