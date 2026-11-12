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
    AnyElement, App, Axis, Div, InteractiveElement, IntoElement, ParentElement, SharedString,
    StatefulInteractiveElement, Styled, Window, div,
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
    let muted = cx.theme().muted_foreground;
    let mut column = div().flex().flex_col().gap_2().w_full();
    if let Some(target) = remote_hosts::host_target(&host) {
        column = column.child(server_state(index, target, cx));
    }
    for (field, caption, placeholder) in FIELDS {
        let input = text_field(
            host.field(field).to_owned(),
            &field_binding(index, field),
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
            settings_file::update(cx, |settings| settings.remote_hosts.remove(index));
        });
    column.child(div().child(remove)).into_any_element()
}

/// The host's server state (live connection, last check) and its Check button.
fn server_state(index: usize, target: remote_hosts::Target, cx: &App) -> Div {
    let check = remote_hosts::host_check(&target, cx);
    let line = status::status_line(
        remote_hosts::is_connected(&target.destination, cx),
        check.as_ref(),
    );
    let checking = check == Some(HostCheck::Checking);
    let button = Button::new(SharedString::from(format!("remote-host-{index}-check")))
        .icon(IconName::RefreshCw)
        .label("Check")
        .ghost()
        .xsmall()
        .disabled(checking)
        .on_click(move |_, _, cx| remote_hosts::check(target.clone(), cx));
    div()
        .flex()
        .items_center()
        .gap_2()
        .child(status_text(index, line, cx))
        .child(div().flex_none().child(button))
}

fn status_text(index: usize, line: StatusLine, cx: &App) -> AnyElement {
    let theme = cx.theme();
    let (icon, color) = match line.tone {
        StatusTone::Fine => (IconName::CircleCheck, theme.success),
        StatusTone::Neutral => (IconName::Info, theme.muted_foreground),
        StatusTone::Checking => (IconName::LoaderCircle, theme.muted_foreground),
        StatusTone::Problem => (IconName::TriangleAlert, theme.warning),
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

/// Edits field `field` of host `index`; nothing once that host is gone.
fn field_binding(index: usize, field: RemoteHostField) -> Binding<String> {
    let set = move |settings: &mut workspace_model::Settings, value: String| {
        settings.remote_hosts.set_field(index, field, value);
    };
    Binding {
        key: format!("remote-host-{index}-{field:?}").into(),
        apply: Rc::new(move |value, cx| settings_file::preview(cx, |s| set(s, value))),
        save: Rc::new(move |value, cx| settings_file::update(cx, |s| set(s, value))),
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
    use super::*;

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
