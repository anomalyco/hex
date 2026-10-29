//! Input-source changes are model-picker choices, not recording-time routing.

#[cfg(any(target_os = "macos", test))]
use std::time::{Duration, Instant};

#[cfg(any(target_os = "macos", test))]
use crate::app_settings::{KeyboardModelChoice, KeyboardModelSwitching};
use crate::transcription_models::ModelDefinition;

#[cfg(any(target_os = "macos", test))]
const DEBOUNCE: Duration = Duration::from_millis(300);

/// One enabled keyboard input source, as the settings window lists it.
#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct InputSource {
    pub(crate) id: String,
    pub(crate) name: String,
    /// The source's BCP 47 languages, most specific first (`ru`, `zh-Hans`).
    pub(crate) languages: Vec<String>,
}

impl InputSource {
    /// The language a new mapping to `model` starts with: the first of this
    /// source's languages the model supports, else `fallback`.
    pub(crate) fn language_for(&self, model: &ModelDefinition, fallback: &str) -> String {
        self.languages
            .iter()
            .map(|tag| tag.split(['-', '_']).next().unwrap_or(tag))
            .find(|code| model.supports_language(code))
            .unwrap_or(fallback)
            .to_owned()
    }
}

/// No models or workers live here: this only admits the latest picker choice.
#[cfg(any(target_os = "macos", test))]
pub(crate) struct Switcher {
    settings: KeyboardModelSwitching,
    /// The observed source; unknown while switching is off, so turning it
    /// on waits for the observer's first report instead of a stale source.
    current: Option<String>,
    due: Option<Instant>,
}

#[cfg(any(target_os = "macos", test))]
impl Switcher {
    pub(crate) fn new(settings: KeyboardModelSwitching, current: Option<String>) -> Self {
        Self {
            current: current.filter(|_| settings.enabled),
            settings,
            due: None,
        }
    }

    /// Returns true on a real change, so the owner can cancel obsolete work.
    pub(crate) fn source_changed(&mut self, source: Option<String>, now: Instant) -> bool {
        if !self.settings.enabled || self.current == source {
            return false;
        }
        self.current = source;
        self.due = self.mapped().map(|_| now + DEBOUNCE);
        true
    }

    fn mapped(&self) -> Option<&KeyboardModelChoice> {
        self.settings
            .enabled
            .then(|| self.settings.mappings.get(self.current.as_ref()?))
            .flatten()
    }

    pub(crate) fn take_due(&mut self, now: Instant) -> Option<KeyboardModelChoice> {
        if now < self.due? {
            return None;
        }
        self.due = None;
        self.mapped().cloned()
    }

    pub(crate) fn manual_choice(&mut self) {
        self.due = None;
    }

