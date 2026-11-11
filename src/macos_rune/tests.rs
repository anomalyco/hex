//! Runtime request/scheduling regressions; no native model loading or settings writes.

use super::runtime::Runtime;
use super::*;
use crate::app_settings::{DictationMode, KeyboardModelChoice, KeyboardModelSwitching};
// `Request` moved beside `ShortcutTarget` into `macos_rune/request.rs`, so the
// runtime's own imports no longer name the model IDs and selections these
// tests build.
use crate::transcription::models::{TranscriptionModelId, TranscriptionSelection};
use crate::transcription::preparation::TranscriptionPreparation;

const WAIT: Duration = Duration::from_secs(3);

/// The pump ticks every frame only while the HUD animates; otherwise it
/// idles and senders wake it (#1378).
#[test]
fn the_pump_idles_unless_the_hud_animates() {
    assert!(pump_interval(true) < 0.02);
    assert!(pump_interval(false) >= 0.2);
}

/// ⌘Tab back to Hex opens its closed window (#1298), but never for a
/// menu-bar-only Hex, an overlay that took the keyboard, or an open window.
#[test]
fn switching_to_hex_opens_its_closed_window() {
    assert!(reopen_on_activation(true, false, false));
    assert!(!reopen_on_activation(false, false, false));
    assert!(!reopen_on_activation(true, true, false));
    assert!(!reopen_on_activation(true, false, true));
}

/// Setup or a missing menu-bar item always opens the window; otherwise a
/// shown Dock icon opens it unless macOS started Hex at login (#1321), and a
/// hidden one never does. `hex app --hidden` opens it only without the
/// menu-bar item.
#[test]
fn login_item_launch_starts_without_the_window() {
    for setup_ready in [false, true] {
        for onboarding_completed in [false, true] {
            for status_item_available in [false, true] {
                for show_dock_icon in [false, true] {
                    let ready = setup_ready && onboarding_completed && status_item_available;
                    let launch = LaunchWindow::decide(
                        false,
                        show_dock_icon,
                        setup_ready,
                        onboarding_completed,
                        status_item_available,
                    );
                    assert_eq!(launch.opens(true), !ready, "{launch:?} at login");
                    assert_eq!(
                        launch.opens(false),
                        !ready || show_dock_icon,
                        "{launch:?} opened by hand"
                    );
                    let hidden = LaunchWindow::decide(
                        true,
                        show_dock_icon,
                        setup_ready,
                        onboarding_completed,
                        status_item_available,
                    );
                    assert_eq!(
                        hidden.opens(false),
                        !status_item_available,
                        "{hidden:?} hidden"
                    );
                }
            }
        }
    }
}

/// Hold each preparation until released, observing its exact request and cancellation.
struct Fixture {
    runtime: Runtime,
    started: Receiver<(TranscriptionSelection, bool)>,
    release: Sender<()>,
    finished: Receiver<bool>,
}

impl Fixture {
    fn new(settings: AppSettings) -> Self {
        let (start, started) = mpsc::channel();
        let (release, releases) = mpsc::channel();
        let releases = Mutex::new(releases);
        let (finish, finished) = mpsc::channel();
        let preparation = TranscriptionPreparation::with_prepare(Arc::new(
            move |selection, installed_only, cancelled, _| {
                start.send((selection.clone(), installed_only)).unwrap();
                releases
                    .lock()
                    .unwrap()
                    .recv_timeout(WAIT)
                    .map_err(|error| error.to_string())?;
                let _ = finish.send(cancelled.load(Ordering::Acquire));
                Ok(())
            },
        ));
        Self {
            runtime: Runtime {
                keyboard_switcher: crate::transcription::keyboard_model_switching::Switcher::new(
                    settings.keyboard_model_switching.clone(),
                    Some("ABC".into()),
                ),
                app_switcher: crate::transcription::app_model_switching::Switcher::new(
                    settings.labs.app_model_switching,
                    None,
                ),
                settings,
                preparation,
                automatic_preparation: false,
                recognition_start: None,
                listener_failure: Default::default(),
                event_reader: crate::desktop::events::EventReader::open(""),
                activity: crate::desktop::activity::DesktopActivity::default(),
                last_completed_at: None,
                started_at: 0,
            },
            started,
            release,
            finished,
        }
    }

    fn source(&mut self, source: &str) {
        self.runtime
            .handle(Request::InputSourceChanged(Some(source.into())));
    }

    fn due(&mut self) {
        self.runtime
            .tick_at(Instant::now() + Duration::from_secs(1));
    }

    /// Feeds one foreground read to the app switcher.
    fn app(&mut self, application: Option<&str>, due: bool) {
        let now = Instant::now()
            + if due {
                Duration::from_secs(1)
            } else {
                Duration::ZERO
            };
        self.runtime.follow_app(
            crate::transcription::app_model_switching::Observation::frontmost(application),
            now,
        );
    }

