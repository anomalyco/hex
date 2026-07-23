use std::fs;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, AtomicU8, AtomicU64, Ordering};
use std::sync::{OnceLock, RwLock};

use color_eyre::eyre::{Result, eyre};
use objc2::MainThreadMarker;
use objc2_app_kit::{NSApplication, NSApplicationActivationPolicy};
use serde::{Deserialize, Serialize};

use crate::transcription_models::TranscriptionSelection;

static RECORDING_AUDIO_BEHAVIOR: AtomicU8 = AtomicU8::new(0);
static DOUBLE_TAP_LOCK: AtomicBool = AtomicBool::new(true);
static COMMANDS_ENABLED: AtomicBool = AtomicBool::new(false);
static CUSTOM_TRANSFORMATIONS_ENABLED: AtomicBool = AtomicBool::new(false);
static DICTATION_HOTKEY: AtomicU64 = AtomicU64::new(1 << 19);
static EDIT_HOTKEY: AtomicU64 = AtomicU64::new((1 << 19) | (1 << 20));
static HOTKEY_CAPTURE_ACTIVE: AtomicBool = AtomicBool::new(false);
static TRANSCRIPTION_SELECTION: OnceLock<RwLock<RuntimeTranscriptionSelection>> = OnceLock::new();
static MICROPHONE_SELECTION: OnceLock<RwLock<RuntimeMicrophoneSelection>> = OnceLock::new();
static VOICE_ACTION_SETTINGS: OnceLock<RwLock<VoiceActionSettings>> = OnceLock::new();

#[derive(Default)]
struct RuntimeTranscriptionSelection {
    revision: u64,
    selection: TranscriptionSelection,
}

#[derive(Default)]
struct RuntimeMicrophoneSelection {
    revision: u64,
    device: Option<String>,
}

const KEY_CODE_MASK: u64 = u16::MAX as u64;
const KEY_CODE_PRESENT: u64 = 1 << 63;
const SHIFT_KEY_MASK: u64 = 1 << 17;
const CONTROL_KEY_MASK: u64 = 1 << 18;
const OPTION_KEY_MASK: u64 = 1 << 19;
const COMMAND_KEY_MASK: u64 = 1 << 20;
const FUNCTION_KEY_MASK: u64 = 1 << 23;
const HOTKEY_MODIFIERS_MASK: u64 =
    SHIFT_KEY_MASK | CONTROL_KEY_MASK | OPTION_KEY_MASK | COMMAND_KEY_MASK | FUNCTION_KEY_MASK;

#[derive(Clone, Copy, Debug, Default, Deserialize, Eq, PartialEq, Serialize)]
#[serde(default)]
pub struct HotkeyModifiers {
    pub control: bool,
    pub option: bool,
    pub shift: bool,
    pub command: bool,
    pub function: bool,
}

impl HotkeyModifiers {
    pub const fn option() -> Self {
        Self {
            option: true,
            control: false,
            shift: false,
            command: false,
            function: false,
        }
    }

    pub const fn option_command() -> Self {
        Self {
            option: true,
            command: true,
            control: false,
            shift: false,
            function: false,
        }
    }

    pub const fn is_empty(self) -> bool {
        !self.control && !self.option && !self.shift && !self.command && !self.function
    }

    pub const fn count(self) -> u8 {
        self.control as u8
            + self.option as u8
            + self.shift as u8
            + self.command as u8
            + self.function as u8
    }

    const fn contains(self, required: Self) -> bool {
        (!required.control || self.control)
            && (!required.option || self.option)
            && (!required.shift || self.shift)
            && (!required.command || self.command)
            && (!required.function || self.function)
    }

    fn event_flags(self) -> u64 {
        (self.control as u64 * CONTROL_KEY_MASK)
            | (self.option as u64 * OPTION_KEY_MASK)
            | (self.shift as u64 * SHIFT_KEY_MASK)
            | (self.command as u64 * COMMAND_KEY_MASK)
            | (self.function as u64 * FUNCTION_KEY_MASK)
    }

    fn keycaps(self) -> impl Iterator<Item = &'static str> {
        [
            (self.function, "fn"),
            (self.control, "⌃"),
            (self.option, "⌥"),
            (self.shift, "⇧"),
            (self.command, "⌘"),
        ]
        .into_iter()
        .filter_map(|(enabled, label)| enabled.then_some(label))
    }
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct HotkeyKey {
    pub code: u16,
    pub label: String,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(default)]
