use std::fs;
use std::path::Path;

use color_eyre::eyre::Result;
use serde::Deserialize;
use serde::de::DeserializeOwned;
use serde_json::{Map, Value};

use crate::app_settings::{
    AppSettings, HotkeyBinding, HotkeyKey, HotkeyModifiers, ModifierSide, RecordingAudioBehavior,
};

const SWIFT_SETTINGS_PATH: &str = "Library/Containers/com.kitlangton.Hex/Data/Library/Application Support/com.kitlangton.Hex/hex_settings.json";
const SWIFT_BASE_SOUND_EFFECT_VOLUME: f32 = 0.2;

pub fn import() -> Option<AppSettings> {
    // Explicit support roots are isolated test, preview, or embedded-service instances.
    if std::env::var_os("HEX_APPLICATION_SUPPORT_DIR").is_some() {
        return None;
    }
    let home = dirs::home_dir()?;
    match import_from_path(&home.join(SWIFT_SETTINGS_PATH)) {
        Ok(settings) => settings,
        Err(error) => {
            tracing::warn!(%error, "could not import Swift HEX preferences");
            None
        }
    }
}

fn import_from_path(path: &Path) -> Result<Option<AppSettings>> {
    let data = match fs::read(path) {
        Ok(data) => data,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(error) => return Err(error.into()),
    };
    let Some(source) = serde_json::from_slice::<Value>(&data)?.as_object().cloned() else {
        return Ok(None);
    };

    Ok(Some(import_fields(&source)))
}

fn import_fields(source: &Map<String, Value>) -> AppSettings {
    let mut settings = AppSettings::default();

    apply(source, "soundEffectsEnabled", |value| {
        settings.sound_effects = value
    });
    apply(source, "soundEffectsVolume", |value: f32| {
        settings.sound_effect_volume = (value / SWIFT_BASE_SOUND_EFFECT_VOLUME).clamp(0.0, 1.0)
    });
    apply(source, "showDockIcon", |value| {
        settings.show_dock_icon = value
    });
    apply(source, "doubleTapLockEnabled", |value| {
        settings.double_tap_lock = value
    });
    apply(source, "useDoubleTapOnly", |value| {
        settings.double_tap_only = value
    });
    apply(source, "superFastModeEnabled", |value: bool| {
        settings.release_microphone_while_idle = !value
    });
    if let Some(behavior) = field::<String>(source, "recordingAudioBehavior")
        .and_then(|value| recording_audio_behavior(&value))
    {
        settings.recording_audio_behavior = behavior;
    }
    if let Some(binding) = field::<SwiftHotkey>(source, "hotkey").and_then(convert_hotkey) {
        settings.dictation_hotkey = binding;
    }
    settings.paste_last_hotkey = match source.get("pasteLastTranscriptHotkey") {
        None | Some(Value::Null) => None,
        Some(value) => serde_json::from_value::<SwiftHotkey>(value.clone())
            .ok()
            .and_then(convert_hotkey)
            .map(Some)
            .unwrap_or_else(|| settings.paste_last_hotkey.clone()),
    };

    settings
}

fn apply<T: DeserializeOwned>(source: &Map<String, Value>, name: &str, apply: impl FnOnce(T)) {
    if let Some(value) = field(source, name) {
        apply(value);
    }
}

fn field<T: DeserializeOwned>(source: &Map<String, Value>, name: &str) -> Option<T> {
    serde_json::from_value(source.get(name)?.clone()).ok()
}

fn recording_audio_behavior(value: &str) -> Option<RecordingAudioBehavior> {
    match value {
        "mute" => Some(RecordingAudioBehavior::Mute),
        "pauseMedia" => Some(RecordingAudioBehavior::PauseMedia),
        "doNothing" => Some(RecordingAudioBehavior::DoNothing),
        _ => None,
    }
}

#[derive(Deserialize)]
struct SwiftHotkey {
    #[serde(default)]
    key: Option<String>,
    modifiers: SwiftModifiers,
}

#[derive(Deserialize)]
struct SwiftModifiers {
    modifiers: Vec<SwiftModifier>,
}

#[derive(Deserialize)]
#[serde(untagged)]
enum SwiftModifier {
    Current {
        kind: SwiftModifierKind,
        #[serde(default)]
        side: ModifierSide,
    },
    Legacy(SwiftModifierKind),
}

