use std::collections::{BTreeMap, VecDeque};
use std::path::Path;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{self, Receiver, SyncSender, TryRecvError, TrySendError};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use color_eyre::Result;

use crate::audio::CaptureInstant;
use crate::commands::{Action, ActionExecutor, ActionOutcome, CommandConfig, Decision, Mode};
use crate::context::{ContextMonitor, ContextSnapshot};
use crate::control::{CaptureAction, CaptureKind, CaptureSnapshot, ControlReply, PrintReply};
use crate::dictation::{
    ControlStability, DictationClip, DictationControl, DictationProtocol, Finish,
    MINIMUM_HOLD_DURATION, POST_RELEASE_TAIL,
};
use crate::dictation_audio::{DictationAudio, DictationAudioEvent, LostCapture};
use crate::dictation_indicator::{DictationIndicatorEvent, DictationIndicatorSender};
use crate::events::{
    CommandOutcome, DictationPhase, EventLog, TranscriptPhase, VoiceEvent, VoiceState, now_ms,
};
use crate::feedback::{self, Tone};
use crate::meeting::MeetingRequest;
use crate::moonshine::Moonshine;
use crate::parakeet::{
    DictationJobId, DictationJobStage, DictationWorker, PasteKind, TranscriptionTarget, WorkerEvent,
};
use crate::recording_environment::RecordingEnvironmentController;
use crate::spoken_text::normalize;
use crate::suppression::{
    DictationHotkey, DiscardCause, HotkeyAction, InputActivity, InputEvent, InputMonitor,
};

const UPDATE_INTERVAL: Duration = Duration::from_millis(200);
const ACTIVATION_STABILITY_WINDOW: Duration = Duration::from_millis(750);
const PROGRAMMATIC_LEASE: Duration = Duration::from_secs(10);
const PROGRAMMATIC_COMPLETION_LIMIT: usize = 16;
/// `/finish` on a programmatic capture the microphone ended before any audio.
const MICROPHONE_LOST: &str = "microphone-lost";

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum CommandModelStatus {
    Disabled,
    Loading,
    Ready,
    Failed,
}

static COMMAND_MODEL_STATUS: Mutex<CommandModelStatus> = Mutex::new(CommandModelStatus::Disabled);

pub fn command_model_status() -> CommandModelStatus {
    *COMMAND_MODEL_STATUS
        .lock()
        .unwrap_or_else(|error| error.into_inner())
}

fn set_command_model_status(status: CommandModelStatus) {
    *COMMAND_MODEL_STATUS
        .lock()
        .unwrap_or_else(|error| error.into_inner()) = status;
}

#[derive(Default)]
struct ActivationStability {
    pending: Option<(String, Instant)>,
}

impl ActivationStability {
    fn observe(
        &mut self,
        protocol: &DictationProtocol,
        text: &str,
        completed: bool,
        now: Instant,
    ) -> bool {
        let Some(prefix_end) = protocol.start_prefix(text) else {
            self.pending = None;
            return false;
        };
        let phrase = normalize(&text[..prefix_end]);
        let stable = self.pending.as_ref().is_some_and(|(pending, observed_at)| {
            pending == &phrase
                && now.saturating_duration_since(*observed_at) <= ACTIVATION_STABILITY_WINDOW
        });
        let accepted = completed || stable;
        self.pending = (!accepted).then_some((phrase, now));
        accepted
    }

    fn reset(&mut self) {
        self.pending = None;
    }
}

enum FinishResult {
    Discarded,
    Silent,
    Submitted,
    Rejected(String),
}

#[derive(Debug)]
pub enum RecognitionControl {
    /// Pastes the last dictation once nothing records; `reply` answers
    /// `hex paste-last`, and the menu bar sends none.
    PasteLast {
        reply: Option<SyncSender<ControlReply>>,
    },
    RetryCommands,
    /// `hex dictation` and `hex voice-action`: drives the shortcut machine
    /// as a press would, so the shortcut and Escape still finish or cancel.
    /// A dictation `stop` with `print` answers `reply` once recording stops
    /// and sends the transcript to `print` instead of pasting it.
    Capture {
        voice_action: bool,
        action: CaptureAction,
        print: Option<PrintReply>,
        reply: SyncSender<ControlReply>,
    },
    /// What is recording, for `hex status`.
    CaptureStatus {
        reply: SyncSender<CaptureSnapshot>,
    },
    /// `hex capture` (the `capture` experiment; see `capture_host`).
    #[cfg(hex_capture)]
    ScrawlCapture {
        action: crate::control::ScrawlCaptureAction,
        reply: SyncSender<ControlReply>,
    },
    /// The capture capsule's switch chips and picker (`capture_host`).
    #[cfg(hex_capture)]
    CaptureCommand {
        command: crate::capture_host::CaptureCommand,
        reply: SyncSender<ControlReply>,
    },
    StartDictation {
        source: String,
        owner_token: String,
        levels: SyncSender<DictationLevel>,
        reply: SyncSender<Result<ProgrammaticDictationStart, String>>,
    },
    AttachDictationAudio {
        id: u64,
        owner_token: String,
        audio: SyncSender<DictationAudioChunk>,
        reply: SyncSender<Result<(), String>>,
    },
    FinishDictation {
        id: u64,
        owner_token: String,
        reply: SyncSender<Result<ProgrammaticDictationResult, String>>,
    },
    CancelDictation {
        id: u64,
        owner_token: String,
        reply: SyncSender<Result<(), String>>,
    },
    HeartbeatDictation {
        id: u64,
        owner_token: String,
        reply: SyncSender<Result<(), String>>,
    },
}

#[derive(Clone, Copy, Debug)]
pub struct DictationLevel {
    pub rms_db: f32,
    pub peak_db: f32,
}

#[derive(Clone, Debug)]
pub struct ProgrammaticDictationResult {
    pub transcript: String,
    pub duration_ms: u64,
}

#[derive(Debug)]
pub struct ProgrammaticDictationStart {
    pub id: u64,
    pub sample_rate: u32,
}

#[derive(Debug)]
pub struct DictationAudioChunk {
    pub samples: Vec<f32>,
}

struct ProgrammaticDictation {
    id: u64,
    owner_token: String,
    source: String,
    levels: SyncSender<DictationLevel>,
    audio: Option<SyncSender<DictationAudioChunk>>,
    last_level: Instant,
    lease_deadline: Instant,
}

impl ProgrammaticDictation {
    fn matches_owner(&self, id: u64, owner_token: &str) -> bool {
        self.id == id && self.owner_token == owner_token
    }
}

/// A programmatic capture the microphone cut short: the audio delivered
/// before the loss, waiting for its owner's `/finish` or `/cancel` while
/// heartbeats renew its lease.
struct LostProgrammaticDictation {
    id: u64,
    owner_token: String,
    source: String,
    lease_deadline: Instant,
    clip: DictationClip,
}

impl LostProgrammaticDictation {
    fn matches_owner(&self, id: u64, owner_token: &str) -> bool {
        self.id == id && self.owner_token == owner_token
    }
}

struct FinishingProgrammaticDictation {
    id: u64,
    source: String,
    owner_token: String,
    duration_ms: u64,
    replies: Vec<SyncSender<Result<ProgrammaticDictationResult, String>>>,
}

impl FinishingProgrammaticDictation {
    fn matches_owner(&self, id: u64, owner_token: &str) -> bool {
        self.id == id && self.owner_token == owner_token
    }
}

#[derive(Clone)]
enum ProgrammaticTerminalOutcome {
    Finished(Result<ProgrammaticDictationResult, String>),
    Cancelled,
}

#[derive(Clone)]
struct ProgrammaticCompletion {
    id: u64,
    owner_token: String,
    outcome: ProgrammaticTerminalOutcome,
}

impl ProgrammaticCompletion {
    fn matches_owner(&self, id: u64, owner_token: &str) -> bool {
        self.id == id && self.owner_token == owner_token
    }
}

/// A voice-delimited dictation capture: the protocol that started it and
/// the stability tracker for its control phrases. Dropping the value is the
/// whole cancellation.
struct VoiceCapture {
    protocol: Arc<DictationProtocol>,
    stability: ControlStability,
    /// Where the start phrase ended: Smart Turn listens from here.
    started_at: CaptureInstant,
}

/// Where the current Moonshine stream's audio began. Every owner-side
/// generation advance makes it stale; readers filter instead of clearing.
#[derive(Clone, Copy)]
struct RecognitionOrigin {
    generation: u64,
    at: CaptureInstant,
}

impl RecognitionOrigin {
    fn for_generation(&self, generation: u64) -> Option<CaptureInstant> {
        (self.generation == generation).then_some(self.at)
    }
}

/// The dictation shortcut machine plus the optional Voice Action machine.
/// `edit` stays `None` until Voice Action is enabled so no shortcut is reserved.
struct Hotkeys {
    dictation: DictationHotkey,
    edit: Option<DictationHotkey>,
}

impl Hotkeys {
    fn any_recording(&self) -> bool {
        self.dictation.is_recording() || self.edit_recording()
    }

    fn edit_recording(&self) -> bool {
        self.edit
            .as_ref()
            .is_some_and(DictationHotkey::is_recording)
    }

    fn suppresses_recognition(&self) -> bool {
        self.dictation.suppresses_recognition()
            || self
                .edit
                .as_ref()
                .is_some_and(DictationHotkey::suppresses_recognition)
    }

    /// Suspends both machines and reports whether either was recording.
    fn suspend_all(&mut self) -> bool {
        let dictation_recording = self.dictation.suspend().is_some();
        let edit_recording = self.suspend_edit();
        dictation_recording || edit_recording
    }

    fn suspend_edit(&mut self) -> bool {
        self.edit
            .as_mut()
            .and_then(DictationHotkey::suspend)
            .is_some()
    }

    fn disarm_pending_gestures(&mut self) {
        self.dictation.disarm_pending_gesture();
        if let Some(edit) = &mut self.edit {
            edit.disarm_pending_gesture();
        }
    }

    fn track_key_state(&mut self, event: InputEvent, at: CaptureInstant) {
        self.dictation.track_key_state(event, at);
        if let Some(edit) = &mut self.edit {
            edit.track_key_state(event, at);
        }
    }

    fn recover_stale_keys(&mut self) {
        self.dictation.recover_stale_keys();
        if let Some(edit) = &mut self.edit {
            edit.recover_stale_keys();
        }
    }

    fn break_chords(&mut self, now: CaptureInstant) {
        self.dictation.break_chord(now);
        if let Some(edit) = &mut self.edit {
            edit.break_chord(now);
        }
    }

    fn forgo_chord_breaks(&mut self) {
        self.dictation.forgo_chord_break();
        if let Some(edit) = &mut self.edit {
            edit.forgo_chord_break();
        }
    }
}

