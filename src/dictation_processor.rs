use std::collections::HashSet;
use std::sync::atomic::{AtomicBool, Ordering};
use std::thread;
use std::time::{Duration, Instant};

use color_eyre::eyre::{Result, WrapErr, eyre};
use serde::{Deserialize, Serialize};

use crate::context::{ContextSelector, ContextSnapshot};
use crate::opencode_service::Service;
use crate::text_replacements::ReplacementSet;
use crate::vocabulary::VocabularyEntry;

const PROTOCOL_PROMPT: &str = "You transform dictated speech into replacement text. Return only the text that should be pasted. Do not add an explanation, label, alternative, or Markdown fence.";
const CATALOG_DEADLINE: Duration = Duration::from_secs(10);
/// Newest one-shot generation route first; older OpenCode V2 builds only serve the later name.
const GENERATE_ENDPOINTS: [&str; 2] = ["/api/experimental/generate", "/api/generate"];

pub use crate::app_settings::DictationPostProcessing as PostProcessingSettings;

#[derive(Clone)]
pub struct Profile {
    name: String,
    ai_enabled: bool,
    prompt: String,
    model: Option<Model>,
    deadline: Duration,
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
            deadline: Duration::from_secs(30),
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
        self.deadline = deadline;
        self
    }

    pub(crate) fn from_mode(mode: &crate::app_settings::DictationMode) -> Self {
        let name = if mode.name.trim().is_empty() {
            "Untitled mode"
        } else {
            &mode.name
        };
        Self::configured(
            name,
            ReplacementSet::new(&mode.replacements),
            mode.transformations.clone(),
            &mode.post_processing,
        )
    }

    /// Builds a runtime profile from persisted settings. A malformed model key
    /// falls back to the default model rather than disabling the mode.
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

#[derive(Clone, Debug, PartialEq, Eq, Deserialize, Serialize)]
pub struct Model {
    #[serde(rename = "providerID")]
    pub provider: String,
    pub id: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub variant: Option<String>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct RecommendedModel {
    pub id: &'static str,
    pub preferred_variants: &'static [&'static str],
}

/// Ranked fast, 100%-protocol-compliant OpenCode models for dictation
/// rewriting, ordered by latency, cost, and strict paste-ready output
/// compliance.
pub const RECOMMENDED_OPENCODE_MODELS: &[RecommendedModel] = &[
    RecommendedModel {
        id: "gemini-3.1-flash-lite",
        preferred_variants: &["minimal", "none", "low"],
    },
    RecommendedModel {
        id: "claude-haiku-4-5",
        preferred_variants: &["none", "minimal", "low"],
    },
    RecommendedModel {
        id: "gpt-5.4-nano",
        preferred_variants: &["none", "minimal", "low"],
    },
    RecommendedModel {
        id: "gpt-4.1-mini",
        preferred_variants: &["none", "minimal", "low"],
    },
    RecommendedModel {
        id: "gpt-4.1-nano",
        preferred_variants: &["none", "minimal", "low"],
    },
    RecommendedModel {
        id: "gpt-5.4-mini",
        preferred_variants: &["none", "minimal", "low"],
    },
    RecommendedModel {
        id: "gemini-3-flash",
        preferred_variants: &["minimal", "none", "low"],
    },
    RecommendedModel {
        id: "gpt-4o-mini",
        preferred_variants: &["none", "minimal", "low"],
    },
    RecommendedModel {
        id: "gpt-4.1",
        preferred_variants: &["none", "minimal", "low"],
    },
    RecommendedModel {
        id: "gpt-5.4",
        preferred_variants: &["none", "minimal", "low"],
    },
    RecommendedModel {
        id: "claude-sonnet-4-6",
        preferred_variants: &["none", "minimal", "low"],
    },
    RecommendedModel {
        id: "claude-sonnet-4-5",
        preferred_variants: &["none", "minimal", "low"],
    },
    RecommendedModel {
        id: "gemini-2.5-flash-lite",
        preferred_variants: &["none", "minimal", "low"],
    },
    RecommendedModel {
        id: "gemini-2.5-flash",
        preferred_variants: &["none", "minimal", "low"],
    },
];

#[derive(Clone)]
struct ContextualProfile {
    selector: ContextSelector,
    profile: Profile,
}

#[derive(Clone)]
pub struct Profiles {
    default: Profile,
    contextual: Vec<ContextualProfile>,
}

impl Profiles {
    pub fn new(default: Profile) -> Self {
        Self {
            default,
            contextual: Vec::new(),
        }
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
        let prompt = prompt(profile, &corrected, context, &crate::vocabulary::entries());
        let (text, observation) = generate_observed(
            &prompt,
            profile.model.as_ref(),
            profile.deadline,
            cancelled,
            &profile.name,
            Instant::now(),
        );
        if observation.fallback.is_none() {
            tracing::info!(
                profile = profile.name,
                latency_ms = observation.latency_ms,
                "dictation post-processing completed"
            );
        }
        Processed {
            text: text.unwrap_or(corrected),
            observation: Some(observation),
            transformations: profile.transformations.clone(),
        }
    }
}

