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
static DOUBLE_TAP_ONLY: AtomicBool = AtomicBool::new(false);
static MICROPHONE_POLICY: AtomicU8 = AtomicU8::new(0);
static CUSTOM_TRANSFORMATIONS_ENABLED: AtomicBool = AtomicBool::new(false);
static HOTKEYS: OnceLock<RwLock<RuntimeHotkeys>> = OnceLock::new();
static PASTE_KEY_CODE: OnceLock<u16> = OnceLock::new();
static HOTKEY_CAPTURE_ACTIVE: AtomicBool = AtomicBool::new(false);
static SETTINGS_TEMP_SEQUENCE: AtomicU64 = AtomicU64::new(0);
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

pub const SHIFT_KEY_MASK: u64 = 1 << 17;
pub const CONTROL_KEY_MASK: u64 = 1 << 18;
pub const OPTION_KEY_MASK: u64 = 1 << 19;
pub const COMMAND_KEY_MASK: u64 = 1 << 20;
pub const FUNCTION_KEY_MASK: u64 = 1 << 23;
pub const HOTKEY_MODIFIERS_MASK: u64 =
    SHIFT_KEY_MASK | CONTROL_KEY_MASK | OPTION_KEY_MASK | COMMAND_KEY_MASK | FUNCTION_KEY_MASK;
pub const LEFT_CONTROL_MASK: u64 = 0x0000_0001;
pub const LEFT_SHIFT_MASK: u64 = 0x0000_0002;
pub const RIGHT_SHIFT_MASK: u64 = 0x0000_0004;
pub const LEFT_COMMAND_MASK: u64 = 0x0000_0008;
pub const RIGHT_COMMAND_MASK: u64 = 0x0000_0010;
pub const LEFT_OPTION_MASK: u64 = 0x0000_0020;
pub const RIGHT_OPTION_MASK: u64 = 0x0000_0040;
pub const RIGHT_CONTROL_MASK: u64 = 0x0000_2000;

#[derive(Clone, Copy, Debug, Default, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ModifierSide {
    Left,
    Right,
    #[default]
    Either,
}

#[derive(Clone, Copy, Debug, Default, Deserialize, Eq, PartialEq, Serialize)]
#[serde(default)]
pub struct HotkeyModifiers {
    #[serde(deserialize_with = "deserialize_modifier_side")]
    pub control: Option<ModifierSide>,
    #[serde(deserialize_with = "deserialize_modifier_side")]
    pub option: Option<ModifierSide>,
    #[serde(deserialize_with = "deserialize_modifier_side")]
    pub shift: Option<ModifierSide>,
    #[serde(deserialize_with = "deserialize_modifier_side")]
    pub command: Option<ModifierSide>,
    pub function: bool,
}

fn deserialize_modifier_side<'de, D>(
    deserializer: D,
) -> std::result::Result<Option<ModifierSide>, D::Error>
where
    D: serde::Deserializer<'de>,
{
    use serde::de::Error;
    let value = Option::<serde_json::Value>::deserialize(deserializer)?;
    match value {
        None | Some(serde_json::Value::Bool(false)) => Ok(None),
        Some(serde_json::Value::Bool(true)) => Ok(Some(ModifierSide::Either)),
        Some(serde_json::Value::String(value)) => {
            serde_json::from_value(serde_json::Value::String(value))
                .map(Some)
                .map_err(D::Error::custom)
        }
        Some(_) => Err(D::Error::custom(
            "modifier side must be left, right, either, or null",
        )),
    }
}

impl HotkeyModifiers {
    pub const fn option() -> Self {
        Self {
            option: Some(ModifierSide::Either),
            control: None,
            shift: None,
            command: None,
            function: false,
        }
    }

    pub const fn option_command() -> Self {
        Self {
            option: Some(ModifierSide::Either),
            command: Some(ModifierSide::Either),
            control: None,
            shift: None,
            function: false,
        }
    }

    pub const fn is_empty(self) -> bool {
        self.control.is_none()
            && self.option.is_none()
            && self.shift.is_none()
            && self.command.is_none()
            && !self.function
    }