#[allow(clippy::too_many_arguments)]
pub fn listen(
    project_root: &Path,
    events: EventLog,
    device_override: Option<&str>,
    commands: CommandConfig,
    shutdown: &AtomicBool,
    indicator: Option<DictationIndicatorSender>,
    meeting_requests: Option<SyncSender<MeetingRequest>>,
    history: Option<crate::history::History>,
    controls: Option<Receiver<RecognitionControl>>,
) -> Result<()> {
    let commands = Arc::new(commands);
    let native_runtime = Arc::new(crate::personal_commands::RuntimeSnapshot::native(
        commands.as_ref().clone(),
    ));
    let transformations = Arc::new(crate::personal_commands::TransformationClient::default());
    shutdown.store(false, Ordering::Relaxed);
    // Feedback admission is advisory: a cold audio stack can exceed the
    // preload timeout, and missing tones must not stop desktop recognition.
    if let Err(error) = feedback::preload() {
        tracing::warn!(%error, "recording sounds are unavailable; continuing without feedback");
    }
    let mut microphone_policy = crate::app_settings::microphone_policy();
    let mut commands_enabled = microphone_policy.commands_enabled;
    let mut recognizer = None;
    set_command_model_status(CommandModelStatus::Disabled);
    let mut command_loader =
        commands_enabled.then(|| load_command_recognizer(project_root.to_path_buf()));
    let mut events = events;
    let input_monitor = InputMonitor::start()?;
    let context_monitor = ContextMonitor::start();
    let mut context = ContextSnapshot::default();
    let mut hotkeys = Hotkeys {
        dictation: DictationHotkey::new(
            CaptureInstant::now(),
            crate::app_settings::double_tap_lock(),
            crate::app_settings::dictation_hotkey(),
        ),
        edit: crate::app_settings::edit_hotkey()
            .map(|binding| DictationHotkey::new_without_paste(CaptureInstant::now(), binding)),
    };
    hotkeys
        .dictation
        .set_double_tap_only(crate::app_settings::double_tap_only());
    hotkeys
        .dictation
        .set_toggle(crate::app_settings::shortcut_toggle());
    let mut edit_context = None;
    let mut edit_pending_since = None;
    let mut mode = Mode::Listening;
    let mut voice: Option<VoiceCapture> = None;
    let mut activation_stability = ActivationStability::default();
    let mut last_update = Instant::now();
    let mut recognition_origin = None::<RecognitionOrigin>;
    let recording_environment = RecordingEnvironmentController::start();
    #[cfg(hex_capture)]
    let capture_environment = recording_environment.clone();
    let (mut microphone_revision, microphone) = crate::app_settings::microphone_selection();
    let dictation_worker = DictationWorker::start(
        input_monitor.activity.clone(),
        transformations.clone(),
        history,
    );
    let input = DictationAudio::open(
        device_override,
        microphone_revision,
        microphone.as_deref(),
        recording_environment,
        input_monitor.pending_events(),
        microphone_policy.release_while_idle,
        dictation_worker.engine(),
    )?;
    #[cfg(hex_capture)]
    let mut capture_host =
        crate::capture_host::CaptureHost::start(dictation_worker.engine(), &capture_environment);
    #[cfg(hex_capture)]
    let mut capture_shortcut = crate::capture_host::CaptureShortcut::default();
    let (mut transcription_revision, _) = crate::app_settings::transcription_selection();
    let mut smart_turn = crate::smart_turn::SmartTurn::start(project_root);
    let mut action_executor: Option<ActionExecutor> = None;
    let mut personal_host_enabled =
        commands_enabled || crate::app_settings::custom_transformations_enabled();
    let mut personal_commands = personal_host_enabled
        .then(|| {
            crate::personal_commands::PersonalCommands::start(
                commands.as_ref().clone(),
                transformations.clone(),
            )
        })
        .flatten();
    let mut next_programmatic_id = 1_u64;
    let mut programmatic = None::<ProgrammaticDictation>;
    let mut programmatic_lost = None::<LostProgrammaticDictation>;
    let mut programmatic_results =
        BTreeMap::<crate::parakeet::DictationJobId, FinishingProgrammaticDictation>::new();
    let mut programmatic_completions = VecDeque::<ProgrammaticCompletion>::new();

    events.emit(&VoiceEvent::SessionStarted {
        timestamp_ms: now_ms(),
    })?;
    if input.is_recovering() {
        hotkeys.suspend_all();
    }
    if hotkeys.dictation.is_recording() {
        let accepted = input.start(capture_boundary(
            &input,
            microphone_policy.release_while_idle,
        ))?;
        if accepted {
            events.dictation(DictationPhase::Started, "")?;
            if let Some(indicator) = &indicator {
                indicator.send(DictationIndicatorEvent::Started);
            }
        } else {
            hotkeys.dictation.suspend();
        }
    }
    if hotkeys.edit_recording() {
        let accepted = input.start(capture_boundary(
            &input,
            microphone_policy.release_while_idle,
        ))?;
        if accepted {
            edit_context = Some(voice_action_context_snapshot(&context));
            events.dictation(DictationPhase::Started, "")?;
            if let Some(indicator) = &indicator {
                indicator.send(DictationIndicatorEvent::EditingStarted);
            }
        } else {
            hotkeys.suspend_edit();
        }
    }
    emit_state(
        &mut events,
        hotkeys.any_recording(),
        mode,
        &input.device_name(),
    )?;
    if commands_enabled {
        tracing::info!(device = %input.device_name(), "voice recognition started");
    } else {
        tracing::info!(device = %input.device_name(), "dictation listener started");
    }

    while !shutdown.load(Ordering::Relaxed) {
        #[cfg(hex_capture)]
        crate::capture_host::CaptureHost::reconcile(
            &mut capture_host,
            dictation_worker.engine().as_ref(),
            &capture_environment,
            &input,
        );
        if let Some(controls) = &controls {
            while let Ok(control) = controls.try_recv() {
                match control {
                    RecognitionControl::RetryCommands => {
                        if crate::app_settings::commands_enabled()
                            && command_loader.is_none()
                            && recognizer.is_none()
                        {
                            command_loader =
                                Some(load_command_recognizer(project_root.to_path_buf()));
                        }
                    }
                    RecognitionControl::PasteLast { reply } => {
                        let capture_active = programmatic.is_some()
                            || voice.is_some()
                            || hotkeys.suppresses_recognition()
                            || edit_context.is_some()
                            || edit_pending_since.is_some()
                            || input.is_recording();
                        let answer = match paste_last_refusal(capture_active) {
                            Some(refusal) => refusal,
                            None => paste_last(
                                &dictation_worker,
                                mode,
                                &input.device_name(),
                                &mut events,
                            )?,
                        };
                        if let Some(reply) = reply {
                            let _ = reply.send(answer);
                        }
                    }
                    #[cfg(hex_capture)]
                    RecognitionControl::ScrawlCapture { action, reply } => {
                        let answer = match capture_host.as_mut() {
                            Some(host) => host.control(action, &input),
                            None => ControlReply::error(
                                "feature-off",
                                "Annotate is off; turn it on in Settings › Annotate or \
                                 run `hex settings set capture.enabled true`",
                            ),
                        };
                        let _ = reply.send(answer);
                    }
                    #[cfg(hex_capture)]
                    RecognitionControl::CaptureCommand { command, reply } => {
                        let answer = match capture_host.as_mut() {
                            Some(host) => host.command(command, &input),
                            None => ControlReply::error("feature-off", "captures are off"),
                        };
                        let _ = reply.send(answer);
                    }
                    RecognitionControl::CaptureStatus { reply } => {
                        let recording = if programmatic.is_some() {
                            Some(CaptureKind::Service)
                        } else if voice.is_some() {
                            Some(CaptureKind::Voice)
                        } else if hotkeys.edit_recording() || edit_context.is_some() {
                            Some(CaptureKind::VoiceAction)
                        } else if hotkeys.dictation.is_recording() {
                            Some(CaptureKind::Dictation)
                        } else {
                            None
                        };
                        let _ = reply.send(CaptureSnapshot {
                            recording,
                            pending_jobs: dictation_worker.pending_count(),
                            microphone_unavailable: input.is_recovering(),
                        });
                    }
                    RecognitionControl::Capture {
                        voice_action,
                        action,
                        print,
                        reply,
                    } => {
                        let release_while_idle = microphone_policy.release_while_idle;
                        let other_recording = programmatic.is_some()
                            || voice.is_some()
                            || if voice_action {
                                hotkeys.dictation.is_recording()
                            } else {
                                hotkeys.edit_recording()
                                    || edit_context.is_some()
                                    || edit_pending_since.is_some()
                            };
                        let suspended =
                            crate::app_settings::hotkey_capture_active() || input.is_recovering();
                        let outcome = if voice_action {
                            control_voice_action(
                                action,
                                other_recording,
                                suspended,
                                &mut hotkeys,
                                &mut edit_pending_since,
                                &mut edit_context,
                                &mut recognizer,
                                &input,
                                &dictation_worker,
                                mode,
                                &context,
                                &mut events,
                                indicator.as_ref(),
                                release_while_idle,
                            )?
                        } else {
                            control_dictation(
                                action,
                                print,
                                other_recording,
                                suspended,
                                &mut hotkeys,
                                &mut recognizer,
                                &input,
                                &dictation_worker,
                                mode,
                                &context,
                                &mut events,
                                indicator.as_ref(),
                                release_while_idle,
                            )?
                        };
                        let _ = reply.send(outcome);
                    }
                    RecognitionControl::StartDictation {
                        source,
                        owner_token,
                        levels,
                        reply,
                    } => {
                        if programmatic.is_some()
                            || input.is_recording()
                            || hotkeys.any_recording()
                            || voice.is_some()
                        {
                            let _ = reply.send(Err("dictation-busy".into()));
                            continue;
                        }
                        let id = next_programmatic_id;
                        next_programmatic_id = next_programmatic_id.wrapping_add(1).max(1);
                        let boundary =
                            capture_boundary(&input, microphone_policy.release_while_idle);
                        match input.start_programmatic(boundary) {
                            Ok(()) => {
                                hotkeys.disarm_pending_gestures();
                                reset_command_recognizer(&input, &mut recognizer)?;
                                tracing::info!(
                                    dictation_id = id,
                                    source,
                                    "programmatic dictation started"
                                );
                                programmatic = Some(ProgrammaticDictation {
                                    id,
                                    owner_token,
                                    source,
                                    levels,
                                    audio: None,
                                    last_level: Instant::now() - Duration::from_millis(30),
                                    lease_deadline: Instant::now() + PROGRAMMATIC_LEASE,
                                });
                                let _ = reply.send(Ok(ProgrammaticDictationStart {
                                    id,
                                    sample_rate: input.sample_rate(),
                                }));
                            }
                            Err(error) => {
                                let _ = reply.send(Err(error.to_string()));
                            }
                        }
                    }
                    RecognitionControl::AttachDictationAudio {
                        id,
                        owner_token,
                        audio,
                        reply,
                    } => {
                        let Some(active) = programmatic
                            .as_mut()
                            .filter(|active| active.matches_owner(id, &owner_token))
                        else {
                            let _ = reply.send(Err("dictation-not-active".into()));
                            continue;
                        };
                        if active.audio.is_some() {
                            let _ = reply.send(Err("audio-already-subscribed".into()));
                            continue;
                        }
                        active.audio = Some(audio);
                        let _ = reply.send(Ok(()));
                    }
                    RecognitionControl::FinishDictation {
                        id,
                        owner_token,
                        reply,
                    } => {
                        if let Some(finishing) = programmatic_results
                            .values_mut()
                            .find(|finishing| finishing.matches_owner(id, &owner_token))
                        {
                            finishing.replies.push(reply);
                            continue;
                        }
                        if let Some(completion) = programmatic_completions
                            .iter()
                            .rev()
                            .find(|completion| completion.matches_owner(id, &owner_token))
                        {
                            let result = match &completion.outcome {
                                ProgrammaticTerminalOutcome::Finished(result) => result.clone(),
                                ProgrammaticTerminalOutcome::Cancelled => Err("cancelled".into()),
                            };
                            let _ = reply.send(result);
                            continue;
                        }
                        let (source, finish, reset_recognizer) = if let Some(lost) =
                            programmatic_lost.take_if(|lost| lost.matches_owner(id, &owner_token))
                        {
                            // No recognizer reset: a capture started since
                            // the microphone recovered may be using it.
                            (lost.source, Finish::Transcribe(lost.clip), false)
                        } else if programmatic
                            .as_ref()
                            .is_some_and(|active| active.matches_owner(id, &owner_token))
                        {
                            let active = programmatic.take().expect("active dictation was checked");
                            let boundary =
                                capture_boundary(&input, microphone_policy.release_while_idle);
                            // The microphone may have failed before this
                            // finish reached the audio owner, with its loss
                            // still queued.
                            let finish = match input.finish(boundary)? {
                                Finish::Discard => match input.take_loss() {
                                    Some(LostCapture::Kept(clip)) => Finish::Transcribe(clip),
                                    Some(LostCapture::Empty) => {
                                        log_programmatic_ended(Some(active), "mic-lost");
                                        finish_programmatic_with_result(
                                            id,
                                            owner_token,
                                            vec![reply],
                                            Err(MICROPHONE_LOST.into()),
                                            &mut programmatic_completions,
                                        );
                                        reset_command_recognizer(&input, &mut recognizer)?;
                                        continue;
                                    }
                                    None => Finish::Discard,
                                },
                                finish => finish,
                            };
                            (active.source, finish, true)
                        } else {
                            let _ = reply.send(Err("dictation-not-active".into()));
                            continue;
                        };
                        match finish {
                            Finish::Transcribe(clip) => {
                                let duration_ms = clip.duration_ms();
                                match dictation_worker.transcribe(
                                    clip,
                                    TranscriptionTarget::Service,
                                    None,
                                    ContextSnapshot::default(),
                                ) {
                                    Ok(job_id) => {
                                        programmatic_results.insert(
                                            job_id,
                                            FinishingProgrammaticDictation {
                                                id,
                                                source: source.clone(),
                                                owner_token,
                                                duration_ms,
                                                replies: vec![reply],
                                            },
                                        );
                                        tracing::info!(
                                            dictation_id = id,
                                            source,
                                            "programmatic dictation submitted"
                                        );
                                    }
                                    Err(error) => {
                                        tracing::warn!(
                                            dictation_id = id,
                                            source,
                                            outcome = "worker-error",
                                            error = %error,
                                            "programmatic dictation ended"
                                        );
                                        finish_programmatic_with_result(
                                            id,
                                            owner_token,
                                            vec![reply],
                                            Err(error.into()),
                                            &mut programmatic_completions,
                                        );
                                    }
                                }
                            }
                            Finish::Discard => {
                                tracing::warn!(
                                    dictation_id = id,
                                    source,
                                    outcome = "discarded",
                                    "programmatic dictation ended"
                                );
                                finish_programmatic_with_result(
                                    id,
                                    owner_token,
                                    vec![reply],
                                    Err("capture-discarded".into()),
                                    &mut programmatic_completions,
                                );
                            }
                        }
                        if reset_recognizer {
                            reset_command_recognizer(&input, &mut recognizer)?;
                        }
                    }
                    RecognitionControl::CancelDictation {
                        id,
                        owner_token,
                        reply,
                    } => {
                        if let Some(completion) = programmatic_completions
                            .iter()
                            .rev()
                            .find(|completion| completion.matches_owner(id, &owner_token))
                            && matches!(completion.outcome, ProgrammaticTerminalOutcome::Cancelled)
                        {
                            let _ = reply.send(Ok(()));
                            continue;
                        }
                        if programmatic_results
                            .values()
                            .any(|finishing| finishing.matches_owner(id, &owner_token))
                        {
                            let _ = reply.send(Err("dictation-finishing".into()));
                            continue;
                        }
                        if let Some(lost) =
                            programmatic_lost.take_if(|lost| lost.matches_owner(id, &owner_token))
                        {
                            tracing::info!(
                                dictation_id = id,
                                source = lost.source,
                                "programmatic dictation cancelled"
                            );
                            remember_programmatic_completion(
                                &mut programmatic_completions,
                                ProgrammaticCompletion {
                                    id,
                                    owner_token,
                                    outcome: ProgrammaticTerminalOutcome::Cancelled,
                                },
                            );
                            let _ = reply.send(Ok(()));
                            continue;
                        }
                        if programmatic
                            .as_ref()
                            .is_none_or(|active| !active.matches_owner(id, &owner_token))
                        {
                            let _ = reply.send(Err("dictation-not-active".into()));
                            continue;
                        }
                        let active = programmatic.take().expect("active dictation was checked");
                        tracing::info!(
                            dictation_id = id,
                            source = active.source,
                            "programmatic dictation cancelled"
                        );
                        cancel_programmatic_capture(
                            active,
                            &input,
                            &mut recognizer,
                            &mut programmatic_completions,
                        )?;
                        let _ = reply.send(Ok(()));
                    }
                    RecognitionControl::HeartbeatDictation {
                        id,
                        owner_token,
                        reply,
                    } => {
                        let lease_deadline = if let Some(active) = programmatic
                            .as_mut()
                            .filter(|active| active.matches_owner(id, &owner_token))
                        {
                            &mut active.lease_deadline
                        } else if let Some(lost) = programmatic_lost
                            .as_mut()
                            .filter(|lost| lost.matches_owner(id, &owner_token))
                        {
                            &mut lost.lease_deadline
                        } else {
                            let _ = reply.send(Err("dictation-not-active".into()));
                            continue;
                        };
                        *lease_deadline = Instant::now() + PROGRAMMATIC_LEASE;
                        let _ = reply.send(Ok(()));
                    }
                }
            }
        }
        if programmatic
            .as_ref()
            .is_some_and(|active| programmatic_lease_expired(active.lease_deadline, Instant::now()))
        {
            let expired = programmatic.take().expect("expired dictation was checked");
            tracing::warn!(
                dictation_id = expired.id,
                source = expired.source,
                "programmatic dictation lease expired"
            );
            cancel_programmatic_capture(
                expired,
                &input,
                &mut recognizer,
                &mut programmatic_completions,
            )?;
        }
        if let Some(expired) = programmatic_lost
            .take_if(|lost| programmatic_lease_expired(lost.lease_deadline, Instant::now()))
        {
            tracing::warn!(
                dictation_id = expired.id,
                source = expired.source,
                "programmatic dictation lease expired"
            );
            remember_programmatic_completion(
                &mut programmatic_completions,
                ProgrammaticCompletion {
                    id: expired.id,
                    owner_token: expired.owner_token,
                    outcome: ProgrammaticTerminalOutcome::Cancelled,
                },
            );
        }
        while let Ok(next_context) = context_monitor.updates.try_recv() {
            if context != next_context {
                input_monitor.activity.invalidate();
            }
            context = next_context;
            events.emit(&VoiceEvent::Context {
                timestamp_ms: now_ms(),
                application: context.application.clone(),
                browser_url: context.browser_url.as_ref().map(ToString::to_string),
            })?;
        }
        if let Some(action_executor) = &action_executor {
            while let Some(outcome) = action_executor.try_recv() {
                handle_action_outcome(outcome, &mut events)?;
            }
        }
        if let Some(personal_commands) = &personal_commands {
            while let Some(outcome) = personal_commands.try_recv() {
                handle_action_outcome(outcome, &mut events)?;
            }
        }
        let next_microphone_policy = crate::app_settings::microphone_policy();
        let next_commands_enabled = next_microphone_policy.commands_enabled;
        let release_policy_changed =
            next_microphone_policy.release_while_idle != microphone_policy.release_while_idle;
        if release_policy_changed && !next_microphone_policy.release_while_idle {
            input.set_release_while_idle(false);
        }
        if next_commands_enabled != commands_enabled {
            commands_enabled = next_commands_enabled;
            mode = Mode::Listening;
            if commands_enabled {
                if command_loader.is_none() {
                    command_loader = Some(load_command_recognizer(project_root.to_path_buf()));
                } else {
                    set_command_model_status(CommandModelStatus::Loading);
                }
                tracing::info!("voice commands enabled; loading command model");
            } else {
                recognizer = None;
                action_executor = None;
                set_command_model_status(CommandModelStatus::Disabled);
                tracing::info!("voice commands disabled");
            }
        }
        if release_policy_changed && next_microphone_policy.release_while_idle {
            input.set_release_while_idle(true);
        }
        microphone_policy = next_microphone_policy;
        let next_personal_host_enabled =
            commands_enabled || crate::app_settings::custom_transformations_enabled();
        if next_personal_host_enabled != personal_host_enabled {
            personal_host_enabled = next_personal_host_enabled;
            personal_commands = personal_host_enabled
                .then(|| {
                    crate::personal_commands::PersonalCommands::start(
                        commands.as_ref().clone(),
                        transformations.clone(),
                    )
                })
                .flatten();
        }
        if let Some(loader) = &command_loader {
            match loader.try_recv() {
                Ok(Ok(loaded)) if commands_enabled => {
                    recognizer = Some(loaded);
                    input.invalidate_recognition()?;
                    action_executor = Some(ActionExecutor::start());
                    command_loader = None;
                    set_command_model_status(CommandModelStatus::Ready);
                    tracing::info!("voice command model loaded");
                }
                Ok(Ok(_)) => command_loader = None,
                Ok(Err(error)) => {
                    command_loader = None;
                    tracing::error!(%error, "could not enable voice commands");
                    if commands_enabled {
                        set_command_model_status(CommandModelStatus::Failed);
                    }
                }
                Err(TryRecvError::Empty) => {}
                Err(TryRecvError::Disconnected) => {
                    command_loader = None;
                    tracing::error!("voice command model loader stopped");
                    if commands_enabled {
                        set_command_model_status(CommandModelStatus::Failed);
                    }
                }
            }
        }
        hotkeys
            .dictation
            .set_double_tap_enabled(crate::app_settings::double_tap_lock());
        hotkeys
            .dictation
            .set_double_tap_only(crate::app_settings::double_tap_only());
        hotkeys
            .dictation
            .set_toggle(crate::app_settings::shortcut_toggle());
        hotkeys
            .dictation
            .set_binding(crate::app_settings::dictation_hotkey());
        match crate::app_settings::edit_hotkey() {
            Some(binding) => match &mut hotkeys.edit {
                Some(edit) => edit.set_binding(binding),
                None => {
                    let mut edit =
                        DictationHotkey::new_without_paste(CaptureInstant::now(), binding);
                    edit.wait_for_release();
                    hotkeys.edit = Some(edit);
                }
            },
            None => {
                if let Some(mut edit) = hotkeys.edit.take() {
                    let was_recording = edit.suspend().is_some();
                    let was_pending = edit_pending_since.take().is_some();
                    if (was_recording || was_pending || edit_context.is_some()) && voice.is_none() {
                        hotkeys.dictation.suppress_until_release();
                        handle_edit_hotkey_action(
                            HotkeyAction::Cancel,
                            CaptureInstant::now(),
                            &mut edit_context,
                            &mut recognizer,
                            &input,
                            &dictation_worker,
                            mode,
                            &context,
                            &input.device_name(),
                            &mut events,
                            indicator.as_ref(),
                        )?;
                    }
                }
            }
        }
        let (next_microphone_revision, microphone) = crate::app_settings::microphone_selection();
        if next_microphone_revision != microphone_revision {
            input.request_selection(next_microphone_revision, microphone.as_deref());
            microphone_revision = next_microphone_revision;
        }
        let (next_revision, selection) = crate::app_settings::transcription_selection();
        if next_revision != transcription_revision && dictation_worker.reload(selection).is_ok() {
            transcription_revision = next_revision;
        }
        let hotkey_capture_active = crate::app_settings::hotkey_capture_active();
        let hotkey_capture_suspended = hotkey_capture_active || input.is_recovering();
        if hotkey_capture_suspended && voice.is_none() {
            edit_pending_since = None;
            if hotkeys.suspend_all() {
                edit_context = None;
                handle_hotkey_action(
                    HotkeyAction::Cancel,
                    input.captured_through(),
                    &mut recognizer,
                    &input,
                    &dictation_worker,
                    mode,
                    &context,
                    &input.device_name(),
                    &mut events,
                    indicator.as_ref(),
                    microphone_policy.release_while_idle,
                )?;
            }
        }
        // The capture shortcut ignores keys while shortcuts are suspended or
        // a programmatic dictation holds them, so their releases go unseen.
        #[cfg(hex_capture)]
        if hotkey_capture_suspended || programmatic.is_some() {
            capture_shortcut.reset();
        }
        while let Ok(observed) = input_monitor.events.try_recv() {
            let _acknowledge = input_monitor.acknowledge_after(observed);
            let input_event = observed.event;
            let capture_at = observed.capture_at;
            // Escape included: it belongs to the owner app (closing a
            // picker while a note records), which cancels through the API.
            if programmatic.is_some() {
                hotkeys.track_key_state(input_event, capture_at);
                continue;
            }
            if hotkey_capture_suspended {
                continue;
            }
            #[cfg(hex_capture)]
            if let Some(host) = capture_host.as_mut() {
                let activity = host.shortcut_activity(capture_at, &input);
                let action = capture_shortcut.input(
                    input_event,
                    crate::app_settings::runtime_hotkeys(),
                    activity,
                );
                if action == crate::capture_host::ShortcutAction::Cancel {
                    // Escape before the shortcut's keys came up: nothing else
                    // hears it.
                    continue;
                }
                host.shortcut(action, &input);
            }
            if voice.is_some() && input_event.is_escape_down() {
                voice = None;
                end_capture(
                    CaptureEnd::Cancelled,
                    &input,
                    &mut recognizer,
                    mode,
                    &input.device_name(),
                    &mut events,
                    indicator.as_ref(),
                )?;
                continue;
            }
            // Escape ends a dictation or Voice Action hold first, then
            // closes the capsule's picker or open list (never an Undo: that is a
            // click), then cancels queued
            // dictation. It never ends a recording capture: it may be meant
            // for the app being recorded (the capsule's ✕ discards).
            #[cfg(hex_capture)]
            if !hotkeys.any_recording()
                && input_event.is_escape_down()
                && crate::capture_host::capsule_key(crate::capture_host::CapsuleKey::Escape)
            {
                continue;
            }
            if !hotkeys.any_recording()
                && input_event.is_escape_down()
                && cancel_latest_job(&dictation_worker, &mut events, indicator.as_ref())?
            {
                continue;
            }
            // Annotate records the narration itself: while it records, the
            // dictation and Voice Action shortcuts start nothing. The tap
            // still swallows their chords, so they never reach the app.
            #[cfg(hex_capture)]
            if !hotkeys.any_recording()
                && voice.is_none()
                && capture_host
                    .as_mut()
                    .is_some_and(|host| host.recording_since(&input).is_some())
            {
                hotkeys.track_key_state(input_event, capture_at);
                continue;
            }
            let edit_action = hotkeys
                .edit
                .as_mut()
                .and_then(|edit| edit.process(input_event, capture_at));
            if matches!(edit_action, Some(HotkeyAction::Start)) && voice.is_none() {
                // Voice Action confirms itself once its own hold counts.
                feedback::retract_start();
                start_pending_voice_action(
                    &mut edit_pending_since,
                    capture_at,
                    input.is_recording(),
                    indicator.as_ref(),
                );
            }
            if let Some(action) = hotkeys.dictation.process(input_event, capture_at) {
                if matches!(action, HotkeyAction::PasteLast | HotkeyAction::PasteMeeting) {
                    voice = None;
                }
                let voice_action_takeover = voice_action_owns_action(
                    action,
                    edit_pending_since.is_some(),
                    hotkeys.edit_recording(),
                    edit_action,
                );
                if voice_action_takeover {
                    let _ = hotkeys.dictation.suspend();
                } else if voice.is_none() {
                    let accepted = handle_hotkey_action(
                        action,
                        capture_at,
                        &mut recognizer,
                        &input,
                        &dictation_worker,
                        mode,
                        &context,
                        &input.device_name(),
                        &mut events,
                        indicator.as_ref(),
                        microphone_policy.release_while_idle,
                    )?;
                    if !accepted {
                        hotkeys.dictation.suspend();
                    }
                }
            }
            if let Some(action) = edit_action
                && voice.is_none()
                && !matches!(action, HotkeyAction::Start)
            {
                let pending_start = edit_pending_since.take();
                match pending_start {
                    None => {
                        let accepted = handle_edit_hotkey_action(
                            action,
                            capture_at,
                            &mut edit_context,
                            &mut recognizer,
                            &input,
                            &dictation_worker,
                            mode,
                            &context,
                            &input.device_name(),
                            &mut events,
                            indicator.as_ref(),
                        )?;
                        if !accepted {
                            hotkeys.suspend_edit();
                        }
                    }
                    Some(started_at)
                        if pending_voice_action_is_intentional(started_at, capture_at, action) =>
                    {
                        let accepted = handle_edit_hotkey_action(
                            HotkeyAction::Start,
                            started_at,
                            &mut edit_context,
                            &mut recognizer,
                            &input,
                            &dictation_worker,
                            mode,
                            &context,
                            &input.device_name(),
                            &mut events,
                            None,
                        )?;
                        if accepted {
                            handle_edit_hotkey_action(
                                HotkeyAction::Finish,
                                capture_at,
                                &mut edit_context,
                                &mut recognizer,
                                &input,
                                &dictation_worker,
                                mode,
                                &context,
                                &input.device_name(),
                                &mut events,
                                indicator.as_ref(),
                            )?;
                        } else if let Some(indicator) = &indicator {
                            indicator.send(DictationIndicatorEvent::Failed);
                        }
                    }
                    Some(_) => {
                        edit_context = None;
                        end_capture(
                            CaptureEnd::Discarded(action.discard_cause()),
                            &input,
                            &mut recognizer,
                            mode,
                            &input.device_name(),
                            &mut events,
                            indicator.as_ref(),
                        )?;
                    }
                }
            }
            // A shortcut press during voice-delimited dictation starts nothing.
            if voice.is_some() {
                hotkeys.forgo_chord_breaks();
            }
        }
        if programmatic.is_none()
            && voice.is_none()
            && !hotkey_capture_suspended
            && input_monitor.pending_events().oldest().is_none()
        {
            hotkeys.recover_stale_keys();
        }
        if hotkeys.dictation.is_recording()
            && edit_pending_since.is_none()
            && input.become_intentional(capture_boundary(
                &input,
                microphone_policy.release_while_idle,
            ))?
            && microphone_policy.release_while_idle
        {
            feedback::play(Tone::DictationStart);
        }
        if edit_pending_since.is_some_and(|started| {
            capture_boundary(&input, microphone_policy.release_while_idle).duration_since(started)
                >= MINIMUM_HOLD_DURATION
        }) && hotkeys.edit_recording()
        {
            let capture_at = edit_pending_since.expect("pending voice action has a boundary");
            edit_pending_since = None;
            let accepted = handle_edit_hotkey_action(
                HotkeyAction::Start,
                capture_at,
                &mut edit_context,
                &mut recognizer,
                &input,
                &dictation_worker,
                mode,
                &context,
                &input.device_name(),
                &mut events,
                None,
            )?;
            if !accepted {
                hotkeys.suspend_edit();
                if let Some(indicator) = &indicator {
                    indicator.send(DictationIndicatorEvent::Failed);
                }
            }
        }
        // Only once every queued edge is handled, so a release already seen
        // ends the press first.
        if voice.is_none()
            && edit_pending_since.is_none()
            && !hotkey_capture_suspended
            && input_monitor.pending_events().oldest().is_none()
        {
            hotkeys.break_chords(CaptureInstant::now());
        }
        let mut received_worker_event = false;
        while let Some(event) = dictation_worker.try_recv() {
            received_worker_event = true;
            match event {
                WorkerEvent::Completed {
                    job_id,
                    target: TranscriptionTarget::Service,
                    result,
                    ..
                } => {
                    if let Some(finishing) = programmatic_results.remove(&job_id) {
                        match &result {
                            Ok(transcript) => tracing::info!(
                                dictation_id = finishing.id,
                                source = finishing.source,
                                outcome = "transcribed",
                                transcript_chars = transcript.chars().count(),
                                duration_ms = finishing.duration_ms,
                                "programmatic dictation ended"
                            ),
                            Err(error) => tracing::warn!(
                                dictation_id = finishing.id,
                                source = finishing.source,
                                outcome = "worker-error",
                                duration_ms = finishing.duration_ms,
                                error = %error,
                                "programmatic dictation ended"
                            ),
                        }
                        let result = result.map(|transcript| ProgrammaticDictationResult {
                            transcript,
                            duration_ms: finishing.duration_ms,
                        });
                        finish_programmatic_with_result(
                            finishing.id,
                            finishing.owner_token,
                            finishing.replies,
                            result,
                            &mut programmatic_completions,
                        );
                    }
                    continue;
                }
                WorkerEvent::Cancelled { job_id } => {
                    if let Some(finishing) = programmatic_results.remove(&job_id) {
                        finish_programmatic_with_result(
                            finishing.id,
                            finishing.owner_token,
                            finishing.replies,
                            Err("cancelled".into()),
                            &mut programmatic_completions,
                        );
                        continue;
                    }
                    handle_dictation_event(
                        WorkerEvent::Cancelled { job_id },
                        &mut events,
                        indicator.as_ref(),
                    )?;
                    continue;
                }
                event => handle_dictation_event(event, &mut events, indicator.as_ref())?,
            }
        }
        // A recording capture never takes Escape: only the capture
        // shortcut's press before it starts, and the capsule's picker.
        #[cfg(hex_capture)]
        let capture_escape = capture_host.is_some() && capture_shortcut.takes_escape();
        #[cfg(not(hex_capture))]
        let capture_escape = false;
        input_monitor.set_escape_cancels(claims_escape(
            programmatic.is_some(),
            hotkeys.any_recording() || voice.is_some(),
            capture_escape,
            || dictation_worker.has_escape_cancellable_job(),
        ));
        if received_worker_event {
            emit_engine_state(
                &mut events,
                hotkeys.any_recording() || voice.is_some(),
                &dictation_worker,
                mode,
                &input.device_name(),
            )?;
        }

        while let Some(audio_event) = input.try_recv_event() {
            match audio_event {
                DictationAudioEvent::ReadyIntentional { .. } => {
                    if input.is_recording() {
                        feedback::play(Tone::DictationStart);
                    }
                }
                DictationAudioEvent::OpenFailed { error, .. } => {
                    programmatic_microphone_lost(
                        programmatic.take(),
                        None,
                        "mic-open-failed",
                        &mut programmatic_lost,
                        &mut programmatic_completions,
                    );
                    abandon_capture(
                        &mut hotkeys,
                        &mut voice,
                        &mut edit_context,
                        &mut edit_pending_since,
                    );
                    fail_capture(
                        format!("Could not open microphone: {error}"),
                        &dictation_worker,
                        mode,
                        &input.device_name(),
                        &mut events,
                        indicator.as_ref(),
                    )?;
                }
                DictationAudioEvent::RecognitionDiscontinuity { dropped_frames } => {
                    tracing::warn!(
                        dropped_recognition_frames = dropped_frames,
                        dropped_recognition_ms =
                            dropped_frames * 1_000 / u64::from(input.sample_rate()),
                        "command recognition fell behind; dictation capture remained continuous"
                    );
                    resync_recognizer(&input, recognizer.as_mut())?;
                }
                DictationAudioEvent::CaptureDiscontinuity {
                    was_recording,
                    gap_ms,
                    kept,
                    ..
                } => {
                    resync_recognizer(&input, recognizer.as_mut())?;
                    if was_recording {
                        programmatic_microphone_lost(
                            programmatic.take(),
                            kept,
                            "mic-gap",
                            &mut programmatic_lost,
                            &mut programmatic_completions,
                        );
                        abandon_capture(
                            &mut hotkeys,
                            &mut voice,
                            &mut edit_context,
                            &mut edit_pending_since,
                        );
                        fail_capture(
                            format!("Microphone audio was interrupted for {gap_ms} ms."),
                            &dictation_worker,
                            mode,
                            &input.device_name(),
                            &mut events,
                            indicator.as_ref(),
                        )?;
                    }
                }
                DictationAudioEvent::Reopened => {
                    resync_recognizer(&input, recognizer.as_mut())?;
                    if !input.is_recording() {
                        emit_state(&mut events, false, mode, &input.device_name())?;
                    }
                }
                DictationAudioEvent::Interrupted {
                    was_recording,
                    kept,
                    ..
                } => {
                    if was_recording {
                        programmatic_microphone_lost(
                            programmatic.take(),
                            kept,
                            "mic-interrupted",
                            &mut programmatic_lost,
                            &mut programmatic_completions,
                        );
                    }
                    abandon_capture(
                        &mut hotkeys,
                        &mut voice,
                        &mut edit_context,
                        &mut edit_pending_since,
                    );
                    resync_recognizer(&input, recognizer.as_mut())?;
                    if was_recording {
                        fail_capture(
                            "Microphone capture was interrupted; reconnecting.".into(),
                            &dictation_worker,
                            mode,
                            &input.device_name(),
                            &mut events,
                            indicator.as_ref(),
                        )?;
                    }
                }
            }
        }

        let audio = input.recv_timeout(Duration::from_millis(20))?;
        if let Some(smart_turn) = &mut smart_turn {
            // Follow a voice or shortcut dictation from its start, so speech
            // while the shortcut is still down counts; only a hands-free one
            // (voice, locked, or Toggle after its release) may stop.
            let followed = input.is_recording()
                && programmatic.is_none()
                && !hotkey_capture_suspended
                && edit_pending_since.is_none()
                && edit_context.is_none()
                && !hotkeys.edit_recording()
                && (voice.is_some() || hotkeys.dictation.is_recording());
            smart_turn.track(followed.then(|| crate::smart_turn::Capture {
                generation: input.capture_generation(),
                listen_from: voice.as_ref().map(|active| active.started_at),
                armed: voice.is_some() || hotkeys.dictation.is_locked(),
            }));
            // Recognition generations only gate Moonshine; Smart Turn orders
            // audio by capture time and skips anything it already heard.
            if let Some(audio) = &audio {
                smart_turn.push(&audio.samples, input.sample_rate(), audio.captured_from());
            }
            for report in smart_turn.poll() {
                events.emit(&report.event(now_ms()))?;
            }
            // A queued Escape or shortcut press is handled first; the stop
            // waits for a later turn of the loop.
            if followed
                && input_monitor.pending_events().oldest().is_none()
                && smart_turn.take_stop()
            {
                smart_turn.track(None);
                let boundary = capture_boundary(&input, microphone_policy.release_while_idle);
                if let Some(active) = voice.take() {
                    // As the spoken stop phrase does. Moonshine may not have
                    // heard a control phrase yet, so one at the end of the
                    // transcript still cancels or sends.
                    finish_dictation_with(
                        &input,
                        boundary,
                        Duration::ZERO,
                        |clip| {
                            dictation_worker.transcribe_voice_turn(
                                clip,
                                active.protocol,
                                context.clone(),
                            )
                        },
                        &input.device_name(),
                        &mut events,
                        indicator.as_ref(),
                    )?;
                    reset_command_recognizer(&input, &mut recognizer)?;
                    emit_engine_state(
                        &mut events,
                        false,
                        &dictation_worker,
                        mode,
                        &input.device_name(),
                    )?;
                } else {
                    // As `hex dictation stop` does: a locked finish press.
                    finish_hotkey_dictation(
                        boundary,
                        &mut recognizer,
                        &input,
                        &dictation_worker,
                        mode,
                        &context,
                        &input.device_name(),
                        &mut events,
                        indicator.as_ref(),
                        microphone_policy.release_while_idle,
                        None,
                    )?;
                    // Unlock only now: a shortcut press during the stop's
                    // tail meant to finish this capture, so it is ignored
                    // instead of starting another.
                    hotkeys.dictation.wait_for_release();
                }
                continue;
            }
        }
        if let (Some(active), Some(audio)) = (&mut programmatic, &audio) {
            if active.last_level.elapsed() >= Duration::from_millis(30) {
                let _ = active.levels.try_send(level_for(&audio.samples));
                active.last_level = Instant::now();
            }
            if let Some(sender) = &active.audio {
                match sender.try_send(DictationAudioChunk {
                    samples: audio.samples.clone(),
                }) {
                    Ok(()) | Err(TrySendError::Full(_)) => {}
                    Err(TrySendError::Disconnected(_)) => active.audio = None,
                }
            }
        }
        if let Some(audio) = &audio
            && audio.is_current(input.recognition_generation())
            && recognizer.is_some()
            && programmatic.is_none()
            && !hotkeys.suppresses_recognition()
        {
            let generation = input.recognition_generation();
            if recognition_origin
                .and_then(|origin| origin.for_generation(generation))
                .is_none()
            {
                recognition_origin = Some(RecognitionOrigin {
                    generation,
                    at: audio.captured_from(),
                });
            }
        }
        let mut fed_generation = None;
        if let Some(audio) = audio.filter(|audio| audio.is_current(input.recognition_generation()))
        {
            let suppress_non_voice = programmatic.is_some() || hotkeys.suppresses_recognition();
            if (voice.is_some() || suppress_non_voice)
                && let Some(indicator) = &indicator
            {
                indicator.meter(&audio.samples);
            }
            if (voice.is_some() || !suppress_non_voice)
                && let Some(recognizer) = &mut recognizer
            {
                let generation = input.recognition_generation();
                recognizer.add_audio(&audio.samples, input.sample_rate())?;
                fed_generation = Some(generation);
            }
        }
        if fed_generation.is_some_and(|generation| generation != input.recognition_generation()) {
            resync_recognizer(&input, recognizer.as_mut())?;
            continue;
        }
        if !input.is_recovering()
            && programmatic.is_none()
            && !hotkeys.suppresses_recognition()
            && last_update.elapsed() >= UPDATE_INTERVAL
            && let Some(recognizer) = &mut recognizer
            && let Some(action_executor) = &action_executor
        {
            let generation = input.recognition_generation();
            if recognition_origin
                .and_then(|origin| origin.for_generation(generation))
                .is_none()
            {
                resync_recognizer(&input, Some(recognizer))?;
                last_update = Instant::now();
                continue;
            }
            let updates = recognizer.update()?;
            if generation != input.recognition_generation() {
                resync_recognizer(&input, Some(recognizer))?;
                last_update = Instant::now();
                continue;
            }
            for update in updates {
                if generation != input.recognition_generation() {
                    resync_recognizer(&input, Some(recognizer))?;
                    break;
                }
                let active_runtime = personal_commands
                    .as_ref()
                    .map_or_else(|| native_runtime.clone(), |runtime| runtime.snapshot());
                let is_completed = matches!(update.phase, TranscriptPhase::Completed);
                let activate = if mode == Mode::Listening && voice.is_none() {
                    activation_stability.observe(
                        &active_runtime.dictation,
                        &update.text,
                        is_completed,
                        Instant::now(),
                    )
                } else {
                    activation_stability.reset();
                    false
                };
                if activate {
                    if generation != input.recognition_generation() {
                        break;
                    }
                    let boundary = recognition_boundary(
                        recognition_origin,
                        generation,
                        update.end_ms,
                        input.captured_through(),
                    );
                    if !start_voice_capture(
                        &mut voice,
                        active_runtime.dictation.clone(),
                        recognizer,
                        &input,
                        boundary,
                        generation,
                        mode,
                        &input.device_name(),
                        &mut events,
                        indicator.as_ref(),
                    )? {
                        break;
                    }
                    events.emit(&VoiceEvent::Command {
                        timestamp_ms: now_ms(),
                        heard: update.text,
                        command: Some("dictation.start".into()),
                        outcome: CommandOutcome::Executed,
                        context: context.label(),
                    })?;
                    break;
                }
                if let Some(active) = &mut voice {
                    let control = active
                        .protocol
                        .control_suffix(&update.text)
                        .map(|(control, _)| control);
                    if let Some(control) = active.stability.observe(control, is_completed) {
                        if !input.consume_recognition(generation)? {
                            break;
                        }
                        let boundary = recognition_boundary(
                            recognition_origin,
                            generation,
                            update.end_ms,
                            input.captured_through(),
                        );
                        let active = voice.take().expect("voice capture is active");
                        recognizer.reset_stream()?;
                        handle_voice_dictation_control(
                            control,
                            &update.text,
                            active.protocol,
                            &input,
                            boundary,
                            &dictation_worker,
                            mode,
                            &context,
                            &input.device_name(),
                            &mut events,
                            indicator.as_ref(),
                        )?;
                        break;
                    }
                    continue;
                }
                events.emit(&VoiceEvent::Transcript {
                    timestamp_ms: now_ms(),
                    phase: update.phase,
                    latency_ms: update.latency_ms,
                    text: update.text.clone(),
                })?;
                if is_completed {
                    if generation != input.recognition_generation() {
                        break;
                    }
                    let boundary = recognition_boundary(
                        recognition_origin,
                        generation,
                        update.end_ms,
                        input.captured_through(),
                    );
                    handle_command(
                        &active_runtime.commands,
                        &mut mode,
                        &mut voice,
                        active_runtime.dictation.clone(),
                        recognizer,
                        &input,
                        boundary,
                        generation,
                        action_executor,
                        personal_commands.as_ref(),
                        meeting_requests.as_ref(),
                        &update.text,
                        &context,
                        &input_monitor.activity,
                        &input.device_name(),
                        &mut events,
                        indicator.as_ref(),
                    )?;
                }
            }
            last_update = Instant::now();
        }
    }

    #[cfg(hex_capture)]
    if let Some(host) = &mut capture_host {
        host.shutdown(&input);
    }
    events.emit(&VoiceEvent::State {
        timestamp_ms: now_ms(),
        state: VoiceState::Stopping,
        device: input.device_name(),
    })?;
    events.flush()?;
    if let Some(indicator) = &indicator {
        indicator.send(DictationIndicatorEvent::Discarded);
    }
    Ok(())
}

