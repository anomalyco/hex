//! The Linux host's model preparation and readiness: Enable checks a model
//! file another app downloaded before its listener starts, as Windows does.

use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicU8, AtomicU64, Ordering};

use color_eyre::Result;

use crate::desktop::host::ListenerStatus;

use super::{LinuxDesktopHost, PreparationResult, PreparedTranscription};

pub(super) struct TranscriptionPreparation {
    pub(super) canceled: Arc<AtomicBool>,
    pub(super) selection: crate::transcription::models::TranscriptionSelection,
    pub(super) progress: Arc<AtomicU64>,
    pub(super) stage: Arc<AtomicU8>,
    pub(super) worker: std::thread::JoinHandle<PreparationResult>,
    /// A keyboard switch, which a manual choice or a later layout change
    /// supersedes.
    pub(super) automatic: bool,
}

/// A preparation waiting for the worker it superseded to exit, so only one
/// model ever loads at a time.
pub(super) struct PendingPreparation {
    pub(super) selection: crate::transcription::models::TranscriptionSelection,
    pub(super) installed_only: bool,
    pub(super) automatic: bool,
    /// The selection a swap preparation swaps back to, if this is a swap.
    pub(super) swap_other: Option<crate::app_settings::KeyboardModelChoice>,
}

impl LinuxDesktopHost {
    /// Downloads (manual) or verifies (a switch, or Enable) and loads a model
    /// off the host thread. `automatic` is a keyboard switch, which a manual
    /// choice supersedes; a check is not automatic.
    pub(super) fn prepare_transcription(
        &mut self,
        selection: crate::transcription::models::TranscriptionSelection,
        installed_only: bool,
        automatic: bool,
    ) -> Result<()> {
        self.request_transcription(PendingPreparation {
            selection,
            installed_only,
            automatic,
            swap_other: None,
        })
    }

    /// Starts a preparation, or queues it behind the worker it supersedes so
    /// two models never load at once. A newer choice cancels a running swap
    /// and starts only once that load exits; a switch never cancels a
    /// download or check someone else started and is refused instead.
    pub(super) fn request_transcription(&mut self, request: PendingPreparation) -> Result<()> {
        let definition = crate::transcription::models::validate(&request.selection)?;
        if self.shortcut_edit.is_some() {
            if request.automatic {
                // A layout switch waits until the shortcut recorder closes
                // instead of failing into Settings.
                self.pending_preparation = Some(request);
                return Ok(());
            }
            let error = "Finish the shortcut change before changing the model".to_string();
            self.transcription_error = Some(error.clone());
            return Err(color_eyre::eyre::eyre!(error));
        }
        if self.transcription_preparation.is_some() {
            if request.swap_other.is_some() && self.swap_other.is_none() {
                // Never cancel a download or check just because the switch
                // key was pressed: refuse the swap instead.
                let error = "The previous model preparation is still running".to_string();
                self.transcription_error = Some(error.clone());
                return Err(color_eyre::eyre::eyre!(error));
            }
            // A newer choice supersedes the running preparation: its load is
            // canceled and it can never commit a model. The new request waits
            // for that worker to exit before it starts, so two models never
            // load at once.
            if let Some(preparation) = &self.transcription_preparation {
                preparation.canceled.store(true, Ordering::Relaxed);
            }
            self.pending_preparation = Some(request);
            return Ok(());
        }
        if let Some(pending) = self.pending_preparation.take() {
            // A manual choice supersedes a queued swap; a switch cannot
            // supersede a queued manual choice.
            if request.swap_other.is_some() && pending.swap_other.is_none() {
                self.pending_preparation = Some(pending);
                let error = "The previous model preparation is still running".to_string();
                self.transcription_error = Some(error.clone());
                return Err(color_eyre::eyre::eyre!(error));
            }
        }
        self.start_transcription(request, definition);
        Ok(())
    }