    pub const fn count(self) -> u8 {
        self.control.is_some() as u8
            + self.option.is_some() as u8
            + self.shift.is_some() as u8
            + self.command.is_some() as u8
            + self.function as u8
    }

    pub const fn without_side_constraints(self) -> Self {
        Self {
            control: either_side(self.control),
            option: either_side(self.option),
            shift: either_side(self.shift),
            command: either_side(self.command),
            function: self.function,
        }
    }

    fn contains(self, required: Self) -> bool {
        modifier_contains(self.control, required.control)
            && modifier_contains(self.option, required.option)
            && modifier_contains(self.shift, required.shift)
            && modifier_contains(self.command, required.command)
            && (!required.function || self.function)
    }

    fn keycaps(self) -> impl Iterator<Item = &'static str> {
        [
            (self.function, "fn"),
            (self.control.is_some(), side_keycap(self.control, "⌃")),
            (self.option.is_some(), side_keycap(self.option, "⌥")),
            (self.shift.is_some(), side_keycap(self.shift, "⇧")),
            (self.command.is_some(), side_keycap(self.command, "⌘")),
        ]
        .into_iter()
        .filter_map(|(enabled, label)| enabled.then_some(label))
    }
}

const fn either_side(side: Option<ModifierSide>) -> Option<ModifierSide> {
    match side {
        Some(_) => Some(ModifierSide::Either),
        None => None,
    }
}

fn modifier_contains(candidate: Option<ModifierSide>, required: Option<ModifierSide>) -> bool {
    match (candidate, required) {
        (_, None) => true,
        (Some(ModifierSide::Either), Some(_)) | (Some(_), Some(ModifierSide::Either)) => true,
        (Some(candidate), Some(required)) => candidate == required,
        (None, Some(_)) => false,
    }
}

