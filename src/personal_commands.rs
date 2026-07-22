use std::collections::{HashMap, HashSet, VecDeque};
use std::fs;
use std::io::{BufRead, BufReader, Read, Write};
use std::path::{Path, PathBuf};
use std::process::{Child, ChildStdin, Command, Stdio};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{self, Receiver, SyncSender, TrySendError};
use std::sync::{Arc, Mutex, RwLock};
use std::thread;
use std::time::{Duration, Instant, SystemTime};

use color_eyre::eyre::{Result, WrapErr, eyre};
use serde::{Deserialize, Serialize};

use crate::commands::{Action, ActionOutcome, CommandConfig, ConfiguredCommand, ContextPredicate};
use crate::context::ContextSnapshot;
use crate::keyboard::{Key, Modifiers};

const MAX_FRAME_BYTES: usize = 256 * 1024;
const MAX_INPUT_FRAME_BYTES: usize = 64 * 1024;
const HOST_START_TIMEOUT: Duration = Duration::from_secs(5);
const POLL_INTERVAL: Duration = Duration::from_millis(250);
const RELOAD_DEBOUNCE: Duration = Duration::from_millis(350);
const MAX_COMMANDS: usize = 512;
const MAX_PHRASES: usize = 16;
const MAX_REPEAT: u8 = 100;
const MAX_PENDING_INVOCATIONS: usize = 1024;
const MAX_PENDING_TOOL_CALLS: usize = 1024;
const MAX_TOOL_CALLS_PER_INVOCATION: usize = 64;
const HOST_WRITE_QUEUE: usize = 64;
const TOOL_QUEUE: usize = 64;
const TOOL_WORKERS: usize = 2;
const RETIRE_TIMEOUT: Duration = Duration::from_secs(30);
const RESTART_BASE_DELAY: Duration = Duration::from_millis(250);
const RESTART_MAX_DELAY: Duration = Duration::from_secs(8);
const RESTART_HEALTHY_PERIOD: Duration = Duration::from_secs(30);
const MAX_AUTOMATIC_RESTARTS: u8 = 5;
const MAX_WATCH_ENTRIES: usize = 4096;
const MAX_WATCH_DEPTH: usize = 32;
const MAX_STATUS_BYTES: usize = 256 * 1024;
const MAX_STATUS_ERROR_BYTES: usize = 4096;

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct StatusSnapshot {
    pub active_generation: Option<u64>,
    pub catalog: Vec<StatusCommand>,
    pub host_state: HostState,
    pub last_reload_error: Option<String>,
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

fn persist_status(status: &StatusSnapshot) {
    let result = crate::app_paths::personal_commands_status_file()
        .and_then(|path| persist_status_at(&path, status));
    if let Err(error) = result {
        tracing::warn!(%error, "could not persist personal command status");
    }
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
    Ok(())
}

pub fn initialize_workspace() -> Result<PathBuf> {
    let workspace = crate::app_paths::personal_commands_workspace()?;
    let sdk = crate::app_paths::personal_commands_sdk()?;
    initialize_workspace_at(&workspace, &sdk)?;
    let bun = find_bun().ok_or_else(|| eyre!("Bun is required to install personal commands"))?;
    let status = Command::new(bun)
        .arg("install")
        .current_dir(&workspace)
        .status()
        .wrap_err("could not install the personal command workspace")?;
    if !status.success() {
        return Err(eyre!(
            "Bun failed to install the personal command workspace"
        ));
    }
    Ok(workspace)
}

pub fn initialize_workspace_at(workspace: &Path, sdk: &Path) -> Result<()> {
    let template = sdk.join("workspace-template");
    if !sdk.join("package.json").is_file()
        || !sdk.join("dist/bin.js").is_file()
        || !template.is_dir()
    {
        return Err(eyre!("personal command SDK resources are incomplete"));
    }
    fs::create_dir_all(workspace)?;
    let staged_sdk = workspace.join(".hex-sdk.tmp");
    let installed_sdk = workspace.join(".hex-sdk");
    if staged_sdk.exists() {
        fs::remove_dir_all(&staged_sdk)?;
    }
    fs::create_dir(&staged_sdk)?;
    fs::copy(sdk.join("package.json"), staged_sdk.join("package.json"))?;
    copy_directory(&sdk.join("dist"), &staged_sdk.join("dist"), true)?;
    if installed_sdk.exists() {
        fs::remove_dir_all(&installed_sdk)?;
    }
    fs::rename(staged_sdk, installed_sdk)?;
    copy_directory(&template, workspace, false)?;
    Ok(())
}

fn copy_directory(source: &Path, destination: &Path, overwrite: bool) -> Result<()> {
    fs::create_dir_all(destination)?;
    for entry in fs::read_dir(source)? {
        let entry = entry?;
        let file_type = entry.file_type()?;
        if file_type.is_symlink() {
            return Err(eyre!("SDK resources must not contain symlinks"));
        }
        let target = destination.join(entry.file_name());
        if file_type.is_dir() {
            copy_directory(&entry.path(), &target, overwrite)?;
        } else if file_type.is_file() && (overwrite || !target.exists()) {
            fs::copy(entry.path(), target)?;
        }
    }
    Ok(())
}

pub struct PersonalCommands {
    active: Arc<RwLock<Arc<CommandConfig>>>,
    requests: SyncSender<Request>,
    outcomes: Receiver<ActionOutcome>,
    stop: Arc<AtomicBool>,
    worker: Option<thread::JoinHandle<()>>,
    worker_done: Receiver<()>,
}

struct Invocation {
    generation: u64,
    command_id: String,
    heard: String,
    context: ContextSnapshot,
}

enum Request {
    Invoke(Box<Invocation>),
    Reload,
}

enum HostEvent {
    Frame(u64, HostOutput),
    Closed(u64, String),
    ToolResult(u64, ToolResult),
}

struct ToolResult {
    invocation_id: String,
    tool_call_id: String,
    result: WireResult,
}

struct HostProcess {
    generation: u64,
    child: Child,
    input: SyncSender<HostInput>,
    retire_at: Option<Instant>,
}

struct PendingInvocation {
    generation: u64,
    command_id: String,
    heard: String,
    context: String,
    tool_call_count: usize,
    started: Instant,
}

#[derive(Default)]
struct RestartBackoff {
    failures: u8,
    retry_at: Option<Instant>,
    active_since: Option<Instant>,
}

struct ToolJob {
    generation: u64,
    invocation_id: String,
    tool_call_id: String,
    action: NativeAction,
}

#[derive(Deserialize)]
#[serde(
    tag = "type",
    rename_all = "camelCase",
    rename_all_fields = "camelCase"
)]
enum HostOutput {
    Registration {
        protocol_version: u8,
        commands: Vec<RegistrationCommand>,
    },
    ToolCall {
        invocation_id: String,
        tool_call_id: String,
        action: NativeAction,
    },
    InvocationResult {
        invocation_id: String,
        result: WireResult,
    },
}

