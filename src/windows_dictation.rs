use std::path::Path;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{self, SyncSender, TrySendError};
use std::sync::{Arc, Mutex, RwLock};
use std::thread;
use std::time::{Duration, Instant};

use color_eyre::eyre::{Result, eyre};

use crate::audio::{CaptureInstant, RecoveringAudioInput, RecoveringAudioInputEvent};
use crate::dictation::{DictationCapture, Finish, POST_RELEASE_TAIL};
use crate::events::{DictationPhase, EventLog, TranscriptPhase, VoiceEvent, VoiceState, now_ms};
use crate::local_transcriber::LocalTranscriber;
use crate::transcription_engine::{PendingTranscript, TranscriptionEngine};
use crate::transcription_models::TranscriptionSelection;
use crate::windows_input::WindowsHotkey;
use crate::windows_input::{HotkeyAction, WindowsHotkeyMonitor};
use crate::windows_paste::WindowsPaster;

const UPDATE_INTERVAL: Duration = Duration::from_millis(5);
/// How long past the tail a release waits for audio that never arrives.
const TAIL_STALL_GRACE: Duration = Duration::from_millis(100);

struct Job {
    transcript: PendingTranscript,
    audio_ms: u64,
}

/// A release whose capture is still collecting its post-release tail.
struct Finishing {
    ended_at: CaptureInstant,
    voice_action: bool,
    deadline: Instant,
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
    pub shortcut_mode: crate::dictation::ShortcutMode,
    pub while_dictating: crate::app_settings::RecordingAudioBehavior,
    pub release_microphone_while_idle: bool,
    pub feedback_volume: u8,
    pub history: Option<crate::history::History>,
    pub last_dictation: Arc<Mutex<Option<String>>>,
    pub indicator: Option<crate::windows_rune_hud::WindowsIndicatorSender>,
    pub mode_runtime: Arc<RwLock<crate::dictation_modes::ModeRuntime>>,
    pub transformations: Arc<crate::personal_commands::TransformationClient>,
    pub voice_action_model: Arc<RwLock<Option<crate::dictation_processor::Model>>>,
    pub fallback_to_default_device: bool,
    /// Where the listener reports a model change that failed to load.
    pub swap_failures: Option<mpsc::Sender<crate::local_transcriber::SwapFailed>>,
}

impl WindowsDictationConfig {
    /// The listener configuration `settings` describe, with fresh runtime
    /// state; a host swaps in the state it shares with the listener.
    pub fn from_settings(settings: &crate::app_settings::AppSettings) -> Self {
        Self {
            device: settings.microphone.clone(),
            hotkey: WindowsHotkey::from(&settings.dictation_hotkey),
            paste_last_hotkey: settings.paste_last_hotkey.as_ref().map(WindowsHotkey::from),
            voice_action_hotkey: settings
                .voice_action
                .enabled
                .then(|| WindowsHotkey::from(&settings.edit_hotkey)),
            double_tap_lock: settings.double_tap_lock,
            double_tap_only: settings.double_tap_only,
            shortcut_mode: settings.shortcut_mode,
            while_dictating: settings.recording_audio_behavior,
            release_microphone_while_idle: settings.release_microphone_while_idle,
            feedback_volume: if settings.sound_effects {
                (settings.sound_effect_volume.clamp(0.0, 1.0) * 100.0).round() as u8
            } else {
                0
            },
            history: None,
            last_dictation: Arc::new(Mutex::new(None)),
            indicator: None,
            mode_runtime: Arc::new(RwLock::new(
                crate::dictation_modes::ModeRuntime::from_settings(settings),
            )),
            transformations: Arc::new(crate::personal_commands::TransformationClient::default()),
            voice_action_model: Arc::new(RwLock::new(voice_action_model(settings))),
            fallback_to_default_device: true,
            swap_failures: None,
        }
    }
}

/// The voice action model chosen in settings; `None` follows OpenCode's
/// default.
pub fn voice_action_model(
    settings: &crate::app_settings::AppSettings,
) -> Option<crate::dictation_processor::Model> {
    let (provider, id) = settings.voice_action.model.as_deref()?.split_once('/')?;
    Some(crate::dictation_processor::Model {
        provider: provider.into(),
        id: id.into(),
        variant: settings.voice_action.variant.clone(),
    })
}