pub struct HotkeyBinding {
    pub modifiers: HotkeyModifiers,
    pub key: Option<HotkeyKey>,
}

impl Default for HotkeyBinding {
    fn default() -> Self {
        Self {
            modifiers: HotkeyModifiers::option(),
            key: None,
        }
    }
}

impl HotkeyBinding {
    pub fn edit_default() -> Self {
        Self {
            modifiers: HotkeyModifiers::option_command(),
            key: None,
        }
    }
}

impl HotkeyBinding {
    pub fn keycaps(&self) -> Vec<String> {
        self.modifiers
            .keycaps()
            .map(str::to_string)
            .chain(self.key.iter().map(|key| key.label.clone()))
            .collect()
    }

    pub fn is_empty(&self) -> bool {
        self.modifiers.is_empty() && self.key.is_none()
    }

    pub fn conflicts_with_paste(&self, paste_key_code: u16) -> bool {
        self.key
            .as_ref()
            .is_some_and(|key| key.code == paste_key_code)
            && !self.modifiers.command
            && self.modifiers.option
            && ((self.modifiers.shift && !self.modifiers.control)
                || (crate::DEVELOPER_FEATURES_ENABLED
                    && self.modifiers.control
                    && !self.modifiers.shift))
    }

    pub fn is_modifier_prefix_of(&self, other: &Self) -> bool {
        self.key.is_none()
            && other.modifiers.contains(self.modifiers)
            && (self.modifiers != other.modifiers || other.key.is_some())
    }

    fn encoded(&self) -> u64 {
        let key = self
            .key
            .as_ref()
            .map_or(0, |key| KEY_CODE_PRESENT | u64::from(key.code));
        self.modifiers.event_flags() | key
    }
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct RuntimeHotkey {
    pub modifiers: u64,
    pub key_code: Option<u16>,
}

impl RuntimeHotkey {
    pub fn is_empty(self) -> bool {
        self.modifiers == 0 && self.key_code.is_none()
    }

    pub fn exact_modifiers(self, flags: u64) -> bool {
        flags & HOTKEY_MODIFIERS_MASK == self.modifiers
    }

    pub fn required_modifiers_down(self, flags: u64) -> bool {
        flags & self.modifiers == self.modifiers
    }

    pub fn matches_key_press(self, code: u16, flags: u64) -> bool {
        self.key_code == Some(code) && self.exact_modifiers(flags)
    }
}

#[derive(Clone, Copy, Debug, Default, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum RecordingAudioBehavior {
    Mute,
    PauseMedia,
    #[default]
    DoNothing,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(default)]
pub struct DictationProcessingSettings {
    pub default_mode: DictationMode,
    pub modes: Vec<DictationMode>,
}

impl Default for DictationProcessingSettings {
    fn default() -> Self {
        Self {
            default_mode: DictationMode {
                name: "Global".into(),
                ..Default::default()
            },
            modes: Vec::new(),
        }
    }
}

#[derive(Clone, Debug, Default, Deserialize, Serialize)]
#[serde(default)]
pub struct DictationMode {
    pub name: String,
    pub applications: Vec<String>,
    pub browser_hosts: Vec<String>,
    pub replacements: Vec<TextReplacement>,
    pub transformations: Vec<String>,
    pub post_processing: DictationPostProcessing,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(default)]
pub struct DictationPostProcessing {
    pub enabled: bool,
    pub prompt: String,
    pub model: Option<String>,
    pub variant: Option<String>,
    pub deadline_seconds: u64,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(default)]
pub struct VoiceActionSettings {
    pub model: Option<String>,
    pub variant: Option<String>,
    pub deadline_seconds: u64,
}

impl Default for VoiceActionSettings {
    fn default() -> Self {
        Self {
            model: None,
            variant: None,
            deadline_seconds: 30,
        }
    }
}

#[derive(Clone, Debug, Default, Deserialize, Eq, PartialEq, Serialize)]
#[serde(default)]
pub struct TextReplacement {
    pub matched_phrase: String,
    pub output: String,
}

