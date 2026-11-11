use std::fs;
use std::path::PathBuf;
#[cfg(test)]
use std::sync::atomic::AtomicU64;
use std::sync::atomic::{AtomicBool, AtomicU8, AtomicU32, Ordering};
use std::sync::{Arc, OnceLock, RwLock};

use color_eyre::eyre::{Result, eyre};
#[cfg(target_os = "macos")]
use objc2::MainThreadMarker;
#[cfg(target_os = "macos")]
use objc2_app_kit::{NSApplication, NSApplicationActivationPolicy};
use serde::{Deserialize, Serialize};

use crate::dictation::vocabulary::{VocabularyEntry, serialize_saved};
use crate::transcription::models::{TranscriptionModelId, TranscriptionSelection};

#[cfg(hex_capture)]
mod capture;
mod dictation_processing;
mod extra_shortcuts;
mod hotkeys;
mod labs;
mod meeting_folder;
mod meetings;
mod persistence;
mod screenshots;
#[cfg(target_os = "linux")]
pub(crate) use persistence::LaunchLoad;
pub(crate) use persistence::LaunchNote;
mod shortcut_repair;

#[cfg(hex_capture)]
pub use capture::*;
pub use dictation_processing::{
    DictationMode, DictationPostProcessing, DictationProcessingSettings,
};
pub use extra_shortcuts::*;
pub use hotkeys::*;
pub use labs::*;
pub use meeting_folder::default_meeting_output_folder;
pub use meetings::*;
#[cfg_attr(not(target_os = "macos"), allow(unused_imports))]
pub use screenshots::{
    ScreenshotSettings, screenshot_hotkey, screenshot_record_hotkey, screenshot_settings,
};

static RECORDING_DUCK_VOLUME: AtomicU32 = AtomicU32::new(0.2_f32.to_bits());
// Until settings load, "While dictating" is its default, not Mute: the
// hotkey pre-roll depends on it.
static RECORDING_AUDIO_BEHAVIOR: AtomicU8 =
    AtomicU8::new(RecordingAudioBehavior::DoNothing.encoded());
static DOUBLE_TAP_LOCK: AtomicBool = AtomicBool::new(true);
static DOUBLE_TAP_ONLY: AtomicBool = AtomicBool::new(false);
static SHORTCUT_TOGGLE: AtomicBool = AtomicBool::new(false);
static MEETINGS_ENABLED: AtomicBool = AtomicBool::new(false);
static PAUSE_BROWSER_MEDIA: AtomicBool = AtomicBool::new(false);
static HEX_MICROPHONE_ENABLED: AtomicBool = AtomicBool::new(false);
static MEETING_SUMMARIES: AtomicU8 = AtomicU8::new(MeetingSummaries::platform_default() as u8);
static MEETING_INDICATOR_ENABLED: AtomicBool = AtomicBool::new(true);
static MEETING_INDICATOR_CORNER: AtomicU8 = AtomicU8::new(0);
/// [`MeetingRetention::encode`]d; [`RETENTION_UNSET`] until settings apply.
static MEETING_RETENTION: AtomicU8 = AtomicU8::new(RETENTION_UNSET);
/// [`MeetingRetention::encode`]d; [`RETENTION_UNSET`] until settings apply.
static MEETING_AUDIO_RETENTION: AtomicU8 = AtomicU8::new(RETENTION_UNSET);
const RETENTION_UNSET: u8 = u8::MAX;
static MEETING_OUTPUT_FOLDER: OnceLock<RwLock<Option<PathBuf>>> = OnceLock::new();
static MEETING_PROVIDERS: OnceLock<RwLock<Vec<MeetingProviderChoice>>> = OnceLock::new();
static CALENDAR_SETTINGS: OnceLock<RwLock<CalendarSettings>> = OnceLock::new();
static MICROPHONE_POLICY: AtomicU8 = AtomicU8::new(0);
static CUSTOM_TRANSFORMATIONS_ENABLED: AtomicBool = AtomicBool::new(false);
static INDICATOR_POSITION: AtomicU8 = AtomicU8::new(0);
static HOTKEYS: OnceLock<RwLock<AppliedHotkeys>> = OnceLock::new();
static HOTKEY_CAPTURE_ACTIVE: AtomicBool = AtomicBool::new(false);
#[cfg(test)]
/// Distinct names for the tests' settings folders.
static SETTINGS_TEMP_SEQUENCE: AtomicU64 = AtomicU64::new(0);
static TRANSCRIPTION_SELECTION: OnceLock<RwLock<RuntimeTranscriptionSelection>> = OnceLock::new();
static MICROPHONE_SELECTION: OnceLock<RwLock<RuntimeMicrophoneSelection>> = OnceLock::new();
#[cfg(hex_capture)]
static CAPTURE_SETTINGS: OnceLock<RwLock<CaptureSettings>> = OnceLock::new();
#[cfg(hex_capture)]
static SCRAWL_APP_RUNNING: AtomicBool = AtomicBool::new(false);
#[cfg(hex_capture)]
static CAPTURE_ON: AtomicBool = AtomicBool::new(false);
/// [`CaptureSettings::capsule_flow`] for the capsule, without a lock.
#[cfg(hex_capture)]
static CAPSULE_FLOW: AtomicU8 = AtomicU8::new(0);

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

/// Experimental opt-in features hidden by default.
#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
#[serde(default)]
pub struct ExperimentalSettings {
    pub hex_microphone: bool,
    pub hex_microphone_mode: crate::audio::voice_filter::VoiceFilterMode,
    pub hex_microphone_comfort_noise: bool,
    pub hex_microphone_strength: f32,
}

impl Default for ExperimentalSettings {
    fn default() -> Self {
        Self {
            hex_microphone: false,
            hex_microphone_mode: crate::audio::voice_filter::VoiceFilterMode::Muffled,
            hex_microphone_comfort_noise: true,
            hex_microphone_strength: 1.0,
        }
    }
}

/// Whether the Voice Filter runs: the listener streams the Hex Microphone
/// and Hex watches its driver. Its Labs toggle (`experimental.hex_microphone`,
/// or `HEX_EXPERIMENTAL_MIC`) or an installed driver turns it on, so a driver
/// apps already use keeps receiving audio.
pub fn voice_filter_enabled(setting: bool, driver_installed: bool) -> bool {
    setting || env_hex_experimental_mic() || driver_installed
}

pub fn env_hex_experimental_mic() -> bool {
    std::env::var("HEX_EXPERIMENTAL_MIC")
        .ok()
        .is_some_and(|v| matches!(v.trim(), "1" | "true" | "TRUE" | "yes"))
}

/// Where Linux and Windows run the speech model (#856): Auto picks a GPU
/// unless it failed before under its current driver, then the CPU.
#[derive(Clone, Copy, Debug, Default, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum TranscriptionDevice {
    #[default]
    Auto,
    Gpu,
    Cpu,
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

/// Which release-service channel a direct install updates from
/// (`crate::updater`).
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum UpdateChannel {
    Stable,
    Dev,
}

impl UpdateChannel {
    pub const ALL: [Self; 2] = [Self::Stable, Self::Dev];

    pub const fn name(self) -> &'static str {
        match self {
            Self::Stable => "stable",
            Self::Dev => "dev",
        }
    }

    pub const fn label(self) -> &'static str {
        match self {
            Self::Stable => "Stable",
            Self::Dev => "Dev",
        }
    }
}

#[derive(Clone, Debug, Default, Deserialize, Eq, PartialEq, Serialize)]
#[serde(default)]
pub struct UpdateSettings {
    /// `None` follows the build: the channel it was published on (stamped at
    /// build time), else `dev` for a dev version, else its install's channel.
    /// Another channel than the build's is a switch the user confirms.
    pub channel: Option<UpdateChannel>,
}

#[derive(Clone, Copy, Debug, Default, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum RecordingAudioBehavior {
    Mute,
    Duck,
    PauseMedia,
    #[default]
    DoNothing,
}

pub use crate::dictation::text_replacements::TextReplacement;

impl RecordingAudioBehavior {
    pub const ALL: [Self; 4] = [Self::Mute, Self::PauseMedia, Self::Duck, Self::DoNothing];

    pub const fn label(self) -> &'static str {
        match self {
            Self::Mute => "Mute",
            Self::Duck => "Lower audio",
            Self::PauseMedia => "Pause media",
            Self::DoNothing => "Do nothing",
        }
    }

    const fn encoded(self) -> u8 {
        match self {
            Self::Mute => 0,
            Self::Duck => 3,
            Self::PauseMedia => 1,
            Self::DoNothing => 2,
        }
    }

    fn decode(value: u8) -> Self {
        match value {
            3 => Self::Duck,
            1 => Self::PauseMedia,
            2 => Self::DoNothing,
            _ => Self::Mute,
        }
    }
}

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
#[serde(default)]
pub struct AppSettings {
    /// Retired spoken-commands opt-in, kept untouched in `settings.json`.
    #[serde(
        default,
        rename = "commands_enabled",
        deserialize_with = "deserialize_retired_value",
        skip_serializing_if = "Option::is_none"
    )]
    pub retired_commands_enabled: Option<serde_json::Value>,
    pub release_microphone_while_idle: bool,
    pub sound_effects: bool,
    pub sound_effect_volume: f32,
    /// Which feedback sounds play (`crate::feedback`).
    pub sound_theme: crate::feedback::SoundSet,
    pub microphone: Option<String>,
    /// System default records from the built-in microphone, else another
    /// real input, rather than a Bluetooth default.
    pub avoid_bluetooth_microphone: bool,
    pub recording_audio_behavior: RecordingAudioBehavior,
    /// Fraction of the original output volume while ducking.
    pub recording_duck_volume: f32,
    /// With Pause media on macOS, also pause a browser that is audibly playing.
    pub pause_browser_media: bool,
    pub double_tap_lock: bool,
    pub double_tap_only: bool,
    pub shortcut_mode: crate::dictation::ShortcutMode,
    pub dictation_hotkey: HotkeyBinding,
    /// Retired Voice Action shortcut, kept untouched in `settings.json`.
    #[serde(
        default,
        rename = "edit_hotkey",
        deserialize_with = "deserialize_retired_value",
        skip_serializing_if = "Option::is_none"
    )]
    pub retired_edit_hotkey: Option<serde_json::Value>,
    pub paste_last_hotkey: Option<HotkeyBinding>,
    /// Toggle mode's cancel shortcut: discards the recording. Hex takes it
    /// only while a toggled recording runs; Hold mode has no cancel.
    pub cancel_hotkey: HotkeyBinding,
    /// Dictate without processing: records like the dictation shortcut and
    /// pastes the transcript as heard. Unbound by default.
    pub raw_dictation_hotkey: Option<HotkeyBinding>,
    /// Labs › Transform typed text (Linux): runs the focused app's Output
    /// steps on the text already typed in the focused field and replaces it.
    /// Unbound by default and ignored while its Labs toggle is off.
    pub typed_text_hotkey: Option<HotkeyBinding>,
    /// Labs › Second dictation shortcut: another shortcut that dictates
    /// exactly as `dictation_hotkey` does. Unbound by default and ignored
    /// while its Labs toggle is off.
    pub second_dictation_hotkey: Option<HotkeyBinding>,
    /// Labs › More dictation shortcuts (macOS): up to
    /// [`MAX_EXTRA_DICTATION_SHORTCUTS`] more, each with its own binding and
    /// activation mode. Saved rows stay while the Lab is off but never
    /// register or match.
    #[serde(default)]
    pub extra_dictation_hotkeys: Vec<ExtraDictationHotkey>,
    /// Labs › Terminal paste shortcut: another shortcut that dictates like
    /// `dictation_hotkey` and always pastes with Ctrl+Shift+V (Windows,
    /// Linux). Unbound by default and ignored while its Labs toggle is off.
    pub terminal_paste_hotkey: Option<HotkeyBinding>,
    pub show_dock_icon: bool,
    pub transcription: TranscriptionSelection,
    /// Where the speech model runs (Linux and Windows).
    pub transcription_device: TranscriptionDevice,
    pub transcription_recents: Vec<TranscriptionSelection>,
    pub keyboard_model_switching: KeyboardModelSwitching,
    pub dictation_processing: DictationProcessingSettings,
    /// Retired Voice Action settings, kept untouched in `settings.json`.
    #[serde(
        default,
        rename = "voice_action",
        deserialize_with = "deserialize_retired_value",
        skip_serializing_if = "Option::is_none"
    )]
    pub retired_voice_action: Option<serde_json::Value>,
    pub history_retention: crate::history::HistoryRetention,
    pub meetings: MeetingSettings,
    #[cfg(hex_capture)]
    pub capture: CaptureSettings,
    /// Capture settings from a build with captures, kept as they were.
    #[cfg(not(hex_capture))]
    #[serde(rename = "capture", skip_serializing_if = "Option::is_none")]
    pub capture_from_other_build: Option<serde_json::Value>,
    #[serde(default)]
    pub experimental: ExperimentalSettings,
    /// The Labs features' own switches (`crate::labs`).
    pub labs: LabsSettings,
    /// Labs › Screenshots' shortcut and folder (macOS).
    pub screenshots: ScreenshotSettings,
    /// Start listening when Hex opens (Windows and Linux, where the
    /// listener can be off).
    pub enable_on_launch: bool,
    /// Paste with Ctrl-Shift-V for terminals (Linux and Windows).
    pub paste_with_shift: bool,
    /// Hex keeps its own GNOME custom keyboard shortcuts, on GNOME without a
    /// shortcuts portal (Linux, #781).
    pub gnome_keyboard_shortcuts: bool,
    /// Let other apps see the dictation shortcut's key presses too
    /// (Windows), so a push-to-talk key can drive both.
    pub share_dictation_shortcut: bool,
    /// A mouse-button shortcut went back to its default because Labs ›
    /// Mouse shortcuts is off; the window says so once, then clears it.
    #[serde(skip_serializing_if = "std::ops::Not::not")]
    pub mouse_shortcut_notice: bool,
    /// The interface language code; `None` follows the system language.
    pub ui_language: Option<String>,
    pub indicator_position: IndicatorPosition,
    pub updates: UpdateSettings,
    /// The shared vocabulary (`crate::dictation::vocabulary`). Left out of the file
    /// while empty, so settings without one stay as they were.
    #[serde(
        skip_serializing_if = "Vec::is_empty",
        serialize_with = "serialize_saved"
    )]
    pub vocabulary: Vec<VocabularyEntry>,
    /// How readily transcription swaps heard words for a vocabulary term.
    pub vocabulary_boost: crate::dictation::vocabulary::VocabularyBoost,
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
/// Annotate became a default feature: on for everyone once.
#[cfg(hex_capture)]
const ANNOTATE_DEFAULT_MIGRATION: &str = "annotate-on-by-default";
/// Settings saved by an `anomaly.hex` build. Up to 2.3.1 Hex was
/// `com.kitlangton.hex2` and used the same folder, so a file without this
/// ID may carry onboarding finished under grants macOS no longer gives the
/// new identity; the macOS launch checks setup once
/// (`onboarding::recheck_after_identity_change`).
const ANOMALY_IDENTITY_MIGRATION: &str = "anomaly-hex-identity";
#[cfg(hex_capture)]
const MIGRATIONS: &[&str] = &[
    STREAMING_ENGLISH_MIGRATION,
    ANNOTATE_DEFAULT_MIGRATION,
    ANOMALY_IDENTITY_MIGRATION,
    MEETING_RETENTION_DEFAULT_MIGRATION,
    MEETING_SUMMARIES_MIGRATION,
];
#[cfg(not(hex_capture))]
const MIGRATIONS: &[&str] = &[
    STREAMING_ENGLISH_MIGRATION,
    ANOMALY_IDENTITY_MIGRATION,
    MEETING_RETENTION_DEFAULT_MIGRATION,
    MEETING_SUMMARIES_MIGRATION,
];
/// Scrawl's settings were read into `capture` once. Left out of
/// `MIGRATIONS` so new settings import them too.
#[cfg(hex_capture)]
const SCRAWL_IMPORT_MIGRATION: &str = "scrawl-capture-settings";
/// Scrawl's router settings were read into `capture.routing` once: its own
/// migration, as it came after the first import.
#[cfg(hex_capture)]
const SCRAWL_ROUTING_IMPORT_MIGRATION: &str = "scrawl-routing-settings";
/// Meeting retention first saved `forever` as its default; that default is
/// now unchosen (24 hours for new meetings, older ones kept).
const MEETING_RETENTION_DEFAULT_MIGRATION: &str = "meeting-retention-default";
/// Summarize with Apple Intelligence and Summarize with OpenCode became one
/// choice, `meetings.summaries`.
const MEETING_SUMMARIES_MIGRATION: &str = "meeting-summaries-choice";

