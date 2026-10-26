use super::*;
use crate::Settings;

fn opacity(percent: f64) -> WindowOpacity {
    WindowOpacity::new(percent)
}

const WINDOWS_11: BlurSupport = BlurSupport::Windows { mica: true };
const WINDOWS_10: BlurSupport = BlurSupport::Windows { mica: false };
const NO_BLUR: BlurSupport = BlurSupport::Other { blur: false };
const KDE_WAYLAND: BlurSupport = BlurSupport::Other { blur: true };

#[test]
fn opacity_defaults_to_opaque_and_clamps_file_values() {
    let read = |json: &str| Settings::from_json(json).unwrap().window_opacity;
    assert_eq!(read("{}").percent(), 100);
    assert!(read("{}").is_opaque());
    assert_eq!(read(r#"{"window_opacity": 85}"#).percent(), 85);
    assert_eq!(
        read(r#"{"window_opacity": 87}"#).percent(),
        87,
        "shown as is"
    );
    assert_eq!(read(r#"{"window_opacity": 10}"#).percent(), 30);
    assert_eq!(read(r#"{"window_opacity": -5}"#).percent(), 30);
    assert_eq!(read(r#"{"window_opacity": 250}"#).percent(), 100);
    assert_eq!(read(r#"{"window_opacity": 62.6}"#).percent(), 63);
    assert_eq!(
        read(r#"{"window_opacity": "80"}"#).percent(),
        100,
        "wrong type"
    );
    assert_eq!(read(r#"{"window_opacity": null}"#).percent(), 100);
}

#[test]
fn opacity_is_saved_as_an_integer_percent() {
    let settings = Settings {
        window_opacity: opacity(85.),
        window_blur: WindowBlur::Mica,
        ..Settings::default()
    };
    let json: serde_json::Value = serde_json::from_str(&settings.to_json().unwrap()).unwrap();
    assert_eq!(json["window_opacity"], serde_json::json!(85));
    assert_eq!(json["window_blur"], serde_json::json!("mica"));
    assert_eq!(opacity(85.).alpha(), 0.85);
    assert_eq!(opacity(f64::NAN), WindowOpacity::default());
}

#[test]
fn blur_reads_its_names_and_falls_back_to_none() {
    let read = |json: &str| Settings::from_json(json).unwrap().window_blur;
    assert_eq!(read("{}"), WindowBlur::None);
    assert_eq!(read(r#"{"window_blur": "blur"}"#), WindowBlur::Blur);
    assert_eq!(read(r#"{"window_blur": "acrylic"}"#), WindowBlur::Blur);
    assert_eq!(read(r#"{"window_blur": "mica"}"#), WindowBlur::Mica);
    assert_eq!(read(r#"{"window_blur": "frosted"}"#), WindowBlur::None);
    assert_eq!(read(r#"{"window_blur": true}"#), WindowBlur::None);
    for blur in [WindowBlur::None, WindowBlur::Blur, WindowBlur::Mica] {
        assert_eq!(WindowBlur::from_name(blur.as_str()), Some(blur));
    }
    assert_eq!(WindowBlur::from_name("acrylic"), None);
}

#[test]
fn linux_blurs_only_on_kde_wayland() {
    let blurs = |compositor, desktop| BlurSupport::linux(compositor, desktop).available();
    assert!(blurs("Wayland", Some("KDE")));
    assert!(blurs("Wayland", Some("kde")));
    assert!(blurs("Wayland", Some("ubuntu:KDE")));
    assert!(!blurs("Wayland", Some("ubuntu:GNOME")));
    assert!(!blurs("Wayland", Some("KDE-ish")));
    assert!(!blurs("Wayland", None));
    assert!(!blurs("X11", Some("KDE")), "X11 has no blur");
    assert!(!blurs("", Some("KDE")), "not Linux");
}

#[test]
fn windows_has_mica_from_22h2() {
    assert_eq!(BlurSupport::windows(19045), WINDOWS_10);
    assert_eq!(BlurSupport::windows(22000), WINDOWS_10);
    assert_eq!(BlurSupport::windows(22621), WINDOWS_11);
    assert!(WINDOWS_10.available());
}

#[test]
fn options_and_shown_choice_follow_the_platform() {
    let values = |windows| blur_options(windows).to_vec();
    assert_eq!(
        values(true),
        [
            (WindowBlur::None, "None"),
            (WindowBlur::Blur, "Acrylic"),
            (WindowBlur::Mica, "Mica")
        ]
    );
    assert_eq!(
        values(false),
        [(WindowBlur::None, "None"), (WindowBlur::Blur, "Blur")]
    );
    assert_eq!(shown_blur(false, WindowBlur::Mica), WindowBlur::Blur);
    assert_eq!(shown_blur(true, WindowBlur::Mica), WindowBlur::Mica);
    assert_eq!(shown_blur(false, WindowBlur::None), WindowBlur::None);
}

#[test]
fn backdrop_stays_opaque_at_full_opacity() {
    for support in [WINDOWS_11, WINDOWS_10, NO_BLUR, KDE_WAYLAND] {
        for blur in [WindowBlur::None, WindowBlur::Blur, WindowBlur::Mica] {
            assert_eq!(
                Backdrop::new(opacity(100.), blur, support),
                Backdrop::Opaque
            );
        }
    }
    assert!(!Backdrop::Opaque.is_translucent());
}

#[test]
fn backdrop_below_full_opacity() {
    let at_85 = |blur, support| Backdrop::new(opacity(85.), blur, support);
    assert_eq!(at_85(WindowBlur::None, WINDOWS_11), Backdrop::Transparent);
    assert_eq!(at_85(WindowBlur::Blur, WINDOWS_11), Backdrop::Blurred);
    assert_eq!(at_85(WindowBlur::Mica, WINDOWS_11), Backdrop::Mica);
    assert_eq!(
        at_85(WindowBlur::Mica, WINDOWS_10),
        Backdrop::Blurred,
        "acrylic"
    );
    assert_eq!(at_85(WindowBlur::Blur, KDE_WAYLAND), Backdrop::Blurred);
    assert_eq!(at_85(WindowBlur::Mica, KDE_WAYLAND), Backdrop::Blurred);
    assert_eq!(at_85(WindowBlur::Blur, NO_BLUR), Backdrop::Transparent);
    assert_eq!(at_85(WindowBlur::Mica, NO_BLUR), Backdrop::Transparent);
    assert!(Backdrop::Transparent.is_translucent());
}

#[test]
fn blur_control_says_why_it_is_disabled() {
    let control = |percent, blur, support| BlurControl::new(opacity(percent), blur, support);
    let disabled = |caption| BlurControl {
        enabled: false,
        caption: Some(caption),
    };
    let enabled = BlurControl {
        enabled: true,
        caption: None,
    };
    assert_eq!(
        control(85., WindowBlur::Blur, NO_BLUR),
        disabled(BLUR_UNAVAILABLE)
    );
    assert_eq!(
        control(100., WindowBlur::Blur, NO_BLUR),
        disabled(BLUR_UNAVAILABLE),
        "unavailable wins"
    );
    assert_eq!(
        control(100., WindowBlur::None, WINDOWS_11),
        disabled(BLUR_NEEDS_OPACITY)
    );
    assert_eq!(
        control(100., WindowBlur::Mica, WINDOWS_10),
        disabled(BLUR_NEEDS_OPACITY)
    );
    assert_eq!(control(85., WindowBlur::Blur, KDE_WAYLAND), enabled);
    assert_eq!(control(85., WindowBlur::Mica, WINDOWS_11), enabled);
    assert_eq!(control(85., WindowBlur::Blur, WINDOWS_10), enabled);
    assert_eq!(
        control(85., WindowBlur::Mica, WINDOWS_10),
        BlurControl {
            enabled: true,
            caption: Some(MICA_FALLBACK),
        }
    );
}

#[test]
fn windows_switch_resets_the_other_backdrop() {
    use Backdrop::*;
    let switch = |next: Backdrop, previous, mica| next.windows_switch(previous, mica);
    let steps = |opaque_first, clear_mica| WindowsSwitch {
        opaque_first,
        clear_mica,
    };
    // Entering Mica clears the accent first; nothing to clear of Mica itself.
    assert_eq!(switch(Mica, Some(Blurred), false), steps(true, false));
    assert_eq!(switch(Mica, None, false), steps(true, false));
    assert_eq!(switch(Mica, Some(Opaque), true), steps(true, false));
    assert_eq!(switch(Mica, Some(Mica), true), steps(false, false));
    // Leaving Mica, or any switch after it was applied once, clears the DWM backdrop.
    assert_eq!(switch(Transparent, Some(Mica), true), steps(false, true));
    assert_eq!(switch(Blurred, Some(Mica), true), steps(false, true));
    assert_eq!(switch(Opaque, Some(Mica), true), steps(false, true));
    assert_eq!(switch(Blurred, Some(Opaque), true), steps(false, true));
    // Mica never applied: GPUI's call is enough.
    assert_eq!(
        switch(Blurred, Some(Transparent), false),
        steps(false, false)
    );
    assert_eq!(switch(Opaque, None, false), steps(false, false));
}
