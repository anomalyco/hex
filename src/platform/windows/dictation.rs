use std::path::Path;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{self, SyncSender, TrySendError};
use std::sync::{Arc, Mutex, RwLock};
use std::thread;
use std::time::{Duration, Instant};

use color_eyre::eyre::{Result, eyre};

use crate::audio::{CaptureInstant, RecoveringAudioInput, RecoveringAudioInputEvent};
use crate::dictation::{DictationCapture, Finish};
use crate::events::{DictationPhase, EventLog, TranscriptPhase, VoiceEvent, VoiceState, now_ms};
use crate::local_transcriber::LocalTranscriber;
use crate::transcription_models::TranscriptionSelection;
use crate::windows_input::{HotkeyAction, WindowsHotkeyMonitor};
use crate::windows_paste::WindowsPaster;
use crate::windows_settings::WindowsHotkey;

const UPDATE_INTERVAL: Duration = Duration::from_millis(5);

struct Job {
    samples: Vec<f32>,
    audio_ms: u64,
}

enum OutputJob {
    Dictation(Job),
    VoiceAction(Job),
    PasteLast,
}

#[derive(Clone, Copy)]
enum OutputKind {
    Dictation,
    VoiceAction,
    Repaste,
}

struct JobResult {
    kind: OutputKind,
    text: Option<String>,
    latency_ms: u32,
    result: Result<(), String>,
}

#[derive(Clone)]
pub struct WindowsDictationConfig {
    pub device: Option<String>,
    pub hotkey: WindowsHotkey,
    pub paste_last_hotkey: Option<WindowsHotkey>,
    pub voice_action_hotkey: Option<WindowsHotkey>,
    pub double_tap_lock: bool,
    pub double_tap_only: bool,
    pub while_dictating: crate::windows_settings::WhileDictating,
    pub release_microphone_while_idle: bool,
    pub feedback_volume: u8,
    pub history: Option<crate::history::History>,
    pub last_dictation: Arc<Mutex<Option<String>>>,
    pub indicator: Option<crate::windows_indicator::WindowsIndicatorSender>,
    pub replacements: Arc<RwLock<crate::text_replacements::ReplacementSet>>,
    pub modes: Arc<RwLock<Vec<crate::windows_settings::WindowsMode>>>,
    pub fallback_to_default_device: bool,
}

pub fn run(
    event_path: &Path,
    selection: &TranscriptionSelection,
    config: WindowsDictationConfig,
    shutdown: &AtomicBool,
) -> Result<()> {
    let transcriber = LocalTranscriber::load(selection)?;
    run_with_transcriber(event_path, config, shutdown, transcriber)
}

