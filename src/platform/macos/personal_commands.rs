use std::collections::{BTreeMap, HashMap, HashSet, VecDeque};
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

use crate::commands::{
    Action, ActionOutcome, CapturedValue, CapturedValues, CommandConfig, ConfiguredCommand,
    ContextPredicate, PersonalCapture,
};
use crate::context::ContextSnapshot;
use crate::dictation::DictationProtocol;
use crate::keyboard::{Key, Modifiers};

const MAX_FRAME_BYTES: usize = 256 * 1024;
const MAX_INPUT_FRAME_BYTES: usize = 64 * 1024;
const HOST_START_TIMEOUT: Duration = Duration::from_secs(5);
const POLL_INTERVAL: Duration = Duration::from_millis(250);
const RELOAD_DEBOUNCE: Duration = Duration::from_millis(350);
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
const MAX_TRANSFORMATION_TEXT_BYTES: usize = 48 * 1024;
const TRANSFORMATION_TIMEOUT: Duration = Duration::from_secs(10);
const LOWERCASE_TRANSFORMATION_ID: &str = "lowercase";
const SPONGEBOB_TRANSFORMATION_ID: &str = "spongebob-case";

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
#[serde(rename_all = "camelCase")]
pub struct StatusTransformation {
    pub id: String,
    pub name: String,
    pub description: Option<String>,
}

pub fn include_builtin_transformations(status: &mut StatusSnapshot) {
    status.transformations.retain(|transformation| {
        !matches!(
            transformation.id.as_str(),
            LOWERCASE_TRANSFORMATION_ID | SPONGEBOB_TRANSFORMATION_ID
        )
    });
    status.transformations.splice(
        0..0,
        [
            StatusTransformation {
                id: LOWERCASE_TRANSFORMATION_ID.into(),
                name: "Lowercase".into(),
                description: Some("Convert the final text to lowercase".into()),
            },
            StatusTransformation {
                id: SPONGEBOB_TRANSFORMATION_ID.into(),
                name: "SpongeBob case".into(),
                description: Some("Alternate lowercase and uppercase letters".into()),
            },
        ],
    );
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
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
    let mut transformation_ids = HashSet::new();
    for transformation in &status.transformations {
        validate_text(&transformation.id, "status transformation id", 128)?;
        validate_text(&transformation.name, "status transformation name", 1024)?;
        if let Some(description) = &transformation.description {
            validate_text(description, "status transformation description", 1024)?;
        }
        if !transformation_ids.insert(&transformation.id) {
            return Err(eyre!("status transformation IDs must be unique"));
        }
    }
    Ok(())
}

pub fn initialize_workspace() -> Result<PathBuf> {
    let workspace = crate::app_paths::personal_commands_workspace()?;
    let sdk = crate::app_paths::personal_commands_sdk()?;
    let sdk_changed = initialize_workspace_at(&workspace, &sdk)?;
    let bun = find_bun().ok_or_else(|| eyre!("Bun is required to install personal commands"))?;
    if sdk_changed {
        remove_installed_managed_sdk(&workspace)?;
    }
    install_workspace_dependencies(&workspace, &bun)?;
    Ok(workspace)
}

pub fn initialize_workspace_at(workspace: &Path, sdk: &Path) -> Result<bool> {
    let template = sdk.join("workspace-template");
    if !sdk.join("package.json").is_file()
        || !sdk.join("dist/bin.js").is_file()
        || !template.is_dir()
    {
        return Err(eyre!("personal command SDK resources are incomplete"));
    }
    fs::create_dir_all(workspace)?;
    let sdk_changed = refresh_managed_sdk_at(workspace, sdk)?;
    copy_directory(&template, workspace, false)?;
    Ok(sdk_changed)
}

