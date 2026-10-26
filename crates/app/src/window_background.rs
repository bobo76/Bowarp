//! The window background from the `window_opacity` and `window_blur` settings (see
//! `workspace_model::Backdrop`). Below 100 % the window asks the platform for a see-through
//! (or blurred) background and the root view stops painting the theme background over it, so
//! the terminal's translucent background shows the desktop; every other pane paints its own
//! opaque background. At 100 % the window stays opaque, as before.

use std::cell::Cell;
use std::rc::Rc;

use gpui_kit::base::Root;
use gpui_kit::{AnyWindowHandle, App, Styled, Window, WindowBackgroundAppearance};
use workspace_model::{Backdrop, BlurSupport, WindowsSwitch};

use crate::settings_file::{self, AppSettings};

/// Which blur this system offers: on Windows from its build number, on Linux from the
/// compositor GPUI runs on and the desktop.
pub fn blur_support(cx: &App) -> BlurSupport {
    #[cfg(windows)]
    {
        let _ = cx;
        BlurSupport::windows(windows_build())
    }
    #[cfg(not(windows))]
    {
        static DESKTOP: std::sync::OnceLock<Option<String>> = std::sync::OnceLock::new();
        let desktop = DESKTOP.get_or_init(|| std::env::var("XDG_CURRENT_DESKTOP").ok());
        BlurSupport::linux(cx.compositor_name(), desktop.as_deref())
    }
}

/// The Windows build number (`CurrentBuildNumber` in the registry), read once; 0 if unknown.
#[cfg(windows)]
fn windows_build() -> u32 {
    static BUILD: std::sync::OnceLock<u32> = std::sync::OnceLock::new();
    *BUILD.get_or_init(|| {
        windows_registry::LOCAL_MACHINE
            .open(r"SOFTWARE\Microsoft\Windows NT\CurrentVersion")
            .and_then(|key| key.get_string("CurrentBuildNumber"))
            .ok()
            .and_then(|build| build.trim().parse().ok())
            .unwrap_or(0)
    })
}

fn backdrop(cx: &App) -> Backdrop {
    let settings = settings_file::settings(cx);
    Backdrop::new(
        settings.window_opacity,
        settings.window_blur,
        blur_support(cx),
    )
}

fn appearance(backdrop: Backdrop) -> WindowBackgroundAppearance {
    match backdrop {
        Backdrop::Opaque => WindowBackgroundAppearance::Opaque,
        Backdrop::Transparent => WindowBackgroundAppearance::Transparent,
        Backdrop::Blurred => WindowBackgroundAppearance::Blurred,
        Backdrop::Mica => WindowBackgroundAppearance::MicaBackdrop,
    }
}

/// The background to open the window with (`WindowOptions::window_background`).
pub fn initial(cx: &App) -> WindowBackgroundAppearance {
    appearance(backdrop(cx))
}

/// Once the window is open: apply the settings to its root view, and again on every change
/// (a slider step, the file edited by hand) that changes the background.
pub fn follow_settings(window: AnyWindowHandle, cx: &mut App) {
    let applied = Rc::new(Applied::default());
    apply(window, &applied, cx);
    // Observers run on no-op changes too: `apply` compares first. Deferred, so a change made
    // from inside the window (the settings page) never updates it from its own update.
    cx.observe_global::<AppSettings>(move |cx| {
        let applied = applied.clone();
        cx.defer(move |cx| apply(window, &applied, cx));
    })
    .detach();
}

/// What this window has had applied.
#[derive(Default)]
struct Applied {
    backdrop: Cell<Option<Backdrop>>,
    /// Mica at any time since the window opened: only the DWM can take it away again.
    mica: Cell<bool>,
}

/// Windows only: GPUI never undoes Mica (a DWM system backdrop) or the accent policy of the
/// other backdrops when it applies another one, so do that first.
fn prepare_switch(window: &mut Window, switch: WindowsSwitch) {
    #[cfg(windows)]
    {
        if switch.opaque_first {
            window.set_background_appearance(WindowBackgroundAppearance::Opaque);
        }
        if switch.clear_mica {
            clear_mica(window);
        }
    }
    #[cfg(not(windows))]
    let _ = (window, switch);
}