fn level_for(samples: &[f32]) -> DictationLevel {
    let (sum_squares, peak) = samples
        .iter()
        .fold((0.0_f64, 0.0_f32), |(sum, peak), sample| {
            let magnitude = sample.abs();
            (
                sum + f64::from(*sample) * f64::from(*sample),
                peak.max(magnitude),
            )
        });
    let rms = if samples.is_empty() {
        0.0
    } else {
        (sum_squares / samples.len() as f64).sqrt() as f32
    };
    DictationLevel {
        rms_db: amplitude_db(rms),
        peak_db: amplitude_db(peak),
    }
}

fn log_programmatic_ended(active: Option<ProgrammaticDictation>, outcome: &str) {
    if let Some(active) = active {
        tracing::warn!(
            dictation_id = active.id,
            source = active.source,
            outcome,
            "programmatic dictation ended"
        );
    }
}

/// Ends a programmatic capture the microphone cut short. The audio delivered
/// before the loss waits for the owner's `/finish`; without any, `/finish`
/// answers `microphone-lost`.
fn programmatic_microphone_lost(
    active: Option<ProgrammaticDictation>,
    kept: Option<DictationClip>,
    outcome: &str,
    lost: &mut Option<LostProgrammaticDictation>,
    completions: &mut VecDeque<ProgrammaticCompletion>,
) {
    let Some(active) = active else {
        return;
    };
    let Some(mut clip) = kept else {
        remember_programmatic_completion(
            completions,
            ProgrammaticCompletion {
                id: active.id,
                owner_token: active.owner_token.clone(),
                outcome: ProgrammaticTerminalOutcome::Finished(Err(MICROPHONE_LOST.into())),
            },
        );
        log_programmatic_ended(Some(active), outcome);
        return;
    };
    // A held clip must not defer background work until the owner finishes.
    drop(clip.take_capture_marker());
    tracing::warn!(
        dictation_id = active.id,
        source = active.source,
        outcome,
        duration_ms = clip.duration_ms(),
        "programmatic dictation kept its audio after a microphone loss"
    );
    let previous = lost.replace(LostProgrammaticDictation {
        id: active.id,
        owner_token: active.owner_token,
        source: active.source,
        lease_deadline: active.lease_deadline,
        clip,
    });
    if let Some(previous) = previous {
        tracing::warn!(
            dictation_id = previous.id,
            source = previous.source,
            outcome = "superseded",
            "programmatic dictation ended"
        );
        remember_programmatic_completion(
            completions,
            ProgrammaticCompletion {
                id: previous.id,
                owner_token: previous.owner_token,
                outcome: ProgrammaticTerminalOutcome::Finished(Err(MICROPHONE_LOST.into())),
            },
        );
    }
}

