use std::path::Path;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{RecvTimeoutError, SyncSender, TrySendError};
use std::time::{Duration, Instant};

use color_eyre::Result;

use crate::audio::AudioInput;
use crate::commands::{Action, ActionExecutor, ActionOutcome, CommandConfig, Decision, Mode};
use crate::config;
use crate::context::{ContextMonitor, ContextSnapshot};
use crate::dictation::{
    ControlStability, DictationCapture, DictationControl, Finish, captains_log_end_suffix,
    captains_log_start_prefix, dictation_control_suffix, dictation_start_prefix,
};
use crate::dictation_indicator::{DictationIndicatorEvent, DictationIndicatorSender};
use crate::events::{
    CommandOutcome, DictationPhase, EventLog, TranscriptPhase, VoiceEvent, VoiceState, now_ms,
};
use crate::feedback::{self, Tone};
use crate::meeting::MeetingRequest;
use crate::moonshine::Moonshine;
use crate::parakeet::{DictationWorker, TranscriptionTarget, WorkerEvent};
use crate::suppression::{self, HotkeyAction, InputMonitor, OptionHotkey};

const UPDATE_INTERVAL: Duration = Duration::from_millis(200);

#[derive(Clone, Copy, Eq, PartialEq)]
enum VoiceCapture {
    Dictation,
    CaptainsLog,
}

enum FinishResult {
    Discarded,
    Submitted,
    Rejected(String),
}