#[derive(Deserialize)]
struct RegistrationCommand {
    id: String,
    phrases: Vec<String>,
    group: Option<String>,
    description: Option<String>,
    when: Option<When>,
    execution: Execution,
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

#[derive(Clone, Deserialize, Serialize)]
#[serde(
    tag = "type",
    rename_all = "camelCase",
    rename_all_fields = "camelCase"
)]
enum WireResult {
    Success,
    Failure { message: String },
}

#[derive(Serialize)]
#[serde(
    tag = "type",
    rename_all = "camelCase",
    rename_all_fields = "camelCase"
)]
enum HostInput {
    Invoke {
        invocation_id: String,
        command_id: String,
        context: InvocationContext,
    },
    ToolResult {
        invocation_id: String,
        tool_call_id: String,
        result: WireResult,
    },
    Shutdown,
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

const fn one() -> u8 {
    1
}

impl RestartBackoff {
    fn reset(&mut self) {
        *self = Self::default();
    }

    fn host_started(&mut self, now: Instant) {
        self.retry_at = None;
        self.active_since = Some(now);
    }

    fn host_failed(&mut self, now: Instant) {
        self.active_since = None;
        self.failures = self.failures.saturating_add(1);
        self.retry_at = (self.failures <= MAX_AUTOMATIC_RESTARTS).then(|| {
            let exponent = u32::from(self.failures.saturating_sub(1));
            let delay = RESTART_BASE_DELAY
                .saturating_mul(1_u32 << exponent)
                .min(RESTART_MAX_DELAY);
            now + delay
        });
    }

    fn ready(&self, now: Instant) -> bool {
        self.retry_at.is_some_and(|retry_at| now >= retry_at)
    }

    fn reset_if_healthy(&mut self, now: Instant) {
        if self
            .active_since
            .is_some_and(|started| now.duration_since(started) >= RESTART_HEALTHY_PERIOD)
        {
            self.reset();
        }
    }
}

impl PersonalCommands {
    pub fn start(base: CommandConfig) -> Option<Self> {
        let active = Arc::new(RwLock::new(Arc::new(base.clone())));
        let (request_sender, requests) = mpsc::sync_channel(32);
        let (outcome_sender, outcomes) = mpsc::sync_channel(64);
        let stop = Arc::new(AtomicBool::new(false));
        let worker_active = active.clone();
        let worker_stop = stop.clone();
        let (done_sender, worker_done) = mpsc::sync_channel(1);
        let worker = thread::Builder::new()
            .name("personal-command-host".into())
            .spawn(move || {
                run_worker(base, worker_active, requests, outcome_sender, worker_stop);
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
        })
    }

    pub fn snapshot(&self) -> Arc<CommandConfig> {
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
    ) -> Result<(), &'static str> {
        self.requests
            .try_send(Request::Invoke(Box::new(Invocation {
                generation,
                command_id: command_id.into(),
                heard: heard.into(),
                context,
            })))
            .map_err(|error| match error {
                TrySendError::Full(_) => "personal command host queue is full",
                TrySendError::Disconnected(_) => "personal command host is unavailable",
            })
    }

    pub fn try_recv(&self) -> Option<ActionOutcome> {
        self.outcomes.try_recv().ok()
    }
}

impl Drop for PersonalCommands {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Release);
        let _ = self.requests.try_send(Request::Reload);
        if self
            .worker_done
            .recv_timeout(Duration::from_secs(2))
            .is_ok()
            && let Some(worker) = self.worker.take()
            && worker.join().is_err()
        {
            tracing::warn!("personal command host worker panicked during shutdown");
        }
    }
}

