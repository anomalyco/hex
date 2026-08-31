use std::collections::HashSet;
use std::ffi::OsStr;
use std::fs;
use std::io::{Read, Write};
use std::os::unix::fs::{MetadataExt, OpenOptionsExt, PermissionsExt};
use std::os::unix::process::CommandExt;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, ExitStatus, Stdio};
use std::sync::atomic::{AtomicBool, Ordering};
use std::thread;
use std::time::{Duration, Instant};

use color_eyre::eyre::{Result, WrapErr, eyre};
use serde::{Deserialize, Serialize};

use crate::context::{ContextSelector, ContextSnapshot};
use crate::text_replacements::ReplacementSet;

const PROTOCOL_PROMPT: &str = "You transform dictated speech into replacement text. Return only the text that should be pasted. Do not add an explanation, label, alternative, or Markdown fence.";
const VOICE_ACTION_PROTOCOL_PROMPT: &str = "You execute a one-off voice instruction. When selected text is provided, transform or use it as instructed. When no text is selected, generate the requested text. Return only the exact paste-ready result without an explanation, label, alternative, or Markdown fence.";
const MAX_OPENCODE_OUTPUT_BYTES: u64 = 16 * 1024 * 1024;

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

pub fn load_model_catalog() -> Result<ModelCatalog> {
    let (endpoint, password) =
        discover_opencode_service(Duration::from_secs(10), &AtomicBool::new(false))?;
    let workspace = crate::app_paths::opencode_workspace()?;
    let models: ModelsResponse = opencode_api(&endpoint, &password, &workspace, "/api/model")?;
    let default: DefaultModelResponse =
        opencode_api(&endpoint, &password, &workspace, "/api/model/default")?;
    Ok(build_model_catalog(models.data, default.data))
}

