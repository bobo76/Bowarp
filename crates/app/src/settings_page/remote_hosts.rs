//! The Remote hosts page: the machines remote tabs run on (settings `remote_hosts`,
//! docs/remote.md), each with its name, ssh destination and server program, added and
//! removed here. Each host with a destination gets a submenu in the `+` menu and a line with
//! its server's state, checked on request (`remote_hosts::check`).

use std::rc::Rc;

use agentws_remote::status::{self, HostCheck, StatusLine, StatusTone};
use gpui_kit::component::button::{Button, ButtonVariants};
use gpui_kit::component::setting::{SettingField, SettingGroup, SettingItem};
use gpui_kit::component::tooltip::Tooltip;
use gpui_kit::component::{ActiveTheme, Disableable, Icon, IconName, Sizable};
use gpui_kit::prelude::FluentBuilder;
use gpui_kit::{
    AnyElement, App, Axis, Div, Entity, InteractiveElement, IntoElement, ParentElement,
    SharedString, StatefulInteractiveElement, Styled, Window, div,
};
use workspace_model::{RemoteHost, RemoteHostField};

use super::{Binding, text_field};
use crate::{remote_hosts, settings_file};

/// The fields of a host as the page shows them: field, caption, placeholder.
const FIELDS: [(RemoteHostField, &str, &str); 3] = [
    (RemoteHostField::Name, "Name", "Build box"),
    (
        RemoteHostField::Destination,
        "ssh destination",
        "me@build-box",
    ),
    (
        RemoteHostField::ServerProgram,
        "Server program",
        workspace_model::DEFAULT_SERVER_PROGRAM,
    ),
];

const KEYWORDS: [&str; 5] = ["remote", "ssh", "host", "server", "machine"];

pub fn remote_host_groups(cx: &App) -> Vec<SettingGroup> {
    let hosts = settings_file::settings(cx).remote_hosts.all().to_vec();
    let rows = hosts
        .iter()
        .enumerate()
        .map(|(index, host)| host_row(index, host));
    vec![
        SettingGroup::new()
            .title("Remote hosts")
            .description(
                "Machines whose shells and agents open in tabs here, through agentws-server \
                 over ssh (docs/remote.md). ssh must log in without asking: use a key or an \
                 agent. Each host with an ssh destination gets a submenu in the + menu.",
            )
            .items(rows)
            .item(add_row()),
    ]
}

/// A host's fields and its Remove button. Rows are keyed by position: after a removal the
/// fields below show their new host's values (`text_field` follows the setting).
fn host_row(index: usize, host: &RemoteHost) -> SettingItem {
    let title = match host.label() {
        "" => "New host".to_owned(),
        label => label.to_owned(),
    };
    SettingItem::new(
        title,
        SettingField::render(move |_, window, cx| host_fields(index, window, cx)),
    )
    .layout(Axis::Vertical)
    .description(host_description(host))
    .keywords(KEYWORDS)
}

/// What connecting to `host` runs, or what it still lacks.
fn host_description(host: &RemoteHost) -> String {
    match host.destination() {
        Some(destination) => format!(
            "Connects with: ssh {destination} {} proxy",
            host.server_program()
        ),
        None => "Needs an ssh destination ([user@]host, or a Host from your ssh config) to \
                 show in the + menu."
            .to_owned(),
    }
}

fn host_fields(index: usize, window: &mut Window, cx: &mut App) -> AnyElement {
    let Some(host) = settings_file::settings(cx)
        .remote_hosts
        .all()
        .get(index)
        .cloned()
    else {
        return div().into_any_element();
    };
    // The host as it is now, for the fields' bindings: an input keeps the binding of its first
    // render, and an edit must find the host as the other fields' edits left it.
    let shown = window.use_keyed_state(
        SharedString::from(format!("remote-host-{index}-shown")),
        cx,
        |_, _| host.clone(),
    );
    shown.update(cx, |shown, _| shown.clone_from(&host));
    let muted = cx.theme().muted_foreground;
    let mut column = div().flex().flex_col().gap_2().w_full();
    if let Some(target) = remote_hosts::host_target(&host) {
        column = column.child(server_state(index, target, cx));
    }
    for (field, caption, placeholder) in FIELDS {
        let input = text_field(
            host.field(field).to_owned(),
            &field_binding(index, &shown, field),
            placeholder.into(),
            window,
            cx,
        );
        column = column.child(
            div()
                .flex()
                .items_center()
                .gap_2()
                .child(div().w_32().text_sm().text_color(muted).child(caption))
                .child(input),
        );
    }
    // Ids carry the index: the field renders outside the row's id scope.
    let remove = Button::new(SharedString::from(format!("remote-host-{index}-remove")))
        .icon(IconName::Delete)
        .label("Remove host")
        .ghost()
        .xsmall()
        .on_click(move |_, _, cx| {
            settings_file::update(cx, |settings| settings.remote_hosts.remove(index, &host));
        });
    column.child(div().child(remove)).into_any_element()
}

