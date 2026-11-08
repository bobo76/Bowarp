use super::*;

fn host(name: &str, destination: &str) -> RemoteHost {
    RemoteHost {
        name: name.into(),
        destination: destination.into(),
        server_program: String::new(),
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
