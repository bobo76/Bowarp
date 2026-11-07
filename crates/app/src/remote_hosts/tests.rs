use workspace_model::{DEFAULT_SERVER_PROGRAM, RemoteHost};

use super::*;

#[cfg(unix)]
mod unix;

fn configured() -> workspace_model::RemoteHosts {
    workspace_model::RemoteHosts::new(vec![RemoteHost {
        name: "Build box".into(),
        destination: "me@box".into(),
        server_program: "~/.cargo/bin/agentws-server".into(),
    }])
}

#[test]
fn a_configured_host_is_found_by_destination_or_name_with_its_program() {
    let expected = Target {
        destination: "me@box".into(),
        program: "~/.cargo/bin/agentws-server".into(),
    };
    for host in ["me@box", "Build box"] {
        // The settings win over the environment variable.
        let target = resolve_target(&configured(), host, Some("/env/server".into()));
        assert_eq!(target, expected, "{host}");
    }
}

#[test]
fn an_unknown_host_is_the_destination_with_the_environment_or_default_program() {
    let target = resolve_target(&configured(), " other ", Some("/env/server".into()));
    assert_eq!(target.destination, "other");
    assert_eq!(target.program, "/env/server");
    let target = resolve_target(&configured(), "other", None);
    assert_eq!(target.program, DEFAULT_SERVER_PROGRAM);
}
