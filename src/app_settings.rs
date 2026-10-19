use std::fs;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, AtomicU8, AtomicU64, Ordering};
use std::sync::{OnceLock, RwLock};

use color_eyre::eyre::{Result, eyre};
#[cfg(target_os = "macos")]
use objc2::MainThreadMarker;
#[cfg(target_os = "macos")]
use objc2_app_kit::{NSApplication, NSApplicationActivationPolicy};
use serde::{Deserialize, Serialize};

use crate::transcription_models::{TranscriptionModelId, TranscriptionSelection};

static RECORDING_AUDIO_BEHAVIOR: AtomicU8 = AtomicU8::new(0);
static DOUBLE_TAP_LOCK: AtomicBool = AtomicBool::new(true);
static DOUBLE_TAP_ONLY: AtomicBool = AtomicBool::new(false);
static SHORTCUT_TOGGLE: AtomicBool = AtomicBool::new(false);
static MEETINGS_ENABLED: AtomicBool = AtomicBool::new(false);
static MEETING_NOTES_ENABLED: AtomicBool = AtomicBool::new(true);
static MEETING_OUTPUT_FOLDER: OnceLock<RwLock<Option<PathBuf>>> = OnceLock::new();
static MICROPHONE_POLICY: AtomicU8 = AtomicU8::new(0);
static CUSTOM_TRANSFORMATIONS_ENABLED: AtomicBool = AtomicBool::new(false);
static INDICATOR_POSITION: AtomicU8 = AtomicU8::new(0);
static HOTKEYS: OnceLock<RwLock<RuntimeHotkeys>> = OnceLock::new();
#[cfg(target_os = "macos")]
static PASTE_KEY_CODE: OnceLock<u16> = OnceLock::new();
#[cfg(target_os = "macos")]
static SCRAWL_KEY_CODE: OnceLock<u16> = OnceLock::new();
static SCRAWL_ENABLED: AtomicBool = AtomicBool::new(false);
static SCRAWL_AUTO_CROP: AtomicBool = AtomicBool::new(true);
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

    #[cfg_attr(not(target_os = "windows"), allow(dead_code))]
    pub const fn control() -> Self {
        Self {
            control: Some(ModifierSide::Either),
            option: None,
            shift: None,
            command: None,
            function: false,
        }
    }

    #[cfg_attr(target_os = "macos", allow(dead_code))]
    pub const fn control_option() -> Self {
        Self {
            control: Some(ModifierSide::Either),
            option: Some(ModifierSide::Either),
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

    #[cfg(target_os = "macos")]
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

    /// Windows names Command the Windows key and Option Alt.
    #[cfg(target_os = "windows")]
    fn keycaps(self) -> impl Iterator<Item = &'static str> {
        [
            (self.control.is_some(), "Ctrl"),
            (self.command.is_some(), "Win"),
            (self.option.is_some(), "Alt"),
            (self.shift.is_some(), "Shift"),
        ]
        .into_iter()
        .filter_map(|(enabled, label)| enabled.then_some(label))
    }

    /// Linux names Command the Super key and Option Alt.
    #[cfg(target_os = "linux")]
    fn keycaps(self) -> impl Iterator<Item = &'static str> {
        [
            (self.control.is_some(), "Ctrl"),
            (self.option.is_some(), "Alt"),
            (self.shift.is_some(), "Shift"),
            (self.command.is_some(), "Super"),
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

#[cfg(target_os = "macos")]
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

/// A shortcut's non-modifier key. macOS identifies it by `code`, the virtual
/// key code resolved through the active keyboard layout. Windows and Linux
/// identify it by `label`, the key's name ("Space", "V", "F12", "PageUp"),
/// which their input layers translate to virtual keys, X11 keysyms, or evdev
/// codes; `code` is zero there. A settings file never moves between OSes.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct HotkeyKey {
    pub code: u16,
    pub label: String,
}

impl HotkeyKey {
    /// A key identified by name on Windows and Linux.
    #[cfg_attr(target_os = "macos", allow(dead_code))]
    pub fn named(name: &str) -> Self {
        Self {
            code: 0,
            label: key_label(name),
        }
    }

    /// The lowercase key name Windows and Linux input layers resolve.
    #[cfg_attr(target_os = "macos", allow(dead_code))]
    pub fn name(&self) -> String {
        self.label.to_ascii_lowercase()
    }

    fn same_key(&self, other: &Self) -> bool {
        if cfg!(target_os = "macos") {
            self.code == other.code
        } else {
            self.label.eq_ignore_ascii_case(&other.label)
        }
    }
}

/// The keycap label for a key name: one character in uppercase, otherwise
/// the name with a leading capital ("space" -> "Space", "f12" -> "F12").
#[cfg_attr(target_os = "macos", allow(dead_code))]
fn key_label(name: &str) -> String {
    let name = name.trim();
    let mut characters = name.chars();
    match characters.next() {
        Some(first) if name.chars().count() == 1 => first.to_uppercase().collect(),
        Some(first) => first.to_uppercase().chain(characters).collect(),
        None => String::new(),
    }
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(default)]
pub struct HotkeyBinding {
    pub modifiers: HotkeyModifiers,
    pub key: Option<HotkeyKey>,
}

impl Default for HotkeyBinding {
    /// Option on macOS, Ctrl+Space on Windows, Alt+Space on Linux.
    fn default() -> Self {
        #[cfg(target_os = "macos")]
        let binding = Self {
            modifiers: HotkeyModifiers::option(),
            key: None,
        };
        #[cfg(target_os = "windows")]
        let binding = Self::chord(HotkeyModifiers::control(), "space");
        #[cfg(target_os = "linux")]
        let binding = Self::chord(HotkeyModifiers::option(), "space");
        binding
    }
}

impl HotkeyBinding {
    /// Modifiers plus a key identified by name (Windows and Linux).
    #[cfg_attr(target_os = "macos", allow(dead_code))]
    fn chord(modifiers: HotkeyModifiers, key: &str) -> Self {
        Self {
            modifiers,
            key: Some(HotkeyKey::named(key)),
        }
    }

    /// Option-Command on macOS; Ctrl+Alt+I elsewhere, where a modifier-only
    /// chord would collide with ordinary typing.
    pub fn edit_default() -> Self {
        #[cfg(target_os = "macos")]
        let binding = Self {
            modifiers: HotkeyModifiers::option_command(),
            key: None,
        };
        #[cfg(not(target_os = "macos"))]
        let binding = Self::chord(HotkeyModifiers::control_option(), "i");
        binding
    }

    /// Option-Shift-V on macOS; Ctrl+Alt+V elsewhere.
    pub fn paste_last_default() -> Self {
        #[cfg(target_os = "macos")]
        let binding = Self {
            modifiers: HotkeyModifiers {
                option: Some(ModifierSide::Either),
                shift: Some(ModifierSide::Either),
                ..Default::default()
            },
            key: Some(HotkeyKey {
                code: paste_key_code(),
                label: "V".into(),
            }),
        };
        #[cfg(not(target_os = "macos"))]
        let binding = Self::chord(HotkeyModifiers::control_option(), "v");
        binding
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

    pub fn scrawl_default() -> Self {
        Self {
            modifiers: HotkeyModifiers {
                option: Some(ModifierSide::Either),
                shift: Some(ModifierSide::Either),
                ..Default::default()
            },
            key: Some(HotkeyKey {
                code: scrawl_key_code(),
                label: "S".into(),
            }),
        }
    }
}

#[cfg(target_os = "macos")]
fn paste_key_code() -> u16 {
    *PASTE_KEY_CODE.get_or_init(|| crate::keyboard::key_code_for('v').unwrap_or(9))
}

#[cfg(not(target_os = "macos"))]
fn paste_key_code() -> u16 {
    0
}

#[cfg(target_os = "macos")]
fn scrawl_key_code() -> u16 {
    *SCRAWL_KEY_CODE.get_or_init(|| crate::keyboard::key_code_for('s').unwrap_or(1))
}

#[cfg(not(target_os = "macos"))]
fn scrawl_key_code() -> u16 {
    0
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
    pub fn is_modifier_prefix_of(&self, other: &Self) -> bool {
        self.key.is_none()
            && other.modifiers.contains(self.modifiers)
            && (self.modifiers != other.modifiers || other.key.is_some())
    }

    pub fn overlaps(&self, other: &Self) -> bool {
        let same_key = match (&self.key, &other.key) {
            (Some(left), Some(right)) => left.same_key(right),
            (None, None) => true,
            _ => false,
        };
        same_key && modifiers_overlap(self.modifiers, other.modifiers)
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
            && self.required_modifiers_down(flags)
    }

    pub fn required_modifiers_down(self, flags: u64) -> bool {
        modifiers_from_flags(flags).contains(self.modifiers)
            // `Either` also represents side-less input and both sides held. Only the
            // original device flags distinguish those cases for a sided binding.
            && [
                (self.modifiers.control, LEFT_CONTROL_MASK, RIGHT_CONTROL_MASK),
                (self.modifiers.option, LEFT_OPTION_MASK, RIGHT_OPTION_MASK),
                (self.modifiers.shift, LEFT_SHIFT_MASK, RIGHT_SHIFT_MASK),
                (self.modifiers.command, LEFT_COMMAND_MASK, RIGHT_COMMAND_MASK),
            ]
            .into_iter()
            .all(|(side, left, right)| match side {
                Some(ModifierSide::Left) => flags & left != 0,
                Some(ModifierSide::Right) => flags & right != 0,
                Some(ModifierSide::Either) | None => true,
            })
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
    pub edit: Option<RuntimeHotkey>,
    pub paste_last: Option<RuntimeHotkey>,
    pub paste_meeting: Option<RuntimeHotkey>,
    pub scrawl: Option<RuntimeHotkey>,
}

impl Default for RuntimeHotkeys {
    fn default() -> Self {
        Self {
            dictation: HotkeyBinding::default().runtime(),
            edit: None,
            paste_last: Some(HotkeyBinding::paste_last_default().runtime()),
            paste_meeting: crate::DEVELOPER_FEATURES_ENABLED
                .then(|| HotkeyBinding::paste_meeting_default().runtime()),
            scrawl: None,
        }
    }
}

/// Opt-in Scrawl screen + voice capture (`⌥⇧S`). Off by default so no shortcut
/// is reserved and Screen Recording permission is never requested until enabled.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(default)]
pub struct ScrawlSettings {
    pub enabled: bool,
    pub hotkey: HotkeyBinding,
    pub auto_crop: bool,
}

impl Default for ScrawlSettings {
    fn default() -> Self {
        Self {
            enabled: false,
            hotkey: HotkeyBinding::scrawl_default(),
            auto_crop: true,
        }
    }
}

/// Opt-in meeting recording. Detection, the Meetings pane, and the meeting
/// paste shortcut stay off in release builds until `enabled` is set.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(default)]
pub struct MeetingSettings {
    pub enabled: bool,
    /// Folder for finished Markdown transcripts; `None` uses the default.
    pub output_folder: Option<PathBuf>,
    /// Title meetings and write a two-sentence summary with Apple
    /// Intelligence when it is available.
    pub summarize_with_apple_intelligence: bool,
}

impl Default for MeetingSettings {
    fn default() -> Self {
        Self {
            enabled: false,
            output_folder: None,
            summarize_with_apple_intelligence: true,
        }
    }
}

impl MeetingSettings {
    pub fn available(&self) -> bool {
        crate::DEVELOPER_FEATURES_ENABLED || self.enabled
    }

    pub fn resolved_output_folder(&self) -> PathBuf {
        self.output_folder
            .clone()
            .filter(|folder| !folder.as_os_str().is_empty())
            .unwrap_or_else(default_meeting_output_folder)
    }
}

pub fn default_meeting_output_folder() -> PathBuf {
    dirs::document_dir()
        .or_else(|| dirs::home_dir().map(|home| home.join("Documents")))
        .unwrap_or_else(|| PathBuf::from("."))
        .join("Hex Meetings")
}

/// Where the recording HUD appears while dictating.
#[derive(Clone, Copy, Debug, Default, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
#[repr(u8)]
pub enum IndicatorPosition {
    #[default]
    Top = 0,
    Bottom = 1,
    Hidden = 2,
}

#[derive(Clone, Copy, Debug, Default, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum RecordingAudioBehavior {
    Mute,
    PauseMedia,
    #[default]
    DoNothing,
}

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
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

#[derive(Clone, Debug, Default, Deserialize, PartialEq, Serialize)]
#[serde(default)]
pub struct DictationMode {
    pub name: String,
    pub applications: Vec<String>,
    pub browser_hosts: Vec<String>,
    pub replacements: Vec<TextReplacement>,
    pub transformations: Vec<String>,
    pub post_processing: DictationPostProcessing,
}

/// Persisted OpenCode rewrite settings for one mode.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(default)]
pub struct DictationPostProcessing {
    pub enabled: bool,
    pub prompt: String,
    /// Optional `provider/model` key from the OpenCode catalog.
    pub model: Option<String>,
    pub variant: Option<String>,
    pub deadline_seconds: u64,
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

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
#[serde(default)]
pub struct VoiceActionSettings {
    pub enabled: bool,
    pub model: Option<String>,
    pub variant: Option<String>,
    pub deadline_seconds: u64,
}

impl Default for VoiceActionSettings {
    fn default() -> Self {
        Self {
            enabled: false,
            model: None,
            variant: None,
            deadline_seconds: 60,
        }
    }
}

pub use crate::text_replacements::TextReplacement;

impl RecordingAudioBehavior {
    pub const ALL: [Self; 3] = [Self::Mute, Self::PauseMedia, Self::DoNothing];

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

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
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
    pub shortcut_mode: crate::dictation::ShortcutMode,
    pub dictation_hotkey: HotkeyBinding,
    #[serde(
        default = "HotkeyBinding::edit_default",
        deserialize_with = "deserialize_edit_hotkey"
    )]
    pub edit_hotkey: HotkeyBinding,
    pub paste_last_hotkey: Option<HotkeyBinding>,
    pub show_dock_icon: bool,
    pub transcription: TranscriptionSelection,
    pub transcription_recents: Vec<TranscriptionSelection>,
    pub dictation_processing: DictationProcessingSettings,
    pub voice_action: VoiceActionSettings,
    pub history_retention: crate::history::HistoryRetention,
    pub meetings: MeetingSettings,
    pub scrawl: ScrawlSettings,
    /// Start listening when Hex opens (Windows and Linux, where the
    /// listener can be off).
    pub enable_on_launch: bool,
    /// Paste with Ctrl-Shift-V for terminals (Linux).
    pub paste_with_shift: bool,
    /// The interface language code; `None` follows the system language.
    pub ui_language: Option<String>,
    pub indicator_position: IndicatorPosition,
    /// One-time settings changes already applied. A file that predates a
    /// change lacks its ID, so the change runs once on the next load; new
    /// settings start with every ID because they need none.
    #[serde(default)]
    pub migrations: Vec<String>,
    #[serde(skip_serializing)]
    pub text_replacements: Vec<TextReplacement>,
}

/// Moves English dictation to the streaming English model once.
const STREAMING_ENGLISH_MIGRATION: &str = "streaming-english-model";
const MIGRATIONS: &[&str] = &[STREAMING_ENGLISH_MIGRATION];

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
            shortcut_mode: crate::dictation::ShortcutMode::Hold,
            dictation_hotkey: HotkeyBinding::default(),
            edit_hotkey: HotkeyBinding::edit_default(),
            paste_last_hotkey: Some(HotkeyBinding::paste_last_default()),
            show_dock_icon: true,
            transcription: default_transcription(),
            transcription_recents: Vec::new(),
            dictation_processing: DictationProcessingSettings::default(),
            voice_action: VoiceActionSettings::default(),
            history_retention: crate::history::HistoryRetention::default(),
            meetings: MeetingSettings::default(),
            scrawl: ScrawlSettings::default(),
            enable_on_launch: true,
            paste_with_shift: false,
            ui_language: None,
            indicator_position: IndicatorPosition::default(),
            migrations: MIGRATIONS.iter().map(|id| id.to_string()).collect(),
            text_replacements: Vec::new(),
        }
    }
}

