use gpui_kit::component::button::{Button, ButtonVariants};
use gpui_kit::component::input::{Escape, Input, InputEvent, InputState};
use gpui_kit::component::{ActiveTheme, IconName, Sizable};
use gpui_kit::{
    AnyElement, AppContext, Context, Entity, InteractiveElement, IntoElement, ParentElement,
    Styled, Subscription, Window, div,
};
use term_core::TerminalSearch;

use super::{CloseFind, Find, TerminalView};

pub(super) struct SearchBar {
    input: Entity<InputState>,
    error: Option<String>,
    _events: Subscription,
}

impl TerminalView {
    pub(super) fn on_find(&mut self, _: &Find, window: &mut Window, cx: &mut Context<Self>) {
        self.history_popup = None;
        if self.search_bar.is_none() {
            let input = cx.new(|cx| InputState::new(window, cx).placeholder("Search terminal..."));
            let events =
                cx.subscribe_in(
                    &input,
                    window,
                    |view, input, event, window, cx| match event {
                        InputEvent::Change => {
                            let result = view.search.set_query(&input.read(cx).value());
                            if let Err(error) = &result {
                                log::warn!("terminal search could not compile: {error}");
                            }
                            if let Some(bar) = &mut view.search_bar {
                                bar.error = result.err();
                            }
                            cx.notify();
                        }
                        InputEvent::PressEnter { shift, .. } => {
                            view.step_terminal_search(!shift, window, cx);
                        }
                        _ => {}
                    },
                );
            self.search_bar = Some(SearchBar {
                input,
                error: None,
                _events: events,
            });
        }
        if let Some(bar) = &self.search_bar {
            bar.input.update(cx, |input, cx| {
                input.select_all(window, cx);
                input.focus(window, cx);
            });
        }
        cx.notify();
    }

    pub(super) fn on_close_find(
        &mut self,
        _: &CloseFind,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.search_bar.is_some() {
            self.close_terminal_search(window, cx);
        } else {
            cx.propagate();
        }
    }

    fn close_terminal_search(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.search_bar = None;
        self.search = TerminalSearch::default();
        window.focus(&self.focus_handle, cx);
        cx.notify();
    }

    fn step_terminal_search(&mut self, forward: bool, window: &mut Window, cx: &mut Context<Self>) {
        {
            let mut term = self.session.lock_term();
            self.search.refresh(&mut term);
            self.search.step(forward);
            self.search.refresh(&mut term);
        }
        if let Some(bar) = &self.search_bar {
            bar.input.update(cx, |input, cx| input.focus(window, cx));
        }
        self.reveal_scrollbar(cx);
        cx.notify();
    }

