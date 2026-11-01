//! The `hex settings` registry: one key per persisted setting, named by its
//! dotted path in `settings.json`, with the same rules and side effects the
//! window applies when you change it there.

use serde::{Deserialize, Serialize};
use serde_json::Value;

use super::ControlError;
use crate::app_settings::{
    AppSettings, CALENDAR_LEAD_MINUTES, HotkeyBinding, HotkeyKey, HotkeyModifiers, HotkeySlot,
    ModifierSide,
};

/// One setting as `hex settings get` reports it.
#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
pub struct SettingValue {
    pub key: String,
    pub value: Value,
    /// bool, number, integer, text, choice, shortcut, path, or json.
    pub kind: String,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub choices: Vec<String>,
    pub writable: bool,
    pub about: String,
    /// False where this OS's Hex lacks what the setting needs ([`needs`]):
    /// it is kept, but does nothing here and `set` refuses it.
    #[serde(default = "available")]
    pub available: bool,
}

fn available() -> bool {
    true
}

#[derive(Clone, Copy, Debug)]
enum Kind {
    Bool,
    Number {
        min: f64,
        max: f64,
    },
    #[cfg_attr(not(hex_capture), allow(dead_code))]
    Integer {
        min: u64,
        max: u64,
    },
    Choice(&'static [&'static str]),
    Text,
    Shortcut(HotkeySlot),
    Path,
    Json,
}

impl Kind {
    fn name(self) -> &'static str {
        match self {
            Self::Bool => "bool",
            Self::Number { .. } => "number",
            Self::Integer { .. } => "integer",
            Self::Choice(_) => "choice",
            Self::Text => "text",
            Self::Shortcut(_) => "shortcut",
            Self::Path => "path",
            Self::Json => "json",
        }
    }
}

struct Key {
    key: &'static str,
    kind: Kind,
    /// `none` clears it.
    optional: bool,
    /// Why it can't be set here, if it can't.
    read_only: Option<&'static str>,
    about: &'static str,
}

const fn key(key: &'static str, kind: Kind, about: &'static str) -> Key {
    Key {
        key,
        kind,
        optional: false,
        read_only: None,
        about,
    }
}

const fn optional(key: &'static str, kind: Kind, about: &'static str) -> Key {
    Key {
        key,
        kind,
        optional: true,
        read_only: None,
        about,
    }
}

const fn read_only(key: &'static str, kind: Kind, about: &'static str, why: &'static str) -> Key {
    Key {
        key,
        kind,
        optional: false,
        read_only: Some(why),
        about,
    }
}

const UNIT: Kind = Kind::Number { min: 0.0, max: 1.0 };
const MODEL_CHANGE: &str = "change the transcription model with `hex model use`";

/// Choice keys whose values are numbers in settings.json.
const NUMERIC_CHOICES: [&str; 2] = ["meetings.calendar.lead_minutes", "labs.unload_idle_minutes"];

