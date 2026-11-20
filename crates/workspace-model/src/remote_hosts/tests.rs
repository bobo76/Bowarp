use super::*;
use crate::agents::CLAUDE;

fn host(name: &str, destination: &str) -> RemoteHost {
    RemoteHost {
        name: name.into(),
        destination: destination.into(),
        ..RemoteHost::default()
    }
}

#[test]
fn label_is_the_name_else_the_destination_without_the_user() {
    assert_eq!(host("Build box", "me@box").label(), "Build box");
    assert_eq!(host("  ", "me@box.lan").label(), "box.lan");
    assert_eq!(host("", "box").label(), "box");
}

#[test]
fn server_program_defaults_when_empty() {
    let mut box_host = host("", "box");
    assert_eq!(box_host.server_program(), DEFAULT_SERVER_PROGRAM);
    box_host.server_program = " ~/.cargo/bin/agentws-server ".into();
    assert_eq!(box_host.server_program(), "~/.cargo/bin/agentws-server");
}

#[test]
fn entries_of_the_wrong_shape_are_dropped_and_wrong_fields_are_empty() {
    let hosts: RemoteHosts = serde_json::from_str(
        r#"[{"name": "a", "destination": "me@a", "server_program": "/opt/s"},
            "not a host", 3,
            {"name": 7, "destination": "b"},
            {}]"#,
    )
    .unwrap();
    assert_eq!(
        hosts.all(),
        [
            RemoteHost {
                name: "a".into(),
                destination: "me@a".into(),
                server_program: "/opt/s".into(),
                ..RemoteHost::default()
            },
            host("", "b"),
            RemoteHost::default(),
        ]
    );
    let not_a_list: RemoteHosts = serde_json::from_str(r#"{"name": "a"}"#).unwrap();
    assert!(not_a_list.all().is_empty());
}

#[test]
fn hosts_are_written_back_as_objects() {
    let hosts = RemoteHosts::new(vec![host("a", "me@a")]);
    let json = serde_json::to_value(&hosts).unwrap();
    assert_eq!(
        json,
        serde_json::json!([{"name": "a", "destination": "me@a", "server_program": ""}])
    );
    let read: RemoteHosts = serde_json::from_value(json).unwrap();
    assert_eq!(read, hosts);
}

#[test]
fn hosts_without_a_destination_are_kept_but_not_usable() {
    let hosts = RemoteHosts::new(vec![host("draft", " "), host("", " me@box ")]);
    let usable: Vec<_> = hosts.usable().map(|(_, destination)| destination).collect();
    assert_eq!(usable, ["me@box"]);
    assert_eq!(hosts.all().len(), 2);
}

#[test]
fn find_matches_the_destination_before_the_name() {
    let hosts = RemoteHosts::new(vec![
        host("box", "me@other"),
        host("Box two", "box"),
        host("draft", ""),
    ]);
    assert_eq!(hosts.find("box").unwrap().name, "Box two");
    assert_eq!(hosts.find(" me@other ").unwrap().name, "box");
    assert_eq!(hosts.find("Box two").unwrap().destination, "box");
    // A host without a destination can't be used, even by name.
    assert_eq!(hosts.find("draft"), None);
    assert_eq!(hosts.find("elsewhere"), None);
}

#[test]
fn label_for_names_the_matching_host_else_the_given_host() {
    let hosts = RemoteHosts::new(vec![
        host("Build box", "me@build"),
        host("", "you@plain"),
        host("draft", ""),
    ]);
    assert_eq!(hosts.label_for("me@build"), "Build box");
    assert_eq!(hosts.label_for("Build box"), "Build box");
    // A host without a name shows its destination without the user.
    assert_eq!(hosts.label_for("you@plain"), "plain");
    // No usable host matches: the given destination, as given.
    assert_eq!(hosts.label_for(" me@elsewhere "), "me@elsewhere");
    assert_eq!(hosts.label_for("draft"), "draft");
    assert_eq!(RemoteHosts::default().label_for("box"), "box");
}

