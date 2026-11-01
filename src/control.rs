//! `hex` control: the typed command/reply protocol an agent uses to drive the
//! one running Hex, the settings registry behind `hex settings`, and the CLI
//! client. Transports (the macOS local API's `POST /control`) only move
//! [`ControlRequest`]s and [`ControlResponse`]s; the app's host carries out
//! each command, and every rule shared between the window and the CLI lives
//! here or in the app's own setters.

pub mod cli;
#[cfg(all(debug_assertions, target_os = "macos"))]
pub(crate) mod dashboard;
#[cfg_attr(not(target_os = "macos"), allow(dead_code))]
pub(crate) mod developer_control;
#[cfg(not(target_os = "macos"))]
pub(crate) mod host;
pub mod settings;
pub(crate) mod transcript_cli;

use std::sync::mpsc::{Receiver, RecvTimeoutError, SyncSender};
use std::time::Duration;

use serde::{Deserialize, Serialize};

use crate::dictation::vocabulary::{self, VocabularyEntry};

/// The wire version of [`ControlRequest`] and [`ControlResponse`]. A change
/// that breaks either shape increments it.
pub const CONTROL_VERSION: u32 = 4;

/// What a capture command does. Commands are idempotent: starting while
/// recording or stopping while idle changes nothing and succeeds.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize, clap::ValueEnum)]
#[serde(rename_all = "kebab-case")]
pub enum CaptureAction {
    /// Start recording, locked as after a double tap: the shortcut or
    /// `stop` finishes it, Escape or `cancel` discards it.
    Start,
    /// Finish the recording and transcribe it.
    Stop,
    /// Discard the recording, or the newest unfinished dictation.
    Cancel,
    /// Start when idle, otherwise stop.
    Toggle,
}

/// What `hex capture` does (the `capture` experiment).
/// Idempotent like [`CaptureAction`].
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize, clap::ValueEnum)]
#[serde(rename_all = "kebab-case")]
pub enum ScrawlCaptureAction {
    Start,
    /// End the capture and write its bundle in the background.
    Stop,
    /// End the capture and discard its bundle.
    Cancel,
    Status,
}

/// The window's panes, by the names `hex window show` takes.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize, clap::ValueEnum)]
#[serde(rename_all = "kebab-case")]
pub enum Pane {
    Settings,
    /// Settings with the transcription model picker open.
    Models,
    #[value(alias = "replacements")]
    Modes,
    History,
    Meetings,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(tag = "command", rename_all = "kebab-case")]
pub enum ControlCommand {
    Status,
    Quit {
        #[serde(default)]
        force: bool,
    },
    Restart {
        #[serde(default)]
        force: bool,
    },
    Dictation {
        action: CaptureAction,
        /// `stop` only: answer with the transcript instead of pasting it.
        #[serde(default, skip_serializing_if = "std::ops::Not::not")]
        print: bool,
        /// `start` and `toggle` only: the dictation this starts pastes the
        /// transcript as heard, skipping the Output pane's steps.
        #[serde(default, skip_serializing_if = "std::ops::Not::not")]
        raw: bool,
    },
    PasteLast,
    MeetingRecord {
        #[serde(default)]
        title: Option<String>,
    },
    MeetingStop,
    MeetingCopyLast,
    SettingsGet {
        #[serde(default)]
        key: Option<String>,
    },
    SettingsSet {
        key: String,
        value: String,
    },
    /// Several settings at once, all or none (`hex settings apply`).
    SettingsApply {
        values: serde_json::Map<String, serde_json::Value>,
    },
    ModelList,
    ModelUse {
        model: String,
        #[serde(default)]
        language: Option<String>,
    },
    ModelInstall {
        model: String,
        #[serde(default)]
        language: Option<String>,
    },
    ModelCancel,
    WindowShow {
        #[serde(default)]
        pane: Option<Pane>,
    },
    WindowHide,
    /// Replay the first-run onboarding in the window (dev, test, and local
    /// builds); settings, history, models, and permissions stay.
    OnboardingReset,
    /// Open the window on a playground story (Labs).
    WindowStory {
        story: String,
        #[serde(default)]
        variant: Option<String>,
        #[serde(default)]
        play: bool,
    },
    HistoryClear,
    CheckForUpdates,
    /// Report Launch at Login, or turn it on or off.
    LaunchAtLogin {
        #[serde(default)]
        enabled: Option<bool>,
    },
    /// The shared vocabulary.
    VocabularyList,
    VocabularyAdd {
        term: String,
        #[serde(default)]
        sounds_like: Vec<String>,
    },
    VocabularyRemove {
        term: String,
    },
    /// A Scrawl capture (the `capture` experiment).
    ScrawlCapture {
        action: ScrawlCaptureAction,
    },
    /// The newest captures in the captures folder, newest first.
    ScrawlCaptureList {
        #[serde(default)]
        limit: Option<usize>,
    },
    /// One capture, by bundle name, path, or `latest`.
    ScrawlCaptureShow {
        capture: String,
    },
    /// Sends a capture to an OpenCode session: `to`, or where its route or
    /// its last send says.
    ScrawlCaptureSend {
        capture: String,
        #[serde(default)]
        to: Option<String>,
        /// Also save a rule sending the capture's site or app there (the
        /// newest routed capture only).
        #[serde(default)]
        always: bool,
    },
    /// Takes the newest send back while it can be undone.
    ScrawlCaptureUndo,
    /// What routing sees for a capture and what it decides. `ask_jev` asks
    /// Jev again (one call).
    ScrawlCaptureRoute {
        capture: String,
        #[serde(default)]
        ask_jev: bool,
    },
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct ControlRequest {
    pub version: u32,
    #[serde(flatten)]
    pub command: ControlCommand,
}

impl ControlRequest {
    pub fn new(command: ControlCommand) -> Self {
        Self {
            version: CONTROL_VERSION,
            command,
        }
    }
}

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
#[serde(tag = "status", rename_all = "kebab-case")]
pub enum ControlReply {
    /// The command ran. `changed` is false when it was already so.
    Done {
        message: String,
        changed: bool,
    },
    Status(Box<StatusReport>),
    Settings {
        settings: Vec<settings::SettingValue>,
    },
    Models {
        models: Vec<ModelInfo>,
    },
    Vocabulary {
        entries: Vec<VocabularyEntry>,
    },
    ScrawlCapture(Box<ScrawlCaptureReport>),
    ScrawlCaptures {
        captures: Vec<ScrawlCaptureSummary>,
    },
    ScrawlCaptureDetail(Box<ScrawlCaptureDetail>),
    ScrawlCaptureRoute {
        explanation: String,
    },
    /// `hex dictation stop --print`: the processed transcript.
    Transcript {
        text: String,
    },
    Error {
        code: String,
        message: String,
    },
}

impl ControlReply {
    pub fn done(message: impl Into<String>) -> Self {
        Self::Done {
            message: message.into(),
            changed: true,
        }
    }

