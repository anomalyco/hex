use std::collections::{BTreeMap, VecDeque};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{Receiver, SyncSender, TrySendError};
use std::time::{Duration, Instant};

use color_eyre::Result;

use crate::audio::CaptureInstant;
use crate::audio::recording_environment::RecordingEnvironmentController;
use crate::control::{CaptureAction, CaptureKind, CaptureSnapshot, ControlReply, PrintReply};
use crate::desktop::events::{DictationPhase, EventLog, VoiceEvent, VoiceState, now_ms};
use crate::dictation::audio::{DictationAudio, DictationAudioEvent, LostCapture};
use crate::dictation::context::ContextSnapshot;
use crate::dictation::indicator::{DictationIndicatorEvent, DictationIndicatorSender};
use crate::dictation::service::{
    Completion, DictationAudioChunk, DictationControl, DictationLevel, LEASE, MICROPHONE_LOST,
    ProgrammaticDictationResult, ProgrammaticDictationStart, TerminalOutcome, finish_with_result,
    lease_expired, level_for, remember_completion,
};
use crate::dictation::{DictationClip, Finish, POST_RELEASE_TAIL};
use crate::feedback::{self, Tone};
use crate::parakeet::{DictationWorker, TranscriptionTarget, WorkerEvent};
use crate::suppression::{DictationHotkey, DiscardCause, HotkeyAction, InputMonitor, TapPolicy};

#[cfg(hex_capture)]
pub(crate) mod cancelled_words;
mod capture_actions;
mod context_watch;
mod events;
mod guards;
mod hotkeys;
mod output;
#[cfg(hex_capture)]
mod paste_picker;
mod stop_phrase;

use capture_actions::{control_dictation, handle_hotkey_action, paste_last};
use context_watch::ContextWatch;
use events::handle_dictation_event;
#[cfg(hex_capture)]
use guards::paste_picker_paused;
use guards::{Release, WarmRelease, capture_boundary, capture_refusal, paste_last_refusal};
use hotkeys::{Hotkeys, Shortcut};
use output::{finish_dictation, finish_dictation_with, interrupted_dictation};

enum FinishResult {
    Discarded,
    Silent,
    Submitted,
    Rejected(String),
}

/// The HUD and the Caps Lock light see the same dictation events. The HUD
/// channel has one consumer; the light's model is fed beside each send
/// instead of taking that channel over.
#[derive(Clone, Copy)]
struct DictationSink<'a> {
    hud: Option<&'a DictationIndicatorSender>,
    caps_lock: &'a crate::caps_lock_dictation::CapsLockDictation,
}

impl DictationSink<'_> {
    fn send(&self, event: DictationIndicatorEvent) {
        if let Some(hud) = self.hud {
            hud.send(event.clone());
        }
        self.caps_lock.observe(event);
    }

    /// One chunk's loudness reaches both consumers: the light even when no
    /// HUD listens, and the HUD exactly as it would have received it.
    fn meter(&self, samples: &[f32]) {
        if let Some(event) = DictationIndicatorEvent::meter(samples) {
            self.send(event);
        }
    }

    /// A non-dictation capture (Annotate) moves only the Caps Lock light, not
    /// the dictation HUD.
    #[cfg(hex_capture)]
    fn caps_only(&self, event: DictationIndicatorEvent) {
        self.caps_lock.observe(event);
    }
}

#[derive(Debug)]
pub enum RecognitionControl {
    /// Pastes the last dictation once nothing records; `reply` answers
    /// `hex paste-last`, and the menu bar sends none.
    PasteLast {
        reply: Option<SyncSender<ControlReply>>,
    },
    /// `hex dictation`: drives the shortcut machine as a press would, so the
    /// shortcut still finishes it.
    /// A `stop` with `print` answers `reply` once recording stops and sends
    /// the transcript to `print` instead of pasting it; a `start` with
    /// `print` keeps `print` for however that recording ends.
    Capture {
        action: CaptureAction,
        print: Option<PrintReply>,
        /// Skip processing for the dictation this starts.
        raw: bool,
        reply: SyncSender<ControlReply>,
    },
    /// What is recording, for `hex status`.
    CaptureStatus { reply: SyncSender<CaptureSnapshot> },
    /// `hex capture` (the `capture` experiment; see `capture_host`).
    #[cfg(hex_capture)]
    ScrawlCapture {
        action: crate::control::ScrawlCaptureAction,
        reply: SyncSender<ControlReply>,
    },
    /// The capture capsule's switch chips and picker (`capture_host`).
    #[cfg(hex_capture)]
    CaptureCommand {
        command: crate::capture_host::CaptureCommand,
        reply: SyncSender<ControlReply>,
    },
    /// `/dictations`, which a local API client owns.
    Dictation(DictationControl),
}

impl From<DictationControl> for RecognitionControl {
    fn from(control: DictationControl) -> Self {
        Self::Dictation(control)
    }
}

struct ProgrammaticDictation {
    id: u64,
    owner_token: String,
    source: String,
    levels: SyncSender<DictationLevel>,
    audio: Option<SyncSender<DictationAudioChunk>>,
    /// Whether this capture was pressed while the warm microphone was
    /// released, so its finish uses wall-clock time instead of the stream's.
    pressed_cold: bool,
    last_level: Instant,
    lease_deadline: Instant,
}

impl ProgrammaticDictation {
    fn matches_owner(&self, id: u64, owner_token: &str) -> bool {
        self.id == id && self.owner_token == owner_token
    }
}

/// A programmatic capture the microphone cut short: the audio delivered
/// before the loss, waiting for its owner's `/finish` or `/cancel` while
/// heartbeats renew its lease.
struct LostProgrammaticDictation {
    id: u64,
    owner_token: String,
    source: String,
    lease_deadline: Instant,
    clip: DictationClip,
}

impl LostProgrammaticDictation {
    fn matches_owner(&self, id: u64, owner_token: &str) -> bool {
        self.id == id && self.owner_token == owner_token
    }
}

struct FinishingProgrammaticDictation {
    id: u64,
    source: String,
    owner_token: String,
    duration_ms: u64,
    replies: Vec<SyncSender<Result<ProgrammaticDictationResult, String>>>,
}

impl FinishingProgrammaticDictation {
    fn matches_owner(&self, id: u64, owner_token: &str) -> bool {
        self.id == id && self.owner_token == owner_token
    }
}

