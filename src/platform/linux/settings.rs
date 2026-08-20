use std::fs::{self, File};
use std::io::Write;
use std::path::PathBuf;

use color_eyre::eyre::{Result, WrapErr, eyre};
use serde::{Deserialize, Serialize};
use x11rb::protocol::xproto::ModMask;
use x11rb::rust_connection::RustConnection;

use crate::linux_input::{Keymap, XK_ALT_L, XK_ALT_R};

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(default)]
pub struct LinuxSettings {
    pub schema_version: u32,
    pub platform: String,
    pub dictation_hotkey: LinuxHotkey,
    /// Optional second held shortcut for Voice Action. `None` keeps the
    /// OpenCode-backed capture target disabled.
    pub voice_action_hotkey: Option<LinuxHotkey>,
    /// Dedicated generation settings; ordinary dictation modes never borrow
    /// this model or deadline.
    pub voice_action: LinuxVoiceActionSettings,
    pub double_tap_lock: bool,
    /// A preferred capture device name; `None` uses the system default.
    pub microphone: Option<String>,
    /// The interface language; `None` follows the system locale.
    pub ui_language: Option<String>,
    /// Opt-in streaming voice commands through the Moonshine runtime.
    /// This persisted prototype is honored only by developer builds until
    /// the Linux release bundles Moonshine and gains physical X11 validation.
    pub commands_enabled: bool,
    /// How long successfully pasted dictations remain in the owner-only
    /// bounded history store. `Off` stops new recording without deleting
    /// entries retained under an earlier policy.
    pub history_retention: crate::history::HistoryRetention,
    /// Phrase-boundary replacements applied after transcription and before
    /// successful output is pasted or retained in History.
    pub text_replacements: Vec<crate::text_replacements::TextReplacement>,
    /// Ordered application modes. The first case-insensitive executable-name
    /// substring match supplies corrections after global replacements.
    pub modes: Vec<LinuxMode>,
    /// Global OpenCode rewrite profile used when no application mode matches.
    pub dictation_post_processing: crate::dictation_processing::PostProcessingSettings,
    /// Ordered final transformation IDs for the global fallback profile.
    pub dictation_transformations: Vec<String>,
    pub transcription: crate::transcription_models::TranscriptionSelection,
}

#[derive(Clone, Debug, Default, Deserialize, Eq, PartialEq, Serialize)]
#[serde(default)]
pub struct LinuxMode {
    pub name: String,
    pub applications: Vec<String>,
    pub corrections: Vec<crate::text_replacements::TextReplacement>,
    pub post_processing: crate::dictation_processing::PostProcessingSettings,
    pub transformations: Vec<String>,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(default)]
pub struct LinuxVoiceActionSettings {
    pub model: Option<crate::opencode::Model>,
    pub deadline_seconds: u64,
}

impl Default for LinuxVoiceActionSettings {
    fn default() -> Self {
        Self {
            model: None,
            deadline_seconds: 60,
        }
    }
}

impl LinuxMode {
    pub fn matches_application(&self, application: &str) -> bool {
        let application = application.to_lowercase();
        self.applications.iter().any(|candidate| {
            let candidate = candidate.trim().to_lowercase();
            !candidate.is_empty() && application.contains(&candidate)
        })
    }
}

impl LinuxSettings {
    pub fn transformations_enabled(&self) -> bool {
        !self.dictation_transformations.is_empty()
            || self
                .modes
                .iter()
                .any(|mode| !mode.transformations.is_empty())
    }

    fn validate(&self) -> Result<()> {
        self.dictation_hotkey.validate()?;
        if let Some(voice_action) = &self.voice_action_hotkey {
            voice_action.validate()?;
            if voice_action == &self.dictation_hotkey {
                return Err(eyre!(
                    "the dictation and Voice Action shortcuts must be different"
                ));
            }
        }
        if self.voice_action.deadline_seconds == 0 {
            return Err(eyre!(
                "the Voice Action deadline must be at least one second"
            ));
        }
        if let Some(model) = &self.voice_action.model
            && (model.provider.trim().is_empty() || model.id.trim().is_empty())
        {
            return Err(eyre!(
                "the Voice Action model requires a provider and model id"
            ));
        }
        crate::transcription_models::validate(&self.transcription).map(|_| ())
    }
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(default)]
pub struct LinuxHotkey {
    pub control: bool,
    pub alt: bool,
    pub shift: bool,
    pub super_key: bool,
    pub key: String,
}