pub fn listen(
    project_root: &Path,
    event_path: &Path,
    device_override: Option<&str>,
    commands: CommandConfig,
    shutdown: &AtomicBool,
    indicator: Option<DictationIndicatorSender>,
    meeting_requests: Option<SyncSender<MeetingRequest>>,
) -> Result<()> {
    shutdown.store(false, Ordering::Relaxed);
    let overridden;
    let preferences = if let Some(device) = device_override {
        overridden = [device];
        &overridden
    } else {
        config::INPUT_DEVICES
    };
    let input = AudioInput::open(preferences)?;
    feedback::preload()?;
    let mut recognizer = Moonshine::load(project_root)?;
    let mut events = EventLog::create(event_path)?;
    let input_monitor = InputMonitor::start()?;
    let context_monitor = ContextMonitor::start();
    let mut context = ContextSnapshot::default();
    let mut hotkey = OptionHotkey::new(suppression::option_key_is_down(), Instant::now());
    let mut mode = Mode::Listening;
    let mut voice_capture = None;
    let mut control_stability = ControlStability::default();
    let mut last_update = Instant::now();
    let mut last_audio_drop_report = Instant::now();
    let mut dropped_audio_chunks = 0;
    let mut dictation = DictationCapture::new(input.sample_rate);
    let dictation_worker = DictationWorker::start();
    let action_executor = ActionExecutor::start();

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
    emit_state(&mut events, hotkey.is_recording(), mode, &input.device_name)?;
    tracing::info!(device = %input.device_name, "voice recognition started");

    while !shutdown.load(Ordering::Relaxed) {
        dropped_audio_chunks += input.take_dropped_chunks();
        if last_audio_drop_report.elapsed() >= Duration::from_secs(1) {
            if dropped_audio_chunks > 0 {
                tracing::warn!(
                    dropped_audio_chunks,
                    "microphone chunks were dropped because audio consumption fell behind"
                );
                dropped_audio_chunks = 0;
            }
            last_audio_drop_report = Instant::now();
        }
        while let Ok(next_context) = context_monitor.updates.try_recv() {
            context = next_context;
            events.emit(&VoiceEvent::Context {
                timestamp_ms: now_ms(),
                application: context.application.clone(),
                browser_url: context.browser_url.as_ref().map(ToString::to_string),
            })?;
        }
        while let Some(outcome) = action_executor.try_recv() {
            handle_action_outcome(outcome, &mut events)?;
        }
        while let Ok(input_event) = input_monitor.events.try_recv() {
            if voice_capture.is_some() && input_event.is_escape_down() {
                voice_capture = None;
                control_stability.reset();
                recognizer.reset_stream()?;
                dictation.cancel();
                feedback::play(Tone::Cancel);
                events.dictation(DictationPhase::Cancelled, "")?;
                emit_state(&mut events, false, mode, &input.device_name)?;
                continue;
            }
            if let Some(action) = hotkey.process(input_event, Instant::now())
                && voice_capture.is_none()
            {
                handle_hotkey_action(
                    action,
                    &mut recognizer,
                    &mut dictation,
                    &dictation_worker,
                    mode,
                    &input.device_name,
                    &mut events,
                    indicator.as_ref(),
                )?;
            }
        }
        if let Some(action) = hotkey.reconcile(suppression::option_key_is_down())
            && voice_capture.is_none()
        {
            tracing::warn!("recovered a missed Option release");
            handle_hotkey_action(
                action,
                &mut recognizer,
                &mut dictation,
                &dictation_worker,
                mode,
                &input.device_name,
                &mut events,
                indicator.as_ref(),
            )?;
        }

        let mut received_worker_event = false;
        while let Some(event) = dictation_worker.try_recv() {
            received_worker_event = true;
            handle_dictation_event(event, &mut events, indicator.as_ref())?;
        }
        if received_worker_event {
            emit_engine_state(
                &mut events,
                hotkey.is_recording() || voice_capture.is_some(),
                &dictation_worker,
                mode,
                &input.device_name,
            )?;
        }

        match input.chunks.recv_timeout(Duration::from_millis(20)) {
            Ok(samples) if voice_capture.is_some() => {
                dictation.push(&samples);
                recognizer.add_audio(&samples, input.sample_rate)?;
            }
            Ok(samples) if hotkey.is_recording() => {
                if let Some(indicator) = &indicator {
                    indicator.meter(&samples);
                }
                dictation.push(&samples);
            }
            Ok(samples) => {
                dictation.keep_warm(&samples);
                recognizer.add_audio(&samples, input.sample_rate)?;
            }
            Err(RecvTimeoutError::Timeout) => {}
            Err(RecvTimeoutError::Disconnected) => break,
        }
        if let Some(capture) = voice_capture
            && dictation.is_full()
        {
            voice_capture = None;
            control_stability.reset();
            recognizer.reset_stream()?;
            let (id, target) = match capture {
                VoiceCapture::Dictation => {
                    ("dictation.maximum-duration", TranscriptionTarget::Paste)
                }
                VoiceCapture::CaptainsLog => (
                    "captains-log.maximum-duration",
                    TranscriptionTarget::CaptainsLog,
                ),
            };
            let finish = finish_dictation(
                &mut dictation,
                &dictation_worker,
                target,
                &input.device_name,
                &mut events,
                indicator.as_ref(),
            )?;
            events.emit(&VoiceEvent::Command {
                timestamp_ms: now_ms(),
                heard: "maximum dictation duration reached".into(),
                command: Some(id.into()),
                outcome: finish_outcome(&finish),
                context: context.label(),
            })?;
            emit_engine_state(
                &mut events,
                false,
                &dictation_worker,
                mode,
                &input.device_name,
            )?;
        }

        if !hotkey.is_recording() && last_update.elapsed() >= UPDATE_INTERVAL {
            for update in recognizer.update()? {
                let is_completed = matches!(update.phase, TranscriptPhase::Completed);
                if mode == Mode::Listening
                    && voice_capture.is_none()
                    && let Some(capture) = voice_capture_start(&update.text)
                {
                    recognizer.reset_stream()?;
                    dictation.start_voice(Instant::now());
                    voice_capture = Some(capture);
                    control_stability.reset();
                    feedback::play(Tone::DictationStart);
                    events.dictation(DictationPhase::Started, "")?;
                    emit_state(&mut events, true, mode, &input.device_name)?;
                    events.emit(&VoiceEvent::Command {
                        timestamp_ms: now_ms(),
                        heard: update.text,
                        command: Some(
                            match capture {
                                VoiceCapture::Dictation => "dictation.start",
                                VoiceCapture::CaptainsLog => "captains-log.start",
                            }
                            .into(),
                        ),
                        outcome: CommandOutcome::Executed,
                        context: context.label(),
                    })?;
                    break;
                }
                if let Some(capture) = voice_capture {
                    let control = match capture {
                        VoiceCapture::Dictation => {
                            dictation_control_suffix(&update.text).map(|(control, _)| control)
                        }
                        VoiceCapture::CaptainsLog => {
                            captains_log_end_suffix(&update.text).map(|_| DictationControl::Stop)
                        }
                    };
                    if let Some(control) = control_stability.observe(control, is_completed) {
                        voice_capture = None;
                        recognizer.reset_stream()?;
                        handle_voice_dictation_control(
                            capture,
                            control,
                            &update.text,
                            &mut dictation,
                            &dictation_worker,
                            mode,
                            &context,
                            &input.device_name,
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
                        &commands,
                        &mut mode,
                        &mut voice_capture,
                        &mut recognizer,
                        &mut dictation,
                        &action_executor,
                        meeting_requests.as_ref(),
                        &update.text,
                        &context,
                        &input.device_name,
                        &mut events,
                    )?;
                }
            }
            last_update = Instant::now();
        }
    }

    events.emit(&VoiceEvent::State {
        timestamp_ms: now_ms(),
        state: VoiceState::Stopping,
        device: input.device_name,
    })?;
    if let Some(indicator) = &indicator {
        indicator.send(DictationIndicatorEvent::Discarded);
    }
    Ok(())
}

fn voice_capture_start(heard: &str) -> Option<VoiceCapture> {
    if dictation_start_prefix(heard).is_some() {
        Some(VoiceCapture::Dictation)
    } else if captains_log_start_prefix(heard).is_some() {
        Some(VoiceCapture::CaptainsLog)
    } else {
        None
    }
}

#[allow(clippy::too_many_arguments)]
fn handle_hotkey_action(
    action: HotkeyAction,
    recognizer: &mut Moonshine,
    dictation: &mut DictationCapture,
    worker: &DictationWorker,
    mode: Mode,
    device: &str,
    events: &mut EventLog,
    indicator: Option<&DictationIndicatorSender>,
) -> Result<()> {
    recognizer.reset_stream()?;
    match action {
        HotkeyAction::Start => {
            dictation.start(Instant::now());
            feedback::play(Tone::DictationStart);
            events.dictation(DictationPhase::Started, "")?;
            if let Some(indicator) = indicator {
                indicator.send(DictationIndicatorEvent::Started);
            }
            emit_state(events, true, mode, device)
        }
        HotkeyAction::Finish => {
            let _ = finish_dictation(
                dictation,
                worker,
                TranscriptionTarget::Paste,
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
    }
}

#[allow(clippy::too_many_arguments)]
fn handle_command(
    commands: &CommandConfig,
    mode: &mut Mode,
    voice_capture: &mut Option<VoiceCapture>,
    recognizer: &mut Moonshine,
    dictation: &mut DictationCapture,
    action_executor: &ActionExecutor,
    meeting_requests: Option<&SyncSender<MeetingRequest>>,
    heard: &str,
    context: &ContextSnapshot,
    device: &str,
    events: &mut EventLog,
) -> Result<()> {
    let (command, outcome) = match commands.resolve(*mode, heard, context) {
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
        Decision::Execute { id, action }
            if matches!(action, Action::StartDictation | Action::StartCaptainsLog) =>
        {
            let capture = match action {
                Action::StartDictation => VoiceCapture::Dictation,
                Action::StartCaptainsLog => VoiceCapture::CaptainsLog,
                _ => unreachable!(),
            };
            recognizer.reset_stream()?;
            dictation.start_voice(Instant::now());
            *voice_capture = Some(capture);
            feedback::play(Tone::DictationStart);
            events.dictation(DictationPhase::Started, "")?;
            emit_state(events, true, *mode, device)?;
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
        command: Some(outcome.id.into()),
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

fn finish_dictation(
    capture: &mut DictationCapture,
    worker: &DictationWorker,
    target: TranscriptionTarget,
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
    if let Some(indicator) = indicator {
        indicator.send(DictationIndicatorEvent::Transcribing);
    }
    match worker.transcribe(clip, target) {
        Ok(()) => Ok(FinishResult::Submitted),
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
    capture: VoiceCapture,
    control: DictationControl,
    heard: &str,
    dictation: &mut DictationCapture,
    worker: &DictationWorker,
    mode: Mode,
    context: &ContextSnapshot,
    device: &str,
    events: &mut EventLog,
    indicator: Option<&DictationIndicatorSender>,
) -> Result<()> {
    let id = match (capture, control) {
        (VoiceCapture::CaptainsLog, _) => "captains-log.end",
        (_, DictationControl::Stop) => "dictation.stop",
        (_, DictationControl::Send) => "dictation.send",
        (_, DictationControl::Cancel) => "dictation.cancel",
    };
    match control {
        DictationControl::Stop | DictationControl::Send => {
            let target = match capture {
                VoiceCapture::CaptainsLog => TranscriptionTarget::CaptainsLog,
                VoiceCapture::Dictation if matches!(control, DictationControl::Send) => {
                    TranscriptionTarget::Send
                }
                VoiceCapture::Dictation => TranscriptionTarget::Paste,
            };
            let finish = finish_dictation(dictation, worker, target, device, events, indicator)?;
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
            result: Ok(text), ..
        } if text.trim().is_empty() => {
            events.dictation(DictationPhase::Discarded, "")?;
            if let Some(indicator) = indicator {
                indicator.send(DictationIndicatorEvent::Completed);
            }
        }
        WorkerEvent::Completed {
            target,
            result: Ok(text),
        } => {
            let phase = match target {
                TranscriptionTarget::CaptainsLog => DictationPhase::Logged,
                TranscriptionTarget::Paste | TranscriptionTarget::Send => DictationPhase::Pasted,
            };
            events.dictation(phase, text)?;
            if let Some(indicator) = indicator {
                indicator.send(DictationIndicatorEvent::Completed);
            }
        }
        WorkerEvent::Completed {
            result: Err(error), ..
        }
        | WorkerEvent::ModelFailed(error) => {
            tracing::error!(%error, "dictation failed");
            feedback::play(Tone::Error);
            events.dictation(DictationPhase::Failed(error), "")?;
            if let Some(indicator) = indicator {
                indicator.send(DictationIndicatorEvent::Failed);
            }
        }
        WorkerEvent::Repasted(Ok(text)) => {
            events.dictation(DictationPhase::Repasted, text)?;
        }
        WorkerEvent::Repasted(Err(error)) => {
            feedback::play(Tone::Error);
            events.dictation(DictationPhase::Failed(error), "")?;
        }
    }
    Ok(())
}
