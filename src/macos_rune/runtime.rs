//! The runtime thread: it applies the window's and the menu bar's requests,
//! prepares and commits the transcription model, follows the keyboard layout
//! switcher, and publishes what the window and the menu bar show.

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{Receiver, RecvTimeoutError, SyncSender};
use std::sync::{Arc, Mutex};
use std::time::Instant;

use crate::app_settings::AppSettings;
use crate::desktop::activity::{DesktopActivity, last_completed_at};
use crate::desktop::events::EventReader;
use crate::desktop::host::{
    DesktopListenerSnapshot, DesktopShortcuts, DesktopTranscriptionSnapshot, ListenerStatus,
};
use crate::transcription::models::TranscriptionSelection;
use crate::transcription::preparation::TranscriptionPreparation;

use super::{Published, RUNTIME_TICK, Request, Shared, UPDATER, lock, update_status};

#[allow(clippy::too_many_arguments)]
pub(super) fn run_runtime(
    settings: AppSettings,
    initial_input_source: Option<String>,
    requests: Receiver<Request>,
    shared: &Shared,
    shutdown: &AtomicBool,
    recognition_start: Option<SyncSender<()>>,
    listener_failure: Arc<Mutex<Option<String>>>,
    hud: crate::dictation::indicator::DictationIndicatorSender,
) {
    let mut event_reader = EventReader::open(shared.event_path().to_path_buf());
    let mut activity = DesktopActivity::default();
    activity.refresh(&mut event_reader);
    let last_completed_at = last_completed_at(event_reader.events().iter());
    let mut runtime = Runtime {
        keyboard_switcher: crate::transcription::keyboard_model_switching::Switcher::new(
            settings.keyboard_model_switching.clone(),
            initial_input_source,
        ),
        app_switcher: crate::transcription::app_model_switching::Switcher::new(
            settings.labs.app_model_switching,
            settings
                .labs
                .app_model_switching
                .then(|| crate::transcription::app_model_switching::startup_choice(&settings))
                .flatten(),
        ),
        automatic_preparation: None,
        pending_swap: None,
        hud: hud.clone(),
        settings,
        preparation: TranscriptionPreparation::default(),
        recognition_start,
        listener_failure,
        event_reader,
        activity,
        last_completed_at,
        started_at: shared.started_at,
        record: shared.record.clone(),
    };
    let mut settings_checked = Instant::now();
    loop {
        let mut next = match requests.recv_timeout(RUNTIME_TICK) {
            Ok(request) => Some(request),
            Err(RecvTimeoutError::Timeout) => None,
            Err(RecvTimeoutError::Disconnected) => break,
        };
        while let Some(request) = next {
            runtime.handle(request);
            next = requests.try_recv().ok();
        }
        if shutdown.load(Ordering::Relaxed) {
            break;
        }
        if settings_checked.elapsed() >= std::time::Duration::from_secs(2) {
            settings_checked = Instant::now();
            runtime.reload_settings();
        }
        runtime.tick();
        runtime.refresh_activity();
        let published = Arc::new(runtime.publish());
        let changed = {
            let mut current = lock(&shared.published);
            let changed = current.as_deref() != Some(&published)
                || current.as_ref().is_some_and(|old| {
                    old.settings.source.revision != published.settings.source.revision
                });
            if changed {
                if current.as_ref().is_none_or(|old| {
                    old.settings.history_retention != published.settings.history_retention
                }) && let Some(history) = super::runtime().and_then(|runtime| runtime.history())
                    && let Err(error) = history.set_retention(published.settings.history_retention)
                {
                    tracing::warn!(%error, "could not apply history retention");
                }
                *current = Some(published);
            }
            changed
        };
        if changed {
            for wake in lock(&shared.wakers).iter() {
                wake();
            }
        }
    }
    runtime.preparation.cancel();
}

/// Two selections that load the same thing.
pub(super) fn same_model_and_language(
    left: &TranscriptionSelection,
    right: &TranscriptionSelection,
) -> bool {
    left.model == right.model && left.language == right.language
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) enum AutomaticSwitch {
    Keyboard,
    App,
    /// Labs › Model switch shortcut: the preparation swaps both selections.
    Swap,
}

