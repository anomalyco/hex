//! Ordered, platform-neutral dictation processing.
//!
//! Native shells capture context and own paste/output. This module selects a
//! configured profile and applies its steps in the product order: phrase
//! corrections, optional OpenCode rewriting, then selected transformations.
//! Every failed step preserves the previous step's text.

#![cfg_attr(any(target_os = "linux", target_os = "windows"), allow(dead_code))]

use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, Instant};

use serde::{Deserialize, Serialize};

use super::command_context::{ContextSelector, ContextSnapshot};
use super::opencode::{Model, generate_cancellable};
use super::text_replacements::ReplacementSet;

const PROTOCOL_PROMPT: &str = "You transform dictated speech into replacement text. Return only the text that should be pasted. Do not add an explanation, label, alternative, or Markdown fence.";

/// Persisted OpenCode rewrite settings shared by every desktop mode schema.
/// Context matching and transformation selection remain owned by their native
/// roots, but rewrite behavior must not drift between platforms.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(default)]
pub struct PostProcessingSettings {
    pub enabled: bool,
    pub prompt: String,
    /// Optional `provider/model` key from the OpenCode catalog.
    pub model: Option<String>,
    pub variant: Option<String>,
    pub deadline_seconds: u64,
}

impl Default for PostProcessingSettings {
    fn default() -> Self {
        Self {
            enabled: false,
            prompt: "Rewrite the transcript into clear, natural text while preserving the speaker's intended meaning.".into(),
            model: None,
            variant: None,
            deadline_seconds: 30,
        }
    }
}

impl PostProcessingSettings {
    /// Apply a user-edited deadline without letting a transient invalid field
    /// erase the last persisted value. Every desktop root uses this contract.
    pub fn update_deadline_from_text(&mut self, text: &str) -> bool {
        let Ok(seconds) = text.trim().parse::<u64>() else {
            return false;
        };
        let seconds = seconds.max(1);
        if self.deadline_seconds == seconds {
            return false;
        }
        self.deadline_seconds = seconds;
        true
    }

    /// A variant cannot be sent without a concrete provider/model request.
    /// Pin OpenCode's current default when a user selects a non-default
    /// variant while the profile still follows the default model.
    pub fn set_variant(&mut self, variant: Option<String>, default_model: Option<&str>) {
        if variant.is_some() && self.model.is_none() {
            self.model = default_model.map(str::to_owned);
        }
        self.variant = variant;
    }
}