const KEYS: &[Key] = &[
    key(
        "keyboard_model_switching.enabled",
        Kind::Bool,
        "Labs: switch the model when the keyboard input source changes (macOS, Windows, X11)",
    ),
    key(
        "keyboard_model_switching.mappings",
        Kind::Json,
        "Input-source IDs mapped to a model and language; absent sources don't switch (macOS, Windows, X11)",
    ),
    key(
        "release_microphone_while_idle",
        Kind::Bool,
        "Release the microphone between dictations",
    ),
    key(
        "sound_effects",
        Kind::Bool,
        "Play recording and error tones",
    ),
    key(
        "sound_effect_volume",
        UNIT,
        "Tone volume from 0 to 1; 0 turns tones off",
    ),
    key(
        "sound_theme",
        Kind::Choice(&[
            "signature",
            "classic",
            "click",
            "soft",
            "wood",
            "analog",
            "thock",
        ]),
        "Which sounds play; classic, Hex's original tones, is the default",
    ),
    optional(
        "microphone",
        Kind::Text,
        "Microphone by name; system-default (or none) follows the system's default input",
    ),
    key(
        "avoid_bluetooth_microphone",
        Kind::Bool,
        "System default records from the built-in or another wired microphone, not a Bluetooth headset's",
    ),
    key(
        "recording_audio_behavior",
        Kind::Choice(&["mute", "pause_media", "do_nothing", "duck"]),
        "What other audio does while you dictate",
    ),
    key(
        "recording_duck_volume",
        UNIT,
        "Fraction of original output volume while lowering audio, from 0 to 1",
    ),
    key(
        "pause_browser_media",
        Kind::Bool,
        "Labs: also pause a playing browser tab while dictating (macOS, with Pause media)",
    ),
    key(
        "double_tap_lock",
        Kind::Bool,
        "Double-tap the dictation shortcut to keep recording hands-free",
    ),
    key(
        "double_tap_only",
        Kind::Bool,
        "Only a double tap starts dictation (needs double_tap_lock and a key chord)",
    ),
    key(
        "shortcut_mode",
        Kind::Choice(&["hold", "toggle"]),
        "Hold the shortcut while speaking, or press once to start and again to finish",
    ),
    key(
        "dictation_hotkey",
        Kind::Shortcut(HotkeySlot::Dictation),
        "The dictation shortcut",
    ),
    optional(
        "paste_last_hotkey",
        Kind::Shortcut(HotkeySlot::PasteLast),
        "The paste-last-dictation shortcut; none turns it off",
    ),
    optional(
        "raw_dictation_hotkey",
        Kind::Shortcut(HotkeySlot::RawDictation),
        "Dictate without processing: dictates and pastes the words as heard, skipping \
         the Output steps; none (the default) turns it off",
    ),
    optional(
        "second_dictation_hotkey",
        Kind::Shortcut(HotkeySlot::SecondDictation),
        "Labs: another shortcut that dictates like dictation_hotkey, such as a key on an \
         external keyboard (macOS, with labs.second_dictation_shortcut); none (the default) \
         turns it off",
    ),
    key(
        "show_dock_icon",
        Kind::Bool,
        "Keep Hex in the Dock when Settings is closed (macOS)",
    ),
    read_only(
        "transcription.model",
        Kind::Text,
        "The dictation model",
        MODEL_CHANGE,
    ),
    read_only(
        "transcription.language",
        Kind::Text,
        "The dictation language",
        MODEL_CHANGE,
    ),
    read_only(
        "transcription_recents",
        Kind::Json,
        "Recently used models, each with its language",
        "Hex records these as you switch models",
    ),
    key(
        "vocabulary_boost",
        Kind::Choice(&["off", "light", "normal", "strong"]),
        "How readily transcription swaps heard words for a vocabulary term",
    ),
    key(
        "dictation_processing.default_mode",
        Kind::Json,
        "The Global mode: replacements, transformations, and rewriting",
    ),
    key(
        "dictation_processing.modes",
        Kind::Json,
        "Modes for specific apps and websites, as a JSON array",
    ),
    key(
        "history_retention",
        Kind::Choice(&["off", "day", "week", "month", "forever"]),
        "How long dictation history is kept",
    ),
    key(
        "labs.live_text",
        Kind::Bool,
        "Labs: provisional words beside the HUD while dictating",
    ),
    key(
        "labs.live_chapters",
        Kind::Bool,
        "Labs: chapter meeting transcripts while recording",
    ),
    key(
        "labs.playground",
        Kind::Bool,
        "Labs: the scene and story browser in the window",
    ),
    key(
        "labs.vocabulary_biasing",
        Kind::Bool,
        "Labs: bias Parakeet transcription toward the vocabulary",
    ),
    key(
        "labs.mouse_shortcuts",
        Kind::Bool,
        "Labs: record mouse buttons as shortcuts (macOS)",
    ),
    key(
        "labs.unload_idle",
        Kind::Bool,
        "Labs: unload the speech model when idle, freeing its memory",
    ),
    key(
        "labs.unload_idle_minutes",
        Kind::Choice(&["5", "15", "30", "60"]),
        "Minutes unused before Labs › Unload model when idle unloads the model",
    ),
    key(
        "labs.restore_shortcut_default",
        Kind::Bool,
        "Labs: offer Restore default beside the Paste last dictation shortcut",
    ),
    key(
        "labs.keep_on_clipboard",
        Kind::Bool,
        "Labs: leave the dictation on the clipboard after pasting instead of restoring what was there (macOS)",
    ),
    key(
        "labs.copy_only",
        Kind::Bool,
        "Labs: put the dictation on the clipboard instead of pasting it (macOS)",
    ),
    key(
        "labs.spoken_formatting",
        Kind::Bool,
        "Labs: turn spoken \"new line\", \"new paragraph\", and \"open/close quote\" into line breaks and quotation marks",
    ),
    key(
        "labs.spoken_symbols",
        Kind::Bool,
        "Labs: turn spoken \"comma\", \"colon\", \"hashtag\", \"underscore\", and similar words into symbols joined to their words",
    ),
    key(
        "labs.filler_words",
        Kind::Bool,
        "Labs: remove filler words such as um and uh from dictations, without an AI model",
    ),
    key(
        "labs.subtitle_export",
        Kind::Bool,
        "Labs: export meeting transcripts as SRT or VTT subtitles with hex meetings show --srt|--vtt",
    ),
    key(
        "labs.second_dictation_shortcut",
        Kind::Bool,
        "Labs: second_dictation_hotkey dictates like the dictation shortcut (macOS)",
    ),
    key(
        "labs.smart_spacing",
        Kind::Bool,
        "Labs: add a space before a dictation pasted right after a word (macOS)",
    ),
    key(
        "labs.picker_sort_filter",
        Kind::Bool,
        "Labs: sort the Local transcription picker by recommendation, size, or name, and show only models for the saved language",
    ),
    key(
        "labs.sided_modifier_chords",
        Kind::Bool,
        "Labs: the shortcut recorder keeps which side each modifier of a modifier-only chord is on, such as Right Command + Right Option (macOS)",
    ),
    key(
        "labs.tap_or_hold",
        Kind::Bool,
        "Labs: in Toggle mode, a press held past the hold threshold dictates until it lifts; a short tap still starts and stops (macOS)",
    ),
    key(
        "labs.benchmark_details",
        Kind::Bool,
        "Labs: the Local transcription picker shows each model's raw error rate (WER or CER), benchmark, source, and conditions",
    ),
    key(
        "labs.custom_models",
        Kind::Bool,
        "Labs: the Local transcription picker can add a GGUF speech model from disk (hex model import)",
    ),
    key(
        "labs.parakeet_ultra",
        Kind::Bool,
        "Labs: Parakeet Ultra, Moondream's post-trained Parakeet v3, in the Local transcription picker; stays on while it is the dictation model",
    ),
    key(
        "labs.standalone_special_keys",
        Kind::Bool,
        "Labs: shortcuts may use Menu, Pause, or Scroll Lock, alone or with modifiers (Linux); off puts such a shortcut back to its default",
    ),
    key(
        "meetings.enabled",
        Kind::Bool,
        "Labs: Meetings (beta), call detection, recording, and transcripts",
    ),
    optional(
        "meetings.output_folder",
        Kind::Path,
        "Folder for meeting Markdown; none uses ~/Documents/Hex Meetings",
    ),
    key(
        "meetings.summarize_with_apple_intelligence",
        Kind::Bool,
        "Title and summarize meetings with Apple Intelligence",
    ),
    key(
        "meetings.summarize_with_opencode",
        Kind::Bool,
        "Title and summarize meetings with the OpenCode model (sends the transcript to it)",
    ),
    key(
        "meetings.show_recording_indicator",
        Kind::Bool,
        "Show the floating recording indicator during meetings",
    ),
    key(
        "meetings.recording_indicator_corner",
        Kind::Choice(&["top_right", "top_left", "bottom_right", "bottom_left"]),
        "Screen corner of the meeting recording indicator",
    ),
    optional(
        "meetings.retention",
        Kind::Choice(&["day", "week", "month", "forever"]),
        "How long finished meetings are kept before they move to the Trash; none keeps new meetings 24 hours and older ones until you choose",
    ),
    key(
        "meetings.providers",
        Kind::Json,
        "Per-app call choices, as a JSON array of {key, label, policy: ask|always_record|never_offer}",
    ),
    read_only(
        "meetings.calendar.connected",
        Kind::Bool,
        "A calendar is connected for meeting reminders",
        "connect a calendar from Settings › Meetings, which asks macOS for access",
    ),
    key(
        "meetings.calendar.excluded_calendars",
        Kind::Json,
        "Calendars left out of reminders, as a JSON array of EventKit identifiers",
    ),
    key(
        "meetings.calendar.remind",
        Kind::Bool,
        "Remind before calendar meetings",
    ),
    key(
        "meetings.calendar.lead_minutes",
        Kind::Choice(&["0", "1", "2", "5"]),
        "Minutes before a meeting its reminder shows",
    ),
    key(
        "experimental.hex_microphone",
        Kind::Bool,
        "Labs: the Voice Filter and its Hex Microphone virtual input",
    ),
    key(
        "experimental.hex_microphone_mode",
        Kind::Choice(&[
            "normal",
            "muffled",
            "quiet",
            "muted",
            "chipmunk",
            "robot",
            "walkie",
            "underwater",
            "far",
        ]),
        "What Hex Microphone does to your voice while you dictate",
    ),
    key(
        "experimental.hex_microphone_comfort_noise",
        Kind::Bool,
        "Hex Microphone fills silence with soft noise",
    ),
    key(
        "experimental.hex_microphone_strength",
        UNIT,
        "How strongly Hex Microphone applies its filter, from 0 to 1",
    ),
    key(
        "enable_on_launch",
        Kind::Bool,
        "Start listening when Hex opens (Windows and Linux)",
    ),
    key(
        "paste_with_shift",
        Kind::Bool,
        "Paste with Ctrl-Shift-V for terminals (Linux and Windows)",
    ),
    key(
        "gnome_keyboard_shortcuts",
        Kind::Bool,
        "Hex's own GNOME custom keyboard shortcuts, for GNOME without a shortcuts portal (Linux)",
    ),
    key(
        "share_dictation_shortcut",
        Kind::Bool,
        "Labs: let other apps see the dictation shortcut too (Windows)",
    ),
    optional(
        "ui_language",
        Kind::Choice(&["en", "pl", "zh", "ja", "de", "es", "uk"]),
        "Interface language; none follows the system",
    ),
    key(
        "indicator_position",
        Kind::Choice(&["top", "bottom", "hidden"]),
        "Where the dictation HUD appears",
    ),
    optional(
        "updates.channel",
        Kind::Choice(&["stable", "dev"]),
        "Update channel; none follows the build (dev builds use dev, others stable)",
    ),
];

