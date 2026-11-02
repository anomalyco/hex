use std::path::Path;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{self, SyncSender, TrySendError};
use std::sync::{Arc, RwLock};
use std::thread;
use std::time::{Duration, Instant};

use color_eyre::Result;
use color_eyre::eyre::eyre;

use crate::app_settings::AppSettings;
use crate::audio::{CaptureInstant, RecoveringAudioInput, RecoveringAudioInputEvent};
use crate::control::{PrintReply, PrintedTranscript};
use crate::desktop::events::{
    DictationPhase, EventLog, TranscriptPhase, VoiceEvent, VoiceState, now_ms,
};
use crate::dictation::{DictationCapture, Finish, POST_RELEASE_TAIL};
use crate::feedback::{self, Tone};
use crate::linux::input::{
    CommandQueue, HotkeyEvent, HotkeyOptions, HotkeyStatus, LinuxHotkey, LinuxHotkeyMonitor,
};
use crate::linux::paste::LinuxPaster;
use crate::linux::rune_hud::LinuxIndicator;
use crate::transcription::engine::{PendingTranscript, TranscriptionEngine};
use crate::transcription::local_transcriber::LocalTranscriber;

const UPDATE_INTERVAL: Duration = Duration::from_millis(20);
/// How long past the tail a release waits for audio that never arrives.
const TAIL_STALL_GRACE: Duration = Duration::from_millis(100);

struct Job {
    transcript: PendingTranscript,
    audio_ms: u64,
    /// `hex dictation stop --print`: the text goes here, never pasted.
    print: Option<PrintReply>,
    /// Dictate without processing: paste the transcript as heard.
    raw: bool,
}

enum OutputJob {
    Dictation(Job),
    /// Pastes the newest completed dictation again.
    PasteLast,
}

#[derive(Clone, Copy, PartialEq)]
enum OutputKind {
    Dictation,
    /// A dictation printed by `hex dictation stop --print`.
    Printed,
    PasteLast,
}

/// A release whose capture is still collecting its post-release tail.
struct Finishing {
    ended_at: CaptureInstant,
    print: Option<PrintReply>,
    raw: bool,
    released: Instant,
    deadline: Instant,
}

struct OutputWorker {
    stop: Arc<AtomicBool>,
    worker: Option<thread::JoinHandle<()>>,
}

impl Drop for OutputWorker {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Release);
        if let Some(worker) = self.worker.take() {
            let _ = worker.join();
        }
    }
}

/// What the host hands a running listener. Settings apply in place while no
/// dictation records, so changing one never stops listening.
pub(crate) enum ListenerUpdate {
    Settings(Box<AppSettings>),
    /// A model the host prepared, swapped in once no transcription is pending.
    Transcriber {
        selection: crate::transcription::models::TranscriptionSelection,
        transcriber: Box<LocalTranscriber>,
    },
    /// True while the window records a shortcut: global matching stops.
    SuspendShortcuts(bool),
}

/// The shortcuts a listener registers for `settings`.
fn shortcuts(settings: &AppSettings) -> (LinuxHotkey, HotkeyOptions) {
    let options = HotkeyOptions {
        double_tap: settings.double_tap_lock,
        // Double-tap-only needs the double-tap window and a non-modifier key.
        double_tap_only: settings.double_tap_lock
            && settings.double_tap_only
            && settings.dictation_hotkey.key.is_some(),
        toggle: settings.shortcut_mode == crate::dictation::ShortcutMode::Toggle,
        paste_last: settings.paste_last_hotkey.as_ref().map(LinuxHotkey::from),
        raw_dictation: settings
            .raw_dictation_hotkey
            .as_ref()
            .map(LinuxHotkey::from),
        capture: settings
            .capture
            .enabled
            .then(|| LinuxHotkey::from(&settings.capture.hotkey)),
    };
    (LinuxHotkey::from(&settings.dictation_hotkey), options)
}

/// Opens `--device`, else the microphone `settings` select, else the system
/// default, or keeps it released until a dictation starts.
fn microphone_input(settings: &AppSettings, device: Option<&str>) -> RecoveringAudioInput {
    let microphone = settings.microphone.as_deref();
    if settings.release_microphone_while_idle {
        RecoveringAudioInput::closed(device, 0, microphone)
    } else {
        RecoveringAudioInput::open(device, 0, microphone)
    }
}

/// What happens to a transcript before and after it is pasted: the modes
/// that process it and the history that keeps it. The host shares it
/// with the listener so saved settings apply without a restart.
#[derive(Clone)]
pub(crate) struct DictationOutput {
    pub(crate) history: Option<crate::history::History>,
    pub(crate) modes: Arc<RwLock<crate::dictation::modes::ModeRuntime>>,
    pub(crate) transformations: Arc<crate::personal_commands::TransformationClient>,
}

impl DictationOutput {
    pub(crate) fn from_settings(
        settings: &crate::app_settings::AppSettings,
        history: Option<crate::history::History>,
    ) -> Self {
        Self {
            history,
            modes: Arc::new(RwLock::new(
                crate::dictation::modes::ModeRuntime::from_settings(settings),
            )),
            transformations: Arc::new(crate::personal_commands::TransformationClient::default()),
        }
    }

