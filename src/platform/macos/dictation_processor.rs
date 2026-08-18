use std::sync::atomic::AtomicBool;
use std::time::{Duration, Instant};

use crate::context::{ContextSelector, ContextSnapshot};
use crate::opencode::{Model, generate_cancellable};
pub use crate::opencode::{ModelCatalog, ModelChoice, load_model_catalog, opencode_installed};
use crate::text_replacements::ReplacementSet;

const PROTOCOL_PROMPT: &str = "You transform dictated speech into replacement text. Return only the text that should be pasted. Do not add an explanation, label, alternative, or Markdown fence.";

#[derive(Clone)]
pub struct Profile {
    name: String,
    ai_enabled: bool,
    prompt: String,
    model: Option<Model>,
    deadline: Option<Duration>,
    replacements: ReplacementSet,
    transformations: Vec<String>,
}

impl Profile {
    pub fn new(name: impl Into<String>, prompt: impl Into<String>) -> Self {
        Self {
            name: name.into(),
            ai_enabled: false,
            prompt: prompt.into(),
            model: None,
            deadline: None,
            replacements: ReplacementSet::default(),
            transformations: Vec::new(),
        }
    }

    pub fn ai_enabled(mut self, enabled: bool) -> Self {
        self.ai_enabled = enabled;
        self
    }

    pub(crate) fn replacements(mut self, replacements: ReplacementSet) -> Self {
        self.replacements = replacements;
        self
    }

    pub fn transformations(mut self, transformations: Vec<String>) -> Self {
        self.transformations = transformations;
        self
    }

    pub fn model(mut self, provider: impl Into<String>, id: impl Into<String>) -> Self {
        self.model = Some(Model {
            provider: provider.into(),
            id: id.into(),
            variant: None,
        });
        self
    }

    pub fn variant(mut self, variant: impl Into<String>) -> Self {
        let Some(model) = &mut self.model else {
            panic!("a processor profile variant requires a model");
        };
        model.variant = Some(variant.into());
        self
    }

    pub fn deadline(mut self, deadline: Duration) -> Self {
        self.deadline = Some(deadline);
        self
    }
}

#[derive(Clone)]
struct ContextualProfile {
    selector: ContextSelector,
    profile: Profile,
}

#[derive(Clone)]
pub struct Profiles {
    default: Profile,
    deadline: Duration,
    contextual: Vec<ContextualProfile>,
}

impl Profiles {
    pub fn new(default: Profile) -> Self {
        Self {
            default,
            deadline: Duration::from_secs(30),
            contextual: Vec::new(),
        }
    }

    pub fn deadline(mut self, deadline: Duration) -> Self {
        self.deadline = deadline;
        self
    }

    pub fn application(self, application: impl Into<String>, profile: Profile) -> Self {
        self.push(ContextSelector::application(application.into()), profile)
    }

    pub fn browser_host(self, host: impl Into<String>, profile: Profile) -> Self {
        self.push(ContextSelector::browser_host(host.into()), profile)
    }

    fn push(mut self, selector: ContextSelector, profile: Profile) -> Self {
        if self
            .contextual
            .iter()
            .any(|existing| existing.selector == selector)
        {
            tracing::warn!(?selector, "ignored duplicate dictation processor selector");
            return self;
        }
        self.contextual
            .push(ContextualProfile { selector, profile });
        self
    }

    fn select<'a>(&'a self, context: &ContextSnapshot) -> &'a Profile {
        let selected = self
            .contextual
            .iter()
            .find(|candidate| {
                candidate.selector.is_browser() && candidate.selector.matches(context)
            })
            .or_else(|| {
                self.contextual.iter().find(|candidate| {
                    !candidate.selector.is_browser() && candidate.selector.matches(context)
                })
            });
        selected.map_or(&self.default, |selected| &selected.profile)
    }

    pub fn processes(&self, context: &ContextSnapshot) -> bool {
        let profile = self.select(context);
        profile.ai_enabled || !profile.transformations.is_empty()
    }

    pub fn process_cancellable(
        &self,
        transcript: &str,
        context: &ContextSnapshot,
        cancelled: &AtomicBool,
    ) -> Processed {
        let profile = self.select(context);
        let corrected = profile.replacements.replace(transcript);
        if !profile.ai_enabled {
            return Processed {
                text: corrected,
                observation: None,
                transformations: profile.transformations.clone(),
            };
        }
        let prompt = prompt(profile, &corrected, context);
        let deadline = profile.deadline.unwrap_or(self.deadline);
        let started = Instant::now();
        match generate_cancellable(&prompt, profile.model.as_ref(), deadline, cancelled) {
            Ok(text) if !text.trim().is_empty() => {
                let latency_ms = started.elapsed().as_millis() as u64;
                tracing::info!(
                    profile = profile.name,
                    latency_ms,
                    "dictation post-processing completed"
                );
                Processed {
                    text: text.trim().into(),
                    observation: Some(ProcessingObservation {
                        profile: profile.name.clone(),
                        latency_ms,
                        fallback: None,
                    }),
                    transformations: profile.transformations.clone(),
                }
            }
            Ok(_) => Processed {
                text: corrected.clone(),
                observation: Some(ProcessingObservation {
                    profile: profile.name.clone(),
                    latency_ms: started.elapsed().as_millis() as u64,
                    fallback: Some("processor returned empty text".into()),
                }),
                transformations: profile.transformations.clone(),
            },
            Err(error) => Processed {
                text: corrected,
                observation: Some(ProcessingObservation {
                    profile: profile.name.clone(),
                    latency_ms: started.elapsed().as_millis() as u64,
                    fallback: Some(error.to_string()),
                }),
                transformations: profile.transformations.clone(),
            },
        }
    }

    pub fn process_voice_action_cancellable(
        &self,
        instruction: &str,
        selected_text: Option<&str>,
        context: &ContextSnapshot,
        cancelled: &AtomicBool,
    ) -> Processed {
        let settings = crate::app_settings::voice_action_settings();
        let prompt = voice_action_prompt(instruction, selected_text, context);
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
        match generate_cancellable(&prompt, model.as_ref(), deadline, cancelled) {
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
                    transformations: Vec::new(),
                }
            }
            Ok(_) => voice_action_failure(started, "processor returned empty text".into()),
            Err(error) => voice_action_failure(started, error.to_string()),
        }
    }
}

