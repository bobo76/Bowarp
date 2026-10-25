//! The window's background settings: how opaque the terminal's background is
//! (`window_opacity`, a percent) and whether the desktop behind it is blurred (`window_blur`).
//! Only the terminal's default background and its padding take the opacity; the rest of the
//! window stays opaque. Blur shows only through that translucent background, so it needs an
//! opacity below 100 %, and it works only where the platform blurs: Windows (acrylic, and Mica
//! from Windows 11 22H2) and KDE Plasma on Wayland. Elsewhere the opacity still applies.

use serde::{Deserialize, Deserializer, Serialize};

/// The terminal background's opacity in percent, kept within
/// [`WindowOpacity::MIN`]..=[`WindowOpacity::MAX`]: a value outside is clamped when read, a
/// fraction is rounded, a value of the wrong type gives [`WindowOpacity::DEFAULT`].
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(transparent)]
pub struct WindowOpacity(u8);

impl WindowOpacity {
    pub const MIN: u8 = 30;
    pub const MAX: u8 = 100;
    pub const DEFAULT: u8 = 100;

    /// `percent` rounded and clamped to the range; not a number gives the default.
    pub fn new(percent: f64) -> Self {
        if percent.is_nan() {
            return Self::default();
        }
        Self(
            percent
                .round()
                .clamp(f64::from(Self::MIN), f64::from(Self::MAX)) as u8,
        )
    }

    pub fn percent(self) -> u8 {
        self.0
    }

    /// The alpha the terminal's background is painted with (1.0 at 100 %).
    pub fn alpha(self) -> f32 {
        f32::from(self.0) / 100.
    }

    pub fn is_opaque(self) -> bool {
        self.0 >= Self::MAX
    }
}

impl Default for WindowOpacity {
    fn default() -> Self {
        Self(Self::DEFAULT)
    }
}

impl<'de> Deserialize<'de> for WindowOpacity {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        f64::deserialize(deserializer).map(Self::new)
    }
}

/// What is behind the translucent terminal background. One value for every platform, so a
/// settings file copied between systems keeps working: on Linux `Mica` means `Blur`.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum WindowBlur {
    /// The desktop shows through as it is.
    #[default]
    None,
    /// The desktop is blurred: acrylic on Windows (`"acrylic"` in the file reads as this too).
    #[serde(alias = "acrylic")]
    Blur,
    /// Windows 11's Mica material; acrylic below Windows 11 22H2, Blur elsewhere.
    Mica,
}

impl WindowBlur {
    /// The value as written in the settings file.
    pub fn as_str(self) -> &'static str {
        match self {
            Self::None => "none",
            Self::Blur => "blur",
            Self::Mica => "mica",
        }
    }

    /// The value written by [`WindowBlur::as_str`]; anything else is `None`.
    pub fn from_name(name: &str) -> Option<Self> {
        [Self::None, Self::Blur, Self::Mica]
            .into_iter()
            .find(|blur| blur.as_str() == name)
    }
}

/// The first Windows build with the Mica backdrop (Windows 11 22H2).
pub const MICA_MIN_WINDOWS_BUILD: u32 = 22621;

/// Which blur the system offers.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BlurSupport {
    /// Windows always has acrylic; `mica` when the build has Mica too.
    Windows { mica: bool },
    /// Any other system: whether its compositor blurs behind the window.
    Other { blur: bool },
}

impl BlurSupport {
    /// On Windows, from the build number (see [`MICA_MIN_WINDOWS_BUILD`]).
    pub fn windows(build: u32) -> Self {
        Self::Windows {
            mica: build >= MICA_MIN_WINDOWS_BUILD,
        }
    }

    /// On Linux, from GPUI's compositor name (`"Wayland"` or `"X11"`) and
    /// `XDG_CURRENT_DESKTOP` (a `:`-separated list): only KDE Plasma's Wayland compositor
    /// offers the blur protocol GPUI uses; X11 has none.
    pub fn linux(compositor_name: &str, current_desktop: Option<&str>) -> Self {
        let kde = current_desktop.is_some_and(|desktops| {
            desktops
                .split(':')
                .any(|desktop| desktop.trim().eq_ignore_ascii_case("kde"))
        });
        Self::Other {
            blur: compositor_name == "Wayland" && kde,
        }
    }

