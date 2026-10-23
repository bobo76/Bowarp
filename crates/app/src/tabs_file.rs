//! Reopening the tabs: what each tab runs, its folder, name, color and place in the sidebar are
//! saved to `tabs.json` when the window closes or a last command changes, and reopened at the next
//! start (`workspace_model::TabsState`). Sessions start fresh; a Claude Code tab starts a new
//! conversation in its folder.

use std::path::PathBuf;

use term_core::{LaunchSpec, SessionKind};
use workspace_model::{SavedKind, SavedTab, TabMeta, TabsState};

use crate::window_bounds::config_file;

fn state_path() -> Option<PathBuf> {
    config_file("tabs.json")
}

/// The tabs saved at the last close; `None` when nothing usable was saved.
pub fn load() -> Option<TabsState> {
    TabsState::load(&state_path()?)
}

/// Save the tabs for the next start. A failure is only reported: it costs the user nothing but
/// reopening their tabs by hand.
pub fn save(state: &TabsState) {
    let Some(path) = state_path() else {
        return;
    };
    if let Err(err) = state.save(&path) {
        eprintln!(
            "agent-workspace: failed to save tabs to {}: {err}",
            path.display()
        );
    }
}

/// The saved form of a tab started from `spec`, now in `folder`. JSON holds only UTF-8 paths:
/// a folder that isn't one is replaced by the launch folder, else left out (the tab then
/// reopens in the app's folder).
pub fn saved_tab(spec: &LaunchSpec, folder: Option<PathBuf>, meta: TabMeta) -> SavedTab {
    let utf8 = |folder: &PathBuf| folder.to_str().is_some();
    SavedTab {
        kind: saved_kind(spec.kind),
        program: spec.program.clone(),
        args: spec.args.clone(),
        folder: folder
            .filter(utf8)
            .or_else(|| spec.cwd.clone().filter(utf8)),
        last_command: None,
        meta,
    }
}

/// The launch reopening a saved tab, in its folder or the nearest parent that still exists.
pub fn launch_spec(tab: &SavedTab) -> LaunchSpec {
    LaunchSpec::new(
        session_kind(tab.kind),
        tab.program.clone(),
        tab.args.clone(),
        tab.launch_folder(),
    )
}

fn saved_kind(kind: SessionKind) -> SavedKind {
    match kind {
        SessionKind::Shell => SavedKind::Shell,
        SessionKind::ClaudeCode => SavedKind::ClaudeCode,
        SessionKind::CopilotCli => SavedKind::CopilotCli,
    }
}

fn session_kind(kind: SavedKind) -> SessionKind {
    match kind {
        SavedKind::Shell => SessionKind::Shell,
        SavedKind::ClaudeCode => SessionKind::ClaudeCode,
        SavedKind::CopilotCli => SessionKind::CopilotCli,
    }
}

#[cfg(test)]
mod tests {
    use workspace_model::TabColor;

    use super::*;

    #[test]
    fn saved_tab_round_trip_keeps_the_launch() {
        let folder = std::env::temp_dir();
        let meta = TabMeta {
            custom_name: Some("api".into()),
            color: Some(TabColor::Green),
        };
        for spec in [
            LaunchSpec::shell(Some("/".into())),
            LaunchSpec::claude_code(None),
            LaunchSpec::copilot_cli(None),
            LaunchSpec::new(
                SessionKind::Shell,
                Some("wsl.exe".into()),
                vec!["-d".into(), "Ubuntu".into()],
                None,
            ),
        ] {
            let saved = saved_tab(&spec, Some(folder.clone()), meta.clone());
            assert_eq!(saved.meta, meta);
            let reopened = launch_spec(&saved);
            assert_eq!(reopened.kind, spec.kind);
            assert_eq!(reopened.program, spec.program);
            assert_eq!(reopened.args, spec.args);
            assert_eq!(reopened.cwd.as_ref(), Some(&folder));
        }
    }

    #[test]
    fn saved_tab_falls_back_to_the_launch_folder() {
        let spec = LaunchSpec::shell(Some("/".into()));
        let saved = saved_tab(&spec, None, TabMeta::default());
        assert_eq!(saved.folder, Some("/".into()));
    }

    #[test]
    fn restored_agent_keeps_original_arguments_not_new_launcher_settings() {
        use crate::launcher::Launcher;
        use workspace_model::{LauncherIcon, Settings};
        let launcher = Launcher {
            id: "claude".into(),
            label: "Claude Code".into(),
            kind: SessionKind::ClaudeCode,
            program: Some("claude".into()),
            args: vec![],
            icon: LauncherIcon::Claude,
        };
        let mut settings = Settings {
            claude_args: "--model 'opus' --settings 'user settings.json'".into(),
            ..Settings::default()
        };
        let spec = launcher.configured_spec(None, &settings).unwrap().unwrap();
        let saved = saved_tab(&spec, None, TabMeta::default());
        let state = TabsState {
            layout: workspace_model::SavedLayout {
                pinned: vec![saved],
                items: vec![],
            },
            selected: Some(0),
        };
        let state = TabsState::from_json(&state.to_json()).unwrap();
        let saved = &state.layout.pinned[0];
        settings.claude_args = "--model sonnet".into();
        assert_eq!(
            launch_spec(saved).args,
            ["--model", "opus", "--settings", "user settings.json"]
        );
        assert_eq!(
            launcher
                .configured_spec(None, &settings)
                .unwrap()
                .unwrap()
                .args,
            ["--model", "sonnet"]
        );
    }

    #[cfg(unix)]
    #[test]
    fn non_utf8_folders_are_not_saved() {
        use std::ffi::OsStr;
        use std::os::unix::ffi::OsStrExt;

        use workspace_model::SavedLayout;

        let bad = PathBuf::from(OsStr::from_bytes(b"/tmp/\xff"));
        let current = saved_tab(
            &LaunchSpec::shell(Some("/".into())),
            Some(bad.clone()),
            TabMeta::default(),
        );
        assert_eq!(current.folder, Some("/".into()));
        let both = saved_tab(
            &LaunchSpec::shell(Some(bad.clone())),
            Some(bad),
            TabMeta::default(),
        );
        assert_eq!(both.folder, None);
        let state = TabsState {
            layout: SavedLayout {
                pinned: vec![current, both],
                items: Vec::new(),
            },
            selected: Some(0),
        };
        assert_eq!(TabsState::from_json(&state.to_json()), Some(state));
    }
}