/// Captures exist only in builds with the `capture` build option.
#[cfg(hex_capture)]
const CAPTURE_KEYS: &[Key] = &[
    key(
        "capture.enabled",
        Kind::Bool,
        "Annotate: record the screen while you draw and talk (on by default)",
    ),
    key(
        "capture.hotkey",
        Kind::Shortcut(HotkeySlot::Capture),
        "The Annotate shortcut: press to start recording and again to finish",
    ),
    key(
        "capture.transcriber",
        Kind::Choice(&["parakeet", "apple"]),
        "What transcribes a capture's narration: Hex's loaded model or Apple's",
    ),
    optional(
        "capture.folder",
        Kind::Path,
        "Folder for new captures; none uses Scrawl's captures folder",
    ),
    optional(
        "capture.media_days",
        RETENTION_DAYS,
        "Days a capture keeps its video and audio; none keeps them",
    ),
    optional(
        "capture.bundle_days",
        RETENTION_DAYS,
        "Days a capture is kept; none keeps it",
    ),
    key(
        "capture.capsule_flow",
        Kind::Choice(&["list", "toast", "menu_bar", "cursor", "notch"]),
        "How the capture capsule offers where to send a capture (prototypes): a list, a \
         one-line toast, the menu bar, a card at the pointer, or a notch",
    ),
    key(
        "capture.routing",
        Kind::Json,
        "Where captures go among OpenCode sessions: Scrawl's router settings (autoSend, sites, apps, smartRouting, …) as a JSON object",
    ),
    key(
        "capture.show_overlays_in_recordings",
        Kind::Bool,
        "Let screen recordings, captures included, show Hex's capsules, ink, and HUD (for demos)",
    ),
];
#[cfg(hex_capture)]
const RETENTION_DAYS: Kind = Kind::Integer { min: 1, max: 3650 };
#[cfg(not(hex_capture))]
const CAPTURE_KEYS: &[Key] = &[];

/// Settings that act only where Hex has a capability (by its
/// `rune_ui::capabilities` name), the same one that shows their row in the
/// window. The first matching prefix decides.
const NEEDS: &[(&str, &str)] = &[
    ("meetings.calendar.", "calendar"),
    ("meetings.", "meetings"),
    ("labs.live_chapters", "meetings"),
    ("labs.subtitle_export", "meetings"),
    ("experimental.hex_microphone", "fun"),
    ("show_dock_icon", "dock_icon"),
    ("keyboard_model_switching.", "keyboard_model_switching"),
    ("enable_on_launch", "listener_control"),
    ("share_dictation_shortcut", "share_shortcut"),
];

/// The capability `key` needs, if any.
fn needs(key: &str) -> Option<&'static str> {
    NEEDS
        .iter()
        .find(|(prefix, _)| key == *prefix || key.starts_with(prefix))
        .map(|(_, capability)| *capability)
}

/// Why `hex settings set` refuses `name` on this OS, if it does.
pub fn unavailable(name: &str) -> Option<ControlError> {
    if let Some(feature) = crate::labs::Feature::for_settings_key(name)
        && !feature.shipped()
    {
        return Some(ControlError::new(
            "unsupported",
            format!("`{name}` belongs to a Labs feature that only dev builds have"),
        ));
    }
    let capability = needs(name).filter(|capability| !crate::rune_ui::capability(capability))?;
    Some(ControlError::new(
        "unsupported",
        format!(
            "`{name}` does nothing on this OS: it needs {capability}, which Hex has only elsewhere"
        ),
    ))
}

/// Every key, in registry order.
fn keys() -> impl Iterator<Item = &'static Key> {
    KEYS.iter().chain(CAPTURE_KEYS)
}

/// Settings `hex settings` never shows: Hex's own bookkeeping.
#[cfg(test)]
const INTERNAL: &[&str] = &["migrations", "mouse_shortcut_notice"];

/// Settings another `hex` command edits instead (`hex vocabulary`).
#[cfg(test)]
const OWN_COMMAND: &[&str] = &["vocabulary"];

/// What the OS knows about shortcuts: how a key name resolves on the
/// active layout, and the rules recording one in Settings applies.
pub struct ShortcutRules {
    pub key_named: fn(&str) -> Result<HotkeyKey, String>,
    pub rule: fn(HotkeySlot, &HotkeyBinding) -> Result<(), String>,
}

/// Something the host does beyond saving `settings.json`.
#[derive(Clone, Debug, PartialEq)]
pub enum Effect {
    HistoryRetention(crate::history::HistoryRetention),
    DockIcon,
    /// Check the newly chosen channel now rather than within the hour.
    CheckForUpdates,
}

/// A validated change, ready to save.
#[derive(Debug)]
pub struct Change {
    pub settings: AppSettings,
    pub effects: Vec<Effect>,
    pub message: String,
}

impl Change {
    pub fn changed(&self, before: &AppSettings) -> bool {
        self.settings != *before
    }
}

/// Deprecated: recognition hints merged into the vocabulary. `get` reads
/// the vocabulary's terms, comma-separated, and `set` adds names to it;
/// `list` leaves it out.
const RECOGNITION_HINTS: &str = "transcription.recognition_hints";

fn recognition_hints(settings: &AppSettings) -> SettingValue {
    SettingValue {
        key: RECOGNITION_HINTS.into(),
        value: Value::String(
            settings
                .vocabulary
                .iter()
                .map(|entry| entry.term.as_str())
                .collect::<Vec<_>>()
                .join(", "),
        ),
        kind: Kind::Text.name().into(),
        choices: Vec::new(),
        writable: true,
        about: "Deprecated: the vocabulary's terms; setting it adds names to the vocabulary                 (use `hex vocabulary`)"
            .into(),
        available: true,
    }
}

/// Every setting, in registry order.
pub fn list(settings: &AppSettings) -> Vec<SettingValue> {
    let json = serde_json::to_value(settings).unwrap_or(Value::Null);
    keys().map(|key| value_of(key, &json)).collect()
}

/// One setting, or every setting under a prefix such as `meetings`.
pub fn get(settings: &AppSettings, name: &str) -> Result<Vec<SettingValue>, ControlError> {
    if name == RECOGNITION_HINTS {
        return Ok(vec![recognition_hints(settings)]);
    }
    let json = serde_json::to_value(settings).unwrap_or(Value::Null);
    let prefix = format!("{name}.");
    let found: Vec<_> = keys()
        .filter(|key| key.key == name || key.key.starts_with(&prefix))
        .map(|key| value_of(key, &json))
        .collect();
    if found.is_empty() {
        return Err(unknown(name));
    }
    Ok(found)
}