/// One generation from OpenCode's default ranked models, for work outside
/// dictation such as meeting notes.
pub(crate) fn generate_text(
    prompt: &str,
    deadline: Duration,
    cancelled: &AtomicBool,
) -> Result<String> {
    generate_cancellable(prompt, None, deadline, cancelled)
}

/// Runs one generation and reports it as a processing observation. The text is
/// `None` whenever the caller must fall back, with the reason recorded in the
/// observation.
fn generate_observed(
    prompt: &str,
    model: Option<&Model>,
    deadline: Duration,
    cancelled: &AtomicBool,
    profile_name: &str,
    started: Instant,
) -> (Option<String>, ProcessingObservation) {
    let (text, fallback) = match generate_cancellable(prompt, model, deadline, cancelled) {
        Ok(text) if !text.trim().is_empty() => (Some(text.trim().into()), None),
        Ok(_) => (None, Some("processor returned empty text".into())),
        Err(error) => (None, Some(error.to_string())),
    };
    let observation = ProcessingObservation {
        profile: profile_name.into(),
        latency_ms: started.elapsed().as_millis() as u64,
        fallback,
    };
    (text, observation)
}

#[derive(Clone, Debug)]
pub struct ModelChoice {
    pub key: String,
    pub name: String,
    pub provider: String,
    pub variants: Vec<String>,
}

#[derive(Clone, Debug)]
pub struct ModelCatalog {
    pub models: Vec<ModelChoice>,
    pub default_key: Option<String>,
    pub default_name: Option<String>,
    pub default_variant: Option<String>,
}

impl ModelChoice {
    /// The request-shaped model this catalog entry selects.
    pub fn model(&self) -> Model {
        let (provider, id) = self
            .key
            .split_once('/')
            .unwrap_or((self.provider.as_str(), self.key.as_str()));
        Model {
            provider: provider.into(),
            id: id.into(),
            variant: None,
        }
    }
}

impl ModelCatalog {
    pub fn recommended_variant_for(&self, key: &str) -> Option<&str> {
        let choice = self.models.iter().find(|choice| choice.key == key)?;
        recommended_variant_for_choice(choice)
    }

    pub fn ranked_candidates(&self, variant_override: Option<&str>) -> Vec<Model> {
        ranked_choice_indices(&self.models)
            .into_iter()
            .filter_map(|index| {
                let choice = &self.models[index];
                let (provider, id) = choice.key.split_once('/')?;
                let variant = variant_override
                    .filter(|v| choice.variants.iter().any(|candidate| candidate == *v))
                    .or_else(|| recommended_variant_for_choice(choice))
                    .map(str::to_owned);
                Some(Model {
                    provider: provider.into(),
                    id: id.into(),
                    variant,
                })
            })
            .collect()
    }
}

fn leaf_model_id(key: &str) -> &str {
    key.rsplit_once('/').map_or(key, |(_, id)| id)
}

fn matches_recommended_model_id(leaf: &str, recommended_id: &str) -> bool {
    if leaf == recommended_id {
        return true;
    }
    let Some(suffix) = leaf.strip_prefix(recommended_id) else {
        return false;
    };
    suffix == "-preview"
        || suffix == "-latest"
        || (suffix.starts_with("-20") && suffix[1..].bytes().all(|byte| byte.is_ascii_digit()))
}

fn recommendation_entry_for_key(key: &str) -> Option<(usize, &'static RecommendedModel)> {
    let leaf = leaf_model_id(key);
    RECOMMENDED_OPENCODE_MODELS
        .iter()
        .enumerate()
        .find(|(_, rec)| matches_recommended_model_id(leaf, rec.id))
}

fn recommended_variant_for_choice(choice: &ModelChoice) -> Option<&str> {
    let (_, rec) = recommendation_entry_for_key(&choice.key)?;
    rec.preferred_variants
        .iter()
        .copied()
        .find(|preferred| choice.variants.iter().any(|v| v == *preferred))
}

fn provider_priority(provider: &str) -> usize {
    match provider {
        "google" | "anthropic" | "openai" => 0,
        "opencode" => 1,
        "github-copilot" => 2,
        p if p.starts_with("console-") => 3,
        _ => 4,
    }
}