impl Default for DictationPostProcessing {
    fn default() -> Self {
        Self {
            enabled: false,
            prompt: "Rewrite the transcript into clear, natural text while preserving the speaker's intended meaning.".into(),
            model: None,
            variant: None,
            deadline_seconds: 30,
        }
    }
}

impl RecordingAudioBehavior {
    pub const ALL: [Self; 3] = [Self::DoNothing, Self::Mute, Self::PauseMedia];

    pub const fn label(self) -> &'static str {
        match self {
            Self::Mute => "Mute",
            Self::PauseMedia => "Pause media",
            Self::DoNothing => "Do nothing",
        }
    }

    const fn encoded(self) -> u8 {
        match self {
            Self::Mute => 0,
            Self::PauseMedia => 1,
            Self::DoNothing => 2,
        }
    }

    fn decode(value: u8) -> Self {
        match value {
            1 => Self::PauseMedia,
            2 => Self::DoNothing,
            _ => Self::Mute,
        }
    }
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(default)]
pub struct AppSettings {
    pub commands_enabled: bool,
    pub sound_effects: bool,
    pub sound_effect_volume: f32,
    pub microphone: Option<String>,
    pub recording_audio_behavior: RecordingAudioBehavior,
    pub double_tap_lock: bool,
    pub dictation_hotkey: HotkeyBinding,
    #[serde(
        default = "HotkeyBinding::edit_default",
        deserialize_with = "deserialize_edit_hotkey"
    )]
    pub edit_hotkey: HotkeyBinding,
    pub show_dock_icon: bool,
    pub transcription: TranscriptionSelection,
    pub dictation_processing: DictationProcessingSettings,
    pub voice_action: VoiceActionSettings,
    #[serde(skip_serializing)]
    pub text_replacements: Vec<TextReplacement>,
}

fn deserialize_edit_hotkey<'de, D>(deserializer: D) -> std::result::Result<HotkeyBinding, D::Error>
where
    D: serde::Deserializer<'de>,
{
    Ok(Option::<HotkeyBinding>::deserialize(deserializer)?
        .unwrap_or_else(HotkeyBinding::edit_default))
}

impl Default for AppSettings {
    fn default() -> Self {
        Self {
            commands_enabled: false,
            sound_effects: true,
            sound_effect_volume: 0.5,
            microphone: None,
            recording_audio_behavior: RecordingAudioBehavior::DoNothing,
            double_tap_lock: true,
            dictation_hotkey: HotkeyBinding::default(),
            edit_hotkey: HotkeyBinding::edit_default(),
            show_dock_icon: false,
            transcription: TranscriptionSelection::default(),
            dictation_processing: DictationProcessingSettings::default(),
            voice_action: VoiceActionSettings::default(),
            text_replacements: Vec::new(),
        }
    }
}

