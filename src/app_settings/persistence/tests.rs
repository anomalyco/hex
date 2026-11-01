use super::*;
use crate::app_settings::{MeetingRetention, MeetingSettings, RecordingAudioBehavior};
use serde::Deserialize;
use serde_json::json;
use std::sync::atomic::Ordering;

struct SettingsFile {
    path: PathBuf,
}

impl SettingsFile {
    fn new(data: &[u8]) -> Self {
        let sequence = crate::app_settings::SETTINGS_TEMP_SEQUENCE.fetch_add(1, Ordering::Relaxed);
        let directory = std::env::temp_dir().join(format!(
            "hex-settings-recovery-{}-{sequence}",
            std::process::id()
        ));
        fs::create_dir_all(&directory).unwrap();
        let path = directory.join("settings.json");
        fs::write(&path, data).unwrap();
        Self { path }
    }

    fn backups(&self) -> Vec<PathBuf> {
        fs::read_dir(self.path.parent().unwrap())
            .unwrap()
            .map(|entry| entry.unwrap().path())
            .filter(|path| {
                path.file_name()
                    .unwrap()
                    .to_string_lossy()
                    .contains(".unreadable-")
            })
            .collect()
    }
}

impl Drop for SettingsFile {
    fn drop(&mut self) {
        fs::remove_dir_all(self.path.parent().unwrap()).unwrap();
    }
}

#[test]
fn unset_retention_saves_a_shape_older_hex_can_read() {
    // The meeting field in 46c3c762 was non-optional, defaulting to Forever.
    #[derive(Deserialize)]
    struct OldMeetings {
        #[serde(default = "forever")]
        retention: MeetingRetention,
    }
    fn forever() -> MeetingRetention {
        MeetingRetention::Forever
    }
    assert!(serde_json::from_str::<OldMeetings>(r#"{"retention":null}"#).is_err());
    let settings: MeetingSettings = serde_json::from_str(r#"{"retention":null}"#).unwrap();
    assert_eq!(settings.retention, None);
    let written = serde_json::to_value(settings).unwrap();
    assert!(written.get("retention").is_none());
    let older: OldMeetings = serde_json::from_value(written.clone()).unwrap();
    assert_eq!(older.retention, MeetingRetention::Forever);
    let current: MeetingSettings = serde_json::from_value(written).unwrap();
    assert_eq!(current.retention, None);
    for retention in MeetingRetention::ALL {
        let written = serde_json::to_value(MeetingSettings {
            retention: Some(retention),
            ..Default::default()
        })
        .unwrap();
        assert_eq!(
            serde_json::from_value::<OldMeetings>(written)
                .unwrap()
                .retention,
            retention
        );
    }
}

#[test]
fn invalid_enum_resets_only_that_field_and_leaves_disk_alone() {
    let original = br#"{"recording_audio_behavior":"future-mode","sound_effect_volume":0.37,"meetings":{"enabled":true}}"#;
    let file = SettingsFile::new(original);
    let settings = AppSettings::load_from(&file.path).unwrap();
    assert_eq!(
        settings.recording_audio_behavior,
        RecordingAudioBehavior::default()
    );
    assert_eq!(settings.sound_effect_volume, 0.37);
    assert!(settings.meetings.enabled);
    assert_eq!(fs::read(&file.path).unwrap(), original);
    assert!(file.backups().is_empty());
}

#[test]
fn unbound_optional_shortcuts_stay_unbound_after_saving() {
    let settings = AppSettings {
        paste_last_hotkey: None,
        raw_dictation_hotkey: None,
        second_dictation_hotkey: None,
        ..Default::default()
    };
    let value = serde_json::to_value(&settings).unwrap();
    // Unlike retention, omitting Paste Last would restore a bound default.
    assert_eq!(value.get("paste_last_hotkey"), Some(&Value::Null));
    let loaded: AppSettings = serde_json::from_value(value).unwrap();
    assert!(loaded.paste_last_hotkey.is_none());
    assert!(loaded.raw_dictation_hotkey.is_none());
    assert!(loaded.second_dictation_hotkey.is_none());
}

#[test]
fn invalid_transcription_language_keeps_unrelated_settings() {
    let file = SettingsFile::new(
        &serde_json::to_vec(&json!({
            "transcription": {"model": "parakeet_unified_en", "language": "not-a-language"},
            "sound_effect_volume": 0.37
        }))
        .unwrap(),
    );
    let settings = AppSettings::load_from(&file.path).unwrap();
    assert_eq!(settings.transcription, AppSettings::default().transcription);
    assert_eq!(settings.sound_effect_volume, 0.37);
    settings.write_to(&file.path).unwrap();
    assert_eq!(file.backups().len(), 1);
}

#[test]
fn wrong_nested_type_and_multiple_bad_fields_keep_siblings() {
    let file = SettingsFile::new(br#"{
        "meetings":{"enabled":true,"calendar":{"connected":true,"lead_minutes":"soon","remind":false}},
        "sound_effects":[], "recording_audio_behavior":"unknown", "microphone":"USB mic"
    }"#);
    let settings = AppSettings::load_from(&file.path).unwrap();
    assert!(settings.meetings.enabled);
    assert!(settings.meetings.calendar.connected);
    assert!(!settings.meetings.calendar.remind);
    assert_eq!(settings.meetings.calendar.lead_minutes, 1);
    assert!(settings.sound_effects);
    assert_eq!(settings.microphone.as_deref(), Some("USB mic"));
}

#[test]
fn bad_list_entries_do_not_discard_good_entries() {
    let (settings, repaired) = decode(
        br#"{"meetings":{"providers":[
        {"key":"good","label":"Good","policy":"always_record"},
        {"key":42,"label":"Bad"},
        {"key":"kept","label":"Kept","policy":"unknown"}
    ]}}"#,
    );
    assert!(repaired);
    assert_eq!(settings.meetings.providers.len(), 2);
    assert_eq!(settings.meetings.providers[0].key, "good");
    assert_eq!(settings.meetings.providers[1].key, "kept");
}