fn refresh_managed_sdk_at(workspace: &Path, sdk: &Path) -> Result<bool> {
    if !sdk.join("package.json").is_file() || !sdk.join("dist/bin.js").is_file() {
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
    if directory_manifest(&staged_sdk)? == directory_manifest(&installed_sdk)? {
        fs::remove_dir_all(staged_sdk)?;
        return Ok(false);
    }
    let previous_sdk = workspace.join(".hex-sdk.previous");
    if previous_sdk.exists() {
        fs::remove_dir_all(&previous_sdk)?;
    }
    if installed_sdk.exists() {
        fs::rename(&installed_sdk, &previous_sdk)?;
    }
    if let Err(error) = fs::rename(&staged_sdk, &installed_sdk) {
        if previous_sdk.exists() {
            let _ = fs::rename(&previous_sdk, &installed_sdk);
        }
        return Err(error.into());
    }
    if previous_sdk.exists() {
        fs::remove_dir_all(previous_sdk)?;
    }
    Ok(true)
}

fn directory_manifest(root: &Path) -> Result<Vec<(PathBuf, Option<Vec<u8>>)>> {
    if !root.exists() {
        return Ok(Vec::new());
    }
    let mut manifest = Vec::new();
    let mut pending = vec![root.to_path_buf()];
    while let Some(directory) = pending.pop() {
        for entry in fs::read_dir(&directory)? {
            let entry = entry?;
            let path = entry.path();
            let file_type = entry.file_type()?;
            if file_type.is_symlink() {
                return Err(eyre!("managed SDK resources must not contain symlinks"));
            }
            let relative = path
                .strip_prefix(root)
                .map_err(|_| eyre!("managed SDK path escaped its root"))?
                .to_path_buf();
            if file_type.is_dir() {
                manifest.push((relative, None));
                pending.push(path);
            } else if file_type.is_file() {
                manifest.push((relative, Some(fs::read(path)?)));
            }
        }
    }
    manifest.sort_by(|left, right| left.0.cmp(&right.0));
    Ok(manifest)
}

fn install_workspace_dependencies(workspace: &Path, bun: &Path) -> Result<()> {
    let status = Command::new(bun)
        .arg("install")
        .current_dir(workspace)
        .status()
        .wrap_err("could not install the personal command workspace")?;
    if status.success() {
        Ok(())
    } else {
        Err(eyre!(
            "Bun failed to install the personal command workspace"
        ))
    }
}

fn remove_installed_managed_sdk(workspace: &Path) -> Result<()> {
    let installed = workspace.join("node_modules/@hex/commands");
    let Ok(metadata) = fs::symlink_metadata(&installed) else {
        return Ok(());
    };
    if metadata.is_dir() && !metadata.file_type().is_symlink() {
        fs::remove_dir_all(installed)?;
    } else {
        fs::remove_file(installed)?;
    }
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
    active: Arc<RwLock<Arc<RuntimeSnapshot>>>,
    requests: SyncSender<Request>,
    outcomes: Receiver<ActionOutcome>,
    stop: Arc<AtomicBool>,
    worker: Option<thread::JoinHandle<()>>,
    worker_done: Receiver<()>,
    transformations: Arc<TransformationClient>,
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
            if matches!(
                id.as_str(),
                LOWERCASE_TRANSFORMATION_ID | SPONGEBOB_TRANSFORMATION_ID
            ) {
                if !custom.is_empty() {
                    output = self.transform_custom(&custom, &output, context, cancelled)?;
                    custom.clear();
                }
                output = apply_builtin_transformation(id, &output).expect("built-in ID is known");
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
                    "custom transformation host is unavailable".to_string()
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
                    return Err("custom transformation host stopped".into());
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

impl crate::dictation_processing::TransformationExecutor for TransformationClient {
    fn transform(
        &self,
        ids: &[String],
        text: &str,
        context: &ContextSnapshot,
        cancelled: &AtomicBool,
    ) -> std::result::Result<String, String> {
        TransformationClient::transform(self, ids, text, context, cancelled)
    }
}

fn apply_builtin_transformation(id: &str, text: &str) -> Option<String> {
    match id {
        LOWERCASE_TRANSFORMATION_ID => Some(text.to_lowercase()),
        SPONGEBOB_TRANSFORMATION_ID => {
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
            Some(output)
        }
        _ => None,
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

struct PendingTransformation {
    generation: u64,
    response: SyncSender<std::result::Result<String, String>>,
    started: Instant,
}

type CandidateRegistration = (
    HostProcess,
    RuntimeSnapshot,
    Vec<StatusCommand>,
    Option<StatusDictationProtocol>,
    Vec<StatusTransformation>,
);

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
        dictation: Option<RegistrationDictationProtocol>,
        #[serde(default)]
        transformations: Vec<RegistrationTransformation>,
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
    TransformationResult {
        invocation_id: String,
        result: TransformationWireResult,
    },
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct RegistrationTransformation {
    id: String,
    name: String,
    description: Option<String>,
}

#[derive(Clone, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct RegistrationDictationProtocol {
    start: Vec<String>,
    stop: Vec<String>,
    send: Vec<String>,
    cancel: Vec<String>,
}

impl From<&RegistrationDictationProtocol> for StatusDictationProtocol {
    fn from(protocol: &RegistrationDictationProtocol) -> Self {
        Self {
            start: protocol.start.clone(),
            stop: protocol.stop.clone(),
            send: protocol.send.clone(),
            cancel: protocol.cancel.clone(),
        }
    }
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

#[derive(Deserialize)]
#[serde(tag = "type", rename_all = "camelCase")]
enum TransformationWireResult {
    Success { text: String },
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
        #[serde(skip_serializing_if = "BTreeMap::is_empty")]
        captures: BTreeMap<String, WireCaptureValue>,
    },
    Transform {
        invocation_id: String,
        transformation_ids: Vec<String>,
        text: String,
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
    pub fn start(base: CommandConfig, transformations: Arc<TransformationClient>) -> Option<Self> {
        let active = Arc::new(RwLock::new(Arc::new(RuntimeSnapshot::native(base.clone()))));
        let (request_sender, requests) = mpsc::sync_channel(32);
        let (outcome_sender, outcomes) = mpsc::sync_channel(64);
        let stop = Arc::new(AtomicBool::new(false));
        let worker_active = active.clone();
        let worker_stop = stop.clone();
        let worker_transformations = transformations.clone();
        let worker_requests = request_sender.clone();
        let (done_sender, worker_done) = mpsc::sync_channel(1);
        let worker = thread::Builder::new()
            .name("personal-command-host".into())
            .spawn(move || {
                run_worker(
                    base,
                    worker_active,
                    requests,
                    worker_requests,
                    outcome_sender,
                    worker_stop,
                    worker_transformations,
                );
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
            tracing::warn!("personal command host worker panicked during shutdown");
        }
    }
}

fn run_worker(
    base: CommandConfig,
    active: Arc<RwLock<Arc<RuntimeSnapshot>>>,
    requests: Receiver<Request>,
    request_sender: SyncSender<Request>,
    outcomes: SyncSender<ActionOutcome>,
    stop: Arc<AtomicBool>,
    transformations: Arc<TransformationClient>,
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
    let sdk = match crate::app_paths::personal_commands_sdk() {
        Ok(sdk) => sdk,
        Err(error) => {
            status.host_state = HostState::Unavailable;
            status.last_reload_error = Some(bounded_status_error(&error.to_string()));
            persist_status(&status);
            tracing::warn!(%error, "personal command SDK is unavailable");
            return;
        }
    };
    let sdk_changed = match refresh_managed_sdk_at(&workspace, &sdk) {
        Ok(changed) => changed,
        Err(error) => {
            status.host_state = HostState::Unavailable;
            status.last_reload_error = Some(bounded_status_error(&error.to_string()));
            persist_status(&status);
            tracing::warn!(%error, "could not refresh the managed personal command SDK");
            return;
        }
    };
    let managed_host = workspace.join("node_modules/@hex/commands/dist/bin.js");
    if sdk_changed || !managed_host.is_file() {
        let refreshed = (|| {
            if sdk_changed {
                remove_installed_managed_sdk(&workspace)?;
            }
            install_workspace_dependencies(&workspace, &bun)
        })();
        if let Err(error) = refreshed {
            status.host_state = HostState::Unavailable;
            status.last_reload_error = Some(bounded_status_error(&error.to_string()));
            persist_status(&status);
            tracing::warn!(%error, "could not activate the current personal command SDK");
            return;
        }
        tracing::info!(sdk_changed, "refreshed the managed personal command SDK");
    }
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
        &request_sender,
        &transformations,
    ) {
        restart_backoff.host_started(Instant::now());
        hosts.insert(host.generation, host);
    }
    let mut next_invocation = 0_u64;
    let mut pending = HashMap::<String, PendingInvocation>::new();
    let mut pending_transformations = HashMap::<String, PendingTransformation>::new();
    let mut pending_tools = HashSet::<(u64, String, String)>::new();

    while !stop.load(Ordering::Acquire) {
        restart_backoff.reset_if_healthy(Instant::now());
        let mut active_host_failed = false;
        while let Ok(event) = events.try_recv() {
            active_host_failed |= handle_host_event(
                event,
                &mut hosts,
                &mut pending,
                &mut pending_transformations,
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
                &request_sender,
                &transformations,
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
                let context = invocation_context(&invocation.context);
                let captures = invocation
                    .captures
                    .iter()
                    .map(|(name, value)| {
                        let value = match value {
                            CapturedValue::Digit(value) => WireCaptureValue::Number(*value),
                            CapturedValue::Letter(value) => {
                                WireCaptureValue::Text(value.to_string())
                            }
                            CapturedValue::Choice(value) => WireCaptureValue::Text(value.clone()),
                            CapturedValue::Text(value) => WireCaptureValue::Text(value.clone()),
                        };
                        (name.clone(), value)
                    })
                    .collect();
                let input = HostInput::Invoke {
                    invocation_id: invocation_id.clone(),
                    command_id: invocation.command_id.clone(),
                    context,
                    captures,
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
                        &mut pending_transformations,
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
            Ok(Request::Transform(request)) => {
                let request = *request;
                if pending_transformations.len() >= MAX_PENDING_INVOCATIONS {
                    let _ = request.response.send(Err(
                        "too many custom transformations are still running".into(),
                    ));
                    continue;
                }
                let Some(host) = hosts.get(&request.generation) else {
                    let _ = request
                        .response
                        .send(Err("custom transformation generation is unavailable".into()));
                    continue;
                };
                next_invocation += 1;
                let invocation_id = format!("{}-transform-{next_invocation}", request.generation);
                let context = invocation_context(&request.context);
                if host
                    .input
                    .try_send(HostInput::Transform {
                        invocation_id: invocation_id.clone(),
                        transformation_ids: request.ids,
                        text: request.text,
                        context,
                    })
                    .is_err()
                {
                    let _ = request
                        .response
                        .send(Err("custom transformation host is unavailable".into()));
                } else {
                    pending_transformations.insert(
                        invocation_id,
                        PendingTransformation {
                            generation: request.generation,
                            response: request.response,
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
        let timed_out_generations = pending_transformations
            .values()
            .filter_map(|invocation| {
                (invocation.started.elapsed() >= TRANSFORMATION_TIMEOUT)
                    .then_some(invocation.generation)
            })
            .collect::<HashSet<_>>();
        for generation in timed_out_generations {
            let restart = generation_is_active(&hosts, generation);
            fail_generation(
                generation,
                &mut hosts,
                &mut pending,
                &mut pending_transformations,
                &mut pending_tools,
                &outcomes,
                "custom transformations timed out",
            );
            if restart {
                restart_backoff.host_failed(now);
            }
        }
        let retired = hosts
            .iter()
            .filter_map(|(generation, host)| {
                let has_pending = pending
                    .values()
                    .any(|invocation| invocation.generation == *generation)
                    || pending_transformations
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
                &mut pending_transformations,
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
    transformations.clear();
    persist_status(&status);
}

#[allow(clippy::too_many_arguments)]
fn reload(
    base: &CommandConfig,
    active: &RwLock<Arc<RuntimeSnapshot>>,
    event_sender: &SyncSender<HostEvent>,
    workspace: &Path,
    config: &Path,
    bun: &Path,
    host_entrypoint: &Path,
    generation: &mut u64,
    status: &mut StatusSnapshot,
    requests: &SyncSender<Request>,
    transformations: &TransformationClient,
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
        Ok((host, runtime, catalog, dictation, transformation_catalog)) => {
            transformations.activate(&runtime, requests.clone());
            *active.write().expect("command registry poisoned") = Arc::new(runtime);
            status.active_generation = Some(*generation);
            status.catalog = catalog;
            status.dictation = dictation;
            status.transformations = transformation_catalog;
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
) -> Result<CandidateRegistration> {
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
                    dictation,
                    transformations,
                    commands,
                },
            )) if event_generation == generation => {
                let catalog = status_catalog(&commands);
                let status_dictation = dictation.as_ref().map(StatusDictationProtocol::from);
                let status_transformations = transformations
                    .iter()
                    .map(|transformation| StatusTransformation {
                        id: transformation.id.clone(),
                        name: transformation.name.clone(),
                        description: transformation.description.clone(),
                    })
                    .collect();
                let compiled = match compile_registration(
                    base,
                    generation,
                    protocol_version,
                    dictation,
                    transformations,
                    commands,
                ) {
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
                    status_dictation,
                    status_transformations,
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

fn handle_host_event(
    event: HostEvent,
    hosts: &mut HashMap<u64, HostProcess>,
    pending: &mut HashMap<String, PendingInvocation>,
    pending_transformations: &mut HashMap<String, PendingTransformation>,
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
                    pending_transformations,
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
                    pending_transformations,
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
                    pending_transformations,
                    pending_tools,
                    outcomes,
                    "personal command host sent an invalid invocation result",
                );
                return restart;
            }
            false
        }
        HostEvent::Frame(
            generation,
            HostOutput::TransformationResult {
                invocation_id,
                result,
            },
        ) => {
            let valid = pending_transformations
                .get(&invocation_id)
                .is_some_and(|invocation| invocation.generation == generation);
            if valid {
                let invocation = pending_transformations
                    .remove(&invocation_id)
                    .expect("transformation invocation checked");
                let result = match result {
                    TransformationWireResult::Success { text }
                        if text.len() <= MAX_TRANSFORMATION_TEXT_BYTES =>
                    {
                        Ok(text)
                    }
                    TransformationWireResult::Success { .. } => {
                        Err("custom transformation output is too large".into())
                    }
                    TransformationWireResult::Failure { message } => Err(bounded_error(&message)),
                };
                tracing::info!(
                    generation,
                    invocation_id,
                    status = if result.is_ok() {
                        "completed"
                    } else {
                        "failed"
                    },
                    duration_ms = invocation.started.elapsed().as_millis(),
                    "custom transformation invocation"
                );
                let _ = invocation.response.send(result);
            } else {
                let restart = generation_is_active(hosts, generation);
                fail_generation(
                    generation,
                    hosts,
                    pending,
                    pending_transformations,
                    pending_tools,
                    outcomes,
                    "personal command host sent an invalid transformation result",
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
                        pending_transformations,
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
                pending_transformations,
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
                pending_transformations,
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
    protocol_version: u8,
    dictation: Option<RegistrationDictationProtocol>,
    transformations: Vec<RegistrationTransformation>,
    commands: Vec<RegistrationCommand>,
) -> Result<RuntimeSnapshot> {
    if protocol_version != 2 {
        return Err(eyre!(
            "unsupported personal command protocol version {protocol_version}"
        ));
    }
    if commands.len() > MAX_COMMANDS {
        return Err(eyre!("personal registry exceeds {MAX_COMMANDS} commands"));
    }
    if transformations.len() > MAX_TRANSFORMATIONS {
        return Err(eyre!(
            "personal registry exceeds {MAX_TRANSFORMATIONS} transformations"
        ));
    }
    let mut transformation_ids = HashSet::new();
    for transformation in transformations {
        validate_text(&transformation.id, "transformation id", 128)?;
        validate_text(&transformation.name, "transformation name", 1024)?;
        if let Some(description) = &transformation.description {
            validate_text(description, "transformation description", 1024)?;
        }
        if !transformation_ids.insert(transformation.id) {
            return Err(eyre!("duplicate transformation id"));
        }
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
    pending_transformations: &mut HashMap<String, PendingTransformation>,
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
    let transformation_ids = pending_transformations
        .iter()
        .filter_map(|(id, invocation)| (invocation.generation == generation).then_some(id.clone()))
        .collect::<Vec<_>>();
    for id in transformation_ids {
        if let Some(invocation) = pending_transformations.remove(&id) {
            let _ = invocation.response.send(Err(error.into()));
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
            r#"{"type":"registration","protocolVersion":2,"commands":[{"id":"native","phrases":["open example"],"execution":{"type":"native","action":{"type":"openUrl","url":"https://example.com"}}},{"id":"handled","phrases":["do work"],"group":"Work","when":{"application":"Slack"},"execution":{"type":"handler"}}]}"#,
        )
        .unwrap();
        let HostOutput::Registration {
            protocol_version,
            commands,
            ..
        } = registration
        else {
            panic!("expected registration")
        };
        let compiled = compile_registration(
            &CommandConfig::new(),
            7,
            protocol_version,
            None,
            vec![],
            commands,
        )
        .unwrap();

        assert!(matches!(
            compiled.commands.resolve(Mode::Listening, "open example", &ContextSnapshot::default()),
            Decision::Execute { action: Action::OpenUrl(url), .. } if url == "https://example.com"
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
        let registration: HostOutput = serde_json::from_str(
            r#"{"type":"registration","protocolVersion":2,"commands":[{"id":"search","phrases":["search amazon for {query}"],"execution":{"type":"handler"}}]}"#,
        )
        .unwrap();
        let HostOutput::Registration {
            protocol_version,
            commands,
            ..
        } = registration
        else {
            panic!("expected registration")
        };
        let compiled = compile_registration(
            &CommandConfig::new(),
            7,
            protocol_version,
            None,
            vec![],
            commands,
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
        let compiled =
            compile_registration(&CommandConfig::new(), 9, 2, None, vec![], vec![command]).unwrap();

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
        let compiled =
            compile_registration(&CommandConfig::new(), 9, 2, None, vec![], vec![command]).unwrap();

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
        let compiled =
            compile_registration(&CommandConfig::new(), 9, 2, None, vec![], vec![command]).unwrap();

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
        let compiled =
            compile_registration(&CommandConfig::new(), 9, 2, None, vec![], vec![command]).unwrap();

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
        assert!(
            compile_registration(&CommandConfig::new(), 1, 2, None, vec![], vec![valid]).is_ok()
        );

        for input in [
            r#"{"id":"bad","phrases":["key {key}"],"captures":{"key":{"type":"union","members":[{"type":"letter"}]}},"execution":{"type":"handler"}}"#,
            r#"{"id":"bad","phrases":["key {key}"],"captures":{"key":{"type":"union","members":[{"type":"letter"},{"type":"text"}]}},"execution":{"type":"handler"}}"#,
            r#"{"id":"bad","phrases":["key {key}"],"captures":{"key":{"type":"union","members":[{"type":"digit","min":0,"max":9},{"type":"choice","choices":{"two":["2"]}}]}},"execution":{"type":"handler"}}"#,
            r#"{"id":"bad","phrases":["key {key}"],"captures":{"key":{"type":"union","members":[{"type":"letter"},{"type":"choice","choices":{"a":["alpha"]}}]}},"execution":{"type":"handler"}}"#,
            r#"{"id":"bad","phrases":["key {key}"],"captures":{"key":{"type":"union","members":[{"type":"letter"},{"type":"union","members":[{"type":"digit","min":0,"max":0},{"type":"union","members":[{"type":"choice","choices":{"home":["home"]}},{"type":"union","members":[{"type":"choice","choices":{"end":["end"]}},{"type":"union","members":[{"type":"choice","choices":{"up":["up"]}},{"type":"choice","choices":{"down":["down"]}}]}]}]}]}]}},"execution":{"type":"handler"}}"#,
        ] {
            let command = serde_json::from_str(input).unwrap();
            assert!(
                compile_registration(&CommandConfig::new(), 1, 2, None, vec![], vec![command])
                    .is_err(),
                "union should be rejected: {input}"
            );
        }

        let members = (0..=MAX_UNION_MEMBERS)
            .map(|index| CaptureDescriptor::Choice {
                choices: BTreeMap::from([(format!("key{index}"), vec![format!("key{index}")])]),
            })
            .collect();
        let command = RegistrationCommand {
            id: "bad".into(),
            phrases: vec!["key {key}".into()],
            group: None,
            description: None,
            when: None,
            captures: BTreeMap::from([("key".into(), CaptureDescriptor::Union { members })]),
            execution: Execution::Handler,
        };
        assert!(
            compile_registration(&CommandConfig::new(), 1, 2, None, vec![], vec![command]).is_err()
        );

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
        let command = RegistrationCommand {
            id: "bad".into(),
            phrases: vec!["key {key}".into()],
            group: None,
            description: None,
            when: None,
            captures: BTreeMap::from([(
                "key".into(),
                CaptureDescriptor::Union {
                    members: vec![large_choice("left"), large_choice("right")],
                },
            )]),
            execution: Execution::Handler,
        };
        assert!(
            compile_registration(&CommandConfig::new(), 1, 2, None, vec![], vec![command]).is_err()
        );
    }

    #[test]
    fn union_overlap_checks_all_existing_token_kinds() {
        let compile_pair = |right: &str| {
            let union = serde_json::from_str(
                r#"{"id":"union","phrases":["press {key}"],"captures":{"key":{"type":"union","members":[{"type":"digit","min":0,"max":2},{"type":"choice","choices":{"home":["home"]}}]}},"execution":{"type":"handler"}}"#,
            )
            .unwrap();
            compile_registration(
                &CommandConfig::new(),
                1,
                2,
                None,
                vec![],
                vec![union, serde_json::from_str(right).unwrap()],
            )
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
            compile_registration(
                &CommandConfig::new(),
                1,
                2,
                None,
                vec![],
                vec![letter, right],
            )
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
                    compile_registration(&CommandConfig::new(), 1, 2, None, vec![], vec![command])
                        .is_err(),
                    "descriptor should be rejected: {input}"
                );
            }
        }

        let command = |choices| RegistrationCommand {
            id: "bad".into(),
            phrases: vec!["move {direction}".into()],
            group: None,
            description: None,
            when: None,
            captures: BTreeMap::from([("direction".into(), CaptureDescriptor::Choice { choices })]),
            execution: Execution::Handler,
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
            assert!(
                compile_registration(
                    &CommandConfig::new(),
                    1,
                    2,
                    None,
                    vec![],
                    vec![command(choices)]
                )
                .is_err()
            );
        }
    }

    #[test]
    fn choice_overlap_checks_literals_digits_choices_and_text() {
        let compile_pair = |left: &str, right: &str| {
            let left = serde_json::from_str(left).unwrap();
            let right = serde_json::from_str(right).unwrap();
            compile_registration(&CommandConfig::new(), 1, 2, None, vec![], vec![left, right])
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
            assert!(
                compile_registration(&CommandConfig::new(), 1, 2, None, vec![], vec![command])
                    .is_err()
            );
        }

        let first = serde_json::from_str(
            r#"{"id":"first","phrases":["press {number}"],"captures":{"number":{"type":"digit","min":1,"max":3}},"execution":{"type":"handler"}}"#,
        )
        .unwrap();
        let disjoint = serde_json::from_str(
            r#"{"id":"disjoint","phrases":["press {number}"],"captures":{"number":{"type":"digit","min":4,"max":6}},"execution":{"type":"handler"}}"#,
        )
        .unwrap();
        assert!(
            compile_registration(
                &CommandConfig::new(),
                1,
                2,
                None,
                vec![],
                vec![first, disjoint]
            )
            .is_ok()
        );

        let first = serde_json::from_str(
            r#"{"id":"first","phrases":["press {number}"],"captures":{"number":{"type":"digit","min":1,"max":3}},"execution":{"type":"handler"}}"#,
        )
        .unwrap();
        let overlapping = serde_json::from_str(
            r#"{"id":"overlapping","phrases":["press {number}"],"captures":{"number":{"type":"digit","min":3,"max":5}},"execution":{"type":"handler"}}"#,
        )
        .unwrap();
        assert!(
            compile_registration(
                &CommandConfig::new(),
                1,
                2,
                None,
                vec![],
                vec![first, overlapping]
            )
            .is_err()
        );
    }

    #[test]
    fn capture_phrases_are_rejected_for_native_commands() {
        let registration: HostOutput = serde_json::from_str(
            r#"{"type":"registration","protocolVersion":2,"commands":[{"id":"search","phrases":["search amazon for {query}"],"execution":{"type":"native","action":{"type":"openUrl","url":"https://example.com"}}}]}"#,
        )
        .unwrap();
        let HostOutput::Registration {
            protocol_version,
            commands,
            ..
        } = registration
        else {
            panic!("expected registration")
        };
        let result = compile_registration(
            &CommandConfig::new(),
            7,
            protocol_version,
            None,
            vec![],
            commands,
        );

        match result {
            Ok(_) => panic!("native capture phrases are invalid"),
            Err(error) => assert!(error.to_string().contains("handler")),
        }
    }

    #[test]
    fn invoke_frames_serialize_captures_only_when_present() {
        let context = InvocationContext {
            application: None,
            browser_host: None,
            browser_url: None,
            window_title: None,
        };
        let with_capture = HostInput::Invoke {
            invocation_id: "7-1".into(),
            command_id: "search".into(),
            context,
            captures: BTreeMap::from([(
                "query".to_string(),
                WireCaptureValue::Text("wool socks".to_string()),
            )]),
        };
        let json = serde_json::to_string(&with_capture).unwrap();
        assert!(json.contains(r#""captures":{"query":"wool socks"}"#));

        let without_capture = HostInput::Invoke {
            invocation_id: "7-2".into(),
            command_id: "search".into(),
            context: InvocationContext {
                application: None,
                browser_host: None,
                browser_url: None,
                window_title: None,
            },
            captures: BTreeMap::new(),
        };
        let json = serde_json::to_string(&without_capture).unwrap();
        assert!(!json.contains("captures"));
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
    fn built_in_transformations_replace_matching_personal_catalog_entries() {
        let mut status = StatusSnapshot {
            transformations: vec![
                StatusTransformation {
                    id: LOWERCASE_TRANSFORMATION_ID.into(),
                    name: "Custom lowercase".into(),
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
                "custom"
            ]
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

        assert!(compile_registration(&base, 1, 2, None, vec![], vec![command]).is_err());
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
            2,
            Some(RegistrationDictationProtocol {
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

        let fallback =
            compile_registration(&CommandConfig::new(), 1, 2, None, vec![], vec![]).unwrap();
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
        let protocol = RegistrationDictationProtocol {
            start: vec!["begin note".into()],
            stop: vec!["finish note".into()],
            send: vec!["send note".into()],
            cancel: vec!["discard note".into()],
        };

        assert!(
            compile_registration(
                &CommandConfig::new(),
                1,
                2,
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
            assert!(compile_registration(&base, 1, 2, None, vec![], vec![command]).is_err());
        }
    }

    #[test]
    fn browser_hosts_are_exact_and_canonical() {
        let command: RegistrationCommand = serde_json::from_str(
            r#"{"id":"bad","phrases":["bad"],"when":{"browserHost":"https://x.com/path"},"execution":{"type":"handler"}}"#,
        )
        .unwrap();
        assert!(
            compile_registration(&CommandConfig::new(), 1, 2, None, vec![], vec![command]).is_err()
        );

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

        assert!(initialize_workspace_at(&workspace, &sdk).unwrap());
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
        assert!(!initialize_workspace_at(&workspace, &sdk).unwrap());
        assert_eq!(
            fs::read_to_string(workspace.join("hex.config.ts")).unwrap(),
            "// user config\n"
        );
        fs::write(workspace.join(".hex-sdk/dist/index.js"), "stale SDK\n").unwrap();
        assert!(initialize_workspace_at(&workspace, &sdk).unwrap());
        assert_eq!(
            fs::read(workspace.join(".hex-sdk/dist/index.js")).unwrap(),
            fs::read(sdk.join("dist/index.js")).unwrap()
        );
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
}
