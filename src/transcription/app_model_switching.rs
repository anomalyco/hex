//! A mode's speech choice follows the foreground application, as keyboard
//! language switching follows the input source (Labs › Switch model with app).
//!
//! Matching is by application on macOS and Windows alike: a mode's website
//! rules are ignored. Hex's own window, and a moment with no application in
//! front at all, name no mode, so they change nothing.
//!
//! No models or workers live here: this only admits the latest mode choice
//! and says when its switch is due. The host prepares and commits the model
//! exactly as a manual picker choice does, installed only.

// The policy is hosted by macOS and Windows only; Linux compiles it for its
// unit tests.
#![cfg_attr(not(any(target_os = "macos", target_os = "windows")), allow(dead_code))]

use std::time::{Duration, Instant};

#[cfg(any(target_os = "macos", target_os = "windows"))]
use crate::app_settings::AppSettings;
use crate::app_settings::KeyboardModelChoice;
use crate::transcription::models::TranscriptionSelection;

const DEBOUNCE: Duration = Duration::from_millis(300);

/// What a foreground read found. Hex's own window and a moment with no
/// frontmost application name no mode, so the switcher keeps what it last
/// matched and neither schedules nor cancels a switch.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum Observation<'a> {
    /// An application other than Hex is in front.
    App(&'a str),
    /// Hex's own window is in front, or the system named no application.
    Ignored,
}

impl<'a> Observation<'a> {
    /// A frontmost read: `None` is Hex's own window or no application at all.
    pub(crate) fn frontmost(application: Option<&'a str>) -> Self {
        application.map_or(Self::Ignored, Self::App)
    }
}

/// The debounced policy. The host feeds the choice of the mode matching the
/// foreground app; no choice means "don't switch".
pub(crate) struct Switcher {
    enabled: bool,
    /// The observed choice; unknown while switching is off, so turning it on
    /// waits for the host's first observation instead of a stale mode.
    current: Option<KeyboardModelChoice>,
    due: Option<Instant>,
}

impl Switcher {
    /// `current` is the choice of the mode in front at launch, recorded so a
    /// launch never switches the model.
    pub(crate) fn new(enabled: bool, current: Option<KeyboardModelChoice>) -> Self {
        Self {
            current: current.filter(|_| enabled),
            enabled,
            due: None,
        }
    }

    /// Feeds one foreground read. An ignored read is Hex's own window or no
    /// application at all: it keeps the last match and its pending switch
    /// exactly as they were. Returns true on a real change, so the owner can
    /// cancel obsolete work.
    pub(crate) fn observe(
        &mut self,
        observation: Observation<'_>,
        mode: impl FnOnce(&str) -> Option<KeyboardModelChoice>,
        now: Instant,
    ) -> bool {
        match observation {
            Observation::App(application) => self.choice_changed(mode(application), now),
            Observation::Ignored => false,
        }
    }

    fn choice_changed(&mut self, choice: Option<KeyboardModelChoice>, now: Instant) -> bool {
        if !self.enabled || self.current == choice {
            return false;
        }
        self.current = choice;
        self.due = self.current.as_ref().map(|_| now + DEBOUNCE);
        true
    }

    /// The choice to switch to once its debounce passes, unless it already
    /// names `selection`. A mode that matches the current model and language
    /// is a no-op. The host never calls this while a dictation records, so a
    /// due switch waits for idle.
    pub(crate) fn take_due(
        &mut self,
        now: Instant,
        selection: &TranscriptionSelection,
    ) -> Option<KeyboardModelChoice> {
        if now < self.due? {
            return None;
        }
        self.due = None;
        let choice = self.current.clone()?;
        (choice.model != selection.model || choice.language != selection.language).then_some(choice)
    }

    /// A manual model choice cancels a pending switch; only another mode
    /// change can schedule one again.
    pub(crate) fn manual_choice(&mut self) {
        self.due = None;
    }

    /// Turning the lab off forgets the mode and cancels its switch. Turning
    /// it on waits for the host's next observation. Returns true when the
    /// owner must supersede an automatic preparation.
    pub(crate) fn configure(&mut self, enabled: bool) -> bool {
        if self.enabled == enabled {
            return false;
        }
        self.enabled = enabled;
        self.due = None;
        if !enabled {
            self.current = None;
        }
        true
    }
}

/// The speech choice of the mode matching `application`.
///
/// Matching is by application on both macOS and Windows: the mode's website
/// rules are ignored, so a mode chosen only by website never matches. macOS
/// builds the same dictation profiles processing uses, and Windows uses
/// `dictation::modes`, each with no browser host.
#[cfg(any(target_os = "macos", target_os = "windows"))]
pub(crate) fn mode_choice(
    settings: &AppSettings,
    application: &str,
) -> Option<KeyboardModelChoice> {
    #[cfg(target_os = "macos")]
    {
        crate::dictation::config::speech_for(
            &settings.dictation_processing,
            &crate::dictation::context::ContextSnapshot {
                application: Some(application.to_owned()),
                ..Default::default()
            },
        )
    }
    #[cfg(not(target_os = "macos"))]
    {
        crate::dictation::modes::speech_for(&settings.dictation_processing, Some(application), None)
            .cloned()
    }
}