    pub fn unchanged(message: impl Into<String>) -> Self {
        Self::Done {
            message: message.into(),
            changed: false,
        }
    }

    pub fn error(code: impl Into<String>, message: impl Into<String>) -> Self {
        Self::Error {
            code: code.into(),
            message: message.into(),
        }
    }
}

impl From<ControlError> for ControlReply {
    fn from(error: ControlError) -> Self {
        Self::error(error.code, error.message)
    }
}

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
pub struct ControlResponse {
    pub version: u32,
    #[serde(flatten)]
    pub reply: ControlReply,
}

impl From<ControlReply> for ControlResponse {
    fn from(reply: ControlReply) -> Self {
        Self {
            version: CONTROL_VERSION,
            reply,
        }
    }
}

#[derive(Clone, Copy, Debug, Default, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum ScrawlCaptureState {
    #[default]
    Idle,
    Recording,
}

/// A `scrawl-capture` command's answer.
#[derive(Clone, Debug, Default, Deserialize, PartialEq, Serialize)]
pub struct ScrawlCaptureReport {
    /// False when the command found nothing to do.
    pub changed: bool,
    pub state: ScrawlCaptureState,
    /// The recording capture's bundle, or the one `stop`/`cancel` just ended.
    pub bundle: Option<std::path::PathBuf>,
    pub elapsed_ms: Option<u64>,
    /// Stopped captures still transcribing or writing.
    #[serde(default)]
    pub finishing: Vec<std::path::PathBuf>,
    /// The most recent capture that finished, failed, or was cancelled.
    pub last: Option<ScrawlCaptureOutcome>,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct ScrawlCaptureOutcome {
    pub bundle: std::path::PathBuf,
    /// done, failed, or cancelled.
    pub stage: String,
    pub words: Option<usize>,
    /// Spans of a written capture's audio that went missing.
    #[serde(default)]
    pub gaps: Option<usize>,
    /// Why a written capture has no transcript: `no_audio`, or the engine's
    /// reason.
    #[serde(default)]
    pub missing_transcript: Option<String>,
}

/// A capture on disk, as `hex capture list` shows it.
#[derive(Clone, Debug, Default, Deserialize, Eq, PartialEq, Serialize)]
pub struct ScrawlCaptureSummary {
    /// The bundle's folder name, which `show`, `send`, and `route` take.
    pub id: String,
    pub bundle: std::path::PathBuf,
    pub created_at_ms: u64,
    pub duration_ms: u64,
    /// `screen`, `window`, `area`, or `iphone`.
    pub mode: String,
    pub app: Option<String>,
    pub words: usize,
    /// The session it was sent to, if it was.
    pub sent_to: Option<ScrawlCaptureSession>,
    /// OpenCode didn't take its send yet; Hex retries it.
    #[serde(default)]
    pub queued: bool,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct ScrawlCaptureSession {
    pub session: String,
    pub title: String,
}

/// `hex capture show`: a capture's summary with its words and files.
#[derive(Clone, Debug, Default, Deserialize, Eq, PartialEq, Serialize)]
pub struct ScrawlCaptureDetail {
    #[serde(flatten)]
    pub summary: ScrawlCaptureSummary,
    pub window_title: Option<String>,
    pub transcript: String,
    pub note: Option<String>,
    /// Why there is no transcript although there was audio.
    pub missing_transcript: Option<String>,
    pub marks: usize,
    pub keyframes: usize,
    pub video: Option<std::path::PathBuf>,
    pub audio: Option<std::path::PathBuf>,
    pub contact_sheet: Option<std::path::PathBuf>,
}

/// A refused or failed command, with a stable code (see `docs/cli.md`).
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ControlError {
    pub code: &'static str,
    pub message: String,
}

impl ControlError {
    pub fn new(code: &'static str, message: impl Into<String>) -> Self {
        Self {
            code,
            message: message.into(),
        }
    }
}

/// How long `hex dictation stop --print` waits for its transcript.
pub const PRINT_WAIT: Duration = Duration::from_secs(60);
/// How long the app may take to carry out any other control command.
const CONTROL_DEADLINE: Duration = Duration::from_secs(10);

impl ControlCommand {
    /// How long a transport waits for the app's answer to this command.
    pub fn deadline(&self) -> Duration {
        match self {
            Self::Dictation { print: true, .. } => PRINT_WAIT + Duration::from_secs(5),
            _ => CONTROL_DEADLINE,
        }
    }
}

/// What a printing stop gets back from the listener once its dictation
/// finishes: the transcript, processed as a paste would be, or why there is
/// none. A printed transcript is never pasted.
#[derive(Debug, Eq, PartialEq)]
pub enum PrintedTranscript {
    Text(String),
    NoSpeech,
    /// The Linux and Windows listeners stop by themselves without a recording.
    #[cfg_attr(target_os = "macos", allow(dead_code))]
    NotRecording,
    Failed(String),
}

/// The one-shot channel a printing stop rides through the listener's queue.
#[derive(Clone, Debug)]
pub struct PrintReply(SyncSender<PrintedTranscript>);

impl PrintReply {
    pub fn channel() -> (Self, Receiver<PrintedTranscript>) {
        let (sender, receiver) = std::sync::mpsc::sync_channel(1);
        (Self(sender), receiver)
    }

