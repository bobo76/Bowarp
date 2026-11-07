//! Conversions between the settings' [`RgbaColor`] and GPUI's colors.

use gpui_kit::{Hsla, Rgba};
use workspace_model::RgbaColor;

const CHANNEL_MAX: f32 = u8::MAX as f32;

pub fn to_hsla(color: RgbaColor) -> Hsla {
    let channel = |value: u8| f32::from(value) / CHANNEL_MAX;
    Rgba {
        r: channel(color.r),
        g: channel(color.g),
        b: channel(color.b),
        a: channel(color.a),
    }
    .into()
}

pub fn from_hsla(color: Hsla) -> RgbaColor {
    let rgba = color.to_rgb();
    // Clamped to 0..=255 first, so the cast can't truncate.
    let channel = |value: f32| (value.clamp(0., 1.) * CHANNEL_MAX).round() as u8;
    RgbaColor::new(
        channel(rgba.r),
        channel(rgba.g),
        channel(rgba.b),
        channel(rgba.a),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn colors_round_trip_through_hsla() {
        for color in [
            RgbaColor::new(0x86, 0xEF, 0xAC, 0x33),
            RgbaColor::new(0, 0, 0, 255),
            RgbaColor::new(255, 255, 255, 0),
            RgbaColor::new(0x12, 0x34, 0x56, 0x78),
        ] {
            assert_eq!(from_hsla(to_hsla(color)), color);
        }
    }
}
