use alacritty_terminal::term::color::{COUNT, Colors};
use alacritty_terminal::vte::ansi::{Color, NamedColor, Rgb};

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
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Palette {
    pub foreground: Rgb,
    pub background: Rgb,
    pub cursor: Rgb,
    /// Black, red, green, yellow, blue, magenta, cyan, white, then their bright variants.
    pub ansi: [Rgb; 16],
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
        }
    }
}

impl Palette {
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
            DIM_BLACK..=DIM_WHITE => dim(self.ansi[index - DIM_BLACK]),
            i if i == NamedColor::DimForeground as usize => dim(self.foreground),
            _ => self.foreground,
        }
    }
}

/// Darken a color for the SGR "dim" attribute.
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
}
