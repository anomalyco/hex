use std::fs::{self, File};
use std::io::Write;
use std::path::PathBuf;

use color_eyre::eyre::{Result, WrapErr, eyre};
use serde::{Deserialize, Serialize};
use windows_sys::Win32::Globalization::GetUserDefaultLocaleName;

use crate::transcription_models::{TranscriptionModelId, TranscriptionSelection};

const LOCALE_NAME_CAPACITY: usize = 85;

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(default)]
pub struct WindowsSettings {
    pub schema_version: u32,
    pub platform: String,
    pub microphone: Option<String>,
    pub listen_on_launch: bool,
    pub dictation_hotkey: WindowsHotkey,
    pub paste_last_hotkey: Option<WindowsHotkey>,
    pub double_tap_lock: bool,
    pub double_tap_only: bool,
    pub while_dictating: WhileDictating,
    pub release_microphone_while_idle: bool,
    pub feedback_volume: u8,
    pub ui_language: Option<String>,
    pub indicator_position: IndicatorPosition,
    pub history_retention: crate::history::HistoryRetention,
    pub text_replacements: Vec<crate::text_replacements::TextReplacement>,
    pub modes: Vec<WindowsMode>,
    pub transcription: TranscriptionSelection,
}

/// A per-application dictation mode: when the focused application matches,
/// the mode's corrections run after the global text replacements.
#[derive(Clone, Debug, Default, Deserialize, Eq, PartialEq, Serialize)]
#[serde(default)]
pub struct WindowsMode {
    pub name: String,
    /// Case-insensitive substrings matched against the focused process's
    /// executable name (e.g. "chrome", "code", "slack").
    pub applications: Vec<String>,
    pub corrections: Vec<crate::text_replacements::TextReplacement>,
}

impl WindowsMode {
    pub fn matches_application(&self, application: &str) -> bool {
        let application = application.to_lowercase();
        self.applications.iter().any(|candidate| {
            let candidate = candidate.trim().to_lowercase();
            !candidate.is_empty() && application.contains(&candidate)
        })
    }
}

/// The first mode whose application rules match the focused application.
pub fn mode_for_application<'a>(
    modes: &'a [WindowsMode],
    application: Option<&str>,
) -> Option<&'a WindowsMode> {
    let application = application?;
    modes
        .iter()
        .find(|mode| mode.matches_application(application))
}

/// Where the recording HUD pill appears while dictating.
#[derive(Clone, Copy, Debug, Default, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum IndicatorPosition {
    #[default]
    Top,
    Bottom,
    Hidden,
}

/// What happens to other applications' audio while a dictation records.
#[derive(Clone, Copy, Debug, Default, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum WhileDictating {
    Mute,
    PauseMedia,
    #[default]
    DoNothing,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(default)]
pub struct WindowsHotkey {
    pub control: bool,
    pub windows: bool,
    pub alt: bool,
    pub shift: bool,
    pub key: Option<String>,
}

impl Default for WindowsSettings {
    fn default() -> Self {
        Self {
            schema_version: 1,
            platform: "windows".into(),
            microphone: None,
            listen_on_launch: true,
            dictation_hotkey: WindowsHotkey::default(),
            paste_last_hotkey: Some(WindowsHotkey::paste_last_default()),
            double_tap_lock: true,
            double_tap_only: false,
            while_dictating: WhileDictating::default(),
            release_microphone_while_idle: false,
            feedback_volume: 50,
            ui_language: None,
            indicator_position: IndicatorPosition::default(),
            history_retention: crate::history::HistoryRetention::default(),
            modes: Vec::new(),
            text_replacements: Vec::new(),
            transcription: recommended_selection(&user_language()),
        }
    }
}

impl Default for WindowsHotkey {
    fn default() -> Self {
        Self {
            control: true,
            windows: true,
            alt: false,
            shift: false,
            key: None,
        }
    }
}