fn unknown(name: &str) -> ControlError {
    let near = keys()
        .map(|key| key.key)
        .find(|key| {
            key.ends_with(name)
                || name.ends_with(key)
                || key.replace('_', "") == name.replace(['_', '-'], "")
        })
        .map(|key| format!(" Did you mean `{key}`?"))
        .unwrap_or_default();
    ControlError::new(
        "unknown-setting",
        format!("unknown setting `{name}`.{near} Run `hex settings list` for every key."),
    )
}

fn value_of(key: &Key, json: &Value) -> SettingValue {
    let raw = json
        .pointer(&pointer(key.key))
        .cloned()
        .unwrap_or(Value::Null);
    let value = match key.kind {
        Kind::Shortcut(_) => match serde_json::from_value::<Option<HotkeyBinding>>(raw) {
            Ok(Some(binding)) => Value::String(format_hotkey(&binding)),
            _ => Value::Null,
        },
        Kind::Choice(_) if NUMERIC_CHOICES.contains(&key.key) => Value::String(raw.to_string()),
        _ => raw,
    };
    SettingValue {
        key: key.key.into(),
        value,
        kind: key.kind.name().into(),
        choices: match key.kind {
            Kind::Choice(choices) => choices.iter().map(|&choice| choice.into()).collect(),
            _ => Vec::new(),
        },
        writable: key.read_only.is_none(),
        about: key.about.into(),
        available: needs(key.key).is_none_or(crate::rune_ui::capability),
    }
}

fn pointer(key: &str) -> String {
    format!("/{}", key.replace('.', "/"))
}

/// Validates `raw` for `name` against `settings` and returns the settings
/// to save, applying the rules the window applies for the same change.
pub fn apply(
    settings: &AppSettings,
    name: &str,
    raw: &str,
    shortcuts: &ShortcutRules,
) -> Result<Change, ControlError> {
    if name == RECOGNITION_HINTS {
        let mut candidate = settings.clone();
        crate::dictation::vocabulary::fold_hints(&mut candidate.vocabulary, raw);
        return Ok(Change {
            settings: candidate,
            effects: Vec::new(),
            message: "Added the names to the vocabulary (transcription.recognition_hints is \
                      deprecated; use `hex vocabulary`)"
                .into(),
        });
    }
    let key = keys()
        .find(|key| key.key == name)
        .ok_or_else(|| unknown(name))?;
    if let Some(why) = key.read_only {
        return Err(ControlError::new(
            "read-only",
            format!("`{name}` can't be set here: {why}"),
        ));
    }
    let value = parse(key, raw, shortcuts)?;
    let mut json = serde_json::to_value(settings)
        .map_err(|error| ControlError::new("internal", error.to_string()))?;
    let slot = json
        .pointer_mut(&pointer(key.key))
        .ok_or_else(|| ControlError::new("internal", format!("`{name}` is missing")))?;
    *slot = value;
    let mut candidate: AppSettings =
        serde_json::from_value(json).map_err(|error| invalid(name, &format!("{error}")))?;
    let mut effects = Vec::new();
    let message = format!("Set {name}");
    match key.key {
        "sound_effect_volume" => {
            let volume = candidate.sound_effect_volume;
            candidate.sound_effects = volume > 0.0;
            if volume <= 0.0 {
                candidate.sound_effect_volume = settings.sound_effect_volume;
            }
        }
        "double_tap_lock" if !candidate.double_tap_lock => candidate.double_tap_only = false,
        "double_tap_only" if candidate.double_tap_only => {
            if !candidate.double_tap_lock || candidate.dictation_hotkey.key.is_none() {
                return Err(invalid(
                    name,
                    "a double-tap-only start needs double_tap_lock on and a dictation shortcut \
                     with a key, such as option+space",
                ));
            }
        }
        "dictation_hotkey"
        | "paste_last_hotkey"
        | "raw_dictation_hotkey"
        | "second_dictation_hotkey"
        | "capture.hotkey" => {
            let Kind::Shortcut(slot) = key.kind else {
                unreachable!("shortcut keys have the shortcut kind");
            };
            let binding = match slot {
                HotkeySlot::Dictation => Some(&candidate.dictation_hotkey),
                HotkeySlot::PasteLast => candidate.paste_last_hotkey.as_ref(),
                HotkeySlot::RawDictation => candidate.raw_dictation_hotkey.as_ref(),
                HotkeySlot::SecondDictation => candidate.second_dictation_hotkey.as_ref(),
                #[cfg(hex_capture)]
                HotkeySlot::Capture => Some(&candidate.capture.hotkey),
            };
            if let Some(binding) = binding {
                if cfg!(target_os = "macos")
                    && !candidate.labs.mouse_shortcuts
                    && binding.key.as_ref().is_some_and(HotkeyKey::is_mouse)
                {
                    return Err(invalid(
                        name,
                        "mouse buttons need Labs › Mouse shortcuts: hex labs on mouse-shortcuts",
                    ));
                }
                if let Some(why) = special_key_refusal(&candidate, binding) {
                    return Err(invalid(name, &why));
                }
                (shortcuts.rule)(slot, binding).map_err(|why| invalid(name, &why))?;
                if settings.hotkey_in_use(slot, binding) {
                    return Err(ControlError::new(
                        "shortcut-in-use",
                        format!(
                            "{} is already in use by another Hex shortcut",
                            format_hotkey(binding)
                        ),
                    ));
                }
            }
            if candidate.dictation_hotkey.key.is_none() {
                candidate.double_tap_only = false;
            }
        }
        "keyboard_model_switching.mappings" => {
            for choice in candidate.keyboard_model_switching.mappings.values() {
                choice
                    .validate()
                    .map_err(|error| invalid(name, &error.to_string()))?;
            }
        }
        "dictation_processing.default_mode" => {
            candidate.dictation_processing.default_mode.name = "Global".into();
        }
        "dictation_processing.modes" => {
            let mut names = std::collections::HashSet::new();
            for mode in &candidate.dictation_processing.modes {
                let mode_name = mode.name.trim();
                if mode_name.is_empty() || !names.insert(mode_name.to_lowercase()) {
                    return Err(invalid(name, "every mode needs a unique, non-empty name"));
                }
            }
        }
        #[cfg(hex_capture)]
        "capture.enabled"
            if candidate.capture.enabled
                && candidate.hotkey_in_use(HotkeySlot::Capture, &candidate.capture.hotkey) =>
        {
            return Err(ControlError::new(
                "shortcut-in-use",
                "The capture shortcut is already in use; set capture.hotkey to another \
                 shortcut first",
            ));
        }
        "labs.standalone_special_keys" => candidate.repair_special_keys(),
        "labs.parakeet_ultra" if !candidate.labs.parakeet_ultra => {
            if let Some(why) = crate::labs::Feature::ParakeetUltra.off_refusal(settings) {
                return Err(invalid(name, why));
            }
        }
        "history_retention" => effects.push(Effect::HistoryRetention(candidate.history_retention)),
        "show_dock_icon" => effects.push(Effect::DockIcon),
        "updates.channel" => effects.push(Effect::CheckForUpdates),
        _ => {}
    }
    Ok(Change {
        settings: candidate,
        effects,
        message,
    })
}