fn run_worker(
    base: CommandConfig,
    active: Arc<RwLock<Arc<CommandConfig>>>,
    requests: Receiver<Request>,
    outcomes: SyncSender<ActionOutcome>,
    stop: Arc<AtomicBool>,
) {
    let mut status = StatusSnapshot {
        host_state: HostState::Starting,
        ..StatusSnapshot::default()
    };
    persist_status(&status);
    let workspace = match crate::app_paths::personal_commands_workspace() {
        Ok(workspace) => workspace,
        Err(error) => {
            status.host_state = HostState::Unavailable;
            status.last_reload_error = Some(bounded_status_error(&error.to_string()));
            persist_status(&status);
            tracing::warn!(%error, "personal commands are unavailable");
            return;
        }
    };
    let config = workspace.join("hex.config.ts");
    if !config.is_file() {
        status.host_state = HostState::NotConfigured;
        persist_status(&status);
        return;
    }
    let Some(bun) = find_bun() else {
        status.host_state = HostState::Unavailable;
        status.last_reload_error = Some("Bun was not found".into());
        persist_status(&status);
        tracing::warn!("Bun was not found; personal commands are unavailable");
        return;
    };
    let host_entrypoint = match crate::app_paths::personal_commands_host() {
        Ok(path) => path,
        Err(error) => {
            status.host_state = HostState::Unavailable;
            status.last_reload_error = Some(bounded_status_error(&error.to_string()));
            persist_status(&status);
            tracing::warn!(%error, "personal commands are unavailable");
            return;
        }
    };
    let (event_sender, events) = mpsc::sync_channel(64);
    let (tool_sender, tool_jobs) = mpsc::sync_channel(TOOL_QUEUE);
    spawn_tool_workers(tool_jobs, event_sender.clone());
    let (reload_sender, reload_requests) = mpsc::sync_channel(1);
    let watcher_stop = stop.clone();
    let watcher_workspace = workspace.clone();
    let _ = thread::Builder::new()
        .name("personal-command-watch".into())
        .spawn(move || watch_workspace(watcher_workspace, reload_sender, watcher_stop));
    let mut generation = 0;
    let mut hosts = HashMap::new();
    let mut restart_backoff = RestartBackoff::default();
    if let Some(host) = reload(
        &base,
        &active,
        &event_sender,
        &workspace,
        &config,
        &bun,
        &host_entrypoint,
        &mut generation,
        &mut status,
    ) {
        restart_backoff.host_started(Instant::now());
        hosts.insert(host.generation, host);
    }
    let mut next_invocation = 0_u64;
    let mut pending = HashMap::<String, PendingInvocation>::new();
    let mut pending_tools = HashSet::<(u64, String, String)>::new();

    while !stop.load(Ordering::Acquire) {
        restart_backoff.reset_if_healthy(Instant::now());
        let mut active_host_failed = false;
        while let Ok(event) = events.try_recv() {
            active_host_failed |= handle_host_event(
                event,
                &mut hosts,
                &mut pending,
                &mut pending_tools,
                &outcomes,
                &tool_sender,
            );
        }
        if active_host_failed {
            restart_backoff.host_failed(Instant::now());
        }
        let config_reload = reload_requests.try_recv().is_ok();
        let request = requests.recv_timeout(Duration::from_millis(50));
        let config_reload = config_reload || matches!(request, Ok(Request::Reload));
        if config_reload {
            restart_backoff.reset();
        }
        let automatic_restart = !config_reload && restart_backoff.ready(Instant::now());
        if config_reload || automatic_restart {
            if let Some(candidate) = reload(
                &base,
                &active,
                &event_sender,
                &workspace,
                &config,
                &bun,
                &host_entrypoint,
                &mut generation,
                &mut status,
            ) {
                let now = Instant::now();
                for host in hosts.values_mut() {
                    if host.retire_at.is_none() {
                        host.retire_at = Some(now + RETIRE_TIMEOUT);
                    }
                }
                restart_backoff.host_started(now);
                hosts.insert(candidate.generation, candidate);
            } else if automatic_restart {
                restart_backoff.host_failed(Instant::now());
            }
        }
        match request {
            Ok(Request::Invoke(invocation)) => {
                let invocation = *invocation;
                if pending.len() >= MAX_PENDING_INVOCATIONS {
                    emit_failure(
                        invocation,
                        &outcomes,
                        "too many personal commands are still running",
                    );
                    continue;
                }
                let Some(host) = hosts.get(&invocation.generation) else {
                    emit_failure(
                        invocation,
                        &outcomes,
                        "personal command generation is no longer available",
                    );
                    continue;
                };
                next_invocation += 1;
                let invocation_id = format!("{}-{next_invocation}", invocation.generation);
                let context = InvocationContext {
                    application: invocation.context.application.clone(),
                    browser_host: invocation.context.browser_host().map(Into::into),
                    browser_url: invocation
                        .context
                        .browser_url
                        .as_ref()
                        .map(ToString::to_string),
                    window_title: invocation.context.window_title.clone(),
                };
                let input = HostInput::Invoke {
                    invocation_id: invocation_id.clone(),
                    command_id: invocation.command_id.clone(),
                    context,
                };
                let host_generation = host.generation;
                if let Err(error) = host.input.try_send(input) {
                    let reason = writer_queue_error(error);
                    emit_failure(invocation, &outcomes, reason);
                    let restart = generation_is_active(&hosts, host_generation);
                    fail_generation(
                        host_generation,
                        &mut hosts,
                        &mut pending,
                        &mut pending_tools,
                        &outcomes,
                        reason,
                    );
                    if restart {
                        restart_backoff.host_failed(Instant::now());
                    }
                } else {
                    tracing::info!(
                        generation = invocation.generation,
                        invocation_id,
                        command_id = invocation.command_id,
                        execution_kind = "handler",
                        status = "dispatched",
                        "personal command invocation"
                    );
                    pending.insert(
                        invocation_id,
                        PendingInvocation {
                            generation: invocation.generation,
                            command_id: invocation.command_id,
                            heard: invocation.heard,
                            context: invocation.context.label(),
                            tool_call_count: 0,
                            started: Instant::now(),
                        },
                    );
                }
            }
            Err(mpsc::RecvTimeoutError::Timeout) => {}
            Err(mpsc::RecvTimeoutError::Disconnected) => break,
            Ok(Request::Reload) => {}
        }

        let now = Instant::now();
        let retired = hosts
            .iter()
            .filter_map(|(generation, host)| {
                let has_pending = pending
                    .values()
                    .any(|invocation| invocation.generation == *generation);
                should_retire(host.retire_at, has_pending, now).then_some(*generation)
            })
            .collect::<Vec<_>>();
        for generation in retired {
            fail_generation(
                generation,
                &mut hosts,
                &mut pending,
                &mut pending_tools,
                &outcomes,
                "personal command generation retired",
            );
        }
    }
    for host in hosts.values() {
        let _ = host.input.try_send(HostInput::Shutdown);
    }
    status.host_state = HostState::Stopped;
    persist_status(&status);
}

