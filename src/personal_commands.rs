//! Personal commands and custom transformations from the user's
//! `hex.config.ts` (or `hex.config.js`), run in-process by embedded QuickJS
//! ([`script`]) on one worker thread. The worker loads the config, compiles
//! its registration into the command registry, reloads it when the config
//! folder changes (keeping the last good registry when a reload fails),
//! answers handler and transformation calls, and hands the actions handlers
//! ask for to Hex's native action executor. The panes read its status from
//! `personal-commands.json`.

mod script;
mod typescript;

use std::collections::{BTreeMap, HashSet, VecDeque};
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{self, Receiver, SyncSender, TrySendError};
use std::sync::{Arc, RwLock};
use std::thread;
use std::time::{Duration, Instant, SystemTime};

use color_eyre::eyre::{Result, WrapErr, eyre};
use serde::{Deserialize, Serialize};

use crate::commands::{
    Action, ActionOutcome, CapturedValue, CapturedValues, CommandConfig, ConfiguredCommand,
    ContextPredicate, PersonalCapture,
};
use crate::context::ContextSnapshot;
use crate::dictation::DictationProtocol;
use crate::keys::{Key, Modifiers};

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
const MAX_COMMANDS: usize = 512;
const MAX_TRANSFORMATIONS: usize = 128;
const MAX_TRANSFORMATIONS_PER_INVOCATION: usize = 32;
const MAX_PHRASES: usize = 16;
const MAX_CAPTURE_ENTRIES: usize = 8;
const MAX_CHOICE_VALUES: usize = 64;
const MAX_CHOICE_ALIASES: usize = 16;
const MAX_CHOICE_VALUE_BYTES: usize = 128;
const MAX_CHOICE_WORD_BYTES: usize = 64;
const MAX_UNION_MEMBERS: usize = 16;
const MAX_UNION_DEPTH: usize = 4;
const MAX_UNION_SERIALIZED_BYTES: usize = 16 * 1024;
const MAX_REPEAT: u8 = 100;
const MAX_ACTIONS_PER_INVOCATION: usize = 64;
const ACTION_QUEUE: usize = 64;
const MAX_WATCH_ENTRIES: usize = 4096;
const MAX_WATCH_DEPTH: usize = 32;
const MAX_STATUS_BYTES: usize = 256 * 1024;
const MAX_STATUS_ERROR_BYTES: usize = 4096;
const MAX_TRANSFORMATION_TEXT_BYTES: usize = 48 * 1024;
/// How long a dictation waits for the worker, which may be loading a config
/// or running a handler first; each script call has its own shorter limit.
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
    pub catalog: Vec<StatusCommand>,
    #[serde(default)]
    pub dictation: Option<StatusDictationProtocol>,
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

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct StatusDictationProtocol {
    pub start: Vec<String>,
    pub stop: Vec<String>,
    pub send: Vec<String>,
    pub cancel: Vec<String>,
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

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct StatusCommand {
    pub id: String,
    pub phrases: Vec<String>,
    pub group: String,
    pub description: String,
    pub context: StatusContext,
    pub execution: StatusExecution,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(
    tag = "type",
    rename_all = "camelCase",
    rename_all_fields = "camelCase"
)]
pub enum StatusContext {
    Global,
    Application { application: String },
    BrowserHost { browser_host: String },
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum StatusExecution {
    Native,
    Handler,
}

impl Default for StatusSnapshot {
    fn default() -> Self {
        Self {
            active_generation: None,
            catalog: Vec::new(),
            dictation: None,
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
    if status.catalog.len() > MAX_COMMANDS {
        return Err(eyre!(
            "personal command status exceeds {MAX_COMMANDS} commands"
        ));
    }
    if status
        .last_reload_error
        .as_ref()
        .is_some_and(|error| error.len() > MAX_STATUS_ERROR_BYTES)
    {
        return Err(eyre!("personal command status error is too large"));
    }
    for command in &status.catalog {
        validate_text(&command.id, "status command id", 128)?;
        validate_text(&command.group, "status command group", 1024)?;
        validate_text(&command.description, "status command description", 1024)?;
        if command.phrases.is_empty() || command.phrases.len() > MAX_PHRASES {
            return Err(eyre!("status command phrases are out of bounds"));
        }
        for phrase in &command.phrases {
            validate_text(phrase, "status command phrase", 256)?;
        }
        match &command.context {
            StatusContext::Global => {}
            StatusContext::Application { application } => {
                validate_text(application, "status application context", 256)?;
            }
            StatusContext::BrowserHost { browser_host } => {
                validate_text(browser_host, "status browser context", 253)?;
            }
        }
    }
    if let Some(dictation) = &status.dictation {
        DictationProtocol::try_new(
            dictation.start.clone(),
            dictation.stop.clone(),
            dictation.send.clone(),
            dictation.cancel.clone(),
        )
        .map_err(|error| eyre!(error))?;
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
    active: Arc<RwLock<Arc<RuntimeSnapshot>>>,
    requests: SyncSender<Request>,
    outcomes: Receiver<ActionOutcome>,
    stop: Arc<AtomicBool>,
    worker: Option<thread::JoinHandle<()>>,
    worker_done: Receiver<()>,
    transformations: Arc<TransformationClient>,
}

impl PersonalCommands {
    pub fn start(base: CommandConfig, transformations: Arc<TransformationClient>) -> Option<Self> {
        let active = Arc::new(RwLock::new(Arc::new(RuntimeSnapshot::native(base.clone()))));
        let (request_sender, requests) = mpsc::sync_channel(32);
        let (outcome_sender, outcomes) = mpsc::sync_channel(64);
        let stop = Arc::new(AtomicBool::new(false));
        let (done_sender, worker_done) = mpsc::sync_channel(1);
        let actions = spawn_action_runner(outcome_sender.clone())?;
        let workspace = crate::app_paths::personal_commands_workspace().ok()?;
        let status_file = crate::app_paths::personal_commands_status_file().ok();
        let worker_active = active.clone();
        let worker_transformations = transformations.clone();
        let worker_requests = request_sender.clone();
        let worker_stop = stop.clone();
        let worker = thread::Builder::new()
            .name("personal-commands".into())
            .stack_size(script::THREAD_STACK_SIZE)
            .spawn(move || {
                // Scripts never leave this thread.
                Worker {
                    base,
                    active: worker_active,
                    transformations: worker_transformations,
                    requests: worker_requests,
                    outcomes: outcome_sender,
                    actions,
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
            active,
            requests: request_sender,
            outcomes,
            stop,
            worker: Some(worker),
            worker_done,
            transformations,
        })
    }

    pub fn snapshot(&self) -> Arc<RuntimeSnapshot> {
        self.active
            .read()
            .expect("command registry poisoned")
            .clone()
    }

    pub fn invoke(
        &self,
        generation: u64,
        command_id: impl Into<String>,
        heard: &str,
        context: ContextSnapshot,
        captures: CapturedValues,
    ) -> Result<(), &'static str> {
        self.requests
            .try_send(Request::Invoke(Box::new(Invocation {
                generation,
                command_id: command_id.into(),
                heard: heard.into(),
                context,
                captures,
            })))
            .map_err(|error| match error {
                TrySendError::Full(_) => "personal command queue is full",
                TrySendError::Disconnected(_) => "personal commands are unavailable",
            })
    }

    pub fn try_recv(&self) -> Option<ActionOutcome> {
        self.outcomes.try_recv().ok()
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

struct Invocation {
    generation: u64,
    command_id: String,
    heard: String,
    context: ContextSnapshot,
    captures: CapturedValues,
}

enum Request {
    Invoke(Box<Invocation>),
    Transform(Box<TransformationRequest>),
    /// Reload the config now; also wakes the worker to stop.
    Reload,
}

/// The actions one handler asked for, run in order by the action runner.
struct ActionJob {
    outcome: ActionOutcome,
    actions: Vec<Action>,
}

/// Runs handler actions one job at a time, in submission order, on Hex's
/// native action executor, so a slow action never holds up the script worker.
fn spawn_action_runner(outcomes: SyncSender<ActionOutcome>) -> Option<SyncSender<ActionJob>> {
    let (sender, jobs) = mpsc::sync_channel::<ActionJob>(ACTION_QUEUE);
    thread::Builder::new()
        .name("personal-command-actions".into())
        .spawn(move || {
            while let Ok(job) = jobs.recv() {
                let result = job
                    .actions
                    .into_iter()
                    .try_for_each(crate::commands::execute)
                    .map_err(|error| bounded_error(&error.to_string()));
                if outcomes
                    .send(ActionOutcome {
                        result,
                        ..job.outcome
                    })
                    .is_err()
                {
                    break;
                }
            }
        })
        .ok()?;
    Some(sender)
}

/// The script worker's state: the loaded config, the registry it compiled
/// to, and the status the panes read.
struct Worker {
    base: CommandConfig,
    active: Arc<RwLock<Arc<RuntimeSnapshot>>>,
    transformations: Arc<TransformationClient>,
    requests: SyncSender<Request>,
    outcomes: SyncSender<ActionOutcome>,
    actions: SyncSender<ActionJob>,
    workspace: PathBuf,
    status_file: Option<PathBuf>,
    /// Counts every load, so a stale invocation never reaches a newer config.
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
                Ok(Request::Invoke(invocation)) => self.invoke(*invocation),
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
        match self.load(config) {
            Ok((script, candidate)) => {
                self.transformations
                    .activate(&candidate.runtime, self.requests.clone());
                *self.active.write().expect("command registry poisoned") =
                    Arc::new(candidate.runtime);
                self.script = Some((self.generation, script));
                self.status = StatusSnapshot {
                    active_generation: Some(self.generation),
                    catalog: candidate.catalog,
                    dictation: candidate.dictation,
                    transformations: candidate.transformations,
                    host_state: HostState::Active,
                    last_reload_error: None,
                };
                self.persist();
                tracing::info!(
                    generation = self.generation,
                    duration_us = started.elapsed().as_micros(),
                    "personal commands loaded"
                );
            }
            Err(error) => self.reject(&format!("{error:#}")),
        }
    }

    fn load(&self, config: &str) -> Result<(Script, Candidate)> {
        load_config(&self.workspace, config, &self.base, self.generation)
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
        *self.active.write().expect("command registry poisoned") =
            Arc::new(RuntimeSnapshot::native(self.base.clone()));
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

    fn invoke(&self, invocation: Invocation) {
        let Some(script) = self.script(invocation.generation) else {
            emit_failure(
                invocation,
                &self.outcomes,
                "personal command generation is no longer available",
            );
            return;
        };
        let started = Instant::now();
        let captures = invocation
            .captures
            .iter()
            .map(|(name, value)| {
                let value = match value {
                    CapturedValue::Digit(value) => WireCaptureValue::Number(*value),
                    CapturedValue::Letter(value) => WireCaptureValue::Text(value.to_string()),
                    CapturedValue::Choice(value) | CapturedValue::Text(value) => {
                        WireCaptureValue::Text(value.clone())
                    }
                };
                (name.clone(), value)
            })
            .collect::<BTreeMap<_, _>>();
        let result = script
            .invoke(
                &invocation.command_id,
                &json(&invocation_context(&invocation.context)),
                &json(&captures),
            )
            .and_then(|actions| parse_actions(&actions));
        tracing::info!(
            generation = invocation.generation,
            command_id = invocation.command_id,
            status = if result.is_ok() {
                "completed"
            } else {
                "failed"
            },
            duration_us = started.elapsed().as_micros(),
            "personal command handler"
        );
        let actions = match result {
            Ok(actions) => actions,
            Err(error) => return emit_failure(invocation, &self.outcomes, &error),
        };
        let outcome = ActionOutcome {
            id: invocation.command_id,
            heard: invocation.heard,
            context: invocation.context.label(),
            result: Ok(()),
        };
        if actions.is_empty() {
            let _ = self.outcomes.send(outcome);
        } else if let Err(error) = self.actions.try_send(ActionJob { outcome, actions }) {
            let (TrySendError::Full(job) | TrySendError::Disconnected(job)) = error;
            let _ = self.outcomes.send(ActionOutcome {
                result: Err("too many personal command actions are waiting".into()),
                ..job.outcome
            });
        }
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

/// Loads `config` from `workspace` and compiles its registration over
/// `base` as `generation`.
fn load_config(
    workspace: &Path,
    config: &str,
    base: &CommandConfig,
    generation: u64,
) -> Result<(Script, Candidate)> {
    let script = Script::load(workspace, config).map_err(|error| eyre!(error))?;
    if script.registration().len() > MAX_REGISTRATION_BYTES {
        return Err(eyre!(
            "the config registers more than {MAX_REGISTRATION_BYTES} bytes of commands"
        ));
    }
    let registration: Registration = serde_json::from_str(script.registration())
        .wrap_err("the config's commands or transformations are invalid")?;
    let catalog = status_catalog(&registration.commands);
    let dictation = registration.dictation.clone();
    let transformations = registration.transformations.clone();
    let runtime = compile_registration(
        base,
        generation,
        registration.dictation,
        registration.transformations,
        registration.commands,
    )?;
    Ok((
        script,
        Candidate {
            runtime,
            catalog,
            dictation,
            transformations,
        },
    ))
}

/// Loads the config as the running Hex would, over `base`, without
/// registering anything: what it registers, or why it fails.
#[cfg(target_os = "macos")]
pub fn check_workspace(base: &CommandConfig) -> Result<String> {
    check_workspace_at(&crate::app_paths::personal_commands_workspace()?, base)
}

#[cfg(any(target_os = "macos", test))]
fn check_workspace_at(workspace: &Path, base: &CommandConfig) -> Result<String> {
    let config = find_config(workspace)?.ok_or_else(|| {
        eyre!(
            "{} has no hex.config.ts; run `hex commands init`",
            workspace.display()
        )
    })?;
    let (_, candidate) = load_config(workspace, config, base, 0)?;
    Ok(format!(
        "{config}: {} commands, {} transformations",
        candidate.catalog.len(),
        candidate.transformations.len()
    ))
}

/// What a successful load registers, before it replaces the active registry.
struct Candidate {
    runtime: RuntimeSnapshot,
    catalog: Vec<StatusCommand>,
    dictation: Option<StatusDictationProtocol>,
    transformations: Vec<StatusTransformation>,
}

fn json(value: &impl Serialize) -> String {
    serde_json::to_string(value).expect("plain data serializes")
}

/// A handler's actions, validated before any of them runs.
fn parse_actions(json: &str) -> std::result::Result<Vec<Action>, String> {
    let actions: Vec<NativeAction> =
        serde_json::from_str(json).map_err(|error| format!("invalid action: {error}"))?;
    if actions.len() > MAX_ACTIONS_PER_INVOCATION {
        return Err(format!(
            "a command may run at most {MAX_ACTIONS_PER_INVOCATION} actions"
        ));
    }
    actions
        .into_iter()
        .map(|action| action.into_action().map_err(|error| error.to_string()))
        .collect()
}

/// The registration a config's `install` produces.
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Registration {
    dictation: Option<StatusDictationProtocol>,
    #[serde(default)]
    transformations: Vec<StatusTransformation>,
    commands: Vec<RegistrationCommand>,
}

#[derive(Clone)]
pub struct RuntimeSnapshot {
    pub commands: CommandConfig,
    pub dictation: Arc<DictationProtocol>,
    generation: u64,
    transformations: HashSet<String>,
}

impl RuntimeSnapshot {
    pub fn native(commands: CommandConfig) -> Self {
        Self {
            commands,
            dictation: Arc::new(DictationProtocol::default()),
            generation: 0,
            transformations: HashSet::new(),
        }
    }
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

    fn activate(&self, runtime: &RuntimeSnapshot, requests: SyncSender<Request>) {
        *self
            .active
            .write()
            .unwrap_or_else(|error| error.into_inner()) = Some(ActiveTransformations {
            generation: runtime.generation,
            ids: runtime.transformations.clone(),
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

#[derive(Deserialize)]
struct RegistrationCommand {
    id: String,
    phrases: Vec<String>,
    group: Option<String>,
    description: Option<String>,
    when: Option<When>,
    #[serde(default)]
    captures: BTreeMap<String, CaptureDescriptor>,
    execution: Execution,
}

#[derive(Clone, Deserialize, Serialize)]
#[serde(tag = "type", rename_all = "camelCase", deny_unknown_fields)]
enum CaptureDescriptor {
    Digit {
        min: u8,
        max: u8,
    },
    Letter,
    Choice {
        choices: BTreeMap<String, Vec<String>>,
    },
    Union {
        members: Vec<CaptureDescriptor>,
    },
    Text,
}

#[derive(Deserialize)]
#[serde(untagged)]
enum When {
    Application(ApplicationWhen),
    BrowserHost(BrowserHostWhen),
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct ApplicationWhen {
    application: String,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
struct BrowserHostWhen {
    browser_host: String,
}

#[derive(Deserialize)]
#[serde(
    tag = "type",
    rename_all = "camelCase",
    rename_all_fields = "camelCase"
)]
enum Execution {
    Native { action: NativeAction },
    Handler,
}

#[derive(Clone, Deserialize)]
#[serde(
    tag = "type",
    rename_all = "camelCase",
    rename_all_fields = "camelCase"
)]
enum NativeAction {
    OpenUrl {
        url: String,
    },
    OpenApplication {
        application: String,
    },
    OpenPath {
        path: String,
    },
    Press {
        key: String,
        #[serde(default)]
        modifiers: Vec<String>,
        #[serde(default = "one")]
        repeat: u8,
    },
    TypeText {
        text: String,
    },
}

#[derive(Serialize)]
#[serde(untagged)]
enum WireCaptureValue {
    Number(u8),
    Text(String),
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

const fn one() -> u8 {
    1
}

fn status_catalog(commands: &[RegistrationCommand]) -> Vec<StatusCommand> {
    commands
        .iter()
        .map(|command| StatusCommand {
            id: command.id.clone(),
            phrases: command.phrases.clone(),
            group: command.group.clone().unwrap_or_else(|| "Other".into()),
            description: command
                .description
                .clone()
                .unwrap_or_else(|| "Custom command".into()),
            context: match &command.when {
                None => StatusContext::Global,
                Some(When::Application(when)) => StatusContext::Application {
                    application: when.application.clone(),
                },
                Some(When::BrowserHost(when)) => StatusContext::BrowserHost {
                    browser_host: when.browser_host.clone(),
                },
            },
            execution: match &command.execution {
                Execution::Native { .. } => StatusExecution::Native,
                Execution::Handler => StatusExecution::Handler,
            },
        })
        .collect()
}

fn compile_capture_descriptor(
    command_id: &str,
    descriptor: &CaptureDescriptor,
    depth: usize,
) -> Result<PersonalCapture> {
    match descriptor {
        CaptureDescriptor::Digit { min, max } => {
            if min > max || *max > 9 {
                return Err(eyre!(
                    "{command_id}: digit capture ranges must be within 0 through 9"
                ));
            }
            Ok(PersonalCapture::Digit {
                min: *min,
                max: *max,
            })
        }
        CaptureDescriptor::Letter => Ok(PersonalCapture::Letter),
        CaptureDescriptor::Choice { choices } => {
            if choices.is_empty() || choices.len() > MAX_CHOICE_VALUES {
                return Err(eyre!(
                    "{command_id}: choice captures must contain 1 through {MAX_CHOICE_VALUES} values"
                ));
            }
            let mut spoken = BTreeMap::new();
            for (value, aliases) in choices {
                validate_text(value, "choice canonical value", MAX_CHOICE_VALUE_BYTES)?;
                if aliases.is_empty() || aliases.len() > MAX_CHOICE_ALIASES {
                    return Err(eyre!(
                        "{command_id}: each choice value must contain 1 through {MAX_CHOICE_ALIASES} aliases"
                    ));
                }
                for alias in aliases {
                    validate_text(alias, "choice spoken alias", MAX_CHOICE_WORD_BYTES)?;
                    let normalized = crate::spoken_text::normalize(alias);
                    if normalized.split_whitespace().count() != 1 {
                        return Err(eyre!(
                            "{command_id}: choice aliases must normalize to exactly one spoken word"
                        ));
                    }
                    if spoken.insert(normalized.clone(), value.clone()).is_some() {
                        return Err(eyre!(
                            "{command_id}: duplicate spoken choice alias {normalized}"
                        ));
                    }
                }
            }
            Ok(PersonalCapture::Choice { choices: spoken })
        }
        CaptureDescriptor::Union { members } => {
            if depth >= MAX_UNION_DEPTH {
                return Err(eyre!(
                    "{command_id}: union capture nesting may not exceed {MAX_UNION_DEPTH}"
                ));
            }
            if members.len() < 2 {
                return Err(eyre!(
                    "{command_id}: union captures must contain at least two members"
                ));
            }
            if serde_json::to_vec(descriptor)?.len() > MAX_UNION_SERIALIZED_BYTES {
                return Err(eyre!(
                    "{command_id}: union capture exceeds {MAX_UNION_SERIALIZED_BYTES} serialized bytes"
                ));
            }
            let mut flattened = Vec::new();
            for member in members {
                match compile_capture_descriptor(command_id, member, depth + 1)? {
                    PersonalCapture::Text => {
                        return Err(eyre!("{command_id}: union captures do not accept text()"));
                    }
                    PersonalCapture::Union { members } => flattened.extend(members),
                    member => flattened.push(member),
                }
            }
            if flattened.len() > MAX_UNION_MEMBERS {
                return Err(eyre!(
                    "{command_id}: union captures may contain at most {MAX_UNION_MEMBERS} flattened members"
                ));
            }
            for (index, left) in flattened.iter().enumerate() {
                if flattened[index + 1..]
                    .iter()
                    .any(|right| left.overlaps(right))
                {
                    return Err(eyre!(
                        "{command_id}: union capture members have overlapping spoken alternatives"
                    ));
                }
            }
            Ok(PersonalCapture::Union { members: flattened })
        }
        CaptureDescriptor::Text => Ok(PersonalCapture::Text),
    }
}

fn compile_registration(
    base: &CommandConfig,
    generation: u64,
    dictation: Option<StatusDictationProtocol>,
    transformations: Vec<StatusTransformation>,
    commands: Vec<RegistrationCommand>,
) -> Result<RuntimeSnapshot> {
    if commands.len() > MAX_COMMANDS {
        return Err(eyre!("personal registry exceeds {MAX_COMMANDS} commands"));
    }
    if transformations.len() > MAX_TRANSFORMATIONS {
        return Err(eyre!(
            "personal registry exceeds {MAX_TRANSFORMATIONS} transformations"
        ));
    }
    let transformation_ids = validate_transformations(&transformations, "")?;
    let mut compiled = base.clone();
    for command in commands {
        validate_text(&command.id, "command id", 128)?;
        if command.phrases.is_empty() || command.phrases.len() > MAX_PHRASES {
            return Err(eyre!(
                "{} must have 1 through {MAX_PHRASES} phrases",
                command.id
            ));
        }
        for phrase in &command.phrases {
            validate_text(phrase, "command phrase", 256)?;
        }
        base.validate_personal_identity(&command.id, &command.phrases)?;
        let context = match command.when {
            None => ContextPredicate::Always,
            Some(When::Application(when)) => {
                validate_text(&when.application, "application context", 256)?;
                ContextPredicate::application(when.application)
            }
            Some(When::BrowserHost(when)) => {
                validate_text(&when.browser_host, "browser host context", 253)?;
                validate_browser_host(&when.browser_host)?;
                ContextPredicate::browser_host(when.browser_host)
            }
        };
        let description = command
            .description
            .unwrap_or_else(|| "Custom command".into());
        validate_text(&description, "command description", 1024)?;
        if let Some(group) = &command.group {
            validate_text(group, "command group", 1024)?;
        }
        let group = Some(command.group.unwrap_or_else(|| "Other".into()));
        if command.captures.len() > MAX_CAPTURE_ENTRIES {
            return Err(eyre!(
                "{}: commands may contain at most {MAX_CAPTURE_ENTRIES} captures",
                command.id
            ));
        }
        let captures = command
            .captures
            .iter()
            .map(|(name, descriptor)| {
                validate_text(name, "capture name", 64)?;
                let capture = compile_capture_descriptor(&command.id, descriptor, 0)?;
                Ok((name.clone(), capture))
            })
            .collect::<Result<BTreeMap<_, _>>>()?;
        let configured = match command.execution {
            Execution::Native { action } => {
                if !captures.is_empty() {
                    return Err(eyre!(
                        "{}: capture descriptors require a handler command (use run instead of action)",
                        command.id
                    ));
                }
                for phrase in &command.phrases {
                    match crate::commands::phrase_placeholder(phrase) {
                        Ok(None) => {}
                        Ok(Some(_)) => {
                            return Err(eyre!(
                                "{}: {{capture}} placeholders require a handler command (use run instead of action)",
                                command.id
                            ));
                        }
                        Err(message) => return Err(eyre!("{}: {message}", command.id)),
                    }
                }
                ConfiguredCommand::personal_literal(
                    command.id,
                    description,
                    command.phrases,
                    context,
                    action.into_action()?,
                    group,
                )?
            }
            Execution::Handler => ConfiguredCommand::personal_command(
                command.id,
                description,
                command.phrases,
                context,
                (!captures.is_empty()).then_some(captures),
                move |captures| Action::InvokeHandler {
                    generation,
                    captures,
                },
                group,
            )?,
        };
        compiled.try_command(configured)?;
    }
    let dictation = match dictation {
        Some(dictation) => DictationProtocol::try_new(
            dictation.start,
            dictation.stop,
            dictation.send,
            dictation.cancel,
        )
        .map_err(|error| eyre!(error))?,
        None => DictationProtocol::default(),
    };
    compiled.validate_protocol_identity("dictation.start", dictation.start_phrases(), true)?;
    let controls = dictation.control_phrases().cloned().collect::<Vec<_>>();
    compiled.validate_protocol_identity("dictation.control", &controls, false)?;
    Ok(RuntimeSnapshot {
        commands: compiled,
        dictation: Arc::new(dictation),
        generation,
        transformations: transformation_ids,
    })
}

impl NativeAction {
    fn into_action(self) -> Result<Action> {
        Ok(match self {
            Self::OpenUrl { url } => {
                validate_text(&url, "URL", 4096)?;
                if url.bytes().any(|byte| byte.is_ascii_control()) {
                    return Err(eyre!(
                        "personal command URL must not contain control characters"
                    ));
                }
                let parsed = url::Url::parse(&url).wrap_err("personal command URL is invalid")?;
                if matches!(parsed.scheme(), "file" | "javascript" | "data" | "vbscript") {
                    return Err(eyre!(
                        "personal command URL scheme is not supported; use openPath for files"
                    ));
                }
                Action::OpenUrl(url)
            }
            Self::OpenApplication { application } => {
                validate_text(&application, "application", 4096)?;
                Action::OpenApplication(application)
            }
            Self::OpenPath { path } => {
                validate_text(&path, "path", 4096)?;
                Action::OpenPath(path)
            }
            Self::TypeText { text } => {
                validate_text(&text, "text", 4096)?;
                Action::TypeText(text)
            }
            Self::Press {
                key,
                modifiers,
                repeat,
            } => {
                if repeat == 0 || repeat > MAX_REPEAT {
                    return Err(eyre!("press repeat must be from 1 through {MAX_REPEAT}"));
                }
                let key = parse_key(&key)?;
                let modifiers = parse_modifiers(&modifiers)?;
                if repeat == 1 {
                    Action::Keystroke { key, modifiers }
                } else {
                    Action::RepeatedKeystroke {
                        key,
                        modifiers,
                        count: repeat,
                    }
                }
            }
        })
    }
}

fn parse_key(value: &str) -> Result<Key> {
    match value.to_ascii_lowercase().as_str() {
        "home" => Ok(Key::Home),
        "end" => Ok(Key::End),
        "up" => Ok(Key::Up),
        "down" => Ok(Key::Down),
        "left" => Ok(Key::Left),
        "right" => Ok(Key::Right),
        "enter" => Ok(Key::Enter),
        "escape" => Ok(Key::Escape),
        _ => {
            let mut characters = value.chars();
            let character = characters
                .next()
                .ok_or_else(|| eyre!("press key is empty"))?;
            if characters.next().is_some() {
                Err(eyre!(
                    "press key must be a character or supported named key"
                ))
            } else {
                Ok(Key::Character(character))
            }
        }
    }
}

fn parse_modifiers(values: &[String]) -> Result<Modifiers> {
    values.iter().try_fold(Modifiers::NONE, |modifiers, value| {
        let next = match value.as_str() {
            "command" => Modifiers::COMMAND,
            "control" => Modifiers::CONTROL,
            "option" => Modifiers::OPTION,
            "shift" => Modifiers::SHIFT,
            _ => return Err(eyre!("unsupported press modifier: {value}")),
        };
        Ok(modifiers.with(next))
    })
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

fn validate_browser_host(host: &str) -> Result<()> {
    let parsed = url::Host::parse(host).wrap_err("browser host context must be an exact host")?;
    if crate::context::browser_hosts_equal(&parsed.to_string(), host) {
        Ok(())
    } else {
        Err(eyre!("browser host context must be an exact host"))
    }
}

fn bounded_error(message: &str) -> String {
    message.chars().take(4096).collect()
}

fn bounded_status_error(message: &str) -> String {
    message[..message.floor_char_boundary(MAX_STATUS_ERROR_BYTES)].to_owned()
}

fn emit_failure(invocation: Invocation, outcomes: &SyncSender<ActionOutcome>, error: &str) {
    let _ = outcomes.send(ActionOutcome {
        id: invocation.command_id,
        heard: invocation.heard,
        context: invocation.context.label(),
        result: Err(bounded_error(error)),
    });
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::commands::{Decision, Mode};
    use std::time::UNIX_EPOCH;

    fn compile_commands(commands: Vec<RegistrationCommand>) -> Result<RuntimeSnapshot> {
        compile_registration(&CommandConfig::new(), 1, None, vec![], commands)
    }

    fn compile_one(command: RegistrationCommand) -> Result<RuntimeSnapshot> {
        compile_commands(vec![command])
    }

    fn compile_registration_json(json: &str) -> Result<RuntimeSnapshot> {
        let registration: Registration = serde_json::from_str(json).unwrap();
        compile_registration(
            &CommandConfig::new(),
            7,
            None,
            vec![],
            registration.commands,
        )
    }

    fn handler_command(
        phrase: &str,
        captures: impl IntoIterator<Item = (&'static str, CaptureDescriptor)>,
    ) -> RegistrationCommand {
        RegistrationCommand {
            id: "bad".into(),
            phrases: vec![phrase.into()],
            group: None,
            description: None,
            when: None,
            captures: captures
                .into_iter()
                .map(|(name, descriptor)| (name.into(), descriptor))
                .collect(),
            execution: Execution::Handler,
        }
    }

    #[test]
    fn native_url_actions_preserve_deep_links() {
        for url in [
            "https://example.com",
            "slack://channel?team=T_EXAMPLE&id=C_EXAMPLE",
            "obsidian://open?vault=Work%20Notes&file=Plans%2FToday#section",
            "mailto:hello@example.com?subject=Hello%20there",
            "x-example+test://route?value=one%26two",
            "SLACK://channel?team=T_EXAMPLE&id=C_EXAMPLE",
        ] {
            let action = NativeAction::OpenUrl { url: url.into() }
                .into_action()
                .unwrap();
            assert!(matches!(action, Action::OpenUrl(value) if value == url));
        }
    }

    #[test]
    fn native_url_actions_reject_invalid_and_non_launchable_urls() {
        for url in [
            "relative/path",
            "//example.com",
            "",
            "slack://channel\0",
            "javascript:alert(1)",
            "JaVaScRiPt:alert(1)",
            "data:text/plain,hello",
            "vbscript:msgbox(1)",
            "file:///tmp/example",
        ] {
            assert!(
                NativeAction::OpenUrl { url: url.into() }
                    .into_action()
                    .is_err(),
                "{url:?}"
            );
        }
    }

    #[test]
    fn registration_compiles_native_and_handler_commands() {
        let compiled = compile_registration_json(
            r#"{"commands":[{"id":"native","phrases":["open example"],"execution":{"type":"native","action":{"type":"openUrl","url":"slack://channel?team=T_EXAMPLE&id=C_EXAMPLE"}}},{"id":"handled","phrases":["do work"],"group":"Work","when":{"application":"Slack"},"execution":{"type":"handler"}}]}"#,
        )
        .unwrap();

        assert!(matches!(
            compiled.commands.resolve(Mode::Listening, "open example", &ContextSnapshot::default()),
            Decision::Execute { action: Action::OpenUrl(url), .. } if url == "slack://channel?team=T_EXAMPLE&id=C_EXAMPLE"
        ));
        let slack = ContextSnapshot {
            application: Some("Slack".into()),
            ..ContextSnapshot::default()
        };
        assert!(matches!(
            compiled
                .commands
                .resolve(Mode::Listening, "do work", &slack),
            Decision::Execute {
                id: "handled",
                action: Action::InvokeHandler {
                    generation: 7,
                    captures,
                },
            } if captures.is_empty()
        ));
        assert_eq!(
            compiled
                .commands
                .catalog()
                .into_iter()
                .find(|command| command.id == "handled")
                .and_then(|command| command.group),
            Some("Work".into())
        );
        assert_eq!(
            compiled
                .commands
                .catalog()
                .into_iter()
                .find(|command| command.id == "native")
                .and_then(|command| command.group),
            Some("Other".into())
        );
    }

    #[test]
    fn capture_phrases_compile_for_handlers_and_carry_the_spoken_remainder() {
        let compiled = compile_registration_json(
            r#"{"commands":[{"id":"search","phrases":["search amazon for {query}"],"execution":{"type":"handler"}}]}"#,
        )
        .unwrap();

        match compiled.commands.resolve(
            Mode::Listening,
            "search amazon for wool socks",
            &ContextSnapshot::default(),
        ) {
            Decision::Execute {
                id: "search",
                action:
                    Action::InvokeHandler {
                        generation: 7,
                        captures,
                    },
            } => {
                assert_eq!(
                    captures.get("query"),
                    Some(&CapturedValue::Text("wool socks".into()))
                );
            }
            _ => panic!("expected the capture command to execute"),
        }
        assert!(matches!(
            compiled.commands.resolve(
                Mode::Listening,
                "search amazon for",
                &ContextSnapshot::default()
            ),
            Decision::Ignore
        ));
    }

    #[test]
    fn typed_capture_phrases_resolve_multiple_digits_and_trailing_text() {
        let command: RegistrationCommand = serde_json::from_str(
            r#"{"id":"move","phrases":["move {from} to {to} named {label}"],"captures":{"from":{"type":"digit","min":1,"max":3},"to":{"type":"digit","min":4,"max":6},"label":{"type":"text"}},"execution":{"type":"handler"}}"#,
        )
        .unwrap();
        let compiled = compile_one(command).unwrap();

        match compiled.commands.resolve(
            Mode::Listening,
            "move two to 5 named important item",
            &ContextSnapshot::default(),
        ) {
            Decision::Execute {
                action: Action::InvokeHandler { captures, .. },
                ..
            } => {
                assert_eq!(captures.get("from"), Some(&CapturedValue::Digit(2)));
                assert_eq!(captures.get("to"), Some(&CapturedValue::Digit(5)));
                assert_eq!(
                    captures.get("label"),
                    Some(&CapturedValue::Text("important item".into()))
                );
            }
            _ => panic!("expected typed capture command"),
        }
        assert!(matches!(
            compiled.commands.resolve(
                Mode::Listening,
                "move four to 5 named important item",
                &ContextSnapshot::default()
            ),
            Decision::Ignore
        ));
    }

    #[test]
    fn choice_captures_resolve_aliases_to_canonical_values() {
        let command: RegistrationCommand = serde_json::from_str(
            r#"{"id":"move","phrases":["move {direction} to {edge}"],"captures":{"direction":{"type":"choice","choices":{"left":["left","back"],"right":["right","forward"]}},"edge":{"type":"choice","choices":{"top":["top"],"bottom":["bottom"]}}},"execution":{"type":"handler"}}"#,
        )
        .unwrap();
        let compiled = compile_one(command).unwrap();

        match compiled.commands.resolve(
            Mode::Listening,
            "move back to bottom",
            &ContextSnapshot::default(),
        ) {
            Decision::Execute {
                action: Action::InvokeHandler { captures, .. },
                ..
            } => {
                assert_eq!(
                    captures.get("direction"),
                    Some(&CapturedValue::Choice("left".into()))
                );
                assert_eq!(
                    captures.get("edge"),
                    Some(&CapturedValue::Choice("bottom".into()))
                );
            }
            _ => panic!("expected choice capture command"),
        }
    }

    #[test]
    fn letter_captures_resolve_spoken_and_nato_aliases_to_lowercase() {
        let command: RegistrationCommand = serde_json::from_str(
            r#"{"id":"control","phrases":["control {key}"],"captures":{"key":{"type":"letter"}},"execution":{"type":"handler"}}"#,
        )
        .unwrap();
        let compiled = compile_one(command).unwrap();

        for (heard, expected) in [
            ("control A", 'a'),
            ("control ay", 'a'),
            ("control alpha", 'a'),
            ("control bee", 'b'),
            ("control echo", 'e'),
            ("control aitch", 'h'),
            ("control juliett", 'j'),
            ("control cue", 'q'),
            ("control whiskey", 'w'),
            ("control xray", 'x'),
            ("control why", 'y'),
            ("control zed", 'z'),
            ("control zulu", 'z'),
        ] {
            match compiled
                .commands
                .resolve(Mode::Listening, heard, &ContextSnapshot::default())
            {
                Decision::Execute {
                    action: Action::InvokeHandler { captures, .. },
                    ..
                } => assert_eq!(captures.get("key"), Some(&CapturedValue::Letter(expected))),
                _ => panic!("expected {heard} to resolve"),
            }
        }
    }

    #[test]
    fn union_captures_resolve_each_kind_and_multiple_capture_positions() {
        let command: RegistrationCommand = serde_json::from_str(
            r#"{"id":"keys","phrases":["key {first} then {second}"],"captures":{"first":{"type":"union","members":[{"type":"letter"},{"type":"digit","min":0,"max":9},{"type":"choice","choices":{"home":["home"],"escape":["escape","cancel"]}}]},"second":{"type":"union","members":[{"type":"digit","min":0,"max":9},{"type":"choice","choices":{"enter":["enter"]}}]}},"execution":{"type":"handler"}}"#,
        )
        .unwrap();
        let compiled = compile_one(command).unwrap();

        for (heard, first, second) in [
            (
                "key alpha then 2",
                CapturedValue::Letter('a'),
                CapturedValue::Digit(2),
            ),
            (
                "key 7 then enter",
                CapturedValue::Digit(7),
                CapturedValue::Choice("enter".into()),
            ),
            (
                "key cancel then 4",
                CapturedValue::Choice("escape".into()),
                CapturedValue::Digit(4),
            ),
        ] {
            match compiled
                .commands
                .resolve(Mode::Listening, heard, &ContextSnapshot::default())
            {
                Decision::Execute {
                    action: Action::InvokeHandler { captures, .. },
                    ..
                } => {
                    assert_eq!(captures.get("first"), Some(&first));
                    assert_eq!(captures.get("second"), Some(&second));
                }
                _ => panic!("expected {heard} to resolve"),
            }
        }
    }

    #[test]
    fn nested_unions_flatten_and_invalid_unions_are_rejected_transactionally() {
        let valid: RegistrationCommand = serde_json::from_str(
            r#"{"id":"key","phrases":["key {key}"],"captures":{"key":{"type":"union","members":[{"type":"letter"},{"type":"union","members":[{"type":"digit","min":0,"max":9},{"type":"choice","choices":{"home":["home"]}}]}]}},"execution":{"type":"handler"}}"#,
        )
        .unwrap();
        assert!(compile_one(valid).is_ok());

        for input in [
            r#"{"id":"bad","phrases":["key {key}"],"captures":{"key":{"type":"union","members":[{"type":"letter"}]}},"execution":{"type":"handler"}}"#,
            r#"{"id":"bad","phrases":["key {key}"],"captures":{"key":{"type":"union","members":[{"type":"letter"},{"type":"text"}]}},"execution":{"type":"handler"}}"#,
            r#"{"id":"bad","phrases":["key {key}"],"captures":{"key":{"type":"union","members":[{"type":"digit","min":0,"max":9},{"type":"choice","choices":{"two":["2"]}}]}},"execution":{"type":"handler"}}"#,
            r#"{"id":"bad","phrases":["key {key}"],"captures":{"key":{"type":"union","members":[{"type":"letter"},{"type":"choice","choices":{"a":["alpha"]}}]}},"execution":{"type":"handler"}}"#,
            r#"{"id":"bad","phrases":["key {key}"],"captures":{"key":{"type":"union","members":[{"type":"letter"},{"type":"union","members":[{"type":"digit","min":0,"max":0},{"type":"union","members":[{"type":"choice","choices":{"home":["home"]}},{"type":"union","members":[{"type":"choice","choices":{"end":["end"]}},{"type":"union","members":[{"type":"choice","choices":{"up":["up"]}},{"type":"choice","choices":{"down":["down"]}}]}]}]}]}]}},"execution":{"type":"handler"}}"#,
        ] {
            let command = serde_json::from_str(input).unwrap();
            assert!(
                compile_one(command).is_err(),
                "union should be rejected: {input}"
            );
        }

        let members = (0..=MAX_UNION_MEMBERS)
            .map(|index| CaptureDescriptor::Choice {
                choices: BTreeMap::from([(format!("key{index}"), vec![format!("key{index}")])]),
            })
            .collect();
        let command = handler_command("key {key}", [("key", CaptureDescriptor::Union { members })]);
        assert!(compile_one(command).is_err());

        let large_choice = |prefix: &str| CaptureDescriptor::Choice {
            choices: (0..16)
                .map(|value| {
                    (
                        format!("{prefix}{value}"),
                        (0..16)
                            .map(|alias| format!("{prefix}-{value}-{alias}-{}", "x".repeat(40)))
                            .collect(),
                    )
                })
                .collect(),
        };
        let command = handler_command(
            "key {key}",
            [(
                "key",
                CaptureDescriptor::Union {
                    members: vec![large_choice("left"), large_choice("right")],
                },
            )],
        );
        assert!(compile_one(command).is_err());
    }

    #[test]
    fn union_overlap_checks_all_existing_token_kinds() {
        let compile_pair = |right: &str| {
            let union = serde_json::from_str(
                r#"{"id":"union","phrases":["press {key}"],"captures":{"key":{"type":"union","members":[{"type":"digit","min":0,"max":2},{"type":"choice","choices":{"home":["home"]}}]}},"execution":{"type":"handler"}}"#,
            )
            .unwrap();
            compile_commands(vec![union, serde_json::from_str(right).unwrap()])
        };

        for right in [
            r#"{"id":"literal","phrases":["press home"],"execution":{"type":"handler"}}"#,
            r#"{"id":"digit","phrases":["press {key}"],"captures":{"key":{"type":"digit","min":2,"max":4}},"execution":{"type":"handler"}}"#,
            r#"{"id":"choice","phrases":["press {key}"],"captures":{"key":{"type":"choice","choices":{"house":["home"]}}},"execution":{"type":"handler"}}"#,
            r#"{"id":"text","phrases":["press {key}"],"captures":{"key":{"type":"text"}},"execution":{"type":"handler"}}"#,
            r#"{"id":"union2","phrases":["press {key}"],"captures":{"key":{"type":"union","members":[{"type":"letter"},{"type":"choice","choices":{"house":["home"]}}]}},"execution":{"type":"handler"}}"#,
        ] {
            assert!(compile_pair(right).is_err(), "expected overlap: {right}");
        }
        assert!(compile_pair(
            r#"{"id":"disjoint","phrases":["press {key}"],"captures":{"key":{"type":"union","members":[{"type":"digit","min":3,"max":5},{"type":"choice","choices":{"end":["end"]}}]}},"execution":{"type":"handler"}}"#
        )
        .is_ok());
    }

    #[test]
    fn letter_overlap_checks_literals_choices_letters_digits_and_text() {
        let compile_pair = |right: &str| {
            let letter = serde_json::from_str(
                r#"{"id":"letter","phrases":["press {key}"],"captures":{"key":{"type":"letter"}},"execution":{"type":"handler"}}"#,
            )
            .unwrap();
            let right = serde_json::from_str(right).unwrap();
            compile_commands(vec![letter, right])
        };

        assert!(
            compile_pair(
                r#"{"id":"literal","phrases":["press alpha"],"execution":{"type":"handler"}}"#
            )
            .is_err()
        );
        assert!(compile_pair(r#"{"id":"choice","phrases":["press {key}"],"captures":{"key":{"type":"choice","choices":{"insect":["bee"]}}},"execution":{"type":"handler"}}"#).is_err());
        assert!(compile_pair(r#"{"id":"letter-2","phrases":["press {key}"],"captures":{"key":{"type":"letter"}},"execution":{"type":"handler"}}"#).is_err());
        assert!(compile_pair(r#"{"id":"text","phrases":["press {key}"],"captures":{"key":{"type":"text"}},"execution":{"type":"handler"}}"#).is_err());
        assert!(compile_pair(r#"{"id":"digit","phrases":["press {key}"],"captures":{"key":{"type":"digit","min":0,"max":9}},"execution":{"type":"handler"}}"#).is_ok());
    }

    #[test]
    fn invalid_choice_descriptors_are_rejected() {
        for input in [
            r#"{"id":"bad","phrases":["move {direction}"],"captures":{"direction":{"type":"choice","choices":{}}},"execution":{"type":"handler"}}"#,
            r#"{"id":"bad","phrases":["move {direction}"],"captures":{"direction":{"type":"choice","choices":{"left":[]}}},"execution":{"type":"handler"}}"#,
            r#"{"id":"bad","phrases":["move {direction}"],"captures":{"direction":{"type":"choice","choices":{"left":["two words"]}}},"execution":{"type":"handler"}}"#,
            r#"{"id":"bad","phrases":["move {direction}"],"captures":{"direction":{"type":"choice","choices":{"left":["LEFT"],"other":["left!"]}}},"execution":{"type":"handler"}}"#,
            r#"{"id":"bad","phrases":["move {direction}"],"captures":{"direction":{"type":"choice","choices":{"left":["left"]},"extra":true}},"execution":{"type":"handler"}}"#,
        ] {
            let command = serde_json::from_str(input);
            if let Ok(command) = command {
                assert!(
                    compile_one(command).is_err(),
                    "descriptor should be rejected: {input}"
                );
            }
        }

        let command = |choices| {
            handler_command(
                "move {direction}",
                [("direction", CaptureDescriptor::Choice { choices })],
            )
        };
        let oversized_values = (0..=MAX_CHOICE_VALUES)
            .map(|index| (format!("value{index}"), vec![format!("word{index}")]))
            .collect();
        for choices in [
            BTreeMap::from([("x".repeat(MAX_CHOICE_VALUE_BYTES + 1), vec!["left".into()])]),
            BTreeMap::from([("left".into(), vec!["x".repeat(MAX_CHOICE_WORD_BYTES + 1)])]),
            BTreeMap::from([(
                "left".into(),
                (0..=MAX_CHOICE_ALIASES)
                    .map(|index| format!("word{index}"))
                    .collect(),
            )]),
            oversized_values,
        ] {
            assert!(compile_one(command(choices)).is_err());
        }
    }

    #[test]
    fn choice_overlap_checks_literals_digits_choices_and_text() {
        let compile_pair = |left: &str, right: &str| {
            let left = serde_json::from_str(left).unwrap();
            let right = serde_json::from_str(right).unwrap();
            compile_commands(vec![left, right])
        };
        let choice = r#"{"id":"choice","phrases":["press {value}"],"captures":{"value":{"type":"choice","choices":{"left":["left"],"second":["two"]}}},"execution":{"type":"handler"}}"#;

        assert!(
            compile_pair(
                choice,
                r#"{"id":"literal","phrases":["press left"],"execution":{"type":"handler"}}"#
            )
            .is_err()
        );
        assert!(compile_pair(
            choice,
            r#"{"id":"digit","phrases":["press {value}"],"captures":{"value":{"type":"digit","min":1,"max":2}},"execution":{"type":"handler"}}"#
        )
        .is_err());
        assert!(compile_pair(
            choice,
            r#"{"id":"other-choice","phrases":["press {value}"],"captures":{"value":{"type":"choice","choices":{"backward":["left"]}}},"execution":{"type":"handler"}}"#
        )
        .is_err());
        assert!(compile_pair(
            choice,
            r#"{"id":"text","phrases":["press {value}"],"captures":{"value":{"type":"text"}},"execution":{"type":"handler"}}"#
        )
        .is_err());
        assert!(compile_pair(
            choice,
            r#"{"id":"disjoint-digit","phrases":["press {value}"],"captures":{"value":{"type":"digit","min":3,"max":5}},"execution":{"type":"handler"}}"#
        )
        .is_ok());
        assert!(compile_pair(
            choice,
            r#"{"id":"disjoint-choice","phrases":["press {value}"],"captures":{"value":{"type":"choice","choices":{"right":["right"]}}},"execution":{"type":"handler"}}"#
        )
        .is_ok());
    }

    #[test]
    fn typed_capture_aliases_and_ranges_are_validated() {
        for input in [
            r#"{"id":"bad","phrases":["move {from} to {to}","move {from}"],"captures":{"from":{"type":"digit","min":1,"max":3},"to":{"type":"digit","min":4,"max":6}},"execution":{"type":"handler"}}"#,
            r#"{"id":"bad","phrases":["say {words} now"],"captures":{"words":{"type":"text"}},"execution":{"type":"handler"}}"#,
            r#"{"id":"bad","phrases":["press {number}"],"captures":{"number":{"type":"digit","min":3,"max":10}},"execution":{"type":"handler"}}"#,
        ] {
            let command = serde_json::from_str(input).unwrap();
            assert!(compile_one(command).is_err());
        }

        let first = serde_json::from_str(
            r#"{"id":"first","phrases":["press {number}"],"captures":{"number":{"type":"digit","min":1,"max":3}},"execution":{"type":"handler"}}"#,
        )
        .unwrap();
        let disjoint = serde_json::from_str(
            r#"{"id":"disjoint","phrases":["press {number}"],"captures":{"number":{"type":"digit","min":4,"max":6}},"execution":{"type":"handler"}}"#,
        )
        .unwrap();
        assert!(compile_commands(vec![first, disjoint]).is_ok());

        let first = serde_json::from_str(
            r#"{"id":"first","phrases":["press {number}"],"captures":{"number":{"type":"digit","min":1,"max":3}},"execution":{"type":"handler"}}"#,
        )
        .unwrap();
        let overlapping = serde_json::from_str(
            r#"{"id":"overlapping","phrases":["press {number}"],"captures":{"number":{"type":"digit","min":3,"max":5}},"execution":{"type":"handler"}}"#,
        )
        .unwrap();
        assert!(compile_commands(vec![first, overlapping]).is_err());
    }

    #[test]
    fn capture_phrases_are_rejected_for_native_commands() {
        let result = compile_registration_json(
            r#"{"commands":[{"id":"search","phrases":["search amazon for {query}"],"execution":{"type":"native","action":{"type":"openUrl","url":"https://example.com"}}}]}"#,
        );

        match result {
            Ok(_) => panic!("native capture phrases are invalid"),
            Err(error) => assert!(error.to_string().contains("handler")),
        }
    }

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

    #[test]
    fn invalid_candidate_does_not_mutate_base_registry() {
        let base = CommandConfig::new().command(
            ConfiguredCommand::literal(
                "base",
                "Base",
                ["reserved phrase"],
                ContextPredicate::Always,
                Action::TypeText("base".into()),
            )
            .unwrap(),
        );
        let command: RegistrationCommand = serde_json::from_str(
            r#"{"id":"personal","phrases":["reserved phrase"],"execution":{"type":"handler"}}"#,
        )
        .unwrap();

        assert!(compile_registration(&base, 1, None, vec![], vec![command]).is_err());
        assert!(matches!(
            base.resolve(
                Mode::Listening,
                "reserved phrase",
                &ContextSnapshot::default()
            ),
            Decision::Execute { id: "base", .. }
        ));
    }

    #[test]
    fn configured_dictation_protocol_replaces_defaults_and_omission_falls_back() {
        let configured = compile_registration(
            &CommandConfig::new(),
            1,
            Some(StatusDictationProtocol {
                start: vec!["begin note".into()],
                stop: vec!["finish note".into()],
                send: vec!["send note".into()],
                cancel: vec!["discard note".into()],
            }),
            vec![],
            vec![],
        )
        .unwrap();
        assert!(
            configured
                .dictation
                .start_prefix("Begin note, hello")
                .is_some()
        );
        assert!(
            configured
                .dictation
                .start_prefix("Dictate start, hello")
                .is_none()
        );

        let fallback = compile_commands(vec![]).unwrap();
        assert!(
            fallback
                .dictation
                .start_prefix("Dictate start, hello")
                .is_some()
        );
    }

    #[test]
    fn dictation_protocol_collisions_reject_the_whole_registration() {
        let command: RegistrationCommand = serde_json::from_str(
            r#"{"id":"personal","phrases":["begin note now"],"execution":{"type":"handler"}}"#,
        )
        .unwrap();
        let protocol = StatusDictationProtocol {
            start: vec!["begin note".into()],
            stop: vec!["finish note".into()],
            send: vec!["send note".into()],
            cancel: vec!["discard note".into()],
        };

        assert!(
            compile_registration(
                &CommandConfig::new(),
                1,
                Some(protocol),
                vec![],
                vec![command]
            )
            .is_err()
        );
    }

    #[test]
    fn wake_sleep_ids_and_phrases_are_reserved() {
        let base = CommandConfig::new()
            .wake_with(["voice control"])
            .sleep_with(["go to sleep"]);
        for input in [
            r#"{"id":"mode.wake","phrases":["something else"],"execution":{"type":"handler"}}"#,
            r#"{"id":"personal","phrases":["voice control"],"execution":{"type":"handler"}}"#,
            r#"{"id":"personal","phrases":["go to sleep"],"execution":{"type":"handler"}}"#,
        ] {
            let command = serde_json::from_str(input).unwrap();
            assert!(compile_registration(&base, 1, None, vec![], vec![command]).is_err());
        }
    }

    #[test]
    fn browser_hosts_are_exact_and_canonical() {
        let command: RegistrationCommand = serde_json::from_str(
            r#"{"id":"bad","phrases":["bad"],"when":{"browserHost":"https://x.com/path"},"execution":{"type":"handler"}}"#,
        )
        .unwrap();
        assert!(compile_one(command).is_err());

        let context = ContextSnapshot {
            application: Some("Brave Browser".into()),
            browser_url: Some(url::Url::parse("https://X.COM./home").unwrap()),
            ..ContextSnapshot::default()
        };
        assert!(context.browser_host_is("x.com"));
        assert!(!context.browser_host_is("www.x.com"));
    }

    #[test]
    fn context_predicates_reject_ambiguous_objects() {
        let result = serde_json::from_str::<RegistrationCommand>(
            r#"{"id":"bad","phrases":["bad"],"when":{"application":"Brave Browser","browserHost":"x.com"},"execution":{"type":"handler"}}"#,
        );

        assert!(result.is_err());
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
            catalog: vec![StatusCommand {
                id: "open-example".into(),
                phrases: vec!["open example".into(), "show example".into()],
                group: "Web".into(),
                description: "Open the example site".into(),
                context: StatusContext::BrowserHost {
                    browser_host: "example.com".into(),
                },
                execution: StatusExecution::Native,
            }],
            dictation: Some(StatusDictationProtocol {
                start: vec!["begin note".into()],
                stop: vec!["finish note".into()],
                send: vec!["send note".into()],
                cancel: vec!["discard note".into()],
            }),
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
    fn status_snapshot_enforces_catalog_and_error_bounds() {
        let command = StatusCommand {
            id: "custom".into(),
            phrases: vec!["custom phrase".into()],
            group: "Other".into(),
            description: "Custom command".into(),
            context: StatusContext::Global,
            execution: StatusExecution::Handler,
        };
        let mut status = StatusSnapshot {
            catalog: vec![command; MAX_COMMANDS + 1],
            ..StatusSnapshot::default()
        };
        assert!(validate_status(&status).is_err());

        status.catalog.clear();
        status.last_reload_error = Some("x".repeat(MAX_STATUS_ERROR_BYTES + 1));
        assert!(validate_status(&status).is_err());
    }

    /// A worker over a fresh config folder, without threads: tests drive it
    /// directly and read the actions it hands off.
    struct Harness {
        root: PathBuf,
        worker: Worker,
        actions: Receiver<ActionJob>,
        outcomes: Receiver<ActionOutcome>,
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
            let (outcome_sender, outcomes) = mpsc::sync_channel(8);
            let (action_sender, actions) = mpsc::sync_channel(8);
            let mut worker = Worker {
                base: CommandConfig::new(),
                active: Arc::new(RwLock::new(Arc::new(RuntimeSnapshot::native(
                    CommandConfig::new(),
                )))),
                transformations: Arc::new(TransformationClient::default()),
                requests,
                outcomes: outcome_sender,
                actions: action_sender,
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
                actions,
                outcomes,
                _requests: request_receiver,
            }
        }

        fn write(&mut self, config: &str) {
            fs::write(self.worker.workspace.join("hex.config.ts"), config).unwrap();
            self.worker.reload();
        }

        fn snapshot(&self) -> Arc<RuntimeSnapshot> {
            self.worker.active.read().unwrap().clone()
        }

        fn error(&self) -> String {
            self.worker
                .status
                .last_reload_error
                .clone()
                .unwrap_or_default()
        }

        /// Says `heard` and runs the handler it resolves to.
        fn say(&self, heard: &str) -> std::result::Result<Vec<Action>, String> {
            let snapshot = self.snapshot();
            let Decision::Execute {
                id,
                action:
                    Action::InvokeHandler {
                        generation,
                        captures,
                    },
            } = snapshot
                .commands
                .resolve(Mode::Listening, heard, &ContextSnapshot::default())
            else {
                panic!("{heard:?} did not resolve to a handler");
            };
            self.worker.invoke(Invocation {
                generation,
                command_id: id.into(),
                heard: heard.into(),
                context: ContextSnapshot::default(),
                captures,
            });
            match self.actions.try_recv() {
                Ok(job) => Ok(job.actions),
                Err(_) => match self.outcomes.try_recv().unwrap().result {
                    Ok(()) => Ok(Vec::new()),
                    Err(error) => Err(error),
                },
            }
        }

        fn transform(&self, ids: &[&str], text: &str) -> std::result::Result<String, String> {
            let (response, result) = mpsc::sync_channel(1);
            self.worker.transform(TransformationRequest {
                generation: self.snapshot().generation,
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
    fn the_starter_config_registers_and_its_handlers_return_actions() {
        let harness = Harness::new(TEMPLATE_CONFIG);
        assert_eq!(
            harness.worker.status.host_state,
            HostState::Active,
            "{}",
            harness.error()
        );
        assert_eq!(
            harness
                .worker
                .status
                .catalog
                .iter()
                .map(|command| (command.id.as_str(), command.execution))
                .collect::<Vec<_>>(),
            [
                ("open-example", StatusExecution::Native),
                ("search-amazon", StatusExecution::Handler),
                ("control-key", StatusExecution::Handler),
            ]
        );
        assert!(matches!(
            harness.say("search amazon for wool socks").unwrap().as_slice(),
            [Action::OpenUrl(url)] if url == "https://www.amazon.com/s?k=wool%20socks"
        ));
        assert!(matches!(
            harness.say("control five").unwrap().as_slice(),
            [Action::Keystroke { key: Key::Character('5'), modifiers }]
                if *modifiers == Modifiers::CONTROL
        ));
        assert!(matches!(
            harness.say("control bravo").unwrap().as_slice(),
            [Action::Keystroke {
                key: Key::Character('b'),
                ..
            }]
        ));
    }

    #[test]
    fn handlers_queue_and_return_actions_in_order_and_fail_on_bad_ones() {
        let harness = Harness::new(
            r#"import { defineHexConfig, openApplication, press, typeText } from "@hex/commands"
export default defineHexConfig({
  commands: {
    both: {
      phrases: ["do both"],
      run: async ({ hex }) => {
        await hex.openApplication("Slack")
        return [press({ key: "k", modifiers: ["command"] }), typeText("hi")]
      },
    },
    nothing: { phrases: ["do nothing"], run: () => {} },
    broken: { phrases: ["do broken"], run: () => press({ key: "nope" }) },
    throws: { phrases: ["do throws"], run: () => { throw new Error("no thanks") } },
    odd: { phrases: ["do odd"], run: () => 42 },
  },
})
"#,
        );
        assert!(matches!(
            harness.say("do both").unwrap().as_slice(),
            [
                Action::OpenApplication(application),
                Action::Keystroke { key: Key::Character('k'), .. },
                Action::TypeText(text),
            ] if application == "Slack" && text == "hi"
        ));
        assert!(harness.say("do nothing").unwrap().is_empty());
        assert!(harness.say("do broken").unwrap_err().contains("press key"));
        let error = harness.say("do throws").unwrap_err();
        assert!(error.starts_with("Error: no thanks"), "{error}");
        assert!(error.contains("hex.config.ts:"), "{error}");
        assert!(
            harness
                .say("do odd")
                .unwrap_err()
                .contains("must return an action")
        );
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
  commands: {},
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
  commands: {},
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
            r#"export default { transformations: { echo: { name: "Echo", transform: (t) => t } }, commands: {} }"#,
        );
        let generation = harness.snapshot().generation;

        harness.write("const a = 1\nenum Color { Red }\nexport default { commands: {} }\n");
        assert!(
            harness
                .error()
                .contains("SyntaxError: hex.config.ts:2:1: TypeScript enums"),
            "{}",
            harness.error()
        );
        assert_eq!(harness.worker.status.host_state, HostState::Active);
        assert_eq!(harness.snapshot().generation, generation);
        assert_eq!(harness.transform(&["echo"], "kept").unwrap(), "kept");

        harness.write("export default {\n  commands: {\n    a: { phrases: [\"x\"], run: () => {} },,\n  },\n}\n");
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
            "export default {\n  commands: {\n    a: { phrases: [\"x\"] run: () => {} },\n  },\n}\n",
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

        harness.write("throw new Error(\"top level\")\nexport default { commands: {} }\n");
        assert!(
            harness.error().starts_with("Error: top level"),
            "{}",
            harness.error()
        );

        harness.write("export default { commands: { a: { phrases: [\"open notes\"], run: () => {} }, b: { phrases: [\"open notes\"], run: () => {} } } }\n");
        assert_eq!(harness.error(), "command patterns overlap: a and b");
        assert_eq!(harness.snapshot().generation, generation);

        harness.write("export default { transformations: { echo: { name: \"Echo\", transform: (t) => t + \"?\" } }, commands: {} }\n");
        assert!(harness.worker.status.last_reload_error.is_none());
        assert!(harness.snapshot().generation > generation);
        assert_eq!(harness.transform(&["echo"], "new").unwrap(), "new?");

        fs::remove_file(harness.worker.workspace.join("hex.config.ts")).unwrap();
        harness.worker.reload();
        assert_eq!(harness.worker.status.host_state, HostState::NotConfigured);
        assert!(harness.worker.script.is_none());
    }

    #[test]
    fn stale_generations_never_reach_a_newer_config() {
        let mut harness = Harness::new(
            r#"export default { transformations: { echo: { name: "Echo", transform: (t) => t } }, commands: {} }"#,
        );
        let (response, result) = mpsc::sync_channel(1);
        let stale = harness.snapshot().generation;
        harness.write(
            r#"export default { transformations: { echo: { name: "Echo", transform: (t) => t + t } }, commands: {} }"#,
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
        let mut harness = Harness::new("export default { commands: {} }");
        let workspace = harness.worker.workspace.clone();
        fs::create_dir_all(workspace.join("lib")).unwrap();
        fs::write(
            workspace.join("lib/words.ts"),
            "import data from \"../data.json\"\nexport const shout = (text: string): string => text.toUpperCase() + data.mark\n",
        )
        .unwrap();
        fs::write(workspace.join("data.json"), "{\"mark\": \"!\"}").unwrap();
        harness.write(
            "import { shout } from \"./lib/words\"\nexport default { transformations: { shout: { name: \"Shout\", transform: shout } }, commands: {} }\n",
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
                "import {{ x }} from {import:?}\nexport default {{ commands: {{}} }}\n"
            ));
            assert!(!harness.error().is_empty(), "{import} was importable");
        }
        harness.write(
            "export default { transformations: { probe: { name: \"Probe\", transform: () => [typeof fetch, typeof setTimeout, typeof require, typeof process, typeof console.log].join(\" \") } }, commands: {} }\n",
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
        fs::write(
            workspace.join("hex.config.js"),
            "export default { commands: {} }\n",
        )
        .unwrap();
        assert!(find_config(&workspace).is_err());
        fs::remove_dir_all(workspace).unwrap();
    }

    #[test]
    fn check_reports_what_the_config_registers() {
        let harness = Harness::new(TEMPLATE_CONFIG);
        assert_eq!(
            check_workspace_at(&harness.worker.workspace, &CommandConfig::new()).unwrap(),
            "hex.config.ts: 3 commands, 0 transformations"
        );
    }
}