    /// Answers the waiting command; one that gave up is not told.
    pub fn send(self, transcript: PrintedTranscript) {
        let _ = self.0.send(transcript);
    }
}

/// Shortcut events compare by kind; where a reply goes is not part of it.
impl PartialEq for PrintReply {
    fn eq(&self, _: &Self) -> bool {
        true
    }
}

impl Eq for PrintReply {}

/// Waits up to `wait` for a printing stop's transcript. A dropped reply
/// means the dictation was cancelled or failed before it had text.
pub fn await_printed(receiver: &Receiver<PrintedTranscript>, wait: Duration) -> ControlReply {
    match receiver.recv_timeout(wait) {
        Ok(PrintedTranscript::Text(text)) => ControlReply::Transcript { text },
        Ok(PrintedTranscript::NoSpeech) => {
            ControlReply::error("no-speech", "no speech was recorded; nothing to print")
        }
        Ok(PrintedTranscript::NotRecording) => ControlReply::error(
            "not-recording",
            "dictation isn't recording; nothing to print",
        ),
        Ok(PrintedTranscript::Failed(error)) => {
            ControlReply::error("failed", format!("couldn't transcribe it: {error}"))
        }
        Err(RecvTimeoutError::Timeout) => ControlReply::error(
            "failed",
            format!(
                "no transcript within {} s; nothing was printed or pasted",
                wait.as_secs()
            ),
        ),
        Err(RecvTimeoutError::Disconnected) => ControlReply::error(
            "failed",
            "the dictation was cancelled or failed; nothing to print",
        ),
    }
}

/// `print` applies only to `stop`.
pub fn check_flags(action: CaptureAction, print: bool, raw: bool) -> Result<(), ControlError> {
    if print && action != CaptureAction::Stop {
        return Err(ControlError::new(
            "invalid-value",
            "--print works only with `hex dictation stop`",
        ));
    }
    if raw && !matches!(action, CaptureAction::Start | CaptureAction::Toggle) {
        return Err(ControlError::new(
            "invalid-value",
            "--raw works only with `hex dictation start` and `hex dictation toggle`",
        ));
    }
    Ok(())
}

/// One request from a transport to the app's control host.
pub struct ControlCall {
    pub request: ControlRequest,
    pub reply: SyncSender<ControlResponse>,
}

/// Rejects a request from a `hex` built for another protocol version.
pub fn check_version(request: &ControlRequest) -> Result<(), ControlError> {
    if request.version == CONTROL_VERSION {
        return Ok(());
    }
    Err(version_mismatch(request.version.into()))
}

pub fn version_mismatch(version: u64) -> ControlError {
    ControlError::new(
        "version-mismatch",
        format!(
            "this hex speaks control version {version}, but the running Hex speaks \
             {CONTROL_VERSION}; use the hex inside the running app or restart Hex"
        ),
    )
}

#[derive(Clone, Debug, Default, Deserialize, PartialEq, Serialize)]
pub struct StatusReport {
    pub app_version: String,
    pub pid: u32,
    /// The process's physical memory footprint, as Activity Monitor shows it.
    pub memory_footprint_bytes: Option<u64>,
    pub listener: ListenerReport,
    pub capture: CaptureReport,
    pub meeting: MeetingReport,
    pub model: ModelReport,
    pub features: FeatureReport,
    pub permissions: PermissionReport,
    pub window: WindowReport,
    /// current, checking, available, or unavailable.
    pub update: String,
    /// The Labs features turned on (`crate::labs`), by `hex labs` name.
    #[serde(default)]
    pub labs: Vec<String>,
    /// What this OS's Hex can do, by capability name (`rune_ui::platform`).
    /// A command for one it can't do answers `unsupported`.
    #[serde(default)]
    pub capabilities: std::collections::BTreeMap<String, bool>,
}

/// `StatusReport::update` for an updater status.
pub fn update_label(status: &crate::desktop::host::DesktopUpdateStatus) -> &'static str {
    use crate::desktop::host::DesktopUpdateStatus;
    match status {
        DesktopUpdateStatus::Checking | DesktopUpdateStatus::Downloading => "checking",
        DesktopUpdateStatus::Available
        | DesktopUpdateStatus::ReadyToRestart
        | DesktopUpdateStatus::SwitchAvailable { .. }
        | DesktopUpdateStatus::PackageUpdate { .. } => "available",
        DesktopUpdateStatus::Unavailable
        | DesktopUpdateStatus::Managed
        | DesktopUpdateStatus::Failed => "unavailable",
        DesktopUpdateStatus::Current
        | DesktopUpdateStatus::SourceBuild
        | DesktopUpdateStatus::PackageManager => "current",
    }
}

#[derive(Clone, Debug, Default, Deserialize, PartialEq, Serialize)]
pub struct ListenerReport {
    pub running: bool,
    pub status: String,
    /// What a Wayland shortcuts portal granted, in the desktop's own keys,
    /// including keys changed only in its settings (#723); empty elsewhere.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub shortcuts: Vec<GrantedShortcutReport>,
}

#[derive(Clone, Debug, Default, Deserialize, PartialEq, Serialize)]
pub struct GrantedShortcutReport {
    pub name: String,
    /// `None` when the desktop refused it or names no key.
    pub trigger: Option<String>,
}

#[derive(Clone, Copy, Debug, Default, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum CaptureState {
    #[default]
    Idle,
    Recording,
    Transcribing,
    /// The listener did not answer in time.
    Unknown,
}

