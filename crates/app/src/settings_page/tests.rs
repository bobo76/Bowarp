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