impl AppSettings {
    pub fn load() -> Result<Self> {
        let path = path()?;
        match fs::read(path) {
            Ok(data) => {
                let mut settings: Self = serde_json::from_slice(&data)?;
                settings.dictation_processing.default_mode.name = "Global".into();
                settings.migrate_legacy_replacements();
                crate::transcription_models::validate(&settings.transcription)?;
                settings.repair_hotkey_conflict();
                settings.apply_runtime();
                Ok(settings)
            }
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                let settings = Self::default();
                settings.apply_runtime();
                Ok(settings)
            }
            Err(error) => Err(error.into()),
        }
    }

    pub fn save(&self) -> Result<()> {
        let path = path()?;
        let parent = path
            .parent()
            .ok_or_else(|| eyre!("settings path has no parent"))?;
        fs::create_dir_all(parent)?;
        let temporary = path.with_extension("json.tmp");
        fs::write(&temporary, serde_json::to_vec_pretty(self)?)?;
        fs::rename(temporary, path)?;
        self.apply_runtime();
        Ok(())
    }

    fn apply_runtime(&self) {
        COMMANDS_ENABLED.store(self.commands_enabled, Ordering::Release);
        CUSTOM_TRANSFORMATIONS_ENABLED.store(
            !self
                .dictation_processing
                .default_mode
                .transformations
                .is_empty()
                || self
                    .dictation_processing
                    .modes
                    .iter()
                    .any(|mode| !mode.transformations.is_empty()),
            Ordering::Release,
        );
        crate::feedback::set_enabled(self.sound_effects);
        crate::feedback::set_volume(self.sound_effect_volume.clamp(0.0, 1.0));
        RECORDING_AUDIO_BEHAVIOR.store(self.recording_audio_behavior.encoded(), Ordering::Relaxed);
        DOUBLE_TAP_LOCK.store(self.double_tap_lock, Ordering::Relaxed);
        DICTATION_HOTKEY.store(self.dictation_hotkey.encoded(), Ordering::Release);
        EDIT_HOTKEY.store(self.edit_hotkey.encoded(), Ordering::Release);
        set_transcription_selection(&self.transcription);
        set_microphone_selection(self.microphone.as_deref());
        crate::config::update_dictation_profiles(&self.dictation_processing);
        *VOICE_ACTION_SETTINGS
            .get_or_init(Default::default)
            .write()
            .unwrap_or_else(|error| error.into_inner()) = self.voice_action.clone();
    }

    fn migrate_legacy_replacements(&mut self) {
        if self.text_replacements.is_empty() {
            return;
        }
        if self
            .dictation_processing
            .default_mode
            .replacements
            .is_empty()
        {
            self.dictation_processing.default_mode.replacements = self.text_replacements.clone();
        }
        for mode in &mut self.dictation_processing.modes {
            if mode.replacements.is_empty() {
                mode.replacements = self.text_replacements.clone();
            }
        }
        self.text_replacements.clear();
    }

    fn repair_hotkey_conflict(&mut self) {
        if !hotkeys_conflict(&self.dictation_hotkey, &self.edit_hotkey) {
            return;
        }
        let edit_with_key = crate::keyboard::key_code_for('e')
            .ok()
            .map(|code| HotkeyBinding {
                modifiers: HotkeyModifiers::option_command(),
                key: Some(HotkeyKey {
                    code,
                    label: "E".into(),
                }),
            });
        let control_command = HotkeyBinding {
            modifiers: HotkeyModifiers {
                control: true,
                command: true,
                ..Default::default()
            },
            key: None,
        };
        if let Some(binding) = [
            Some(HotkeyBinding::edit_default()),
            edit_with_key,
            Some(control_command),
        ]
        .into_iter()
        .flatten()
        .find(|binding| !hotkeys_conflict(&self.dictation_hotkey, binding))
        {
            tracing::warn!("replaced a conflicting Voice Action shortcut");
            self.edit_hotkey = binding;
        }
    }
}

pub fn hotkeys_conflict(dictation: &HotkeyBinding, edit: &HotkeyBinding) -> bool {
    dictation == edit || edit.is_modifier_prefix_of(dictation)
}

fn set_microphone_selection(device: Option<&str>) {
    let device = device
        .map(str::trim)
        .filter(|device| !device.is_empty())
        .map(str::to_string);
    let state = MICROPHONE_SELECTION.get_or_init(Default::default);
    let mut state = state.write().unwrap_or_else(|error| error.into_inner());
    if state.device != device {
        state.revision = state.revision.wrapping_add(1);
        state.device = device;
    }
}

pub fn microphone_selection() -> (u64, Option<String>) {
    let state = MICROPHONE_SELECTION.get_or_init(Default::default);
    let state = state.read().unwrap_or_else(|error| error.into_inner());
    (state.revision, state.device.clone())
}

fn set_transcription_selection(selection: &TranscriptionSelection) {
    let state = TRANSCRIPTION_SELECTION.get_or_init(Default::default);
    let mut state = state.write().unwrap_or_else(|error| error.into_inner());
    if state.selection != *selection {
        state.revision = state.revision.wrapping_add(1);
        state.selection = selection.clone();
    }
}

pub fn transcription_selection() -> (u64, TranscriptionSelection) {
    let state = TRANSCRIPTION_SELECTION.get_or_init(Default::default);
    let state = state.read().unwrap_or_else(|error| error.into_inner());
    (state.revision, state.selection.clone())
}

pub fn recording_audio_behavior() -> RecordingAudioBehavior {
    RecordingAudioBehavior::decode(RECORDING_AUDIO_BEHAVIOR.load(Ordering::Relaxed))
}

pub fn commands_enabled() -> bool {
    COMMANDS_ENABLED.load(Ordering::Acquire)
}