    /// Processes (unless `raw`) and pastes one transcript, or only
    /// processes it when there is no paster (a printing stop), then records
    /// it.
    fn deliver(
        &self,
        raw_text: String,
        raw: bool,
        paster: Option<&mut LinuxPaster>,
        audio_ms: u64,
        inference_ms: u64,
        started: Instant,
    ) -> Result<String> {
        let context = crate::linux::context::capture();
        let processed = if raw {
            crate::dictation::processor::Processed::unchanged(raw_text.trim())
        } else {
            let modes = self
                .modes
                .read()
                .unwrap_or_else(|error| error.into_inner())
                .clone();
            modes.process(&raw_text, &context, &self.transformations)
        };
        if processed.text.trim().is_empty() {
            return Err(eyre!("text replacements produced empty output"));
        }
        let printed = paster.is_none();
        if let Some(paster) = paster {
            paster.paste(&processed.text)?;
        }
        if let Some(history) = &self.history {
            let mut draft = crate::dictation::modes::history_draft(
                raw_text,
                &processed,
                context.application,
                audio_ms,
                inference_ms,
                started.elapsed().as_millis() as u64,
            );
            if printed {
                draft.kind = crate::history::HistoryKind::Printed;
            } else if raw {
                draft.kind = crate::history::HistoryKind::Raw;
            }
            if let Err(error) = history.record(draft) {
                tracing::warn!(%error, "could not retain Linux dictation history");
            }
        }
        Ok(processed.text)
    }
}

