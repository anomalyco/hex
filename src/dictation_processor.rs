use std::collections::HashSet;
use std::ffi::OsStr;
#[cfg(not(target_os = "windows"))]
use std::fs;
use std::io::Read;
#[cfg(not(target_os = "windows"))]
use std::io::Write;
#[cfg(not(target_os = "windows"))]
use std::os::unix::fs::{MetadataExt, OpenOptionsExt, PermissionsExt};
#[cfg(not(target_os = "windows"))]
use std::os::unix::process::CommandExt;
use std::path::{Path, PathBuf};
use std::process::ExitStatus;
#[cfg(not(target_os = "windows"))]
use std::process::{Child, Command, Stdio};
use std::sync::atomic::{AtomicBool, Ordering};
use std::thread;
use std::time::{Duration, Instant};

use color_eyre::eyre::{Result, WrapErr, eyre};
use serde::{Deserialize, Serialize};

use crate::context::{ContextSelector, ContextSnapshot};
use crate::text_replacements::ReplacementSet;
use crate::vocabulary::VocabularyEntry;

const PROTOCOL_PROMPT: &str = "You transform dictated speech into replacement text. Return only the text that should be pasted. Do not add an explanation, label, alternative, or Markdown fence.";
const MAX_OPENCODE_OUTPUT_BYTES: u64 = 16 * 1024 * 1024;
/// Newest service info route first; older OpenCode V2 builds only serve the later names.
#[cfg(not(target_os = "windows"))]
const SERVICE_INFO_ENDPOINTS: [&str; 3] = ["/api/info", "/api/status", "/api/health"];
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

    /// The global profile without the `modes` experiment: the default
    /// mode's replacements only, no rewriting or transformations.
    pub(crate) fn global_replacements(mode: &crate::app_settings::DictationMode) -> Self {
        Self::new("Global", "").replacements(ReplacementSet::new(&mode.replacements))
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

#[cfg_attr(not(target_os = "windows"), allow(dead_code))]
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

#[cfg(not(target_os = "windows"))]
static CACHED_RANKED_CANDIDATES: std::sync::Mutex<Option<(String, Vec<Model>)>> =
    std::sync::Mutex::new(None);

#[cfg(not(target_os = "windows"))]
fn cache_ranked_candidates(endpoint: &str, catalog: &ModelCatalog) {
    if let Ok(mut guard) = CACHED_RANKED_CANDIDATES.lock() {
        *guard = Some((endpoint.to_owned(), catalog.ranked_candidates(None)));
    }
}

#[cfg(not(target_os = "windows"))]
fn ranked_candidates_for_endpoint(endpoint: &str, password: &str) -> Vec<Model> {
    if let Ok(guard) = CACHED_RANKED_CANDIDATES.lock()
        && let Some((cached_endpoint, candidates)) = guard.as_ref()
        && cached_endpoint == endpoint
    {
        return candidates.clone();
    }
    let Ok(workspace) = crate::app_paths::opencode_workspace() else {
        return Vec::new();
    };
    let Ok(models) = opencode_api::<ModelsResponse>(endpoint, password, &workspace, "/api/model")
    else {
        return Vec::new();
    };
    let catalog = build_model_catalog(models.data, None);
    let candidates = catalog.ranked_candidates(None);
    cache_ranked_candidates(endpoint, &catalog);
    candidates
}

#[cfg(not(target_os = "windows"))]
pub fn load_model_catalog() -> Result<ModelCatalog> {
    let (endpoint, password) =
        discover_opencode_service(Duration::from_secs(10), &AtomicBool::new(false))?;
    let workspace = crate::app_paths::opencode_workspace()?;
    let models: ModelsResponse = opencode_api(&endpoint, &password, &workspace, "/api/model")?;
    let default: DefaultModelResponse =
        opencode_api(&endpoint, &password, &workspace, "/api/model/default")?;
    let catalog = build_model_catalog(models.data, default.data);
    cache_ranked_candidates(&endpoint, &catalog);
    Ok(catalog)
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

#[cfg(not(target_os = "windows"))]
fn is_executable(path: &std::path::Path) -> bool {
    path.metadata()
        .is_ok_and(|metadata| metadata.is_file() && metadata.permissions().mode() & 0o111 != 0)
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

#[cfg(not(target_os = "windows"))]
fn opencode_api<T: for<'de> Deserialize<'de>>(
    endpoint: &str,
    password: &str,
    workspace: &Path,
    path: &str,
) -> Result<T> {
    // The CLI can truncate large catalog responses when stdout is piped.
    let response = opencode_http(
        endpoint,
        password,
        path,
        None,
        Some(workspace),
        Duration::from_secs(10),
        &AtomicBool::new(false),
    )?;
    decode_api_response(path, &response.body)
}

#[cfg(not(target_os = "windows"))]
struct HttpResponse {
    status: u16,
    body: Vec<u8>,
}

/// One authenticated loopback request through the system curl. A non-zero
/// curl exit is a transport failure; HTTP error statuses return normally so
/// callers can distinguish a missing route from a served error body.
#[cfg(not(target_os = "windows"))]
fn opencode_http(
    endpoint: &str,
    password: &str,
    path: &str,
    data: Option<&str>,
    workspace: Option<&Path>,
    deadline: Duration,
    cancelled: &AtomicBool,
) -> Result<HttpResponse> {
    let (command, input) = opencode_http_command(endpoint, password, path, data, workspace)?;
    let output = run_command(command, Some(input), deadline, path, cancelled)?;
    if !output.status.success() {
        return Err(eyre!(
            "OpenCode {path} exited with {}: {}",
            output.status,
            String::from_utf8_lossy(&output.stderr).trim()
        ));
    }
    split_http_status(output.stdout).ok_or_else(|| eyre!("OpenCode {path} returned no HTTP status"))
}

/// curl appends `\n<status>` after the body; see `opencode_http_command`.
#[cfg(not(target_os = "windows"))]
fn split_http_status(mut stdout: Vec<u8>) -> Option<HttpResponse> {
    let newline = stdout.iter().rposition(|byte| *byte == b'\n')?;
    let status = std::str::from_utf8(&stdout[newline + 1..])
        .ok()?
        .parse()
        .ok()?;
    stdout.truncate(newline);
    Some(HttpResponse {
        status,
        body: stdout,
    })
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

#[cfg(not(target_os = "windows"))]
fn generate_cancellable(
    prompt: &str,
    model: Option<&Model>,
    deadline: Duration,
    cancelled: &AtomicBool,
) -> Result<String> {
    let started = Instant::now();
    let (endpoint, password) =
        discover_opencode_service(deadline.saturating_sub(started.elapsed()), cancelled)?;
    generate_with_candidates(
        prompt,
        model,
        || ranked_candidates_for_endpoint(&endpoint, &password),
        started,
        deadline,
        cancelled,
        |data, remaining, cancelled| {
            post_generation(&endpoint, &password, data, remaining, cancelled)
        },
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
            "opencode2 generation exceeded {} seconds",
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
#[cfg(not(target_os = "windows"))]
fn post_generation(
    endpoint: &str,
    password: &str,
    data: &str,
    deadline: Duration,
    cancelled: &AtomicBool,
) -> Result<Response> {
    let started = Instant::now();
    let (newest, older) = GENERATE_ENDPOINTS.split_first().expect("endpoints");
    let mut response = opencode_http(
        endpoint,
        password,
        newest,
        Some(data),
        None,
        deadline,
        cancelled,
    )?;
    for path in older {
        if response.status != 404 {
            break;
        }
        response = opencode_http(
            endpoint,
            password,
            path,
            Some(data),
            None,
            deadline.saturating_sub(started.elapsed()),
            cancelled,
        )?;
    }
    if response.status == 404 {
        return Err(eyre!("OpenCode does not serve a generation route"));
    }
    serde_json::from_slice(&response.body)
        .wrap_err("opencode2 returned an invalid generation response")
}

#[cfg(not(target_os = "windows"))]
fn discover_opencode_service(
    deadline: Duration,
    cancelled: &AtomicBool,
) -> Result<(String, String)> {
    let started = Instant::now();
    let workspace = crate::app_paths::opencode_workspace()?;
    discover_opencode_service_with(
        &opencode_executable(),
        &workspace,
        default_opencode_state_dir().as_deref(),
        deadline.saturating_sub(started.elapsed()),
        cancelled,
    )
}

/// OpenCode's own state directory for Hex's environment: `$XDG_STATE_HOME`
/// when absolute, else `~/.local/state`, then `opencode`.
#[cfg(not(target_os = "windows"))]
fn default_opencode_state_dir() -> Option<PathBuf> {
    let root = std::env::var_os("XDG_STATE_HOME")
        .map(PathBuf::from)
        .filter(|path| path.is_absolute())
        .or_else(|| dirs::home_dir().map(|home| home.join(".local/state")))?;
    Some(root.join("opencode"))
}

/// Asks the CLI which service it reaches, then reads that service's private
/// registration for the URL and password. The registration is looked up in
/// the state directory the CLI reports and in OpenCode's default one: a
/// wrapper can reject `debug paths` or point the CLI's own state elsewhere
/// while it still reaches the default service. Only a registration matching
/// the live server's pid and version counts.
#[cfg(not(target_os = "windows"))]
fn discover_opencode_service_with(
    executable: &Path,
    workspace: &Path,
    default_state: Option<&Path>,
    deadline: Duration,
    cancelled: &AtomicBool,
) -> Result<(String, String)> {
    let started = Instant::now();
    fs::create_dir_all(workspace)?;
    let run = |args: &[&str]| {
        let mut command = Command::new(executable);
        command.current_dir(workspace).args(args);
        run_command(
            command,
            None,
            deadline.saturating_sub(started.elapsed()),
            "service discovery",
            cancelled,
        )
    };
    let decode = |output: CommandOutput| -> Result<String> {
        if !output.status.success() {
            // Discovery can return credentials; never include its output in diagnostics.
            return Err(eyre!(
                "OpenCode service discovery failed ({})",
                output.status
            ));
        }
        let mut value = String::from_utf8(output.stdout)
            .wrap_err("OpenCode service discovery returned invalid UTF-8")?;
        if value.ends_with('\n') {
            value.pop();
        }
        Ok(value)
    };
    #[derive(Deserialize)]
    struct ServiceInfo {
        pid: u32,
        version: String,
    }
    let (newest, older) = SERVICE_INFO_ENDPOINTS.split_first().expect("endpoints");
    let mut output = run(&["api", "get", newest])?;
    // Older OpenCode versions expose the service info under an earlier name.
    // Only a CLI 404 diagnostic tries the next name; other failures are final.
    for endpoint in older {
        if !endpoint_is_missing(&output) {
            break;
        }
        output = run(&["api", "get", endpoint])?;
    }
    let info: ServiceInfo = serde_json::from_str(&decode(output)?)
        .map_err(|_| eyre!("OpenCode returned an invalid service info response"))?;
    // A failed `debug paths` is not final: the default state directory remains.
    let reported_state = decode(run(&["debug", "paths"])?).ok().and_then(|paths| {
        paths.lines().find_map(|line| {
            let (key, value) = line.split_once(char::is_whitespace)?;
            let path = PathBuf::from(value.trim_start());
            (key == "state" && path.is_absolute()).then_some(path)
        })
    });
    let mut states: Vec<&Path> = reported_state.as_deref().into_iter().collect();
    if let Some(default_state) = default_state
        && !states.contains(&default_state)
    {
        states.push(default_state);
    }
    let mut last_error = eyre!("OpenCode did not report its state directory");
    for state in states {
        match read_service_registration(state, info.pid, &info.version) {
            Ok(endpoint) => return Ok(endpoint),
            Err(error) => last_error = error,
        }
    }
    Err(last_error)
}

/// The CLI reports a missing route as a final `HTTP 404 Not Found` stderr
/// line; earlier lines may carry warnings such as a client/server version skew.
fn endpoint_is_missing(output: &CommandOutput) -> bool {
    !output.status.success()
        && String::from_utf8_lossy(&output.stderr)
            .lines()
            .map(str::trim)
            .rfind(|line| !line.is_empty())
            == Some("HTTP 404 Not Found")
}

#[cfg(not(target_os = "windows"))]
fn read_service_registration(state: &Path, pid: u32, version: &str) -> Result<(String, String)> {
    #[derive(Deserialize)]
    struct Registration {
        pid: u32,
        version: String,
        url: String,
        password: String,
    }
    let mut found = None;
    for entry in fs::read_dir(state)?.take(512).flatten() {
        let name = entry.file_name();
        let name = name.to_string_lossy();
        if !name.starts_with("service") || !name.ends_with(".json") {
            continue;
        }
        let Ok(mut file) = fs::OpenOptions::new()
            .read(true)
            .custom_flags(libc::O_NOFOLLOW | libc::O_NONBLOCK)
            .open(entry.path())
        else {
            continue;
        };
        let metadata = file.metadata()?;
        if !metadata.is_file()
            || metadata.uid() != unsafe { libc::geteuid() }
            || metadata.permissions().mode() & 0o077 != 0
            || metadata.len() > 64 * 1024
        {
            continue;
        }
        let mut bytes = Vec::new();
        Read::by_ref(&mut file)
            .take(64 * 1024 + 1)
            .read_to_end(&mut bytes)?;
        if bytes.len() > 64 * 1024 {
            continue;
        }
        let Ok(registration) = serde_json::from_slice::<Registration>(&bytes) else {
            continue;
        };
        if registration.pid != pid
            || registration.version != version
            || registration.password.is_empty()
        {
            continue;
        }
        let endpoint = (registration.url, registration.password);
        if found.as_ref().is_some_and(|previous| previous != &endpoint) {
            return Err(eyre!(
                "OpenCode has conflicting active service registrations"
            ));
        }
        found = Some(endpoint);
    }
    found.ok_or_else(|| eyre!("OpenCode active service registration is unavailable"))
}

/// macOS always has /usr/bin/curl, and most Linux distributions do too.
/// NixOS doesn't; Hex's Nix package puts curl on PATH instead.
#[cfg(not(target_os = "windows"))]
fn curl_program() -> PathBuf {
    let system = Path::new("/usr/bin/curl");
    if cfg!(target_os = "macos") || system.exists() {
        system.into()
    } else {
        PathBuf::from("curl")
    }
}

#[cfg(not(target_os = "windows"))]
fn opencode_http_command(
    endpoint: &str,
    password: &str,
    path: &str,
    data: Option<&str>,
    workspace: Option<&Path>,
) -> Result<(Command, Vec<u8>)> {
    let mut url = url::Url::parse(endpoint).wrap_err("invalid OpenCode service endpoint")?;
    if url.scheme() != "http"
        || !url.username().is_empty()
        || url.password().is_some()
        || url.path() != "/"
        || url.query().is_some()
        || url.fragment().is_some()
        || url.port_or_known_default() == Some(0)
    {
        return Err(eyre!("OpenCode service must use a loopback HTTP endpoint"));
    }
    match url.host() {
        Some(url::Host::Domain("localhost")) => url.set_host(Some("127.0.0.1"))?,
        Some(url::Host::Ipv4(ip)) if ip.is_unspecified() => url.set_host(Some("127.0.0.1"))?,
        Some(url::Host::Ipv6(ip)) if ip.is_unspecified() => url.set_host(Some("[::1]"))?,
        Some(url::Host::Ipv4(ip)) if ip.is_loopback() => {}
        Some(url::Host::Ipv6(ip)) if ip.is_loopback() => {}
        _ => return Err(eyre!("OpenCode service must use a loopback HTTP endpoint")),
    }
    url.set_path(path);
    let mut command = Command::new(curl_program());
    let directory = if let Some(workspace) = workspace {
        fs::create_dir_all(workspace).wrap_err_with(|| {
            format!(
                "could not create OpenCode workspace {}",
                workspace.display()
            )
        })?;
        let workspace = workspace.canonicalize().wrap_err_with(|| {
            format!(
                "could not resolve OpenCode workspace {}",
                workspace.display()
            )
        })?;
        let directory = workspace
            .to_str()
            .ok_or_else(|| eyre!("OpenCode workspace path is not valid UTF-8"))?;
        command.current_dir(&workspace);
        Some(format!(
            "x-opencode-directory:{}",
            encode_uri_component(directory)
        ))
    } else {
        None
    };
    let user = format!("opencode:{password}");
    let mut options = vec![
        ("url", url.as_str()),
        ("user", user.as_str()),
        ("header", "Content-Type: application/json"),
    ];
    if let Some(data) = data {
        options.push(("data-binary", data));
    }
    if let Some(directory) = directory.as_deref() {
        options.push(("header", directory));
    }
    let mut input = String::new();
    for (key, value) in options {
        input.push_str(key);
        input.push_str(" = \"");
        for ch in value.chars() {
            match ch {
                '\\' => input.push_str("\\\\"),
                '"' => input.push_str("\\\""),
                '\n' => input.push_str("\\n"),
                '\r' => input.push_str("\\r"),
                '\t' => input.push_str("\\t"),
                ch if ch.is_control() => {
                    return Err(eyre!("unsupported OpenCode request character"));
                }
                ch => input.push(ch),
            }
        }
        input.push_str("\"\n");
    }
    // Ignore curlrc, proxies, and redirects. Both credentials and body stay off argv and disk.
    // The trailing status line lets callers tell a missing route from a served error body.
    command.args([
        "--disable",
        "--silent",
        "--show-error",
        "--write-out",
        "\n%{http_code}",
        "--noproxy",
        "*",
        "--proxy",
        "",
        "--proto",
        "=http",
        "--basic",
        "--config",
        "-",
    ]);
    Ok((command, input.into_bytes()))
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
            return Err(eyre!("opencode2 generation was cancelled"));
        }
        if started.elapsed() >= deadline {
            return Err(eyre!(
                "opencode2 generation exceeded {} seconds",
                deadline.as_secs()
            ));
        }
        thread::sleep(Duration::from_millis(20));
    }
    Ok(())
}

struct CommandOutput {
    status: ExitStatus,
    stdout: Vec<u8>,
    stderr: Vec<u8>,
}

#[cfg(not(target_os = "windows"))]
fn run_command(
    mut command: Command,
    input: Option<Vec<u8>>,
    deadline: Duration,
    operation: &str,
    cancelled: &AtomicBool,
) -> Result<CommandOutput> {
    if cancelled.load(Ordering::Acquire) {
        return Err(eyre!("opencode2 {operation} was cancelled"));
    }
    if deadline.is_zero() {
        return Err(eyre!("opencode2 {operation} deadline expired"));
    }
    let mut child = command
        .process_group(0)
        .stdin(if input.is_some() {
            Stdio::piped()
        } else {
            Stdio::null()
        })
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|error| {
            if error.kind() == std::io::ErrorKind::NotFound {
                eyre!("opencode2 is not installed")
            } else {
                eyre!("could not start opencode2 for {operation}: {error}")
            }
        })?;
    let stdout = child.stdout.take().expect("opencode2 stdout must be piped");
    let stderr = child.stderr.take().expect("opencode2 stderr must be piped");
    let input = input.map(|input| {
        let mut stdin = child.stdin.take().expect("OpenCode input must be piped");
        thread::spawn(move || stdin.write_all(&input))
    });
    let stdout = thread::spawn(move || read_output(stdout));
    let stderr = thread::spawn(move || read_output(stderr));
    let started = Instant::now();
    let outcome: Result<ExitStatus> = loop {
        match child.try_wait() {
            Ok(Some(status)) => break Ok(status),
            Ok(None) => {}
            Err(error) => break Err(error).wrap_err("could not inspect opencode2"),
        }
        if cancelled.load(Ordering::Acquire) {
            break Err(eyre!("opencode2 {operation} was cancelled"));
        }
        if started.elapsed() >= deadline {
            break Err(eyre!(
                "opencode2 {operation} exceeded {} seconds",
                deadline.as_secs()
            ));
        }
        thread::sleep(Duration::from_millis(20));
    };
    // An aborted child is torn down before the pipes are joined so blocked readers
    // and the input writer observe EOF instead of holding the deadline open.
    match &outcome {
        Ok(_) => kill_process_group(child.id()),
        Err(_) => terminate_process_group(&mut child),
    }
    let stdout = stdout.join();
    let stderr = stderr.join();
    let input = input.map(|input| input.join());
    let status = outcome?;
    if let Some(input) = input {
        let result = input.map_err(|_| eyre!("OpenCode input writer panicked"))?;
        if status.success() {
            result.wrap_err("could not write OpenCode request")?;
        }
    }
    let stdout = stdout.map_err(|_| eyre!("opencode2 stdout reader panicked"))??;
    let stderr = stderr.map_err(|_| eyre!("opencode2 stderr reader panicked"))??;
    Ok(CommandOutput {
        status,
        stdout,
        stderr,
    })
}

#[cfg(not(target_os = "windows"))]
fn terminate_process_group(child: &mut Child) {
    kill_process_group(child.id());
    let _ = child.kill();
    let _ = child.wait();
}

#[cfg(not(target_os = "windows"))]
fn kill_process_group(pid: u32) {
    // run_command creates a new process group whose ID is the child PID.
    unsafe {
        libc::killpg(pid as libc::pid_t, libc::SIGKILL);
    }
}

fn read_output(mut output: impl Read) -> Result<Vec<u8>> {
    read_bounded_output(&mut output, MAX_OPENCODE_OUTPUT_BYTES)
}

fn read_bounded_output(mut output: impl Read, max_bytes: u64) -> Result<Vec<u8>> {
    let mut bytes = Vec::new();
    output
        .by_ref()
        .take(max_bytes + 1)
        .read_to_end(&mut bytes)
        .wrap_err("could not read opencode2 output")?;
    if bytes.len() as u64 > max_bytes {
        Err(eyre!("opencode2 output exceeded {max_bytes} bytes"))
    } else {
        Ok(bytes)
    }
}

fn opencode_executable() -> PathBuf {
    installed_opencode_executable().unwrap_or_else(|| "opencode2".into())
}

/// The OpenCode V2 CLI Hex runs; see [`find_opencode_install`].
pub(crate) fn installed_opencode_executable() -> Option<PathBuf> {
    match find_opencode_install() {
        OpenCodeInstall::Ready(executable) => Some(executable),
        OpenCodeInstall::V1Only | OpenCodeInstall::Missing => None,
    }
}

/// What looking for the OpenCode CLI found.
#[derive(Debug, PartialEq, Eq)]
pub(crate) enum OpenCodeInstall {
    /// A V2 CLI: an `opencode2`, or an `opencode` that proved to be V2.
    Ready(PathBuf),
    /// Only a V1 `opencode`, which Hex never runs.
    V1Only,
    Missing,
}

/// The names the V2 CLI ships under, preferred first: any `opencode2` beats
/// every `opencode`, and an `opencode` counts only once it proves to be V2.
const OPENCODE_BINARY_NAMES: [&str; 2] = ["opencode2", "opencode"];
const OPENCODE_PROBE_DEADLINE: Duration = Duration::from_secs(10);

/// Finds the CLI as a GUI app must: launchd's `PATH` lacks the usual install
/// folders, so they are searched too. An `opencode` may run a bounded probe,
/// so call this off the UI thread; verdicts are kept until the file changes.
pub(crate) fn find_opencode_install() -> OpenCodeInstall {
    discover_opencode(opencode_is_v2)
}

fn discover_opencode(is_v2: impl FnMut(&Path) -> bool) -> OpenCodeInstall {
    let current_directory = std::env::current_dir().ok();
    find_opencode_executable(
        std::env::var_os("HEX_OPENCODE_CLI").map(PathBuf::from),
        std::env::var_os("PATH").as_deref(),
        dirs::home_dir().as_deref(),
        current_directory.as_deref(),
        is_v2,
    )
}

/// The configured override is trusted as is; otherwise every location is
/// searched for `opencode2` before any `opencode` is considered.
fn find_opencode_executable(
    configured: Option<PathBuf>,
    path: Option<&OsStr>,
    home: Option<&Path>,
    current_directory: Option<&Path>,
    mut is_v2: impl FnMut(&Path) -> bool,
) -> OpenCodeInstall {
    if let Some(configured) = configured {
        return OpenCodeInstall::Ready(absolute_path(configured, current_directory));
    }
    let mut found_v1 = false;
    for name in OPENCODE_BINARY_NAMES {
        for candidate in opencode_candidates(name, path, home, current_directory) {
            if !is_executable(&candidate) {
                continue;
            }
            if name == "opencode2" || is_v2(&candidate) {
                return OpenCodeInstall::Ready(candidate);
            }
            found_v1 = true;
        }
    }
    if found_v1 {
        OpenCodeInstall::V1Only
    } else {
        OpenCodeInstall::Missing
    }
}

/// When an executable changed, so a kept verdict no longer applies.
type OpenCodeFingerprint = (Option<std::time::SystemTime>, u64);

static OPENCODE_VERDICTS: std::sync::Mutex<
    Option<std::collections::HashMap<PathBuf, (OpenCodeFingerprint, bool)>>,
> = std::sync::Mutex::new(None);

fn opencode_fingerprint(executable: &Path) -> Option<OpenCodeFingerprint> {
    let metadata = std::fs::metadata(executable).ok()?;
    Some((metadata.modified().ok(), metadata.len()))
}

fn cached_opencode_verdict(executable: &Path) -> Option<bool> {
    let fingerprint = opencode_fingerprint(executable)?;
    let verdicts = OPENCODE_VERDICTS.lock().ok()?;
    let (kept, is_v2) = verdicts.as_ref()?.get(executable)?;
    (*kept == fingerprint).then_some(*is_v2)
}

/// Whether an `opencode` is OpenCode V2. Only a definite answer is kept, so
/// a probe that timed out runs again at the next (coarse) catalog check.
fn opencode_is_v2(executable: &Path) -> bool {
    if let Some(is_v2) = cached_opencode_verdict(executable) {
        return is_v2;
    }
    let fingerprint = opencode_fingerprint(executable);
    let verdict = probe_opencode_generation(executable);
    if let (Some(is_v2), Some(fingerprint)) = (verdict, fingerprint)
        && let Ok(mut verdicts) = OPENCODE_VERDICTS.lock()
    {
        verdicts
            .get_or_insert_with(Default::default)
            .insert(executable.to_owned(), (fingerprint, is_v2));
    }
    if verdict == Some(false) {
        tracing::info!(executable = %executable.display(), "ignoring OpenCode V1");
    }
    verdict.unwrap_or(false)
}

/// `--version` decides a release. Prerelease builds of both generations
/// print `0.0.0-…`, so those must answer `api get /api/info`, a command V1
/// rejects; a served 404 still proves the V2 CLI.
fn probe_opencode_generation(executable: &Path) -> Option<bool> {
    let version = run_opencode_probe(executable, &["--version"]).ok()?;
    if version.status.success()
        && let Some(is_v2) = release_is_v2(&String::from_utf8_lossy(&version.stdout))
    {
        return Some(is_v2);
    }
    let info = run_opencode_probe(executable, &["api", "get", "/api/info"]).ok()?;
    (info.status.success() || endpoint_is_missing(&info)).then_some(true)
}

/// Whether `--version` output names a V2 release (`opencode v2.2.0`,
/// `2.2.0`); `None` when it can't tell.
fn release_is_v2(output: &str) -> Option<bool> {
    let version = output.split_whitespace().last()?;
    let version = version.strip_prefix('v').unwrap_or(version);
    let (core, prerelease) = match version.split_once('-') {
        Some((core, prerelease)) => (core, Some(prerelease)),
        None => (version, None),
    };
    let major: u64 = core.split('.').next()?.parse().ok()?;
    if core == "0.0.0" && prerelease.is_some() {
        return None;
    }
    Some(major >= 2)
}

fn run_opencode_probe(executable: &Path, args: &[&str]) -> Result<CommandOutput> {
    let mut command = crate::background_process::command(executable);
    command.args(args);
    #[cfg(not(target_os = "windows"))]
    let output = run_command(
        command,
        None,
        OPENCODE_PROBE_DEADLINE,
        "version check",
        &AtomicBool::new(false),
    );
    #[cfg(target_os = "windows")]
    let output = cli::run_command(
        command,
        OPENCODE_PROBE_DEADLINE,
        "version check",
        &AtomicBool::new(false),
    );
    output
}

#[cfg(not(target_os = "windows"))]
const HOME_INSTALL_DIRECTORIES: [&str; 14] = [
    ".opencode/bin",
    ".vite-plus/bin",
    ".bun/bin",
    "Library/pnpm",
    "Library/pnpm/bin",
    ".yarn/bin",
    ".config/yarn/global/node_modules/.bin",
    ".npm-global/bin",
    ".local/bin",
    "bin",
    ".volta/bin",
    ".asdf/shims",
    ".local/share/mise/shims",
    ".nodenv/shims",
];

/// Where `name` may be installed, in search order: `PATH`, the usual home
/// install folders, Node version managers, then Homebrew.
#[cfg(not(target_os = "windows"))]
fn opencode_candidates(
    name: &str,
    path: Option<&OsStr>,
    home: Option<&Path>,
    current_directory: Option<&Path>,
) -> Vec<PathBuf> {
    let path_candidates = path
        .into_iter()
        .flat_map(std::env::split_paths)
        .map(|directory| absolute_path(directory.join(name), current_directory));
    let home_candidates = home
        .into_iter()
        .flat_map(|home| HOME_INSTALL_DIRECTORIES.map(|directory| home.join(directory).join(name)));
    path_candidates
        .chain(home_candidates)
        .chain(version_manager_candidates(home, name))
        .chain(
            ["/opt/homebrew/bin", "/usr/local/bin"]
                .map(|directory| Path::new(directory).join(name)),
        )
        .collect()
}

fn absolute_path(path: PathBuf, current_directory: Option<&Path>) -> PathBuf {
    if path.is_absolute() {
        path
    } else {
        current_directory
            .map(|directory| directory.join(&path))
            .unwrap_or(path)
    }
}

#[cfg(not(target_os = "windows"))]
fn version_manager_candidates(home: Option<&Path>, name: &str) -> Vec<PathBuf> {
    let Some(home) = home else {
        return Vec::new();
    };
    let roots = [
        (home.join(".nvm/versions/node"), "bin"),
        (
            home.join("Library/Application Support/fnm/node-versions"),
            "installation/bin",
        ),
        (
            home.join(".local/share/fnm/node-versions"),
            "installation/bin",
        ),
        (home.join(".fnm/node-versions"), "installation/bin"),
    ];
    roots
        .into_iter()
        .flat_map(|(root, bin)| {
            fs::read_dir(root)
                .into_iter()
                .flatten()
                .flatten()
                .take(64)
                .map(move |entry| entry.path().join(bin).join(name))
        })
        .collect()
}

fn encode_uri_component(value: &str) -> String {
    let mut encoded = String::with_capacity(value.len());
    for byte in value.bytes() {
        if byte.is_ascii_alphanumeric() {
            encoded.push(char::from(byte));
        } else {
            encoded.push('%');
            encoded.push(char::from(b"0123456789ABCDEF"[(byte >> 4) as usize]));
            encoded.push(char::from(b"0123456789ABCDEF"[(byte & 0x0f) as usize]));
        }
    }
    encoded
}

// Windows reaches the service through the `opencode2 api` CLI. Discovery,
// the ranked catalog, and generation follow the same policy as the curl
// transport above.
#[cfg(target_os = "windows")]
pub use cli::load_model_catalog;
#[cfg(target_os = "windows")]
use cli::{generate_cancellable, is_executable, opencode_candidates};

#[cfg(target_os = "windows")]
mod cli {
    use std::ffi::OsStr;
    use std::path::{Path, PathBuf};
    use std::process::{Child, Command, Stdio};
    use std::sync::atomic::{AtomicBool, Ordering};
    use std::thread;
    use std::time::{Duration, Instant};

    use color_eyre::eyre::{Result, WrapErr, eyre};
    use serde::Deserialize;

    use super::{
        CommandOutput, DefaultModelResponse, GENERATE_ENDPOINTS, Model, ModelCatalog,
        ModelsResponse, Response, absolute_path, build_model_catalog, decode_api_response,
        encode_uri_component, endpoint_is_missing, generate_with_candidates, read_output,
    };

    const CATALOG_DEADLINE: Duration = Duration::from_secs(10);

    static CACHED_RANKED_CANDIDATES: std::sync::Mutex<Option<Vec<Model>>> =
        std::sync::Mutex::new(None);

    pub fn load_model_catalog() -> Result<ModelCatalog> {
        let models: ModelsResponse = api_get("/api/model")?;
        let default: DefaultModelResponse = api_get("/api/model/default")?;
        let catalog = build_model_catalog(models.data, default.data);
        cache_ranked_candidates(&catalog);
        Ok(catalog)
    }

    fn cache_ranked_candidates(catalog: &ModelCatalog) {
        if let Ok(mut guard) = CACHED_RANKED_CANDIDATES.lock() {
            *guard = Some(catalog.ranked_candidates(None));
        }
    }

    fn ranked_candidates() -> Vec<Model> {
        if let Ok(guard) = CACHED_RANKED_CANDIDATES.lock()
            && let Some(candidates) = guard.as_ref()
        {
            return candidates.clone();
        }
        let Ok(models) = api_get::<ModelsResponse>("/api/model") else {
            return Vec::new();
        };
        let catalog = build_model_catalog(models.data, None);
        cache_ranked_candidates(&catalog);
        catalog.ranked_candidates(None)
    }

    pub(super) fn generate_cancellable(
        prompt: &str,
        model: Option<&Model>,
        deadline: Duration,
        cancelled: &AtomicBool,
    ) -> Result<String> {
        generate_with_candidates(
            prompt,
            model,
            ranked_candidates,
            Instant::now(),
            deadline,
            cancelled,
            post_generation,
        )
    }

    /// Posts to the newest generation route, falling back to the older name
    /// only when the CLI reports that route missing. One deadline covers both.
    fn post_generation(data: &str, deadline: Duration, cancelled: &AtomicBool) -> Result<Response> {
        let started = Instant::now();
        let mut output = None;
        for path in GENERATE_ENDPOINTS {
            let mut command = opencode_command()?;
            command.args(["post", path, "--data", data]);
            let attempt = run_command(
                command,
                deadline.saturating_sub(started.elapsed()),
                path,
                cancelled,
            )?;
            let missing = endpoint_is_missing(&attempt);
            output = Some(attempt);
            if !missing {
                break;
            }
        }
        let output = output.expect("at least one generation route");
        if endpoint_is_missing(&output) {
            return Err(eyre!("OpenCode does not serve a generation route"));
        }
        if !output.status.success() {
            return Err(eyre!(
                "opencode2 exited with {}: {}",
                output.status,
                String::from_utf8_lossy(&output.stderr).trim()
            ));
        }
        serde_json::from_slice(&output.stdout)
            .wrap_err("opencode2 returned an invalid generation response")
    }

    fn api_get<T: for<'de> Deserialize<'de>>(path: &str) -> Result<T> {
        let mut command = opencode_command()?;
        command.args(["get", path]);
        let output = run_command(command, CATALOG_DEADLINE, path, &AtomicBool::new(false))?;
        if !output.status.success() {
            return Err(eyre!(
                "OpenCode {path} exited with {}: {}",
                output.status,
                String::from_utf8_lossy(&output.stderr).trim()
            ));
        }
        decode_api_response(path, &output.stdout)
    }

    fn opencode_command() -> Result<Command> {
        opencode_command_in(&crate::app_paths::opencode_workspace()?)
    }

    /// An `opencode2 api` command scoped to the Hex OpenCode workspace.
    fn opencode_command_in(workspace: &Path) -> Result<Command> {
        std::fs::create_dir_all(workspace).wrap_err_with(|| {
            format!(
                "could not create OpenCode workspace {}",
                workspace.display()
            )
        })?;
        let workspace = workspace.canonicalize().wrap_err_with(|| {
            format!(
                "could not resolve OpenCode workspace {}",
                workspace.display()
            )
        })?;
        let directory = workspace
            .to_str()
            .ok_or_else(|| eyre!("OpenCode workspace path is not valid UTF-8"))?;
        // canonicalize returns a verbatim path; OpenCode wants the drive form.
        let directory = directory.strip_prefix(r"\\?\").unwrap_or(directory);
        let mut command = crate::background_process::command(super::opencode_executable());
        command.current_dir(&workspace).args([
            "api",
            "--header",
            &format!("x-opencode-directory:{}", encode_uri_component(directory)),
        ]);
        Ok(command)
    }

    pub(super) fn run_command(
        mut command: Command,
        deadline: Duration,
        operation: &str,
        cancelled: &AtomicBool,
    ) -> Result<CommandOutput> {
        if cancelled.load(Ordering::Acquire) {
            return Err(eyre!("opencode2 {operation} was cancelled"));
        }
        if deadline.is_zero() {
            return Err(eyre!("opencode2 {operation} deadline expired"));
        }
        let mut job = job::Job::kill_on_close();
        let mut child = command
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .map_err(|error| {
                if error.kind() == std::io::ErrorKind::NotFound {
                    eyre!("opencode2 is not installed")
                } else {
                    eyre!("could not start opencode2 for {operation}: {error}")
                }
            })?;
        // Descendants started after this join the job, so killing it reaps
        // the whole tree and the pipe readers always reach EOF.
        if job.as_ref().is_some_and(|job| !job.assign(&child)) {
            job = None;
        }
        let stdout = child.stdout.take().expect("opencode2 stdout must be piped");
        let stderr = child.stderr.take().expect("opencode2 stderr must be piped");
        let stdout = thread::spawn(move || read_output(stdout));
        let stderr = thread::spawn(move || read_output(stderr));
        let started = Instant::now();
        let outcome = loop {
            match child.try_wait() {
                Ok(Some(status)) => break Ok(status),
                Ok(None) => {}
                Err(error) => break Err(error).wrap_err("could not inspect opencode2"),
            }
            if cancelled.load(Ordering::Acquire) {
                break Err(eyre!("opencode2 {operation} was cancelled"));
            }
            if started.elapsed() >= deadline {
                break Err(eyre!(
                    "opencode2 {operation} exceeded {} seconds",
                    deadline.as_secs()
                ));
            }
            thread::sleep(Duration::from_millis(20));
        };
        terminate(&mut child, job.as_ref(), outcome.is_err());
        let stdout = stdout.join();
        let stderr = stderr.join();
        let status = outcome?;
        let stdout = stdout.map_err(|_| eyre!("opencode2 stdout reader panicked"))??;
        let stderr = stderr.map_err(|_| eyre!("opencode2 stderr reader panicked"))??;
        Ok(CommandOutput {
            status,
            stdout,
            stderr,
        })
    }

    fn terminate(child: &mut Child, job: Option<&job::Job>, aborted: bool) {
        if let Some(job) = job {
            job.terminate();
        }
        if aborted {
            let _ = child.kill();
            let _ = child.wait();
        }
    }

    /// Spawning only ever runs `.exe` files; npm's extensionless shell shims
    /// must not count as installed.
    pub(super) fn is_executable(path: &Path) -> bool {
        path.extension()
            .is_some_and(|extension| extension.eq_ignore_ascii_case("exe"))
            && path.is_file()
    }

    /// Where `name` may be installed: each `PATH` folder in order, the real
    /// binary npm shims wrap first. JSON request bodies survive .exe argv
    /// quoting but are shredded by cmd shims.
    pub(super) fn opencode_candidates(
        name: &str,
        path: Option<&OsStr>,
        _home: Option<&Path>,
        current_directory: Option<&Path>,
    ) -> Vec<PathBuf> {
        path.into_iter()
            .flat_map(std::env::split_paths)
            .flat_map(|directory| {
                [
                    directory.join(format!(r"node_modules\@opencode\cli\bin\{name}.exe")),
                    directory.join(format!(r"node_modules\@opencode-ai\cli\bin\{name}.exe")),
                    directory.join(format!("{name}.exe")),
                ]
            })
            .map(|candidate| absolute_path(candidate, current_directory))
            .collect()
    }

    mod job {
        use std::os::windows::io::AsRawHandle;
        use std::process::Child;

        use windows_sys::Win32::Foundation::{CloseHandle, HANDLE};
        use windows_sys::Win32::System::JobObjects::{
            AssignProcessToJobObject, CreateJobObjectW, JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE,
            JOBOBJECT_EXTENDED_LIMIT_INFORMATION, JobObjectExtendedLimitInformation,
            SetInformationJobObject, TerminateJobObject,
        };

        /// A job that kills its remaining members when the handle closes, so
        /// even an early return reaps the whole process tree.
        pub(super) struct Job(HANDLE);

        impl Job {
            pub(super) fn kill_on_close() -> Option<Self> {
                unsafe {
                    let handle = CreateJobObjectW(std::ptr::null(), std::ptr::null());
                    if handle.is_null() {
                        return None;
                    }
                    let mut info: JOBOBJECT_EXTENDED_LIMIT_INFORMATION = std::mem::zeroed();
                    info.BasicLimitInformation.LimitFlags = JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE;
                    let set = SetInformationJobObject(
                        handle,
                        JobObjectExtendedLimitInformation,
                        std::ptr::from_ref(&info).cast(),
                        std::mem::size_of::<JOBOBJECT_EXTENDED_LIMIT_INFORMATION>() as u32,
                    );
                    if set == 0 {
                        CloseHandle(handle);
                        return None;
                    }
                    Some(Self(handle))
                }
            }

            pub(super) fn assign(&self, child: &Child) -> bool {
                unsafe { AssignProcessToJobObject(self.0, child.as_raw_handle() as HANDLE) != 0 }
            }

            pub(super) fn terminate(&self) {
                unsafe {
                    TerminateJobObject(self.0, 1);
                }
            }
        }

        impl Drop for Job {
            fn drop(&mut self) {
                unsafe {
                    CloseHandle(self.0);
                }
            }
        }
    }

    #[cfg(test)]
    mod tests {
        use super::super::{OpenCodeInstall, find_opencode_executable};
        use super::*;

        fn discovery_root(label: &str) -> PathBuf {
            std::env::temp_dir().join(format!(
                "hex-opencode-{label}-{}-{:?}",
                std::process::id(),
                thread::current().id()
            ))
        }

        fn write_fake_executable(path: &Path) {
            std::fs::create_dir_all(path.parent().unwrap()).unwrap();
            std::fs::write(path, "").unwrap();
        }

        #[test]
        fn opencode_requests_are_scoped_to_the_hex_workspace() {
            let root = discovery_root("workspace").join("scope %2F café");

            let command = opencode_command_in(&root).unwrap();
            let workspace = root.canonicalize().unwrap();
            let directory = workspace.to_str().unwrap();
            let directory = directory.strip_prefix(r"\\?\").unwrap_or(directory);
            let args = command
                .get_args()
                .map(|arg| arg.to_string_lossy().into_owned())
                .collect::<Vec<_>>();

            assert_eq!(command.get_current_dir(), Some(workspace.as_path()));
            assert_eq!(
                args,
                [
                    "api",
                    "--header",
                    &format!("x-opencode-directory:{}", encode_uri_component(directory))
                ]
            );
            std::fs::remove_dir_all(root.parent().unwrap()).unwrap();
        }

        #[test]
        fn opencode2_wins_over_an_opencode_earlier_on_path() {
            let root = discovery_root("prefer2");
            let old = root.join(r"first\opencode.exe");
            let new = root.join(r"second\opencode2.exe");
            write_fake_executable(&old);
            write_fake_executable(&new);
            let path = std::env::join_paths([root.join("first"), root.join("second")]).unwrap();

            let found = find_opencode_executable(None, Some(&path), None, Some(&root), |_| true);

            assert_eq!(found, OpenCodeInstall::Ready(new));
            std::fs::remove_dir_all(root).unwrap();
        }

        #[test]
        fn plain_opencode_is_found_when_opencode2_is_absent() {
            let root = discovery_root("plain");
            let shim = root.join(r"bin\opencode.exe");
            let real = root.join(r"bin\node_modules\@opencode-ai\cli\bin\opencode.exe");
            write_fake_executable(&shim);
            write_fake_executable(&real);
            let path = std::env::join_paths([root.join("bin")]).unwrap();

            let find = |is_v2: bool| {
                find_opencode_executable(None, Some(&path), None, Some(&root), |_| is_v2)
            };
            // The real npm-installed binary beats the shim exe alongside it.
            assert_eq!(find(true), OpenCodeInstall::Ready(real.clone()));
            assert_eq!(find(false), OpenCodeInstall::V1Only);

            std::fs::remove_file(&real).unwrap();
            assert_eq!(find(true), OpenCodeInstall::Ready(shim));

            std::fs::remove_dir_all(root).unwrap();
        }

        #[test]
        fn cancelled_generation_process_is_killed_promptly() {
            let mut command = crate::background_process::command("cmd");
            command.args(["/C", "ping -n 6 127.0.0.1 > NUL"]);
            let started = Instant::now();

            let error = run_command(
                command,
                Duration::from_secs(5),
                "test generation",
                &AtomicBool::new(true),
            )
            .err()
            .expect("cancelled command unexpectedly completed");

            assert!(error.to_string().contains("was cancelled"));
            assert!(started.elapsed() < Duration::from_secs(1));
        }

        #[test]
        fn descendant_holding_output_cannot_outlive_the_deadline() {
            let mut command = crate::background_process::command("cmd");
            command.args(["/C", "ping -n 6 127.0.0.1"]);
            let started = Instant::now();

            let _ = run_command(
                command,
                Duration::from_millis(500),
                "test descendant",
                &AtomicBool::new(false),
            );

            assert!(started.elapsed() < Duration::from_secs(2));
        }
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
            input_revision: None,
        }
    }

    fn profiles() -> Profiles {
        Profiles::new(Profile::new("default", "default prompt").ai_enabled(true))
            .application("Slack", Profile::new("slack", "slack prompt"))
            .browser_host("x.com", Profile::new("x", "x prompt"))
    }

    /// Writes an executable fixture script from a child process. Writing it
    /// here would leave its descriptor open in children other test threads
    /// fork meanwhile, and running it would then fail with "Text file busy".
    #[cfg(not(target_os = "windows"))]
    fn write_script(path: &Path, contents: &str) {
        use std::io::Write;
        let mut child = Command::new("/bin/sh")
            .args(["-c", "cat > \"$1\" && chmod 755 \"$1\"", "sh"])
            .arg(path)
            .stdin(Stdio::piped())
            .spawn()
            .unwrap();
        child
            .stdin
            .take()
            .unwrap()
            .write_all(contents.as_bytes())
            .unwrap();
        assert!(child.wait().unwrap().success());
    }

    #[cfg(not(target_os = "windows"))]
    fn fake_executable(path: &Path) {
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(path, "#!/bin/sh\n").unwrap();
        fs::set_permissions(path, fs::Permissions::from_mode(0o755)).unwrap();
    }

    /// Accepts one connection, failing instead of hanging when the client
    /// under test never connects.
    #[cfg(not(target_os = "windows"))]
    fn accept_within(listener: &std::net::TcpListener) -> std::net::TcpStream {
        listener.set_nonblocking(true).unwrap();
        let deadline = Instant::now() + Duration::from_secs(10);
        loop {
            match listener.accept() {
                Ok((stream, _)) => {
                    stream.set_nonblocking(false).unwrap();
                    return stream;
                }
                Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {
                    assert!(Instant::now() < deadline, "the client never connected");
                    thread::sleep(Duration::from_millis(5));
                }
                Err(error) => panic!("accept failed: {error}"),
            }
        }
    }

    /// Reads one authenticated request and returns its headers and body.
    #[cfg(not(target_os = "windows"))]
    fn receive_request(stream: &mut std::net::TcpStream, start_line: &str) -> (String, Vec<u8>) {
        use std::io::BufRead;
        stream
            .set_read_timeout(Some(Duration::from_secs(5)))
            .unwrap();
        let mut reader = std::io::BufReader::new(stream);
        let mut headers = String::new();
        loop {
            let mut line = String::new();
            assert!(reader.read_line(&mut line).unwrap() > 0);
            if line == "\r\n" {
                break;
            }
            headers.push_str(&line);
        }
        assert!(
            headers.starts_with(&format!("{start_line} HTTP/1.1\r\n")),
            "{headers}"
        );
        assert!(headers.contains("Authorization: Basic b3BlbmNvZGU6Zml4dHVyZS1wYXNzd29yZA==\r\n"));
        let length: usize = headers
            .lines()
            .find_map(|line| {
                let (key, value) = line.split_once(':')?;
                key.eq_ignore_ascii_case("content-length")
                    .then(|| value.trim().parse().unwrap())
            })
            .unwrap_or(0);
        if headers
            .to_ascii_lowercase()
            .contains("expect: 100-continue")
        {
            reader
                .get_mut()
                .write_all(b"HTTP/1.1 100 Continue\r\n\r\n")
                .unwrap();
        }
        let mut body = vec![0; length];
        reader.read_exact(&mut body).unwrap();
        (headers, body)
    }

    #[cfg(not(target_os = "windows"))]
    #[test]
    fn standard_install_location_finds_opencode_outside_the_gui_path() {
        let root = std::env::temp_dir().join(format!(
            "hex-opencode-discovery-{}-{:?}",
            std::process::id(),
            thread::current().id()
        ));
        for location in [
            ".opencode/bin/opencode2",
            ".vite-plus/bin/opencode2",
            "Library/pnpm/bin/opencode2",
        ] {
            let executable = root.join(location);
            fake_executable(&executable);

            let found = find_opencode_executable(
                None,
                Some(OsStr::new("/usr/bin:/bin")),
                Some(&root),
                Some(&root),
                |_| unreachable!("opencode2 is never probed"),
            );

            assert_eq!(found, OpenCodeInstall::Ready(executable));
            std::fs::remove_dir_all(&root).unwrap();
        }
    }

    #[cfg(not(target_os = "windows"))]
    #[test]
    fn catalog_http_decodes_large_responses_with_private_auth_and_workspace_scope() {
        let root = std::env::temp_dir().join(format!(
            "hex-opencode-workspace-{}-{:?}",
            std::process::id(),
            thread::current().id()
        ));
        let root = root.join("scope %2F caf\u{00e9}");
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let endpoint = format!("http://{}", listener.local_addr().unwrap());
        let (command, _) = opencode_http_command(
            &endpoint,
            "fixture-password",
            "/api/model",
            None,
            Some(&root),
        )
        .unwrap();
        let workspace = root.canonicalize().unwrap();
        assert_eq!(command.get_program(), curl_program().as_os_str());
        assert_eq!(command.get_current_dir(), Some(workspace.as_path()));
        for arg in command.get_args() {
            assert!(!arg.to_string_lossy().contains("fixture-password"));
            assert!(
                !arg.to_string_lossy()
                    .contains("b3BlbmNvZGU6Zml4dHVyZS1wYXNzd29yZA==")
            );
        }
        let scope_header = format!(
            "x-opencode-directory:{}\r\n",
            encode_uri_component(workspace.to_str().unwrap())
        );
        let wire_models: Vec<_> = (0..2048)
            .map(|index| {
                serde_json::json!({
                    "id": format!("rewrite-{index}"),
                    "providerID": "example",
                    "name": format!("Example Rewrite {index}"),
                    "enabled": true,
                    "capabilities": { "output": ["text"] },
                    "variants": [{ "id": "high" }]
                })
            })
            .collect();
        let models = serde_json::json!({ "data": wire_models }).to_string();
        let default = serde_json::json!({ "data": wire_models.last().unwrap() }).to_string();
        assert!(models.len() > 256 * 1024);
        let server = thread::spawn(move || {
            for (path, response) in [("/api/model", models), ("/api/model/default", default)] {
                let mut stream = accept_within(&listener);
                stream
                    .set_write_timeout(Some(Duration::from_secs(5)))
                    .unwrap();
                let (headers, _) = receive_request(&mut stream, &format!("GET {path}"));
                assert!(headers.contains(&scope_header));
                assert!(!headers.to_ascii_lowercase().contains("content-length:"));
                write!(
                    stream,
                    "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{response}",
                    response.len()
                )
                .unwrap();
            }
        });
        let models: ModelsResponse =
            opencode_api(&endpoint, "fixture-password", &root, "/api/model").unwrap();
        let default: DefaultModelResponse =
            opencode_api(&endpoint, "fixture-password", &root, "/api/model/default").unwrap();
        server.join().unwrap();
        let catalog = build_model_catalog(models.data, default.data);
        assert_eq!(catalog.models.len(), 2048);
        assert_eq!(catalog.default_key.as_deref(), Some("example/rewrite-2047"));
        assert_eq!(
            catalog.default_name.as_deref(),
            Some("Example Rewrite 2047")
        );
        for (index, model) in catalog.models.iter().enumerate() {
            assert_eq!(model.key, format!("example/rewrite-{index}"));
            assert_eq!(model.name, format!("Example Rewrite {index}"));
            assert_eq!(model.variants, ["high"]);
        }
        std::fs::remove_dir_all(root.parent().unwrap()).unwrap();
    }

    #[cfg(not(target_os = "windows"))]
    #[test]
    fn configured_relative_executable_is_resolved_before_the_workspace_changes() {
        let current = Path::new("/tmp/hex-launch");

        let found = find_opencode_executable(
            Some(PathBuf::from("bin/opencode2")),
            None,
            None,
            Some(current),
            |_| unreachable!("the override is never probed"),
        );

        assert_eq!(found, OpenCodeInstall::Ready(current.join("bin/opencode2")));
    }

    #[cfg(not(target_os = "windows"))]
    #[test]
    fn official_install_preserves_override_path_and_executable_checks() {
        let root = std::env::temp_dir().join(format!(
            "hex-opencode-precedence-{}-{:?}",
            std::process::id(),
            thread::current().id()
        ));
        let official = root.join(".opencode/bin/opencode2");
        let path_executable = root.join("path/opencode2");
        let fallback = root.join(".bun/bin/opencode2");
        for executable in [&official, &path_executable, &fallback] {
            fake_executable(executable);
        }
        let path = root.join("path");
        let find = |configured, path| {
            find_opencode_executable(configured, path, Some(&root), Some(&root), |_| true)
        };
        assert_eq!(
            find(Some("override/opencode2".into()), Some(path.as_os_str())),
            OpenCodeInstall::Ready(root.join("override/opencode2"))
        );
        assert_eq!(
            find(None, Some(path.as_os_str())),
            OpenCodeInstall::Ready(path_executable)
        );
        assert_eq!(find(None, None), OpenCodeInstall::Ready(official.clone()));
        fs::set_permissions(&official, fs::Permissions::from_mode(0o644)).unwrap();
        assert_eq!(find(None, None), OpenCodeInstall::Ready(fallback.clone()));
        fs::remove_file(&official).unwrap();
        fs::create_dir(&official).unwrap();
        assert_eq!(find(None, None), OpenCodeInstall::Ready(fallback));
        fs::remove_dir_all(root).unwrap();
    }

    #[cfg(not(target_os = "windows"))]
    #[test]
    fn version_manager_discovery_is_bounded_to_known_roots() {
        let root = std::env::temp_dir().join(format!(
            "hex-opencode-nvm-{}-{:?}",
            std::process::id(),
            thread::current().id()
        ));
        let executable = root.join(".nvm/versions/node/v24.4.1/bin/opencode2");
        fake_executable(&executable);

        let found = find_opencode_executable(None, None, Some(&root), Some(&root), |_| true);

        assert_eq!(found, OpenCodeInstall::Ready(executable));
        std::fs::remove_dir_all(root).unwrap();
    }

    /// An `opencode` answering `--version` with `version`, and `api get
    /// /api/info` only when `serves_api`.
    #[cfg(not(target_os = "windows"))]
    fn fake_opencode(path: &Path, version: &str, serves_api: bool) {
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        let api = if serves_api {
            "echo '{\"pid\":42,\"version\":\"fixture\"}'"
        } else {
            "echo 'Commands:' >&2; exit 1"
        };
        fs::write(
            path,
            format!(
                "#!/bin/sh\ncase \"$*\" in\n'--version') echo '{version}' ;;\n'api get /api/info') {api} ;;\n*) exit 2 ;;\nesac\n"
            ),
        )
        .unwrap();
        fs::set_permissions(path, fs::Permissions::from_mode(0o755)).unwrap();
    }

    #[cfg(not(target_os = "windows"))]
    #[test]
    fn opencode2_is_preferred_and_only_a_v2_opencode_is_accepted() {
        let root = std::env::temp_dir().join(format!(
            "hex-opencode-generation-{}-{:?}",
            std::process::id(),
            thread::current().id()
        ));
        let first = root.join("first/opencode");
        let second = root.join("second/opencode");
        let shim = root.join(".opencode/bin/opencode2");
        let path = std::env::join_paths([root.join("first"), root.join("second")]).unwrap();
        let find = || {
            find_opencode_executable(None, Some(&path), Some(&root), Some(&root), opencode_is_v2)
        };

        fake_opencode(&first, "1.19.5", false);
        assert_eq!(find(), OpenCodeInstall::V1Only);
        // A V1 snapshot build prints a prerelease version and rejects `api`.
        fake_opencode(&second, "0.0.0-dev-202610280534", false);
        assert_eq!(find(), OpenCodeInstall::V1Only);

        fake_opencode(&second, "opencode v2.2.0", false);
        assert_eq!(find(), OpenCodeInstall::Ready(second.clone()));
        fake_opencode(&second, "opencode v0.0.0-dev-88", true);
        assert_eq!(find(), OpenCodeInstall::Ready(second.clone()));

        // Any `opencode2` beats an `opencode` earlier in the search.
        fake_executable(&shim);
        assert_eq!(find(), OpenCodeInstall::Ready(shim));
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn release_versions_tell_the_generations_apart() {
        assert_eq!(release_is_v2("opencode v2.2.0\n"), Some(true));
        assert_eq!(release_is_v2("2.10.1"), Some(true));
        assert_eq!(release_is_v2("1.19.5\n"), Some(false));
        assert_eq!(release_is_v2("0.15.31"), Some(false));
        assert_eq!(release_is_v2("opencode v0.0.0-dev-88"), None);
        assert_eq!(release_is_v2("0.0.0-snapshot-pnpm-202511302105"), None);
        assert_eq!(release_is_v2("local"), None);
        assert_eq!(release_is_v2(""), None);
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

    #[cfg(not(target_os = "windows"))]
    #[test]
    fn cancelled_generation_process_is_killed_promptly() {
        let mut command = Command::new("/bin/sleep");
        command.arg("5");
        let cancelled = AtomicBool::new(true);
        let started = Instant::now();

        let error = match run_command(
            command,
            None,
            Duration::from_secs(5),
            "test generation",
            &cancelled,
        ) {
            Ok(_) => panic!("cancelled command unexpectedly completed"),
            Err(error) => error,
        };

        assert!(error.to_string().contains("was cancelled"));
        assert!(started.elapsed() < Duration::from_secs(1));
    }

    #[cfg(not(target_os = "windows"))]
    #[test]
    fn descendant_holding_output_cannot_outlive_the_command() {
        let mut command = Command::new("/bin/sh");
        command.args(["-c", "sleep 5 &"]);
        let started = Instant::now();

        let _ = run_command(
            command,
            None,
            Duration::from_millis(500),
            "test descendant",
            &AtomicBool::new(false),
        );

        assert!(started.elapsed() < Duration::from_secs(1));
    }

    #[test]
    fn command_output_is_bounded() {
        let error = read_bounded_output(std::io::Cursor::new(b"12345"), 4)
            .expect_err("oversized output unexpectedly succeeded");

        assert!(error.to_string().contains("exceeded 4 bytes"));
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

    #[cfg(not(target_os = "windows"))]
    #[test]
    fn service_discovery_supports_current_and_legacy_endpoints() {
        let root = std::env::temp_dir().join(format!(
            "hex-service-endpoints-{}-{:?}",
            std::process::id(),
            thread::current().id()
        ));
        fs::create_dir_all(&root).unwrap();
        let registration = root.join("service.json");
        fs::write(
            &registration,
            br#"{"pid":42,"version":"fixture-version","url":"http://127.0.0.1:1234","password":"registered-password"}"#,
        )
        .unwrap();
        fs::set_permissions(&registration, fs::Permissions::from_mode(0o600)).unwrap();
        let executable = root.join("opencode2");
        const INFO: &str = "printf '%s\\n' '{\"pid\":42,\"version\":\"fixture-version\"}'";
        const MISSING: &str = "echo 'HTTP 404 Not Found' >&2; exit 1";
        const SKEWED_MISSING: &str = "echo 'Warning: Server at http://127.0.0.1:4096 has version a; this client is b. Continuing anyway.' >&2; echo 'HTTP 404 Not Found' >&2; echo >&2; exit 1";
        for (info, status, health, succeeds, expected_calls) in [
            (
                SKEWED_MISSING,
                SKEWED_MISSING,
                INFO,
                true,
                "api get /api/info\napi get /api/status\napi get /api/health\ndebug paths\n",
            ),
            (
                "echo 'HTTP 404 Not Found' >&2; echo 'private trailing diagnostic' >&2; exit 1",
                INFO,
                INFO,
                false,
                "api get /api/info\n",
            ),
            (
                INFO,
                MISSING,
                MISSING,
                true,
                "api get /api/info\ndebug paths\n",
            ),
            (
                MISSING,
                INFO,
                MISSING,
                true,
                "api get /api/info\napi get /api/status\ndebug paths\n",
            ),
            (
                MISSING,
                MISSING,
                INFO,
                true,
                "api get /api/info\napi get /api/status\napi get /api/health\ndebug paths\n",
            ),
            (
                "echo 'HTTP 401 Unauthorized' >&2; exit 1",
                INFO,
                INFO,
                false,
                "api get /api/info\n",
            ),
            (
                "echo 'HTTP 500 Internal Server Error' >&2; exit 1",
                INFO,
                INFO,
                false,
                "api get /api/info\n",
            ),
            (
                "echo 'private diagnostic mentioning HTTP 404 Not Found' >&2; exit 1",
                INFO,
                INFO,
                false,
                "api get /api/info\n",
            ),
            (
                "echo 'private invalid response'",
                INFO,
                INFO,
                false,
                "api get /api/info\n",
            ),
            (
                MISSING,
                "echo 'private diagnostic' >&2; exit 1",
                INFO,
                false,
                "api get /api/info\napi get /api/status\n",
            ),
            (
                MISSING,
                MISSING,
                MISSING,
                false,
                "api get /api/info\napi get /api/status\napi get /api/health\n",
            ),
        ] {
            write_script(
                &executable,
                &format!(
                    "#!/bin/sh\nprintf '%s\\n' \"$*\" >> calls\ncase \"$*\" in\n'api get /api/info') {info} ;;\n'api get /api/status') {status} ;;\n'api get /api/health') {health} ;;\n'debug paths') printf 'state %s\\n' \"$PWD\" ;;\n*) exit 2 ;;\nesac\n"
                ),
            );
            // No fixture times out, so a busy machine gets a generous deadline.
            let result = discover_opencode_service_with(
                &executable,
                &root,
                None,
                Duration::from_secs(20),
                &AtomicBool::new(false),
            );
            assert_eq!(result.is_ok(), succeeds, "{result:?}");
            assert_eq!(
                fs::read_to_string(root.join("calls")).unwrap_or_default(),
                expected_calls,
                "fixtures: {info} / {status} / {health}: {result:?}"
            );
            if succeeds {
                assert_eq!(
                    result.unwrap(),
                    ("http://127.0.0.1:1234".into(), "registered-password".into())
                );
            } else {
                let error = format!("{:#}", result.unwrap_err());
                assert!(!error.contains("private"), "{error}");
                assert!(!error.contains("registered-password"), "{error}");
            }
            fs::remove_file(root.join("calls")).unwrap();
        }
        fs::remove_dir_all(root).unwrap();
    }

    /// A wrapper like Kit's Studio guard appends `--server` to every
    /// subcommand, so `debug paths` prints help and exits 1, and it points the
    /// CLI's own state elsewhere while the service registers in the default one.
    #[cfg(not(target_os = "windows"))]
    #[test]
    fn service_discovery_finds_the_default_registration_behind_a_wrapper() {
        let root = std::env::temp_dir().join(format!(
            "hex-service-wrapper-{}-{:?}",
            std::process::id(),
            thread::current().id()
        ));
        let reported = root.join("wrapper-state");
        let default_state = root.join("default-state");
        fs::create_dir_all(&reported).unwrap();
        fs::create_dir_all(&default_state).unwrap();
        let registration = default_state.join("service.json");
        fs::write(
            &registration,
            br#"{"id":"x","version":"fixture-version","url":"http://127.0.0.1:1234","pid":42,"password":"registered-password"}"#,
        )
        .unwrap();
        fs::set_permissions(&registration, fs::Permissions::from_mode(0o600)).unwrap();
        let executable = root.join("opencode2");
        const INFO: &str = "echo 'Warning: Server at http://127.0.0.1:4096 has version a; this client is b. Continuing anyway.' >&2; printf '%s\\n' '{\"pid\":42,\"version\":\"fixture-version\",\"urls\":[\"http://127.0.0.1:4096\"]}'";
        let guard_paths = "printf 'DESCRIPTION\\n  Show global paths (data, config, cache, state)\\n'; printf '\\nERROR\\n  Unrecognized flag: --server in command opencode debug paths\\n' >&2; exit 1";
        let reporting_paths = format!("printf 'state      %s\\n' '{}'", reported.display());
        for (paths, default, succeeds) in [
            (guard_paths, Some(default_state.as_path()), true),
            (
                reporting_paths.as_str(),
                Some(default_state.as_path()),
                true,
            ),
            (guard_paths, None, false),
            (reporting_paths.as_str(), None, false),
        ] {
            write_script(
                &executable,
                &format!(
                    "#!/bin/sh\ncase \"$*\" in\n'api get /api/info') {INFO} ;;\n'debug paths') {paths} ;;\n*) exit 2 ;;\nesac\n"
                ),
            );
            let result = discover_opencode_service_with(
                &executable,
                &root,
                default,
                Duration::from_secs(20),
                &AtomicBool::new(false),
            );
            if succeeds {
                assert_eq!(
                    result.unwrap(),
                    ("http://127.0.0.1:1234".into(), "registered-password".into())
                );
            } else {
                let error = format!("{:#}", result.unwrap_err());
                assert!(!error.contains("registered-password"), "{error}");
            }
        }
        // A registration for another server process never counts.
        fs::write(
            &registration,
            br#"{"version":"fixture-version","url":"http://127.0.0.1:1234","pid":43,"password":"stale-password"}"#,
        )
        .unwrap();
        let error = discover_opencode_service_with(
            &executable,
            &root,
            Some(&default_state),
            Duration::from_secs(20),
            &AtomicBool::new(false),
        )
        .unwrap_err();
        assert!(!format!("{error:#}").contains("stale-password"));
        fs::remove_dir_all(root).unwrap();
    }

    #[cfg(not(target_os = "windows"))]
    #[test]
    fn service_discovery_fallback_shares_deadline_and_observes_cancellation() {
        let root = std::env::temp_dir().join(format!(
            "hex-service-deadline-{}-{:?}",
            std::process::id(),
            thread::current().id()
        ));
        fs::create_dir_all(&root).unwrap();
        let executable = root.join("opencode2");
        write_script(
            &executable,
            "#!/bin/sh\nprintf '%s\\n' \"$*\" >> calls\ncase \"$*\" in\n'api get /api/info') echo 'HTTP 404 Not Found' >&2; exit 1 ;;\n'api get /api/status') sleep 2; echo 'HTTP 404 Not Found' >&2; exit 1 ;;\n'api get /api/health') sleep 10; echo '{\"pid\":42,\"version\":\"fixture-version\"}' ;;\n*) exit 2 ;;\nesac\n",
        );
        let error = discover_opencode_service_with(
            &executable,
            &root,
            None,
            // Leaves the health probe ~4 s to start on a busy machine.
            Duration::from_secs(6),
            &AtomicBool::new(false),
        )
        .unwrap_err();
        assert!(error.to_string().contains("exceeded"), "{error}");
        assert_eq!(
            fs::read_to_string(root.join("calls")).unwrap(),
            "api get /api/info\napi get /api/status\napi get /api/health\n"
        );
        fs::remove_file(root.join("calls")).unwrap();
        let error = discover_opencode_service_with(
            &executable,
            &root,
            None,
            Duration::from_secs(2),
            &AtomicBool::new(true),
        )
        .unwrap_err();
        assert!(error.to_string().contains("was cancelled"), "{error}");
        assert!(!root.join("calls").exists());
        fs::remove_dir_all(root).unwrap();
    }

    #[cfg(not(target_os = "windows"))]
    #[test]
    fn service_discovery_uses_private_active_registration_without_rewriting_config() {
        let root = std::env::temp_dir().join(format!(
            "hex-service-registration-{}-{:?}",
            std::process::id(),
            thread::current().id()
        ));
        let state = root.join("state");
        fs::create_dir_all(&state).unwrap();
        let registration = state.join("service.json");
        let bytes = br#"{"pid":42,"version":"fixture-version","url":"http://127.0.0.1:1234","password":"registered-password"}"#;
        fs::write(&registration, bytes).unwrap();
        fs::set_permissions(&registration, fs::Permissions::from_mode(0o600)).unwrap();
        let config = root.join("config/service.json");
        for configured in [false, true] {
            if configured {
                fs::create_dir_all(config.parent().unwrap()).unwrap();
                fs::write(&config, br#"{"password":"different-configured-password"}"#).unwrap();
            }
            let before = fs::read(&config).ok();
            assert_eq!(
                read_service_registration(&state, 42, "fixture-version").unwrap(),
                ("http://127.0.0.1:1234".into(), "registered-password".into())
            );
            assert_eq!(fs::read(&config).ok(), before);
            assert_eq!(fs::read(&registration).unwrap(), bytes);
        }
        assert!(read_service_registration(&state, 43, "fixture-version").is_err());
        assert!(read_service_registration(&state, 42, "other-version").is_err());
        fs::set_permissions(&registration, fs::Permissions::from_mode(0o644)).unwrap();
        assert!(read_service_registration(&state, 42, "fixture-version").is_err());
        fs::set_permissions(&registration, fs::Permissions::from_mode(0o600)).unwrap();
        let legacy = state.join("service-legacy.json");
        fs::copy(&registration, &legacy).unwrap();
        assert!(read_service_registration(&state, 42, "fixture-version").is_ok());
        fs::write(
            &legacy,
            String::from_utf8_lossy(bytes).replace("registered-password", "conflicting-password"),
        )
        .unwrap();
        assert!(read_service_registration(&state, 42, "fixture-version").is_err());
        fs::remove_file(&legacy).unwrap();
        let target = state.join("fixture-registration");
        fs::rename(&registration, &target).unwrap();
        std::os::unix::fs::symlink(&target, &registration).unwrap();
        assert!(read_service_registration(&state, 42, "fixture-version").is_err());
        fs::remove_dir_all(root).unwrap();
    }

    /// Answers each expected route in order and returns the request bodies it saw.
    #[cfg(not(target_os = "windows"))]
    fn generation_server(
        routes: Vec<(&'static str, u16, &'static str)>,
    ) -> (String, thread::JoinHandle<Vec<Vec<u8>>>) {
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let endpoint = format!("http://{}", listener.local_addr().unwrap());
        let server = thread::spawn(move || {
            routes
                .into_iter()
                .map(|(path, status, response)| {
                    let mut stream = accept_within(&listener);
                    let (_, body) = receive_request(&mut stream, &format!("POST {path}"));
                    write!(
                        stream,
                        "HTTP/1.1 {status} Fixture\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{response}",
                        response.len()
                    )
                    .unwrap();
                    body
                })
                .collect()
        });
        (endpoint, server)
    }

    #[cfg(not(target_os = "windows"))]
    #[test]
    fn generation_pipes_credentials_and_large_json_without_argv_exposure() {
        let (endpoint, server) = generation_server(vec![(
            "/api/experimental/generate",
            200,
            r#"{"data":{"text":"fixture result"}}"#,
        )]);
        let prompt = format!(
            "fixture prompt: \"quoted\"\\path\n\r\t\u{00e9}{}",
            "a".repeat(1024 * 1024)
        );
        let data = serde_json::to_string(&Request {
            prompt: &prompt,
            model: None,
        })
        .unwrap();
        let (command, _) = opencode_http_command(
            &endpoint,
            "fixture-password",
            "/api/experimental/generate",
            Some(&data),
            None,
        )
        .unwrap();
        for arg in command.get_args() {
            assert!(!arg.to_string_lossy().contains("fixture"));
            assert!(!arg.to_string_lossy().contains(&prompt));
        }
        let response = post_generation(
            &endpoint,
            "fixture-password",
            &data,
            Duration::from_secs(5),
            &AtomicBool::new(false),
        )
        .unwrap();
        assert!(matches!(response, Response::Success { data } if data.text == "fixture result"));
        assert_eq!(server.join().unwrap(), [data.as_bytes()]);
    }

    #[cfg(not(target_os = "windows"))]
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
            let (endpoint, server) = generation_server(routes);
            let result = post_generation(
                &endpoint,
                "fixture-password",
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

    #[cfg(not(target_os = "windows"))]
    #[test]
    fn http_status_is_split_from_the_body() {
        let response = split_http_status(b"{\"data\":1}\n200".to_vec()).unwrap();
        assert_eq!(
            (response.status, response.body.as_slice()),
            (200, &b"{\"data\":1}"[..])
        );
        let response = split_http_status(b"\n404".to_vec()).unwrap();
        assert_eq!((response.status, response.body.as_slice()), (404, &b""[..]));
        assert!(split_http_status(b"no status".to_vec()).is_none());
    }

    #[cfg(not(target_os = "windows"))]
    #[test]
    fn generation_refuses_non_loopback_or_ambiguous_endpoints() {
        for endpoint in [
            "https://127.0.0.1:1234",
            "http://example.com",
            "http://127.0.0.1.example.com",
            "http://user:password@127.0.0.1",
            "http://127.0.0.1/base",
            "http://127.0.0.1/?query",
            "http://127.0.0.1/#fragment",
            "http://127.0.0.1:0",
            "file:///tmp/fixture",
        ] {
            assert!(
                opencode_http_command(
                    endpoint,
                    "fixture-password",
                    "/api/generate",
                    Some("{}"),
                    None,
                )
                .is_err()
            );
        }
        for (endpoint, expected) in [
            (
                "http://localhost:1234",
                "http://127.0.0.1:1234/api/generate",
            ),
            ("http://0.0.0.0:1234", "http://127.0.0.1:1234/api/generate"),
            ("http://[::]:1234", "http://[::1]:1234/api/generate"),
        ] {
            let (_, input) = opencode_http_command(
                endpoint,
                "fixture-password",
                "/api/generate",
                Some("{}"),
                None,
            )
            .unwrap();
            assert!(String::from_utf8(input).unwrap().contains(expected));
        }
    }

    #[cfg(not(target_os = "windows"))]
    #[test]
    fn generation_http_wait_observes_cancellation_and_deadline() {
        for cancel in [false, true] {
            let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
            let endpoint = format!("http://{}", listener.local_addr().unwrap());
            let cancelled = std::sync::Arc::new(AtomicBool::new(false));
            let flag = cancelled.clone();
            let server = thread::spawn(move || {
                let mut stream = accept_within(&listener);
                receive_request(&mut stream, "POST /api/generate");
                flag.store(cancel, Ordering::Release);
                let mut byte = [0];
                assert_eq!(stream.read(&mut byte).unwrap(), 0);
            });
            let (command, input) = opencode_http_command(
                &endpoint,
                "fixture-password",
                "/api/generate",
                Some("{}"),
                None,
            )
            .unwrap();
            let started = Instant::now();
            let error = run_command(
                command,
                Some(input),
                Duration::from_millis(300),
                "test generation",
                &cancelled,
            )
            .err()
            .unwrap();
            assert!(
                error
                    .to_string()
                    .contains(if cancel { "cancelled" } else { "exceeded" })
            );
            server.join().unwrap();
            assert!(started.elapsed() < Duration::from_secs(2));
        }
    }

    #[cfg(not(target_os = "windows"))]
    #[test]
    fn blocked_request_pipe_does_not_outlive_the_deadline() {
        // From PATH: the Nix build sandbox has no /bin/sleep.
        let mut command = Command::new("sleep");
        command.arg("5");
        let started = Instant::now();
        let error = run_command(
            command,
            Some(vec![b'x'; 1024 * 1024]),
            Duration::from_millis(100),
            "test input",
            &AtomicBool::new(false),
        )
        .err()
        .unwrap();
        assert!(error.to_string().contains("exceeded"));
        assert!(started.elapsed() < Duration::from_secs(1));
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

    #[cfg(not(target_os = "windows"))]
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
        let (endpoint, server) = generation_server(vec![
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
            |data, remaining, cancelled| {
                post_generation(&endpoint, "fixture-password", data, remaining, cancelled)
            },
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
    #[ignore = "requires the installed opencode2 CLI and its managed service"]
    fn live_catalog() {
        let catalog =
            load_model_catalog().unwrap_or_else(|_| panic!("could not load live model catalog"));
        println!("{}", catalog.models.len());
    }

    #[test]
    #[ignore = "requires the running opencode2 service and a configured model"]
    fn live_generation_returns_text_without_creating_a_session() {
        assert_eq!(
            generate("Reply with exactly: test", None, Duration::from_secs(30)).unwrap(),
            "test"
        );
    }
}