pub fn custom_transformations_enabled() -> bool {
    CUSTOM_TRANSFORMATIONS_ENABLED.load(Ordering::Acquire)
}

pub fn double_tap_lock() -> bool {
    DOUBLE_TAP_LOCK.load(Ordering::Relaxed)
}

pub fn dictation_hotkey() -> RuntimeHotkey {
    decode_hotkey(DICTATION_HOTKEY.load(Ordering::Acquire))
}

pub fn edit_hotkey() -> RuntimeHotkey {
    decode_hotkey(EDIT_HOTKEY.load(Ordering::Acquire))
}

pub fn voice_action_settings() -> VoiceActionSettings {
    VOICE_ACTION_SETTINGS
        .get_or_init(Default::default)
        .read()
        .unwrap_or_else(|error| error.into_inner())
        .clone()
}

fn decode_hotkey(encoded: u64) -> RuntimeHotkey {
    RuntimeHotkey {
        modifiers: encoded & HOTKEY_MODIFIERS_MASK,
        key_code: (encoded & KEY_CODE_PRESENT != 0).then_some((encoded & KEY_CODE_MASK) as u16),
    }
}

pub fn hotkey_capture_active() -> bool {
    HOTKEY_CAPTURE_ACTIVE.load(Ordering::Acquire)
}

pub fn set_hotkey_capture_active(active: bool) {
    HOTKEY_CAPTURE_ACTIVE.store(active, Ordering::Release);
}

pub fn set_dock_icon_visible(visible: bool) {
    let Some(mtm) = MainThreadMarker::new() else {
        tracing::warn!("Dock icon visibility must be changed on the main thread");
        return;
    };
    let application = NSApplication::sharedApplication(mtm);
    let policy = if visible {
        NSApplicationActivationPolicy::Regular
    } else {
        NSApplicationActivationPolicy::Accessory
    };
    if !application.setActivationPolicy(policy) {
        tracing::warn!(visible, "could not change Dock icon visibility");
    }
}

pub fn hide_application() {
    let Some(marker) = MainThreadMarker::new() else {
        tracing::warn!("cannot hide HEX outside the main thread");
        return;
    };
    NSApplication::sharedApplication(marker).hide(None);
}

