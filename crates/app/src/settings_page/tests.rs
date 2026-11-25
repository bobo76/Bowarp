use gpui_kit::component::Root;
use gpui_kit::{Context, Render, TestAppContext, VisualTestContext};

use super::*;

struct Page {
    field: SettingsField<String>,
    binding: Binding<String>,
    focus: gpui_kit::FocusHandle,
    input: Option<Entity<InputState>>,
    shown: bool,
}

impl Render for Page {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        if !self.shown {
            return div().track_focus(&self.focus);
        }
        let value = (self.field.get)(settings_file::settings(cx));
        let control = text_field(value, &self.binding, "".into(), window, cx);
        let state: Entity<TextField> =
            window.use_keyed_state(self.binding.state_key("input"), cx, |_, _| {
                unreachable!("text_field created the state")
            });
        self.input = Some(state.read(cx).input.clone());
        div().track_focus(&self.focus).child(control)
    }
}

fn open(
    cx: &mut TestAppContext,
    field: SettingsField<String>,
) -> (VisualTestContext, Entity<Page>) {
    cx.update(|cx| {
        gpui_kit::init(cx);
        settings_file::init_in_memory(cx);
    });
    let captured = std::cell::OnceCell::new();
    let window = cx.add_window(|window, cx| {
        let page = cx.new(|cx| {
            cx.observe_global::<settings_file::AppSettings>(|_, cx| cx.notify())
                .detach();
            let set = field.set.clone();
            Page {
                field,
                binding: Binding {
                    key: "test-text-field".into(),
                    apply: Rc::new(|_, _| unreachable!("text edits do not preview settings")),
                    save: Rc::new(move |value, cx| {
                        settings_file::update(cx, |settings| set(settings, value));
                    }),
                },
                focus: cx.focus_handle(),
                input: None,
                shown: true,
            }
        });
        captured.set(page.clone()).ok().unwrap();
        Root::new(page, window, cx)
    });
    let mut cx = VisualTestContext::from_window(window.into(), cx);
    cx.update(|window, _| window.activate_window());
    cx.run_until_parked();
    cx.update(|window, cx| {
        window.draw(cx).clear(cx);
    });
    (cx, captured.into_inner().unwrap())
}

fn saved(cx: &mut VisualTestContext, page: &Entity<Page>) -> String {
    cx.update(|_, cx| (page.read(cx).field.get)(settings_file::settings(cx)))
}

fn blur(cx: &mut VisualTestContext, page: &Entity<Page>) {
    cx.update(|window, cx| {
        let focus = page.read(cx).focus.clone();
        window.focus(&focus, cx);
        window.draw(cx).clear(cx);
    });
    cx.run_until_parked();
}

fn focus_input(cx: &mut VisualTestContext, page: &Entity<Page>) -> Entity<InputState> {
    let input = cx.update(|window, cx| {
        let input = page.read(cx).input.clone().unwrap();
        window.focus(&input.focus_handle(cx), cx);
        input
    });
    cx.run_until_parked();
    cx.update(|window, cx| {
        window.draw(cx).clear(cx);
    });
    input
}

#[gpui::test]
fn agent_edits_survive_a_redraw_before_blur_is_delivered(cx: &mut TestAppContext) {
    for field in [
        SettingsField::new(
            |settings| settings.claude_args.clone(),
            |settings, value| settings.claude_args = value,
        ),
        SettingsField::new(
            |settings| settings.copilot_args.clone(),
            |settings, value| settings.copilot_args = value,
        ),
    ] {
        let (mut cx, page) = open(cx, field);
        let input = focus_input(&mut cx, &page);
        cx.simulate_input("--model example");
        cx.update(|window, cx| {
            assert_eq!(input.read(cx).value().as_ref(), "--model example");
            assert_eq!((page.read(cx).field.get)(settings_file::settings(cx)), "");
            let focus = page.read(cx).focus.clone();
            window.focus(&focus, cx);
            window.draw(cx).clear(cx);
        });
        cx.run_until_parked();
        cx.update(|_, cx| {
            assert_eq!(input.read(cx).value().as_ref(), "--model example");
            assert_eq!(
                (page.read(cx).field.get)(settings_file::settings(cx)),
                "--model example"
            );
        });
    }
}

#[gpui::test]
fn enter_commits_without_ending_edits_and_blur_can_save_an_empty_value(cx: &mut TestAppContext) {
    let (mut cx, page) = open(
        cx,
        SettingsField::new(
            |settings| settings.copilot_args.clone(),
            |settings, value| settings.copilot_args = value,
        ),
    );
    let input = focus_input(&mut cx, &page);
    cx.simulate_input("--model example");
    cx.simulate_keystrokes("enter");
    assert_eq!(saved(&mut cx, &page), "--model example");
    cx.simulate_input(" --yolo");
    cx.update(|window, cx| {
        settings_file::update(cx, |settings| settings.claude_args = "--model opus".into());
        window.draw(cx).clear(cx);
        assert_eq!(input.read(cx).value().as_ref(), "--model example --yolo");
    });
    assert_eq!(saved(&mut cx, &page), "--model example");
    blur(&mut cx, &page);
    assert_eq!(saved(&mut cx, &page), "--model example --yolo");
    focus_input(&mut cx, &page);
    cx.simulate_keystrokes("ctrl-a backspace");
    blur(&mut cx, &page);
    assert_eq!(saved(&mut cx, &page), "");
    cx.update(|_, cx| {
        assert_eq!(settings_file::settings(cx).claude_args, "--model opus");
        assert_eq!(input.read(cx).value().as_ref(), "");
    });
}