pub fn run_with_transcriber(
    event_path: &Path,
    config: WindowsDictationConfig,
    shutdown: &AtomicBool,
    transcriber: LocalTranscriber,
) -> Result<()> {
    let WindowsDictationConfig {
        device,
        hotkey,
        paste_last_hotkey,
        voice_action_hotkey,
        double_tap_lock,
        double_tap_only,
        while_dictating,
        release_microphone_while_idle,
        feedback_volume,
        history,
        last_dictation,
        indicator,
        replacements,
        modes,
        fallback_to_default_device,
    } = config;
    let _indicator_guard = IndicatorGuard(indicator.clone());
    crate::feedback::set_enabled(feedback_volume > 0);
    crate::feedback::set_volume(f32::from(feedback_volume) / 100.0);
    if feedback_volume > 0
        && let Err(error) = crate::feedback::preload()
    {
        tracing::warn!(%error, "Windows recording feedback is unavailable");
    }
    let release_while_idle = release_microphone_while_idle;
    let mut input = if release_while_idle {
        released_input(device.as_deref(), fallback_to_default_device)?
    } else {
        open_input(device.as_deref(), fallback_to_default_device)?
    };
    let mut events = EventLog::create(event_path)?;
    // With the microphone released the real sample rate is unknown until the
    // first open; this placeholder capture is replaced on Reopened.
    let mut capture = DictationCapture::new(if input.is_open() {
        input.sample_rate()
    } else {
        16_000
    });
    let mut captured_through = if release_while_idle {
        CaptureInstant::ZERO
    } else {
        prime_microphone(&mut input, &mut capture, shutdown)?
    };
    let mut audio_ready = !release_while_idle;
    let mut device_label = if input.is_open() {
        input.device_name().to_string()
    } else {
        device.clone().unwrap_or_else(|| "microphone".into())
    };
    // A hotkey press that arrived while the released microphone was still
    // opening; recording begins at the first chunk.
    let mut start_pending = false;
    if shutdown.load(Ordering::Relaxed) {
        return Ok(());
    }
    let hotkey_label = hotkey.label();
    // Double-tap-only depends on the double-tap window; without the lock
    // there is no second tap to wait for.
    let hotkey = WindowsHotkeyMonitor::start(
        hotkey,
        paste_last_hotkey,
        voice_action_hotkey,
        double_tap_lock,
        double_tap_lock && double_tap_only,
    )?;
    let audio_suppressor = crate::windows_audio_control::AudioSuppressor::start(while_dictating);
    let (jobs, job_receiver) = mpsc::sync_channel::<OutputJob>(2);
    let (result_sender, results) = mpsc::channel();
    let worker = thread::Builder::new()
        .name("windows-dictation-output".into())
        .spawn(move || {
            let mut transcriber = transcriber;
            let mut paster = WindowsPaster::new();
            while let Ok(job) = job_receiver.recv() {
                let result = match job {
                    OutputJob::Dictation(job) => process_dictation_job(
                        job,
                        &mut transcriber,
                        &mut paster,
                        history.as_ref(),
                        &last_dictation,
                        &replacements,
                        &modes,
                    ),
                    OutputJob::VoiceAction(job) => process_voice_action_job(
                        job,
                        &mut transcriber,
                        &mut paster,
                        &last_dictation,
                    ),
                    OutputJob::PasteLast => process_paste_last(&mut paster, &last_dictation),
                };
                if result_sender.send(result).is_err() {
                    break;
                }
            }
        })?;

    let mut recording = false;
    let mut recording_voice = false;
    let mut pending_voice = false;
    let mut recording_feedback_started = false;
    let mut pending = 0_usize;
    events.emit(&VoiceEvent::SessionStarted {
        timestamp_ms: now_ms(),
    })?;
    emit_state(&mut events, VoiceState::Listening, &device_label)?;
    println!(
        "HEX is ready on {}. Hold {} to dictate, release to paste; Escape cancels; Ctrl-C stops.",
        &device_label, hotkey_label
    );

    while !shutdown.load(Ordering::Relaxed) {
        if let Ok(error) = hotkey.errors.try_recv() {
            return Err(eyre!("Windows hotkey monitor stopped: {error}"));
        }
        while let Ok(event) = hotkey.events.try_recv() {
            let occurred_at = event.occurred_at(captured_through);
            match event.action {
                HotkeyAction::Start if !recording && audio_ready => {
                    capture.start_at(occurred_at);
                    recording = true;
                    recording_voice = false;
                    recording_feedback_started = false;
                    if let Some(suppressor) = &audio_suppressor {
                        suppressor.suppress();
                    }
                    if let Some(indicator) = &indicator {
                        indicator.send(crate::windows_indicator::WindowsIndicatorEvent::Recording);
                    }
                    events.dictation(DictationPhase::Started, "")?;
                    emit_state(&mut events, VoiceState::Dictating, &device_label)?;
                }
                HotkeyAction::VoiceStart
                    if !recording && release_while_idle && !start_pending && !audio_ready =>
                {
                    start_pending = true;
                    pending_voice = true;
                    input.request_open();
                    if let Some(suppressor) = &audio_suppressor {
                        suppressor.suppress();
                    }
                    if let Some(indicator) = &indicator {
                        indicator.send(crate::windows_indicator::WindowsIndicatorEvent::Recording);
                    }
                    events.dictation(DictationPhase::Started, "")?;
                    emit_state(&mut events, VoiceState::Dictating, &device_label)?;
                }
                HotkeyAction::VoiceStart if !recording && audio_ready => {
                    capture.start_at(occurred_at);
                    recording = true;
                    recording_voice = true;
                    recording_feedback_started = false;
                    if let Some(suppressor) = &audio_suppressor {
                        suppressor.suppress();
                    }
                    if let Some(indicator) = &indicator {
                        indicator.send(crate::windows_indicator::WindowsIndicatorEvent::Recording);
                    }
                    events.dictation(DictationPhase::Started, "")?;
                    emit_state(&mut events, VoiceState::Dictating, &device_label)?;
                }
                HotkeyAction::Start if !recording && release_while_idle && !start_pending => {
                    // The released microphone opens on demand; recording
                    // begins at the first chunk (no audio pre-roll).
                    start_pending = true;
                    pending_voice = false;
                    input.request_open();
                    if let Some(suppressor) = &audio_suppressor {
                        suppressor.suppress();
                    }
                    if let Some(indicator) = &indicator {
                        indicator.send(crate::windows_indicator::WindowsIndicatorEvent::Recording);
                    }
                    events.dictation(DictationPhase::Started, "")?;
                    emit_state(&mut events, VoiceState::Dictating, &device_label)?;
                }
                HotkeyAction::Finish
                | HotkeyAction::Cancel
                | HotkeyAction::VoiceFinish
                | HotkeyAction::VoiceCancel
                    if start_pending =>
                {
                    pending_voice = false;
                    // Released before the opening microphone produced audio:
                    // nothing was captured, so treat it as a discard.
                    start_pending = false;
                    if let Some(suppressor) = &audio_suppressor {
                        suppressor.restore();
                    }
                    input.close();
                    audio_ready = false;
                    events.dictation(
                        if matches!(
                            event.action,
                            HotkeyAction::Cancel | HotkeyAction::VoiceCancel
                        ) {
                            DictationPhase::Cancelled
                        } else {
                            DictationPhase::Discarded
                        },
                        "",
                    )?;
                    if let Some(indicator) = &indicator {
                        indicator.send(if pending > 0 {
                            crate::windows_indicator::WindowsIndicatorEvent::Processing
                        } else {
                            crate::windows_indicator::WindowsIndicatorEvent::Hidden
                        });
                    }
                    emit_state(
                        &mut events,
                        if pending > 0 {
                            VoiceState::Transcribing
                        } else {
                            VoiceState::Listening
                        },
                        &device_label,
                    )?;
                }
                HotkeyAction::VoiceFinish if recording && recording_voice => {
                    if let Some(suppressor) = &audio_suppressor {
                        suppressor.restore();
                    }
                    if !recording_feedback_started && capture.become_intentional(occurred_at) {
                        crate::feedback::play(crate::feedback::Tone::DictationStart);
                        recording_feedback_started = true;
                    }
                    if recording_feedback_started {
                        crate::feedback::play(crate::feedback::Tone::DictationStop);
                    }
                    recording = false;
                    recording_voice = false;
                    recording_feedback_started = false;
                    let submitted = submit_capture(
                        &mut capture,
                        occurred_at,
                        &jobs,
                        &mut events,
                        &mut pending,
                        true,
                    )?;
                    if let Some(indicator) = &indicator {
                        indicator.send(if submitted || pending > 0 {
                            crate::windows_indicator::WindowsIndicatorEvent::Processing
                        } else {
                            crate::windows_indicator::WindowsIndicatorEvent::Hidden
                        });
                    }
                    emit_state(
                        &mut events,
                        if pending > 0 {
                            VoiceState::Transcribing
                        } else {
                            VoiceState::Listening
                        },
                        &device_label,
                    )?;
                    if release_while_idle {
                        input.close();
                        audio_ready = false;
                    }
                }
                HotkeyAction::VoiceCancel if recording && recording_voice => {
                    if let Some(suppressor) = &audio_suppressor {
                        suppressor.restore();
                    }
                    if recording_feedback_started {
                        crate::feedback::play(crate::feedback::Tone::Cancel);
                    }
                    recording = false;
                    recording_voice = false;
                    recording_feedback_started = false;
                    capture.cancel();
                    if let Some(indicator) = &indicator {
                        indicator.send(if pending > 0 {
                            crate::windows_indicator::WindowsIndicatorEvent::Processing
                        } else {
                            crate::windows_indicator::WindowsIndicatorEvent::Hidden
                        });
                    }
                    events.dictation(DictationPhase::Cancelled, "")?;
                    emit_state(
                        &mut events,
                        if pending > 0 {
                            VoiceState::Transcribing
                        } else {
                            VoiceState::Listening
                        },
                        &device_label,
                    )?;
                    if release_while_idle {
                        input.close();
                        audio_ready = false;
                    }
                }
                HotkeyAction::Finish if recording && !recording_voice => {
                    if let Some(suppressor) = &audio_suppressor {
                        suppressor.restore();
                    }
                    if !recording_feedback_started && capture.become_intentional(occurred_at) {
                        crate::feedback::play(crate::feedback::Tone::DictationStart);
                        recording_feedback_started = true;
                    }
                    if recording_feedback_started {
                        crate::feedback::play(crate::feedback::Tone::DictationStop);
                    }
                    recording_feedback_started = false;
                    recording = false;
                    let submitted = submit_capture(
                        &mut capture,
                        occurred_at,
                        &jobs,
                        &mut events,
                        &mut pending,
                        false,
                    )?;
                    if let Some(indicator) = &indicator {
                        indicator.send(if submitted || pending > 0 {
                            crate::windows_indicator::WindowsIndicatorEvent::Processing
                        } else {
                            crate::windows_indicator::WindowsIndicatorEvent::Hidden
                        });
                    }
                    emit_state(
                        &mut events,
                        if pending > 0 {
                            VoiceState::Transcribing
                        } else {
                            VoiceState::Listening
                        },
                        &device_label,
                    )?;
                    if release_while_idle {
                        input.close();
                        audio_ready = false;
                    }
                }
                HotkeyAction::Cancel if recording && !recording_voice => {
                    if let Some(suppressor) = &audio_suppressor {
                        suppressor.restore();
                    }
                    if recording_feedback_started {
                        crate::feedback::play(crate::feedback::Tone::Cancel);
                    }
                    recording_feedback_started = false;
                    capture.cancel();
                    recording = false;
                    if let Some(indicator) = &indicator {
                        indicator.send(if pending > 0 {
                            crate::windows_indicator::WindowsIndicatorEvent::Processing
                        } else {
                            crate::windows_indicator::WindowsIndicatorEvent::Hidden
                        });
                    }
                    events.dictation(DictationPhase::Cancelled, "")?;
                    emit_state(
                        &mut events,
                        if pending > 0 {
                            VoiceState::Transcribing
                        } else {
                            VoiceState::Listening
                        },
                        &device_label,
                    )?;
                    if release_while_idle {
                        input.close();
                        audio_ready = false;
                    }
                }
                HotkeyAction::PasteLast if !recording => {
                    submit_paste_last(&jobs, &mut events, &mut pending)?;
                    emit_state(&mut events, VoiceState::Transcribing, &device_label)?;
                }
                _ => {}
            }
        }
        while let Ok(result) = results.try_recv() {
            pending = pending.saturating_sub(1);
            match result.result {
                Ok(()) => {
                    let text = result.text.unwrap_or_default();
                    match result.kind {
                        OutputKind::Dictation | OutputKind::VoiceAction => {
                            events.emit(&VoiceEvent::Transcript {
                                timestamp_ms: now_ms(),
                                phase: TranscriptPhase::Completed,
                                latency_ms: result.latency_ms,
                                text: text.clone(),
                            })?;
                            events.dictation(DictationPhase::Pasted, text)?;
                        }
                        OutputKind::Repaste => {
                            events.dictation(DictationPhase::Repasted, text)?;
                        }
                    }
                }
                Err(error) => events.dictation(DictationPhase::Failed(error), "")?,
            }
            emit_state(
                &mut events,
                if recording {
                    VoiceState::Dictating
                } else if pending > 0 {
                    VoiceState::Transcribing
                } else {
                    VoiceState::Listening
                },
                &device_label,
            )?;
            if let Some(indicator) = &indicator
                && !recording
            {
                indicator.send(if pending > 0 {
                    crate::windows_indicator::WindowsIndicatorEvent::Processing
                } else {
                    crate::windows_indicator::WindowsIndicatorEvent::Hidden
                });
            }
        }

        match input.recv_timeout(UPDATE_INTERVAL, !recording) {
            RecoveringAudioInputEvent::Chunk {
                samples,
                captured_through: chunk_captured_through,
            } => {
                captured_through = chunk_captured_through;
                if !audio_ready {
                    capture = DictationCapture::new(input.sample_rate());
                    capture.keep_warm(&samples);
                    audio_ready = true;
                    device_label = input.device_name().to_string();
                    if start_pending {
                        // The on-demand open finished while the shortcut is
                        // still held: recording starts here, without the
                        // pre-roll a held-open microphone would have had.
                        start_pending = false;
                        capture.start_at(chunk_captured_through);
                        recording = true;
                        recording_voice = pending_voice;
                        pending_voice = false;
                        recording_feedback_started = false;
                        emit_state(&mut events, VoiceState::Dictating, &device_label)?;
                        continue;
                    }
                    emit_state(
                        &mut events,
                        if pending > 0 {
                            VoiceState::Transcribing
                        } else {
                            VoiceState::Listening
                        },
                        &device_label,
                    )?;
                    continue;
                }
                if recording {
                    capture.ingest(&samples, captured_through);
                    if let Some(indicator) = &indicator {
                        indicator.meter(&samples);
                    }
                    if capture.become_intentional(captured_through) && !recording_feedback_started {
                        crate::feedback::play(crate::feedback::Tone::DictationStart);
                        recording_feedback_started = true;
                    }
                } else {
                    capture.keep_warm(&samples);
                }
            }
            RecoveringAudioInputEvent::Timeout => {}
            RecoveringAudioInputEvent::Interrupted => {
                audio_ready = false;
                if recording {
                    if recording_feedback_started {
                        crate::feedback::play(crate::feedback::Tone::Cancel);
                    }
                    recording_feedback_started = false;
                    recording = false;
                    recording_voice = false;
                    capture.cancel();
                    events.dictation(DictationPhase::Cancelled, "")?;
                }
                events.dictation(
                    DictationPhase::Failed("microphone stream interrupted; reopening".into()),
                    "",
                )?;
                if let Some(indicator) = &indicator {
                    indicator.send(if pending > 0 {
                        crate::windows_indicator::WindowsIndicatorEvent::Processing
                    } else {
                        crate::windows_indicator::WindowsIndicatorEvent::Hidden
                    });
                }
                emit_state(
                    &mut events,
                    if pending > 0 {
                        VoiceState::Transcribing
                    } else {
                        VoiceState::Listening
                    },
                    &device_label,
                )?;
            }
            RecoveringAudioInputEvent::Reopened => {
                capture = DictationCapture::new(input.sample_rate());
                audio_ready = false;
                device_label = input.device_name().to_string();
            }
            RecoveringAudioInputEvent::OpenFailed(error) => {
                if !release_while_idle {
                    return Err(eyre!("could not reopen microphone: {error}"));
                }
                // On-demand open failed (device busy or unplugged): report
                // and stay released rather than killing the listener.
                start_pending = false;
                if let Some(suppressor) = &audio_suppressor {
                    suppressor.restore();
                }
                events.dictation(
                    DictationPhase::Failed(format!("could not open microphone: {error}")),
                    "",
                )?;
                if let Some(indicator) = &indicator {
                    indicator.send(if pending > 0 {
                        crate::windows_indicator::WindowsIndicatorEvent::Processing
                    } else {
                        crate::windows_indicator::WindowsIndicatorEvent::Hidden
                    });
                }
                emit_state(
                    &mut events,
                    if pending > 0 {
                        VoiceState::Transcribing
                    } else {
                        VoiceState::Listening
                    },
                    &device_label,
                )?;
            }
        }
    }

    if recording {
        if recording_feedback_started {
            crate::feedback::play(crate::feedback::Tone::Cancel);
        }
        capture.cancel();
        events.dictation(DictationPhase::Cancelled, "")?;
    }
    emit_state(&mut events, VoiceState::Stopping, &device_label)?;
    drop(hotkey);
    drop(input);
    drop(jobs);
    if worker.join().is_err() {
        return Err(eyre!("Windows dictation worker panicked"));
    }
    events.flush()?;
    println!("Stopped.");
    Ok(())
}

