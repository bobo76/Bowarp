use gpui_kit::{AppContext, Render, TestAppContext, Window, div};
use workspace_model::{WindowBlur, WindowOpacity};

use super::*;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Call {
    Appearance(Backdrop),
    ClearMica,
    Frame(bool),
}

#[derive(Default)]
struct FakeBackground {
    calls: Vec<Call>,
    fail: Option<Call>,
}

impl FakeBackground {
    fn call(&mut self, call: Call, operation: &str) -> Result<(), String> {
        self.calls.push(call);
        let result = if self.fail == Some(call) { i32::MIN } else { 0 };
        dwm_result(result, operation)
    }
}

impl BackgroundOperations for FakeBackground {
    fn set_appearance(&mut self, backdrop: Backdrop) {
        self.calls.push(Call::Appearance(backdrop));
    }

    fn clear_mica(&mut self) -> Result<(), String> {
        self.call(Call::ClearMica, "clear Mica")
    }

    fn set_mica_frame(&mut self, enabled: bool) -> Result<(), String> {
        self.call(Call::Frame(enabled), "update the Mica frame")
    }
}

#[test]
fn windows_transitions_order_native_calls_and_skip_unchanged_backdrops() {
    use Backdrop::*;
    use Call::*;

    for previous in [Opaque, Transparent, Blurred, Mica] {
        for next in [Opaque, Transparent, Blurred, Mica] {
            let applied = Applied::default();
            let mut native = FakeBackground::default();
            applied.update(previous, true, &mut native).unwrap();
            native.calls.clear();
            applied.update(next, true, &mut native).unwrap();
            let expected = if next == previous {
                vec![]
            } else if next == Mica {
                vec![Appearance(Opaque), Appearance(Mica), Frame(true)]
            } else if previous == Mica {
                vec![ClearMica, Frame(false), Appearance(next)]
            } else {
                vec![Appearance(next)]
            };
            assert_eq!(native.calls, expected, "{previous:?} -> {next:?}");
            assert_eq!(applied.backdrop.get(), Some(next));
            assert_eq!(applied.mica.get(), previous == Mica || next == Mica);
        }
    }
}

#[test]
fn failed_dwm_calls_stop_the_switch_and_can_be_retried() {
    use Backdrop::*;
    use Call::*;

    for (previous, next, failure, expected) in [
        (
            Transparent,
            Mica,
            Frame(true),
            vec![Appearance(Opaque), Appearance(Mica), Frame(true)],
        ),
        (Mica, Transparent, ClearMica, vec![ClearMica]),
        (Mica, Opaque, Frame(false), vec![ClearMica, Frame(false)]),
    ] {
        let applied = Applied::default();
        let mut native = FakeBackground::default();
        applied.update(previous, true, &mut native).unwrap();
        native.calls.clear();
        native.fail = Some(failure);
        let error = applied.update(next, true, &mut native).unwrap_err();
        assert!(error.contains("DWM 0x80000000"), "{error}");
        assert_eq!(native.calls, expected);
        assert_eq!(applied.backdrop.get(), None, "failure must allow retry");
        assert!(applied.mica.get(), "partial Mica must remain clearable");

        native.calls.clear();
        native.fail = None;
        applied.update(next, true, &mut native).unwrap();
        assert_eq!(
            native.calls,
            if next == Mica {
                vec![Appearance(Opaque), Appearance(Mica), Frame(true)]
            } else {
                vec![ClearMica, Frame(false), Appearance(next)]
            }
        );
        assert_eq!(applied.backdrop.get(), Some(next));
    }
}

#[test]
fn failed_mica_frame_is_cleared_when_the_user_selects_another_backdrop() {
    let applied = Applied::default();
    let mut native = FakeBackground {
        fail: Some(Call::Frame(true)),
        ..Default::default()
    };
    assert!(applied.update(Backdrop::Mica, true, &mut native).is_err());
    native.calls.clear();
    applied
        .update(Backdrop::Transparent, true, &mut native)
        .unwrap();
    assert_eq!(
        native.calls,
        [
            Call::ClearMica,
            Call::Frame(false),
            Call::Appearance(Backdrop::Transparent)
        ]
    );
}

#[test]
fn dwm_hresult_accepts_nonnegative_success_codes() {
    assert_eq!(dwm_result(0, "clear Mica"), Ok(()));
    assert_eq!(dwm_result(1, "clear Mica"), Ok(()));
    assert_eq!(
        dwm_result(-1, "clear Mica"),
        Err("Could not clear Mica (DWM 0xffffffff)".into())
    );
}

#[test]
fn non_windows_backdrops_do_not_call_dwm() {
    let applied = Applied::default();
    let mut native = FakeBackground::default();
    for backdrop in [Backdrop::Transparent, Backdrop::Blurred, Backdrop::Opaque] {
        applied.update(backdrop, false, &mut native).unwrap();
    }
    assert_eq!(
        native.calls,
        [
            Call::Appearance(Backdrop::Transparent),
            Call::Appearance(Backdrop::Blurred),
            Call::Appearance(Backdrop::Opaque)
        ]
    );
}

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
fn native_failure_notifies_without_changing_the_root_and_retry_applies_it(cx: &mut TestAppContext) {
    use gpui_kit::component::WindowExt;

    cx.update(gpui_kit::init);
    let window = cx.add_window(|window, cx| {
        let view = cx.new(|_| Empty);
        Root::new(view, window, cx)
    });
    let applied = Applied::default();
    let mut native = FakeBackground {
        fail: Some(Call::Frame(true)),
        ..Default::default()
    };
    for _ in 0..2 {
        cx.update(|cx| {
            apply_with(window.into(), &applied, Backdrop::Mica, cx, |_| {
                applied.update(Backdrop::Mica, true, &mut native)
            });
        });
        window
            .update(cx, |root, _, _| {
                assert!(root.style().background.is_none());
            })
            .unwrap();
        cx.update(|cx| {
            AnyWindowHandle::from(window)
                .update(cx, |_, window, cx| {
                    assert_eq!(
                        window.notifications(cx).len(),
                        1,
                        "same warning is replaced"
                    );
                })
                .unwrap();
        });
        assert_eq!(applied.backdrop.get(), None);
    }
    native.fail = None;
    cx.update(|cx| {
        apply_with(window.into(), &applied, Backdrop::Mica, cx, |_| {
            applied.update(Backdrop::Mica, true, &mut native)
        });
    });
    window
        .update(cx, |root, _, _| assert!(root.style().background.is_some()))
        .unwrap();
    assert_eq!(applied.backdrop.get(), Some(Backdrop::Mica));
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
