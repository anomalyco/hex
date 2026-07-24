use std::path::Path;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{self, Receiver, SyncSender, TryRecvError, TrySendError};
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
) -> Result<()> {
    let commands = Arc::new(commands);
    let native_runtime = Arc::new(crate::personal_commands::RuntimeSnapshot::native(
        commands.as_ref().clone(),
    ));
    let transformations = Arc::new(crate::personal_commands::TransformationClient::default());
    shutdown.store(false, Ordering::Relaxed);
    feedback::preload()?;
    let mut commands_enabled = crate::app_settings::commands_enabled();
    let mut recognizer = commands_enabled
        .then(|| Moonshine::load(project_root))
        .transpose()?;
    let mut command_loader = None;
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
    let mut edit_hotkey = DictationHotkey::new_without_paste(
        CaptureInstant::now(),
        input_monitor.paste_key_code,
        crate::app_settings::edit_hotkey(),
    );
    let mut edit_context = None;
    let mut edit_pending_since = None;
    let mut mode = Mode::Listening;
    let mut voice_protocol: Option<Arc<DictationProtocol>> = None;
    let mut activation_stability = ActivationStability::default();
    let mut control_stability = ControlStability::default();
    let mut last_update = Instant::now();
    let mut recognition_origin = None::<(u64, CaptureInstant)>;
    let recording_environment = RecordingEnvironmentController::start();
    let (microphone_revision, microphone) = crate::app_settings::microphone_selection();
    let input = DictationAudio::open(
        device_override,
        microphone_revision,
        microphone.as_deref(),
        recording_environment,
        input_monitor.pending_events(),
    )?;
    let dictation_worker = DictationWorker::start(
        input_monitor.activity.clone(),
        transformations.clone(),
        history.clone(),
    );
    let (mut transcription_revision, _) = crate::app_settings::transcription_selection();
    let mut action_executor = commands_enabled.then(ActionExecutor::start);
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

    events.emit(&VoiceEvent::SessionStarted {
        timestamp_ms: now_ms(),
    })?;
    if hotkey.is_recording() {
        input.start(input.captured_through())?;
        events.dictation(DictationPhase::Started, "")?;
        if let Some(indicator) = &indicator {
            indicator.send(DictationIndicatorEvent::Started);
        }
    }
    if edit_hotkey.is_recording() {
        input.start(input.captured_through())?;
        edit_context = Some(voice_action_context_snapshot(&context));
        events.dictation(DictationPhase::Started, "")?;
        if let Some(indicator) = &indicator {
            indicator.send(DictationIndicatorEvent::EditingStarted);
        }
    }
    emit_state(
        &mut events,
        hotkey.is_recording() || edit_hotkey.is_recording(),
        mode,
        &input.device_name(),
    )?;
    if commands_enabled {
        tracing::info!(device = %input.device_name(), "voice recognition started");
    } else {
        tracing::info!(device = %input.device_name(), "dictation listener started");
    }

    while !shutdown.load(Ordering::Relaxed) {
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
        let next_commands_enabled = crate::app_settings::commands_enabled();
        if next_commands_enabled != commands_enabled {
            commands_enabled = next_commands_enabled;
            mode = Mode::Listening;
            if commands_enabled {
                command_loader = Some(load_command_recognizer(project_root.to_path_buf()));
                tracing::info!("voice commands enabled; loading command model");
            } else {
                command_loader = None;
                recognizer = None;
                action_executor = None;
                tracing::info!("voice commands disabled");
            }
        }
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
                    tracing::info!("voice command model loaded");
                }
                Ok(Ok(_)) => command_loader = None,
                Ok(Err(error)) => {
                    command_loader = None;
                    tracing::error!(%error, "could not enable voice commands");
                }
                Err(TryRecvError::Empty) => {}
                Err(TryRecvError::Disconnected) => {
                    command_loader = None;
                    tracing::error!("voice command model loader stopped");
                }
            }
        }
        hotkey.set_double_tap_enabled(crate::app_settings::double_tap_lock());
        hotkey.set_binding(crate::app_settings::dictation_hotkey());
        edit_hotkey.set_binding(crate::app_settings::edit_hotkey());
        let (next_microphone_revision, microphone) = crate::app_settings::microphone_selection();
        input.request_selection(next_microphone_revision, microphone.as_deref());
        let (next_revision, selection) = crate::app_settings::transcription_selection();
        if next_revision != transcription_revision && dictation_worker.reload(selection).is_ok() {
            transcription_revision = next_revision;
        }
        let hotkey_capture_active = crate::app_settings::hotkey_capture_active();
        let hotkey_capture_suspended = hotkey_capture_active || input.is_recovering();
        if hotkey_capture_suspended && voice_protocol.is_none() {
            edit_pending_since = None;
            let normal_action = hotkey.suspend();
            let edit_action = edit_hotkey.suspend();
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
                && !edit_hotkey.is_recording()
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
            let edit_action = edit_hotkey.process(input_event, capture_at);
            if matches!(edit_action, Some(HotkeyAction::Start)) && voice_protocol.is_none() {
                edit_pending_since = Some(capture_at);
            }
            if let Some(action) = hotkey.process(input_event, capture_at) {
                if matches!(action, HotkeyAction::PasteLast | HotkeyAction::PasteMeeting) {
                    voice_protocol = None;
                    control_stability.reset();
                }
                let voice_action_takeover = voice_action_owns_action(
                    action,
                    edit_pending_since.is_some(),
                    edit_hotkey.is_recording(),
                    edit_action,
                );
                if voice_action_takeover {
                    let _ = hotkey.suspend();
                } else if voice_protocol.is_none() {
                    handle_hotkey_action(
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
                            edit_hotkey.suspend();
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
                            indicator.as_ref(),
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
        if hotkey.is_recording()
            && edit_pending_since.is_none()
            && input.become_intentional(input.captured_through())?
        {
            feedback::play(Tone::DictationStart);
        }
        if edit_pending_since.is_some_and(|started| {
            input.captured_through().duration_since(started) >= MINIMUM_HOLD_DURATION
        }) && edit_hotkey.is_recording()
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
                indicator.as_ref(),
            )?;
            if !accepted {
                edit_hotkey.suspend();
            }
        }
        if let Some(history) = &history {
            while let Some(text) = history.next_paste_request() {
                if let Err(error) = dictation_worker.paste_history(text) {
                    tracing::warn!(%error, "could not paste the history entry");
                    feedback::play(Tone::Error);
                }
            }
        }
        let mut received_worker_event = false;
        while let Some(event) = dictation_worker.try_recv() {
            received_worker_event = true;
            handle_dictation_event(event, &mut events, indicator.as_ref())?;
        }
        input_monitor.set_escape_cancels(
            hotkey.is_recording()
                || edit_hotkey.is_recording()
                || voice_protocol.is_some()
                || dictation_worker.pending_count() > 0,
        );
        if received_worker_event {
            emit_engine_state(
                &mut events,
                hotkey.is_recording() || edit_hotkey.is_recording() || voice_protocol.is_some(),
                &dictation_worker,
                mode,
                &input.device_name(),
            )?;
        }

        while let Some(audio_event) = input.try_recv_event() {
            match audio_event {
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
                    was_recording: _,
                    capture_generation,
                    gap_ms,
                } if input.is_recording() && input.capture_generation() != capture_generation => {
                    tracing::debug!(
                        capture_generation,
                        gap_ms,
                        "ignored stale audio discontinuity"
                    );
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
                        hotkey.suspend();
                        edit_hotkey.suspend();
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
                DictationAudioEvent::Interrupted {
                    was_recording: _,
                    capture_generation,
                } if input.is_recording() && input.capture_generation() != capture_generation => {
                    tracing::debug!(capture_generation, "ignored stale microphone interruption");
                }
                DictationAudioEvent::Interrupted { was_recording, .. } => {
                    voice_protocol = None;
                    control_stability.reset();
                    edit_context = None;
                    edit_pending_since = None;
                    hotkey.suspend();
                    edit_hotkey.suspend();
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
        if let Some(audio) = &audio
            && audio.is_current(input.recognition_generation())
            && recognizer.is_some()
            && !hotkey.suppresses_recognition()
            && !edit_hotkey.suppresses_recognition()
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
                if hotkey.suppresses_recognition() || edit_hotkey.suppresses_recognition() =>
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
            && !hotkey.suppresses_recognition()
            && !edit_hotkey.suppresses_recognition()
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

fn load_command_recognizer(
    project_root: std::path::PathBuf,
) -> Receiver<Result<Moonshine, String>> {
    let (sender, receiver) = mpsc::sync_channel(1);
    std::thread::spawn(move || {
        let result = crate::moonshine::install_model()
            .and_then(|_| Moonshine::load(&project_root))
            .map_err(|error| error.to_string());
        let _ = sender.send(result);
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
) -> Result<()> {
    match action {
        HotkeyAction::Start => {
            dictation.start(action_at)?;
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
    }
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
            if !crate::dictation_processor::opencode_installed() {
                feedback::play(Tone::Error);
                events.dictation(
                    DictationPhase::Failed("Voice Action requires OpenCode".into()),
                    "",
                )?;
                return Ok(false);
            }
            let promoted = dictation.is_recording();
            if !promoted {
                dictation.start(action_at)?;
                reset_command_recognizer(dictation, recognizer)?;
            }
            let _ = dictation.become_intentional(dictation.captured_through())?;
            *edit_context = Some(voice_action_context_snapshot(context));
            feedback::play(Tone::DictationStart);
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
        HotkeyAction::PasteLast | HotkeyAction::PasteMeeting => Ok(false),
    }
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
                    capture,
                },
        } => {
            let outcome = personal_commands
                .ok_or("personal command host is unavailable")
                .and_then(|runtime| {
                    runtime.invoke(generation, id, heard, context.clone(), capture)
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
        && !matches!(action, HotkeyAction::PasteLast | HotkeyAction::PasteMeeting)
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
                PasteKind::LastTranscript | PasteKind::History => DictationPhase::Repasted,
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
                modifiers: OPTION,
                key_code: None,
            },
        );
        let mut edit = DictationHotkey::new_without_paste(
            now,
            42,
            RuntimeHotkey {
                modifiers: OPTION | COMMAND,
                key_code: None,
            },
        );

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
}