fn process_dictation_job(
    job: Job,
    transcriber: &mut LocalTranscriber,
    paster: &mut Result<WindowsPaster>,
    history: Option<&crate::history::History>,
    last_dictation: &Mutex<Option<String>>,
    replacements: &RwLock<crate::text_replacements::ReplacementSet>,
    modes: &RwLock<Vec<crate::windows_settings::WindowsMode>>,
) -> JobResult {
    let started = Instant::now();
    let inference_started = Instant::now();
    let transcription = transcriber
        .transcribe(&job.samples)
        .map(|text| text.trim().to_string())
        .map_err(|error| format!("{error:#}"));
    let inference_ms = inference_started.elapsed().as_millis() as u64;
    let application = crate::windows_input::foreground_process_stem();
    let result = transcription.and_then(|raw_text| {
        if raw_text.is_empty() {
            return Err("transcription was empty".into());
        }
        let text = replacements
            .read()
            .unwrap_or_else(|error| error.into_inner())
            .replace(&raw_text);
        // Mode corrections run after the global replacements, scoped to the
        // application the text is about to land in.
        let text = {
            let modes = modes.read().unwrap_or_else(|error| error.into_inner());
            match crate::windows_settings::mode_for_application(&modes, application.as_deref()) {
                Some(mode) if !mode.corrections.is_empty() => {
                    crate::text_replacements::ReplacementSet::new(&mode.corrections).replace(&text)
                }
                _ => text,
            }
        };
        if text.trim().is_empty() {
            return Err("text replacements produced empty output".into());
        }
        paster
            .as_mut()
            .map_err(|error| format!("{error:#}"))?
            .paste(&text)
            .map_err(|error| format!("{error:#}"))?;
        *last_dictation
            .lock()
            .unwrap_or_else(|error| error.into_inner()) = Some(text.clone());
        if let Some(history) = history {
            let draft = crate::history::HistoryDraft {
                kind: crate::history::HistoryKind::Dictation,
                raw_text,
                final_text: text.clone(),
                application: application.clone(),
                processing: None,
                audio_ms: job.audio_ms,
                inference_ms,
                total_ms: started.elapsed().as_millis() as u64,
            };
            if let Err(error) = history.record(draft) {
                tracing::warn!(%error, "could not retain Windows dictation history");
            }
        }
        Ok(text)
    });
    let latency_ms = started.elapsed().as_millis().min(u128::from(u32::MAX)) as u32;
    tracing::info!(
        audio_ms = job.audio_ms,
        latency_ms,
        "completed Windows dictation job"
    );
    job_result(OutputKind::Dictation, result, latency_ms)
}