#[derive(Clone, Copy, Deserialize)]
#[serde(rename_all = "lowercase")]
enum SwiftModifierKind {
    Command,
    Option,
    Shift,
    Control,
    Fn,
}

impl SwiftModifier {
    fn parts(&self) -> (SwiftModifierKind, ModifierSide) {
        match *self {
            Self::Current { kind, side } => (kind, side),
            Self::Legacy(kind) => (kind, ModifierSide::Either),
        }
    }
}

fn convert_hotkey(hotkey: SwiftHotkey) -> Option<HotkeyBinding> {
    let mut modifiers = HotkeyModifiers::default();
    for modifier in hotkey.modifiers.modifiers {
        let (kind, side) = modifier.parts();
        let slot = match kind {
            SwiftModifierKind::Command => &mut modifiers.command,
            SwiftModifierKind::Option => &mut modifiers.option,
            SwiftModifierKind::Shift => &mut modifiers.shift,
            SwiftModifierKind::Control => &mut modifiers.control,
            SwiftModifierKind::Fn => {
                if modifiers.function {
                    return None;
                }
                modifiers.function = true;
                continue;
            }
        };
        if slot.replace(side).is_some() {
            return None;
        }
    }
    let key = match hotkey.key.as_deref() {
        Some(name) => Some(swift_key(name)?),
        None => None,
    };
    let binding = HotkeyBinding { modifiers, key };
    (!binding.is_empty()).then_some(binding)
}

fn swift_key(name: &str) -> Option<HotkeyKey> {
    if let Some(character) = swift_character(name) {
        return crate::keyboard::key_code_for(character)
            .ok()
            .map(|code| HotkeyKey {
                code,
                label: character.to_uppercase().collect(),
            });
    }
    let (code, label) = match name {
        "return" => (36, "Return"),
        "tab" => (48, "Tab"),
        "space" => (49, "Space"),
        "delete" => (51, "Delete"),
        "help" => (114, "Help"),
        "home" => (115, "Home"),
        "pageUp" => (116, "Page Up"),
        "forwardDelete" => (117, "Forward Delete"),
        "end" => (119, "End"),
        "pageDown" => (121, "Page Down"),
        "leftArrow" => (123, "Left"),
        "rightArrow" => (124, "Right"),
        "downArrow" => (125, "Down"),
        "upArrow" => (126, "Up"),
        "f1" => (122, "F1"),
        "f2" => (120, "F2"),
        "f3" => (99, "F3"),
        "f4" => (118, "F4"),
        "f5" => (96, "F5"),
        "f6" => (97, "F6"),
        "f7" => (98, "F7"),
        "f8" => (100, "F8"),
        "f9" => (101, "F9"),
        "f10" => (109, "F10"),
        "f11" => (103, "F11"),
        "f12" => (111, "F12"),
        "f13" => (105, "F13"),
        "f14" => (107, "F14"),
        "f15" => (113, "F15"),
        "f16" => (106, "F16"),
        "f17" => (64, "F17"),
        "f18" => (79, "F18"),
        "f19" => (80, "F19"),
        "f20" => (90, "F20"),
        "keypadDecimal" => (65, "Keypad ."),
        "keypadMultiply" => (67, "Keypad *"),
        "keypadPlus" => (69, "Keypad +"),
        "keypadClear" => (71, "Keypad Clear"),
        "keypadDivide" => (75, "Keypad /"),
        "keypadEnter" => (76, "Keypad Enter"),
        "keypadMinus" => (78, "Keypad -"),
        "keypadEquals" => (81, "Keypad ="),
        "keypadZero" => (82, "Keypad 0"),
        "keypadOne" => (83, "Keypad 1"),
        "keypadTwo" => (84, "Keypad 2"),
        "keypadThree" => (85, "Keypad 3"),
        "keypadFour" => (86, "Keypad 4"),
        "keypadFive" => (87, "Keypad 5"),
        "keypadSix" => (88, "Keypad 6"),
        "keypadSeven" => (89, "Keypad 7"),
        "keypadEight" => (91, "Keypad 8"),
        "keypadNine" => (92, "Keypad 9"),
        _ => return None,
    };
    Some(HotkeyKey {
        code,
        label: label.into(),
    })
}