pub(super) struct Runtime {
    pub(super) settings: AppSettings,
    pub(super) preparation: TranscriptionPreparation,
    pub(super) keyboard_switcher: crate::transcription::keyboard_model_switching::Switcher,
    /// Labs › Switch model with app: the foreground application's mode choice.
    pub(super) app_switcher: crate::transcription::app_model_switching::Switcher,
    pub(super) automatic_preparation: Option<AutomaticSwitch>,
    /// The selection a running swap preparation swaps back to.
    pub(super) pending_swap: Option<crate::app_settings::KeyboardModelChoice>,
    /// The HUD, for a switch that failed where the person asked for it.
    pub(super) hud: crate::dictation::indicator::DictationIndicatorSender,
    /// Starts the listener once setup is ready; `None` once started.
    pub(super) recognition_start: Option<SyncSender<()>>,
    /// Why the listener stopped, once it has.
    pub(super) listener_failure: Arc<Mutex<Option<String>>>,
    /// The listener's event log, read on this thread so the window can show a
    /// failed dictation without another polling thread.
    pub(super) event_reader: EventReader,
    pub(super) activity: DesktopActivity,
    /// When the current session's newest dictation completed.
    pub(super) last_completed_at: Option<u64>,
    /// When this runtime started, in the event log's wall-clock ms. Failures
    /// older than this belong to the previous session.
    pub(super) started_at: u64,
    /// Consented recordings the window can stop or cancel.
    pub(super) record: Option<Arc<crate::local_api::RecordSessions>>,
}

impl Runtime {
    pub(super) fn handle(&mut self, request: Request) {
        match request {
            Request::ChooseModel { model, language } => {
                self.manual_choice();
                let selection = TranscriptionSelection { model, language };
                self.start_preparation(selection, false);
            }
            Request::SelectInstalledModel(model) => {
                self.reload_settings();
                self.manual_choice();
                self.start_preparation(self.settings.transcription_for_model(model), true);
            }
            Request::UseInstalledSelection(selection) => {
                self.manual_choice();
                // A saved file still needs its check even when the choice
                // names it again; one with no file has nothing to load.
                let same = same_model_and_language(&selection, &self.settings.transcription);
                let check = crate::transcription::models::validate(&selection).is_ok_and(|model| {
                    crate::transcription::models::needs_check(&model, &selection.language)
                });
                if !same || check {
                    self.start_preparation(selection, true);
                }
            }
            Request::InputSourceChanged(source) => {
                if self
                    .keyboard_switcher
                    .source_changed(source, Instant::now())
                {
                    self.supersede_automatic();
                }
            }
            Request::CancelModel => {
                self.manual_choice();
                self.preparation.cancel();
            }
            Request::SwapModel => self.swap_model(),
            Request::RecordControl { id, stop } => {
                if let Some(record) = &self.record {
                    if stop {
                        record.stop(id);
                    } else {
                        record.cancel(id);
                    }
                }
            }
            Request::RevokePermission { id } => {
                if let Some(record) = &self.record {
                    record.revoke_permission(id);
                }
            }
            Request::RevokeRecordGrant { id } => {
                if let Some(record) = &self.record {
                    record.revoke(id);
                }
            }
            Request::Website { origin, action } => {
                if let Some(record) = &self.record {
                    record.website(&origin, action);
                }
            }
            Request::ReloadSettings => self.reload_settings(),
            Request::Desktop(_)
            | Request::CaptureShortcutFor(_)
            | Request::CancelShortcut
            | Request::SetShortcut { .. } => {}
        }
    }

    fn reload_settings(&mut self) {
        match self.settings.refreshed() {
            Ok(Some(settings)) => {
                if self
                    .keyboard_switcher
                    .configure(settings.keyboard_model_switching.clone(), Instant::now())
                {
                    self.supersede_automatic();
                }
                if self
                    .app_switcher
                    .configure(settings.labs.app_model_switching)
                {
                    self.supersede_automatic();
                }
                self.settings = settings;
            }
            Ok(None) => {}
            Err(error) => tracing::warn!(%error, "could not reload settings"),
        }
    }

    fn manual_choice(&mut self) {
        self.keyboard_switcher.manual_choice();
        self.app_switcher.manual_choice();
        self.supersede_automatic();
    }

    /// Starts a preparation that isn't a swap, dropping any swap it
    /// supersedes so that swap's way back can never commit.
    fn start_preparation(&mut self, selection: TranscriptionSelection, installed_only: bool) {
        self.pending_swap = None;
        self.preparation.start(selection, installed_only);
    }

    fn supersede_automatic(&mut self) {
        // A source being cycled through must not cancel a manual operation.
        // Manual work is superseded only when the debounced choice is due.
        if let Some(source) = self.automatic_preparation.take() {
            self.preparation.cancel();
            match source {
                AutomaticSwitch::Keyboard => self.keyboard_switcher.cancel_applying(),
                AutomaticSwitch::App => self.app_switcher.cancel_applying(),
                AutomaticSwitch::Swap => self.pending_swap = None,
            }
        }
    }