#[cfg(windows)]
fn clear_mica(window: &Window) {
    use raw_window_handle::{HasWindowHandle, RawWindowHandle};
    use windows_sys::Win32::Foundation::HWND;
    use windows_sys::Win32::Graphics::Dwm::{
        DWMSBT_NONE, DWMWA_SYSTEMBACKDROP_TYPE, DwmSetWindowAttribute,
    };

    let Ok(handle) = HasWindowHandle::window_handle(window) else {
        return;
    };
    let RawWindowHandle::Win32(handle) = handle.as_raw() else {
        return;
    };
    let none = DWMSBT_NONE;
    // SAFETY: `hwnd` is this window's live handle (we hold `&Window`); the attribute value
    // is a readable `i32` of the size passed. The HRESULT is ignored: a failure leaves Mica.
    unsafe {
        DwmSetWindowAttribute(
            handle.hwnd.get() as HWND,
            DWMWA_SYSTEMBACKDROP_TYPE as u32,
            (&raw const none).cast(),
            size_of_val(&none) as u32,
        );
    }
}

fn apply(window: AnyWindowHandle, applied: &Applied, cx: &mut App) {
    let backdrop = backdrop(cx);
    if applied.backdrop.get() == Some(backdrop) {
        return;
    }
    // Fails once the window is gone (nothing left to apply) or while it is being updated.
    let result = window.update(cx, |root, window, cx| {
        prepare_switch(
            window,
            backdrop.windows_switch(applied.backdrop.get(), applied.mica.get()),
        );
        window.set_background_appearance(appearance(backdrop));
        if let Ok(root) = root.downcast::<Root>() {
            root.update(cx, |root, cx| {
                // None gives the theme background back (gpui-component's root default).
                root.style().background = backdrop
                    .is_translucent()
                    .then(|| gpui_kit::transparent_black().into());
                cx.notify();
            });
        }
    });
    if result.is_ok() {
        applied.backdrop.set(Some(backdrop));
        applied
            .mica
            .set(applied.mica.get() || backdrop == Backdrop::Mica);
    }
}

/// The alpha of the terminal's default background and padding (1.0 at 100 % opacity, and
/// while settings aren't loaded: unit tests).
pub fn terminal_alpha(cx: &App) -> f32 {
    settings_file::window_opacity(cx).alpha()
}

#[cfg(test)]
mod tests {
    use gpui_kit::{AppContext, Render, TestAppContext, Window, div};
    use workspace_model::{WindowBlur, WindowOpacity};

    use super::*;

    struct Empty;

    impl Render for Empty {
        fn render(
            &mut self,
            _: &mut Window,
            _: &mut gpui_kit::Context<Self>,
        ) -> impl gpui_kit::IntoElement {
            div()
        }
    }

    #[gpui::test]
    fn root_background_follows_the_opacity(cx: &mut TestAppContext) {
        cx.update(|cx| {
            gpui_kit::init(cx);
            settings_file::init_in_memory(cx);
        });
        let window = cx.add_window(|window, cx| {
            let view = cx.new(|_| Empty);
            Root::new(view, window, cx)
        });
        let root_background = |cx: &mut TestAppContext| {
            window
                .update(cx, |root, _, _| root.style().background.is_some())
                .unwrap()
        };
        cx.update(|cx| {
            assert_eq!(initial(cx), WindowBackgroundAppearance::Opaque);
            assert_eq!(terminal_alpha(cx), 1.);
            follow_settings(window.into(), cx);
        });
        assert!(!root_background(cx), "opaque: the theme background stays");
        // As the settings page does: from inside the window.
        window
            .update(cx, |_, _, cx| {
                settings_file::preview(cx, |settings| {
                    settings.window_opacity = WindowOpacity::new(50.);
                    settings.window_blur = WindowBlur::Blur;
                });
            })
            .unwrap();
        cx.run_until_parked();
        assert!(root_background(cx), "see-through: the root paints nothing");
        cx.update(|cx| {
            assert_eq!(terminal_alpha(cx), 0.5);
            // No blur on the test platform (no compositor name): plain transparency.
            let expected = if cfg!(windows) {
                WindowBackgroundAppearance::Blurred
            } else {
                WindowBackgroundAppearance::Transparent
            };
            assert_eq!(initial(cx), expected);
            settings_file::preview(cx, |settings| {
                settings.window_opacity = WindowOpacity::default();
            });
        });
        cx.run_until_parked();
        assert!(!root_background(cx), "back to opaque");
    }
}
