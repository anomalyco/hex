use std::path::Path;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{self, SyncSender, TrySendError};
use std::sync::{Arc, RwLock};
use std::thread;
use std::time::{Duration, Instant};

use color_eyre::Result;
use color_eyre::eyre::eyre;

use crate::app_settings::AppSettings;
use crate::audio::linger::MicrophoneLinger;
use crate::audio::{CaptureInstant, RecoveringAudioInput, RecoveringAudioInputEvent};
use crate::control::{PrintReply, PrintedTranscript};
use crate::desktop::events::{
    DictationPhase, EventLog, TranscriptPhase, VoiceEvent, VoiceState, now_ms,
};
use crate::desktop::host::DesktopShortcuts;
use crate::dictation::service::{DictationQueue, Effect, MICROPHONE_LOST, ServiceDictations};
use crate::dictation::{DictationCapture, Finish, ReleaseWait};
use crate::feedback::{self, Tone};
use crate::linux::input::{
    CommandQueue, HotkeyEvent, HotkeyOptions, HotkeyStatus, LinuxHotkey, LinuxHotkeyMonitor,
};
use crate::linux::paste::LinuxPaster;
use crate::linux::rune_hud::LinuxIndicator;
use crate::transcription::engine::{PendingTranscript, TranscriptionEngine};
use crate::transcription::local_transcriber::LocalTranscriber;

const UPDATE_INTERVAL: Duration = Duration::from_millis(20);

mod gap;
mod shortcuts;
mod silence;
mod typing;

use gap::{ChunkStep, chunk_step};
use shortcuts::shortcuts;
use silence::reopen_silent_stream;
use typing::CaptureTyping;

struct Job {
    transcript: PendingTranscript,
    audio_ms: u64,
    /// The language the model was asked for at submission.
    language: String,
    /// The clip, kept for History while Labs › Keep recordings is on.
    recording: Option<crate::history::Recording>,
    /// `hex dictation stop --print`: the text goes here, never pasted.
    print: Option<PrintReply>,
    /// The shortcut that started this capture: dictation, Dictate without
    /// processing, or Labs › Terminal paste.
    flavour: crate::dictation::DictationFlavour,
    /// Labs › Live typing: the capture's input-method session, so its final
    /// text is committed instead of pasted. Dropping an unfinished job
    /// cancels it.
    typing: Option<crate::linux::live_typing::Session>,
}

enum OutputJob {
    Dictation(Job),
    /// Pastes the newest completed dictation again.
    PasteLast,
    /// Runs the focused field's text through its Output steps and replaces
    /// it (Labs › Transform typed text).
    TypedText,
}

#[derive(Clone, Copy, PartialEq)]
enum OutputKind {
    Dictation,
    /// A dictation printed by `hex dictation stop --print`.
    Printed,
    PasteLast,
    TypedText,
}

/// A release whose capture is still collecting its post-release tail.
struct Finishing {
    release: ReleaseWait,
    print: Option<PrintReply>,
    flavour: crate::dictation::DictationFlavour,
    released: Instant,
}

/// Where the microphone stream's clock stands between chunks: the newest
/// chunk's end plus the time since it arrived. PipeWire and ALSA can deliver
/// large chunks (#1309), so the newest audio may trail a press by most of
/// one; shortcut presses and releases are placed here instead.
#[derive(Default)]
struct StreamClock {
    newest: Option<(CaptureInstant, Instant)>,
}

impl StreamClock {
    /// A stalled stream's lag is not counted past this.
    const MAX_LAG: Duration = Duration::from_secs(2);

    fn chunk(&mut self, captured_through: CaptureInstant, received: Instant) {
        self.newest = Some((captured_through, received));
    }

    /// A replaced or reopened stream keeps its own clock: nothing of the old
    /// one carries over.
    fn reset(&mut self) {
        self.newest = None;
    }

    /// The end of the newest chunk.
    fn through(&self) -> CaptureInstant {
        self.newest
            .map_or(CaptureInstant::ZERO, |(through, _)| through)
    }

    /// The stream time at `now`; `None` before this stream's first chunk.
    fn at(&self, now: Instant) -> Option<CaptureInstant> {
        let (through, received) = self.newest?;
        Some(through + now.saturating_duration_since(received).min(Self::MAX_LAG))
    }
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

    /// Reuses the mode runtime and transformation client the local API's
    /// `process=global` reads, so both see the same settings and personal
    /// commands host.
    pub(crate) fn sharing(mut self, processing: crate::dictation::modes::ProcessingState) -> Self {
        self.modes = processing.modes;
        self.transformations = processing.transformations;
        self
    }

