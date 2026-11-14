//! What happens to a finished Linux dictation's transcript: processing,
//! the paste, Paste last dictation, and History.

use std::sync::{Arc, RwLock};
use std::time::Instant;

use color_eyre::Result;
use color_eyre::eyre::eyre;

/// What happens to a transcript before and after it is pasted: the modes
/// that process it and the history that keeps it. The host shares it
/// with the listener so saved settings apply without a restart.
#[derive(Clone)]
pub(crate) struct DictationOutput {
    pub(crate) history: Option<crate::history::History>,
    pub(crate) modes: Arc<RwLock<crate::dictation::modes::ModeRuntime>>,
    pub(crate) transformations: Arc<crate::personal_commands::TransformationClient>,
}

/// How a finished dictation is pasted: its text, whether it asks for
/// Ctrl+Shift+V, and whether Return follows (Labs › YOLO mode).
/// No paste at all means a printing stop.
pub(super) type Paste<'a> = &'a mut dyn FnMut(&str, bool, bool) -> Result<()>;

impl DictationOutput {
    pub(crate) fn from_settings(
        settings: &crate::app_settings::AppSettings,
        history: Option<crate::history::History>,
    ) -> Self {
        Self {
            history,
            modes: Arc::new(RwLock::new(
                crate::dictation::modes::ModeRuntime::from_settings(settings),
            )),
            transformations: Arc::new(crate::personal_commands::TransformationClient::default()),
        }
    }

    /// Reuses the mode runtime and transformation client the local API's
    /// `process=global` reads, so both see the same settings and personal
    /// commands host.
    pub(crate) fn sharing(mut self, processing: crate::dictation::modes::ProcessingState) -> Self {
        self.modes = processing.modes;
        self.transformations = processing.transformations;
        self
    }

    /// Processes (unless raw) and pastes one transcript, or only
    /// processes it when there is no paste (a printing stop), then records
    /// it. A failed paste still becomes Paste last dictation's.
    #[allow(clippy::too_many_arguments)]
    pub(super) fn deliver(
        &self,
        raw_text: String,
        flavour: crate::dictation::DictationFlavour,
        language: &str,
        paste: Option<Paste<'_>>,
        last_dictation: &mut Option<String>,
        audio_ms: u64,
        inference_ms: u64,
        started: Instant,
        recording: Option<crate::history::Recording>,
        typing: Option<crate::linux::live_typing::Session>,
        context: crate::dictation::context::ContextSnapshot,
        held_ms: Option<u64>,
    ) -> Result<String> {
        let application = context.application.clone();
        let modes = self
            .modes
            .read()
            .unwrap_or_else(|error| error.into_inner())
            .clone();
        let processed = if flavour.raw() {
            crate::dictation::processor::Processed::unchanged(raw_text.trim())
        } else {
            modes.process(
                &raw_text,
                language,
                &context,
                &self.transformations,
                crate::dictation::processor::Source::Dictation,
            )
        };
        let printed = paste.is_none();
        // A voice prompt that deliberately asked nothing ends quietly: no
        // failure, no paste, no last dictation, and no History. A live
        // preedit for the capture is discarded rather than committed empty.
        if processed.nothing {
            if let Some(session) = typing {
                session.cancel();
            }
            return Ok(processed.text);
        }
        // Live typing inserts the final text through the input method; a
        // printed or copy-only dictation clears its preedit and takes the
        // usual path instead.
        let inserted = match typing {
            Some(session) if !printed && !crate::labs::copy_only() => {
                session.finish(&processed.text)
            }
            Some(session) => {
                session.cancel();
                false
            }
            None => false,
        };
        if processed.text.trim().is_empty() {
            return Err(eyre!("text replacements produced empty output"));
        }
        // Labs › YOLO mode, read as the output commits.
        let send = !inserted
            && !flavour.raw()
            && !crate::labs::copy_only()
            && held_ms.is_some_and(|held_ms| {
                crate::dictation::yolo_mode::sends(held_ms, application.as_deref())
            });
        let mut sent = false;
        if let Some(paste) = paste
            && !inserted
        {
            // A paste that fails (another app changed the clipboard while
            // Hex saved it, or a format couldn't be read) keeps the text
            // for Paste last dictation, so the dictation isn't lost;
            // History keeps only pasted output.
            paste(&processed.text, flavour.terminal_paste(), send)
                .inspect_err(|_| *last_dictation = Some(processed.text.clone()))?;
            sent = send;
        }
        if !printed {
            *last_dictation = Some(processed.text.clone());
        }
        if let Some(history) = &self.history {
            let mut draft = crate::dictation::modes::history_draft(
                raw_text,
                &processed,
                context.application,
                audio_ms,
                inference_ms,
                started.elapsed().as_millis() as u64,
                recording,
            );
            if printed {
                draft.kind = crate::history::HistoryKind::Printed;
            } else if sent {
                draft.kind = crate::history::HistoryKind::Send;
            } else if flavour.raw() {
                draft.kind = crate::history::HistoryKind::Raw;
            }
            if let Err(error) = history.record(draft) {
                tracing::warn!(%error, "could not retain Linux dictation history");
            }
        }
        Ok(processed.text)
    }
}