pub fn listen(
    events: EventLog,
    device_override: Option<&str>,
    shutdown: &AtomicBool,
    indicator: Option<DictationIndicatorSender>,
    history: Option<crate::history::History>,
    controls: Option<Receiver<RecognitionControl>>,
    transformations: Arc<crate::personal_commands::TransformationClient>,
) -> Result<()> {
    shutdown.store(false, Ordering::Relaxed);
    // Feedback admission is advisory: a cold audio stack can exceed the
    // preload timeout, and missing tones must not stop desktop recognition.
    if let Err(error) = feedback::preload() {
        tracing::warn!(%error, "recording sounds are unavailable; continuing without feedback");
    }
    // A warm microphone is released while the setting asks or the person
    // demonstrably cannot dictate; the owner is told as that changes. A
    // capture keeps what its own press saw: the latch clears once the stream
    // is back, but that capture still owes its start tone.
    let (mut warm_release, mut release) = WarmRelease::start();
    let mut events = events;
    let input_monitor = InputMonitor::start()?;
    let mut context = ContextWatch::start(input_monitor.activity.clone());
    let mut hotkeys = Hotkeys::new(
        DictationHotkey::new(
            CaptureInstant::now(),
            crate::app_settings::double_tap_lock(),
            crate::app_settings::dictation_hotkey(),
        ),
        DictationHotkey::new(
            CaptureInstant::now(),
            false,
            crate::app_settings::RuntimeHotkey::default(),
        ),
    );
    hotkeys.configure(CaptureInstant::now());
    let hud = indicator;
    let caps_lock = crate::caps_lock_dictation::CapsLockDictation::new();
    let indicator = DictationSink {
        hud: hud.as_ref(),
        caps_lock: &caps_lock,
    };
    let recording_environment = RecordingEnvironmentController::start();
    #[cfg(hex_capture)]
    let capture_environment = recording_environment.clone();
    let (mut microphone_revision, microphone) = crate::app_settings::microphone_selection();
    #[cfg(hex_capture)]
    let paste_history = history.clone();
    let dictation_worker = DictationWorker::start(
        input_monitor.activity.clone(),
        transformations.clone(),
        history,
    );
    // Labs › Paste Last picker follows its switch on every pass, so turning
    // it on or off applies without a restart.
    #[cfg(hex_capture)]
    let mut paste_lab = paste_picker::Lab::new(paste_history, dictation_worker.engine());
    let input = DictationAudio::open(
        device_override,
        microphone_revision,
        microphone.as_deref(),
        recording_environment,
        input_monitor.pending_events(),
        release.press,
        dictation_worker.engine(),
    )?;
    #[cfg(hex_capture)]
    let mut capture_host =
        crate::capture_host::CaptureHost::start(dictation_worker.engine(), &capture_environment);
    #[cfg(hex_capture)]
    let mut capture_shortcut = crate::capture_host::CaptureShortcut::default();
    let (mut transcription_revision, _) = crate::app_settings::transcription_selection();
    let mut personal_host_enabled = crate::personal_commands::host_needed(
        crate::app_settings::custom_transformations_selected(),
    );
    let mut next_personal_host_check = Instant::now();
    let mut personal_commands = personal_host_enabled
        .then(|| crate::personal_commands::PersonalCommands::start(transformations.clone()))
        .flatten();
    let mut next_programmatic_id = 1_u64;
    let mut programmatic = None::<ProgrammaticDictation>;
    let mut programmatic_lost = None::<LostProgrammaticDictation>;
    let mut programmatic_results =
        BTreeMap::<crate::parakeet::DictationJobId, FinishingProgrammaticDictation>::new();
    let mut programmatic_completions = VecDeque::<Completion>::new();

    events.emit(&VoiceEvent::SessionStarted {
        timestamp_ms: now_ms(),
    })?;
    if input.is_recovering() {
        hotkeys.suspend_all();
    }
    if hotkeys.dictation.is_recording() {
        let accepted = input.start(capture_boundary(&input, release.press))?;
        if accepted {
            release.begin();
            events.dictation(DictationPhase::Started, "")?;
            indicator.send(DictationIndicatorEvent::Started);
        } else {
            hotkeys.dictation.suspend();
        }
    }
    emit_state(&mut events, hotkeys.any_recording(), &input.device_name())?;
    // Labs › Clean up what I say: the tagger's model loads as Hex starts,
    // so the first dictation after launch (even one held through the
    // start, which skips the press's own prepare) finds it warm.
    #[cfg(target_os = "macos")]
    crate::dictation::cleanup::prepare();
    tracing::info!(device = %input.device_name(), "dictation listener started");

    #[cfg(hex_capture)]
    let mut capture_was_recording = false;

    while !shutdown.load(Ordering::Relaxed) {
        release = warm_release.reconcile(&input, release);
        #[cfg(hex_capture)]
        crate::capture_host::CaptureHost::reconcile(
            &mut capture_host,
            dictation_worker.engine().as_ref(),
            &capture_environment,
            &input,
        );
        #[cfg(hex_capture)]
        {
            let capture_recording = capture_host
                .as_mut()
                .is_some_and(|host| host.recording_since(&input).is_some());
            if capture_recording != capture_was_recording {
                capture_was_recording = capture_recording;
                indicator.caps_only(if capture_recording {
                    DictationIndicatorEvent::Started
                } else {
                    DictationIndicatorEvent::Cancelled
                });
            }
        }
        #[cfg(hex_capture)]
        paste_lab.sync(crate::paste::recent::enabled());
        let caps_lock_on = crate::labs::caps_lock_dictation();
        caps_lock.set_enabled(caps_lock_on);
        // Turning the lab off mid-capture cancels it, as unbinding the
        // second shortcut does.
        if !caps_lock_on && hotkeys.stop_caps_lock() {
            handle_hotkey_action(
                HotkeyAction::Cancel,
                false,
                None,
                input.captured_through(),
                &input,
                &dictation_worker,
                &mut context,
                &input.device_name(),
                &mut events,
                indicator,
                &mut release,
            )?;
        }
        if let Some(controls) = &controls {
            while let Ok(control) = controls.try_recv() {
                match control {
                    RecognitionControl::PasteLast { reply } => {
                        let capture_active = programmatic.is_some()
                            || hotkeys.suppresses_recognition()
                            || input.is_recording();
                        #[cfg(hex_capture)]
                        let capture_active = capture_active || paste_lab.busy();
                        let answer = match paste_last_refusal(capture_active) {
                            Some(refusal) => refusal,
                            None => {
                                paste_last(&dictation_worker, &input.device_name(), &mut events)?
                            }
                        };
                        if let Some(reply) = reply {
                            let _ = reply.send(answer);
                        }
                    }
                    #[cfg(hex_capture)]
                    RecognitionControl::ScrawlCapture { action, reply } => {
                        let answer = match capture_host.as_mut() {
                            Some(host) => host.control(action, &input),
                            None => ControlReply::error(
                                "feature-off",
                                "Annotate is off; turn it on in Settings › Annotate or \
                                 run `hex settings set capture.enabled true`",
                            ),
                        };
                        let _ = reply.send(answer);
                    }
                    #[cfg(hex_capture)]
                    RecognitionControl::CaptureCommand { command, reply } => {
                        let answer = match capture_host.as_mut() {
                            Some(host) => host.command(command, &input),
                            None => ControlReply::error("feature-off", "captures are off"),
                        };
                        let _ = reply.send(answer);
                    }
                    RecognitionControl::CaptureStatus { reply } => {
                        let recording = if programmatic.is_some() {
                            Some(CaptureKind::Service)
                        } else if hotkeys.any_recording() {
                            Some(CaptureKind::Dictation)
                        } else {
                            None
                        };
                        let _ = reply.send(CaptureSnapshot {
                            recording,
                            pending_jobs: dictation_worker.pending_count(),
                            microphone_unavailable: input.is_recovering(),
                        });
                    }
                    RecognitionControl::Capture {
                        action,
                        print,
                        raw,
                        reply,
                    } => {
                        let other_recording = programmatic.is_some();
                        let suspended =
                            crate::app_settings::hotkey_capture_active() || input.is_recovering();
                        let outcome = control_dictation(
                            action,
                            print,
                            raw,
                            other_recording,
                            suspended,
                            &mut hotkeys,
                            &input,
                            &dictation_worker,
                            &mut context,
                            &mut events,
                            indicator,
                            &mut release,
                        )?;
                        let _ = reply.send(outcome);
                    }
                    RecognitionControl::Dictation(DictationControl::Start {
                        source,
                        owner_token,
                        levels,
                        partials,
                        reply,
                    }) => {
                        // A programmatic capture doesn't stream here, so it
                        // has no live text: a client's partials end at once.
                        drop(partials);
                        if programmatic.is_some() || input.is_recording() || hotkeys.any_recording()
                        {
                            let _ = reply.send(Err("dictation-busy".into()));
                            continue;
                        }
                        let id = next_programmatic_id;
                        next_programmatic_id = next_programmatic_id.wrapping_add(1).max(1);
                        let boundary = capture_boundary(&input, release.press);
                        match input.start_programmatic(boundary) {
                            Ok(()) => {
                                hotkeys.disarm_pending_gestures();
                                tracing::info!(
                                    dictation_id = id,
                                    source,
                                    "programmatic dictation started"
                                );
                                programmatic = Some(ProgrammaticDictation {
                                    id,
                                    owner_token,
                                    source,
                                    levels,
                                    audio: None,
                                    pressed_cold: release.press,
                                    last_level: Instant::now() - Duration::from_millis(30),
                                    lease_deadline: Instant::now() + LEASE,
                                });
                                let _ = reply.send(Ok(ProgrammaticDictationStart {
                                    id,
                                    sample_rate: input.sample_rate(),
                                }));
                            }
                            Err(error) => {
                                let _ = reply.send(Err(error.to_string()));
                            }
                        }
                    }
                    RecognitionControl::Dictation(DictationControl::AttachAudio {
                        id,
                        owner_token,
                        audio,
                        reply,
                    }) => {
                        let Some(active) = programmatic
                            .as_mut()
                            .filter(|active| active.matches_owner(id, &owner_token))
                        else {
                            let _ = reply.send(Err("dictation-not-active".into()));
                            continue;
                        };
                        if active.audio.is_some() {
                            let _ = reply.send(Err("audio-already-subscribed".into()));
                            continue;
                        }
                        active.audio = Some(audio);
                        let _ = reply.send(Ok(()));
                    }
                    RecognitionControl::Dictation(DictationControl::Finish {
                        id,
                        owner_token,
                        reply,
                    }) => {
                        if let Some(finishing) = programmatic_results
                            .values_mut()
                            .find(|finishing| finishing.matches_owner(id, &owner_token))
                        {
                            finishing.replies.push(reply);
                            continue;
                        }
                        if let Some(completion) = programmatic_completions
                            .iter()
                            .rev()
                            .find(|completion| completion.matches_owner(id, &owner_token))
                        {
                            let result = match &completion.outcome {
                                TerminalOutcome::Finished(result) => result.clone(),
                                TerminalOutcome::Cancelled => Err("cancelled".into()),
                            };
                            let _ = reply.send(result);
                            continue;
                        }
                        let (source, finish) = if let Some(lost) =
                            programmatic_lost.take_if(|lost| lost.matches_owner(id, &owner_token))
                        {
                            (lost.source, Finish::Transcribe(lost.clip))
                        } else if programmatic
                            .as_ref()
                            .is_some_and(|active| active.matches_owner(id, &owner_token))
                        {
                            let active = programmatic.take().expect("active dictation was checked");
                            let boundary = capture_boundary(&input, active.pressed_cold);
                            // The microphone may have failed before this
                            // finish reached the audio owner, with its loss
                            // still queued.
                            let finish = match input.finish(boundary)? {
                                Finish::Discard => match input.take_loss() {
                                    Some(LostCapture::Kept(clip)) => Finish::Transcribe(clip),
                                    Some(LostCapture::Empty) => {
                                        log_programmatic_ended(Some(active), "mic-lost");
                                        finish_with_result(
                                            id,
                                            owner_token,
                                            vec![reply],
                                            Err(MICROPHONE_LOST.into()),
                                            &mut programmatic_completions,
                                        );
                                        continue;
                                    }
                                    None => Finish::Discard,
                                },
                                finish => finish,
                            };
                            (active.source, finish)
                        } else {
                            let _ = reply.send(Err("dictation-not-active".into()));
                            continue;
                        };
                        match finish {
                            Finish::Transcribe(clip) => {
                                let duration_ms = clip.duration_ms();
                                match dictation_worker.transcribe(
                                    clip,
                                    TranscriptionTarget::Service,
                                    ContextSnapshot::default(),
                                    None,
                                ) {
                                    Ok(job_id) => {
                                        programmatic_results.insert(
                                            job_id,
                                            FinishingProgrammaticDictation {
                                                id,
                                                source: source.clone(),
                                                owner_token,
                                                duration_ms,
                                                replies: vec![reply],
                                            },
                                        );
                                        tracing::info!(
                                            dictation_id = id,
                                            source,
                                            "programmatic dictation submitted"
                                        );
                                    }
                                    Err(error) => {
                                        tracing::warn!(
                                            dictation_id = id,
                                            source,
                                            outcome = "worker-error",
                                            error = %error,
                                            "programmatic dictation ended"
                                        );
                                        finish_with_result(
                                            id,
                                            owner_token,
                                            vec![reply],
                                            Err(error.into()),
                                            &mut programmatic_completions,
                                        );
                                    }
                                }
                            }
                            Finish::Discard => {
                                tracing::warn!(
                                    dictation_id = id,
                                    source,
                                    outcome = "discarded",
                                    "programmatic dictation ended"
                                );
                                finish_with_result(
                                    id,
                                    owner_token,
                                    vec![reply],
                                    Err("capture-discarded".into()),
                                    &mut programmatic_completions,
                                );
                            }
                        }
                    }
                    RecognitionControl::Dictation(DictationControl::Cancel {
                        id,
                        owner_token,
                        reply,
                    }) => {
                        if let Some(completion) = programmatic_completions
                            .iter()
                            .rev()
                            .find(|completion| completion.matches_owner(id, &owner_token))
                            && matches!(completion.outcome, TerminalOutcome::Cancelled)
                        {
                            let _ = reply.send(Ok(()));
                            continue;
                        }
                        if programmatic_results
                            .values()
                            .any(|finishing| finishing.matches_owner(id, &owner_token))
                        {
                            let _ = reply.send(Err("dictation-finishing".into()));
                            continue;
                        }
                        if let Some(lost) =
                            programmatic_lost.take_if(|lost| lost.matches_owner(id, &owner_token))
                        {
                            tracing::info!(
                                dictation_id = id,
                                source = lost.source,
                                "programmatic dictation cancelled"
                            );
                            remember_completion(
                                &mut programmatic_completions,
                                Completion {
                                    id,
                                    owner_token,
                                    outcome: TerminalOutcome::Cancelled,
                                },
                            );
                            let _ = reply.send(Ok(()));
                            continue;
                        }
                        if programmatic
                            .as_ref()
                            .is_none_or(|active| !active.matches_owner(id, &owner_token))
                        {
                            let _ = reply.send(Err("dictation-not-active".into()));
                            continue;
                        }
                        let active = programmatic.take().expect("active dictation was checked");
                        tracing::info!(
                            dictation_id = id,
                            source = active.source,
                            "programmatic dictation cancelled"
                        );
                        cancel_programmatic_capture(active, &input, &mut programmatic_completions)?;
                        let _ = reply.send(Ok(()));
                    }
                    RecognitionControl::Dictation(DictationControl::Heartbeat {
                        id,
                        owner_token,
                        reply,
                    }) => {
                        let lease_deadline = if let Some(active) = programmatic
                            .as_mut()
                            .filter(|active| active.matches_owner(id, &owner_token))
                        {
                            &mut active.lease_deadline
                        } else if let Some(lost) = programmatic_lost
                            .as_mut()
                            .filter(|lost| lost.matches_owner(id, &owner_token))
                        {
                            &mut lost.lease_deadline
                        } else {
                            let _ = reply.send(Err("dictation-not-active".into()));
                            continue;
                        };
                        *lease_deadline = Instant::now() + LEASE;
                        let _ = reply.send(Ok(()));
                    }
                }
            }
        }
        if programmatic
            .as_ref()
            .is_some_and(|active| lease_expired(active.lease_deadline, Instant::now()))
        {
            let expired = programmatic.take().expect("expired dictation was checked");
            tracing::warn!(
                dictation_id = expired.id,
                source = expired.source,
                "programmatic dictation lease expired"
            );
            cancel_programmatic_capture(expired, &input, &mut programmatic_completions)?;
        }
        if let Some(expired) =
            programmatic_lost.take_if(|lost| lease_expired(lost.lease_deadline, Instant::now()))
        {
            tracing::warn!(
                dictation_id = expired.id,
                source = expired.source,
                "programmatic dictation lease expired"
            );
            remember_completion(
                &mut programmatic_completions,
                Completion {
                    id: expired.id,
                    owner_token: expired.owner_token,
                    outcome: TerminalOutcome::Cancelled,
                },
            );
        }
        context.drain(&mut events)?;
        if Instant::now() >= next_personal_host_check {
            next_personal_host_check = Instant::now() + Duration::from_millis(250);
            let next_personal_host_enabled = crate::personal_commands::host_needed(
                crate::app_settings::custom_transformations_selected(),
            );
            if next_personal_host_enabled != personal_host_enabled {
                personal_host_enabled = next_personal_host_enabled;
                personal_commands = personal_host_enabled
                    .then(|| {
                        crate::personal_commands::PersonalCommands::start(transformations.clone())
                    })
                    .flatten();
            }
        }
        if hotkeys.configure(CaptureInstant::now()) {
            handle_hotkey_action(
                HotkeyAction::Cancel,
                false,
                None,
                input.captured_through(),
                &input,
                &dictation_worker,
                &mut context,
                &input.device_name(),
                &mut events,
                indicator,
                &mut release,
            )?;
        }
        let (next_microphone_revision, microphone) = crate::app_settings::microphone_selection();
        if next_microphone_revision != microphone_revision {
            input.request_selection(next_microphone_revision, microphone.as_deref());
            microphone_revision = next_microphone_revision;
        }
        let (next_revision, selection) = crate::app_settings::transcription_selection();
        if next_revision != transcription_revision && dictation_worker.reload(selection).is_ok() {
            transcription_revision = next_revision;
        }
        let hotkey_capture_suspended = crate::shortcut_pause::tick() || input.is_recovering();
        if hotkeys.suspend_when(hotkey_capture_suspended) {
            handle_hotkey_action(
                HotkeyAction::Cancel,
                false,
                None,
                input.captured_through(),
                &input,
                &dictation_worker,
                &mut context,
                &input.device_name(),
                &mut events,
                indicator,
                &mut release,
            )?;
        }
        // Caps Lock presses arrive from the tap, off the audio and input
        // threads, with the physical press's own timestamp. Never start one
        // while a shortcut is recorded or a programmatic dictation owns the
        // microphone.
        while let Some(at) = caps_lock.try_recv_press() {
            // An Annotate capture recording never holds a press back: the
            // capture keeps recording beside the dictation.
            if hotkey_capture_suspended || programmatic.is_some() {
                continue;
            }
            let Some(action) = hotkeys.caps_lock_press() else {
                continue;
            };
            let (raw, print) = if action == HotkeyAction::Start {
                (false, None)
            } else {
                hotkeys.resolve_action(Shortcut::CapsLock, action)
            };
            let accepted = handle_hotkey_action(
                action,
                raw,
                print,
                at,
                &input,
                &dictation_worker,
                &mut context,
                &input.device_name(),
                &mut events,
                indicator,
                &mut release,
            )?;
            if !accepted {
                hotkeys.suspend(Shortcut::CapsLock);
            }
        }
        // Labs › Middle mouse dictation: the tap times the middle button, and
        // a press held half a second toggles hands-free dictation here, at
        // the moment it became long, under the same rules as Caps Lock.
        let middle_mouse_on = crate::labs::middle_mouse_dictation();
        if !middle_mouse_on {
            crate::middle_mouse_dictation::reset();
        } else if crate::middle_mouse_dictation::take_long_press(
            CaptureInstant::now(),
            crate::suppression::middle_button_down,
        ) && !hotkey_capture_suspended
            && programmatic.is_none()
            && let Some((shortcut, action)) = hotkeys.middle_mouse_press()
        {
            tracing::info!(action = ?action, "middle mouse long press");
            let (raw, print) = if action == HotkeyAction::Start {
                (false, None)
            } else {
                hotkeys.resolve_action(shortcut, action)
            };
            let accepted = handle_hotkey_action(
                action,
                raw,
                print,
                CaptureInstant::now(),
                &input,
                &dictation_worker,
                &mut context,
                &input.device_name(),
                &mut events,
                indicator,
                &mut release,
            )?;
            if !accepted {
                hotkeys.suspend(shortcut);
            }
        }
        // The capture shortcut ignores keys while shortcuts are suspended or
        // a programmatic dictation holds them, so their releases go unseen.
        #[cfg(hex_capture)]
        if hotkey_capture_suspended || programmatic.is_some() {
            capture_shortcut.reset();
        }
        while let Ok(observed) = input_monitor.events.try_recv() {
            let _acknowledge = input_monitor.acknowledge_after(observed);
            let input_event = observed.event;
            let capture_at = observed.capture_at;
            // Escape included: it belongs to the owner app (closing a
            // picker while a note records), which cancels through the API.
            if programmatic.is_some() {
                hotkeys.track_key_state(input_event, capture_at);
                continue;
            }
            if hotkey_capture_suspended {
                continue;
            }
            #[cfg(hex_capture)]
            if let Some(picker) = paste_lab.picker()
                && let Err(error) = picker.edge(input_event, capture_at, &dictation_worker)
            {
                events.dictation(DictationPhase::Failed(error.into()), "")?;
            }
            // Labs › YOLO mode: Escape after a release skips that
            // dictation's Return.
            if input_event.is_escape_down() {
                crate::dictation::yolo_mode::note_escape();
            }
            #[cfg(hex_capture)]
            if let Some(host) = capture_host.as_mut() {
                // Escape while drawing stops drawing, never the recording.
                if input_event.is_escape_down() && crate::capture_host::exit_drawing() {
                    continue;
                }
                let activity = host.shortcut_activity(capture_at, &input);
                let action = capture_shortcut.input(
                    input_event,
                    crate::app_settings::runtime_hotkeys(),
                    activity,
                );
                let swallow = matches!(
                    action,
                    crate::capture_host::ShortcutAction::Cancel
                        | crate::capture_host::ShortcutAction::ReplayCancel
                );
                host.shortcut_at(action, capture_at, &input);
                if swallow {
                    // Escape before the shortcut's keys came up (or while
                    // narrating a replay): nothing else hears it.
                    continue;
                }
            }
            // Escape closes the capsule's picker or open list (never an
            // Undo: that is a click). It never ends a dictation or a
            // recording capture: it may be meant for the app in front (#786).
            #[cfg(hex_capture)]
            if input_event.is_escape_down()
                && crate::capture_host::capsule_key(crate::capture_host::CapsuleKey::Escape)
            {
                continue;
            }
            // An Annotate capture never takes the dictation shortcut: it
            // dictates as usual while the capture records, the capture's own
            // narration hearing the same microphone timeline.
            for (shortcut, action) in hotkeys.process(input_event, capture_at) {
                let (raw, print) = hotkeys.resolve_action(shortcut, action);
                // Only a physical edge from the owner of a shortcut recording.
                // Control requests also use CancelShortcut, but never come here.
                #[cfg(hex_capture)]
                if action == HotkeyAction::CancelShortcut
                    && dictation_worker.started_by_shortcut()
                    && let Some(writer) = paste_lab.writer()
                {
                    writer.cancel(&input, capture_at)?;
                    context.cancel();
                    feedback::retract_start();
                    crate::screenshots::narration::ended();
                    feedback::play(Tone::Cancel);
                    report_capture_end(
                        CaptureEnd::Cancelled,
                        &input.device_name(),
                        &mut events,
                        indicator,
                    )?;
                    continue;
                }
                #[cfg(hex_capture)]
                if action == HotkeyAction::PasteLast
                    && let Some(picker) = paste_lab.picker()
                    && let Some(binding) = crate::app_settings::runtime_hotkeys().paste_last
                {
                    context.cancel();
                    feedback::retract_start();
                    input.cancel()?;
                    if let Err(error) = picker.press(binding, capture_at, &dictation_worker) {
                        events.dictation(DictationPhase::Failed(error.into()), "")?;
                    }
                    emit_engine_state(&mut events, false, &dictation_worker, &input.device_name())?;
                    continue;
                }
                let accepted = handle_hotkey_action(
                    action,
                    raw,
                    print,
                    capture_at,
                    &input,
                    &dictation_worker,
                    &mut context,
                    &input.device_name(),
                    &mut events,
                    indicator,
                    &mut release,
                )?;
                if !accepted {
                    hotkeys.suspend(shortcut);
                }
            }
        }
        if programmatic.is_none()
            && !hotkey_capture_suspended
            && input_monitor.pending_events().oldest().is_none()
        {
            hotkeys.recover_stale_keys();
        }
        if hotkeys.any_recording()
            && input.become_intentional(capture_boundary(&input, release.capture))?
        {
            // The press is a real capture now: read its page once, here
            // rather than on every provisional press. A tap this loop never
            // sees that way asks when it finishes instead. A capture pressed
            // while the microphone was released still owes its tone here,
            // even if the stream has come back since.
            context.intend();
            if release.capture {
                feedback::play(Tone::DictationStart);
            }
        }
        // Only once every queued edge is handled, so a release already seen
        // ends the press first.
        if !hotkey_capture_suspended && input_monitor.pending_events().oldest().is_none() {
            hotkeys.break_chords(CaptureInstant::now());
        }
        #[cfg(hex_capture)]
        if let Some(picker) = paste_lab.picker()
            && let Err(error) = picker.tick(
                paste_picker_paused(
                    hotkey_capture_suspended,
                    programmatic.is_some(),
                    hotkeys.any_recording(),
                ),
                &dictation_worker,
            )
        {
            events.dictation(DictationPhase::Failed(error.into()), "")?;
        }
        let mut received_worker_event = false;
        while let Some(event) = dictation_worker.try_recv() {
            received_worker_event = true;
            match event {
                WorkerEvent::Completed {
                    job_id,
                    target: TranscriptionTarget::Service,
                    result,
                    ..
                } => {
                    if let Some(finishing) = programmatic_results.remove(&job_id) {
                        match &result {
                            Ok(transcript) => tracing::info!(
                                dictation_id = finishing.id,
                                source = finishing.source,
                                outcome = "transcribed",
                                transcript_chars = transcript.chars().count(),
                                duration_ms = finishing.duration_ms,
                                "programmatic dictation ended"
                            ),
                            Err(error) => tracing::warn!(
                                dictation_id = finishing.id,
                                source = finishing.source,
                                outcome = "worker-error",
                                duration_ms = finishing.duration_ms,
                                error = %error,
                                "programmatic dictation ended"
                            ),
                        }
                        let result = result.map(|transcript| ProgrammaticDictationResult {
                            transcript,
                            duration_ms: finishing.duration_ms,
                        });
                        finish_with_result(
                            finishing.id,
                            finishing.owner_token,
                            finishing.replies,
                            result,
                            &mut programmatic_completions,
                        );
                    }
                    continue;
                }
                event => handle_dictation_event(event, &mut events, indicator)?,
            }
        }
        // A recording capture takes Escape only while it draws (Escape stops
        // drawing); otherwise only the capture shortcut's press before it
        // starts, and the capsule's picker.
        #[cfg(hex_capture)]
        let capture_escape = capture_host.is_some() && capture_shortcut.takes_escape();
        #[cfg(not(hex_capture))]
        let capture_escape = false;
        // A held Hold capture takes Escape once it is past the hold threshold;
        // Toggle's cancel key (and Caps Lock / extra rows' cancel keys) is
        // Hex's only while its recording runs. During a programmatic capture
        // the listener ignores every key, so the policy claims nothing.
        input_monitor.set_tap_policy(TapPolicy::for_listener(
            programmatic.is_some(),
            capture_escape,
            hotkeys.takes_escape(CaptureInstant::now()),
            hotkeys.recording_cancel(),
        ));
        if received_worker_event {
            emit_engine_state(
                &mut events,
                hotkeys.any_recording(),
                &dictation_worker,
                &input.device_name(),
            )?;
        }

        while let Some(audio_event) = input.try_recv_event() {
            match audio_event {
                DictationAudioEvent::ReadyIntentional { .. } => {
                    // A released microphone opened after the press already
                    // counted as intentional: read this capture's page now.
                    context.intend();
                    if input.is_recording() {
                        feedback::play(Tone::DictationStart);
                    }
                }
                DictationAudioEvent::OpenFailed { error, .. } => {
                    programmatic_microphone_lost(
                        programmatic.take(),
                        None,
                        "mic-open-failed",
                        &mut programmatic_lost,
                        &mut programmatic_completions,
                    );
                    hotkeys.suspend_all();
                    fail_capture(
                        format!("Could not open microphone: {error}"),
                        &dictation_worker,
                        &input.device_name(),
                        &mut events,
                        indicator,
                    )?;
                }
                DictationAudioEvent::CaptureDiscontinuity {
                    was_recording,
                    gap_ms,
                    kept,
                    ..
                } => {
                    if was_recording {
                        programmatic_microphone_lost(
                            programmatic.take(),
                            kept,
                            "mic-gap",
                            &mut programmatic_lost,
                            &mut programmatic_completions,
                        );
                        hotkeys.suspend_all();
                        fail_capture(
                            format!("Microphone audio was interrupted for {gap_ms} ms."),
                            &dictation_worker,
                            &input.device_name(),
                            &mut events,
                            indicator,
                        )?;
                    }
                }
                DictationAudioEvent::Reopened => {
                    if !input.is_recording() {
                        emit_state(&mut events, false, &input.device_name())?;
                    }
                }
                // Labs › Stop phrase: the capture heard its phrase twice and
                // ends as its shortcut's finishing press would.
                DictationAudioEvent::StopPhrase { phrase, .. } => {
                    stop_phrase::finish(
                        phrase,
                        &mut hotkeys,
                        &input,
                        &dictation_worker,
                        &mut context,
                        &mut events,
                        indicator,
                        release.capture,
                    )?;
                }
                DictationAudioEvent::Interrupted {
                    was_recording,
                    kept,
                    keep_hotkey,
                    ..
                } => {
                    if was_recording && keep_hotkey && kept.is_none() {
                        // The shortcut's finish already took the kept clip
                        // and submitted it; that capture is over.
                        continue;
                    }
                    // A kept clip still answers a print-only recording;
                    // `suspend_all` answers any reply whose recording is over.
                    let interrupted = was_recording.then(|| hotkeys.take_capture()).flatten();
                    let (raw, print) =
                        interrupted.map_or((false, None), |active| (active.raw, active.print));
                    hotkeys.suspend_all();
                    if was_recording {
                        // A recording shortcut keeps what it heard, copied
                        // instead of pasted; without a kept clip the previous
                        // failure stands.
                        let salvaged = match kept {
                            Some(clip) if keep_hotkey => Some((clip, context.finish(&mut events)?)),
                            kept => {
                                programmatic_microphone_lost(
                                    programmatic.take(),
                                    kept,
                                    "mic-interrupted",
                                    &mut programmatic_lost,
                                    &mut programmatic_completions,
                                );
                                None
                            }
                        };
                        interrupted_dictation(
                            salvaged,
                            print,
                            raw,
                            &dictation_worker,
                            &input.device_name(),
                            &mut events,
                            indicator,
                        )?;
                    }
                }
            }
        }

        // Every chunk already queued, not one per pass: a pass can take
        // longer than a chunk lasts, and a backlog that overflows leaves the
        // meter only stale audio.
        for audio in input.recv_ready(Duration::from_millis(20))? {
            if let Some(active) = &mut programmatic {
                if active.last_level.elapsed() >= Duration::from_millis(30) {
                    let _ = active.levels.try_send(level_for(&audio.samples));
                    active.last_level = Instant::now();
                }
                if let Some(sender) = &active.audio {
                    match sender.try_send(DictationAudioChunk {
                        samples: audio.samples.clone(),
                    }) {
                        Ok(()) | Err(TrySendError::Full(_)) => {}
                        Err(TrySendError::Disconnected(_)) => active.audio = None,
                    }
                }
            }
            if audio.is_current(input.recognition_generation())
                && (programmatic.is_some() || hotkeys.suppresses_recognition())
            {
                indicator.meter(&audio.samples);
            }
            // Annotate's narration feeds only the Caps Lock light, never the
            // dictation HUD; without this its recording light never leaves
            // Waiting.
            #[cfg(hex_capture)]
            if capture_was_recording
                && audio.is_current(input.recognition_generation())
                && let Some(event) = DictationIndicatorEvent::meter(&audio.samples)
            {
                indicator.caps_only(event);
            }
        }
    }

    #[cfg(hex_capture)]
    if let Some(host) = &mut capture_host {
        host.shutdown(&input);
    }
    drop(personal_commands);
    events.emit(&VoiceEvent::State {
        timestamp_ms: now_ms(),
        state: VoiceState::Stopping,
        device: input.device_name(),
    })?;
    events.flush()?;
    indicator.send(DictationIndicatorEvent::Discarded);
    Ok(())
}