    /// Processes (unless raw) and pastes one transcript, or only
    /// processes it when there is no paster (a printing stop), then records
    /// it.
    #[allow(clippy::too_many_arguments)]
    fn deliver(
        &self,
        raw_text: String,
        flavour: crate::dictation::DictationFlavour,
        language: &str,
        paster: Option<&mut LinuxPaster>,
        audio_ms: u64,
        inference_ms: u64,
        started: Instant,
        recording: Option<crate::history::Recording>,
        typing: Option<crate::linux::live_typing::Session>,
    ) -> Result<String> {
        let modes = self
            .modes
            .read()
            .unwrap_or_else(|error| error.into_inner())
            .clone();
        let context = crate::linux::context::capture(!flavour.raw() && modes.reads_page());
        let processed = if flavour.raw() {
            crate::dictation::processor::Processed::unchanged(raw_text.trim())
        } else {
            modes.process(
                &raw_text,
                language,
                &context,
                &self.transformations,
                crate::dictation::processor::Source::Dictation,
            )
        };
        let printed = paster.is_none();
        // A voice prompt that deliberately asked nothing ends quietly: no
        // failure, no paste, no last dictation, and no History. A live
        // preedit for the capture is discarded rather than committed empty.
        if processed.nothing {
            if let Some(session) = typing {
                session.cancel();
            }
            return Ok(processed.text);
        }
        // Live typing inserts the final text through the input method; a
        // printed or copy-only dictation clears its preedit and takes the
        // usual path instead.
        let inserted = match typing {
            Some(session) if !printed && !crate::labs::copy_only() => {
                session.finish(&processed.text)
            }
            Some(session) => {
                session.cancel();
                false
            }
            None => false,
        };
        if processed.text.trim().is_empty() {
            return Err(eyre!("text replacements produced empty output"));
        }
        if let Some(paster) = paster
            && !inserted
        {
            paster.paste(&processed.text, flavour.terminal_paste())?;
        }
        if let Some(history) = &self.history {
            let mut draft = crate::dictation::modes::history_draft(
                raw_text,
                &processed,
                context.application,
                audio_ms,
                inference_ms,
                started.elapsed().as_millis() as u64,
                recording,
            );
            if printed {
                draft.kind = crate::history::HistoryKind::Printed;
            } else if flavour.raw() {
                draft.kind = crate::history::HistoryKind::Raw;
            }
            if let Err(error) = history.record(draft) {
                tracing::warn!(%error, "could not retain Linux dictation history");
            }
        }
        Ok(processed.text)
    }
}

/// The line a listener prints once its shortcuts are settled: what actually
/// starts dictation from here. `commands` is whether `hex` commands reach
/// this listener.
fn readiness_message(
    device: &str,
    hotkey: &str,
    shortcuts: &DesktopShortcuts,
    commands: bool,
) -> String {
    let ready = format!("Hex dictation is ready on {device}.");
    match shortcuts {
        DesktopShortcuts::Registered | DesktopShortcuts::Portal(_) => {
            format!("{ready} Hold {hotkey} to dictate; Ctrl-C stops.")
        }
        // A listener sets its status before it reports ready, so no running
        // listener prints this.
        DesktopShortcuts::Unregistered => format!("{ready} Ctrl-C stops."),
        DesktopShortcuts::Commands if commands => format!(
            "{ready} No shortcut is bound: bind `hex dictation start` and `hex dictation stop` \
             (or `hex dictation toggle`) in your desktop's keybindings; Ctrl-C stops."
        ),
        DesktopShortcuts::Commands => format!(
            "{ready} No shortcut is bound and this foreground listener can't receive `hex` \
             commands: run `hex start` and bind `hex dictation toggle` in your desktop's \
             keybindings; Ctrl-C stops."
        ),
    }
}