/// Transcribe the held instruction, pair it with the focused application's
/// selection, ask OpenCode, and paste the reply.
fn process_voice_action_job(
    job: Job,
    transcriber: &mut LocalTranscriber,
    paster: &mut Result<WindowsPaster>,
    last_dictation: &Mutex<Option<String>>,
) -> JobResult {
    let started = Instant::now();
    let transcription = transcriber
        .transcribe(&job.samples)
        .map(|text| text.trim().to_string())
        .map_err(|error| format!("{error:#}"));
    let application = crate::windows_input::foreground_process_stem();
    let result = transcription.and_then(|instruction| {
        if instruction.is_empty() {
            return Err("the voice instruction was empty".into());
        }
        let paster = paster.as_mut().map_err(|error| format!("{error:#}"))?;
        let selected = paster.copy_selected_text().unwrap_or_else(|error| {
            tracing::warn!(%error, "could not copy the selection for the voice action");
            None
        });
        let prompt = crate::windows_voice_action::voice_action_prompt(
            &instruction,
            application.as_deref(),
            selected.as_deref(),
        );
        let reply = crate::windows_voice_action::generate(
            &prompt,
            crate::windows_voice_action::GENERATION_DEADLINE,
        )
        .map_err(|error| format!("{error:#}"))?;
        paster.paste(&reply).map_err(|error| format!("{error:#}"))?;
        *last_dictation
            .lock()
            .unwrap_or_else(|error| error.into_inner()) = Some(reply.clone());
        Ok(reply)
    });
    match result {
        Ok(text) => JobResult {
            kind: OutputKind::VoiceAction,
            text: Some(text),
            latency_ms: started.elapsed().as_millis() as u32,
            result: Ok(()),
        },
        Err(error) => JobResult {
            kind: OutputKind::VoiceAction,
            text: None,
            latency_ms: started.elapsed().as_millis() as u32,
            result: Err(error),
        },
    }
}

