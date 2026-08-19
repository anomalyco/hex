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
    pub voice_action_model: Arc<RwLock<Option<crate::opencode::Model>>>,
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
        voice_action_model,
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
                        &voice_action_model,
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
    // The worker drains its queue in order, so a FIFO of ids pairs each
    // result with the HUD job it belongs to. Repaste jobs get ids too (to
    // keep the queue aligned) but are never announced to the HUD.
    let mut next_job_id = 0_u64;
    let mut job_ids = std::collections::VecDeque::new();
    events.emit(&VoiceEvent::SessionStarted {
        timestamp_ms: now_ms(),
    })?;
    emit_state(&mut events, VoiceState::Listening, &device_label)?;
    println!(
        "HEX is ready on {}. Hold {} to dictate, release to paste; Escape cancels; Ctrl-C stops.",
        device_label, hotkey_label
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
                        indicator.send(crate::windows_indicator::WindowsIndicatorEvent::Started);
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
                        indicator
                            .send(crate::windows_indicator::WindowsIndicatorEvent::EditingStarted);
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
                        indicator
                            .send(crate::windows_indicator::WindowsIndicatorEvent::EditingStarted);
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
                        indicator.send(crate::windows_indicator::WindowsIndicatorEvent::Started);
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
                        indicator.send(
                            if matches!(
                                event.action,
                                HotkeyAction::Cancel | HotkeyAction::VoiceCancel
                            ) {
                                crate::windows_indicator::WindowsIndicatorEvent::Cancelled
                            } else {
                                crate::windows_indicator::WindowsIndicatorEvent::Discarded
                            },
                        );
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
                    if submitted {
                        next_job_id += 1;
                        job_ids.push_back(next_job_id);
                    }
                    if let Some(indicator) = &indicator {
                        indicator.send(if submitted {
                            crate::windows_indicator::WindowsIndicatorEvent::Submitted {
                                job_id: next_job_id,
                            }
                        } else {
                            crate::windows_indicator::WindowsIndicatorEvent::Discarded
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
                        indicator.send(crate::windows_indicator::WindowsIndicatorEvent::Cancelled);
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
                    if submitted {
                        next_job_id += 1;
                        job_ids.push_back(next_job_id);
                    }
                    if let Some(indicator) = &indicator {
                        indicator.send(if submitted {
                            crate::windows_indicator::WindowsIndicatorEvent::Submitted {
                                job_id: next_job_id,
                            }
                        } else {
                            crate::windows_indicator::WindowsIndicatorEvent::Discarded
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
                        indicator.send(crate::windows_indicator::WindowsIndicatorEvent::Cancelled);
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
                    let queued = pending;
                    submit_paste_last(&jobs, &mut events, &mut pending)?;
                    if pending > queued {
                        next_job_id += 1;
                        job_ids.push_back(next_job_id);
                    }
                    emit_state(&mut events, VoiceState::Transcribing, &device_label)?;
                }
                _ => {}
            }
        }
        while let Ok(result) = results.try_recv() {
            pending = pending.saturating_sub(1);
            let job_id = job_ids.pop_front();
            if let (Some(indicator), Some(job_id)) = (&indicator, job_id) {
                // Repaste ids were never announced, so the HUD ignores them.
                indicator.send(if result.result.is_ok() {
                    crate::windows_indicator::WindowsIndicatorEvent::JobCompleted { job_id }
                } else {
                    crate::windows_indicator::WindowsIndicatorEvent::JobFailed { job_id }
                });
            }
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
                    if let Some(indicator) = &indicator {
                        indicator.send(crate::windows_indicator::WindowsIndicatorEvent::Cancelled);
                    }
                }
                events.dictation(
                    DictationPhase::Failed("microphone stream interrupted; reopening".into()),
                    "",
                )?;
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
                    indicator.send(crate::windows_indicator::WindowsIndicatorEvent::Failed);
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
    // The context read starts now so the UIA walk overlaps inference.
    let pending_context = crate::windows_context::begin_capture();
    let inference_started = Instant::now();
    let transcription = transcriber
        .transcribe(&job.samples)
        .map(|text| text.trim().to_string())
        .map_err(|error| format!("{error:#}"));
    let inference_ms = inference_started.elapsed().as_millis() as u64;
    let context = pending_context.finish();
    let application = context.application.clone();
    let result = transcription.and_then(|raw_text| {
        if raw_text.is_empty() {
            return Err("transcription was empty".into());
        }
        let text = replacements
            .read()
            .unwrap_or_else(|error| error.into_inner())
            .replace(&raw_text);
        // Context selection remains in the native adapter because Windows
        // application rules intentionally match executable substrings. The
        // selected mode then enters the shared ordered processing policy.
        let processed = {
            let modes = modes.read().unwrap_or_else(|error| error.into_inner());
            process_mode_text(&text, &context, &modes)
        };
        let text = processed.text;
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
                processing: processed.observation.map(|processing| {
                    crate::history::HistoryProcessing {
                        profile: processing.profile,
                        latency_ms: processing.latency_ms,
                        fallback: processing.fallback,
                    }
                }),
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

fn process_mode_text(
    text: &str,
    context: &crate::command_context::ContextSnapshot,
    modes: &[crate::windows_settings::WindowsMode],
) -> crate::dictation_processing::Processed {
    let mode = crate::windows_settings::mode_for_context(
        modes,
        context.application.as_deref(),
        context.browser_host(),
    );
    let profile = mode.map_or_else(
        || crate::dictation_processing::Profile::new("Global", ""),
        |mode| {
            let name = if mode.name.trim().is_empty() {
                "Untitled mode"
            } else {
                &mode.name
            };
            crate::dictation_processing::Profile::new(name, "").replacements(
                crate::text_replacements::ReplacementSet::new(&mode.corrections),
            )
        },
    );
    crate::dictation_processing::Profiles::new(profile).process_cancellable(
        text,
        context,
        None,
        &AtomicBool::new(false),
    )
}

/// Transcribe the held instruction, pair it with the focused application's
/// selection, ask OpenCode, and paste the reply.
fn process_voice_action_job(
    job: Job,
    transcriber: &mut LocalTranscriber,
    paster: &mut Result<WindowsPaster>,
    last_dictation: &Mutex<Option<String>>,
    voice_action_model: &RwLock<Option<crate::opencode::Model>>,
) -> JobResult {
    let started = Instant::now();
    // The context read starts now so the UIA walk overlaps inference.
    let pending_context = crate::windows_context::begin_capture();
    let transcription = transcriber
        .transcribe(&job.samples)
        .map(|text| text.trim().to_string())
        .map_err(|error| format!("{error:#}"));
    let context = pending_context.finish();
    let result = transcription.and_then(|instruction| {
        if instruction.is_empty() {
            return Err("the voice instruction was empty".into());
        }
        let paster = paster.as_mut().map_err(|error| format!("{error:#}"))?;
        let selected = paster.copy_selected_text().unwrap_or_else(|error| {
            tracing::warn!(%error, "could not copy the selection for the voice action");
            None
        });
        let model = voice_action_model
            .read()
            .unwrap_or_else(|error| error.into_inner())
            .clone();
        let reply = crate::windows_voice_action::fulfil(
            &instruction,
            context.application.as_deref(),
            context.browser_host(),
            selected.as_deref(),
            model.as_ref(),
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
            indicator.send(crate::windows_indicator::WindowsIndicatorEvent::Reset);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn windows_mode_corrections_use_the_shared_processing_policy() {
        let modes = vec![crate::windows_settings::WindowsMode {
            name: "Code".into(),
            applications: vec!["code".into()],
            corrections: vec![crate::text_replacements::TextReplacement {
                matched_phrase: "open code".into(),
                output: "OpenCode".into(),
            }],
            ..crate::windows_settings::WindowsMode::default()
        }];
        let context = crate::command_context::ContextSnapshot {
            application: Some("Visual Studio Code".into()),
            ..crate::command_context::ContextSnapshot::default()
        };

        let processed = process_mode_text("Use open code.", &context, &modes);

        assert_eq!(processed.text, "Use OpenCode.");
        assert!(processed.observation.is_none());
    }
}
