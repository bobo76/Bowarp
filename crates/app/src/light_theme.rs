//! The light-mode UI themes, Light and Sepia: gpui-kit's stock "Default Light" with tinted
//! surfaces (none lighter than #F6EEDC), darker muted text and visible borders, because the
//! stock near-white was hard on the eyes. Colors a palette leaves out keep the stock value; the
//! dark theme is the stock one. See docs/ux-decisions.md, "Light theme too white" and "Sepia
//! theme".

use gpui_kit::component::{ActiveTheme, ThemeConfig};
use gpui_kit::{App, Hsla, SharedString, rgb};

/// The colors of one light-mode theme, as 0xRRGGBB.
pub struct Palette {
    pub name: &'static str,
    background: u32,
    sidebar: u32,
    bar: u32,
    raised: u32,
    selected: u32,
    secondary: u32,
    secondary_hover: u32,
    secondary_active: u32,
    stripe: u32,
    border: u32,
    input_border: u32,
    ring: u32,
    switch_off: u32,
    foreground: u32,
    muted_foreground: u32,
    selection: u32,
    list_active: &'static str,
    list_active_border: u32,
    /// Buttons, switch tracks and sliders: background, hover, pressed, text. `None` keeps stock.
    primary: Option<[u32; 4]>,
    /// Working, NeedsAttention, ReadyForReview/Idle, Running, Active, Quiet/Unavailable/Exited.
    pub status: [u32; 6],
}

pub const SOFT_LIGHT: Palette = Palette {
    name: "Agent Workspace Light",
    background: 0xEEECE8,
    sidebar: 0xE6E3DE,
    bar: 0xE2DFDA,
    raised: 0xF5F3EF,
    selected: 0xD5D1CA,
    secondary: 0xDEDBD5,
    secondary_hover: 0xD8D4CE,
    secondary_active: 0xCDC9C2,
    stripe: 0xEAE7E3,
    border: 0xCCC8C1,
    input_border: 0xADA8A0,
    ring: 0x8A847B,
    switch_off: 0xC2BDB5,
    foreground: 0x1F1D1A,
    muted_foreground: 0x57534E,
    selection: 0xA9C4EB,
    list_active: "#2D6FD626",
    list_active_border: 0x2D6FD6,
    primary: None,
    status: [0x1F63C9, 0x9A5800, 0x1A7F37, 0x0B7568, 0x5F5A53, 0x8F897F],
};

pub const SEPIA: Palette = Palette {
    name: "Agent Workspace Sepia",
    background: 0xEFE4CC,
    sidebar: 0xE8DBBF,
    bar: 0xE3D5B8,
    raised: 0xF6EEDC,
    selected: 0xD9C8A6,
    secondary: 0xE0D2B4,
    secondary_hover: 0xD9C9A9,
    secondary_active: 0xCDBB97,
    stripe: 0xEBDFC6,
    border: 0xC9B794,
    input_border: 0xA8946F,
    ring: 0x8C7656,
    switch_off: 0xC4B08C,
    foreground: 0x3B2F22,
    muted_foreground: 0x5F4D3A,
    selection: 0xD8C08E,
    list_active: "#8B5E3C26",
    list_active_border: 0x8B5E3C,
    primary: Some([0x5B4636, 0x4A3828, 0x3B2F22, 0xF6EEDC]),
    status: [0x1F5AA6, 0x8A4700, 0x2D6A27, 0x0A665A, 0x5A4A39, 0x938168],
};

const PALETTES: [&Palette; 2] = [&SOFT_LIGHT, &SEPIA];

/// The palette of the theme in use, or `None` for the dark (stock) theme.
pub fn active_palette(cx: &App) -> Option<&'static Palette> {
    let theme = cx.theme();
    if theme.is_dark() {
        return None;
    }
    let name = theme.theme_name();
    PALETTES
        .into_iter()
        .find(|palette| palette.name == name.as_ref())
}

