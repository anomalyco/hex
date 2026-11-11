//! The `hex settings` registry: one key per persisted setting, named by its
//! dotted path in `settings.json`, with the same rules and side effects the
//! window applies when you change it there.

use serde::{Deserialize, Serialize};
use serde_json::Value;

use super::ControlError;
use crate::app_settings::{
    AppSettings, CALENDAR_LEAD_MINUTES, HotkeyBinding, HotkeyKey, HotkeySlot,
    MAX_EXTRA_DICTATION_SHORTCUTS,
};

mod hotkeys;
mod registry;

use hotkeys::check_shortcut;
pub use hotkeys::{format_hotkey, parse_hotkey};
use registry::{CAPTURE_KEYS, KEYS, Key, Kind, NUMERIC_CHOICES, RETIRED, needs};

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

/// Why `hex settings set` refuses `name` on this OS, if it does.
pub fn unavailable(name: &str) -> Option<ControlError> {
    unavailable_under(name, crate::policy::Policy::current())
}

fn unavailable_under(name: &str, policy: crate::policy::Policy) -> Option<ControlError> {
    if let Some(lock) = policy.setting_lock(name) {
        return Some(managed(name, lock));
    }
    if let Some(feature) = crate::labs::Feature::for_settings_key(name)
        && !feature.shipped()
    {
        return Some(ControlError::new(
            "unsupported",
            format!("`{name}` belongs to a Labs feature that only dev builds have"),
        ));
    }
    // A `labs.` key is only its feature's switch, so off its OS it does nothing.
    if let Some(feature) = crate::labs::Feature::for_settings_key(name)
        && (name.starts_with("labs.") || name == "capture.replay_hotkey")
        && !feature.on_this_os()
    {
        return Some(ControlError::new(
            "unsupported",
            format!("`{name}` belongs to a Labs feature this OS doesn't have"),
        ));
    }
    if name.starts_with("screenshots.") && !cfg!(target_os = "macos") {
        return Some(ControlError::new(
            "unsupported",
            format!("`{name}` belongs to Labs › Screenshots, which only macOS has"),
        ));
    }
    if name == "extra_dictation_hotkeys" && !cfg!(target_os = "macos") {
        return Some(ControlError::new(
            "unsupported",
            format!("`{name}` belongs to Labs › More dictation shortcuts, which only macOS has"),
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

fn managed(name: &str, lock: crate::policy::Lock) -> ControlError {
    ControlError::new(
        "managed",
        format!("`{name}` can't be changed: {}", lock.refusal()),
    )
}

/// Every key, in registry order.
fn keys() -> impl Iterator<Item = &'static Key> {
    KEYS.iter().chain(CAPTURE_KEYS)
}

/// Settings `hex settings` never shows: Hex's own bookkeeping.
#[cfg(test)]
const INTERNAL: &[&str] = &["migrations", "mouse_shortcut_notice", "labs.groups"];

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
    if let Some((_, replacement)) = RETIRED.iter().find(|(retired, _)| *retired == name) {
        return ControlError::new(
            "unknown-setting",
            format!("`{name}` was replaced: use `hex settings set {replacement}`"),
        );
    }
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
        writable: key.read_only.is_none()
            && crate::policy::Policy::current()
                .setting_lock(key.key)
                .is_none(),
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
    // An unset optional setting is left out of the file, so give it a slot.
    let path = pointer(key.key);
    if json.pointer(&path).is_none()
        && let Some((parent, leaf)) = path.rsplit_once('/')
        && let Some(Value::Object(object)) = json.pointer_mut(parent)
    {
        object.insert(leaf.to_owned(), Value::Null);
    }
    let slot = json
        .pointer_mut(&path)
        .ok_or_else(|| ControlError::new("internal", format!("`{name}` is missing")))?;
    *slot = value;
    let mut candidate: AppSettings =
        serde_json::from_value(json).map_err(|error| invalid(name, &format!("{error}")))?;
    let mut effects = Vec::new();
    let message = format!("Set {name}");
    match key.key {
        "meetings.summaries"
            if candidate.meetings.summaries == crate::app_settings::MeetingSummaries::OpenCode
                && crate::policy::Policy::current()
                    .locks(crate::policy::Lock::OpenCodeProcessing) =>
        {
            return Err(managed(name, crate::policy::Lock::OpenCodeProcessing));
        }
        "sound_effect_volume" => {
            let volume = candidate.sound_effect_volume;
            candidate.sound_effects = volume > 0.0;
            if volume <= 0.0 {
                candidate.sound_effect_volume = settings.sound_effect_volume;
            }
        }
        "double_tap_lock" if !candidate.double_tap_lock => candidate.double_tap_only = false,
        "double_tap_only" if candidate.double_tap_only => {
            if !candidate.double_tap_lock
                || (candidate.dictation_hotkey.key.is_none()
                    && !candidate.double_tap_modifiers_allowed())
            {
                let modifier_only = if cfg!(target_os = "macos") {
                    ", or with Labs › Double-tap on a lone modifier on, a shortcut without a key"
                } else {
                    ""
                };
                return Err(invalid(
                    name,
                    &format!(
                        "a double-tap-only start needs double_tap_lock on and a dictation \
                         shortcut with a key, such as option+space{modifier_only}"
                    ),
                ));
            }
        }
        "labs.double_tap_modifiers" => candidate.normalize_double_tap_settings(),
        "dictation_hotkey"
        | "paste_last_hotkey"
        | "raw_dictation_hotkey"
        | "typed_text_hotkey"
        | "second_dictation_hotkey"
        | "terminal_paste_hotkey"
        | "cancel_hotkey"
        | "screenshots.hotkey"
        | "screenshots.record_hotkey"
        | "capture.hotkey"
        | "capture.replay_hotkey" => {
            let Kind::Shortcut(slot) = key.kind else {
                unreachable!("shortcut keys have the shortcut kind");
            };
            let binding = match slot {
                HotkeySlot::Dictation => Some(&candidate.dictation_hotkey),
                HotkeySlot::PasteLast => candidate.paste_last_hotkey.as_ref(),
                HotkeySlot::RawDictation => candidate.raw_dictation_hotkey.as_ref(),
                HotkeySlot::TypedText => candidate.typed_text_hotkey.as_ref(),
                HotkeySlot::SecondDictation => candidate.second_dictation_hotkey.as_ref(),
                // Each row of `extra_dictation_hotkeys` is validated from
                // its own JSON list, not a shortcut key.
                HotkeySlot::ExtraDictation(_) => None,
                HotkeySlot::TerminalPaste => candidate.terminal_paste_hotkey.as_ref(),
                HotkeySlot::Cancel => Some(&candidate.cancel_hotkey),
                HotkeySlot::Screenshot => Some(&candidate.screenshots.hotkey),
                HotkeySlot::ScreenshotRecord => candidate.screenshots.record_hotkey.as_ref(),
                #[cfg(hex_capture)]
                HotkeySlot::Capture => Some(&candidate.capture.hotkey),
                #[cfg(hex_capture)]
                HotkeySlot::CaptureReplay => candidate.capture.replay_hotkey.as_ref(),
            };
            if let Some(binding) = binding {
                check_shortcut(settings, &candidate, name, slot, binding, shortcuts)?;
            }
            if candidate.dictation_hotkey.key.is_none() && !candidate.double_tap_modifiers_allowed()
            {
                candidate.double_tap_only = false;
            }
        }
        "extra_dictation_hotkeys" => {
            if candidate.extra_dictation_hotkeys.len() > MAX_EXTRA_DICTATION_SHORTCUTS {
                return Err(invalid(
                    name,
                    &format!("at most {MAX_EXTRA_DICTATION_SHORTCUTS} extra dictation shortcuts"),
                ));
            }
            for (index, row) in candidate.extra_dictation_hotkeys.iter().enumerate() {
                if let Some(binding) = &row.binding {
                    check_shortcut(
                        &candidate,
                        &candidate,
                        name,
                        HotkeySlot::ExtraDictation(index),
                        binding,
                        shortcuts,
                    )?;
                }
            }
            // A row can't be double-tap only without a key, or with Labs ›
            // Double-tap on a lone modifier, without a key but modifiers.
            let modifiers_allowed = candidate.double_tap_modifiers_allowed();
            for row in &mut candidate.extra_dictation_hotkeys {
                row.mode = row
                    .mode
                    .for_binding(row.binding.as_ref(), modifiers_allowed);
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
            validate_mode_speech(name, &candidate.dictation_processing.default_mode)?;
        }
        "dictation_processing.modes" => {
            let mut names = std::collections::HashSet::new();
            for mode in &candidate.dictation_processing.modes {
                let mode_name = mode.name.trim();
                if mode_name.is_empty() || !names.insert(mode_name.to_lowercase()) {
                    return Err(invalid(name, "every mode needs a unique, non-empty name"));
                }
                validate_mode_speech(name, mode)?;
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
        "labs.screenshots"
            if candidate.labs.screenshots
                && candidate
                    .hotkey_in_use(HotkeySlot::Screenshot, &candidate.screenshots.hotkey) =>
        {
            return Err(ControlError::new(
                "shortcut-in-use",
                "The screenshot shortcut is already in use; set screenshots.hotkey to another \
                 shortcut first",
            ));
        }
        "labs.standalone_special_keys" => candidate.repair_special_keys(),
        "labs.section_key_alone" => candidate.repair_section_key(),
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

/// A mode's speech choice must name a model and a language it supports.
fn validate_mode_speech(
    name: &str,
    mode: &crate::app_settings::DictationMode,
) -> Result<(), ControlError> {
    if let Some(choice) = &mode.speech {
        choice
            .validate()
            .map_err(|error| invalid(name, &error.to_string()))?;
    }
    Ok(())
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
                .find(|choice| choice.to_ascii_lowercase().replace('-', "_") == normalized)
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
                Some(rest) => crate::app_paths::home_dir()
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::app_settings::ModifierSide;

    use super::registry::NEEDS;

    #[test]
    fn a_managed_setting_is_refused() {
        use crate::policy::{Lock, Policy};
        let policy = Policy::default()
            .with(Lock::Meetings)
            .with(Lock::Annotate)
            .with(Lock::OpenCodeProcessing);
        for name in ["meetings.enabled", "capture.enabled"] {
            let error = unavailable_under(name, policy).expect(name);
            assert_eq!(error.code, "managed");
            assert!(error.message.contains("managed by your organization"));
        }
        assert!(unavailable_under("sound_effects", policy).is_none());
        assert!(unavailable_under("labs.live_text", policy).is_none());
        let labs = Policy::default().with(Lock::Labs);
        assert_eq!(
            unavailable_under("labs.live_text", labs).map(|error| error.code),
            Some("managed")
        );
        assert!(unavailable_under("labs.live_text", Policy::default()).is_none());
    }

    #[test]
    fn a_labs_switch_for_another_os_is_refused() {
        let refused = unavailable_under("labs.notch_indicator", crate::policy::Policy::default())
            .map(|error| error.code);
        if cfg!(target_os = "macos") {
            assert_eq!(refused, None);
        } else {
            assert_eq!(refused, Some("unsupported"));
        }
    }

    /// A saved record shortcut and Annotate's shortcut can't share a chord
    /// with either off. The reported sequence: Annotate off with the Lab on
    /// and a record shortcut saved. Setting Annotate's shortcut to the
    /// record chord is refused, and so is setting the record shortcut to
    /// Annotate's chord. The old code allowed both, and Annotate then won
    /// the key at runtime, recording the whole screen.
    #[cfg(target_os = "macos")]
    #[test]
    fn the_screenshot_lab_and_annotate_never_share_a_shortcut() {
        let chord = |code: u16, label: &str, modifiers: crate::app_settings::HotkeyModifiers| {
            HotkeyBinding {
                modifiers,
                key: Some(HotkeyKey {
                    code,
                    label: label.into(),
                }),
            }
        };
        let mut settings = AppSettings {
            capture: crate::app_settings::CaptureSettings {
                enabled: false,
                ..Default::default()
            },
            ..AppSettings::default()
        };
        settings.labs.screenshots = true;
        settings.capture.hotkey = chord(
            u16::from(b's'),
            "S",
            crate::app_settings::HotkeyModifiers {
                option: Some(ModifierSide::Either),
                shift: Some(ModifierSide::Either),
                ..Default::default()
            },
        );
        let record = chord(
            u16::from(b'r'),
            "R",
            crate::app_settings::HotkeyModifiers {
                command: Some(ModifierSide::Either),
                shift: Some(ModifierSide::Either),
                ..Default::default()
            },
        );
        settings.screenshots.record_hotkey = Some(record.clone());

        let error = apply(
            &settings,
            "capture.hotkey",
            &format_hotkey(&record),
            &rules(),
        )
        .unwrap_err();
        assert_eq!(error.code, "shortcut-in-use");

        let annotate = settings.capture.hotkey.clone();
        let error = apply(
            &settings,
            "screenshots.record_hotkey",
            &format_hotkey(&annotate),
            &rules(),
        )
        .unwrap_err();
        assert_eq!(error.code, "shortcut-in-use");
    }

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
                audio_retention: Some(crate::app_settings::MeetingRetention::Day),
                opencode_model: Some("anthropic/claude-haiku".into()),
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
    fn ui_language_accepts_traditional_chinese_in_any_case() {
        for raw in ["zh-Hant", "zh-hant", "ZH_HANT"] {
            let change = apply(&AppSettings::default(), "ui_language", raw, &rules()).unwrap();
            let shown = get(&change.settings, "ui_language").unwrap();
            assert_eq!(shown[0].value, Value::String("zh-Hant".into()), "{raw}");
        }
        let change = apply(&AppSettings::default(), "ui_language", "zh", &rules()).unwrap();
        let shown = get(&change.settings, "ui_language").unwrap();
        assert_eq!(shown[0].value, Value::String("zh".into()));
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
    fn app_model_switching_is_named_and_modes_validate_their_speech_choice() {
        let settings = apply(
            &AppSettings::default(),
            "labs.app_model_switching",
            "on",
            &rules(),
        )
        .unwrap()
        .settings;
        assert!(settings.labs.app_model_switching);
        let modes = r#"[{"name":"Spanish","applications":["Notes"],"speech":{"model":"parakeet_v3","language":"es"}}]"#;
        let change = apply(&settings, "dictation_processing.modes", modes, &rules()).unwrap();
        assert_eq!(
            change.settings.dictation_processing.modes[0].speech,
            Some(crate::app_settings::KeyboardModelChoice {
                model: crate::transcription::models::TranscriptionModelId::ParakeetV3,
                language: "es".into(),
            })
        );
        let invalid =
            r#"[{"name":"Japanese","speech":{"model":"parakeet_unified_en","language":"ja"}}]"#;
        assert!(
            apply(&settings, "dictation_processing.modes", invalid, &rules()).is_err(),
            "a language the model lacks was accepted"
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
            ("labs.caps_lock_light", "fast"),
            ("labs.appearance_theme", "sepia"),
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
        let change = apply(&settings, "labs.caps_lock_light", "slow_blink", &rules()).unwrap();
        assert_eq!(
            change.settings.labs.caps_lock_light,
            crate::app_settings::CapsLockLight::SlowBlink
        );
        assert_eq!(
            get(&change.settings, "labs.caps_lock_light").unwrap()[0].value,
            Value::String("slow_blink".into())
        );
        let change = apply(&settings, "labs.appearance_theme", "dark", &rules()).unwrap();
        assert_eq!(
            change.settings.labs.appearance_theme,
            crate::app_settings::AppearanceTheme::Dark
        );
        assert_eq!(
            get(&change.settings, "labs.appearance_theme").unwrap()[0].value,
            Value::String("dark".into())
        );
        let change = apply(&settings, "ui_language", "none", &rules()).unwrap();
        assert_eq!(change.settings.ui_language, None);
        let change = apply(
            &settings,
            "meetings.opencode_model",
            "anthropic/claude-haiku",
            &rules(),
        )
        .unwrap();
        assert_eq!(
            change.settings.meetings.opencode_model.as_deref(),
            Some("anthropic/claude-haiku")
        );
        assert_eq!(
            get(&change.settings, "meetings.opencode_model").unwrap()[0].value,
            "anthropic/claude-haiku"
        );
        let cleared = apply(
            &change.settings,
            "meetings.opencode_model",
            "none",
            &rules(),
        )
        .unwrap();
        assert_eq!(cleared.settings.meetings.opencode_model, None);
        assert!(
            get(&cleared.settings, "meetings.opencode_model").unwrap()[0]
                .value
                .is_null()
        );
        let change = apply(&settings, "meetings.summaries", "opencode", &rules()).unwrap();
        assert_eq!(
            change.settings.meetings.summaries,
            crate::app_settings::MeetingSummaries::OpenCode
        );
        assert_eq!(
            get(&change.settings, "meetings.summaries").unwrap()[0].value,
            "opencode"
        );
        let retired = apply(
            &settings,
            "meetings.summarize_with_opencode",
            "on",
            &rules(),
        )
        .unwrap_err();
        assert!(retired.message.contains("meetings.summaries opencode"));
        let mut saved = settings.clone();
        saved.microphone = Some("USB Mic".into());
        let change = apply(&saved, "microphone", "system-default", &rules()).unwrap();
        assert_eq!(change.settings.microphone, None);
        let history = apply(&settings, "history_retention", "month", &rules()).unwrap();
        let change = apply(&settings, "meetings.audio_retention", "week", &rules()).unwrap();
        assert_eq!(
            change.settings.meetings.audio_retention,
            Some(crate::app_settings::MeetingRetention::Week)
        );
        let cleared = apply(
            &change.settings,
            "meetings.audio_retention",
            "none",
            &rules(),
        )
        .unwrap();
        assert_eq!(cleared.settings.meetings.audio_retention, None);
        assert_eq!(
            history.effects,
            vec![Effect::HistoryRetention(
                crate::history::HistoryRetention::Month
            )]
        );
    }

    /// Issue 767: Menu alone (Caps Lock remapped to it) is a Linux
    /// shortcut only with Labs › Special keys alone on, and turning the
    /// toggle off puts it back to the default. Windows takes it alone
    /// without the toggle (#794).
    #[test]
    fn menu_alone_needs_its_labs_toggle() {
        assert!(parse_hotkey("menu", rules().key_named).is_ok());
        assert!(parse_hotkey("space", rules().key_named).is_err());
        let settings = AppSettings::default();
        let without_toggle = apply(&settings, "dictation_hotkey", "menu", &rules());
        if cfg!(target_os = "windows") {
            let change = without_toggle.unwrap();
            assert_eq!(format_hotkey(&change.settings.dictation_hotkey), "menu");
            return;
        }
        assert_eq!(without_toggle.unwrap_err().code, "invalid-value");
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

    /// Issue 1050: § alone is a Linux shortcut only with Labs › Section key
    /// alone on, and turning the toggle off puts it back to the default.
    #[test]
    fn section_alone_needs_its_labs_toggle() {
        let settings = AppSettings::default();
        let without_toggle = apply(&settings, "dictation_hotkey", "§", &rules());
        assert_eq!(without_toggle.unwrap_err().code, "invalid-value");
        if !cfg!(any(target_os = "linux", target_os = "macos")) {
            return;
        }
        let on = apply(&settings, "labs.section_key_alone", "on", &rules())
            .unwrap()
            .settings;
        let change = apply(&on, "dictation_hotkey", "§", &rules()).unwrap();
        assert!(change.settings.dictation_hotkey.is_lone_section_key());
        assert_eq!(format_hotkey(&change.settings.dictation_hotkey), "§");
        let off = apply(&change.settings, "labs.section_key_alone", "off", &rules()).unwrap();
        assert_eq!(off.settings.dictation_hotkey, HotkeyBinding::default());
    }

    #[test]
    fn interface_language_offers_every_translation() {
        let key = keys().find(|key| key.key == "ui_language").unwrap();
        let Kind::Choice(choices) = key.kind else {
            panic!("ui_language is a choice");
        };
        let languages: Vec<&str> = crate::desktop::i18n::LANGUAGE_CHOICES
            .iter()
            .filter_map(|(code, _)| *code)
            .collect();
        assert_eq!(languages, choices);
        let change = apply(&AppSettings::default(), "ui_language", "fr", &rules()).unwrap();
        assert_eq!(change.settings.ui_language.as_deref(), Some("fr"));
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

    /// `extra_dictation_hotkeys` takes up to four rows, each binding meets a
    /// recorded shortcut's checks, a row's mode folds to one its binding can
    /// trigger, and the key only works on macOS.
    #[test]
    fn extra_dictation_hotkeys_takes_up_to_four_checked_rows() {
        use crate::app_settings::{
            ExtraDictationMode, HotkeyModifiers, MAX_EXTRA_DICTATION_SHORTCUTS,
        };
        let rules = rules();
        let binding = |name: &str| {
            serde_json::to_value(HotkeyBinding {
                modifiers: HotkeyModifiers::control_option(),
                key: Some(HotkeyKey::named(name)),
            })
            .unwrap()
        };
        let rows = |rows: Vec<Value>| Value::Array(rows).to_string();

        let change = apply(
            &AppSettings::default(),
            "extra_dictation_hotkeys",
            &rows(vec![
                serde_json::json!({"binding": binding("f5"), "mode": "tap"}),
            ]),
            &rules,
        )
        .unwrap();
        assert_eq!(change.settings.extra_dictation_hotkeys.len(), 1);
        assert_eq!(
            change.settings.extra_dictation_hotkeys[0].mode,
            ExtraDictationMode::Tap
        );

        // A row without a key can't be double-tap only.
        let change = apply(
            &AppSettings::default(),
            "extra_dictation_hotkeys",
            &rows(vec![serde_json::json!({"mode": "double_tap"})]),
            &rules,
        )
        .unwrap();
        assert_eq!(
            change.settings.extra_dictation_hotkeys[0].mode,
            ExtraDictationMode::HoldOrDoubleTap
        );

        // More than four rows are refused.
        let too_many = rows(
            (0..=MAX_EXTRA_DICTATION_SHORTCUTS)
                .map(|_| serde_json::json!({"mode": "hold"}))
                .collect(),
        );
        assert_eq!(
            apply(
                &AppSettings::default(),
                "extra_dictation_hotkeys",
                &too_many,
                &rules
            )
            .unwrap_err()
            .code,
            "invalid-value"
        );

        // Windows accepts Menu alone without the special-keys Lab.
        let menu = HotkeyBinding {
            modifiers: HotkeyModifiers::default(),
            key: Some(HotkeyKey::named("menu")),
        };
        let without_lab = apply(
            &AppSettings::default(),
            "extra_dictation_hotkeys",
            &rows(vec![serde_json::json!({"binding": menu})]),
            &rules,
        );
        if cfg!(target_os = "windows") {
            let change = without_lab.unwrap();
            assert_eq!(
                change.settings.extra_dictation_hotkeys[0].binding,
                Some(menu)
            );
        } else {
            assert_eq!(without_lab.unwrap_err().code, "invalid-value");
        }

        // The OS's own rule for a recorded shortcut applies to a row too.
        let refuse = ShortcutRules {
            key_named: rules.key_named,
            rule: |_, _| Err("Modifier-only shortcuts aren't supported here".into()),
        };
        assert_eq!(
            apply(
                &AppSettings::default(),
                "extra_dictation_hotkeys",
                &rows(vec![serde_json::json!({"binding": binding("f5")})]),
                &refuse
            )
            .unwrap_err()
            .code,
            "invalid-value"
        );

        // Two rows can't share a key while their Lab is on (macOS).
        #[cfg(target_os = "macos")]
        {
            let mut settings = AppSettings::default();
            settings.labs.more_dictation_shortcuts = true;
            assert_eq!(
                apply(
                    &settings,
                    "extra_dictation_hotkeys",
                    &rows(vec![
                        serde_json::json!({"binding": binding("f5")}),
                        serde_json::json!({"binding": binding("f5")}),
                    ]),
                    &rules
                )
                .unwrap_err()
                .code,
                "shortcut-in-use"
            );
        }

        // Off macOS the key is refused before it is read.
        if cfg!(target_os = "macos") {
            assert!(unavailable("extra_dictation_hotkeys").is_none());
        } else {
            assert_eq!(
                unavailable("extra_dictation_hotkeys").map(|error| error.code),
                Some("unsupported")
            );
        }
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
    fn double_tap_only_takes_a_lone_modifier_with_the_lab() {
        let settings = AppSettings {
            dictation_hotkey: parse_hotkey("option", rules().key_named).unwrap(),
            ..AppSettings::default()
        };
        let refused = apply(&settings, "double_tap_only", "on", &rules()).unwrap_err();
        assert_eq!(refused.code, "invalid-value");
        // Only macOS has the lab, so only there does the refusal name it.
        assert_eq!(
            refused
                .message
                .contains("Labs › Double-tap on a lone modifier"),
            cfg!(target_os = "macos"),
            "{}",
            refused.message
        );
        let mut with_lab = settings.clone();
        with_lab.labs.double_tap_modifiers = true;
        if with_lab.double_tap_modifiers_allowed() {
            let change = apply(&with_lab, "double_tap_only", "on", &rules()).unwrap();
            assert!(change.settings.double_tap_only);
        } else {
            assert!(apply(&with_lab, "double_tap_only", "on", &rules()).is_err());
        }
        // Turning the lab off clears a modifier-only double-tap-only start.
        with_lab.double_tap_only = true;
        let change = apply(&with_lab, "labs.double_tap_modifiers", "off", &rules()).unwrap();
        assert!(!change.settings.double_tap_only);
        assert!(change.settings.double_tap_lock);
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