    pub fn available(self) -> bool {
        match self {
            Self::Windows { .. } => true,
            Self::Other { blur } => blur,
        }
    }
}

/// The blur dropdown's choices on Windows (`windows`) or elsewhere: `(value, label)`.
pub fn blur_options(windows: bool) -> &'static [(WindowBlur, &'static str)] {
    if windows {
        &[
            (WindowBlur::None, "None"),
            (WindowBlur::Blur, "Acrylic"),
            (WindowBlur::Mica, "Mica"),
        ]
    } else {
        &[(WindowBlur::None, "None"), (WindowBlur::Blur, "Blur")]
    }
}

/// The choice the dropdown shows for `blur`: Mica shows as Blur where there is no Mica choice.
pub fn shown_blur(windows: bool, blur: WindowBlur) -> WindowBlur {
    match blur {
        WindowBlur::Mica if !windows => WindowBlur::Blur,
        blur => blur,
    }
}

/// The window background to ask the platform for (GPUI's `WindowBackgroundAppearance`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Backdrop {
    /// Nothing shows through: the default, and the cheapest for the compositor.
    Opaque,
    Transparent,
    Blurred,
    Mica,
}

impl Backdrop {
    pub fn new(opacity: WindowOpacity, blur: WindowBlur, support: BlurSupport) -> Self {
        if opacity.is_opaque() {
            return Self::Opaque;
        }
        if !support.available() {
            return Self::Transparent;
        }
        match (blur, support) {
            (WindowBlur::None, _) => Self::Transparent,
            (WindowBlur::Mica, BlurSupport::Windows { mica: true }) => Self::Mica,
            (WindowBlur::Blur | WindowBlur::Mica, _) => Self::Blurred,
        }
    }

    /// Whether anything shows through the window (the app's own background must then not
    /// cover the terminal's).
    pub fn is_translucent(self) -> bool {
        self != Self::Opaque
    }

    /// What Windows needs besides GPUI's own call to show `self` after `previous`: GPUI sets
    /// Mica through the DWM and the rest through the accent policy and never undoes the other.
    /// `mica_applied` is whether Mica was applied at any time since the window opened (going
    /// to `Opaque` doesn't clear it).
    pub fn windows_switch(self, previous: Option<Self>, mica_applied: bool) -> WindowsSwitch {
        WindowsSwitch {
            opaque_first: self == Self::Mica && previous != Some(Self::Mica),
            clear_mica: self != Self::Mica && mica_applied,
        }
    }
}

/// Extra steps before applying a [`Backdrop`] on Windows, see [`Backdrop::windows_switch`].
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct WindowsSwitch {
    /// Apply `Opaque` first, which disables the accent (acrylic) that would sit over Mica.
    pub opaque_first: bool,
    /// Set the DWM system backdrop to none first, which removes Mica.
    pub clear_mica: bool,
}

/// How the blur dropdown looks: disabled (keeping its value) when blur can't apply, with a
/// caption saying why, or a note while enabled.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct BlurControl {
    pub enabled: bool,
    pub caption: Option<&'static str>,
}

pub const BLUR_UNAVAILABLE: &str = "Not available on this system";
pub const BLUR_NEEDS_OPACITY: &str = "Needs Opacity below 100 %";
pub const MICA_FALLBACK: &str = "Mica needs Windows 11 22H2 or later; Acrylic is used";

impl BlurControl {
    pub fn new(opacity: WindowOpacity, blur: WindowBlur, support: BlurSupport) -> Self {
        let disabled = |caption| Self {
            enabled: false,
            caption: Some(caption),
        };
        if !support.available() {
            return disabled(BLUR_UNAVAILABLE);
        }
        if opacity.is_opaque() {
            return disabled(BLUR_NEEDS_OPACITY);
        }
        let mica_fallback =
            blur == WindowBlur::Mica && support == BlurSupport::Windows { mica: false };
        Self {
            enabled: true,
            caption: mica_fallback.then_some(MICA_FALLBACK),
        }
    }
}

#[cfg(test)]
mod tests;