pub struct Processed {
    pub text: String,
    pub observation: Option<ProcessingObservation>,
    pub transformations: Vec<String>,
}

#[derive(Clone, Debug)]
pub struct ProcessingObservation {
    pub profile: String,
    pub latency_ms: u64,
    pub fallback: Option<String>,
}

fn prompt(profile: &Profile, transcript: &str, context: &ContextSnapshot) -> String {
    let application = context.application.as_deref().unwrap_or("unknown");
    let host = context.browser_host().unwrap_or("none");
    format!(
        "{PROTOCOL_PROMPT}\n\nProfile instructions:\n{}\n\nForeground application: {application}\nBrowser host: {host}\n\nRaw transcript:\n<transcript>\n{transcript}\n</transcript>",
        profile.prompt
    )
}

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
        transformations: Vec::new(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use url::Url;

    fn context(application: &str, url: Option<&str>) -> ContextSnapshot {
        ContextSnapshot {
            application: Some(application.into()),
            browser_url: url.map(|url| Url::parse(url).unwrap()),
            window_title: None,
            selected_text: None,
            input_revision: None,
        }
    }

    fn profiles() -> Profiles {
        Profiles::new(Profile::new("default", "default prompt").ai_enabled(true))
            .application("Slack", Profile::new("slack", "slack prompt"))
            .browser_host("x.com", Profile::new("x", "x prompt"))
    }

    #[test]
    fn browser_host_beats_application_and_application_beats_default() {
        let profiles = profiles();
        assert_eq!(
            profiles
                .select(&context("Brave Browser", Some("https://x.com/home")))
                .name,
            "x"
        );
        assert_eq!(profiles.select(&context("Slack", None)).name, "slack");
        assert_eq!(profiles.select(&context("Zed", None)).name, "default");
    }

    #[test]
    fn prompt_contains_only_current_transcript_and_selected_context() {
        let profile = Profile::new("default", "Rewrite freely.");
        let prompt = prompt(
            &profile,
            "This is the current transcript.",
            &context("Slack", None),
        );
        assert!(prompt.contains("Rewrite freely."));
        assert!(prompt.contains("Foreground application: Slack"));
        assert!(prompt.contains("This is the current transcript."));
    }

    #[test]
    fn voice_action_prompt_keeps_the_instruction_separate_from_selected_text() {
        let prompt = voice_action_prompt(
            "Make this concise.",
            Some("This is the original selected paragraph."),
            &context("Slack", None),
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
        let prompt =
            voice_action_prompt("Write a funny release note.", None, &context("Slack", None));

        assert!(prompt.contains("No text was selected"));
        assert!(prompt.contains("Write a funny release note."));
    }

    #[test]
    fn a_contextual_raw_mode_overrides_processed_default() {
        let profiles = Profiles::new(Profile::new("default", "default prompt").ai_enabled(true))
            .application("Slack", Profile::new("raw", "").ai_enabled(false));

        assert!(!profiles.select(&context("Slack", None)).ai_enabled);
        assert_eq!(profiles.select(&context("Zed", None)).name, "default");
    }

    #[test]
    fn mode_corrections_run_before_registered_transformations() {
        let profile = Profile::new("Global", "")
            .replacements(ReplacementSet::new(&[
                crate::app_settings::TextReplacement {
                    matched_phrase: "open code".into(),
                    output: "OpenCode".into(),
                },
            ]))
            .transformations(vec!["lowercase".into()]);
        let profiles = Profiles::new(profile);

        let processed = profiles.process_cancellable(
            "Use open code.",
            &context("Zed", None),
            &AtomicBool::new(false),
        );

        assert_eq!(processed.text, "Use OpenCode.");
        assert_eq!(processed.transformations, ["lowercase"]);
        assert!(processed.observation.is_none());
    }
}