#[test]
fn editing_ignores_hosts_that_are_gone() {
    let mut hosts = RemoteHosts::default();
    hosts.add();
    hosts.set_field(0, RemoteHostField::Destination, "me@box".into());
    hosts.set_field(0, RemoteHostField::Name, "Box".into());
    hosts.set_field(5, RemoteHostField::Name, "nobody".into());
    assert_eq!(hosts.all(), [host("Box", "me@box")]);
    assert_eq!(hosts.all()[0].field(RemoteHostField::Name), "Box");
    hosts.remove(3);
    assert_eq!(hosts.all().len(), 1);
    hosts.remove(0);
    assert!(hosts.all().is_empty());
}

#[test]
fn launch_memory_is_read_and_written_only_when_set() {
    let hosts: RemoteHosts = serde_json::from_str(
        r#"[{"destination": "box", "last_folder": "/srv/app", "last_program": "claude"}]"#,
    )
    .unwrap();
    let read = &hosts.all()[0];
    assert_eq!(read.last_folder, "/srv/app");
    assert_eq!(read.last_kind(), SessionKind::Agent(CLAUDE));
    let json = serde_json::to_value(RemoteHosts::new(vec![host("a", "me@a")])).unwrap();
    assert_eq!(
        json,
        serde_json::json!([{"name": "a", "destination": "me@a", "server_program": ""}])
    );
}

#[test]
fn an_unknown_or_missing_last_program_is_a_shell() {
    let mut box_host = host("", "box");
    assert_eq!(box_host.last_kind(), SessionKind::Shell);
    box_host.last_program = "emacs".into();
    assert_eq!(box_host.last_kind(), SessionKind::Shell);
}

#[test]
fn same_host_matches_a_destination_with_its_hosts_name() {
    let hosts = RemoteHosts::new(vec![host("Build box", "me@build"), host("", "other")]);
    assert!(hosts.same_host("Build box", " me@build "));
    assert!(hosts.same_host("me@elsewhere", "me@elsewhere"));
    assert!(!hosts.same_host("me@build", "other"));
}

#[test]
fn a_launch_starts_in_the_selected_tabs_folder_only_on_its_host() {
    let hosts = RemoteHosts::new(vec![host("Build box", "me@build"), host("", "other")]);
    let on_build = Some(("Build box", Some("/srv/app")));
    assert_eq!(hosts.folder_on("me@build", on_build), Some("/srv/app"));
    assert_eq!(hosts.folder_on("other", on_build), None);
    // A local tab, or a remote one whose folder is unknown: the home folder.
    assert_eq!(hosts.folder_on("me@build", None), None);
    assert_eq!(hosts.folder_on("me@build", Some(("me@build", None))), None);
}

#[test]
fn open_in_folder_offers_the_tabs_folder_else_the_last_one_used() {
    let mut hosts = RemoteHosts::new(vec![host("Build box", "me@build"), host("", "other")]);
    assert_eq!(hosts.offered_folder("me@build", None), "");
    hosts.remember_launch(" me@build ", " /srv/last ", SessionKind::Agent(CLAUDE));
    assert_eq!(hosts.offered_folder("me@build", None), "/srv/last");
    assert_eq!(
        hosts.find("me@build").unwrap().last_kind(),
        SessionKind::Agent(CLAUDE)
    );
    let on_build = Some(("me@build", Some("/srv/tab")));
    assert_eq!(hosts.offered_folder("me@build", on_build), "/srv/tab");
    assert_eq!(hosts.offered_folder("other", on_build), "");
}

#[test]
fn remembering_a_launch_on_an_unknown_host_changes_nothing() {
    let mut hosts = RemoteHosts::new(vec![host("", "box")]);
    hosts.remember_launch("elsewhere", "/srv", SessionKind::Shell);
    assert_eq!(hosts.all(), [host("", "box")]);
    hosts.remember_launch("box", "", SessionKind::Shell);
    assert_eq!(hosts.all()[0].last_program, "shell");
    assert_eq!(hosts.all()[0].last_folder, "");
}