/// A new Windows install starts with the model recommended for the system
/// language; macOS and Linux start with the default model.
fn default_transcription() -> TranscriptionSelection {
    #[cfg(target_os = "windows")]
    let selection =
        crate::transcription_models::recommended_selection(&crate::desktop_i18n::system_language());
    #[cfg(not(target_os = "windows"))]
    let selection = TranscriptionSelection::default();
    selection
}

impl AppSettings {
    pub fn transcription_for_model(&self, id: TranscriptionModelId) -> TranscriptionSelection {
        if self.transcription.model == id {
            return self.transcription.clone();
        }
        if let Some(selection) = self.transcription_recents.iter().rev().find(|selection| {
            selection.model == id && crate::transcription_models::validate(selection).is_ok()
        }) {
            return selection.clone();
        }
        let model = crate::transcription_models::definition(id);
        let language = if model.supports_language(&self.transcription.language) {
            self.transcription.language.clone()
        } else if model.supports_language("en") {
            "en".into()
        } else {
            model.languages[0].into()
        };
        TranscriptionSelection {
            model: id,
            language,
            recognition_hints: String::new(),
        }
    }

    pub fn remember_transcription(&mut self, selection: TranscriptionSelection) {
        self.transcription_recents.push(self.transcription.clone());
        self.transcription_recents.push(selection.clone());
        self.normalize_transcription_recents();
        self.transcription = selection;
    }