fn process_paste_last(
    paster: &mut Result<WindowsPaster>,
    last_dictation: &Mutex<Option<String>>,
) -> JobResult {
    let started = Instant::now();
    let result = last_dictation
        .lock()
        .unwrap_or_else(|error| error.into_inner())
        .clone()
        .ok_or_else(|| "there is no completed dictation to paste".to_string())
        .and_then(|text| {
            paster
                .as_mut()
                .map_err(|error| format!("{error:#}"))?
                .paste(&text)
                .map_err(|error| format!("{error:#}"))?;
            Ok(text)
        });
    let latency_ms = started.elapsed().as_millis().min(u128::from(u32::MAX)) as u32;
    tracing::info!(latency_ms, "completed Windows Paste Last job");
    job_result(OutputKind::Repaste, result, latency_ms)
}

fn job_result(kind: OutputKind, result: Result<String, String>, latency_ms: u32) -> JobResult {
    match result {
        Ok(text) => JobResult {
            kind,
            text: Some(text),
            latency_ms,
            result: Ok(()),
        },
        Err(error) => JobResult {
            kind,
            text: None,
            latency_ms,
            result: Err(error),
        },
    }
}

fn prime_microphone(
    input: &mut RecoveringAudioInput,
    capture: &mut DictationCapture,
    shutdown: &AtomicBool,
) -> Result<CaptureInstant> {
    while !shutdown.load(Ordering::Relaxed) {
        match input.recv_timeout(Duration::from_millis(100), true) {
            RecoveringAudioInputEvent::Chunk {
                samples,
                captured_through,
            } => {
                capture.keep_warm(&samples);
                return Ok(captured_through);
            }
            RecoveringAudioInputEvent::Timeout | RecoveringAudioInputEvent::Interrupted => {}
            RecoveringAudioInputEvent::Reopened => {
                *capture = DictationCapture::new(input.sample_rate());
            }
            RecoveringAudioInputEvent::OpenFailed(error) => {
                return Err(eyre!("could not reopen microphone: {error}"));
            }
        }
    }
    Ok(CaptureInstant::ZERO)
}

