use alacritty_terminal::term::color::{COUNT, Colors};
use alacritty_terminal::vte::ansi::{Color, NamedColor, Rgb};
use workspace_model::UiTheme;

const ANSI_COLOR_COUNT: usize = 16;
const COLOR_CUBE_START: usize = ANSI_COLOR_COUNT;
const COLOR_CUBE_SIDE: usize = 6;
const COLOR_CUBE_PLANE: usize = COLOR_CUBE_SIDE * COLOR_CUBE_SIDE;
const GRAYSCALE_START: usize = COLOR_CUBE_START + COLOR_CUBE_PLANE * COLOR_CUBE_SIDE;
const COLOR_CUBE_END: usize = GRAYSCALE_START - 1;
const COLOR_CUBE_STEP: usize = 40;
const COLOR_CUBE_BASE: usize = 55;
const GRAYSCALE_BASE: usize = 8;
const GRAYSCALE_STEP: usize = 10;

/// Default terminal colors. Programs may override individual entries at runtime (OSC 4/10/11),
/// which alacritty records in the terminal's [`Colors`].
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Palette {
    pub foreground: Rgb,
    pub background: Rgb,
    pub cursor: Rgb,
    /// The background of selected text.
    pub selection: Rgb,
    /// Search matches: background, current match's background, text.
    pub search: Rgb,
    pub active_search: Rgb,
    pub search_foreground: Rgb,
    /// Black, red, green, yellow, blue, magenta, cyan, white, then their bright variants.
    pub ansi: [Rgb; 16],
    /// Whether the background is light: dim text then fades toward it instead of darkening,
    /// and new tabs get `COLORFGBG=0;15`.
    pub light_background: bool,
}

const fn rgb(hex: u32) -> Rgb {
    Rgb {
        r: (hex >> 16) as u8,
        g: (hex >> 8) as u8,
        b: hex as u8,
    }
}

impl Default for Palette {
    fn default() -> Self {
        Self {
            foreground: rgb(0xd4d4d4),
            background: rgb(0x1e1e1e),
            cursor: rgb(0xaeafad),
            selection: rgb(0x264f78),
            search: rgb(0xe6c070),
            active_search: rgb(0xff9640),
            search_foreground: rgb(0x181818),
            ansi: [
                rgb(0x000000),
                rgb(0xcd3131),
                rgb(0x0dbc79),
                rgb(0xe5e510),
                rgb(0x2472c8),
                rgb(0xbc3fbc),
                rgb(0x11a8cd),
                rgb(0xe5e5e5),
                rgb(0x666666),
                rgb(0xf14c4c),
                rgb(0x23d18b),
                rgb(0xf5f543),
                rgb(0x3b8eea),
                rgb(0xd670d6),
                rgb(0x29b8db),
                rgb(0xe5e5e5),
            ],
            light_background: false,
        }
    }
}

impl Palette {
    /// The terminal colors of `theme`.
    pub fn for_theme(theme: UiTheme) -> Self {
        match theme {
            UiTheme::Dark => Self::default(),
            UiTheme::Light => Self::light(),
            UiTheme::Sepia => Self::sepia(),
        }
    }

    fn light() -> Self {
        Self {
            foreground: rgb(0x1f1d1a),
            background: rgb(0xf5f3ef),
            cursor: rgb(0x3a3631),
            selection: rgb(0xb4ccee),
            ansi: [
                0x2e2b27, 0xa82a20, 0x1c6e2a, 0x7a5600, 0x1d58b3, 0x8c2b93, 0x0a6878, 0x5e5953,
                0x6b665f, 0xbf3328, 0x217a35, 0x8a6100, 0x2867cc, 0x9c37a3, 0x0c7489, 0x8a847b,
            ]
            .map(rgb),
            light_background: true,
            ..Self::default()
        }
    }

    fn sepia() -> Self {
        Self {
            foreground: rgb(0x3b2f22),
            background: rgb(0xf6eedc),
            cursor: rgb(0x5b4636),
            selection: rgb(0xd8c08e),
            // The dark theme's match yellow is too close to the sepia selection.
            search: rgb(0xf2d35c),
            active_search: rgb(0xf29a4a),
            ansi: [
                0x2e241a, 0xa0301e, 0x3a651d, 0x765300, 0x22529a, 0x83377f, 0x146670, 0x5f4d3a,
                0x6e5d49, 0xb03823, 0x417022, 0x845e00, 0x2b5eac, 0x91408b, 0x19727d, 0x8a7860,
            ]
            .map(rgb),
            light_background: true,
            ..Self::default()
        }
    }

    /// `color` for the SGR "dim" attribute: darker on a dark background, closer to the
    /// background (3/4 color, 1/4 background) on a light one, where darkening would stand out.
    pub fn dim(&self, color: Rgb) -> Rgb {
        if !self.light_background {
            return dim(color);
        }
        let mix = |value: u8, background: u8| ((value as u16 * 3 + background as u16) / 4) as u8;
        Rgb {
            r: mix(color.r, self.background.r),
            g: mix(color.g, self.background.g),
            b: mix(color.b, self.background.b),
        }
    }

