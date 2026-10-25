use super::*;
use crate::TabColor;
use crate::git::tests::TempDir;
use crate::layout::{SavedGroup, SavedItem};
use crate::window_state::temp_path;

fn tab(kind: SavedKind, folder: &str) -> SavedTab {
    SavedTab {
        kind,
        program: None,
        args: Vec::new(),
        folder: Some(folder.into()),
        last_command: None,
        meta: TabMeta::default(),
    }
}

fn sample() -> TabsState {
    let mut claude = tab(SavedKind::ClaudeCode, "/work/api");
    claude.program = Some("/usr/bin/claude".into());
    claude.meta.custom_name = Some("api".into());
    claude.meta.color = Some(TabColor::Pink);
    let mut wsl = tab(SavedKind::Shell, "/home/me");
    wsl.program = Some("wsl.exe".into());
    wsl.args = vec!["-d".into(), "Ubuntu".into()];
    TabsState {
        layout: SavedLayout {
            pinned: vec![claude],
            items: vec![
                SavedItem::Tab(tab(SavedKind::Shell, "/tmp")),
                SavedItem::Group(SavedGroup {
                    name: "Agents".into(),
                    color: Some(TabColor::Blue),
                    collapsed: true,
                    tabs: vec![tab(SavedKind::CopilotCli, "/work"), wsl],
                }),
            ],
        },
        selected: Some(2),
    }
}

fn with_commands() -> TabsState {
    let mut state = sample();
    state.layout.pinned[0].last_command = Some("cargo test".into());
    state.layout.items[0] = SavedItem::Tab({
        let mut plain = tab(SavedKind::Shell, "/tmp");
        plain.last_command = Some("make".into());
        plain
    });
    if let SavedItem::Group(group) = &mut state.layout.items[1] {
        group.tabs[0].last_command = Some("cargo test".into());
        group.tabs[1].last_command = Some("ls -la".into());
    }
    state
}

fn commands(state: &TabsState) -> Vec<Option<&str>> {
    state
        .layout
        .tabs()
        .into_iter()
        .map(|tab| tab.last_command.as_deref())
        .collect()
}

#[test]
fn scrub_all_removes_only_last_commands() {
    let mut state = with_commands();
    assert!(state.scrub_last_commands(None));
    assert_eq!(commands(&state), [None; 4]);
    assert_eq!(state, sample());
    assert!(!state.scrub_last_commands(None));
}

#[test]
fn scrub_one_removes_every_tab_holding_exactly_that_command() {
    let mut state = with_commands();
    assert!(state.scrub_last_commands(Some("cargo test")));
    assert_eq!(commands(&state), [None, Some("make"), None, Some("ls -la")]);
    assert!(!state.scrub_last_commands(Some("cargo")));
}

#[test]
fn scrub_file_rewrites_only_when_a_command_goes() {
    let dir = TempDir::new();
    let path = dir.0.join("tabs.json");
    with_commands().save(&path).unwrap();
    TabsState::scrub_file(&path, Some("nothing")).unwrap();
    assert_eq!(TabsState::load(&path), Some(with_commands()));
    TabsState::scrub_file(&path, None).unwrap();
    assert_eq!(TabsState::load(&path), Some(sample()));
}

#[test]
fn scrub_file_missing_is_a_no_op_and_unreadable_is_an_error() {
    let dir = TempDir::new();
    let missing = dir.0.join("tabs.json");
    TabsState::scrub_file(&missing, None).unwrap();
    assert!(!missing.exists());
    std::fs::write(&missing, "{ not json").unwrap();
    assert!(TabsState::scrub_file(&missing, None).is_err());
    assert_eq!(std::fs::read_to_string(&missing).unwrap(), "{ not json");
    // A directory where the file should be cannot be read.
    let unreadable = dir.0.join("dir");
    std::fs::create_dir(&unreadable).unwrap();
    assert!(TabsState::scrub_file(&unreadable, None).is_err());
}

#[test]
fn json_round_trip() {
    let state = sample();
    assert_eq!(TabsState::from_json(&state.to_json()), Some(state));
}