fn remember_programmatic_completion(
    completions: &mut VecDeque<ProgrammaticCompletion>,
    completion: ProgrammaticCompletion,
) {
    completions.push_back(completion);
    while completions.len() > PROGRAMMATIC_COMPLETION_LIMIT {
        completions.pop_front();
    }
}

fn finish_programmatic_with_result(
    id: u64,
    owner_token: String,
    replies: Vec<SyncSender<Result<ProgrammaticDictationResult, String>>>,
    result: Result<ProgrammaticDictationResult, String>,
    completions: &mut VecDeque<ProgrammaticCompletion>,
) {
    for reply in replies {
        let _ = reply.send(result.clone());
    }
    remember_programmatic_completion(
        completions,
        ProgrammaticCompletion {
            id,
            owner_token,
            outcome: ProgrammaticTerminalOutcome::Finished(result),
        },
    );
}

fn cancel_programmatic_capture(
    active: ProgrammaticDictation,
    input: &DictationAudio,
    recognizer: &mut Option<Moonshine>,
    completions: &mut VecDeque<ProgrammaticCompletion>,
) -> Result<()> {
    input.cancel()?;
    reset_command_recognizer(input, recognizer)?;
    remember_programmatic_completion(
        completions,
        ProgrammaticCompletion {
            id: active.id,
            owner_token: active.owner_token,
            outcome: ProgrammaticTerminalOutcome::Cancelled,
        },
    );
    Ok(())
}