    pub(super) fn render_terminal_search(&self, cx: &mut Context<Self>) -> Option<AnyElement> {
        let bar = self.search_bar.as_ref()?;
        let count = match self.search.current() {
            Some(index) => format!("{} of {}", index + 1, self.search.matches().len()),
            None => "0 matches".to_string(),
        };
        let theme = cx.theme();
        Some(
            div()
                .flex_none()
                .flex()
                .items_center()
                .gap_2()
                .px_3()
                .py_1()
                .bg(theme.secondary)
                .border_b_1()
                .border_color(theme.border)
                .text_sm()
                .text_color(theme.secondary_foreground)
                .on_action(cx.listener(|view, _: &Escape, window, cx| {
                    view.close_terminal_search(window, cx);
                }))
                .child(
                    div()
                        .flex_1()
                        .min_w_0()
                        .child(Input::new(&bar.input).small()),
                )
                .child(
                    div()
                        .text_color(if bar.error.is_some() {
                            theme.danger
                        } else {
                            theme.muted_foreground
                        })
                        .child(if bar.error.is_some() {
                            "Search unavailable".to_string()
                        } else {
                            count
                        }),
                )
                .child(
                    Button::new("previous-match")
                        .icon(IconName::ChevronUp)
                        .ghost()
                        .small()
                        .tooltip("Previous match (Shift+Enter)")
                        .on_click(cx.listener(|view, _, window, cx| {
                            view.step_terminal_search(false, window, cx);
                        })),
                )
                .child(
                    Button::new("next-match")
                        .icon(IconName::ChevronDown)
                        .ghost()
                        .small()
                        .tooltip("Next match (Enter)")
                        .on_click(cx.listener(|view, _, window, cx| {
                            view.step_terminal_search(true, window, cx);
                        })),
                )
                .child(
                    Button::new("close-search")
                        .icon(IconName::Close)
                        .ghost()
                        .small()
                        .tooltip("Close search (Esc)")
                        .on_click(cx.listener(|view, _, window, cx| {
                            view.close_terminal_search(window, cx);
                        })),
                )
                .into_any_element(),
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::RefCell;
    use std::rc::Rc;
    use std::sync::{Arc, mpsc};
    use std::time::Duration;

    use gpui_kit::component::Root;
    use gpui_kit::{Task, TestAppContext, VisualTestContext};
    use term_core::alacritty_terminal::index::{Column, Point};
    use term_core::alacritty_terminal::vte::ansi::{Processor, StdSyncHandler};
    use term_core::{LaunchSpec, Palette, Session, SessionEvent, SessionKind, TermDimensions};

    fn draw(cx: &mut VisualTestContext) {
        cx.run_until_parked();
        cx.update(|window, cx| window.draw(cx).clear(cx));
    }

    #[gpui::test]
    fn search_shortcuts_keep_input_out_of_the_program(cx: &mut TestAppContext) {
        cx.update(|cx| {
            gpui_kit::init(cx);
            crate::settings_file::init(cx);
            crate::terminal_element::init(cx);
            super::super::init(cx);
        });
        #[cfg(windows)]
        let (program, args) = (
            "powershell",
            vec!["-NoProfile", "-NonInteractive", "-Command", "exit 0"],
        );
        #[cfg(not(windows))]
        let (program, args) = ("/bin/sh", vec!["-c", "exit 0"]);
        let spec = LaunchSpec::new(
            SessionKind::Shell,
            Some(program.into()),
            args.into_iter().map(String::from).collect(),
            None,
        );
        let (exited, exit_events) = mpsc::channel();
        let session = Session::spawn(
            1,
            &spec,
            TermDimensions::new(80, 24),
            Palette::default(),
            Arc::new(move |event| {
                if let SessionEvent::Exited(_) = event {
                    exited.send(()).expect("test must wait for the child to exit");
                }
            }),
        )
        .unwrap();
        // Drain ConPTY startup output before seeding a grid that only the test can mutate.
        exit_events
            .recv_timeout(Duration::from_secs(30))
            .expect("test child did not exit and drain its terminal output");
        Processor::<StdSyncHandler>::new()
            .advance(&mut *session.lock_term(), b"\x1bcone one");
        let captured = Rc::new(RefCell::new(None));
        let capture = captured.clone();
        let window = cx.add_window(move |window, cx| {
            let view = cx.new(|cx| TerminalView::new(session, &spec, false, Task::ready(()), cx));
            view.update(cx, |view, cx| window.focus(&view.focus_handle, cx));
            *capture.borrow_mut() = Some(view.clone());
            Root::new(view, window, cx)
        });
        let view = captured.borrow().clone().unwrap();
        let mut cx = VisualTestContext::from_window(window.into(), cx);
        draw(&mut cx);
        cx.simulate_keystrokes("ctrl-shift-f");
        draw(&mut cx);
        assert!(view.read_with(&cx, |view, _| view.search_bar.is_some()));
        cx.simulate_input("ONE");
        draw(&mut cx);
        view.read_with(&cx, |view, _| {
            assert_eq!(view.search.matches().len(), 2);
            assert_eq!(view.search.current(), Some(0));
            assert!(!view.session.awaiting_echo());
        });
        cx.simulate_keystrokes("enter");
        draw(&mut cx);
        assert_eq!(
            view.read_with(&cx, |view, _| view.search.current()),
            Some(1)
        );
        assert!(!view.read_with(&cx, |view, _| view.session.awaiting_echo()));
        cx.simulate_keystrokes("shift-enter");
        draw(&mut cx);
        assert_eq!(
            view.read_with(&cx, |view, _| view.search.current()),
            Some(0)
        );
        assert!(!view.read_with(&cx, |view, _| view.session.awaiting_echo()));
        cx.simulate_keystrokes("ctrl-shift-f");
        draw(&mut cx);
        cx.simulate_input("absent");
        draw(&mut cx);
        view.read_with(&cx, |view, cx| {
            let bar = view.search_bar.as_ref().unwrap();
            assert_eq!(bar.input.read(cx).value().as_ref(), "absent");
            assert!(view.search.matches().is_empty());
            assert!(!view.session.awaiting_echo());
        });
        cx.simulate_keystrokes("escape");
        draw(&mut cx);
        cx.update(|window, cx| {
            let view = view.read(cx);
            assert!(view.search_bar.is_none());
            assert!(view.search.matches().is_empty());
            assert!(view.focus_handle.is_focused(window));
            assert!(!view.session.awaiting_echo());
            assert_eq!(view.session.visible_text()[0], "one one");
            assert_eq!(
                view.session.lock_term().grid().cursor.point,
                Point::new(0.into(), Column(7)),
            );
        });
        cx.simulate_input("x");
        view.read_with(&cx, |view, _| {
            assert!(view.session.awaiting_echo());
        });
    }
}