/// Runs hotkey dictation until `shutdown`. Settings from `updates` apply in
/// place while no dictation records, so changing one never stops listening.
pub fn run(
    event_path: &Path,
    selection: &TranscriptionSelection,
    config: WindowsDictationConfig,
    updates: mpsc::Receiver<Box<crate::app_settings::AppSettings>>,
    shutdown: &AtomicBool,
) -> Result<()> {
    let transcriber = LocalTranscriber::load(selection)?;
    let WindowsDictationConfig {
        device,
        hotkey,
        paste_last_hotkey,
        voice_action_hotkey,
        double_tap_lock,
        double_tap_only,
        shortcut_mode,
        while_dictating,
        release_microphone_while_idle,
        feedback_volume,
        history,
        last_dictation,
        indicator,
        mode_runtime,
        transformations,
        voice_action_model,
        fallback_to_default_device,
        swap_failures,
    } = config;
    let _indicator_guard = IndicatorGuard(indicator.clone());
    crate::feedback::set_enabled(feedback_volume > 0);
    crate::feedback::set_volume(f32::from(feedback_volume) / 100.0);
    if feedback_volume > 0
        && let Err(error) = crate::feedback::preload()
    {
        tracing::warn!(%error, "Windows recording feedback is unavailable");
    }
    let mut release_while_idle = release_microphone_while_idle;
    let mut device = device;
    let mut input = create_input(
        device.as_deref(),
        fallback_to_default_device,
        release_while_idle,
    )?;
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
    let mut toggle_mode = shortcut_mode == crate::dictation::ShortcutMode::Toggle;
    let mut shortcuts = Shortcuts {
        hotkey,
        paste_last_hotkey,
        voice_action_hotkey,
        double_tap_lock,
        double_tap_only,
        shortcut_mode,
    };
    let mut hotkey = shortcuts.start()?;
    let mut while_dictating = while_dictating;
    let mut audio_suppressor =
        crate::windows_audio_control::AudioSuppressor::start(while_dictating);
    let mut model = crate::local_transcriber::ModelSwap::new(selection);
    let mut engine = TranscriptionEngine::start(transcriber)?;
    let mut finishing: Option<Finishing> = None;
    let (jobs, job_receiver) = mpsc::sync_channel::<OutputJob>(2);
    let (result_sender, results) = mpsc::channel();
    let worker = thread::Builder::new()
        .name("windows-dictation-output".into())
        .spawn(move || {
            let mut paster = WindowsPaster::new();
            while let Ok(job) = job_receiver.recv() {
                let result = match job {
                    OutputJob::Dictation(job) => process_dictation_job(
                        job,
                        &mut paster,
                        history.as_ref(),
                        &last_dictation,
                        &mode_runtime,
                        transformations.as_ref(),
                    ),
                    OutputJob::VoiceAction(job) => {
                        process_voice_action_job(job, &mut paster, &voice_action_model)
                    }
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
    // Whether this capture's start tone already played on its press.
    let mut start_tone_played = false;
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
        "Hex is ready on {}. Hold {} to dictate, release to paste; Escape cancels; Ctrl-C stops.",
        device_label, hotkey_label
    );

    while !shutdown.load(Ordering::Relaxed) {
        if let Some(failed) = model.poll() {
            events.dictation(DictationPhase::Failed(failed.error.clone()), "")?;
            if let Some(swap_failures) = &swap_failures {
                let _ = swap_failures.send(failed);
            }
        }
        // Only the newest saved settings matter; they wait for a moment
        // without a recording.
        if let Some(next) = updates.try_iter().last()
            && !recording
            && !start_pending
            && finishing.is_none()
        {
            let next_config = WindowsDictationConfig::from_settings(&next);
            let next_shortcuts = Shortcuts {
                hotkey: next_config.hotkey,
                paste_last_hotkey: next_config.paste_last_hotkey,
                voice_action_hotkey: next_config.voice_action_hotkey,
                double_tap_lock: next_config.double_tap_lock,
                double_tap_only: next_config.double_tap_only,
                shortcut_mode: next_config.shortcut_mode,
            };
            if next_shortcuts != shortcuts {
                // The old hook goes first; one keyboard hook runs at a time.
                drop(hotkey);
                shortcuts = next_shortcuts;
                hotkey = shortcuts.start()?;
                toggle_mode = shortcuts.shortcut_mode == crate::dictation::ShortcutMode::Toggle;
            }
            if next_config.while_dictating != while_dictating {
                while_dictating = next_config.while_dictating;
                audio_suppressor =
                    crate::windows_audio_control::AudioSuppressor::start(while_dictating);
            }
            if next_config.device != device
                || next_config.release_microphone_while_idle != release_while_idle
            {
                drop(input);
                device = next_config.device;
                release_while_idle = next_config.release_microphone_while_idle;
                input = create_input(
                    device.as_deref(),
                    fallback_to_default_device,
                    release_while_idle,
                )?;
                capture = DictationCapture::new(if input.is_open() {
                    input.sample_rate()
                } else {
                    16_000
                });
                if !release_while_idle {
                    captured_through = prime_microphone(&mut input, &mut capture, shutdown)?;
                }
                audio_ready = !release_while_idle;
                device_label = if input.is_open() {
                    input.device_name().to_string()
                } else {
                    device.clone().unwrap_or_else(|| "microphone".into())
                };
                emit_current_state(&mut events, recording, pending, &device_label)?;
            }
            model.want(&next.transcription);
        }
        if !recording
            && !start_pending
            && finishing.is_none()
            && pending == 0
            && let Some(next) = model.take_ready()
        {
            std::mem::replace(&mut engine, TranscriptionEngine::start(next)?).retire();
        }
        if let Ok(error) = hotkey.errors.try_recv() {
            return Err(eyre!("Windows hotkey monitor stopped: {error}"));
        }
        // A release keeps recording through its tail; shortcut events wait,
        // so a press inside the tail starts after this clip is submitted.
        if let Some(release) = &finishing
            && (captured_through >= release.ended_at + POST_RELEASE_TAIL
                || Instant::now() >= release.deadline)
        {
            let Some(Finishing {
                ended_at,
                voice_action,
                ..
            }) = finishing.take()
            else {
                unreachable!("checked above");
            };
            let submitted = submit_capture(
                &mut capture,
                ended_at,
                &engine,
                &jobs,
                &mut events,
                &mut pending,
                voice_action,
            )?;
            if submitted {
                next_job_id += 1;
                job_ids.push_back(next_job_id);
            }
            if let Some(indicator) = &indicator {
                indicator.send(if submitted {
                    crate::windows_rune_hud::WindowsIndicatorEvent::Submitted {
                        job_id: next_job_id,
                    }
                } else {
                    crate::windows_rune_hud::WindowsIndicatorEvent::Discarded
                });
            }
            emit_current_state(&mut events, recording, pending, &device_label)?;
            if release_while_idle {
                input.close();
                audio_ready = false;
            }
        }
        while finishing.is_none()
            && let Ok(event) = hotkey.events.try_recv()
        {
            let occurred_at = event.occurred_at(captured_through);
            match event.action {
                HotkeyAction::Start | HotkeyAction::VoiceStart
                    if !recording && (audio_ready || (release_while_idle && !start_pending)) =>
                {
                    let voice = matches!(event.action, HotkeyAction::VoiceStart);
                    if audio_ready {
                        capture.start_at(occurred_at);
                        recording = true;
                        recording_voice = voice;
                        if voice {
                            recording_feedback_started = false;
                            start_tone_played = false;
                        } else {
                            // A toggle press is deliberate, so it streams at once.
                            recording_feedback_started = toggle_mode;
                            // The tone confirms the press at once; a short tap or a
                            // chord meant for another application retracts it.
                            crate::feedback::play_provisional_start();
                            start_tone_played = true;
                        }
                    } else {
                        // The released microphone opens on demand; recording
                        // begins at the first chunk (no audio pre-roll).
                        start_pending = true;
                        pending_voice = voice;
                        input.request_open();
                        if !voice {
                            crate::feedback::prepare();
                        }
                    }
                    if let Some(suppressor) = &audio_suppressor {
                        suppressor.suppress();
                    }
                    if let Some(indicator) = &indicator {
                        indicator.send(if voice {
                            crate::windows_rune_hud::WindowsIndicatorEvent::EditingStarted
                        } else {
                            crate::windows_rune_hud::WindowsIndicatorEvent::Started
                        });
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
                    let cancelled = matches!(
                        event.action,
                        HotkeyAction::Cancel | HotkeyAction::VoiceCancel
                    );
                    events.dictation(
                        if cancelled {
                            DictationPhase::Cancelled
                        } else {
                            DictationPhase::Discarded
                        },
                        "",
                    )?;
                    if let Some(indicator) = &indicator {
                        indicator.send(if cancelled {
                            crate::windows_rune_hud::WindowsIndicatorEvent::Cancelled
                        } else {
                            crate::windows_rune_hud::WindowsIndicatorEvent::Discarded
                        });
                    }
                    emit_current_state(&mut events, recording, pending, &device_label)?;
                }
                HotkeyAction::Finish | HotkeyAction::VoiceFinish
                    if recording
                        && recording_voice == matches!(event.action, HotkeyAction::VoiceFinish) =>
                {
                    let voice = recording_voice;
                    if let Some(suppressor) = &audio_suppressor {
                        suppressor.restore();
                    }
                    if !recording_feedback_started && capture.become_intentional(occurred_at) {
                        if !start_tone_played {
                            crate::feedback::play(crate::feedback::Tone::DictationStart);
                        }
                        recording_feedback_started = true;
                    }
                    if recording_feedback_started {
                        crate::feedback::play(crate::feedback::Tone::DictationStop);
                    } else if !voice {
                        crate::feedback::retract_start();
                    }
                    recording = false;
                    recording_voice = false;
                    recording_feedback_started = false;
                    finishing = Some(begin_finishing(&capture, occurred_at, voice));
                }
                HotkeyAction::Cancel | HotkeyAction::VoiceCancel
                    if recording
                        && recording_voice == matches!(event.action, HotkeyAction::VoiceCancel) =>
                {
                    if let Some(suppressor) = &audio_suppressor {
                        suppressor.restore();
                    }
                    if !recording_voice {
                        crate::feedback::retract_start();
                    }
                    if recording_feedback_started {
                        crate::feedback::play(crate::feedback::Tone::Cancel);
                    }
                    recording = false;
                    recording_voice = false;
                    recording_feedback_started = false;
                    capture.cancel();
                    if let Some(indicator) = &indicator {
                        indicator.send(crate::windows_rune_hud::WindowsIndicatorEvent::Cancelled);
                    }
                    events.dictation(DictationPhase::Cancelled, "")?;
                    emit_current_state(&mut events, recording, pending, &device_label)?;
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
                    crate::windows_rune_hud::WindowsIndicatorEvent::JobCompleted { job_id }
                } else {
                    crate::windows_rune_hud::WindowsIndicatorEvent::JobFailed { job_id }
                });
            }
            match result.result {
                Ok(()) => {
                    let text = result.text.unwrap_or_default();
                    match result.kind {
                        OutputKind::Dictation => {
                            events.emit(&VoiceEvent::Transcript {
                                timestamp_ms: now_ms(),
                                phase: TranscriptPhase::Completed,
                                latency_ms: result.latency_ms,
                                text: text.clone(),
                            })?;
                            events.dictation(DictationPhase::Pasted, text)?;
                        }
                        OutputKind::VoiceAction => {
                            events.emit(&VoiceEvent::Transcript {
                                timestamp_ms: now_ms(),
                                phase: TranscriptPhase::Completed,
                                latency_ms: result.latency_ms,
                                text: text.clone(),
                            })?;
                            events.dictation(DictationPhase::VoiceAction, text)?;
                        }
                        OutputKind::Repaste => {
                            events.dictation(DictationPhase::Repasted, text)?;
                        }
                    }
                }
                Err(error) => events.dictation(DictationPhase::Failed(error), "")?,
            }
            emit_current_state(&mut events, recording, pending, &device_label)?;
        }

        match input.recv_timeout(UPDATE_INTERVAL, !recording) {
            RecoveringAudioInputEvent::Chunk {
                samples,
                captured_through: chunk_captured_through,
            } => {
                captured_through = chunk_captured_through;
                if !audio_ready {
                    capture = DictationCapture::new(input.sample_rate());
                    capture.ingest_with_pending(&samples, captured_through, None);
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
                        // The released microphone is ready only now.
                        recording_feedback_started = toggle_mode && !recording_voice;
                        start_tone_played = recording_feedback_started;
                        if recording_feedback_started {
                            crate::feedback::play(crate::feedback::Tone::DictationStart);
                        }
                        emit_state(&mut events, VoiceState::Dictating, &device_label)?;
                        continue;
                    }
                    emit_current_state(&mut events, recording, pending, &device_label)?;
                    continue;
                }
                capture.ingest_with_pending(&samples, captured_through, None);
                if recording {
                    if let Some(indicator) = &indicator {
                        indicator.meter(&samples);
                    }
                    if capture.become_intentional(captured_through) && !recording_feedback_started {
                        if !start_tone_played {
                            crate::feedback::play(crate::feedback::Tone::DictationStart);
                        }
                        recording_feedback_started = true;
                    }
                    // Short taps and shortcut chords never reach the model;
                    // the stream starts with the backlog once the hold counts.
                    if recording_feedback_started {
                        capture.begin_stream(&engine);
                    }
                }
            }
            RecoveringAudioInputEvent::Timeout => {}
            RecoveringAudioInputEvent::Interrupted => {
                // A release still in its tail finishes with the audio it has.
                if let Some(release) = &mut finishing {
                    release.deadline = Instant::now();
                }
                audio_ready = false;
                if recording {
                    crate::feedback::retract_start();
                    if recording_feedback_started {
                        crate::feedback::play(crate::feedback::Tone::Cancel);
                    }
                    recording_feedback_started = false;
                    recording = false;
                    recording_voice = false;
                    capture.cancel();
                    events.dictation(DictationPhase::Cancelled, "")?;
                    if let Some(indicator) = &indicator {
                        indicator.send(crate::windows_rune_hud::WindowsIndicatorEvent::Cancelled);
                    }
                }
                events.dictation(
                    DictationPhase::Failed("microphone stream interrupted; reopening".into()),
                    "",
                )?;
                emit_current_state(&mut events, recording, pending, &device_label)?;
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
                    indicator.send(crate::windows_rune_hud::WindowsIndicatorEvent::Failed);
                }
                emit_current_state(&mut events, recording, pending, &device_label)?;
            }
        }
    }

    if recording || finishing.is_some() {
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
    // Accepted jobs still wait on the engine, so it stops after them.
    if worker.join().is_err() {
        return Err(eyre!("Windows dictation worker panicked"));
    }
    drop(engine);
    events.flush()?;
    println!("Stopped.");
    Ok(())
}

#[allow(clippy::too_many_arguments)]
fn process_dictation_job(
    job: Job,
    paster: &mut Result<WindowsPaster>,
    history: Option<&crate::history::History>,
    last_dictation: &Mutex<Option<String>>,
    mode_runtime: &RwLock<crate::dictation_modes::ModeRuntime>,
    transformations: &crate::personal_commands::TransformationClient,
) -> JobResult {
    let started = Instant::now();
    // The context read starts now so the UIA walk overlaps inference.
    let pending_context = crate::windows_context::begin_capture();
    let inference_started = Instant::now();
    let transcription = job.transcript.wait().map(|transcribed| {
        tracing::info!(
            source = transcribed.source.label(),
            finish_ms = transcribed.finish_ms,
            audio_ms = job.audio_ms,
            "transcribed Windows dictation"
        );
        transcribed.text
    });
    let inference_ms = inference_started.elapsed().as_millis() as u64;
    let context = pending_context.finish();
    let application = context.application.clone();
    let result = transcription.and_then(|raw_text| {
        if raw_text.is_empty() {
            return Err("transcription was empty".into());
        }
        let mode_runtime = mode_runtime
            .read()
            .unwrap_or_else(|error| error.into_inner())
            .clone();
        let processed = mode_runtime.process(&raw_text, &context, transformations);
        let text = processed.text.clone();
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
            let draft = crate::dictation_modes::history_draft(
                raw_text,
                &processed,
                application.clone(),
                job.audio_ms,
                inference_ms,
                started.elapsed().as_millis() as u64,
            );
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

/// Voice Action replies may take longer than a dictation rewrite.
const VOICE_ACTION_DEADLINE: Duration = Duration::from_secs(45);

/// Transcribe the held instruction, pair it with the focused application's
/// selection, ask OpenCode, and paste the reply.
fn process_voice_action_job(
    job: Job,
    paster: &mut Result<WindowsPaster>,
    voice_action_model: &RwLock<Option<crate::dictation_processor::Model>>,
) -> JobResult {
    let started = Instant::now();
    // The context read starts now so the UIA walk overlaps inference.
    let pending_context = crate::windows_context::begin_capture();
    let transcription = job.transcript.wait().map(|transcribed| transcribed.text);
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
        let processed = crate::dictation_processor::process_voice_action(
            &instruction,
            selected.as_deref(),
            &context,
            model.as_ref(),
            VOICE_ACTION_DEADLINE,
            &AtomicBool::new(false),
            Instant::now(),
        );
        if let Some(error) = processed
            .observation
            .and_then(|observation| observation.fallback)
        {
            return Err(error);
        }
        let reply = processed.text;
        paster.paste(&reply).map_err(|error| format!("{error:#}"))?;
        Ok(reply)
    });
    let latency_ms = started.elapsed().as_millis().min(u128::from(u32::MAX)) as u32;
    job_result(OutputKind::VoiceAction, result, latency_ms)
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

/// The shortcuts a listener's keyboard hook registers.
#[derive(PartialEq)]
struct Shortcuts {
    hotkey: WindowsHotkey,
    paste_last_hotkey: Option<WindowsHotkey>,
    voice_action_hotkey: Option<WindowsHotkey>,
    double_tap_lock: bool,
    double_tap_only: bool,
    shortcut_mode: crate::dictation::ShortcutMode,
}

impl Shortcuts {
    fn start(&self) -> Result<WindowsHotkeyMonitor> {
        // Double-tap-only depends on the double-tap window; without the lock
        // there is no second tap to wait for.
        WindowsHotkeyMonitor::start(
            self.hotkey.clone(),
            self.paste_last_hotkey.clone(),
            self.voice_action_hotkey.clone(),
            self.double_tap_lock,
            self.double_tap_lock && self.double_tap_only,
            self.shortcut_mode,
        )
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
                capture.ingest_with_pending(&samples, captured_through, None);
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

fn create_input(
    device: Option<&str>,
    fallback_to_default_device: bool,
    release_while_idle: bool,
) -> Result<RecoveringAudioInput> {
    let build = if release_while_idle {
        RecoveringAudioInput::closed
    } else {
        RecoveringAudioInput::open
    };
    if fallback_to_default_device {
        return Ok(build(None, 0, device));
    }
    let exact_name = device.map(resolve_device_name).transpose()?;
    Ok(build(exact_name.as_deref(), 0, None))
}

fn resolve_device_name(query: &str) -> Result<String> {
    let query_lower = query.to_lowercase();
    crate::audio::input_device_names()?
        .into_iter()
        .find(|name| name.to_lowercase().contains(&query_lower))
        .ok_or_else(|| eyre!("microphone is unavailable: {query}"))
}

fn begin_finishing(
    capture: &DictationCapture,
    ended_at: CaptureInstant,
    voice_action: bool,
) -> Finishing {
    let now = Instant::now();
    Finishing {
        ended_at,
        voice_action,
        // A short tap is discarded on its release alone and needs no tail.
        deadline: if capture.would_transcribe(ended_at) {
            now + POST_RELEASE_TAIL + TAIL_STALL_GRACE
        } else {
            now
        },
    }
}

#[allow(clippy::too_many_arguments)]
fn submit_capture(
    capture: &mut DictationCapture,
    ended_at: CaptureInstant,
    engine: &TranscriptionEngine,
    jobs: &SyncSender<OutputJob>,
    events: &mut EventLog,
    pending: &mut usize,
    voice_action: bool,
) -> Result<bool> {
    let mut submitted = false;
    match capture.finish_with_tail(ended_at, POST_RELEASE_TAIL) {
        Finish::Discard => events.dictation(DictationPhase::Discarded, "")?,
        Finish::Transcribe(clip) if clip.is_near_silent() => {
            events.dictation(DictationPhase::Discarded, "")?
        }
        Finish::Transcribe(mut clip) => {
            let audio_ms = clip.duration_ms();
            let job = Job {
                transcript: engine.finish(clip.take_stream(), clip.into_transcription_samples())?,
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

fn emit_current_state(
    events: &mut EventLog,
    recording: bool,
    pending: usize,
    device: &str,
) -> Result<()> {
    let state = if recording {
        VoiceState::Dictating
    } else if pending > 0 {
        VoiceState::Transcribing
    } else {
        VoiceState::Listening
    };
    emit_state(events, state, device)
}

fn emit_state(events: &mut EventLog, state: VoiceState, device: &str) -> Result<()> {
    events.emit(&VoiceEvent::State {
        timestamp_ms: now_ms(),
        state,
        device: device.into(),
    })?;
    Ok(())
}

struct IndicatorGuard(Option<crate::windows_rune_hud::WindowsIndicatorSender>);

impl Drop for IndicatorGuard {
    fn drop(&mut self) {
        if let Some(indicator) = &self.0 {
            indicator.send(crate::windows_rune_hud::WindowsIndicatorEvent::Reset);
        }
    }
}