impl WindowsSettings {
    pub fn load() -> Result<Self> {
        let path = settings_path()?;
        if !path.exists() {
            return Ok(Self::default());
        }
        let settings: Self = serde_json::from_slice(&fs::read(&path)?)
            .wrap_err_with(|| format!("could not parse {}", path.display()))?;
        if settings.platform != "windows" {
            return Err(eyre!(
                "unsupported Windows settings platform: {}",
                settings.platform
            ));
        }
        settings.dictation_hotkey.validate()?;
        if settings.feedback_volume > 100 {
            return Err(eyre!("feedback volume must be between 0 and 100"));
        }
        if let Some(paste_last) = &settings.paste_last_hotkey {
            paste_last.validate()?;
            if paste_last.key.is_none() {
                return Err(eyre!("Paste Last requires a non-modifier key"));
            }
            if paste_last == &settings.dictation_hotkey {
                return Err(eyre!(
                    "the dictation and Paste Last shortcuts must be different"
                ));
            }
        }
        crate::transcription_models::validate(&settings.transcription)?;
        Ok(settings)
    }

    pub fn save(&self) -> Result<()> {
        self.dictation_hotkey.validate()?;
        if self.feedback_volume > 100 {
            return Err(eyre!("feedback volume must be between 0 and 100"));
        }
        if let Some(paste_last) = &self.paste_last_hotkey {
            paste_last.validate()?;
            if paste_last.key.is_none() {
                return Err(eyre!("Paste Last requires a non-modifier key"));
            }
            if paste_last == &self.dictation_hotkey {
                return Err(eyre!(
                    "the dictation and Paste Last shortcuts must be different"
                ));
            }
        }
        crate::transcription_models::validate(&self.transcription)?;
        let path = settings_path()?;
        let directory = path
            .parent()
            .ok_or_else(|| eyre!("Windows settings path has no parent"))?;
        fs::create_dir_all(directory)?;
        let partial = path.with_extension("json.partial");
        let mut file = File::create(&partial)?;
        serde_json::to_writer_pretty(&mut file, self)?;
        file.write_all(b"\n")?;
        file.sync_all()?;
        crate::transcription_models::atomic_replace(&partial, &path)?;
        Ok(())
    }
}

impl WindowsHotkey {
    pub fn ctrl_alt_space() -> Self {
        Self {
            control: true,
            windows: false,
            alt: true,
            shift: false,
            key: Some("space".into()),
        }
    }

    pub fn paste_last_default() -> Self {
        Self {
            control: true,
            windows: false,
            alt: true,
            shift: false,
            key: Some("v".into()),
        }
    }

    pub fn validate(&self) -> Result<()> {
        let modifier_count = [self.control, self.windows, self.alt, self.shift]
            .into_iter()
            .filter(|enabled| *enabled)
            .count();
        let Some(key) = self.key.as_deref() else {
            if modifier_count < 2 {
                return Err(eyre!(
                    "modifier-only Windows dictation shortcuts require at least two modifiers"
                ));
            }
            return Ok(());
        };
        if key.trim().is_empty() {
            return Err(eyre!("Windows dictation shortcut key cannot be empty"));
        }
        crate::windows_input::virtual_key(key)?;
        let function_key = key
            .strip_prefix('f')
            .or_else(|| key.strip_prefix('F'))
            .and_then(|number| number.parse::<u8>().ok())
            .is_some_and(|number| (1..=24).contains(&number));
        if !function_key && modifier_count == 0 {
            return Err(eyre!(
                "Windows dictation shortcuts require a modifier or a function key"
            ));
        }
        Ok(())
    }

    pub fn label(&self) -> String {
        self.keycaps().join("+")
    }

    pub fn keycaps(&self) -> Vec<String> {
        let mut parts = Vec::new();
        if self.control {
            parts.push("Ctrl".into());
        }
        if self.windows {
            parts.push("Win".into());
        }
        if self.alt {
            parts.push("Alt".into());
        }
        if self.shift {
            parts.push("Shift".into());
        }
        if let Some(key) = &self.key {
            parts.push(match key.to_ascii_lowercase().as_str() {
                "space" => "Space".into(),
                "enter" | "return" => "Enter".into(),
                key => key.to_ascii_uppercase(),
            });
        }
        parts
    }
}

pub fn recommended_selection(language: &str) -> TranscriptionSelection {
    let language = if language.eq_ignore_ascii_case("pl") {
        "pl"
    } else {
        "en"
    };
    TranscriptionSelection {
        model: if language == "pl" {
            TranscriptionModelId::ParakeetV3
        } else {
            TranscriptionModelId::ParakeetUnifiedEnglish
        },
        language: language.into(),
        recognition_hints: String::new(),
    }
}

