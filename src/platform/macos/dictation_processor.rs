//! macOS integration around the shared ordered dictation-processing policy.
//!
//! Ordinary mode processing lives in `common::dictation_processing`. This
//! module retains the macOS settings lookup used by Voice Action and the
//! OpenCode catalog exports consumed by the app UI.

use std::sync::atomic::AtomicBool;
use std::time::{Duration, Instant};

use crate::context::ContextSnapshot;
use crate::opencode::{Model, fulfil_voice_action_cancellable};

pub use crate::dictation_processing::{Processed, ProcessingObservation, Profile, Profiles};
pub use crate::opencode::{ModelCatalog, ModelChoice, load_model_catalog, opencode_installed};

impl Profiles {
    pub fn process_voice_action_cancellable(
        &self,
        instruction: &str,
        selected_text: Option<&str>,
        context: &ContextSnapshot,
        cancelled: &AtomicBool,
    ) -> Processed {
        let settings = crate::app_settings::voice_action_settings();
        let deadline = Duration::from_secs(settings.deadline_seconds.max(1));
        let started = Instant::now();
        let model = match settings.model.as_deref() {
            None => None,
            Some(key) => {
                let Some((provider, id)) = key.split_once('/') else {
                    return voice_action_failure(
                        started,
                        "Voice Action model must use provider/model format".into(),
                    );
                };
                Some(Model {
                    provider: provider.into(),
                    id: id.into(),
                    variant: settings.variant.clone(),
                })
            }
        };
        match fulfil_voice_action_cancellable(
            instruction,
            context.application.as_deref(),
            context.browser_host(),
            selected_text,
            model.as_ref(),
            deadline,
            cancelled,
        ) {
            Ok(text) if !text.trim().is_empty() => {
                let latency_ms = started.elapsed().as_millis() as u64;
                tracing::info!(latency_ms, "voice action completed");
                Processed {
                    text: text.trim().into(),
                    observation: Some(ProcessingObservation {
                        profile: "Voice Action".into(),
                        latency_ms,
                        fallback: None,
                    }),
                }
            }
            Ok(_) => voice_action_failure(started, "processor returned empty text".into()),
            Err(error) => voice_action_failure(started, error.to_string()),
        }
    }
}

#[cfg(test)]
fn voice_action_prompt(
    instruction: &str,
    selected_text: Option<&str>,
    context: &ContextSnapshot,
) -> String {
    crate::opencode::voice_action_prompt(
        instruction,
        context.application.as_deref(),
        context.browser_host(),
        selected_text,
    )
}

fn voice_action_failure(started: Instant, error: String) -> Processed {
    Processed {
        text: String::new(),
        observation: Some(ProcessingObservation {
            profile: "Voice Action".into(),
            latency_ms: started.elapsed().as_millis() as u64,
            fallback: Some(error),
        }),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn context(application: &str) -> ContextSnapshot {
        ContextSnapshot {
            application: Some(application.into()),
            ..ContextSnapshot::default()
        }
    }

    #[test]
    fn voice_action_prompt_keeps_the_instruction_separate_from_selected_text() {
        let prompt = voice_action_prompt(
            "Make this concise.",
            Some("This is the original selected paragraph."),
            &context("Slack"),
        );

        assert!(prompt.contains("Foreground application: Slack"));
        assert!(prompt.contains("Dictated instruction (18 UTF-8 bytes):\nMake this concise."));
        assert!(
            prompt.contains(
                "Selected text (40 UTF-8 bytes):\nThis is the original selected paragraph."
            )
        );
        assert!(prompt.contains("Return only the exact paste-ready result"));
    }

    #[test]
    fn voice_action_prompt_supports_generation_without_a_selection() {
        let prompt = voice_action_prompt("Write a funny release note.", None, &context("Slack"));

        assert!(prompt.contains("No text was selected"));
        assert!(prompt.contains("Write a funny release note."));
    }
}
