use super::*;

#[test]
fn copilot_gets_a_per_tab_permission_plugin_before_user_arguments() {
    let server = HookServer::start(
        format!("agentws-attach-test-{}.sock", agent_status::now_micros()),
        |_| {},
    )
    .unwrap();
    let copilot_plugins_dir =
        std::env::temp_dir().join(format!("agentws-hooks-test-{}", agent_status::now_micros()));
    let hooks = ClaudeHooks {
        server,
        settings_path: copilot_plugins_dir.join("claude-settings.json"),
        hook_program: PathBuf::from("agentws-hook"),
        copilot_plugins_dir,
    };
    let mut copilot = LaunchSpec::copilot_cli(None);
    copilot.args = vec!["--mode".into(), "plan".into()];
    assert!(!hooks.attach(12, &mut copilot));
    let plugin = hooks.copilot_plugins_dir.join("12");
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
            &hooks.copilot_plugins_dir,
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
    assert_eq!(copilot.env[HOOK_ENV], "agentws-hook");
    let mut other = LaunchSpec::copilot_cli(None);
    assert!(!hooks.attach(15, &mut other));
    assert_ne!(other.args[1], copilot.args[1]);
    assert_eq!(other.env[SESSION_ENV], "15");

    let mut shell = LaunchSpec::shell(None);
    assert!(!hooks.attach(13, &mut shell));
    assert!(shell.env.is_empty());

    let mut claude = LaunchSpec::claude_code(None);
    claude.args = vec!["--settings".into(), "user.json".into()];
    assert!(hooks.attach(14, &mut claude));
    assert_eq!(
        claude.args,
        [
            "--settings",
            hooks.settings_path.to_str().unwrap(),
            "--settings",
            "user.json"
        ]
    );
    assert_eq!(claude.env[SESSION_ENV], "14");
    assert!(!claude.env.contains_key(HOOK_ENV));
    hooks.remove_session(12);
    assert!(!plugin.exists(), "closing a tab removes its plugin");
    assert!(
        hooks
            .copilot_plugins_dir
            .join("15")
            .join("hooks.json")
            .is_file()
    );
    hooks.remove_session(12);
    let partial = hooks.copilot_plugins_dir.join("16");
    create_private_dir(&partial).unwrap();
    fs::create_dir(partial.join("hooks.json")).unwrap();
    let mut failed = LaunchSpec::copilot_cli(None);
    assert!(!hooks.attach(16, &mut failed));
    assert!(failed.args.is_empty());
    assert!(
        !partial.exists(),
        "failed creation removes the partial plugin"
    );
    assert!(hooks.copilot_plugins_dir.join("15").exists());
    hooks.remove_settings();
    assert!(!hooks.copilot_plugins_dir.exists());
}

#[test]
fn sweeps_only_files_owned_by_dead_processes() {
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
    // Claude settings files of crashed runs go too; a live owner's stays.
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
fn unavailable_copilot_plugin_keeps_progress_and_the_mode_bridge() {
    let server = HookServer::start(
        format!("agentws-fallback-test-{}.sock", agent_status::now_micros()),
        |_| {},
    )
    .unwrap();
    let hooks = ClaudeHooks {
        server,
        settings_path: PathBuf::from("settings.json"),
        hook_program: PathBuf::from("agentws-hook"),
        copilot_plugins_dir: PathBuf::from("nonexistent-parent").join("plugins"),
    };
    let mut copilot = LaunchSpec::copilot_cli(None);
    copilot.args = vec!["--yolo".into()];
    assert!(!hooks.attach(12, &mut copilot));
    assert_eq!(copilot.args, ["--yolo"]);
    assert_eq!(copilot.env[HOOK_ENV], "agentws-hook");
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
