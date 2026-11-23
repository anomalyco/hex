//! What happens to a finished Linux dictation's transcript: processing,
//! live typing or clipboard insertion, Paste last dictation, and History,
//! driven by the shared output policy in `crate::dictation::modes::output`.

use std::sync::{Arc, RwLock};
use std::time::Instant;

use color_eyre::Result;
use color_eyre::eyre::eyre;

use crate::dictation::modes::output::{
    DeliveryInput, DeliveryOutcome, EMPTY_PROCESSED_ERROR, InsertPlan, InsertResult, OutputAdapter,
    OutputTarget, deliver, resolve_paste_last,
};
use crate::dictation::paste_last::LastDictation;
use crate::linux::paste::LinuxPaster;

/// What happens to a transcript before and after it is pasted: the modes
/// that process it and the history that keeps it. The host shares it
/// with the listener so saved settings apply without a restart.
#[derive(Clone)]
pub(crate) struct DictationOutput {
    pub(crate) history: Option<crate::history::History>,
    pub(crate) modes: Arc<RwLock<crate::dictation::modes::ModeRuntime>>,
    pub(crate) transformations: Arc<crate::personal_commands::TransformationClient>,
}

/// How a finished dictation is inserted (copy-only or paste + optional Return).
pub(super) type Insert<'a> = &'a mut dyn FnMut(&str, InsertPlan) -> Result<InsertResult>;

/// Paste last dictation: reinsert the remembered text, standing alone when it
/// carries the signature line so the next dictation never joins it.
pub(super) fn paste_last(
    paster: &mut Result<LinuxPaster>,
    last: Option<&LastDictation>,
) -> Result<()> {
    let last = resolve_paste_last(last, None).map_err(|error| eyre!(error))?;
    paster
        .as_mut()
        .map_err(|error| eyre!("{error:#}"))?
        .paste_dictation(last.text(), false, last.standalone())
}

struct LinuxOutputAdapter<'a> {
    typing: Option<crate::linux::live_typing::Session>,
    insert: Option<Insert<'a>>,
    last_dictation: &'a mut Option<LastDictation>,
}

impl OutputAdapter for LinuxOutputAdapter<'_> {
    fn cancel_pending(&mut self) {
        if let Some(session) = self.typing.take() {
            session.cancel();
        }
    }

    fn insert(&mut self, text: &str, plan: InsertPlan) -> Result<InsertResult, String> {
        match plan {
            InsertPlan::None => {
                self.cancel_pending();
                Ok(InsertResult::default())
            }
            InsertPlan::CopyOnly => {
                self.cancel_pending();
                let Some(insert) = self.insert.as_mut() else {
                    return Ok(InsertResult {
                        sent: false,
                        copied: true,
                    });
                };
                insert(text, plan).map_err(|error| format!("{error:#}"))
            }
            InsertPlan::Paste { .. } => {
                // Live typing inserts the final text through the input method;
                // a failed or absent session falls back to the clipboard paste.
                if let Some(session) = self.typing.take()
                    && session.finish(text)
                {
                    return Ok(InsertResult::default());
                }
                let Some(insert) = self.insert.as_mut() else {
                    return Ok(InsertResult::default());
                };
                insert(text, plan).map_err(|error| format!("{error:#}"))
            }
        }
    }

    fn remember_last(&mut self, last: LastDictation, _recorded_in_history: bool) {
        *self.last_dictation = Some(last);
    }
}

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

    /// Processes (unless raw) and delivers one transcript according to
    /// `target`, updating Paste last dictation and History through the shared
    /// policy. A failed paste still becomes Paste last dictation's.
    #[allow(clippy::too_many_arguments)]
    pub(super) fn deliver(
        &self,
        raw_text: String,
        target: OutputTarget,
        language: &str,
        insert: Option<Insert<'_>>,
        last_dictation: &mut Option<LastDictation>,
        audio_ms: u64,
        inference_ms: u64,
        started: Instant,
        recording: Option<crate::history::Recording>,
        typing: Option<crate::linux::live_typing::Session>,
        context: crate::dictation::context::ContextSnapshot,
        by_shortcut: bool,
    ) -> Result<String> {
        let processed = if target.processes() {
            let modes = self
                .modes
                .read()
                .unwrap_or_else(|error| error.into_inner())
                .clone();
            modes.process(
                &raw_text,
                language,
                &context,
                &self.transformations,
                crate::dictation::processor::Source::Dictation,
            )
        } else {
            crate::dictation::processor::Processed::unchanged(raw_text.trim())
        };
        let input = DeliveryInput {
            target,
            raw_text,
            text: processed.text,
            nothing: processed.nothing,
            application: context.application,
            bundle_name: None,
            processing: processed.observation,
            audio_ms,
            inference_ms,
            total_started: started,
            recording,
            allow_yolo: by_shortcut,
        };
        let mut adapter = LinuxOutputAdapter {
            typing,
            insert,
            last_dictation,
        };
        match deliver(input, self.history.as_ref(), &mut adapter).map_err(|error| eyre!(error))? {
            DeliveryOutcome::VoicePromptNothing => Ok(String::new()),
            DeliveryOutcome::EmptyOutput => Err(eyre!(EMPTY_PROCESSED_ERROR)),
            DeliveryOutcome::Delivered { text, .. } => Ok(text),
        }
    }
}