/// Validates every `key: value` in `values` against `settings` and returns
/// the settings to save, all or none (`hex settings apply`, the Home Manager
/// module). Values are JSON: booleans, numbers, `null` for `none`, objects
/// and arrays for json settings, text for the rest. `transcription.model`
/// and `transcription.language` select a model, as `hex model use` would,
/// without needing it installed. Keys that depend on each other (a
/// double-tap-only start and its shortcut) apply in any order.
pub fn apply_all(
    settings: &AppSettings,
    values: &serde_json::Map<String, Value>,
    shortcuts: &ShortcutRules,
) -> Result<Change, ControlError> {
    let mut candidate = settings.clone();
    let model = values.get("transcription.model");
    let language = values.get("transcription.language");
    if model.is_some() || language.is_some() {
        let text = |name: &str, value: Option<&Value>| match value {
            None => Ok(None),
            Some(Value::String(text)) => Ok(Some(text.clone())),
            Some(_) => Err(invalid(name, "expected text")),
        };
        let model = text("transcription.model", model)?
            .unwrap_or_else(|| candidate.transcription.model.as_str().to_owned());
        let language = text("transcription.language", language)?;
        candidate.transcription = super::model_selection(&candidate, &model, language.as_deref())?;
    }
    let mut pending = Vec::new();
    for (name, value) in values {
        if matches!(
            name.as_str(),
            "transcription.model" | "transcription.language"
        ) {
            continue;
        }
        if let Some(error) = unavailable(name) {
            return Err(error);
        }
        if name == RECOGNITION_HINTS {
            let Value::String(text) = value else {
                return Err(invalid(name, "expected text"));
            };
            pending.push((name.as_str(), text.clone()));
            continue;
        }
        let key = keys()
            .find(|key| key.key == name)
            .ok_or_else(|| unknown(name))?;
        let raw = match (key.kind, value) {
            (Kind::Json, value) => value.to_string(),
            (_, Value::String(text)) => text.clone(),
            (_, Value::Null) => "none".into(),
            (_, value) => value.to_string(),
        };
        pending.push((name.as_str(), raw));
    }
    let mut effects = Vec::new();
    while !pending.is_empty() {
        let tried = pending.len();
        let mut failed = Vec::new();
        let mut first_error = None;
        for (name, raw) in pending.drain(..) {
            match apply(&candidate, name, &raw, shortcuts) {
                Ok(change) => {
                    candidate = change.settings;
                    effects.extend(change.effects);
                }
                Err(error) => {
                    first_error.get_or_insert(error);
                    failed.push((name, raw));
                }
            }
        }
        if let Some(error) = first_error
            && failed.len() == tried
        {
            return Err(error);
        }
        pending = failed;
    }
    Ok(Change {
        settings: candidate,
        effects,
        message: format!("Applied {} settings", values.len()),
    })
}

fn invalid(name: &str, why: &str) -> ControlError {
    ControlError::new(
        "invalid-value",
        format!("invalid value for `{name}`: {why}"),
    )
}

fn parse(key: &Key, raw: &str, shortcuts: &ShortcutRules) -> Result<Value, ControlError> {
    let name = key.key;
    let trimmed = raw.trim();
    if key.optional && matches!(trimmed.to_ascii_lowercase().as_str(), "none" | "null" | "") {
        return Ok(Value::Null);
    }
    if name == "microphone" && trimmed.eq_ignore_ascii_case("system-default") {
        return Ok(Value::Null);
    }
    match key.kind {
        Kind::Bool => match trimmed.to_ascii_lowercase().as_str() {
            "on" | "true" | "yes" | "1" => Ok(Value::Bool(true)),
            "off" | "false" | "no" | "0" => Ok(Value::Bool(false)),
            _ => Err(invalid(name, "expected on or off")),
        },
        Kind::Number { min, max } => trimmed
            .parse::<f64>()
            .ok()
            .filter(|value| value.is_finite() && (min..=max).contains(value))
            .and_then(serde_json::Number::from_f64)
            .map(Value::Number)
            .ok_or_else(|| invalid(name, &format!("expected a number from {min} to {max}"))),
        Kind::Integer { min, max } => trimmed
            .parse::<u64>()
            .ok()
            .filter(|value| (min..=max).contains(value))
            .map(Value::from)
            .ok_or_else(|| {
                invalid(
                    name,
                    &format!("expected a whole number from {min} to {max}"),
                )
            }),
        Kind::Choice(choices) => {
            let normalized = trimmed.to_ascii_lowercase().replace('-', "_");
            let choice = choices
                .iter()
                .find(|choice| **choice == normalized)
                .ok_or_else(|| invalid(name, &format!("expected one of {}", choices.join(", "))))?;
            Ok(if name == "meetings.calendar.lead_minutes" {
                let minutes: u8 = choice.parse().expect("lead choices are numbers");
                debug_assert!(CALENDAR_LEAD_MINUTES.contains(&minutes));
                Value::from(minutes)
            } else if name == "labs.unload_idle_minutes" {
                let minutes: u32 = choice.parse().expect("idle choices are numbers");
                debug_assert!(crate::app_settings::UNLOAD_IDLE_MINUTES.contains(&minutes));
                Value::from(minutes)
            } else {
                Value::String((*choice).into())
            })
        }
        Kind::Text => Ok(Value::String(raw.into())),
        Kind::Path => {
            let path = match trimmed.strip_prefix("~/") {
                Some(rest) => dirs::home_dir()
                    .ok_or_else(|| invalid(name, "no home folder to expand ~ in"))?
                    .join(rest),
                None => std::path::PathBuf::from(trimmed),
            };
            if !path.is_absolute() {
                return Err(invalid(name, "expected an absolute path"));
            }
            Ok(Value::String(path.to_string_lossy().into_owned()))
        }
        Kind::Json => serde_json::from_str(raw).map_err(|error| invalid(name, &error.to_string())),
        Kind::Shortcut(_) => {
            let binding =
                parse_hotkey(trimmed, shortcuts.key_named).map_err(|why| invalid(name, &why))?;
            serde_json::to_value(binding)
                .map_err(|error| ControlError::new("internal", error.to_string()))
        }
    }
}