fn ranked_choice_indices(choices: &[ModelChoice]) -> Vec<usize> {
    let mut ranked = Vec::new();
    for rec in RECOMMENDED_OPENCODE_MODELS {
        if let Some((best_idx, _)) = choices
            .iter()
            .enumerate()
            .filter(|(_, choice)| matches_recommended_model_id(leaf_model_id(&choice.key), rec.id))
            .min_by_key(|(idx, choice)| (provider_priority(&choice.provider), *idx))
            && !ranked.contains(&best_idx)
        {
            ranked.push(best_idx);
        }
    }
    ranked
}

#[derive(Deserialize)]
struct ModelsResponse {
    data: Vec<ModelInfo>,
}

#[derive(Deserialize)]
struct DefaultModelResponse {
    data: Option<ModelInfo>,
}

#[derive(Clone, Deserialize)]
struct ModelInfo {
    id: String,
    #[serde(rename = "providerID")]
    provider_id: String,
    name: String,
    enabled: bool,
    capabilities: ModelCapabilities,
    variants: Vec<ModelVariant>,
}

#[derive(Clone, Deserialize)]
struct ModelCapabilities {
    output: Vec<String>,
}

#[derive(Clone, Deserialize)]
struct ModelVariant {
    id: String,
}

fn model_is_available(model: &ModelInfo) -> bool {
    model.enabled
        && model
            .capabilities
            .output
            .iter()
            .any(|output| output == "text")
}

/// The ranked default candidates for the service at `address`, cached with
/// the last catalog so a rewrite doesn't fetch it every time.
static CACHED_RANKED_CANDIDATES: std::sync::Mutex<Option<(Service, Vec<Model>)>> =
    std::sync::Mutex::new(None);

fn cache_ranked_candidates(service: &Service, catalog: &ModelCatalog) {
    if let Ok(mut guard) = CACHED_RANKED_CANDIDATES.lock() {
        *guard = Some((service.clone(), catalog.ranked_candidates(None)));
    }
}

fn ranked_candidates_for(service: &Service) -> Vec<Model> {
    if let Ok(guard) = CACHED_RANKED_CANDIDATES.lock()
        && let Some((cached, candidates)) = guard.as_ref()
        && cached == service
    {
        return candidates.clone();
    }
    let Ok(models) = opencode_api::<ModelsResponse>(service, "/api/model") else {
        return Vec::new();
    };
    let catalog = build_model_catalog(models.data, None);
    cache_ranked_candidates(service, &catalog);
    catalog.ranked_candidates(None)
}

/// The running service's model catalog. Reads OpenCode's registration and
/// talks loopback HTTP; never starts anything. Call off the UI thread.
pub fn load_model_catalog() -> Result<Option<ModelCatalog>> {
    let Some(service) = crate::opencode_service::find() else {
        return Ok(None);
    };
    let models: ModelsResponse = opencode_api(&service, "/api/model")?;
    let default: DefaultModelResponse = opencode_api(&service, "/api/model/default")?;
    let catalog = build_model_catalog(models.data, default.data);
    cache_ranked_candidates(&service, &catalog);
    Ok(Some(catalog))
}


fn build_model_catalog(models: Vec<ModelInfo>, default: Option<ModelInfo>) -> ModelCatalog {
    let fallback_default_key = default
        .as_ref()
        .map(|model| format!("{}/{}", model.provider_id, model.id));
    let fallback_default_name = default.as_ref().map(|model| model.name.clone());
    let mut seen = HashSet::new();
    let raw_choices: Vec<ModelChoice> = models
        .into_iter()
        .chain(default)
        .filter(model_is_available)
        .filter_map(|model| {
            let key = format!("{}/{}", model.provider_id, model.id);
            seen.insert(key.clone()).then_some(ModelChoice {
                key,
                name: model.name,
                provider: model.provider_id,
                variants: model
                    .variants
                    .into_iter()
                    .map(|variant| variant.id)
                    .collect(),
            })
        })
        .collect();
    let ranked_indices = ranked_choice_indices(&raw_choices);
    let mut choices = Vec::with_capacity(raw_choices.len());
    for &idx in &ranked_indices {
        choices.push(raw_choices[idx].clone());
    }
    for (idx, choice) in raw_choices.into_iter().enumerate() {
        if !ranked_indices.contains(&idx) {
            choices.push(choice);
        }
    }
    let (default_key, default_name, default_variant) = if !ranked_indices.is_empty()
        && let Some(top) = choices.first()
    {
        (
            Some(top.key.clone()),
            Some(top.name.clone()),
            recommended_variant_for_choice(top).map(str::to_owned),
        )
    } else {
        (fallback_default_key, fallback_default_name, None)
    };
    ModelCatalog {
        default_key,
        default_name,
        default_variant,
        models: choices,
    }
}