    fn normalize_transcription_recents(&mut self) {
        let mut seen = std::collections::HashSet::new();
        self.transcription_recents.reverse();
        self.transcription_recents.retain(|selection| {
            crate::transcription_models::validate(selection).is_ok() && seen.insert(selection.model)
        });
        self.transcription_recents.reverse();
    }

    pub fn save_transcription(&mut self, selection: TranscriptionSelection) -> Result<()> {
        self.save_transcription_with(selection, Self::save)
    }

    fn save_transcription_with(
        &mut self,
        selection: TranscriptionSelection,
        save: impl FnOnce(&Self) -> Result<()>,
    ) -> Result<()> {
        crate::transcription_models::validate(&selection)?;
        let mut candidate = self.clone();
        candidate.remember_transcription(selection);
        self.commit_with(candidate, save)
    }

    /// Persist the entire candidate before replacing the current editor value.
    /// The candidate may include unrelated debounced edits, which must survive
    /// both failed immediate changes and successful commits.
    pub(crate) fn commit_with(
        &mut self,
        candidate: Self,
        save: impl FnOnce(&Self) -> Result<()>,
    ) -> Result<()> {
        save(&candidate)?;
        *self = candidate;
        Ok(())
    }

    fn normalize_microphone_policy(&mut self) {
        if self.commands_enabled && self.release_microphone_while_idle {
            tracing::warn!("disabled voice commands because idle microphone release is enabled");
            self.commands_enabled = false;
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
        migrate_legacy_settings(&path)?;
        let settings = Self::load_from(&path)?;
        settings.apply_runtime();
        Ok(settings)
    }

    pub(crate) fn load_from(path: &std::path::Path) -> Result<Self> {
        match fs::read(path) {
            Ok(data) => {
                let mut settings: Self = serde_json::from_slice(&data)?;
                let loaded = serde_json::to_value(&settings)?;
                settings.normalize_microphone_policy();
                settings.dictation_processing.default_mode.name = "Global".into();
                settings.normalize_double_tap_settings();
                settings.migrate_legacy_replacements();
                settings.normalize_mode_application_names();
                settings.migrate_disabled_transcription_model();
                settings.apply_one_time_migrations();
                settings.normalize_transcription_recents();
                crate::transcription_models::validate(&settings.transcription)?;
                settings.repair_hotkey_conflict();
                if serde_json::to_value(&settings)? != loaded
                    && let Err(error) = settings.write_to(path)
                {
                    tracing::warn!(%error, "could not persist normalized settings");
                }
                Ok(settings)
            }
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(Self::default()),
            Err(error) => Err(error.into()),
        }
    }

    pub fn save(&self) -> Result<()> {
        let path = path()?;
        self.write_to(&path)?;
        self.apply_runtime();
        Ok(())
    }

    fn write_to(&self, path: &std::path::Path) -> Result<()> {
        self.validate_microphone_policy()?;
        let parent = path
            .parent()
            .ok_or_else(|| eyre!("settings path has no parent"))?;
        fs::create_dir_all(parent)?;
        #[cfg(target_os = "macos")]
        if !path.exists() {
            // Saving new preferences does not imply onboarding completion.
            crate::onboarding::record_pending_at(parent)?;
        }
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
            transformations_used(&self.dictation_processing),
            Ordering::Release,
        );
        crate::feedback::set_enabled(self.sound_effects);
        crate::feedback::set_volume(self.sound_effect_volume.clamp(0.0, 1.0));
        crate::desktop_i18n::apply(self.ui_language.as_deref());
        INDICATOR_POSITION.store(self.indicator_position as u8, Ordering::Relaxed);
        RECORDING_AUDIO_BEHAVIOR.store(self.recording_audio_behavior.encoded(), Ordering::Relaxed);
        DOUBLE_TAP_LOCK.store(self.double_tap_lock, Ordering::Relaxed);
        DOUBLE_TAP_ONLY.store(
            self.double_tap_lock && self.double_tap_only && self.dictation_hotkey.key.is_some(),
            Ordering::Relaxed,
        );
        SHORTCUT_TOGGLE.store(
            self.shortcut_mode == crate::dictation::ShortcutMode::Toggle,
            Ordering::Relaxed,
        );
        MEETINGS_ENABLED.store(self.meetings.available(), Ordering::Release);
        MEETING_NOTES_ENABLED.store(
            self.meetings.summarize_with_apple_intelligence,
            Ordering::Release,
        );
        SCRAWL_ENABLED.store(self.scrawl.enabled, Ordering::Release);
        SCRAWL_AUTO_CROP.store(self.scrawl.auto_crop, Ordering::Release);
        *MEETING_OUTPUT_FOLDER
            .get_or_init(Default::default)
            .write()
            .unwrap_or_else(|error| error.into_inner()) =
            Some(self.meetings.resolved_output_folder());
        *HOTKEYS
            .get_or_init(Default::default)
            .write()
            .unwrap_or_else(|error| error.into_inner()) = self.runtime_hotkeys();
        set_transcription_selection(&self.transcription);
        set_microphone_selection(self.microphone.as_deref());
        #[cfg(target_os = "macos")]
        crate::config::update_dictation_profiles(&self.dictation_processing);
        *VOICE_ACTION_SETTINGS
            .get_or_init(Default::default)
            .write()
            .unwrap_or_else(|error| error.into_inner()) = self.voice_action.clone();
    }

