//! The Git page diff's colors, as set on the settings page: the diff's background and text,
//! and the tint of added and removed lines. Each is a hex string (`#RGB`, `#RRGGBB` or
//! `#RRGGBBAA`, the `#` optional); empty or not a color gives its default: the UI theme's
//! background and text, a dark green and a dark red.

use serde::{Deserialize, Serialize};

/// A color with 8-bit channels; `a` is the opacity (255 opaque).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct RgbaColor {
    pub r: u8,
    pub g: u8,
    pub b: u8,
    pub a: u8,
}

impl RgbaColor {
    pub const fn new(r: u8, g: u8, b: u8, a: u8) -> Self {
        Self { r, g, b, a }
    }

    /// `#RGB`, `#RRGGBB` or `#RRGGBBAA` (the `#` optional, either case), else `None`.
    pub fn parse(text: &str) -> Option<Self> {
        let hex = text.trim();
        let hex = hex.strip_prefix('#').unwrap_or(hex);
        if !hex.is_ascii() || !hex.chars().all(|c| c.is_ascii_hexdigit()) {
            return None;
        }
        let channel = |index: usize, width: usize| {
            let value = u8::from_str_radix(&hex[index * width..(index + 1) * width], 16).ok()?;
            Some(if width == 1 { value * 0x11 } else { value })
        };
        match hex.len() {
            3 => Some(Self::new(
                channel(0, 1)?,
                channel(1, 1)?,
                channel(2, 1)?,
                u8::MAX,
            )),
            6 | 8 => Some(Self::new(
                channel(0, 2)?,
                channel(1, 2)?,
                channel(2, 2)?,
                if hex.len() == 8 {
                    channel(3, 2)?
                } else {
                    u8::MAX
                },
            )),
            _ => None,
        }
    }

    /// `#RRGGBB` when opaque, else `#RRGGBBAA`: what [`RgbaColor::parse`] reads back.
    pub fn to_hex(self) -> String {
        let Self { r, g, b, a } = self;
        if a == u8::MAX {
            format!("#{r:02X}{g:02X}{b:02X}")
        } else {
            format!("#{r:02X}{g:02X}{b:02X}{a:02X}")
        }
    }
}

/// The diff colors as written in the settings file; empty strings (the default) for the
/// default colors. Read them through the methods, which apply the defaults.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct DiffColors {
    #[serde(deserialize_with = "crate::settings::lenient")]
    pub background: String,
    #[serde(deserialize_with = "crate::settings::lenient")]
    pub text: String,
    #[serde(deserialize_with = "crate::settings::lenient")]
    pub added: String,
    #[serde(deserialize_with = "crate::settings::lenient")]
    pub removed: String,
}

impl DiffColors {
    /// An opaque dark green (`#14803D`).
    pub const DEFAULT_ADDED: RgbaColor = RgbaColor::new(0x14, 0x80, 0x3D, u8::MAX);
    /// An opaque dark red (`#7F1D1D`), to match [`DiffColors::DEFAULT_ADDED`].
    pub const DEFAULT_REMOVED: RgbaColor = RgbaColor::new(0x7F, 0x1D, 0x1D, u8::MAX);

    /// The diff's background; `None` for the UI theme's.
    pub fn background(&self) -> Option<RgbaColor> {
        RgbaColor::parse(&self.background)
    }

    /// The color of the lines' text; `None` for the UI theme's.
    pub fn text(&self) -> Option<RgbaColor> {
        RgbaColor::parse(&self.text)
    }

    /// The background of added lines.
    pub fn added(&self) -> RgbaColor {
        RgbaColor::parse(&self.added).unwrap_or(Self::DEFAULT_ADDED)
    }

    /// The background of removed lines.
    pub fn removed(&self) -> RgbaColor {
        RgbaColor::parse(&self.removed).unwrap_or(Self::DEFAULT_REMOVED)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_reads_short_long_and_translucent_hex_with_or_without_hash() {
        assert_eq!(
            RgbaColor::parse("#fff"),
            Some(RgbaColor::new(255, 255, 255, 255))
        );
        assert_eq!(
            RgbaColor::parse("1a2B3c"),
            Some(RgbaColor::new(0x1A, 0x2B, 0x3C, 255))
        );
        assert_eq!(
            RgbaColor::parse(" #1A2B3C80 "),
            Some(RgbaColor::new(0x1A, 0x2B, 0x3C, 0x80))
        );
    }

    #[test]
    fn parse_refuses_anything_else() {
        for text in [
            "", "#", "#12", "#12345", "#1234567", "#GGGGGG", "red", "#ééé",
        ] {
            assert_eq!(RgbaColor::parse(text), None, "{text:?}");
        }
    }

    #[test]
    fn to_hex_round_trips_and_drops_an_opaque_alpha() {
        let opaque = RgbaColor::new(0x0A, 0xBC, 0xFF, 255);
        assert_eq!(opaque.to_hex(), "#0ABCFF");
        let translucent = RgbaColor::new(0x0A, 0xBC, 0xFF, 0x40);
        assert_eq!(translucent.to_hex(), "#0ABCFF40");
        assert_eq!(RgbaColor::parse(&translucent.to_hex()), Some(translucent));
    }

    #[test]
    fn empty_or_invalid_colors_give_their_defaults() {
        let colors = DiffColors {
            background: String::new(),
            text: "nope".into(),
            added: String::new(),
            removed: "#12".into(),
        };
        assert_eq!(colors.background(), None);
        assert_eq!(colors.text(), None);
        assert_eq!(colors.added(), DiffColors::DEFAULT_ADDED);
        assert_eq!(colors.removed(), DiffColors::DEFAULT_REMOVED);
    }

    #[test]
    fn set_colors_win_over_the_defaults() {
        let colors = DiffColors {
            background: "#000".into(),
            text: "#ffffff".into(),
            added: "#00ff0080".into(),
            removed: "#ff0000".into(),
        };
        assert_eq!(colors.background(), Some(RgbaColor::new(0, 0, 0, 255)));
        assert_eq!(colors.text(), Some(RgbaColor::new(255, 255, 255, 255)));
        assert_eq!(colors.added(), RgbaColor::new(0, 255, 0, 0x80));
        assert_eq!(colors.removed(), RgbaColor::new(255, 0, 0, 255));
    }

    #[test]
    fn settings_read_diff_colors_leniently() {
        let settings = crate::Settings::from_json(
            r##"{"git_diff_colors": {"added": "#00ff00", "removed": 5}}"##,
        )
        .unwrap();
        assert_eq!(settings.git_diff_colors.added, "#00ff00");
        assert_eq!(settings.git_diff_colors.removed, "");
        let settings = crate::Settings::from_json(r#"{"git_diff_colors": "red"}"#).unwrap();
        assert_eq!(settings.git_diff_colors, DiffColors::default());
    }
}