/// What is recording.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum CaptureKind {
    Dictation,
    /// A host-audio dictation through the local API (`POST /dictations`).
    Service,
}

#[derive(Clone, Debug, Default, Deserialize, PartialEq, Serialize)]
pub struct CaptureReport {
    pub state: CaptureState,
    pub recording: Option<CaptureKind>,
    /// Accepted dictations still transcribing, processing, or pasting.
    pub pending_jobs: usize,
}

/// The listener's capture state, as it answers a status query.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct CaptureSnapshot {
    pub recording: Option<CaptureKind>,
    pub pending_jobs: usize,
    /// No microphone is open: none was found, or the stream is reopening.
    pub microphone_unavailable: bool,
}

impl From<CaptureSnapshot> for CaptureReport {
    fn from(snapshot: CaptureSnapshot) -> Self {
        Self {
            state: if snapshot.recording.is_some() {
                CaptureState::Recording
            } else if snapshot.pending_jobs > 0 {
                CaptureState::Transcribing
            } else {
                CaptureState::Idle
            },
            recording: snapshot.recording,
            pending_jobs: snapshot.pending_jobs,
        }
    }
}

#[derive(Clone, Debug, Default, Deserialize, PartialEq, Serialize)]
pub struct MeetingReport {
    /// idle, starting, recording, or transcribing.
    pub state: String,
    pub title: Option<String>,
    pub elapsed_seconds: Option<u64>,
}

#[derive(Clone, Debug, Default, Deserialize, PartialEq, Serialize)]
pub struct ModelReport {
    pub id: String,
    pub name: String,
    pub language: String,
    pub installed: bool,
    /// The model being downloaded or loaded, if any.
    pub preparing: Option<String>,
    pub downloaded_bytes: u64,
    pub error: Option<String>,
}

#[derive(Clone, Debug, Default, Deserialize, PartialEq, Serialize)]
pub struct FeatureReport {
    pub meetings: bool,
    pub release_microphone_while_idle: bool,
    pub history_retention: String,
}

#[derive(Clone, Debug, Default, Deserialize, PartialEq, Serialize)]
pub struct PermissionReport {
    /// Every permission granted and the selected model installed.
    pub ready: bool,
    /// ready, needs-request, needs-settings, or needs-restart.
    pub microphone: String,
    pub accessibility: String,
    pub transcription_model: bool,
}

#[derive(Clone, Debug, Default, Deserialize, PartialEq, Serialize)]
pub struct WindowReport {
    pub open: bool,
    pub pane: Option<String>,
}

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
pub struct ModelInfo {
    pub id: String,
    pub name: String,
    pub languages: Vec<String>,
    pub size: String,
    pub installed: bool,
    pub selected: bool,
    pub streams: bool,
    /// How transcription takes the vocabulary: `prompt` (Whisper,
    /// Qwen3-ASR), `bias` (Parakeet, with Labs › Vocabulary biasing), or
    /// `none`.
    pub vocabulary: String,
    /// Deprecated: whether the model reads the vocabulary as its prompt,
    /// which is what recognition hints became. Use `vocabulary`.
    pub supports_recognition_hints: bool,
}

