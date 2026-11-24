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
const RETRY_BASE: Duration = Duration::from_secs(2);
const RETRY_MAX: Duration = Duration::from_secs(60);

fn retry_backoff(failures: u32) -> Duration {
    RETRY_BASE
        .saturating_mul(1_u32 << failures.min(5))
        .min(RETRY_MAX)
}

/// What a foreground read found. Hex's own window and a moment with no
/// frontmost application name no mode, so the switcher keeps what it last
/// matched and neither schedules nor cancels a switch.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum Observation<'a> {
    /// An application other than Hex is in front: its own name, and on
    /// macOS the name Finder shows for its bundle when that differs.
    App(&'a str, Option<&'a str>),
    /// Hex's own window is in front, or the system named no application.
    Ignored,
}

impl<'a> Observation<'a> {
    /// A frontmost read: `None` is Hex's own window or no application at all.
    pub(crate) fn frontmost(application: Option<&'a str>, bundle_name: Option<&'a str>) -> Self {
        application.map_or(Self::Ignored, |application| {
            Self::App(application, bundle_name)
        })
    }
}

#[derive(Debug)]
enum State {
    /// Labs › Switch model with app is off.
    Off,
    /// Enabled, currently on an app with no mode choice (or waiting for the
    /// first observation after being enabled).
    Idle,
    /// A mode choice is waiting out its debounce (or retry backoff after a
    /// failed automatic preparation).
    Pending {
        choice: KeyboardModelChoice,
        due: Instant,
        failures: u32,
    },
    /// `take_due` handed `choice` to the host; waiting for `commit` or `fail`.
    Applying {
        choice: KeyboardModelChoice,
        failures: u32,
    },
    /// `choice` is active (at launch, already selected, committed, or manually
    /// overridden). Nothing runs until the observed choice changes.
    Settled(KeyboardModelChoice),
}

/// The debounced policy. The host feeds the choice of the mode matching the
/// foreground app; no choice means "don't switch".
pub(crate) struct Switcher {
    state: State,
}

impl Switcher {
    /// `current` is the choice of the mode in front at launch, recorded so a
    /// launch never switches the model.
    pub(crate) fn new(enabled: bool, current: Option<KeyboardModelChoice>) -> Self {
        let state = if !enabled {
            State::Off
        } else if let Some(choice) = current {
            State::Settled(choice)
        } else {
            State::Idle
        };
        Self { state }
    }

    fn observed_choice(&self) -> Option<&KeyboardModelChoice> {
        match &self.state {
            State::Off | State::Idle => None,
            State::Pending { choice, .. }
            | State::Applying { choice, .. }
            | State::Settled(choice) => Some(choice),
        }
    }

    /// Feeds one foreground read. An ignored read is Hex's own window or no
    /// application at all: it keeps the last match and its pending switch
    /// exactly as they were. Returns true on a real change, so the owner can
    /// cancel obsolete work.
    pub(crate) fn observe(
        &mut self,
        observation: Observation<'_>,
        mode: impl FnOnce(&str, Option<&str>) -> Option<KeyboardModelChoice>,
        now: Instant,
    ) -> bool {
        match observation {
            Observation::App(application, bundle_name) => {
                self.choice_changed(mode(application, bundle_name), now)
            }
            Observation::Ignored => false,
        }
    }

    fn choice_changed(&mut self, choice: Option<KeyboardModelChoice>, now: Instant) -> bool {
        if matches!(self.state, State::Off) || self.observed_choice() == choice.as_ref() {
            return false;
        }
        self.state = match choice {
            Some(choice) => State::Pending {
                choice,
                due: now + DEBOUNCE,
                failures: 0,
            },
            None => State::Idle,
        };
        true
    }

    /// The choice to switch to once its debounce (or retry backoff) passes,
    /// unless it already names `selection`. A mode that matches the current
    /// model and language settles immediately as a no-op. The host never calls
    /// this while a dictation records, so a due switch waits for idle.
    pub(crate) fn take_due(
        &mut self,
        now: Instant,
        selection: &TranscriptionSelection,
    ) -> Option<KeyboardModelChoice> {
        let State::Pending {
            choice,
            due,
            failures,
        } = &self.state
        else {
            return None;
        };
        if now < *due {
            return None;
        }
        let choice = choice.clone();
        let failures = *failures;
        if choice.model == selection.model && choice.language == selection.language {
            self.state = State::Settled(choice);
            return None;
        }
        self.state = State::Applying {
            choice: choice.clone(),
            failures,
        };
        Some(choice)
    }

    /// Records that the automatic preparation returned by `take_due` committed.
    pub(crate) fn commit(&mut self) {
        if let State::Applying { choice, .. } = &self.state {
            self.state = State::Settled(choice.clone());
        }
    }