/// The host's server state (live connection, last check) and its Check, Start and Stop
/// buttons. Start is off while the server is known to run, Stop while it is known not to.
fn server_state(index: usize, target: remote_hosts::Target, cx: &App) -> Div {
    let check = remote_hosts::host_check(&target, cx);
    let line = status::status_line(
        remote_hosts::is_connected(&target.destination, cx),
        check.as_ref(),
    );
    let busy = check.as_ref().is_some_and(HostCheck::is_busy);
    let running = check.as_ref().and_then(HostCheck::server_running);
    let button = |name: &str, icon: IconName, label: &'static str, disabled: bool| {
        Button::new(SharedString::from(format!("remote-host-{index}-{name}")))
            .icon(icon)
            .label(label)
            .ghost()
            .xsmall()
            .disabled(busy || disabled)
    };
    let checked = target.clone();
    let started = target.clone();
    let check = button("check", IconName::RefreshCw, "Check", false)
        .on_click(move |_, _, cx| remote_hosts::check(checked.clone(), cx));
    let start = button("start", IconName::Play, "Start", running == Some(true))
        .tooltip("Start the server there, running until stopped")
        .on_click(move |_, _, cx| remote_hosts::start_server(started.clone(), cx));
    let stop = button("stop", IconName::Square, "Stop", running == Some(false))
        .tooltip("Stop the server there; asks first while sessions run")
        .on_click(move |_, window, cx| remote_hosts::stop_server(target.clone(), window, cx));
    div()
        .flex()
        .items_center()
        .gap_2()
        .child(status_text(index, line, cx))
        .child(
            div()
                .flex_none()
                .flex()
                .gap_1()
                .child(check)
                .child(start)
                .child(stop),
        )
}

fn status_text(index: usize, line: StatusLine, cx: &App) -> AnyElement {
    let theme = cx.theme();
    let icon = remote_hosts::tone_icon(line.tone);
    let color = match line.tone {
        StatusTone::Fine => theme.success,
        StatusTone::Neutral | StatusTone::Checking => theme.muted_foreground,
        StatusTone::Problem => theme.warning,
    };
    let text = div()
        .id(SharedString::from(format!("remote-host-{index}-state")))
        .min_w_0()
        .flex_shrink(1.)
        .flex()
        .items_center()
        .gap_2()
        .text_sm()
        .child(Icon::new(icon).small().text_color(color).flex_none())
        .child(div().min_w_0().truncate().child(line.text))
        .when_some(line.hint, |text, hint| {
            text.child(
                div()
                    .flex_none()
                    .text_color(theme.muted_foreground)
                    .child(format!("· {hint}")),
            )
        });
    text.when_some(line.details, |text, details| {
        text.tooltip(move |window, cx| Tooltip::new(details.clone()).build(window, cx))
    })
    .into_any_element()
}

/// Edits field `field` of the host `shown` holds, shown at `index`; nothing once that host is
/// gone. `shown` is read at each edit: it follows the host's other fields as they are saved.
fn field_binding(
    index: usize,
    shown: &Entity<RemoteHost>,
    field: RemoteHostField,
) -> Binding<String> {
    let shown = shown.clone();
    let edit = Rc::new(move |value: String, cx: &App| {
        let host = shown.read(cx).clone();
        move |settings: &mut workspace_model::Settings| {
            settings.remote_hosts.set_field(index, &host, field, value);
        }
    });
    let preview = edit.clone();
    Binding {
        key: format!("remote-host-{index}-{field:?}").into(),
        apply: Rc::new(move |value, cx| {
            let change = preview(value, cx);
            settings_file::preview(cx, change);
        }),
        save: Rc::new(move |value, cx| {
            let change = edit(value, cx);
            settings_file::update(cx, change);
        }),
    }
}

