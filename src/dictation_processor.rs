use std::collections::HashSet;
use std::io::Read;
use std::os::unix::fs::PermissionsExt;
use std::path::PathBuf;
use std::process::{Command, ExitStatus, Stdio};
use std::sync::OnceLock;
use std::sync::atomic::{AtomicBool, Ordering};
use std::thread;
use std::time::{Duration, Instant};

use color_eyre::eyre::{Result, WrapErr, eyre};
use serde::{Deserialize, Serialize};

use crate::context::{ContextSelector, ContextSnapshot};

const PROTOCOL_PROMPT: &str = "You transform dictated speech into replacement text. Return only the text that should be pasted. Do not add an explanation, label, alternative, or Markdown fence.";
const VOICE_ACTION_PROTOCOL_PROMPT: &str = "You execute a one-off voice instruction. When selected text is provided, transform or use it as instructed. When no text is selected, generate the requested text. Return only the exact paste-ready result without an explanation, label, alternative, or Markdown fence.";
static MODEL_CATALOG: OnceLock<ModelCatalog> = OnceLock::new();

#[derive(Clone)]
pub struct Profile {
    name: String,
    prompt: String,
    model: Option<Model>,
    deadline: Option<Duration>,
}

impl Profile {
    pub fn new(name: impl Into<String>, prompt: impl Into<String>) -> Self {
        Self {
            name: name.into(),
            prompt: prompt.into(),
            model: None,
            deadline: None,
        }
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

#[derive(Clone, Serialize)]
struct Model {
    #[serde(rename = "providerID")]
    provider: String,
    id: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    variant: Option<String>,
}

#[derive(Clone)]
struct ContextualProfile {
    selector: ContextSelector,
    profile: Option<Profile>,
}

#[derive(Clone)]
pub struct Profiles {
    default: Option<Profile>,
    deadline: Duration,
    contextual: Vec<ContextualProfile>,
}

impl Profiles {
    pub fn new(default: Option<Profile>) -> Self {
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
        self.push(
            ContextSelector::application(application.into()),
            Some(profile),
        )
    }

    pub fn application_raw(self, application: impl Into<String>) -> Self {
        self.push(ContextSelector::application(application.into()), None)
    }

    pub fn browser_host(self, host: impl Into<String>, profile: Profile) -> Self {
        self.push(ContextSelector::browser_host(host.into()), Some(profile))
    }

    pub fn browser_host_raw(self, host: impl Into<String>) -> Self {
        self.push(ContextSelector::browser_host(host.into()), None)
    }

    fn push(mut self, selector: ContextSelector, profile: Option<Profile>) -> Self {
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

    fn select<'a>(&'a self, context: &ContextSnapshot) -> Option<&'a Profile> {
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
        selected
            .map(|selected| selected.profile.as_ref())
            .unwrap_or(self.default.as_ref())
    }

    pub fn processes(&self, context: &ContextSnapshot) -> bool {
        self.select(context).is_some()
    }

    pub fn process_cancellable(
        &self,
        transcript: &str,
        context: &ContextSnapshot,
        cancelled: &AtomicBool,
    ) -> Processed {
        let Some(profile) = self.select(context) else {
            return Processed {
                text: transcript.into(),
                observation: None,
            };
        };
        let prompt = prompt(profile, transcript, context);
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
                }
            }
            Ok(_) => Processed {
                text: transcript.into(),
                observation: Some(ProcessingObservation {
                    profile: profile.name.clone(),
                    latency_ms: started.elapsed().as_millis() as u64,
                    fallback: Some("processor returned empty text".into()),
                }),
            },
            Err(error) => Processed {
                text: transcript.into(),
                observation: Some(ProcessingObservation {
                    profile: profile.name.clone(),
                    latency_ms: started.elapsed().as_millis() as u64,
                    fallback: Some(error.to_string()),
                }),
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
                }
            }
            Ok(_) => voice_action_failure(started, "processor returned empty text".into()),
            Err(error) => voice_action_failure(started, error.to_string()),
        }
    }
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
    #[serde(rename = "modelID")]
    model_id: String,
    #[serde(rename = "providerID")]
    provider_id: String,
    name: String,
    enabled: bool,
    status: String,
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

