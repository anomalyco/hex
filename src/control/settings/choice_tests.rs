//! Drift guards for `control::settings::registry`: every `Kind::Choice`,
//! `Kind::NumberChoice`, and `Kind::Shortcut` key stays in lockstep with
//! its underlying enum's serde names, backing constant, or `HotkeySlot`.

use serde::Serialize;
use serde_json::Value;

use super::{Kind, ShortcutRules, apply, get, keys};
use crate::app_settings::{
    AppSettings, AppearanceTheme, CALENDAR_LEAD_MINUTES, CapsLockLight, DictationStyle, ExportSize,
    HotkeyKey, HotkeySlot, IndicatorPosition, KEEP_MICROPHONE_OPEN_SECONDS, MaxResolution,
    MeetingIndicatorCorner, MeetingPolicy, MeetingRetention, MeetingSummaries,
    RecordingAudioBehavior, RecordingQuality, SCREENSHOT_FRAME_RATES, TranscriptionDevice,
    UNLOAD_IDLE_MINUTES, UpdateChannel,
};

fn test_rules() -> ShortcutRules {
    ShortcutRules {
        key_named: |name| Ok(HotkeyKey::named(name)),
        rule: |_, _| Ok(()),
    }
}

fn serde_names<T: Serialize>(all: &[T]) -> Vec<String> {
    all.iter()
        .map(|variant| {
            serde_json::to_value(variant)
                .unwrap()
                .as_str()
                .expect("choice enum must serialize as a JSON string")
                .to_owned()
        })
        .collect()
}

fn assert_same_set(key: &str, declared: &[&str], expected: Vec<String>) {
    let mut declared_sorted: Vec<String> = declared.iter().map(|s| (*s).to_owned()).collect();
    declared_sorted.sort_unstable();
    let declared_len = declared_sorted.len();
    declared_sorted.dedup();
    assert_eq!(
        declared_sorted.len(),
        declared_len,
        "`{key}` lists a duplicate choice"
    );

    let mut expected_sorted = expected;
    expected_sorted.sort_unstable();
    let expected_len = expected_sorted.len();
    expected_sorted.dedup();
    assert_eq!(
        expected_sorted.len(),
        expected_len,
        "`{key}` enum has duplicate serde names"
    );

    assert_eq!(
        declared_sorted, expected_sorted,
        "`{key}` choices drifted from its underlying enum's serde names"
    );
}

/// Every `Kind::Choice` and `Kind::NumberChoice` value advertised by
/// `registry.rs` must deserialize cleanly through `apply` and round-trip
/// through `get` with the exact same string.
#[test]
fn every_choice_key_round_trips_through_apply_and_get() {
    let defaults = AppSettings::default();
    let rules = test_rules();
    for key in keys() {
        let choices = match key.kind {
            Kind::Choice(choices) | Kind::NumberChoice(choices) => choices,
            _ => continue,
        };
        assert!(!choices.is_empty(), "`{}` declares no choices", key.key);
        for &choice in choices {
            let change = apply(&defaults, key.key, choice, &rules).unwrap_or_else(|error| {
                panic!(
                    "`{}` advertised choice `{choice}`, but apply rejected it: {}",
                    key.key, error.message
                )
            });
            let shown = get(&change.settings, key.key).unwrap();
            assert_eq!(
                shown[0].value,
                Value::String(choice.into()),
                "`{}` choice `{choice}` did not round-trip through serde",
                key.key
            );
        }
        let bad = apply(&defaults, key.key, "__not_a_valid_choice__", &rules).unwrap_err();
        assert_eq!(bad.code, "invalid-value", "`{}`", key.key);
        if key.optional {
            let cleared = apply(&defaults, key.key, "none", &rules).unwrap();
            let shown = get(&cleared.settings, key.key).unwrap();
            assert!(shown[0].value.is_null(), "`{}` none did not clear", key.key);
        }
    }
}

