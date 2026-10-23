use super::*;
use crate::Platform;
use crate::git::tests::TempDir;
use crate::window_state::temp_path;

fn with_extra(key: &str, value: Value) -> Settings {
    let mut settings = Settings::default();
    settings.extra.insert(key.into(), value);
    settings
}

#[test]
fn empty_object_gives_the_defaults() {
    assert_eq!(Settings::from_json("{}").unwrap(), Settings::default());
}

#[test]
fn tab_card_defaults_each_missing_or_wrong_typed_field_independently() {
    assert_eq!(
        Settings::from_json(r#"{"tab_card":{"changes":false}}"#)
            .unwrap()
            .tab_card,
        TabCardFields {
            changes: false,
            ..TabCardFields::default()
        },
    );
    for value in ["null", "false", "[]", "42", r#""off""#] {
        let settings = Settings::from_json(&format!(r#"{{"tab_card": {value}}}"#)).unwrap();
        assert_eq!(settings.tab_card, TabCardFields::default());
    }
    let settings = Settings::from_json(
        r#"{"tab_card":{"enabled":false,"status":"false","location":false,
        "changes":null,"program_title":42,"session_kind":[]},
        "claude_args":"--model 'opus'","copilot_args":false}"#,
    )
    .unwrap();
    assert_eq!(
        settings.tab_card,
        TabCardFields {
            enabled: false,
            location: false,
            ..TabCardFields::default()
        }
    );
    assert_eq!(settings.claude_args, "--model 'opus'");
    assert_eq!(settings.copilot_args, "");
    assert_eq!(
        Settings::from_json(&settings.to_json().unwrap()).unwrap(),
        settings
    );
}

#[test]
fn tab_card_set_keeps_only_checked_pieces_without_computing_the_rest() {
    let fields = [
        TabCardField::Status,
        TabCardField::Location,
        TabCardField::Changes,
        TabCardField::ProgramTitle,
        TabCardField::SessionKind,
    ];
    let none = TabCardFields {
        status: false,
        location: false,
        changes: false,
        program_title: false,
        session_kind: false,
        ..TabCardFields::default()
    };
    assert!(!none.active());
    for (index, selected) in fields.iter().enumerate() {
        let mut one = none;
        match selected {
            TabCardField::Status => one.status = true,
            TabCardField::Location => one.location = true,
            TabCardField::Changes => one.changes = true,
            TabCardField::ProgramTitle => one.program_title = true,
            TabCardField::SessionKind => one.session_kind = true,
        }
        assert!(one.active());
        for (other, field) in fields.iter().enumerate() {
            assert_eq!(one.keep(*field, || 7), (index == other).then_some(7));
        }
        one.enabled = false;
        assert!(!one.active());
        assert_eq!(
            one.keep(*selected, || panic!("disabled piece was computed")),
            None::<()>
        );
    }
}

#[test]
fn unknown_fields_survive_a_round_trip() {
    let text = r#"{"future": {"nested": [1, 2]}, "fontSize": 14}"#;
    let settings = Settings::from_json(text).unwrap();
    assert_eq!(settings.extra.len(), 2);
    let again = Settings::from_json(&settings.to_json().unwrap()).unwrap();
    assert_eq!(again, settings);
    assert_eq!(again.extra["fontSize"], Value::from(14));
}

#[test]
fn not_an_object_is_broken() {
    for text in ["", "{", "[]", "3", "null", "{\"a\": }"] {
        assert!(Settings::from_json(text).is_err(), "{text:?}");
    }
}

#[test]
fn read_tells_missing_loaded_and_broken_apart() {
    let root = TempDir::new();
    let path = root.0.join("settings.json");
    assert_eq!(SettingsLoad::read(&path), SettingsLoad::Missing);
    fs::write(&path, r#"{"a": true}"#).unwrap();
    assert_eq!(
        SettingsLoad::read(&path),
        SettingsLoad::Loaded(with_extra("a", Value::Bool(true)))
    );
    fs::write(&path, "{").unwrap();
    assert!(matches!(SettingsLoad::read(&path), SettingsLoad::Broken(_)));
    // A folder in the file's place can't be read.
    fs::remove_file(&path).unwrap();
    fs::create_dir(&path).unwrap();
    assert!(matches!(SettingsLoad::read(&path), SettingsLoad::Broken(_)));
}

#[test]
fn backup_path_appends_bak() {
    assert_eq!(
        settings_backup_path(Path::new("/c/settings.json")),
        Path::new("/c/settings.json.bak")
    );
}

#[test]
fn missing_file_loads_defaults_and_save_creates_it() {
    let root = TempDir::new();
    let path = root.0.join("nested").join("settings.json");
    let mut file = SettingsFile::load(path.clone());
    assert_eq!(file.load_error(), None);
    assert_eq!(file.settings(), &Settings::default());
    assert!(!path.exists());
    file.save().unwrap();
    assert!(!temp_path(&path).exists());
    assert_eq!(
        SettingsLoad::read(&path),
        SettingsLoad::Loaded(Settings::default())
    );
}

#[test]
fn create_if_missing_writes_only_a_missing_file() {
    let root = TempDir::new();
    let path = root.0.join("settings.json");
    let mut file = SettingsFile::load(path.clone());
    assert!(file.create_if_missing().unwrap());
    assert_eq!(
        SettingsLoad::read(&path),
        SettingsLoad::Loaded(Settings::default())
    );
    // An existing file keeps its text, broken or not.
    fs::write(&path, r#"{"a": 1}"#).unwrap();
    assert!(!file.create_if_missing().unwrap());
    assert_eq!(fs::read_to_string(&path).unwrap(), r#"{"a": 1}"#);
    fs::write(&path, "{ broken").unwrap();
    assert!(!file.create_if_missing().unwrap());
    assert_eq!(fs::read_to_string(&path).unwrap(), "{ broken");
    assert!(!settings_backup_path(&path).exists());
    assert!(!SettingsFile::default().create_if_missing().unwrap());
}

#[test]
fn update_saves_only_a_change() {
    let root = TempDir::new();
    let path = root.0.join("settings.json");
    let mut file = SettingsFile::load(path.clone());
    assert!(!file.update(|_| {}).unwrap());
    assert!(!path.exists());
    let changed = file
        .update(|settings| {
            settings.extra.insert("a".into(), Value::from(1));
        })
        .unwrap();
    assert!(changed);
    let reloaded = SettingsFile::load(path);
    assert_eq!(reloaded.load_error(), None);
    assert_eq!(reloaded.settings(), &with_extra("a", Value::from(1)));
}

#[test]
fn broken_file_is_kept_until_the_first_save_moves_it_aside() {
    let root = TempDir::new();
    let path = root.0.join("settings.json");
    let backup = settings_backup_path(&path);
    fs::write(&backup, "older backup").unwrap();
    fs::write(&path, "{ broken").unwrap();
    let mut file = SettingsFile::load(path.clone());
    assert!(file.load_error().is_some());
    assert_eq!(file.settings(), &Settings::default());
    assert_eq!(fs::read_to_string(&path).unwrap(), "{ broken");
    // An update that changes nothing doesn't touch the file either.
    assert!(!file.update(|_| {}).unwrap());
    assert_eq!(fs::read_to_string(&path).unwrap(), "{ broken");

    file.save().unwrap();
    assert_eq!(file.load_error(), None);
    assert_eq!(fs::read_to_string(&backup).unwrap(), "{ broken");
    assert_eq!(
        SettingsLoad::read(&path),
        SettingsLoad::Loaded(Settings::default())
    );
    // Later saves keep the backup.
    file.update(|settings| {
        settings.extra.insert("a".into(), Value::from(1));
    })
    .unwrap();
    assert_eq!(fs::read_to_string(&backup).unwrap(), "{ broken");
}

#[test]
fn update_unless_broken_leaves_a_broken_file_alone() {
    let root = TempDir::new();
    let path = root.0.join("settings.json");
    fs::write(&path, "{ broken").unwrap();
    let mut file = SettingsFile::load(path.clone());
    let narrow = ViewerFraction::new(0.3);
    let changed = file
        .update_unless_broken(|settings| settings.file_viewer_fraction = narrow)
        .unwrap();
    assert!(!changed);
    assert_eq!(file.settings().file_viewer_fraction, narrow);
    assert!(file.load_error().is_some());
    assert_eq!(fs::read_to_string(&path).unwrap(), "{ broken");
    assert!(!settings_backup_path(&path).exists());

    // Once the file is fixed, it saves like `update`.
    fs::write(&path, "{}").unwrap();
    assert!(
        file.update_unless_broken(|settings| settings.file_viewer_fraction = narrow)
            .unwrap()
    );
    assert_eq!(
        SettingsLoad::read(&path),
        SettingsLoad::Loaded(file.settings().clone())
    );
    assert_eq!(file.settings().file_viewer_fraction, narrow);
}

#[test]
fn broken_file_removed_meanwhile_still_saves() {
    let root = TempDir::new();
    let path = root.0.join("settings.json");
    fs::write(&path, "[]").unwrap();
    let mut file = SettingsFile::load(path.clone());
    assert!(file.load_error().is_some());
    fs::remove_file(&path).unwrap();
    file.save().unwrap();
    assert!(!settings_backup_path(&path).exists());
    assert!(path.exists());
}

#[test]
fn without_a_path_saving_does_nothing() {
    let mut file = SettingsFile::default();
    assert_eq!(file.path(), None);
    assert!(
        file.update(|settings| {
            settings.extra.insert("a".into(), Value::from(1));
        })
        .unwrap()
    );
    assert_eq!(file.settings(), &with_extra("a", Value::from(1)));
}

#[test]
fn set_unsaved_changes_memory_only_until_the_update_saves() {
    let root = TempDir::new();
    let path = root.0.join("settings.json");
    fs::write(&path, r#"{"a": 1}"#).unwrap();
    let mut file = SettingsFile::load(path.clone());
    file.set_unsaved(|settings| {
        settings.extra.insert("a".into(), Value::from(2));
    });
    file.set_unsaved(|settings| {
        settings.extra.insert("a".into(), Value::from(3));
    });
    assert_eq!(file.settings(), &with_extra("a", Value::from(3)));
    assert_eq!(fs::read_to_string(&path).unwrap(), r#"{"a": 1}"#);
    // Edited on disk meanwhile: the save at the end of the gesture keeps that edit.
    fs::write(&path, r#"{"a": 1, "b": 2}"#).unwrap();
    assert!(
        file.update(|settings| {
            settings.extra.insert("a".into(), Value::from(3));
        })
        .unwrap()
    );
    let mut expected = with_extra("a", Value::from(3));
    expected.extra.insert("b".into(), Value::from(2));
    assert_eq!(file.settings(), &expected);
    assert_eq!(SettingsLoad::read(&path), SettingsLoad::Loaded(expected));
}

#[test]
fn reload_drops_unsaved_changes_and_clears_a_fixed_load_error() {
    let root = TempDir::new();
    let path = root.0.join("settings.json");
    fs::write(&path, "{ broken").unwrap();
    let mut file = SettingsFile::load(path.clone());
    assert!(file.load_error().is_some());
    file.set_unsaved(|settings| {
        settings.extra.insert("a".into(), Value::from(2));
    });
    // Fixed by hand (in the file viewer).
    fs::write(&path, r#"{"b": 1}"#).unwrap();
    file.reload();
    assert_eq!(file.load_error(), None);
    assert_eq!(file.settings(), &with_extra("b", Value::from(1)));
}

#[test]
fn update_keeps_changes_made_on_disk_since_the_load() {
    let root = TempDir::new();
    let path = root.0.join("settings.json");
    fs::write(&path, r#"{"a": 1}"#).unwrap();
    let mut file = SettingsFile::load(path.clone());
    // By hand, or by another instance.
    fs::write(&path, r#"{"a": 1, "b": 2}"#).unwrap();
    file.update(|settings| {
        settings.extra.insert("c".into(), Value::from(3));
    })
    .unwrap();
    let mut expected = with_extra("a", Value::from(1));
    expected.extra.insert("b".into(), Value::from(2));
    expected.extra.insert("c".into(), Value::from(3));
    assert_eq!(file.settings(), &expected);
    assert_eq!(SettingsLoad::read(&path), SettingsLoad::Loaded(expected));
}

#[test]
fn file_fixed_by_hand_before_the_update_is_not_moved_aside() {
    let root = TempDir::new();
    let path = root.0.join("settings.json");
    fs::write(&path, "{ broken").unwrap();
    let mut file = SettingsFile::load(path.clone());
    assert!(file.load_error().is_some());
    fs::write(&path, r#"{"a": 1}"#).unwrap();
    file.update(|settings| {
        settings.extra.insert("b".into(), Value::from(2));
    })
    .unwrap();
    assert_eq!(file.load_error(), None);
    assert!(!settings_backup_path(&path).exists());
    let mut expected = with_extra("a", Value::from(1));
    expected.extra.insert("b".into(), Value::from(2));
    assert_eq!(SettingsLoad::read(&path), SettingsLoad::Loaded(expected));
}

#[test]
fn broken_file_saved_by_another_instance_keeps_its_backup() {
    let root = TempDir::new();
    let path = root.0.join("settings.json");
    let backup = settings_backup_path(&path);
    fs::write(&path, "{ broken").unwrap();
    let mut first = SettingsFile::load(path.clone());
    let mut second = SettingsFile::load(path.clone());
    first
        .update(|settings| {
            settings.extra.insert("a".into(), Value::from(1));
        })
        .unwrap();
    assert_eq!(fs::read_to_string(&backup).unwrap(), "{ broken");
    second
        .update(|settings| {
            settings.extra.insert("b".into(), Value::from(2));
        })
        .unwrap();
    assert_eq!(fs::read_to_string(&backup).unwrap(), "{ broken");
    let mut expected = with_extra("a", Value::from(1));
    expected.extra.insert("b".into(), Value::from(2));
    assert_eq!(SettingsLoad::read(&path), SettingsLoad::Loaded(expected));
}

#[test]
fn file_broken_after_the_load_is_moved_aside_at_the_next_change() {
    let root = TempDir::new();
    let path = root.0.join("settings.json");
    fs::write(&path, r#"{"a": 1}"#).unwrap();
    let mut file = SettingsFile::load(path.clone());
    fs::write(&path, "{ oops").unwrap();
    // A change that does nothing reports the broken file but leaves it.
    assert!(!file.update(|_| {}).unwrap());
    assert!(file.load_error().is_some());
    assert_eq!(fs::read_to_string(&path).unwrap(), "{ oops");
    file.update(|settings| {
        settings.extra.insert("b".into(), Value::from(2));
    })
    .unwrap();
    assert_eq!(file.load_error(), None);
    assert_eq!(
        fs::read_to_string(settings_backup_path(&path)).unwrap(),
        "{ oops"
    );
    // The last good settings plus the change.
    let mut expected = with_extra("a", Value::from(1));
    expected.extra.insert("b".into(), Value::from(2));
    assert_eq!(SettingsLoad::read(&path), SettingsLoad::Loaded(expected));
}

#[test]
fn failed_save_is_kept_until_one_succeeds() {
    let root = TempDir::new();
    // A file where the config folder should be: saving fails. Reading it is broken on Unix
    // (not a directory) but missing on Windows (path not found), so not asserted.
    let folder = root.0.join("config");
    fs::write(&folder, "").unwrap();
    let path = folder.join("settings.json");
    let mut file = SettingsFile::load(path);
    assert!(
        file.update(|settings| {
            settings.extra.insert("a".into(), Value::from(1));
        })
        .is_err()
    );
    assert!(file.save_error().is_some());
    assert_eq!(file.settings(), &with_extra("a", Value::from(1)));
    fs::remove_file(&folder).unwrap();
    file.save().unwrap();
    assert_eq!(file.save_error(), None);
    assert_eq!(file.load_error(), None);
}

#[test]
fn font_settings_default_to_the_built_in_font() {
    let settings = Settings::default();
    assert_eq!(settings.font_family(), None);
    assert_eq!(settings.font_size.points(), FontSize::DEFAULT);
}

#[test]
fn ui_theme_defaults_to_dark_and_reads_known_choices_only() {
    for text in [
        "{}",
        r#"{"ui_theme": null}"#,
        r#"{"ui_theme": 1}"#,
        r#"{"ui_theme": "system"}"#,
    ] {
        assert_eq!(Settings::from_json(text).unwrap().ui_theme, UiTheme::Dark);
    }
    for theme in [UiTheme::Light, UiTheme::Dark] {
        let settings = Settings {
            ui_theme: theme,
            ..Settings::default()
        };
        assert_eq!(
            Settings::from_json(&settings.to_json().unwrap()).unwrap(),
            settings
        );
    }
}

#[test]
fn ui_theme_is_saved_and_reloaded_without_losing_other_fields() {
    let root = TempDir::new();
    let path = root.0.join("settings.json");
    fs::write(&path, r#"{"font_size": 18, "future": true}"#).unwrap();
    let mut file = SettingsFile::load(path.clone());
    file.update(|settings| settings.ui_theme = UiTheme::Light)
        .unwrap();
    let saved = SettingsFile::load(path.clone());
    assert_eq!(saved.settings().ui_theme, UiTheme::Light);
    assert_eq!(saved.settings().font_size.points(), 18.);
    assert_eq!(saved.settings().extra["future"], true);
    fs::write(&path, r#"{"ui_theme": "dark"}"#).unwrap();
    file.reload();
    assert_eq!(file.settings().ui_theme, UiTheme::Dark);
}

#[test]
fn font_settings_read_good_values() {
    let settings =
        Settings::from_json(r#"{"font_family": " Fira Code ", "font_size": 20.5}"#).unwrap();
    assert_eq!(settings.font_family(), Some("Fira Code"));
    assert_eq!(settings.font_size.points(), 20.5);
    assert!(settings.extra.is_empty());
    let again = Settings::from_json(&settings.to_json().unwrap()).unwrap();
    assert_eq!(again, settings);
}

#[test]
fn blank_font_family_is_the_built_in_one() {
    for text in [r#"{"font_family": ""}"#, r#"{"font_family": "  "}"#] {
        assert_eq!(
            Settings::from_json(text).unwrap().font_family(),
            None,
            "{text}"
        );
    }
}

#[test]
fn font_settings_with_wrong_types_get_their_defaults() {
    let settings = Settings::from_json(r#"{"font_family": 3, "font_size": "big"}"#).unwrap();
    assert_eq!(settings, Settings::default());
    let settings = Settings::from_json(r#"{"font_size": null}"#).unwrap();
    assert_eq!(settings.font_size, FontSize::default());
}

#[test]
fn default_shell_reads_a_name_and_blank_is_automatic() {
    let settings = Settings::from_json(r#"{"default_shell": " Zsh "}"#).unwrap();
    assert_eq!(settings.default_shell(), Some("Zsh"));
    let again = Settings::from_json(&settings.to_json().unwrap()).unwrap();
    assert_eq!(again, settings);
    for text in [
        r#"{}"#,
        r#"{"default_shell": ""}"#,
        r#"{"default_shell": " "}"#,
    ] {
        let settings = Settings::from_json(text).unwrap();
        assert_eq!(settings.default_shell(), None, "{text}");
    }
    let settings = Settings::from_json(r#"{"default_shell": ["zsh"]}"#).unwrap();
    assert_eq!(settings, Settings::default());
}

#[test]
fn font_size_is_clamped() {
    for (text, points) in [
        (r#"{"font_size": 2}"#, FontSize::MIN),
        (r#"{"font_size": -14}"#, FontSize::MIN),
        (r#"{"font_size": 500}"#, FontSize::MAX),
        (r#"{"font_size": 1e40}"#, FontSize::MAX),
        (r#"{"font_size": 6}"#, 6.0),
        (r#"{"font_size": 72}"#, 72.0),
    ] {
        let settings = Settings::from_json(text).unwrap();
        assert_eq!(settings.font_size.points(), points, "{text}");
    }
    assert_eq!(FontSize::new(f32::NAN), FontSize::default());
    assert_eq!(FontSize::new(f32::INFINITY).points(), FontSize::MAX);
    // Written back clamped.
    let settings = Settings::from_json(r#"{"font_size": 100}"#).unwrap();
    assert!(settings.to_json().unwrap().contains(r#""font_size": 72.0"#));
}

#[test]
fn file_viewer_fraction_is_read_clamped_and_defaults() {
    let settings = Settings::default();
    assert_eq!(settings.file_viewer_fraction, ViewerFraction::default());
    for (text, fraction) in [
        (r#"{"file_viewer_fraction": 0.3}"#, 0.3),
        (r#"{"file_viewer_fraction": -1}"#, ViewerFraction::MIN),
        (r#"{"file_viewer_fraction": 7}"#, ViewerFraction::MAX),
        (
            r#"{"file_viewer_fraction": "wide"}"#,
            ViewerFraction::default().get(),
        ),
        (
            r#"{"file_viewer_fraction": null}"#,
            ViewerFraction::default().get(),
        ),
    ] {
        let settings = Settings::from_json(text).unwrap();
        assert_eq!(settings.file_viewer_fraction.get(), fraction, "{text}");
    }
    let settings = Settings::from_json(r#"{"file_viewer_fraction": 2}"#).unwrap();
    assert!(
        settings
            .to_json()
            .unwrap()
            .contains(r#""file_viewer_fraction": 1.0"#)
    );
    assert!(
        Settings::default()
            .to_json()
            .unwrap()
            .contains(r#""file_viewer_fraction": 0.45"#)
    );
}

#[test]
fn file_explorer_width_is_read_clamped_and_defaults() {
    assert_eq!(
        Settings::default().file_explorer_width,
        ExplorerWidth::default()
    );
    for (text, width) in [
        (r#"{"file_explorer_width": 420}"#, 420.),
        (r#"{"file_explorer_width": 20}"#, ExplorerWidth::MIN),
        (r#"{"file_explorer_width": 5000}"#, ExplorerWidth::MAX),
        (r#"{"file_explorer_width": "wide"}"#, 300.),
        (r#"{"file_explorer_width": null}"#, 300.),
    ] {
        let settings = Settings::from_json(text).unwrap();
        assert_eq!(settings.file_explorer_width.get(), width, "{text}");
    }
    let settings = Settings::from_json(r#"{"file_explorer_width": 5000}"#).unwrap();
    assert!(
        settings
            .to_json()
            .unwrap()
            .contains(r#""file_explorer_width": 600.0"#)
    );
    assert!(
        Settings::default()
            .to_json()
            .unwrap()
            .contains(r#""file_explorer_width": 300.0"#)
    );
}

#[derive(Debug, Default, PartialEq, Deserialize)]
#[serde(default)]
struct Sample {
    #[serde(deserialize_with = "lenient")]
    size: u16,
    #[serde(deserialize_with = "lenient")]
    name: String,
    #[serde(flatten)]
    extra: Map<String, Value>,
}

fn sample(text: &str) -> serde_json::Result<Sample> {
    serde_json::from_str(text)
}

#[test]
fn lenient_field_reads_a_good_value() {
    let parsed = sample(r#"{"size": 14, "name": "Mono", "other": true}"#).unwrap();
    assert_eq!(parsed.size, 14);
    assert_eq!(parsed.name, "Mono");
    assert_eq!(parsed.extra.len(), 1);
    assert_eq!(sample("{}").unwrap(), Sample::default());
}

#[test]
fn lenient_field_with_a_wrong_value_gets_its_default() {
    for text in [
        r#"{"size": "14", "name": "Mono"}"#,
        r#"{"size": null, "name": "Mono"}"#,
        r#"{"size": 70000, "name": "Mono"}"#,
        r#"{"size": -1, "name": "Mono"}"#,
        r#"{"size": {"a": [1]}, "name": "Mono"}"#,
    ] {
        let parsed = sample(text).unwrap();
        assert_eq!(parsed.size, 0, "{text}");
        assert_eq!(parsed.name, "Mono", "{text}");
        // Dropped, not kept with the unknown fields.
        assert!(parsed.extra.is_empty(), "{text}");
    }
}

#[test]
fn lenient_fields_still_need_a_json_object() {
    for text in ["", "[]", "3", "null", r#"{"size": }"#, r#"{"size": 1"#] {
        assert!(sample(text).is_err(), "{text:?}");
    }
}

fn names(list: &[&str]) -> Vec<String> {
    list.iter().map(|name| (*name).to_owned()).collect()
}

#[test]
fn installed_font_family_matches_exactly_when_case_counts() {
    let installed = names(&["DejaVu Sans Mono", "Liberation Mono"]);
    let found = installed_font_family(&installed, "Liberation Mono", Platform::Linux);
    assert_eq!(found, Some("Liberation Mono"));
    assert_eq!(
        installed_font_family(&installed, "liberation mono", Platform::Linux),
        None
    );
    assert_eq!(
        installed_font_family(&installed, "Liberation", Platform::Linux),
        None
    );
    assert_eq!(
        installed_font_family(&installed, "No Such Font", Platform::Linux),
        None
    );
}

#[test]
fn installed_font_family_ignores_case_like_windows() {
    let installed = names(&["Cascadia Mono", "Consolas", "Écrin Mono"]);
    let found = installed_font_family(&installed, "cascadia MONO", Platform::Windows);
    assert_eq!(found, Some("Cascadia Mono"));
    assert_eq!(
        installed_font_family(&installed, "écrin mono", Platform::Windows),
        Some("Écrin Mono")
    );
    assert_eq!(
        installed_font_family(&installed, "Cascadia", Platform::Windows),
        None
    );
}

#[test]
fn installed_font_family_refuses_aliases_and_empty_names() {
    let installed = names(&[".SystemUIFont", ".ZedMono", ""]);
    for platform in [Platform::Linux, Platform::Windows, Platform::MacOs] {
        for requested in [".SystemUIFont", ".ZedMono", ".zedmono", ""] {
            let found = installed_font_family(&installed, requested, platform);
            assert_eq!(found, None, "{requested:?}");
        }
    }
    assert_eq!(
        installed_font_family(&[], "Liberation Mono", Platform::Linux),
        None
    );
}