pub fn load_model_catalog() -> Result<ModelCatalog> {
    if let Some(catalog) = MODEL_CATALOG.get() {
        return Ok(catalog.clone());
    }
    let models: ModelsResponse = opencode_api("/api/model")?;
    let default: DefaultModelResponse = opencode_api("/api/model/default")?;
    let mut seen = HashSet::new();
    let choices = models
        .data
        .into_iter()
        .filter(|model| {
            model.enabled
                && model.status == "active"
                && model
                    .capabilities
                    .output
                    .iter()
                    .any(|output| output == "text")
        })
        .filter_map(|model| {
            let key = format!("{}/{}", model.provider_id, model.model_id);
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
    let default = default.data;
    let catalog = ModelCatalog {
        default_key: default
            .as_ref()
            .map(|model| format!("{}/{}", model.provider_id, model.model_id)),
        default_name: default.map(|model| model.name),
        models: choices,
    };
    let _ = MODEL_CATALOG.set(catalog.clone());
    Ok(catalog)
}

pub fn opencode_installed() -> bool {
    let executable = opencode_executable();
    if executable.components().count() > 1 {
        return is_executable(&executable);
    }
    std::env::var_os("PATH").is_some_and(|path| {
        std::env::split_paths(&path).any(|directory| is_executable(&directory.join(&executable)))
    })
}

fn is_executable(path: &std::path::Path) -> bool {
    path.metadata()
        .is_ok_and(|metadata| metadata.is_file() && metadata.permissions().mode() & 0o111 != 0)
}

fn opencode_api<T: for<'de> Deserialize<'de>>(path: &str) -> Result<T> {
    let mut command = Command::new(opencode_executable());
    command.args(["api", "get", path]).stdin(Stdio::null());
    let output = run_command(
        command,
        Duration::from_secs(10),
        path,
        &AtomicBool::new(false),
    )?;
    if !output.status.success() {
        return Err(eyre!(
            "OpenCode {path} exited with {}: {}",
            output.status,
            String::from_utf8_lossy(&output.stderr).trim()
        ));
    }
    let value: serde_json::Value = serde_json::from_slice(&output.stdout)
        .wrap_err_with(|| format!("OpenCode {path} returned invalid JSON"))?;
    if let Some(message) = value.get("message").and_then(|message| message.as_str())
        && value.get("_tag").is_some()
    {
        return Err(eyre!("OpenCode {path} failed: {message}"));
    }
    serde_json::from_value(value)
        .wrap_err_with(|| format!("OpenCode {path} returned an invalid response"))
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

fn voice_action_prompt(
    instruction: &str,
    selected_text: Option<&str>,
    context: &ContextSnapshot,
) -> String {
    let application = context.application.as_deref().unwrap_or("unknown");
    let host = context.browser_host().unwrap_or("none");
    let selection = selected_text.map_or_else(
        || "No text was selected. Generate the requested paste-ready text.".into(),
        |text| format!("Selected text ({} UTF-8 bytes):\n{text}", text.len()),
    );
    format!(
        "{VOICE_ACTION_PROTOCOL_PROMPT}\n\nForeground application: {application}\nBrowser host: {host}\n\nDictated instruction ({} UTF-8 bytes):\n{instruction}\n\n{selection}",
        instruction.len()
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
    let request = Request { prompt, model };
    let data = serde_json::to_string(&request)?;
    let started = Instant::now();
    for attempt in 0..2 {
        let remaining = deadline.saturating_sub(started.elapsed());
        if remaining.is_zero() {
            return Err(eyre!(
                "opencode2 /api/generate exceeded {} seconds",
                deadline.as_secs()
            ));
        }
        let mut command = Command::new(opencode_executable());
        command
            .args(["api", "post", "/api/generate", "--data", &data])
            .stdin(Stdio::null());
        let output = run_command(command, remaining, "/api/generate", cancelled)?;
        let status = output.status;
        if !status.success() {
            return Err(eyre!(
                "opencode2 exited with {status}: {}",
                String::from_utf8_lossy(&output.stderr).trim()
            ));
        }
        let response: Response = serde_json::from_slice(&output.stdout)
            .wrap_err("opencode2 returned an invalid generation response")?;
        match response {
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
            return Err(eyre!("opencode2 /api/generate was cancelled"));
        }
        if started.elapsed() >= deadline {
            return Err(eyre!(
                "opencode2 /api/generate exceeded {} seconds",
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

fn run_command(
    mut command: Command,
    deadline: Duration,
    operation: &str,
    cancelled: &AtomicBool,
) -> Result<CommandOutput> {
    let mut child = command
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
    let stdout = thread::spawn(move || read_output(stdout));
    let stderr = thread::spawn(move || read_output(stderr));
    let started = Instant::now();
    let status = loop {
        if let Some(status) = child.try_wait().wrap_err("could not inspect opencode2")? {
            break status;
        }
        if cancelled.load(Ordering::Acquire) {
            let _ = child.kill();
            let _ = child.wait();
            let _ = stdout.join();
            let _ = stderr.join();
            return Err(eyre!("opencode2 {operation} was cancelled"));
        }
        if started.elapsed() >= deadline {
            let _ = child.kill();
            let _ = child.wait();
            let _ = stdout.join();
            let _ = stderr.join();
            return Err(eyre!(
                "opencode2 {operation} exceeded {} seconds",
                deadline.as_secs()
            ));
        }
        thread::sleep(Duration::from_millis(20));
    };
    let stdout = stdout
        .join()
        .map_err(|_| eyre!("opencode2 stdout reader panicked"))??;
    let stderr = stderr
        .join()
        .map_err(|_| eyre!("opencode2 stderr reader panicked"))??;
    Ok(CommandOutput {
        status,
        stdout,
        stderr,
    })
}

fn read_output(mut output: impl Read) -> Result<Vec<u8>> {
    let mut bytes = Vec::new();
    output
        .read_to_end(&mut bytes)
        .wrap_err("could not read opencode2 output")?;
    Ok(bytes)
}

fn opencode_executable() -> PathBuf {
    std::env::var_os("VOICE_CONTROL_OPENCODE_CLI")
        .map(PathBuf::from)
        .or_else(|| dirs::home_dir().map(|home| home.join(".bun/bin/opencode2")))
        .filter(|path| path.is_file())
        .unwrap_or_else(|| "opencode2".into())
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
        Profiles::new(Some(Profile::new("default", "default prompt")))
            .application("Slack", Profile::new("slack", "slack prompt"))
            .browser_host("x.com", Profile::new("x", "x prompt"))
    }

    #[test]
    fn browser_host_beats_application_and_application_beats_default() {
        let profiles = profiles();
        assert_eq!(
            profiles
                .select(&context("Brave Browser", Some("https://x.com/home")))
                .unwrap()
                .name,
            "x"
        );
        assert_eq!(
            profiles.select(&context("Slack", None)).unwrap().name,
            "slack"
        );
        assert_eq!(
            profiles.select(&context("Zed", None)).unwrap().name,
            "default"
        );
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
        let profiles =
            Profiles::new(Some(Profile::new("default", "default prompt"))).application_raw("Slack");

        assert!(profiles.select(&context("Slack", None)).is_none());
        assert_eq!(
            profiles
                .select(&context("Zed", None))
                .expect("default profile")
                .name,
            "default"
        );
    }

    #[test]
    fn cancelled_generation_process_is_killed_promptly() {
        let mut command = Command::new("/bin/sleep");
        command.arg("5");
        let cancelled = AtomicBool::new(true);
        let started = Instant::now();

        let error = match run_command(
            command,
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

    #[test]
    fn only_model_unavailability_is_retried() {
        assert!(retryable_generation_error(
            "Model unavailable: openai/gpt-5.6-sol"
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

    #[test]
    #[ignore = "requires the running opencode2 service and a configured model"]
    fn live_generation_returns_text_without_creating_a_session() {
        assert_eq!(
            generate("Reply with exactly: test", None, Duration::from_secs(30)).unwrap(),
            "test"
        );
    }
}