/// Every `Kind::Choice` key in `registry.rs` must list the exact set of
/// serde names its backing enum (or language table) defines.
#[test]
fn every_choice_key_matches_its_enum_serde_names() {
    for key in keys() {
        let Kind::Choice(declared) = key.kind else {
            continue;
        };
        let expected: Vec<String> = match key.key {
            "sound_theme" => serde_names(&crate::feedback::SoundSet::ALL),
            "recording_audio_behavior" => serde_names(&RecordingAudioBehavior::ALL),
            "shortcut_mode" => serde_names(&crate::dictation::ShortcutMode::ALL),
            "screenshots.max_resolution" => serde_names(&MaxResolution::ALL),
            "screenshots.quality" => serde_names(&RecordingQuality::ALL),
            "screenshots.export_size" => serde_names(&ExportSize::ALL),
            "screenshots.record_kind" => serde_names(&crate::app_settings::RecordKind::ALL),
            "vocabulary_boost" => serde_names(&crate::dictation::vocabulary::VocabularyBoost::ALL),
            "history_retention" => serde_names(&crate::history::HistoryRetention::ALL),
            "labs.caps_lock_light" => serde_names(&CapsLockLight::ALL),
            "labs.appearance_theme" => serde_names(&AppearanceTheme::ALL),
            "labs.dictation_style_choice" => serde_names(&DictationStyle::ALL),
            "meetings.summaries" => {
                let all: Vec<_> = MeetingSummaries::ALL
                    .into_iter()
                    .filter(|s| cfg!(target_os = "macos") || *s != MeetingSummaries::Mac)
                    .collect();
                serde_names(&all)
            }
            "meetings.recording_indicator_corner" => serde_names(&MeetingIndicatorCorner::ALL),
            "meetings.retention" | "meetings.audio_retention" => {
                serde_names(&MeetingRetention::ALL)
            }
            "meetings.default_policy" => serde_names(&MeetingPolicy::ALL),
            "experimental.hex_microphone_mode" => {
                serde_names(&crate::audio::voice_filter::VoiceFilterMode::ALL)
            }
            "ui_language" => crate::desktop::i18n::LANGUAGE_CHOICES
                .iter()
                .filter_map(|(code, _)| code.map(str::to_owned))
                .collect(),
            "transcription_device" => serde_names(&TranscriptionDevice::ALL),
            "indicator_position" => serde_names(&IndicatorPosition::ALL),
            "updates.channel" => serde_names(&UpdateChannel::ALL),
            #[cfg(hex_capture)]
            "capture.transcriber" => serde_names(&crate::app_settings::CaptureTranscriber::ALL),
            #[cfg(hex_capture)]
            "capture.capsule_flow" => serde_names(&crate::app_settings::CapsuleFlow::ALL),
            other => panic!(
                "Kind::Choice key `{other}` is missing from every_choice_key_matches_its_enum_serde_names"
            ),
        };
        assert_same_set(key.key, declared, expected);
    }
}

/// Every `Kind::NumberChoice` key in `registry.rs` must match its backing
/// constant in `app_settings`.
#[test]
fn every_number_choice_key_matches_its_backing_constant() {
    for key in keys() {
        let Kind::NumberChoice(declared) = key.kind else {
            continue;
        };
        let parsed: Vec<u32> = declared
            .iter()
            .map(|value| {
                value
                    .parse::<u32>()
                    .expect("number choice must parse as u32")
            })
            .collect();
        let expected: Vec<u32> = match key.key {
            "screenshots.frame_rate" => SCREENSHOT_FRAME_RATES.to_vec(),
            "labs.unload_idle_minutes" => UNLOAD_IDLE_MINUTES.to_vec(),
            "labs.always_listening_pause_seconds" => {
                crate::app_settings::ALWAYS_LISTENING_PAUSE_SECONDS.to_vec()
            }
            "labs.keep_microphone_open_seconds" => KEEP_MICROPHONE_OPEN_SECONDS.to_vec(),
            "meetings.calendar.lead_minutes" => CALENDAR_LEAD_MINUTES.map(u32::from).to_vec(),
            other => panic!(
                "Kind::NumberChoice key `{other}` is missing from every_number_choice_key_matches_its_backing_constant"
            ),
        };
        assert_eq!(parsed, expected, "`{}` drifted from its constant", key.key);
    }
}

/// Every named `HotkeySlot` descriptor matches its `Kind::Shortcut(slot)`
/// entry in `registry.rs` 1-to-1.
#[test]
fn every_shortcut_slot_matches_its_registry_key() {
    let registry_slots: Vec<(&str, HotkeySlot)> = keys()
        .filter_map(|key| match key.kind {
            Kind::Shortcut(slot) => Some((key.key, slot)),
            _ => None,
        })
        .collect();
    let descriptor_slots: Vec<(&str, HotkeySlot)> = HotkeySlot::NAMED
        .iter()
        .map(|desc| (desc.key, desc.slot))
        .collect();
    assert_eq!(
        descriptor_slots.len(),
        registry_slots.len(),
        "HotkeySlot::NAMED and registry Kind::Shortcut entries have different counts"
    );
    for pair in &descriptor_slots {
        assert!(
            registry_slots.contains(pair),
            "HotkeySlot::NAMED entry {pair:?} is missing or mismatched in registry.rs"
        );
    }
}