#[gpui::test]
fn font_edits_normalize_and_external_values_sync_without_overwriting_drafts(
    cx: &mut TestAppContext,
) {
    let (mut cx, page) = open(
        cx,
        SettingsField::new(
            |settings| settings.font_family.clone(),
            |settings, value| settings.font_family = value.trim().to_owned(),
        ),
    );
    let input = focus_input(&mut cx, &page);
    cx.simulate_input("  Example Font  ");
    cx.update(|window, cx| {
        settings_file::preview(cx, |settings| settings.font_family = "External Font".into());
        let focus = page.read(cx).focus.clone();
        window.focus(&focus, cx);
        window.draw(cx).clear(cx);
        assert_eq!(input.read(cx).value().as_ref(), "  Example Font  ");
    });
    cx.run_until_parked();
    assert_eq!(saved(&mut cx, &page), "Example Font");
    cx.update(|window, cx| {
        window.draw(cx).clear(cx);
        assert_eq!(input.read(cx).value().as_ref(), "Example Font");
        settings_file::update(cx, |settings| {
            settings.font_family = Settings::default().font_family;
        });
    });
    cx.run_until_parked();
    cx.update(|window, cx| {
        window.draw(cx).clear(cx);
        assert_eq!(
            input.read(cx).value().as_ref(),
            Settings::default().font_family
        );
        settings_file::preview(cx, |settings| settings.font_family = "Reloaded Font".into());
    });
    cx.run_until_parked();
    cx.update(|window, cx| {
        window.draw(cx).clear(cx);
        assert_eq!(input.read(cx).value().as_ref(), "Reloaded Font");
    });
}

#[gpui::test]
fn closing_and_reopening_the_page_keeps_the_blurred_edit(cx: &mut TestAppContext) {
    let (mut cx, page) = open(
        cx,
        SettingsField::new(
            |settings| settings.claude_args.clone(),
            |settings, value| settings.claude_args = value,
        ),
    );
    focus_input(&mut cx, &page);
    cx.simulate_input("--model opus");
    cx.update(|window, cx| {
        let focus = page.read(cx).focus.clone();
        window.focus(&focus, cx);
        page.update(cx, |page, cx| {
            page.shown = false;
            cx.notify();
        });
        window.draw(cx).clear(cx);
    });
    cx.run_until_parked();
    assert_eq!(saved(&mut cx, &page), "--model opus");
    cx.update(|window, cx| {
        page.update(cx, |page, cx| {
            page.shown = true;
            cx.notify();
        });
        window.draw(cx).clear(cx);
        assert_eq!(
            page.read(cx)
                .input
                .as_ref()
                .unwrap()
                .read(cx)
                .value()
                .as_ref(),
            "--model opus"
        );
    });
}

fn card_piece_caption(field: TabCardField, settings: &Settings) -> Option<String> {
    let states: Vec<_> = card_piece_parents(field)
        .iter()
        .map(|(name, parent)| (*name, parent(settings)))
        .collect();
    DependentState::requiring(&states).caption
}

#[test]
fn card_rows_for_changes_need_git_status_after_the_card() {
    let mut settings = Settings {
        git_status: FeatureSwitch::new(false),
        ..Settings::default()
    };
    let git_caption = format!("Needs \"{GIT_STATUS_TITLE}\" to be on.");
    for field in [TabCardField::Changes, TabCardField::ChangedFiles] {
        assert_eq!(
            card_piece_caption(field, &settings),
            Some(git_caption.clone())
        );
    }
    assert_eq!(card_piece_caption(TabCardField::Status, &settings), None);

    settings.tab_card.enabled = false;
    assert_eq!(
        card_piece_caption(TabCardField::Changes, &settings),
        Some("Needs \"Tab hover card\" to be on.".to_string())
    );
}

/// A slider row's control beside a title, laid out as gpui-kit lays out a setting row.
struct SliderPage {
    binding: Binding<f32>,
    slider: Option<Entity<SliderState>>,
}

impl Render for SliderPage {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let control = slider_field(
            OPACITY_SLIDER.max,
            OPACITY_SLIDER,
            &self.binding,
            window,
            cx,
        );
        let field: Entity<SliderField> =
            window.use_keyed_state(self.binding.state_key("slider"), cx, |_, _| {
                unreachable!("slider_field created the state")
            });
        self.slider = Some(field.read(cx).slider.clone());
        div()
            .w(px(800.))
            .flex()
            .justify_between()
            .items_center()
            .gap_3()
            .child(div().flex_1().max_w_3_5().child("Opacity ".repeat(40)))
            .child(
                div()
                    .id("field")
                    .child(div().flex().flex_col().items_end().gap_1().child(control)),
            )
    }
}

#[gpui::test]
fn a_slider_keeps_its_track_width_in_a_setting_row(cx: &mut TestAppContext) {
    cx.update(gpui_kit::init);
    let binding = Binding {
        key: "test-slider".into(),
        apply: Rc::new(|_, _| {}),
        save: Rc::new(|_, _| {}),
    };
    let captured = std::cell::OnceCell::new();
    let window = cx.add_window(|window, cx| {
        let page = cx.new(|_| SliderPage {
            binding,
            slider: None,
        });
        captured.set(page.clone()).ok().unwrap();
        Root::new(page, window, cx)
    });
    let page = captured.into_inner().unwrap();
    let mut cx = VisualTestContext::from_window(window.into(), cx);
    cx.update(|window, cx| {
        window.draw(cx).clear(cx);
        window.draw(cx).clear(cx);
    });
    let width = cx.update(|_, cx| {
        let slider = page.read(cx).slider.clone().unwrap();
        slider.read(cx).bounds().size.width
    });
    // `w_48`: 12 rem.
    assert_eq!(width, px(192.));
}