fn programmatic_lease_expired(deadline: Instant, now: Instant) -> bool {
    now >= deadline
}

fn amplitude_db(amplitude: f32) -> f32 {
    (20.0 * amplitude.max(0.000_001).log10()).max(-120.0)
}

fn load_command_recognizer(
    project_root: std::path::PathBuf,
) -> Receiver<Result<Moonshine, String>> {
    set_command_model_status(CommandModelStatus::Loading);
    spawn_command_load(move || {
        crate::moonshine::install_model().and_then(|_| Moonshine::load(&project_root))
    })
}

fn spawn_command_load<T: Send + 'static>(
    load: impl FnOnce() -> Result<T> + Send + 'static,
) -> Receiver<Result<T, String>> {
    let (sender, receiver) = mpsc::sync_channel(1);
    std::thread::spawn(move || {
        let _ = sender.send(load().map_err(|error| error.to_string()));
    });
    receiver
}

#[allow(clippy::too_many_arguments)]
fn start_voice_capture(
    voice: &mut Option<VoiceCapture>,
    protocol: Arc<DictationProtocol>,
    recognizer: &mut Moonshine,
    dictation: &DictationAudio,
    boundary: CaptureInstant,
    recognition_generation: u64,
    mode: Mode,
    device: &str,
    events: &mut EventLog,
    indicator: Option<&DictationIndicatorSender>,
) -> Result<bool> {
    if !dictation.start_voice(boundary, recognition_generation)? {
        return Ok(false);
    }
    recognizer.reset_stream()?;
    *voice = Some(VoiceCapture {
        protocol,
        stability: ControlStability::default(),
        started_at: boundary,
    });
    feedback::play(Tone::DictationStart);
    events.dictation(DictationPhase::Started, "")?;
    if let Some(indicator) = indicator {
        indicator.send(DictationIndicatorEvent::Started);
    }
    emit_state(events, true, mode, device)?;
    Ok(true)
}

#[allow(clippy::too_many_arguments)]
fn handle_hotkey_action(
    action: HotkeyAction,
    action_at: CaptureInstant,
    recognizer: &mut Option<Moonshine>,
    dictation: &DictationAudio,
    worker: &DictationWorker,
    mode: Mode,
    context: &ContextSnapshot,
    device: &str,
    events: &mut EventLog,
    indicator: Option<&DictationIndicatorSender>,
    release_while_idle: bool,
) -> Result<bool> {
    match action {
        HotkeyAction::Start => {
            if !dictation.start(action_at)? {
                return Ok(false);
            }
            // The tone confirms the press at once; a chord or short tap
            // retracts it. A released microphone confirms only once it opens.
            if release_while_idle {
                feedback::prepare();
            } else {
                feedback::play_provisional_start();
            }
            reset_command_recognizer(dictation, recognizer)?;
            worker.prepare_paste();
            events.dictation(DictationPhase::Started, "")?;
            if let Some(indicator) = indicator {
                indicator.send(DictationIndicatorEvent::Started);
            }
            emit_state(events, true, mode, device)
        }
        HotkeyAction::Finish => finish_hotkey_dictation(
            action_at,
            recognizer,
            dictation,
            worker,
            mode,
            context,
            device,
            events,
            indicator,
            release_while_idle,
            None,
        )
        .map(drop),
        HotkeyAction::Discard(_) | HotkeyAction::Cancel => {
            feedback::retract_start();
            end_capture(
                if matches!(action, HotkeyAction::Cancel) {
                    CaptureEnd::Cancelled
                } else {
                    CaptureEnd::Discarded(action.discard_cause())
                },
                dictation,
                recognizer,
                mode,
                device,
                events,
                indicator,
            )
        }
        HotkeyAction::PasteLast | HotkeyAction::PasteMeeting => {
            // The paste chord's modifier press started a provisional capture.
            feedback::retract_start();
            dictation.cancel()?;
            reset_command_recognizer(dictation, recognizer)?;
            let paste_result = match action {
                HotkeyAction::PasteLast => worker.paste_last(),
                HotkeyAction::PasteMeeting => worker.paste_meeting(),
                _ => unreachable!(),
            };
            if let Err(error) = paste_result {
                feedback::play(Tone::Error);
                events.dictation(DictationPhase::Failed(error.into()), "")?;
            }
            emit_engine_state(events, false, worker, mode, device)
        }
    }?;
    Ok(true)
}

/// Ends a shortcut dictation at `action_at` and hands it to transcription.
#[allow(clippy::too_many_arguments)]
fn finish_hotkey_dictation(
    action_at: CaptureInstant,
    recognizer: &mut Option<Moonshine>,
    dictation: &DictationAudio,
    worker: &DictationWorker,
    mode: Mode,
    context: &ContextSnapshot,
    device: &str,
    events: &mut EventLog,
    indicator: Option<&DictationIndicatorSender>,
    release_while_idle: bool,
    print: Option<PrintReply>,
) -> Result<FinishResult> {
    if dictation.become_intentional(action_at)? && release_while_idle {
        feedback::play(Tone::DictationStart);
    }
    let finished = match print {
        None => finish_dictation(
            dictation,
            action_at,
            POST_RELEASE_TAIL,
            worker,
            TranscriptionTarget::Paste,
            None,
            context,
            device,
            events,
            indicator,
        )?,
        Some(print) => finish_dictation_with(
            dictation,
            action_at,
            POST_RELEASE_TAIL,
            |clip| worker.transcribe_printed(clip, print, context.clone()),
            device,
            events,
            indicator,
        )?,
    };
    if matches!(finished, FinishResult::Discarded) {
        feedback::retract_start();
    }
    reset_command_recognizer(dictation, recognizer)?;
    emit_engine_state(events, false, worker, mode, device)?;
    Ok(finished)
}

/// Whether Hex takes Escape from the app in front. Never while a programmatic
/// dictation records: the listener ignores every key then, so Escape stays
/// with its owner app rather than being swallowed.
fn claims_escape(
    programmatic: bool,
    recording: bool,
    capture_escape: bool,
    cancellable_job: impl FnOnce() -> bool,
) -> bool {
    !programmatic && (recording || capture_escape || cancellable_job())
}

/// Cancels the newest unfinished dictation, as Escape does while idle.
fn cancel_latest_job(
    worker: &DictationWorker,
    events: &mut EventLog,
    indicator: Option<&DictationIndicatorSender>,
) -> Result<bool> {
    let Some(job_id) = worker.cancel_latest() else {
        return Ok(false);
    };
    feedback::play(Tone::Cancel);
    events.dictation(DictationPhase::Cancelled, "")?;
    if let Some(indicator) = indicator {
        indicator.send(DictationIndicatorEvent::JobCancelled {
            job_id: job_id.value(),
        });
    }
    Ok(true)
}

/// Why a control can't start a capture now, if it can't.
fn capture_refusal(other_recording: bool, suspended: bool) -> Option<ControlReply> {
    if other_recording {
        Some(ControlReply::error(
            "busy",
            "another recording is in progress; finish or cancel it first",
        ))
    } else if suspended {
        Some(ControlReply::error(
            "busy",
            "Hex is recording a shortcut in Settings or reopening the microphone; try again",
        ))
    } else {
        None
    }
}

/// A `cancel` with its own capture idle. Escape cancels the active capture
/// before any unfinished job, so while another capture records this refuses
/// rather than reach past it to a job.
fn idle_cancel_refusal(other_recording: bool) -> Option<ControlReply> {
    other_recording.then(|| {
        ControlReply::error(
            "busy",
            "another recording is in progress; cancel it first (Escape cancels it)",
        )
    })
}

/// A control start has no press to judge, so its capture counts as
/// intentional from its first sample: the recording environment starts and
/// the live stream opens now. `tone_owed` plays the start tone for a capture
/// that's already open; one still opening the microphone plays it on
/// `ReadyIntentional`.
fn make_control_capture_intentional(input: &DictationAudio, tone_owed: bool) -> Result<()> {
    if input.mark_intentional()? && tone_owed {
        feedback::play(Tone::DictationStart);
    }
    Ok(())
}

/// The reply to a control `stop`: only a submitted clip transcribes.
fn finish_reply(finished: Option<FinishResult>, submitted: &str) -> ControlReply {
    match finished {
        Some(FinishResult::Submitted) => ControlReply::done(submitted),
        Some(FinishResult::Discarded) | None => ControlReply::error(
            "discarded",
            "too short, discarded: no audio was recorded yet (the microphone may still have \
             been opening)",
        ),
        Some(FinishResult::Silent) => ControlReply::error(
            "discarded",
            "the recording was silent, discarded; nothing to transcribe",
        ),
        Some(FinishResult::Rejected(error)) => {
            ControlReply::error("failed", format!("couldn't transcribe it: {error}"))
        }
    }
}

/// `hex dictation`: a start locks the shortcut machine as a double tap
/// does, so the next press or `stop` finishes it and Escape cancels it.
#[allow(clippy::too_many_arguments)]
fn control_dictation(
    action: CaptureAction,
    print: Option<PrintReply>,
    other_recording: bool,
    suspended: bool,
    hotkeys: &mut Hotkeys,
    recognizer: &mut Option<Moonshine>,
    input: &DictationAudio,
    worker: &DictationWorker,
    mode: Mode,
    context: &ContextSnapshot,
    events: &mut EventLog,
    indicator: Option<&DictationIndicatorSender>,
    release_while_idle: bool,
) -> Result<ControlReply> {
    let recording = hotkeys.dictation.is_recording();
    let device = input.device_name();
    let action = match action {
        CaptureAction::Toggle if recording => CaptureAction::Stop,
        CaptureAction::Toggle => CaptureAction::Start,
        action => action,
    };
    Ok(match action {
        CaptureAction::Start if recording => {
            ControlReply::unchanged("Dictation is already recording")
        }
        CaptureAction::Start => {
            if let Some(refusal) = capture_refusal(other_recording, suspended) {
                return Ok(refusal);
            }
            if !hotkeys.dictation.lock() {
                return Ok(ControlReply::error(
                    "busy",
                    "the dictation shortcut is held; release it and try again",
                ));
            }
            let accepted = handle_hotkey_action(
                HotkeyAction::Start,
                capture_boundary(input, release_while_idle),
                recognizer,
                input,
                worker,
                mode,
                context,
                &device,
                events,
                indicator,
                release_while_idle,
            )?;
            if accepted {
                // A warm start already queued its provisional tone.
                make_control_capture_intentional(input, release_while_idle)?;
                ControlReply::done(
                    "Recording; `hex dictation stop` or the shortcut finishes, Escape cancels",
                )
            } else {
                hotkeys.dictation.suspend();
                ControlReply::error("busy", "the microphone is busy; try again")
            }
        }
        CaptureAction::Stop if !recording && print.is_some() => ControlReply::error(
            "not-recording",
            "dictation isn't recording; nothing to print",
        ),
        CaptureAction::Stop if !recording => ControlReply::unchanged("Dictation isn't recording"),
        CaptureAction::Stop => {
            hotkeys.dictation.suspend();
            let printing = print.is_some();
            let finished = finish_hotkey_dictation(
                capture_boundary(input, release_while_idle),
                recognizer,
                input,
                worker,
                mode,
                context,
                &device,
                events,
                indicator,
                release_while_idle,
                print,
            )?;
            match finished {
                FinishResult::Discarded | FinishResult::Silent if printing => {
                    ControlReply::error("no-speech", "no speech was recorded; nothing to print")
                }
                finished if printing => finish_reply(
                    Some(finished),
                    "Finished recording; printing the transcript",
                ),
                finished => finish_reply(
                    Some(finished),
                    "Finished recording; Hex pastes the text when it's ready",
                ),
            }
        }
        CaptureAction::Cancel if recording => {
            hotkeys.dictation.suspend();
            handle_hotkey_action(
                HotkeyAction::Cancel,
                input.captured_through(),
                recognizer,
                input,
                worker,
                mode,
                context,
                &device,
                events,
                indicator,
                release_while_idle,
            )?;
            ControlReply::done("Cancelled the recording")
        }
        CaptureAction::Cancel | CaptureAction::Toggle => {
            if let Some(refusal) = idle_cancel_refusal(other_recording) {
                refusal
            } else if cancel_latest_job(worker, events, indicator)? {
                ControlReply::done("Cancelled the newest unfinished dictation")
            } else {
                ControlReply::unchanged("Nothing to cancel")
            }
        }
    })
}