    /// Records that the automatic preparation returned by `take_due` failed,
    /// scheduling a retry with bounded exponential backoff so transient
    /// failures or newly installed models recover without leaving the app.
    pub(crate) fn fail(&mut self, now: Instant) {
        if let State::Applying { choice, failures } = &self.state {
            let delay = retry_backoff(*failures);
            self.state = State::Pending {
                choice: choice.clone(),
                due: now + delay,
                failures: failures.saturating_add(1),
            };
        }
    }

    /// Drops an in-flight automatic preparation that was superseded before
    /// finishing, without disturbing a newer pending switch.
    pub(crate) fn cancel_applying(&mut self) {
        if let State::Applying { choice, .. } = &self.state {
            self.state = State::Settled(choice.clone());
        }
    }

    /// A manual model choice cancels a pending or retrying switch; only
    /// another mode change can schedule one again.
    pub(crate) fn manual_choice(&mut self) {
        match &self.state {
            State::Pending { choice, .. } | State::Applying { choice, .. } => {
                self.state = State::Settled(choice.clone());
            }
            State::Off | State::Idle | State::Settled(_) => {}
        }
    }

    /// Turning the lab off forgets the mode and cancels its switch. Turning
    /// it on waits for the host's next observation. Returns true when the
    /// owner must supersede an automatic preparation.
    pub(crate) fn configure(&mut self, enabled: bool) -> bool {
        let was_enabled = !matches!(self.state, State::Off);
        if was_enabled == enabled {
            return false;
        }
        self.state = if enabled { State::Idle } else { State::Off };
        true
    }
}

/// The speech choice of the mode matching `application`, or on macOS its
/// bundle's Finder name, which is what the app picker saves.
///
/// Matching is by application on both macOS and Windows: the mode's website
/// rules are ignored, so a mode chosen only by website never matches. macOS
/// builds the same dictation profiles processing uses, and Windows uses
/// `dictation::modes`, each with no browser host.
#[cfg(any(target_os = "macos", target_os = "windows"))]
pub(crate) fn mode_choice(
    settings: &AppSettings,
    application: &str,
    bundle_name: Option<&str>,
) -> Option<KeyboardModelChoice> {
    #[cfg(target_os = "macos")]
    {
        crate::dictation::config::speech_for(
            &settings.dictation_processing,
            &crate::dictation::context::ContextSnapshot {
                application: Some(application.to_owned()),
                bundle_name: bundle_name.map(str::to_owned),
                ..Default::default()
            },
        )
    }
    #[cfg(not(target_os = "macos"))]
    {
        let _ = bundle_name;
        crate::dictation::modes::speech_for(&settings.dictation_processing, Some(application), None)
    }
}