/// Every model `hex model` can name, with what this Mac has installed.
pub fn model_infos(
    selection: &crate::transcription::models::TranscriptionSelection,
) -> Vec<ModelInfo> {
    use crate::transcription::models::VocabularyUse;
    crate::transcription::models::all_models()
        .into_iter()
        .filter(|model| model.available())
        .map(|model| ModelInfo {
            id: model.id.as_str().into(),
            name: model.name.into(),
            languages: model.languages.iter().map(|&code| code.into()).collect(),
            size: model.size_label(),
            installed: crate::transcription::models::is_installed(model, &selection.language),
            selected: model.id == selection.model,
            streams: model.streams,
            vocabulary: match model.vocabulary_use() {
                VocabularyUse::Prompt => "prompt",
                VocabularyUse::Bias => "bias",
                VocabularyUse::None => "none",
            }
            .into(),
            supports_recognition_hints: model.vocabulary_use() == VocabularyUse::Prompt,
        })
        .collect()
}

/// The selection `hex model use` or `install` asks for: a known, available
/// model, and a language it supports (the model's best match for the
/// current language when none is given).
pub fn model_selection(
    settings: &crate::app_settings::AppSettings,
    model: &str,
    language: Option<&str>,
) -> Result<crate::transcription::models::TranscriptionSelection, ControlError> {
    use crate::transcription::custom_models::{self, Status};
    use crate::transcription::models::TranscriptionModelId;
    let parsed = model.parse::<TranscriptionModelId>().ok();
    // A known import says why it can't be used rather than that it's unknown.
    if let Some(TranscriptionModelId::Custom(id)) = parsed
        && custom_models::is_registered(id)
    {
        let why = match custom_models::status(id) {
            Status::Ready => None,
            Status::Missing => Some("its file is missing; import it again or remove it"),
            Status::Changed => {
                Some("its file changed since it was imported; import it again or remove it")
            }
            Status::Off => {
                Some("turn on Labs › Use your own speech model: hex labs on custom-models")
            }
        };
        if let Some(why) = why {
            return Err(ControlError::new(
                "model-unavailable",
                format!("`{model}` can't be used: {why}"),
            ));
        }
    }
    let id = parsed
        .filter(|id| crate::transcription::models::definition(*id).available())
        .ok_or_else(|| {
            ControlError::new(
                "unknown-model",
                format!("unknown model `{model}`; run `hex model list` for the ids"),
            )
        })?;
    // The model's own remembered language, as the menu bar picks it; only
    // the language can be overridden.
    let mut selection = settings.transcription_for_model(id);
    if let Some(language) = language {
        selection.language = language.into();
    }
    let definition = crate::transcription::models::definition(id);
    crate::transcription::models::validate(&selection).map_err(|error| {
        let supported = if definition.languages.contains(&"*") {
            "any language code Hex knows (en, de, fr, ja, …)".to_owned()
        } else {
            definition.languages.join(", ")
        };
        ControlError::new(
            "invalid-value",
            format!("{error}; {} supports {supported}", definition.name),
        )
    })?;
    Ok(selection)
}

/// Refuses to stop Hex while it records, unless forced. `capture` is the
/// listener's answer (idle when no listener runs or setup gates it); a
/// listener that couldn't answer might be recording, so it refuses too.
pub fn check_quit(
    meeting: &MeetingReport,
    capture: Result<CaptureSnapshot, ControlError>,
    force: bool,
) -> Result<(), ControlError> {
    if force {
        return Ok(());
    }
    if meeting.state != "idle" {
        return Err(ControlError::new(
            "meeting-active",
            format!(
                "a meeting is {}; stop it first (`hex meetings stop`) or pass --force",
                meeting.state
            ),
        ));
    }
    match capture {
        Ok(capture) if capture.recording.is_some() || capture.pending_jobs > 0 => {
            Err(ControlError::new(
                "busy",
                "a dictation is recording or transcribing; wait for it or pass --force",
            ))
        }
        Ok(_) => Ok(()),
        Err(error) => Err(ControlError::new(
            "busy",
            format!(
                "couldn't confirm nothing is recording ({}); try again or pass --force",
                error.message
            ),
        )),
    }
}