    /// Labs › Model switch shortcut: prepares the saved other model, installed
    /// only, and swaps both selections together once it loads. A press while a
    /// capture records is ignored; a failure leaves the running model and the
    /// saved selection as they were.
    fn swap_model(&mut self) {
        use crate::transcription::other_model;
        if crate::transcription::engine::dictation_busy() {
            tracing::info!("the model switch request was ignored while dictating");
            return;
        }
        let plan = match other_model::planned(&self.settings) {
            Ok(Some(plan)) => plan,
            Ok(None) => return,
            Err(error) => {
                self.report_switch_error(&error);
                return;
            }
        };
        if let Err(error) = other_model::installed(&plan) {
            self.report_switch_error(&error);
            return;
        }
        self.manual_choice();
        self.pending_swap = Some(plan.other);
        self.preparation.start(plan.to, true);
        self.automatic_preparation = Some(AutomaticSwitch::Swap);
    }

    /// Shows a failed switch in Settings, and beside the HUD when Labs ›
    /// Failure notices is on.
    fn report_switch_error(&self, error: &crate::transcription::other_model::SwitchError) {
        self.preparation.set_error(Some(error.message()));
        if self.settings.labs.failure_notices
            && let Some((template, detail)) = error.notice()
        {
            self.hud.send(
                crate::dictation::indicator::DictationIndicatorEvent::Failed {
                    message: Some(crate::desktop::i18n::tr_fill(template, detail)),
                },
            );
        }
    }

    /// Commits a prepared model and starts a listener that waited for setup.
    fn tick(&mut self) {
        self.tick_at(Instant::now());
    }

    /// Follows the keyboard layout switcher's due choice, if any.
    fn switch_with_keyboard(&mut self, now: Instant) {
        if let Some(choice) = self.keyboard_switcher.take_due(now) {
            let selection = TranscriptionSelection {
                language: choice.language,
                ..self.settings.transcription_for_model(choice.model)
            };
            if !same_model_and_language(&selection, &self.settings.transcription) {
                if self.automatic_preparation == Some(AutomaticSwitch::App) {
                    self.app_switcher.cancel_applying();
                }
                self.start_preparation(selection, true);
                self.automatic_preparation = Some(AutomaticSwitch::Keyboard);
            } else {
                // The debounced choice keeps the current model, but must
                // still prevent an older manual preparation replacing it.
                self.keyboard_switcher.commit();
                if self.automatic_preparation.take() == Some(AutomaticSwitch::App) {
                    self.app_switcher.cancel_applying();
                }
                self.pending_swap = None;
                self.preparation.cancel();
            }
        }
    }

    /// Feeds one foreground read to the app switcher. Hex's own window and a
    /// moment with no application in front carry no mode, so they leave the
    /// last match and any switch it scheduled alone. A dictation that records
    /// or transcribes holds a due switch until it finishes.
    pub(super) fn follow_app(
        &mut self,
        observation: crate::transcription::app_model_switching::Observation<'_>,
        now: Instant,
    ) {
        if !self.settings.labs.app_model_switching {
            return;
        }
        if self.app_switcher.observe(
            observation,
            |application, bundle_name| {
                crate::transcription::app_model_switching::mode_choice(
                    &self.settings,
                    application,
                    bundle_name,
                )
            },
            now,
        ) {
            self.supersede_automatic();
        }
        if crate::transcription::engine::dictation_busy() {
            return;
        }
        let Some(choice) = self
            .app_switcher
            .take_due(now, &self.settings.transcription)
        else {
            return;
        };
        let selection = TranscriptionSelection {
            language: choice.language,
            ..self.settings.transcription_for_model(choice.model)
        };
        if self.automatic_preparation == Some(AutomaticSwitch::Keyboard) {
            self.keyboard_switcher.cancel_applying();
        }
        self.start_preparation(selection, true);
        self.automatic_preparation = Some(AutomaticSwitch::App);
    }

    /// Reads the listener's newest events, bounded by `EventReader`'s
    /// retention, so the window's warning follows failed dictations.
    fn refresh_activity(&mut self) {
        self.activity.refresh(&mut self.event_reader);
        self.last_completed_at = last_completed_at(self.event_reader.events().iter());
    }