/// Reads a shortcut such as `option`, `right-command`, `fn`, `f5`,
/// `left-option+right-option` (both Options), or `control+option+space`:
/// modifiers, then at most one key.
pub fn parse_hotkey(
    text: &str,
    key_named: fn(&str) -> Result<HotkeyKey, String>,
) -> Result<HotkeyBinding, String> {
    let mut modifiers = HotkeyModifiers::default();
    let mut key = None;
    let mut sided = false;
    let parts: Vec<&str> = text.split('+').map(str::trim).collect();
    if parts.iter().any(|part| part.is_empty()) {
        return Err("expected modifiers and a key joined by +, such as option+space".into());
    }
    for part in parts {
        let lower = part.to_ascii_lowercase();
        let (side, name) = if let Some(rest) =
            lower.strip_prefix("left-").or(lower.strip_prefix("left"))
        {
            (Some(ModifierSide::Left), rest.to_string())
        } else if let Some(rest) = lower.strip_prefix("right-").or(lower.strip_prefix("right")) {
            (Some(ModifierSide::Right), rest.to_string())
        } else {
            (None, lower.clone())
        };
        let slot = match name.as_str() {
            "control" | "ctrl" => Some(&mut modifiers.control),
            "option" | "opt" | "alt" => Some(&mut modifiers.option),
            "shift" => Some(&mut modifiers.shift),
            "command" | "cmd" | "super" | "win" | "meta" => Some(&mut modifiers.command),
            _ => None,
        };
        match slot {
            Some(slot) => {
                sided |= side.is_some();
                *slot = Some(match (*slot, side) {
                    (None, side) => side.unwrap_or(ModifierSide::Either),
                    (Some(ModifierSide::Left), Some(ModifierSide::Right))
                    | (Some(ModifierSide::Right), Some(ModifierSide::Left)) => ModifierSide::Both,
                    _ => return Err(format!("{part} appears twice")),
                });
            }
            None if side.is_none() && matches!(name.as_str(), "fn" | "function" | "globe") => {
                if modifiers.function {
                    return Err(format!("{part} appears twice"));
                }
                modifiers.function = true;
            }
            None if side.is_some() && name.is_empty() => {
                // "left" or "right" alone is the arrow key.
                if key.is_some() {
                    return Err("a shortcut has at most one key".into());
                }
                key = Some(key_named(&lower)?);
            }
            None => {
                if key.is_some() {
                    return Err("a shortcut has at most one key".into());
                }
                key = Some(key_named(part)?);
            }
        }
    }
    let binding = HotkeyBinding { modifiers, key };
    if let Some(key) = &binding.key {
        if sided {
            return Err("left- and right- apply only to shortcuts without a key".into());
        }
        if modifiers.function {
            return Err("fn can't be combined with a key".into());
        }
        if binding.is_bare_primary_click() {
            return Err("mouse1 (left click) needs a modifier, such as option+mouse1".into());
        }
        // Menu, Pause, and Scroll Lock alone, and on Windows the navigation
        // keys alone, are checked against their Labs toggle when set
        // (`special_key_refusal`).
        if modifiers.is_empty()
            && !is_function_key(&key.label)
            && !key.is_mouse()
            && !HotkeyKey::special_name(&key.label)
            && !(cfg!(target_os = "windows") && HotkeyKey::navigation_name(&key.label))
        {
            return Err("add a modifier, such as option+".to_string() + &key.label.to_lowercase());
        }
    } else if modifiers.is_empty() {
        return Err("expected at least one modifier or key".into());
    }
    Ok(binding)
}

/// Why `binding` can't be saved for its Menu, Pause, or Scroll Lock key: on
/// Linux they need Labs › Special keys alone; Windows takes them alone, and
/// a navigation key alone needs the toggle there; elsewhere one alone still
/// needs a modifier.
fn special_key_refusal(settings: &AppSettings, binding: &HotkeyBinding) -> Option<String> {
    if cfg!(target_os = "windows") {
        let key = binding.key.as_ref()?;
        return (binding.is_lone_navigation_key() && !settings.special_keys_allowed()).then(|| {
            format!(
                "{} alone needs Labs › Special keys alone: hex labs on special-keys",
                key.label.to_lowercase().replace(' ', "")
            )
        });
    }
    let key = binding.key.as_ref().filter(|key| key.is_special())?;
    if settings.special_keys_allowed() {
        return None;
    }
    let name = key.label.to_lowercase().replace(' ', "");
    if cfg!(target_os = "linux") {
        return Some(format!(
            "{name} needs Labs › Special keys alone: hex labs on special-keys"
        ));
    }
    binding
        .modifiers
        .is_empty()
        .then(|| format!("add a modifier, such as option+{name}"))
}

fn is_function_key(label: &str) -> bool {
    label
        .strip_prefix(['F', 'f'])
        .and_then(|number| number.parse::<u8>().ok())
        .is_some_and(|number| (1..=20).contains(&number))
}