fn log_programmatic_ended(active: Option<ProgrammaticDictation>, outcome: &str) {
    if let Some(active) = active {
        tracing::warn!(
            dictation_id = active.id,
            source = active.source,
            outcome,
            "programmatic dictation ended"
        );
    }
}

/// Ends a programmatic capture the microphone cut short. The audio delivered
/// before the loss waits for the owner's `/finish`; without any, `/finish`
/// answers `microphone-lost`.
fn programmatic_microphone_lost(
    active: Option<ProgrammaticDictation>,
    kept: Option<DictationClip>,
    outcome: &str,
    lost: &mut Option<LostProgrammaticDictation>,
    completions: &mut VecDeque<Completion>,
) {
    let Some(active) = active else {
        return;
    };
    let Some(mut clip) = kept else {
        remember_completion(
            completions,
            Completion {
                id: active.id,
                owner_token: active.owner_token.clone(),
                outcome: TerminalOutcome::Finished(Err(MICROPHONE_LOST.into())),
            },
        );
        log_programmatic_ended(Some(active), outcome);
        return;
    };
    // A held clip must not defer background work until the owner finishes.
    drop(clip.take_capture_marker());
    tracing::warn!(
        dictation_id = active.id,
        source = active.source,
        outcome,
        duration_ms = clip.duration_ms(),
        "programmatic dictation kept its audio after a microphone loss"
    );
    let previous = lost.replace(LostProgrammaticDictation {
        id: active.id,
        owner_token: active.owner_token,
        source: active.source,
        lease_deadline: active.lease_deadline,
        clip,
    });
    if let Some(previous) = previous {
        tracing::warn!(
            dictation_id = previous.id,
            source = previous.source,
            outcome = "superseded",
            "programmatic dictation ended"
        );
        remember_completion(
            completions,
            Completion {
                id: previous.id,
                owner_token: previous.owner_token,
                outcome: TerminalOutcome::Finished(Err(MICROPHONE_LOST.into())),
            },
        );
    }
}

