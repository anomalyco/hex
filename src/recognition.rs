use std::path::Path;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{self, Receiver, SyncSender, TryRecvError, TrySendError};
use std::time::{Duration, Instant};

use color_eyre::Result;

use crate::audio::{RecoveringAudioInput, RecoveringAudioInputEvent};
use crate::command_grammar::normalize;
use crate::commands::{Action, ActionExecutor, ActionOutcome, CommandConfig, Decision, Mode};
use crate::context::{ContextMonitor, ContextSnapshot};
use crate::dictation::{
    ControlStability, DictationCapture, DictationControl, DictationProtocol, Finish,
    MINIMUM_HOLD_DURATION,
};
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

pub fn listen(
    project_root: &Path,
    events: EventLog,
    device_override: Option<&str>,
    commands: CommandConfig,
    shutdown: &AtomicBool,
    indicator: Option<DictationIndicatorSender>,
    meeting_requests: Option<SyncSender<MeetingRequest>>,
) -> Result<()> {
    let commands = Arc::new(commands);
    let native_runtime = Arc::new(crate::personal_commands::RuntimeSnapshot::native(
        commands.as_ref().clone(),
    ));
    shutdown.store(false, Ordering::Relaxed);
    let (microphone_revision, microphone) = crate::app_settings::microphone_selection();
    let mut input =
        RecoveringAudioInput::open(device_override, microphone_revision, microphone.as_deref())?;
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
        Instant::now(),
        crate::app_settings::double_tap_lock(),
        input_monitor.paste_key_code,
        crate::app_settings::dictation_hotkey(),
    );
    let mut edit_hotkey = DictationHotkey::new_without_paste(
        Instant::now(),
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
    let recording_environment = RecordingEnvironmentController::start();
    let mut dictation = DictationCapture::new(input.sample_rate());
    dictation.enable_recording_environment(recording_environment.clone());
    let dictation_worker = DictationWorker::start(input_monitor.activity.clone());
    let (mut transcription_revision, _) = crate::app_settings::transcription_selection();
    let mut action_executor = commands_enabled.then(ActionExecutor::start);
    let mut personal_commands = commands_enabled
        .then(|| crate::personal_commands::PersonalCommands::start(commands.as_ref().clone()))
        .flatten();

    events.emit(&VoiceEvent::SessionStarted {
        timestamp_ms: now_ms(),
    })?;
    if hotkey.is_recording() {
        dictation.start(Instant::now());
        events.dictation(DictationPhase::Started, "")?;
        if let Some(indicator) = &indicator {
            indicator.send(DictationIndicatorEvent::Started);
        }
    }
    if edit_hotkey.is_recording() {
        dictation.start(Instant::now());
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
        input.device_name(),
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
                personal_commands =
                    crate::personal_commands::PersonalCommands::start(commands.as_ref().clone());
                tracing::info!("voice commands enabled; loading command model");
            } else {
                command_loader = None;
                recognizer = None;
                action_executor = None;
                personal_commands = None;
                tracing::info!("voice commands disabled");
            }
        }
        if let Some(loader) = &command_loader {
            match loader.try_recv() {
                Ok(Ok(loaded)) if commands_enabled => {
                    recognizer = Some(loaded);
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
                    &mut recognizer,
                    &mut dictation,
                    &dictation_worker,
                    mode,
                    &context,
                    input.device_name(),
                    &mut events,
                    indicator.as_ref(),
                )?;
            }
        }
        while let Ok(input_event) = input_monitor.events.try_recv() {
            if hotkey_capture_suspended {
                continue;
            }
            if voice_protocol.is_some() && input_event.is_escape_down() {
                voice_protocol = None;
                control_stability.reset();
                reset_recognizer(&mut recognizer)?;
                dictation.cancel();
                feedback::play(Tone::Cancel);
                events.dictation(DictationPhase::Cancelled, "")?;
                if let Some(indicator) = &indicator {
                    indicator.send(DictationIndicatorEvent::Cancelled);
                }
                emit_state(&mut events, false, mode, input.device_name())?;
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
            let edit_action = edit_hotkey.process(input_event, Instant::now());
            if matches!(edit_action, Some(HotkeyAction::Start)) && voice_protocol.is_none() {
                edit_pending_since = Some(Instant::now());
            }
            if let Some(action) = hotkey.process(input_event, Instant::now()) {
                if matches!(action, HotkeyAction::PasteLast | HotkeyAction::PasteMeeting) {
                    voice_protocol = None;
                    control_stability.reset();
                }
                let voice_action_takeover = matches!(action, HotkeyAction::Discard)
                    && edit_pending_since.is_some()
                    && edit_hotkey.is_recording();
                if voice_protocol.is_none() && !voice_action_takeover {
                    handle_hotkey_action(
                        action,
                        &mut recognizer,
                        &mut dictation,
                        &dictation_worker,
                        mode,
                        &context,
                        input.device_name(),
                        &mut events,
                        indicator.as_ref(),
                    )?;
                }
            }
            if let Some(action) = edit_action
                && voice_protocol.is_none()
                && !matches!(action, HotkeyAction::Start)
            {
                if edit_pending_since.take().is_none() {
                    let accepted = handle_edit_hotkey_action(
                        action,
                        &mut edit_context,
                        &mut recognizer,
                        &mut dictation,
                        &dictation_worker,
                        mode,
                        &context,
                        input.device_name(),
                        &mut events,
                        indicator.as_ref(),
                    )?;
                    if !accepted {
                        edit_hotkey.suspend();
                    }
                } else {
                    edit_context = None;
                    dictation.cancel();
                    events.dictation(DictationPhase::Discarded, "")?;
                    if let Some(indicator) = &indicator {
                        indicator.send(DictationIndicatorEvent::Discarded);
                    }
                    emit_state(&mut events, false, mode, input.device_name())?;
                }
            }
        }
        if edit_pending_since.is_some_and(|started| started.elapsed() >= MINIMUM_HOLD_DURATION)
            && edit_hotkey.is_recording()
        {
            edit_pending_since = None;
            let accepted = handle_edit_hotkey_action(
                HotkeyAction::Start,
                &mut edit_context,
                &mut recognizer,
                &mut dictation,
                &dictation_worker,
                mode,
                &context,
                input.device_name(),
                &mut events,
                indicator.as_ref(),
            )?;
            if !accepted {
                edit_hotkey.suspend();
            }
        }
        let missed_input_event = input_monitor.take_missed_event();
        if !hotkey_capture_active
            && let Some(action) = hotkey.reconcile(missed_input_event)
            && voice_protocol.is_none()
        {
            tracing::warn!("recovered a missed dictation hotkey release");
            handle_hotkey_action(
                action,
                &mut recognizer,
                &mut dictation,
                &dictation_worker,
                mode,
                &context,
                input.device_name(),
                &mut events,
                indicator.as_ref(),
            )?;
        }
        if !hotkey_capture_active
            && let Some(action) = edit_hotkey.reconcile(missed_input_event)
            && voice_protocol.is_none()
            && edit_pending_since.take().is_none()
        {
            tracing::warn!("recovered a missed voice action hotkey release");
            handle_edit_hotkey_action(
                action,
                &mut edit_context,
                &mut recognizer,
                &mut dictation,
                &dictation_worker,
                mode,
                &context,
                input.device_name(),
                &mut events,
                indicator.as_ref(),
            )?;
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
                input.device_name(),
            )?;
        }

        let capture_idle =
            !hotkey.is_recording() && !edit_hotkey.is_recording() && voice_protocol.is_none();
        let audio_event = input.recv_timeout(Duration::from_millis(20), capture_idle);
        match audio_event {
            RecoveringAudioInputEvent::Chunk(samples) if voice_protocol.is_some() => {
                if let Some(indicator) = &indicator {
                    indicator.meter(&samples);
                }
                let _ = dictation.push(&samples, Instant::now());
                if let Some(recognizer) = &mut recognizer {
                    recognizer.add_audio(&samples, input.sample_rate())?;
                }
            }
            RecoveringAudioInputEvent::Chunk(samples)
                if hotkey.suppresses_recognition() || edit_hotkey.suppresses_recognition() =>
            {
                if dictation.is_recording() {
                    if let Some(indicator) = &indicator {
                        indicator.meter(&samples);
                    }
                    if dictation.push(&samples, Instant::now()) && hotkey.is_recording() {
                        feedback::play(Tone::DictationStart);
                    }
                } else {
                    dictation.keep_warm(&samples);
                }
            }
            RecoveringAudioInputEvent::Chunk(samples) => {
                dictation.keep_warm(&samples);
                if let Some(recognizer) = &mut recognizer {
                    recognizer.add_audio(&samples, input.sample_rate())?;
                }
            }
            RecoveringAudioInputEvent::Timeout => {}
            RecoveringAudioInputEvent::Reopened => {
                reset_recognizer(&mut recognizer)?;
                dictation = DictationCapture::new(input.sample_rate());
                dictation.enable_recording_environment(recording_environment.clone());
                emit_state(&mut events, false, mode, input.device_name())?;
                continue;
            }
            RecoveringAudioInputEvent::Interrupted => {
                let was_recording =
                    hotkey.is_recording() || edit_hotkey.is_recording() || voice_protocol.is_some();
                voice_protocol = None;
                control_stability.reset();
                edit_context = None;
                edit_pending_since = None;
                hotkey.suspend();
                edit_hotkey.suspend();
                dictation.cancel();
                reset_recognizer(&mut recognizer)?;
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
                        input.device_name(),
                    )?;
                }
                continue;
            }
        }
        if !input.is_recovering()
            && !hotkey.suppresses_recognition()
            && !edit_hotkey.suppresses_recognition()
            && last_update.elapsed() >= UPDATE_INTERVAL
            && let Some(recognizer) = &mut recognizer
            && let Some(action_executor) = &action_executor
        {
            for update in recognizer.update()? {
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
                    start_voice_capture(
                        &mut voice_protocol,
                        active_runtime.dictation.clone(),
                        &mut control_stability,
                        recognizer,
                        &mut dictation,
                        mode,
                        input.device_name(),
                        &mut events,
                        indicator.as_ref(),
                    )?;
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
                        let protocol = voice_protocol.take().expect("voice protocol is active");
                        recognizer.reset_stream()?;
                        handle_voice_dictation_control(
                            control,
                            &update.text,
                            protocol,
                            &mut dictation,
                            &dictation_worker,
                            mode,
                            &context,
                            input.device_name(),
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
                    handle_command(
                        &active_runtime.commands,
                        &mut mode,
                        &mut voice_protocol,
                        active_runtime.dictation.clone(),
                        &mut control_stability,
                        recognizer,
                        &mut dictation,
                        action_executor,
                        personal_commands.as_ref(),
                        meeting_requests.as_ref(),
                        &update.text,
                        &context,
                        &input_monitor.activity,
                        input.device_name(),
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
        device: input.device_name().into(),
    })?;
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
    dictation: &mut DictationCapture,
    mode: Mode,
    device: &str,
    events: &mut EventLog,
    indicator: Option<&DictationIndicatorSender>,
) -> Result<()> {
    recognizer.reset_stream()?;
    dictation.start_voice(Instant::now());
    *voice_protocol = Some(protocol);
    control_stability.reset();
    feedback::play(Tone::DictationStart);
    events.dictation(DictationPhase::Started, "")?;
    if let Some(indicator) = indicator {
        indicator.send(DictationIndicatorEvent::Started);
    }
    emit_state(events, true, mode, device)
}

#[allow(clippy::too_many_arguments)]
fn handle_hotkey_action(
    action: HotkeyAction,
    recognizer: &mut Option<Moonshine>,
    dictation: &mut DictationCapture,
    worker: &DictationWorker,
    mode: Mode,
    context: &ContextSnapshot,
    device: &str,
    events: &mut EventLog,
    indicator: Option<&DictationIndicatorSender>,
) -> Result<()> {
    reset_recognizer(recognizer)?;
    match action {
        HotkeyAction::Start => {
            dictation.start(Instant::now());
            worker.prepare_paste();
            events.dictation(DictationPhase::Started, "")?;
            if let Some(indicator) = indicator {
                indicator.send(DictationIndicatorEvent::Started);
            }
            emit_state(events, true, mode, device)
        }
        HotkeyAction::Finish => {
            if dictation.become_intentional(Instant::now()) {
                feedback::play(Tone::DictationStart);
            }
            let _ = finish_dictation(
                dictation,
                worker,
                TranscriptionTarget::Paste,
                None,
                context,
                device,
                events,
                indicator,
            )?;
            emit_engine_state(events, false, worker, mode, device)
        }
        HotkeyAction::Discard => {
            dictation.cancel();
            events.dictation(DictationPhase::Discarded, "")?;
            if let Some(indicator) = indicator {
                indicator.send(DictationIndicatorEvent::Discarded);
            }
            emit_state(events, false, mode, device)
        }
        HotkeyAction::Cancel => {
            dictation.cancel();
            feedback::play(Tone::Cancel);
            events.dictation(DictationPhase::Cancelled, "")?;
            if let Some(indicator) = indicator {
                indicator.send(DictationIndicatorEvent::Cancelled);
            }
            emit_state(events, false, mode, device)
        }
        HotkeyAction::PasteLast => {
            dictation.cancel();
            if let Err(error) = worker.paste_last() {
                feedback::play(Tone::Error);
                events.dictation(DictationPhase::Failed(error.into()), "")?;
            }
            emit_engine_state(events, false, worker, mode, device)
        }
        HotkeyAction::PasteMeeting => {
            dictation.cancel();
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
    context.selected_text = crate::selected_text::capture_optional();
    context.input_revision = None;
    context
}

#[allow(clippy::too_many_arguments)]
fn handle_edit_hotkey_action(
    action: HotkeyAction,
    edit_context: &mut Option<ContextSnapshot>,
    recognizer: &mut Option<Moonshine>,
    dictation: &mut DictationCapture,
    worker: &DictationWorker,
    mode: Mode,
    context: &ContextSnapshot,
    device: &str,
    events: &mut EventLog,
    indicator: Option<&DictationIndicatorSender>,
) -> Result<bool> {
    match action {
        HotkeyAction::Start => {
            let now = Instant::now();
            let promoted = dictation.is_recording();
            if !promoted {
                reset_recognizer(recognizer)?;
                dictation.start(now);
            }
            let _ = dictation.become_intentional(now);
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
            reset_recognizer(recognizer)?;
            let Some(context) = edit_context.take() else {
                dictation.cancel();
                return Ok(false);
            };
            let _ = finish_dictation(
                dictation,
                worker,
                TranscriptionTarget::VoiceAction,
                None,
                &context,
                device,
                events,
                indicator,
            )?;
            emit_engine_state(events, false, worker, mode, device)?;
            Ok(true)
        }
        HotkeyAction::Discard => {
            reset_recognizer(recognizer)?;
            *edit_context = None;
            dictation.cancel();
            events.dictation(DictationPhase::Discarded, "")?;
            if let Some(indicator) = indicator {
                indicator.send(DictationIndicatorEvent::Discarded);
            }
            emit_state(events, false, mode, device)?;
            Ok(true)
        }
        HotkeyAction::Cancel => {
            reset_recognizer(recognizer)?;
            *edit_context = None;
            dictation.cancel();
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

#[allow(clippy::too_many_arguments)]
fn handle_command(
    commands: &CommandConfig,
    mode: &mut Mode,
    voice_protocol: &mut Option<Arc<DictationProtocol>>,
    active_protocol: Arc<DictationProtocol>,
    control_stability: &mut ControlStability,
    recognizer: &mut Moonshine,
    dictation: &mut DictationCapture,
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
    if matches!(decision, Decision::Execute { .. }) {
        input_activity.invalidate();
    }
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
            start_voice_capture(
                voice_protocol,
                active_protocol,
                control_stability,
                recognizer,
                dictation,
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
            action: Action::InvokeHandler { generation },
        } => {
            let outcome = personal_commands
                .ok_or("personal command host is unavailable")
                .and_then(|runtime| runtime.invoke(generation, id, heard, context.clone()));
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
    capture: &mut DictationCapture,
    worker: &DictationWorker,
    target: TranscriptionTarget,
    protocol: Option<Arc<DictationProtocol>>,
    context: &ContextSnapshot,
    device: &str,
    events: &mut EventLog,
    indicator: Option<&DictationIndicatorSender>,
) -> Result<FinishResult> {
    let Finish::Transcribe(clip) = capture.finish(Instant::now()) else {
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
    dictation: &mut DictationCapture,
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
            dictation.cancel();
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
}