    pub(super) fn tick_at(&mut self, now: Instant) {
        if crate::transcription::other_model::take_request() {
            self.swap_model();
        }
        self.switch_with_keyboard(now);
        // Read the frontmost application only while the lab is on. Hex's own
        // window names no mode and is ignored.
        let (application, bundle_name) = self
            .settings
            .labs
            .app_model_switching
            .then(crate::dictation::context::frontmost_other_application)
            .flatten()
            .unzip();
        self.follow_app(
            crate::transcription::app_model_switching::Observation::frontmost(
                application.as_deref(),
                bundle_name.flatten().as_deref(),
            ),
            now,
        );
        if let Some(result) = self.preparation.poll() {
            let automatic = self.automatic_preparation.take();
            // A swap's way back is saved with the selection it swaps in, in
            // one write, only once the new model has loaded.
            let swap_other = (automatic == Some(AutomaticSwitch::Swap))
                .then(|| self.pending_swap.take())
                .flatten();
            let result = result.and_then(|selection| {
                crate::transcription::models::validate(&selection).map_err(|e| e.to_string())?;
                // Only a preparation that changes what Hex runs replaces the
                // saved choice: a check of that same model, or of a lost
                // import's fallback, leaves the saved selection alone.
                let effective =
                    crate::transcription::custom_models::runnable(&self.settings.transcription);
                if same_model_and_language(&selection, &effective) && swap_other.is_none() {
                    return Ok(());
                }
                let mut settings = AppSettings::load().map_err(|e| format!("{e:#}"))?;
                let mut candidate = settings.clone();
                if let Some(other) = swap_other {
                    candidate.model_switch_other = Some(other);
                }
                candidate.remember_transcription(selection);
                settings = settings.update(&candidate).map_err(|e| format!("{e:#}"))?;
                self.settings = settings;
                Ok(())
            });
            match result {
                Ok(()) => {
                    match automatic {
                        Some(AutomaticSwitch::Keyboard) => self.keyboard_switcher.commit(),
                        Some(AutomaticSwitch::App) => self.app_switcher.commit(),
                        Some(AutomaticSwitch::Swap) | None => {}
                    }
                    self.preparation.set_error(None);
                }
                Err(error) => {
                    match automatic {
                        Some(AutomaticSwitch::Keyboard) => self.keyboard_switcher.fail(now),
                        Some(AutomaticSwitch::App) => self.app_switcher.fail(now),
                        Some(AutomaticSwitch::Swap) | None => {}
                    }
                    tracing::warn!(%error, "could not select transcription model");
                    if automatic == Some(AutomaticSwitch::Swap)
                        && self.settings.labs.failure_notices
                    {
                        self.hud.send(
                            crate::dictation::indicator::DictationIndicatorEvent::Failed {
                                message: Some(crate::desktop::i18n::tr_fill(
                                    "Couldn't switch the speech model: {}",
                                    &error,
                                )),
                            },
                        );
                    }
                    self.preparation.set_error(Some(error));
                }
            }
        }
        if self.recognition_start.is_some()
            && crate::desktop::onboarding::launch_ready(&self.settings.transcription)
            && let Some(start) = self.recognition_start.take()
        {
            let _ = start.try_send(());
        }
    }

    fn publish(&self) -> Published {
        let preparation = self.preparation.status();
        let failure = lock(&self.listener_failure).clone();
        let running = self.recognition_start.is_none() && failure.is_none();
        Published {
            listener: DesktopListenerSnapshot {
                running,
                warming: false,
                status: if failure.is_some() {
                    ListenerStatus::Stopped
                } else if running {
                    ListenerStatus::Enabled
                } else {
                    ListenerStatus::WaitingForSetup
                },
                warning: failure,
                notice: None,
                last_failure: self
                    .activity
                    .visible_failure_since(self.started_at, self.last_completed_at),
                shortcuts: DesktopShortcuts::Registered,
                shortcuts_configurable: false,
            },
            transcription: DesktopTranscriptionSnapshot {
                downloaded_bytes: preparation.downloaded_bytes,
                error: preparation.error.clone(),
                preparation_stage: preparation.stage,
                selection: self.settings.transcription.clone(),
                preparing: preparation.model,
                backend: None,
            },
            update_status: update_status(),
            update_notes: UPDATER.get().and_then(crate::updater::Updater::notes),
            settings: self.settings.clone(),
            preparation,
            record: self.record.as_ref().and_then(|record| record.view()),
            permissions: self
                .record
                .as_ref()
                .map(|r| r.permissions())
                .unwrap_or_default(),
            grants: self
                .record
                .as_ref()
                .map(|record| record.grants())
                .unwrap_or_default(),
            websites: self
                .record
                .as_ref()
                .map(|record| record.websites())
                .unwrap_or_default(),
        }
    }
}