fn cancel_programmatic_capture(
    active: ProgrammaticDictation,
    input: &DictationAudio,
    completions: &mut VecDeque<Completion>,
) -> Result<()> {
    input.cancel()?;
    remember_completion(
        completions,
        Completion {
            id: active.id,
            owner_token: active.owner_token,
            outcome: TerminalOutcome::Cancelled,
        },
    );
    Ok(())
}

fn fail_capture(
    message: String,
    worker: &DictationWorker,
    device: &str,
    events: &mut EventLog,
    indicator: DictationSink<'_>,
) -> Result<()> {
    feedback::play(Tone::Error);
    events.dictation(DictationPhase::Failed(message.clone()), "")?;
    indicator.send(DictationIndicatorEvent::Failed {
        message: Some(message),
    });
    emit_engine_state(events, false, worker, device)
}

#[derive(Clone, Copy)]
enum CaptureEnd {
    /// The capture never became a dictation; no feedback tone.
    Discarded(Option<DiscardCause>),
    /// The capture ended unfinished: the cancel shortcut, or Hex itself
    /// when its shortcuts were suspended or the event tap was lost.
    Cancelled,
}

/// Cancels the active capture before reporting the ending.
fn end_capture(
    end: CaptureEnd,
    dictation: &DictationAudio,
    device: &str,
    events: &mut EventLog,
    indicator: DictationSink<'_>,
) -> Result<()> {
    dictation.cancel()?;
    report_capture_end(end, device, events, indicator)
}