fn decode_api_response<T: for<'de> Deserialize<'de>>(path: &str, body: &[u8]) -> Result<T> {
    let value: serde_json::Value = serde_json::from_slice(body)
        .wrap_err_with(|| format!("OpenCode {path} returned invalid JSON"))?;
    if let Some(message) = value.get("message").and_then(|message| message.as_str())
        && value.get("_tag").is_some()
    {
        return Err(eyre!("OpenCode {path} failed: {message}"));
    }
    serde_json::from_value(value)
        .wrap_err_with(|| format!("OpenCode {path} returned an invalid response"))
}

fn opencode_api<T: for<'de> Deserialize<'de>>(service: &Service, path: &str) -> Result<T> {
    let workspace = crate::app_paths::opencode_workspace()?;
    let response = service.request(
        "GET",
        path,
        None,
        Some(&workspace),
        CATALOG_DEADLINE,
        &AtomicBool::new(false),
    )?;
    if !(200..300).contains(&response.status) {
        return Err(eyre!("OpenCode {path} answered HTTP {}", response.status));
    }
    decode_api_response(path, &response.body)
}


pub struct Processed {
    pub text: String,
    pub observation: Option<ProcessingObservation>,
    pub transformations: Vec<String>,
}

pub type ProcessingObservation = crate::events::DictationProcessing;