#[test]
fn json_is_readable_and_leaves_out_unset_fields() {
    let json = sample().to_json();
    assert!(json.contains(r#""kind": "claude-code""#));
    assert!(json.contains(r#""custom_name": "api""#));
    assert!(json.contains(r#""color": "pink""#));
    assert!(json.contains(r#""tab": {"#));
    let plain = TabsState {
        layout: SavedLayout {
            pinned: Vec::new(),
            items: vec![SavedItem::Tab(tab(SavedKind::Shell, "/tmp"))],
        },
        selected: None,
    };
    let json = plain.to_json();
    for field in ["program", "args", "custom_name", "color", "selected"] {
        assert!(!json.contains(field), "{field} in {json}");
    }
}

#[test]
fn missing_fields_take_defaults() {
    let state =
        TabsState::from_json(r#"{"layout": {"items": [{"tab": {"kind": "shell"}}]}}"#).unwrap();
    let tabs = state.layout.tabs();
    assert_eq!(tabs.len(), 1);
    assert_eq!(tabs[0].kind, SavedKind::Shell);
    assert_eq!(tabs[0].program, None);
    assert_eq!(tabs[0].folder, None);
    assert_eq!(tabs[0].last_command, None);
    assert_eq!(tabs[0].meta, TabMeta::default());
    assert_eq!(state.selected, None);
    assert_eq!(TabsState::from_json("{}"), Some(TabsState::default()));
}

#[test]
fn damaged_files_are_rejected() {
    for text in [
        "",
        "not json",
        r#"{"layout": {"items": [{"tab": {"kind": "vim"}}]}}"#,
        r#"{"layout": {"items": [{"tab": {"kind": "shell", "color": "teal"}}]}}"#,
        r#"{"layout": {"pinned": [{"kind": "shell"}], "items": "x"}}"#,
    ] {
        assert_eq!(TabsState::from_json(text), None, "{text}");
    }
}

#[test]
fn last_command_round_trip() {
    let mut state = sample();
    state.layout.pinned[0].last_command = Some("cargo test".into());
    assert_eq!(TabsState::from_json(&state.to_json()), Some(state));
}

#[cfg(unix)]
#[test]
fn every_save_replaces_public_permissions_with_owner_only() {
    use std::os::unix::fs::PermissionsExt;

    let root = TempDir::new();
    let path = root.0.join("tabs.json");
    fs::write(&path, "old").unwrap();
    fs::set_permissions(&path, fs::Permissions::from_mode(0o644)).unwrap();
    for _ in 0..2 {
        sample().save(&path).unwrap();
        assert_eq!(
            fs::metadata(&path).unwrap().permissions().mode() & 0o777,
            0o600
        );
    }
}

#[test]
fn launch_folder_falls_back_to_the_nearest_existing_parent() {
    let root = TempDir::new();
    let existing = root.0.join("project");
    fs::create_dir_all(&existing).unwrap();
    let mut saved = tab(SavedKind::Shell, "/");
    saved.folder = Some(existing.clone());
    assert_eq!(saved.launch_folder(), Some(existing.clone()));
    saved.folder = Some(existing.join("deleted").join("deeper"));
    assert_eq!(saved.launch_folder(), Some(existing));
    saved.folder = Some("relative-folder-that-does-not-exist".into());
    assert_eq!(saved.launch_folder(), None);
    saved.folder = None;
    assert_eq!(saved.launch_folder(), None);
}

#[test]
fn launch_folder_reads_past_the_deleted_marker() {
    let root = TempDir::new();
    let recreated = root.0.join("project");
    fs::create_dir_all(&recreated).unwrap();
    let mut saved = tab(SavedKind::Shell, "/");
    saved.folder = Some(root.0.join("project (deleted)"));
    assert_eq!(saved.launch_folder(), Some(recreated));
    // A folder really named so is kept.
    let named = root.0.join("old (deleted)");
    fs::create_dir_all(&named).unwrap();
    saved.folder = Some(named.clone());
    assert_eq!(saved.launch_folder(), Some(named));
}

#[test]
fn save_then_load() {
    let root = TempDir::new();
    let path = root.0.join("nested").join("tabs.json");
    let state = sample();
    state.save(&path).unwrap();
    assert_eq!(TabsState::load(&path), Some(state));
    assert!(!temp_path(&path).exists());
    fs::write(&path, "{").unwrap();
    assert_eq!(TabsState::load(&path), None);
    assert_eq!(TabsState::load(&root.0.join("missing.json")), None);
}
