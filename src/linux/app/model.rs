//! The Linux host's model preparation and readiness: Enable checks a model
//! file another app downloaded before its listener starts, as Windows does.

use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicU8, AtomicU64, Ordering};

use color_eyre::Result;

use crate::desktop::host::ListenerStatus;

use super::{LinuxDesktopHost, PreparedTranscription, TranscriptionPreparation};

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
        if self.shortcut_edit.is_some() {
            let error = "Finish the shortcut change before changing the model".to_string();
            self.transcription_error = Some(error.clone());
            return Err(color_eyre::eyre::eyre!(error));
        }
        if self.transcription_preparation.is_some() {
            self.cancel_transcription_preparation();
            let error = "The previous model preparation is still stopping".to_string();
            self.transcription_error = Some(error.clone());
            return Err(color_eyre::eyre::eyre!(error));
        }
        let model = selection.model;
        let definition = crate::transcription::models::validate(&selection)?;
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
        let worker = std::thread::spawn(move || {
            (|| {
                if installed_only {
                    crate::transcription::models::ModelPreparationStage::Verifying
                        .store(&worker_stage);
                    crate::transcription::models::verify_installed(definition, &worker_canceled)?;
                } else {
                    crate::transcription::models::download_with_stage_progress(
                        definition,
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
        self.transcription_preparation = Some(TranscriptionPreparation {
            canceled,
            model,
            progress,
            stage,
            worker,
            automatic,
        });
        self.transcription_error = None;
        Ok(())
    }

    /// Verifies and loads the selected model when its file is here but not
    /// ready yet (another app's catalog download), using the installed-only
    /// preparation. No file means no check: Enable says Model required.
    pub(super) fn check_selected_model(&mut self) -> bool {
        let selection = crate::transcription::custom_models::runnable(&self.settings.transcription);
        let Ok(model) = crate::transcription::models::validate(&selection) else {
            return false;
        };
        if !crate::transcription::models::needs_check(model, &selection.language) {
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
