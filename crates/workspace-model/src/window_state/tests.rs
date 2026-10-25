use super::*;

fn rect(x: f32, y: f32, width: f32, height: f32) -> Rect {
    Rect {
        x,
        y,
        width,
        height,
    }
}

fn windowed(bounds: Rect) -> WindowState {
    WindowState {
        bounds,
        mode: WindowMode::Windowed,
    }
}

#[test]
fn json_round_trip() {
    let state = WindowState {
        bounds: rect(-1200., 40.5, 1000., 650.),
        mode: WindowMode::Maximized,
    };
    assert_eq!(WindowState::from_json(&state.to_json()), Some(state));
    assert!(state.to_json().contains("\"maximized\""));
}

#[test]
fn missing_mode_is_windowed() {
    let text = r#"{"bounds":{"x":10,"y":20,"width":800,"height":600}}"#;
    let state = WindowState::from_json(text).unwrap();
    assert_eq!(state, windowed(rect(10., 20., 800., 600.)));
}

#[test]
fn rejects_damaged_or_unusable_files() {
    assert_eq!(WindowState::from_json(""), None);
    assert_eq!(WindowState::from_json("{\"bounds\":"), None);
    assert_eq!(WindowState::from_json("[1, 2]"), None);
    let tiny = r#"{"bounds":{"x":0,"y":0,"width":20,"height":600}}"#;
    assert_eq!(WindowState::from_json(tiny), None);
    let huge = r#"{"bounds":{"x":0,"y":0,"width":1e9,"height":600}}"#;
    assert_eq!(WindowState::from_json(huge), None);
    let tallest = r#"{"bounds":{"x":0,"y":0,"width":800,"height":16384}}"#;
    assert!(WindowState::from_json(tallest).is_some());
    let too_tall = r#"{"bounds":{"x":0,"y":0,"width":800,"height":16385}}"#;
    assert_eq!(WindowState::from_json(too_tall), None);
    let unknown_mode = r#"{"bounds":{"x":0,"y":0,"width":800,"height":600},"mode":"tiled"}"#;
    assert_eq!(WindowState::from_json(unknown_mode), None);
}

#[test]
fn on_display_needs_a_grabbable_overlap() {
    let displays = [rect(0., 0., 1920., 1080.), rect(1920., 0., 1280., 1024.)];
    assert!(windowed(rect(100., 100., 1000., 650.)).is_on_any_display(&displays));
    // On the second display only.
    assert!(windowed(rect(2000., 300., 800., 600.)).is_on_any_display(&displays));
    // Mostly off the right edge of the second display, but a corner still shows.
    assert!(windowed(rect(3100., 900., 800., 600.)).is_on_any_display(&displays));
    // Only a 20 px sliver on screen.
    assert!(!windowed(rect(3180., 100., 800., 600.)).is_on_any_display(&displays));
    // Where an unplugged display on the left used to be.
    assert!(!windowed(rect(-1500., 100., 1000., 650.)).is_on_any_display(&displays));
    // Below every display.
    assert!(!windowed(rect(100., 1100., 1000., 650.)).is_on_any_display(&displays));
    assert!(!windowed(rect(100., 100., 1000., 650.)).is_on_any_display(&[]));
}

#[test]
fn on_display_limit_is_exactly_min_visible() {
    let display = [rect(0., 0., 1920., 1080.)];
    // 48 px of the window's width on screen counts, 47.9 px does not.
    assert!(windowed(rect(1872., 100., 800., 600.)).is_on_any_display(&display));
    assert!(!windowed(rect(1872.1, 100., 800., 600.)).is_on_any_display(&display));
    // Same for the height, off the bottom edge.
    assert!(windowed(rect(100., 1032., 800., 600.)).is_on_any_display(&display));
    assert!(!windowed(rect(100., 1032.1, 800., 600.)).is_on_any_display(&display));
}

#[test]
fn maximized_and_full_screen_keep_the_saved_bounds_and_save_as_maximized() {
    let saved = windowed(rect(30., 40., 900., 700.));
    let screen = rect(0., 0., 1920., 1080.);
    for mode in [WindowMode::Maximized, WindowMode::Fullscreen] {
        let closing = WindowState {
            bounds: screen,
            mode,
        };
        let expected = WindowState {
            bounds: saved.bounds,
            mode: WindowMode::Maximized,
        };
        assert_eq!(closing.keeping_saved_bounds(Some(saved)), expected);
        // Nothing saved yet: what the platform reports is all there is.
        let reported = WindowState {
            bounds: screen,
            mode: WindowMode::Maximized,
        };
        assert_eq!(closing.keeping_saved_bounds(None), reported);
    }
}

#[test]
fn windowed_replaces_the_saved_bounds() {
    let saved = WindowState {
        bounds: rect(30., 40., 900., 700.),
        mode: WindowMode::Maximized,
    };
    let closing = windowed(rect(200., 100., 1200., 800.));
    assert_eq!(closing.keeping_saved_bounds(Some(saved)), closing);
    assert_eq!(closing.keeping_saved_bounds(None), closing);
}

#[test]
fn save_then_load() {
    let dir = std::env::temp_dir().join(format!("agentws-window-{}", std::process::id()));
    let path = dir.join("nested").join("window.json");
    let state = windowed(rect(30., 40., 900., 700.));
    state.save(&path).unwrap();
    assert_eq!(WindowState::load(&path), Some(state));
    assert!(!temp_path(&path).exists());
    fs::write(&path, "not json").unwrap();
    assert_eq!(WindowState::load(&path), None);
    assert_eq!(WindowState::load(&dir.join("missing.json")), None);
    fs::remove_dir_all(&dir).unwrap();
}