#[allow(clippy::too_many_arguments)]
fn reload(
    base: &CommandConfig,
    active: &RwLock<Arc<CommandConfig>>,
    event_sender: &SyncSender<HostEvent>,
    workspace: &Path,
    config: &Path,
    bun: &Path,
    host_entrypoint: &Path,
    generation: &mut u64,
    status: &mut StatusSnapshot,
) -> Option<HostProcess> {
    *generation += 1;
    match start_candidate(
        *generation,
        base,
        event_sender,
        workspace,
        config,
        bun,
        host_entrypoint,
    ) {
        Ok((host, commands, catalog)) => {
            *active.write().expect("command registry poisoned") = Arc::new(commands);
            status.active_generation = Some(*generation);
            status.catalog = catalog;
            status.host_state = HostState::Active;
            status.last_reload_error = None;
            persist_status(status);
            tracing::info!(generation, "personal commands activated");
            Some(host)
        }
        Err(error) => {
            status.host_state = if status.active_generation.is_some() {
                HostState::Active
            } else {
                HostState::Unavailable
            };
            status.last_reload_error = Some(bounded_status_error(&error.to_string()));
            persist_status(status);
            tracing::error!(%error, "personal command reload rejected; preserving active registry");
            None
        }
    }
}

#[allow(clippy::too_many_arguments)]
fn start_candidate(
    generation: u64,
    base: &CommandConfig,
    event_sender: &SyncSender<HostEvent>,
    workspace: &Path,
    config: &Path,
    bun: &Path,
    host_entrypoint: &Path,
) -> Result<(HostProcess, CommandConfig, Vec<StatusCommand>)> {
    let mut child = Command::new(bun)
        .arg(host_entrypoint)
        .arg(config)
        .current_dir(workspace)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::inherit())
        .spawn()
        .wrap_err("could not start personal command host")?;
    let Some(input) = child.stdin.take() else {
        kill_and_wait(&mut child);
        return Err(eyre!("host stdin unavailable"));
    };
    let Some(output) = child.stdout.take() else {
        kill_and_wait(&mut child);
        return Err(eyre!("host stdout unavailable"));
    };
    let (candidate_sender, candidate_events) = mpsc::sync_channel(64);
    spawn_reader(generation, output, candidate_sender);
    let deadline = Instant::now() + HOST_START_TIMEOUT;
    loop {
        let remaining = deadline.saturating_duration_since(Instant::now());
        if remaining.is_zero() {
            kill_and_wait(&mut child);
            return Err(eyre!("personal command host registration timed out"));
        }
        match candidate_events.recv_timeout(remaining) {
            Ok(HostEvent::Frame(
                event_generation,
                HostOutput::Registration {
                    protocol_version,
                    commands,
                },
            )) if event_generation == generation => {
                let catalog = status_catalog(&commands);
                let compiled =
                    match compile_registration(base, generation, protocol_version, commands) {
                        Ok(compiled) => compiled,
                        Err(error) => {
                            kill_and_wait(&mut child);
                            return Err(error);
                        }
                    };
                let active_sender = event_sender.clone();
                thread::spawn(move || {
                    while let Ok(event) = candidate_events.recv() {
                        if active_sender.send(event).is_err() {
                            break;
                        }
                    }
                });
                let input = spawn_writer(generation, input, event_sender.clone());
                return Ok((
                    HostProcess {
                        generation,
                        child,
                        input,
                        retire_at: None,
                    },
                    compiled,
                    catalog,
                ));
            }
            Ok(HostEvent::Closed(event_generation, error)) if event_generation == generation => {
                kill_and_wait(&mut child);
                return Err(eyre!(
                    "personal command host stopped before registration: {error}"
                ));
            }
            Ok(_) => {}
            Err(_) => {
                kill_and_wait(&mut child);
                return Err(eyre!("personal command host registration timed out"));
            }
        }
    }
}