/// Carries out `hex vocabulary add|remove` on the saved list. The reply's
/// `changed` says whether the list needs saving.
pub fn edit_vocabulary(
    list: &mut Vec<VocabularyEntry>,
    command: &ControlCommand,
) -> Result<ControlReply, ControlError> {
    let invalid = |message: String| ControlError::new("invalid-value", message);
    match command {
        ControlCommand::VocabularyAdd { term, sounds_like } => {
            let sounds_like: Vec<String> = sounds_like
                .iter()
                .flat_map(|spellings| vocabulary::parse_sounds_like(spellings))
                .collect();
            let existed = vocabulary::contains(list, term);
            Ok(
                match vocabulary::add(list, term, &sounds_like).map_err(invalid)? {
                    false => ControlReply::unchanged(format!(
                        "“{}” is already in the vocabulary",
                        term.trim()
                    )),
                    true if existed => {
                        ControlReply::done(format!("Added spellings to “{}”", term.trim()))
                    }
                    true => ControlReply::done(format!("Added “{}”", term.trim())),
                },
            )
        }
        ControlCommand::VocabularyRemove { term } => Ok(if vocabulary::remove(list, term) {
            ControlReply::done(format!("Removed “{}”", term.trim()))
        } else {
            ControlReply::unchanged(format!("“{}” isn't in the vocabulary", term.trim()))
        }),
        _ => Err(ControlError::new("internal", "not a vocabulary edit")),
    }
}