#[test]
fn unknown_keys_are_ignored_without_repair() {
    let (settings, repaired) =
        decode(br#"{"future":{"flag":true},"meetings":{"future":42,"enabled":true}}"#);
    assert!(!repaired);
    assert!(settings.meetings.enabled);
}

#[test]
fn first_save_preserves_exact_original_once_even_after_reload() {
    let original =
        br#"{ "sound_theme":"future", "future_key": [1,2,3], "sound_effect_volume":0.37 }"#;
    let file = SettingsFile::new(original);
    let settings = AppSettings::load_from(&file.path).unwrap();
    settings.write_to(&file.path).unwrap();
    let backups = file.backups();
    assert_eq!(backups.len(), 1);
    assert_eq!(fs::read(&backups[0]).unwrap(), original);
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        assert_eq!(
            fs::metadata(&backups[0]).unwrap().permissions().mode() & 0o777,
            0o600
        );
    }
    let loaded = AppSettings::load_from(&file.path).unwrap();
    assert_eq!(loaded.sound_effect_volume, 0.37);
    loaded.write_to(&file.path).unwrap();
    assert_eq!(file.backups(), backups);
}

#[test]
fn malformed_json_and_non_objects_launch_with_defaults_and_are_backed_up() {
    for original in [&b"{not json"[..], &b"null"[..], &b"[]"[..], &b"\xff"[..]] {
        let file = SettingsFile::new(original);
        let settings = AppSettings::load_from(&file.path).unwrap();
        assert_eq!(settings, AppSettings::default());
        assert_eq!(fs::read(&file.path).unwrap(), original);
        settings.write_to(&file.path).unwrap();
        assert_eq!(file.backups().len(), 1);
        assert_eq!(fs::read(&file.backups()[0]).unwrap(), original);
    }
}

#[test]
fn backup_failure_blocks_save_and_preserves_original() {
    let original = b"{bad json";
    let file = SettingsFile::new(original);
    fs::create_dir(backup_path(&file.path, original)).unwrap();
    assert!(AppSettings::default().write_to(&file.path).is_err());
    assert_eq!(fs::read(&file.path).unwrap(), original);
}

#[test]
fn interrupted_save_reuses_the_complete_backup_but_refuses_a_partial_one() {
    let original = b"{bad json";
    let file = SettingsFile::new(original);
    backup(&file.path, original).unwrap();
    AppSettings::default().write_to(&file.path).unwrap();
    assert_eq!(file.backups().len(), 1);
    fs::write(&file.path, original).unwrap();
    fs::write(&file.backups()[0], b"partial").unwrap();
    assert!(AppSettings::default().write_to(&file.path).is_err());
    assert_eq!(fs::read(&file.path).unwrap(), original);
}

#[test]
fn save_checks_current_disk_bytes_not_only_what_was_loaded() {
    let file = SettingsFile::new(b"{}");
    let settings = AppSettings::load_from(&file.path).unwrap();
    let damaged = br#"{"sound_effects":"bad"}"#;
    fs::write(&file.path, damaged).unwrap();
    settings.write_to(&file.path).unwrap();
    assert_eq!(fs::read(&file.backups()[0]).unwrap(), damaged);
}

#[test]
fn out_of_range_setting_cannot_prevent_launch() {
    let file = SettingsFile::new(
        &serde_json::to_vec(&json!({
            "recording_duck_volume": 2.0, "sound_effect_volume": 0.37
        }))
        .unwrap(),
    );
    let settings = AppSettings::load_from(&file.path).unwrap();
    assert_eq!(settings.recording_duck_volume, 0.2);
    assert_eq!(settings.sound_effect_volume, 0.37);
    settings.write_to(&file.path).unwrap();
    assert_eq!(file.backups().len(), 1);
}

#[test]
fn unreadable_file_does_not_block_load_or_get_overwritten() {
    let file = SettingsFile::new(b"{}");
    fs::remove_file(&file.path).unwrap();
    fs::create_dir(&file.path).unwrap();
    assert_eq!(
        AppSettings::load_from(&file.path).unwrap(),
        AppSettings::default()
    );
    assert!(AppSettings::default().write_to(&file.path).is_err());
    assert!(file.path.is_dir());
}