impl Processed {
    /// Runs the selected transformations, the last processing stage. The chain
    /// is one stage: a failure keeps the previous text and records the error.
    pub fn apply_transformations(
        &mut self,
        client: &crate::personal_commands::TransformationClient,
        context: &ContextSnapshot,
        cancelled: &AtomicBool,
    ) {
        if self.transformations.is_empty() || cancelled.load(Ordering::Acquire) {
            return;
        }
        let started = Instant::now();
        let transformed = client.transform(&self.transformations, &self.text, context, cancelled);
        let observation = self
            .observation
            .get_or_insert_with(|| ProcessingObservation {
                profile: "Custom transformations".into(),
                latency_ms: 0,
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

fn prompt(
    profile: &Profile,
    transcript: &str,
    context: &ContextSnapshot,
    vocabulary: &[VocabularyEntry],
) -> String {
    let application = context.application.as_deref().unwrap_or("unknown");
    let host = context.browser_host().unwrap_or("none");
    format!(
        "{PROTOCOL_PROMPT}\n\nProfile instructions:\n{}\n\n{}Foreground application: {application}\nBrowser host: {host}\n\nRaw transcript:\n<transcript>\n{transcript}\n</transcript>",
        profile.prompt,
        vocabulary_lines(vocabulary)
    )
}

/// The vocabulary as prompt lines, so a rewrite spells its terms right;
/// nothing when it is empty (or its experiment is off).
fn vocabulary_lines(vocabulary: &[VocabularyEntry]) -> String {
    if vocabulary.is_empty() {
        return String::new();
    }
    let terms = vocabulary
        .iter()
        .map(|entry| match entry.sounds_like.as_slice() {
            [] => entry.term.clone(),
            spellings => format!(
                "{} (may be transcribed as {})",
                entry.term,
                spellings.join(", ")
            ),
        })
        .collect::<Vec<_>>()
        .join("; ");
    format!("Vocabulary (spell these exactly when the speaker means them): {terms}\n\n")
}

#[derive(Serialize)]
struct Request<'a> {
    prompt: &'a str,
    #[serde(skip_serializing_if = "Option::is_none")]
    model: Option<&'a Model>,
}

#[derive(Deserialize)]
#[serde(untagged)]
enum Response {
    Success { data: ResponseData },
    Error { message: String },
}

#[derive(Deserialize)]
struct ResponseData {
    text: String,
}

#[cfg(test)]
fn generate(prompt: &str, model: Option<&Model>, deadline: Duration) -> Result<String> {
    generate_cancellable(prompt, model, deadline, &AtomicBool::new(false))
}

fn generate_cancellable(
    prompt: &str,
    model: Option<&Model>,
    deadline: Duration,
    cancelled: &AtomicBool,
) -> Result<String> {
    let started = Instant::now();
    let service = crate::opencode_service::find()
        .ok_or_else(|| eyre!(crate::opencode_service::NOT_RUNNING))?;
    generate_with_candidates(
        prompt,
        model,
        || ranked_candidates_for(&service),
        started,
        deadline,
        cancelled,
        |data, remaining, cancelled| post_generation(&service, data, remaining, cancelled),
    )
}


fn generate_with_candidates(
    prompt: &str,
    model: Option<&Model>,
    candidates: impl FnOnce() -> Vec<Model>,
    started: Instant,
    deadline: Duration,
    cancelled: &AtomicBool,
    post: impl FnMut(&str, Duration, &AtomicBool) -> Result<Response>,
) -> Result<String> {
    if model.is_none() {
        let candidates = candidates();
        if !candidates.is_empty() {
            return generate_with_ranked_candidates(
                prompt,
                &candidates,
                started,
                deadline,
                cancelled,
                post,
            );
        }
    }
    generate_single_model(prompt, model, started, deadline, cancelled, post)
}

fn remaining_generation_deadline(started: Instant, deadline: Duration) -> Result<Duration> {
    let remaining = deadline.saturating_sub(started.elapsed());
    if remaining.is_zero() {
        Err(eyre!(
            "OpenCode generation exceeded {} seconds",
            deadline.as_secs()
        ))
    } else {
        Ok(remaining)
    }
}

fn generate_with_ranked_candidates(
    prompt: &str,
    candidates: &[Model],
    started: Instant,
    deadline: Duration,
    cancelled: &AtomicBool,
    mut post: impl FnMut(&str, Duration, &AtomicBool) -> Result<Response>,
) -> Result<String> {
    let mut last_error = None;
    for (index, candidate) in candidates.iter().enumerate() {
        let remaining = remaining_generation_deadline(started, deadline)?;
        let data = serde_json::to_string(&Request {
            prompt,
            model: Some(candidate),
        })?;
        match post(&data, remaining, cancelled)? {
            Response::Success { data } => return Ok(data.text),
            Response::Error { message } => {
                if index + 1 < candidates.len() {
                    tracing::warn!(
                        provider = %candidate.provider,
                        model = %candidate.id,
                        %message,
                        "ranked OpenCode model failed; trying next available candidate"
                    );
                }
                last_error = Some(eyre!("OpenCode generation failed: {message}"));
            }
        }
    }
    Err(last_error.unwrap_or_else(|| eyre!("no OpenCode models available")))
}

fn generate_single_model(
    prompt: &str,
    model: Option<&Model>,
    started: Instant,
    deadline: Duration,
    cancelled: &AtomicBool,
    mut post: impl FnMut(&str, Duration, &AtomicBool) -> Result<Response>,
) -> Result<String> {
    let request = Request { prompt, model };
    let data = serde_json::to_string(&request)?;
    for attempt in 0..2 {
        let remaining = remaining_generation_deadline(started, deadline)?;
        match post(&data, remaining, cancelled)? {
            Response::Success { data } => return Ok(data.text),
            Response::Error { message } if attempt == 0 && retryable_generation_error(&message) => {
                tracing::warn!(%message, "OpenCode model unavailable during startup; retrying");
                wait_for_generation_retry(started, deadline, cancelled)?;
            }
            Response::Error { message } => {
                return Err(eyre!("OpenCode generation failed: {message}"));
            }
        }
    }
    unreachable!("generation loop returns on its second attempt")
}

/// Posts to the newest generation route and falls back to the older name only
/// when the server reports that route missing. Every attempt shares one deadline.
fn post_generation(
    service: &Service,
    data: &str,
    deadline: Duration,
    cancelled: &AtomicBool,
) -> Result<Response> {
    let started = Instant::now();
    let mut response = None;
    for path in GENERATE_ENDPOINTS {
        let attempt = service.request(
            "POST",
            path,
            Some(data.as_bytes()),
            None,
            deadline.saturating_sub(started.elapsed()),
            cancelled,
        )?;
        let missing = attempt.status == 404;
        response = Some(attempt);
        if !missing {
            break;
        }
    }
    let response = response.expect("at least one generation route");
    if response.status == 404 {
        return Err(eyre!("OpenCode does not serve a generation route"));
    }
    serde_json::from_slice(&response.body).wrap_err("OpenCode returned an invalid generation response")
}


fn retryable_generation_error(message: &str) -> bool {
    message.starts_with("Model unavailable:")
}

fn wait_for_generation_retry(
    started: Instant,
    deadline: Duration,
    cancelled: &AtomicBool,
) -> Result<()> {
    let wait_started = Instant::now();
    while wait_started.elapsed() < Duration::from_millis(1_500) {
        if cancelled.load(Ordering::Acquire) {
            return Err(eyre!("OpenCode generation was cancelled"));
        }
        if started.elapsed() >= deadline {
            return Err(eyre!(
                "OpenCode generation exceeded {} seconds",
                deadline.as_secs()
            ));
        }
        thread::sleep(Duration::from_millis(20));
    }
    Ok(())
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
            input_revision: None,
        }
    }

    fn profiles() -> Profiles {
        Profiles::new(Profile::new("default", "default prompt").ai_enabled(true))
            .application("Slack", Profile::new("slack", "slack prompt"))
            .browser_host("x.com", Profile::new("x", "x prompt"))
    }

    #[test]
    fn model_catalog_uses_wire_catalog_ids_and_preserves_aliases() {
        let wire_models = ["rewrite-fast", "rewrite-careful", "rewrite-fast"].map(|id| {
            serde_json::json!({
                "id": id,
                "modelID": "shared-deployment",
                "providerID": "example",
                "name": id,
                "enabled": true,
                "status": "active",
                "capabilities": { "tools": false, "input": ["text"], "output": ["text"] },
                "variants": [{ "id": "high" }],
                "time": { "released": 0 },
                "cost": [],
                "limit": { "context": 10000, "output": 1000 }
            })
        });
        let models: ModelsResponse =
            serde_json::from_value(serde_json::json!({ "data": wire_models })).unwrap();
        let mut default: DefaultModelResponse =
            serde_json::from_value(serde_json::json!({ "data": wire_models[1] })).unwrap();
        let default_model = default.data.as_mut().unwrap();
        default_model.name = "Default display name".into();
        default_model.variants.clear();
        let catalog = build_model_catalog(models.data, default.data);

        assert_eq!(catalog.models.len(), 2);
        assert_eq!(
            catalog.default_name.as_deref(),
            Some("Default display name")
        );
        assert_eq!(
            catalog.default_key.as_deref(),
            Some("example/rewrite-careful")
        );
        for (choice, expected_id) in catalog
            .models
            .iter()
            .zip(["rewrite-fast", "rewrite-careful"])
        {
            assert_eq!(choice.key, format!("example/{expected_id}"));
            assert_eq!(choice.name, expected_id);
            assert_eq!(choice.variants, ["high"]);
            let (provider, id) = choice.key.split_once('/').unwrap();
            let model = Model {
                provider: provider.into(),
                id: id.into(),
                variant: Some("high".into()),
            };
            let request = serde_json::to_value(Request {
                prompt: "Rewrite this.",
                model: Some(&model),
            })
            .unwrap();
            assert_eq!(
                request["model"],
                serde_json::json!({
                    "providerID": "example", "id": expected_id, "variant": "high"
                })
            );
        }

        let mut missing_catalog_id = wire_models[0].clone();
        missing_catalog_id.as_object_mut().unwrap().remove("id");
        assert!(serde_json::from_value::<ModelInfo>(missing_catalog_id).is_err());
    }

    #[test]
    fn model_catalog_keeps_an_available_default_missing_from_the_model_list() {
        let default: DefaultModelResponse = serde_json::from_value(serde_json::json!({
            "data": {
                "id": "rewrite-default",
                "modelID": "shared-deployment",
                "providerID": "example",
                "name": "Example Rewrite",
                "enabled": true,
                "status": "beta",
                "capabilities": { "tools": false, "input": ["text"], "output": ["text"] },
                "variants": [{ "id": "high" }],
                "time": { "released": 0 },
                "cost": [],
                "limit": { "context": 10000, "output": 1000 }
            }
        }))
        .unwrap();
        let model = default.data.unwrap();
        let catalog = build_model_catalog(Vec::new(), Some(model.clone()));

        assert_eq!(catalog.models.len(), 1);
        assert_eq!(catalog.models[0].key, "example/rewrite-default");
        assert_eq!(catalog.models[0].variants, ["high"]);
        assert_eq!(
            catalog.default_key.as_deref(),
            Some("example/rewrite-default")
        );
        assert_eq!(catalog.default_name.as_deref(), Some("Example Rewrite"));

        for unavailable in [
            ModelInfo {
                enabled: false,
                ..model.clone()
            },
            ModelInfo {
                capabilities: ModelCapabilities {
                    output: vec!["image".into()],
                },
                ..model
            },
        ] {
            let catalog = build_model_catalog(Vec::new(), Some(unavailable));
            assert!(catalog.models.is_empty());
            assert_eq!(
                catalog.default_key.as_deref(),
                Some("example/rewrite-default")
            );
            assert_eq!(catalog.default_name.as_deref(), Some("Example Rewrite"));
        }
    }

    #[test]
    fn model_catalog_keeps_available_text_models_regardless_of_release_status() {
        let wire_models: Vec<_> = [
            ("active", "active", true, "text"),
            ("alpha", "alpha", true, "text"),
            ("beta", "beta", true, "text"),
            ("deprecated", "deprecated", true, "text"),
            ("disabled", "active", false, "text"),
            ("image", "active", true, "image"),
        ]
        .into_iter()
        .map(|(id, status, enabled, output)| {
            serde_json::json!({
                "id": id,
                "modelID": id,
                "providerID": "example",
                "name": id,
                "enabled": enabled,
                "status": status,
                "capabilities": { "tools": false, "input": ["text"], "output": [output] },
                "variants": [],
                "time": { "released": 0 },
                "cost": [],
                "limit": { "context": 10000, "output": 1000 }
            })
        })
        .collect();
        let models: ModelsResponse =
            serde_json::from_value(serde_json::json!({ "data": wire_models })).unwrap();
        let catalog = build_model_catalog(models.data, None);

        assert_eq!(
            catalog
                .models
                .iter()
                .map(|choice| choice.key.as_str())
                .collect::<Vec<_>>(),
            [
                "example/active",
                "example/alpha",
                "example/beta",
                "example/deprecated"
            ]
        );
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
            &[],
        );
        assert!(prompt.contains("Rewrite freely."));
        assert!(prompt.contains("Foreground application: Slack"));
        assert!(prompt.contains("This is the current transcript."));
        assert!(!prompt.contains("Vocabulary"));
    }