fn side_keycap(side: Option<ModifierSide>, symbol: &'static str) -> &'static str {
    match (side, symbol) {
        (Some(ModifierSide::Left), "⌃") => "L⌃",
        (Some(ModifierSide::Right), "⌃") => "R⌃",
        (Some(ModifierSide::Left), "⌥") => "L⌥",
        (Some(ModifierSide::Right), "⌥") => "R⌥",
        (Some(ModifierSide::Left), "⇧") => "L⇧",
        (Some(ModifierSide::Right), "⇧") => "R⇧",
        (Some(ModifierSide::Left), "⌘") => "L⌘",
        (Some(ModifierSide::Right), "⌘") => "R⌘",
        _ => symbol,
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

    pub fn paste_last_default() -> Self {
        Self {
            modifiers: HotkeyModifiers {
                option: Some(ModifierSide::Either),
                shift: Some(ModifierSide::Either),
                ..Default::default()
            },
            key: Some(HotkeyKey {
                code: paste_key_code(),
                label: "V".into(),
            }),
        }
    }

    pub fn paste_meeting_default() -> Self {
        Self {
            modifiers: HotkeyModifiers {
                option: Some(ModifierSide::Either),
                control: Some(ModifierSide::Either),
                ..Default::default()
            },
            key: Some(HotkeyKey {
                code: paste_key_code(),
                label: "V".into(),
            }),
        }
    }
}

fn paste_key_code() -> u16 {
    *PASTE_KEY_CODE.get_or_init(|| crate::keyboard::key_code_for('v').unwrap_or(9))
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

    pub fn runtime(&self) -> RuntimeHotkey {
        RuntimeHotkey {
            modifiers: self.modifiers,
            key_code: self.key.as_ref().map(|key| key.code),
        }
    }

    #[cfg(test)]
    pub fn conflicts_with_paste(&self, paste_key_code: u16) -> bool {
        self.key
            .as_ref()
            .is_some_and(|key| key.code == paste_key_code)
            && self.modifiers.command.is_none()
            && self.modifiers.option.is_some()
            && ((self.modifiers.shift.is_some() && self.modifiers.control.is_none())
                || (crate::DEVELOPER_FEATURES_ENABLED
                    && self.modifiers.control.is_some()
                    && self.modifiers.shift.is_none()))
    }

    #[cfg(test)]
    pub fn is_modifier_prefix_of(&self, other: &Self) -> bool {
        self.key.is_none()
            && other.modifiers.contains(self.modifiers)
            && (self.modifiers != other.modifiers || other.key.is_some())
    }

    pub fn overlaps(&self, other: &Self) -> bool {
        self.key.as_ref().map(|key| key.code) == other.key.as_ref().map(|key| key.code)
            && modifiers_overlap(self.modifiers, other.modifiers)
    }
}

fn modifiers_overlap(left: HotkeyModifiers, right: HotkeyModifiers) -> bool {
    modifier_overlap(left.control, right.control)
        && modifier_overlap(left.option, right.option)
        && modifier_overlap(left.shift, right.shift)
        && modifier_overlap(left.command, right.command)
        && left.function == right.function
}

fn modifier_overlap(left: Option<ModifierSide>, right: Option<ModifierSide>) -> bool {
    match (left, right) {
        (None, None) => true,
        (Some(ModifierSide::Either), Some(_)) | (Some(_), Some(ModifierSide::Either)) => true,
        (Some(left), Some(right)) => left == right,
        _ => false,
    }
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct RuntimeHotkey {
    pub modifiers: HotkeyModifiers,
    pub key_code: Option<u16>,
}

impl RuntimeHotkey {
    pub fn is_empty(self) -> bool {
        self.modifiers.is_empty() && self.key_code.is_none()
    }

    pub fn exact_modifiers(self, flags: u64) -> bool {
        modifiers_from_flags(flags).matches_exactly(self.modifiers)
    }

    pub fn required_modifiers_down(self, flags: u64) -> bool {
        modifiers_from_flags(flags).contains(self.modifiers)
    }

    pub fn matches_key_press(self, code: u16, flags: u64) -> bool {
        self.key_code == Some(code) && self.exact_modifiers(flags)
    }
}

impl HotkeyModifiers {
    pub fn matches_exactly(self, expected: Self) -> bool {
        modifiers_overlap(self, expected)
            && self.control.is_some() == expected.control.is_some()
            && self.option.is_some() == expected.option.is_some()
            && self.shift.is_some() == expected.shift.is_some()
            && self.command.is_some() == expected.command.is_some()
            && self.function == expected.function
    }
}

pub fn modifiers_from_flags(flags: u64) -> HotkeyModifiers {
    HotkeyModifiers {
        control: side_from_flags(
            flags,
            CONTROL_KEY_MASK,
            LEFT_CONTROL_MASK,
            RIGHT_CONTROL_MASK,
        ),
        option: side_from_flags(flags, OPTION_KEY_MASK, LEFT_OPTION_MASK, RIGHT_OPTION_MASK),
        shift: side_from_flags(flags, SHIFT_KEY_MASK, LEFT_SHIFT_MASK, RIGHT_SHIFT_MASK),
        command: side_from_flags(
            flags,
            COMMAND_KEY_MASK,
            LEFT_COMMAND_MASK,
            RIGHT_COMMAND_MASK,
        ),
        function: flags & FUNCTION_KEY_MASK != 0,
    }
}

fn side_from_flags(flags: u64, general: u64, left: u64, right: u64) -> Option<ModifierSide> {
    match (flags & left != 0, flags & right != 0) {
        (true, false) => Some(ModifierSide::Left),
        (false, true) => Some(ModifierSide::Right),
        (true, true) | (false, false) if flags & general != 0 => Some(ModifierSide::Either),
        _ => None,
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct RuntimeHotkeys {
    pub dictation: RuntimeHotkey,
    pub edit: RuntimeHotkey,
    pub paste_last: Option<RuntimeHotkey>,
    pub paste_meeting: Option<RuntimeHotkey>,
}

impl Default for RuntimeHotkeys {
    fn default() -> Self {
        Self {
            dictation: HotkeyBinding::default().runtime(),
            edit: HotkeyBinding::edit_default().runtime(),
            paste_last: Some(HotkeyBinding::paste_last_default().runtime()),
            paste_meeting: crate::DEVELOPER_FEATURES_ENABLED
                .then(|| HotkeyBinding::paste_meeting_default().runtime()),
        }
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
            deadline_seconds: 60,
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
    pub release_microphone_while_idle: bool,
    pub sound_effects: bool,
    pub sound_effect_volume: f32,
    pub microphone: Option<String>,
    pub recording_audio_behavior: RecordingAudioBehavior,
    pub double_tap_lock: bool,
    pub double_tap_only: bool,
    pub dictation_hotkey: HotkeyBinding,
    #[serde(
        default = "HotkeyBinding::edit_default",
        deserialize_with = "deserialize_edit_hotkey"
    )]
    pub edit_hotkey: HotkeyBinding,
    pub paste_last_hotkey: Option<HotkeyBinding>,
    pub show_dock_icon: bool,
    pub transcription: TranscriptionSelection,
    pub dictation_processing: DictationProcessingSettings,
    pub voice_action: VoiceActionSettings,
    pub history_retention: crate::history::HistoryRetention,
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
            release_microphone_while_idle: false,
            sound_effects: true,
            sound_effect_volume: 0.5,
            microphone: None,
            recording_audio_behavior: RecordingAudioBehavior::DoNothing,
            double_tap_lock: true,
            double_tap_only: false,
            dictation_hotkey: HotkeyBinding::default(),
            edit_hotkey: HotkeyBinding::edit_default(),
            paste_last_hotkey: Some(HotkeyBinding::paste_last_default()),
            show_dock_icon: true,
            transcription: TranscriptionSelection::default(),
            dictation_processing: DictationProcessingSettings::default(),
            voice_action: VoiceActionSettings::default(),
            history_retention: crate::history::HistoryRetention::default(),
            text_replacements: Vec::new(),
        }
    }
}

impl AppSettings {
    fn normalize_microphone_policy(&mut self) -> bool {
        if self.commands_enabled && self.release_microphone_while_idle {
            tracing::warn!("disabled voice commands because idle microphone release is enabled");
            self.commands_enabled = false;
            true
        } else {
            false
        }
    }

    fn validate_microphone_policy(&self) -> Result<()> {
        if self.commands_enabled && self.release_microphone_while_idle {
            return Err(eyre!(
                "voice commands require the microphone to remain ready"
            ));
        }
        Ok(())
    }

    pub fn set_commands_enabled(&mut self, enabled: bool) -> Result<()> {
        if enabled && self.release_microphone_while_idle {
            return Err(eyre!(
                "disable idle microphone release before enabling voice commands"
            ));
        }
        self.commands_enabled = enabled;
        Ok(())
    }

    pub fn set_release_microphone_while_idle(&mut self, enabled: bool) -> Result<()> {
        if enabled && self.commands_enabled {
            return Err(eyre!(
                "disable voice commands before enabling idle microphone release"
            ));
        }
        self.release_microphone_while_idle = enabled;
        Ok(())
    }

    pub fn keep_microphone_ready_and_enable_commands(&mut self) {
        self.release_microphone_while_idle = false;
        self.commands_enabled = true;
    }

    pub fn disable_commands_and_release_microphone(&mut self) {
        self.commands_enabled = false;
        self.release_microphone_while_idle = true;
    }

    fn normalize_double_tap_settings(&mut self) {
        if !self.double_tap_lock || self.dictation_hotkey.key.is_none() {
            self.double_tap_only = false;
        }
    }

    pub fn load() -> Result<Self> {
        let path = path()?;
        match fs::read(&path) {
            Ok(data) => {
                let mut settings: Self = serde_json::from_slice(&data)?;
                let microphone_policy_migrated = settings.normalize_microphone_policy();
                settings.dictation_processing.default_mode.name = "Global".into();
                settings.normalize_double_tap_settings();
                settings.migrate_legacy_replacements();
                let transcription_migrated = settings.migrate_disabled_transcription_model();
                crate::transcription_models::validate(&settings.transcription)?;
                settings.repair_hotkey_conflict();
                if (transcription_migrated || microphone_policy_migrated)
                    && let Err(error) = settings.save()
                {
                    tracing::warn!(%error, "could not persist the replacement transcription model");
                }
                settings.apply_runtime();
                Ok(settings)
            }
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                if let Some(mut settings) = crate::swift_settings_import::import() {
                    settings.normalize_double_tap_settings();
                    settings.repair_hotkey_conflict();
                    settings.save()?;
                    tracing::info!("imported preferences from Swift HEX");
                    return Ok(settings);
                }
                let settings = Self::default();
                settings.apply_runtime();
                Ok(settings)
            }
            Err(error) => Err(error.into()),
        }
    }

    pub fn save(&self) -> Result<()> {
        self.validate_microphone_policy()?;
        let path = path()?;
        self.write_to(&path)?;
        self.apply_runtime();
        Ok(())
    }

    fn write_to(&self, path: &std::path::Path) -> Result<()> {
        let parent = path
            .parent()
            .ok_or_else(|| eyre!("settings path has no parent"))?;
        fs::create_dir_all(parent)?;
        let temporary = settings_temporary_path(path);
        fs::write(&temporary, serde_json::to_vec_pretty(self)?)?;
        fs::rename(temporary, path)?;
        Ok(())
    }

    fn apply_runtime(&self) {
        let policy =
            u8::from(self.commands_enabled) | (u8::from(self.release_microphone_while_idle) << 1);
        MICROPHONE_POLICY.store(policy, Ordering::Release);
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
        DOUBLE_TAP_ONLY.store(
            self.double_tap_lock && self.double_tap_only && self.dictation_hotkey.key.is_some(),
            Ordering::Relaxed,
        );
        *HOTKEYS
            .get_or_init(Default::default)
            .write()
            .unwrap_or_else(|error| error.into_inner()) = RuntimeHotkeys {
            dictation: self.dictation_hotkey.runtime(),
            edit: self.edit_hotkey.runtime(),
            paste_last: self.paste_last_hotkey.as_ref().map(HotkeyBinding::runtime),
            paste_meeting: crate::DEVELOPER_FEATURES_ENABLED
                .then(|| HotkeyBinding::paste_meeting_default().runtime()),
        };
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

    fn migrate_disabled_transcription_model(&mut self) -> bool {
        if crate::transcription_models::definition(self.transcription.model).available() {
            return false;
        }
        tracing::warn!(
            model = self.transcription.model.as_str(),
            "replaced a disabled transcription model"
        );
        self.transcription = TranscriptionSelection::default();
        true
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
                control: Some(ModifierSide::Either),
                command: Some(ModifierSide::Either),
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

fn settings_temporary_path(path: &std::path::Path) -> PathBuf {
    let sequence = SETTINGS_TEMP_SEQUENCE.fetch_add(1, Ordering::Relaxed);
    path.with_extension(format!("json.{}.{}.tmp", std::process::id(), sequence))
}

pub fn hotkeys_conflict(dictation: &HotkeyBinding, edit: &HotkeyBinding) -> bool {
    dictation.overlaps(edit)
}

pub fn hotkey_conflicts(
    candidate: &HotkeyBinding,
    others: impl IntoIterator<Item = HotkeyBinding>,
) -> bool {
    others
        .into_iter()
        .any(|binding| candidate.overlaps(&binding))
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
    microphone_policy().commands_enabled
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct MicrophonePolicy {
    pub commands_enabled: bool,
    pub release_while_idle: bool,
}

pub fn microphone_policy() -> MicrophonePolicy {
    let policy = MICROPHONE_POLICY.load(Ordering::Acquire);
    MicrophonePolicy {
        commands_enabled: policy & 1 != 0,
        release_while_idle: policy & 2 != 0,
    }
}

pub fn custom_transformations_enabled() -> bool {
    CUSTOM_TRANSFORMATIONS_ENABLED.load(Ordering::Acquire)
}

pub fn double_tap_lock() -> bool {
    DOUBLE_TAP_LOCK.load(Ordering::Relaxed)
}

pub fn double_tap_only() -> bool {
    DOUBLE_TAP_ONLY.load(Ordering::Relaxed)
}

pub fn runtime_hotkeys() -> RuntimeHotkeys {
    *HOTKEYS
        .get_or_init(Default::default)
        .read()
        .unwrap_or_else(|error| error.into_inner())
}

pub fn dictation_hotkey() -> RuntimeHotkey {
    runtime_hotkeys().dictation
}

pub fn edit_hotkey() -> RuntimeHotkey {
    runtime_hotkeys().edit
}

pub fn voice_action_settings() -> VoiceActionSettings {
    VOICE_ACTION_SETTINGS
        .get_or_init(Default::default)
        .read()
        .unwrap_or_else(|error| error.into_inner())
        .clone()
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
        assert!(!settings.release_microphone_while_idle);
        assert!(settings.sound_effects);
        assert_eq!(settings.sound_effect_volume, 0.5);
        assert_eq!(settings.microphone, None);
        assert_eq!(
            settings.recording_audio_behavior,
            RecordingAudioBehavior::DoNothing
        );
        assert!(settings.double_tap_lock);
        assert!(!settings.double_tap_only);
        assert_eq!(settings.dictation_hotkey, HotkeyBinding::default());
        assert_eq!(settings.edit_hotkey, HotkeyBinding::edit_default());
        assert_eq!(
            settings.paste_last_hotkey,
            Some(HotkeyBinding::paste_last_default())
        );
        assert!(settings.show_dock_icon);
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
        assert_eq!(settings.voice_action.deadline_seconds, 60);
        assert!(settings.text_replacements.is_empty());
    }

    #[test]
    fn concurrent_settings_writers_use_distinct_temporary_paths() {
        let path = PathBuf::from("/tmp/settings.json");

        assert_ne!(
            settings_temporary_path(&path),
            settings_temporary_path(&path)
        );
    }

    #[test]
    fn legacy_sleep_prevention_setting_is_ignored() {
        let settings: AppSettings =
            serde_json::from_str(r#"{"prevent_system_sleep":false}"#).unwrap();
        let serialized = serde_json::to_value(settings).unwrap();

        assert!(serialized.get("prevent_system_sleep").is_none());
    }

    #[test]
    fn microphone_policy_requires_explicit_combined_transitions() {
        let mut settings = AppSettings::default();
        settings.set_release_microphone_while_idle(true).unwrap();
        assert!(settings.set_commands_enabled(true).is_err());
        assert!(!settings.commands_enabled);
        assert!(settings.release_microphone_while_idle);

        settings.keep_microphone_ready_and_enable_commands();
        assert!(settings.commands_enabled);
        assert!(!settings.release_microphone_while_idle);
        assert!(settings.set_release_microphone_while_idle(true).is_err());

        settings.disable_commands_and_release_microphone();
        assert!(!settings.commands_enabled);
        assert!(settings.release_microphone_while_idle);
    }

    #[test]
    fn invalid_persisted_microphone_policy_prefers_idle_release() {
        let mut settings: AppSettings = serde_json::from_str(
            r#"{"commands_enabled":true,"release_microphone_while_idle":true}"#,
        )
        .unwrap();

        assert!(settings.normalize_microphone_policy());
        assert!(!settings.commands_enabled);
        assert!(settings.release_microphone_while_idle);
        assert!(settings.validate_microphone_policy().is_ok());
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
    fn automatic_language_selection_round_trips_explicitly() {
        let settings = AppSettings {
            transcription: TranscriptionSelection {
                model: crate::transcription_models::TranscriptionModelId::WhisperLargeV3Turbo,
                language: crate::transcription_models::AUTO_LANGUAGE.into(),
                recognition_hints: String::new(),
            },
            ..AppSettings::default()
        };

        let encoded = serde_json::to_string(&settings).unwrap();
        let decoded: AppSettings = serde_json::from_str(&encoded).unwrap();

        assert!(encoded.contains(r#""language":"auto""#));
        assert_eq!(decoded.transcription, settings.transcription);
        assert!(crate::transcription_models::validate(&decoded.transcription).is_ok());
    }

    #[test]
    fn disabled_apple_speech_selection_migrates_to_the_default_model() {
        let mut settings: AppSettings = serde_json::from_str(
            r#"{"transcription":{"model":"apple_speech","language":"de","recognition_hints":""}}"#,
        )
        .unwrap();

        assert!(settings.migrate_disabled_transcription_model());
        assert_eq!(settings.transcription, TranscriptionSelection::default());
        assert!(!settings.migrate_disabled_transcription_model());
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
    fn hotkey_runtime_preserves_modifier_sides_and_key_code() {
        let binding = HotkeyBinding {
            modifiers: HotkeyModifiers {
                control: Some(ModifierSide::Left),
                option: None,
                shift: Some(ModifierSide::Right),
                command: None,
                function: true,
            },
            key: Some(HotkeyKey {
                code: 49,
                label: "Space".into(),
            }),
        };
        let runtime = binding.runtime();

        assert_eq!(runtime.modifiers, binding.modifiers);
        assert_eq!(runtime.key_code, Some(49));
    }

    #[test]
    fn modifier_sides_round_trip_and_legacy_booleans_decode_as_either() {
        let settings = AppSettings {
            dictation_hotkey: HotkeyBinding {
                modifiers: HotkeyModifiers {
                    option: Some(ModifierSide::Right),
                    command: Some(ModifierSide::Left),
                    ..Default::default()
                },
                key: Some(HotkeyKey {
                    code: 0,
                    label: "A".into(),
                }),
            },
            paste_last_hotkey: None,
            double_tap_only: true,
            ..Default::default()
        };

        let encoded = serde_json::to_string(&settings).unwrap();
        let decoded: AppSettings = serde_json::from_str(&encoded).unwrap();
        assert_eq!(decoded.dictation_hotkey, settings.dictation_hotkey);
        assert_eq!(decoded.paste_last_hotkey, None);
        assert!(decoded.double_tap_only);

        let legacy: HotkeyModifiers = serde_json::from_str(
            r#"{"control":false,"option":true,"shift":false,"command":false,"function":false}"#,
        )
        .unwrap();
        assert_eq!(legacy.option, Some(ModifierSide::Either));
    }

    #[test]
    fn side_constraints_can_be_removed_from_chord_modifiers() {
        let modifiers = HotkeyModifiers {
            option: Some(ModifierSide::Left),
            command: Some(ModifierSide::Right),
            function: true,
            ..Default::default()
        };

        assert_eq!(
            modifiers.without_side_constraints(),
            HotkeyModifiers {
                option: Some(ModifierSide::Either),
                command: Some(ModifierSide::Either),
                function: true,
                ..Default::default()
            }
        );
    }

    #[test]
    fn side_aware_matching_and_conflicts_share_the_same_algebra() {
        let left = HotkeyBinding {
            modifiers: HotkeyModifiers {
                option: Some(ModifierSide::Left),
                ..Default::default()
            },
            key: Some(HotkeyKey {
                code: 0,
                label: "A".into(),
            }),
        };
        let right = HotkeyBinding {
            modifiers: HotkeyModifiers {
                option: Some(ModifierSide::Right),
                ..Default::default()
            },
            key: left.key.clone(),
        };
        let either = HotkeyBinding {
            modifiers: HotkeyModifiers {
                option: Some(ModifierSide::Either),
                ..Default::default()
            },
            key: left.key.clone(),
        };

        assert!(!left.overlaps(&right));
        assert!(left.overlaps(&either));
        assert!(
            left.runtime()
                .matches_key_press(0, OPTION_KEY_MASK | LEFT_OPTION_MASK,)
        );
        assert!(
            !left
                .runtime()
                .matches_key_press(0, OPTION_KEY_MASK | RIGHT_OPTION_MASK,)
        );
        assert!(
            either
                .runtime()
                .matches_key_press(0, OPTION_KEY_MASK | RIGHT_OPTION_MASK,)
        );
    }

    #[test]
    fn meeting_and_last_transcript_paste_shortcuts_are_reserved() {
        let paste_key = HotkeyKey {
            code: 47,
            label: "V".into(),
        };
        for modifiers in [
            HotkeyModifiers {
                option: Some(ModifierSide::Either),
                shift: Some(ModifierSide::Either),
                ..Default::default()
            },
            HotkeyModifiers {
                option: Some(ModifierSide::Either),
                control: Some(ModifierSide::Either),
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