fn build_model_catalog(models: Vec<ModelInfo>, default: Option<ModelInfo>) -> ModelCatalog {
    let mut seen = HashSet::new();
    let mut choices: Vec<ModelChoice> = models
        .into_iter()
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
    if let Some(model) = default.as_ref().filter(|model| model_is_available(model)) {
        let key = format!("{}/{}", model.provider_id, model.id);
        if seen.insert(key.clone()) {
            choices.push(ModelChoice {
                key,
                name: model.name.clone(),
                provider: model.provider_id.clone(),
                variants: model
                    .variants
                    .iter()
                    .map(|variant| variant.id.clone())
                    .collect(),
            });
        }
    }
    ModelCatalog {
        default_key: default
            .as_ref()
            .map(|model| format!("{}/{}", model.provider_id, model.id)),
        default_name: default.map(|model| model.name),
        models: choices,
    }
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

fn opencode_api<T: for<'de> Deserialize<'de>>(
    endpoint: &str,
    password: &str,
    workspace: &Path,
    path: &str,
) -> Result<T> {
    // The CLI can truncate large catalog responses when stdout is piped.
    let (command, input) = opencode_http_command(endpoint, password, path, None, Some(workspace))?;
    let output = run_command(
        command,
        Some(input),
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
        transformations: Vec::new(),
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
    let (endpoint, password) =
        discover_opencode_service(deadline.saturating_sub(started.elapsed()), cancelled)?;
    for attempt in 0..2 {
        let remaining = deadline.saturating_sub(started.elapsed());
        if remaining.is_zero() {
            return Err(eyre!(
                "opencode2 /api/generate exceeded {} seconds",
                deadline.as_secs()
            ));
        }
        let (command, input) =
            opencode_http_command(&endpoint, &password, "/api/generate", Some(&data), None)?;
        let output = run_command(command, Some(input), remaining, "/api/generate", cancelled)?;
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

fn discover_opencode_service(
    deadline: Duration,
    cancelled: &AtomicBool,
) -> Result<(String, String)> {
    let started = Instant::now();
    let workspace = crate::app_paths::opencode_workspace()?;
    fs::create_dir_all(&workspace)?;
    let executable = opencode_executable();
    let run = |args: &[&str]| -> Result<String> {
        let mut command = Command::new(&executable);
        command.current_dir(&workspace).args(args);
        let output = run_command(
            command,
            None,
            deadline.saturating_sub(started.elapsed()),
            "service discovery",
            cancelled,
        )?;
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
    struct Health {
        pid: u32,
        version: String,
    }
    let health: Health = serde_json::from_str(&run(&["api", "get", "/api/health"])?)
        .map_err(|_| eyre!("OpenCode returned an invalid service health response"))?;
    let paths = run(&["debug", "paths"])?;
    let state = paths
        .lines()
        .find_map(|line| {
            let (key, value) = line.split_once(char::is_whitespace)?;
            (key == "state").then(|| Path::new(value.trim_start()))
        })
        .filter(|path| path.is_absolute())
        .ok_or_else(|| eyre!("OpenCode did not report its state directory"))?;
    read_service_registration(state, health.pid, &health.version)
}

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
    let mut command = Command::new("/usr/bin/curl");
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
    command.args([
        "--disable",
        "--silent",
        "--show-error",
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
    let status = loop {
        match child.try_wait() {
            Ok(Some(status)) => break status,
            Ok(None) => {}
            Err(error) => {
                terminate_process_group(&mut child);
                let _ = stdout.join();
                let _ = stderr.join();
                if let Some(input) = input {
                    let _ = input.join();
                }
                return Err(error).wrap_err("could not inspect opencode2");
            }
        }
        if cancelled.load(Ordering::Acquire) {
            terminate_process_group(&mut child);
            let _ = stdout.join();
            let _ = stderr.join();
            if let Some(input) = input {
                let _ = input.join();
            }
            return Err(eyre!("opencode2 {operation} was cancelled"));
        }
        if started.elapsed() >= deadline {
            terminate_process_group(&mut child);
            let _ = stdout.join();
            let _ = stderr.join();
            if let Some(input) = input {
                let _ = input.join();
            }
            return Err(eyre!(
                "opencode2 {operation} exceeded {} seconds",
                deadline.as_secs()
            ));
        }
        thread::sleep(Duration::from_millis(20));
    };
    kill_process_group(child.id());
    if let Some(input) = input {
        let result = input
            .join()
            .map_err(|_| eyre!("OpenCode input writer panicked"))?;
        if status.success() {
            result.wrap_err("could not write OpenCode request")?;
        }
    }
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

fn terminate_process_group(child: &mut Child) {
    kill_process_group(child.id());
    let _ = child.kill();
    let _ = child.wait();
}

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
    let current_directory = std::env::current_dir().ok();
    find_opencode_executable(
        std::env::var_os("VOICE_CONTROL_OPENCODE_CLI").map(PathBuf::from),
        std::env::var_os("PATH").as_deref(),
        dirs::home_dir().as_deref(),
        current_directory.as_deref(),
    )
    .unwrap_or_else(|| "opencode2".into())
}

fn find_opencode_executable(
    configured: Option<PathBuf>,
    path: Option<&OsStr>,
    home: Option<&Path>,
    current_directory: Option<&Path>,
) -> Option<PathBuf> {
    if let Some(configured) = configured {
        return Some(absolute_path(configured, current_directory));
    }
    let path_candidates = path
        .into_iter()
        .flat_map(std::env::split_paths)
        .map(|directory| absolute_path(directory.join("opencode2"), current_directory));
    let home_candidates = home.into_iter().flat_map(|home| {
        [
            home.join(".opencode/bin/opencode2"),
            home.join(".bun/bin/opencode2"),
            home.join("Library/pnpm/opencode2"),
            home.join("Library/pnpm/bin/opencode2"),
            home.join(".yarn/bin/opencode2"),
            home.join(".config/yarn/global/node_modules/.bin/opencode2"),
            home.join(".npm-global/bin/opencode2"),
            home.join(".local/bin/opencode2"),
            home.join("bin/opencode2"),
            home.join(".volta/bin/opencode2"),
            home.join(".asdf/shims/opencode2"),
            home.join(".local/share/mise/shims/opencode2"),
            home.join(".nodenv/shims/opencode2"),
        ]
    });
    path_candidates
        .chain(home_candidates)
        .chain(version_manager_candidates(home))
        .chain([
            PathBuf::from("/opt/homebrew/bin/opencode2"),
            PathBuf::from("/usr/local/bin/opencode2"),
        ])
        .find(|path| is_executable(path))
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

fn version_manager_candidates(home: Option<&Path>) -> Vec<PathBuf> {
    let Some(home) = home else {
        return Vec::new();
    };
    let roots = [
        (home.join(".nvm/versions/node"), "bin/opencode2"),
        (
            home.join("Library/Application Support/fnm/node-versions"),
            "installation/bin/opencode2",
        ),
        (
            home.join(".local/share/fnm/node-versions"),
            "installation/bin/opencode2",
        ),
        (
            home.join(".fnm/node-versions"),
            "installation/bin/opencode2",
        ),
    ];
    roots
        .into_iter()
        .flat_map(|(root, suffix)| {
            fs::read_dir(root)
                .into_iter()
                .flatten()
                .flatten()
                .take(64)
                .map(move |entry| entry.path().join(suffix))
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
    fn standard_install_location_finds_opencode_outside_the_gui_path() {
        let root = std::env::temp_dir().join(format!(
            "hex-opencode-discovery-{}-{:?}",
            std::process::id(),
            thread::current().id()
        ));
        for location in [".opencode/bin/opencode2", "Library/pnpm/bin/opencode2"] {
            let executable = root.join(location);
            std::fs::create_dir_all(executable.parent().unwrap()).unwrap();
            std::fs::write(&executable, "#!/bin/sh\n").unwrap();
            let mut permissions = executable.metadata().unwrap().permissions();
            permissions.set_mode(0o755);
            std::fs::set_permissions(&executable, permissions).unwrap();

            let found = find_opencode_executable(
                None,
                Some(OsStr::new("/usr/bin:/bin")),
                Some(&root),
                Some(&root),
            );

            assert_eq!(found, Some(executable));
            std::fs::remove_dir_all(&root).unwrap();
        }
    }

    #[test]
    fn catalog_http_decodes_large_responses_with_private_auth_and_workspace_scope() {
        use std::io::BufRead;

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
        assert_eq!(command.get_program(), OsStr::new("/usr/bin/curl"));
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
                let (mut stream, _) = listener.accept().unwrap();
                stream
                    .set_read_timeout(Some(Duration::from_secs(5)))
                    .unwrap();
                stream
                    .set_write_timeout(Some(Duration::from_secs(5)))
                    .unwrap();
                let mut reader = std::io::BufReader::new(&mut stream);
                let mut headers = String::new();
                loop {
                    let mut line = String::new();
                    assert!(reader.read_line(&mut line).unwrap() > 0);
                    if line == "\r\n" {
                        break;
                    }
                    headers.push_str(&line);
                }
                assert!(headers.starts_with(&format!("GET {path} HTTP/1.1\r\n")));
                assert!(
                    headers
                        .contains("Authorization: Basic b3BlbmNvZGU6Zml4dHVyZS1wYXNzd29yZA==\r\n")
                );
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

    #[test]
    fn configured_relative_executable_is_resolved_before_the_workspace_changes() {
        let current = Path::new("/tmp/hex-launch");

        let found = find_opencode_executable(
            Some(PathBuf::from("bin/opencode2")),
            None,
            None,
            Some(current),
        );

        assert_eq!(found, Some(current.join("bin/opencode2")));
    }

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
            fs::create_dir_all(executable.parent().unwrap()).unwrap();
            fs::write(executable, "#!/bin/sh\n").unwrap();
            fs::set_permissions(executable, fs::Permissions::from_mode(0o755)).unwrap();
        }
        let path = root.join("path");
        let find =
            |configured, path| find_opencode_executable(configured, path, Some(&root), Some(&root));
        assert_eq!(
            find(Some("override/opencode2".into()), Some(path.as_os_str())),
            Some(root.join("override/opencode2"))
        );
        assert_eq!(find(None, Some(path.as_os_str())), Some(path_executable));
        assert_eq!(find(None, None), Some(official.clone()));
        fs::set_permissions(&official, fs::Permissions::from_mode(0o644)).unwrap();
        assert_eq!(find(None, None), Some(fallback.clone()));
        fs::remove_file(&official).unwrap();
        fs::create_dir(&official).unwrap();
        assert_eq!(find(None, None), Some(fallback));
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn version_manager_discovery_is_bounded_to_known_roots() {
        let root = std::env::temp_dir().join(format!(
            "hex-opencode-nvm-{}-{:?}",
            std::process::id(),
            thread::current().id()
        ));
        let executable = root.join(".nvm/versions/node/v24.4.1/bin/opencode2");
        std::fs::create_dir_all(executable.parent().unwrap()).unwrap();
        std::fs::write(&executable, "#!/bin/sh\n").unwrap();
        let mut permissions = executable.metadata().unwrap().permissions();
        permissions.set_mode(0o755);
        std::fs::set_permissions(&executable, permissions).unwrap();

        let found = find_opencode_executable(None, None, Some(&root), Some(&root));

        assert_eq!(found, Some(executable));
        std::fs::remove_dir_all(root).unwrap();
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
        let default: DefaultModelResponse =
            serde_json::from_value(serde_json::json!({ "data": wire_models[1] })).unwrap();
        let catalog = build_model_catalog(models.data, default.data);

        assert_eq!(catalog.models.len(), 2);
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
        let catalog = build_model_catalog(Vec::new(), default.data);

        assert_eq!(catalog.models.len(), 1);
        assert_eq!(catalog.models[0].key, "example/rewrite-default");
        assert_eq!(catalog.models[0].variants, ["high"]);
        assert_eq!(
            catalog.default_key.as_deref(),
            Some("example/rewrite-default")
        );
        assert_eq!(catalog.default_name.as_deref(), Some("Example Rewrite"));
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

    fn receive_generation_request(stream: &mut std::net::TcpStream) -> Vec<u8> {
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
        assert!(headers.starts_with("POST /api/generate HTTP/1.1\r\n"));
        assert!(headers.contains("Authorization: Basic b3BlbmNvZGU6Zml4dHVyZS1wYXNzd29yZA==\r\n"));
        let length: usize = headers
            .lines()
            .find_map(|line| {
                let (key, value) = line.split_once(':')?;
                key.eq_ignore_ascii_case("content-length")
                    .then(|| value.trim().parse().unwrap())
            })
            .unwrap();
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
        body
    }

    #[test]
    fn generation_pipes_credentials_and_large_json_without_argv_exposure() {
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let endpoint = format!("http://{}", listener.local_addr().unwrap());
        let server = thread::spawn(move || {
            let (mut stream, _) = listener.accept().unwrap();
            let body = receive_generation_request(&mut stream);
            let response = r#"{"data":{"text":"fixture result"}}"#;
            write!(
                stream,
                "HTTP/1.1 200 OK\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{response}",
                response.len()
            )
            .unwrap();
            body
        });
        let prompt = format!(
            "fixture prompt: \"quoted\"\\path\n\r\t\u{00e9}{}",
            "a".repeat(1024 * 1024)
        );
        let data = serde_json::to_string(&Request {
            prompt: &prompt,
            model: None,
        })
        .unwrap();
        let (command, input) = opencode_http_command(
            &endpoint,
            "fixture-password",
            "/api/generate",
            Some(&data),
            None,
        )
        .unwrap();
        for arg in command.get_args() {
            assert!(!arg.to_string_lossy().contains("fixture"));
            assert!(!arg.to_string_lossy().contains(&prompt));
        }
        let output = run_command(
            command,
            Some(input),
            Duration::from_secs(5),
            "test generation",
            &AtomicBool::new(false),
        )
        .unwrap();
        assert!(output.status.success());
        assert_eq!(server.join().unwrap(), data.as_bytes());
        assert!(
            matches!(serde_json::from_slice::<Response>(&output.stdout).unwrap(), Response::Success { data } if data.text == "fixture result")
        );
    }

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

    #[test]
    fn generation_http_wait_observes_cancellation_and_deadline() {
        for cancel in [false, true] {
            let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
            let endpoint = format!("http://{}", listener.local_addr().unwrap());
            let cancelled = std::sync::Arc::new(AtomicBool::new(false));
            let flag = cancelled.clone();
            let server = thread::spawn(move || {
                let (mut stream, _) = listener.accept().unwrap();
                receive_generation_request(&mut stream);
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

    #[test]
    fn blocked_request_pipe_does_not_outlive_the_deadline() {
        let mut command = Command::new("/bin/sleep");
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
