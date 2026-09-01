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
use crate::dictation::{
    ControlStability, DictationControl, DictationProtocol, Finish, MINIMUM_HOLD_DURATION,
};
use crate::dictation_audio::{DictationAudio, DictationAudioEvent};
use crate::dictation_indicator::{DictationIndicatorEvent, DictationIndicatorSender};
use crate::events::{
    CommandOutcome, DictationPhase, DictationProcessing, EventLog, TranscriptPhase, VoiceEvent,
    VoiceState, now_ms,
};
use crate::feedback::{self, Tone};
use crate::meeting::MeetingRequest;
use crate::moonshine::Moonshine;
use crate::parakeet::{
    DictationJobStage, DictationWorker, PasteKind, TranscriptionTarget, WorkerEvent,
};
use crate::recording_environment::RecordingEnvironmentController;
use crate::spoken_text::normalize;
use crate::suppression::{DictationHotkey, HotkeyAction, InputActivity, InputMonitor};

const UPDATE_INTERVAL: Duration = Duration::from_millis(200);
const ACTIVATION_STABILITY_WINDOW: Duration = Duration::from_millis(750);
const PROGRAMMATIC_LEASE: Duration = Duration::from_secs(10);
const PROGRAMMATIC_COMPLETION_LIMIT: usize = 16;

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
    Submitted,
    Rejected(String),
}

#[derive(Debug)]
pub enum RecognitionControl {
    PasteLast,
    RetryCommands,
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

struct FinishingProgrammaticDictation {
    id: u64,
    owner_token: String,
    duration_ms: u64,
    replies: Vec<SyncSender<Result<ProgrammaticDictationResult, String>>>,
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
    feedback::preload()?;
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
    let mut hotkey = DictationHotkey::new(
        CaptureInstant::now(),
        crate::app_settings::double_tap_lock(),
        input_monitor.paste_key_code,
        crate::app_settings::dictation_hotkey(),
    );
    hotkey.set_double_tap_only(crate::app_settings::double_tap_only());
    let mut edit_hotkey = crate::app_settings::edit_hotkey().map(|binding| {
        DictationHotkey::new_without_paste(
            CaptureInstant::now(),
            input_monitor.paste_key_code,
            binding,
        )
    });
    let mut edit_context = None;
    let mut edit_pending_since = None;
    let mut mode = Mode::Listening;
    let mut voice_protocol: Option<Arc<DictationProtocol>> = None;
    let mut activation_stability = ActivationStability::default();
    let mut control_stability = ControlStability::default();
    let mut last_update = Instant::now();
    let mut recognition_origin = None::<(u64, CaptureInstant)>;
    let recording_environment = RecordingEnvironmentController::start();
    let (mut microphone_revision, microphone) = crate::app_settings::microphone_selection();
    let input = DictationAudio::open(
        device_override,
        microphone_revision,
        microphone.as_deref(),
        recording_environment,
        input_monitor.pending_events(),
        microphone_policy.release_while_idle,
    )?;
    let dictation_worker = DictationWorker::start(
        input_monitor.activity.clone(),
        transformations.clone(),
        history,
    );
    let (mut transcription_revision, _) = crate::app_settings::transcription_selection();
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
    let mut programmatic_results =
        BTreeMap::<crate::parakeet::DictationJobId, FinishingProgrammaticDictation>::new();
    let mut programmatic_completions = VecDeque::<ProgrammaticCompletion>::new();

    events.emit(&VoiceEvent::SessionStarted {
        timestamp_ms: now_ms(),
    })?;
    if input.is_recovering() {
        hotkey.suspend();
        edit_hotkey.as_mut().and_then(DictationHotkey::suspend);
    }
    if hotkey.is_recording() {
        let accepted = input.start(if microphone_policy.release_while_idle {
            CaptureInstant::now()
        } else {
            input.captured_through()
        })?;
        if accepted {
            events.dictation(DictationPhase::Started, "")?;
            if let Some(indicator) = &indicator {
                indicator.send(DictationIndicatorEvent::Started);
            }
        } else {
            hotkey.suspend();
        }
    }
    if edit_hotkey
        .as_ref()
        .is_some_and(DictationHotkey::is_recording)
    {
        let accepted = input.start(if microphone_policy.release_while_idle {
            CaptureInstant::now()
        } else {
            input.captured_through()
        })?;
        if accepted {
            edit_context = Some(voice_action_context_snapshot(&context));
            events.dictation(DictationPhase::Started, "")?;
            if let Some(indicator) = &indicator {
                indicator.send(DictationIndicatorEvent::EditingStarted);
            }
        } else {
            edit_hotkey.as_mut().and_then(DictationHotkey::suspend);
        }
    }
    emit_state(
        &mut events,
        hotkey.is_recording()
            || edit_hotkey
                .as_ref()
                .is_some_and(DictationHotkey::is_recording),
        mode,
        &input.device_name(),
    )?;
    if commands_enabled {
        tracing::info!(device = %input.device_name(), "voice recognition started");
    } else {
        tracing::info!(device = %input.device_name(), "dictation listener started");
    }