    /// Only enabling, disabling, or editing the active mapping affects work.
    /// Returns true when the owner must supersede an automatic preparation.
    pub(crate) fn configure(&mut self, settings: KeyboardModelSwitching, now: Instant) -> bool {
        let before = (self.settings.enabled, self.mapped().cloned());
        self.settings = settings;
        if !self.settings.enabled {
            self.current = None;
        }
        if before == (self.settings.enabled, self.mapped().cloned()) {
            return false;
        }
        self.due = self.mapped().map(|_| now);
        true
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::transcription_models::{TranscriptionModelId, definition};

    fn choice(model: TranscriptionModelId, language: &str) -> KeyboardModelChoice {
        KeyboardModelChoice {
            model,
            language: language.into(),
        }
    }

    fn mapped(entries: &[(&str, KeyboardModelChoice)]) -> KeyboardModelSwitching {
        KeyboardModelSwitching {
            enabled: true,
            mappings: entries
                .iter()
                .map(|(source, choice)| ((*source).into(), choice.clone()))
                .collect(),
        }
    }

    fn spanish() -> KeyboardModelChoice {
        choice(TranscriptionModelId::ParakeetV3, "es")
    }

    #[test]
    fn a_mapped_startup_source_does_not_switch_until_its_mapping_is_edited() {
        let now = Instant::now();
        let mut settings = mapped(&[("Spanish", spanish())]);
        let mut switcher = Switcher::new(settings.clone(), Some("Spanish".into()));
        assert_eq!(switcher.take_due(now + Duration::from_secs(1)), None);
        let whisper = choice(TranscriptionModelId::WhisperLargeV3Turbo, "es");
        settings.mappings.insert("Spanish".into(), whisper.clone());
        assert!(switcher.configure(settings, now));
        assert_eq!(switcher.take_due(now), Some(whisper));
    }

    #[test]
    fn source_changes_are_debounced_and_duplicates_ignored() {
        let now = Instant::now();
        let mut switcher = Switcher::new(mapped(&[("Spanish", spanish())]), Some("ABC".into()));
        assert!(!switcher.source_changed(Some("ABC".into()), now));
        assert_eq!(switcher.take_due(now + Duration::from_secs(1)), None);
        assert!(switcher.source_changed(Some("Spanish".into()), now));
        assert_eq!(switcher.take_due(now + Duration::from_millis(299)), None);
        assert_eq!(
            switcher.take_due(now + Duration::from_millis(300)),
            Some(spanish())
        );
        assert_eq!(switcher.take_due(now + Duration::from_secs(1)), None);
    }

    #[test]
    fn a_language_only_edit_of_the_active_mapping_applies_now() {
        let now = Instant::now();
        let mut settings = mapped(&[("Russian", choice(TranscriptionModelId::ParakeetV3, "en"))]);
        let mut switcher = Switcher::new(settings.clone(), Some("Russian".into()));
        let russian = choice(TranscriptionModelId::ParakeetV3, "ru");
        settings.mappings.insert("Russian".into(), russian.clone());
        assert!(switcher.configure(settings, now));
        assert_eq!(switcher.take_due(now), Some(russian));
    }

    #[test]
    fn a_manual_choice_cancels_the_pending_switch_until_another_source_change() {
        let now = Instant::now();
        let mut switcher = Switcher::new(mapped(&[("Spanish", spanish())]), Some("ABC".into()));
        switcher.source_changed(Some("Spanish".into()), now);
        switcher.manual_choice();
        assert_eq!(switcher.take_due(now + Duration::from_secs(1)), None);
        // Duplicate native notifications must not undo the manual choice.
        assert!(!switcher.source_changed(Some("Spanish".into()), now));
        assert_eq!(switcher.take_due(now + Duration::from_secs(2)), None);
    }

    #[test]
    fn enabling_or_editing_the_active_mapping_applies_now_but_other_edits_do_not() {
        let now = Instant::now();
        let mut switcher = Switcher::new(KeyboardModelSwitching::default(), None);
        let mut settings = mapped(&[("Spanish", spanish())]);
        // Enabling waits for the observer's report of the current source.
        assert!(switcher.configure(settings.clone(), now));
        assert_eq!(switcher.take_due(now), None);
        assert!(switcher.source_changed(Some("Spanish".into()), now));
        assert_eq!(switcher.take_due(now + DEBOUNCE), Some(spanish()));
        settings.mappings.insert(
            "ABC".into(),
            choice(TranscriptionModelId::ParakeetUnifiedEnglish, "en"),
        );
        assert!(!switcher.configure(settings.clone(), now));
        assert_eq!(switcher.take_due(now), None);
        let whisper = choice(TranscriptionModelId::WhisperLargeV3Turbo, "es");
        settings.mappings.insert("Spanish".into(), whisper.clone());
        assert!(switcher.configure(settings.clone(), now));
        assert_eq!(switcher.take_due(now), Some(whisper));
        settings.enabled = false;
        assert!(switcher.configure(settings, now));
        assert_eq!(switcher.take_due(now), None);
    }

    #[test]
    fn cycling_past_a_source_only_switches_to_the_last_mapping() {
        let now = Instant::now();
        let french = choice(TranscriptionModelId::WhisperLargeV3Turbo, "fr");
        let mut switcher = Switcher::new(
            mapped(&[("French", french), ("Spanish", spanish())]),
            Some("ABC".into()),
        );
        switcher.source_changed(Some("French".into()), now);
        switcher.source_changed(Some("Spanish".into()), now + Duration::from_millis(100));
        assert_eq!(switcher.take_due(now + Duration::from_millis(300)), None);
        assert_eq!(
            switcher.take_due(now + Duration::from_millis(400)),
            Some(spanish())
        );
        switcher.source_changed(Some("French".into()), now + Duration::from_secs(1));
        switcher.source_changed(Some("ABC".into()), now + Duration::from_millis(1100));
        assert_eq!(switcher.take_due(now + Duration::from_secs(2)), None);
    }

    #[test]
    fn turning_off_the_feature_cancels_a_debounced_switch_and_forgets_the_source() {
        let now = Instant::now();
        let mut settings = mapped(&[("Spanish", spanish())]);
        let mut switcher = Switcher::new(settings.clone(), Some("ABC".into()));
        switcher.source_changed(Some("Spanish".into()), now);
        settings.enabled = false;
        switcher.configure(settings.clone(), now + Duration::from_millis(100));
        assert_eq!(switcher.take_due(now + Duration::from_secs(1)), None);
        assert!(!switcher.source_changed(Some("ABC".into()), now));
        assert!(!switcher.source_changed(Some("Spanish".into()), now));
        assert_eq!(switcher.take_due(now + Duration::from_secs(2)), None);
        // Turning it back on does not act on the source seen before.
        settings.enabled = true;
        switcher.configure(settings, now);
        assert_eq!(switcher.take_due(now + Duration::from_secs(3)), None);
    }

    #[test]
    fn a_new_mapping_starts_in_the_sources_language_when_the_model_has_it() {
        let source = InputSource {
            id: "com.apple.keylayout.Russian".into(),
            name: "Russian".into(),
            languages: vec!["ru".into(), "en-US".into()],
        };
        let v3 = definition(TranscriptionModelId::ParakeetV3);
        let english = definition(TranscriptionModelId::ParakeetUnifiedEnglish);
        assert_eq!(source.language_for(v3, "en"), "ru");
        assert_eq!(source.language_for(english, "en"), "en");
        let unknown = InputSource {
            languages: vec!["xx".into()],
            ..source
        };
        assert_eq!(unknown.language_for(v3, "de"), "de");
    }
}