fn deserialize_retired_value<'de, D>(
    deserializer: D,
) -> std::result::Result<Option<serde_json::Value>, D::Error>
where
    D: serde::Deserializer<'de>,
{
    serde_json::Value::deserialize(deserializer).map(Some)
}

impl Default for AppSettings {
    fn default() -> Self {
        Self {
            retired_commands_enabled: None,
            release_microphone_while_idle: false,
            sound_effects: true,
            sound_effect_volume: 0.5,
            sound_theme: crate::feedback::SoundSet::default(),
            microphone: None,
            avoid_bluetooth_microphone: true,
            recording_audio_behavior: RecordingAudioBehavior::DoNothing,
            recording_duck_volume: 0.2,
            pause_browser_media: false,
            double_tap_lock: true,
            double_tap_only: false,
            shortcut_mode: crate::dictation::ShortcutMode::Hold,
            dictation_hotkey: HotkeyBinding::default(),
            retired_edit_hotkey: None,
            paste_last_hotkey: Some(HotkeyBinding::paste_last_default()),
            cancel_hotkey: HotkeyBinding::cancel_default(),
            raw_dictation_hotkey: None,
            typed_text_hotkey: None,
            second_dictation_hotkey: None,
            extra_dictation_hotkeys: Vec::new(),
            terminal_paste_hotkey: None,
            show_dock_icon: true,
            transcription: default_transcription(),
            transcription_device: TranscriptionDevice::Auto,
            transcription_recents: Vec::new(),
            keyboard_model_switching: KeyboardModelSwitching::default(),
            dictation_processing: DictationProcessingSettings::default(),
            retired_voice_action: None,
            history_retention: crate::history::HistoryRetention::default(),
            meetings: MeetingSettings::default(),
            #[cfg(hex_capture)]
            capture: CaptureSettings::default(),
            #[cfg(not(hex_capture))]
            capture_from_other_build: None,
            experimental: ExperimentalSettings::default(),
            labs: LabsSettings::default(),
            screenshots: ScreenshotSettings::default(),
            enable_on_launch: true,
            paste_with_shift: false,
            gnome_keyboard_shortcuts: false,
            share_dictation_shortcut: false,
            mouse_shortcut_notice: false,
            ui_language: None,
            indicator_position: IndicatorPosition::default(),
            updates: UpdateSettings::default(),
            vocabulary: Vec::new(),
            vocabulary_boost: crate::dictation::vocabulary::VocabularyBoost::default(),
            migrations: MIGRATIONS.iter().map(|id| id.to_string()).collect(),
            text_replacements: Vec::new(),
        }
    }
}

/// macOS input-source IDs mapped to the model and language they select. An
/// absent entry means "Don't switch"; this never changes the model merely
/// because dictation starts.
#[derive(Clone, Debug, Default, Deserialize, Eq, PartialEq, Serialize)]
#[serde(default)]
pub struct KeyboardModelSwitching {
    pub enabled: bool,
    pub mappings: std::collections::BTreeMap<String, KeyboardModelChoice>,
}

/// What one input source selects: a model and a language it supports.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct KeyboardModelChoice {
    pub model: TranscriptionModelId,
    pub language: String,
}

impl KeyboardModelChoice {
    /// The picker's downloadable models only; Apple Speech is never mapped.
    pub fn validate(&self) -> Result<()> {
        let selection = TranscriptionSelection {
            model: self.model,
            language: self.language.clone(),
        };
        // An import stays mapped while it is missing or off, like the
        // saved selection; switching to it then runs the default model.
        if self.model.is_labs_gated() {
            return crate::transcription::models::validate_saved(&selection);
        }
        let model = crate::transcription::models::validate(&selection)?;
        match model.runtime {
            crate::transcription::models::ModelRuntime::Gguf(_) => Ok(()),
            crate::transcription::models::ModelRuntime::AppleSpeech => {
                Err(eyre!("{} can't be switched to by input source", model.name))
            }
        }
    }
}

/// A new install starts with the model recommended for the person's
/// language, so first-run setup can download it without asking.
fn default_transcription() -> TranscriptionSelection {
    crate::transcription::models::recommended_selection(&crate::desktop::i18n::preferred_language())
}