/// Platform-owned transformation execution. macOS and Windows currently back
/// this with the shared bounded Bun personal-command host; Linux supplies its
/// real host once that capability is implemented.
pub trait TransformationExecutor {
    fn transform(
        &self,
        ids: &[String],
        text: &str,
        context: &ContextSnapshot,
        cancelled: &AtomicBool,
    ) -> Result<String, String>;
}

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

    /// Build one runtime profile from the shared persisted rewrite contract.
    /// Invalid model keys deliberately fall back to OpenCode's default, which
    /// preserves the existing macOS behavior and keeps a stale catalog entry
    /// from disabling corrections or transformations.
    pub(crate) fn configured(
        name: impl Into<String>,
        replacements: ReplacementSet,
        transformations: Vec<String>,
        settings: &PostProcessingSettings,
    ) -> Self {
        let mut profile = Self::new(name, &settings.prompt)
            .ai_enabled(settings.enabled)
            .replacements(replacements)
            .transformations(transformations);
        if let Some((provider, model)) = settings
            .model
            .as_deref()
            .and_then(|key| key.split_once('/'))
        {
            profile = profile.model(provider, model);
            if let Some(variant) = settings
                .variant
                .as_deref()
                .filter(|variant| !variant.is_empty())
            {
                profile = profile.variant(variant);
            }
        }
        profile.deadline(Duration::from_secs(settings.deadline_seconds.max(1)))
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
        transformations: Option<&dyn TransformationExecutor>,
        cancelled: &AtomicBool,
    ) -> Processed {
        self.process_with(
            transcript,
            context,
            transformations,
            cancelled,
            |prompt, model, deadline, cancelled| {
                generate_cancellable(prompt, model, deadline, cancelled)
                    .map_err(|error| error.to_string())
            },
        )
    }

    fn process_with<Rewrite>(
        &self,
        transcript: &str,
        context: &ContextSnapshot,
        transformations: Option<&dyn TransformationExecutor>,
        cancelled: &AtomicBool,
        rewrite: Rewrite,
    ) -> Processed
    where
        Rewrite: FnOnce(&str, Option<&Model>, Duration, &AtomicBool) -> Result<String, String>,
    {
        let profile = self.select(context);
        let corrected = profile.replacements.replace(transcript);
        let mut processed = if profile.ai_enabled {
            let prompt = prompt(profile, &corrected, context);
            let deadline = profile.deadline.unwrap_or(self.deadline);
            let started = Instant::now();
            match rewrite(&prompt, profile.model.as_ref(), deadline, cancelled) {
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
                    }
                }
                Ok(_) => Processed {
                    text: corrected.clone(),
                    observation: Some(ProcessingObservation {
                        profile: profile.name.clone(),
                        latency_ms: started.elapsed().as_millis() as u64,
                        fallback: Some("processor returned empty text".into()),
                    }),
                },
                Err(error) => Processed {
                    text: corrected,
                    observation: Some(ProcessingObservation {
                        profile: profile.name.clone(),
                        latency_ms: started.elapsed().as_millis() as u64,
                        fallback: Some(error),
                    }),
                },
            }
        } else {
            Processed {
                text: corrected,
                observation: None,
            }
        };

        if profile.transformations.is_empty() || cancelled.load(Ordering::Acquire) {
            return processed;
        }

        let started = Instant::now();
        let transformed = transformations
            .ok_or_else(|| "custom transformations are unavailable".to_string())
            .and_then(|executor| {
                executor.transform(
                    &profile.transformations,
                    &processed.text,
                    context,
                    cancelled,
                )
            });
        let latency_ms = started.elapsed().as_millis() as u64;
        let observation = processed
            .observation
            .get_or_insert_with(|| ProcessingObservation {
                profile: "Custom transformations".into(),
                latency_ms: 0,
                fallback: None,
            });
        observation.latency_ms = observation.latency_ms.saturating_add(latency_ms);
        match transformed {
            Ok(text) => processed.text = text,
            Err(error) => observation.fallback = Some(error),
        }
        processed
    }
}

pub struct Processed {
    pub text: String,
    pub observation: Option<ProcessingObservation>,
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

#[cfg(test)]
mod tests {
    use super::super::text_replacements::TextReplacement;
    use super::*;
    use url::Url;

    struct FakeTransform {
        expected: String,
        result: Result<String, String>,
    }

    impl TransformationExecutor for FakeTransform {
        fn transform(
            &self,
            ids: &[String],
            text: &str,
            _context: &ContextSnapshot,
            _cancelled: &AtomicBool,
        ) -> Result<String, String> {
            assert_eq!(ids, ["lowercase"]);
            assert_eq!(text, self.expected);
            self.result.clone()
        }
    }

