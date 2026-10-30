//! Custom transformations from the user's `hex.config.ts` (or `hex.config.js`),
//! run in-process by embedded QuickJS ([`script`]) on one worker thread. The
//! worker loads the config, reloads it when the config folder changes (keeping
//! the last good registration when a reload fails), and answers transformation
//! calls. The Modes pane reads its status from `personal-commands.json`.

mod script;
mod typescript;

use std::collections::{HashSet, VecDeque};
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{self, Receiver, SyncSender, TrySendError};
use std::sync::{Arc, RwLock};
use std::thread;
use std::time::{Duration, Instant, SystemTime};

use color_eyre::eyre::{Result, WrapErr, eyre};
use serde::{Deserialize, Serialize};

use crate::context::ContextSnapshot;

use script::Script;

/// The config file names Hex looks for, in the config folder.
const CONFIG_FILES: [&str; 2] = ["hex.config.ts", "hex.config.js"];
/// Files Hex owns in the config folder and rewrites when they differ.
const MANAGED_FILES: [(&str, &str); 2] = [
    ("hex.d.ts", include_str!("personal_commands/hex.d.ts")),
    ("AGENTS.md", include_str!("personal_commands/AGENTS.md")),
];
/// Starting points written only where the user has none.
const TEMPLATE_FILES: [(&str, &str); 1] = [(
    "tsconfig.json",
    include_str!("personal_commands/tsconfig.json"),
)];
const TEMPLATE_CONFIG: &str = include_str!("personal_commands/hex.config.ts");
const POLL_INTERVAL: Duration = Duration::from_millis(250);
const RELOAD_DEBOUNCE: Duration = Duration::from_millis(350);
const MAX_REGISTRATION_BYTES: usize = 256 * 1024;
const MAX_TRANSFORMATIONS: usize = 128;
const MAX_TRANSFORMATIONS_PER_INVOCATION: usize = 32;
const MAX_WATCH_ENTRIES: usize = 4096;
const MAX_WATCH_DEPTH: usize = 32;
const MAX_STATUS_BYTES: usize = 256 * 1024;
const MAX_STATUS_ERROR_BYTES: usize = 4096;
const MAX_TRANSFORMATION_TEXT_BYTES: usize = 48 * 1024;
/// How long a dictation waits for the worker, which may be loading a config
/// first; each script call has its own shorter limit.
const TRANSFORMATION_TIMEOUT: Duration = Duration::from_secs(5);
const LOWERCASE_TRANSFORMATION_ID: &str = "lowercase";
const SPONGEBOB_TRANSFORMATION_ID: &str = "spongebob-case";
const NO_TRAILING_PUNCTUATION_TRANSFORMATION_ID: &str = "no-trailing-punctuation";

struct BuiltinTransformation {
    id: &'static str,
    name: &'static str,
    description: &'static str,
    apply: fn(&str) -> String,
}

const BUILTIN_TRANSFORMATIONS: &[BuiltinTransformation] = &[
    BuiltinTransformation {
        id: LOWERCASE_TRANSFORMATION_ID,
        name: "Lowercase",
        description: "Convert the final text to lowercase",
        apply: str::to_lowercase,
    },
    BuiltinTransformation {
        id: SPONGEBOB_TRANSFORMATION_ID,
        name: "SpongeBob case",
        description: "Alternate lowercase and uppercase letters",
        apply: spongebob_case,
    },
    BuiltinTransformation {
        id: NO_TRAILING_PUNCTUATION_TRANSFORMATION_ID,
        name: "No trailing punctuation",
        description: "Remove punctuation at the end of the text",
        apply: remove_trailing_punctuation,
    },
];

fn builtin_transformation(id: &str) -> Option<&'static BuiltinTransformation> {
    BUILTIN_TRANSFORMATIONS
        .iter()
        .find(|transformation| transformation.id == id)
}

