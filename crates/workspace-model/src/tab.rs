use serde::{Deserialize, Serialize};

/// Colors a tab can be marked with.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
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
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct TabMeta {
    /// Replaces the program's title in the sidebar.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub custom_name: Option<String>,
    /// Wins over the group's color.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub color: Option<TabColor>,
}

impl TabMeta {
    /// Set the custom name from user input; blank input goes back to the program's title.
    pub fn rename(&mut self, input: &str) {
        let name = input.trim();
        self.custom_name = (!name.is_empty()).then(|| name.to_string());
    }

    /// Drop the custom name: the tab shows the program's title again.
    pub fn reset_name(&mut self) {
        self.custom_name = None;
    }

    /// The name shown for the tab: the custom name, else the program's current title.
    pub fn display_name<'a>(&'a self, program_title: &'a str) -> &'a str {
        self.custom_name.as_deref().unwrap_or(program_title)
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

    #[test]
    fn reset_name_follows_the_program_title_again() {
        let mut meta = TabMeta::default();
        meta.rename("build");
        assert_eq!(meta.display_name("✳ Fix login bug"), "build");
        meta.reset_name();
        assert_eq!(meta.custom_name, None);
        assert_eq!(meta.display_name("✳ Fix login bug"), "✳ Fix login bug");
        assert_eq!(meta.display_name("✳ Add tests"), "✳ Add tests");
    }
}
