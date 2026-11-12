use std::path::Path;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{self, SyncSender, TrySendError};
use std::sync::{Arc, Mutex, RwLock};
use std::thread;
use std::time::{Duration, Instant};

use color_eyre::eyre::{Result, eyre};

use crate::audio::linger::MicrophoneLinger;
use crate::audio::{CaptureInstant, RecoveringAudioInput, RecoveringAudioInputEvent};
use crate::control::{CaptureAction, PrintReply, PrintedTranscript};
use crate::desktop::events::{
    DictationPhase, EventLog, TranscriptPhase, VoiceEvent, VoiceState, now_ms,
};
use crate::dictation::service::{DictationQueue, Effect, MICROPHONE_LOST, ServiceDictations};
use crate::dictation::{DictationCapture, Finish, ReleaseWait};
use crate::transcription::engine::{PendingTranscript, TranscriptionEngine};
use crate::transcription::local_transcriber::LocalTranscriber;
use crate::transcription::models::TranscriptionSelection;
use crate::windows::input::WindowsHotkey;
use crate::windows::input::{HotkeyAction, WindowsHotkeyMonitor};
use crate::windows::paste::WindowsPaster;

mod shortcuts;

use shortcuts::Shortcuts;

const UPDATE_INTERVAL: Duration = Duration::from_millis(5);
/// The longest wait with the microphone streaming, idle or recording. Its
/// chunks (about every 10 ms) wake the loop, which then takes shortcut
/// events and commands and ends a release's tail, so it needs no wakeup of
/// its own between them. A shortcut's audio boundaries come from its
/// event's own time, so taking it at the next chunk moves nothing.
const STREAM_WAIT: Duration = Duration::from_millis(50);
/// The longest wait while idle with the microphone released: a shortcut
/// event or a command ends it at once ([`wake`]); settings and Annotate's
/// controls are taken within it.
const IDLE_RELEASED_WAIT: Duration = Duration::from_millis(50);

/// The running listener's thread, which [`wake`] unparks.
static LISTENER: Mutex<Option<thread::Thread>> = Mutex::new(None);

/// Ends the listener's idle wait at once: the keyboard hook calls it for
/// each shortcut event, and the tray and `hex` commands after sending one.
pub fn wake() {
    if let Some(listener) = LISTENER
        .lock()
        .unwrap_or_else(|error| error.into_inner())
        .as_ref()
    {
        listener.unpark();
    }
}

struct Job {
    transcript: PendingTranscript,
    audio_ms: u64,
    /// The language the model was asked for at submission.
    language: String,
    /// The clip, kept for History while Labs › Keep recordings is on.
    recording: Option<crate::history::Recording>,
    /// The shortcut that started this capture (dictation, raw, terminal
    /// paste, or second dictation).
    flavour: crate::dictation::DictationFlavour,
    /// `hex dictation stop --print`: the text goes here, never pasted.
    print: Option<PrintReply>,
}

/// A release whose capture is still collecting its post-release tail.
struct Finishing {
    release: ReleaseWait,
    print: Option<PrintReply>,
    flavour: crate::dictation::DictationFlavour,
}

enum OutputJob {
    Dictation(Job),
    PasteLast,
}

#[derive(Clone, Copy, PartialEq)]
enum OutputKind {
    Dictation,
    Repaste,
    Printed,
}

struct JobResult {
    kind: OutputKind,
    text: Option<String>,
    latency_ms: u32,
    result: Result<(), String>,
}

pub struct WindowsDictationConfig {
    pub device: Option<String>,
    pub hotkey: WindowsHotkey,
    pub paste_last_hotkey: Option<WindowsHotkey>,
    /// Dictate without processing, while it is bound.
    pub raw_dictation_hotkey: Option<WindowsHotkey>,
    /// Labs › Terminal paste shortcut, while the lab is on and it is bound.
    pub terminal_paste_hotkey: Option<WindowsHotkey>,
    /// Labs › Second dictation shortcut, while the lab is on and it is bound.
    pub second_dictation_hotkey: Option<WindowsHotkey>,
    /// The Annotate key, while Annotate is on.
    pub annotate_hotkey: Option<WindowsHotkey>,
    /// Toggle mode's cancel shortcut; none in Hold mode.
    pub cancel_hotkey: Option<WindowsHotkey>,
    pub double_tap_lock: bool,
    pub double_tap_only: bool,
    pub shortcut_mode: crate::dictation::ShortcutMode,
    pub share_dictation_shortcut: bool,
    pub while_dictating: crate::app_settings::RecordingAudioBehavior,
    pub release_microphone_while_idle: bool,
    pub feedback_volume: u8,
    pub sound_theme: crate::feedback::SoundSet,
    pub history: Option<crate::history::History>,
    pub last_dictation: Arc<Mutex<Option<String>>>,
    pub indicator: Option<crate::rune_hud::HudSender>,
    pub mode_runtime: Arc<RwLock<crate::dictation::modes::ModeRuntime>>,
    pub transformations: Arc<crate::personal_commands::TransformationClient>,
    pub fallback_to_default_device: bool,
    /// Terminal paste: Ctrl+Shift+V instead of Ctrl+V.
    pub paste_with_shift: bool,
    /// Where the listener reports a model change that failed to load.
    pub swap_failures: Option<mpsc::Sender<crate::transcription::local_transcriber::SwapFailed>>,
    /// What the tray asks of the listener.
    pub commands: Option<mpsc::Receiver<ListenerCommand>>,
    /// The local API's `/dictations`: a client's dictation recorded beside
    /// the shortcut, one capture at a time.
    pub dictations: DictationQueue,
}