    /// The `COLORFGBG` value telling programs whether the background is dark or light.
    pub fn colorfgbg(&self) -> &'static str {
        if self.light_background {
            "0;15"
        } else {
            "15;0"
        }
    }

    /// Resolve a cell color, preferring runtime overrides set by the program.
    pub fn resolve(&self, color: Color, overrides: &Colors) -> Rgb {
        match color {
            Color::Spec(rgb) => rgb,
            Color::Indexed(index) => self.resolve_index(index as usize, overrides),
            Color::Named(named) => self.resolve_index(named as usize, overrides),
        }
    }

    /// Resolve a color index: 0-255 for the 256-color table, higher values for [`NamedColor`].
    pub fn resolve_index(&self, index: usize, overrides: &Colors) -> Rgb {
        if index < COUNT
            && let Some(rgb) = overrides[index]
        {
            return rgb;
        }
        self.default_color(index)
    }

    /// The palette's own color for an index, ignoring runtime overrides.
    pub fn default_color(&self, index: usize) -> Rgb {
        const DIM_BLACK: usize = NamedColor::DimBlack as usize;
        const DIM_WHITE: usize = NamedColor::DimWhite as usize;
        match index {
            0..=15 => self.ansi[index],
            COLOR_CUBE_START..=COLOR_CUBE_END => {
                let cube = index - COLOR_CUBE_START;
                let level = |value: usize| {
                    if value == 0 {
                        0
                    } else {
                        (value * COLOR_CUBE_STEP + COLOR_CUBE_BASE) as u8
                    }
                };
                Rgb {
                    r: level(cube / COLOR_CUBE_PLANE),
                    g: level((cube / COLOR_CUBE_SIDE) % COLOR_CUBE_SIDE),
                    b: level(cube % COLOR_CUBE_SIDE),
                }
            }
            GRAYSCALE_START..=255 => {
                let gray = (GRAYSCALE_BASE + (index - GRAYSCALE_START) * GRAYSCALE_STEP) as u8;
                Rgb {
                    r: gray,
                    g: gray,
                    b: gray,
                }
            }
            i if i == NamedColor::Foreground as usize => self.foreground,
            i if i == NamedColor::Background as usize => self.background,
            i if i == NamedColor::Cursor as usize => self.cursor,
            DIM_BLACK..=DIM_WHITE => self.dim(self.ansi[index - DIM_BLACK]),
            i if i == NamedColor::DimForeground as usize => self.dim(self.foreground),
            _ => self.foreground,
        }
    }
}

/// Darken a color for the SGR "dim" attribute on a dark background.
pub fn dim(color: Rgb) -> Rgb {
    let scale = |value: u8| (value as u16 * 2 / 3) as u8;
    Rgb {
        r: scale(color.r),
        g: scale(color.g),
        b: scale(color.b),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn indexed_colors_follow_xterm_table() {
        let palette = Palette::default();
        assert_eq!(palette.default_color(1), palette.ansi[1]);
        assert_eq!(palette.default_color(16), rgb(0x000000));
        assert_eq!(palette.default_color(196), rgb(0xff0000));
        assert_eq!(palette.default_color(231), rgb(0xffffff));
        assert_eq!(palette.default_color(232), rgb(0x080808));
        assert_eq!(palette.default_color(255), rgb(0xeeeeee));
    }

    #[test]
    fn named_colors_map_to_palette_entries() {
        let palette = Palette::default();
        let colors = Colors::default();
        assert_eq!(
            palette.resolve(Color::Named(NamedColor::Foreground), &colors),
            palette.foreground
        );
        assert_eq!(
            palette.resolve(Color::Named(NamedColor::Background), &colors),
            palette.background
        );
        assert_eq!(
            palette.resolve(Color::Named(NamedColor::DimRed), &colors),
            dim(palette.ansi[1])
        );
    }

    #[test]
    fn runtime_overrides_win() {
        let palette = Palette::default();
        let mut colors = Colors::default();
        colors[NamedColor::Background] = Some(rgb(0x123456));
        assert_eq!(
            palette.resolve(Color::Named(NamedColor::Background), &colors),
            rgb(0x123456)
        );
    }

    #[test]
    fn dim_fades_toward_a_light_background_and_darkens_on_dark() {
        let dark = Palette::default();
        assert_eq!(dark.dim(dark.foreground), dim(dark.foreground));
        let light = Palette::for_theme(UiTheme::Light);
        let faded = light.dim(rgb(0x000000));
        assert_eq!(faded, rgb(0x3d3c3b));
        assert_eq!(
            light.resolve(Color::Named(NamedColor::DimRed), &Colors::default()),
            light.dim(light.ansi[1])
        );
    }

    #[test]
    fn each_theme_has_its_own_colors_and_colorfgbg() {
        let [dark, light, sepia] =
            [UiTheme::Dark, UiTheme::Light, UiTheme::Sepia].map(Palette::for_theme);
        assert_eq!(dark, Palette::default());
        assert_ne!(light.background, sepia.background);
        assert_eq!(dark.colorfgbg(), "15;0");
        assert_eq!(light.colorfgbg(), "0;15");
        assert_eq!(sepia.colorfgbg(), "0;15");
    }
}