fn swift_character(name: &str) -> Option<char> {
    if name.len() == 1 {
        return name
            .chars()
            .next()
            .filter(|character| character.is_ascii_alphabetic());
    }
    match name {
        "zero" => Some('0'),
        "one" => Some('1'),
        "two" => Some('2'),
        "three" => Some('3'),
        "four" => Some('4'),
        "five" => Some('5'),
        "six" => Some('6'),
        "seven" => Some('7'),
        "eight" => Some('8'),
        "nine" => Some('9'),
        "equal" => Some('='),
        "minus" => Some('-'),
        "rightBracket" => Some(']'),
        "leftBracket" => Some('['),
        "quote" => Some('\''),
        "semicolon" => Some(';'),
        "backslash" => Some('\\'),
        "comma" => Some(','),
        "slash" => Some('/'),
        "period" => Some('.'),
        "grave" => Some('`'),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use super::*;

    #[test]
    fn imports_allowlisted_preferences_and_side_aware_hotkeys() {
        let source = serde_json::from_str::<Value>(
            r#"{
                "soundEffectsEnabled": false,
                "soundEffectsVolume": 0.1,
                "showDockIcon": false,
                "recordingAudioBehavior": "pauseMedia",
                "superFastModeEnabled": false,
                "doubleTapLockEnabled": true,
                "useDoubleTapOnly": true,
                "hotkey": {
                    "key": "v",
                    "modifiers": {"modifiers": [
                        {"kind": "option", "side": "right"},
                        {"kind": "command", "side": "either"}
                    ]}
                },
                "pasteLastTranscriptHotkey": null
            }"#,
        )
        .unwrap();

        let settings = import_fields(source.as_object().unwrap());

        assert!(!settings.sound_effects);
        assert_eq!(settings.sound_effect_volume, 0.5);
        assert!(!settings.show_dock_icon);
        assert_eq!(
            settings.recording_audio_behavior,
            RecordingAudioBehavior::PauseMedia
        );
        assert!(settings.release_microphone_while_idle);
        assert!(settings.double_tap_lock);
        assert!(settings.double_tap_only);
        assert_eq!(
            settings.dictation_hotkey.modifiers.option,
            Some(ModifierSide::Right)
        );
        assert_eq!(
            settings.dictation_hotkey.modifiers.command,
            Some(ModifierSide::Either)
        );
        assert_eq!(settings.dictation_hotkey.key.as_ref().unwrap().label, "V");
        assert!(settings.paste_last_hotkey.is_none());
    }

    #[test]
    fn malformed_fields_do_not_block_other_preferences() {
        let source = serde_json::from_str::<Value>(
            r#"{
                "soundEffectsEnabled": "yes",
                "soundEffectsVolume": 8,
                "showDockIcon": false,
                "recordingAudioBehavior": "unknown",
                "hotkey": {"modifiers": {"modifiers": ["option"]}},
                "pasteLastTranscriptHotkey": {"key": 42}
            }"#,
        )
        .unwrap();

        let settings = import_fields(source.as_object().unwrap());

        assert!(settings.sound_effects);
        assert_eq!(settings.sound_effect_volume, 1.0);
        assert!(!settings.show_dock_icon);
        assert_eq!(
            settings.recording_audio_behavior,
            RecordingAudioBehavior::DoNothing
        );
        assert_eq!(
            settings.dictation_hotkey.modifiers.option,
            Some(ModifierSide::Either)
        );
        assert_eq!(
            settings.paste_last_hotkey,
            Some(HotkeyBinding::paste_last_default())
        );
    }

    #[test]
    fn unknown_shortcut_key_preserves_the_rust_default() {
        let source = serde_json::from_str::<Value>(
            r#"{
                "hotkey": {
                    "key": "escape",
                    "modifiers": {"modifiers": ["command"]}
                }
            }"#,
        )
        .unwrap();

        let settings = import_fields(source.as_object().unwrap());

        assert_eq!(settings.dictation_hotkey, HotkeyBinding::default());
    }

    #[test]
    fn omitted_paste_last_shortcut_imports_as_disabled() {
        let source = serde_json::from_str::<Value>(r#"{"soundEffectsEnabled":true}"#).unwrap();

        let settings = import_fields(source.as_object().unwrap());

        assert!(settings.paste_last_hotkey.is_none());
    }

    #[test]
    fn missing_source_is_not_an_import() {
        let path = PathBuf::from(format!(
            "/tmp/hex-missing-swift-settings-{}",
            std::process::id()
        ));

        assert!(import_from_path(&path).unwrap().is_none());
    }
}
