//! The user's settings, kept in `settings.json` in the config folder. Every field is optional
//! with a default, a field with a value of the wrong type gets its default, and fields this
//! version doesn't know (from a newer version, or misspelled) are kept and written back. A
//! file that can't be read or isn't a JSON object is left on disk untouched: the defaults
//! apply, and the first save moves the broken file to `settings.json.bak` so the user's text
//! is never lost. Every change re-reads the file first, so edits made by hand or by another
//! instance since the start are kept.

use std::fs;
use std::io;
use std::path::{Path, PathBuf};

use serde::de::DeserializeOwned;
use serde::{Deserialize, Deserializer, Serialize};
use serde_json::{Map, Value};

use crate::window_state::write_atomic;

/// The settings. A new field gets a default (through `Default`, as `#[serde(default)]` asks
/// for), so a file without it still loads, and `#[serde(deserialize_with = "lenient")]`, so a
/// value of the wrong type falls back to `T::default()` instead of breaking the whole file.
/// A field whose default isn't its type's `Default` needs a type that has it (a newtype).
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Settings {
    /// Fields this version doesn't know, written back as they were.
    #[serde(flatten)]
    pub extra: Map<String, Value>,
}

impl Settings {
    pub fn from_json(text: &str) -> serde_json::Result<Self> {
        serde_json::from_str(text)
    }

    /// Pretty JSON. Fails only for a value JSON can't hold (a non-UTF-8 path).
    pub fn to_json(&self) -> serde_json::Result<String> {
        serde_json::to_string_pretty(self)
    }
}

/// `deserialize_with` for every `Settings` field: a value of the wrong type (or `null`) gives
/// `T::default()` and is dropped at the next save. Fails only on invalid JSON.
#[cfg_attr(
    not(test),
    expect(dead_code, reason = "used by the fields the next steps add")
)]
fn lenient<'de, D, T>(deserializer: D) -> Result<T, D::Error>
where
    D: Deserializer<'de>,
    T: DeserializeOwned + Default,
{
    let value = Value::deserialize(deserializer)?;
    Ok(serde_json::from_value(value).unwrap_or_default())
}

/// What reading the settings file found.
#[derive(Debug, PartialEq)]
pub enum SettingsLoad {
    /// No file yet: the defaults apply.
    Missing,
    Loaded(Settings),
    /// The file can't be read or isn't valid settings; the error says why.
    Broken(String),
}

impl SettingsLoad {
    pub fn read(path: &Path) -> Self {
        match fs::read_to_string(path) {
            Ok(text) => match Settings::from_json(&text) {
                Ok(settings) => Self::Loaded(settings),
                Err(err) => Self::Broken(err.to_string()),
            },
            Err(err) if err.kind() == io::ErrorKind::NotFound => Self::Missing,
            Err(err) => Self::Broken(err.to_string()),
        }
    }
}

/// Where a broken settings file is moved before the first save replaces it.
pub fn settings_backup_path(path: &Path) -> PathBuf {
    let mut name = path.file_name().unwrap_or_default().to_os_string();
    name.push(".bak");
    path.with_file_name(name)
}

/// The settings and the file they are saved to. Without a file (no config folder) they live
/// only in memory and saving does nothing.
#[derive(Debug, Default)]
pub struct SettingsFile {
    settings: Settings,
    path: Option<PathBuf>,
    /// Why the file read last is broken, until a save moves it aside.
    load_error: Option<String>,
    /// Why the last save failed, until one succeeds.
    save_error: Option<String>,
}

impl SettingsFile {
    /// The settings saved at `path`, else the defaults. A broken file is left as it is, with
    /// the reason in `load_error`.
    pub fn load(path: PathBuf) -> Self {
        let mut file = Self {
            path: Some(path),
            ..Self::default()
        };
        file.reload();
        file
    }

    pub fn settings(&self) -> &Settings {
        &self.settings
    }

    pub fn path(&self) -> Option<&Path> {
        self.path.as_deref()
    }

    /// Why the file is ignored (broken: the defaults or the last good settings apply), until a
    /// save moves it to `settings_backup_path`.
    pub fn load_error(&self) -> Option<&str> {
        self.load_error.as_deref()
    }

    /// Why the last save failed (the change still applies in memory), until a save succeeds.
    pub fn save_error(&self) -> Option<&str> {
        self.save_error.as_deref()
    }

    /// Take the settings from the file again: a good file replaces them, a missing or broken
    /// one keeps them (a broken one sets `load_error`).
    fn reload(&mut self) {
        let Some(path) = &self.path else {
            return;
        };
        match SettingsLoad::read(path) {
            SettingsLoad::Missing => self.load_error = None,
            SettingsLoad::Loaded(settings) => {
                self.settings = settings;
                self.load_error = None;
            }
            SettingsLoad::Broken(error) => self.load_error = Some(error),
        }
    }

    /// Re-read the file (keeping changes made on disk since), apply `change` and save when it
    /// changed anything; `Ok(true)` when it did. The re-read alone may change the settings
    /// too, so `Ok(false)` doesn't mean they are as before. On a failed save the change is
    /// kept in memory and the error in `save_error`.
    ///
    /// Every call that changes something writes the file: for a value dragged or stepped
    /// continuously (slider, pane width), call it once when the gesture ends, not per frame.
    pub fn update(&mut self, change: impl FnOnce(&mut Settings)) -> io::Result<bool> {
        self.reload();
        let before = self.settings.clone();
        change(&mut self.settings);
        if self.settings == before {
            return Ok(false);
        }
        self.save()?;
        Ok(true)
    }

    /// Write the settings to the file (see `write_atomic`), first moving it to
    /// `settings_backup_path` (replacing an older backup) if it is broken now. Overwrites
    /// changes made on disk since the last read: `update` re-reads first.
    pub fn save(&mut self) -> io::Result<()> {
        let result = self.write();
        self.save_error = result.as_ref().err().map(ToString::to_string);
        result
    }

    fn write(&mut self) -> io::Result<()> {
        let Some(path) = &self.path else {
            return Ok(());
        };
        let text = self.settings.to_json().map_err(io::Error::other)?;
        // Checked again: another instance or the user may have fixed or broken it since.
        if let SettingsLoad::Broken(_) = SettingsLoad::read(path) {
            match fs::rename(path, settings_backup_path(path)) {
                Ok(()) => {}
                // Removed or moved meanwhile: nothing to keep.
                Err(err) if err.kind() == io::ErrorKind::NotFound => {}
                Err(err) => return Err(err),
            }
        }
        self.load_error = None;
        write_atomic(path, &text)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
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
        // A file where the config folder should be: reading and saving fail.
        let folder = root.0.join("config");
        fs::write(&folder, "").unwrap();
        let path = folder.join("settings.json");
        let mut file = SettingsFile::load(path);
        assert!(file.load_error().is_some());
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
}