    fn context(application: &str, url: Option<&str>) -> ContextSnapshot {
        ContextSnapshot {
            application: Some(application.into()),
            browser_url: url.map(|url| Url::parse(url).unwrap()),
            ..ContextSnapshot::default()
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
    fn a_contextual_raw_mode_overrides_processed_default() {
        let profiles = Profiles::new(Profile::new("default", "default prompt").ai_enabled(true))
            .application("Slack", Profile::new("raw", "").ai_enabled(false));

        assert!(!profiles.select(&context("Slack", None)).ai_enabled);
        assert_eq!(profiles.select(&context("Zed", None)).name, "default");
    }

    #[test]
    fn persisted_rewrite_settings_build_the_same_runtime_profile_everywhere() {
        let settings = PostProcessingSettings {
            enabled: true,
            prompt: "Keep technical terms exact.".into(),
            model: Some("openai/gpt-5.6-sol".into()),
            variant: Some("fast".into()),
            deadline_seconds: 12,
        };
        let profile = Profile::configured(
            "Coding",
            ReplacementSet::default(),
            vec!["lowercase".into()],
            &settings,
        );

        assert!(profile.ai_enabled);
        assert_eq!(profile.prompt, "Keep technical terms exact.");
        assert_eq!(
            profile.model,
            Some(Model {
                provider: "openai".into(),
                id: "gpt-5.6-sol".into(),
                variant: Some("fast".into()),
            })
        );
        assert_eq!(profile.deadline, Some(Duration::from_secs(12)));
        assert_eq!(profile.transformations, ["lowercase"]);
    }

    #[test]
    fn deadline_edits_preserve_invalid_values_and_clamp_zero() {
        let mut settings = PostProcessingSettings::default();

        assert!(!settings.update_deadline_from_text("not a number"));
        assert_eq!(settings.deadline_seconds, 30);
        assert!(settings.update_deadline_from_text("0"));
        assert_eq!(settings.deadline_seconds, 1);
        assert!(settings.update_deadline_from_text(" 15 "));
        assert_eq!(settings.deadline_seconds, 15);
        assert!(!settings.update_deadline_from_text("15"));
    }

    #[test]
    fn selecting_a_variant_pins_the_current_default_model() {
        let mut settings = PostProcessingSettings::default();

        settings.set_variant(Some("high".into()), Some("openai/default"));
        assert_eq!(settings.model.as_deref(), Some("openai/default"));
        assert_eq!(settings.variant.as_deref(), Some("high"));
        settings.set_variant(None, Some("anthropic/new-default"));
        assert_eq!(settings.model.as_deref(), Some("openai/default"));
        assert!(settings.variant.is_none());
    }

    #[test]
    fn corrections_rewrite_and_transformations_run_in_order() {
        let profile = Profile::new("Global", "Rewrite precisely.")
            .ai_enabled(true)
            .replacements(ReplacementSet::new(&[TextReplacement {
                matched_phrase: "open code".into(),
                output: "OpenCode".into(),
            }]))
            .transformations(vec!["lowercase".into()]);
        let profiles = Profiles::new(profile);
        let transformation = FakeTransform {
            expected: "Rewritten OpenCode.".into(),
            result: Ok("rewritten opencode.".into()),
        };

        let processed = profiles.process_with(
            "Use open code.",
            &context("Zed", None),
            Some(&transformation),
            &AtomicBool::new(false),
            |prompt, _model, _deadline, _cancelled| {
                assert!(prompt.contains("<transcript>\nUse OpenCode.\n</transcript>"));
                Ok("Rewritten OpenCode.".into())
            },
        );

        assert_eq!(processed.text, "rewritten opencode.");
        let observation = processed.observation.unwrap();
        assert_eq!(observation.profile, "Global");
        assert!(observation.fallback.is_none());
    }

    #[test]
    fn a_failed_transformation_preserves_the_rewrite_output() {
        let profile = Profile::new("Global", "Rewrite precisely.")
            .ai_enabled(true)
            .transformations(vec!["lowercase".into()]);
        let profiles = Profiles::new(profile);
        let transformation = FakeTransform {
            expected: "Rewritten output.".into(),
            result: Err("transformation failed".into()),
        };

        let processed = profiles.process_with(
            "Raw output.",
            &ContextSnapshot::default(),
            Some(&transformation),
            &AtomicBool::new(false),
            |_prompt, _model, _deadline, _cancelled| Ok("Rewritten output.".into()),
        );

        assert_eq!(processed.text, "Rewritten output.");
        assert_eq!(
            processed.observation.unwrap().fallback.as_deref(),
            Some("transformation failed")
        );
    }
}
