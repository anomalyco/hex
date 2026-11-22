//! The text one processing run hands the paste, History, and Paste Last, and
//! the last stage that rewrites it: the mode's transformations.

use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Instant;

use crate::dictation::context::ContextSnapshot;

use super::{CUSTOM_TRANSFORMATIONS, Source};

pub struct Processed {
    pub text: String,
    /// A voice prompt that deliberately asked nothing: the listeners finish
    /// quietly instead of failing on empty text. Never true for an ordinary
    /// empty output.
    pub nothing: bool,
    pub observation: Option<ProcessingObservation>,
    pub transformations: Vec<String>,
}

pub type ProcessingObservation = crate::desktop::events::DictationProcessing;

impl Processed {
    /// `text` as heard, with no step run (Dictate without processing on the
    /// Linux and Windows listeners; macOS skips processing in `parakeet`).
    #[cfg_attr(target_os = "macos", allow(dead_code))]
    pub fn unchanged(text: &str) -> Self {
        Self {
            text: text.to_string(),
            nothing: false,
            observation: None,
            transformations: Vec::new(),
        }
    }

    /// Runs the selected transformations, the last processing stage. The chain
    /// is one stage: a failure keeps the previous text and records the error.
    pub fn apply_transformations(
        &mut self,
        client: &crate::personal_commands::TransformationClient,
        context: &ContextSnapshot,
        cancelled: &AtomicBool,
        source: Source,
    ) {
        if self.transformations.is_empty() || cancelled.load(Ordering::Acquire) {
            return;
        }
        let started = Instant::now();
        let transformed = client.transform(
            &self.transformations,
            &self.text,
            context,
            cancelled,
            source,
        );
        let observation = self
            .observation
            .get_or_insert_with(|| ProcessingObservation {
                profile: CUSTOM_TRANSFORMATIONS.into(),
                latency_ms: 0,
                model: None,
                fallback: None,
            });
        observation.latency_ms = observation
            .latency_ms
            .saturating_add(started.elapsed().as_millis() as u64);
        match transformed {
            Ok(text) => self.text = text,
            Err(error) => observation.fallback = Some(error),
        }
    }
}
