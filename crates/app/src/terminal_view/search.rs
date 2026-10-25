use gpui_kit::component::button::{Button, ButtonVariants};
use gpui_kit::component::input::{Escape, Input, InputEvent, InputState};
use gpui_kit::component::{ActiveTheme, IconName, Sizable};
use gpui_kit::{
    AnyElement, AppContext, Context, Entity, InteractiveElement, IntoElement, ParentElement,
    Styled, Subscription, Window, div,
};
use term_core::TerminalSearch;
use workspace_model::layout::Direction;

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
                            let direction = if *shift {
                                Direction::Up
                            } else {
                                Direction::Down
                            };
                            view.step_terminal_search(direction, window, cx);
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

    fn step_terminal_search(
        &mut self,
        direction: Direction,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        {
            let mut term = self.session.lock_term();
            self.search.refresh(&mut term);
            self.search.step(direction);
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
                            view.step_terminal_search(Direction::Up, window, cx);
                        })),
                )
                .child(
                    Button::new("next-match")
                        .icon(IconName::ChevronDown)
                        .ghost()
                        .small()
                        .tooltip("Next match (Enter)")
                        .on_click(cx.listener(|view, _, window, cx| {
                            view.step_terminal_search(Direction::Down, window, cx);
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
mod tests;
