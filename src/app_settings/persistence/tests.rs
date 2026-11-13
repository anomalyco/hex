use super::*;
use crate::app_settings::{
    MeetingRetention, MeetingSettings, MeetingSummaries, RecordingAudioBehavior,
};
use crate::dictation::vocabulary::VocabularyEntry;
use crate::transcription::models::{TranscriptionModelId, TranscriptionSelection};
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
fn base_aware_updates_keep_other_writers_and_nested_unknown_fields() {
    let file = SettingsFile::new(br#"{"future":42,"meetings":{"future":{"mode":"keep"}}}"#);
    let base = AppSettings::load_from(&file.path).unwrap();
    let mut cli = base.clone();
    cli.meetings.show_recording_indicator = !base.meetings.show_recording_indicator;
    let first = base.update_at(&cli, &file.path, Policy::default()).unwrap();
    let mut window = base.clone();
    window.sound_effect_volume = 0.37;
    let second = base
        .update_at(&window, &file.path, Policy::default())
        .unwrap();
    assert_eq!(second.sound_effect_volume, 0.37);
    assert_eq!(
        second.meetings.show_recording_indicator,
        cli.meetings.show_recording_indicator
    );
    assert!(second.source.revision > first.source.revision);
    let saved: Value = serde_json::from_slice(&fs::read(&file.path).unwrap()).unwrap();
    assert_eq!(saved["future"], 42);
    assert_eq!(saved["meetings"]["future"]["mode"], "keep");
}

#[test]
fn simultaneous_updates_serialize_the_read_merge_and_write() {
    let file = SettingsFile::new(b"{}");
    let base = AppSettings::load_from(&file.path).unwrap();
    let barrier = std::sync::Barrier::new(2);
    std::thread::scope(|scope| {
        for volume in [true, false] {
            let (base, path, barrier) = (&base, &file.path, &barrier);
            scope.spawn(move || {
                let mut candidate = base.clone();
                if volume {
                    candidate.sound_effect_volume = 0.37;
                } else {
                    candidate.meetings.show_recording_indicator =
                        !base.meetings.show_recording_indicator;
                }
                barrier.wait();
                base.update_at(&candidate, path, Policy::default()).unwrap();
            });
        }
    });
    let saved = AppSettings::load_from(&file.path).unwrap();
    assert_eq!(saved.sound_effect_volume, 0.37);
    assert_ne!(
        saved.meetings.show_recording_indicator,
        base.meetings.show_recording_indicator
    );
}

#[test]
fn a_delayed_read_cannot_republish_runtime_values_from_before_a_save() {
    let _restore = RestoredRuntime;
    let file = SettingsFile::new(b"{}");
    let base = AppSettings::load_from(&file.path).unwrap();
    let mut candidate = base.clone();
    candidate.release_microphone_while_idle = true;
    base.update_at(&candidate, &file.path, Policy::default())
        .unwrap();
    base.apply_runtime();
    assert!(crate::app_settings::microphone_policy().release_while_idle);
}

#[cfg(any(target_os = "macos", target_os = "windows"))]
#[test]
fn a_revert_before_the_host_reloads_still_publishes_a_new_revision() {
    let _restore = RestoredRuntime;
    let file = SettingsFile::new(b"{}");
    let host = AppSettings::load_from(&file.path).unwrap();
    let mut candidate = host.clone();
    candidate.release_microphone_while_idle = true;
    let window = host
        .update_at(&candidate, &file.path, Policy::default())
        .unwrap();
    // An external editor reverts before the host has seen the window's save.
    fs::write(&file.path, serde_json::to_vec(&host).unwrap()).unwrap();
    let reverted = host.refreshed().unwrap().expect("notify the window");
    assert_eq!(reverted, host);
    assert!(reverted.source.revision > window.source.revision);
    assert!(!crate::app_settings::microphone_policy().release_while_idle);
    assert!(
        reverted.refreshed().unwrap().is_none(),
        "unchanged polls stay quiet"
    );
}

#[test]
fn a_fresh_reader_cannot_authorize_a_failed_base() {
    let file = SettingsFile::new(b"[]");
    let failed = AppSettings::load_from(&file.path).unwrap();
    fs::write(&file.path, br#"{"sound_effect_volume":0.37}"#).unwrap();
    let healthy = AppSettings::load_from(&file.path).unwrap();
    let original = fs::read(&file.path).unwrap();
    let mut candidate = failed.clone();
    candidate.sound_effects = false;
    assert!(
        failed
            .update_at(&candidate, &file.path, Policy::default())
            .is_err()
    );
    assert_eq!(fs::read(&file.path).unwrap(), original);
    let mut candidate = healthy.clone();
    candidate.sound_effects = false;
    let saved = healthy
        .update_at(&candidate, &file.path, Policy::default())
        .unwrap();
    assert_eq!(saved.sound_effect_volume, 0.37);
    assert!(!saved.sound_effects);
}

#[test]
fn update_preserves_locked_values_and_backups_invalid_fields() {
    let original =
        br#"{"sound_effect_volume":"bad","meetings":{"enabled":true,"retention":"invalid"}}"#;
    let file = SettingsFile::new(original);
    let policy = Policy::default().with(crate::policy::Lock::Meetings);
    let base = AppSettings::read_managed_from(&file.path, policy)
        .unwrap()
        .0;
    assert!(!base.meetings.enabled);
    let mut candidate = base.clone();
    candidate.sound_effect_volume = 0.37;
    let saved = base.update_at(&candidate, &file.path, policy).unwrap();
    assert!(!saved.meetings.enabled);
    let raw: Value = serde_json::from_slice(&fs::read(&file.path).unwrap()).unwrap();
    assert_eq!(raw["meetings"]["enabled"], true);
    assert!(raw["meetings"].get("retention").is_none());
    assert_eq!(fs::read(&file.backups()[0]).unwrap(), original);
}

#[test]
fn failed_transaction_leaves_the_base_and_file_unchanged() {
    let file = SettingsFile::new(b"{}");
    let base = AppSettings::load_from(&file.path).unwrap();
    let damaged = br#"{"sound_effects":"bad"}"#;
    fs::write(&file.path, damaged).unwrap();
    fs::create_dir(backup_path(&file.path, damaged)).unwrap();
    let mut candidate = base.clone();
    candidate.sound_effect_volume = 0.37;
    assert!(
        base.update_at(&candidate, &file.path, Policy::default())
            .is_err()
    );
    assert_eq!(fs::read(&file.path).unwrap(), damaged);
    assert_ne!(base.sound_effect_volume, candidate.sound_effect_volume);
}

/// The note `load_for_launch` builds for `path`, without using the real
/// settings folder.
fn launch_note(path: &Path) -> Option<String> {
    match AppSettings::load_checked_from(path).unwrap().1 {
        Some(LoadIssue::Unparseable(reason)) => Some(reason.launch_note()),
        Some(LoadIssue::Unreadable) | None => None,
    }
}

/// `load_for_launch` publishes process-global runtime state; restore the defaults afterwards.
struct RestoredRuntime;

impl Drop for RestoredRuntime {
    fn drop(&mut self) {
        AppSettings::default().apply_runtime();
    }
}

#[test]
fn unreadable_load_cannot_overwrite_a_file_that_becomes_readable() {
    let file = SettingsFile::new(b"{}");
    fs::remove_file(&file.path).unwrap();
    fs::create_dir(&file.path).unwrap();
    let (settings, issue) = AppSettings::load_checked_from(&file.path).unwrap();
    assert!(matches!(issue, Some(LoadIssue::Unreadable)));

    fs::remove_dir(&file.path).unwrap();
    let original = br#"{"sound_effect_volume":0.37}"#;
    fs::write(&file.path, original).unwrap();
    assert!(settings.write_to(&file.path).is_err());
    assert_eq!(fs::read(&file.path).unwrap(), original);
    assert!(file.backups().is_empty());
}

#[test]
fn unreadable_launch_shows_a_note() {
    let _restore = RestoredRuntime;
    let file = SettingsFile::new(b"{}");
    fs::remove_file(&file.path).unwrap();
    fs::create_dir(&file.path).unwrap();
    let launch = AppSettings::load_for_launch_at(Ok(file.path.clone()));
    assert!(matches!(launch.note, Some(LaunchNote::Failed(_))));
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
fn saved_keys_are_sorted_at_every_level() {
    let file = SettingsFile::new(b"{}");
    let settings = AppSettings {
        sound_effect_volume: 0.37,
        vocabulary: vec![VocabularyEntry {
            term: "Cappuccino".into(),
            sounds_like: vec!["capuchin".into()],
        }],
        ..AppSettings::default()
    };
    settings.write_to(&file.path).unwrap();
    let raw = fs::read_to_string(&file.path).unwrap();
    let keys = assert_sorted_keys(&raw);
    assert!(keys > 40, "only {keys} keys checked");
    // f32 settings keep their short form instead of widening through `Value`.
    for (key, value) in [
        ("sound_effect_volume", "0.37"),
        ("recording_duck_volume", "0.2"),
        ("hex_microphone_strength", "1.0"),
    ] {
        let line = format!("\"{key}\": {value}");
        assert!(
            raw.lines()
                .any(|written| written.trim().trim_end_matches(',') == line),
            "settings.json has no `{line}`"
        );
    }
    let saved: Value = serde_json::from_str(&raw).unwrap();
    // These sections have several keys each, and the vocabulary entry is an
    // object inside an array, so nested objects are covered.
    assert!(saved["meetings"].as_object().unwrap().len() > 1);
    assert!(saved["transcription"].as_object().unwrap().len() > 1);
    assert_eq!(saved["vocabulary"][0]["sounds_like"], json!(["capuchin"]));
    assert_eq!(AppSettings::load_from(&file.path).unwrap(), settings);
}

/// Asserts that every object in the pretty-printed JSON `raw` has ascending
/// keys, including the objects inside arrays, and returns the number of keys it
/// checked. `Value` sorts keys as it parses, so this reads the file's own text:
/// serde_json gives every key its own line, indented two spaces per level.
fn assert_sorted_keys(raw: &str) -> usize {
    /// The key of a line that starts a member (`"key": ...`).
    fn key_at(line: &str) -> Option<&str> {
        let rest = line.strip_prefix('"')?;
        let key = &rest[..rest.find('"')?];
        rest[key.len() + 1..].starts_with(':').then_some(key)
    }
    // Each open object by the indentation of its keys, with the last key seen.
    let mut open: Vec<(usize, &str)> = Vec::new();
    let mut checked = 0;
    for line in raw.lines() {
        let indent = line.len() - line.trim_start().len();
        open.retain(|(key_indent, _)| *key_indent <= indent);
        let Some(key) = key_at(line.trim_start()) else {
            continue;
        };
        checked += 1;
        match open.last_mut() {
            Some((key_indent, previous)) if *key_indent == indent => {
                assert!(
                    *previous < key,
                    "settings.json writes \"{key}\" after \"{previous}\" at nesting level {}",
                    indent / 2
                );
                *previous = key;
            }
            _ => open.push((indent, key)),
        }
    }
    checked
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
fn an_invalid_appearance_theme_resets_only_that_field() {
    let original = br#"{"labs":{"appearance":true,"appearance_theme":"sepia","live_text":true}}"#;
    let file = SettingsFile::new(original);
    let settings = AppSettings::load_from(&file.path).unwrap();
    assert!(settings.labs.appearance);
    assert_eq!(
        settings.labs.appearance_theme,
        crate::app_settings::AppearanceTheme::default()
    );
    assert!(settings.labs.live_text);
    assert_eq!(fs::read(&file.path).unwrap(), original);
    assert!(file.backups().is_empty());
}

#[test]
fn a_microphone_linger_window_outside_the_choices_loads_the_default() {
    let original = br#"{"labs":{"keep_microphone_open":true,"keep_microphone_open_seconds":45,"live_text":true}}"#;
    let file = SettingsFile::new(original);
    let settings = AppSettings::load_from(&file.path).unwrap();
    assert!(
        settings.labs.keep_microphone_open,
        "the switch stays as saved"
    );
    assert_eq!(
        settings.labs.keep_microphone_open_seconds,
        crate::app_settings::LabsSettings::DEFAULT_KEEP_MICROPHONE_OPEN_SECONDS
    );
    assert_eq!(
        settings.labs.keep_microphone_open_for(),
        Some(std::time::Duration::from_secs(60))
    );
    assert!(settings.labs.live_text);
    assert_eq!(fs::read(&file.path).unwrap(), original);
    assert!(file.backups().is_empty());
}

#[test]
fn a_microphone_linger_window_from_the_choices_keeps_its_seconds() {
    let original = br#"{"labs":{"keep_microphone_open":true,"keep_microphone_open_seconds":180}}"#;
    let file = SettingsFile::new(original);
    let settings = AppSettings::load_from(&file.path).unwrap();
    assert_eq!(
        settings.labs.keep_microphone_open_for(),
        Some(std::time::Duration::from_secs(180))
    );
}

#[test]
fn an_invalid_voice_prompt_phrase_loads_the_default_and_keeps_its_switch() {
    let original =
        br#"{"labs":{"voice_prompt":true,"voice_prompt_phrase":"nope","live_text":true}}"#;
    let file = SettingsFile::new(original);
    let settings = AppSettings::load_from(&file.path).unwrap();
    assert!(
        settings.labs.voice_prompt,
        "the switch stays as saved; only the phrase falls back"
    );
    assert_eq!(
        settings.labs.voice_prompt_phrase,
        crate::dictation::voice_prompt::DEFAULT_PHRASE
    );
    assert!(settings.labs.live_text);
    assert_eq!(fs::read(&file.path).unwrap(), original);
    assert!(file.backups().is_empty());
}

#[test]
fn browser_origins_load_normalized_and_ignore_anything_that_is_not_one() {
    let file = SettingsFile::new(
        br#"{"labs":{"api_browser_access":true,"api_browser_origins":["HTTPS://Example.com:443/","*","not an origin","http://localhost:5173"]}}"#,
    );
    let settings = AppSettings::load_from(&file.path).unwrap();
    assert!(settings.labs.api_browser_access);
    assert_eq!(
        settings.labs.api_browser_origins,
        vec!["https://example.com", "http://localhost:5173"]
    );
    // The normalized list is what the next read sees, so an entry that
    // isn't an origin can never widen what the local API allows.
    let saved: Value = serde_json::from_slice(&fs::read(&file.path).unwrap()).unwrap();
    assert_eq!(
        saved["labs"]["api_browser_origins"],
        json!(["https://example.com", "http://localhost:5173"])
    );
}

#[test]
fn unbound_optional_shortcuts_stay_unbound_after_saving() {
    let settings = AppSettings {
        paste_last_hotkey: None,
        raw_dictation_hotkey: None,
        typed_text_hotkey: None,
        second_dictation_hotkey: None,
        terminal_paste_hotkey: None,
        ..Default::default()
    };
    let value = serde_json::to_value(&settings).unwrap();
    // Unlike retention, omitting Paste Last would restore a bound default.
    assert_eq!(value.get("paste_last_hotkey"), Some(&Value::Null));
    let loaded: AppSettings = serde_json::from_value(value).unwrap();
    assert!(loaded.paste_last_hotkey.is_none());
    assert!(loaded.raw_dictation_hotkey.is_none());
    assert!(loaded.typed_text_hotkey.is_none());
    assert!(loaded.second_dictation_hotkey.is_none());
    assert!(loaded.terminal_paste_hotkey.is_none());
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
fn malformed_json_uses_defaults_and_refuses_to_be_replaced() {
    let original = b"{\n    \"sound_effects\": true,\n    \"not json\":\n}";
    let file = SettingsFile::new(original);
    let settings = AppSettings::load_from(&file.path).unwrap();
    assert_eq!(settings, AppSettings::default());
    let mut modified = settings;
    modified.sound_effect_volume = 0.5;
    let error = modified.write_to(&file.path).unwrap_err();
    let message = format!("{error:#}");
    assert!(message.contains("isn't valid JSON"), "{message}");
    assert!(message.contains("line 4 column 1"), "{message}");
    assert!(
        message.contains("Fix or remove it, then restart Hex"),
        "{message}"
    );
    assert!(message.contains("Nothing was saved"), "{message}");
    assert_eq!(fs::read(&file.path).unwrap(), original);
    assert!(file.backups().is_empty());

    // The process still remembers the broken load: a fixed file is not
    // replaced by the defaults that load used.
    fs::write(&file.path, b"{}").unwrap();
    let error = modified.write_to(&file.path).unwrap_err();
    let message = format!("{error:#}");
    assert!(
        message.contains("couldn't be read when Hex loaded it"),
        "{message}"
    );
    assert!(
        message.contains("Restart Hex to load the fixed file"),
        "{message}"
    );
    assert_eq!(fs::read(&file.path).unwrap(), b"{}");
    assert!(file.backups().is_empty());

    // Reading the fixed file does not clear the refusal; the save stays
    // refused until a restart or the Linux poll adopts the file.
    AppSettings::read_from(&file.path).unwrap();
    assert!(modified.write_to(&file.path).is_err());
    assert_eq!(fs::read(&file.path).unwrap(), b"{}");
    AppSettings::load_from(&file.path)
        .unwrap()
        .write_to(&file.path)
        .unwrap();
    assert!(modified.write_to(&file.path).is_err());
    assert!(file.backups().is_empty());
}

#[test]
fn stale_defaults_cannot_replace_a_file_fixed_while_the_process_runs() {
    let broken = b"{\n    \"sound_effects\": true,\n    \"not json\":\n}";
    let file = SettingsFile::new(broken);
    let stale = AppSettings::load_from(&file.path).unwrap();
    assert_eq!(stale, AppSettings::default());

    let fixed = br#"{"sound_effect_volume":0.37}"#;
    fs::write(&file.path, fixed).unwrap();
    let error = stale.write_to(&file.path).unwrap_err();
    let message = format!("{error:#}");
    assert!(
        message.contains("couldn't be read when Hex loaded it"),
        "{message}"
    );
    assert!(
        message.contains("Restart Hex to load the fixed file"),
        "{message}"
    );
    assert_eq!(fs::read(&file.path).unwrap(), fixed);
    assert!(file.backups().is_empty());

    // Reading the fixed file does not clear the refusal: the running app's
    // in-memory settings are still the stale defaults that load used.
    let loaded = AppSettings::read_from(&file.path).unwrap().0;
    assert_eq!(loaded.sound_effect_volume, 0.37);
    let (read, _) = AppSettings::read_from(&file.path).unwrap();
    assert_eq!(read.sound_effect_volume, 0.37);
    let error = stale.write_to(&file.path).unwrap_err();
    assert!(format!("{error:#}").contains("couldn't be read when Hex loaded it"));
    assert_eq!(fs::read(&file.path).unwrap(), fixed);
    assert!(file.backups().is_empty());

    // A successful snapshot can save; it never authorizes the stale one.
    loaded.write_to(&file.path).unwrap();
    assert!(stale.write_to(&file.path).is_err());
    let saved: Value = serde_json::from_slice(&fs::read(&file.path).unwrap()).unwrap();
    assert_eq!(saved["sound_effect_volume"], json!(0.37));
    assert!(file.backups().is_empty());
}

#[test]
fn non_object_json_uses_defaults_and_refuses_to_be_replaced() {
    for original in [&b"null"[..], &b"[]"[..], &b"42"[..]] {
        let file = SettingsFile::new(original);
        let settings = AppSettings::load_from(&file.path).unwrap();
        assert_eq!(settings, AppSettings::default());
        let error = AppSettings::default().write_to(&file.path).unwrap_err();
        let message = format!("{error:#}");
        assert!(message.contains("must be a JSON object"), "{message}");
        assert!(
            message.contains("Fix or remove it, then restart Hex"),
            "{message}"
        );
        assert_eq!(fs::read(&file.path).unwrap(), original);
        assert!(file.backups().is_empty());
    }
}

#[test]
fn a_broken_settings_file_is_named_at_launch_with_its_parse_error() {
    let file = SettingsFile::new(b"{\n    \"sound_effects\": true,\n    \"not json\":\n}");
    let note = launch_note(&file.path).unwrap();
    assert!(note.contains("started with defaults"), "{note}");
    assert!(note.contains("Your file wasn't changed"), "{note}");
    assert!(note.contains("then restart Hex"), "{note}");
    assert!(note.contains("line 4 column 1"), "{note}");

    let file = SettingsFile::new(b"[]");
    assert!(
        launch_note(&file.path)
            .unwrap()
            .contains("must be a JSON object")
    );

    let file = SettingsFile::new(b"{}");
    assert!(launch_note(&file.path).is_none());
}

#[test]
fn launch_publishes_the_saved_selection_before_the_worker_starts() {
    let _restore = RestoredRuntime;
    let file = SettingsFile::new(b"{}");
    let saved = TranscriptionSelection {
        model: TranscriptionModelId::WhisperLargeV3Turbo,
        language: "fr".into(),
    };
    AppSettings {
        transcription: saved.clone(),
        ..AppSettings::default()
    }
    .write_to(&file.path)
    .unwrap();

    AppSettings::load_for_launch_at(Ok(file.path.clone()));
    assert_eq!(crate::app_settings::transcription_selection().1, saved);

    // The defaults fallback publishes too, so a launch that can't read settings starts from defaults.
    AppSettings::load_for_launch_at(Err(eyre!("settings path unavailable")));
    assert_eq!(
        crate::app_settings::transcription_selection().1,
        TranscriptionSelection::default()
    );
}

#[test]
fn backup_failure_blocks_save_and_preserves_original() {
    let original = br#"{"sound_effects":"bad"}"#;
    let file = SettingsFile::new(original);
    fs::create_dir(backup_path(&file.path, original)).unwrap();
    assert!(AppSettings::default().write_to(&file.path).is_err());
    assert_eq!(fs::read(&file.path).unwrap(), original);
}

#[test]
fn interrupted_save_reuses_the_complete_backup_but_refuses_a_partial_one() {
    let original = br#"{"sound_effects":"bad"}"#;
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
    // An unreadable file is not a parse error, so launch has no note for it.
    assert!(!matches!(
        AppSettings::load_checked_from(&file.path).unwrap().1,
        Some(LoadIssue::Unparseable(_))
    ));
    assert!(launch_note(&file.path).is_none());
    assert!(AppSettings::default().write_to(&file.path).is_err());
    assert!(file.path.is_dir());
}

#[test]
fn a_managed_lock_reads_off_and_never_rewrites_the_saved_switch() {
    use crate::policy::{Lock, Policy};
    let file = SettingsFile::new(
        br#"{"meetings":{"enabled":true,"summarize_with_apple_intelligence":false,"summarize_with_opencode":true},"labs":{"live_text":true}}"#,
    );
    let policy = Policy::default()
        .with(Lock::Meetings)
        .with(Lock::OpenCodeProcessing)
        .with(Lock::Labs);
    let (mut settings, _) = AppSettings::read_managed_from(&file.path, policy).unwrap();
    assert!(!settings.meetings.enabled);
    assert_eq!(settings.meetings.summaries, MeetingSummaries::Off);
    assert!(!settings.labs.live_text);

    // Saving another change, or the locked switch itself, keeps the file's.
    settings.sound_effects = false;
    settings.meetings.enabled = true;
    settings.write_managed_to(&file.path, policy).unwrap();
    let saved: serde_json::Value = serde_json::from_slice(&fs::read(&file.path).unwrap()).unwrap();
    assert_eq!(saved["meetings"]["enabled"], json!(true));
    assert_eq!(saved["meetings"]["summaries"], json!("opencode"));
    assert_eq!(saved["labs"]["live_text"], json!(true));
    assert_eq!(saved["sound_effects"], json!(false));

    // Without the policy the user's own choices come back.
    let (unmanaged, _) = AppSettings::read_managed_from(&file.path, Policy::default()).unwrap();
    assert!(unmanaged.meetings.enabled);
    assert!(unmanaged.labs.live_text);
}