    #[test]
    fn prompt_names_vocabulary_terms_and_their_mistranscriptions() {
        let vocabulary = [
            VocabularyEntry {
                term: "abattoir".into(),
                sounds_like: vec!["avatar".into(), "a bat war".into()],
            },
            VocabularyEntry {
                term: "Rune".into(),
                sounds_like: vec![],
            },
        ];
        let prompt = prompt(
            &Profile::new("default", "Rewrite freely."),
            "Ship the avatar change.",
            &context("Slack", None),
            &vocabulary,
        );
        assert!(prompt.contains(
            "Vocabulary (spell these exactly when the speaker means them): abattoir (may be \
             transcribed as avatar, a bat war); Rune\n\nForeground application: Slack"
        ));
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
                crate::text_replacements::TextReplacement {
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

    #[test]
    fn only_model_unavailability_is_retried() {
        assert!(retryable_generation_error(
            "Model unavailable: example/rewrite-model"
        ));
        assert!(!retryable_generation_error("Provider request failed"));
        assert!(!retryable_generation_error("processor returned empty text"));
    }

    #[test]
    fn generation_retry_wait_observes_cancellation() {
        let cancelled = AtomicBool::new(true);
        let started = Instant::now();

        let error = wait_for_generation_retry(started, Duration::from_secs(30), &cancelled)
            .expect_err("cancelled retry unexpectedly waited");

        assert!(error.to_string().contains("was cancelled"));
        assert!(started.elapsed() < Duration::from_secs(1));
    }

    /// The service fixture answering each `(path, status, body)` POST in order.
    fn generation_server(
        routes: Vec<(&'static str, u16, &'static str)>,
    ) -> (Service, thread::JoinHandle<Vec<Vec<u8>>>) {
        let (url, server) = crate::opencode_service::tests::serve(
            routes
                .into_iter()
                .map(|(path, status, body)| (format!("POST {path}"), status, body.to_owned()))
                .collect(),
        );
        let server = thread::spawn(move || {
            server
                .join()
                .unwrap()
                .into_iter()
                .map(|(_, body)| body)
                .collect()
        });
        (crate::opencode_service::tests::fixture(&url), server)
    }

    #[test]
    fn generation_falls_back_to_the_legacy_route_only_when_the_new_one_is_missing() {
        let success = r#"{"data":{"text":"fixture result"}}"#;
        let unavailable = r#"{"_tag":"InvalidRequestError","message":"Model unavailable: x"}"#;
        for (routes, expected) in [
            (
                vec![
                    ("/api/experimental/generate", 404, ""),
                    ("/api/generate", 200, success),
                ],
                Ok("fixture result"),
            ),
            (
                vec![("/api/experimental/generate", 400, unavailable)],
                Err("Model unavailable: x"),
            ),
            (
                vec![
                    (
                        "/api/experimental/generate",
                        404,
                        r#"{"error":"Not Found"}"#,
                    ),
                    ("/api/generate", 404, ""),
                ],
                Err("does not serve a generation route"),
            ),
        ] {
            let requests = routes.len();
            let (service, server) = generation_server(routes);
            let result = post_generation(
                &service,
                "{}",
                Duration::from_secs(5),
                &AtomicBool::new(false),
            );
            match (result, expected) {
                (Ok(Response::Success { data }), Ok(text)) => assert_eq!(data.text, text),
                (Ok(Response::Error { message }), Err(text)) => assert_eq!(message, text),
                (Err(error), Err(text)) => assert!(error.to_string().contains(text), "{error}"),
                (result, expected) => {
                    panic!(
                        "unexpected outcome for {expected:?}: {:?}",
                        result.map(|_| ())
                    )
                }
            }
            assert_eq!(server.join().unwrap().len(), requests);
        }
    }

    #[test]
    fn model_catalog_ranks_recommended_models_and_selects_top_available_default_with_variant() {
        let model_info = |provider: &str, id: &str, name: &str, variants: &[&str]| ModelInfo {
            id: id.into(),
            provider_id: provider.into(),
            name: name.into(),
            enabled: true,
            capabilities: ModelCapabilities {
                output: vec!["text".into()],
            },
            variants: variants
                .iter()
                .map(|v| ModelVariant { id: (*v).into() })
                .collect(),
        };
        let heavy_default = model_info("example", "heavy-agent", "Heavy Agent", &["high"]);
        let models = vec![
            model_info("example", "unranked-a", "Unranked A", &[]),
            model_info(
                "openai",
                "gpt-5.4-nano",
                "GPT-5.4 Nano",
                &["none", "low", "medium"],
            ),
            model_info(
                "anthropic",
                "claude-haiku-4-5-20251001",
                "Claude Haiku 4.5",
                &[],
            ),
            model_info(
                "google",
                "gemini-3.1-flash-lite",
                "Gemini 3.1 Flash Lite",
                &["minimal", "low", "high"],
            ),
        ];

        let catalog = build_model_catalog(models.clone(), Some(heavy_default.clone()));
        assert_eq!(
            catalog.default_key.as_deref(),
            Some("google/gemini-3.1-flash-lite")
        );
        assert_eq!(
            catalog.default_name.as_deref(),
            Some("Gemini 3.1 Flash Lite")
        );
        assert_eq!(catalog.default_variant.as_deref(), Some("minimal"));
        assert_eq!(
            catalog
                .models
                .iter()
                .map(|choice| choice.key.as_str())
                .collect::<Vec<_>>(),
            [
                "google/gemini-3.1-flash-lite",
                "anthropic/claude-haiku-4-5-20251001",
                "openai/gpt-5.4-nano",
                "example/unranked-a",
                "example/heavy-agent",
            ]
        );

        // Removing the #1 recommendation falls back to #2 (`claude-haiku-4-5`), then #3 (`gpt-5.4-nano` with `none`).
        let without_gemini: Vec<_> = models
            .iter()
            .filter(|m| m.id != "gemini-3.1-flash-lite")
            .cloned()
            .collect();
        let catalog2 = build_model_catalog(without_gemini.clone(), Some(heavy_default.clone()));
        assert_eq!(
            catalog2.default_key.as_deref(),
            Some("anthropic/claude-haiku-4-5-20251001")
        );
        assert_eq!(catalog2.default_variant, None);

        let only_nano: Vec<_> = without_gemini
            .into_iter()
            .filter(|m| !m.id.starts_with("claude-haiku-4-5"))
            .collect();
        let catalog3 = build_model_catalog(only_nano, Some(heavy_default));
        assert_eq!(catalog3.default_key.as_deref(), Some("openai/gpt-5.4-nano"));
        assert_eq!(catalog3.default_variant.as_deref(), Some("none"));
    }

    #[test]
    fn ranked_generation_walks_candidates_until_one_succeeds() {
        let candidates = vec![
            Model {
                provider: "google".into(),
                id: "gemini-3.1-flash-lite".into(),
                variant: Some("minimal".into()),
            },
            Model {
                provider: "anthropic".into(),
                id: "claude-haiku-4-5".into(),
                variant: None,
            },
            Model {
                provider: "openai".into(),
                id: "gpt-5.4-nano".into(),
                variant: Some("none".into()),
            },
        ];
        let (service, server) = generation_server(vec![
            (
                "/api/experimental/generate",
                400,
                r#"{"message":"Model unavailable: google/gemini-3.1-flash-lite"}"#,
            ),
            (
                "/api/experimental/generate",
                200,
                r#"{"data":{"text":"resolved from second ranked model"}}"#,
            ),
        ]);
        let output = generate_with_ranked_candidates(
            "Make this concise.",
            &candidates,
            Instant::now(),
            Duration::from_secs(5),
            &AtomicBool::new(false),
            |data, remaining, cancelled| post_generation(&service, data, remaining, cancelled),
        )
        .unwrap();
        assert_eq!(output, "resolved from second ranked model");
        let requests = server.join().unwrap();
        assert_eq!(requests.len(), 2);
        let first: serde_json::Value = serde_json::from_slice(&requests[0]).unwrap();
        let second: serde_json::Value = serde_json::from_slice(&requests[1]).unwrap();
        assert_eq!(
            first["model"],
            serde_json::json!({
                "providerID": "google",
                "id": "gemini-3.1-flash-lite",
                "variant": "minimal"
            })
        );
        assert_eq!(
            second["model"],
            serde_json::json!({
                "providerID": "anthropic",
                "id": "claude-haiku-4-5"
            })
        );
    }

    #[test]
    #[ignore = "requires a running OpenCode service"]
    fn live_catalog() {
        let catalog = load_model_catalog()
            .unwrap()
            .expect("OpenCode should be running");
        println!("{}", catalog.models.len());
    }

    #[test]
    #[ignore = "requires a running OpenCode service and a configured model"]
    fn live_generation_returns_text_without_creating_a_session() {
        assert_eq!(
            generate("Reply with exactly: test", None, Duration::from_secs(30)).unwrap(),
            "test"
        );
    }
}