/// A request from outside the keyboard hook, from the tray or a `hex`
/// command (`crate::control::host`), handled as its shortcut would be.
#[derive(Debug)]
pub enum ListenerCommand {
    /// `raw`: the dictation this starts skips processing.
    Dictation {
        action: CaptureAction,
        raw: bool,
    },
    PasteLast,
    /// `hex dictation stop --print`.
    StopAndPrint(PrintReply),
}

impl WindowsDictationConfig {
    /// The listener configuration `settings` describe, with fresh runtime
    /// state; a host swaps in the state it shares with the listener.
    pub fn from_settings(settings: &crate::app_settings::AppSettings) -> Self {
        Self {
            device: settings.microphone.clone(),
            hotkey: WindowsHotkey::from(&settings.dictation_hotkey),
            paste_last_hotkey: settings.paste_last_hotkey.as_ref().map(WindowsHotkey::from),
            raw_dictation_hotkey: settings
                .raw_dictation_hotkey
                .as_ref()
                .map(WindowsHotkey::from),
            terminal_paste_hotkey: settings
                .active_terminal_paste_hotkey()
                .map(WindowsHotkey::from),
            second_dictation_hotkey: settings
                .active_second_dictation_hotkey()
                .map(WindowsHotkey::from),
            annotate_hotkey: settings
                .capture_on()
                .then(|| WindowsHotkey::from(&settings.capture.hotkey)),
            cancel_hotkey: settings.active_cancel_hotkey().map(WindowsHotkey::from),
            double_tap_lock: settings.double_tap_lock,
            double_tap_only: settings.double_tap_only,
            shortcut_mode: settings.shortcut_mode,
            share_dictation_shortcut: settings.share_dictation_shortcut,
            while_dictating: settings.recording_audio_behavior,
            release_microphone_while_idle: settings.release_microphone_while_idle,
            feedback_volume: if settings.sound_effects {
                (settings.sound_effect_volume.clamp(0.0, 1.0) * 100.0).round() as u8
            } else {
                0
            },
            sound_theme: settings.sound_theme,
            history: None,
            last_dictation: Arc::new(Mutex::new(None)),
            indicator: None,
            mode_runtime: Arc::new(RwLock::new(
                crate::dictation::modes::ModeRuntime::from_settings(settings),
            )),
            transformations: Arc::new(crate::personal_commands::TransformationClient::default()),
            fallback_to_default_device: true,
            paste_with_shift: settings.paste_with_shift,
            swap_failures: None,
            commands: None,
            dictations: DictationQueue::default(),
        }
    }
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
    // Labs › Unload model when idle: a launch stays parked until the first
    // dictation, as after an idle unload.
    let transcriber = if crate::labs::unload_idle().is_some() {
        None
    } else {
        Some(LocalTranscriber::load(selection)?)
    };
    // The hook's shortcuts from the same settings, before its fields are
    // destructured below.
    let mut shortcuts = Shortcuts::from_config(&config);
    let WindowsDictationConfig {
        device,
        hotkey,
        while_dictating,
        release_microphone_while_idle,
        feedback_volume,
        sound_theme,
        history,
        last_dictation,
        indicator,
        mode_runtime,
        transformations,
        fallback_to_default_device,
        paste_with_shift,
        swap_failures,
        commands,
        dictations,
        ..
    } = config;
    // Read by the output worker at each paste, so a change applies in place.
    let paste_with_shift = Arc::new(AtomicBool::new(paste_with_shift));
    let _indicator_guard = IndicatorGuard(indicator.clone());
    let _woken = Woken::register();
    crate::feedback::set_enabled(feedback_volume > 0);
    crate::feedback::set_volume(f32::from(feedback_volume) / 100.0);
    crate::feedback::set_sound_set(sound_theme);
    if feedback_volume > 0
        && let Err(error) = crate::feedback::preload()
    {
        tracing::warn!(%error, "Windows recording feedback is unavailable");
    }
    let mut release_while_idle = release_microphone_while_idle;
    // Labs › Keep mic open after dictating: a finished capture's
    // microphone stays open for its window before release.
    let mut linger = MicrophoneLinger::default();
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
        device.clone().unwrap_or_else(|| "System default".into())
    };
    // A hotkey press that arrived while the released microphone was still
    // opening; recording begins at the first chunk.
    let mut start_pending = false;
    if shutdown.load(Ordering::Relaxed) {
        return Ok(());
    }
    let hotkey_label = hotkey.label();
    let mut toggle_mode = shortcuts.shortcut_mode == crate::dictation::ShortcutMode::Toggle;
    let mut hotkey = shortcuts.start()?;
    // A shortcut event the idle wait took, handled at the next turn.
    let mut held_event = None;
    let mut while_dictating = while_dictating;
    let mut audio_suppressor =
        crate::windows::audio_control::AudioSuppressor::start(while_dictating);
    let mut model = crate::transcription::local_transcriber::ModelSwap::new(selection);
    let mut engine = match transcriber {
        Some(transcriber) => Arc::new(TranscriptionEngine::start(transcriber)?),
        None => Arc::new(TranscriptionEngine::start_parked(selection.clone())?),
    };
    // The local API transcribes host audio on this engine's model.
    engine.share();
    // The `capture` experiment: its host, the audio it copies from this
    // listener's microphone, and the capsule's commands.
    let timeline = crate::capture_host::ListenerTimeline::default();
    let environment = crate::capture_host::Environment;
    let mut capture_host =
        crate::capture_host::CaptureHost::start(Some(engine.clone()), &environment);
    let capture_controls = crate::capture_host::listen_for_controls();
    let mut finishing: Option<Finishing> = None;
    let (jobs, job_receiver) = mpsc::sync_channel::<OutputJob>(2);
    let (result_sender, results) = mpsc::channel();
    let worker_paste_with_shift = paste_with_shift.clone();
    let worker = thread::Builder::new()
        .name("windows-dictation-output".into())
        .spawn(move || {
            let mut paster = WindowsPaster::new(worker_paste_with_shift);
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
                    OutputJob::PasteLast => process_paste_last(&mut paster, &last_dictation),
                };
                if result_sender.send(result).is_err() {
                    break;
                }
            }
        })?;

    let mut recording = false;
    let mut recording_flavour = crate::dictation::DictationFlavour::Normal;
    let mut recording_feedback_started = false;
    // Whether this capture's start tone already played on its press.
    let mut start_tone_played = false;
    let mut pending = 0_usize;
    // The worker drains its queue in order, so a FIFO of ids pairs each
    // result with the HUD job it belongs to. Repaste jobs get ids too (to
    // keep the queue aligned) but are never announced to the HUD.
    let mut next_job_id = 0_u64;
    let mut job_ids = std::collections::VecDeque::new();
    // A local API client's dictation, and its transcripts as they finish.
    let mut service = ServiceDictations::default();
    // Whether the client's dictation records, shown on the HUD.
    let mut service_recording = false;
    let (service_results, service_finished) = mpsc::channel::<(u64, Result<String, String>)>();
    events.emit(&VoiceEvent::SessionStarted {
        timestamp_ms: now_ms(),
    })?;
    emit_state(&mut events, VoiceState::Listening, &device_label)?;
    println!(
        "Hex is ready on {}. Hold {} to dictate, release to paste; Ctrl-C stops.",
        device_label, hotkey_label
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
                    recording
                        || start_pending
                        || finishing.is_some()
                        || service.is_active()
                        || timeline.wants_audio(),
                )
            {
                input.close();
                audio_ready = false;
            }
        };
    }

    while !shutdown.load(Ordering::Relaxed) {
        if let Some(failed) = model.poll() {
            fail(&events, failed.error.clone())?;
            if let Some(swap_failures) = &swap_failures {
                let _ = swap_failures.send(failed);
            }
        }
        // Only the newest saved settings matter; they wait for a moment
        // without a recording.
        if !recording
            && !start_pending
            && finishing.is_none()
            && !service.is_active()
            && let Some(next) = updates.try_iter().last()
        {
            let next_config = WindowsDictationConfig::from_settings(&next);
            paste_with_shift.store(next_config.paste_with_shift, Ordering::Relaxed);
            let next_shortcuts = Shortcuts::from_config(&next_config);
            if next_shortcuts != shortcuts {
                // The old hook goes first; one keyboard hook runs at a time.
                drop(hotkey);
                held_event = None;
                shortcuts = next_shortcuts;
                hotkey = shortcuts.start()?;
                toggle_mode = shortcuts.shortcut_mode == crate::dictation::ShortcutMode::Toggle;
            }
            if next_config.while_dictating != while_dictating {
                while_dictating = next_config.while_dictating;
                audio_suppressor =
                    crate::windows::audio_control::AudioSuppressor::start(while_dictating);
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
                    device.clone().unwrap_or_else(|| "System default".into())
                };
                emit_current_state(&mut events, recording, pending, &device_label)?;
            }
            model.want(&next.transcription);
        }
        // A GPU without room for two copies: the engine swaps in place.
        if !recording
            && !start_pending
            && finishing.is_none()
            && !service.is_active()
            && pending == 0
            && let Some(selection) = model.take_reload()
            && let Err(error) = engine.reload(selection)
        {
            tracing::warn!(%error, "could not replace the transcription model");
        }
        if !recording
            && !start_pending
            && finishing.is_none()
            && !service.is_active()
            && pending == 0
            && let Some(next) = model.take_ready()
        {
            let old = std::mem::replace(&mut engine, Arc::new(TranscriptionEngine::start(next)?));
            engine.share();
            crate::capture_host::use_engine(&engine);
            // A request on the local API or a capture still transcribing
            // holds the old engine a moment.
            if let Ok(old) = Arc::try_unwrap(old) {
                old.retire();
            }
        }
        crate::capture_host::CaptureHost::reconcile(
            &mut capture_host,
            Some(&engine),
            &environment,
            &timeline,
        );
        while let Ok(control) = capture_controls.try_recv() {
            let reply = match (&mut capture_host, control) {
                (Some(host), crate::capture_host::CaptureControl::Scrawl(action)) => {
                    host.control(action, &timeline)
                }
                (Some(host), crate::capture_host::CaptureControl::Command(command)) => {
                    host.command(command, &timeline)
                }
                (host, crate::capture_host::CaptureControl::Reply(action, answer)) => {
                    let _ = answer.send(match host {
                        Some(host) => host.control(action, &timeline),
                        None => crate::control::ControlReply::error(
                            "feature-off",
                            "Annotate is off; turn it on with `hex settings set capture.enabled on`",
                        ),
                    });
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
        if let Ok(error) = hotkey.errors.try_recv() {
            return Err(eyre!("Windows hotkey monitor stopped: {error}"));
        }
        // A release keeps recording through its tail; shortcut events wait,
        // so a press inside the tail starts after this clip is submitted.
        if let Some(finish) = &finishing
            && let Some(tail) = finish.release.tail(&capture, Instant::now())
        {
            let Some(Finishing {
                release,
                print,
                flavour,
                ..
            }) = finishing.take()
            else {
                unreachable!("checked above");
            };
            let submitted = submit_capture(
                &mut capture,
                release.ended_at(),
                tail,
                &engine,
                &jobs,
                &mut events,
                &mut pending,
                print,
                flavour,
            )?;
            if submitted {
                next_job_id += 1;
                job_ids.push_back(next_job_id);
            }
            if let Some(indicator) = &indicator {
                indicator.send(if submitted {
                    crate::rune_hud::HudEvent::Submitted {
                        job_id: next_job_id,
                    }
                } else {
                    crate::rune_hud::HudEvent::Discarded
                });
            }
            emit_current_state(&mut events, recording, pending, &device_label)?;
            // The capture's end releases the microphone here, as before, when
            // the lab is off.
            release_microphone!();
        }
        // `/dictations`: a client's dictation never ends a shortcut capture,
        // and only its owner (or its lapsed lease) ends it.
        let now = Instant::now();
        let mut service_ended = service.expire(now);
        for control in dictations.drain() {
            let busy = recording
                || start_pending
                || finishing.is_some()
                || capture_host
                    .as_mut()
                    .is_some_and(|host| host.recording_since(&timeline).is_some());
            match service.control(control, busy, now) {
                Effect::None => {}
                Effect::Start if audio_ready && input.is_open() => {
                    service_recording = start_service_capture(
                        &mut capture,
                        &mut service,
                        &audio_suppressor,
                        indicator.as_ref(),
                        captured_through,
                        input.sample_rate(),
                    );
                    service_ended |= !service.is_active();
                }
                // Recording starts at the released microphone's first chunk.
                // A failed stream keeps its backoff, but the client's start
                // asks for the microphone now.
                Effect::Start => {
                    input.retry_now();
                    if !input.is_active() {
                        input.request_open();
                    }
                }
                Effect::Finish(id) => {
                    // The client's finish is its exact end: no tail.
                    match capture.finish_with_tail(captured_through, Duration::ZERO) {
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
            end_service_capture(
                &mut capture,
                &audio_suppressor,
                indicator.as_ref(),
                &mut service_recording,
            );
            // A client's dictation ended here before too.
            release_microphone!();
        }
        for (id, result) in service_finished.try_iter() {
            service.complete(id, result);
        }
        // A tray or `hex` command acts as its shortcut would, at once; a
        // printing stop's reply rides with the capture it finishes.
        let mut print = None;
        while finishing.is_none()
            && let Some((action, occurred_at, deliberate, flavour)) = next_action(
                &hotkey,
                &mut held_event,
                commands.as_ref(),
                recording || start_pending,
                captured_through,
                &mut print,
            )
        {
            match action {
                // A client's dictation records: the shortcut and `hex
                // dictation` neither start nor end anything.
                HotkeyAction::Start | HotkeyAction::Finish | HotkeyAction::Cancel
                    if service.is_active() => {}
                // Annotate records the narration itself: while it records,
                // dictation starts nothing.
                HotkeyAction::Start
                    if capture_host
                        .as_mut()
                        .is_some_and(|host| host.recording_since(&timeline).is_some()) => {}
                // One key: a tap starts and stops; held while recording, it
                // draws until it comes up.
                HotkeyAction::Capture => {
                    if let Some(host) = &mut capture_host {
                        host.annotate_down(&timeline);
                    }
                }
                HotkeyAction::CaptureUp => {
                    if let Some(host) = &mut capture_host {
                        host.annotate_up(&timeline);
                    }
                }
                // A failed stream keeps its backoff; the press asks for the
                // microphone now, though it cannot capture until it opens.
                HotkeyAction::Start
                    if !recording && input.is_recovering() && !release_while_idle =>
                {
                    input.retry_now();
                }
                HotkeyAction::Start
                    if !recording && (audio_ready || (release_while_idle && !start_pending)) =>
                {
                    recording_flavour = flavour;
                    if audio_ready {
                        capture.start_hotkey_at(
                            occurred_at,
                            crate::dictation::hotkey_pre_roll(while_dictating),
                        );
                        recording = true;
                        // A toggle press or a `hex` command is deliberate, so
                        // it streams at once.
                        recording_feedback_started = toggle_mode || deliberate;
                        if recording_feedback_started {
                            audio_suppressor.suppress();
                        }
                        // The tone confirms the press at once; a short tap or a
                        // chord meant for another application retracts it.
                        crate::feedback::play_provisional_start();
                        start_tone_played = true;
                    } else {
                        // The released microphone opens on demand; recording
                        // begins at the first chunk (no audio pre-roll). A
                        // failed stream keeps its backoff, but the press
                        // asks for it now.
                        if input.is_recovering() {
                            input.retry_now();
                        }
                        start_pending = true;
                        input.request_open();
                        crate::feedback::prepare();
                    }
                    if let Some(indicator) = &indicator {
                        indicator.send(crate::rune_hud::HudEvent::Started);
                    }
                    events.dictation(DictationPhase::Started, "")?;
                    emit_state(&mut events, VoiceState::Dictating, &device_label)?;
                }
                HotkeyAction::Finish | HotkeyAction::Cancel if start_pending => {
                    // Released before the opening microphone produced audio:
                    // nothing was captured, so treat it as a discard.
                    start_pending = false;
                    audio_suppressor.restore();
                    release_microphone!();
                    if let Some(print) = print.take() {
                        print.send(PrintedTranscript::NoSpeech);
                    }
                    let cancelled = matches!(action, HotkeyAction::Cancel);
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
                            crate::rune_hud::HudEvent::Cancelled
                        } else {
                            crate::rune_hud::HudEvent::Discarded
                        });
                    }
                    emit_current_state(&mut events, recording, pending, &device_label)?;
                }
                HotkeyAction::Finish if recording => {
                    audio_suppressor.restore();
                    if !recording_feedback_started && capture.become_intentional(occurred_at) {
                        if !start_tone_played {
                            crate::feedback::play(crate::feedback::Tone::DictationStart);
                        }
                        recording_feedback_started = true;
                    }
                    if recording_feedback_started {
                        crate::feedback::play(crate::feedback::Tone::DictationStop);
                    } else {
                        crate::feedback::retract_start();
                    }
                    recording = false;
                    recording_feedback_started = false;
                    finishing = Some(Finishing {
                        flavour: recording_flavour,
                        print: print.take(),
                        ..begin_finishing(&capture, occurred_at)
                    });
                }
                HotkeyAction::Cancel if recording => {
                    audio_suppressor.restore();
                    crate::feedback::retract_start();
                    if recording_feedback_started {
                        crate::feedback::play(crate::feedback::Tone::Cancel);
                    }
                    recording = false;
                    recording_feedback_started = false;
                    capture.cancel();
                    if let Some(indicator) = &indicator {
                        indicator.send(crate::rune_hud::HudEvent::Cancelled);
                    }
                    events.dictation(DictationPhase::Cancelled, "")?;
                    emit_current_state(&mut events, recording, pending, &device_label)?;
                    release_microphone!();
                }
                HotkeyAction::PasteLast if !recording => {
                    submit_paste_last(
                        &jobs,
                        &mut events,
                        &mut pending,
                        &mut next_job_id,
                        &mut job_ids,
                    )?;
                    emit_state(&mut events, VoiceState::Transcribing, &device_label)?;
                }
                _ => {}
            }
            if let Some(print) = print.take() {
                print.send(PrintedTranscript::NotRecording);
            }
        }
        while let Ok(result) = results.try_recv() {
            pending = pending.saturating_sub(1);
            let job_id = job_ids.pop_front();
            let nothing = result.result.is_ok() && result.text.as_deref() == Some("");
            if let (Some(indicator), Some(job_id)) = (&indicator, job_id) {
                // Repaste ids were never announced; a failed Paste last gets no notice.
                let failure = (result.kind != OutputKind::Repaste)
                    .then(|| result.result.as_ref().err().cloned())
                    .flatten();
                indicator.send(if nothing {
                    crate::rune_hud::HudEvent::JobCancelled { job_id }
                } else if let Some(message) = failure {
                    crate::rune_hud::HudEvent::JobFailed {
                        job_id,
                        message: Some(message),
                    }
                } else {
                    crate::rune_hud::HudEvent::JobCompleted { job_id }
                });
            }
            match result.result {
                Ok(()) if nothing => events.discarded_voice_prompt()?,
                Ok(()) => {
                    let text = result.text.unwrap_or_default();
                    if !matches!(result.kind, OutputKind::Repaste) {
                        crate::feedback::play(crate::feedback::Tone::DictationDone);
                    }
                    match result.kind {
                        OutputKind::Printed => {
                            events.dictation(DictationPhase::Printed, text)?;
                        }
                        OutputKind::Dictation => {
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
                Err(error) => fail(&events, error)?,
            }
            emit_current_state(&mut events, recording, pending, &device_label)?;
        }

        // The released microphone closes once nothing records, no client
        // dictates, no release tail runs, no capture copies it, and Labs ›
        // Keep mic open after dictating's window has passed. This also
        // releases one a stream interruption left open, which the old
        // latch-based sites never did.
        release_microphone!();

        let idle = !recording
            && !start_pending
            && finishing.is_none()
            && !service.is_active()
            && pending == 0
            && !timeline.wants_audio();
        // A released microphone delivers nothing: wait for the next shortcut
        // event, which is taken at once, instead of sleeping 5 ms at a time.
        if idle && !input.is_active() {
            // Shortcut events and commands unpark the loop ([`wake`]).
            held_event = hotkey.events.try_recv().ok();
            if held_event.is_none() {
                thread::park_timeout(IDLE_RELEASED_WAIT);
                held_event = hotkey.events.try_recv().ok();
            }
            continue;
        }
        // Only an opening or recovering microphone is polled.
        let streaming = audio_ready && input.is_open();
        match input.recv_timeout(
            if streaming {
                STREAM_WAIT
            } else {
                UPDATE_INTERVAL
            },
            crate::audio::capture_idle(
                recording || service_recording || timeline.wants_audio(),
                start_pending || service.is_starting(),
                finishing.is_some(),
            ),
        ) {
            RecoveringAudioInputEvent::Chunk {
                samples,
                captured_through: chunk_captured_through,
            } => {
                captured_through = chunk_captured_through;
                crate::audio::input_level::record(&samples);
                timeline.feed(&samples, input.sample_rate(), captured_through);
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
                        capture.start_hotkey_at(
                            chunk_captured_through,
                            crate::dictation::hotkey_pre_roll(while_dictating),
                        );
                        recording = true;
                        // The released microphone is ready only now.
                        recording_feedback_started = toggle_mode;
                        start_tone_played = recording_feedback_started;
                        if recording_feedback_started {
                            crate::feedback::play(crate::feedback::Tone::DictationStart);
                            audio_suppressor.suppress();
                        }
                        emit_state(&mut events, VoiceState::Dictating, &device_label)?;
                        continue;
                    }
                    if service.is_starting() {
                        // A client's dictation waited for the released
                        // microphone: it records from here.
                        service_recording = start_service_capture(
                            &mut capture,
                            &mut service,
                            &audio_suppressor,
                            indicator.as_ref(),
                            chunk_captured_through,
                            input.sample_rate(),
                        );
                        // A start that gave up releases the microphone here,
                        // as before, when the lab is off.
                        release_microphone!();
                    }
                    emit_current_state(&mut events, recording, pending, &device_label)?;
                    continue;
                }
                capture.ingest_with_pending(&samples, captured_through, None);
                if service_recording {
                    if let Some(indicator) = &indicator {
                        indicator.meter(&samples);
                    }
                    service.audio(&samples, Instant::now());
                    // A client's dictation counts at once, so it streams at
                    // once: its finish is quicker, and it has live text.
                    capture.begin_stream(&engine);
                    if let Some(partial) = capture.report_live_text() {
                        service.partial(&partial);
                    }
                }
                if recording {
                    if let Some(indicator) = &indicator {
                        indicator.meter(&samples);
                    }
                    if capture.become_intentional(captured_through) && !recording_feedback_started {
                        if !start_tone_played {
                            crate::feedback::play(crate::feedback::Tone::DictationStart);
                        }
                        recording_feedback_started = true;
                        // Ordinary shortcut chords and short taps never mute,
                        // lower, or pause audio, or prevent sleep.
                        audio_suppressor.suppress();
                    }
                    // Short taps and shortcut chords never reach the model;
                    // the stream starts with the backlog once the hold counts.
                    if recording_feedback_started {
                        capture.begin_stream(&engine);
                    }
                    capture.report_live_text();
                }
            }
            RecoveringAudioInputEvent::Timeout => {}
            RecoveringAudioInputEvent::Interrupted => {
                timeline.restart();
                // A release still in its tail finishes with the audio it has.
                if let Some(finish) = &mut finishing {
                    finish.release.stop_waiting();
                }
                audio_ready = false;
                if service.is_active() {
                    end_service_capture(
                        &mut capture,
                        &audio_suppressor,
                        indicator.as_ref(),
                        &mut service_recording,
                    );
                    service.lost(MICROPHONE_LOST);
                } else if recording {
                    audio_suppressor.restore();
                    crate::feedback::retract_start();
                    recording_feedback_started = false;
                    recording = false;
                    capture.cancel();
                    events.dictation(DictationPhase::Cancelled, "")?;
                    if let Some(indicator) = &indicator {
                        indicator.send(crate::rune_hud::HudEvent::Cancelled);
                    }
                }
                events.dictation(
                    DictationPhase::Failed("microphone stream interrupted; reopening".into()),
                    "",
                )?;
                emit_current_state(&mut events, recording, pending, &device_label)?;
            }
            RecoveringAudioInputEvent::Reopened => {
                timeline.restart();
                capture = DictationCapture::new(input.sample_rate());
                audio_ready = false;
                device_label = input.device_name().to_string();
                emit_current_state(&mut events, recording, pending, &device_label)?;
            }
            RecoveringAudioInputEvent::OpenFailed(error) => {
                if !release_while_idle {
                    return Err(eyre!("could not reopen microphone: {error}"));
                }
                // On-demand open failed (device busy or unplugged): report
                // and stay released rather than killing the listener.
                start_pending = false;
                if service.is_active() {
                    end_service_capture(
                        &mut capture,
                        &audio_suppressor,
                        indicator.as_ref(),
                        &mut service_recording,
                    );
                    service.lost(MICROPHONE_LOST);
                }
                audio_suppressor.restore();
                let error = format!("could not open microphone: {error}");
                fail(&events, error.clone())?;
                if let Some(indicator) = &indicator {
                    indicator.send(crate::rune_hud::HudEvent::Failed {
                        message: Some(error),
                    });
                }
                emit_current_state(&mut events, recording, pending, &device_label)?;
            }
        }
    }

    if recording || finishing.is_some() {
        capture.cancel();
        events.dictation(DictationPhase::Cancelled, "")?;
    }
    if let Some(mut host) = capture_host.take() {
        host.shutdown(&timeline);
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
    mut job: Job,
    paster: &mut Result<WindowsPaster>,
    history: Option<&crate::history::History>,
    last_dictation: &Mutex<Option<String>>,
    mode_runtime: &RwLock<crate::dictation::modes::ModeRuntime>,
    transformations: &crate::personal_commands::TransformationClient,
) -> JobResult {
    let started = Instant::now();
    let printed = job.print.is_some();
    // The context read starts now so the UIA walk overlaps inference.
    let reads_page = !job.flavour.raw()
        && mode_runtime
            .read()
            .unwrap_or_else(|error| error.into_inner())
            .reads_page();
    let pending_context = crate::windows::context::begin_capture(reads_page);
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
        let processed = if job.flavour.raw() {
            crate::dictation::processor::Processed::unchanged(raw_text.trim())
        } else {
            let mode_runtime = mode_runtime
                .read()
                .unwrap_or_else(|error| error.into_inner())
                .clone();
            mode_runtime.process(
                &raw_text,
                &job.language,
                &context,
                transformations,
                crate::dictation::processor::Source::Dictation,
            )
        };
        let text = processed.text.clone();
        // A voice prompt that deliberately asked nothing ends quietly: no
        // failure, no paste, no last dictation, and no History.
        if processed.nothing {
            return Ok(text);
        }
        if text.trim().is_empty() {
            return Err("text replacements produced empty output".into());
        }
        // A printed dictation is never pasted, and Paste Last skips it.
        if !printed {
            let paster = paster.as_mut().map_err(|error| format!("{error:#}"))?;
            // Labs › Copy dictation only, read as the output commits:
            // dictations go to the clipboard unpasted; Paste Last still pastes.
            if crate::labs::copy_only() {
                paster
                    .copy_only(&text)
                    .map_err(|error| format!("{error:#}"))?;
            } else {
                paster
                    .paste(&text, job.flavour.terminal_paste())
                    .map_err(|error| format!("{error:#}"))?;
            }
            *last_dictation
                .lock()
                .unwrap_or_else(|error| error.into_inner()) = Some(text.clone());
        }
        if let Some(history) = history {
            let mut draft = crate::dictation::modes::history_draft(
                raw_text,
                &processed,
                application.clone(),
                job.audio_ms,
                inference_ms,
                started.elapsed().as_millis() as u64,
                job.recording.take(),
            );
            if printed {
                draft.kind = crate::history::HistoryKind::Printed;
            } else if job.flavour.raw() {
                draft.kind = crate::history::HistoryKind::Raw;
            }
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
    if let Some(print) = job.print.take() {
        print.send(match &result {
            Ok(text) if text.trim().is_empty() => PrintedTranscript::NoSpeech,
            Ok(text) => PrintedTranscript::Text(text.clone()),
            Err(error) => PrintedTranscript::Failed(error.clone()),
        });
        return job_result(OutputKind::Printed, result, latency_ms);
    }
    job_result(OutputKind::Dictation, result, latency_ms)
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
                .paste(&text, false)
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

/// The next shortcut event or `hex` command, as the action the loop takes,
/// when it occurred, whether it was deliberate (a command records at once,
/// like a toggle press), and the shortcut's flavour. A printing stop leaves
/// its reply in `print`.
fn next_action(
    hotkey: &WindowsHotkeyMonitor,
    held: &mut Option<crate::windows::input::HotkeyEvent>,
    commands: Option<&mpsc::Receiver<ListenerCommand>>,
    recording: bool,
    captured_through: CaptureInstant,
    print: &mut Option<PrintReply>,
) -> Option<(
    HotkeyAction,
    CaptureInstant,
    bool,
    crate::dictation::DictationFlavour,
)> {
    if let Some(event) = held.take().or_else(|| hotkey.events.try_recv().ok()) {
        return Some((
            event.action,
            event.occurred_at(captured_through),
            false,
            event.flavour,
        ));
    }
    let (action, flavour) = match commands?.try_recv().ok()? {
        ListenerCommand::Dictation { action, raw } => (
            match action {
                CaptureAction::Start => HotkeyAction::Start,
                CaptureAction::Stop => HotkeyAction::Finish,
                CaptureAction::Cancel => HotkeyAction::Cancel,
                CaptureAction::Toggle if recording => HotkeyAction::Finish,
                CaptureAction::Toggle => HotkeyAction::Start,
            },
            if raw {
                crate::dictation::DictationFlavour::Raw
            } else {
                crate::dictation::DictationFlavour::Normal
            },
        ),
        ListenerCommand::PasteLast => (
            HotkeyAction::PasteLast,
            crate::dictation::DictationFlavour::Normal,
        ),
        ListenerCommand::StopAndPrint(reply) => {
            *print = Some(reply);
            (
                HotkeyAction::Finish,
                crate::dictation::DictationFlavour::Normal,
            )
        }
    };
    Some((action, captured_through, true, flavour))
}

/// Starts a client's dictation, which counts at once (no hold threshold),
/// and answers its start. A client that already gave up gets no recording.
/// True when it records.
fn start_service_capture(
    capture: &mut DictationCapture,
    service: &mut ServiceDictations,
    suppressor: &crate::windows::audio_control::AudioSuppressor,
    indicator: Option<&crate::rune_hud::HudSender>,
    started_at: CaptureInstant,
    sample_rate: u32,
) -> bool {
    capture.start_programmatic_at(started_at);
    if !service.started(sample_rate) {
        capture.cancel();
        return false;
    }
    suppressor.suppress();
    if let Some(indicator) = indicator {
        indicator.send(crate::rune_hud::HudEvent::Started);
    }
    true
}

/// Drops what is left of a client's ended dictation (its finish already took
/// the clip) and clears the HUD it showed.
fn end_service_capture(
    capture: &mut DictationCapture,
    suppressor: &crate::windows::audio_control::AudioSuppressor,
    indicator: Option<&crate::rune_hud::HudSender>,
    recording: &mut bool,
) {
    capture.cancel();
    suppressor.restore();
    if std::mem::take(recording)
        && let Some(indicator) = indicator
    {
        indicator.send(crate::rune_hud::HudEvent::Cancelled);
    }
}

fn begin_finishing(capture: &DictationCapture, ended_at: CaptureInstant) -> Finishing {
    Finishing {
        // A short tap is discarded on its release alone and needs no tail.
        release: ReleaseWait::new(capture, ended_at),
        print: None,
        flavour: crate::dictation::DictationFlavour::Normal,
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
) -> Result<bool> {
    let mut submitted = false;
    let no_speech = |print: Option<PrintReply>| {
        if let Some(print) = print {
            print.send(PrintedTranscript::NoSpeech);
        }
    };
    match capture.finish_with_tail(ended_at, tail) {
        Finish::Discard => {
            no_speech(print);
            events.dictation(DictationPhase::Discarded, "")?
        }
        Finish::Transcribe(clip) if clip.is_near_silent() => {
            no_speech(print);
            events.dictation(DictationPhase::Discarded, "")?
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
            });
            match jobs.try_send(job) {
                Ok(()) => {
                    *pending += 1;
                    submitted = true;
                    events.dictation(DictationPhase::Transcribing, "")?;
                }
                Err(TrySendError::Full(_)) => fail(events, "dictation queue is full")?,
                Err(TrySendError::Disconnected(_)) => {
                    return Err(eyre!("Windows dictation worker is unavailable"));
                }
            }
        }
    }
    Ok(submitted)
}

/// Logs a failed dictation or paste with the sound set's error sound.
fn fail(events: &EventLog, error: impl Into<String>) -> Result<()> {
    crate::feedback::play(crate::feedback::Tone::Error);
    events.dictation(DictationPhase::Failed(error.into()), "")?;
    Ok(())
}

/// Queues a repaste of the last dictation. Its job id keeps the result
/// queue aligned and is never announced to the HUD.
fn submit_paste_last(
    jobs: &SyncSender<OutputJob>,
    events: &mut EventLog,
    pending: &mut usize,
    next_job_id: &mut u64,
    job_ids: &mut std::collections::VecDeque<u64>,
) -> Result<()> {
    match jobs.try_send(OutputJob::PasteLast) {
        Ok(()) => {
            *pending += 1;
            *next_job_id += 1;
            job_ids.push_back(*next_job_id);
        }
        Err(TrySendError::Full(_)) => fail(events, "dictation output queue is full")?,
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

/// This thread as the one [`wake`] unparks, until the listener returns.
struct Woken(thread::ThreadId);

impl Woken {
    fn register() -> Self {
        let current = thread::current();
        let id = current.id();
        *LISTENER.lock().unwrap_or_else(|error| error.into_inner()) = Some(current);
        Self(id)
    }
}

impl Drop for Woken {
    fn drop(&mut self) {
        let mut listener = LISTENER.lock().unwrap_or_else(|error| error.into_inner());
        if listener
            .as_ref()
            .is_some_and(|thread| thread.id() == self.0)
        {
            *listener = None;
        }
    }
}

struct IndicatorGuard(Option<crate::rune_hud::HudSender>);

impl Drop for IndicatorGuard {
    fn drop(&mut self) {
        if let Some(indicator) = &self.0 {
            indicator.send(crate::rune_hud::HudEvent::Reset);
        }
    }
}