impl Default for LinuxSettings {
    fn default() -> Self {
        Self {
            schema_version: 1,
            platform: "x11".into(),
            dictation_hotkey: LinuxHotkey::default(),
            voice_action_hotkey: None,
            voice_action: LinuxVoiceActionSettings::default(),
            double_tap_lock: true,
            microphone: None,
            ui_language: None,
            commands_enabled: false,
            history_retention: crate::history::HistoryRetention::default(),
            text_replacements: Vec::new(),
            modes: Vec::new(),
            dictation_post_processing: crate::dictation_processing::PostProcessingSettings::default(
            ),
            dictation_transformations: Vec::new(),
            transcription: crate::transcription_models::TranscriptionSelection::default(),
        }
    }
}

impl Default for LinuxHotkey {
    fn default() -> Self {
        Self {
            control: false,
            alt: true,
            shift: false,
            super_key: false,
            key: "space".into(),
        }
    }
}

impl LinuxSettings {
    pub fn load() -> Result<Self> {
        let path = settings_path()?;
        if !path.exists() {
            return Ok(Self::default());
        }
        let settings: Self = serde_json::from_slice(&fs::read(&path)?)
            .wrap_err_with(|| format!("could not parse {}", path.display()))?;
        if settings.platform != "x11" {
            return Err(eyre!(
                "unsupported Linux settings platform: {}",
                settings.platform
            ));
        }
        settings.validate()?;
        Ok(settings)
    }

    pub fn save(&self) -> Result<()> {
        self.validate()?;
        let path = settings_path()?;
        let directory = path.parent().unwrap();
        fs::create_dir_all(directory)?;
        let partial = path.with_extension("json.partial");
        let mut file = File::create(&partial)?;
        serde_json::to_writer_pretty(&mut file, self)?;
        file.write_all(b"\n")?;
        file.sync_all()?;
        fs::rename(partial, &path)?;
        File::open(directory)?.sync_all()?;
        Ok(())
    }
}

impl LinuxHotkey {
    pub fn voice_action_default() -> Self {
        Self {
            control: true,
            alt: true,
            shift: false,
            super_key: false,
            key: "i".into(),
        }
    }

    pub fn validate(&self) -> Result<()> {
        if self.key.trim().is_empty() {
            return Err(eyre!("Linux hotkeys require a non-modifier key"));
        }
        Ok(())
    }

    pub fn label(&self) -> String {
        self.keycaps().join("+")
    }

    pub fn keycaps(&self) -> Vec<String> {
        let mut parts = Vec::new();
        if self.control {
            parts.push("Ctrl".to_string());
        }
        if self.alt {
            parts.push("Alt".to_string());
        }
        if self.shift {
            parts.push("Shift".to_string());
        }
        if self.super_key {
            parts.push("Super".to_string());
        }
        let key = if self.key == "space" {
            "Space".into()
        } else {
            self.key.to_ascii_uppercase()
        };
        parts.push(key);
        parts
    }

    pub(crate) fn modifier_mask(
        &self,
        connection: &RustConnection,
        keymap: &Keymap,
    ) -> Result<ModMask> {
        let mut result = ModMask::default();
        for (enabled, symbols) in [
            (self.control, &[0xffe3, 0xffe4][..]),
            (self.alt, &[XK_ALT_L, XK_ALT_R][..]),
            (self.shift, &[0xffe1, 0xffe2][..]),
            (self.super_key, &[0xffeb, 0xffec][..]),
        ] {
            if enabled {
                result |= keymap.modifier_for(connection, symbols)?;
            }
        }
        Ok(result)
    }
}

/// Whether a persisted settings file exists; a first run has none, and
/// the app then offers onboarding.
pub fn exists() -> bool {
    settings_path().is_ok_and(|path| path.exists())
}