fn path() -> Result<PathBuf> {
    Ok(crate::app_paths::support_dir()?.join("settings.json"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn missing_fields_receive_defaults() {
        let settings: AppSettings = serde_json::from_str("{}").unwrap();
        assert!(!settings.commands_enabled);
        assert!(settings.sound_effects);
        assert_eq!(settings.sound_effect_volume, 0.5);
        assert_eq!(settings.microphone, None);
        assert_eq!(
            settings.recording_audio_behavior,
            RecordingAudioBehavior::DoNothing
        );
        assert!(settings.double_tap_lock);
        assert_eq!(settings.dictation_hotkey, HotkeyBinding::default());
        assert_eq!(settings.edit_hotkey, HotkeyBinding::edit_default());
        assert!(!settings.show_dock_icon);
        assert_eq!(settings.transcription, TranscriptionSelection::default());
        assert!(
            !settings
                .dictation_processing
                .default_mode
                .post_processing
                .enabled
        );
        assert!(settings.voice_action.model.is_none());
        assert!(settings.voice_action.variant.is_none());
        assert_eq!(settings.voice_action.deadline_seconds, 30);
        assert!(settings.text_replacements.is_empty());
    }

    #[test]
    fn legacy_sleep_prevention_setting_is_ignored() {
        let settings: AppSettings =
            serde_json::from_str(r#"{"prevent_system_sleep":false}"#).unwrap();
        let serialized = serde_json::to_value(settings).unwrap();

        assert!(serialized.get("prevent_system_sleep").is_none());
    }

    #[test]
    fn legacy_disabled_edit_shortcut_migrates_to_the_new_default() {
        let settings: AppSettings = serde_json::from_str(r#"{"edit_hotkey":null}"#).unwrap();

        assert_eq!(settings.edit_hotkey, HotkeyBinding::edit_default());
    }

    #[test]
    fn transcription_selection_round_trips() {
        let settings = AppSettings {
            transcription: TranscriptionSelection {
                model: crate::transcription_models::TranscriptionModelId::WhisperLargeV3Turbo,
                language: "zh".into(),
                recognition_hints: "OpenCode, Effect".into(),
            },
            ..AppSettings::default()
        };

        let encoded = serde_json::to_string(&settings).unwrap();
        let decoded: AppSettings = serde_json::from_str(&encoded).unwrap();

        assert_eq!(decoded.transcription, settings.transcription);
        assert!(crate::transcription_models::validate(&decoded.transcription).is_ok());
    }

    #[test]
    fn voice_action_settings_round_trip() {
        let settings = AppSettings {
            voice_action: VoiceActionSettings {
                model: Some("openai/gpt-5.6-sol".into()),
                variant: Some("fast".into()),
                deadline_seconds: 12,
            },
            ..AppSettings::default()
        };

        let encoded = serde_json::to_string(&settings).unwrap();
        let decoded: AppSettings = serde_json::from_str(&encoded).unwrap();

        assert_eq!(decoded.voice_action.model, settings.voice_action.model);
        assert_eq!(decoded.voice_action.variant, settings.voice_action.variant);
        assert_eq!(decoded.voice_action.deadline_seconds, 12);
    }

    #[test]
    fn legacy_global_replacements_migrate_into_every_mode() {
        let mut settings: AppSettings = serde_json::from_str(
            r#"{"text_replacements":[{"matched_phrase":"open code","output":"OpenCode"}]}"#,
        )
        .unwrap();
        settings.dictation_processing.modes.push(DictationMode {
            name: "Messages".into(),
            ..DictationMode::default()
        });
        settings.migrate_legacy_replacements();

        assert_eq!(
            settings.dictation_processing.default_mode.replacements,
            [TextReplacement {
                matched_phrase: "open code".into(),
                output: "OpenCode".into(),
            }]
        );
        assert_eq!(
            settings.dictation_processing.modes[0].replacements,
            settings.dictation_processing.default_mode.replacements
        );
        assert!(settings.text_replacements.is_empty());
        assert!(
            !serde_json::to_string(&settings)
                .unwrap()
                .contains("text_replacements")
        );
    }

    #[test]
    fn hotkey_runtime_encoding_preserves_modifiers_and_key_code() {
        let binding = HotkeyBinding {
            modifiers: HotkeyModifiers {
                control: true,
                option: false,
                shift: true,
                command: false,
                function: true,
            },
            key: Some(HotkeyKey {
                code: 49,
                label: "Space".into(),
            }),
        };
        DICTATION_HOTKEY.store(binding.encoded(), Ordering::Release);

        let runtime = dictation_hotkey();

        assert_eq!(
            runtime.modifiers,
            CONTROL_KEY_MASK | SHIFT_KEY_MASK | FUNCTION_KEY_MASK
        );
        assert_eq!(runtime.key_code, Some(49));
        DICTATION_HOTKEY.store(HotkeyBinding::default().encoded(), Ordering::Release);
    }

    #[test]
    fn meeting_and_last_transcript_paste_shortcuts_are_reserved() {
        let paste_key = HotkeyKey {
            code: 47,
            label: "V".into(),
        };
        for modifiers in [
            HotkeyModifiers {
                option: true,
                shift: true,
                ..Default::default()
            },
            HotkeyModifiers {
                option: true,
                control: true,
                ..Default::default()
            },
        ] {
            assert!(
                HotkeyBinding {
                    modifiers,
                    key: Some(paste_key.clone()),
                }
                .conflicts_with_paste(47)
            );
        }
    }

    #[test]
    fn edit_shortcut_cannot_shadow_the_dictation_shortcut() {
        let option = HotkeyBinding::default();
        let option_command = HotkeyBinding::edit_default();

        assert!(option.is_modifier_prefix_of(&option_command));
        assert!(!option_command.is_modifier_prefix_of(&option));
    }

    #[test]
    fn old_dictation_binding_cannot_be_taken_over_by_the_new_edit_default() {
        let mut settings = AppSettings {
            dictation_hotkey: HotkeyBinding::edit_default(),
            edit_hotkey: HotkeyBinding::edit_default(),
            ..Default::default()
        };

        settings.repair_hotkey_conflict();

        assert!(!hotkeys_conflict(
            &settings.dictation_hotkey,
            &settings.edit_hotkey
        ));
    }
}