/// Runs hotkey dictation until `shutdown`. A prepared transcriber skips the
/// model load; otherwise the supplied transcription selection is loaded here,
/// or parked until the first dictation with Labs › Unload model when idle.
/// `updates` change settings, the model, and shortcut registration in place.
/// `dictations` records a local API client's dictation beside the shortcut:
/// one capture at a time, and neither ends the other.
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
    dictations: DictationQueue,
    updates: mpsc::Receiver<ListenerUpdate>,
) -> Result<()> {
    let mut settings = settings.clone();
    // Labs › Unload model when idle: a launch with no model Settings prepared
    // for this listener starts parked, as after an idle unload, until the
    // first dictation. A prepared model starts loaded; the idle timer unloads
    // it.
    let transcriber = match transcriber {
        Some(transcriber) => Some(transcriber),
        None if crate::labs::unload_idle().is_some() => None,
        None => Some(LocalTranscriber::load(
            &crate::transcription::custom_models::runnable(&settings.transcription),
        )?),
    };
    crate::transcription::speech_detector::prewarm();
    // The engine outlives the output worker, which waits on its transcripts.
    // Captures transcribe on it too, holding it weakly.
    let mut engine = match transcriber {
        Some(transcriber) => Arc::new(TranscriptionEngine::start(transcriber)?),
        None => Arc::new(TranscriptionEngine::start_parked(
            crate::transcription::custom_models::runnable(&settings.transcription),
        )?),
    };
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
    // Labs › Keep mic open after dictating: a finished capture's
    // microphone stays open for its window before release.
    let mut linger = MicrophoneLinger::default();
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
                                .paste(&text, false)
                        })
                        .map(|()| String::new())
                        .map_err(|error| format!("{error:#}"));
                    if result_sender.send((OutputKind::PasteLast, result)).is_err() {
                        break;
                    }
                    continue;
                }
                OutputJob::TypedText => {
                    let result = paster
                        .as_mut()
                        .map_err(|error| eyre!("{error:#}"))
                        .and_then(|paster| {
                            crate::linux::typed_text::run_job(
                                paster,
                                &output_settings.modes,
                                &output_settings.transformations,
                                &worker_stop,
                            )
                        })
                        .map(|()| String::new())
                        .map_err(|error| format!("{error:#}"));
                    if result_sender.send((OutputKind::TypedText, result)).is_err() {
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
                        job.flavour,
                        &job.language,
                        None,
                        job.audio_ms,
                        inference_ms,
                        started,
                        job.recording.take(),
                        job.typing.take(),
                    );
                }
                let paster = paster.as_mut().map_err(|error| eyre!("{error:#}"))?;
                output_settings.deliver(
                    text,
                    job.flavour,
                    &job.language,
                    Some(paster),
                    job.audio_ms,
                    inference_ms,
                    started,
                    job.recording.take(),
                    job.typing.take(),
                )
            })()
            .map_err(|error| format!("{error:#}"));
            if let Some(print) = job.print.take() {
                print.send(match &result {
                    Ok(text) if text.trim().is_empty() => PrintedTranscript::NoSpeech,
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
                && !text.trim().is_empty()
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
    // Labs › Live typing: one owner, started at the first hotkey capture on
    // Wayland, and one session per capture.
    let wayland = crate::linux::session::LinuxSession::detect().is_wayland();
    let mut live_typing: Option<Arc<crate::linux::live_typing::LiveTyping>> = None;
    let mut active_typing: Option<CaptureTyping> = None;
    let mut next_capture_id = 0_u64;
    // Updates wait for a moment without a recording.
    let mut next_settings: Option<AppSettings> = None;
    let mut model =
        crate::transcription::local_transcriber::ModelSwap::new(&settings.transcription);
    let mut next_suspended: Option<bool> = None;
    // A press that arrived while the microphone was opening, or before its
    // stream's first chunk; recording starts at that chunk.
    let mut start_pending = false;
    let mut clock = StreamClock::default();
    // The end of the last chunk on this stream's timeline; reset whenever
    // the stream is closed, replaced, or reopened.
    let mut last_captured_through: Option<CaptureInstant> = None;
    // A discontinuity ended the capture a release was finishing: its audio
    // belongs to the old capture, so replace that once it is submitted. The
    // rate is the one in effect when the gap arrived.
    let mut restart_capture: Option<u32> = None;
    let mut finishing: Option<Finishing> = None;
    // The flavour of the shortcut that started the recording (Dictate
    // without processing, or Labs › Terminal paste).
    let mut recording_flavour = crate::dictation::DictationFlavour::Normal;
    let mut pending = 0_usize;
    // A local API client's dictation, and its transcripts as they finish.
    let mut service = ServiceDictations::default();
    let (service_results, service_finished) = mpsc::channel::<(u64, Result<String, String>)>();
    events.emit(&VoiceEvent::SessionStarted {
        timestamp_ms: now_ms(),
    })?;
    emit_state(&mut events, VoiceState::Listening, &device_label)?;
    println!(
        "{}",
        readiness_message(
            &device_label,
            &hotkey_label,
            &hotkey_status.shortcuts(),
            commands.connected(),
        )
    );

    // Labs › Keep mic open after dictating: the one release check the pass and
    // its event sites share, so a press handled in the same pass still sees a
    // closed microphone at once when the lab is off.
    macro_rules! release_microphone {
        () => {
            if release_while_idle
                && input.is_active()
                && linger.may_close(
                    Instant::now(),
                    capture.is_recording()
                        || start_pending
                        || finishing.is_some()
                        || service.is_active()
                        || timeline.wants_audio(),
                )
            {
                input.close();
                last_captured_through = None;
                audio_ready = false;
            }
        };
    }

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
        if !capture.is_recording() && !start_pending && finishing.is_none() && !service.is_active()
        {
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
                    clock.reset();
                    last_captured_through = None;
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
            // A GPU without room for two copies: the engine swaps in place.
            if pending == 0
                && let Some(selection) = model.take_reload()
                && let Err(error) = engine.reload(selection)
            {
                tracing::warn!(%error, "could not replace the transcription model");
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
        // A capture copies this microphone: a released one opens for it, and
        // the one release check below closes it once nothing records.
        if release_while_idle && timeline.wants_audio() && !input.is_active() {
            input.request_open();
        }
        if let Some(hotkey) = &hotkey
            && let Ok(error) = hotkey.errors.try_recv()
        {
            return Err(eyre!("Linux hotkey monitor stopped: {error}"));
        }
        // A release keeps recording through its tail; shortcut events wait,
        // so a press inside the tail starts after this clip is submitted.
        if let Some(finish) = &finishing
            && let Some(tail) = finish.release.tail(&capture, Instant::now())
        {
            tracing::debug!(
                waited_ms = finish.released.elapsed().as_millis(),
                tail_ms = tail.as_millis(),
                "post-release tail captured"
            );
            let Finishing {
                release,
                print,
                flavour,
                ..
            } = finishing.take().expect("a release is finishing");
            let silent = submit_capture(
                &mut capture,
                release.ended_at(),
                tail,
                &engine,
                &jobs,
                &mut events,
                &mut pending,
                print,
                flavour,
                &mut active_typing,
            )?;
            if silent
                && reopen_silent_stream(
                    &mut input,
                    release_while_idle,
                    &mut linger,
                    crate::audio::linger::window(),
                )
            {
                // Recovery blocks audio until a reopened stream arrives, so
                // a press must wait for it rather than start a dead capture.
                // A lingering microphone closed instead, and opens fresh.
                timeline.restart();
                clock.reset();
                last_captured_through = None;
                audio_ready = false;
            }
            if let Some(sample_rate) = restart_capture.take() {
                capture = DictationCapture::new(sample_rate);
            }
            suppressor.restore();
            // The capture's end releases the microphone here, as before, when
            // the lab is off.
            release_microphone!();
            emit_state(
                &mut events,
                active_state(capture.is_recording(), pending),
                &device_label,
            )?;
        }
        // `/dictations`: a client's dictation never ends a shortcut capture,
        // and only its owner (or its lapsed lease) ends it.
        let now = Instant::now();
        let mut service_ended = service.expire(now);
        for control in dictations.drain() {
            let busy = capture.is_recording()
                || start_pending
                || finishing.is_some()
                || capture_host
                    .as_mut()
                    .is_some_and(|host| host.recording_since(&timeline).is_some());
            match service.control(control, busy, now) {
                Effect::None => {}
                Effect::Start if audio_ready && input.is_open() && clock.at(now).is_some() => {
                    start_service_capture(
                        &mut capture,
                        &mut service,
                        &suppressor,
                        clock.through(),
                        input.sample_rate(),
                    );
                    service_ended |= !service.is_active();
                }
                // Recording starts at the first chunk. A failed stream keeps
                // its backoff, but the client's start asks for it now.
                Effect::Start => {
                    input.retry_now();
                    if release_while_idle && !input.is_active() {
                        input.request_open();
                    }
                }
                Effect::Finish(id) => {
                    // The client's finish is its exact end: no tail.
                    match capture.finish_with_tail(clock.through(), Duration::ZERO) {
                        Finish::Discard => service.complete(id, Err("capture-discarded".into())),
                        Finish::Transcribe(mut clip) => {
                            service.transcribing(id, clip.duration_ms());
                            match engine
                                .finish(clip.take_stream(), clip.into_transcription_samples())
                            {
                                Ok(transcript) => {
                                    let results = service_results.clone();
                                    thread::spawn(move || {
                                        let result =
                                            transcript.wait().map(|transcribed| transcribed.text);
                                        let _ = results.send((id, result));
                                    });
                                }
                                Err(error) => service.complete(id, Err(format!("{error:#}"))),
                            }
                        }
                    }
                    service_ended = true;
                }
                Effect::Cancel => service_ended = true,
            }
        }
        if service_ended && !service.is_active() {
            capture.cancel();
            suppressor.restore();
            // A client's dictation ended here before too.
            release_microphone!();
        }
        for (id, result) in service_finished.try_iter() {
            service.complete(id, result);
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
                // A client's dictation records: the shortcut and `hex
                // dictation` neither start nor end anything.
                HotkeyEvent::Start { .. } | HotkeyEvent::Finish | HotkeyEvent::Cancel
                    if service.is_active() =>
                {
                    if let Some(print) = print.take() {
                        print.send(PrintedTranscript::NotRecording);
                    }
                }
                // Annotate records the narration itself: while it records,
                // dictation starts nothing. The desktop already took the keys,
                // so nothing reaches the app.
                HotkeyEvent::Start { .. }
                    if capture_host
                        .as_mut()
                        .is_some_and(|host| host.recording_since(&timeline).is_some()) => {}
                HotkeyEvent::Start { flavour } if !capture.is_recording() && !start_pending => {
                    // A failed stream keeps its backoff; the press asks now.
                    if input.is_recovering() {
                        input.retry_now();
                    }
                    recording_flavour = flavour;
                    // A short tap or chord must never create an input method;
                    // Live typing starts at the hold threshold (or the Toggle
                    // press), before the stream opens so it can demand text.
                    cancel_typing(&mut active_typing);
                    let pressed_at = clock.at(Instant::now()).filter(|_| audio_ready);
                    if let Some(pressed_at) = pressed_at {
                        let pre_roll =
                            crate::dictation::hotkey_pre_roll(settings.recording_audio_behavior);
                        start_capture(&mut capture, pressed_at, pre_roll, || {
                            feedback::play_provisional_start();
                        });
                        begin_live_typing(
                            &capture,
                            toggle,
                            wayland,
                            &mut live_typing,
                            &mut next_capture_id,
                            &mut active_typing,
                        );
                        stream_when_due(&mut capture, &engine, toggle);
                    } else if audio_ready || release_while_idle {
                        start_pending = true;
                        if !audio_ready {
                            input.request_open();
                        }
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
                    cancel_typing(&mut active_typing);
                    if let Some(print) = print.take() {
                        print.send(PrintedTranscript::NoSpeech);
                    }
                    suppressor.restore();
                    release_microphone!();
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
                    let released_at = clock.at(Instant::now()).unwrap_or(CaptureInstant::ZERO);
                    finishing = Some(Finishing {
                        flavour: recording_flavour,
                        ..begin_finishing(&capture, released_at, print.take(), || {
                            suppressor.restore_then(feedback::deferred_stop())
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
                HotkeyEvent::TypedText
                    if !capture.is_recording()
                        && !start_pending
                        && finishing.is_none()
                        && !service.is_active() =>
                {
                    match jobs.try_send(OutputJob::TypedText) {
                        Ok(()) | Err(TrySendError::Full(_)) => {}
                        Err(TrySendError::Disconnected(_)) => {
                            return Err(eyre!("dictation worker is unavailable"));
                        }
                    }
                }
                HotkeyEvent::Cancel if capture.is_recording() => {
                    capture.cancel();
                    cancel_typing(&mut active_typing);
                    feedback::retract_start();
                    feedback::play(Tone::Cancel);
                    suppressor.restore();
                    release_microphone!();
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
            // Typed text never touches History, Paste Last, or pending work.
            if kind == OutputKind::TypedText {
                if let Err(error) = result {
                    fail(&events, error)?;
                }
                continue;
            }
            pending = pending.saturating_sub(1);
            match result {
                // A voice prompt that asked nothing ends as a discard: no
                // done tone and no empty pasted transcript. The HUD closes
                // the submitted job as cancelled in `update` below.
                Ok(text) if text.is_empty() => {
                    indicator.discarded();
                    events.discarded_voice_prompt()?;
                }
                Ok(text) => {
                    indicator.finished(None);
                    feedback::play(Tone::DictationDone);
                    events.emit(&VoiceEvent::Transcript {
                        timestamp_ms: now_ms(),
                        phase: TranscriptPhase::Completed,
                        latency_ms: 0,
                        text: text.clone(),
                    })?;
                    events.dictation(phase, text)?;
                }
                Err(error) => {
                    indicator.finished(Some(&error));
                    fail(&events, error)?;
                }
            }
            emit_state(
                &mut events,
                active_state(capture.is_recording(), pending),
                &device_label,
            )?;
        }

        // The released microphone closes once nothing records, no client
        // dictates, no release tail runs, no capture copies it, and Labs ›
        // Keep mic open after dictating's window has passed. This also
        // releases one a stream interruption left open, which the old
        // latch-based sites never did.
        release_microphone!();

        indicator.update(capture.is_recording() || start_pending, pending);
        match input.recv_timeout(
            UPDATE_INTERVAL,
            crate::audio::capture_idle(
                capture.is_recording() || timeline.wants_audio(),
                start_pending || service.is_starting(),
                finishing.is_some(),
            ),
        ) {
            RecoveringAudioInputEvent::Chunk {
                samples,
                captured_through,
            } => {
                let sample_rate = input.sample_rate();
                let step = chunk_step(
                    last_captured_through,
                    samples.len(),
                    captured_through,
                    sample_rate,
                );
                if let ChunkStep::Discontinuity(gap) = step {
                    // A gap past the stall watchdog cannot be bridged: end
                    // any live capture and start the audio after it fresh.
                    let cancelled =
                        service.is_active() || (capture.is_recording() && finishing.is_none());
                    tracing::warn!(
                        gap_ms = gap.as_millis() as u64,
                        recording = capture.is_recording(),
                        cancelled,
                        "microphone audio skipped; {}",
                        if cancelled {
                            "discarded the capture"
                        } else {
                            "restarted the capture timeline"
                        }
                    );
                    clock.reset();
                    // A release still in its tail finishes with the audio it
                    // has, from the old capture.
                    if let Some(finish) = &mut finishing {
                        finish.release.stop_waiting();
                    }
                    if service.is_active() {
                        capture.cancel();
                        suppressor.restore();
                        service.lost(MICROPHONE_LOST);
                    } else if capture.is_recording() && finishing.is_none() {
                        capture.cancel();
                        cancel_typing(&mut active_typing);
                        feedback::retract_start();
                        suppressor.restore();
                        events.dictation(DictationPhase::Cancelled, "")?;
                    }
                    if cancelled {
                        events.dictation(
                            DictationPhase::Failed(
                                "microphone audio skipped; the capture was discarded".into(),
                            ),
                            "",
                        )?;
                        emit_state(
                            &mut events,
                            active_state(capture.is_recording(), pending),
                            &device_label,
                        )?;
                    }
                    last_captured_through = Some(captured_through);
                    if finishing.is_some() {
                        // The release submits the old capture next loop;
                        // replace it before any later audio arrives.
                        restart_capture = Some(sample_rate);
                        continue; // This chunk belongs to neither capture.
                    }
                    capture = DictationCapture::new(sample_rate);
                }
                clock.chunk(captured_through, Instant::now());
                if !audio_ready {
                    capture = DictationCapture::new(sample_rate);
                    audio_ready = true;
                    device_label = input.device_name().to_string();
                }
                if start_pending {
                    // The stream's first chunk arrived while the shortcut is
                    // held: recording starts here, without pre-roll.
                    start_pending = false;
                    start_capture(&mut capture, captured_through, Duration::ZERO, || {
                        feedback::play_provisional_start();
                    });
                }
                if service.is_starting() && !capture.is_recording() {
                    start_service_capture(
                        &mut capture,
                        &mut service,
                        &suppressor,
                        captured_through,
                        sample_rate,
                    );
                }
                let mut padded = Vec::new();
                if let ChunkStep::Pad(frames) = step {
                    // A hiccup (often a Bluetooth stream starting) is filled
                    // with silence so the capture keeps going.
                    padded.resize(frames, 0.0);
                    padded.extend_from_slice(&samples);
                    tracing::warn!(
                        gap_ms = (frames as f64 * 1_000.0 / f64::from(sample_rate)) as u64,
                        recording = capture.is_recording(),
                        "microphone audio skipped; filled the gap with silence"
                    );
                }
                let captured: &[f32] = if padded.is_empty() { &samples } else { &padded };
                crate::audio::input_level::record(captured);
                // The sink turns timestamp gaps into its own Gap markers, so
                // it gets the real chunk rather than the padded one.
                timeline.feed(&samples, sample_rate, captured_through);
                capture.ingest(captured, captured_through);
                last_captured_through = Some(captured_through);
                if capture.is_recording() {
                    indicator.meter(captured);
                    service.audio(captured, Instant::now());
                }
                if capture.become_intentional(captured_through) && finishing.is_none() {
                    // Ordinary shortcut chords and short taps never mute,
                    // lower, or pause audio.
                    suppressor.suppress();
                }
                // A release in its tail never starts an input method: a
                // short tap must stay inert even while its last audio lands.
                // A client's dictation returns its text and types nothing.
                if finishing.is_none() && !service.is_active() {
                    begin_live_typing(
                        &capture,
                        toggle,
                        wayland,
                        &mut live_typing,
                        &mut next_capture_id,
                        &mut active_typing,
                    );
                }
                stream_when_due(&mut capture, &engine, toggle);
                if let Some(partial) = capture.report_live_text() {
                    if let Some(typing) = &active_typing {
                        typing.session.preedit(&partial.stable, &partial.tentative);
                    }
                    // A client's dictation is the only capture while it records.
                    service.partial(&partial);
                }
            }
            RecoveringAudioInputEvent::Timeout => {}
            RecoveringAudioInputEvent::Interrupted => {
                timeline.restart();
                clock.reset();
                last_captured_through = None;
                // A release still in its tail finishes with the audio it has.
                if let Some(finish) = &mut finishing {
                    finish.release.stop_waiting();
                }
                audio_ready = false;
                if service.is_active() {
                    capture.cancel();
                    suppressor.restore();
                    service.lost(MICROPHONE_LOST);
                } else if capture.is_recording() && finishing.is_none() {
                    capture.cancel();
                    cancel_typing(&mut active_typing);
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
                clock.reset();
                last_captured_through = None;
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
                cancel_typing(&mut active_typing);
                service.lost(MICROPHONE_LOST);
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
    cancel_typing(&mut active_typing);
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

fn start_capture(
    capture: &mut DictationCapture,
    pressed_at: CaptureInstant,
    pre_roll: Duration,
    play: impl FnOnce(),
) {
    capture.start_at_press(pressed_at, pre_roll);
    play();
}

/// Starts a client's dictation, which counts at once (no hold threshold),
/// and answers its start. A client that already gave up gets no recording.
fn start_service_capture(
    capture: &mut DictationCapture,
    service: &mut ServiceDictations,
    suppressor: &crate::linux::audio_control::AudioSuppressor,
    started_at: CaptureInstant,
    sample_rate: u32,
) {
    capture.start_programmatic_at(started_at);
    if service.started(sample_rate) {
        suppressor.suppress();
    } else {
        capture.cancel();
    }
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
    let release = ReleaseWait::new(capture, ended_at);
    if release.waits() {
        announce();
    }
    Finishing {
        release,
        print,
        flavour: crate::dictation::DictationFlavour::Normal,
        released: Instant::now(),
    }
}

#[allow(clippy::too_many_arguments)]
fn submit_capture(
    capture: &mut DictationCapture,
    ended_at: CaptureInstant,
    tail: Duration,
    engine: &TranscriptionEngine,
    jobs: &SyncSender<OutputJob>,
    events: &mut EventLog,
    pending: &mut usize,
    print: Option<PrintReply>,
    flavour: crate::dictation::DictationFlavour,
    typing: &mut Option<CaptureTyping>,
) -> Result<bool> {
    // A discarded capture drops this guard without starting or ending a
    // session; a transcribed one moves it into the job.
    let typing = typing.take();
    let no_speech = |print: Option<PrintReply>| {
        if let Some(print) = print {
            print.send(PrintedTranscript::NoSpeech);
        }
    };
    match capture.finish_with_tail(ended_at, tail) {
        Finish::Discard => {
            // A short tap or chord withdraws the start tone of its press.
            feedback::retract_start();
            no_speech(print);
            events.dictation(DictationPhase::Discarded, "")?
        }
        Finish::Transcribe(clip) if clip.is_near_silent() => {
            let silent = clip.is_digital_silence();
            no_speech(print);
            events.dictation(DictationPhase::Discarded, "")?;
            return Ok(silent);
        }
        Finish::Transcribe(mut clip) => {
            let audio_ms = clip.duration_ms();
            let stream = clip.take_stream();
            let samples = clip.into_transcription_samples();
            let language = engine
                .held_selection()
                .unwrap_or_else(|| crate::app_settings::transcription_selection().1)
                .language;
            let recording = crate::history::Recording::keep(&samples);
            let job = OutputJob::Dictation(Job {
                transcript: engine.finish(stream, samples)?,
                audio_ms,
                language,
                recording,
                print,
                flavour,
                typing: typing.map(|CaptureTyping { session, .. }| session),
            });
            match jobs.try_send(job) {
                Ok(()) => {
                    *pending += 1;
                    events.dictation(DictationPhase::Transcribing, "")?;
                }
                Err(TrySendError::Full(_)) => {
                    // The dropped job's session guard cancels its preedit.
                    fail(events, "dictation queue is full")?;
                }
                Err(TrySendError::Disconnected(_)) => {
                    return Err(eyre!("dictation worker is unavailable"));
                }
            }
        }
    }
    Ok(false)
}

/// Starts Live typing at the hold threshold (or the Toggle press), before
/// the stream opens so the engine sees the demand; see
/// [`typing::begin`]. Short taps and chords create no input method.
fn begin_live_typing(
    capture: &DictationCapture,
    toggle: bool,
    wayland: bool,
    live_typing: &mut Option<Arc<crate::linux::live_typing::LiveTyping>>,
    next_capture_id: &mut u64,
    typing: &mut Option<CaptureTyping>,
) {
    typing::begin(
        capture,
        toggle,
        wayland,
        live_typing,
        next_capture_id,
        typing,
    );
}

/// Ends a capture's Live typing session without committing anything.
fn cancel_typing(typing: &mut Option<CaptureTyping>) {
    typing::cancel(typing);
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
    use crate::dictation::POST_RELEASE_TAIL;
    use crate::transcription::engine::Command;

    const PRE_ROLL: Duration = Duration::from_millis(450);

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

    #[test]
    fn the_ready_line_names_what_starts_dictation() {
        assert_eq!(
            readiness_message("System default", "Alt", &DesktopShortcuts::Registered, true),
            "Hex dictation is ready on System default. Hold Alt to dictate; Ctrl-C stops."
        );
        assert_eq!(
            readiness_message(
                "Yeti",
                "Alt+Space",
                &DesktopShortcuts::Portal(Vec::new()),
                false
            ),
            "Hex dictation is ready on Yeti. Hold Alt+Space to dictate; Ctrl-C stops."
        );
        let unregistered =
            readiness_message("Yeti", "Alt+Space", &DesktopShortcuts::Unregistered, true);
        assert!(!unregistered.contains("Hold"));

        let service = readiness_message("Yeti", "Alt+Space", &DesktopShortcuts::Commands, true);
        assert!(service.contains("No shortcut is bound"));
        assert!(service.contains("`hex dictation start`"));
        assert!(service.contains("`hex dictation stop`"));
        assert!(service.contains("`hex dictation toggle`"));
        assert!(service.contains("desktop's keybindings"));
        assert!(!service.contains("Hold"));

        let foreground = readiness_message("Yeti", "Alt+Space", &DesktopShortcuts::Commands, false);
        assert!(foreground.contains("No shortcut is bound"));
        assert!(foreground.contains("can't receive `hex` commands"));
        assert!(foreground.contains("run `hex start`"));
        assert!(foreground.contains("`hex dictation toggle`"));
        assert!(!foreground.contains("Hold"));
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
        start_capture(capture, start, PRE_ROLL, || {});
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
    fn a_press_between_large_chunks_is_placed_on_the_stream_clock() {
        let mut clock = StreamClock::default();
        let mut capture = DictationCapture::new(16_000);
        let received = Instant::now();
        let through = capture_time();
        capture.ingest(&[1.0; 16_000], through);
        clock.chunk(through, received);
        // Pressed 800 ms after a 1 s chunk arrived: none of it is pre-roll.
        let pressed = clock.at(received + Duration::from_millis(800)).unwrap();
        assert_eq!(pressed, through + Duration::from_millis(800));
        start_capture(&mut capture, pressed, PRE_ROLL, || {});
        assert!(capture.recorded_from(0).is_empty());
        capture.ingest(&[2.0; 16_000], through + Duration::from_secs(1));
        assert_eq!(capture.recorded_from(0).len(), 7_200 + 3_200);
        assert!(!capture.become_intentional(through + Duration::from_secs(1)));
        // A stalled stream's lag is bounded.
        assert_eq!(
            clock.at(received + Duration::from_secs(60)),
            Some(through + StreamClock::MAX_LAG)
        );
    }

    #[test]
    fn a_short_microphone_gap_is_padded_into_a_finished_clip() {
        let mut capture = DictationCapture::new(16_000);
        let start = capture_time();
        start_capture(&mut capture, start, PRE_ROLL, || {});
        let first_through = start + Duration::from_secs(1);
        capture.ingest(&[0.1; 16_000], first_through);
        // This chunk starts 100 ms after the last one ended.
        let second_from = first_through + Duration::from_millis(100);
        let second_through = second_from + Duration::from_secs(1);
        let ChunkStep::Pad(padding) =
            chunk_step(Some(first_through), 16_000, second_through, 16_000)
        else {
            panic!("a 100 ms gap is padded");
        };
        assert_eq!(padding, 1_600);
        let mut padded = vec![0.0; padding];
        padded.extend_from_slice(&[0.2; 16_000]);
        capture.ingest(&padded, second_through);

        // Releasing 500 ms into the second chunk keeps 200 ms of tail: the
        // clip runs to 1.8 s because the gap is on the timeline. Without the
        // padding, truncation would drop 100 ms of real speech before it.
        let released = second_from + Duration::from_millis(500);
        let Finish::Transcribe(clip) = capture.finish_with_tail(released, POST_RELEASE_TAIL) else {
            panic!("the release transcribes");
        };
        let samples = clip.into_transcription_samples();
        assert_eq!(samples.len(), 1_800 * 16);
        assert!(samples[16_000..17_600].iter().all(|sample| *sample == 0.0));
        assert_eq!(samples[17_600], 0.2);
        assert_eq!(samples[28_799], 0.2);
    }

    #[test]
    fn submit_capture_reports_when_a_clip_is_digital_silence() {
        let (engine, _commands) = TranscriptionEngine::for_test();
        let (mut events, path) = event_log();
        let (jobs, queued) = mpsc::sync_channel(2);
        let mut capture = DictationCapture::new(16_000);
        let start = capture_time();
        start_capture(&mut capture, start, PRE_ROLL, || {});
        let released = start + Duration::from_secs(1);
        capture.ingest(&[0.0; 16_000], released);
        let mut pending = 0;
        let silent = submit_capture(
            &mut capture,
            released,
            POST_RELEASE_TAIL,
            &engine,
            &jobs,
            &mut events,
            &mut pending,
            None,
            crate::dictation::DictationFlavour::Normal,
            &mut None,
        )
        .unwrap();
        assert!(silent);
        assert_eq!(pending, 0);
        assert!(queued.try_recv().is_err());
        drop(events);
        let _ = std::fs::remove_file(path);
    }

    #[test]
    fn a_reopened_stream_carries_no_old_clock() {
        let mut clock = StreamClock::default();
        let received = Instant::now();
        clock.chunk(capture_time(), received);
        clock.reset();
        // A press before the new stream's first chunk waits for it.
        assert_eq!(clock.at(received + Duration::from_millis(10)), None);
        assert_eq!(clock.through(), CaptureInstant::ZERO);
        let fresh = CaptureInstant::from_nanos(1_000_000_000);
        clock.chunk(fresh, received + Duration::from_millis(20));
        assert_eq!(
            clock.at(received + Duration::from_millis(30)),
            Some(fresh + Duration::from_millis(10))
        );
    }

    #[test]
    fn a_client_dictation_counts_at_once_and_an_abandoned_one_records_nothing() {
        let suppressor = || {
            crate::linux::audio_control::AudioSuppressor::new(
                crate::app_settings::RecordingAudioBehavior::DoNothing,
            )
        };
        let start = |owner: &mut ServiceDictations| {
            let (levels, _) = mpsc::sync_channel(1);
            let (reply, answer) = mpsc::sync_channel(1);
            assert_eq!(
                owner.control(
                    crate::dictation::service::DictationControl::Start {
                        source: "test".into(),
                        owner_token: "hex_a".into(),
                        levels,
                        partials: None,
                        reply,
                    },
                    false,
                    Instant::now(),
                ),
                Effect::Start
            );
            answer
        };

        let mut owner = ServiceDictations::default();
        let mut capture = DictationCapture::new(16_000);
        let answer = start(&mut owner);
        let started = capture_time();
        start_service_capture(&mut capture, &mut owner, &suppressor(), started, 16_000);
        assert_eq!(answer.try_recv().unwrap().unwrap().sample_rate, 16_000);
        // No hold threshold: a finish 100 ms in still transcribes.
        let through = started + Duration::from_millis(100);
        capture.ingest(&[0.1; 1_600], through);
        assert!(matches!(
            capture.finish_with_tail(through, Duration::ZERO),
            Finish::Transcribe(_)
        ));

        let mut owner = ServiceDictations::default();
        drop(start(&mut owner));
        start_service_capture(&mut capture, &mut owner, &suppressor(), started, 16_000);
        assert!(!capture.is_recording());
        assert!(!owner.is_active());
    }

    #[test]
    fn start_sound_precedes_audio_and_does_not_change_short_tap_discard() {
        let mut capture = DictationCapture::new(16_000);
        let started_at = capture_time();
        let mut sounds = Vec::new();

        start_capture(&mut capture, started_at, PRE_ROLL, || {
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
            start_capture(&mut capture, start, PRE_ROLL, || {});
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
        assert!(finishing.release.waits());
        submit_capture(
            &mut capture,
            finishing.release.ended_at(),
            POST_RELEASE_TAIL,
            &engine,
            &jobs,
            &mut events,
            &mut pending,
            None,
            crate::dictation::DictationFlavour::Normal,
            &mut None,
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
        start_capture(&mut short, start, PRE_ROLL, || {});
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
        assert!(!finishing.release.waits(), "a short tap needs no tail");
        submit_capture(
            &mut short,
            finishing.release.ended_at(),
            POST_RELEASE_TAIL,
            &engine,
            &jobs,
            &mut events,
            &mut pending,
            finishing.print,
            crate::dictation::DictationFlavour::Normal,
            &mut None,
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
            POST_RELEASE_TAIL,
            &engine,
            &jobs,
            &mut events,
            &mut pending,
            None,
            crate::dictation::DictationFlavour::Normal,
            &mut None,
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
    fn typed_text_is_registered_only_while_its_lab_is_on() {
        use crate::app_settings::{HotkeyBinding, HotkeyKey, HotkeyModifiers};
        let binding = HotkeyBinding {
            modifiers: HotkeyModifiers::default(),
            key: Some(HotkeyKey::named("f8")),
        };
        let mut settings = AppSettings {
            typed_text_hotkey: Some(binding.clone()),
            ..AppSettings::default()
        };
        let (_, options) = shortcuts(&settings);
        assert!(
            options.typed_text.is_none(),
            "the lab off registers nothing"
        );

        settings.labs.typed_text = true;
        assert_eq!(
            shortcuts(&settings).1.typed_text,
            Some(LinuxHotkey::from(&binding))
        );

        settings.typed_text_hotkey = None;
        assert!(shortcuts(&settings).1.typed_text.is_none());
    }

    #[test]
    fn second_dictation_is_registered_only_while_its_lab_is_on() {
        use crate::app_settings::{HotkeyBinding, HotkeyKey, HotkeyModifiers};
        let binding = HotkeyBinding {
            modifiers: HotkeyModifiers::default(),
            key: Some(HotkeyKey::named("f7")),
        };
        let mut settings = AppSettings {
            second_dictation_hotkey: Some(binding.clone()),
            ..AppSettings::default()
        };
        let (_, options) = shortcuts(&settings);
        assert!(
            options.second_dictation.is_none(),
            "the lab off registers nothing"
        );

        settings.labs.second_dictation_shortcut = true;
        assert_eq!(
            shortcuts(&settings).1.second_dictation,
            Some(LinuxHotkey::from(&binding))
        );

        // A shortcut without a key never runs on Linux.
        settings.second_dictation_hotkey = Some(HotkeyBinding {
            modifiers: HotkeyModifiers {
                control: Some(crate::app_settings::ModifierSide::Either),
                ..HotkeyModifiers::default()
            },
            key: None,
        });
        assert!(shortcuts(&settings).1.second_dictation.is_none());

        settings.second_dictation_hotkey = None;
        assert!(shortcuts(&settings).1.second_dictation.is_none());
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