fn report_capture_end(
    end: CaptureEnd,
    device: &str,
    events: &mut EventLog,
    indicator: DictationSink<'_>,
) -> Result<()> {
    let indicator_event = match end {
        CaptureEnd::Discarded(cause) => {
            events.discarded_dictation(cause.map(DiscardCause::as_str))?;
            DictationIndicatorEvent::Discarded
        }
        CaptureEnd::Cancelled => {
            events.dictation(DictationPhase::Cancelled, "")?;
            DictationIndicatorEvent::Cancelled
        }
    };
    indicator.send(indicator_event);
    emit_state(events, false, device)
}

fn emit_state(events: &mut EventLog, suppressed: bool, device: &str) -> Result<()> {
    emit_voice_state(events, suppressed, false, device)
}

fn emit_engine_state(
    events: &mut EventLog,
    capturing: bool,
    worker: &DictationWorker,
    device: &str,
) -> Result<()> {
    emit_voice_state(events, capturing, worker.is_busy(), device)
}

fn emit_voice_state(
    events: &mut EventLog,
    capturing: bool,
    transcribing: bool,
    device: &str,
) -> Result<()> {
    events.emit(&VoiceEvent::State {
        timestamp_ms: now_ms(),
        state: if capturing {
            VoiceState::Dictating
        } else if transcribing {
            VoiceState::Transcribing
        } else {
            VoiceState::Listening
        },
        device: device.into(),
    })?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::app_settings::{
        CONTROL_KEY_MASK, ExtraDictationMode, HotkeyModifiers, MAX_EXTRA_DICTATION_SHORTCUTS,
        ModifierSide, OPTION_KEY_MASK, RuntimeHotkey, SHIFT_KEY_MASK,
    };
    use crate::recognition::guards::release_for_press;
    use crate::suppression::InputEvent;

    /// Labs › Second dictation shortcut (issue 627): each shortcut follows
    /// the same double-tap lock, the one that started a capture finishes
    /// it, and the other stands down while it records.
    #[test]
    fn the_shortcut_that_starts_a_capture_owns_it() {
        let base = CaptureInstant::from_nanos(60_000_000_000);
        let at = |ms: u64| base + Duration::from_millis(ms);
        let option = RuntimeHotkey {
            modifiers: HotkeyModifiers::option(),
            key_code: None,
        };
        let control = RuntimeHotkey {
            modifiers: HotkeyModifiers {
                control: Some(ModifierSide::Either),
                ..HotkeyModifiers::default()
            },
            key_code: None,
        };
        let mut hotkeys = Hotkeys {
            dictation: DictationHotkey::new(base, true, option),
            second: Some(DictationHotkey::new(base, true, control)),
            raw: None,
            extra: std::array::from_fn(|_| None),
            caps_lock: DictationHotkey::new(base, false, RuntimeHotkey::default()),
            active: None,
            suspended: false,
        };
        let mut press = |flags: u64, ms: u64| hotkeys.process(InputEvent::Flags(flags), at(ms));
        const START: HotkeyAction = HotkeyAction::Start;
        const FINISH: HotkeyAction = HotkeyAction::Finish;
        const DICTATION: Shortcut = Shortcut::Dictation;
        const SECOND: Shortcut = Shortcut::Second;

        // The second shortcut double-tap locks its own capture.
        assert_eq!(press(CONTROL_KEY_MASK, 0), [(SECOND, START)]);
        assert_eq!(press(0, 80), [(SECOND, FINISH)]);
        assert_eq!(press(CONTROL_KEY_MASK, 180), [(SECOND, START)]);
        assert!(press(0, 260).is_empty());
        // The dictation shortcut stands down while it records.
        assert!(press(OPTION_KEY_MASK, 1_000).is_empty());
        assert!(press(0, 1_100).is_empty());
        // Only the second shortcut finishes it.
        assert_eq!(press(CONTROL_KEY_MASK, 2_000), [(SECOND, FINISH)]);
        assert!(press(0, 2_100).is_empty());

        // And the reverse.
        assert_eq!(press(OPTION_KEY_MASK, 5_000), [(DICTATION, START)]);
        assert_eq!(press(0, 5_080), [(DICTATION, FINISH)]);
        assert_eq!(press(OPTION_KEY_MASK, 5_180), [(DICTATION, START)]);
        assert!(press(0, 5_260).is_empty());
        assert!(press(CONTROL_KEY_MASK, 6_000).is_empty());
        assert!(press(0, 6_100).is_empty());
        assert_eq!(press(OPTION_KEY_MASK, 7_000), [(DICTATION, FINISH)]);
        assert!(press(0, 7_100).is_empty());
        assert!(!hotkeys.any_recording());
    }

    /// A bound extra row's machine, as the listener configures it.
    fn extra_hotkeys(
        base: CaptureInstant,
        dictation: RuntimeHotkey,
        rows: &[(RuntimeHotkey, ExtraDictationMode)],
    ) -> Hotkeys {
        let mut bindings = [None; MAX_EXTRA_DICTATION_SHORTCUTS];
        for (index, (binding, _)) in rows.iter().enumerate() {
            bindings[index] = Some(*binding);
        }
        let mut hotkeys = Hotkeys {
            dictation: DictationHotkey::new(base, true, dictation),
            second: None,
            raw: None,
            extra: std::array::from_fn(|_| None),
            caps_lock: DictationHotkey::new(base, false, RuntimeHotkey::default()),
            active: None,
            suspended: false,
        };
        hotkeys.rebind_extra(bindings, base);
        let cancel = crate::app_settings::HotkeyBinding::cancel_default().runtime();
        for (index, (_, mode)) in rows.iter().enumerate() {
            let machine = hotkeys.extra[index].as_mut().expect("the row is bound");
            machine.set_trigger(*mode);
            machine.set_cancel(Some(cancel));
            machine.set_cancel_in_any_mode(true);
        }
        hotkeys
    }

    fn modifier_hotkey(modifiers: HotkeyModifiers) -> RuntimeHotkey {
        RuntimeHotkey {
            modifiers,
            key_code: None,
        }
    }

    fn shift_hotkey() -> RuntimeHotkey {
        modifier_hotkey(HotkeyModifiers {
            shift: Some(ModifierSide::Either),
            ..HotkeyModifiers::default()
        })
    }

    /// Labs › More dictation shortcuts: each extra row triggers its own way
    /// while the dictation shortcut keeps its global mode.
    #[test]
    fn each_extra_shortcut_keeps_its_own_mode() {
        let base = CaptureInstant::from_nanos(110_000_000_000);
        let at = |ms: u64| base + Duration::from_millis(ms);
        let option = modifier_hotkey(HotkeyModifiers::option());
        let control = modifier_hotkey(HotkeyModifiers::control());
        let mut hotkeys = extra_hotkeys(
            base,
            option,
            &[
                (control, ExtraDictationMode::Tap),
                (shift_hotkey(), ExtraDictationMode::Hold),
            ],
        );
        const EXTRA_TAP: Shortcut = Shortcut::Extra(0);
        const EXTRA_HOLD: Shortcut = Shortcut::Extra(1);
        const START: HotkeyAction = HotkeyAction::Start;
        const FINISH: HotkeyAction = HotkeyAction::Finish;
        let press = |hotkeys: &mut Hotkeys, flags: u64, ms: u64| {
            hotkeys.process(InputEvent::Flags(flags), at(ms))
        };

        // Tap starts hands-free on one press; the release keeps recording.
        assert_eq!(
            press(&mut hotkeys, CONTROL_KEY_MASK, 0),
            [(EXTRA_TAP, START)]
        );
        assert!(press(&mut hotkeys, 0, 80).is_empty());
        assert!(hotkeys.extra[0].as_ref().unwrap().is_recording());
        assert_eq!(
            press(&mut hotkeys, CONTROL_KEY_MASK, 200),
            [(EXTRA_TAP, FINISH)]
        );
        assert!(press(&mut hotkeys, 0, 280).is_empty());

        // Hold lets go to finish.
        assert_eq!(
            press(&mut hotkeys, SHIFT_KEY_MASK, 400),
            [(EXTRA_HOLD, START)]
        );
        assert_eq!(press(&mut hotkeys, 0, 480), [(EXTRA_HOLD, FINISH)]);

        // The dictation shortcut keeps its own hold behavior.
        const DICTATION: Shortcut = Shortcut::Dictation;
        assert_eq!(
            press(&mut hotkeys, OPTION_KEY_MASK, 600),
            [(DICTATION, START)]
        );
        assert_eq!(press(&mut hotkeys, 0, 680), [(DICTATION, FINISH)]);
        assert!(!hotkeys.any_recording());
    }

    /// An extra shortcut that starts a capture owns it: the other extras and
    /// the dictation shortcut stand down until it finishes.
    #[test]
    fn an_extra_shortcut_owns_the_capture_it_starts() {
        let base = CaptureInstant::from_nanos(120_000_000_000);
        let at = |ms: u64| base + Duration::from_millis(ms);
        let option = modifier_hotkey(HotkeyModifiers::option());
        let mut hotkeys = extra_hotkeys(
            base,
            option,
            &[
                (
                    modifier_hotkey(HotkeyModifiers::control()),
                    ExtraDictationMode::HoldOrDoubleTap,
                ),
                (shift_hotkey(), ExtraDictationMode::HoldOrDoubleTap),
            ],
        );
        const EXTRA0: Shortcut = Shortcut::Extra(0);
        const START: HotkeyAction = HotkeyAction::Start;
        const FINISH: HotkeyAction = HotkeyAction::Finish;
        let press = |hotkeys: &mut Hotkeys, flags: u64, ms: u64| {
            hotkeys.process(InputEvent::Flags(flags), at(ms))
        };

        // Double-tap it hands-free.
        assert_eq!(press(&mut hotkeys, CONTROL_KEY_MASK, 0), [(EXTRA0, START)]);
        assert_eq!(press(&mut hotkeys, 0, 80), [(EXTRA0, FINISH)]);
        assert_eq!(
            press(&mut hotkeys, CONTROL_KEY_MASK, 180),
            [(EXTRA0, START)]
        );
        assert!(press(&mut hotkeys, 0, 260).is_empty());
        assert!(hotkeys.extra[0].as_ref().unwrap().is_locked());

        // The other extra and the dictation shortcut stand down.
        assert!(press(&mut hotkeys, SHIFT_KEY_MASK, 400).is_empty());
        assert!(press(&mut hotkeys, 0, 480).is_empty());
        assert!(press(&mut hotkeys, OPTION_KEY_MASK, 600).is_empty());
        assert!(press(&mut hotkeys, 0, 680).is_empty());
        assert!(hotkeys.extra[0].as_ref().unwrap().is_recording());
        assert!(!hotkeys.extra[1].as_ref().unwrap().is_recording());
        assert!(!hotkeys.dictation.is_recording());

        // Only the shortcut that started it finishes it.
        assert_eq!(
            press(&mut hotkeys, CONTROL_KEY_MASK, 800),
            [(EXTRA0, FINISH)]
        );
        assert!(press(&mut hotkeys, 0, 880).is_empty());
        assert!(!hotkeys.any_recording());
    }

    /// Rebinding matches rows by binding, so adding or removing another row
    /// never cancels a recording extra's capture; dropping its own row does.
    #[test]
    fn removing_another_row_keeps_a_recording_extra() {
        let base = CaptureInstant::from_nanos(130_000_000_000);
        let at = |ms: u64| base + Duration::from_millis(ms);
        let option = modifier_hotkey(HotkeyModifiers::option());
        let control = modifier_hotkey(HotkeyModifiers::control());
        let shift = shift_hotkey();
        let mut hotkeys = extra_hotkeys(
            base,
            option,
            &[
                (control, ExtraDictationMode::Hold),
                (shift, ExtraDictationMode::Hold),
            ],
        );
        assert_eq!(
            hotkeys.process(InputEvent::Flags(CONTROL_KEY_MASK), at(0)),
            [(Shortcut::Extra(0), HotkeyAction::Start)]
        );

        // Adding a row before it moves the machine without dropping it.
        let mut added = [None; MAX_EXTRA_DICTATION_SHORTCUTS];
        added[0] = Some(shift);
        added[1] = Some(control);
        assert!(!hotkeys.rebind_extra(added, at(100)));
        assert!(hotkeys.extra[1].as_ref().unwrap().is_recording());

        // Removing the other row leaves it recording, in either position.
        let mut removed = [None; MAX_EXTRA_DICTATION_SHORTCUTS];
        removed[0] = Some(control);
        assert!(!hotkeys.rebind_extra(removed, at(200)));
        assert!(hotkeys.extra[0].as_ref().unwrap().is_recording());

        // Dropping the recording row reports the cancel the listener acts on.
        assert!(hotkeys.rebind_extra([None; MAX_EXTRA_DICTATION_SHORTCUTS], at(300)));
        assert!(!hotkeys.any_recording());
    }

    /// Escape cancels a recording extra in its own mode, locked included,
    /// and the Hold-mode dictation shortcut only while it is held past the
    /// hold threshold.
    #[test]
    fn escape_cancels_a_locked_extra_that_records() {
        let base = CaptureInstant::from_nanos(140_000_000_000);
        let at = |ms: u64| base + Duration::from_millis(ms);
        let option = modifier_hotkey(HotkeyModifiers::option());
        let mut hotkeys = extra_hotkeys(
            base,
            option,
            &[(
                modifier_hotkey(HotkeyModifiers::control()),
                ExtraDictationMode::HoldOrDoubleTap,
            )],
        );
        const EXTRA0: Shortcut = Shortcut::Extra(0);
        const START: HotkeyAction = HotkeyAction::Start;
        const FINISH: HotkeyAction = HotkeyAction::Finish;
        let press = |hotkeys: &mut Hotkeys, flags: u64, ms: u64| {
            hotkeys.process(InputEvent::Flags(flags), at(ms))
        };
        let escape = |hotkeys: &mut Hotkeys, down: bool, flags: u64, ms: u64| {
            hotkeys.process(
                InputEvent::Key {
                    code: 53,
                    down,
                    flags,
                },
                at(ms),
            )
        };

        // Double-tap the extra hands-free.
        assert_eq!(press(&mut hotkeys, CONTROL_KEY_MASK, 0), [(EXTRA0, START)]);
        assert_eq!(press(&mut hotkeys, 0, 80), [(EXTRA0, FINISH)]);
        assert_eq!(
            press(&mut hotkeys, CONTROL_KEY_MASK, 180),
            [(EXTRA0, START)]
        );
        assert!(press(&mut hotkeys, 0, 260).is_empty());
        assert!(hotkeys.extra[0].as_ref().unwrap().is_locked());
        assert_eq!(
            escape(&mut hotkeys, true, 0, 400),
            [(EXTRA0, HotkeyAction::CancelShortcut)]
        );
        assert!(escape(&mut hotkeys, false, 0, 500).is_empty());
        assert!(!hotkeys.extra[0].as_ref().unwrap().is_recording());

        // Hold dictation ignores Escape until the hold threshold makes the
        // press Hex's (it may be Option-Escape); then Escape cancels it, and
        // the tap keeps it from the app.
        assert_eq!(
            press(&mut hotkeys, OPTION_KEY_MASK, 600),
            [(Shortcut::Dictation, START)]
        );
        assert!(!hotkeys.takes_escape(at(680)));
        assert!(escape(&mut hotkeys, true, OPTION_KEY_MASK, 680).is_empty());
        assert!(escape(&mut hotkeys, false, OPTION_KEY_MASK, 700).is_empty());
        assert!(hotkeys.dictation.is_recording());
        assert!(hotkeys.takes_escape(at(900)));
        assert_eq!(
            escape(&mut hotkeys, true, OPTION_KEY_MASK, 1_000),
            [(Shortcut::Dictation, HotkeyAction::CancelShortcut)]
        );
        assert!(!hotkeys.dictation.is_recording());
        assert!(!hotkeys.takes_escape(at(1_000)));
    }

    fn caps_lock_hotkeys(
        base: CaptureInstant,
        dictation: crate::app_settings::RuntimeHotkey,
    ) -> Hotkeys {
        let mut caps_lock =
            DictationHotkey::new(base, false, crate::app_settings::RuntimeHotkey::default());
        caps_lock.set_toggle(true);
        caps_lock.set_cancel(Some(
            crate::app_settings::HotkeyBinding::cancel_default().runtime(),
        ));
        Hotkeys {
            dictation: DictationHotkey::new(base, false, dictation),
            second: None,
            raw: None,
            extra: std::array::from_fn(|_| None),
            caps_lock,
            active: None,
            suspended: false,
        }
    }

    fn option_binding() -> crate::app_settings::RuntimeHotkey {
        crate::app_settings::RuntimeHotkey {
            modifiers: crate::app_settings::HotkeyModifiers::option(),
            key_code: None,
        }
    }

    /// A released microphone's press takes wall-clock time; while it is being
    /// opened again a press keeps that, so it never anchors to the closed
    /// timeline's zero instant.
    #[test]
    fn a_press_stays_cold_until_the_warm_microphone_is_back() {
        // Warm, and cold once released.
        assert!(!release_for_press(false, false, true));
        assert!(release_for_press(true, false, true));
        // Off while the stream is still closed: still cold.
        assert!(release_for_press(false, true, false));
        // Back: the next press takes the stream's timeline again.
        assert!(!release_for_press(false, true, true));
        assert!(!release_for_press(false, false, false));
    }

    #[test]
    fn escape_cancels_a_caps_lock_capture_in_hold_mode() {
        let base = CaptureInstant::from_nanos(70_000_000_000);
        let mut hotkeys = caps_lock_hotkeys(base, option_binding());
        assert_eq!(hotkeys.caps_lock_press(), Some(HotkeyAction::Start));
        assert!(hotkeys.caps_lock.is_recording());

        let actions = hotkeys.process(
            InputEvent::Key {
                code: 53,
                down: true,
                flags: 0,
            },
            base + Duration::from_millis(500),
        );
        assert!(
            actions.contains(&(Shortcut::CapsLock, HotkeyAction::CancelShortcut)),
            "Hold mode still cancels a Caps Lock capture on Escape: {actions:?}"
        );
        assert!(!hotkeys.caps_lock.is_recording());
    }

    #[test]
    fn the_dictation_shortcut_stands_down_while_caps_lock_records() {
        let base = CaptureInstant::from_nanos(80_000_000_000);
        let mut hotkeys = caps_lock_hotkeys(base, option_binding());
        assert_eq!(hotkeys.caps_lock_press(), Some(HotkeyAction::Start));

        assert!(
            hotkeys
                .process(
                    InputEvent::Flags(crate::app_settings::OPTION_KEY_MASK),
                    base + Duration::from_millis(100),
                )
                .is_empty(),
            "the normal shortcut does not start or finish a Caps Lock capture"
        );
        assert!(hotkeys.caps_lock.is_recording());
        assert!(!hotkeys.dictation.is_recording());
    }

    #[test]
    fn turning_the_caps_lock_lab_off_stops_its_capture() {
        let base = CaptureInstant::from_nanos(90_000_000_000);
        let mut hotkeys = caps_lock_hotkeys(base, option_binding());
        assert_eq!(hotkeys.caps_lock_press(), Some(HotkeyAction::Start));
        assert!(hotkeys.stop_caps_lock());
        assert!(!hotkeys.caps_lock.is_recording());
        assert!(!hotkeys.stop_caps_lock(), "nothing is left to cancel");
    }

    /// Labs › Middle mouse dictation: a long press locks the dictation
    /// machine, so the dictation shortcut's next press finishes it.
    #[test]
    fn a_middle_mouse_capture_finishes_with_the_dictation_shortcut() {
        let base = CaptureInstant::from_nanos(95_000_000_000);
        let at = |ms: u64| base + Duration::from_millis(ms);
        let mut hotkeys = caps_lock_hotkeys(base, option_binding());
        assert_eq!(
            hotkeys.middle_mouse_press(),
            Some((Shortcut::Dictation, HotkeyAction::Start))
        );
        assert!(hotkeys.dictation.is_locked());
        assert_eq!(
            hotkeys.process(InputEvent::Flags(OPTION_KEY_MASK), at(1_000)),
            [(Shortcut::Dictation, HotkeyAction::Finish)]
        );
        let (raw, _) = hotkeys.resolve_action(Shortcut::Dictation, HotkeyAction::Finish);
        assert!(!raw);
        assert!(!hotkeys.any_recording());
    }

    /// And the reverse: a long press finishes the hands-free capture another
    /// input locked, as that machine's own press would, keeping its output.
    #[test]
    fn a_middle_mouse_press_finishes_a_hands_free_capture_it_did_not_start() {
        let base = CaptureInstant::from_nanos(96_000_000_000);
        let mut hotkeys = caps_lock_hotkeys(base, option_binding());
        hotkeys.raw = Some(DictationHotkey::new(base, false, option_binding()));
        assert!(hotkeys.raw.as_mut().is_some_and(DictationHotkey::lock));
        hotkeys.begin_capture(Shortcut::Raw, true);
        assert_eq!(
            hotkeys.middle_mouse_press(),
            Some((Shortcut::Raw, HotkeyAction::Finish))
        );
        let (raw, _) = hotkeys.resolve_action(Shortcut::Raw, HotkeyAction::Finish);
        assert!(raw, "the capture keeps Dictate without processing's output");
        assert!(!hotkeys.any_recording());
    }

    /// A long press never takes over the dictation shortcut's own pending
    /// double tap.
    #[test]
    fn a_middle_mouse_press_leaves_a_pending_double_tap_alone() {
        let base = CaptureInstant::from_nanos(99_000_000_000);
        let space = RuntimeHotkey {
            modifiers: HotkeyModifiers::option(),
            key_code: Some(49),
        };
        let mut hotkeys = caps_lock_hotkeys(base, space);
        hotkeys.dictation.set_double_tap_enabled(true);
        hotkeys.dictation.set_double_tap_only(true);
        let press = InputEvent::Key {
            code: 49,
            down: true,
            flags: OPTION_KEY_MASK,
        };
        assert!(hotkeys.process(press, base).is_empty());
        assert!(hotkeys.dictation.has_pending_gesture());
        assert_eq!(hotkeys.middle_mouse_press(), None);
        assert!(!hotkeys.dictation.is_locked());
    }

    /// A long press never disturbs a press still held.
    #[test]
    fn a_middle_mouse_press_leaves_a_held_capture_alone() {
        let base = CaptureInstant::from_nanos(97_000_000_000);
        let at = |ms: u64| base + Duration::from_millis(ms);
        let mut hotkeys = caps_lock_hotkeys(base, option_binding());
        assert_eq!(
            hotkeys.process(InputEvent::Flags(OPTION_KEY_MASK), at(0)),
            [(Shortcut::Dictation, HotkeyAction::Start)]
        );
        assert_eq!(hotkeys.middle_mouse_press(), None);
        assert!(hotkeys.dictation.is_recording());
        assert!(!hotkeys.dictation.is_locked());
    }

    /// With the lab on, Escape cancels a hands-free capture even in Hold
    /// mode, and the tap keeps it from the app; off, Hold mode's lock has no
    /// cancel, as before.
    #[test]
    fn escape_cancels_a_hands_free_capture_only_with_middle_mouse_dictation() {
        let base = CaptureInstant::from_nanos(98_000_000_000);
        let at = |ms: u64| base + Duration::from_millis(ms);
        let escape = InputEvent::Key {
            code: 53,
            down: true,
            flags: 0,
        };
        let mut hotkeys = caps_lock_hotkeys(base, option_binding());
        assert!(hotkeys.middle_mouse_press().is_some());
        assert!(!hotkeys.takes_escape(at(500)));
        assert!(hotkeys.process(escape, at(500)).is_empty());
        assert!(
            hotkeys
                .process(
                    InputEvent::Key {
                        code: 53,
                        down: false,
                        flags: 0,
                    },
                    at(550),
                )
                .is_empty()
        );
        assert!(hotkeys.dictation.is_locked());

        hotkeys.dictation.set_escape_cancels_lock(true);
        assert!(hotkeys.takes_escape(at(600)));
        assert_eq!(
            hotkeys.process(escape, at(600)),
            [(Shortcut::Dictation, HotkeyAction::CancelShortcut)]
        );
        assert!(!hotkeys.any_recording());
        assert!(!hotkeys.takes_escape(at(700)));
    }

    #[test]
    fn caps_lock_never_starts_over_a_pending_double_tap() {
        let base = CaptureInstant::from_nanos(100_000_000_000);
        let space = RuntimeHotkey {
            modifiers: HotkeyModifiers::option(),
            key_code: Some(49),
        };
        let mut hotkeys = caps_lock_hotkeys(base, space);
        hotkeys.dictation.set_double_tap_enabled(true);
        hotkeys.dictation.set_double_tap_only(true);
        assert!(
            hotkeys
                .process(
                    InputEvent::Key {
                        code: 49,
                        down: true,
                        flags: OPTION_KEY_MASK,
                    },
                    base,
                )
                .is_empty()
        );
        assert!(hotkeys.dictation.has_pending_gesture());
        assert_eq!(
            hotkeys.caps_lock_press(),
            None,
            "a Caps Lock start leaves the pending double tap alone"
        );
        assert!(!hotkeys.caps_lock.is_recording());
    }

    #[test]
    fn control_stops_reply_from_what_the_finish_did() {
        let code = |finished| match capture_actions::finish_reply(finished, "ok") {
            ControlReply::Done { changed: true, .. } => "done".to_owned(),
            ControlReply::Error { code, .. } => code,
            reply => panic!("unexpected {reply:?}"),
        };
        assert_eq!(code(Some(FinishResult::Submitted)), "done");
        assert_eq!(code(Some(FinishResult::Discarded)), "discarded");
        assert_eq!(code(Some(FinishResult::Silent)), "discarded");
        assert_eq!(code(None), "discarded");
        assert_eq!(code(Some(FinishResult::Rejected("full".into()))), "failed");
    }

    /// Paste Last's picker stands down for a suspension, a programmatic
    /// dictation, or a shortcut dictation, and for nothing else: an Annotate
    /// capture or a meeting recording isn't even an input, so a tap or a hold
    /// works over either and leaves it recording.
    #[cfg(hex_capture)]
    #[test]
    fn the_paste_picker_works_over_annotate_and_meetings() {
        assert!(!paste_picker_paused(false, false, false));
        assert!(paste_picker_paused(true, false, false));
        assert!(paste_picker_paused(false, true, false));
        assert!(paste_picker_paused(false, false, true));
    }

    #[test]
    fn paste_last_never_reaches_past_an_active_capture() {
        let busy = |reply: Option<ControlReply>| matches!(reply, Some(ControlReply::Error { code, .. }) if code == "busy");
        assert!(busy(paste_last_refusal(true)));
        assert!(paste_last_refusal(false).is_none());
    }

    #[test]
    fn a_capture_or_a_held_dictation_claims_escape() {
        // The capture picker would otherwise claim Escape that the listener
        // then ignores during a programmatic dictation.
        assert!(!TapPolicy::for_listener(true, true, false, None).takes_escape());
        assert!(!TapPolicy::for_listener(true, false, true, None).takes_escape());
        assert!(TapPolicy::for_listener(false, true, false, None).takes_escape());
        assert!(TapPolicy::for_listener(false, false, true, None).takes_escape());
        // A hands-free dictation never takes Escape from the app (#786).
        assert!(!TapPolicy::for_listener(false, false, false, None).takes_escape());
    }

    #[test]
    fn microphone_loss_keeps_a_programmatic_clip_for_finish() {
        let (levels, _levels) = std::sync::mpsc::sync_channel(1);
        let active = |id: u64| ProgrammaticDictation {
            id,
            owner_token: format!("owner-{id}"),
            source: "seer".into(),
            levels: levels.clone(),
            audio: None,
            pressed_cold: false,
            last_level: Instant::now(),
            lease_deadline: Instant::now() + LEASE,
        };
        let finished_error = |completions: &VecDeque<Completion>, id: u64| {
            completions
                .iter()
                .find(|completion| completion.matches_owner(id, &format!("owner-{id}")))
                .and_then(|completion| match &completion.outcome {
                    TerminalOutcome::Finished(Err(code)) => Some(code.clone()),
                    _ => None,
                })
        };
        let mut lost = None;
        let mut completions = VecDeque::new();

        // Audio delivered before the loss waits for the owner's finish.
        programmatic_microphone_lost(
            Some(active(1)),
            Some(DictationClip::for_test(vec![0.25; 16_000])),
            "mic-interrupted",
            &mut lost,
            &mut completions,
        );
        let kept = lost.as_ref().expect("the clip is kept");
        assert!(kept.matches_owner(1, "owner-1"));
        assert_eq!(kept.clip.duration_ms(), 1_000);
        assert!(completions.is_empty());

        // No audio: finish answers microphone-lost, and the kept clip stays.
        programmatic_microphone_lost(
            Some(active(2)),
            None,
            "mic-open-failed",
            &mut lost,
            &mut completions,
        );
        assert_eq!(
            finished_error(&completions, 2).as_deref(),
            Some(MICROPHONE_LOST)
        );
        assert!(lost.as_ref().is_some_and(|lost| lost.id == 1));

        // A later loss supersedes an unfinished kept clip.
        programmatic_microphone_lost(
            Some(active(3)),
            Some(DictationClip::for_test(vec![0.25; 1_600])),
            "mic-gap",
            &mut lost,
            &mut completions,
        );
        assert!(lost.as_ref().is_some_and(|lost| lost.id == 3));
        assert_eq!(
            finished_error(&completions, 1).as_deref(),
            Some(MICROPHONE_LOST)
        );

        // Nothing was recording programmatically: nothing changes.
        programmatic_microphone_lost(None, None, "mic-gap", &mut lost, &mut completions);
        assert_eq!(completions.len(), 2);
    }

    /// The light follows the microphone even when no HUD listens: a chunk's
    /// meter reaches the Caps Lock model through the sink, and a silent
    /// chunk leaves it waiting.
    #[test]
    fn a_meter_reaches_the_light_without_a_hud() {
        use crate::caps_lock_dictation::LedPattern;

        let caps_lock = crate::caps_lock_dictation::CapsLockDictation::for_test();
        let indicator = DictationSink {
            hud: None,
            caps_lock: &caps_lock,
        };
        indicator.send(DictationIndicatorEvent::Started);
        assert_eq!(
            caps_lock.pattern_for_test(),
            LedPattern::Off,
            "the light waits for the capture's first audio"
        );

        indicator.meter(&[0.5, -0.5]);
        assert_ne!(
            caps_lock.pattern_for_test(),
            LedPattern::Off,
            "the light takes the meter even with no HUD listening"
        );

        indicator.meter(&[]);
        assert_ne!(
            caps_lock.pattern_for_test(),
            LedPattern::Off,
            "a silent chunk turns nothing off"
        );
    }

    /// A refused Paste Last explains itself in the HUD, as a refused
    /// dictation paste does; another Paste Last failure only keeps its tone.
    #[test]
    fn a_refused_paste_last_reaches_the_hud_notice() {
        let caps_lock = crate::caps_lock_dictation::CapsLockDictation::for_test();
        let (hud, notices) = crate::dictation::indicator::channel();
        let path = std::env::temp_dir().join(format!(
            "hex-paste-last-refusal-{}-{}.ndjson",
            std::process::id(),
            now_ms()
        ));
        let mut events = EventLog::create(&path).unwrap();
        let refused = crate::paste::POST_EVENT_REFUSED.to_owned();
        handle_dictation_event(
            crate::parakeet::WorkerEvent::Pasted {
                kind: crate::parakeet::PasteKind::LastTranscript,
                result: Err(refused.clone()),
            },
            &mut events,
            DictationSink {
                hud: Some(&hud),
                caps_lock: &caps_lock,
            },
        )
        .unwrap();
        assert!(matches!(
            notices.try_recv(),
            Ok(DictationIndicatorEvent::Failed { message: Some(message) }) if message == refused
        ));
        handle_dictation_event(
            crate::parakeet::WorkerEvent::Pasted {
                kind: crate::parakeet::PasteKind::LastTranscript,
                result: Err("nothing to paste".into()),
            },
            &mut events,
            DictationSink {
                hud: Some(&hud),
                caps_lock: &caps_lock,
            },
        )
        .unwrap();
        assert!(
            notices.try_recv().is_err(),
            "another Paste Last failure keeps the tone only"
        );
    }

    /// With Release microphone while idle, a lost microphone lets the next
    /// press start at once, so a release and that press can both be queued
    /// before the listener reads the loss. The release's finish claims the
    /// audio the shortcut kept and copies it under its own capture's target;
    /// the press still records, and the old loss arrives stale.
    #[test]
    fn a_release_queued_behind_a_microphone_loss_copies_the_kept_audio_before_the_next_press() {
        let pressed_at = CaptureInstant::from_nanos(60_000_000_000);
        let (mut owner, input) =
            crate::dictation::audio::OwnerForTest::interrupted_hold(pressed_at);
        let path = std::env::temp_dir().join(format!(
            "hex-kept-hotkey-{}-{}.ndjson",
            std::process::id(),
            now_ms()
        ));
        let mut events = EventLog::create(&path).unwrap();
        let caps_lock = crate::caps_lock_dictation::CapsLockDictation::for_test();
        let indicator = DictationSink {
            hud: None,
            caps_lock: &caps_lock,
        };
        let released_at = pressed_at + Duration::from_millis(500);
        let next_press = released_at + Duration::from_millis(150);
        let mut submitted = Vec::new();
        std::thread::scope(|scope| {
            scope.spawn(|| owner.serve(2));
            // The queued edges in order: the release, then the next press.
            let finished = finish_dictation_with(
                &input,
                released_at,
                Duration::ZERO,
                |clip, salvaged| {
                    submitted.push((clip.duration_ms(), salvaged));
                    Ok(crate::parakeet::DictationJobId::for_test(1))
                },
                "Test microphone",
                &mut events,
                indicator,
            )
            .unwrap();
            assert!(matches!(finished, FinishResult::Submitted));
            assert!(input.start(next_press).unwrap());
        });
        let _ = std::fs::remove_file(&path);

        // Submitted once, as a salvage, with the 300 ms heard before the loss.
        let [(duration_ms, true)] = submitted[..] else {
            panic!("expected one salvaged submission, got {submitted:?}");
        };
        assert!((295..=305).contains(&duration_ms), "{duration_ms}");
        assert!(input.is_recording(), "the next press records");
        assert!(input.try_recv_event().is_none(), "the old loss is stale");
    }
}