/// Runs hotkey dictation until `shutdown`. A prepared transcriber skips the
/// model load; otherwise the supplied transcription selection is loaded here.
/// `updates` change settings, the model, and shortcut registration in place.
#[allow(clippy::too_many_arguments)]
pub(crate) fn run(
    event_path: &Path,
    device: Option<&str>,
    shutdown: &AtomicBool,
    settings: &AppSettings,
    output_settings: DictationOutput,
    transcriber: Option<LocalTranscriber>,
    hotkey_status: HotkeyStatus,
    commands: CommandQueue,
    updates: mpsc::Receiver<ListenerUpdate>,
) -> Result<()> {
    let mut settings = settings.clone();
    let transcriber = transcriber.map_or_else(
        || {
            LocalTranscriber::load(&crate::transcription::custom_models::runnable(
                &settings.transcription,
            ))
        },
        Ok,
    )?;
    crate::transcription::speech_detector::prewarm();
    // The engine outlives the output worker, which waits on its transcripts.
    // Captures transcribe on it too, holding it weakly.
    let mut engine = Arc::new(TranscriptionEngine::start(transcriber)?);
    // The local API transcribes host audio on this engine's model.
    engine.share();
    // Drop audio/hotkeys and close the job queue before joining slow output on
    // every exit, including failures during startup or recording.
    let mut output = OutputWorker {
        stop: Arc::new(AtomicBool::new(false)),
        worker: None,
    };
    // A missing microphone falls back to the default one. A released
    // microphone opens only while a dictation records.
    let mut input = microphone_input(&settings, device);
    let mut release_while_idle = settings.release_microphone_while_idle;
    let (binding, options) = shortcuts(&settings);
    let hotkey_label = binding.label();
    let mut toggle = options.toggle;
    let start_shortcuts = |(binding, options): (LinuxHotkey, HotkeyOptions)| {
        LinuxHotkeyMonitor::start(binding, options, hotkey_status.clone(), commands.clone())
    };
    let mut hotkey = Some(start_shortcuts((binding, options))?);
    let mut shortcuts_suspended = false;
    let mut indicator = LinuxIndicator::new(settings.indicator_position);
    let mut suppressor =
        crate::linux::audio_control::AudioSuppressor::dictation(settings.recording_audio_behavior);
    let paste_with_shift = Arc::new(AtomicBool::new(settings.paste_with_shift));
    let worker_paste_with_shift = paste_with_shift.clone();
    let paste_keys = hotkey_status.keys();
    let (jobs, job_receiver) = mpsc::sync_channel::<OutputJob>(2);
    let (result_sender, results) = mpsc::channel();
    let worker_stop = output.stop.clone();
    output.worker = Some(thread::spawn(move || {
        let mut paster = LinuxPaster::new(worker_stop.clone(), worker_paste_with_shift, paste_keys);
        let mut last_dictation: Option<String> = None;
        while let Ok(job) = job_receiver.recv() {
            if worker_stop.load(Ordering::Acquire) {
                break;
            }
            let (mut job, kind) = match job {
                OutputJob::Dictation(job) if job.print.is_some() => (job, OutputKind::Printed),
                OutputJob::Dictation(job) => (job, OutputKind::Dictation),
                OutputJob::PasteLast => {
                    let result = last_dictation
                        .clone()
                        .ok_or_else(|| eyre!("there is no completed dictation to paste"))
                        .and_then(|text| {
                            paster
                                .as_mut()
                                .map_err(|error| eyre!("{error:#}"))?
                                .paste(&text)
                        })
                        .map(|()| String::new())
                        .map_err(|error| format!("{error:#}"));
                    if result_sender.send((OutputKind::PasteLast, result)).is_err() {
                        break;
                    }
                    continue;
                }
            };
            let started = Instant::now();
            let result = (|| -> Result<String> {
                let transcribed = job.transcript.wait().map_err(|error| eyre!(error))?;
                let inference_ms = started.elapsed().as_millis() as u64;
                tracing::info!(
                    source = transcribed.source.label(),
                    finish_ms = transcribed.finish_ms,
                    audio_ms = job.audio_ms,
                    "transcribed Linux dictation"
                );
                let text = transcribed.text;
                if text.is_empty() {
                    if let Some(print) = job.print.take() {
                        print.send(PrintedTranscript::NoSpeech);
                    }
                    return Err(eyre!("transcription was empty"));
                }
                if kind == OutputKind::Printed {
                    return output_settings.deliver(
                        text,
                        job.raw,
                        None,
                        job.audio_ms,
                        inference_ms,
                        started,
                    );
                }
                let paster = paster.as_mut().map_err(|error| eyre!("{error:#}"))?;
                output_settings.deliver(
                    text,
                    job.raw,
                    Some(paster),
                    job.audio_ms,
                    inference_ms,
                    started,
                )
            })()
            .map_err(|error| format!("{error:#}"));
            if let Some(print) = job.print.take() {
                print.send(match &result {
                    Ok(text) => PrintedTranscript::Text(text.clone()),
                    Err(error) => PrintedTranscript::Failed(error.clone()),
                });
            }
            tracing::info!(
                audio_ms = job.audio_ms,
                elapsed_ms = started.elapsed().as_millis(),
                "completed Linux dictation job"
            );
            if let Ok(text) = &result
                && kind == OutputKind::Dictation
            {
                last_dictation = Some(text.clone());
            }
            if result_sender.send((kind, result)).is_err() {
                break;
            }
        }
    }));

    // The `capture` experiment: its host, the audio it copies from this
    // listener's microphone, and the capsule's commands.
    let timeline = crate::capture_host::ListenerTimeline::default();
    let environment = crate::capture_host::Environment;
    let mut capture_host =
        crate::capture_host::CaptureHost::start(Some(engine.clone()), &environment);
    let capture_controls = crate::capture_host::listen_for_controls();
    // The microphone stays open for a capture even while released when idle.
    let mut capture_holds_microphone = false;
    // The draw shortcut started a recording: drawing turns on once its
    // engine takes input.
    let mut draw_pending = false;

    let mut events = EventLog::create(event_path)?;
    let mut audio_ready = input.is_open();
    // A released microphone's sample rate is unknown until it opens; the
    // first chunk replaces this placeholder capture.
    let mut capture = DictationCapture::new(if audio_ready {
        input.sample_rate()
    } else {
        16_000
    });
    let released_label = |settings: &AppSettings| {
        device
            .or(settings.microphone.as_deref())
            .unwrap_or("System default")
            .to_string()
    };
    let mut device_label = if audio_ready {
        input.device_name().to_string()
    } else {
        released_label(&settings)
    };
    // Updates wait for a moment without a recording.
    let mut next_settings: Option<AppSettings> = None;
    let mut model =
        crate::transcription::local_transcriber::ModelSwap::new(&settings.transcription);
    let mut next_suspended: Option<bool> = None;
    // A press that arrived while the released microphone was opening;
    // recording starts at its first chunk.
    let mut start_pending = false;
    let mut captured_through = CaptureInstant::ZERO;
    let mut finishing: Option<Finishing> = None;
    // The recording skips processing (Dictate without processing).
    let mut recording_raw = false;
    let mut pending = 0_usize;
    events.emit(&VoiceEvent::SessionStarted {
        timestamp_ms: now_ms(),
    })?;
    emit_state(&mut events, VoiceState::Listening, &device_label)?;
    println!(
        "Hex dictation is ready on {device_label}. Hold {hotkey_label} to dictate; Ctrl-C stops."
    );

    while !shutdown.load(Ordering::Relaxed) {
        // No microphone could be opened (none connected, or the default
        // went away): record why while it keeps retrying in the background.
        if let Some(error) = input.take_failure() {
            events.dictation(
                DictationPhase::Failed(format!("could not open the microphone: {error}")),
                "",
            )?;
        }
        for update in updates.try_iter() {
            match update {
                ListenerUpdate::Settings(next) => next_settings = Some(*next),
                ListenerUpdate::Transcriber {
                    selection,
                    transcriber,
                } => model.offer(&selection, *transcriber),
                ListenerUpdate::SuspendShortcuts(suspended) => next_suspended = Some(suspended),
            }
        }
        if let Some(failed) = model.poll() {
            fail(&events, failed.error)?;
        }
        if !capture.is_recording() && !start_pending && finishing.is_none() {
            let mut register = next_suspended.take().is_some_and(|suspended| {
                std::mem::replace(&mut shortcuts_suspended, suspended) != suspended
            });
            if let Some(next) = next_settings.take() {
                register |= shortcuts(&next) != shortcuts(&settings);
                model.want(&next.transcription);
                toggle = next.shortcut_mode == crate::dictation::ShortcutMode::Toggle;
                paste_with_shift.store(next.paste_with_shift, Ordering::Relaxed);
                if next.indicator_position != settings.indicator_position {
                    indicator = LinuxIndicator::new(next.indicator_position);
                }
                if next.recording_audio_behavior != settings.recording_audio_behavior {
                    suppressor = crate::linux::audio_control::AudioSuppressor::dictation(
                        next.recording_audio_behavior,
                    );
                }
                if device.is_none() && next.microphone != settings.microphone
                    || next.release_microphone_while_idle != release_while_idle
                {
                    drop(input);
                    input = microphone_input(&next, device);
                    release_while_idle = next.release_microphone_while_idle;
                    audio_ready = input.is_open();
                    if audio_ready {
                        capture = DictationCapture::new(input.sample_rate());
                        device_label = input.device_name().to_string();
                    } else {
                        device_label = released_label(&next);
                    }
                    emit_state(&mut events, active_state(false, pending), &device_label)?;
                }
                settings = next;
            }
            if register {
                // The old registration goes first: X11 grabs the same keys again.
                drop(hotkey.take());
                if !shortcuts_suspended {
                    hotkey = Some(start_shortcuts(shortcuts(&settings))?);
                }
            }
            if pending == 0
                && let Some(next) = model.take_ready()
            {
                let old =
                    std::mem::replace(&mut engine, Arc::new(TranscriptionEngine::start(next)?));
                engine.share();
                crate::capture_host::use_engine(&engine);
                // A capture still transcribing holds the old engine a moment.
                if let Ok(old) = Arc::try_unwrap(old) {
                    old.retire();
                }
            }
        }
        crate::capture_host::CaptureHost::reconcile(
            &mut capture_host,
            Some(&engine),
            &environment,
            &timeline,
        );
        if draw_pending && crate::capture_host::wants_input() {
            draw_pending = false;
            crate::capture_host::set_pen(true);
        } else if draw_pending
            && capture_host
                .as_mut()
                .is_none_or(|host| host.recording_since(&timeline).is_none())
        {
            // The recording never started.
            draw_pending = false;
        }
        while let Ok(control) = capture_controls.try_recv() {
            let reply = match (&mut capture_host, control) {
                (Some(host), crate::capture_host::CaptureControl::Draw) => {
                    draw_pending = host.draw_shortcut(&timeline);
                    crate::control::ControlReply::done("drawing")
                }
                (Some(host), crate::capture_host::CaptureControl::Scrawl(action)) => {
                    host.control(action, &timeline)
                }
                (Some(host), crate::capture_host::CaptureControl::Command(command)) => {
                    host.command(command, &timeline)
                }
                (host, crate::capture_host::CaptureControl::Reply(action, answer)) => {
                    let report = match host {
                        Some(host) => host.control(action, &timeline),
                        None => crate::control::ControlReply::error(
                            "feature-off",
                            "Annotate is off; turn it on with `hex settings set capture.enabled on`",
                        ),
                    };
                    let _ = answer.send(report);
                    continue;
                }
                (None, _) => crate::control::ControlReply::error("feature-off", "captures are off"),
            };
            if let crate::control::ControlReply::Error { message, .. } = reply {
                tracing::warn!(message, "a capture command did nothing");
            }
        }
        // A capture copies this microphone: a released one opens for it,
        // and closes again once nothing records.
        if release_while_idle {
            let wants = timeline.wants_audio();
            if wants && !input.is_active() {
                input.request_open();
            }
            if wants {
                capture_holds_microphone = true;
            } else if capture_holds_microphone
                && !capture.is_recording()
                && !start_pending
                && finishing.is_none()
            {
                capture_holds_microphone = false;
                input.close();
                audio_ready = false;
            }
        }
        if let Some(hotkey) = &hotkey
            && let Ok(error) = hotkey.errors.try_recv()
        {
            return Err(eyre!("Linux hotkey monitor stopped: {error}"));
        }
        // A release keeps recording through its tail; shortcut events wait,
        // so a press inside the tail starts after this clip is submitted.
        if let Some(release) = &finishing
            && (captured_through >= release.ended_at + POST_RELEASE_TAIL
                || Instant::now() >= release.deadline)
        {
            tracing::debug!(
                tail_ms = release.released.elapsed().as_millis(),
                "post-release tail captured"
            );
            let Finishing {
                ended_at,
                print,
                raw,
                ..
            } = finishing.take().expect("a release is finishing");
            submit_capture(
                &mut capture,
                ended_at,
                &engine,
                &jobs,
                &mut events,
                &mut pending,
                print,
                raw,
            )?;
            suppressor.restore();
            if release_while_idle && !timeline.wants_audio() {
                input.close();
                audio_ready = false;
            }
            emit_state(
                &mut events,
                active_state(capture.is_recording(), pending),
                &device_label,
            )?;
        }
        while finishing.is_none()
            && let Some(action) = hotkey
                .as_ref()
                .and_then(|hotkey| hotkey.events.try_recv().ok())
        {
            // A printing stop finishes as a stop does; its reply follows.
            let (action, mut print) = match action {
                HotkeyEvent::FinishPrinting(reply) => (HotkeyEvent::Finish, Some(reply)),
                action => (action, None),
            };
            match action {
                // Annotate records the narration itself: while it records,
                // dictation starts nothing. The desktop already took the keys,
                // so nothing reaches the app.
                HotkeyEvent::Start { .. }
                    if capture_host
                        .as_mut()
                        .is_some_and(|host| host.recording_since(&timeline).is_some()) => {}
                HotkeyEvent::Start { raw } if !capture.is_recording() && !start_pending => {
                    recording_raw = raw;
                    if audio_ready {
                        start_capture(&mut capture, captured_through, || {
                            feedback::play_provisional_start();
                        });
                        stream_when_due(&mut capture, &engine, toggle);
                    } else if release_while_idle {
                        start_pending = true;
                        input.request_open();
                        feedback::prepare();
                    } else {
                        continue;
                    }
                    events.dictation(DictationPhase::Started, "")?;
                    emit_state(&mut events, VoiceState::Dictating, &device_label)?;
                }
                HotkeyEvent::Finish | HotkeyEvent::Cancel if start_pending => {
                    // Released before the opening microphone produced audio:
                    // nothing was captured.
                    start_pending = false;
                    if let Some(print) = print.take() {
                        print.send(PrintedTranscript::NoSpeech);
                    }
                    suppressor.restore();
                    if !timeline.wants_audio() {
                        input.close();
                        audio_ready = false;
                    }
                    events.dictation(
                        if action == HotkeyEvent::Cancel {
                            DictationPhase::Cancelled
                        } else {
                            DictationPhase::Discarded
                        },
                        "",
                    )?;
                    emit_state(&mut events, active_state(false, pending), &device_label)?;
                }
                HotkeyEvent::Finish if capture.is_recording() => {
                    finishing = Some(Finishing {
                        raw: recording_raw,
                        ..begin_finishing(&capture, captured_through, print.take(), || {
                            suppressor.restore_then(|| feedback::play(Tone::DictationStop))
                        })
                    });
                }
                // One key: a tap starts and stops; held while recording, it
                // draws until it comes up.
                HotkeyEvent::Capture => {
                    if let Some(host) = &mut capture_host {
                        host.annotate_down(&timeline);
                    }
                }
                HotkeyEvent::CaptureUp => {
                    if let Some(host) = &mut capture_host {
                        host.annotate_up(&timeline);
                    }
                }
                HotkeyEvent::PasteLast if !capture.is_recording() => {
                    match jobs.try_send(OutputJob::PasteLast) {
                        Ok(()) | Err(TrySendError::Full(_)) => {}
                        Err(TrySendError::Disconnected(_)) => {
                            return Err(eyre!("dictation worker is unavailable"));
                        }
                    }
                }
                HotkeyEvent::Cancel if capture.is_recording() => {
                    capture.cancel();
                    feedback::retract_start();
                    suppressor.restore();
                    if release_while_idle && !timeline.wants_audio() {
                        input.close();
                        audio_ready = false;
                    }
                    events.dictation(DictationPhase::Cancelled, "")?;
                    emit_state(
                        &mut events,
                        active_state(capture.is_recording(), pending),
                        &device_label,
                    )?;
                }
                _ => {}
            }
        }
        while let Ok((kind, result)) = results.try_recv() {
            let phase = if kind == OutputKind::Printed {
                DictationPhase::Printed
            } else {
                DictationPhase::Pasted
            };
            // A repeated paste has no transcript and is not pending work.
            if kind == OutputKind::PasteLast {
                if let Err(error) = result {
                    fail(&events, error)?;
                }
                continue;
            }
            pending = pending.saturating_sub(1);
            indicator.finished(result.is_ok());
            match result {
                Ok(text) => {
                    feedback::play(Tone::DictationDone);
                    events.emit(&VoiceEvent::Transcript {
                        timestamp_ms: now_ms(),
                        phase: TranscriptPhase::Completed,
                        latency_ms: 0,
                        text: text.clone(),
                    })?;
                    events.dictation(phase, text)?;
                }
                Err(error) => fail(&events, error)?,
            }
            emit_state(
                &mut events,
                active_state(capture.is_recording(), pending),
                &device_label,
            )?;
        }

        indicator.update(capture.is_recording() || start_pending, pending);
        match input.recv_timeout(
            UPDATE_INTERVAL,
            crate::audio::capture_idle(
                capture.is_recording() || timeline.wants_audio(),
                start_pending,
                finishing.is_some(),
            ),
        ) {
            RecoveringAudioInputEvent::Chunk {
                samples,
                captured_through: chunk_captured_through,
            } => {
                captured_through = chunk_captured_through;
                if !audio_ready {
                    capture = DictationCapture::new(input.sample_rate());
                    audio_ready = true;
                    device_label = input.device_name().to_string();
                    if start_pending {
                        // The on-demand open finished while the shortcut is
                        // held: recording starts here, without pre-roll.
                        start_pending = false;
                        start_capture(&mut capture, captured_through, || {
                            feedback::play_provisional_start();
                        });
                    }
                }
                crate::audio::input_level::record(&samples);
                timeline.feed(&samples, input.sample_rate(), captured_through);
                capture.ingest(&samples, captured_through);
                if capture.is_recording() {
                    indicator.meter(&samples);
                }
                if capture.become_intentional(captured_through) && finishing.is_none() {
                    // Ordinary shortcut chords and short taps never mute or pause.
                    suppressor.suppress();
                }
                stream_when_due(&mut capture, &engine, toggle);
                capture.report_live_text();
            }
            RecoveringAudioInputEvent::Timeout => {}
            RecoveringAudioInputEvent::Interrupted => {
                timeline.restart();
                // A release still in its tail finishes with the audio it has.
                if let Some(release) = &mut finishing {
                    release.deadline = Instant::now();
                }
                audio_ready = false;
                if capture.is_recording() && finishing.is_none() {
                    capture.cancel();
                    feedback::retract_start();
                    suppressor.restore();
                    events.dictation(DictationPhase::Cancelled, "")?;
                }
                events.dictation(
                    DictationPhase::Failed("microphone stream interrupted; reopening".into()),
                    "",
                )?;
                emit_state(
                    &mut events,
                    active_state(capture.is_recording(), pending),
                    &device_label,
                )?;
            }
            RecoveringAudioInputEvent::Reopened => {
                timeline.restart();
                audio_ready = false;
                device_label = input.device_name().to_string();
                // A press waiting on the released microphone is already
                // Dictating; its first chunk starts the capture.
                emit_state(
                    &mut events,
                    active_state(capture.is_recording() || start_pending, pending),
                    &device_label,
                )?;
            }
            RecoveringAudioInputEvent::OpenFailed(error) => {
                if !release_while_idle {
                    return Err(eyre!("could not open the microphone: {error}"));
                }
                // An on-demand open failed (device busy or unplugged): report
                // it and stay released rather than stopping the listener.
                start_pending = false;
                suppressor.restore();
                fail(&events, format!("could not open microphone: {error}"))?;
                emit_state(&mut events, active_state(false, pending), &device_label)?;
            }
        }
    }

    if capture.is_recording() {
        capture.cancel();
        events.dictation(DictationPhase::Cancelled, "")?;
    }
    if let Some(mut host) = capture_host.take() {
        host.shutdown(&timeline);
    }
    indicator.update(false, pending);
    suppressor.restore();
    emit_state(&mut events, VoiceState::Stopping, &device_label)?;
    output.stop.store(true, Ordering::Release);
    drop(hotkey);
    drop(input);
    drop(jobs);
    if output
        .worker
        .take()
        .expect("dictation output worker exists")
        .join()
        .is_err()
    {
        return Err(eyre!("Linux dictation worker panicked"));
    }
    println!("Stopped.");
    Ok(())
}