    fn started(&self) -> (TranscriptionSelection, bool) {
        self.started
            .recv_timeout(WAIT)
            .expect("preparation did not start")
    }

    fn release(&self) {
        self.release.send(()).unwrap();
    }

    fn start_manual(&mut self) {
        self.runtime.handle(Request::ChooseModel {
            model: TranscriptionModelId::WhisperLargeV3Turbo,
            language: "en".into(),
        });
        let (selection, installed_only) = self.started();
        assert_eq!(selection.model, TranscriptionModelId::WhisperLargeV3Turbo);
        assert!(!installed_only);
    }

    fn assert_manual_pending(&self) {
        assert_eq!(
            self.runtime.preparation.status().model,
            Some(TranscriptionModelId::WhisperLargeV3Turbo)
        );
    }

    fn complete_manual(&mut self) {
        self.release();
        let deadline = Instant::now() + WAIT;
        loop {
            if let Some(result) = self.runtime.preparation.poll() {
                assert_eq!(
                    result.unwrap().model,
                    TranscriptionModelId::WhisperLargeV3Turbo
                );
                break;
            }
            assert!(
                Instant::now() < deadline,
                "manual preparation was cancelled"
            );
            thread::sleep(Duration::from_millis(1));
        }
    }

    fn next_after_cancellation(&mut self) -> (TranscriptionSelection, bool) {
        let deadline = Instant::now() + WAIT;
        loop {
            assert!(
                self.runtime.preparation.poll().is_none(),
                "obsolete choice escaped cancellation"
            );
            if let Ok(choice) = self.started.try_recv() {
                return choice;
            }
            assert!(
                Instant::now() < deadline,
                "replacement preparation did not start"
            );
            thread::sleep(Duration::from_millis(1));
        }
    }
}

/// "Spanish" mapped to `model` in the language it would get from the picker.
fn mapped_settings(model: TranscriptionModelId) -> AppSettings {
    let settings = AppSettings::default();
    let language = settings.transcription_for_model(model).language;
    mapped_settings_in(model, &language)
}

fn mapped_settings_in(model: TranscriptionModelId, language: &str) -> AppSettings {
    AppSettings {
        keyboard_model_switching: KeyboardModelSwitching {
            enabled: true,
            mappings: [(
                "Spanish".into(),
                KeyboardModelChoice {
                    model,
                    language: language.into(),
                },
            )]
            .into(),
        },
        ..Default::default()
    }
}

#[test]
fn a_mapping_to_the_current_model_in_another_language_switches_the_language() {
    let mut settings = mapped_settings_in(TranscriptionModelId::ParakeetV3, "es");
    settings.transcription = TranscriptionSelection {
        model: TranscriptionModelId::ParakeetV3,
        language: "en".into(),
    };
    let mut fixture = Fixture::new(settings);
    fixture.source("Spanish");
    fixture.due();
    let (selection, installed_only) = fixture.started();
    assert_eq!(
        (selection.model, selection.language.as_str(), installed_only),
        (TranscriptionModelId::ParakeetV3, "es", true)
    );
    fixture.release();
}

#[test]
fn a_modes_speech_choice_switches_when_its_app_comes_to_the_front() {
    let mut settings = AppSettings::default();
    settings.labs.app_model_switching = true;
    settings.dictation_processing.modes = vec![DictationMode {
        name: "Spanish".into(),
        applications: vec!["Notes".into()],
        speech: Some(KeyboardModelChoice {
            model: TranscriptionModelId::ParakeetV3,
            language: "es".into(),
        }),
        ..DictationMode::default()
    }];
    let mut fixture = Fixture::new(settings);
    fixture.app(Some("Notes"), false);
    assert!(
        fixture.started.try_recv().is_err(),
        "the switch started before its debounce"
    );
    fixture.app(Some("Notes"), true);
    let (selection, installed_only) = fixture.started();
    assert_eq!(
        (selection.model, selection.language.as_str(), installed_only),
        (TranscriptionModelId::ParakeetV3, "es", true)
    );
    fixture.release();
}