/// The theme `palette` describes, built over gpui-kit's stock light `stock`.
pub fn config(stock: &ThemeConfig, palette: &Palette) -> ThemeConfig {
    let mut config = stock.clone();
    config.name = palette.name.into();
    let colors = &mut config.colors;
    for (field, color) in [
        (&mut colors.background, palette.background),
        (&mut colors.sidebar, palette.sidebar),
        (&mut colors.title_bar, palette.bar),
        (&mut colors.status_bar, palette.bar),
        (&mut colors.popover, palette.raised),
        (&mut colors.list, palette.raised),
        (&mut colors.table, palette.raised),
        (&mut colors.accordion, palette.raised),
        (&mut colors.slider_thumb, palette.raised),
        (&mut colors.sidebar_accent, palette.selected),
        (&mut colors.secondary, palette.secondary),
        (&mut colors.muted, palette.secondary),
        (&mut colors.accent, palette.secondary),
        (&mut colors.tab_bar, palette.secondary),
        (&mut colors.group_box, palette.secondary),
        (&mut colors.skeleton, palette.secondary),
        (&mut colors.secondary_hover, palette.secondary_hover),
        (&mut colors.list_hover, palette.secondary_hover),
        (&mut colors.table_hover, palette.secondary_hover),
        (&mut colors.secondary_active, palette.secondary_active),
        (&mut colors.list_even, palette.stripe),
        (&mut colors.list_head, palette.stripe),
        (&mut colors.table_even, palette.stripe),
        (&mut colors.table_head, palette.stripe),
        (&mut colors.border, palette.border),
        (&mut colors.sidebar_border, palette.border),
        (&mut colors.title_bar_border, palette.border),
        (&mut colors.status_bar_border, palette.border),
        (&mut colors.window_border, palette.border),
        (&mut colors.table_row_border, palette.border),
        (&mut colors.input, palette.input_border),
        (&mut colors.ring, palette.ring),
        (&mut colors.switch, palette.switch_off),
        (&mut colors.foreground, palette.foreground),
        (&mut colors.popover_foreground, palette.foreground),
        (&mut colors.sidebar_foreground, palette.foreground),
        (&mut colors.sidebar_accent_foreground, palette.foreground),
        (&mut colors.secondary_foreground, palette.foreground),
        (&mut colors.accent_foreground, palette.foreground),
        (&mut colors.muted_foreground, palette.muted_foreground),
        (&mut colors.selection, palette.selection),
        (&mut colors.list_active_border, palette.list_active_border),
    ] {
        *field = Some(hex(color));
    }
    colors.list_active = Some(palette.list_active.into());
    if let Some([background, hover, active, foreground]) = palette.primary {
        colors.primary = Some(hex(background));
        colors.primary_hover = Some(hex(hover));
        colors.primary_active = Some(hex(active));
        colors.primary_foreground = Some(hex(foreground));
    }
    if let Some(highlight) = &mut config.highlight {
        highlight.editor_background = Some(hsla(palette.background));
        highlight.editor_active_line = Some(hsla(palette.sidebar));
    }
    config
}

fn hex(color: u32) -> SharedString {
    format!("#{color:06X}").into()
}

pub fn hsla(color: u32) -> Hsla {
    rgb(color).into()
}

#[cfg(test)]
mod tests {
    use gpui_kit::TestAppContext;
    use gpui_kit::component::{Theme, ThemeMode, ThemeRegistry};

    use super::*;

    #[gpui::test]
    fn each_applied_palette_sets_its_surfaces_and_is_found_again(cx: &mut TestAppContext) {
        cx.update(|cx| {
            gpui_kit::init(cx);
            for palette in PALETTES {
                let config = config(ThemeRegistry::global(cx).default_light_theme(), palette);
                Theme::global_mut(cx).light_theme = config.into();
                Theme::change(ThemeMode::Light, None, cx);
                let theme = cx.theme();
                assert_eq!(theme.background, hsla(palette.background));
                assert_eq!(theme.sidebar, hsla(palette.sidebar));
                assert_eq!(theme.popover, hsla(palette.raised));
                assert_eq!(theme.muted_foreground, hsla(palette.muted_foreground));
                assert_eq!(
                    active_palette(cx).map(|found| found.name),
                    Some(palette.name)
                );
            }
            Theme::change(ThemeMode::Dark, None, cx);
            assert!(active_palette(cx).is_none());
        });
    }
}
