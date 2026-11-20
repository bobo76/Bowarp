//! "Open in folder…" of a remote host's `+` submenu: a folder on the host and what to run
//! there, offered as last time (`RemoteHosts::offered_folder`, `RemoteHost::last_kind`).

use gpui_kit::component::button::{Button, ButtonGroup, ButtonVariants};
use gpui_kit::component::input::{Escape, Input, InputEvent, InputState};
use gpui_kit::component::{ActiveTheme, Selectable, Sizable, WindowExt};
use gpui_kit::{
    App, AppContext, Context, Entity, InteractiveElement, IntoElement, ParentElement, Render,
    Styled, Subscription, WeakEntity, Window, div, px,
};
use term_core::SessionKind;

use super::WorkspaceView;
use crate::launcher;
use crate::sidebar;

const DIALOG_WIDTH: f32 = 440.;

/// What the dialog offers, and where its answer goes.
pub(super) struct Request {
    /// The host's ssh destination.
    pub destination: String,
    /// The host's name, for the title.
    pub label: String,
    pub folder: String,
    pub kind: SessionKind,
}

/// The dialog's form: the folder field and the program picker.
pub(super) struct OpenInFolder {
    destination: String,
    input: Entity<InputState>,
    kind: SessionKind,
    workspace: WeakEntity<WorkspaceView>,
    _enter: Subscription,
}

impl OpenInFolder {
    /// Open the dialog: Open or Enter opens the choice on the host, Cancel or Escape nothing.
    pub(super) fn open(
        workspace: WeakEntity<WorkspaceView>,
        request: Request,
        window: &mut Window,
        cx: &mut App,
    ) -> Entity<Self> {
        let title = format!("Open on {}", request.label);
        let form = cx.new(|cx| Self::new(workspace, request, window, cx));
        let shown = form.clone();
        window.open_dialog(cx, move |dialog, _, _| {
            let submitted = shown.clone();
            dialog
                .title(title.clone())
                .w(px(DIALOG_WIDTH))
                .child(shown.clone())
                .footer(
                    div()
                        .flex()
                        .justify_end()
                        .gap_2()
                        .child(
                            Button::new("open-in-folder-cancel")
                                .label("Cancel")
                                .outline()
                                .on_click(|_, window, cx| window.close_dialog(cx)),
                        )
                        .child(
                            Button::new("open-in-folder-open")
                                .label("Open")
                                .primary()
                                .on_click(move |_, window, cx| {
                                    submitted.update(cx, |form, cx| form.submit(window, cx))
                                }),
                        ),
                )
        });
        form
    }

    fn new(
        workspace: WeakEntity<WorkspaceView>,
        request: Request,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        let input = cx.new(|cx| {
            InputState::new(window, cx)
                .placeholder("Home folder")
                .default_value(request.folder)
        });
        input.update(cx, |input, cx| {
            input.select_all(window, cx);
            input.focus(window, cx);
        });
        let enter = cx.subscribe_in(&input, window, |form, _, event, window, cx| {
            if matches!(event, InputEvent::PressEnter { .. }) {
                form.submit(window, cx);
            }
        });
        Self {
            destination: request.destination,
            input,
            kind: request.kind,
            workspace,
            _enter: enter,
        }
    }

    /// The folder typed (empty: the home folder) and the program picked.
    pub(super) fn choice(&self, cx: &App) -> (String, SessionKind) {
        (self.input.read(cx).value().to_string(), self.kind)
    }

    pub(super) fn select(&mut self, kind: SessionKind, cx: &mut Context<Self>) {
        if self.kind != kind {
            self.kind = kind;
            cx.notify();
        }
    }

    /// Close the dialog and open the choice on the host.
    fn submit(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let (folder, kind) = self.choice(cx);
        let destination = self.destination.clone();
        window.close_dialog(cx);
        let _ = self.workspace.update(cx, |workspace, cx| {
            workspace.open_in_folder(&destination, &folder, kind, window, cx)
        });
    }
}

impl Render for OpenInFolder {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let launches = launcher::remote_launches();
        let kinds: Vec<_> = launches.iter().map(|launch| launch.kind).collect();
        let buttons = launches.into_iter().enumerate().map(|(index, launch)| {
            Button::new(("open-in-folder-kind", index))
                .icon(sidebar::launcher_icon(launch.icon))
                .label(launch.label)
                .selected(launch.kind == self.kind)
        });
        div()
            .flex()
            .flex_col()
            .gap_3()
            .on_action(cx.listener(|_, _: &Escape, window, cx| window.close_dialog(cx)))
            .child(
                div()
                    .flex()
                    .flex_col()
                    .gap_1()
                    .child(
                        div()
                            .text_sm()
                            .text_color(cx.theme().muted_foreground)
                            .child("Folder on the host"),
                    )
                    .child(Input::new(&self.input)),
            )
            .child(
                ButtonGroup::new("open-in-folder-program")
                    .outline()
                    .small()
                    .children(buttons)
                    .on_click(cx.listener(move |form, clicked: &Vec<usize>, _, cx| {
                        if let Some(&kind) = clicked.first().and_then(|&index| kinds.get(index)) {
                            form.select(kind, cx);
                        }
                    })),
            )
    }
}