/// Applies the saved sound settings and preloads the tones.
pub(crate) fn initialize_feedback(settings: &crate::app_settings::AppSettings) {
    feedback::set_enabled(settings.sound_effects);
    feedback::set_volume(settings.sound_effect_volume.clamp(0.0, 1.0));
    feedback::set_sound_set(settings.sound_theme);
    if let Err(error) = feedback::preload() {
        tracing::warn!(%error, "recording sounds are unavailable; continuing without feedback");
    }
}

fn start_capture(capture: &mut DictationCapture, started_at: CaptureInstant, play: impl FnOnce()) {
    capture.start(started_at);
    play();
}

/// Streams once the hold counts, so short taps and chords never reach the
/// model; a Toggle press is deliberate and streams at once.
fn stream_when_due(capture: &mut DictationCapture, engine: &TranscriptionEngine, toggle: bool) {
    if capture.is_recording() && (toggle || capture.is_intentional()) {
        capture.begin_stream(engine);
    }
}

/// Starts collecting a release's tail. A release that will be transcribed
/// is announced at once rather than after its tail; a short tap is discarded
/// on its release alone, needs no tail, and stays silent.
///
/// The release restores any muted output before playing the stop tone so it is
/// heard while the tail is still captured. The silence gate ignores the tail,
/// so the tone itself never makes a silent clip transcribe.
fn begin_finishing(
    capture: &DictationCapture,
    ended_at: CaptureInstant,
    print: Option<PrintReply>,
    announce: impl FnOnce(),
) -> Finishing {
    let now = Instant::now();
    let transcribes = capture.would_transcribe(ended_at);
    if transcribes {
        announce();
    }
    Finishing {
        ended_at,
        print,
        raw: false,
        released: now,
        deadline: if transcribes {
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
    print: Option<PrintReply>,
    raw: bool,
) -> Result<()> {
    let no_speech = |print: Option<PrintReply>| {
        if let Some(print) = print {
            print.send(PrintedTranscript::NoSpeech);
        }
    };
    match capture.finish_with_tail(ended_at, POST_RELEASE_TAIL) {
        Finish::Discard => {
            // A short tap or chord withdraws the start tone of its press.
            feedback::retract_start();
            no_speech(print);
            events.dictation(DictationPhase::Discarded, "")?
        }
        Finish::Transcribe(clip) if clip.is_near_silent() => {
            no_speech(print);
            events.dictation(DictationPhase::Discarded, "")?
        }
        Finish::Transcribe(mut clip) => {
            let audio_ms = clip.duration_ms();
            let job = OutputJob::Dictation(Job {
                transcript: engine.finish(clip.take_stream(), clip.into_transcription_samples())?,
                audio_ms,
                print,
                raw,
            });
            match jobs.try_send(job) {
                Ok(()) => {
                    *pending += 1;
                    events.dictation(DictationPhase::Transcribing, "")?;
                }
                Err(TrySendError::Full(_)) => fail(events, "dictation queue is full")?,
                Err(TrySendError::Disconnected(_)) => {
                    return Err(eyre!("dictation worker is unavailable"));
                }
            }
        }
    }
    Ok(())
}

fn active_state(recording: bool, pending: usize) -> VoiceState {
    if recording {
        VoiceState::Dictating
    } else if pending > 0 {
        VoiceState::Transcribing
    } else {
        VoiceState::Listening
    }
}

/// Logs a failed dictation or paste with the sound set's error sound.
fn fail(events: &EventLog, error: impl Into<String>) -> Result<()> {
    feedback::play(Tone::Error);
    events.dictation(DictationPhase::Failed(error.into()), "")?;
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::transcription::engine::Command;

    fn capture_time() -> CaptureInstant {
        CaptureInstant::from_nanos(60_000_000_000)
    }

    fn event_log() -> (EventLog, std::path::PathBuf) {
        let path = std::env::temp_dir().join(format!(
            "hex-linux-dictation-{}-{}.ndjson",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        (EventLog::create(&path).unwrap(), path)
    }

    /// Records `seconds` of speech-level audio in 100 ms chunks, streaming
    /// as the live loop would.
    fn record(
        capture: &mut DictationCapture,
        engine: &TranscriptionEngine,
        toggle: bool,
        seconds: u64,
    ) -> CaptureInstant {
        let start = capture_time();
        start_capture(capture, start, || {});
        stream_when_due(capture, engine, toggle);
        let mut through = start;
        for _ in 0..seconds * 10 {
            through = through + Duration::from_millis(100);
            let tone = (0..1_600)
                .map(|index| 0.1 * (index as f32 * 0.05).sin())
                .collect::<Vec<_>>();
            capture.ingest(&tone, through);
            capture.become_intentional(through);
            stream_when_due(capture, engine, toggle);
        }
        through
    }

    #[test]
    fn start_sound_precedes_audio_and_does_not_change_short_tap_discard() {
        let mut capture = DictationCapture::new(16_000);
        let started_at = capture_time();
        let mut sounds = Vec::new();

        start_capture(&mut capture, started_at, || {
            sounds.push(Tone::DictationStart)
        });

        assert!(capture.is_recording());
        assert_eq!(sounds, [Tone::DictationStart]);
        assert!(matches!(
            capture.finish(started_at + Duration::from_millis(100)),
            Finish::Discard
        ));
    }

    #[test]
    fn hold_streams_after_the_threshold_and_toggle_streams_from_the_press() {
        for toggle in [false, true] {
            let (engine, commands) = TranscriptionEngine::for_test();
            let mut capture = DictationCapture::new(16_000);
            let start = capture_time();
            start_capture(&mut capture, start, || {});
            stream_when_due(&mut capture, &engine, toggle);
            assert_eq!(capture.is_streaming(), toggle);
            capture.ingest(&[0.1; 1_600], start + Duration::from_millis(100));
            capture.become_intentional(start + Duration::from_millis(100));
            stream_when_due(&mut capture, &engine, toggle);
            assert_eq!(capture.is_streaming(), toggle, "toggle={toggle}");
            capture.ingest(&[0.1; 3_200], start + MINIMUM_HOLD);
            capture.become_intentional(start + MINIMUM_HOLD);
            stream_when_due(&mut capture, &engine, toggle);
            assert!(capture.is_streaming());
            let begun = commands
                .try_iter()
                .filter(|command| matches!(command, Command::Begin { .. }))
                .count();
            assert_eq!(begun, 1);
        }
    }

    const MINIMUM_HOLD: Duration = crate::dictation::MINIMUM_HOLD_DURATION;

    #[test]
    fn a_release_finishes_its_stream_with_the_complete_clip() {
        let (engine, commands) = TranscriptionEngine::for_test();
        let (mut events, path) = event_log();
        let (jobs, queued) = mpsc::sync_channel(2);
        let mut capture = DictationCapture::new(16_000);
        let released = record(&mut capture, &engine, false, 2);
        let mut pending = 0;
        // The release is announced at once, then waits for its tail before
        // it is submitted.
        let mut sounds = Vec::new();
        let finishing = begin_finishing(
            &capture,
            released.checked_sub(POST_RELEASE_TAIL).unwrap(),
            None,
            || sounds.push(Tone::DictationStop),
        );
        assert_eq!(sounds, [Tone::DictationStop]);
        assert!(finishing.deadline > Instant::now());
        submit_capture(
            &mut capture,
            finishing.ended_at,
            &engine,
            &jobs,
            &mut events,
            &mut pending,
            None,
            false,
        )
        .unwrap();
        assert_eq!(pending, 1);
        let Ok(OutputJob::Dictation(job)) = queued.try_recv() else {
            panic!("a dictation job");
        };
        assert_eq!(job.audio_ms, 2_000);
        let received = commands.try_iter().collect::<Vec<_>>();
        let fed: usize = received
            .iter()
            .map(|command| match command {
                Command::Feed { samples, .. } => samples.len(),
                _ => 0,
            })
            .sum();
        assert!(matches!(
            received.first(),
            Some(Command::Begin { stream: 1, .. })
        ));
        assert!(matches!(
            received.last(),
            Some(Command::Finish { stream: Some(1), clip, .. }) if clip.len() == 32_000
        ));
        assert!(fed > 16_000 && fed <= 32_000);
        assert!(
            !received
                .iter()
                .any(|command| matches!(command, Command::Cancel { .. }))
        );
        drop(events);
        let _ = std::fs::remove_file(path);
    }

    #[test]
    fn escape_and_short_taps_cancel_the_stream_and_submit_nothing() {
        let (engine, commands) = TranscriptionEngine::for_test();
        let (mut events, path) = event_log();
        let (jobs, queued) = mpsc::sync_channel(2);
        let mut pending = 0;

        let mut cancelled = DictationCapture::new(16_000);
        record(&mut cancelled, &engine, true, 1);
        cancelled.cancel();

        let mut short = DictationCapture::new(16_000);
        let start = capture_time();
        start_capture(&mut short, start, || {});
        stream_when_due(&mut short, &engine, true);
        short.ingest(&[0.1; 1_600], start + Duration::from_millis(100));
        // A printing stop of a short tap prints nothing and says why.
        let (print, printed) = PrintReply::channel();
        let finishing = begin_finishing(
            &short,
            start + Duration::from_millis(100),
            Some(print),
            || panic!("a short tap plays no stop tone"),
        );
        assert!(
            finishing.deadline <= Instant::now(),
            "a short tap needs no tail"
        );
        submit_capture(
            &mut short,
            finishing.ended_at,
            &engine,
            &jobs,
            &mut events,
            &mut pending,
            finishing.print,
            false,
        )
        .unwrap();

        assert_eq!(printed.try_recv().unwrap(), PrintedTranscript::NoSpeech);
        assert_eq!(pending, 0);
        assert!(queued.try_recv().is_err());
        let received = commands.try_iter().collect::<Vec<_>>();
        let count = |matches: fn(&Command) -> bool| received.iter().filter(|c| matches(c)).count();
        assert_eq!(count(|c| matches!(c, Command::Begin { .. })), 2);
        assert_eq!(count(|c| matches!(c, Command::Cancel { .. })), 2);
        assert_eq!(count(|c| matches!(c, Command::Finish { .. })), 0);
        drop(events);
        let _ = std::fs::remove_file(path);
    }

    #[test]
    fn a_model_that_cannot_stream_transcribes_the_whole_clip_after_release() {
        let (engine, commands) = TranscriptionEngine::for_test_without_streaming();
        let (mut events, path) = event_log();
        let (jobs, queued) = mpsc::sync_channel(2);
        let mut capture = DictationCapture::new(16_000);
        let released = record(&mut capture, &engine, true, 2);
        assert!(!capture.is_streaming());
        let mut pending = 0;
        submit_capture(
            &mut capture,
            released.checked_sub(POST_RELEASE_TAIL).unwrap(),
            &engine,
            &jobs,
            &mut events,
            &mut pending,
            None,
            false,
        )
        .unwrap();
        assert!(queued.try_recv().is_ok());
        assert!(matches!(
            commands.try_iter().collect::<Vec<_>>().as_slice(),
            [Command::Finish { stream: None, clip, .. }] if clip.len() == 32_000
        ));
        drop(events);
        let _ = std::fs::remove_file(path);
    }

    #[test]
    fn cancelled_capture_keeps_pending_output_visible() {
        assert_eq!(active_state(true, 2), VoiceState::Dictating);
        assert_eq!(active_state(false, 2), VoiceState::Transcribing);
        assert_eq!(active_state(false, 1), VoiceState::Transcribing);
        assert_eq!(active_state(false, 0), VoiceState::Listening);
        assert_eq!(active_state(true, 0), VoiceState::Dictating);
    }

    #[test]
    fn output_guard_cancels_and_joins_even_on_listener_error() {
        let stopped = Arc::new(AtomicBool::new(false));
        let finished = Arc::new(AtomicBool::new(false));
        let result: Result<()> = {
            let mut output = OutputWorker {
                stop: stopped.clone(),
                worker: None,
            };
            let (_jobs, receiver) = mpsc::channel::<()>();
            let stop = stopped.clone();
            let completed = finished.clone();
            output.worker = Some(thread::spawn(move || {
                // The sender must close before the guard joins this worker.
                assert!(receiver.recv().is_err());
                while !stop.load(Ordering::Acquire) {
                    thread::yield_now();
                }
                completed.store(true, Ordering::Release);
            }));
            Err(eyre!("listener failure"))
        };
        assert!(result.is_err());
        assert!(stopped.load(Ordering::Acquire));
        assert!(finished.load(Ordering::Acquire));
    }
}