#[test]
fn hexs_own_window_applies_nothing_and_an_edit_waits_for_another_app() {
    let mut settings = AppSettings::default();
    settings.labs.app_model_switching = true;
    settings.dictation_processing.default_mode.speech = Some(KeyboardModelChoice {
        model: TranscriptionModelId::ParakeetV3,
        language: "es".into(),
    });
    let mut fixture = Fixture::new(settings);
    // Hex's own window names no mode, even though Global matches every app.
    fixture.app(None, true);
    assert!(
        fixture.started.try_recv().is_err(),
        "Hex's own window switched the model"
    );
    // Editing Global while Hex is in front changes nothing yet.
    fixture
        .runtime
        .settings
        .dictation_processing
        .default_mode
        .speech = Some(KeyboardModelChoice {
        model: TranscriptionModelId::WhisperLargeV3Turbo,
        language: "fr".into(),
    });
    fixture.app(None, true);
    assert!(fixture.started.try_recv().is_err());
    // The next app in front applies the edited choice after its debounce.
    fixture.app(Some("Finder"), false);
    assert!(
        fixture.started.try_recv().is_err(),
        "the switch started before its debounce"
    );
    fixture.app(Some("Finder"), true);
    let (selection, installed_only) = fixture.started();
    assert_eq!(
        (selection.model, selection.language.as_str(), installed_only),
        (TranscriptionModelId::WhisperLargeV3Turbo, "fr", true)
    );
    fixture.release();
}

#[test]
fn hex_model_use_with_the_current_model_and_language_loads_nothing() {
    let mut fixture = Fixture::new(AppSettings::default());
    let same = fixture.runtime.settings.transcription.clone();
    fixture.runtime.handle(Request::UseInstalledSelection(same));
    assert_eq!(fixture.runtime.preparation.status().model, None);
}

#[test]
fn input_source_choices_use_installed_only_preparation_and_manual_choices_supersede_them() {
    let settings = mapped_settings(TranscriptionModelId::ParakeetV3);
    let expected = settings.transcription_for_model(TranscriptionModelId::ParakeetV3);
    let mut fixture = Fixture::new(settings);
    fixture.source("ABC");
    fixture.source("Spanish");
    fixture.due();
    assert_eq!(fixture.started(), (expected, true));
    let manual = TranscriptionSelection {
        model: TranscriptionModelId::WhisperLargeV3Turbo,
        language: "de".into(),
    };
    fixture
        .runtime
        .handle(Request::UseInstalledSelection(manual.clone()));
    fixture.release();
    assert_eq!(fixture.next_after_cancellation(), (manual, true));
    fixture.release();
}

#[test]
fn choosing_the_current_model_cancels_a_debounced_automatic_choice_without_loading() {
    let mut fixture = Fixture::new(mapped_settings(TranscriptionModelId::ParakeetV3));
    fixture.source("Spanish");
    fixture.runtime.handle(Request::UseInstalledSelection(
        fixture.runtime.settings.transcription.clone(),
    ));
    assert_eq!(fixture.runtime.preparation.status().model, None);
    assert_eq!(
        fixture
            .runtime
            .keyboard_switcher
            .take_due(Instant::now() + Duration::from_secs(1)),
        None
    );
}

#[test]
fn choosing_the_current_model_keeps_manual_preparation_when_switching_is_disabled() {
    let settings = AppSettings::default();
    assert!(!settings.keyboard_model_switching.enabled);
    let mut fixture = Fixture::new(settings);
    fixture.start_manual();
    fixture.runtime.handle(Request::UseInstalledSelection(
        fixture.runtime.settings.transcription.clone(),
    ));
    fixture.assert_manual_pending();
    fixture.complete_manual();
}

#[test]
fn cycling_through_a_mapped_source_keeps_manual_preparation_until_a_switch_is_due() {
    let mut fixture = Fixture::new(mapped_settings(TranscriptionModelId::ParakeetV3));
    fixture.start_manual();
    fixture.source("Spanish");
    fixture.assert_manual_pending();
    fixture.source("ABC");
    fixture.due();
    fixture.assert_manual_pending();
    fixture.complete_manual();
    assert!(
        fixture.started.try_recv().is_err(),
        "pass-through source must not prepare a model"
    );
}

#[test]
fn a_due_source_choice_supersedes_manual_preparation_even_for_the_current_model() {
    for mapped in [
        TranscriptionModelId::ParakeetV3,
        TranscriptionModelId::ParakeetUnifiedEnglish,
    ] {
        let mut settings = mapped_settings(mapped);
        settings.transcription = TranscriptionSelection {
            model: TranscriptionModelId::ParakeetUnifiedEnglish,
            language: "en".into(),
        };
        let mut fixture = Fixture::new(settings);
        fixture.start_manual();
        fixture.source("Spanish");
        fixture.assert_manual_pending();
        fixture.due();
        let already_current = mapped == fixture.runtime.settings.transcription.model;
        assert_eq!(
            fixture.runtime.preparation.status().model,
            (!already_current).then_some(mapped)
        );
        fixture.release();
        assert!(
            fixture.finished.recv_timeout(WAIT).unwrap(),
            "obsolete manual preparation wasn't cancelled"
        );
        if !already_current {
            let (selection, installed_only) = fixture.next_after_cancellation();
            assert_eq!(selection.model, mapped);
            assert!(installed_only);
            fixture.release();
        }
    }
}