pub(crate) fn user_language() -> String {
    let mut buffer = [0_u16; LOCALE_NAME_CAPACITY];
    let length = unsafe { GetUserDefaultLocaleName(buffer.as_mut_ptr(), buffer.len() as i32) };
    if length <= 1 {
        return "en".into();
    }
    let locale = String::from_utf16_lossy(&buffer[..length as usize - 1]);
    locale
        .split(['-', '_'])
        .next()
        .unwrap_or("en")
        .to_ascii_lowercase()
}

fn settings_path() -> Result<PathBuf> {
    Ok(crate::app_paths::support_dir()?.join("windows-settings.json"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn polish_uses_the_multilingual_model() {
        let selection = recommended_selection("pl");
        assert_eq!(selection.model, TranscriptionModelId::ParakeetV3);
        assert_eq!(selection.language, "pl");
    }

    #[test]
    fn other_languages_fall_back_to_english() {
        let selection = recommended_selection("de");
        assert_eq!(
            selection.model,
            TranscriptionModelId::ParakeetUnifiedEnglish
        );
        assert_eq!(selection.language, "en");
    }

    #[test]
    fn default_binding_matches_the_windows_push_to_talk_convention() {
        let binding = WindowsHotkey::default();
        assert_eq!(binding.label(), "Ctrl+Win");
        assert!(binding.validate().is_ok());
    }

    #[test]
    fn retained_history_defaults_to_one_week() {
        assert_eq!(
            WindowsSettings::default().history_retention,
            crate::history::HistoryRetention::Week
        );
    }

    #[test]
    fn modes_match_case_insensitive_application_substrings() {
        let modes = vec![
            WindowsMode {
                name: "Code".into(),
                applications: vec!["Code".into(), "rustrover".into()],
                corrections: Vec::new(),
            },
            WindowsMode {
                name: "Chat".into(),
                applications: vec!["slack".into()],
                corrections: Vec::new(),
            },
        ];

        assert_eq!(
            mode_for_application(&modes, Some("code")).map(|mode| mode.name.as_str()),
            Some("Code")
        );
        assert_eq!(
            mode_for_application(&modes, Some("slack")).map(|mode| mode.name.as_str()),
            Some("Chat")
        );
        // Substring semantics: "vscode" contains "code".
        assert_eq!(
            mode_for_application(&modes, Some("vscode")).map(|mode| mode.name.as_str()),
            Some("Code")
        );
        assert_eq!(mode_for_application(&modes, Some("notepad")), None);
        assert_eq!(mode_for_application(&modes, None), None);
    }

    #[test]
    fn blank_application_rules_never_match() {
        let modes = vec![WindowsMode {
            name: "Broken".into(),
            applications: vec!["  ".into(), String::new()],
            corrections: Vec::new(),
        }];
        assert_eq!(mode_for_application(&modes, Some("anything")), None);
    }

    #[test]
    fn double_tap_lock_is_enabled_by_default() {
        assert!(WindowsSettings::default().double_tap_lock);
    }

    #[test]
    fn paste_last_defaults_to_ctrl_alt_v() {
        assert_eq!(
            WindowsSettings::default()
                .paste_last_hotkey
                .expect("Paste Last is enabled")
                .label(),
            "Ctrl+Alt+V"
        );
    }

    #[test]
    fn feedback_volume_defaults_to_fifty_percent() {
        assert_eq!(WindowsSettings::default().feedback_volume, 50);
    }

    #[test]
    fn indicator_defaults_to_the_top_of_the_screen() {
        assert_eq!(
            WindowsSettings::default().indicator_position,
            IndicatorPosition::Top
        );
    }

    #[test]
    fn unmodified_letters_are_rejected() {
        let binding = WindowsHotkey {
            control: false,
            windows: false,
            alt: false,
            shift: false,
            key: Some("d".into()),
        };
        assert!(binding.validate().is_err());
    }

    #[test]
    fn one_modifier_is_not_safe_as_a_modifier_only_binding() {
        let binding = WindowsHotkey {
            control: true,
            windows: false,
            alt: false,
            shift: false,
            key: None,
        };
        assert!(binding.validate().is_err());
    }
}