fn open_input(
    device: Option<&str>,
    fallback_to_default_device: bool,
) -> Result<RecoveringAudioInput> {
    if fallback_to_default_device {
        return RecoveringAudioInput::open(None, 0, device);
    }
    let Some(query) = device else {
        return RecoveringAudioInput::open(None, 0, None);
    };
    let exact_name = resolve_device_name(query)?;
    RecoveringAudioInput::open(Some(&exact_name), 0, None)
}

/// Like [`open_input`], but the microphone stays released; the loop opens it
/// on demand when a dictation starts.
fn released_input(
    device: Option<&str>,
    fallback_to_default_device: bool,
) -> Result<RecoveringAudioInput> {
    if fallback_to_default_device {
        return Ok(RecoveringAudioInput::closed(None, 0, device));
    }
    let Some(query) = device else {
        return Ok(RecoveringAudioInput::closed(None, 0, None));
    };
    let exact_name = resolve_device_name(query)?;
    Ok(RecoveringAudioInput::closed(Some(&exact_name), 0, None))
}

fn resolve_device_name(query: &str) -> Result<String> {
    let query_lower = query.to_lowercase();
    crate::audio::input_device_names()?
        .into_iter()
        .find(|name| name.to_lowercase().contains(&query_lower))
        .ok_or_else(|| eyre!("microphone is unavailable: {query}"))
}

