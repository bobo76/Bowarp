use super::*;
use workspace_model::AgentId;
use workspace_model::agents::COPILOT;

fn test_hooks(server: HookServer, root: PathBuf, hook_program: PathBuf) -> AgentHooks {
    let claude = AgentId::from_cli_selector("claude").unwrap();
    AgentHooks {
        server,
        hook_program,
        artifacts: HashMap::from([
            (claude, root.join("claude-settings.json")),
            (COPILOT, root.join("copilot")),
        ]),
    }
}

fn server(name: &str) -> HookServer {
    HookServer::start(
        format!("agentws-{name}-{}.sock", agent_status::now_micros()),
        |_| {},
    )
    .unwrap()
}

fn agent_spec(selector: &str) -> LaunchSpec {
    LaunchSpec::agent(AgentId::from_cli_selector(selector).unwrap(), None)
}

#[test]
fn attachments_are_provider_driven_and_keep_hooks_before_user_arguments() {
    let root =
        std::env::temp_dir().join(format!("agentws-hooks-test-{}", agent_status::now_micros()));
    fs::create_dir(&root).unwrap();
    let hooks = test_hooks(
        server("attach-test"),
        root.clone(),
        PathBuf::from("agentws-hook"),
    );
    let mut copilot = agent_spec("copilot");
    copilot.args = vec!["--mode".into(), "plan".into()];
    assert_eq!(
        hooks.attach(12, &mut copilot).reporting,
        HookReporting::Supplemental
    );
    let plugin = root.join("copilot").join("12");
    assert_eq!(
        copilot.args,
        ["--plugin-dir", plugin.to_str().unwrap(), "--mode", "plan"]
    );
    assert!(plugin.join("plugin.json").is_file());
    assert!(plugin.join("hooks.json").is_file());
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        for path in [
            &root.join("copilot"),
            &plugin,
            &plugin.join("plugin.json"),
            &plugin.join("hooks.json"),
        ] {
            assert_eq!(fs::metadata(path).unwrap().permissions().mode() & 0o077, 0);
        }
    }
    assert!(!copilot.env.contains_key("COPILOT_HOME"));
    assert_eq!(copilot.env[SESSION_ENV], "12");
    assert_eq!(copilot.env[PIPE_ENV], hooks.server.pipe_name());
    assert_eq!(copilot.env["AGENTWS_HOOK"], "agentws-hook");

    // A shell without integration gets nothing; with it, the hooks without a launch flag.
    let mut shell = LaunchSpec::shell(None);
    assert_eq!(hooks.attach(13, &mut shell).reporting, HookReporting::None);
    assert!(shell.env.is_empty());
    shell.mark_token = Some("t0k".into());
    assert_eq!(hooks.attach(13, &mut shell).reporting, HookReporting::None);
    assert!(shell.args.is_empty());
    let shell_plugin = root.join("copilot").join("13");
    assert_eq!(shell.env[SESSION_ENV], "13");
    assert_eq!(shell.env[PIPE_ENV], hooks.server.pipe_name());
    assert_eq!(shell.env["AGENTWS_HOOK"], "agentws-hook");
    assert_eq!(
        shell.env["AGENTWS_CLAUDE_SETTINGS"],
        root.join("claude-settings.json").to_str().unwrap()
    );
    assert_eq!(
        shell.env["AGENTWS_COPILOT_PLUGIN"],
        shell_plugin.to_str().unwrap()
    );
    assert!(!shell.env.contains_key(AGENT_ENV));
    assert!(shell_plugin.join("hooks.json").is_file());
    hooks.remove_session(13);
    assert!(
        !shell_plugin.exists(),
        "closing a shell tab removes its plugin"
    );

    let mut claude = agent_spec("claude");
    claude.args = vec!["--settings".into(), "user.json".into()];
    assert_eq!(
        hooks.attach(14, &mut claude).reporting,
        HookReporting::FullActivity
    );
    assert_eq!(
        claude.args,
        [
            "--settings",
            root.join("claude-settings.json").to_str().unwrap(),
            "--settings",
            "user.json"
        ]
    );
    assert_eq!(claude.env[SESSION_ENV], "14");
    assert!(!claude.env.contains_key("AGENTWS_HOOK"));

    hooks.remove_session(12);
    assert!(!plugin.exists(), "closing a tab removes its plugin");
    hooks.remove_settings();
    assert!(!root.join("copilot").exists());
    assert!(!root.join("claude-settings.json").exists());
    fs::remove_dir_all(root).unwrap();
}