    /// Starts the worker for a validated request. Only one preparation runs
    /// at a time; start_pending_preparation starts the next.
    fn start_transcription(
        &mut self,
        request: PendingPreparation,
        definition: crate::transcription::models::ModelRef,
    ) {
        let PendingPreparation {
            selection,
            installed_only,
            automatic,
            swap_other,
        } = request;
        // A running listener keeps the current model until the new one is ready.
        let canceled = Arc::new(AtomicBool::new(false));
        let progress = Arc::new(AtomicU64::new(0));
        let stage = Arc::new(AtomicU8::new(if installed_only {
            crate::transcription::models::ModelPreparationStage::Verifying as u8
        } else {
            crate::transcription::models::ModelPreparationStage::Downloading as u8
        }));
        let worker_canceled = canceled.clone();
        let worker_progress = progress.clone();
        let worker_stage = stage.clone();
        let tracked = selection.clone();
        let worker = std::thread::spawn(move || {
            (|| {
                if installed_only {
                    crate::transcription::models::ModelPreparationStage::Verifying
                        .store(&worker_stage);
                    crate::transcription::models::verify_installed(&definition, &worker_canceled)?;
                } else {
                    crate::transcription::models::download_with_stage_progress(
                        &definition,
                        &worker_canceled,
                        &worker_progress,
                        &worker_stage,
                    )?;
                }
                if worker_canceled.load(Ordering::Relaxed) {
                    return Err(color_eyre::eyre::eyre!("model activation canceled"));
                }
                crate::transcription::models::ModelPreparationStage::Loading.store(&worker_stage);
                let transcriber =
                    crate::transcription::local_transcriber::LocalTranscriber::load(&selection)?;
                if worker_canceled.load(Ordering::Relaxed) {
                    return Err(color_eyre::eyre::eyre!("model activation canceled"));
                }
                Ok(PreparedTranscription {
                    selection,
                    transcriber,
                })
            })()
            .map_err(|error| format!("{error:#}"))
        });
        // Only an installed-only, non-automatic preparation is Enable's
        // check: a manual download and a keyboard switch are not.
        self.checking = installed_only && !automatic;
        self.swap_other = swap_other;
        self.transcription_preparation = Some(TranscriptionPreparation {
            canceled,
            selection: tracked,
            progress,
            stage,
            worker,
            automatic,
        });
        self.transcription_error = None;
    }

    /// Starts the preparation a newer choice queued once the canceled worker
    /// has exited, so its model load never overlaps the old one.
    pub(super) fn start_pending_preparation(&mut self) {
        if self.transcription_preparation.is_some() || self.shortcut_edit.is_some() {
            return;
        }
        let Some(pending) = self.pending_preparation.take() else {
            return;
        };
        match crate::transcription::models::validate(&pending.selection) {
            Ok(definition) => self.start_transcription(pending, definition),
            Err(error) => self.transcription_error = Some(format!("{error:#}")),
        }
    }

    /// Labs › Model switch shortcut: prepares the saved other model,
    /// installed only, and remembers what it swaps back to. Both selections
    /// are saved together once that preparation loads; a failure leaves the
    /// running model and the saved selection as they were.
    pub(super) fn swap_model(&mut self) {
        use crate::transcription::other_model::{self, SwitchError};
        if crate::transcription::engine::dictation_busy() {
            tracing::info!("the model switch request was ignored while dictating");
            return;
        }
        // A switch never cancels a download or check someone else started.
        if self.pending_preparation.is_some()
            || (self.transcription_preparation.is_some() && self.swap_other.is_none())
        {
            self.report_switch_error(&SwitchError::Busy);
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
        // The same swap already loading changes nothing.
        if self.swap_other.as_ref() == Some(&plan.other)
            && self
                .transcription_preparation
                .as_ref()
                .is_some_and(|preparation| preparation.selection == plan.to)
        {
            return;
        }
        self.manual_choice();
        let request = PendingPreparation {
            selection: plan.to.clone(),
            installed_only: true,
            automatic: false,
            swap_other: Some(plan.other.clone()),
        };
        if let Err(error) = self.request_transcription(request) {
            self.report_switch_error(&SwitchError::Invalid(format!("{error:#}")));
        }
    }

    /// Shows a failed switch in Settings, and beside the HUD when Labs ›
    /// Failure notices is on.
    fn report_switch_error(&mut self, error: &crate::transcription::other_model::SwitchError) {
        self.transcription_error = Some(error.message());
        if self.settings.labs.failure_notices
            && let Some((template, detail)) = error.notice()
            && let Some(listener) = &self.listener
        {
            listener.update(super::ListenerUpdate::Notice(super::tr_fill(
                template, detail,
            )));
        }
    }

    /// Verifies and loads the selected model when its file is here but not
    /// ready yet (another app's catalog download), using the installed-only
    /// preparation. No file means no check: Enable says Model required.
    pub(super) fn check_selected_model(&mut self) -> bool {
        let selection = crate::transcription::custom_models::runnable(&self.settings.transcription);
        let Ok(model) = crate::transcription::models::validate(&selection) else {
            return false;
        };
        if !crate::transcription::models::needs_check(&model, &selection.language) {
            return false;
        }
        if self.prepare_transcription(selection, true, false).is_err() {
            return false;
        }
        self.status = ListenerStatus::Enabling;
        true
    }

    /// A listener that stopped to check a model failed to reach listening;
    /// say so instead of Enabling… forever.
    pub(super) fn note_model_check_failure(&mut self) {
        if self.listen_when_ready && !self.is_running() && self.status == ListenerStatus::Enabling {
            self.status = ListenerStatus::CouldNotEnable;
        }
    }
}
