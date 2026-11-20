//! What happens to a dictation's text before it pastes: Global's steps and
//! the modes chosen by application or website.

use serde::{Deserialize, Serialize};

use super::{KeyboardModelChoice, TextReplacement};
use crate::dictation::vocabulary::{VocabularyEntry, serialize_saved};

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
    /// The speech model and language this mode selects while it matches the
    /// foreground app (Labs › Switch model with app). None means don't switch.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub speech: Option<KeyboardModelChoice>,
    /// Names heard and spelled here ahead of the shared vocabulary.
    #[serde(
        skip_serializing_if = "Vec::is_empty",
        serialize_with = "serialize_saved"
    )]
    pub vocabulary: Vec<VocabularyEntry>,
    /// Rewrite this mode's dictations into plain, neutral wording on this Mac
    /// (Labs › Neutral voice, macOS). Off for every existing mode.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub neutral_voice: bool,
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

#[cfg(test)]
mod tests {
    use super::DictationMode;

    /// A mode saved before Labs › Neutral voice has no field, and loads with
    /// the stage off.
    #[test]
    fn a_mode_without_neutral_voice_loads_it_off() {
        let mode: DictationMode = serde_json::from_str(
            r#"{"name":"Chat","applications":["Slack"],"replacements":[],"transformations":[]}"#,
        )
        .unwrap();
        assert!(!mode.neutral_voice);
    }

    #[test]
    fn a_mode_keeps_its_neutral_voice_choice() {
        let mode: DictationMode =
            serde_json::from_str(r#"{"name":"Chat","neutral_voice":true}"#).unwrap();
        assert!(mode.neutral_voice);
        let saved = serde_json::to_value(&mode).unwrap();
        assert_eq!(saved["neutral_voice"], true);
    }

    /// An off mode writes no key, so an existing settings file isn't
    /// rewritten with `"neutral_voice": false`.
    #[test]
    fn an_off_mode_serializes_without_neutral_voice() {
        let saved = serde_json::to_value(DictationMode::default()).unwrap();
        assert!(saved.get("neutral_voice").is_none());
    }
}
