use std::path::Path;
use std::sync::mpsc::RecvTimeoutError;
use std::time::{Duration, Instant};

use color_eyre::Result;

use crate::audio::AudioInput;
use crate::commands::{Action, CommandConfig, Decision, Mode};
use crate::config;
use crate::context::{ContextMonitor, ContextSnapshot};
use crate::dictation::{DictationCapture, DictationControl, Finish, dictation_control_suffix};
use crate::events::{
    CommandOutcome, DictationPhase, EventLog, TranscriptPhase, VoiceEvent, VoiceState, now_ms,
};
use crate::feedback::{self, Tone};
use crate::moonshine::Moonshine;
use crate::parakeet::{DictationWorker, WorkerEvent};
use crate::suppression::{self, HotkeyAction, InputMonitor, OptionHotkey};

const UPDATE_INTERVAL: Duration = Duration::from_millis(200);

pub fn listen(
    project_root: &Path,
    event_path: &Path,
    device_override: Option<&str>,
    commands: CommandConfig,
) -> Result<()> {
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
    let mut voice_dictating = false;
    let mut last_update = Instant::now();
    let mut dictation = DictationCapture::new(input.sample_rate);
    let dictation_worker = DictationWorker::start();

    events.emit(&VoiceEvent::SessionStarted {
        timestamp_ms: now_ms(),
    })?;
    if hotkey.is_recording() {
        dictation.start(Instant::now());
        events.dictation(DictationPhase::Started, "")?;
    }
    emit_state(&mut events, hotkey.is_recording(), mode, &input.device_name)?;
    tracing::info!(device = %input.device_name, "voice recognition started");

    loop {
        while let Ok(next_context) = context_monitor.updates.try_recv() {
            context = next_context;
            events.emit(&VoiceEvent::Context {
                timestamp_ms: now_ms(),
                application: context.application.clone(),
                browser_url: context.browser_url.as_ref().map(ToString::to_string),
            })?;
        }
        while let Ok(input_event) = input_monitor.events.try_recv() {
            if let Some(action) = hotkey.process(input_event, Instant::now())
                && !voice_dictating
            {
                handle_hotkey_action(
                    action,
                    &mut recognizer,
                    &mut dictation,
                    &dictation_worker,
                    mode,
                    &input.device_name,
                    &mut events,
                )?;
            }
        }
        if let Some(action) = hotkey.reconcile(suppression::option_key_is_down())
            && !voice_dictating
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
            )?;
        }

        if let Some(event) = dictation_worker.try_recv() {
            handle_dictation_event(event, &mut events)?;
            emit_state(
                &mut events,
                hotkey.is_recording() || voice_dictating,
                mode,
                &input.device_name,
            )?;
        }

        match input.chunks.recv_timeout(Duration::from_millis(20)) {
            Ok(samples) if voice_dictating => {
                dictation.push(&samples);
                recognizer.add_audio(&samples, input.sample_rate)?;
            }
            Ok(samples) if hotkey.is_recording() => dictation.push(&samples),
            Ok(samples) => {
                dictation.keep_warm(&samples);
                recognizer.add_audio(&samples, input.sample_rate)?;
            }
            Err(RecvTimeoutError::Timeout) => {}
            Err(RecvTimeoutError::Disconnected) => break,
        }

        if !hotkey.is_recording() && last_update.elapsed() >= UPDATE_INTERVAL {
            for update in recognizer.update()? {
                let is_completed = matches!(update.phase, TranscriptPhase::Completed);
                if !voice_dictating && has_voice_dictation_start_suffix(&update.text) {
                    recognizer.reset_stream()?;
                    dictation.start_without_pre_roll(Instant::now());
                    voice_dictating = true;
                    feedback::play(Tone::DictationStart);
                    events.dictation(DictationPhase::Started, "")?;
                    emit_state(&mut events, true, mode, &input.device_name)?;
                    events.emit(&VoiceEvent::Command {
                        timestamp_ms: now_ms(),
                        heard: update.text,
                        command: Some("dictation.start".into()),
                        outcome: CommandOutcome::Executed,
                        context: context.label(),
                    })?;
                    break;
                }
                if voice_dictating {
                    if let Some((control, _)) = dictation_control_suffix(&update.text) {
                        voice_dictating = false;
                        recognizer.reset_stream()?;
                        handle_voice_dictation_control(
                            control,
                            &update.text,
                            &mut dictation,
                            &dictation_worker,
                            mode,
                            &context,
                            &input.device_name,
                            &mut events,
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
                        &mut voice_dictating,
                        &mut recognizer,
                        &mut dictation,
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
    Ok(())
}

fn has_voice_dictation_start_suffix(heard: &str) -> bool {
    let heard = heard
        .trim()
        .trim_end_matches(|character: char| character.is_ascii_punctuation())
        .trim_end()
        .to_ascii_lowercase();
    [
        "dictate start",
        "dictates start",
        "dictate starts",
        "dictates starts",
        "start dictating",
    ]
    .into_iter()
    .any(|suffix| {
        heard
            .strip_suffix(suffix)
            .is_some_and(|prefix| prefix.is_empty() || prefix.ends_with(char::is_whitespace))
    })
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
) -> Result<()> {
    recognizer.reset_stream()?;
    match action {
        HotkeyAction::Start => {
            dictation.start(Instant::now());
            feedback::play(Tone::DictationStart);
            events.dictation(DictationPhase::Started, "")?;
            emit_state(events, true, mode, device)
        }
        HotkeyAction::Finish => {
            if !finish_dictation(dictation, worker, device, events)? {
                emit_state(events, false, mode, device)?;
            }
            Ok(())
        }
        HotkeyAction::Discard => {
            dictation.cancel();
            events.dictation(DictationPhase::Discarded, "")?;
            emit_state(events, false, mode, device)
        }
        HotkeyAction::Cancel => {
            dictation.cancel();
            feedback::play(Tone::Cancel);
            events.dictation(DictationPhase::Cancelled, "")?;
            emit_state(events, false, mode, device)
        }
        HotkeyAction::PasteLast => {
            dictation.cancel();
            if let Err(error) = worker.paste_last() {
                feedback::play(Tone::Error);
                events.dictation(DictationPhase::Failed(error.into()), "")?;
            }
            emit_state(events, false, mode, device)
        }
    }
}

#[allow(clippy::too_many_arguments)]
fn handle_command(
    commands: &CommandConfig,
    mode: &mut Mode,
    voice_dictating: &mut bool,
    recognizer: &mut Moonshine,
    dictation: &mut DictationCapture,
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
        Decision::Execute {
            id,
            action: Action::StartDictation,
        } => {
            recognizer.reset_stream()?;
            dictation.start_without_pre_roll(Instant::now());
            *voice_dictating = true;
            feedback::play(Tone::DictationStart);
            events.dictation(DictationPhase::Started, "")?;
            emit_state(events, true, *mode, device)?;
            (Some(id.into()), CommandOutcome::Executed)
        }
        Decision::Execute { id, action } => match crate::commands::execute(action) {
            Ok(()) => (Some(id.into()), CommandOutcome::Executed),
            Err(error) => {
                feedback::play(Tone::Error);
                (Some(id.into()), CommandOutcome::Failed(error.to_string()))
            }
        },
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

fn finish_dictation(
    capture: &mut DictationCapture,
    worker: &DictationWorker,
    device: &str,
    events: &mut EventLog,
) -> Result<bool> {
    let Finish::Transcribe(clip) = capture.finish(Instant::now()) else {
        events.dictation(DictationPhase::Discarded, "")?;
        return Ok(false);
    };
    feedback::play(Tone::DictationStop);
    events.emit(&VoiceEvent::State {
        timestamp_ms: now_ms(),
        state: VoiceState::Transcribing,
        device: device.into(),
    })?;
    events.dictation(DictationPhase::Transcribing, "")?;
    match worker.transcribe(clip, false) {
        Ok(()) => Ok(true),
        Err(error) => {
            feedback::play(Tone::Error);
            events.dictation(DictationPhase::Failed(error.into()), "")?;
            Ok(false)
        }
    }
}

#[allow(clippy::too_many_arguments)]
fn handle_voice_dictation_control(
    control: DictationControl,
    heard: &str,
    dictation: &mut DictationCapture,
    worker: &DictationWorker,
    mode: Mode,
    context: &ContextSnapshot,
    device: &str,
    events: &mut EventLog,
) -> Result<()> {
    let id = match control {
        DictationControl::Stop => "dictation.stop",
        DictationControl::Send => "dictation.send",
        DictationControl::Cancel => "dictation.cancel",
    };
    match control {
        DictationControl::Stop | DictationControl::Send => {
            let Finish::Transcribe(clip) = dictation.finish(Instant::now()) else {
                events.dictation(DictationPhase::Discarded, "")?;
                emit_state(events, false, mode, device)?;
                return Ok(());
            };
            feedback::play(Tone::DictationStop);
            events.emit(&VoiceEvent::State {
                timestamp_ms: now_ms(),
                state: VoiceState::Transcribing,
                device: device.into(),
            })?;
            events.dictation(DictationPhase::Transcribing, "")?;
            if let Err(error) = worker.transcribe(clip, matches!(control, DictationControl::Send)) {
                feedback::play(Tone::Error);
                events.dictation(DictationPhase::Failed(error.into()), "")?;
                emit_state(events, false, mode, device)?;
            }
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

fn handle_dictation_event(event: WorkerEvent, events: &mut EventLog) -> Result<()> {
    match event {
        WorkerEvent::Completed(Ok(text)) if text.trim().is_empty() => {
            events.dictation(DictationPhase::Discarded, "")?;
        }
        WorkerEvent::Completed(Ok(text)) => {
            events.dictation(DictationPhase::Pasted, text)?;
        }
        WorkerEvent::Completed(Err(error)) | WorkerEvent::ModelFailed(error) => {
            tracing::error!(%error, "dictation failed");
            feedback::play(Tone::Error);
            events.dictation(DictationPhase::Failed(error), "")?;
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

#[cfg(test)]
mod tests {
    use super::has_voice_dictation_start_suffix;

    #[test]
    fn dictation_start_is_a_resilient_suffix() {
        assert!(has_voice_dictation_start_suffix("Dictate start."));
        assert!(has_voice_dictation_start_suffix(
            "Okay, whenever you're ready, dictates start."
        ));
        assert!(has_voice_dictation_start_suffix("Start dictating."));
        assert!(!has_voice_dictation_start_suffix("Dictate."));
        assert!(!has_voice_dictation_start_suffix(
            "What does dictate start mean?"
        ));
    }
}