/// Process memory footprint on macOS; `None` where unmeasured.
pub fn memory_footprint() -> Option<u64> {
    #[cfg(target_os = "macos")]
    {
        // SAFETY: rusage_info_v4 is plain data that proc_pid_rusage fills.
        let mut info = unsafe { std::mem::zeroed::<libc::rusage_info_v4>() };
        // SAFETY: the pointer refers to a live, correctly sized buffer.
        let status = unsafe {
            libc::proc_pid_rusage(
                std::process::id() as i32,
                libc::RUSAGE_INFO_V4,
                (&mut info as *mut libc::rusage_info_v4).cast(),
            )
        };
        (status == 0).then_some(info.ri_phys_footprint)
    }
    #[cfg(not(target_os = "macos"))]
    {
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_printing_stop_answers_through_its_reply_channel() {
        let wait = Duration::from_millis(50);
        let (reply, printed) = PrintReply::channel();
        reply.send(PrintedTranscript::Text("Ship it.".into()));
        assert_eq!(
            await_printed(&printed, wait),
            ControlReply::Transcript {
                text: "Ship it.".into()
            }
        );
        let (reply, printed) = PrintReply::channel();
        reply.send(PrintedTranscript::NoSpeech);
        assert!(matches!(
            await_printed(&printed, wait),
            ControlReply::Error { code, .. } if code == "no-speech"
        ));
        // A dictation cancelled or failed before it had text drops the reply.
        let (reply, printed) = PrintReply::channel();
        drop(reply);
        assert!(matches!(
            await_printed(&printed, wait),
            ControlReply::Error { code, .. } if code == "failed"
        ));
        let (_reply, printed) = PrintReply::channel();
        assert!(matches!(
            await_printed(&printed, wait),
            ControlReply::Error { code, .. } if code == "failed"
        ));
        // Only `stop` prints, and older requests without the field still parse.
        assert!(check_flags(CaptureAction::Start, true, false).is_err());
        assert!(check_flags(CaptureAction::Stop, true, false).is_ok());
        assert!(check_flags(CaptureAction::Toggle, false, true).is_ok());
        assert!(check_flags(CaptureAction::Stop, false, true).is_err());
        assert_eq!(
            serde_json::from_str::<ControlRequest>(
                r#"{"version":4,"command":"dictation","action":"stop"}"#
            )
            .unwrap()
            .command,
            ControlCommand::Dictation {
                action: CaptureAction::Stop,
                print: false,
                raw: false,
            }
        );
    }

    fn round_trip_request(command: ControlCommand) {
        let request = ControlRequest::new(command);
        let json = serde_json::to_string(&request).unwrap();
        assert_eq!(
            serde_json::from_str::<ControlRequest>(&json).unwrap(),
            request
        );
    }

    #[test]
    fn every_command_round_trips() {
        for command in [
            ControlCommand::Status,
            ControlCommand::Quit { force: true },
            ControlCommand::Restart { force: false },
            ControlCommand::Dictation {
                action: CaptureAction::Toggle,
                print: false,
                raw: false,
            },
            ControlCommand::Dictation {
                action: CaptureAction::Stop,
                print: true,
                raw: false,
            },
            ControlCommand::Dictation {
                action: CaptureAction::Start,
                print: false,
                raw: true,
            },
            ControlCommand::PasteLast,
            ControlCommand::MeetingRecord {
                title: Some("Design sync".into()),
            },
            ControlCommand::MeetingStop,
            ControlCommand::MeetingCopyLast,
            ControlCommand::SettingsGet { key: None },
            ControlCommand::SettingsSet {
                key: "release_microphone_while_idle".into(),
                value: "on".into(),
            },
            ControlCommand::SettingsApply {
                values: serde_json::Map::from_iter([("sound_effects".into(), false.into())]),
            },
            ControlCommand::ModelList,
            ControlCommand::ModelUse {
                model: "parakeet_v3".into(),
                language: Some("pl".into()),
            },
            ControlCommand::ModelInstall {
                model: "parakeet_v2".into(),
                language: None,
            },
            ControlCommand::ModelCancel,
            ControlCommand::WindowShow {
                pane: Some(Pane::History),
            },
            ControlCommand::WindowHide,
            ControlCommand::OnboardingReset,
            ControlCommand::WindowStory {
                story: "HUD/Processing".into(),
                variant: Some("2".into()),
                play: true,
            },
            ControlCommand::HistoryClear,
            ControlCommand::CheckForUpdates,
            ControlCommand::LaunchAtLogin {
                enabled: Some(true),
            },
            ControlCommand::VocabularyList,
            ControlCommand::VocabularyAdd {
                term: "cappuccino".into(),
                sounds_like: vec!["capuchin".into()],
            },
            ControlCommand::VocabularyRemove {
                term: "cappuccino".into(),
            },
        ] {
            round_trip_request(command);
        }
        let request: ControlRequest =
            serde_json::from_str(r#"{"version":4,"command":"vocabulary-add","term":"Rune"}"#)
                .unwrap();
        assert_eq!(
            request.command,
            ControlCommand::VocabularyAdd {
                term: "Rune".into(),
                sounds_like: vec![],
            }
        );
    }

    #[test]
    fn vocabulary_edits_report_whether_they_changed_the_list() {
        let add = |term: &str, sounds_like: &[&str]| ControlCommand::VocabularyAdd {
            term: term.into(),
            sounds_like: sounds_like.iter().map(|&text| text.into()).collect(),
        };
        let remove = |term: &str| ControlCommand::VocabularyRemove { term: term.into() };
        let mut list = Vec::new();
        assert_eq!(
            edit_vocabulary(&mut list, &add("cappuccino", &["capuchin, cap a chino"])).unwrap(),
            ControlReply::done("Added “cappuccino”")
        );
        assert_eq!(list[0].sounds_like, ["capuchin", "cap a chino"]);
        assert_eq!(
            edit_vocabulary(&mut list, &add("Cappuccino", &[])).unwrap(),
            ControlReply::unchanged("“Cappuccino” is already in the vocabulary")
        );
        assert_eq!(
            edit_vocabulary(&mut list, &add("cappuccino", &["cap you chino"])).unwrap(),
            ControlReply::done("Added spellings to “cappuccino”")
        );
        assert_eq!(
            edit_vocabulary(&mut list, &add(" ", &[])).unwrap_err().code,
            "invalid-value"
        );
        assert_eq!(
            edit_vocabulary(&mut list, &remove("CAPPUCCINO")).unwrap(),
            ControlReply::done("Removed “CAPPUCCINO”")
        );
        assert_eq!(
            edit_vocabulary(&mut list, &remove("cappuccino")).unwrap(),
            ControlReply::unchanged("“cappuccino” isn't in the vocabulary")
        );
        assert!(list.is_empty());
    }

    #[test]
    fn requests_are_flat_and_versioned() {
        let json = serde_json::to_value(ControlRequest::new(ControlCommand::MeetingRecord {
            title: Some("Standup".into()),
        }))
        .unwrap();
        assert_eq!(
            json,
            serde_json::json!({"version": 4, "command": "meeting-record", "title": "Standup"})
        );
        // Optional fields may be left out.
        let request: ControlRequest =
            serde_json::from_str(r#"{"version":4,"command":"quit"}"#).unwrap();
        assert_eq!(request.command, ControlCommand::Quit { force: false });
        assert!(serde_json::from_str::<ControlRequest>(r#"{"version":4,"command":"x"}"#).is_err());
        let capture: ControlRequest =
            serde_json::from_str(r#"{"version":4,"command":"scrawl-capture","action":"stop"}"#)
                .unwrap();
        assert_eq!(
            capture.command,
            ControlCommand::ScrawlCapture {
                action: ScrawlCaptureAction::Stop
            }
        );
        let send: ControlRequest = serde_json::from_str(
            r#"{"version":4,"command":"scrawl-capture-send","capture":"latest"}"#,
        )
        .unwrap();
        assert_eq!(
            send.command,
            ControlCommand::ScrawlCaptureSend {
                capture: "latest".into(),
                to: None,
                always: false,
            }
        );
    }

    #[test]
    fn every_reply_round_trips() {
        for reply in [
            ControlReply::done("Started dictation"),
            ControlReply::unchanged("Already recording"),
            ControlReply::Status(Box::default()),
            ControlReply::Settings {
                settings: vec![settings::SettingValue {
                    key: "sound_effects".into(),
                    value: serde_json::json!(true),
                    kind: "bool".into(),
                    choices: vec![],
                    writable: true,
                    about: "Play tones".into(),
                    available: true,
                }],
            },
            ControlReply::Models { models: vec![] },
            ControlReply::Vocabulary {
                entries: vec![VocabularyEntry {
                    term: "cappuccino".into(),
                    sounds_like: vec!["capuchin".into()],
                }],
            },
            ControlReply::ScrawlCapture(Box::new(ScrawlCaptureReport {
                changed: true,
                state: ScrawlCaptureState::Idle,
                bundle: Some("/tmp/1.capture".into()),
                elapsed_ms: None,
                finishing: vec!["/tmp/1.capture".into()],
                last: Some(ScrawlCaptureOutcome {
                    bundle: "/tmp/0.capture".into(),
                    stage: "done".into(),
                    words: Some(3),
                    gaps: Some(1),
                    missing_transcript: None,
                }),
            })),
            ControlReply::ScrawlCaptures {
                captures: vec![ScrawlCaptureSummary {
                    id: "1.scrawl".into(),
                    sent_to: Some(ScrawlCaptureSession {
                        session: "ses_1".into(),
                        title: "Hex".into(),
                    }),
                    ..ScrawlCaptureSummary::default()
                }],
            },
            ControlReply::ScrawlCaptureDetail(Box::new(ScrawlCaptureDetail {
                summary: ScrawlCaptureSummary {
                    id: "1.scrawl".into(),
                    words: 3,
                    ..ScrawlCaptureSummary::default()
                },
                transcript: "make it blue".into(),
                video: Some("/tmp/1.scrawl/video.mp4".into()),
                ..ScrawlCaptureDetail::default()
            })),
            ControlReply::ScrawlCaptureRoute {
                explanation: "Capture   1.scrawl".into(),
            },
            ControlReply::Transcript {
                text: "Ship it.".into(),
            },
            ControlReply::error("busy", "Hex is busy"),
        ] {
            let response = ControlResponse::from(reply);
            let json = serde_json::to_string(&response).unwrap();
            assert_eq!(
                serde_json::from_str::<ControlResponse>(&json).unwrap(),
                response
            );
        }
        let json = serde_json::to_value(ControlResponse::from(ControlReply::Transcript {
            text: "Ship it.".into(),
        }))
        .unwrap();
        assert_eq!(
            json,
            serde_json::json!({"version": 4, "status": "transcript", "text": "Ship it."})
        );
        let json =
            serde_json::to_value(ControlResponse::from(ControlReply::error("busy", "x"))).unwrap();
        assert_eq!(
            json,
            serde_json::json!({"version": 4, "status": "error", "code": "busy", "message": "x"})
        );
    }

    #[test]
    fn a_request_from_another_version_is_refused() {
        let mut request = ControlRequest::new(ControlCommand::Status);
        assert!(check_version(&request).is_ok());
        request.version = 99;
        assert_eq!(
            check_version(&request).unwrap_err().code,
            "version-mismatch"
        );
    }

    #[test]
    fn quitting_waits_for_recordings_unless_forced() {
        let idle = MeetingReport {
            state: "idle".into(),
            ..Default::default()
        };
        let recording = MeetingReport {
            state: "recording".into(),
            ..Default::default()
        };
        let quiet = || Ok(CaptureSnapshot::default());
        assert!(check_quit(&idle, quiet(), false).is_ok());
        assert_eq!(
            check_quit(&recording, quiet(), false).unwrap_err().code,
            "meeting-active"
        );
        assert!(check_quit(&recording, quiet(), true).is_ok());
        let busy = CaptureSnapshot {
            recording: None,
            pending_jobs: 1,
            microphone_unavailable: false,
        };
        assert_eq!(check_quit(&idle, Ok(busy), false).unwrap_err().code, "busy");
        // A listener that can't answer may be recording: only --force quits.
        let unanswered = || {
            Err(ControlError::new(
                "unavailable",
                "the listener didn't answer",
            ))
        };
        assert_eq!(
            check_quit(&idle, unanswered(), false).unwrap_err().code,
            "busy"
        );
        assert!(check_quit(&idle, unanswered(), true).is_ok());
    }

    #[test]
    fn model_selection_keeps_the_remembered_language_and_validates_it() {
        use crate::transcription::models::{TranscriptionModelId as Id, TranscriptionSelection};
        let settings = crate::app_settings::AppSettings {
            transcription: TranscriptionSelection {
                model: Id::ParakeetV3,
                language: "en".into(),
            },
            transcription_recents: vec![TranscriptionSelection {
                model: Id::WhisperLargeV3Turbo,
                language: "de".into(),
            }],
            ..Default::default()
        };
        let pick = |model, language| model_selection(&settings, model, language);
        let remembered = pick("whisper_large_v3_turbo", None).unwrap();
        assert_eq!(remembered.language, "de");
        let french = pick("whisper_large_v3_turbo", Some("fr")).unwrap();
        assert_eq!(french.language, "fr");
        // Whisper's wildcard takes any language Hex knows, not any string.
        for (model, language) in [
            ("whisper_large_v3_turbo", "klingon"),
            ("sense_voice_small", "de"),
        ] {
            assert_eq!(
                pick(model, Some(language)).unwrap_err().code,
                "invalid-value",
                "{model} {language}"
            );
        }
        assert_eq!(
            pick("apple_speech", None).unwrap_err().code,
            "unknown-model"
        );
    }

    #[test]
    fn capture_reports_derive_their_state() {
        let report = CaptureReport::from(CaptureSnapshot {
            recording: Some(CaptureKind::Dictation),
            pending_jobs: 2,
            microphone_unavailable: false,
        });
        assert_eq!(report.state, CaptureState::Recording);
        let report = CaptureReport::from(CaptureSnapshot {
            recording: None,
            pending_jobs: 2,
            microphone_unavailable: true,
        });
        assert_eq!(report.state, CaptureState::Transcribing);
    }
}