/// `hex voice-action`, only while Voice Action is on.
#[allow(clippy::too_many_arguments)]
fn control_voice_action(
    action: CaptureAction,
    other_recording: bool,
    suspended: bool,
    hotkeys: &mut Hotkeys,
    edit_pending_since: &mut Option<CaptureInstant>,
    edit_context: &mut Option<ContextSnapshot>,
    recognizer: &mut Option<Moonshine>,
    input: &DictationAudio,
    worker: &DictationWorker,
    mode: Mode,
    context: &ContextSnapshot,
    events: &mut EventLog,
    indicator: Option<&DictationIndicatorSender>,
    release_while_idle: bool,
) -> Result<ControlReply> {
    let Some(edit) = hotkeys.edit.as_mut() else {
        return Ok(ControlReply::error(
            "feature-off",
            "Voice Action is off; turn it on with `hex settings set voice_action.enabled on`",
        ));
    };
    let recording = edit.is_recording() || edit_context.is_some();
    let device = input.device_name();
    let action = match action {
        CaptureAction::Toggle if recording => CaptureAction::Stop,
        CaptureAction::Toggle => CaptureAction::Start,
        action => action,
    };
    Ok(match action {
        CaptureAction::Start if recording => {
            ControlReply::unchanged("Voice Action is already recording")
        }
        CaptureAction::Start => {
            if let Some(refusal) = capture_refusal(other_recording, suspended) {
                return Ok(refusal);
            }
            if !edit.lock() {
                return Ok(ControlReply::error(
                    "busy",
                    "the Voice Action shortcut is held; release it and try again",
                ));
            }
            *edit_pending_since = None;
            let accepted = handle_edit_hotkey_action(
                HotkeyAction::Start,
                capture_boundary(input, release_while_idle),
                edit_context,
                recognizer,
                input,
                worker,
                mode,
                context,
                &device,
                events,
                indicator,
            )?;
            if accepted {
                make_control_capture_intentional(input, true)?;
                ControlReply::done(
                    "Recording an instruction; `hex voice-action stop` or the shortcut finishes, \
                     Escape cancels",
                )
            } else {
                hotkeys.suspend_edit();
                ControlReply::error(
                    "unavailable",
                    "Voice Action couldn't start; it needs OpenCode (`opencode2`) and a free \
                     microphone",
                )
            }
        }
        CaptureAction::Stop if !recording => {
            ControlReply::unchanged("Voice Action isn't recording")
        }
        CaptureAction::Stop => {
            hotkeys.suspend_edit();
            *edit_pending_since = None;
            let finished = finish_voice_action(
                capture_boundary(input, release_while_idle),
                edit_context,
                recognizer,
                input,
                worker,
                mode,
                &device,
                events,
                indicator,
            )?;
            finish_reply(
                finished,
                "Finished the instruction; Hex pastes the result when it's ready",
            )
        }
        CaptureAction::Cancel if recording => {
            hotkeys.suspend_edit();
            *edit_pending_since = None;
            handle_edit_hotkey_action(
                HotkeyAction::Cancel,
                capture_boundary(input, release_while_idle),
                edit_context,
                recognizer,
                input,
                worker,
                mode,
                context,
                &device,
                events,
                indicator,
            )?;
            ControlReply::done("Cancelled Voice Action")
        }
        CaptureAction::Cancel | CaptureAction::Toggle => {
            if let Some(refusal) = idle_cancel_refusal(other_recording) {
                refusal
            } else if cancel_latest_job(worker, events, indicator)? {
                ControlReply::done("Cancelled the newest unfinished dictation")
            } else {
                ControlReply::unchanged("Nothing to cancel")
            }
        }
    })
}

/// A `hex paste-last` (or the menu bar's Paste Last Dictation) waits for
/// every capture and shortcut gesture to end: pasting cancels the audio
/// owner's capture, which would strand the machine that started it.
fn paste_last_refusal(capture_active: bool) -> Option<ControlReply> {
    capture_active.then(|| {
        ControlReply::error(
            "busy",
            "a recording or shortcut is in progress; finish or cancel it first",
        )
    })
}

/// Pastes the last dictation at the current focus.
fn paste_last(
    worker: &DictationWorker,
    mode: Mode,
    device: &str,
    events: &mut EventLog,
) -> Result<ControlReply> {
    let reply = match worker.paste_last() {
        Ok(()) => ControlReply::done("Pasting the last dictation"),
        Err(error) => {
            feedback::play(Tone::Error);
            events.dictation(DictationPhase::Failed(error.into()), "")?;
            ControlReply::error("failed", format!("couldn't paste: {error}"))
        }
    };
    emit_engine_state(events, false, worker, mode, device)?;
    Ok(reply)
}

/// Ends a Voice Action capture and hands its instruction to OpenCode;
/// `None` when no Voice Action was recording.
#[allow(clippy::too_many_arguments)]
fn finish_voice_action(
    action_at: CaptureInstant,
    edit_context: &mut Option<ContextSnapshot>,
    recognizer: &mut Option<Moonshine>,
    dictation: &DictationAudio,
    worker: &DictationWorker,
    mode: Mode,
    device: &str,
    events: &mut EventLog,
    indicator: Option<&DictationIndicatorSender>,
) -> Result<Option<FinishResult>> {
    let Some(context) = edit_context.take() else {
        dictation.cancel()?;
        return Ok(None);
    };
    let finished = finish_dictation(
        dictation,
        action_at,
        POST_RELEASE_TAIL,
        worker,
        TranscriptionTarget::VoiceAction,
        None,
        &context,
        device,
        events,
        indicator,
    )?;
    reset_command_recognizer(dictation, recognizer)?;
    emit_engine_state(events, false, worker, mode, device)?;
    Ok(Some(finished))
}

fn voice_action_context_snapshot(fallback: &ContextSnapshot) -> ContextSnapshot {
    let mut context = fallback.clone();
    context.selected_text = crate::accessibility::capture_optional();
    context.input_revision = None;
    context
}

#[allow(clippy::too_many_arguments)]
fn handle_edit_hotkey_action(
    action: HotkeyAction,
    action_at: CaptureInstant,
    edit_context: &mut Option<ContextSnapshot>,
    recognizer: &mut Option<Moonshine>,
    dictation: &DictationAudio,
    worker: &DictationWorker,
    mode: Mode,
    context: &ContextSnapshot,
    device: &str,
    events: &mut EventLog,
    indicator: Option<&DictationIndicatorSender>,
) -> Result<bool> {
    match action {
        HotkeyAction::Start => {
            if !admit_voice_action(
                crate::dictation_processor::opencode_installed(),
                edit_context,
                || {
                    dictation.cancel()?;
                    reset_command_recognizer(dictation, recognizer)
                },
            )? {
                feedback::play(Tone::Error);
                events.dictation(
                    DictationPhase::Failed("Voice Action requires OpenCode".into()),
                    "",
                )?;
                emit_engine_state(events, false, worker, mode, device)?;
                return Ok(false);
            }
            let promoted = dictation.is_recording();
            if !promoted {
                if !dictation.start(action_at)? {
                    *edit_context = None;
                    return Ok(false);
                }
                reset_command_recognizer(dictation, recognizer)?;
            }
            let became_intentional = dictation.become_intentional(CaptureInstant::now())?;
            *edit_context = Some(voice_action_context_snapshot(context));
            if became_intentional {
                feedback::play(Tone::DictationStart);
            }
            if !promoted {
                events.dictation(DictationPhase::Started, "")?;
            }
            if let Some(indicator) = indicator {
                indicator.send(if promoted {
                    DictationIndicatorEvent::PromotedToVoiceAction
                } else {
                    DictationIndicatorEvent::EditingStarted
                });
            }
            emit_state(events, true, mode, device)?;
            Ok(true)
        }
        HotkeyAction::Finish => Ok(finish_voice_action(
            action_at,
            edit_context,
            recognizer,
            dictation,
            worker,
            mode,
            device,
            events,
            indicator,
        )?
        .is_some()),
        HotkeyAction::Discard(_) | HotkeyAction::Cancel => {
            *edit_context = None;
            end_capture(
                if matches!(action, HotkeyAction::Cancel) {
                    CaptureEnd::Cancelled
                } else {
                    CaptureEnd::Discarded(action.discard_cause())
                },
                dictation,
                recognizer,
                mode,
                device,
                events,
                indicator,
            )?;
            Ok(true)
        }
        HotkeyAction::PasteLast | HotkeyAction::PasteMeeting => Ok(false),
    }
}

fn admit_voice_action(
    installed: bool,
    edit_context: &mut Option<ContextSnapshot>,
    cancel_capture: impl FnOnce() -> Result<()>,
) -> Result<bool> {
    if installed {
        return Ok(true);
    }
    *edit_context = None;
    cancel_capture()?;
    Ok(false)
}

/// The instant a shortcut edge applies to. A released microphone has no
/// authoritative timeline yet, so the edge uses wall-clock time instead.
fn capture_boundary(input: &DictationAudio, release_while_idle: bool) -> CaptureInstant {
    if release_while_idle {
        CaptureInstant::now()
    } else {
        input.captured_through()
    }
}

fn reset_recognizer(recognizer: Option<&mut Moonshine>) -> Result<()> {
    if let Some(recognizer) = recognizer {
        recognizer.reset_stream()?;
    }
    Ok(())
}

/// Drops queued command audio and restarts the Moonshine stream after the
/// owner already advanced the recognition generation.
fn resync_recognizer(dictation: &DictationAudio, recognizer: Option<&mut Moonshine>) -> Result<()> {
    dictation.discard_recognition_backlog();
    reset_recognizer(recognizer)
}

/// Advances the recognition generation, then resynchronizes the recognizer.
fn reset_command_recognizer(
    dictation: &DictationAudio,
    recognizer: &mut Option<Moonshine>,
) -> Result<()> {
    dictation.invalidate_recognition()?;
    reset_recognizer(recognizer.as_mut())
}

/// Drops the shortcut-, Voice Action-, and voice-owned capture state after the
/// microphone stopped delivering audio. Programmatic ownership stays with the
/// caller because only a recording interruption ends it.
fn abandon_capture(
    hotkeys: &mut Hotkeys,
    voice: &mut Option<VoiceCapture>,
    edit_context: &mut Option<ContextSnapshot>,
    edit_pending_since: &mut Option<CaptureInstant>,
) {
    hotkeys.suspend_all();
    *voice = None;
    *edit_context = None;
    *edit_pending_since = None;
}

fn fail_capture(
    message: String,
    worker: &DictationWorker,
    mode: Mode,
    device: &str,
    events: &mut EventLog,
    indicator: Option<&DictationIndicatorSender>,
) -> Result<()> {
    feedback::play(Tone::Error);
    events.dictation(DictationPhase::Failed(message), "")?;
    if let Some(indicator) = indicator {
        indicator.send(DictationIndicatorEvent::Failed);
    }
    emit_engine_state(events, false, worker, mode, device)
}

#[derive(Clone, Copy)]
enum CaptureEnd {
    /// The capture never became a dictation; no feedback tone.
    Discarded(Option<DiscardCause>),
    /// The user cancelled an intentional capture; plays the cancel tone.
    Cancelled,
}

/// Cancels the active capture and resets command recognition before
/// reporting the ending.
fn end_capture(
    end: CaptureEnd,
    dictation: &DictationAudio,
    recognizer: &mut Option<Moonshine>,
    mode: Mode,
    device: &str,
    events: &mut EventLog,
    indicator: Option<&DictationIndicatorSender>,
) -> Result<()> {
    dictation.cancel()?;
    reset_command_recognizer(dictation, recognizer)?;
    report_capture_end(end, mode, device, events, indicator)
}

fn report_capture_end(
    end: CaptureEnd,
    mode: Mode,
    device: &str,
    events: &mut EventLog,
    indicator: Option<&DictationIndicatorSender>,
) -> Result<()> {
    let indicator_event = match end {
        CaptureEnd::Discarded(cause) => {
            events.discarded_dictation(cause.map(DiscardCause::as_str))?;
            DictationIndicatorEvent::Discarded
        }
        CaptureEnd::Cancelled => {
            feedback::play(Tone::Cancel);
            events.dictation(DictationPhase::Cancelled, "")?;
            DictationIndicatorEvent::Cancelled
        }
    };
    if let Some(indicator) = indicator {
        indicator.send(indicator_event);
    }
    emit_state(events, false, mode, device)
}

#[allow(clippy::too_many_arguments)]
fn handle_command(
    commands: &CommandConfig,
    mode: &mut Mode,
    voice: &mut Option<VoiceCapture>,
    active_protocol: Arc<DictationProtocol>,
    recognizer: &mut Moonshine,
    dictation: &DictationAudio,
    boundary: CaptureInstant,
    recognition_generation: u64,
    action_executor: &ActionExecutor,
    personal_commands: Option<&crate::personal_commands::PersonalCommands>,
    meeting_requests: Option<&SyncSender<MeetingRequest>>,
    heard: &str,
    context: &ContextSnapshot,
    input_activity: &InputActivity,
    device: &str,
    events: &mut EventLog,
    indicator: Option<&DictationIndicatorSender>,
) -> Result<()> {
    let decision = commands.resolve(*mode, heard, context);
    let starts_dictation = matches!(
        &decision,
        Decision::Execute {
            action: Action::StartDictation,
            ..
        }
    );
    if !matches!(&decision, Decision::Ignore)
        && !starts_dictation
        && !dictation.consume_recognition(recognition_generation)?
    {
        return Ok(());
    }
    if matches!(
        &decision,
        Decision::Execute { .. } | Decision::ExecuteSequence { .. }
    ) {
        input_activity.invalidate();
    }
    let decision = match decision {
        Decision::ExecuteSequence { commands } => {
            let submission = action_executor.submit_sequence(
                commands
                    .iter()
                    .map(|command| (command.id, command.action.clone())),
                heard,
                context.label(),
            );
            if submission.is_err() {
                feedback::play(Tone::Error);
            }
            for command in commands {
                events.emit(&VoiceEvent::Command {
                    timestamp_ms: now_ms(),
                    heard: heard.into(),
                    command: Some(command.id.into()),
                    outcome: match submission {
                        Ok(()) => CommandOutcome::Submitted,
                        Err(error) => CommandOutcome::Failed(error.into()),
                    },
                    context: context.label(),
                })?;
            }
            return Ok(());
        }
        decision => decision,
    };
    let (command, outcome) = match decision {
        Decision::Ignore => (None, CommandOutcome::Ignored),
        Decision::Wake => {
            *mode = Mode::Listening;
            feedback::play(Tone::Wake);
            emit_state(events, false, *mode, device)?;
            (Some("mode.wake".into()), CommandOutcome::Woke)
        }
        Decision::Sleep => {
            *mode = Mode::Sleeping;
            feedback::play(Tone::Sleep);
            emit_state(events, false, *mode, device)?;
            (Some("mode.sleep".into()), CommandOutcome::Slept)
        }
        Decision::Execute {
            id,
            action: Action::StartDictation,
        } => {
            let _ = start_voice_capture(
                voice,
                active_protocol,
                recognizer,
                dictation,
                boundary,
                recognition_generation,
                *mode,
                device,
                events,
                indicator,
            )?;
            (Some(id.into()), CommandOutcome::Executed)
        }
        Decision::Execute { id, action }
            if matches!(action, Action::StartMeeting | Action::StopMeeting) =>
        {
            let request = match action {
                Action::StartMeeting => MeetingRequest::Start { title: None },
                Action::StopMeeting => MeetingRequest::Stop,
                _ => unreachable!(),
            };
            let outcome = meeting_requests
                .ok_or("meeting controls require the Hex app")
                .and_then(|requests| {
                    requests.try_send(request).map_err(|error| match error {
                        TrySendError::Full(_) => "meeting controller is busy",
                        TrySendError::Disconnected(_) => "meeting controller is unavailable",
                    })
                });
            submission_outcome(id, outcome, CommandOutcome::Executed)
        }
        Decision::Execute {
            id,
            action:
                Action::InvokeHandler {
                    generation,
                    captures,
                },
        } => {
            let outcome = personal_commands
                .ok_or("personal commands are unavailable")
                .and_then(|runtime| {
                    runtime.invoke(generation, id, heard, context.clone(), captures)
                });
            submission_outcome(id, outcome, CommandOutcome::Submitted)
        }
        Decision::Execute { id, action } => submission_outcome(
            id,
            action_executor.submit(id, action, heard, context.label()),
            CommandOutcome::Submitted,
        ),
        Decision::ExecuteSequence { .. } => unreachable!(),
    };
    events.emit(&VoiceEvent::Command {
        timestamp_ms: now_ms(),
        heard: heard.into(),
        command,
        outcome,
        context: context.label(),
    })?;
    Ok(())
}