fn status_catalog(commands: &[RegistrationCommand]) -> Vec<StatusCommand> {
    commands
        .iter()
        .map(|command| StatusCommand {
            id: command.id.clone(),
            phrases: command.phrases.clone(),
            group: command.group.clone().unwrap_or_else(|| "Personal".into()),
            description: command
                .description
                .clone()
                .unwrap_or_else(|| "Personal command".into()),
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

fn handle_host_event(
    event: HostEvent,
    hosts: &mut HashMap<u64, HostProcess>,
    pending: &mut HashMap<String, PendingInvocation>,
    pending_tools: &mut HashSet<(u64, String, String)>,
    outcomes: &SyncSender<ActionOutcome>,
    tool_sender: &SyncSender<ToolJob>,
) -> bool {
    match event {
        HostEvent::Frame(
            generation,
            HostOutput::ToolCall {
                invocation_id,
                tool_call_id,
                action,
            },
        ) => {
            let valid = hosts.contains_key(&generation)
                && accept_tool_call(
                    pending,
                    pending_tools,
                    generation,
                    &invocation_id,
                    &tool_call_id,
                );
            if !valid {
                let restart = generation_is_active(hosts, generation);
                fail_generation(
                    generation,
                    hosts,
                    pending,
                    pending_tools,
                    outcomes,
                    "personal command host sent an invalid or excessive tool call",
                );
                return restart;
            }
            let correlation = (generation, invocation_id.clone(), tool_call_id.clone());
            if tool_sender
                .try_send(ToolJob {
                    generation,
                    invocation_id,
                    tool_call_id,
                    action,
                })
                .is_err()
            {
                pending_tools.remove(&correlation);
                let restart = generation_is_active(hosts, generation);
                fail_generation(
                    generation,
                    hosts,
                    pending,
                    pending_tools,
                    outcomes,
                    "personal command native tool queue is unavailable",
                );
                return restart;
            }
            false
        }
        HostEvent::Frame(
            generation,
            HostOutput::InvocationResult {
                invocation_id,
                result,
            },
        ) => {
            let valid = pending
                .get(&invocation_id)
                .is_some_and(|invocation| invocation.generation == generation)
                && !pending_tools
                    .iter()
                    .any(|(tool_generation, tool_invocation, _)| {
                        *tool_generation == generation && tool_invocation == &invocation_id
                    });
            if valid {
                let invocation = pending.remove(&invocation_id).expect("invocation checked");
                let result = match result {
                    WireResult::Success => Ok(()),
                    WireResult::Failure { message } => Err(bounded_error(&message)),
                };
                tracing::info!(
                    generation,
                    invocation_id,
                    command_id = invocation.command_id,
                    execution_kind = "handler",
                    status = if result.is_ok() {
                        "completed"
                    } else {
                        "failed"
                    },
                    duration_ms = invocation.started.elapsed().as_millis(),
                    "personal command invocation"
                );
                let _ = outcomes.send(ActionOutcome {
                    id: invocation.command_id,
                    heard: invocation.heard,
                    context: invocation.context,
                    result,
                });
            } else {
                let restart = generation_is_active(hosts, generation);
                fail_generation(
                    generation,
                    hosts,
                    pending,
                    pending_tools,
                    outcomes,
                    "personal command host sent an invalid invocation result",
                );
                return restart;
            }
            false
        }
        HostEvent::ToolResult(generation, result) => {
            let correlation = (
                generation,
                result.invocation_id.clone(),
                result.tool_call_id.clone(),
            );
            if pending_tools.remove(&correlation)
                && let Some(host) = hosts.get(&generation)
            {
                let frame = HostInput::ToolResult {
                    invocation_id: result.invocation_id,
                    tool_call_id: result.tool_call_id,
                    result: result.result,
                };
                if host.input.try_send(frame).is_err() {
                    let restart = generation_is_active(hosts, generation);
                    fail_generation(
                        generation,
                        hosts,
                        pending,
                        pending_tools,
                        outcomes,
                        "personal command host writer is unavailable",
                    );
                    return restart;
                }
            }
            false
        }
        HostEvent::Closed(generation, error) if hosts.contains_key(&generation) => {
            tracing::error!(generation, %error, "personal command host stopped");
            let restart = generation_is_active(hosts, generation);
            fail_generation(
                generation,
                hosts,
                pending,
                pending_tools,
                outcomes,
                "personal command host stopped",
            );
            restart
        }
        HostEvent::Frame(generation, HostOutput::Registration { .. }) => {
            let restart = generation_is_active(hosts, generation);
            fail_generation(
                generation,
                hosts,
                pending,
                pending_tools,
                outcomes,
                "personal command host sent an unexpected registration",
            );
            restart
        }
        _ => false,
    }
}

fn generation_is_active(hosts: &HashMap<u64, HostProcess>, generation: u64) -> bool {
    hosts
        .get(&generation)
        .is_some_and(|host| host.retire_at.is_none())
}

fn should_retire(retire_at: Option<Instant>, has_pending: bool, now: Instant) -> bool {
    retire_at.is_some_and(|deadline| !has_pending || now >= deadline)
}

fn accept_tool_call(
    pending: &mut HashMap<String, PendingInvocation>,
    pending_tools: &mut HashSet<(u64, String, String)>,
    generation: u64,
    invocation_id: &str,
    tool_call_id: &str,
) -> bool {
    if pending_tools.len() >= MAX_PENDING_TOOL_CALLS {
        return false;
    }
    let Some(invocation) = pending.get_mut(invocation_id) else {
        return false;
    };
    if invocation.generation != generation
        || invocation.tool_call_count >= MAX_TOOL_CALLS_PER_INVOCATION
    {
        return false;
    }
    if !pending_tools.insert((generation, invocation_id.into(), tool_call_id.into())) {
        return false;
    }
    invocation.tool_call_count += 1;
    true
}

fn compile_registration(
    base: &CommandConfig,
    generation: u64,
    protocol_version: u8,
    commands: Vec<RegistrationCommand>,
) -> Result<CommandConfig> {
    if protocol_version != 1 {
        return Err(eyre!(
            "unsupported personal command protocol version {protocol_version}"
        ));
    }
    if commands.len() > MAX_COMMANDS {
        return Err(eyre!("personal registry exceeds {MAX_COMMANDS} commands"));
    }
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
        let action = match command.execution {
            Execution::Native { action } => action.into_action()?,
            Execution::Handler => Action::InvokeHandler { generation },
        };
        let description = command
            .description
            .unwrap_or_else(|| "Personal command".into());
        validate_text(&description, "command description", 1024)?;
        if let Some(group) = &command.group {
            validate_text(group, "command group", 1024)?;
        }
        let configured = ConfiguredCommand::personal_literal(
            command.id,
            description,
            command.phrases,
            context,
            action,
            Some(command.group.unwrap_or_else(|| "Personal".into())),
        )?;
        compiled.try_command(configured)?;
    }
    Ok(compiled)
}

impl NativeAction {
    fn into_action(self) -> Result<Action> {
        Ok(match self {
            Self::OpenUrl { url } => {
                validate_text(&url, "URL", 4096)?;
                let parsed = url::Url::parse(&url).wrap_err("personal command URL is invalid")?;
                if !matches!(parsed.scheme(), "http" | "https") {
                    return Err(eyre!("personal command URL must use http or https"));
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

fn spawn_writer(
    generation: u64,
    mut input: ChildStdin,
    events: SyncSender<HostEvent>,
) -> SyncSender<HostInput> {
    let (sender, frames) = mpsc::sync_channel(HOST_WRITE_QUEUE);
    thread::spawn(move || {
        while let Ok(frame) = frames.recv() {
            let shutdown = matches!(frame, HostInput::Shutdown);
            if let Err(error) = write_frame(&mut input, &frame) {
                let _ = events.send(HostEvent::Closed(
                    generation,
                    format!("host input failed: {error}"),
                ));
                break;
            }
            if shutdown {
                break;
            }
        }
    });
    sender
}

fn spawn_tool_workers(jobs: Receiver<ToolJob>, events: SyncSender<HostEvent>) {
    let jobs = Arc::new(Mutex::new(jobs));
    for _ in 0..TOOL_WORKERS {
        let jobs = jobs.clone();
        let events = events.clone();
        thread::spawn(move || {
            loop {
                let job = jobs.lock().expect("tool queue poisoned").recv();
                let Ok(job) = job else {
                    break;
                };
                let result = job
                    .action
                    .into_action()
                    .and_then(crate::commands::execute)
                    .map(|()| WireResult::Success)
                    .unwrap_or_else(|error| WireResult::Failure {
                        message: bounded_error(&error.to_string()),
                    });
                if events
                    .send(HostEvent::ToolResult(
                        job.generation,
                        ToolResult {
                            invocation_id: job.invocation_id,
                            tool_call_id: job.tool_call_id,
                            result,
                        },
                    ))
                    .is_err()
                {
                    break;
                }
            }
        });
    }
}

fn fail_generation(
    generation: u64,
    hosts: &mut HashMap<u64, HostProcess>,
    pending: &mut HashMap<String, PendingInvocation>,
    pending_tools: &mut HashSet<(u64, String, String)>,
    outcomes: &SyncSender<ActionOutcome>,
    error: &str,
) {
    hosts.remove(&generation);
    pending_tools.retain(|(tool_generation, _, _)| *tool_generation != generation);
    let invocation_ids = pending
        .iter()
        .filter_map(|(id, invocation)| (invocation.generation == generation).then_some(id.clone()))
        .collect::<Vec<_>>();
    for id in invocation_ids {
        if let Some(invocation) = pending.remove(&id) {
            tracing::warn!(
                generation,
                invocation_id = id,
                command_id = invocation.command_id,
                execution_kind = "handler",
                status = "failed",
                duration_ms = invocation.started.elapsed().as_millis(),
                error,
                "personal command invocation"
            );
            let _ = outcomes.send(ActionOutcome {
                id: invocation.command_id,
                heard: invocation.heard,
                context: invocation.context,
                result: Err(error.into()),
            });
        }
    }
}

fn writer_queue_error(error: TrySendError<HostInput>) -> &'static str {
    match error {
        TrySendError::Full(_) => "personal command host writer queue is full",
        TrySendError::Disconnected(_) => "personal command host writer is unavailable",
    }
}

fn kill_and_wait(child: &mut Child) {
    let _ = child.kill();
    let _ = child.wait();
}

fn spawn_reader(
    generation: u64,
    output: impl Read + Send + 'static,
    sender: SyncSender<HostEvent>,
) {
    thread::spawn(move || {
        let mut reader = BufReader::new(output);
        loop {
            match read_frame(&mut reader) {
                Ok(Some(bytes)) => match serde_json::from_slice(&bytes) {
                    Ok(frame) => {
                        if sender.send(HostEvent::Frame(generation, frame)).is_err() {
                            break;
                        }
                    }
                    Err(error) => {
                        let _ = sender.send(HostEvent::Closed(
                            generation,
                            format!("invalid host frame: {error}"),
                        ));
                        break;
                    }
                },
                Ok(None) => {
                    let _ = sender.send(HostEvent::Closed(generation, "end of output".into()));
                    break;
                }
                Err(error) => {
                    let _ = sender.send(HostEvent::Closed(generation, error.to_string()));
                    break;
                }
            }
        }
    });
}

fn read_frame(reader: &mut impl BufRead) -> Result<Option<Vec<u8>>> {
    let mut frame = Vec::new();
    loop {
        let available = reader.fill_buf()?;
        if available.is_empty() {
            return if frame.is_empty() {
                Ok(None)
            } else {
                Ok(Some(frame))
            };
        }
        let newline = available.iter().position(|byte| *byte == b'\n');
        let consumed = match newline {
            Some(newline) => {
                frame.extend_from_slice(&available[..newline]);
                newline + 1
            }
            None => {
                frame.extend_from_slice(available);
                available.len()
            }
        };
        reader.consume(consumed);
        if frame.len() > MAX_FRAME_BYTES {
            return Err(eyre!(
                "personal command host frame exceeds {MAX_FRAME_BYTES} bytes"
            ));
        }
        if newline.is_some() {
            return Ok(Some(frame));
        }
    }
}

fn write_frame(output: &mut impl Write, frame: &HostInput) -> Result<()> {
    let encoded = serde_json::to_vec(frame)?;
    if encoded.len() > MAX_INPUT_FRAME_BYTES {
        return Err(eyre!(
            "personal command input frame exceeds {MAX_INPUT_FRAME_BYTES} bytes"
        ));
    }
    output.write_all(&encoded)?;
    output.write_all(b"\n")?;
    output.flush()?;
    Ok(())
}

fn watch_workspace(workspace: PathBuf, requests: SyncSender<Request>, stop: Arc<AtomicBool>) {
    let mut fingerprint = workspace_fingerprint(&workspace);
    let mut changed_at = None;
    while !stop.load(Ordering::Acquire) {
        thread::sleep(POLL_INTERVAL);
        let next = workspace_fingerprint(&workspace);
        if next != fingerprint {
            fingerprint = next;
            changed_at = Some(Instant::now());
        }
        if changed_at.is_some_and(|changed| changed.elapsed() >= RELOAD_DEBOUNCE) {
            match requests.try_send(Request::Reload) {
                Ok(()) => changed_at = None,
                Err(TrySendError::Full(_)) => {}
                Err(TrySendError::Disconnected(_)) => break,
            }
        }
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

fn find_bun() -> Option<PathBuf> {
    let candidates = std::env::var_os("PATH")
        .into_iter()
        .flat_map(|path| std::env::split_paths(&path).collect::<Vec<_>>())
        .map(|path| path.join("bun"))
        .chain([
            dirs::home_dir().unwrap_or_default().join(".bun/bin/bun"),
            PathBuf::from("/opt/homebrew/bin/bun"),
            PathBuf::from("/usr/local/bin/bun"),
        ]);
    find_bun_candidate(candidates)
}

fn find_bun_candidate(candidates: impl IntoIterator<Item = PathBuf>) -> Option<PathBuf> {
    candidates.into_iter().find(|path| path.is_file())
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
    if parsed
        .to_string()
        .trim_end_matches('.')
        .eq_ignore_ascii_case(host.trim_end_matches('.'))
    {
        Ok(())
    } else {
        Err(eyre!("browser host context must be an exact host"))
    }
}

fn bounded_error(message: &str) -> String {
    message.chars().take(4096).collect()
}

fn bounded_status_error(message: &str) -> String {
    let mut end = 0;
    for (index, character) in message.char_indices() {
        let next = index + character.len_utf8();
        if next > MAX_STATUS_ERROR_BYTES {
            break;
        }
        end = next;
    }
    message[..end].to_owned()
}

fn emit_failure(invocation: Invocation, outcomes: &SyncSender<ActionOutcome>, error: &str) {
    let _ = outcomes.send(ActionOutcome {
        id: invocation.command_id,
        heard: invocation.heard,
        context: invocation.context.label(),
        result: Err(bounded_error(error)),
    });
}

impl Drop for HostProcess {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::commands::{Decision, Mode};
    use std::time::UNIX_EPOCH;

    #[test]
    fn registration_compiles_native_and_handler_commands() {
        let registration: HostOutput = serde_json::from_str(
            r#"{"type":"registration","protocolVersion":1,"commands":[{"id":"native","phrases":["open example"],"execution":{"type":"native","action":{"type":"openUrl","url":"https://example.com"}}},{"id":"handled","phrases":["do work"],"group":"Work","when":{"application":"Slack"},"execution":{"type":"handler"}}]}"#,
        )
        .unwrap();
        let HostOutput::Registration {
            protocol_version,
            commands,
        } = registration
        else {
            panic!("expected registration")
        };
        let compiled =
            compile_registration(&CommandConfig::new(), 7, protocol_version, commands).unwrap();

        assert!(matches!(
            compiled.resolve(Mode::Listening, "open example", &ContextSnapshot::default()),
            Decision::Execute { action: Action::OpenUrl(url), .. } if url == "https://example.com"
        ));
        let slack = ContextSnapshot {
            application: Some("Slack".into()),
            ..ContextSnapshot::default()
        };
        assert!(matches!(
            compiled.resolve(Mode::Listening, "do work", &slack),
            Decision::Execute {
                id: "handled",
                action: Action::InvokeHandler { generation: 7 },
            }
        ));
        assert_eq!(
            compiled
                .catalog()
                .into_iter()
                .find(|command| command.id == "handled")
                .and_then(|command| command.group),
            Some("Work".into())
        );
        assert_eq!(
            compiled
                .catalog()
                .into_iter()
                .find(|command| command.id == "native")
                .and_then(|command| command.group),
            Some("Personal".into())
        );
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

        assert!(compile_registration(&base, 1, 1, vec![command]).is_err());
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
            assert!(compile_registration(&base, 1, 1, vec![command]).is_err());
        }
    }

    #[test]
    fn browser_hosts_are_exact_and_canonical() {
        let command: RegistrationCommand = serde_json::from_str(
            r#"{"id":"bad","phrases":["bad"],"when":{"browserHost":"https://x.com/path"},"execution":{"type":"handler"}}"#,
        )
        .unwrap();
        assert!(compile_registration(&CommandConfig::new(), 1, 1, vec![command]).is_err());

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

    #[test]
    fn tool_calls_are_unique_while_pending_and_cumulatively_bounded() {
        let mut pending = HashMap::from([(
            "1-1".into(),
            PendingInvocation {
                generation: 1,
                command_id: "handled".into(),
                heard: "do work".into(),
                context: "test".into(),
                tool_call_count: 0,
                started: Instant::now(),
            },
        )]);
        let mut tools = HashSet::new();

        assert!(accept_tool_call(
            &mut pending,
            &mut tools,
            1,
            "1-1",
            "tool-1"
        ));
        assert!(!accept_tool_call(
            &mut pending,
            &mut tools,
            1,
            "1-1",
            "tool-1"
        ));
        tools.clear();
        for index in 1..MAX_TOOL_CALLS_PER_INVOCATION {
            assert!(accept_tool_call(
                &mut pending,
                &mut tools,
                1,
                "1-1",
                &format!("tool-{index}")
            ));
            tools.clear();
        }
        assert!(!accept_tool_call(
            &mut pending,
            &mut tools,
            1,
            "1-1",
            "one-too-many"
        ));
        assert!(!accept_tool_call(
            &mut pending,
            &mut tools,
            2,
            "1-1",
            "tool-2"
        ));
    }

    #[test]
    fn superseded_hosts_retire_when_idle_or_at_the_deadline() {
        let now = Instant::now();
        let deadline = now + RETIRE_TIMEOUT;

        assert!(should_retire(Some(deadline), false, now));
        assert!(!should_retire(Some(deadline), true, now));
        assert!(should_retire(Some(deadline), true, deadline));
        assert!(!should_retire(None, false, deadline));
    }

    #[test]
    fn automatic_restarts_back_off_stop_and_reset() {
        let mut backoff = RestartBackoff::default();
        let mut now = Instant::now();

        for failure in 1..=MAX_AUTOMATIC_RESTARTS {
            backoff.host_failed(now);
            let retry_at = backoff.retry_at.expect("retry should remain in budget");
            let expected = RESTART_BASE_DELAY
                .saturating_mul(1_u32 << u32::from(failure - 1))
                .min(RESTART_MAX_DELAY);
            assert_eq!(retry_at.duration_since(now), expected);
            assert!(!backoff.ready(now));
            assert!(backoff.ready(retry_at));
            now = retry_at;
        }
        backoff.host_failed(now);
        assert!(backoff.retry_at.is_none());

        backoff.reset();
        assert_eq!(backoff.failures, 0);
        backoff.host_failed(now);
        backoff.host_started(now);
        backoff.reset_if_healthy(now + RESTART_HEALTHY_PERIOD);
        assert_eq!(backoff.failures, 0);
        assert!(backoff.retry_at.is_none());
    }

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
    fn bun_discovery_does_not_execute_candidates() {
        let unique = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let root = std::env::temp_dir().join(format!("hex-bun-{unique}"));
        let candidate = root.join("bun");
        let marker = root.join("executed");
        fs::create_dir(&root).unwrap();
        fs::write(
            &candidate,
            format!("#!/bin/sh\ntouch {}\n", marker.display()),
        )
        .unwrap();

        assert_eq!(find_bun_candidate([candidate.clone()]), Some(candidate));
        assert!(!marker.exists());
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn failed_children_are_reaped() {
        let mut child = Command::new("/bin/sh")
            .args(["-c", "sleep 30"])
            .spawn()
            .unwrap();
        kill_and_wait(&mut child);
        assert!(child.try_wait().unwrap().is_some());
    }

    #[test]
    fn clean_workspace_provisioning_preserves_user_config_on_refresh() {
        let unique = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let workspace = std::env::temp_dir().join(format!("hex-workspace-{unique}"));
        let sdk = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("sdk/commands");

        initialize_workspace_at(&workspace, &sdk).unwrap();
        assert!(workspace.join(".hex-sdk/dist/bin.js").is_file());
        assert!(workspace.join("package.json").is_file());
        assert!(workspace.join("tsconfig.json").is_file());
        assert!(workspace.join("AGENTS.md").is_file());
        assert!(
            workspace
                .join(".agents/skills/personal-commands/SKILL.md")
                .is_file()
        );
        fs::write(workspace.join("hex.config.ts"), "// user config\n").unwrap();
        initialize_workspace_at(&workspace, &sdk).unwrap();
        assert_eq!(
            fs::read_to_string(workspace.join("hex.config.ts")).unwrap(),
            "// user config\n"
        );
        fs::remove_dir_all(workspace).unwrap();
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
            id: "personal".into(),
            phrases: vec!["personal phrase".into()],
            group: "Personal".into(),
            description: "Personal command".into(),
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
}