/// The choice of the mode in front at launch, recorded so launching never
/// switches the model. Hex's own window names no mode.
#[cfg(any(target_os = "macos", target_os = "windows"))]
pub(crate) fn startup_choice(settings: &AppSettings) -> Option<KeyboardModelChoice> {
    #[cfg(target_os = "macos")]
    let (application, bundle_name) = crate::dictation::context::frontmost_other_application()?;
    #[cfg(target_os = "windows")]
    let (application, bundle_name) = (
        crate::windows::app::foreground_other_process_stem()?,
        None::<String>,
    );
    mode_choice(settings, &application, bundle_name.as_deref())
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
            |application, _| (application == "Notes").then(spanish),
            now,
        )
    }

    /// The picker saves Finder's name (`Rex Beta.app`), while the running
    /// app calls itself `Rex`: the mode still switches the model.
    #[cfg(target_os = "macos")]
    #[test]
    fn a_mode_picked_under_its_finder_name_switches() {
        let mut settings = AppSettings::default();
        settings.dictation_processing.modes = vec![crate::app_settings::DictationMode {
            name: "Spanish".into(),
            applications: vec!["Rex Beta".into()],
            speech: Some(spanish()),
            ..Default::default()
        }];
        let mode = |application: &str, bundle_name: Option<&str>| {
            mode_choice(&settings, application, bundle_name)
        };
        let now = Instant::now();
        let mut switcher = Switcher::new(true, None);
        let current = selection(TranscriptionModelId::ParakeetUnifiedEnglish, "en");
        assert!(!switcher.observe(Observation::frontmost(Some("Rex"), None), mode, now));
        assert!(switcher.observe(
            Observation::frontmost(Some("Rex"), Some("Rex Beta")),
            mode,
            now,
        ));
        assert_eq!(switcher.take_due(now + DEBOUNCE, &current), Some(spanish()));
    }

    #[test]
    fn hexs_own_window_keeps_the_last_apps_choice_and_its_pending_switch() {
        let now = Instant::now();
        let mut switcher = Switcher::new(true, None);
        let current = selection(TranscriptionModelId::ParakeetUnifiedEnglish, "en");
        assert!(observe(&mut switcher, Observation::App("Notes", None), now));
        assert_eq!(switcher.take_due(now + DEBOUNCE, &current), Some(spanish()));
        // Hex's own window names no mode: the Notes choice stays, nothing
        // new is due, and the mode is never even looked up.
        assert!(!switcher.observe(
            Observation::Ignored,
            |_, _| panic!("Hex's own window must not name a mode"),
            now + Duration::from_secs(1),
        ));
        assert_eq!(
            switcher.take_due(now + Duration::from_secs(2), &current),
            None
        );
        // Coming back to Notes is not a change either.
        assert!(!observe(
            &mut switcher,
            Observation::App("Notes", None),
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
        assert!(observe(&mut switcher, Observation::App("Notes", None), now));
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
        assert_eq!(Observation::frontmost(None, None), Observation::Ignored);
        assert!(!observe(
            &mut switcher,
            Observation::frontmost(None, None),
            now
        ));
        assert_eq!(
            switcher.take_due(now + Duration::from_secs(1), &current),
            None
        );
        // Only a real app read names a mode; the same one seen again after
        // the blank moment is not a change.
        assert_eq!(
            Observation::frontmost(Some("Notes"), None),
            Observation::App("Notes", None)
        );
        assert!(!observe(
            &mut switcher,
            Observation::frontmost(Some("Notes"), None),
            now + Duration::from_secs(2),
        ));
        assert_eq!(
            switcher.take_due(now + Duration::from_secs(3), &current),
            None
        );
    }

    #[test]
    fn a_failed_switch_retries_with_bounded_backoff_until_committed_or_overridden() {
        let now = Instant::now();
        let mut switcher = Switcher::new(true, None);
        let current = selection(TranscriptionModelId::ParakeetUnifiedEnglish, "en");
        assert!(observe(&mut switcher, Observation::App("Notes", None), now));
        assert_eq!(switcher.take_due(now + DEBOUNCE, &current), Some(spanish()));
        // While applying, another tick does not start a duplicate switch.
        assert_eq!(
            switcher.take_due(now + Duration::from_secs(5), &current),
            None
        );

        // First failure backs off by RETRY_BASE (2s), not immediately.
        let fail_1 = now + Duration::from_millis(500);
        switcher.fail(fail_1);
        assert_eq!(
            switcher.take_due(fail_1 + Duration::from_millis(1_999), &current),
            None
        );
        // Same app observed again does not reset or cancel the retry.
        assert!(!observe(
            &mut switcher,
            Observation::App("Notes", None),
            fail_1 + Duration::from_secs(1)
        ));
        assert_eq!(
            switcher.take_due(fail_1 + RETRY_BASE, &current),
            Some(spanish())
        );

        // Second failure doubles backoff (4s); committing settles it.
        let fail_2 = fail_1 + RETRY_BASE;
        switcher.fail(fail_2);
        assert_eq!(
            switcher.take_due(fail_2 + Duration::from_millis(3_999), &current),
            None
        );
        assert_eq!(
            switcher.take_due(fail_2 + Duration::from_secs(4), &current),
            Some(spanish())
        );
        switcher.commit();
        assert_eq!(
            switcher.take_due(fail_2 + Duration::from_secs(120), &current),
            None
        );

        // Backoff is capped at RETRY_MAX.
        assert_eq!(retry_backoff(0), Duration::from_secs(2));
        assert_eq!(retry_backoff(1), Duration::from_secs(4));
        assert_eq!(retry_backoff(5), RETRY_MAX);
        assert_eq!(retry_backoff(20), RETRY_MAX);
    }

    #[test]
    fn manual_choice_or_matching_selection_cancels_a_pending_retry() {
        let now = Instant::now();
        let mut switcher = Switcher::new(true, None);
        let current = selection(TranscriptionModelId::ParakeetUnifiedEnglish, "en");
        assert!(observe(&mut switcher, Observation::App("Notes", None), now));
        assert_eq!(switcher.take_due(now + DEBOUNCE, &current), Some(spanish()));
        switcher.fail(now + DEBOUNCE);
        switcher.manual_choice();
        assert_eq!(
            switcher.take_due(now + Duration::from_secs(10), &current),
            None
        );

        // Switching away and back schedules fresh; if the live selection already
        // matches when the retry fires, it settles with no switch.
        assert!(observe(
            &mut switcher,
            Observation::App("Safari", None),
            now + Duration::from_secs(11)
        ));
        let t = now + Duration::from_secs(12);
        assert!(observe(&mut switcher, Observation::App("Notes", None), t));
        assert_eq!(switcher.take_due(t + DEBOUNCE, &current), Some(spanish()));
        switcher.fail(t + DEBOUNCE);
        let already_spanish = selection(TranscriptionModelId::ParakeetV3, "es");
        assert_eq!(
            switcher.take_due(t + DEBOUNCE + RETRY_BASE, &already_spanish),
            None
        );
        assert_eq!(
            switcher.take_due(t + Duration::from_secs(120), &current),
            None
        );
    }
}