/// The command event for a submission: `ok` on success, otherwise a failure
/// outcome with the error tone.
fn submission_outcome(
    id: &str,
    result: Result<(), &str>,
    ok: CommandOutcome,
) -> (Option<String>, CommandOutcome) {
    let outcome = match result {
        Ok(()) => ok,
        Err(error) => {
            feedback::play(Tone::Error);
            CommandOutcome::Failed(error.into())
        }
    };
    (Some(id.into()), outcome)
}

fn handle_action_outcome(outcome: ActionOutcome, events: &mut EventLog) -> Result<()> {
    let command_outcome = match outcome.result {
        Ok(()) => CommandOutcome::Executed,
        Err(error) => {
            feedback::play(Tone::Error);
            CommandOutcome::Failed(error)
        }
    };
    events.emit(&VoiceEvent::Command {
        timestamp_ms: now_ms(),
        heard: outcome.heard,
        command: Some(outcome.id),
        outcome: command_outcome,
        context: outcome.context,
    })?;
    Ok(())
}

fn emit_state(events: &mut EventLog, suppressed: bool, mode: Mode, device: &str) -> Result<()> {
    emit_voice_state(events, suppressed, false, mode, device)
}

fn emit_engine_state(
    events: &mut EventLog,
    capturing: bool,
    worker: &DictationWorker,
    mode: Mode,
    device: &str,
) -> Result<()> {
    emit_voice_state(events, capturing, worker.is_busy(), mode, device)
}

fn emit_voice_state(
    events: &mut EventLog,
    capturing: bool,
    transcribing: bool,
    mode: Mode,
    device: &str,
) -> Result<()> {
    events.emit(&VoiceEvent::State {
        timestamp_ms: now_ms(),
        state: if capturing {
            VoiceState::Dictating
        } else if transcribing {
            VoiceState::Transcribing
        } else if mode == Mode::Sleeping {
            VoiceState::Sleeping
        } else {
            VoiceState::Listening
        },
        device: device.into(),
    })?;
    Ok(())
}

#[allow(clippy::too_many_arguments)]
fn finish_dictation(
    capture: &DictationAudio,
    ended_at: CaptureInstant,
    tail: Duration,
    worker: &DictationWorker,
    target: TranscriptionTarget,
    protocol: Option<Arc<DictationProtocol>>,
    context: &ContextSnapshot,
    device: &str,
    events: &mut EventLog,
    indicator: Option<&DictationIndicatorSender>,
) -> Result<FinishResult> {
    finish_dictation_with(
        capture,
        ended_at,
        tail,
        |clip| worker.transcribe(clip, target, protocol, context.clone()),
        device,
        events,
        indicator,
    )
}

/// Finishes the capture and hands its clip to `submit`.
fn finish_dictation_with(
    capture: &DictationAudio,
    ended_at: CaptureInstant,
    tail: Duration,
    submit: impl FnOnce(DictationClip) -> Result<DictationJobId, &'static str>,
    device: &str,
    events: &mut EventLog,
    indicator: Option<&DictationIndicatorSender>,
) -> Result<FinishResult> {
    let clip = match capture.finish_with_tail(ended_at, tail)? {
        Finish::Transcribe(clip) if !clip.is_near_silent() => clip,
        finish => {
            events.dictation(DictationPhase::Discarded, "")?;
            if let Some(indicator) = indicator {
                indicator.send(DictationIndicatorEvent::Discarded);
            }
            return Ok(match finish {
                Finish::Discard => FinishResult::Discarded,
                Finish::Transcribe(clip) => {
                    crate::dictation_diagnostics::persist_near_silent(
                        &clip.into_transcription_samples(),
                    );
                    FinishResult::Silent
                }
            });
        }
    };
    // The audio owner already announced a release with a tail.
    if tail.is_zero() {
        feedback::play(Tone::DictationStop);
    }
    events.emit(&VoiceEvent::State {
        timestamp_ms: now_ms(),
        state: VoiceState::Transcribing,
        device: device.into(),
    })?;
    events.dictation(DictationPhase::Transcribing, "")?;
    match submit(clip) {
        Ok(job_id) => {
            if let Some(indicator) = indicator {
                indicator.send(DictationIndicatorEvent::Submitted {
                    job_id: job_id.value(),
                });
            }
            Ok(FinishResult::Submitted)
        }
        Err(error) => {
            feedback::play(Tone::Error);
            events.dictation(DictationPhase::Failed(error.into()), "")?;
            if let Some(indicator) = indicator {
                indicator.send(DictationIndicatorEvent::Failed);
            }
            Ok(FinishResult::Rejected(error.into()))
        }
    }
}

#[allow(clippy::too_many_arguments)]
fn handle_voice_dictation_control(
    control: DictationControl,
    heard: &str,
    protocol: Arc<DictationProtocol>,
    dictation: &DictationAudio,
    boundary: CaptureInstant,
    worker: &DictationWorker,
    mode: Mode,
    context: &ContextSnapshot,
    device: &str,
    events: &mut EventLog,
    indicator: Option<&DictationIndicatorSender>,
) -> Result<()> {
    let id = match control {
        DictationControl::Stop => "dictation.stop",
        DictationControl::Send => "dictation.send",
        DictationControl::Cancel => "dictation.cancel",
    };
    match control {
        DictationControl::Stop | DictationControl::Send => {
            let target = match control {
                DictationControl::Send => TranscriptionTarget::Send,
                DictationControl::Stop => TranscriptionTarget::Paste,
                DictationControl::Cancel => unreachable!(),
            };
            let finish = finish_dictation(
                dictation,
                boundary,
                Duration::ZERO,
                worker,
                target,
                Some(protocol),
                context,
                device,
                events,
                indicator,
            )?;
            emit_engine_state(events, false, worker, mode, device)?;
            events.emit(&VoiceEvent::Command {
                timestamp_ms: now_ms(),
                heard: heard.into(),
                command: Some(id.into()),
                outcome: finish_outcome(&finish),
                context: context.label(),
            })?;
            return Ok(());
        }
        DictationControl::Cancel => {
            // The caller already consumed the recognition generation and reset
            // the stream, so only the capture itself is cancelled here.
            dictation.cancel()?;
            report_capture_end(CaptureEnd::Cancelled, mode, device, events, indicator)?;
        }
    }
    events.emit(&VoiceEvent::Command {
        timestamp_ms: now_ms(),
        heard: heard.into(),
        command: Some(id.into()),
        outcome: CommandOutcome::Executed,
        context: context.label(),
    })?;
    Ok(())
}

fn recognition_boundary(
    origin: Option<RecognitionOrigin>,
    generation: u64,
    stream_ms: u64,
    fallback: CaptureInstant,
) -> CaptureInstant {
    origin
        .and_then(|origin| origin.for_generation(generation))
        .and_then(|origin| origin.checked_add(Duration::from_millis(stream_ms)))
        .unwrap_or(fallback)
}

/// Whether an in-flight Option-Command gesture owns a dictation-hotkey
/// action. While the Voice Action gesture is pending, recording, or acting on
/// this event, the Option-only machine must not start, finish, or discard the
/// capture: with a slow chord (Command joining after the hold threshold) both
/// machines are otherwise live, and whichever modifier lifts first would let
/// plain dictation consume the capture and leave the Voice Action to discard
/// silently. Explicit paste shortcuts remain available.
fn voice_action_owns_action(
    action: HotkeyAction,
    edit_pending: bool,
    edit_recording: bool,
    edit_action: Option<HotkeyAction>,
) -> bool {
    (edit_pending || edit_recording || edit_action.is_some())
        && !matches!(action, HotkeyAction::PasteLast | HotkeyAction::PasteMeeting)
}

fn start_pending_voice_action(
    pending_since: &mut Option<CaptureInstant>,
    started_at: CaptureInstant,
    promoted: bool,
    indicator: Option<&DictationIndicatorSender>,
) {
    *pending_since = Some(started_at);
    if let Some(indicator) = indicator {
        indicator.send(if promoted {
            DictationIndicatorEvent::PromotedToVoiceAction
        } else {
            DictationIndicatorEvent::EditingStarted
        });
    }
}

fn pending_voice_action_is_intentional(
    started_at: CaptureInstant,
    finished_at: CaptureInstant,
    action: HotkeyAction,
) -> bool {
    matches!(action, HotkeyAction::Finish)
        && finished_at.duration_since(started_at) >= MINIMUM_HOLD_DURATION
}

fn finish_outcome(result: &FinishResult) -> CommandOutcome {
    match result {
        FinishResult::Submitted => CommandOutcome::Executed,
        FinishResult::Discarded => CommandOutcome::Failed("capture was too short".into()),
        FinishResult::Silent => CommandOutcome::Failed("capture was silent".into()),
        FinishResult::Rejected(error) => CommandOutcome::Failed(error.clone()),
    }
}