/// The choice of the mode in front at launch, recorded so launching never
/// switches the model. Hex's own window names no mode.
#[cfg(any(target_os = "macos", target_os = "windows"))]
pub(crate) fn startup_choice(settings: &AppSettings) -> Option<KeyboardModelChoice> {
    #[cfg(target_os = "macos")]
    let application = crate::dictation::context::frontmost_other_application();
    #[cfg(target_os = "windows")]
    let application = crate::windows::app::foreground_other_process_stem();
    mode_choice(settings, application.as_deref()?)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::transcription::models::TranscriptionModelId;

    fn choice(model: TranscriptionModelId, language: &str) -> KeyboardModelChoice {
        KeyboardModelChoice {
            model,
            language: language.into(),
        }
    }

    fn spanish() -> KeyboardModelChoice {
        choice(TranscriptionModelId::ParakeetV3, "es")
    }

    fn selection(model: TranscriptionModelId, language: &str) -> TranscriptionSelection {
        TranscriptionSelection {
            model,
            language: language.into(),
        }
    }

    #[test]
    fn the_startup_observation_does_not_switch() {
        let now = Instant::now();
        let mut switcher = Switcher::new(true, Some(spanish()));
        assert_eq!(
            switcher.take_due(
                now + Duration::from_secs(1),
                &selection(TranscriptionModelId::ParakeetUnifiedEnglish, "en")
            ),
            None
        );
        // The same mode seen again is not a change either.
        assert!(!switcher.choice_changed(Some(spanish()), now));
        assert_eq!(
            switcher.take_due(
                now + Duration::from_secs(2),
                &selection(TranscriptionModelId::ParakeetUnifiedEnglish, "en")
            ),
            None
        );
    }

    #[test]
    fn mode_changes_are_debounced_and_duplicates_ignored() {
        let now = Instant::now();
        let mut switcher = Switcher::new(true, None);
        assert!(switcher.choice_changed(Some(spanish()), now));
        assert!(!switcher.choice_changed(Some(spanish()), now + Duration::from_millis(100)));
        let current = selection(TranscriptionModelId::ParakeetUnifiedEnglish, "en");
        assert_eq!(
            switcher.take_due(now + Duration::from_millis(299), &current),
            None
        );
        assert_eq!(
            switcher.take_due(now + Duration::from_millis(300), &current),
            Some(spanish())
        );
        assert_eq!(
            switcher.take_due(now + Duration::from_secs(1), &current),
            None
        );
    }

    #[test]
    fn no_mode_or_no_choice_never_switches() {
        let now = Instant::now();
        let mut switcher = Switcher::new(true, Some(spanish()));
        let current = selection(TranscriptionModelId::ParakeetUnifiedEnglish, "en");
        assert!(switcher.choice_changed(None, now));
        assert_eq!(
            switcher.take_due(now + Duration::from_secs(1), &current),
            None
        );
    }

    #[test]
    fn a_choice_equal_to_the_current_selection_is_a_no_op() {
        let now = Instant::now();
        let mut switcher = Switcher::new(true, None);
        let current = selection(TranscriptionModelId::ParakeetV3, "es");
        assert!(switcher.choice_changed(Some(spanish()), now));
        assert_eq!(switcher.take_due(now + DEBOUNCE, &current), None);
        assert_eq!(
            switcher.take_due(now + Duration::from_secs(1), &current),
            None
        );
    }

    #[test]
    fn a_language_only_change_switches_the_language() {
        let now = Instant::now();
        let mut switcher = Switcher::new(true, None);
        let current = selection(TranscriptionModelId::ParakeetV3, "en");
        switcher.choice_changed(Some(spanish()), now);
        assert_eq!(switcher.take_due(now + DEBOUNCE, &current), Some(spanish()));
    }

    #[test]
    fn a_manual_choice_cancels_the_pending_switch_until_another_mode_change() {
        let now = Instant::now();
        let mut switcher = Switcher::new(true, None);
        let current = selection(TranscriptionModelId::ParakeetUnifiedEnglish, "en");
        switcher.choice_changed(Some(spanish()), now);
        switcher.manual_choice();
        assert_eq!(
            switcher.take_due(now + Duration::from_secs(1), &current),
            None
        );
        // The same mode observed again must not undo the manual choice.
        assert!(!switcher.choice_changed(Some(spanish()), now));
        assert_eq!(
            switcher.take_due(now + Duration::from_secs(2), &current),
            None
        );
    }

    #[test]
    fn cycling_past_modes_only_switches_to_the_last_choice() {
        let now = Instant::now();
        let french = choice(TranscriptionModelId::WhisperLargeV3Turbo, "fr");
        let mut switcher = Switcher::new(true, None);
        let current = selection(TranscriptionModelId::ParakeetUnifiedEnglish, "en");
        switcher.choice_changed(Some(french.clone()), now);
        switcher.choice_changed(Some(spanish()), now + Duration::from_millis(100));
        assert_eq!(
            switcher.take_due(now + Duration::from_millis(300), &current),
            None
        );
        assert_eq!(
            switcher.take_due(now + Duration::from_millis(400), &current),
            Some(spanish())
        );
        switcher.choice_changed(Some(french), now + Duration::from_secs(1));
        switcher.choice_changed(None, now + Duration::from_millis(1100));
        assert_eq!(
            switcher.take_due(now + Duration::from_secs(2), &current),
            None
        );
    }

    #[test]
    fn turning_off_the_feature_cancels_a_debounced_switch_and_forgets_the_mode() {
        let now = Instant::now();
        let mut switcher = Switcher::new(true, None);
        let current = selection(TranscriptionModelId::ParakeetUnifiedEnglish, "en");
        switcher.choice_changed(Some(spanish()), now);
        assert!(switcher.configure(false));
        assert_eq!(
            switcher.take_due(now + Duration::from_secs(1), &current),
            None
        );
        assert!(!switcher.choice_changed(Some(spanish()), now));
        assert_eq!(
            switcher.take_due(now + Duration::from_secs(2), &current),
            None
        );
        // Turning it back on waits for the host's next observation.
        assert!(switcher.configure(true));
        assert_eq!(
            switcher.take_due(now + Duration::from_secs(3), &current),
            None
        );
        assert!(switcher.choice_changed(Some(spanish()), now));
        assert_eq!(
            switcher.take_due(now + Duration::from_secs(4), &current),
            Some(spanish())
        );
    }

    /// Feeds an app named "Notes" (Spanish) or any other app (no choice).
    fn observe(switcher: &mut Switcher, observation: Observation<'_>, now: Instant) -> bool {
        switcher.observe(
            observation,
            |application| (application == "Notes").then(spanish),
            now,
        )
    }

    #[test]
    fn hexs_own_window_keeps_the_last_apps_choice_and_its_pending_switch() {
        let now = Instant::now();
        let mut switcher = Switcher::new(true, None);
        let current = selection(TranscriptionModelId::ParakeetUnifiedEnglish, "en");
        assert!(observe(&mut switcher, Observation::App("Notes"), now));
        assert_eq!(switcher.take_due(now + DEBOUNCE, &current), Some(spanish()));
        // Hex's own window names no mode: the Notes choice stays, nothing
        // new is due, and the mode is never even looked up.
        assert!(!switcher.observe(
            Observation::Ignored,
            |_| panic!("Hex's own window must not name a mode"),
            now + Duration::from_secs(1),
        ));
        assert_eq!(
            switcher.take_due(now + Duration::from_secs(2), &current),
            None
        );
        // Coming back to Notes is not a change either.
        assert!(!observe(
            &mut switcher,
            Observation::App("Notes"),
            now + Duration::from_secs(3),
        ));
        assert_eq!(
            switcher.take_due(now + Duration::from_secs(4), &current),
            None
        );
    }

    #[test]
    fn hex_in_front_during_the_debounce_does_not_cancel_the_switch() {
        let now = Instant::now();
        let mut switcher = Switcher::new(true, None);
        let current = selection(TranscriptionModelId::ParakeetUnifiedEnglish, "en");
        assert!(observe(&mut switcher, Observation::App("Notes"), now));
        assert!(!observe(
            &mut switcher,
            Observation::Ignored,
            now + Duration::from_millis(100),
        ));
        assert_eq!(switcher.take_due(now + DEBOUNCE, &current), Some(spanish()));
    }

    #[test]
    fn no_application_in_front_keeps_the_last_apps_choice() {
        let now = Instant::now();
        let mut switcher = Switcher::new(true, Some(spanish()));
        let current = selection(TranscriptionModelId::ParakeetUnifiedEnglish, "en");
        assert_eq!(Observation::frontmost(None), Observation::Ignored);
        assert!(!observe(&mut switcher, Observation::frontmost(None), now));
        assert_eq!(
            switcher.take_due(now + Duration::from_secs(1), &current),
            None
        );
        // Only a real app read names a mode; the same one seen again after
        // the blank moment is not a change.
        assert_eq!(
            Observation::frontmost(Some("Notes")),
            Observation::App("Notes")
        );
        assert!(!observe(
            &mut switcher,
            Observation::frontmost(Some("Notes")),
            now + Duration::from_secs(2),
        ));
        assert_eq!(
            switcher.take_due(now + Duration::from_secs(3), &current),
            None
        );
    }
}