/// `text` after the built-in transformation `id`, or None when `id` is not
/// built in (a script runs in the transformation host instead).
pub fn apply_builtin_transformation(id: &str, text: &str) -> Option<String> {
    builtin_transformation(id).map(|builtin| (builtin.apply)(text))
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct StatusSnapshot {
    pub active_generation: Option<u64>,
    #[serde(default)]
    pub transformations: Vec<StatusTransformation>,
    pub host_state: HostState,
    pub last_reload_error: Option<String>,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct StatusTransformation {
    pub id: String,
    pub name: String,
    pub description: Option<String>,
}

pub fn include_builtin_transformations(status: &mut StatusSnapshot) {
    status
        .transformations
        .retain(|transformation| builtin_transformation(&transformation.id).is_none());
    status.transformations.splice(
        0..0,
        BUILTIN_TRANSFORMATIONS
            .iter()
            .map(|transformation| StatusTransformation {
                id: transformation.id.into(),
                name: transformation.name.into(),
                description: Some(transformation.description.into()),
            }),
    );
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum HostState {
    NotConfigured,
    Starting,
    Active,
    Unavailable,
    Stopped,
}

impl Default for StatusSnapshot {
    fn default() -> Self {
        Self {
            active_generation: None,
            transformations: Vec::new(),
            host_state: HostState::NotConfigured,
            last_reload_error: None,
        }
    }
}

pub fn load_status() -> Result<StatusSnapshot> {
    let path = crate::app_paths::personal_commands_status_file()?;
    load_status_at(&path)
}

fn load_status_at(path: &Path) -> Result<StatusSnapshot> {
    let bytes = match fs::read(path) {
        Ok(bytes) => bytes,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            return Ok(StatusSnapshot::default());
        }
        Err(error) => return Err(error.into()),
    };
    if bytes.len() > MAX_STATUS_BYTES {
        return Err(eyre!(
            "personal command status exceeds {MAX_STATUS_BYTES} bytes"
        ));
    }
    let status: StatusSnapshot = serde_json::from_slice(&bytes)?;
    validate_status(&status)?;
    Ok(status)
}

fn persist_status_at(path: &Path, status: &StatusSnapshot) -> Result<()> {
    validate_status(status)?;
    let encoded = serde_json::to_vec(status)?;
    if encoded.len() > MAX_STATUS_BYTES {
        return Err(eyre!(
            "personal command status exceeds {MAX_STATUS_BYTES} bytes"
        ));
    }
    let parent = path
        .parent()
        .ok_or_else(|| eyre!("personal command status path has no parent"))?;
    fs::create_dir_all(parent)?;
    let temporary = path.with_extension(format!("json.{}.tmp", std::process::id()));
    fs::write(&temporary, encoded)?;
    fs::rename(temporary, path)?;
    Ok(())
}

fn validate_status(status: &StatusSnapshot) -> Result<()> {
    if status
        .last_reload_error
        .as_ref()
        .is_some_and(|error| error.len() > MAX_STATUS_ERROR_BYTES)
    {
        return Err(eyre!("personal command status error is too large"));
    }
    if status.transformations.len() > MAX_TRANSFORMATIONS {
        return Err(eyre!("personal transformation catalog is too large"));
    }
    validate_transformations(&status.transformations, "status ")?;
    Ok(())
}

fn validate_transformations(
    transformations: &[StatusTransformation],
    label_prefix: &str,
) -> Result<HashSet<String>> {
    let mut ids = HashSet::new();
    for transformation in transformations {
        validate_text(
            &transformation.id,
            &format!("{label_prefix}transformation id"),
            128,
        )?;
        validate_text(
            &transformation.name,
            &format!("{label_prefix}transformation name"),
            1024,
        )?;
        if let Some(description) = &transformation.description {
            validate_text(
                description,
                &format!("{label_prefix}transformation description"),
                1024,
            )?;
        }
        if !ids.insert(transformation.id.clone()) {
            return Err(eyre!("{label_prefix}transformation IDs must be unique"));
        }
    }
    Ok(ids)
}

/// The config file in `workspace`, if there is exactly one.
fn find_config(workspace: &Path) -> Result<Option<&'static str>> {
    let found = CONFIG_FILES
        .into_iter()
        .filter(|name| workspace.join(name).is_file())
        .collect::<Vec<_>>();
    match found.as_slice() {
        [] => Ok(None),
        [config] => Ok(Some(config)),
        [first, second, ..] => Err(eyre!(
            "{first} and {second} both exist; keep only one config file"
        )),
    }
}

/// Whether the personal command folder has a config to load.
pub fn workspace_configured() -> bool {
    crate::app_paths::personal_commands_workspace()
        .is_ok_and(|workspace| matches!(find_config(&workspace), Ok(Some(_)) | Err(_)))
}

/// Creates the config folder with Hex's typings and agent notes, and a
/// starter `hex.config.ts` unless a config already exists.
pub fn initialize_workspace() -> Result<PathBuf> {
    let workspace = crate::app_paths::personal_commands_workspace()?;
    initialize_workspace_at(&workspace)?;
    Ok(workspace)
}

/// Loads `~/.config/hex`'s config as Hex does and summarizes its transformations.
#[cfg_attr(not(target_os = "macos"), allow(dead_code))]
pub fn check_workspace() -> Result<String> {
    let workspace = crate::app_paths::personal_commands_workspace()?;
    check_workspace_at(&workspace)
}

#[cfg_attr(not(target_os = "macos"), allow(dead_code))]
fn check_workspace_at(workspace: &Path) -> Result<String> {
    let workspace = workspace.to_path_buf();
    let handle = thread::Builder::new()
        .name("personal-commands-check".into())
        .stack_size(script::THREAD_STACK_SIZE)
        .spawn(move || {
            let Some(config) = find_config(&workspace)? else {
                return Err(eyre!(
                    "no hex.config.ts or hex.config.js in {}",
                    workspace.display()
                ));
            };
            let (_, candidate) = load_config(&workspace, config)?;
            Ok(format!(
                "{}: {} transformation{}",
                workspace.join(config).display(),
                candidate.transformations.len(),
                if candidate.transformations.len() == 1 {
                    ""
                } else {
                    "s"
                }
            ))
        })?;
    handle
        .join()
        .map_err(|_| eyre!("personal transformation check panicked"))?
}

fn initialize_workspace_at(workspace: &Path) -> Result<()> {
    fs::create_dir_all(workspace)?;
    refresh_managed_files(workspace)?;
    for (name, contents) in TEMPLATE_FILES {
        let path = workspace.join(name);
        if !path.exists() {
            fs::write(path, contents)?;
        }
    }
    if find_config(workspace)?.is_none() {
        // The staged name ends in .tmp so the watcher never loads a partial file.
        let staged = workspace.join(format!(".{}.{}.tmp", CONFIG_FILES[0], std::process::id()));
        fs::write(&staged, TEMPLATE_CONFIG)?;
        if let Err(error) = fs::rename(&staged, workspace.join(CONFIG_FILES[0])) {
            let _ = fs::remove_file(&staged);
            return Err(error.into());
        }
    }
    Ok(())
}

/// Writes the typings and agent notes this build of Hex ships, where they
/// differ, so an existing folder follows app updates.
fn refresh_managed_files(workspace: &Path) -> Result<()> {
    for (name, contents) in MANAGED_FILES {
        let path = workspace.join(name);
        if fs::read(&path).is_ok_and(|current| current == contents.as_bytes()) {
            continue;
        }
        fs::write(&path, contents)
            .wrap_err_with(|| format!("could not write {}", path.display()))?;
    }
    Ok(())
}

pub struct PersonalCommands {
    requests: SyncSender<Request>,
    stop: Arc<AtomicBool>,
    worker: Option<thread::JoinHandle<()>>,
    worker_done: Receiver<()>,
    transformations: Arc<TransformationClient>,
}

impl PersonalCommands {
    pub fn start(transformations: Arc<TransformationClient>) -> Option<Self> {
        let (request_sender, requests) = mpsc::sync_channel(32);
        let stop = Arc::new(AtomicBool::new(false));
        let (done_sender, worker_done) = mpsc::sync_channel(1);
        let workspace = crate::app_paths::personal_commands_workspace().ok()?;
        let status_file = crate::app_paths::personal_commands_status_file().ok();
        let worker_transformations = transformations.clone();
        let worker_requests = request_sender.clone();
        let worker_stop = stop.clone();
        let worker = thread::Builder::new()
            .name("personal-commands".into())
            .stack_size(script::THREAD_STACK_SIZE)
            .spawn(move || {
                // Scripts never leave this thread.
                Worker {
                    transformations: worker_transformations,
                    requests: worker_requests,
                    workspace,
                    status_file,
                    generation: 0,
                    script: None,
                    status: StatusSnapshot {
                        host_state: HostState::Starting,
                        ..StatusSnapshot::default()
                    },
                }
                .run(&requests, &worker_stop);
                let _ = done_sender.send(());
            })
            .ok()?;
        Some(Self {
            requests: request_sender,
            stop,
            worker: Some(worker),
            worker_done,
            transformations,
        })
    }
}

impl Drop for PersonalCommands {
    fn drop(&mut self) {
        self.transformations.clear();
        self.stop.store(true, Ordering::Release);
        let _ = self.requests.try_send(Request::Reload);
        if self
            .worker_done
            .recv_timeout(Duration::from_secs(2))
            .is_ok()
            && let Some(worker) = self.worker.take()
            && worker.join().is_err()
        {
            tracing::warn!("personal command worker panicked during shutdown");
        }
    }
}

enum Request {
    Transform(Box<TransformationRequest>),
    /// Reload the config now; also wakes the worker to stop.
    Reload,
}

/// The script worker's state: the loaded config and the status the Modes pane reads.
struct Worker {
    transformations: Arc<TransformationClient>,
    requests: SyncSender<Request>,
    workspace: PathBuf,
    status_file: Option<PathBuf>,
    /// Counts every load, so a stale transformation never reaches a newer config.
    generation: u64,
    /// The loaded config and the generation it registered as.
    script: Option<(u64, Script)>,
    status: StatusSnapshot,
}

impl Worker {
    fn run(mut self, requests: &Receiver<Request>, stop: &AtomicBool) {
        self.persist();
        if matches!(find_config(&self.workspace), Ok(Some(_)))
            && let Err(error) = refresh_managed_files(&self.workspace)
        {
            tracing::warn!(%error, "could not refresh the personal command typings");
        }
        let mut fingerprint = workspace_fingerprint(&self.workspace);
        self.reload();
        let mut changed_at = None;
        let mut reload_now = false;
        let mut next_poll = Instant::now() + POLL_INTERVAL;
        while !stop.load(Ordering::Acquire) {
            match requests.recv_timeout(next_poll.saturating_duration_since(Instant::now())) {
                Ok(Request::Transform(request)) => self.transform(*request),
                Ok(Request::Reload) => reload_now = true,
                Err(mpsc::RecvTimeoutError::Timeout) => {}
                Err(mpsc::RecvTimeoutError::Disconnected) => break,
            }
            if stop.load(Ordering::Acquire) {
                break;
            }
            let now = Instant::now();
            if now >= next_poll {
                next_poll = now + POLL_INTERVAL;
                let next = workspace_fingerprint(&self.workspace);
                if next != fingerprint {
                    fingerprint = next;
                    changed_at = Some(now);
                }
            }
            if reload_now || changed_at.is_some_and(|changed| changed.elapsed() >= RELOAD_DEBOUNCE)
            {
                reload_now = false;
                changed_at = None;
                self.reload();
            }
        }
        self.transformations.clear();
        self.status.host_state = HostState::Stopped;
        self.persist();
    }

    /// Loads the config as it is now. A config that fails to load leaves the
    /// last good one active and reports the error; a removed config
    /// unregisters everything.
    fn reload(&mut self) {
        let config = match find_config(&self.workspace) {
            Ok(Some(config)) => config,
            Ok(None) => {
                if self.script.is_some() || self.status.host_state != HostState::NotConfigured {
                    self.deactivate();
                    self.persist();
                }
                return;
            }
            Err(error) => {
                self.reject(&error.to_string());
                return;
            }
        };
        self.generation += 1;
        let started = Instant::now();
        match load_config(&self.workspace, config) {
            Ok((script, candidate)) => {
                self.transformations.activate(
                    self.generation,
                    candidate.ids,
                    self.requests.clone(),
                );
                self.script = Some((self.generation, script));
                self.status = StatusSnapshot {
                    active_generation: Some(self.generation),
                    transformations: candidate.transformations,
                    host_state: HostState::Active,
                    last_reload_error: None,
                };
                self.persist();
                tracing::info!(
                    generation = self.generation,
                    duration_us = started.elapsed().as_micros(),
                    "personal transformations loaded"
                );
            }
            Err(error) => self.reject(&format!("{error:#}")),
        }
    }

    fn reject(&mut self, error: &str) {
        self.status.host_state = if self.script.is_some() {
            HostState::Active
        } else {
            HostState::Unavailable
        };
        self.status.last_reload_error = Some(bounded_status_error(error));
        self.persist();
        tracing::error!(
            error,
            "personal command config rejected; keeping the previous one"
        );
    }

    /// Forgets everything the last config registered.
    fn deactivate(&mut self) {
        self.script = None;
        self.transformations.clear();
        self.status = StatusSnapshot {
            host_state: HostState::NotConfigured,
            ..StatusSnapshot::default()
        };
    }

    fn persist(&self) {
        let Some(path) = &self.status_file else {
            return;
        };
        if let Err(error) = persist_status_at(path, &self.status) {
            tracing::warn!(%error, "could not persist personal command status");
        }
    }

    fn script(&self, generation: u64) -> Option<&Script> {
        self.script
            .as_ref()
            .filter(|(loaded, _)| *loaded == generation)
            .map(|(_, script)| script)
    }

    fn transform(&self, request: TransformationRequest) {
        let started = Instant::now();
        let result = match self.script(request.generation) {
            None => Err("custom transformation generation is unavailable".into()),
            Some(script) => script
                .transform(
                    &json(&request.ids),
                    &request.text,
                    &json(&invocation_context(&request.context)),
                )
                .and_then(|text| {
                    if text.len() > MAX_TRANSFORMATION_TEXT_BYTES {
                        Err("custom transformation output is too large".into())
                    } else {
                        Ok(text)
                    }
                })
                .map_err(|error| bounded_error(&error)),
        };
        tracing::info!(
            generation = request.generation,
            status = if result.is_ok() {
                "completed"
            } else {
                "failed"
            },
            duration_us = started.elapsed().as_micros(),
            "custom transformation"
        );
        let _ = request.response.send(result);
    }
}

/// Loads `config` from `workspace` and validates its transformations.
fn load_config(workspace: &Path, config: &str) -> Result<(Script, Candidate)> {
    let script = Script::load(workspace, config).map_err(|error| eyre!(error))?;
    if script.registration().len() > MAX_REGISTRATION_BYTES {
        return Err(eyre!(
            "the config registers more than {MAX_REGISTRATION_BYTES} bytes of transformations"
        ));
    }
    let registration: Registration = serde_json::from_str(script.registration())
        .wrap_err("the config's transformations are invalid")?;
    if registration.transformations.len() > MAX_TRANSFORMATIONS {
        return Err(eyre!(
            "personal registry exceeds {MAX_TRANSFORMATIONS} transformations"
        ));
    }
    let ids = validate_transformations(&registration.transformations, "")?;
    Ok((
        script,
        Candidate {
            ids,
            transformations: registration.transformations,
        },
    ))
}

struct Candidate {
    ids: HashSet<String>,
    transformations: Vec<StatusTransformation>,
}

fn json(value: &impl Serialize) -> String {
    serde_json::to_string(value).expect("plain data serializes")
}

/// The registration a config's `install` produces.
#[derive(Deserialize)]
struct Registration {
    #[serde(default)]
    transformations: Vec<StatusTransformation>,
}

#[derive(Default)]
pub struct TransformationClient {
    active: RwLock<Option<ActiveTransformations>>,
}

#[derive(Clone)]
struct ActiveTransformations {
    generation: u64,
    ids: HashSet<String>,
    requests: SyncSender<Request>,
}

struct TransformationRequest {
    generation: u64,
    ids: Vec<String>,
    text: String,
    context: ContextSnapshot,
    response: SyncSender<std::result::Result<String, String>>,
}

impl TransformationClient {
    pub fn transform(
        &self,
        ids: &[String],
        text: &str,
        context: &ContextSnapshot,
        cancelled: &AtomicBool,
    ) -> std::result::Result<String, String> {
        if ids.is_empty() {
            return Ok(text.into());
        }
        if text.len() > MAX_TRANSFORMATION_TEXT_BYTES {
            return Err("custom transformation input is too large".into());
        }
        if ids.len() > MAX_TRANSFORMATIONS_PER_INVOCATION {
            return Err("mode references too many transformations".into());
        }
        let mut output = text.to_string();
        let mut custom = Vec::new();
        for id in ids {
            if let Some(builtin) = builtin_transformation(id) {
                if !custom.is_empty() {
                    output = self.transform_custom(&custom, &output, context, cancelled)?;
                    custom.clear();
                }
                output = (builtin.apply)(&output);
            } else {
                custom.push(id.clone());
            }
        }
        if !custom.is_empty() {
            output = self.transform_custom(&custom, &output, context, cancelled)?;
        }
        Ok(output)
    }

    fn transform_custom(
        &self,
        ids: &[String],
        text: &str,
        context: &ContextSnapshot,
        cancelled: &AtomicBool,
    ) -> std::result::Result<String, String> {
        let active = self
            .active
            .read()
            .unwrap_or_else(|error| error.into_inner())
            .clone()
            .ok_or_else(|| "custom transformations are unavailable".to_string())?;
        if ids.iter().any(|id| !active.ids.contains(id)) {
            return Err("mode references an unavailable custom transformation".into());
        }
        let (sender, receiver) = mpsc::sync_channel(1);
        active
            .requests
            .try_send(Request::Transform(Box::new(TransformationRequest {
                generation: active.generation,
                ids: ids.to_vec(),
                text: text.into(),
                context: context.clone(),
                response: sender,
            })))
            .map_err(|error| match error {
                TrySendError::Full(_) => "custom transformation queue is full".to_string(),
                TrySendError::Disconnected(_) => {
                    "custom transformations are unavailable".to_string()
                }
            })?;
        let started = Instant::now();
        loop {
            if cancelled.load(Ordering::Acquire) {
                return Err("dictation was cancelled".into());
            }
            let remaining = TRANSFORMATION_TIMEOUT.saturating_sub(started.elapsed());
            if remaining.is_zero() {
                return Err("custom transformations timed out".into());
            }
            match receiver.recv_timeout(remaining.min(Duration::from_millis(50))) {
                Ok(result) => return result,
                Err(mpsc::RecvTimeoutError::Timeout) => {}
                Err(mpsc::RecvTimeoutError::Disconnected) => {
                    return Err("custom transformations stopped".into());
                }
            }
        }
    }

    fn activate(&self, generation: u64, ids: HashSet<String>, requests: SyncSender<Request>) {
        *self
            .active
            .write()
            .unwrap_or_else(|error| error.into_inner()) = Some(ActiveTransformations {
            generation,
            ids,
            requests,
        });
    }

    fn clear(&self) {
        *self
            .active
            .write()
            .unwrap_or_else(|error| error.into_inner()) = None;
    }
}

fn spongebob_case(text: &str) -> String {
    let mut uppercase = false;
    let mut output = String::with_capacity(text.len());
    for character in text.chars() {
        if !character.is_alphabetic() {
            output.push(character);
            continue;
        }
        if uppercase {
            output.extend(character.to_uppercase());
        } else {
            output.extend(character.to_lowercase());
        }
        uppercase = !uppercase;
    }
    output
}

// Sentence marks only. Closing quotes and brackets stay so a dictated
// quotation or parenthetical keeps its pairing intact.
const TRAILING_PUNCTUATION: &[char] = &[
    '.', ',', '!', '?', ';', ':', '\u{2026}', '\u{3002}', '\u{ff01}', '\u{ff1f}', '\u{3001}',
    '\u{ff0c}', '\u{ff1b}', '\u{ff1a}',
];

fn remove_trailing_punctuation(text: &str) -> String {
    let body = text.trim_end();
    let trailing_whitespace = &text[body.len()..];
    let mut output = body.trim_end_matches(TRAILING_PUNCTUATION).to_string();
    output.push_str(trailing_whitespace);
    output
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct InvocationContext {
    #[serde(skip_serializing_if = "Option::is_none")]
    application: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    browser_host: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    browser_url: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    window_title: Option<String>,
}

fn invocation_context(context: &ContextSnapshot) -> InvocationContext {
    InvocationContext {
        application: context.application.clone(),
        browser_host: context.browser_host().map(Into::into),
        browser_url: context.browser_url.as_ref().map(ToString::to_string),
        window_title: context.window_title.clone(),
    }
}

fn workspace_fingerprint(root: &Path) -> u64 {
    let mut hash = 1469598103934665603;
    let mut queue = VecDeque::from([(root.to_path_buf(), 0_usize)]);
    let mut visited = 0;
    while let Some((path, depth)) = queue.pop_front() {
        if depth > MAX_WATCH_DEPTH || visited >= MAX_WATCH_ENTRIES {
            hash ^= u64::MAX;
            break;
        }
        let Ok(entries) = fs::read_dir(path) else {
            continue;
        };
        let mut entries = entries.flatten().collect::<Vec<_>>();
        entries.sort_by_key(|entry| entry.file_name());
        for entry in entries {
            visited += 1;
            if visited > MAX_WATCH_ENTRIES {
                hash ^= u64::MAX;
                break;
            }
            let path = entry.path();
            let name = entry.file_name();
            if name == "node_modules" || name == ".git" {
                continue;
            }
            let Ok(metadata) = fs::symlink_metadata(&path) else {
                continue;
            };
            if metadata.file_type().is_symlink() {
                continue;
            }
            if metadata.is_dir() {
                queue.push_back((path, depth + 1));
            } else if metadata.is_file()
                && matches!(
                    path.extension().and_then(|value| value.to_str()),
                    Some("ts" | "json")
                )
            {
                for byte in path.as_os_str().as_encoded_bytes() {
                    hash = hash
                        .wrapping_mul(1099511628211)
                        .wrapping_add(u64::from(*byte));
                }
                hash ^= metadata.len();
                let modified = metadata.modified().unwrap_or(SystemTime::UNIX_EPOCH);
                if let Ok(duration) = modified.duration_since(SystemTime::UNIX_EPOCH) {
                    hash ^= duration.as_nanos() as u64;
                }
            }
        }
    }
    hash
}

fn validate_text(value: &str, label: &str, max_bytes: usize) -> Result<()> {
    if value.trim().is_empty() || value.len() > max_bytes {
        Err(eyre!(
            "{label} must be non-empty and at most {max_bytes} bytes"
        ))
    } else {
        Ok(())
    }
}

fn bounded_error(message: &str) -> String {
    message.chars().take(4096).collect()
}

fn bounded_status_error(message: &str) -> String {
    message[..message.floor_char_boundary(MAX_STATUS_ERROR_BYTES)].to_owned()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::UNIX_EPOCH;

    #[test]
    fn transformation_client_dispatches_only_registered_ids() {
        let (requests, receiver) = mpsc::sync_channel(1);
        let client = TransformationClient::default();
        *client
            .active
            .write()
            .unwrap_or_else(|error| error.into_inner()) = Some(ActiveTransformations {
            generation: 4,
            ids: HashSet::from(["prefix".into()]),
            requests,
        });
        let worker = thread::spawn(move || {
            let Request::Transform(request) = receiver.recv().unwrap() else {
                panic!("expected transformation request");
            };
            assert_eq!(request.generation, 4);
            assert_eq!(request.ids, ["prefix"]);
            request
                .response
                .send(Ok(format!("PREFIX:{}", request.text)))
                .unwrap();
        });

        let transformed = client
            .transform(
                &["prefix".into(), LOWERCASE_TRANSFORMATION_ID.into()],
                "HELLO",
                &ContextSnapshot::default(),
                &AtomicBool::new(false),
            )
            .unwrap();

        assert_eq!(transformed, "prefix:hello");
        assert!(
            client
                .transform(
                    &["missing".into()],
                    "HELLO",
                    &ContextSnapshot::default(),
                    &AtomicBool::new(false),
                )
                .is_err()
        );
        worker.join().unwrap();
    }

    #[test]
    fn built_in_transformations_do_not_require_the_personal_command_host() {
        let client = TransformationClient::default();
        let context = ContextSnapshot::default();
        let cancelled = AtomicBool::new(false);

        assert_eq!(
            client
                .transform(
                    &[LOWERCASE_TRANSFORMATION_ID.into()],
                    "Hello, WORLD!",
                    &context,
                    &cancelled,
                )
                .unwrap(),
            "hello, world!"
        );
        assert_eq!(
            client
                .transform(
                    &[SPONGEBOB_TRANSFORMATION_ID.into()],
                    "Sponge Bob!",
                    &context,
                    &cancelled,
                )
                .unwrap(),
            "sPoNgE bOb!"
        );
    }

    #[test]
    fn no_trailing_punctuation_strips_only_the_final_run() {
        assert_eq!(
            remove_trailing_punctuation("The quick, brown fox jumped over the lazy dog."),
            "The quick, brown fox jumped over the lazy dog"
        );
        assert_eq!(remove_trailing_punctuation("Wait, what?!…"), "Wait, what");
        assert_eq!(remove_trailing_punctuation("好的。"), "好的");
        assert_eq!(remove_trailing_punctuation("你好："), "你好");
        assert_eq!(remove_trailing_punctuation("你好；"), "你好");
    }

    #[test]
    fn no_trailing_punctuation_keeps_quotes_brackets_and_symbols() {
        for input in ["(see above)", "He said \"hi.\"", "up 5%"] {
            assert_eq!(remove_trailing_punctuation(input), input);
        }
    }

    #[test]
    fn no_trailing_punctuation_preserves_trailing_whitespace_and_empty_input() {
        assert_eq!(remove_trailing_punctuation("Good dog. "), "Good dog ");
        assert_eq!(remove_trailing_punctuation("   "), "   ");
        assert_eq!(remove_trailing_punctuation(""), "");
        assert_eq!(remove_trailing_punctuation("..."), "");
    }

    #[test]
    fn built_in_transformations_chain_in_selected_order() {
        let client = TransformationClient::default();
        let context = ContextSnapshot::default();
        let cancelled = AtomicBool::new(false);
        let chain = |ids: &[&str]| {
            client
                .transform(
                    &ids.iter().map(|id| (*id).into()).collect::<Vec<_>>(),
                    "abcd",
                    &context,
                    &cancelled,
                )
                .unwrap()
        };

        assert_eq!(
            chain(&[LOWERCASE_TRANSFORMATION_ID, SPONGEBOB_TRANSFORMATION_ID]),
            "aBcD"
        );
        assert_eq!(
            chain(&[SPONGEBOB_TRANSFORMATION_ID, LOWERCASE_TRANSFORMATION_ID]),
            "abcd"
        );
        assert_eq!(
            client
                .transform(
                    &[NO_TRAILING_PUNCTUATION_TRANSFORMATION_ID.into()],
                    "abcd.",
                    &context,
                    &cancelled,
                )
                .unwrap(),
            "abcd"
        );
    }

    #[test]
    fn built_in_transformations_replace_matching_personal_catalog_entries() {
        let mut status = StatusSnapshot {
            transformations: vec![
                StatusTransformation {
                    id: LOWERCASE_TRANSFORMATION_ID.into(),
                    name: "Custom lowercase".into(),
                    description: None,
                },
                StatusTransformation {
                    id: NO_TRAILING_PUNCTUATION_TRANSFORMATION_ID.into(),
                    name: "Custom trailing punctuation".into(),
                    description: None,
                },
                StatusTransformation {
                    id: "custom".into(),
                    name: "Custom".into(),
                    description: None,
                },
            ],
            ..StatusSnapshot::default()
        };

        include_builtin_transformations(&mut status);

        assert_eq!(
            status
                .transformations
                .iter()
                .map(|transformation| transformation.id.as_str())
                .collect::<Vec<_>>(),
            [
                LOWERCASE_TRANSFORMATION_ID,
                SPONGEBOB_TRANSFORMATION_ID,
                NO_TRAILING_PUNCTUATION_TRANSFORMATION_ID,
                "custom"
            ]
        );
        assert_eq!(status.transformations[0].name, "Lowercase");
        assert_eq!(status.transformations[2].name, "No trailing punctuation");
    }

    #[cfg(unix)]
    #[test]
    fn workspace_fingerprint_does_not_follow_symlinks_or_excessive_depth() {
        let unique = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let root = std::env::temp_dir().join(format!("hex-watch-{unique}"));
        let outside = std::env::temp_dir().join(format!("hex-watch-outside-{unique}.ts"));
        fs::create_dir_all(&root).unwrap();
        fs::write(&outside, "first").unwrap();
        std::os::unix::fs::symlink(&outside, root.join("linked.ts")).unwrap();
        let mut deep = root.clone();
        for _ in 0..=MAX_WATCH_DEPTH {
            deep.push("nested");
            fs::create_dir(&deep).unwrap();
        }
        let hidden = deep.join("hidden.ts");
        fs::write(&hidden, "first").unwrap();
        let initial = workspace_fingerprint(&root);
        fs::write(&outside, "changed outside").unwrap();
        fs::write(&hidden, "changed too deep").unwrap();

        assert_eq!(workspace_fingerprint(&root), initial);
        fs::remove_dir_all(root).unwrap();
        fs::remove_file(outside).unwrap();
    }

    #[test]
    fn status_snapshot_roundtrips_atomically() {
        let unique = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let root = std::env::temp_dir().join(format!("hex-status-{unique}"));
        let path = root.join("personal-commands.json");
        let status = StatusSnapshot {
            active_generation: Some(4),
            transformations: vec![StatusTransformation {
                id: "lowercase".into(),
                name: "Lowercase".into(),
                description: Some("Use lowercase output".into()),
            }],
            host_state: HostState::Active,
            last_reload_error: None,
        };

        persist_status_at(&path, &status).unwrap();
        assert_eq!(load_status_at(&path).unwrap(), status);
        assert!(!path.with_extension("json.tmp").exists());
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn status_snapshot_enforces_error_bounds() {
        let status = StatusSnapshot {
            last_reload_error: Some("x".repeat(MAX_STATUS_ERROR_BYTES + 1)),
            ..StatusSnapshot::default()
        };
        assert!(validate_status(&status).is_err());
    }

    struct Harness {
        root: PathBuf,
        worker: Worker,
        _requests: Receiver<Request>,
    }

    impl Harness {
        fn new(config: &str) -> Self {
            let unique = SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap()
                .as_nanos();
            let root = std::env::temp_dir().join(format!("hex-scripts-{unique}"));
            let workspace = root.join("hex");
            fs::create_dir_all(&workspace).unwrap();
            fs::write(workspace.join("hex.config.ts"), config).unwrap();
            let (requests, request_receiver) = mpsc::sync_channel(4);
            let mut worker = Worker {
                transformations: Arc::new(TransformationClient::default()),
                requests,
                workspace,
                status_file: Some(root.join("personal-commands.json")),
                generation: 0,
                script: None,
                status: StatusSnapshot::default(),
            };
            worker.reload();
            Self {
                root,
                worker,
                _requests: request_receiver,
            }
        }

        fn write(&mut self, config: &str) {
            fs::write(self.worker.workspace.join("hex.config.ts"), config).unwrap();
            self.worker.reload();
        }

        fn generation(&self) -> u64 {
            self.worker.status.active_generation.unwrap_or(0)
        }

        fn error(&self) -> String {
            self.worker
                .status
                .last_reload_error
                .clone()
                .unwrap_or_default()
        }

        fn transform(&self, ids: &[&str], text: &str) -> std::result::Result<String, String> {
            let (response, result) = mpsc::sync_channel(1);
            self.worker.transform(TransformationRequest {
                generation: self.generation(),
                ids: ids.iter().map(ToString::to_string).collect(),
                text: text.into(),
                context: ContextSnapshot::default(),
                response,
            });
            result.recv().unwrap()
        }
    }

    impl Drop for Harness {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.root);
        }
    }

    #[test]
    fn the_starter_config_registers_transformations() {
        let harness = Harness::new(TEMPLATE_CONFIG);
        assert_eq!(
            harness.worker.status.host_state,
            HostState::Active,
            "{}",
            harness.error()
        );
        assert_eq!(
            harness.transform(&["trim-whitespace"], "  hi  ").unwrap(),
            "hi"
        );
    }

    #[test]
    fn legacy_configs_with_commands_still_load_transformations() {
        let harness = Harness::new(
            r#"import { choice, defineHexConfig, digit, letter, openUrl, union } from "@hex/commands"
export default defineHexConfig({
  transformations: {
    exclaim: { name: "Exclaim", transform: (text) => `${text}!` },
  },
  commands: {
    "open-example": {
      phrases: ["open example"],
      action: openUrl("https://example.com"),
    },
    "control-key": {
      phrases: ["control {key}"],
      captures: { key: union(letter(), digit(), choice(["home", "end"] as const)) },
      run: ({ hex, captures }) => hex.press({ key: String(captures.key), modifiers: ["control"] }),
    },
  },
})
"#,
        );
        assert_eq!(
            harness.worker.status.host_state,
            HostState::Active,
            "{}",
            harness.error()
        );
        assert_eq!(harness.transform(&["exclaim"], "hi").unwrap(), "hi!");
    }

    #[test]
    fn transformations_chain_with_built_ins_and_failures_keep_nothing() {
        let harness = Harness::new(
            r#"import { defineHexConfig } from "@hex/commands"
export default defineHexConfig({
  transformations: {
    exclaim: { name: "Exclaim", transform: (text: string): string => `${text}!` },
    later: { name: "Later", transform: async (text) => text.toUpperCase() },
    broken: { name: "Broken", transform: () => { throw new TypeError("bad input") } },
    number: { name: "Number", transform: () => 7 },
  },
})
"#,
        );
        assert_eq!(
            harness.transform(&["exclaim", "later"], "hi").unwrap(),
            "HI!"
        );
        let error = harness.transform(&["exclaim", "broken"], "hi").unwrap_err();
        assert!(error.starts_with("TypeError: bad input"), "{error}");
        assert!(
            harness
                .transform(&["number"], "hi")
                .unwrap_err()
                .contains("did not return a string")
        );
        assert_eq!(
            harness
                .worker
                .status
                .transformations
                .iter()
                .map(|transformation| transformation.id.as_str())
                .collect::<Vec<_>>(),
            ["exclaim", "later", "broken", "number"]
        );
    }

    #[test]
    fn runaway_scripts_stop_at_the_time_limit_and_the_config_keeps_working() {
        let harness = Harness::new(
            r#"export default {
  transformations: {
    spin: { name: "Spin", transform: () => { for (;;) {} } },
    wait: { name: "Wait", transform: () => new Promise(() => {}) },
    grow: { name: "Grow", transform: () => { const a = []; for (;;) a.push("x".repeat(1024)) } },
    echo: { name: "Echo", transform: (text) => text },
  },
}
"#,
        );
        let started = Instant::now();
        assert_eq!(
            harness.transform(&["spin"], "hi").unwrap_err(),
            "timed out after 1000 ms"
        );
        assert!(started.elapsed() < script::CALL_TIME_LIMIT * 2);
        assert!(
            harness
                .transform(&["wait"], "hi")
                .unwrap_err()
                .contains("never settled")
        );
        let error = harness.transform(&["grow"], "hi").unwrap_err();
        assert!(
            error.contains("out of memory") || error.contains("timed out"),
            "{error}"
        );
        assert_eq!(
            harness.transform(&["echo"], "still here").unwrap(),
            "still here"
        );
    }

    #[test]
    fn load_errors_name_the_file_line_and_column_and_keep_the_last_good_config() {
        let mut harness = Harness::new(
            r#"export default { transformations: { echo: { name: "Echo", transform: (t) => t } } }"#,
        );
        let generation = harness.generation();

        harness.write("const a = 1\nenum Color { Red }\nexport default {}\n");
        assert!(
            harness
                .error()
                .contains("SyntaxError: hex.config.ts:2:1: TypeScript enums"),
            "{}",
            harness.error()
        );
        assert_eq!(harness.worker.status.host_state, HostState::Active);
        assert_eq!(harness.generation(), generation);
        assert_eq!(harness.transform(&["echo"], "kept").unwrap(), "kept");

        harness.write("export default {\n  transformations: {\n    a: { name: \"x\", transform: (t) => t },,\n  },\n}\n");
        assert!(
            harness.error().starts_with("SyntaxError"),
            "{}",
            harness.error()
        );
        assert!(
            harness.error().contains("hex.config.ts:3:"),
            "{}",
            harness.error()
        );

        fs::remove_file(harness.worker.workspace.join("hex.config.ts")).unwrap();
        fs::write(
            harness.worker.workspace.join("hex.config.js"),
            "export default {\n  transformations: {\n    a: { name: \"x\" transform: (t) => t },\n  },\n}\n",
        )
        .unwrap();
        harness.worker.reload();
        assert!(
            harness.error().starts_with("SyntaxError"),
            "{}",
            harness.error()
        );
        assert!(
            harness.error().contains("hex.config.js:3:"),
            "{}",
            harness.error()
        );
        fs::remove_file(harness.worker.workspace.join("hex.config.js")).unwrap();

        harness.write("throw new Error(\"top level\")\nexport default {}\n");
        assert!(
            harness.error().starts_with("Error: top level"),
            "{}",
            harness.error()
        );
        assert_eq!(harness.generation(), generation);

        harness.write("export default { transformations: { echo: { name: \"Echo\", transform: (t) => t + \"?\" } } }\n");
        assert!(harness.worker.status.last_reload_error.is_none());
        assert!(harness.generation() > generation);
        assert_eq!(harness.transform(&["echo"], "new").unwrap(), "new?");

        fs::remove_file(harness.worker.workspace.join("hex.config.ts")).unwrap();
        harness.worker.reload();
        assert_eq!(harness.worker.status.host_state, HostState::NotConfigured);
        assert!(harness.worker.script.is_none());
    }

    #[test]
    fn stale_generations_never_reach_a_newer_config() {
        let mut harness = Harness::new(
            r#"export default { transformations: { echo: { name: "Echo", transform: (t) => t } } }"#,
        );
        let (response, result) = mpsc::sync_channel(1);
        let stale = harness.generation();
        harness.write(
            r#"export default { transformations: { echo: { name: "Echo", transform: (t) => t + t } } }"#,
        );
        harness.worker.transform(TransformationRequest {
            generation: stale,
            ids: vec!["echo".into()],
            text: "a".into(),
            context: ContextSnapshot::default(),
            response,
        });
        assert!(result.recv().unwrap().is_err());
        assert_eq!(harness.transform(&["echo"], "a").unwrap(), "aa");
    }

    #[test]
    fn scripts_import_only_hex_commands_and_files_in_the_folder() {
        let mut harness = Harness::new("export default {}");
        let workspace = harness.worker.workspace.clone();
        fs::create_dir_all(workspace.join("lib")).unwrap();
        fs::write(
            workspace.join("lib/words.ts"),
            "import data from \"../data.json\"\nexport const shout = (text: string): string => text.toUpperCase() + data.mark\n",
        )
        .unwrap();
        fs::write(workspace.join("data.json"), "{\"mark\": \"!\"}").unwrap();
        harness.write(
            "import { shout } from \"./lib/words\"\nexport default { transformations: { shout: { name: \"Shout\", transform: shout } } }\n",
        );
        assert_eq!(harness.transform(&["shout"], "hi").unwrap(), "HI!");

        for import in [
            "node:fs",
            "effect",
            "@hex/commands/effect",
            "../outside.ts",
            "hex:host",
        ] {
            harness.write(&format!(
                "import {{ x }} from {import:?}\nexport default {{}}\n"
            ));
            assert!(!harness.error().is_empty(), "{import} was importable");
        }
        harness.write(
            "export default { transformations: { probe: { name: \"Probe\", transform: () => [typeof fetch, typeof setTimeout, typeof require, typeof process, typeof console.log].join(\" \") } } }\n",
        );
        assert_eq!(
            harness.transform(&["probe"], "").unwrap(),
            "undefined undefined undefined undefined function"
        );
    }

    #[test]
    fn initialization_writes_typings_and_a_starter_config_but_keeps_the_users() {
        let unique = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let workspace = std::env::temp_dir().join(format!("hex-init-{unique}"));
        initialize_workspace_at(&workspace).unwrap();
        assert!(
            check_workspace_at(&workspace)
                .unwrap()
                .contains("1 transformation")
        );
        assert_eq!(
            fs::read_to_string(workspace.join("hex.config.ts")).unwrap(),
            TEMPLATE_CONFIG
        );
        for name in ["hex.d.ts", "AGENTS.md", "tsconfig.json"] {
            assert!(workspace.join(name).is_file(), "{name}");
        }
        fs::write(workspace.join("hex.config.ts"), "// mine\n").unwrap();
        fs::write(workspace.join("hex.d.ts"), "// stale\n").unwrap();
        initialize_workspace_at(&workspace).unwrap();
        assert_eq!(
            fs::read_to_string(workspace.join("hex.config.ts")).unwrap(),
            "// mine\n"
        );
        assert_eq!(
            fs::read_to_string(workspace.join("hex.d.ts")).unwrap(),
            MANAGED_FILES[0].1
        );
        fs::write(workspace.join("hex.config.js"), "export default {}\n").unwrap();
        assert!(find_config(&workspace).is_err());
        fs::remove_dir_all(workspace).unwrap();
    }
}