fn handle_dictation_event(
    event: WorkerEvent,
    events: &mut EventLog,
    indicator: Option<&DictationIndicatorSender>,
) -> Result<()> {
    match event {
        WorkerEvent::Completed {
            job_id,
            result: Ok(text),
            ..
        } if text.trim().is_empty() => {
            events.dictation(DictationPhase::Discarded, "")?;
            if let Some(indicator) = indicator {
                indicator.send(DictationIndicatorEvent::JobCompleted {
                    job_id: job_id.value(),
                });
            }
        }
        WorkerEvent::Completed {
            job_id,
            target,
            result: Ok(text),
            processing,
        } => {
            let phase = match target {
                TranscriptionTarget::VoiceAction => DictationPhase::VoiceAction,
                TranscriptionTarget::Paste | TranscriptionTarget::Send => DictationPhase::Pasted,
                TranscriptionTarget::Print => DictationPhase::Printed,
                TranscriptionTarget::Service => return Ok(()),
            };
            feedback::play(Tone::DictationDone);
            events.processed_dictation(phase, text, processing)?;
            if let Some(indicator) = indicator {
                indicator.send(DictationIndicatorEvent::JobCompleted {
                    job_id: job_id.value(),
                });
            }
        }
        WorkerEvent::Completed {
            job_id,
            result: Err(error),
            ..
        } => {
            tracing::error!(%error, "dictation failed");
            feedback::play(Tone::Error);
            events.dictation(DictationPhase::Failed(error), "")?;
            if let Some(indicator) = indicator {
                indicator.send(DictationIndicatorEvent::JobFailed {
                    job_id: job_id.value(),
                });
            }
        }
        WorkerEvent::Stage { job_id, stage } => {
            if let Some(indicator) = indicator {
                indicator.send(match stage {
                    DictationJobStage::Transcribing => DictationIndicatorEvent::Transcribing {
                        job_id: job_id.value(),
                    },
                    DictationJobStage::Processing => DictationIndicatorEvent::Processing {
                        job_id: job_id.value(),
                    },
                });
            }
        }
        WorkerEvent::Cancelled { .. } => {}
        WorkerEvent::SpokenCancel { job_id } => {
            feedback::play(Tone::Cancel);
            events.dictation(DictationPhase::Cancelled, "")?;
            if let Some(indicator) = indicator {
                indicator.send(DictationIndicatorEvent::JobCancelled {
                    job_id: job_id.value(),
                });
            }
        }
        WorkerEvent::ModelFailed(error) => {
            tracing::error!(%error, "dictation failed");
            feedback::play(Tone::Error);
            events.dictation(DictationPhase::Failed(error), "")?;
            if let Some(indicator) = indicator {
                indicator.send(DictationIndicatorEvent::Failed);
            }
        }
        WorkerEvent::Pasted {
            kind,
            result: Ok(text),
        } => {
            let phase = match kind {
                PasteKind::LastTranscript => DictationPhase::Repasted,
                PasteKind::MeetingDelta => DictationPhase::MeetingPasted,
            };
            events.dictation(phase, text)?;
        }
        WorkerEvent::Pasted {
            result: Err(error), ..
        } => {
            feedback::play(Tone::Error);
            events.dictation(DictationPhase::Failed(error), "")?;
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::app_settings::{COMMAND_KEY_MASK, OPTION_KEY_MASK};

    #[test]
    fn command_preparation_does_not_block_the_listener_or_propagate_failure() {
        let (release, wait) = mpsc::sync_channel(1);
        let (loaded, receiver) = mpsc::sync_channel(1);
        let caller = std::thread::spawn(move || {
            let pending = spawn_command_load(move || {
                wait.recv().unwrap();
                Err::<(), _>(color_eyre::eyre::eyre!("fixture download failed"))
            });
            loaded.send(pending).unwrap();
        });
        let pending = receiver
            .recv_timeout(Duration::from_secs(2))
            .expect("listener must not wait for the command model");
        assert!(matches!(pending.try_recv(), Err(TryRecvError::Empty)));
        release.send(()).unwrap();
        assert_eq!(
            pending
                .recv_timeout(Duration::from_secs(2))
                .unwrap()
                .unwrap_err(),
            "fixture download failed"
        );
        caller.join().unwrap();
    }

    fn say_protocol() -> DictationProtocol {
        DictationProtocol::try_new(
            vec!["say".into()],
            vec!["say stop".into()],
            vec!["say send".into()],
            vec!["say cancel".into()],
        )
        .unwrap()
    }

    #[test]
    fn programmatic_meter_reports_rms_and_peak_in_decibels() {
        let level = level_for(&[0.5, -0.5, 0.0, 0.0]);
        assert!((level.rms_db - -9.0309).abs() < 0.001);
        assert!((level.peak_db - -6.0206).abs() < 0.001);
        assert_eq!(level_for(&[]).rms_db, -120.0);
    }

    #[test]
    fn control_stops_reply_from_what_the_finish_did() {
        let code = |finished| match finish_reply(finished, "ok") {
            ControlReply::Done { changed: true, .. } => "done".to_owned(),
            ControlReply::Error { code, .. } => code,
            reply => panic!("unexpected {reply:?}"),
        };
        assert_eq!(code(Some(FinishResult::Submitted)), "done");
        assert_eq!(code(Some(FinishResult::Discarded)), "discarded");
        assert_eq!(code(Some(FinishResult::Silent)), "discarded");
        assert_eq!(code(None), "discarded");
        assert_eq!(code(Some(FinishResult::Rejected("full".into()))), "failed");
    }

    #[test]
    fn idle_controls_never_reach_past_an_active_capture() {
        let busy = |reply: Option<ControlReply>| matches!(reply, Some(ControlReply::Error { code, .. }) if code == "busy");
        // Escape cancels the recording before any job, so an idle cancel
        // must not cancel a job while the other capture records.
        assert!(busy(idle_cancel_refusal(true)));
        assert!(idle_cancel_refusal(false).is_none());
        assert!(busy(paste_last_refusal(true)));
        assert!(paste_last_refusal(false).is_none());
    }

    #[test]
    fn programmatic_lease_expires_at_its_deadline() {
        let now = Instant::now();
        let deadline = now + PROGRAMMATIC_LEASE;
        assert!(!programmatic_lease_expired(
            deadline,
            deadline - Duration::from_millis(1)
        ));
        assert!(programmatic_lease_expired(deadline, deadline));
    }

    #[test]
    fn programmatic_completion_cache_is_bounded() {
        let mut completions = VecDeque::new();
        for id in 0..PROGRAMMATIC_COMPLETION_LIMIT as u64 + 2 {
            remember_programmatic_completion(
                &mut completions,
                ProgrammaticCompletion {
                    id,
                    owner_token: format!("owner-{id}"),
                    outcome: ProgrammaticTerminalOutcome::Cancelled,
                },
            );
        }
        assert_eq!(completions.len(), PROGRAMMATIC_COMPLETION_LIMIT);
        assert_eq!(completions.front().map(|completion| completion.id), Some(2));
    }

    #[test]
    fn a_programmatic_dictation_leaves_escape_with_its_owner() {
        // A queued hotkey job or the capture picker would otherwise claim
        // Escape that the listener then ignores.
        assert!(!claims_escape(true, false, true, || true));
        assert!(claims_escape(false, false, false, || true));
        assert!(claims_escape(false, false, true, || false));
        assert!(claims_escape(false, true, false, || false));
        assert!(!claims_escape(false, false, false, || false));
    }

    #[test]
    fn microphone_loss_keeps_a_programmatic_clip_for_finish() {
        let (levels, _levels) = std::sync::mpsc::sync_channel(1);
        let active = |id: u64| ProgrammaticDictation {
            id,
            owner_token: format!("owner-{id}"),
            source: "seer".into(),
            levels: levels.clone(),
            audio: None,
            last_level: Instant::now(),
            lease_deadline: Instant::now() + PROGRAMMATIC_LEASE,
        };
        let finished_error = |completions: &VecDeque<ProgrammaticCompletion>, id: u64| {
            completions
                .iter()
                .find(|completion| completion.matches_owner(id, &format!("owner-{id}")))
                .and_then(|completion| match &completion.outcome {
                    ProgrammaticTerminalOutcome::Finished(Err(code)) => Some(code.clone()),
                    _ => None,
                })
        };
        let mut lost = None;
        let mut completions = VecDeque::new();

        // Audio delivered before the loss waits for the owner's finish.
        programmatic_microphone_lost(
            Some(active(1)),
            Some(DictationClip::for_test(vec![0.25; 16_000])),
            "mic-interrupted",
            &mut lost,
            &mut completions,
        );
        let kept = lost.as_ref().expect("the clip is kept");
        assert!(kept.matches_owner(1, "owner-1"));
        assert_eq!(kept.clip.duration_ms(), 1_000);
        assert!(completions.is_empty());

        // No audio: finish answers microphone-lost, and the kept clip stays.
        programmatic_microphone_lost(
            Some(active(2)),
            None,
            "mic-open-failed",
            &mut lost,
            &mut completions,
        );
        assert_eq!(
            finished_error(&completions, 2).as_deref(),
            Some(MICROPHONE_LOST)
        );
        assert!(lost.as_ref().is_some_and(|lost| lost.id == 1));

        // A later loss supersedes an unfinished kept clip.
        programmatic_microphone_lost(
            Some(active(3)),
            Some(DictationClip::for_test(vec![0.25; 1_600])),
            "mic-gap",
            &mut lost,
            &mut completions,
        );
        assert!(lost.as_ref().is_some_and(|lost| lost.id == 3));
        assert_eq!(
            finished_error(&completions, 1).as_deref(),
            Some(MICROPHONE_LOST)
        );

        // Nothing was recording programmatically: nothing changes.
        programmatic_microphone_lost(None, None, "mic-gap", &mut lost, &mut completions);
        assert_eq!(completions.len(), 2);
    }

    #[test]
    fn recognition_boundary_ignores_an_origin_from_a_stale_generation() {
        let at = CaptureInstant::from_nanos(60_000_000_000);
        let fallback = CaptureInstant::from_nanos(90_000_000_000);
        let origin = Some(RecognitionOrigin { generation: 3, at });

        assert_eq!(
            recognition_boundary(origin, 3, 250, fallback),
            at + Duration::from_millis(250)
        );
        assert_eq!(recognition_boundary(origin, 4, 250, fallback), fallback);
        assert_eq!(recognition_boundary(None, 3, 250, fallback), fallback);
    }

    #[test]
    fn transient_start_hypothesis_does_not_activate_voice_dictation() {
        let now = Instant::now();
        let mut stability = ActivationStability::default();
        let protocol = say_protocol();

        assert!(!stability.observe(&protocol, "say", false, now));
        assert!(!stability.observe(
            &protocol,
            "denormalized",
            false,
            now + Duration::from_millis(200)
        ));
    }

    #[test]
    fn stable_standalone_start_phrase_activates_voice_dictation() {
        let now = Instant::now();
        let mut stability = ActivationStability::default();
        let protocol = say_protocol();

        assert!(!stability.observe(&protocol, "say", false, now));
        assert!(stability.observe(&protocol, "Say.", false, now + Duration::from_millis(200)));
    }

    #[test]
    fn stable_start_prefix_with_dictated_speech_activates() {
        let now = Instant::now();
        let mut stability = ActivationStability::default();
        let protocol = say_protocol();

        assert!(!stability.observe(&protocol, "say", false, now));
        assert!(stability.observe(
            &protocol,
            "say something about normalization",
            false,
            now + Duration::from_millis(200)
        ));
    }

    #[test]
    fn completed_standalone_start_phrase_activates_immediately() {
        let mut stability = ActivationStability::default();

        assert!(stability.observe(&say_protocol(), "Say!", true, Instant::now()));
    }

    #[test]
    fn slow_chord_release_keeps_voice_action_ownership() {
        use crate::app_settings::RuntimeHotkey;
        use crate::suppression::{DictationHotkey, InputEvent};

        let now = CaptureInstant::from_nanos(60_000_000_000);
        let mut dictation = DictationHotkey::new(
            now,
            true,
            RuntimeHotkey {
                modifiers: crate::app_settings::modifiers_from_flags(OPTION_KEY_MASK),
                key_code: None,
            },
        );
        let mut edit = DictationHotkey::new_without_paste(
            now,
            RuntimeHotkey {
                modifiers: crate::app_settings::modifiers_from_flags(
                    OPTION_KEY_MASK | COMMAND_KEY_MASK,
                ),
                key_code: None,
            },
        );

        dictation.suspend();
        edit.suspend();

        // Option down: plain dictation starts, no Voice Action gesture yet.
        let event = InputEvent::Flags(OPTION_KEY_MASK);
        let edit_action = edit.process(event, now);
        let action = dictation.process(event, now).expect("dictation starts");
        assert_eq!(action, HotkeyAction::Start);
        assert!(!voice_action_owns_action(
            action,
            false,
            edit.is_recording(),
            edit_action,
        ));

        // Command joins after the hold threshold: the dictation machine stays
        // silently recording while the edit machine starts the gesture.
        let chord_at = now + MINIMUM_HOLD_DURATION + Duration::from_millis(150);
        let event = InputEvent::Flags(OPTION_KEY_MASK | COMMAND_KEY_MASK);
        let edit_action = edit.process(event, chord_at);
        assert_eq!(edit_action, Some(HotkeyAction::Start));
        assert_eq!(dictation.process(event, chord_at), None);

        // Option lifts first: both machines emit Finish on the same event.
        // The Voice Action gesture must own the dictation Finish, or plain
        // dictation would consume the capture out from under it.
        let release_at = chord_at + Duration::from_secs(1);
        let event = InputEvent::Flags(COMMAND_KEY_MASK);
        let edit_action = edit.process(event, release_at);
        assert_eq!(edit_action, Some(HotkeyAction::Finish));
        let action = dictation
            .process(event, release_at)
            .expect("dictation machine also finishes");
        assert_eq!(action, HotkeyAction::Finish);
        assert!(voice_action_owns_action(
            action,
            true,
            edit.is_recording(),
            edit_action,
        ));
        let _ = dictation.suspend();

        // A later plain Option dictation is unaffected.
        let idle_at = release_at + Duration::from_secs(2);
        let event = InputEvent::Flags(OPTION_KEY_MASK);
        let edit_action = edit.process(event, idle_at);
        let action = dictation.process(event, idle_at).expect("dictation starts");
        assert_eq!(action, HotkeyAction::Start);
        assert!(!voice_action_owns_action(
            action,
            false,
            edit.is_recording(),
            edit_action,
        ));
    }

    #[test]
    fn rejected_voice_action_cancels_promoted_capture() {
        use crate::app_settings::RuntimeHotkey;
        use crate::dictation::DictationCapture;
        use crate::suppression::InputEvent;

        let now = CaptureInstant::from_nanos(60_000_000_000);
        let mut hotkey = DictationHotkey::new(
            now,
            true,
            RuntimeHotkey {
                modifiers: crate::app_settings::modifiers_from_flags(OPTION_KEY_MASK),
                key_code: None,
            },
        );
        let mut edit_hotkey = DictationHotkey::new_without_paste(
            now,
            RuntimeHotkey {
                modifiers: crate::app_settings::modifiers_from_flags(
                    OPTION_KEY_MASK | COMMAND_KEY_MASK,
                ),
                key_code: None,
            },
        );
        hotkey.suspend();
        edit_hotkey.suspend();
        let mut capture = DictationCapture::new(16_000);
        let option = InputEvent::Flags(OPTION_KEY_MASK);
        assert_eq!(edit_hotkey.process(option, now), None);
        assert_eq!(hotkey.process(option, now), Some(HotkeyAction::Start));
        capture.start_at(now);

        let chord_at = now + Duration::from_millis(40);
        let chord = InputEvent::Flags(OPTION_KEY_MASK | COMMAND_KEY_MASK);
        let edit_action = edit_hotkey.process(chord, chord_at);
        assert_eq!(edit_action, Some(HotkeyAction::Start));
        let mut pending = None;
        start_pending_voice_action(&mut pending, chord_at, capture.is_recording(), None);
        let action = hotkey.process(chord, chord_at).unwrap();
        assert!(voice_action_owns_action(
            action,
            pending.is_some(),
            edit_hotkey.is_recording(),
            edit_action,
        ));
        hotkey.suspend();

        let rejected_at = chord_at + MINIMUM_HOLD_DURATION;
        capture.ingest(&[0.5; 5_440], rejected_at);
        let mut edit_context = Some(ContextSnapshot::default());
        assert!(
            !admit_voice_action(false, &mut edit_context, || {
                capture.cancel();
                Ok(())
            })
            .unwrap()
        );
        edit_hotkey.suspend();
        assert!(!capture.is_recording());
        assert!(edit_context.is_none());

        let released_at = rejected_at + Duration::from_millis(100);
        assert_eq!(hotkey.process(InputEvent::Flags(0), released_at), None);
        assert_eq!(edit_hotkey.process(InputEvent::Flags(0), released_at), None);
        capture.ingest(&[0.5; 1_600], released_at);
        assert!(matches!(capture.finish(released_at), Finish::Discard));
        assert_eq!(
            hotkey.process(option, released_at + Duration::from_secs(1)),
            Some(HotkeyAction::Start)
        );
    }

    #[test]
    fn accepted_voice_action_preserves_promoted_capture() {
        let mut capture = crate::dictation::DictationCapture::new(16_000);
        let now = CaptureInstant::from_nanos(60_000_000_000);
        capture.start_at(now);
        let mut edit_context = None;

        assert!(
            admit_voice_action(true, &mut edit_context, || {
                capture.cancel();
                Ok(())
            })
            .unwrap()
        );
        assert!(capture.is_recording());
    }

    #[test]
    fn explicit_paste_shortcuts_stay_available_during_a_voice_action_gesture() {
        assert!(!voice_action_owns_action(
            HotkeyAction::PasteLast,
            true,
            true,
            None,
        ));
        assert!(voice_action_owns_action(
            HotkeyAction::Finish,
            false,
            false,
            Some(HotkeyAction::Finish),
        ));
        assert!(!voice_action_owns_action(
            HotkeyAction::Finish,
            false,
            false,
            None,
        ));
    }

    #[test]
    fn delayed_voice_action_edges_use_their_source_timestamps() {
        let started_at = CaptureInstant::from_nanos(1_000_000_000);

        assert!(!pending_voice_action_is_intentional(
            started_at,
            started_at + Duration::from_millis(299),
            HotkeyAction::Finish,
        ));
        assert!(pending_voice_action_is_intentional(
            started_at,
            started_at + MINIMUM_HOLD_DURATION,
            HotkeyAction::Finish,
        ));
        assert!(!pending_voice_action_is_intentional(
            started_at,
            started_at + Duration::from_secs(1),
            HotkeyAction::Discard(DiscardCause::Key),
        ));
    }

    #[test]
    fn pending_voice_action_updates_the_hud_before_the_hold_threshold() {
        let started_at = CaptureInstant::now();
        let (indicator, events) = crate::dictation_indicator::channel();
        let mut pending_since = None;

        start_pending_voice_action(&mut pending_since, started_at, false, Some(&indicator));

        assert_eq!(pending_since, Some(started_at));
        assert!(matches!(
            events.try_recv(),
            Ok(DictationIndicatorEvent::EditingStarted)
        ));
    }
}