fn settings_path() -> Result<PathBuf> {
    Ok(crate::app_paths::support_dir()?.join("linux-settings.json"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_binding_is_alt_space() {
        assert_eq!(
            LinuxSettings::default().dictation_hotkey.label(),
            "Alt+Space"
        );
    }

    #[test]
    fn voice_action_default_is_unique_and_disabled_until_opted_in() {
        let settings = LinuxSettings::default();
        let voice_action = LinuxHotkey::voice_action_default();

        assert!(settings.voice_action_hotkey.is_none());
        assert_ne!(voice_action, settings.dictation_hotkey);
        assert_eq!(voice_action.label(), "Ctrl+Alt+I");
        assert_eq!(settings.voice_action.deadline_seconds, 60);
    }

    #[test]
    fn modifier_only_bindings_are_rejected() {
        let binding = LinuxHotkey {
            key: String::new(),
            ..LinuxHotkey::default()
        };
        assert!(binding.validate().is_err());
    }

    #[test]
    fn standalone_function_keys_are_accepted() {
        let binding = LinuxHotkey {
            alt: false,
            key: "f12".into(),
            ..LinuxHotkey::default()
        };
        assert_eq!(binding.label(), "F12");
        assert!(binding.validate().is_ok());
    }

    #[test]
    fn settings_without_transcription_keep_the_default_selection() {
        let settings: LinuxSettings = serde_json::from_str(
            r#"{
                "schema_version": 1,
                "platform": "x11",
                "dictation_hotkey": {
                    "control": false,
                    "alt": true,
                    "shift": false,
                    "super_key": false,
                    "key": "space"
                },
                "double_tap_lock": true
            }"#,
        )
        .unwrap();

        assert_eq!(
            settings.transcription,
            crate::transcription_models::TranscriptionSelection::default()
        );
        assert!(!settings.commands_enabled);
        assert_eq!(
            settings.history_retention,
            crate::history::HistoryRetention::default()
        );
        assert!(settings.text_replacements.is_empty());
        assert!(settings.modes.is_empty());
        assert_eq!(
            settings.dictation_post_processing,
            crate::dictation_processing::PostProcessingSettings::default()
        );
        assert!(settings.dictation_transformations.is_empty());
        assert!(settings.voice_action_hotkey.is_none());
        assert_eq!(settings.voice_action, LinuxVoiceActionSettings::default());
    }

    #[test]
    fn voice_action_settings_round_trip_and_require_a_unique_shortcut() {
        let settings = LinuxSettings {
            voice_action_hotkey: Some(LinuxHotkey::voice_action_default()),
            voice_action: LinuxVoiceActionSettings {
                model: Some(crate::opencode::Model {
                    provider: "openai".into(),
                    id: "gpt-5".into(),
                    variant: Some("high".into()),
                }),
                deadline_seconds: 25,
            },
            ..LinuxSettings::default()
        };

        let encoded = serde_json::to_string(&settings).unwrap();
        let decoded: LinuxSettings = serde_json::from_str(&encoded).unwrap();

        assert_eq!(decoded.voice_action_hotkey, settings.voice_action_hotkey);
        assert_eq!(decoded.voice_action, settings.voice_action);
        assert!(decoded.validate().is_ok());

        let invalid = LinuxSettings {
            voice_action_hotkey: Some(settings.dictation_hotkey.clone()),
            ..settings
        };
        assert!(invalid.validate().is_err());
    }

    #[test]
    fn text_replacements_round_trip_in_the_linux_schema() {
        let settings = LinuxSettings {
            text_replacements: vec![crate::text_replacements::TextReplacement {
                matched_phrase: "open code".into(),
                output: "OpenCode".into(),
            }],
            modes: vec![LinuxMode {
                name: "Browser".into(),
                applications: vec!["firefox".into()],
                corrections: vec![crate::text_replacements::TextReplacement {
                    matched_phrase: "full stop".into(),
                    output: ".".into(),
                }],
                post_processing: crate::dictation_processing::PostProcessingSettings {
                    enabled: true,
                    prompt: "Keep browser prose concise.".into(),
                    model: Some("openai/model".into()),
                    variant: Some("high".into()),
                    deadline_seconds: 12,
                },
                transformations: vec!["trim-code-block".into(), "lowercase".into()],
            }],
            dictation_post_processing: crate::dictation_processing::PostProcessingSettings {
                prompt: "Clean up global dictation.".into(),
                ..Default::default()
            },
            dictation_transformations: vec!["lowercase".into()],
            ..LinuxSettings::default()
        };

        let encoded = serde_json::to_string(&settings).unwrap();
        let decoded: LinuxSettings = serde_json::from_str(&encoded).unwrap();

        assert_eq!(decoded.text_replacements, settings.text_replacements);
        assert_eq!(decoded.modes, settings.modes);
        assert_eq!(
            decoded.dictation_post_processing,
            settings.dictation_post_processing
        );
        assert_eq!(
            decoded.dictation_transformations,
            settings.dictation_transformations
        );
        assert!(decoded.transformations_enabled());
    }

    #[test]
    fn modes_match_case_insensitive_application_substrings_in_order() {
        let modes = [
            LinuxMode {
                name: "Browser".into(),
                applications: vec!["firefox".into()],
                ..LinuxMode::default()
            },
            LinuxMode {
                name: "Fallback browser".into(),
                applications: vec!["fox".into()],
                ..LinuxMode::default()
            },
        ];

        assert_eq!(
            modes
                .iter()
                .find(|mode| mode.matches_application("Firefox"))
                .map(|mode| mode.name.as_str()),
            Some("Browser")
        );
        assert!(!modes.iter().any(|mode| mode.matches_application("code")));
    }
}