impl AppSettings {
    pub fn transcription_for_model(&self, id: TranscriptionModelId) -> TranscriptionSelection {
        if self.transcription.model == id {
            return self.transcription.clone();
        }
        if let Some(selection) = self.transcription_recents.iter().rev().find(|selection| {
            selection.model == id && crate::transcription::models::validate_saved(selection).is_ok()
        }) {
            return selection.clone();
        }
        let model = crate::transcription::models::definition(id);
        TranscriptionSelection {
            model: id,
            language: model.language_for(&self.transcription.language).into(),
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
            crate::transcription::models::validate_saved(selection).is_ok()
                && seen.insert(selection.model)
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
        crate::transcription::models::validate(&selection)?;
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

    pub fn load() -> Result<Self> {
        let path = path()?;
        #[cfg_attr(not(hex_capture), allow(unused_mut))]
        let mut settings = Self::load_from(&path)?;
        #[cfg(hex_capture)]
        {
            let read = |path: PathBuf| fs::read(path).ok();
            let engine = read(scrawl_capture::handoff::default_config_path());
            let mut imported = false;
            if !settings.has_migration(SCRAWL_IMPORT_MIGRATION) {
                settings.import_scrawl_settings(
                    engine.as_deref(),
                    crate::app_paths::data_dir()
                        .and_then(|data| read(data.join("scrawl/config.json")))
                        .as_deref(),
                );
                settings.migrations.push(SCRAWL_IMPORT_MIGRATION.into());
                imported = true;
            }
            if !settings.has_migration(SCRAWL_ROUTING_IMPORT_MIGRATION) {
                settings.import_scrawl_routing(engine.as_deref());
                settings
                    .migrations
                    .push(SCRAWL_ROUTING_IMPORT_MIGRATION.into());
                imported = true;
            }
            if imported
                && path.exists()
                && persistence::healthy_file(&path)
                && let Err(error) = settings.write_to(&path)
            {
                tracing::warn!(%error, "could not record the Scrawl settings import");
            }
        }
        settings.apply_runtime();
        Ok(settings)
    }

    /// Reads Scrawl's settings into `capture`, never writing Scrawl's
    /// files: `engine` is `~/.config/scrawl/config.json` (transcriber,
    /// captures folder, retention) and `app` is the app's shortcut file.
    /// Missing or unreadable values keep Hex's defaults.
    #[cfg(hex_capture)]
    fn import_scrawl_settings(&mut self, engine: Option<&[u8]>, app: Option<&[u8]>) {
        if let Some(config) = engine.and_then(|bytes| {
            serde_json::from_slice::<serde_json::Value>(bytes)
                .ok()
                .filter(serde_json::Value::is_object)
        }) {
            if config
                .get("transcriber")
                .and_then(serde_json::Value::as_str)
                == Some("apple")
            {
                self.capture.transcriber = CaptureTranscriber::Apple;
            }
            let home = crate::app_paths::home_dir();
            if let Some(folder) = config
                .get("capturesDir")
                .or_else(|| config.get("captures_dir"))
                .and_then(serde_json::Value::as_str)
                .map(|dir| match (dir.strip_prefix("~/"), &home) {
                    (Some(rest), Some(home)) => home.join(rest),
                    _ => PathBuf::from(dir),
                })
                .filter(|dir| dir.is_absolute())
            {
                self.capture.folder = Some(folder);
            }
            if let Some(policy) = config
                .get("retention")
                .and_then(|value| serde_json::from_value(value.clone()).ok())
            {
                self.capture.set_retention(policy);
            }
        }
        if let Some(binding) = app.and_then(scrawl_shortcut)
            && !self.hotkey_in_use(HotkeySlot::Capture, &binding)
        {
            self.capture.hotkey = binding;
        }
    }

    /// Reads the router's settings from Scrawl's `config.json` (`engine`):
    /// its rules, `autoSend`, and the `smartRouting` and `jevRouting`
    /// experiments, leniently as Scrawl reads them, leaving out the Jev key.
    /// A missing or unreadable file keeps the defaults.
    #[cfg(hex_capture)]
    fn import_scrawl_routing(&mut self, engine: Option<&[u8]>) {
        let Some(serde_json::Value::Object(root)) =
            engine.and_then(|bytes| serde_json::from_slice(bytes).ok())
        else {
            return;
        };
        let mut warnings = Vec::new();
        self.capture.routing =
            scrawl_capture::router_config_from_scrawl(&root, &mut warnings).into();
        // A warning quotes the value it dropped, which may be the Jev key.
        if !warnings.is_empty() {
            tracing::info!(
                count = warnings.len(),
                "Scrawl routing settings were left out"
            );
        }
    }

    /// The saved `labs` switches, read without loading, normalizing, or
    /// writing anything else.
    pub fn saved_labs() -> LabsSettings {
        #[derive(Deserialize)]
        struct File {
            #[serde(default)]
            labs: LabsSettings,
        }
        path()
            .ok()
            .and_then(|path| fs::read(path).ok())
            .and_then(|data| serde_json::from_slice::<File>(&data).ok())
            .map(|file| file.labs)
            .unwrap_or_default()
    }

    /// The saved `updates.channel`, read without loading, normalizing, or
    /// writing anything else, so the updater thread never races a save.
    pub fn saved_update_channel() -> Option<UpdateChannel> {
        #[derive(Deserialize)]
        struct File {
            #[serde(default)]
            updates: UpdateSettings,
        }
        let data = fs::read(path().ok()?).ok()?;
        serde_json::from_slice::<File>(&data).ok()?.updates.channel
    }

    /// The saved settings as `load` would see them, for a reader that must
    /// never change the file: no migration is written back.
    pub fn read() -> Result<Self> {
        Ok(Self::read_from(&path()?)?.0)
    }

    pub fn save(&self) -> Result<()> {
        let path = path()?;
        self.write_to(&path)?;
        self.apply_runtime();
        Ok(())
    }

    fn validate_recording_duck_volume(&self) -> Result<()> {
        if !self.recording_duck_volume.is_finite()
            || !(0.0..=1.0).contains(&self.recording_duck_volume)
        {
            return Err(eyre!("recording_duck_volume must be between 0 and 1"));
        }
        Ok(())
    }

    fn apply_runtime(&self) {
        // Whatever an editor holds, a managed lock keeps its switches off.
        let policy = crate::policy::Policy::current();
        if policy.any() {
            let mut managed = self.clone();
            policy.enforce(&mut managed);
            if managed != *self {
                return managed.apply_runtime();
            }
        }
        MICROPHONE_POLICY.store(
            u8::from(self.release_microphone_while_idle),
            Ordering::Release,
        );
        crate::audio::set_avoid_bluetooth_microphone(self.avoid_bluetooth_microphone);
        CUSTOM_TRANSFORMATIONS_ENABLED.store(
            transformations_used(&self.dictation_processing)
                && !policy.locks(crate::policy::Lock::Transforms),
            Ordering::Release,
        );
        crate::feedback::set_enabled(self.sound_effects);
        crate::feedback::set_volume(self.sound_effect_volume.clamp(0.0, 1.0));
        crate::feedback::set_sound_set(self.sound_theme);
        crate::desktop::i18n::apply(self.ui_language.as_deref());
        INDICATOR_POSITION.store(self.indicator_position as u8, Ordering::Relaxed);
        #[cfg(not(target_os = "macos"))]
        crate::transcription::local_transcriber::set_device(self.transcription_device);
        RECORDING_DUCK_VOLUME.store(self.recording_duck_volume.to_bits(), Ordering::Relaxed);
        RECORDING_AUDIO_BEHAVIOR.store(self.recording_audio_behavior.encoded(), Ordering::Relaxed);
        PAUSE_BROWSER_MEDIA.store(self.pause_browser_media, Ordering::Relaxed);
        DOUBLE_TAP_LOCK.store(self.double_tap_lock, Ordering::Relaxed);
        DOUBLE_TAP_ONLY.store(
            self.double_tap_lock
                && self.double_tap_only
                && (self.dictation_hotkey.key.is_some() || self.double_tap_modifiers_allowed()),
            Ordering::Relaxed,
        );
        SHORTCUT_TOGGLE.store(
            self.shortcut_mode == crate::dictation::ShortcutMode::Toggle,
            Ordering::Relaxed,
        );
        MEETINGS_ENABLED.store(
            self.meetings.available() && crate::labs::Feature::Meetings.shipped(),
            Ordering::Release,
        );
        crate::labs::apply(&self.labs);
        screenshots::apply_screenshots(self);
        #[cfg(hex_capture)]
        {
            *CAPTURE_SETTINGS
                .get_or_init(Default::default)
                .write()
                .unwrap_or_else(|error| error.into_inner()) = self.capture.clone();
            CAPTURE_ON.store(self.capture_on(), Ordering::Release);
            crate::capture_host::record_own_windows(self.capture.show_overlays_in_recordings);
            let flow = CapsuleFlow::ALL
                .iter()
                .position(|flow| *flow == self.capture.capsule_flow)
                .unwrap_or_default();
            CAPSULE_FLOW.store(flow as u8, Ordering::Release);
        }
        HEX_MICROPHONE_ENABLED.store(self.experimental.hex_microphone, Ordering::Release);
        crate::audio::hex_microphone::sync_from_settings(
            self.experimental.hex_microphone_mode,
            self.experimental.hex_microphone_comfort_noise,
            self.experimental.hex_microphone_strength,
        );
        MEETING_SUMMARIES.store(self.meetings.summaries as u8, Ordering::Release);
        crate::meeting::generator::set_summary_model(self.meetings.opencode_model.as_deref());
        MEETING_INDICATOR_ENABLED.store(self.meetings.show_recording_indicator, Ordering::Release);
        MEETING_INDICATOR_CORNER.store(
            self.meetings.recording_indicator_corner as u8,
            Ordering::Release,
        );
        MEETING_RETENTION.store(
            MeetingRetention::encode(self.meetings.retention),
            Ordering::Release,
        );
        MEETING_AUDIO_RETENTION.store(
            MeetingRetention::encode(self.meetings.audio_retention),
            Ordering::Release,
        );
        *MEETING_OUTPUT_FOLDER
            .get_or_init(Default::default)
            .write()
            .unwrap_or_else(|error| error.into_inner()) =
            Some(self.meetings.resolved_output_folder());
        *MEETING_PROVIDERS
            .get_or_init(Default::default)
            .write()
            .unwrap_or_else(|error| error.into_inner()) = self.meetings.providers.clone();
        *CALENDAR_SETTINGS
            .get_or_init(Default::default)
            .write()
            .unwrap_or_else(|error| error.into_inner()) = self.meetings.calendar.clone();
        *HOTKEYS
            .get_or_init(Default::default)
            .write()
            .unwrap_or_else(|error| error.into_inner()) = AppliedHotkeys {
            runtime: self.runtime_hotkeys(),
            settings: Some(Arc::new(self.clone())),
        };
        // An import that can't run stays saved; the listener runs the
        // default model until it is fixed.
        set_transcription_selection(&crate::transcription::custom_models::runnable(
            &self.transcription,
        ));
        set_microphone_selection(self.microphone.as_deref());
        #[cfg(target_os = "macos")]
        crate::dictation::config::update_dictation_profiles(&self.dictation_processing);
        crate::dictation::vocabulary::apply(&self.vocabulary, self.vocabulary_boost);
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
                let normalized =
                    crate::dictation::context::strip_bundle_extension(application).to_owned();
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
        self.apply_one_time_migrations_with(|model| {
            crate::transcription::models::is_installed(model, "en")
        });
    }

    fn apply_one_time_migrations_with(
        &mut self,
        installed: impl Fn(&crate::transcription::models::ModelDefinition) -> bool,
    ) {
        if !self.has_migration(STREAMING_ENGLISH_MIGRATION) {
            self.switch_english_to_streaming_model(installed);
            self.migrations.push(STREAMING_ENGLISH_MIGRATION.into());
        }
        if !self.has_migration(MEETING_RETENTION_DEFAULT_MIGRATION) {
            if self.meetings.retention == Some(MeetingRetention::Forever) {
                self.meetings.retention = None;
            }
            self.migrations
                .push(MEETING_RETENTION_DEFAULT_MIGRATION.into());
        }
        self.migrate_meeting_summaries();
        #[cfg(hex_capture)]
        if !self.has_migration(ANNOTATE_DEFAULT_MIGRATION) {
            self.capture.enabled = true;
            self.migrations.push(ANNOTATE_DEFAULT_MIGRATION.into());
        }
    }

    /// The two retired summary switches become `meetings.summaries` once.
    pub(super) fn migrate_meeting_summaries(&mut self) {
        if !self.has_migration(MEETING_SUMMARIES_MIGRATION) {
            self.meetings.summaries = MeetingSummaries::from_retired(
                self.meetings
                    .summarize_with_apple_intelligence
                    .unwrap_or(true),
                self.meetings.summarize_with_opencode.unwrap_or(false),
            );
            self.migrations.push(MEETING_SUMMARIES_MIGRATION.into());
        }
        self.meetings.summarize_with_apple_intelligence = None;
        self.meetings.summarize_with_opencode = None;
    }

    fn has_migration(&self, id: &str) -> bool {
        self.migrations.iter().any(|applied| applied == id)
    }

    /// Whether these settings come from before the `anomaly.hex` identity.
    #[cfg(target_os = "macos")]
    pub(crate) fn predates_anomaly_identity(&self) -> bool {
        !self.has_migration(ANOMALY_IDENTITY_MIGRATION)
    }

    /// Records the identity check in the file at `path`, changing nothing else.
    #[cfg(target_os = "macos")]
    pub(crate) fn record_anomaly_identity(&mut self, path: &std::path::Path) -> Result<()> {
        if self.predates_anomaly_identity() {
            self.migrations.push(ANOMALY_IDENTITY_MIGRATION.into());
            self.write_to(path)?;
        }
        Ok(())
    }

    /// English dictation moves to Parakeet Unified English, which streams
    /// while the shortcut is held. The previous model stays downloaded and
    /// in the recent models, one click away. Other languages and Auto keep
    /// their model because Parakeet Unified is English-only. Only an installed
    /// model is switched to, so an upgrade never leaves dictation waiting on a
    /// download.
    fn switch_english_to_streaming_model(
        &mut self,
        installed: impl Fn(&crate::transcription::models::ModelDefinition) -> bool,
    ) {
        if self.transcription.language != "en"
            || self.transcription.model == TranscriptionModelId::ParakeetUnifiedEnglish
        {
            return;
        }
        let model =
            crate::transcription::models::definition(TranscriptionModelId::ParakeetUnifiedEnglish);
        if !installed(model) {
            return;
        }
        let selection = TranscriptionSelection {
            model: model.id,
            language: "en".into(),
        };
        if crate::transcription::models::validate(&selection).is_err() {
            return;
        }
        tracing::info!(
            from = self.transcription.model.as_str(),
            "switched English dictation to the streaming English model"
        );
        self.remember_transcription(selection);
    }

    fn migrate_disabled_transcription_model(&mut self) {
        if self.transcription.model.is_labs_gated()
            || crate::transcription::models::definition(self.transcription.model).available()
        {
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
            paste_last: self.paste_last_hotkey.as_ref().map(HotkeyBinding::runtime),
            paste_meeting: self
                .meetings
                .available()
                .then(|| HotkeyBinding::paste_meeting_default().runtime()),
            #[cfg(hex_capture)]
            capture: self.capture_on().then(|| self.capture.hotkey.runtime()),
            #[cfg(not(hex_capture))]
            capture: None,
            #[cfg(hex_capture)]
            capture_replay: self
                .active_capture_replay_hotkey()
                .map(HotkeyBinding::runtime),
            #[cfg(not(hex_capture))]
            capture_replay: None,
            raw_dictation: self
                .raw_dictation_hotkey
                .as_ref()
                .map(HotkeyBinding::runtime),
            typed_text: self.active_typed_text_hotkey().map(HotkeyBinding::runtime),
            second_dictation: self
                .active_second_dictation_hotkey()
                .map(HotkeyBinding::runtime),
            extra_dictation: std::array::from_fn(|index| {
                self.active_extra_dictation_hotkey(index)
                    .map(HotkeyBinding::runtime)
            }),
            extra_dictation_modes: std::array::from_fn(|index| self.extra_dictation_mode(index)),
            cancel: self.active_cancel_hotkey().map(HotkeyBinding::runtime),
        }
    }

    /// The cancel shortcut while it takes effect: in Toggle mode only.
    pub fn active_cancel_hotkey(&self) -> Option<&HotkeyBinding> {
        (self.shortcut_mode == crate::dictation::ShortcutMode::Toggle)
            .then_some(&self.cancel_hotkey)
    }

    /// Labs › Second dictation shortcut's binding while it takes effect:
    /// its toggle on, bound, sharing no key with another active shortcut,
    /// and, on Linux and Windows, naming a key (both register a key). On
    /// macOS a shortcut without a key can still run. Otherwise Hex matches,
    /// suppresses, registers, and checks nothing for it.
    pub fn active_second_dictation_hotkey(&self) -> Option<&HotkeyBinding> {
        if !self.labs.second_dictation_shortcut {
            return None;
        }
        self.second_dictation_hotkey
            .as_ref()
            .filter(|binding| cfg!(target_os = "macos") || binding.key.is_some())
            .filter(|binding| !self.hotkey_in_use(HotkeySlot::SecondDictation, binding))
    }

    /// Labs › Transform typed text's binding while it takes effect: Linux,
    /// its toggle on, bound, and sharing no key with another active
    /// shortcut. Otherwise the listener registers nothing for it.
    pub fn active_typed_text_hotkey(&self) -> Option<&HotkeyBinding> {
        if !cfg!(target_os = "linux") || !self.labs.typed_text {
            return None;
        }
        self.typed_text_hotkey
            .as_ref()
            .filter(|binding| !self.hotkey_in_use(HotkeySlot::TypedText, binding))
    }

    /// Labs › Terminal paste shortcut's binding while it takes effect:
    /// Windows or Linux, its toggle on, bound, and sharing no key with
    /// another active shortcut. Otherwise Hex registers, matches, and
    /// checks nothing for it.
    #[cfg_attr(not(any(target_os = "windows", target_os = "linux")), allow(dead_code))]
    pub fn active_terminal_paste_hotkey(&self) -> Option<&HotkeyBinding> {
        if !cfg!(any(target_os = "windows", target_os = "linux"))
            || !self.labs.terminal_paste_shortcut
        {
            return None;
        }
        self.terminal_paste_hotkey
            .as_ref()
            .filter(|binding| !self.hotkey_in_use(HotkeySlot::TerminalPaste, binding))
    }

    /// Annotate is on: the person hasn't turned it off.
    #[cfg(hex_capture)]
    pub fn capture_on(&self) -> bool {
        self.capture.enabled
    }

    /// Labs › Annotate 15 s replay's shortcut while it takes effect: macOS 27+,
    /// Annotate and the Lab on, bound, and sharing no key with another active
    /// shortcut.
    #[cfg(hex_capture)]
    pub fn active_capture_replay_hotkey(&self) -> Option<&HotkeyBinding> {
        if !cfg!(target_os = "macos")
            || !self.capture_on()
            || !self.labs.annotate_replay
            || !crate::labs::Feature::AnnotateReplay.on_this_os()
        {
            return None;
        }
        self.capture
            .replay_hotkey
            .as_ref()
            .filter(|binding| !self.hotkey_in_use(HotkeySlot::CaptureReplay, binding))
    }

    /// A shortcut saved as a bare left click swallowed every click, so its
    /// owner couldn't change it in Settings. Such a shortcut goes back to its
    /// default when settings load.
    fn repair_bare_primary_clicks(&mut self) {
        if self.dictation_hotkey.is_bare_primary_click() {
            tracing::warn!("replaced a left-click dictation shortcut with the default");
            self.dictation_hotkey = HotkeyBinding::default();
        }
        if self
            .paste_last_hotkey
            .as_ref()
            .is_some_and(HotkeyBinding::is_bare_primary_click)
        {
            tracing::warn!("replaced a left-click paste-last shortcut with the default");
            self.paste_last_hotkey = Some(HotkeyBinding::paste_last_default());
        }
        if self.cancel_hotkey.is_bare_primary_click() {
            tracing::warn!("replaced a left-click cancel shortcut with the default");
            self.cancel_hotkey = HotkeyBinding::cancel_default();
        }
        if self
            .raw_dictation_hotkey
            .as_ref()
            .is_some_and(HotkeyBinding::is_bare_primary_click)
        {
            tracing::warn!("cleared a left-click raw dictation shortcut");
            self.raw_dictation_hotkey = None;
        }
        if self
            .typed_text_hotkey
            .as_ref()
            .is_some_and(HotkeyBinding::is_bare_primary_click)
        {
            tracing::warn!("cleared a left-click typed text shortcut");
            self.typed_text_hotkey = None;
        }
        if self
            .second_dictation_hotkey
            .as_ref()
            .is_some_and(HotkeyBinding::is_bare_primary_click)
        {
            tracing::warn!("cleared a left-click second dictation shortcut");
            self.second_dictation_hotkey = None;
        }
        if self
            .terminal_paste_hotkey
            .as_ref()
            .is_some_and(HotkeyBinding::is_bare_primary_click)
        {
            tracing::warn!("cleared a left-click terminal paste shortcut");
            self.terminal_paste_hotkey = None;
        }
        #[cfg(hex_capture)]
        if self.capture.hotkey.is_bare_primary_click() {
            tracing::warn!("replaced a left-click capture shortcut with the default");
            self.capture.hotkey = HotkeyBinding::capture_default();
        }
        #[cfg(hex_capture)]
        if self
            .capture
            .replay_hotkey
            .as_ref()
            .is_some_and(HotkeyBinding::is_bare_primary_click)
        {
            tracing::warn!("cleared a left-click capture replay shortcut");
            self.capture.replay_hotkey = None;
        }
        self.screenshots.clear_left_click_record();
    }
}

/// A shortcut Settings and `hex settings` record.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum HotkeySlot {
    Dictation,
    PasteLast,
    RawDictation,
    /// Labs › Transform typed text's shortcut (Linux).
    TypedText,
    #[cfg(hex_capture)]
    Capture,
    /// Labs › Annotate 15 s replay shortcut (macOS 27+).
    #[cfg(hex_capture)]
    CaptureReplay,
    /// Labs › Second dictation shortcut.
    SecondDictation,
    /// Labs › More dictation shortcuts (macOS): row `index`'s binding.
    ExtraDictation(usize),
    /// Labs › Terminal paste shortcut (Windows, Linux).
    TerminalPaste,
    /// Toggle mode's cancel shortcut.
    Cancel,
    /// Labs › Screenshots (macOS).
    Screenshot,
    /// Labs › Screenshots' record shortcut (macOS, with Annotate).
    ScreenshotRecord,
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

pub fn recording_duck_volume() -> f32 {
    f32::from_bits(RECORDING_DUCK_VOLUME.load(Ordering::Relaxed))
}

pub fn recording_audio_behavior() -> RecordingAudioBehavior {
    RecordingAudioBehavior::decode(RECORDING_AUDIO_BEHAVIOR.load(Ordering::Relaxed))
}

pub fn pause_browser_media() -> bool {
    PAUSE_BROWSER_MEDIA.load(Ordering::Relaxed)
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct MicrophonePolicy {
    pub release_while_idle: bool,
}

pub fn microphone_policy() -> MicrophonePolicy {
    let policy = MICROPHONE_POLICY.load(Ordering::Acquire);
    MicrophonePolicy {
        release_while_idle: policy != 0,
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

/// [`voice_filter_enabled`] for the running listener.
pub fn hex_microphone_enabled() -> bool {
    crate::labs::Feature::VoiceFilter.shipped()
        && crate::labs::Feature::VoiceFilter.managed().is_none()
        && voice_filter_enabled(
            HEX_MICROPHONE_ENABLED.load(Ordering::Acquire),
            crate::audio::hex_microphone::driver_bundle_installed(),
        )
}

/// The shortcuts the input layers match, and the settings they came from,
/// so a keyboard layout change can resolve their letter keys again.
#[derive(Default)]
struct AppliedHotkeys {
    runtime: RuntimeHotkeys,
    #[cfg_attr(not(target_os = "macos"), allow(dead_code))]
    settings: Option<Arc<AppSettings>>,
}

impl AppliedHotkeys {
    /// Resolves the shortcuts again from the settings they came from, on the
    /// layout published now, unless newer settings were applied meanwhile.
    #[cfg(target_os = "macos")]
    fn resolve_again(
        applied: &RwLock<Self>,
        resolve: impl FnOnce(Option<&AppSettings>) -> RuntimeHotkeys,
    ) {
        let settings = applied
            .read()
            .unwrap_or_else(|error| error.into_inner())
            .settings
            .clone();
        let runtime = resolve(settings.as_deref());
        let mut applied = applied.write().unwrap_or_else(|error| error.into_inner());
        // Settings applied while this resolved carry the new layout already.
        let same = match (&applied.settings, &settings) {
            (Some(current), Some(resolved)) => Arc::ptr_eq(current, resolved),
            (None, None) => true,
            _ => false,
        };
        if same {
            applied.runtime = runtime;
        }
    }
}

/// Resolves the shortcuts' letter keys again after the main thread published
/// a new keyboard layout, from the settings last applied, as a settings
/// reload does. The listener rebinds each shortcut once it is idle.
#[cfg(target_os = "macos")]
pub fn refresh_layout_hotkeys() {
    AppliedHotkeys::resolve_again(HOTKEYS.get_or_init(Default::default), |settings| {
        settings.map_or_else(RuntimeHotkeys::default, AppSettings::runtime_hotkeys)
    });
}

pub fn runtime_hotkeys() -> RuntimeHotkeys {
    #[cfg_attr(not(hex_capture), allow(unused_mut))]
    let mut hotkeys = HOTKEYS
        .get_or_init(Default::default)
        .read()
        .unwrap_or_else(|error| error.into_inner())
        .runtime;
    #[cfg(hex_capture)]
    if scrawl_app_running() {
        hotkeys.capture = None;
        hotkeys.capture_replay = None;
    }
    hotkeys
}

pub fn dictation_hotkey() -> RuntimeHotkey {
    runtime_hotkeys().dictation
}

pub fn hotkey_capture_active() -> bool {
    HOTKEY_CAPTURE_ACTIVE.load(Ordering::Acquire)
}

pub fn set_hotkey_capture_active(active: bool) {
    HOTKEY_CAPTURE_ACTIVE.store(active, Ordering::Release);
}

/// Whether shortcut matching is suspended: a new shortcut binding is being
/// recorded, or Labs › Pause from menu bar paused Hex. Every capture
/// shortcut reads this instead of `hotkey_capture_active` alone.
pub fn shortcuts_suspended() -> bool {
    hotkey_capture_active() || crate::shortcut_pause::suspended()
}

/// The Dock policy while no Settings window is open. An open window always
/// shows the icon: hiding it deactivates Hex asynchronously, which would drop
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
    if application.activationPolicy() != policy && !application.setActivationPolicy(policy) {
        tracing::warn!(visible, "could not change Dock icon visibility");
    }
}

#[cfg(target_os = "macos")]
pub fn hide_application() {
    let Some(marker) = MainThreadMarker::new() else {
        tracing::warn!("cannot hide Hex outside the main thread");
        return;
    };
    NSApplication::sharedApplication(marker).hide(None);
}

pub(crate) fn path() -> Result<PathBuf> {
    Ok(crate::app_paths::support_dir()?.join("settings.json"))
}

/// Whether settings were ever saved; a first run has none and onboards.
#[cfg_attr(target_os = "macos", allow(dead_code))]
pub fn exists() -> bool {
    path().is_ok_and(|path| path.exists())
}

#[cfg(test)]
mod tests {
    use super::*;

    /// GNOME's window menu and KDE's KRunner take Alt+Space, so new setups
    /// there dictate with Ctrl+Alt+Space; it shares no key with Paste last or
    /// Annotate.
    #[cfg(target_os = "linux")]
    #[test]
    fn gnome_dictates_with_a_key_gnome_leaves_free() {
        assert_eq!(
            HotkeyBinding::linux_dictation_default(false).keycaps(),
            ["Alt", "Space"]
        );
        let gnome = HotkeyBinding::linux_dictation_default(true);
        assert_eq!(gnome.keycaps(), ["Ctrl", "Alt", "Space"]);
        assert_ne!(gnome, HotkeyBinding::paste_last_default());
        #[cfg(hex_capture)]
        assert_ne!(gnome, HotkeyBinding::capture_default());
    }

    /// Annotate is on out of the box, so its shortcut and the rest of the
    /// defaults share no key on any OS.
    #[cfg(hex_capture)]
    #[test]
    fn annotates_default_shortcuts_share_no_key() {
        let settings = AppSettings::default();
        assert!(settings.capture.enabled);
        assert!(!settings.hotkey_in_use(HotkeySlot::Capture, &settings.capture.hotkey));
    }

    /// A settings file from before Annotate was on by default turns it on
    /// once.
    #[cfg(hex_capture)]
    #[test]
    fn annotate_turns_on_once_for_older_settings() {
        let mut settings = AppSettings {
            migrations: vec![STREAMING_ENGLISH_MIGRATION.into()],
            ..AppSettings::default()
        };
        settings.capture.enabled = false;
        settings.apply_one_time_migrations_with(|_| true);
        assert!(settings.capture.enabled);
        settings.capture.enabled = false;
        settings.apply_one_time_migrations_with(|_| true);
        assert!(!settings.capture.enabled, "only once");
    }

    /// The two retired summary switches become one choice once; On this Mac
    /// wins over the old OpenCode fallback.
    #[test]
    fn retired_summary_switches_become_one_choice_once() {
        let migrated = |meetings: &str| {
            let mut settings: AppSettings =
                serde_json::from_str(&format!(r#"{{"meetings":{meetings}}}"#)).unwrap();
            settings.apply_one_time_migrations_with(|_| true);
            assert!(settings.has_migration(MEETING_SUMMARIES_MIGRATION));
            let written = serde_json::to_value(&settings).unwrap();
            assert!(written["meetings"].get("summarize_with_opencode").is_none());
            assert!(
                written["meetings"]
                    .get("summarize_with_apple_intelligence")
                    .is_none()
            );
            settings.meetings.summaries
        };
        let mac = |or: MeetingSummaries| {
            if cfg!(target_os = "macos") {
                MeetingSummaries::Mac
            } else {
                or
            }
        };
        use MeetingSummaries::{Off, OpenCode};
        assert_eq!(migrated("{}"), mac(Off));
        assert_eq!(
            migrated(
                r#"{"summarize_with_apple_intelligence":true,"summarize_with_opencode":true}"#
            ),
            mac(OpenCode)
        );
        assert_eq!(
            migrated(
                r#"{"summarize_with_apple_intelligence":false,"summarize_with_opencode":true}"#
            ),
            OpenCode
        );
        assert_eq!(
            migrated(r#"{"summarize_with_opencode":true}"#),
            mac(OpenCode)
        );
        assert_eq!(
            migrated(r#"{"summarize_with_apple_intelligence":false}"#),
            Off
        );

        // Later choices stick.
        let mut settings: AppSettings = serde_json::from_str(
            r#"{"meetings":{"summaries":"off","summarize_with_opencode":true},"migrations":["meeting-summaries-choice"]}"#,
        )
        .unwrap();
        settings.apply_one_time_migrations_with(|_| true);
        assert_eq!(settings.meetings.summaries, Off);
        assert!(AppSettings::default().has_migration(MEETING_SUMMARIES_MIGRATION));
    }

    #[test]
    fn keyboard_model_switching_is_opt_in_and_source_mappings_round_trip() {
        let mut settings: AppSettings = serde_json::from_str("{}").unwrap();
        assert!(!settings.keyboard_model_switching.enabled);
        assert!(settings.keyboard_model_switching.mappings.is_empty());
        settings.keyboard_model_switching.enabled = true;
        settings.keyboard_model_switching.mappings.insert(
            "com.apple.keylayout.Spanish".into(),
            KeyboardModelChoice {
                model: TranscriptionModelId::ParakeetV3,
                language: "es".into(),
            },
        );
        let restored: AppSettings =
            serde_json::from_str(&serde_json::to_string(&settings).unwrap()).unwrap();
        assert_eq!(
            restored.keyboard_model_switching,
            settings.keyboard_model_switching
        );
    }

    #[test]
    fn loading_drops_keyboard_mappings_to_a_language_the_model_lacks() {
        let dir = TestSettingsDir::new("keyboard-mappings");
        let mut settings = AppSettings::default();
        let russian = KeyboardModelChoice {
            model: TranscriptionModelId::ParakeetV3,
            language: "ru".into(),
        };
        settings
            .keyboard_model_switching
            .mappings
            .insert("Russian".into(), russian.clone());
        settings.keyboard_model_switching.mappings.insert(
            "Japanese".into(),
            KeyboardModelChoice {
                model: TranscriptionModelId::ParakeetUnifiedEnglish,
                language: "ja".into(),
            },
        );
        settings.write_to(&dir.path).unwrap();
        let loaded = AppSettings::load_from(&dir.path).unwrap();
        assert_eq!(
            loaded.keyboard_model_switching.mappings,
            [("Russian".to_owned(), russian)].into()
        );
    }

    #[test]
    fn app_model_switching_is_opt_in_and_mode_speech_choices_round_trip() {
        let mut settings: AppSettings = serde_json::from_str("{}").unwrap();
        assert!(!settings.labs.app_model_switching);
        assert!(settings.dictation_processing.default_mode.speech.is_none());
        let speech = KeyboardModelChoice {
            model: TranscriptionModelId::ParakeetV3,
            language: "es".into(),
        };
        settings.labs.app_model_switching = true;
        settings.dictation_processing.default_mode.speech = Some(speech.clone());
        let saved = serde_json::to_value(&settings).unwrap();
        assert_eq!(saved["labs"]["app_model_switching"], true);
        let restored: AppSettings = serde_json::from_value(saved).unwrap();
        assert!(restored.labs.app_model_switching);
        assert_eq!(
            restored.dictation_processing.default_mode.speech,
            Some(speech)
        );
        // A mode without a choice writes no `speech` at all.
        let bare: AppSettings = serde_json::from_str("{}").unwrap();
        let saved = serde_json::to_value(&bare).unwrap();
        assert!(
            saved["dictation_processing"]["default_mode"]
                .get("speech")
                .is_none()
        );
    }

    #[test]
    fn loading_drops_mode_speech_choices_that_no_longer_validate() {
        let dir = TestSettingsDir::new("mode-speech");
        let mut settings = AppSettings::default();
        let russian = KeyboardModelChoice {
            model: TranscriptionModelId::ParakeetV3,
            language: "ru".into(),
        };
        settings.dictation_processing.modes = vec![
            DictationMode {
                name: "Russian".into(),
                speech: Some(russian.clone()),
                ..DictationMode::default()
            },
            DictationMode {
                name: "Japanese".into(),
                speech: Some(KeyboardModelChoice {
                    model: TranscriptionModelId::ParakeetUnifiedEnglish,
                    language: "ja".into(),
                }),
                ..DictationMode::default()
            },
        ];
        settings.write_to(&dir.path).unwrap();
        let loaded = AppSettings::load_from(&dir.path).unwrap();
        assert_eq!(loaded.dictation_processing.modes[0].speech, Some(russian));
        assert_eq!(loaded.dictation_processing.modes[1].speech, None);
        assert_eq!(loaded.dictation_processing.modes[1].name, "Japanese");
    }

    #[test]
    fn loading_folds_saved_recognition_hints_into_the_vocabulary_once() {
        let dir = TestSettingsDir::new("folded-hints");
        let mut file = serde_json::to_value(AppSettings {
            transcription: TranscriptionSelection {
                model: TranscriptionModelId::WhisperLargeV3Turbo,
                language: "en".into(),
            },
            vocabulary: vec![VocabularyEntry {
                term: "Rune".into(),
                sounds_like: vec!["rune's".into()],
            }],
            sound_effect_volume: 0.25,
            ..AppSettings::default()
        })
        .unwrap();
        file["transcription"]["recognition_hints"] = "Hex, rune\nKaffar".into();
        file["transcription_recents"] = serde_json::json!([
            {"model": "parakeet_v3", "language": "de", "recognition_hints": "Old; Hex"},
            {"model": "qwen3_asr06_b", "language": "en", "recognition_hints": "Rodolfo"},
        ]);
        fs::write(&dir.path, serde_json::to_vec(&file).unwrap()).unwrap();
        let loaded = AppSettings::load_from(&dir.path).unwrap();
        let terms: Vec<_> = loaded
            .vocabulary
            .iter()
            .map(|entry| entry.term.as_str())
            .collect();
        assert_eq!(terms, ["Rune", "Hex", "Kaffar", "Rodolfo", "Old"]);
        assert_eq!(loaded.vocabulary[0].sounds_like, ["rune's"]);
        assert_eq!(loaded.sound_effect_volume, 0.25);
        let written = fs::read_to_string(&dir.path).unwrap();
        assert!(!written.contains("recognition_hints"));
        assert_eq!(
            AppSettings::load_from(&dir.path).unwrap().vocabulary,
            loaded.vocabulary
        );
    }

    /// Labs › Second dictation shortcut (issue 627): its binding takes
    /// effect on every OS with its toggle on, never sharing a key with
    /// another shortcut, and the other shortcuts' checks count it. Linux and
    /// Windows register a key, so a shortcut without one never runs there.
    #[test]
    fn the_second_dictation_shortcut_needs_its_lab_and_its_own_keys() {
        let function = HotkeyBinding {
            modifiers: HotkeyModifiers {
                function: true,
                ..HotkeyModifiers::default()
            },
            key: None,
        };
        let mut settings = AppSettings {
            second_dictation_hotkey: Some(function.clone()),
            ..AppSettings::default()
        };
        assert!(!settings.labs.second_dictation_shortcut);
        assert_eq!(settings.runtime_hotkeys().second_dictation, None);
        assert!(!settings.hotkey_in_use(HotkeySlot::RawDictation, &function));

        settings.labs.second_dictation_shortcut = true;
        let active = settings.runtime_hotkeys().second_dictation;
        if cfg!(target_os = "macos") {
            assert_eq!(active, Some(function.runtime()));
            assert!(settings.hotkey_in_use(HotkeySlot::RawDictation, &function));
            assert!(settings.hotkey_in_use(HotkeySlot::Dictation, &function));
        } else {
            assert_eq!(active, None, "a keyless shortcut never runs off macOS");
            assert!(!settings.hotkey_in_use(HotkeySlot::RawDictation, &function));
        }

        // A key runs on every OS, and the other shortcuts see it.
        let keyed = HotkeyBinding {
            modifiers: HotkeyModifiers {
                control: Some(ModifierSide::Either),
                shift: Some(ModifierSide::Either),
                ..HotkeyModifiers::default()
            },
            key: Some(HotkeyKey::named("f7")),
        };
        settings.second_dictation_hotkey = Some(keyed.clone());
        assert_eq!(
            settings.runtime_hotkeys().second_dictation,
            Some(keyed.runtime())
        );
        assert!(settings.hotkey_in_use(HotkeySlot::RawDictation, &keyed));

        // The dictation shortcut's own keys are refused, and a saved overlap
        // is dropped rather than matched twice.
        let dictation = settings.dictation_hotkey.clone();
        assert!(settings.hotkey_in_use(HotkeySlot::SecondDictation, &dictation));
        settings.second_dictation_hotkey = Some(dictation);
        assert_eq!(settings.runtime_hotkeys().second_dictation, None);
    }

    /// A saved Screenshots record shortcut and Annotate's own capture
    /// shortcuts never share a key, whether Annotate or the Lab is on:
    /// turning either on can't leave the record shortcut dead and let
    /// Annotate record the whole screen. Other shortcuts see the record key
    /// only while a recording is available.
    #[test]
    fn the_record_shortcut_and_annotates_capture_shortcuts_never_share_a_key() {
        let key = |label: &str| HotkeyKey {
            code: u16::from(label.as_bytes()[0]),
            label: label.to_uppercase(),
        };
        let record = HotkeyBinding {
            modifiers: HotkeyModifiers {
                command: Some(ModifierSide::Either),
                shift: Some(ModifierSide::Either),
                ..HotkeyModifiers::default()
            },
            key: Some(key("r")),
        };
        let replay = HotkeyBinding {
            modifiers: HotkeyModifiers {
                command: Some(ModifierSide::Either),
                shift: Some(ModifierSide::Either),
                ..HotkeyModifiers::default()
            },
            key: Some(key("e")),
        };
        let mut settings = AppSettings {
            capture: CaptureSettings {
                enabled: false,
                ..CaptureSettings::default()
            },
            ..AppSettings::default()
        };
        settings.capture.replay_hotkey = Some(replay.clone());
        settings.screenshots.record_hotkey = Some(record.clone());
        if cfg!(target_os = "macos") {
            // Annotate's checks count the saved record shortcut with
            // Annotate and the Lab off, so neither can take it when it's
            // turned on.
            assert!(settings.hotkey_in_use(HotkeySlot::Capture, &record));
            assert!(settings.hotkey_in_use(HotkeySlot::CaptureReplay, &record));
            // The record check counts Annotate's saved shortcuts the same
            // way.
            assert!(settings.hotkey_in_use(HotkeySlot::ScreenshotRecord, &settings.capture.hotkey));
            assert!(settings.hotkey_in_use(HotkeySlot::ScreenshotRecord, &replay));
            // Another shortcut only sees the record key while a recording
            // is available: the Lab and Annotate on.
            assert!(!settings.hotkey_in_use(HotkeySlot::RawDictation, &record));
            settings.labs.screenshots = true;
            assert!(!settings.hotkey_in_use(HotkeySlot::RawDictation, &record));
            settings.capture.enabled = true;
            assert!(settings.hotkey_in_use(HotkeySlot::RawDictation, &record));
            // Annotate's check still counts the record key with the Lab off.
            settings.labs.screenshots = false;
            assert!(settings.hotkey_in_use(HotkeySlot::Capture, &record));
        } else {
            assert!(!settings.hotkey_in_use(HotkeySlot::Capture, &record));
            assert!(
                !settings.hotkey_in_use(HotkeySlot::ScreenshotRecord, &settings.capture.hotkey)
            );
        }
    }

    /// A settings file saved before the record shortcut and Annotate's
    /// capture shortcuts were kept apart has its record shortcut cleared on
    /// load: Annotate's shortcut isn't filtered at runtime, so the chord
    /// would still record the whole screen, and a replay saved on the record
    /// chord turned replay off even with the Lab off (its default).
    #[test]
    fn a_saved_record_shortcut_sharing_annotates_chord_is_cleared_on_load() {
        let dir = TestSettingsDir::new("record-shortcut-overlap");
        let load = |record: HotkeyBinding, replay: Option<HotkeyBinding>| {
            let mut settings = AppSettings::default();
            settings.labs.annotate_replay = true;
            settings.capture.replay_hotkey = replay;
            settings.screenshots.record_hotkey = Some(record);
            fs::write(&dir.path, serde_json::to_vec(&settings).unwrap()).unwrap();
            (settings, AppSettings::load_from(&dir.path).unwrap())
        };
        let replay = HotkeyBinding {
            modifiers: HotkeyModifiers {
                command: Some(ModifierSide::Either),
                shift: Some(ModifierSide::Either),
                ..HotkeyModifiers::default()
            },
            key: Some(HotkeyKey {
                code: 7,
                label: "E".into(),
            }),
        };

        let (_, loaded) = load(HotkeyBinding::capture_default(), None);
        assert_eq!(loaded.screenshots.record_hotkey, None);
        // The repair counts as a migration, so the file loses the shortcut
        // too instead of being cleared again on every load.
        let written: serde_json::Value =
            serde_json::from_slice(&fs::read(&dir.path).unwrap()).unwrap();
        assert_eq!(
            written["screenshots"]["record_hotkey"],
            serde_json::Value::Null
        );

        let (saved, loaded) = load(replay.clone(), Some(replay.clone()));
        if cfg!(target_os = "macos") {
            assert!(saved.hotkey_in_use(HotkeySlot::CaptureReplay, &replay));
        }
        assert_eq!(loaded.screenshots.record_hotkey, None);
        assert!(!loaded.hotkey_in_use(HotkeySlot::CaptureReplay, &replay));

        // Any other chord stays saved.
        let (_, loaded) = load(replay.clone(), None);
        assert_eq!(loaded.screenshots.record_hotkey, Some(replay));

        // macOS puts Annotate's mouse-button shortcut back on its default
        // with Labs › Mouse shortcuts off, so a record shortcut saved on that
        // default is cleared after the reset, not before it.
        #[cfg(target_os = "macos")]
        {
            let mut settings = AppSettings::default();
            settings.labs.mouse_shortcuts = false;
            settings.capture.hotkey = HotkeyBinding {
                modifiers: HotkeyModifiers::default(),
                key: HotkeyKey::mouse(3),
            };
            settings.screenshots.record_hotkey = Some(HotkeyBinding::capture_default());
            fs::write(&dir.path, serde_json::to_vec(&settings).unwrap()).unwrap();
            assert!(
                !settings.hotkey_in_use(
                    HotkeySlot::ScreenshotRecord,
                    &HotkeyBinding::capture_default()
                ),
                "the reset, not the saved shortcut, makes the two share a key"
            );
            let loaded = AppSettings::load_from(&dir.path).unwrap();
            assert_eq!(loaded.capture.hotkey, HotkeyBinding::capture_default());
            assert_eq!(loaded.screenshots.record_hotkey, None);
        }
    }

    /// Labs › Transform typed text: its binding takes effect only on Linux
    /// with its toggle on, never sharing a key with another shortcut, and
    /// the other shortcuts' checks count it while it is active.
    #[test]
    fn the_typed_text_shortcut_needs_its_lab_and_its_own_keys() {
        let function = HotkeyBinding {
            modifiers: HotkeyModifiers {
                function: true,
                ..HotkeyModifiers::default()
            },
            key: None,
        };
        let mut settings = AppSettings {
            typed_text_hotkey: Some(function.clone()),
            ..AppSettings::default()
        };
        assert!(!settings.labs.typed_text);
        assert_eq!(settings.runtime_hotkeys().typed_text, None);
        assert!(!settings.hotkey_in_use(HotkeySlot::RawDictation, &function));

        settings.labs.typed_text = true;
        let active = settings.runtime_hotkeys().typed_text;
        if cfg!(target_os = "linux") {
            assert_eq!(active, Some(function.runtime()));
            assert!(settings.hotkey_in_use(HotkeySlot::RawDictation, &function));
            assert!(settings.hotkey_in_use(HotkeySlot::Dictation, &function));
        } else {
            assert_eq!(active, None);
        }

        // The dictation shortcut's own keys are refused, and a saved overlap
        // is dropped rather than matched twice.
        let dictation = settings.dictation_hotkey.clone();
        assert!(settings.hotkey_in_use(HotkeySlot::TypedText, &dictation));
        settings.typed_text_hotkey = Some(dictation);
        assert_eq!(settings.runtime_hotkeys().typed_text, None);
    }

    /// Labs › Terminal paste shortcut: its binding takes effect only on
    /// Windows and Linux with its toggle on, never sharing a key with
    /// another shortcut, and off it stays saved but inactive.
    #[test]
    fn the_terminal_paste_shortcut_needs_its_lab_and_its_own_keys() {
        let binding = HotkeyBinding {
            modifiers: HotkeyModifiers {
                control: Some(crate::app_settings::ModifierSide::Either),
                shift: Some(crate::app_settings::ModifierSide::Either),
                ..HotkeyModifiers::default()
            },
            key: Some(crate::app_settings::HotkeyKey::named("t")),
        };
        let mut settings = AppSettings {
            terminal_paste_hotkey: Some(binding.clone()),
            ..AppSettings::default()
        };
        assert!(!settings.labs.terminal_paste_shortcut);
        assert_eq!(settings.active_terminal_paste_hotkey(), None);
        assert!(!settings.hotkey_in_use(HotkeySlot::RawDictation, &binding));

        settings.labs.terminal_paste_shortcut = true;
        let active = settings.active_terminal_paste_hotkey();
        if cfg!(any(target_os = "windows", target_os = "linux")) {
            assert_eq!(active, Some(&binding));
            assert!(settings.hotkey_in_use(HotkeySlot::RawDictation, &binding));
            assert!(settings.hotkey_in_use(HotkeySlot::Dictation, &binding));
        } else {
            assert_eq!(active, None);
        }

        // The dictation shortcut's own keys are refused, and a saved overlap
        // is dropped rather than matched twice.
        let dictation = settings.dictation_hotkey.clone();
        assert!(settings.hotkey_in_use(HotkeySlot::TerminalPaste, &dictation));
        settings.terminal_paste_hotkey = Some(dictation);
        assert_eq!(settings.active_terminal_paste_hotkey(), None);

        // The binding survives a save and load while the lab is off.
        settings.labs.terminal_paste_shortcut = false;
        settings.terminal_paste_hotkey = Some(binding.clone());
        let decoded: AppSettings =
            serde_json::from_str(&serde_json::to_string(&settings).unwrap()).unwrap();
        assert_eq!(decoded.terminal_paste_hotkey, Some(binding));
        assert_eq!(decoded.active_terminal_paste_hotkey(), None);
    }

    #[test]
    fn ducking_settings_are_opt_in_bounded_and_round_trip() {
        let mut settings: AppSettings = serde_json::from_str("{}").unwrap();
        assert_eq!(
            settings.recording_audio_behavior,
            RecordingAudioBehavior::DoNothing
        );
        assert_eq!(settings.recording_duck_volume, 0.2);
        settings.recording_audio_behavior = RecordingAudioBehavior::Duck;
        settings.recording_duck_volume = 0.37;
        let decoded: AppSettings =
            serde_json::from_str(&serde_json::to_string(&settings).unwrap()).unwrap();
        assert_eq!(
            decoded.recording_audio_behavior,
            RecordingAudioBehavior::Duck
        );
        assert_eq!(decoded.recording_duck_volume, 0.37);
        for invalid in [-0.1, 1.1, f32::NAN, f32::INFINITY] {
            settings.recording_duck_volume = invalid;
            assert!(settings.validate_recording_duck_volume().is_err());
        }
        for value in [0.0, 0.2, 1.0] {
            settings.recording_duck_volume = value;
            assert!(settings.validate_recording_duck_volume().is_ok());
        }
    }

    #[test]
    fn mouse_names_have_one_portable_representation() {
        for number in 1..=32 {
            let label = format!("Mouse {number}");
            let key = HotkeyKey::named(&label);
            assert!(key.is_mouse());
            assert_eq!(key.name(), format!("mouse{number}"));
            assert_eq!(key, HotkeyKey::mouse(number - 1).unwrap());
            assert_eq!(HotkeyKey::named(&key.name()), key);
        }
        for invalid in ["mouse0", "mouse33", "mouse", "mouse-1"] {
            assert_eq!(HotkeyKey::mouse_number(invalid), None);
        }
    }

    #[test]
    fn a_saved_bare_left_click_shortcut_returns_to_the_default_on_load() {
        let left_click = HotkeyBinding {
            modifiers: HotkeyModifiers::default(),
            key: HotkeyKey::mouse(0),
        };
        assert!(left_click.is_bare_primary_click());
        let option_click = HotkeyBinding {
            modifiers: HotkeyModifiers::option(),
            key: HotkeyKey::mouse(0),
        };
        assert!(!option_click.is_bare_primary_click());
        let back_button = HotkeyBinding {
            modifiers: HotkeyModifiers::default(),
            key: HotkeyKey::mouse(3),
        };
        assert!(!back_button.is_bare_primary_click());

        let dir = TestSettingsDir::new("left-click-shortcut");
        let settings = AppSettings {
            dictation_hotkey: left_click,
            ..AppSettings::default()
        };
        fs::write(&dir.path, serde_json::to_vec(&settings).unwrap()).unwrap();
        let loaded = AppSettings::load_from(&dir.path).unwrap();
        assert_eq!(loaded.dictation_hotkey, HotkeyBinding::default());

        let mut settings = AppSettings {
            dictation_hotkey: back_button.clone(),
            ..AppSettings::default()
        };
        // macOS keeps a mouse shortcut only with Labs › Mouse shortcuts on.
        settings.labs.mouse_shortcuts = true;
        fs::write(&dir.path, serde_json::to_vec(&settings).unwrap()).unwrap();
        let loaded = AppSettings::load_from(&dir.path).unwrap();
        assert_eq!(loaded.dictation_hotkey, back_button);
    }

    #[test]
    fn the_setting_or_an_installed_driver_turns_the_voice_filter_on_and_labs_never_does() {
        if env_hex_experimental_mic() {
            return;
        }
        assert!(!voice_filter_enabled(false, false));
        assert!(voice_filter_enabled(true, false));
        assert!(voice_filter_enabled(false, true));
    }

    #[test]
    fn mouse_shortcuts_go_back_to_their_default_while_their_labs_toggle_is_off() {
        let mouse = HotkeyBinding {
            modifiers: HotkeyModifiers::default(),
            key: HotkeyKey::mouse(3),
        };
        let mut settings = AppSettings {
            dictation_hotkey: mouse.clone(),
            ..Default::default()
        };
        settings.labs.mouse_shortcuts = true;
        settings.repair_mouse_shortcuts();
        assert_eq!(settings.dictation_hotkey, mouse);
        assert!(!settings.mouse_shortcut_notice);
        settings.labs.mouse_shortcuts = false;
        settings.repair_mouse_shortcuts();
        assert_eq!(settings.dictation_hotkey, HotkeyBinding::default());
        assert!(settings.mouse_shortcut_notice);
    }

    #[test]
    fn missing_fields_receive_defaults() {
        let settings: AppSettings = serde_json::from_str("{}").unwrap();
        assert_eq!(settings.retired_commands_enabled, None);
        assert!(!settings.release_microphone_while_idle);
        assert!(settings.sound_effects);
        assert_eq!(settings.sound_effect_volume, 0.5);
        assert_eq!(settings.microphone, None);
        assert_eq!(
            settings.recording_audio_behavior,
            RecordingAudioBehavior::DoNothing
        );
        assert!(!settings.pause_browser_media);
        assert!(settings.double_tap_lock);
        assert!(!settings.double_tap_only);
        assert_eq!(settings.dictation_hotkey, HotkeyBinding::default());
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
        assert!(!settings.meetings.enabled);
        assert_eq!(settings.meetings.output_folder, None);
        assert_eq!(
            settings.meetings.summaries,
            MeetingSummaries::platform_default()
        );
        assert!(settings.meetings.show_recording_indicator);
        assert_eq!(
            settings.meetings.recording_indicator_corner,
            MeetingIndicatorCorner::TopRight
        );
        assert!(
            settings
                .meetings
                .resolved_output_folder()
                .ends_with("Hex Meetings")
        );
        assert!(settings.text_replacements.is_empty());
    }

    #[test]
    fn calendar_reminders_stay_off_until_connected_and_survive_older_files() {
        let older: AppSettings =
            serde_json::from_str(r#"{"meetings":{"enabled":true,"providers":[]}}"#).unwrap();
        assert_eq!(older.meetings.calendar, CalendarSettings::default());
        assert!(!older.meetings.calendar.connected);
        assert!(!older.meetings.calendar.reminders_active());
        assert_eq!(
            older.meetings.calendar.lead(),
            std::time::Duration::from_secs(60)
        );

        let partial: AppSettings =
            serde_json::from_str(r#"{"meetings":{"calendar":{"connected":true}}}"#).unwrap();
        assert!(partial.meetings.calendar.reminders_active());
        assert!(partial.meetings.calendar.includes("any-calendar"));

        let mut calendar = partial.meetings.calendar.clone();
        calendar.excluded_calendars.push("work".into());
        calendar.lead_minutes = 5;
        let written = serde_json::to_value(&calendar).unwrap();
        let reread: CalendarSettings = serde_json::from_value(written).unwrap();
        assert_eq!(reread, calendar);
        assert!(!reread.includes("work"));
        assert_eq!(reread.lead(), std::time::Duration::from_secs(300));

        // A hand-edited lead time outside the choices falls back to a minute.
        let odd: CalendarSettings = serde_json::from_str(r#"{"lead_minutes":7}"#).unwrap();
        assert_eq!(odd.lead(), std::time::Duration::from_secs(60));
    }

    #[test]
    fn meeting_providers_ask_by_default_and_legacy_never_offer_entries_stay_never() {
        let fresh: AppSettings = serde_json::from_str(r#"{"meetings":{"enabled":true}}"#).unwrap();
        assert!(fresh.meetings.providers.is_empty());

        let legacy: AppSettings = serde_json::from_str(
            r#"{"meetings":{"never_offer":[{"key":"problems.kitlangton.com","label":"problems.kitlangton.com"}]}}"#,
        )
        .unwrap();
        assert_eq!(
            legacy.meetings.providers,
            vec![MeetingProviderChoice {
                key: "problems.kitlangton.com".into(),
                label: "problems.kitlangton.com".into(),
                policy: MeetingPolicy::NeverOffer,
            }]
        );

        let mut meetings = legacy.meetings.clone();
        assert!(set_meeting_provider(
            &mut meetings,
            MeetingProviderChoice {
                key: "com.tinyspeck.slackmacgap".into(),
                label: "Slack huddles".into(),
                policy: MeetingPolicy::AlwaysRecord,
            }
        ));
        let written = serde_json::to_value(&meetings).unwrap();
        assert!(written.get("never_offer").is_none());
        assert_eq!(written["providers"][0]["policy"], "never_offer");
        assert_eq!(written["providers"][1]["policy"], "always_record");
        let reread: MeetingSettings = serde_json::from_value(written).unwrap();
        assert_eq!(reread, meetings);
        // Choosing Ask again keeps the row, so Settings still lists the site.
        let mut asked = reread.clone();
        assert!(set_meeting_provider(
            &mut asked,
            MeetingProviderChoice {
                policy: MeetingPolicy::Ask,
                ..reread.providers[0].clone()
            }
        ));
        assert_eq!(asked.providers.len(), 2);
        assert_eq!(asked.providers[0].policy, MeetingPolicy::Ask);
    }

    #[test]
    fn existing_meeting_settings_summarize_by_default() {
        let settings: AppSettings =
            serde_json::from_str(r#"{"meetings":{"enabled":true}}"#).unwrap();
        assert!(settings.meetings.enabled);
        assert_eq!(
            settings.meetings.summaries,
            MeetingSummaries::platform_default()
        );
        assert!(settings.meetings.show_recording_indicator);
        assert_eq!(
            settings.meetings.recording_indicator_corner,
            MeetingIndicatorCorner::TopRight
        );
    }

    fn english(model: TranscriptionModelId) -> String {
        format!(
            r#"{{"transcription":{{"model":"{}","language":"en"}}}}"#,
            model.as_str()
        )
    }

    #[test]
    fn english_dictation_switches_to_the_installed_streaming_model_once() {
        let mut settings: AppSettings =
            serde_json::from_str(&english(TranscriptionModelId::ParakeetV3)).unwrap();
        settings.apply_one_time_migrations_with(|_| true);
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

        // Switching back after the migration sticks.
        let dir = TestSettingsDir::new("streaming-english");
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
    fn english_dictation_keeps_its_model_when_the_streaming_model_is_missing() {
        let mut settings: AppSettings =
            serde_json::from_str(&english(TranscriptionModelId::ParakeetV3)).unwrap();
        settings.apply_one_time_migrations_with(|_| false);
        assert_eq!(
            settings.transcription.model,
            TranscriptionModelId::ParakeetV3
        );
        assert!(
            settings.has_migration(STREAMING_ENGLISH_MIGRATION),
            "a later download never switches the model behind the user's back"
        );
    }

    #[test]
    fn other_languages_and_auto_keep_their_model() {
        for language in ["de", "auto"] {
            let model = crate::transcription::models::choices_for_runtime(language)
                .iter()
                .map(|choice| choice.model.id)
                .find(|model| *model != TranscriptionModelId::ParakeetUnifiedEnglish)
                .expect("a model for the language");
            let dir = TestSettingsDir::new("streaming-other");
            fs::write(
                &dir.path,
                format!(
                    r#"{{"transcription":{{"model":"{}","language":"{language}"}}}}"#,
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

    #[test]
    fn vocabulary_round_trips_and_stays_out_of_files_without_one() {
        // A file from before the vocabulary loads with an empty one and is
        // not rewritten to gain the field.
        let dir = TestSettingsDir::new("vocabulary");
        let older = r#"{"sound_effects": false}"#;
        fs::write(&dir.path, older).unwrap();
        let settings = AppSettings::load_from(&dir.path).unwrap();
        assert!(settings.vocabulary.is_empty());
        let written: serde_json::Value =
            serde_json::from_str(&fs::read_to_string(&dir.path).unwrap()).unwrap();
        assert!(written.get("vocabulary").is_none());
        assert!(
            serde_json::to_value(AppSettings::default())
                .unwrap()
                .get("vocabulary")
                .is_none()
        );

        let filled = AppSettings {
            vocabulary: vec![
                VocabularyEntry {
                    term: "Cappuccino".into(),
                    sounds_like: vec!["capuchin".into(), "cap a chino".into()],
                },
                VocabularyEntry {
                    term: "Rune".into(),
                    sounds_like: vec![],
                },
            ],
            ..settings
        };
        filled.write_to(&dir.path).unwrap();
        let json: serde_json::Value =
            serde_json::from_str(&fs::read_to_string(&dir.path).unwrap()).unwrap();
        assert_eq!(
            json["vocabulary"],
            serde_json::json!([
                {"term": "Cappuccino", "sounds_like": ["capuchin", "cap a chino"]},
                "Rune",
            ])
        );
        assert_eq!(json["vocabulary_boost"], "normal");
        let reloaded = AppSettings::load_from(&dir.path).unwrap();
        assert_eq!(reloaded.vocabulary, filled.vocabulary);
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

    /// #744: a value this build can't read costs only that setting, and the
    /// file (perhaps a newer build's) is left as it was.
    #[test]
    fn an_unreadable_setting_keeps_its_default_and_the_rest_loads() {
        let dir = TestSettingsDir::new("lenient");
        let file = r#"{
            "sound_effects": false,
            "history_retention": null,
            "show_dock_icon": "sometimes",
            "meetings": {"enabled": true, "retention": "quarterly", "audio_retention": "never"},
            "recording_duck_volume": 7.0
        }"#;
        fs::write(&dir.path, file).unwrap();
        let settings = AppSettings::load_from(&dir.path).unwrap();
        let defaults = AppSettings::default();
        assert!(!settings.sound_effects);
        assert!(settings.meetings.enabled);
        assert_eq!(settings.history_retention, defaults.history_retention);
        assert_eq!(settings.show_dock_icon, defaults.show_dock_icon);
        assert_eq!(settings.meetings.retention, None);
        assert_eq!(settings.meetings.audio_retention, None);
        assert_eq!(
            settings.recording_duck_volume,
            defaults.recording_duck_volume
        );
        assert_eq!(
            fs::read_to_string(&dir.path).unwrap(),
            file,
            "not rewritten"
        );

        // `null` reads as unchosen, and unchosen is never written as `null`,
        // which 3.1.0 can't read.
        fs::write(
            &dir.path,
            r#"{"meetings": {"retention": null, "audio_retention": null}}"#,
        )
        .unwrap();
        let settings = AppSettings::load_from(&dir.path).unwrap();
        assert_eq!(settings.meetings.retention, None);
        assert_eq!(settings.meetings.audio_retention, None);
        let saved = serde_json::to_value(AppSettings::default()).unwrap();
        assert!(saved["meetings"].get("retention").is_none());
        assert!(saved["meetings"].get("audio_retention").is_none());

        // A chosen audio retention survives a save and load.
        fs::write(&dir.path, r#"{"meetings": {"audio_retention": "week"}}"#).unwrap();
        let settings = AppSettings::load_from(&dir.path).unwrap();
        assert_eq!(
            settings.meetings.audio_retention,
            Some(crate::app_settings::MeetingRetention::Week)
        );

        // Even malformed JSON defaults in memory and stays in place.
        fs::write(&dir.path, "{ not json").unwrap();
        assert_eq!(AppSettings::load_from(&dir.path).unwrap(), defaults);
        assert_eq!(fs::read_to_string(&dir.path).unwrap(), "{ not json");
    }

    #[cfg(target_os = "macos")]
    #[test]
    fn new_settings_require_onboarding_across_reloads() {
        use crate::desktop::onboarding::{completion_recorded_at, record_completion_at};
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
        assert!(crate::desktop::onboarding::completion_recorded_at(
            &dir.directory
        ));
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
    fn legacy_sleep_prevention_setting_is_ignored() {
        let settings: AppSettings =
            serde_json::from_str(r#"{"prevent_system_sleep":false}"#).unwrap();
        let serialized = serde_json::to_value(settings).unwrap();

        assert!(serialized.get("prevent_system_sleep").is_none());
    }

    #[test]
    fn an_unset_microphone_is_system_default_and_a_saved_one_is_kept() {
        let automatic: AppSettings = serde_json::from_str(r#"{"microphone":null}"#).unwrap();
        assert_eq!(automatic.microphone, None);
        let absent: AppSettings = serde_json::from_str("{}").unwrap();
        assert_eq!(absent.microphone, None);
        let saved: AppSettings =
            serde_json::from_str(r#"{"microphone":"Studio Display Microphone"}"#).unwrap();
        assert_eq!(
            saved.microphone.as_deref(),
            Some("Studio Display Microphone")
        );
    }

    #[test]
    fn retired_commands_settings_load_cleanly_and_survive_saves() {
        let directory = std::env::temp_dir().join(format!(
            "hex-retired-commands-{}-{}",
            std::process::id(),
            SETTINGS_TEMP_SEQUENCE.fetch_add(1, Ordering::Relaxed)
        ));
        fs::create_dir_all(&directory).unwrap();
        let path = directory.join("settings.json");

        let mut initial = serde_json::to_value(AppSettings::default()).unwrap();
        assert!(initial.get("commands_enabled").is_none());
        initial["commands_enabled"] = serde_json::json!(true);
        initial["release_microphone_while_idle"] = serde_json::json!(true);
        let raw = serde_json::to_vec_pretty(&initial).unwrap();
        fs::write(&path, &raw).unwrap();

        let mut loaded = AppSettings::load_from(&path).unwrap();
        assert!(loaded.release_microphone_while_idle);
        assert_eq!(
            fs::read(&path).unwrap(),
            raw,
            "loading already-normalized settings does not rewrite retired keys away"
        );

        loaded.sound_effect_volume = 0.75;
        loaded.write_to(&path).unwrap();
        let saved: serde_json::Value = serde_json::from_slice(&fs::read(&path).unwrap()).unwrap();
        assert_eq!(saved["commands_enabled"], serde_json::json!(true));
        assert_eq!(
            saved["release_microphone_while_idle"],
            serde_json::json!(true)
        );

        let _ = fs::remove_dir_all(directory);
    }

    #[test]
    fn retired_voice_action_settings_load_cleanly_and_survive_saves() {
        let directory = std::env::temp_dir().join(format!(
            "hex-retired-voice-action-{}-{}",
            std::process::id(),
            SETTINGS_TEMP_SEQUENCE.fetch_add(1, Ordering::Relaxed)
        ));
        fs::create_dir_all(&directory).unwrap();
        let path = directory.join("settings.json");

        let mut initial = serde_json::to_value(AppSettings::default()).unwrap();
        initial["edit_hotkey"] = serde_json::json!({
            "modifiers": {"option": "either", "command": "either"},
            "key": null
        });
        initial["voice_action"] = serde_json::json!({
            "enabled": true,
            "model": "anthropic/claude-sonnet-4-5",
            "variant": "fast",
            "deadline_seconds": 45
        });
        let raw = serde_json::to_vec_pretty(&initial).unwrap();
        fs::write(&path, &raw).unwrap();

        let mut loaded = AppSettings::load_from(&path).unwrap();
        assert_eq!(
            fs::read(&path).unwrap(),
            raw,
            "loading already-normalized settings does not rewrite retired keys away"
        );

        loaded.sound_effect_volume = 0.75;
        loaded.write_to(&path).unwrap();
        let saved: serde_json::Value = serde_json::from_slice(&fs::read(&path).unwrap()).unwrap();
        assert_eq!(saved["edit_hotkey"], initial["edit_hotkey"]);
        assert_eq!(saved["voice_action"], initial["voice_action"]);

        let null_edit: AppSettings = serde_json::from_str(r#"{"edit_hotkey":null}"#).unwrap();
        assert_eq!(
            serde_json::to_value(null_edit).unwrap()["edit_hotkey"],
            serde_json::Value::Null
        );

        let _ = fs::remove_dir_all(directory);
    }

    #[test]
    fn transcription_selection_round_trips() {
        let settings = AppSettings {
            transcription: TranscriptionSelection {
                model: crate::transcription::models::TranscriptionModelId::WhisperLargeV3Turbo,
                language: "zh".into(),
            },
            ..AppSettings::default()
        };

        let encoded = serde_json::to_string(&settings).unwrap();
        let decoded: AppSettings = serde_json::from_str(&encoded).unwrap();

        assert_eq!(decoded.transcription, settings.transcription);
        assert!(crate::transcription::models::validate(&decoded.transcription).is_ok());
    }

    #[test]
    fn model_switches_remember_language_without_changing_other_settings() {
        let french = TranscriptionSelection {
            model: TranscriptionModelId::WhisperLargeV3Turbo,
            language: "fr".into(),
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
    fn saving_a_selection_persists_it_and_keeps_other_settings() {
        let dir = TestSettingsDir::new("save-selection");
        let mut settings = AppSettings {
            sound_effects: false,
            microphone: Some("Test microphone".into()),
            ..AppSettings::default()
        };
        let previous = settings.transcription.clone();
        let selection = TranscriptionSelection {
            model: TranscriptionModelId::WhisperLargeV3Turbo,
            language: "fr".into(),
        };
        settings
            .save_transcription_with(selection.clone(), |next| next.write_to(&dir.path))
            .unwrap();

        let (persisted, _) = AppSettings::read_from(&dir.path).unwrap();
        assert_eq!(persisted.transcription, selection);
        assert_eq!(persisted.microphone.as_deref(), Some("Test microphone"));
        assert!(!persisted.sound_effects);
        assert!(persisted.transcription_recents.contains(&previous));
    }

    #[test]
    fn invalid_remembered_languages_are_not_offered_by_model_switching() {
        let mut settings = AppSettings {
            transcription_recents: vec![
                TranscriptionSelection {
                    model: TranscriptionModelId::ParakeetV2,
                    language: "fr".into(),
                },
                TranscriptionSelection {
                    model: TranscriptionModelId::WhisperLargeV3Turbo,
                    language: "not-a-language".into(),
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
                model: crate::transcription::models::TranscriptionModelId::WhisperLargeV3Turbo,
                language: crate::transcription::models::AUTO_LANGUAGE.into(),
            },
            ..AppSettings::default()
        };

        let encoded = serde_json::to_string(&settings).unwrap();
        let decoded: AppSettings = serde_json::from_str(&encoded).unwrap();

        assert!(encoded.contains(r#""language":"auto""#));
        assert_eq!(decoded.transcription, settings.transcription);
        assert!(crate::transcription::models::validate(&decoded.transcription).is_ok());
    }

    #[test]
    fn disabled_apple_speech_selection_migrates_to_the_default_model() {
        let mut settings: AppSettings =
            serde_json::from_str(r#"{"transcription":{"model":"apple_speech","language":"de"}}"#)
                .unwrap();

        settings.migrate_disabled_transcription_model();
        assert_eq!(settings.transcription, TranscriptionSelection::default());
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
                // Both sides held hold a sided binding's key, but only an
                // either-side binding matches them exactly.
                for (device_flags, held, exact) in [
                    (0, side == "either", side == "either"),
                    (left, side != "right", side != "right"),
                    (right, side != "left", side != "left"),
                    (left | right, true, side == "either"),
                ] {
                    let flags = general | device_flags;
                    assert_eq!(
                        binding.required_modifiers_down(flags),
                        held,
                        "{side:?}, flags={flags:#x}"
                    );
                    assert_eq!(
                        binding.matches_key_press(0, flags),
                        exact,
                        "{side:?}, flags={flags:#x}"
                    );
                    assert!(!binding.exact_modifiers(flags | FUNCTION_KEY_MASK));
                    assert_eq!(
                        binding.required_modifiers_down(flags | FUNCTION_KEY_MASK),
                        held
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

    /// Launched on Dvorak, then switched to U.S. (t1304): letter shortcuts,
    /// saved or default, follow the layout the main thread published; other
    /// keys keep their recorded code.
    #[cfg(target_os = "macos")]
    #[test]
    fn letter_shortcuts_resolve_again_on_a_new_layout() {
        use super::hotkeys::test_layout;
        let resolve = |settings: Option<&AppSettings>| {
            settings.map_or_else(RuntimeHotkeys::default, AppSettings::runtime_hotkeys)
        };
        test_layout::set([('v', 47)]);
        let mut settings = AppSettings::default();
        settings.meetings.enabled = true;
        settings.raw_dictation_hotkey = Some(HotkeyBinding {
            modifiers: HotkeyModifiers::option(),
            key: Some(HotkeyKey {
                code: 96,
                label: "F5".into(),
            }),
        });
        let saved_on_dvorak = settings.paste_last_hotkey.clone().unwrap();
        assert_eq!(saved_on_dvorak.key.as_ref().unwrap().code, 47);
        let applied = RwLock::new(AppliedHotkeys {
            runtime: settings.runtime_hotkeys(),
            settings: Some(Arc::new(settings)),
        });
        let key = |applied: &RwLock<AppliedHotkeys>| {
            let runtime = applied.read().unwrap().runtime;
            [
                runtime.paste_last,
                runtime.paste_meeting,
                runtime.raw_dictation,
            ]
            .map(|hotkey| hotkey.and_then(|hotkey| hotkey.key_code))
        };
        assert_eq!(key(&applied), [Some(47), Some(47), Some(96)]);

        test_layout::set([('v', 9)]);
        AppliedHotkeys::resolve_again(&applied, resolve);
        assert_eq!(key(&applied), [Some(9), Some(9), Some(96)]);

        // A layout without the letter keeps the recorded key.
        test_layout::set([('x', 7)]);
        assert_eq!(saved_on_dvorak.runtime().key_code, Some(47));

        // Settings applied while it resolved win: they saw the new layout.
        test_layout::set([('v', 47)]);
        let newer = AppSettings {
            paste_last_hotkey: None,
            ..AppSettings::default()
        };
        AppliedHotkeys::resolve_again(&applied, |settings| {
            let resolved = resolve(settings);
            *applied.write().unwrap() = AppliedHotkeys {
                runtime: newer.runtime_hotkeys(),
                settings: Some(Arc::new(newer.clone())),
            };
            resolved
        });
        assert_eq!(key(&applied)[0], None);
    }

    #[test]
    fn side_selection_changes_one_modifier_chords_only() {
        let mut standalone = HotkeyBinding {
            modifiers: HotkeyModifiers {
                option: Some(ModifierSide::Left),
                ..Default::default()
            },
            key: None,
        };
        assert_eq!(standalone.standalone_side(), Some(ModifierSide::Left));
        standalone.set_standalone_side(ModifierSide::Right);
        assert_eq!(standalone.modifiers.option, Some(ModifierSide::Right));
        standalone.set_standalone_side(ModifierSide::Both);
        assert_eq!(standalone.modifiers.option, Some(ModifierSide::Right));

        for modifiers in [
            HotkeyModifiers {
                option: Some(ModifierSide::Left),
                command: Some(ModifierSide::Left),
                ..Default::default()
            },
            HotkeyModifiers {
                option: Some(ModifierSide::Both),
                ..Default::default()
            },
        ] {
            let chord = HotkeyBinding {
                modifiers,
                key: None,
            };
            assert_eq!(chord.standalone_side(), None, "{modifiers:?}");
        }

        let mut chord = HotkeyBinding {
            modifiers: HotkeyModifiers {
                option: Some(ModifierSide::Left),
                ..Default::default()
            },
            key: Some(HotkeyKey {
                code: 0,
                label: "A".into(),
            }),
        };
        assert_eq!(chord.standalone_side(), None);
        chord.set_standalone_side(ModifierSide::Right);
        assert_eq!(chord.modifiers.option, Some(ModifierSide::Left));
    }

    #[test]
    fn shortcuts_store_as_chords_and_old_files_convert_once() {
        let both_options = HotkeyBinding {
            modifiers: HotkeyModifiers {
                option: Some(ModifierSide::Both),
                ..Default::default()
            },
            key: None,
        };
        assert_eq!(
            serde_json::to_value(&both_options).unwrap(),
            serde_json::json!({ "keys": [
                { "modifier": "option", "side": "left" },
                { "modifier": "option", "side": "right" },
            ]})
        );
        let chord: HotkeyBinding = serde_json::from_value(serde_json::json!({ "keys": [
            { "code": 49, "label": "Space" },
            { "modifier": "option", "side": "right" },
            { "modifier": "option", "side": "left" },
        ]}))
        .unwrap();
        assert_eq!(
            chord.modifiers.option,
            Some(ModifierSide::Both),
            "any order"
        );
        assert_eq!(chord.key.unwrap().code, 49);
        for refused in [
            serde_json::json!({ "keys": [
                { "modifier": "option", "side": "left" },
                { "modifier": "option", "side": "left" },
            ]}),
            serde_json::json!({ "keys": [
                { "modifier": "option", "side": "any" },
                { "modifier": "option", "side": "right" },
            ]}),
            serde_json::json!({ "keys": [
                { "code": 49, "label": "Space" },
                { "code": 0, "label": "A" },
            ]}),
            serde_json::json!({ "keys": [{ "modifier": "hyper" }] }),
        ] {
            assert!(
                serde_json::from_value::<HotkeyBinding>(refused.clone()).is_err(),
                "{refused}"
            );
        }

        // Every shortcut in a file from before chords is the same chord after.
        let legacy = serde_json::json!({
            "dictation_hotkey": { "modifiers": { "option": "right" }, "key": null },
            "edit_hotkey": { "modifiers": { "option": true, "command": true, "control": false }, "key": null },
            "paste_last_hotkey": {
                "modifiers": { "option": "either", "shift": "either" },
                "key": { "code": 9, "label": "V" }
            },
        });
        let settings: AppSettings = serde_json::from_value(legacy.clone()).unwrap();
        assert_eq!(
            settings.dictation_hotkey.modifiers,
            HotkeyModifiers {
                option: Some(ModifierSide::Right),
                ..Default::default()
            }
        );
        let stored = serde_json::to_value(&settings).unwrap();
        assert_eq!(stored["edit_hotkey"], legacy["edit_hotkey"]);
        assert_eq!(
            stored["dictation_hotkey"],
            serde_json::json!({ "keys": [{ "modifier": "option", "side": "right" }] })
        );
        assert_eq!(
            stored["paste_last_hotkey"],
            serde_json::json!({ "keys": [
                { "modifier": "option", "side": "any" },
                { "modifier": "shift", "side": "any" },
                { "code": 9, "label": "V" },
            ]})
        );
        let reloaded: AppSettings = serde_json::from_value(stored).unwrap();
        assert_eq!(reloaded.dictation_hotkey, settings.dictation_hotkey);
        assert_eq!(reloaded.retired_edit_hotkey, settings.retired_edit_hotkey);
        assert_eq!(reloaded.paste_last_hotkey, settings.paste_last_hotkey);
        for binding in [
            HotkeyBinding::default(),
            HotkeyBinding::paste_last_default(),
            HotkeyBinding::paste_meeting_default(),
        ] {
            let json = serde_json::to_value(&binding).unwrap();
            assert_eq!(
                serde_json::from_value::<HotkeyBinding>(json).unwrap(),
                binding
            );
        }
    }

    #[test]
    fn both_sides_conflict_only_with_either_side_and_match_only_together() {
        let option = |side| HotkeyBinding {
            modifiers: HotkeyModifiers {
                option: Some(side),
                ..Default::default()
            },
            key: None,
        };
        let both = option(ModifierSide::Both);
        assert!(both.overlaps(&option(ModifierSide::Either)));
        assert!(!both.overlaps(&option(ModifierSide::Left)));
        assert!(!both.overlaps(&option(ModifierSide::Right)));
        assert_eq!(both.modifiers.count(), 2);
        let both = both.runtime();
        let general = OPTION_KEY_MASK;
        assert!(both.exact_modifiers(general | LEFT_OPTION_MASK | RIGHT_OPTION_MASK));
        assert!(!both.exact_modifiers(general | LEFT_OPTION_MASK));
        assert!(!both.exact_modifiers(general | RIGHT_OPTION_MASK));
        assert!(!both.exact_modifiers(general), "input naming no side");
        assert!(
            !both
                .exact_modifiers(general | LEFT_OPTION_MASK | RIGHT_OPTION_MASK | COMMAND_KEY_MASK)
        );
        assert!(both.required_modifiers_down(
            general | LEFT_OPTION_MASK | RIGHT_OPTION_MASK | COMMAND_KEY_MASK
        ));
        assert_eq!(
            both.both_sides_masks().collect::<Vec<_>>(),
            [(LEFT_OPTION_MASK, RIGHT_OPTION_MASK)]
        );
        assert_eq!(
            option(ModifierSide::Either)
                .runtime()
                .both_sides_masks()
                .count(),
            0
        );
    }

    #[test]
    fn recorded_sides_keep_one_modifiers_side_and_both_sides() {
        let lone = HotkeyModifiers {
            option: Some(ModifierSide::Right),
            ..Default::default()
        };
        assert_eq!(lone.recorded_sides(), lone);
        let pair = HotkeyModifiers {
            option: Some(ModifierSide::Left),
            command: Some(ModifierSide::Right),
            ..Default::default()
        };
        assert_eq!(pair.recorded_sides(), HotkeyModifiers::option_command());
        let both = HotkeyModifiers {
            option: Some(ModifierSide::Both),
            shift: Some(ModifierSide::Left),
            ..Default::default()
        };
        assert_eq!(
            both.recorded_sides(),
            HotkeyModifiers {
                option: Some(ModifierSide::Both),
                shift: Some(ModifierSide::Either),
                ..Default::default()
            }
        );
    }

    #[cfg(hex_capture)]
    #[test]
    fn scrawls_routing_imports_without_its_jev_key() {
        let engine = br#"{
            "autoSend": false,
            "jevApiKey": "not-a-real-key",
            "experiments": { "smartRouting": true, "jevRouting": "on" },
            "routing": {
                "sites": { "http://localhost:5173": "~/code/site" },
                "undoWindowSec": 120
            }
        }"#;
        let mut settings = AppSettings::default();
        settings.import_scrawl_routing(Some(engine));
        let config = settings.capture.routing.config();
        assert!(!config.send_captures);
        assert!(config.smart_routing && config.jev_routing);
        assert_eq!(config.undo_window_sec, Some(120));
        assert_eq!(
            config
                .sites
                .get("http://localhost:5173")
                .map(String::as_str),
            Some("~/code/site")
        );
        assert_eq!(config.jev_api_key, None);

        let saved = serde_json::to_string(&settings).unwrap();
        assert!(!saved.contains("not-a-real-key"));
        let read: AppSettings = serde_json::from_str(&saved).unwrap();
        assert_eq!(read.capture.routing, settings.capture.routing);

        // A key typed into settings.json is dropped on read.
        let typed = saved.replace("\"jevApiKey\":null", "\"jevApiKey\":\"x\"");
        assert_ne!(typed, saved);
        let read: AppSettings = serde_json::from_str(&typed).unwrap();
        assert_eq!(read.capture.routing.config().jev_api_key, None);

        // Nothing to read keeps the defaults.
        let mut settings = AppSettings::default();
        settings.import_scrawl_routing(None);
        settings.import_scrawl_routing(Some(b"not json"));
        assert_eq!(settings.capture.routing, CaptureRouting::default());
    }
}