fn add_row() -> SettingItem {
    SettingItem::new(
        "Add a host",
        SettingField::render(|_, _, _| {
            Button::new("remote-host-add")
                .icon(IconName::Plus)
                .label("Add host")
                .outline()
                .small()
                .on_click(|_, _, cx| {
                    settings_file::update(cx, |settings| settings.remote_hosts.add())
                })
        }),
    )
    .description("A new host, to fill in above.")
    .keywords(KEYWORDS)
}

#[cfg(test)]
mod tests {
    use gpui_kit::component::Root;
    use gpui_kit::component::input::InputState;
    use gpui_kit::{
        AppContext, Context, FocusHandle, Focusable, Render, TestAppContext, VisualTestContext,
    };

    use super::super::TextField;
    use super::*;

    struct HostPage {
        focus: FocusHandle,
        inputs: Vec<Entity<InputState>>,
    }

    impl Render for HostPage {
        fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
            let fields = host_fields(0, window, cx);
            self.inputs = FIELDS
                .iter()
                .map(|(field, _, _)| {
                    let key = SharedString::from(format!("remote-host-0-{field:?}-input"));
                    let state: Entity<TextField> = window.use_keyed_state(key, cx, |_, _| {
                        unreachable!("text_field created the state")
                    });
                    state.read(cx).input.clone()
                })
                .collect();
            div().track_focus(&self.focus).child(fields)
        }
    }

    /// Focus the input of `FIELDS[index]` (`None`: none), letting the one left save.
    fn focus(cx: &mut VisualTestContext, page: &Entity<HostPage>, index: Option<usize>) {
        cx.update(|window, cx| {
            window.draw(cx).clear(cx);
            let handle = match index {
                Some(index) => page.read(cx).inputs[index].focus_handle(cx),
                None => page.read(cx).focus.clone(),
            };
            window.focus(&handle, cx);
            window.draw(cx).clear(cx);
        });
        cx.run_until_parked();
    }

    #[gpui::test]
    fn editing_one_field_then_another_keeps_both_edits(cx: &mut TestAppContext) {
        cx.update(|cx| {
            gpui_kit::init(cx);
            settings_file::init_in_memory(cx);
            settings_file::update(cx, |settings| settings.remote_hosts.add());
        });
        let captured = std::cell::OnceCell::new();
        let window = cx.add_window(|window, cx| {
            let page = cx.new(|cx| {
                cx.observe_global::<settings_file::AppSettings>(|_, cx| cx.notify())
                    .detach();
                HostPage {
                    focus: cx.focus_handle(),
                    inputs: Vec::new(),
                }
            });
            captured.set(page.clone()).ok().unwrap();
            Root::new(page, window, cx)
        });
        let page = captured.into_inner().unwrap();
        let mut cx = VisualTestContext::from_window(window.into(), cx);
        cx.update(|window, _| window.activate_window());
        cx.run_until_parked();

        focus(&mut cx, &page, Some(1));
        cx.simulate_input("me@box");
        // Leaving Destination for Name saves the destination.
        focus(&mut cx, &page, Some(0));
        cx.simulate_input("Box");
        focus(&mut cx, &page, None);
        cx.update(|_, cx| {
            let hosts = settings_file::settings(cx).remote_hosts.all();
            assert_eq!(hosts.len(), 1);
            assert_eq!(
                (hosts[0].name.as_str(), hosts[0].destination.as_str()),
                ("Box", "me@box")
            );
        });
    }

    #[test]
    fn a_host_row_names_what_connecting_runs_or_what_it_lacks() {
        let mut host = RemoteHost::default();
        assert!(host_description(&host).starts_with("Needs an ssh destination"));
        host.destination = " me@box ".into();
        assert_eq!(
            host_description(&host),
            "Connects with: ssh me@box agentws-server proxy"
        );
        host.server_program = "~/bin/agentws-server".into();
        assert!(host_description(&host).ends_with("~/bin/agentws-server proxy"));
    }
}