#[test]
fn failed_supplemental_attachment_reports_diagnostic_and_keeps_progress() {
    let hooks = test_hooks(
        server("fallback-test"),
        PathBuf::from("nonexistent-parent").join("plugins"),
        PathBuf::from("agentws-hook"),
    );
    let mut copilot = agent_spec("copilot");
    copilot.args = vec!["--yolo".into()];
    let attachment = hooks.attach(12, &mut copilot);
    assert_eq!(attachment.reporting, HookReporting::None);
    assert_eq!(attachment.diagnostics.len(), 1);
    assert!(attachment.diagnostics[0].contains("using progress"));
    assert_eq!(copilot.args, ["--yolo"]);
    assert_eq!(copilot.env["AGENTWS_HOOK"], "agentws-hook");
}

#[test]
fn sweeps_only_artifacts_owned_by_dead_processes() {
    let root = std::env::temp_dir().join(format!(
        "agentws-sweep-test-{}-{}",
        std::process::id(),
        agent_status::now_micros()
    ));
    fs::create_dir(&root).unwrap();
    for name in [
        "agentws-7-100-copilot",
        "agentws-8-200-copilot",
        "agentws-8-invalid-copilot",
        "agentws-0-200-copilot",
        "agentws-8-200-copilot-other",
        "unrelated",
    ] {
        fs::create_dir(root.join(name)).unwrap();
        fs::write(root.join(name).join("hooks.json"), "{}").unwrap();
    }
    let file = root.join("agentws-8-300-copilot");
    fs::write(&file, "not a directory").unwrap();
    fs::write(root.join("agentws-8-claude-settings.json"), "{}").unwrap();
    fs::write(root.join("agentws-7-claude-settings.json"), "{}").unwrap();
    fs::write(root.join("agentws-x-claude-settings.json"), "{}").unwrap();
    #[cfg(unix)]
    std::os::unix::fs::symlink(root.join("unrelated"), root.join("agentws-8-400-copilot")).unwrap();

    sweep_stale_files(&root, |pid| pid == 7).unwrap();
    assert!(!root.join("agentws-8-200-copilot").exists());
    for name in [
        "agentws-7-100-copilot",
        "agentws-8-invalid-copilot",
        "agentws-0-200-copilot",
        "agentws-8-200-copilot-other",
        "unrelated",
    ] {
        assert!(root.join(name).join("hooks.json").is_file(), "{name}");
    }
    assert!(file.is_file());
    assert!(!root.join("agentws-8-claude-settings.json").exists());
    assert!(root.join("agentws-7-claude-settings.json").is_file());
    assert!(root.join("agentws-x-claude-settings.json").is_file());
    #[cfg(unix)]
    assert!(root.join("agentws-8-400-copilot").is_symlink());
    fs::remove_dir_all(root).unwrap();
}

#[test]
fn start_failures_explain_the_cause_and_the_fix() {
    let missing = HooksError::HookProgramMissing(PathBuf::from("/opt/bowarp/agentws-hook"));
    assert_eq!(
        missing.warning(),
        "Agent status is off: agentws-hook not found next to the app. Reinstall to fix."
    );
    let missing_exe = HooksError::HookProgramMissing(PathBuf::from("/opt/bowarp/agentws-hook.exe"));
    assert_eq!(
        missing_exe.warning(),
        "Agent status is off: agentws-hook.exe not found next to the app. Reinstall to fix.",
        "the real file name, no folder"
    );
    let denied = || io::Error::from(io::ErrorKind::PermissionDenied);
    assert_eq!(
        HooksError::Server(denied()).warning(),
        format!(
            "Agent status is off: could not start the status listener ({}). Restart the app to \
             retry.",
            denied()
        )
    );
    let path = PathBuf::from("/run/user/1000/agentws-7-claude-settings.json");
    assert_eq!(
        HooksError::SettingsWrite {
            path: path.clone(),
            error: denied(),
        }
        .warning(),
        format!(
            "Agent status is off: could not write {} ({}).",
            path.display(),
            denied()
        )
    );
    assert_eq!(
        HooksError::Other(io::Error::other("no current exe")).warning(),
        "Agent status is off: no current exe."
    );
}