    while !shutdown.load(Ordering::Relaxed) {
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
                    RecognitionControl::PasteLast if programmatic.is_none() => {
                        handle_hotkey_action(
                            HotkeyAction::PasteLast,
                            CaptureInstant::now(),
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
                    RecognitionControl::PasteLast => {}
                    RecognitionControl::StartDictation {
                        source,
                        owner_token,
                        levels,
                        reply,
                    } => {
                        if programmatic.is_some()
                            || input.is_recording()
                            || hotkey.is_recording()
                            || edit_hotkey
                                .as_ref()
                                .is_some_and(DictationHotkey::is_recording)
                            || voice_protocol.is_some()
                        {
                            let _ = reply.send(Err("dictation-busy".into()));
                            continue;
                        }
                        let id = next_programmatic_id;
                        next_programmatic_id = next_programmatic_id.wrapping_add(1).max(1);
                        let boundary = if microphone_policy.release_while_idle {
                            CaptureInstant::now()
                        } else {
                            input.captured_through()
                        };
                        match input.start_programmatic(boundary) {
                            Ok(()) => {
                                hotkey.disarm_pending_gesture();
                                if let Some(edit) = &mut edit_hotkey {
                                    edit.disarm_pending_gesture();
                                }
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
                                input_monitor.set_escape_cancels(true);
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
                            .filter(|active| active.id == id && active.owner_token == owner_token)
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
                        if let Some(finishing) =
                            programmatic_results.values_mut().find(|finishing| {
                                finishing.id == id && finishing.owner_token == owner_token
                            })
                        {
                            finishing.replies.push(reply);
                            continue;
                        }
                        if let Some(completion) =
                            programmatic_completions.iter().rev().find(|completion| {
                                completion.id == id && completion.owner_token == owner_token
                            })
                        {
                            let result = match &completion.outcome {
                                ProgrammaticTerminalOutcome::Finished(result) => result.clone(),
                                ProgrammaticTerminalOutcome::Cancelled => Err("cancelled".into()),
                            };
                            let _ = reply.send(result);
                            continue;
                        }
                        if programmatic.as_ref().is_none_or(|active| {
                            active.id != id || active.owner_token != owner_token
                        }) {
                            let _ = reply.send(Err("dictation-not-active".into()));
                            continue;
                        }
                        let active = programmatic.take().expect("active dictation was checked");
                        let boundary = if microphone_policy.release_while_idle {
                            CaptureInstant::now()
                        } else {
                            input.captured_through()
                        };
                        match input.finish(boundary)? {
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
                                                owner_token,
                                                duration_ms,
                                                replies: vec![reply],
                                            },
                                        );
                                        tracing::info!(
                                            dictation_id = id,
                                            source = active.source,
                                            "programmatic dictation submitted"
                                        );
                                    }
                                    Err(error) => {
                                        let result = Err(error.into());
                                        let _ = reply.send(result.clone());
                                        remember_programmatic_completion(
                                            &mut programmatic_completions,
                                            ProgrammaticCompletion {
                                                id,
                                                owner_token,
                                                outcome: ProgrammaticTerminalOutcome::Finished(
                                                    result,
                                                ),
                                            },
                                        );
                                    }
                                }
                            }
                            Finish::Discard => {
                                let result = Err("capture-discarded".into());
                                let _ = reply.send(result.clone());
                                remember_programmatic_completion(
                                    &mut programmatic_completions,
                                    ProgrammaticCompletion {
                                        id,
                                        owner_token,
                                        outcome: ProgrammaticTerminalOutcome::Finished(result),
                                    },
                                );
                            }
                        }
                        reset_command_recognizer(&input, &mut recognizer)?;
                    }
                    RecognitionControl::CancelDictation {
                        id,
                        owner_token,
                        reply,
                    } => {
                        if let Some(completion) =
                            programmatic_completions.iter().rev().find(|completion| {
                                completion.id == id && completion.owner_token == owner_token
                            })
                            && matches!(completion.outcome, ProgrammaticTerminalOutcome::Cancelled)
                        {
                            let _ = reply.send(Ok(()));
                            continue;
                        }
                        if programmatic_results.values().any(|finishing| {
                            finishing.id == id && finishing.owner_token == owner_token
                        }) {
                            let _ = reply.send(Err("dictation-finishing".into()));
                            continue;
                        }
                        if programmatic.as_ref().is_none_or(|active| {
                            active.id != id || active.owner_token != owner_token
                        }) {
                            let _ = reply.send(Err("dictation-not-active".into()));
                            continue;
                        }
                        let active = programmatic.take().expect("active dictation was checked");
                        input.cancel()?;
                        reset_command_recognizer(&input, &mut recognizer)?;
                        tracing::info!(
                            dictation_id = id,
                            source = active.source,
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
                    }
                    RecognitionControl::HeartbeatDictation {
                        id,
                        owner_token,
                        reply,
                    } => {
                        let Some(active) = programmatic
                            .as_mut()
                            .filter(|active| active.id == id && active.owner_token == owner_token)
                        else {
                            let _ = reply.send(Err("dictation-not-active".into()));
                            continue;
                        };
                        active.lease_deadline = Instant::now() + PROGRAMMATIC_LEASE;
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
            input.cancel()?;
            reset_command_recognizer(&input, &mut recognizer)?;
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
                    recognition_origin = None;
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
        hotkey.set_double_tap_enabled(crate::app_settings::double_tap_lock());
        hotkey.set_double_tap_only(crate::app_settings::double_tap_only());
        hotkey.set_binding(crate::app_settings::dictation_hotkey());
        match crate::app_settings::edit_hotkey() {
            Some(binding) => match &mut edit_hotkey {
                Some(edit) => edit.set_binding(binding),
                None => {
                    let mut edit = DictationHotkey::new_without_paste(
                        CaptureInstant::now(),
                        input_monitor.paste_key_code,
                        binding,
                    );
                    edit.wait_for_release();
                    edit_hotkey = Some(edit);
                }
            },
            None => {
                if let Some(mut edit) = edit_hotkey.take() {
                    let was_recording = edit.suspend().is_some();
                    let was_pending = edit_pending_since.take().is_some();
                    if (was_recording || was_pending || edit_context.is_some())
                        && voice_protocol.is_none()
                    {
                        hotkey.suppress_until_release();
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
        if hotkey_capture_suspended && voice_protocol.is_none() {
            edit_pending_since = None;
            let normal_action = hotkey.suspend();
            let edit_action = edit_hotkey.as_mut().and_then(DictationHotkey::suspend);
            if normal_action.is_some() || edit_action.is_some() {
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
                )?;
            }
        }
        while let Ok(observed) = input_monitor.events.try_recv() {
            let _acknowledge = input_monitor.acknowledge_after(observed);
            let input_event = observed.event;
            let capture_at = observed.capture_at;
            if programmatic.is_some() {
                hotkey.track_key_state(input_event, capture_at);
                if let Some(edit) = &mut edit_hotkey {
                    edit.track_key_state(input_event, capture_at);
                }
            }
            if programmatic.is_some() && input_event.is_escape_down() {
                let cancelled = programmatic
                    .take()
                    .expect("programmatic dictation is active");
                input.cancel()?;
                reset_command_recognizer(&input, &mut recognizer)?;
                feedback::play(Tone::Cancel);
                remember_programmatic_completion(
                    &mut programmatic_completions,
                    ProgrammaticCompletion {
                        id: cancelled.id,
                        owner_token: cancelled.owner_token,
                        outcome: ProgrammaticTerminalOutcome::Cancelled,
                    },
                );
                continue;
            } else if programmatic.is_some() {
                continue;
            }
            if hotkey_capture_suspended {
                continue;
            }
            if voice_protocol.is_some() && input_event.is_escape_down() {
                voice_protocol = None;
                control_stability.reset();
                reset_command_recognizer(&input, &mut recognizer)?;
                input.cancel()?;
                feedback::play(Tone::Cancel);
                events.dictation(DictationPhase::Cancelled, "")?;
                if let Some(indicator) = &indicator {
                    indicator.send(DictationIndicatorEvent::Cancelled);
                }
                emit_state(&mut events, false, mode, &input.device_name())?;
                continue;
            }
            if !hotkey.is_recording()
                && !edit_hotkey
                    .as_ref()
                    .is_some_and(DictationHotkey::is_recording)
                && input_event.is_escape_down()
                && let Some(job_id) = dictation_worker.cancel_latest()
            {
                feedback::play(Tone::Cancel);
                events.dictation(DictationPhase::Cancelled, "")?;
                if let Some(indicator) = &indicator {
                    indicator.send(DictationIndicatorEvent::JobCancelled {
                        job_id: job_id.value(),
                    });
                }
                continue;
            }
            let edit_action = edit_hotkey
                .as_mut()
                .and_then(|edit| edit.process(input_event, capture_at));
            if matches!(edit_action, Some(HotkeyAction::Start)) && voice_protocol.is_none() {
                start_pending_voice_action(
                    &mut edit_pending_since,
                    capture_at,
                    input.is_recording(),
                    indicator.as_ref(),
                );
            }
            if let Some(action) = hotkey.process(input_event, capture_at) {
                if matches!(
                    action,
                    HotkeyAction::PasteLast
                        | HotkeyAction::RewriteLast
                        | HotkeyAction::PasteMeeting
                ) {
                    voice_protocol = None;
                    control_stability.reset();
                }
                let voice_action_takeover = voice_action_owns_action(
                    action,
                    edit_pending_since.is_some(),
                    edit_hotkey
                        .as_ref()
                        .is_some_and(DictationHotkey::is_recording),
                    edit_action,
                );
                if voice_action_takeover {
                    let _ = hotkey.suspend();
                } else if voice_protocol.is_none() {
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
                    )?;
                    if !accepted {
                        hotkey.suspend();
                    }
                }
            }
            if let Some(action) = edit_action
                && voice_protocol.is_none()
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
                            edit_hotkey.as_mut().and_then(DictationHotkey::suspend);
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
                        input.cancel()?;
                        reset_command_recognizer(&input, &mut recognizer)?;
                        events.dictation(DictationPhase::Discarded, "")?;
                        if let Some(indicator) = &indicator {
                            indicator.send(DictationIndicatorEvent::Discarded);
                        }
                        emit_state(&mut events, false, mode, &input.device_name())?;
                    }
                }
            }
        }
        if programmatic.is_none()
            && voice_protocol.is_none()
            && !hotkey_capture_suspended
            && input_monitor.pending_events().oldest().is_none()
        {
            hotkey.recover_stale_keys();
            if let Some(edit) = &mut edit_hotkey {
                edit.recover_stale_keys();
            }
        }
        if hotkey.is_recording()
            && edit_pending_since.is_none()
            && input.become_intentional(if microphone_policy.release_while_idle {
                CaptureInstant::now()
            } else {
                input.captured_through()
            })?
        {
            feedback::play(Tone::DictationStart);
        }
        if edit_pending_since.is_some_and(|started| {
            (if microphone_policy.release_while_idle {
                CaptureInstant::now()
            } else {
                input.captured_through()
            })
            .duration_since(started)
                >= MINIMUM_HOLD_DURATION
        }) && edit_hotkey
            .as_ref()
            .is_some_and(DictationHotkey::is_recording)
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
                edit_hotkey.as_mut().and_then(DictationHotkey::suspend);
                if let Some(indicator) = &indicator {
                    indicator.send(DictationIndicatorEvent::Failed);
                }
            }
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
                        let result = result.map(|transcript| ProgrammaticDictationResult {
                            transcript,
                            duration_ms: finishing.duration_ms,
                        });
                        for reply in finishing.replies {
                            let _ = reply.send(result.clone());
                        }
                        remember_programmatic_completion(
                            &mut programmatic_completions,
                            ProgrammaticCompletion {
                                id: finishing.id,
                                owner_token: finishing.owner_token,
                                outcome: ProgrammaticTerminalOutcome::Finished(result),
                            },
                        );
                    }
                    continue;
                }
                WorkerEvent::Cancelled { job_id } => {
                    if let Some(finishing) = programmatic_results.remove(&job_id) {
                        let result = Err("cancelled".into());
                        for reply in finishing.replies {
                            let _ = reply.send(result.clone());
                        }
                        remember_programmatic_completion(
                            &mut programmatic_completions,
                            ProgrammaticCompletion {
                                id: finishing.id,
                                owner_token: finishing.owner_token,
                                outcome: ProgrammaticTerminalOutcome::Finished(result),
                            },
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
        input_monitor.set_escape_cancels(
            hotkey.is_recording()
                || edit_hotkey
                    .as_ref()
                    .is_some_and(DictationHotkey::is_recording)
                || voice_protocol.is_some()
                || programmatic.is_some()
                || dictation_worker.pending_count() > 0,
        );
        if received_worker_event {
            emit_engine_state(
                &mut events,
                hotkey.is_recording()
                    || edit_hotkey
                        .as_ref()
                        .is_some_and(DictationHotkey::is_recording)
                    || voice_protocol.is_some(),
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
                    programmatic = None;
                    hotkey.suspend();
                    edit_hotkey.as_mut().and_then(DictationHotkey::suspend);
                    edit_context = None;
                    edit_pending_since = None;
                    feedback::play(Tone::Error);
                    events.dictation(
                        DictationPhase::Failed(format!("Could not open microphone: {error}")),
                        "",
                    )?;
                    if let Some(indicator) = &indicator {
                        indicator.send(DictationIndicatorEvent::Failed);
                    }
                    emit_engine_state(
                        &mut events,
                        false,
                        &dictation_worker,
                        mode,
                        &input.device_name(),
                    )?;
                }
                DictationAudioEvent::RecognitionDiscontinuity { dropped_frames } => {
                    tracing::warn!(
                        dropped_recognition_frames = dropped_frames,
                        dropped_recognition_ms =
                            dropped_frames * 1_000 / u64::from(input.sample_rate()),
                        "command recognition fell behind; dictation capture remained continuous"
                    );
                    input.discard_recognition_backlog();
                    reset_recognizer(&mut recognizer)?;
                    recognition_origin = None;
                }
                DictationAudioEvent::CaptureDiscontinuity {
                    was_recording,
                    gap_ms,
                    ..
                } => {
                    input.discard_recognition_backlog();
                    reset_recognizer(&mut recognizer)?;
                    recognition_origin = None;
                    if was_recording {
                        programmatic = None;
                        hotkey.suspend();
                        edit_hotkey.as_mut().and_then(DictationHotkey::suspend);
                        voice_protocol = None;
                        edit_context = None;
                        edit_pending_since = None;
                        feedback::play(Tone::Error);
                        events.dictation(
                            DictationPhase::Failed(format!(
                                "Microphone audio was interrupted for {gap_ms} ms."
                            )),
                            "",
                        )?;
                        if let Some(indicator) = &indicator {
                            indicator.send(DictationIndicatorEvent::Failed);
                        }
                        emit_engine_state(
                            &mut events,
                            false,
                            &dictation_worker,
                            mode,
                            &input.device_name(),
                        )?;
                    }
                }
                DictationAudioEvent::Reopened => {
                    input.discard_recognition_backlog();
                    reset_recognizer(&mut recognizer)?;
                    recognition_origin = None;
                    if !input.is_recording() {
                        emit_state(&mut events, false, mode, &input.device_name())?;
                    }
                }
                DictationAudioEvent::Interrupted { was_recording, .. } => {
                    if was_recording {
                        programmatic = None;
                    }
                    voice_protocol = None;
                    control_stability.reset();
                    edit_context = None;
                    edit_pending_since = None;
                    hotkey.suspend();
                    edit_hotkey.as_mut().and_then(DictationHotkey::suspend);
                    input.discard_recognition_backlog();
                    reset_recognizer(&mut recognizer)?;
                    recognition_origin = None;
                    if was_recording {
                        feedback::play(Tone::Error);
                        events.dictation(
                            DictationPhase::Failed(
                                "Microphone capture was interrupted; reconnecting.".into(),
                            ),
                            "",
                        )?;
                        if let Some(indicator) = &indicator {
                            indicator.send(DictationIndicatorEvent::Failed);
                        }
                        emit_engine_state(
                            &mut events,
                            false,
                            &dictation_worker,
                            mode,
                            &input.device_name(),
                        )?;
                    }
                }
            }
        }

        let audio = input.recv_timeout(Duration::from_millis(20))?;
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
            && !hotkey.suppresses_recognition()
            && !edit_hotkey
                .as_ref()
                .is_some_and(DictationHotkey::suppresses_recognition)
        {
            let generation = input.recognition_generation();
            if recognition_origin.is_none_or(|(current, _)| current != generation) {
                recognition_origin = Some((generation, audio.captured_from()));
            }
        }
        let mut fed_generation = None;
        match audio {
            Some(audio) if !audio.is_current(input.recognition_generation()) => {}
            Some(audio) if voice_protocol.is_some() => {
                if let Some(indicator) = &indicator {
                    indicator.meter(&audio.samples);
                }
                if let Some(recognizer) = &mut recognizer {
                    let generation = input.recognition_generation();
                    recognizer.add_audio(&audio.samples, input.sample_rate())?;
                    fed_generation = Some(generation);
                }
            }
            Some(audio)
                if programmatic.is_some()
                    || hotkey.suppresses_recognition()
                    || edit_hotkey
                        .as_ref()
                        .is_some_and(DictationHotkey::suppresses_recognition) =>
            {
                if let Some(indicator) = &indicator {
                    indicator.meter(&audio.samples);
                }
            }
            Some(audio) => {
                if let Some(recognizer) = &mut recognizer {
                    let generation = input.recognition_generation();
                    recognizer.add_audio(&audio.samples, input.sample_rate())?;
                    fed_generation = Some(generation);
                }
            }
            None => {}
        }
        if fed_generation.is_some_and(|generation| generation != input.recognition_generation()) {
            input.discard_recognition_backlog();
            reset_recognizer(&mut recognizer)?;
            recognition_origin = None;
            continue;
        }
        if !input.is_recovering()
            && programmatic.is_none()
            && !hotkey.suppresses_recognition()
            && !edit_hotkey
                .as_ref()
                .is_some_and(DictationHotkey::suppresses_recognition)
            && last_update.elapsed() >= UPDATE_INTERVAL
            && let Some(recognizer) = &mut recognizer
            && let Some(action_executor) = &action_executor
        {
            let generation = input.recognition_generation();
            if recognition_origin
                .is_none_or(|(origin_generation, _)| origin_generation != generation)
            {
                input.discard_recognition_backlog();
                recognizer.reset_stream()?;
                recognition_origin = None;
                last_update = Instant::now();
                continue;
            }
            let updates = recognizer.update()?;
            if generation != input.recognition_generation() {
                input.discard_recognition_backlog();
                recognizer.reset_stream()?;
                recognition_origin = None;
                last_update = Instant::now();
                continue;
            }
            for update in updates {
                if generation != input.recognition_generation() {
                    input.discard_recognition_backlog();
                    recognizer.reset_stream()?;
                    recognition_origin = None;
                    break;
                }
                let active_runtime = personal_commands
                    .as_ref()
                    .map_or_else(|| native_runtime.clone(), |runtime| runtime.snapshot());
                let is_completed = matches!(update.phase, TranscriptPhase::Completed);
                let activate = if mode == Mode::Listening && voice_protocol.is_none() {
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
                        &mut voice_protocol,
                        active_runtime.dictation.clone(),
                        &mut control_stability,
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
                if let Some(protocol) = &voice_protocol {
                    let control = protocol
                        .control_suffix(&update.text)
                        .map(|(control, _)| control);
                    if let Some(control) = control_stability.observe(control, is_completed) {
                        if !input.consume_recognition(generation)? {
                            break;
                        }
                        let boundary = recognition_boundary(
                            recognition_origin,
                            generation,
                            update.end_ms,
                            input.captured_through(),
                        );
                        let protocol = voice_protocol.take().expect("voice protocol is active");
                        recognition_origin = None;
                        recognizer.reset_stream()?;
                        handle_voice_dictation_control(
                            control,
                            &update.text,
                            protocol,
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
                        &mut voice_protocol,
                        active_runtime.dictation.clone(),
                        &mut control_stability,
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

fn remember_programmatic_completion(
    completions: &mut VecDeque<ProgrammaticCompletion>,
    completion: ProgrammaticCompletion,
) {
    completions.push_back(completion);
    while completions.len() > PROGRAMMATIC_COMPLETION_LIMIT {
        completions.pop_front();
    }
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
    voice_protocol: &mut Option<Arc<DictationProtocol>>,
    protocol: Arc<DictationProtocol>,
    control_stability: &mut ControlStability,
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
    *voice_protocol = Some(protocol);
    control_stability.reset();
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
) -> Result<bool> {
    match action {
        HotkeyAction::Start => {
            if !dictation.start(action_at)? {
                return Ok(false);
            }
            reset_command_recognizer(dictation, recognizer)?;
            worker.prepare_paste();
            events.dictation(DictationPhase::Started, "")?;
            if let Some(indicator) = indicator {
                indicator.send(DictationIndicatorEvent::Started);
            }
            emit_state(events, true, mode, device)
        }
        HotkeyAction::Finish => {
            if dictation.become_intentional(action_at)? {
                feedback::play(Tone::DictationStart);
            }
            let _ = finish_dictation(
                dictation,
                action_at,
                worker,
                TranscriptionTarget::Paste,
                None,
                context,
                device,
                events,
                indicator,
            )?;
            reset_command_recognizer(dictation, recognizer)?;
            emit_engine_state(events, false, worker, mode, device)
        }
        HotkeyAction::Discard => {
            dictation.cancel()?;
            reset_command_recognizer(dictation, recognizer)?;
            events.dictation(DictationPhase::Discarded, "")?;
            if let Some(indicator) = indicator {
                indicator.send(DictationIndicatorEvent::Discarded);
            }
            emit_state(events, false, mode, device)
        }
        HotkeyAction::Cancel => {
            dictation.cancel()?;
            reset_command_recognizer(dictation, recognizer)?;
            feedback::play(Tone::Cancel);
            events.dictation(DictationPhase::Cancelled, "")?;
            if let Some(indicator) = indicator {
                indicator.send(DictationIndicatorEvent::Cancelled);
            }
            emit_state(events, false, mode, device)
        }
        HotkeyAction::PasteLast => {
            dictation.cancel()?;
            reset_command_recognizer(dictation, recognizer)?;
            if let Err(error) = worker.paste_last() {
                feedback::play(Tone::Error);
                events.dictation(DictationPhase::Failed(error.into()), "")?;
            }
            emit_engine_state(events, false, worker, mode, device)
        }
        HotkeyAction::PasteMeeting => {
            dictation.cancel()?;
            reset_command_recognizer(dictation, recognizer)?;
            if let Err(error) = worker.paste_meeting() {
                feedback::play(Tone::Error);
                events.dictation(DictationPhase::Failed(error.into()), "")?;
            }
            emit_engine_state(events, false, worker, mode, device)
        }
        HotkeyAction::RewriteLast => {
            dictation.cancel()?;
            reset_command_recognizer(dictation, recognizer)?;
            let submission = if !crate::dictation_processor::opencode_installed() {
                Err("Rewrite requires OpenCode")
            } else {
                worker.rewrite_last(context.clone())
            };
            match submission {
                Ok(job_id) => {
                    if let Some(indicator) = indicator {
                        indicator.send(DictationIndicatorEvent::Submitted {
                            job_id: job_id.value(),
                        });
                    }
                }
                Err(error) => {
                    feedback::play(Tone::Error);
                    events.dictation(DictationPhase::Failed(error.into()), "")?;
                }
            }
            emit_engine_state(events, false, worker, mode, device)
        }
    }?;
    Ok(true)
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
        HotkeyAction::Finish => {
            let Some(context) = edit_context.take() else {
                dictation.cancel()?;
                return Ok(false);
            };
            let _ = finish_dictation(
                dictation,
                action_at,
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
            Ok(true)
        }
        HotkeyAction::Discard => {
            reset_command_recognizer(dictation, recognizer)?;
            *edit_context = None;
            dictation.cancel()?;
            events.dictation(DictationPhase::Discarded, "")?;
            if let Some(indicator) = indicator {
                indicator.send(DictationIndicatorEvent::Discarded);
            }
            emit_state(events, false, mode, device)?;
            Ok(true)
        }
        HotkeyAction::Cancel => {
            reset_command_recognizer(dictation, recognizer)?;
            *edit_context = None;
            dictation.cancel()?;
            feedback::play(Tone::Cancel);
            events.dictation(DictationPhase::Cancelled, "")?;
            if let Some(indicator) = indicator {
                indicator.send(DictationIndicatorEvent::Cancelled);
            }
            emit_state(events, false, mode, device)?;
            Ok(true)
        }
        HotkeyAction::PasteLast | HotkeyAction::RewriteLast | HotkeyAction::PasteMeeting => {
            Ok(false)
        }
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

fn reset_recognizer(recognizer: &mut Option<Moonshine>) -> Result<()> {
    if let Some(recognizer) = recognizer {
        recognizer.reset_stream()?;
    }
    Ok(())
}

fn reset_command_recognizer(
    dictation: &DictationAudio,
    recognizer: &mut Option<Moonshine>,
) -> Result<()> {
    dictation.invalidate_recognition()?;
    reset_recognizer(recognizer)
}

#[allow(clippy::too_many_arguments)]
fn handle_command(
    commands: &CommandConfig,
    mode: &mut Mode,
    voice_protocol: &mut Option<Arc<DictationProtocol>>,
    active_protocol: Arc<DictationProtocol>,
    control_stability: &mut ControlStability,
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
                voice_protocol,
                active_protocol,
                control_stability,
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
                Action::StartMeeting => MeetingRequest::Start,
                Action::StopMeeting => MeetingRequest::Stop,
                _ => unreachable!(),
            };
            let outcome = meeting_requests
                .ok_or("meeting controls require the HEX app")
                .and_then(|requests| {
                    requests.try_send(request).map_err(|error| match error {
                        TrySendError::Full(_) => "meeting controller is busy",
                        TrySendError::Disconnected(_) => "meeting controller is unavailable",
                    })
                });
            match outcome {
                Ok(()) => (Some(id.into()), CommandOutcome::Executed),
                Err(error) => {
                    feedback::play(Tone::Error);
                    (Some(id.into()), CommandOutcome::Failed(error.into()))
                }
            }
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
                .ok_or("personal command host is unavailable")
                .and_then(|runtime| {
                    runtime.invoke(generation, id, heard, context.clone(), captures)
                });
            match outcome {
                Ok(()) => (Some(id.into()), CommandOutcome::Submitted),
                Err(error) => {
                    feedback::play(Tone::Error);
                    (Some(id.into()), CommandOutcome::Failed(error.into()))
                }
            }
        }
        Decision::Execute { id, action } => {
            match action_executor.submit(id, action, heard, context.label()) {
                Ok(()) => (Some(id.into()), CommandOutcome::Submitted),
                Err(error) => {
                    feedback::play(Tone::Error);
                    (Some(id.into()), CommandOutcome::Failed(error.into()))
                }
            }
        }
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
    events.emit(&VoiceEvent::State {
        timestamp_ms: now_ms(),
        state: if suppressed {
            VoiceState::Dictating
        } else if mode == Mode::Sleeping {
            VoiceState::Sleeping
        } else {
            VoiceState::Listening
        },
        device: device.into(),
    })?;
    Ok(())
}

fn emit_engine_state(
    events: &mut EventLog,
    capturing: bool,
    worker: &DictationWorker,
    mode: Mode,
    device: &str,
) -> Result<()> {
    events.emit(&VoiceEvent::State {
        timestamp_ms: now_ms(),
        state: if capturing {
            VoiceState::Dictating
        } else if worker.is_busy() {
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
    worker: &DictationWorker,
    target: TranscriptionTarget,
    protocol: Option<Arc<DictationProtocol>>,
    context: &ContextSnapshot,
    device: &str,
    events: &mut EventLog,
    indicator: Option<&DictationIndicatorSender>,
) -> Result<FinishResult> {
    let Finish::Transcribe(clip) = capture.finish(ended_at)? else {
        events.dictation(DictationPhase::Discarded, "")?;
        if let Some(indicator) = indicator {
            indicator.send(DictationIndicatorEvent::Discarded);
        }
        return Ok(FinishResult::Discarded);
    };
    feedback::play(Tone::DictationStop);
    events.emit(&VoiceEvent::State {
        timestamp_ms: now_ms(),
        state: VoiceState::Transcribing,
        device: device.into(),
    })?;
    events.dictation(DictationPhase::Transcribing, "")?;
    match worker.transcribe(clip, target, protocol, context.clone()) {
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
            dictation.cancel()?;
            feedback::play(Tone::Cancel);
            events.dictation(DictationPhase::Cancelled, "")?;
            if let Some(indicator) = indicator {
                indicator.send(DictationIndicatorEvent::Cancelled);
            }
            emit_state(events, false, mode, device)?;
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
    origin: Option<(u64, CaptureInstant)>,
    generation: u64,
    stream_ms: u64,
    fallback: CaptureInstant,
) -> CaptureInstant {
    origin
        .filter(|(origin_generation, _)| *origin_generation == generation)
        .and_then(|(_, origin)| origin.checked_add(Duration::from_millis(stream_ms)))
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
        && !matches!(
            action,
            HotkeyAction::PasteLast | HotkeyAction::RewriteLast | HotkeyAction::PasteMeeting
        )
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
                TranscriptionTarget::RewriteLast => DictationPhase::Repasted,
                TranscriptionTarget::Service => return Ok(()),
            };
            events.processed_dictation(
                phase,
                text,
                processing.map(|processing| DictationProcessing {
                    profile: processing.profile,
                    latency_ms: processing.latency_ms,
                    fallback: processing.fallback,
                }),
            )?;
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

        const OPTION: u64 = 1 << 19;
        const COMMAND: u64 = 1 << 20;
        let now = CaptureInstant::from_nanos(60_000_000_000);
        let mut dictation = DictationHotkey::new(
            now,
            true,
            42,
            RuntimeHotkey {
                modifiers: crate::app_settings::modifiers_from_flags(OPTION),
                key_code: None,
            },
        );
        let mut edit = DictationHotkey::new_without_paste(
            now,
            42,
            RuntimeHotkey {
                modifiers: crate::app_settings::modifiers_from_flags(OPTION | COMMAND),
                key_code: None,
            },
        );

        dictation.suspend();
        edit.suspend();

        // Option down: plain dictation starts, no Voice Action gesture yet.
        let event = InputEvent::Flags(OPTION);
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
        let event = InputEvent::Flags(OPTION | COMMAND);
        let edit_action = edit.process(event, chord_at);
        assert_eq!(edit_action, Some(HotkeyAction::Start));
        assert_eq!(dictation.process(event, chord_at), None);

        // Option lifts first: both machines emit Finish on the same event.
        // The Voice Action gesture must own the dictation Finish, or plain
        // dictation would consume the capture out from under it.
        let release_at = chord_at + Duration::from_secs(1);
        let event = InputEvent::Flags(COMMAND);
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
        let event = InputEvent::Flags(OPTION);
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

        const OPTION: u64 = 1 << 19;
        const COMMAND: u64 = 1 << 20;
        let now = CaptureInstant::from_nanos(60_000_000_000);
        let mut hotkey = DictationHotkey::new(
            now,
            true,
            42,
            RuntimeHotkey {
                modifiers: crate::app_settings::modifiers_from_flags(OPTION),
                key_code: None,
            },
        );
        let mut edit_hotkey = DictationHotkey::new_without_paste(
            now,
            42,
            RuntimeHotkey {
                modifiers: crate::app_settings::modifiers_from_flags(OPTION | COMMAND),
                key_code: None,
            },
        );
        hotkey.suspend();
        edit_hotkey.suspend();
        let mut capture = DictationCapture::new(16_000);
        let option = InputEvent::Flags(OPTION);
        assert_eq!(edit_hotkey.process(option, now), None);
        assert_eq!(hotkey.process(option, now), Some(HotkeyAction::Start));
        capture.start_at(now);

        let chord_at = now + Duration::from_millis(40);
        let chord = InputEvent::Flags(OPTION | COMMAND);
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
            HotkeyAction::Discard,
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