/// The text `parse_hotkey` reads back: `left-option`, `option+shift+v`,
/// `left-option+right-option`.
pub fn format_hotkey(binding: &HotkeyBinding) -> String {
    let modifiers = binding.modifiers;
    let side = |side: Option<ModifierSide>, name: &str| match side {
        Some(ModifierSide::Left) => Some(format!("left-{name}")),
        Some(ModifierSide::Right) => Some(format!("right-{name}")),
        Some(ModifierSide::Either) => Some(name.to_string()),
        Some(ModifierSide::Both) => Some(format!("left-{name}+right-{name}")),
        None => None,
    };
    let mut parts: Vec<String> = [
        modifiers.function.then(|| "fn".to_string()),
        side(modifiers.control, "control"),
        side(modifiers.option, "option"),
        side(modifiers.shift, "shift"),
        side(modifiers.command, "command"),
    ]
    .into_iter()
    .flatten()
    .collect();
    if let Some(key) = &binding.key {
        parts.push(key.label.to_lowercase().replace(' ', ""));
    }
    parts.join("+")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn rules() -> ShortcutRules {
        ShortcutRules {
            key_named: |name| match name.to_ascii_lowercase().as_str() {
                "space" => Ok(HotkeyKey {
                    code: 49,
                    label: "Space".into(),
                }),
                "f5" => Ok(HotkeyKey {
                    code: 96,
                    label: "F5".into(),
                }),
                "left" => Ok(HotkeyKey {
                    code: 123,
                    label: "Left".into(),
                }),
                "menu" => Ok(HotkeyKey {
                    code: 0,
                    label: "Menu".into(),
                }),
                name if name.chars().count() == 1 => Ok(HotkeyKey {
                    code: u16::from(name.as_bytes()[0]),
                    label: name.to_uppercase(),
                }),
                _ => Err("Unsupported key".into()),
            },
            rule: |_, _| Ok(()),
        }
    }

    fn leaves(value: &Value, path: String, out: &mut Vec<String>) {
        match value {
            Value::Object(map) if !map.is_empty() => {
                for (name, value) in map {
                    let path = if path.is_empty() {
                        name.clone()
                    } else {
                        format!("{path}.{name}")
                    };
                    leaves(value, path, out);
                }
            }
            _ => out.push(path),
        }
    }

    /// Fails when a persisted setting has no `hex settings` key: add one to
    /// `KEYS` (or to `INTERNAL` if it is Hex's own bookkeeping, or to
    /// `OWN_COMMAND` if another `hex` command edits it).
    #[test]
    fn every_persisted_setting_has_a_cli_key() {
        // Lists left empty and unchosen options are left out of the file;
        // fill them so they count.
        let settings = AppSettings {
            vocabulary: vec![crate::dictation::vocabulary::VocabularyEntry {
                term: "Rune".into(),
                sounds_like: vec!["roon".into()],
            }],
            meetings: crate::app_settings::MeetingSettings {
                retention: Some(crate::app_settings::MeetingRetention::Day),
                ..Default::default()
            },
            ..Default::default()
        };
        let json = serde_json::to_value(settings).unwrap();
        let mut paths = Vec::new();
        leaves(&json, String::new(), &mut paths);
        for path in &paths {
            let covered = keys()
                .map(|key| key.key)
                .chain(INTERNAL.iter().copied())
                .chain(OWN_COMMAND.iter().copied())
                .any(|key| path == key || path.starts_with(&format!("{key}.")));
            assert!(
                covered,
                "settings.json has `{path}`, which `hex settings` can't name"
            );
        }
        for key in keys() {
            assert!(
                json.pointer(&pointer(key.key)).is_some(),
                "`{}` names no setting",
                key.key
            );
        }
        let mut names: Vec<_> = keys().map(|key| key.key).collect();
        names.sort_unstable();
        names.dedup();
        assert_eq!(names.len(), keys().count(), "a key is listed twice");
    }

    #[test]
    fn settings_name_real_capabilities_and_keys() {
        let capabilities = crate::rune_ui::capabilities();
        for (prefix, capability) in NEEDS {
            assert!(
                capabilities.contains_key(*capability),
                "`{prefix}` needs `{capability}`, which is no capability"
            );
            assert!(
                keys().any(|key| key.key == *prefix || key.key.starts_with(prefix)),
                "`{prefix}` names no setting"
            );
        }
    }

    #[cfg(hex_capture)]
    #[test]
    fn capture_routing_is_set_as_one_object_without_a_jev_key() {
        let settings = AppSettings::default();
        let value = concat!(
            r#"{"smartRouting": true, "sites": {"https://example.com": "ses_a"}, "#,
            r#""jevApiKey": "not-a-real-key"}"#
        );
        let change = apply(&settings, "capture.routing", value, &rules()).unwrap();
        let config = change.settings.capture.routing.config();
        assert!(config.smart_routing);
        assert_eq!(config.sites.len(), 1);
        assert_eq!(config.jev_api_key, None);
        let shown = get(&change.settings, "capture.routing").unwrap();
        assert!(!shown[0].value.to_string().contains("not-a-real-key"));
        assert_eq!(
            apply(&settings, "capture.routing", "[", &rules())
                .unwrap_err()
                .code,
            "invalid-value"
        );
    }

    #[test]
    fn keyboard_model_switching_settings_map_sources_to_a_model_and_its_language() {
        let settings = apply(
            &AppSettings::default(),
            "keyboard_model_switching.enabled",
            "on",
            &rules(),
        )
        .unwrap()
        .settings;
        assert!(settings.keyboard_model_switching.enabled);
        let change = apply(
            &settings,
            "keyboard_model_switching.mappings",
            r#"{"com.apple.keylayout.Spanish":{"model":"parakeet_v3","language":"es"}}"#,
            &rules(),
        )
        .unwrap();
        assert_eq!(
            change
                .settings
                .keyboard_model_switching
                .mappings
                .get("com.apple.keylayout.Spanish"),
            Some(&crate::app_settings::KeyboardModelChoice {
                model: crate::transcription::models::TranscriptionModelId::ParakeetV3,
                language: "es".into(),
            })
        );
        for invalid in [
            r#"{"Spanish":"parakeet_v3"}"#,
            r#"{"Spanish":{"model":"unknown-model","language":"es"}}"#,
            r#"{"Japanese":{"model":"parakeet_unified_en","language":"ja"}}"#,
            r#"{"ABC":{"model":"apple_speech","language":"en"}}"#,
        ] {
            assert!(
                apply(
                    &settings,
                    "keyboard_model_switching.mappings",
                    invalid,
                    &rules()
                )
                .is_err(),
                "{invalid} was accepted"
            );
        }
    }

    #[test]
    fn settings_read_by_key_and_prefix() {
        let settings = AppSettings::default();
        let all = list(&settings);
        assert_eq!(all.len(), keys().count());
        let calendar = get(&settings, "meetings.calendar").unwrap();
        assert_eq!(calendar.len(), 4);
        let lead = get(&settings, "meetings.calendar.lead_minutes").unwrap();
        assert_eq!(lead[0].value, Value::String("1".into()));
        assert_eq!(get(&settings, "nope").unwrap_err().code, "unknown-setting");
        let retention = get(&settings, "history_retention").unwrap();
        assert_eq!(retention[0].value, Value::String("week".into()));
        assert_eq!(retention[0].choices.len(), 5);
    }

    #[test]
    fn recognition_hints_alias_the_vocabulary() {
        let settings = AppSettings {
            vocabulary: vec![crate::dictation::vocabulary::VocabularyEntry {
                term: "Rune".into(),
                sounds_like: Vec::new(),
            }],
            ..Default::default()
        };
        let change = apply(&settings, RECOGNITION_HINTS, "rune, Kaffar", &rules()).unwrap();
        let terms: Vec<_> = change
            .settings
            .vocabulary
            .iter()
            .map(|entry| entry.term.as_str())
            .collect();
        assert_eq!(terms, ["Rune", "Kaffar"]);
        let read = get(&change.settings, RECOGNITION_HINTS).unwrap();
        assert_eq!(read[0].value, Value::String("Rune, Kaffar".into()));
        assert!(
            list(&settings)
                .iter()
                .all(|value| value.key != RECOGNITION_HINTS)
        );
        let values = serde_json::json!({ RECOGNITION_HINTS: "Rodolfo" });
        let applied = apply_all(&settings, values.as_object().unwrap(), &rules()).unwrap();
        assert_eq!(applied.settings.vocabulary.len(), 2);
    }

    #[test]
    fn values_are_parsed_and_checked() {
        let settings = AppSettings::default();
        let change = apply(&settings, "sound_effects", "off", &rules()).unwrap();
        assert!(!change.settings.sound_effects);
        assert!(change.changed(&settings));
        for (key, value) in [
            ("sound_effects", "maybe"),
            ("sound_effect_volume", "2"),
            ("shortcut_mode", "tap"),
            ("meetings.calendar.lead_minutes", "3"),
            ("meetings.output_folder", "relative/path"),
            ("dictation_processing.modes", "[{"),
        ] {
            assert_eq!(
                apply(&settings, key, value, &rules()).unwrap_err().code,
                "invalid-value",
                "{key}={value}"
            );
        }
        assert_eq!(
            apply(&settings, "transcription.model", "parakeet_v2", &rules())
                .unwrap_err()
                .code,
            "read-only"
        );
        assert_eq!(
            apply(&settings, "bogus", "1", &rules()).unwrap_err().code,
            "unknown-setting"
        );
        let change = apply(&settings, "shortcut_mode", "Toggle", &rules()).unwrap();
        assert_eq!(
            change.settings.shortcut_mode,
            crate::dictation::ShortcutMode::Toggle
        );
        let change = apply(&settings, "meetings.calendar.lead_minutes", "5", &rules()).unwrap();
        assert_eq!(change.settings.meetings.calendar.lead_minutes, 5);
        let change = apply(&settings, "ui_language", "none", &rules()).unwrap();
        assert_eq!(change.settings.ui_language, None);
        let mut saved = settings.clone();
        saved.microphone = Some("USB Mic".into());
        let change = apply(&saved, "microphone", "system-default", &rules()).unwrap();
        assert_eq!(change.settings.microphone, None);
        let change = apply(&settings, "history_retention", "month", &rules()).unwrap();
        assert_eq!(
            change.effects,
            vec![Effect::HistoryRetention(
                crate::history::HistoryRetention::Month
            )]
        );
    }

    /// Issue 767: Menu alone (Caps Lock remapped to it) is a Linux
    /// shortcut only with Labs › Special keys alone on, and turning the
    /// toggle off puts it back to the default.
    #[test]
    fn menu_alone_needs_its_labs_toggle() {
        assert!(parse_hotkey("menu", rules().key_named).is_ok());
        assert!(parse_hotkey("space", rules().key_named).is_err());
        let settings = AppSettings::default();
        assert_eq!(
            apply(&settings, "dictation_hotkey", "menu", &rules())
                .unwrap_err()
                .code,
            "invalid-value"
        );
        let on = apply(&settings, "labs.standalone_special_keys", "on", &rules())
            .unwrap()
            .settings;
        let change = apply(&on, "dictation_hotkey", "menu", &rules());
        if cfg!(target_os = "linux") {
            let change = change.unwrap();
            assert_eq!(format_hotkey(&change.settings.dictation_hotkey), "menu");
            let off = apply(
                &change.settings,
                "labs.standalone_special_keys",
                "off",
                &rules(),
            )
            .unwrap();
            assert_eq!(off.settings.dictation_hotkey, HotkeyBinding::default());
        } else {
            assert!(change.is_err(), "only Linux has the toggle");
        }
    }

    #[test]
    fn zero_volume_turns_tones_off_and_keeps_the_level() {
        let settings = AppSettings::default();
        let change = apply(&settings, "sound_effect_volume", "0", &rules()).unwrap();
        assert!(!change.settings.sound_effects);
        assert_eq!(
            change.settings.sound_effect_volume,
            settings.sound_effect_volume
        );
        let change = apply(&change.settings, "sound_effect_volume", "0.25", &rules()).unwrap();
        assert!(change.settings.sound_effects);
        assert_eq!(change.settings.sound_effect_volume, 0.25);
    }

    #[test]
    fn sound_set_offers_every_set() {
        let key = keys().find(|key| key.key == "sound_theme").unwrap();
        let Kind::Choice(choices) = key.kind else {
            panic!("sound_theme is a choice");
        };
        let sets: Vec<String> = crate::feedback::SoundSet::ALL
            .iter()
            .map(|set| serde_json::to_value(set).unwrap().as_str().unwrap().into())
            .collect();
        assert_eq!(sets, choices);
        let change = apply(&AppSettings::default(), "sound_theme", "wood", &rules()).unwrap();
        assert_eq!(change.settings.sound_theme, crate::feedback::SoundSet::Wood);
    }

    #[test]
    fn shortcuts_round_trip_and_refuse_conflicts() {
        let rules = rules();
        for text in [
            "option",
            "right-command",
            "fn",
            "f5",
            "control+option+space",
            "option+shift+v",
            "left-option+left-shift",
            "left-option+right-shift",
            "left-option+right-option",
            "control+left-option+right-option",
        ] {
            let binding = parse_hotkey(text, rules.key_named).unwrap();
            assert_eq!(format_hotkey(&binding), text);
        }
        for text in [
            "",
            "v",
            "option+",
            "fn+v",
            "left-option+v",
            "option+v+b",
            "option+option",
            "left-option+left-option",
            "option+right-option",
            "left-option+right-option+v",
        ] {
            assert!(parse_hotkey(text, rules.key_named).is_err(), "{text}");
        }
        assert_eq!(
            parse_hotkey("right-option+left-option", rules.key_named)
                .unwrap()
                .modifiers
                .option,
            Some(ModifierSide::Both),
            "both Options in either order"
        );
        assert_eq!(
            parse_hotkey("control+left", rules.key_named)
                .unwrap()
                .key
                .unwrap()
                .label,
            "Left"
        );

        let settings = AppSettings::default();
        let dictation = format_hotkey(&settings.dictation_hotkey);
        assert_eq!(
            apply(&settings, "paste_last_hotkey", &dictation, &rules)
                .unwrap_err()
                .code,
            "shortcut-in-use"
        );
        let change = apply(&settings, "paste_last_hotkey", "none", &rules).unwrap();
        assert_eq!(change.settings.paste_last_hotkey, None);
        let refuse = ShortcutRules {
            key_named: rules.key_named,
            rule: |_, _| Err("Modifier-only shortcuts aren't supported here".into()),
        };
        assert_eq!(
            apply(&settings, "dictation_hotkey", "control+shift", &refuse)
                .unwrap_err()
                .code,
            "invalid-value"
        );
    }

    #[test]
    fn a_bare_left_click_is_refused_but_other_mouse_buttons_are_not() {
        let named: fn(&str) -> Result<HotkeyKey, String> = |name| Ok(HotkeyKey::named(name));
        assert!(parse_hotkey("mouse1", named).is_err());
        assert!(parse_hotkey("option+mouse1", named).is_ok());
        assert!(parse_hotkey("mouse2", named).is_ok());
        assert!(parse_hotkey("mouse4", named).is_ok());
    }

    #[test]
    fn double_tap_only_needs_a_key_chord() {
        let settings = AppSettings::default();
        let without_key = AppSettings {
            dictation_hotkey: parse_hotkey("option", rules().key_named).unwrap(),
            ..settings.clone()
        };
        assert_eq!(
            apply(&without_key, "double_tap_only", "on", &rules())
                .unwrap_err()
                .code,
            "invalid-value"
        );
        let with_key = apply(&without_key, "dictation_hotkey", "option+space", &rules())
            .unwrap()
            .settings;
        let change = apply(&with_key, "double_tap_only", "on", &rules()).unwrap();
        assert!(change.settings.double_tap_only);
        let change = apply(&change.settings, "double_tap_lock", "off", &rules()).unwrap();
        assert!(!change.settings.double_tap_only);
    }

    #[test]
    fn many_settings_apply_together_or_not_at_all() {
        let settings = AppSettings {
            dictation_hotkey: parse_hotkey("option", rules().key_named).unwrap(),
            ..AppSettings::default()
        };
        let values = |json: &str| serde_json::from_str::<serde_json::Map<_, _>>(json).unwrap();
        let change = apply_all(
            &settings,
            &values(
                r#"{"double_tap_only": true, "double_tap_lock": true,
                    "dictation_hotkey": "option+space", "sound_effect_volume": 0.4,
                    "microphone": null, "transcription.model": "parakeet_v3",
                    "transcription.language": "de"}"#,
            ),
            &rules(),
        )
        .unwrap();
        assert!(change.settings.double_tap_only);
        assert_eq!(
            format_hotkey(&change.settings.dictation_hotkey),
            "option+space"
        );
        assert_eq!(change.settings.sound_effect_volume, 0.4);
        assert_eq!(change.settings.transcription.model.as_str(), "parakeet_v3");
        assert_eq!(change.settings.transcription.language, "de");

        for refused in [
            r#"{"sound_effects": false, "no_such_setting": 1}"#,
            r#"{"sound_effects": false, "sound_effect_volume": 7}"#,
            r#"{"transcription.model": "no_such_model"}"#,
        ] {
            assert!(apply_all(&settings, &values(refused), &rules()).is_err());
        }
    }
}