fn submit_capture(
    capture: &mut DictationCapture,
    ended_at: CaptureInstant,
    jobs: &SyncSender<OutputJob>,
    events: &mut EventLog,
    pending: &mut usize,
    voice_action: bool,
) -> Result<bool> {
    let mut submitted = false;
    match capture.finish(ended_at) {
        Finish::Discard => events.dictation(DictationPhase::Discarded, "")?,
        Finish::Transcribe(clip) => {
            let audio_ms = clip.duration_ms();
            let job = Job {
                samples: clip.into_parakeet_samples(),
                audio_ms,
            };
            let job = if voice_action {
                OutputJob::VoiceAction(job)
            } else {
                OutputJob::Dictation(job)
            };
            match jobs.try_send(job) {
                Ok(()) => {
                    *pending += 1;
                    submitted = true;
                    events.dictation(DictationPhase::Transcribing, "")?;
                }
                Err(TrySendError::Full(_)) => events
                    .dictation(DictationPhase::Failed("dictation queue is full".into()), "")?,
                Err(TrySendError::Disconnected(_)) => {
                    return Err(eyre!("Windows dictation worker is unavailable"));
                }
            }
        }
    }
    Ok(submitted)
}

fn submit_paste_last(
    jobs: &SyncSender<OutputJob>,
    events: &mut EventLog,
    pending: &mut usize,
) -> Result<()> {
    match jobs.try_send(OutputJob::PasteLast) {
        Ok(()) => *pending += 1,
        Err(TrySendError::Full(_)) => events.dictation(
            DictationPhase::Failed("dictation output queue is full".into()),
            "",
        )?,
        Err(TrySendError::Disconnected(_)) => {
            return Err(eyre!("Windows dictation worker is unavailable"));
        }
    }
    Ok(())
}

fn emit_state(events: &mut EventLog, state: VoiceState, device: &str) -> Result<()> {
    events.emit(&VoiceEvent::State {
        timestamp_ms: now_ms(),
        state,
        device: device.into(),
    })?;
    Ok(())
}

struct IndicatorGuard(Option<crate::windows_indicator::WindowsIndicatorSender>);

impl Drop for IndicatorGuard {
    fn drop(&mut self) {
        if let Some(indicator) = &self.0 {
            indicator.send(crate::windows_indicator::WindowsIndicatorEvent::Hidden);
        }
    }
}