    /// Mode activations saved while Finder showed filename extensions carry a
    /// `.app` suffix that never matched the foreground application name.
    #[cfg(not(target_os = "macos"))]
    fn normalize_mode_application_names(&mut self) {}

    #[cfg(target_os = "macos")]
    fn normalize_mode_application_names(&mut self) {
        let modes = std::iter::once(&mut self.dictation_processing.default_mode)
            .chain(self.dictation_processing.modes.iter_mut());
        for mode in modes {
            let mut mode_migrated = false;
            for application in &mut mode.applications {
                let normalized = crate::context::strip_bundle_extension(application).to_owned();
                if normalized != *application {
                    *application = normalized;
                    mode_migrated = true;
                }
            }
            if mode_migrated {
                mode.applications.sort_by_key(|name| name.to_lowercase());
                mode.applications.dedup();
            }
        }
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

    /// Applies each one-time change this file has not seen, then records it
    /// so a later choice, such as switching back, sticks.
    fn apply_one_time_migrations(&mut self) {
        if !self.has_migration(STREAMING_ENGLISH_MIGRATION) {
            self.switch_english_to_streaming_model();
            self.migrations.push(STREAMING_ENGLISH_MIGRATION.into());
        }
    }

    fn has_migration(&self, id: &str) -> bool {
        self.migrations.iter().any(|applied| applied == id)
    }

    /// English dictation moves to Parakeet Unified English, which streams
    /// while the shortcut is held. The previous model stays downloaded and
    /// in the recent models, one click away. Other languages and Auto keep
    /// their model because Parakeet Unified is English-only.
    fn switch_english_to_streaming_model(&mut self) {
        if self.transcription.language != "en"
            || self.transcription.model == TranscriptionModelId::ParakeetUnifiedEnglish
        {
            return;
        }
        let model =
            crate::transcription_models::definition(TranscriptionModelId::ParakeetUnifiedEnglish);
        let selection = TranscriptionSelection {
            model: model.id,
            language: "en".into(),
            recognition_hints: if model.supports_recognition_hints {
                self.transcription.recognition_hints.clone()
            } else {
                String::new()
            },
        };
        if crate::transcription_models::validate(&selection).is_err() {
            return;
        }
        tracing::info!(
            from = self.transcription.model.as_str(),
            "switched English dictation to the streaming English model"
        );
        self.remember_transcription(selection);
    }

    fn migrate_disabled_transcription_model(&mut self) {
        if crate::transcription_models::definition(self.transcription.model).available() {
            return;
        }
        tracing::warn!(
            model = self.transcription.model.as_str(),
            "replaced a disabled transcription model"
        );
        self.transcription = TranscriptionSelection::default();
    }

    pub fn runtime_hotkeys(&self) -> RuntimeHotkeys {
        RuntimeHotkeys {
            dictation: self.dictation_hotkey.runtime(),
            edit: self
                .voice_action
                .enabled
                .then(|| self.edit_hotkey.runtime()),
            paste_last: self.paste_last_hotkey.as_ref().map(HotkeyBinding::runtime),
            paste_meeting: self
                .meetings
                .available()
                .then(|| HotkeyBinding::paste_meeting_default().runtime()),
            scrawl: self.scrawl.enabled.then(|| self.scrawl.hotkey.runtime()),
        }
    }

    fn repair_hotkey_conflict(&mut self) {
        if !self.voice_action.enabled || !self.dictation_hotkey.overlaps(&self.edit_hotkey) {
            return;
        }
        #[cfg(target_os = "macos")]
        let edit_with_key = crate::keyboard::key_code_for('e')
            .ok()
            .map(|code| HotkeyBinding {
                modifiers: HotkeyModifiers::option_command(),
                key: Some(HotkeyKey {
                    code,
                    label: "E".into(),
                }),
            });
        #[cfg(not(target_os = "macos"))]
        let edit_with_key = Some(HotkeyBinding::chord(HotkeyModifiers::option_command(), "e"));
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
        .find(|binding| !self.dictation_hotkey.overlaps(binding))
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

/// Whether the global mode or any mode runs a transformation.
pub fn transformations_used(processing: &DictationProcessingSettings) -> bool {
    !processing.default_mode.transformations.is_empty()
        || processing
            .modes
            .iter()
            .any(|mode| !mode.transformations.is_empty())
}

/// Where the recording HUD appears, as last saved.
#[cfg_attr(not(target_os = "macos"), allow(dead_code))]
pub fn indicator_position() -> IndicatorPosition {
    match INDICATOR_POSITION.load(Ordering::Relaxed) {
        1 => IndicatorPosition::Bottom,
        2 => IndicatorPosition::Hidden,
        _ => IndicatorPosition::Top,
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

pub fn shortcut_toggle() -> bool {
    SHORTCUT_TOGGLE.load(Ordering::Relaxed)
}

/// Whether meeting detection and recording may run: always in developer
/// builds, otherwise only after the user opts in.
pub fn meetings_enabled() -> bool {
    MEETINGS_ENABLED.load(Ordering::Acquire)
}

pub fn meeting_notes_enabled() -> bool {
    MEETING_NOTES_ENABLED.load(Ordering::Acquire)
}

pub fn scrawl_enabled() -> bool {
    SCRAWL_ENABLED.load(Ordering::Acquire)
}

pub fn scrawl_auto_crop() -> bool {
    SCRAWL_AUTO_CROP.load(Ordering::Acquire)
}

pub fn meeting_output_folder() -> PathBuf {
    MEETING_OUTPUT_FOLDER
        .get_or_init(Default::default)
        .read()
        .unwrap_or_else(|error| error.into_inner())
        .clone()
        .unwrap_or_else(default_meeting_output_folder)
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

pub fn edit_hotkey() -> Option<RuntimeHotkey> {
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

/// The Dock policy while no Settings window is open. An open window always
/// shows the icon: hiding it deactivates HEX asynchronously, which would drop
/// that window behind other applications.
pub fn dock_icon_visible(show_dock_icon: bool, status_item_available: bool) -> bool {
    show_dock_icon || !status_item_available
}

#[cfg(target_os = "macos")]
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

#[cfg(target_os = "macos")]
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

/// The per-OS settings file that predates `settings.json`.
const LEGACY_SETTINGS_FILE: Option<&str> = if cfg!(target_os = "windows") {
    Some("windows-settings.json")
} else if cfg!(target_os = "linux") {
    Some("linux-settings.json")
} else {
    None
};

/// Whether settings were ever saved, including a legacy file that the next
/// load migrates; a first run has neither and onboards.
#[cfg_attr(target_os = "macos", allow(dead_code))]
pub fn exists() -> bool {
    path().is_ok_and(|path| saved_at(&path))
}

fn saved_at(path: &std::path::Path) -> bool {
    path.exists()
        || LEGACY_SETTINGS_FILE
            .zip(path.parent())
            .is_some_and(|(legacy, directory)| directory.join(legacy).exists())
}

/// Before the first `settings.json`, adopt a legacy per-OS settings file
/// once: write `settings.json`, then keep the old file as `*.migrated`.
pub(crate) fn migrate_legacy_settings(path: &std::path::Path) -> Result<()> {
    if path.exists() {
        return Ok(());
    }
    let Some(directory) = path.parent() else {
        return Ok(());
    };
    #[cfg(target_os = "windows")]
    let legacy = crate::windows_migration::legacy_settings(directory)?;
    #[cfg(target_os = "linux")]
    let legacy = crate::linux_migration::legacy_settings(directory)?;
    #[cfg(target_os = "macos")]
    let legacy: Option<(PathBuf, AppSettings)> = {
        let _ = directory;
        None
    };
    let Some((legacy_path, mut settings)) = legacy else {
        return Ok(());
    };
    settings.migrations.clear();
    settings.write_to(path)?;
    let mut migrated = legacy_path.clone().into_os_string();
    migrated.push(".migrated");
    fs::rename(&legacy_path, migrated)?;
    tracing::info!(from = %legacy_path.display(), "migrated legacy settings into settings.json");
    Ok(())
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
        assert_eq!(settings.transcription, AppSettings::default().transcription);
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
        assert!(!settings.meetings.enabled);
        assert_eq!(settings.meetings.output_folder, None);
        assert!(settings.meetings.summarize_with_apple_intelligence);
        assert!(
            settings
                .meetings
                .resolved_output_folder()
                .ends_with("Hex Meetings")
        );
        assert!(!settings.scrawl.enabled);
        assert_eq!(settings.scrawl.hotkey, HotkeyBinding::scrawl_default());
        assert!(settings.scrawl.auto_crop);
        assert_eq!(settings.runtime_hotkeys().scrawl, None);
        assert!(settings.text_replacements.is_empty());
    }

    #[test]
    fn existing_meeting_settings_summarize_by_default() {
        let settings: AppSettings =
            serde_json::from_str(r#"{"meetings":{"enabled":true}}"#).unwrap();
        assert!(settings.meetings.enabled);
        assert!(settings.meetings.summarize_with_apple_intelligence);
    }

    #[test]
    fn concurrent_settings_writers_use_distinct_temporary_paths() {
        let path = PathBuf::from("/tmp/settings.json");

        assert_ne!(
            settings_temporary_path(&path),
            settings_temporary_path(&path)
        );
    }

    fn english(model: TranscriptionModelId) -> String {
        format!(
            r#"{{"transcription":{{"model":"{}","language":"en","recognition_hints":""}}}}"#,
            model.as_str()
        )
    }

    #[test]
    fn english_dictation_switches_to_the_streaming_model_once() {
        let dir = TestSettingsDir::new("streaming-english");
        fs::write(&dir.path, english(TranscriptionModelId::ParakeetV3)).unwrap();

        let settings = AppSettings::load_from(&dir.path).unwrap();
        assert_eq!(
            settings.transcription.model,
            TranscriptionModelId::ParakeetUnifiedEnglish
        );
        assert!(settings.has_migration(STREAMING_ENGLISH_MIGRATION));
        assert!(
            settings
                .transcription_recents
                .iter()
                .any(|recent| recent.model == TranscriptionModelId::ParakeetV3),
            "the previous model stays one click away"
        );
        let saved: AppSettings = serde_json::from_slice(&fs::read(&dir.path).unwrap()).unwrap();
        assert!(saved.has_migration(STREAMING_ENGLISH_MIGRATION));

        // Switching back after the migration sticks.
        let mut chosen = settings.clone();
        chosen.transcription.model = TranscriptionModelId::ParakeetV3;
        chosen.write_to(&dir.path).unwrap();
        let reloaded = AppSettings::load_from(&dir.path).unwrap();
        assert_eq!(
            reloaded.transcription.model,
            TranscriptionModelId::ParakeetV3
        );
    }

    #[test]
    fn other_languages_and_auto_keep_their_model() {
        for language in ["de", "auto"] {
            let model = crate::transcription_models::choices_for_runtime(language)
                .iter()
                .map(|choice| choice.model.id)
                .find(|model| *model != TranscriptionModelId::ParakeetUnifiedEnglish)
                .expect("a model for the language");
            let dir = TestSettingsDir::new("streaming-other");
            fs::write(
                &dir.path,
                format!(
                    r#"{{"transcription":{{"model":"{}","language":"{language}","recognition_hints":""}}}}"#,
                    model.as_str()
                ),
            )
            .unwrap();
            let settings = AppSettings::load_from(&dir.path).unwrap();
            assert_eq!(settings.transcription.model, model);
            assert_eq!(settings.transcription.language, language);
            assert!(settings.has_migration(STREAMING_ENGLISH_MIGRATION));
        }
    }

    #[test]
    fn new_settings_need_no_one_time_migration() {
        assert!(AppSettings::default().has_migration(STREAMING_ENGLISH_MIGRATION));
        let dir = TestSettingsDir::new("streaming-new");
        let settings = AppSettings {
            transcription: TranscriptionSelection {
                model: TranscriptionModelId::ParakeetV3,
                language: "en".into(),
                recognition_hints: String::new(),
            },
            ..AppSettings::default()
        };
        settings.write_to(&dir.path).unwrap();
        let reloaded = AppSettings::load_from(&dir.path).unwrap();
        assert_eq!(
            reloaded.transcription.model,
            TranscriptionModelId::ParakeetV3
        );
    }

    struct TestSettingsDir {
        directory: PathBuf,
        path: PathBuf,
    }

    impl TestSettingsDir {
        fn new(prefix: &str) -> Self {
            let directory = std::env::temp_dir().join(format!(
                "hex-{prefix}-{}-{}",
                std::process::id(),
                SETTINGS_TEMP_SEQUENCE.fetch_add(1, Ordering::Relaxed),
            ));
            fs::create_dir(&directory).unwrap();
            let path = directory.join("settings.json");
            Self { directory, path }
        }
    }

    impl Drop for TestSettingsDir {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.directory);
        }
    }

    #[cfg(target_os = "macos")]
    #[test]
    fn new_settings_require_onboarding_across_reloads() {
        use crate::onboarding::{completion_recorded_at, record_completion_at};
        use std::os::unix::fs::PermissionsExt;

        let dir = TestSettingsDir::new("new-onboarding");
        assert!(!completion_recorded_at(&dir.directory));

        let mut settings = AppSettings::load_from(&dir.path).unwrap();
        assert!(!dir.path.exists());
        assert_eq!(settings.sound_effects, AppSettings::default().sound_effects);
        settings.sound_effects = false;
        settings.double_tap_lock = false;
        settings.write_to(&dir.path).unwrap();
        assert!(dir.path.is_file());
        assert!(!dir.directory.join("models").exists());
        assert!(!completion_recorded_at(&dir.directory));
        assert!(!dir.directory.join("onboarding-complete").exists());
        assert_eq!(
            fs::metadata(dir.directory.join("onboarding-pending"))
                .unwrap()
                .permissions()
                .mode()
                & 0o777,
            0o600,
        );
        drop(settings);

        let reloaded = AppSettings::load_from(&dir.path).unwrap();
        assert!(!reloaded.sound_effects);
        assert!(!reloaded.double_tap_lock);
        reloaded.write_to(&dir.path).unwrap();
        assert!(!completion_recorded_at(&dir.directory));
        assert!(!dir.directory.join("onboarding-complete").exists());

        record_completion_at(&dir.directory).unwrap();
        assert!(completion_recorded_at(&dir.directory));
        drop(reloaded);
        AppSettings::load_from(&dir.path).unwrap();
        assert!(completion_recorded_at(&dir.directory));
    }

    #[cfg(target_os = "macos")]
    #[test]
    fn loading_strips_finder_bundle_extensions_from_mode_applications() {
        let dir = TestSettingsDir::new("mode-applications");
        fs::write(
            &dir.path,
            br#"{"dictation_processing":{"modes":[{"name":"Code","applications":["Zed.app","Ghostty.app","Zed"]}]}}"#,
        )
        .unwrap();

        let settings = AppSettings::load_from(&dir.path).unwrap();
        assert_eq!(
            settings.dictation_processing.modes[0].applications,
            vec!["Ghostty".to_string(), "Zed".to_string()]
        );
        let persisted: AppSettings = serde_json::from_slice(&fs::read(&dir.path).unwrap()).unwrap();
        assert_eq!(
            persisted.dictation_processing.modes[0].applications,
            vec!["Ghostty".to_string(), "Zed".to_string()]
        );
    }

    #[test]
    fn loading_persists_normalizations_only_when_they_change_settings() {
        let dir = TestSettingsDir::new("normalized-settings");
        fs::write(
            &dir.path,
            br#"{"text_replacements":[{"matched_phrase":"open code","output":"OpenCode"}]}"#,
        )
        .unwrap();

        let settings = AppSettings::load_from(&dir.path).unwrap();
        assert!(settings.text_replacements.is_empty());
        let persisted = fs::read_to_string(&dir.path).unwrap();
        assert!(!persisted.contains("text_replacements"));
        assert!(persisted.contains("OpenCode"));

        let written = fs::metadata(&dir.path).unwrap().modified().unwrap();
        std::thread::sleep(std::time::Duration::from_millis(10));
        let reloaded = AppSettings::load_from(&dir.path).unwrap();
        assert_eq!(
            reloaded.dictation_processing.default_mode.replacements,
            settings.dictation_processing.default_mode.replacements
        );
        assert_eq!(
            fs::metadata(&dir.path).unwrap().modified().unwrap(),
            written
        );
    }

    #[cfg(target_os = "macos")]
    #[test]
    fn rewriting_legacy_rust_settings_preserves_onboarding_migration() {
        let dir = TestSettingsDir::new("legacy-onboarding");
        fs::write(&dir.path, b"{}").unwrap();

        let settings = AppSettings::load_from(&dir.path).unwrap();
        settings.write_to(&dir.path).unwrap();
        assert!(!dir.directory.join("onboarding-pending").exists());
        assert!(crate::onboarding::completion_recorded_at(&dir.directory));
        assert!(dir.directory.join("onboarding-complete").is_file());
    }

    #[cfg(target_os = "macos")]
    #[test]
    fn first_settings_write_requires_the_pending_marker() {
        let dir = TestSettingsDir::new("pending-onboarding");
        fs::create_dir(dir.directory.join("onboarding-pending")).unwrap();

        assert!(AppSettings::default().write_to(&dir.path).is_err());
        assert!(!dir.path.exists());
    }

    #[test]
    fn a_legacy_settings_file_counts_as_saved_settings() {
        let dir = TestSettingsDir::new("legacy-exists");
        assert!(!saved_at(&dir.path));
        if let Some(legacy) = LEGACY_SETTINGS_FILE {
            fs::write(dir.directory.join(legacy), b"{}").unwrap();
            assert!(saved_at(&dir.path));
            fs::remove_file(dir.directory.join(legacy)).unwrap();
        }
        fs::write(&dir.path, b"{}").unwrap();
        assert!(saved_at(&dir.path));
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

        settings.normalize_microphone_policy();
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
    fn model_switches_remember_language_and_hints_without_changing_other_settings() {
        let french = TranscriptionSelection {
            model: TranscriptionModelId::WhisperLargeV3Turbo,
            language: "fr".into(),
            recognition_hints: "Example project".into(),
        };
        let mut settings = AppSettings {
            show_dock_icon: false,
            microphone: Some("Test microphone".into()),
            ..Default::default()
        };
        settings.remember_transcription(french.clone());
        assert_eq!(
            settings
                .transcription_for_model(TranscriptionModelId::ParakeetV2)
                .language,
            "en"
        );
        assert_eq!(
            settings
                .transcription_for_model(TranscriptionModelId::CohereTranscribe)
                .language,
            "fr"
        );
        settings.remember_transcription(TranscriptionSelection::default());
        let encoded = serde_json::to_vec(&settings).unwrap();
        let mut decoded: AppSettings = serde_json::from_slice(&encoded).unwrap();
        assert_eq!(
            decoded.transcription_for_model(TranscriptionModelId::WhisperLargeV3Turbo),
            french
        );
        for _ in 0..100 {
            decoded.remember_transcription(french.clone());
            decoded.remember_transcription(TranscriptionSelection::default());
        }
        assert_eq!(decoded.transcription_recents.len(), 2);
        assert!(!decoded.show_dock_icon);
        assert_eq!(decoded.microphone.as_deref(), Some("Test microphone"));
        let old: AppSettings = serde_json::from_str("{}").unwrap();
        assert!(old.transcription_recents.is_empty());
    }

    #[test]
    fn failed_model_selection_save_preserves_selection_recents_and_unsaved_edits() {
        let mut settings = AppSettings::default();
        settings.dictation_processing.default_mode.name = "Unsaved mode".into();
        let before = serde_json::to_value(&settings).unwrap();
        let candidate = TranscriptionSelection {
            model: TranscriptionModelId::WhisperLargeV3Turbo,
            language: "fr".into(),
            recognition_hints: String::new(),
        };
        let error = settings
            .save_transcription_with(candidate.clone(), |next| {
                assert_eq!(next.transcription, candidate);
                assert_eq!(next.dictation_processing.default_mode.name, "Unsaved mode");
                Err(eyre!("fixture save failed"))
            })
            .unwrap_err();
        assert_eq!(error.to_string(), "fixture save failed");
        assert_eq!(serde_json::to_value(&settings).unwrap(), before);
        settings
            .save_transcription_with(candidate.clone(), |_| Ok(()))
            .unwrap();
        assert_eq!(settings.transcription, candidate);
        assert_eq!(
            settings.dictation_processing.default_mode.name,
            "Unsaved mode"
        );
    }

    #[test]
    fn invalid_remembered_languages_are_not_offered_by_model_switching() {
        let mut settings = AppSettings {
            transcription_recents: vec![
                TranscriptionSelection {
                    model: TranscriptionModelId::ParakeetV2,
                    language: "fr".into(),
                    recognition_hints: String::new(),
                },
                TranscriptionSelection {
                    model: TranscriptionModelId::WhisperLargeV3Turbo,
                    language: "not-a-language".into(),
                    recognition_hints: String::new(),
                },
            ],
            ..Default::default()
        };
        assert_eq!(
            settings
                .transcription_for_model(TranscriptionModelId::ParakeetV2)
                .language,
            "en"
        );
        assert_eq!(
            settings
                .transcription_for_model(TranscriptionModelId::WhisperLargeV3Turbo)
                .language,
            "en"
        );
        settings.normalize_transcription_recents();
        assert!(settings.transcription_recents.is_empty());
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

        settings.migrate_disabled_transcription_model();
        assert_eq!(settings.transcription, TranscriptionSelection::default());
    }

    #[test]
    fn voice_action_requires_explicit_opt_in_for_new_and_existing_settings() {
        for json in [
            r#"{}"#,
            r#"{"voice_action":{"model":"example/model-a","variant":"fast"}}"#,
        ] {
            let settings: AppSettings = serde_json::from_str(json).unwrap();
            assert!(settings.runtime_hotkeys().edit.is_none());
            let saved = serde_json::to_value(settings).unwrap();
            assert_eq!(saved["voice_action"]["enabled"], false);
        }
    }

    #[test]
    fn voice_action_settings_round_trip() {
        let settings = AppSettings {
            voice_action: VoiceActionSettings {
                enabled: true,
                model: Some("example/rewrite-model".into()),
                variant: Some("fast".into()),
                deadline_seconds: 12,
            },
            ..AppSettings::default()
        };

        let encoded = serde_json::to_string(&settings).unwrap();
        let decoded: AppSettings = serde_json::from_str(&encoded).unwrap();

        assert!(decoded.voice_action.enabled);
        assert_eq!(
            decoded.runtime_hotkeys().edit,
            Some(decoded.edit_hotkey.runtime())
        );
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
    fn shortcut_mode_defaults_to_hold_and_round_trips() {
        let legacy: AppSettings = serde_json::from_str("{}").unwrap();
        assert_eq!(legacy.shortcut_mode, crate::dictation::ShortcutMode::Hold);
        let settings = AppSettings {
            shortcut_mode: crate::dictation::ShortcutMode::Toggle,
            ..Default::default()
        };
        let decoded: AppSettings =
            serde_json::from_str(&serde_json::to_string(&settings).unwrap()).unwrap();
        assert_eq!(
            decoded.shortcut_mode,
            crate::dictation::ShortcutMode::Toggle
        );
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
        // Settings saved before dictation streamed still load.
        assert!(
            serde_json::from_str::<AppSettings>(r#"{"incremental_transcription":false}"#).is_ok()
        );

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
    fn runtime_side_constraints_require_the_requested_device_flag() {
        for (modifier, general, left, right) in [
            (
                "control",
                CONTROL_KEY_MASK,
                LEFT_CONTROL_MASK,
                RIGHT_CONTROL_MASK,
            ),
            (
                "option",
                OPTION_KEY_MASK,
                LEFT_OPTION_MASK,
                RIGHT_OPTION_MASK,
            ),
            ("shift", SHIFT_KEY_MASK, LEFT_SHIFT_MASK, RIGHT_SHIFT_MASK),
            (
                "command",
                COMMAND_KEY_MASK,
                LEFT_COMMAND_MASK,
                RIGHT_COMMAND_MASK,
            ),
        ] {
            for side in ["left", "right", "either"] {
                let binding: HotkeyBinding = serde_json::from_value(serde_json::json!({
                    "modifiers": { (modifier): side },
                    "key": { "code": 0, "label": "A" },
                }))
                .unwrap();
                let binding = binding.runtime();
                for (device_flags, matches) in [
                    (0, side == "either"),
                    (left, side != "right"),
                    (right, side != "left"),
                    (left | right, true),
                ] {
                    let flags = general | device_flags;
                    assert_eq!(
                        binding.required_modifiers_down(flags),
                        matches,
                        "{side:?}, flags={flags:#x}"
                    );
                    assert_eq!(
                        binding.matches_key_press(0, flags),
                        matches,
                        "{side:?}, flags={flags:#x}"
                    );
                    assert!(!binding.exact_modifiers(flags | FUNCTION_KEY_MASK));
                    assert_eq!(
                        binding.required_modifiers_down(flags | FUNCTION_KEY_MASK),
                        matches
                    );
                }
            }
        }
    }

    #[test]
    fn default_paste_hotkeys_follow_build_capabilities() {
        for hotkeys in [
            RuntimeHotkeys::default(),
            AppSettings::default().runtime_hotkeys(),
        ] {
            assert_eq!(
                hotkeys.paste_last,
                Some(HotkeyBinding::paste_last_default().runtime())
            );
            #[cfg(debug_assertions)]
            assert_eq!(
                hotkeys.paste_meeting,
                Some(HotkeyBinding::paste_meeting_default().runtime())
            );
            #[cfg(not(debug_assertions))]
            assert_eq!(hotkeys.paste_meeting, None);
        }
        let mut opted_in = AppSettings::default();
        opted_in.meetings.enabled = true;
        assert_eq!(
            opted_in.runtime_hotkeys().paste_meeting,
            Some(HotkeyBinding::paste_meeting_default().runtime())
        );
    }

    #[cfg(target_os = "macos")]
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
        settings.voice_action.enabled = true;

        settings.repair_hotkey_conflict();

        assert!(!settings.dictation_hotkey.overlaps(&settings.edit_hotkey));
    }

    #[test]
    fn disabled_voice_action_preserves_its_binding_without_reserving_it() {
        let mut settings = AppSettings {
            dictation_hotkey: HotkeyBinding::edit_default(),
            ..Default::default()
        };
        let saved_binding = settings.edit_hotkey.clone();
        settings.repair_hotkey_conflict();
        assert_eq!(settings.edit_hotkey, saved_binding);
        assert!(settings.runtime_hotkeys().edit.is_none());
        settings.voice_action.enabled = true;
        assert_eq!(
            settings.runtime_hotkeys().edit,
            Some(saved_binding.runtime())
        );
        settings.voice_action.enabled = false;
        assert!(settings.runtime_hotkeys().edit.is_none());
        assert_eq!(settings.edit_hotkey, saved_binding);
    }
}
