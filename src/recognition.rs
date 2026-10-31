use std::collections::{BTreeMap, VecDeque};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{Receiver, SyncSender, TrySendError};
use std::time::{Duration, Instant};

use color_eyre::Result;

use crate::audio::CaptureInstant;
use crate::context::{ContextMonitor, ContextSnapshot};
use crate::control::{CaptureAction, CaptureKind, CaptureSnapshot, ControlReply, PrintReply};
use crate::dictation::{DictationClip, Finish, POST_RELEASE_TAIL};
use crate::dictation_audio::{DictationAudio, DictationAudioEvent, LostCapture};
use crate::dictation_indicator::{DictationIndicatorEvent, DictationIndicatorSender};
use crate::events::{DictationPhase, EventLog, VoiceEvent, VoiceState, now_ms};
use crate::feedback::{self, Tone};
use crate::parakeet::{
    DictationJobId, DictationJobStage, DictationWorker, PasteKind, TranscriptionTarget, WorkerEvent,
};
use crate::recording_environment::RecordingEnvironmentController;
use crate::suppression::{DictationHotkey, DiscardCause, HotkeyAction, InputEvent, InputMonitor};

const PROGRAMMATIC_LEASE: Duration = Duration::from_secs(10);
const PROGRAMMATIC_COMPLETION_LIMIT: usize = 16;
/// `/finish` on a programmatic capture the microphone ended before any audio.
const MICROPHONE_LOST: &str = "microphone-lost";

enum FinishResult {
    Discarded,
    Silent,
    Submitted,
    Rejected(String),
}

#[derive(Debug)]
pub enum RecognitionControl {
    /// Pastes the last dictation once nothing records; `reply` answers
    /// `hex paste-last`, and the menu bar sends none.
    PasteLast {
        reply: Option<SyncSender<ControlReply>>,
    },
    /// `hex dictation`: drives the shortcut machine as a press would, so the
    /// shortcut and Escape still finish or cancel.
    /// A dictation `stop` with `print` answers `reply` once recording stops
    /// and sends the transcript to `print` instead of pasting it.
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
    StartDictation {
        source: String,
        owner_token: String,
        levels: SyncSender<DictationLevel>,
        reply: SyncSender<Result<ProgrammaticDictationStart, String>>,
    },
    AttachDictationAudio {
        id: u64,
        owner_token: String,
        audio: SyncSender<DictationAudioChunk>,
        reply: SyncSender<Result<(), String>>,
    },
    FinishDictation {
        id: u64,
        owner_token: String,
        reply: SyncSender<Result<ProgrammaticDictationResult, String>>,
    },
    CancelDictation {
        id: u64,
        owner_token: String,
        reply: SyncSender<Result<(), String>>,
    },
    HeartbeatDictation {
        id: u64,
        owner_token: String,
        reply: SyncSender<Result<(), String>>,
    },
}

#[derive(Clone, Copy, Debug)]
pub struct DictationLevel {
    pub rms_db: f32,
    pub peak_db: f32,
}

#[derive(Clone, Debug)]
pub struct ProgrammaticDictationResult {
    pub transcript: String,
    pub duration_ms: u64,
}

#[derive(Debug)]
pub struct ProgrammaticDictationStart {
    pub id: u64,
    pub sample_rate: u32,
}

#[derive(Debug)]
pub struct DictationAudioChunk {
    pub samples: Vec<f32>,
}

struct ProgrammaticDictation {
    id: u64,
    owner_token: String,
    source: String,
    levels: SyncSender<DictationLevel>,
    audio: Option<SyncSender<DictationAudioChunk>>,
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

#[derive(Clone)]
enum ProgrammaticTerminalOutcome {
    Finished(Result<ProgrammaticDictationResult, String>),
    Cancelled,
}

#[derive(Clone)]
struct ProgrammaticCompletion {
    id: u64,
    owner_token: String,
    outcome: ProgrammaticTerminalOutcome,
}

impl ProgrammaticCompletion {
    fn matches_owner(&self, id: u64, owner_token: &str) -> bool {
        self.id == id && self.owner_token == owner_token
    }
}

/// The dictation shortcut machines: dictation, and Dictate without
/// processing while it is bound. One records at a time.
struct Hotkeys {
    dictation: DictationHotkey,
    raw: Option<DictationHotkey>,
    /// The recording (or the last) skips processing: the raw shortcut or
    /// `hex dictation start --raw` started it.
    raw_capture: bool,
}

impl Hotkeys {
    fn machines(&mut self) -> impl Iterator<Item = &mut DictationHotkey> {
        std::iter::once(&mut self.dictation).chain(self.raw.as_mut())
    }

    fn any_recording(&self) -> bool {
        self.dictation.is_recording() || self.raw_recording()
    }

    fn raw_recording(&self) -> bool {
        self.raw.as_ref().is_some_and(DictationHotkey::is_recording)
    }

    fn suppresses_recognition(&self) -> bool {
        self.dictation.suppresses_recognition()
            || self
                .raw
                .as_ref()
                .is_some_and(DictationHotkey::suppresses_recognition)
    }

    /// Suspends every machine (never stopping at the first, as `any`
    /// would) and reports whether one was recording.
    fn suspend_all(&mut self) -> bool {
        self.machines().filter_map(DictationHotkey::suspend).count() > 0
    }

    fn disarm_pending_gestures(&mut self) {
        self.machines()
            .for_each(DictationHotkey::disarm_pending_gesture);
    }

    fn track_key_state(&mut self, event: InputEvent, at: CaptureInstant) {
        for machine in self.machines() {
            machine.track_key_state(event, at);
        }
    }

    fn recover_stale_keys(&mut self) {
        self.machines()
            .for_each(DictationHotkey::recover_stale_keys);
    }

    fn break_chords(&mut self, now: CaptureInstant) {
        for machine in self.machines() {
            machine.break_chord(now);
        }
    }

    /// Applies the saved shortcut options. Unbinding the raw shortcut while
    /// it records reports true: the caller cancels that recording.
    fn configure(&mut self, now: CaptureInstant) -> bool {
        let raw_binding = crate::app_settings::runtime_hotkeys().raw_dictation;
        let mut cancelled = false;
        match raw_binding {
            None => cancelled = self.raw.take().is_some_and(|raw| raw.is_recording()),
            Some(binding) => match self.raw.as_mut() {
                Some(raw) => raw.set_binding(binding),
                None => {
                    self.raw = Some(DictationHotkey::new(
                        now,
                        crate::app_settings::double_tap_lock(),
                        binding,
                    ));
                }
            },
        }
        self.dictation
            .set_binding(crate::app_settings::dictation_hotkey());
        for machine in self.machines() {
            machine.set_double_tap_enabled(crate::app_settings::double_tap_lock());
            machine.set_double_tap_only(crate::app_settings::double_tap_only());
            machine.set_toggle(crate::app_settings::shortcut_toggle());
        }
        cancelled
    }

    /// Feeds one input to both machines, and returns what each did with
    /// whether it was the raw machine. A machine that starts while the other
    /// records stands down until its keys come up, and only the dictation
    /// machine pastes.
    fn process(&mut self, event: InputEvent, at: CaptureInstant) -> Vec<(bool, HotkeyAction)> {
        let mut actions = Vec::new();
        let raw_recording = self.raw_recording();
        match self.dictation.process(event, at) {
            Some(HotkeyAction::Start) if raw_recording => {
                self.dictation.suspend();
            }
            Some(action) => actions.push((false, action)),
            None => {}
        }
        let dictation_recording = self.dictation.is_recording();
        if let Some(raw) = &mut self.raw {
            match raw.process(event, at) {
                Some(HotkeyAction::Start) if dictation_recording => {
                    raw.suspend();
                }
                Some(HotkeyAction::PasteLast | HotkeyAction::PasteMeeting) | None => {}
                Some(action) => actions.push((true, action)),
            }
        }
        actions
    }

    /// Stops whichever machine records, as a `stop` or `cancel` command does.
    fn suspend_recording(&mut self) {
        if self.dictation.is_recording() {
            self.dictation.suspend();
        }
        if let Some(raw) = self.raw.as_mut().filter(|raw| raw.is_recording()) {
            raw.suspend();
        }
    }
}

pub fn listen(
    events: EventLog,
    device_override: Option<&str>,
    shutdown: &AtomicBool,
    indicator: Option<DictationIndicatorSender>,
    history: Option<crate::history::History>,
    controls: Option<Receiver<RecognitionControl>>,
) -> Result<()> {
    let transformations = Arc::new(crate::personal_commands::TransformationClient::default());
    shutdown.store(false, Ordering::Relaxed);
    // Feedback admission is advisory: a cold audio stack can exceed the
    // preload timeout, and missing tones must not stop desktop recognition.
    if let Err(error) = feedback::preload() {
        tracing::warn!(%error, "recording sounds are unavailable; continuing without feedback");
    }
    let mut microphone_policy = crate::app_settings::microphone_policy();
    let mut events = events;
    let input_monitor = InputMonitor::start()?;
    let context_monitor = ContextMonitor::start();
    let mut context = ContextSnapshot::default();
    let mut hotkeys = Hotkeys {
        dictation: DictationHotkey::new(
            CaptureInstant::now(),
            crate::app_settings::double_tap_lock(),
            crate::app_settings::dictation_hotkey(),
        ),
        raw: None,
        raw_capture: false,
    };
    hotkeys.configure(CaptureInstant::now());
    let recording_environment = RecordingEnvironmentController::start();
    #[cfg(hex_capture)]
    let capture_environment = recording_environment.clone();
    let (mut microphone_revision, microphone) = crate::app_settings::microphone_selection();
    let dictation_worker = DictationWorker::start(
        input_monitor.activity.clone(),
        transformations.clone(),
        history,
    );
    let input = DictationAudio::open(
        device_override,
        microphone_revision,
        microphone.as_deref(),
        recording_environment,
        input_monitor.pending_events(),
        microphone_policy.release_while_idle,
        dictation_worker.engine(),
    )?;
    #[cfg(hex_capture)]
    let mut capture_host =
        crate::capture_host::CaptureHost::start(dictation_worker.engine(), &capture_environment);
    #[cfg(hex_capture)]
    let mut capture_shortcut = crate::capture_host::CaptureShortcut::default();
    let (mut transcription_revision, _) = crate::app_settings::transcription_selection();
    let mut personal_host_enabled = crate::app_settings::custom_transformations_enabled();
    let mut personal_commands = personal_host_enabled
        .then(|| crate::personal_commands::PersonalCommands::start(transformations.clone()))
        .flatten();
    let mut next_programmatic_id = 1_u64;
    let mut programmatic = None::<ProgrammaticDictation>;
    let mut programmatic_lost = None::<LostProgrammaticDictation>;
    let mut programmatic_results =
        BTreeMap::<crate::parakeet::DictationJobId, FinishingProgrammaticDictation>::new();
    let mut programmatic_completions = VecDeque::<ProgrammaticCompletion>::new();

    events.emit(&VoiceEvent::SessionStarted {
        timestamp_ms: now_ms(),
    })?;
    if input.is_recovering() {
        hotkeys.suspend_all();
    }
    if hotkeys.dictation.is_recording() {
        let accepted = input.start(capture_boundary(
            &input,
            microphone_policy.release_while_idle,
        ))?;
        if accepted {
            events.dictation(DictationPhase::Started, "")?;
            if let Some(indicator) = &indicator {
                indicator.send(DictationIndicatorEvent::Started);
            }
        } else {
            hotkeys.dictation.suspend();
        }
    }
    emit_state(&mut events, hotkeys.any_recording(), &input.device_name())?;
    tracing::info!(device = %input.device_name(), "dictation listener started");

    while !shutdown.load(Ordering::Relaxed) {
        #[cfg(hex_capture)]
        crate::capture_host::CaptureHost::reconcile(
            &mut capture_host,
            dictation_worker.engine().as_ref(),
            &capture_environment,
            &input,
        );
        if let Some(controls) = &controls {
            while let Ok(control) = controls.try_recv() {
                match control {
                    RecognitionControl::PasteLast { reply } => {
                        let capture_active = programmatic.is_some()
                            || hotkeys.suppresses_recognition()
                            || input.is_recording();
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
                        let release_while_idle = microphone_policy.release_while_idle;
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
                            &context,
                            &mut events,
                            indicator.as_ref(),
                            release_while_idle,
                        )?;
                        let _ = reply.send(outcome);
                    }
                    RecognitionControl::StartDictation {
                        source,
                        owner_token,
                        levels,
                        reply,
                    } => {
                        if programmatic.is_some() || input.is_recording() || hotkeys.any_recording()
                        {
                            let _ = reply.send(Err("dictation-busy".into()));
                            continue;
                        }
                        let id = next_programmatic_id;
                        next_programmatic_id = next_programmatic_id.wrapping_add(1).max(1);
                        let boundary =
                            capture_boundary(&input, microphone_policy.release_while_idle);
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
                                    last_level: Instant::now() - Duration::from_millis(30),
                                    lease_deadline: Instant::now() + PROGRAMMATIC_LEASE,
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
                    RecognitionControl::AttachDictationAudio {
                        id,
                        owner_token,
                        audio,
                        reply,
                    } => {
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
                    RecognitionControl::FinishDictation {
                        id,
                        owner_token,
                        reply,
                    } => {
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
                                ProgrammaticTerminalOutcome::Finished(result) => result.clone(),
                                ProgrammaticTerminalOutcome::Cancelled => Err("cancelled".into()),
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
                            let boundary =
                                capture_boundary(&input, microphone_policy.release_while_idle);
                            // The microphone may have failed before this
                            // finish reached the audio owner, with its loss
                            // still queued.
                            let finish = match input.finish(boundary)? {
                                Finish::Discard => match input.take_loss() {
                                    Some(LostCapture::Kept(clip)) => Finish::Transcribe(clip),
                                    Some(LostCapture::Empty) => {
                                        log_programmatic_ended(Some(active), "mic-lost");
                                        finish_programmatic_with_result(
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
                                        finish_programmatic_with_result(
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
                                finish_programmatic_with_result(
                                    id,
                                    owner_token,
                                    vec![reply],
                                    Err("capture-discarded".into()),
                                    &mut programmatic_completions,
                                );
                            }
                        }
                    }
                    RecognitionControl::CancelDictation {
                        id,
                        owner_token,
                        reply,
                    } => {
                        if let Some(completion) = programmatic_completions
                            .iter()
                            .rev()
                            .find(|completion| completion.matches_owner(id, &owner_token))
                            && matches!(completion.outcome, ProgrammaticTerminalOutcome::Cancelled)
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
                            remember_programmatic_completion(
                                &mut programmatic_completions,
                                ProgrammaticCompletion {
                                    id,
                                    owner_token,
                                    outcome: ProgrammaticTerminalOutcome::Cancelled,
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
                    RecognitionControl::HeartbeatDictation {
                        id,
                        owner_token,
                        reply,
                    } => {
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
                        *lease_deadline = Instant::now() + PROGRAMMATIC_LEASE;
                        let _ = reply.send(Ok(()));
                    }
                }
            }
        }
        if programmatic
            .as_ref()
            .is_some_and(|active| programmatic_lease_expired(active.lease_deadline, Instant::now()))
        {
            let expired = programmatic.take().expect("expired dictation was checked");
            tracing::warn!(
                dictation_id = expired.id,
                source = expired.source,
                "programmatic dictation lease expired"
            );
            cancel_programmatic_capture(expired, &input, &mut programmatic_completions)?;
        }
        if let Some(expired) = programmatic_lost
            .take_if(|lost| programmatic_lease_expired(lost.lease_deadline, Instant::now()))
        {
            tracing::warn!(
                dictation_id = expired.id,
                source = expired.source,
                "programmatic dictation lease expired"
            );
            remember_programmatic_completion(
                &mut programmatic_completions,
                ProgrammaticCompletion {
                    id: expired.id,
                    owner_token: expired.owner_token,
                    outcome: ProgrammaticTerminalOutcome::Cancelled,
                },
            );
        }
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
        let next_microphone_policy = crate::app_settings::microphone_policy();
        if next_microphone_policy.release_while_idle != microphone_policy.release_while_idle {
            input.set_release_while_idle(next_microphone_policy.release_while_idle);
        }
        microphone_policy = next_microphone_policy;
        let next_personal_host_enabled = crate::app_settings::custom_transformations_enabled();
        if next_personal_host_enabled != personal_host_enabled {
            personal_host_enabled = next_personal_host_enabled;
            personal_commands = personal_host_enabled
                .then(|| crate::personal_commands::PersonalCommands::start(transformations.clone()))
                .flatten();
        }
        if hotkeys.configure(CaptureInstant::now()) {
            handle_hotkey_action(
                HotkeyAction::Cancel,
                false,
                input.captured_through(),
                &input,
                &dictation_worker,
                &context,
                &input.device_name(),
                &mut events,
                indicator.as_ref(),
                microphone_policy.release_while_idle,
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
        let hotkey_capture_active = crate::app_settings::hotkey_capture_active();
        let hotkey_capture_suspended = hotkey_capture_active || input.is_recovering();
        if hotkey_capture_suspended && hotkeys.suspend_all() {
            handle_hotkey_action(
                HotkeyAction::Cancel,
                false,
                input.captured_through(),
                &input,
                &dictation_worker,
                &context,
                &input.device_name(),
                &mut events,
                indicator.as_ref(),
                microphone_policy.release_while_idle,
            )?;
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
                if action == crate::capture_host::ShortcutAction::Cancel {
                    // Escape before the shortcut's keys came up: nothing else
                    // hears it.
                    continue;
                }
                host.shortcut(action, &input);
            }
            // Escape ends a dictation hold first, then
            // closes the capsule's picker or open list (never an Undo: that is a
            // click), then cancels queued
            // dictation. It never ends a recording capture: it may be meant
            // for the app being recorded (the capsule's ✕ discards).
            #[cfg(hex_capture)]
            if !hotkeys.any_recording()
                && input_event.is_escape_down()
                && crate::capture_host::capsule_key(crate::capture_host::CapsuleKey::Escape)
            {
                continue;
            }
            if !hotkeys.any_recording()
                && input_event.is_escape_down()
                && cancel_latest_job(&dictation_worker, &mut events, indicator.as_ref())?
            {
                continue;
            }
            // Annotate records the narration itself: while it records, the
            // dictation shortcut starts nothing. The tap still swallows its
            // chord, so it never reaches the app.
            #[cfg(hex_capture)]
            if !hotkeys.any_recording()
                && capture_host
                    .as_mut()
                    .is_some_and(|host| host.recording_since(&input).is_some())
            {
                hotkeys.track_key_state(input_event, capture_at);
                continue;
            }
            for (raw, action) in hotkeys.process(input_event, capture_at) {
                if action == HotkeyAction::Start {
                    hotkeys.raw_capture = raw;
                }
                let accepted = handle_hotkey_action(
                    action,
                    hotkeys.raw_capture,
                    capture_at,
                    &input,
                    &dictation_worker,
                    &context,
                    &input.device_name(),
                    &mut events,
                    indicator.as_ref(),
                    microphone_policy.release_while_idle,
                )?;
                if !accepted {
                    match &mut hotkeys.raw {
                        Some(machine) if raw => machine.suspend(),
                        _ => hotkeys.dictation.suspend(),
                    };
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
            && input.become_intentional(capture_boundary(
                &input,
                microphone_policy.release_while_idle,
            ))?
            && microphone_policy.release_while_idle
        {
            feedback::play(Tone::DictationStart);
        }
        // Only once every queued edge is handled, so a release already seen
        // ends the press first.
        if !hotkey_capture_suspended && input_monitor.pending_events().oldest().is_none() {
            hotkeys.break_chords(CaptureInstant::now());
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
                        finish_programmatic_with_result(
                            finishing.id,
                            finishing.owner_token,
                            finishing.replies,
                            result,
                            &mut programmatic_completions,
                        );
                    }
                    continue;
                }
                WorkerEvent::Cancelled { job_id } => {
                    if let Some(finishing) = programmatic_results.remove(&job_id) {
                        finish_programmatic_with_result(
                            finishing.id,
                            finishing.owner_token,
                            finishing.replies,
                            Err("cancelled".into()),
                            &mut programmatic_completions,
                        );
                        continue;
                    }
                    handle_dictation_event(
                        WorkerEvent::Cancelled { job_id },
                        &mut events,
                        indicator.as_ref(),
                    )?;
                    continue;
                }
                event => handle_dictation_event(event, &mut events, indicator.as_ref())?,
            }
        }
        // A recording capture takes Escape only while it draws (Escape stops
        // drawing); otherwise only the capture shortcut's press before it
        // starts, and the capsule's picker.
        #[cfg(hex_capture)]
        let capture_escape = capture_host.is_some() && capture_shortcut.takes_escape();
        #[cfg(not(hex_capture))]
        let capture_escape = false;
        input_monitor.set_escape_cancels(claims_escape(
            programmatic.is_some(),
            hotkeys.any_recording(),
            capture_escape,
            || dictation_worker.has_escape_cancellable_job(),
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
                        indicator.as_ref(),
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
                            indicator.as_ref(),
                        )?;
                    }
                }
                DictationAudioEvent::Reopened => {
                    if !input.is_recording() {
                        emit_state(&mut events, false, &input.device_name())?;
                    }
                }
                DictationAudioEvent::Interrupted {
                    was_recording,
                    kept,
                    ..
                } => {
                    if was_recording {
                        programmatic_microphone_lost(
                            programmatic.take(),
                            kept,
                            "mic-interrupted",
                            &mut programmatic_lost,
                            &mut programmatic_completions,
                        );
                    }
                    hotkeys.suspend_all();
                    if was_recording {
                        fail_capture(
                            "Microphone capture was interrupted; reconnecting.".into(),
                            &dictation_worker,
                            &input.device_name(),
                            &mut events,
                            indicator.as_ref(),
                        )?;
                    }
                }
            }
        }

        let audio = input.recv_timeout(Duration::from_millis(20))?;
        if let (Some(active), Some(audio)) = (&mut programmatic, &audio) {
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
        if let Some(audio) = audio.filter(|audio| audio.is_current(input.recognition_generation()))
            && (programmatic.is_some() || hotkeys.suppresses_recognition())
            && let Some(indicator) = &indicator
        {
            indicator.meter(&audio.samples);
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
    if let Some(indicator) = &indicator {
        indicator.send(DictationIndicatorEvent::Discarded);
    }
    Ok(())
}

fn level_for(samples: &[f32]) -> DictationLevel {
    let (sum_squares, peak) = samples
        .iter()
        .fold((0.0_f64, 0.0_f32), |(sum, peak), sample| {
            let magnitude = sample.abs();
            (
                sum + f64::from(*sample) * f64::from(*sample),
                peak.max(magnitude),
            )
        });
    let rms = if samples.is_empty() {
        0.0
    } else {
        (sum_squares / samples.len() as f64).sqrt() as f32
    };
    DictationLevel {
        rms_db: amplitude_db(rms),
        peak_db: amplitude_db(peak),
    }
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
    completions: &mut VecDeque<ProgrammaticCompletion>,
) {
    let Some(active) = active else {
        return;
    };
    let Some(mut clip) = kept else {
        remember_programmatic_completion(
            completions,
            ProgrammaticCompletion {
                id: active.id,
                owner_token: active.owner_token.clone(),
                outcome: ProgrammaticTerminalOutcome::Finished(Err(MICROPHONE_LOST.into())),
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
        remember_programmatic_completion(
            completions,
            ProgrammaticCompletion {
                id: previous.id,
                owner_token: previous.owner_token,
                outcome: ProgrammaticTerminalOutcome::Finished(Err(MICROPHONE_LOST.into())),
            },
        );
    }
}

fn remember_programmatic_completion(
    completions: &mut VecDeque<ProgrammaticCompletion>,
    completion: ProgrammaticCompletion,
) {
    completions.push_back(completion);
    while completions.len() > PROGRAMMATIC_COMPLETION_LIMIT {
        completions.pop_front();
    }
}

fn finish_programmatic_with_result(
    id: u64,
    owner_token: String,
    replies: Vec<SyncSender<Result<ProgrammaticDictationResult, String>>>,
    result: Result<ProgrammaticDictationResult, String>,
    completions: &mut VecDeque<ProgrammaticCompletion>,
) {
    for reply in replies {
        let _ = reply.send(result.clone());
    }
    remember_programmatic_completion(
        completions,
        ProgrammaticCompletion {
            id,
            owner_token,
            outcome: ProgrammaticTerminalOutcome::Finished(result),
        },
    );
}

fn cancel_programmatic_capture(
    active: ProgrammaticDictation,
    input: &DictationAudio,
    completions: &mut VecDeque<ProgrammaticCompletion>,
) -> Result<()> {
    input.cancel()?;
    remember_programmatic_completion(
        completions,
        ProgrammaticCompletion {
            id: active.id,
            owner_token: active.owner_token,
            outcome: ProgrammaticTerminalOutcome::Cancelled,
        },
    );
    Ok(())
}

fn programmatic_lease_expired(deadline: Instant, now: Instant) -> bool {
    now >= deadline
}

fn amplitude_db(amplitude: f32) -> f32 {
    (20.0 * amplitude.max(0.000_001).log10()).max(-120.0)
}

#[allow(clippy::too_many_arguments)]
fn handle_hotkey_action(
    action: HotkeyAction,
    raw: bool,
    action_at: CaptureInstant,
    dictation: &DictationAudio,
    worker: &DictationWorker,
    context: &ContextSnapshot,
    device: &str,
    events: &mut EventLog,
    indicator: Option<&DictationIndicatorSender>,
    release_while_idle: bool,
) -> Result<bool> {
    match action {
        HotkeyAction::Start => {
            if !dictation.start(action_at)? {
                return Ok(false);
            }
            // The tone confirms the press at once; a chord or short tap
            // retracts it. A released microphone confirms only once it opens.
            if release_while_idle {
                feedback::prepare();
            } else {
                feedback::play_provisional_start();
            }
            worker.prepare_paste();
            events.dictation(DictationPhase::Started, "")?;
            if let Some(indicator) = indicator {
                indicator.send(DictationIndicatorEvent::Started);
            }
            emit_state(events, true, device)
        }
        HotkeyAction::Finish => finish_hotkey_dictation(
            action_at,
            dictation,
            worker,
            context,
            device,
            events,
            indicator,
            release_while_idle,
            None,
            raw,
        )
        .map(drop),
        HotkeyAction::Discard(_) | HotkeyAction::Cancel => {
            feedback::retract_start();
            end_capture(
                if matches!(action, HotkeyAction::Cancel) {
                    CaptureEnd::Cancelled
                } else {
                    CaptureEnd::Discarded(action.discard_cause())
                },
                dictation,
                device,
                events,
                indicator,
            )
        }
        HotkeyAction::PasteLast | HotkeyAction::PasteMeeting => {
            // The paste chord's modifier press started a provisional capture.
            feedback::retract_start();
            dictation.cancel()?;
            let paste_result = match action {
                HotkeyAction::PasteLast => worker.paste_last(),
                HotkeyAction::PasteMeeting => worker.paste_meeting(),
                _ => unreachable!(),
            };
            if let Err(error) = paste_result {
                feedback::play(Tone::Error);
                events.dictation(DictationPhase::Failed(error.into()), "")?;
            }
            emit_engine_state(events, false, worker, device)
        }
    }?;
    Ok(true)
}

/// Ends a shortcut dictation at `action_at` and hands it to transcription.
#[allow(clippy::too_many_arguments)]
fn finish_hotkey_dictation(
    action_at: CaptureInstant,
    dictation: &DictationAudio,
    worker: &DictationWorker,
    context: &ContextSnapshot,
    device: &str,
    events: &mut EventLog,
    indicator: Option<&DictationIndicatorSender>,
    release_while_idle: bool,
    print: Option<PrintReply>,
    raw: bool,
) -> Result<FinishResult> {
    if dictation.become_intentional(action_at)? && release_while_idle {
        feedback::play(Tone::DictationStart);
    }
    let finished = match print {
        None => finish_dictation(
            dictation,
            action_at,
            POST_RELEASE_TAIL,
            worker,
            if raw {
                TranscriptionTarget::RawPaste
            } else {
                TranscriptionTarget::Paste
            },
            context,
            device,
            events,
            indicator,
        )?,
        Some(print) => finish_dictation_with(
            dictation,
            action_at,
            POST_RELEASE_TAIL,
            |clip| worker.transcribe_printed(clip, print, raw, context.clone()),
            device,
            events,
            indicator,
        )?,
    };
    if matches!(finished, FinishResult::Discarded) {
        feedback::retract_start();
    }
    emit_engine_state(events, false, worker, device)?;
    Ok(finished)
}

/// Whether Hex takes Escape from the app in front. Never while a programmatic
/// dictation records: the listener ignores every key then, so Escape stays
/// with its owner app rather than being swallowed.
fn claims_escape(
    programmatic: bool,
    recording: bool,
    capture_escape: bool,
    cancellable_job: impl FnOnce() -> bool,
) -> bool {
    !programmatic && (recording || capture_escape || cancellable_job())
}

/// Cancels the newest unfinished dictation, as Escape does while idle.
fn cancel_latest_job(
    worker: &DictationWorker,
    events: &mut EventLog,
    indicator: Option<&DictationIndicatorSender>,
) -> Result<bool> {
    let Some(job_id) = worker.cancel_latest() else {
        return Ok(false);
    };
    feedback::play(Tone::Cancel);
    events.dictation(DictationPhase::Cancelled, "")?;
    if let Some(indicator) = indicator {
        indicator.send(DictationIndicatorEvent::JobCancelled {
            job_id: job_id.value(),
        });
    }
    Ok(true)
}

/// Why a control can't start a capture now, if it can't.
fn capture_refusal(other_recording: bool, suspended: bool) -> Option<ControlReply> {
    if other_recording {
        Some(ControlReply::error(
            "busy",
            "another recording is in progress; finish or cancel it first",
        ))
    } else if suspended {
        Some(ControlReply::error(
            "busy",
            "Hex is recording a shortcut in Settings or reopening the microphone; try again",
        ))
    } else {
        None
    }
}

/// A `cancel` with its own capture idle. Escape cancels the active capture
/// before any unfinished job, so while another capture records this refuses
/// rather than reach past it to a job.
fn idle_cancel_refusal(other_recording: bool) -> Option<ControlReply> {
    other_recording.then(|| {
        ControlReply::error(
            "busy",
            "another recording is in progress; cancel it first (Escape cancels it)",
        )
    })
}

/// A control start has no press to judge, so its capture counts as
/// intentional from its first sample: the recording environment starts and
/// the live stream opens now. `tone_owed` plays the start tone for a capture
/// that's already open; one still opening the microphone plays it on
/// `ReadyIntentional`.
fn make_control_capture_intentional(input: &DictationAudio, tone_owed: bool) -> Result<()> {
    if input.mark_intentional()? && tone_owed {
        feedback::play(Tone::DictationStart);
    }
    Ok(())
}

/// The reply to a control `stop`: only a submitted clip transcribes.
fn finish_reply(finished: Option<FinishResult>, submitted: &str) -> ControlReply {
    match finished {
        Some(FinishResult::Submitted) => ControlReply::done(submitted),
        Some(FinishResult::Discarded) | None => ControlReply::error(
            "discarded",
            "too short, discarded: no audio was recorded yet (the microphone may still have \
             been opening)",
        ),
        Some(FinishResult::Silent) => ControlReply::error(
            "discarded",
            "the recording was silent, discarded; nothing to transcribe",
        ),
        Some(FinishResult::Rejected(error)) => {
            ControlReply::error("failed", format!("couldn't transcribe it: {error}"))
        }
    }
}

/// `hex dictation`: a start locks the shortcut machine as a double tap
/// does, so the next press or `stop` finishes it and Escape cancels it.
#[allow(clippy::too_many_arguments)]
fn control_dictation(
    action: CaptureAction,
    print: Option<PrintReply>,
    raw: bool,
    other_recording: bool,
    suspended: bool,
    hotkeys: &mut Hotkeys,
    input: &DictationAudio,
    worker: &DictationWorker,
    context: &ContextSnapshot,
    events: &mut EventLog,
    indicator: Option<&DictationIndicatorSender>,
    release_while_idle: bool,
) -> Result<ControlReply> {
    let recording = hotkeys.any_recording();
    let device = input.device_name();
    let action = match action {
        CaptureAction::Toggle if recording => CaptureAction::Stop,
        CaptureAction::Toggle => CaptureAction::Start,
        action => action,
    };
    Ok(match action {
        CaptureAction::Start if recording => {
            ControlReply::unchanged("Dictation is already recording")
        }
        CaptureAction::Start => {
            if let Some(refusal) = capture_refusal(other_recording, suspended) {
                return Ok(refusal);
            }
            if !hotkeys.dictation.lock() {
                return Ok(ControlReply::error(
                    "busy",
                    "the dictation shortcut is held; release it and try again",
                ));
            }
            hotkeys.raw_capture = raw;
            let accepted = handle_hotkey_action(
                HotkeyAction::Start,
                raw,
                capture_boundary(input, release_while_idle),
                input,
                worker,
                context,
                &device,
                events,
                indicator,
                release_while_idle,
            )?;
            if accepted {
                // A warm start already queued its provisional tone.
                make_control_capture_intentional(input, release_while_idle)?;
                ControlReply::done(
                    "Recording; `hex dictation stop` or the shortcut finishes, Escape cancels",
                )
            } else {
                hotkeys.dictation.suspend();
                ControlReply::error("busy", "the microphone is busy; try again")
            }
        }
        CaptureAction::Stop if !recording && print.is_some() => ControlReply::error(
            "not-recording",
            "dictation isn't recording; nothing to print",
        ),
        CaptureAction::Stop if !recording => ControlReply::unchanged("Dictation isn't recording"),
        CaptureAction::Stop => {
            hotkeys.suspend_recording();
            let printing = print.is_some();
            let finished = finish_hotkey_dictation(
                capture_boundary(input, release_while_idle),
                input,
                worker,
                context,
                &device,
                events,
                indicator,
                release_while_idle,
                print,
                hotkeys.raw_capture,
            )?;
            match finished {
                FinishResult::Discarded | FinishResult::Silent if printing => {
                    ControlReply::error("no-speech", "no speech was recorded; nothing to print")
                }
                finished if printing => finish_reply(
                    Some(finished),
                    "Finished recording; printing the transcript",
                ),
                finished => finish_reply(
                    Some(finished),
                    "Finished recording; Hex pastes the text when it's ready",
                ),
            }
        }
        CaptureAction::Cancel if recording => {
            hotkeys.suspend_recording();
            handle_hotkey_action(
                HotkeyAction::Cancel,
                false,
                input.captured_through(),
                input,
                worker,
                context,
                &device,
                events,
                indicator,
                release_while_idle,
            )?;
            ControlReply::done("Cancelled the recording")
        }
        CaptureAction::Cancel | CaptureAction::Toggle => {
            if let Some(refusal) = idle_cancel_refusal(other_recording) {
                refusal
            } else if cancel_latest_job(worker, events, indicator)? {
                ControlReply::done("Cancelled the newest unfinished dictation")
            } else {
                ControlReply::unchanged("Nothing to cancel")
            }
        }
    })
}

/// A `hex paste-last` (or the menu bar's Paste Last Dictation) waits for
/// every capture and shortcut gesture to end: pasting cancels the audio
/// owner's capture, which would strand the machine that started it.
fn paste_last_refusal(capture_active: bool) -> Option<ControlReply> {
    capture_active.then(|| {
        ControlReply::error(
            "busy",
            "a recording or shortcut is in progress; finish or cancel it first",
        )
    })
}

/// Pastes the last dictation at the current focus.
fn paste_last(
    worker: &DictationWorker,
    device: &str,
    events: &mut EventLog,
) -> Result<ControlReply> {
    let reply = match worker.paste_last() {
        Ok(()) => ControlReply::done("Pasting the last dictation"),
        Err(error) => {
            feedback::play(Tone::Error);
            events.dictation(DictationPhase::Failed(error.into()), "")?;
            ControlReply::error("failed", format!("couldn't paste: {error}"))
        }
    };
    emit_engine_state(events, false, worker, device)?;
    Ok(reply)
}

/// The instant a shortcut edge applies to. A released microphone has no
/// authoritative timeline yet, so the edge uses wall-clock time instead.
fn capture_boundary(input: &DictationAudio, release_while_idle: bool) -> CaptureInstant {
    if release_while_idle {
        CaptureInstant::now()
    } else {
        input.captured_through()
    }
}

fn fail_capture(
    message: String,
    worker: &DictationWorker,
    device: &str,
    events: &mut EventLog,
    indicator: Option<&DictationIndicatorSender>,
) -> Result<()> {
    feedback::play(Tone::Error);
    events.dictation(DictationPhase::Failed(message), "")?;
    if let Some(indicator) = indicator {
        indicator.send(DictationIndicatorEvent::Failed);
    }
    emit_engine_state(events, false, worker, device)
}

#[derive(Clone, Copy)]
enum CaptureEnd {
    /// The capture never became a dictation; no feedback tone.
    Discarded(Option<DiscardCause>),
    /// The user cancelled an intentional capture; plays the cancel tone.
    Cancelled,
}

/// Cancels the active capture before reporting the ending.
fn end_capture(
    end: CaptureEnd,
    dictation: &DictationAudio,
    device: &str,
    events: &mut EventLog,
    indicator: Option<&DictationIndicatorSender>,
) -> Result<()> {
    dictation.cancel()?;
    let indicator_event = match end {
        CaptureEnd::Discarded(cause) => {
            events.discarded_dictation(cause.map(DiscardCause::as_str))?;
            DictationIndicatorEvent::Discarded
        }
        CaptureEnd::Cancelled => {
            feedback::play(Tone::Cancel);
            events.dictation(DictationPhase::Cancelled, "")?;
            DictationIndicatorEvent::Cancelled
        }
    };
    if let Some(indicator) = indicator {
        indicator.send(indicator_event);
    }
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

#[allow(clippy::too_many_arguments)]
fn finish_dictation(
    capture: &DictationAudio,
    ended_at: CaptureInstant,
    tail: Duration,
    worker: &DictationWorker,
    target: TranscriptionTarget,
    context: &ContextSnapshot,
    device: &str,
    events: &mut EventLog,
    indicator: Option<&DictationIndicatorSender>,
) -> Result<FinishResult> {
    finish_dictation_with(
        capture,
        ended_at,
        tail,
        |clip| worker.transcribe(clip, target, context.clone()),
        device,
        events,
        indicator,
    )
}

/// Finishes the capture and hands its clip to `submit`.
fn finish_dictation_with(
    capture: &DictationAudio,
    ended_at: CaptureInstant,
    tail: Duration,
    submit: impl FnOnce(DictationClip) -> Result<DictationJobId, &'static str>,
    device: &str,
    events: &mut EventLog,
    indicator: Option<&DictationIndicatorSender>,
) -> Result<FinishResult> {
    let clip = match capture.finish_with_tail(ended_at, tail)? {
        Finish::Transcribe(clip) if !clip.is_near_silent() => clip,
        finish => {
            events.dictation(DictationPhase::Discarded, "")?;
            if let Some(indicator) = indicator {
                indicator.send(DictationIndicatorEvent::Discarded);
            }
            return Ok(match finish {
                Finish::Discard => FinishResult::Discarded,
                Finish::Transcribe(clip) => {
                    crate::dictation_diagnostics::persist_near_silent(
                        &clip.into_transcription_samples(),
                    );
                    FinishResult::Silent
                }
            });
        }
    };
    // The audio owner already announced a release with a tail.
    if tail.is_zero() {
        feedback::play(Tone::DictationStop);
    }
    events.emit(&VoiceEvent::State {
        timestamp_ms: now_ms(),
        state: VoiceState::Transcribing,
        device: device.into(),
    })?;
    events.dictation(DictationPhase::Transcribing, "")?;
    match submit(clip) {
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
                TranscriptionTarget::Paste | TranscriptionTarget::RawPaste => {
                    DictationPhase::Pasted
                }
                TranscriptionTarget::Print | TranscriptionTarget::RawPrint => {
                    DictationPhase::Printed
                }
                TranscriptionTarget::Service => return Ok(()),
            };
            feedback::play(Tone::DictationDone);
            events.processed_dictation(phase, text, processing)?;
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

    #[test]
    fn programmatic_meter_reports_rms_and_peak_in_decibels() {
        let level = level_for(&[0.5, -0.5, 0.0, 0.0]);
        assert!((level.rms_db - -9.0309).abs() < 0.001);
        assert!((level.peak_db - -6.0206).abs() < 0.001);
        assert_eq!(level_for(&[]).rms_db, -120.0);
    }

    #[test]
    fn control_stops_reply_from_what_the_finish_did() {
        let code = |finished| match finish_reply(finished, "ok") {
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

    #[test]
    fn idle_controls_never_reach_past_an_active_capture() {
        let busy = |reply: Option<ControlReply>| matches!(reply, Some(ControlReply::Error { code, .. }) if code == "busy");
        // Escape cancels the recording before any job, so an idle cancel
        // must not cancel a job while the other capture records.
        assert!(busy(idle_cancel_refusal(true)));
        assert!(idle_cancel_refusal(false).is_none());
        assert!(busy(paste_last_refusal(true)));
        assert!(paste_last_refusal(false).is_none());
    }

    #[test]
    fn programmatic_lease_expires_at_its_deadline() {
        let now = Instant::now();
        let deadline = now + PROGRAMMATIC_LEASE;
        assert!(!programmatic_lease_expired(
            deadline,
            deadline - Duration::from_millis(1)
        ));
        assert!(programmatic_lease_expired(deadline, deadline));
    }

    #[test]
    fn programmatic_completion_cache_is_bounded() {
        let mut completions = VecDeque::new();
        for id in 0..PROGRAMMATIC_COMPLETION_LIMIT as u64 + 2 {
            remember_programmatic_completion(
                &mut completions,
                ProgrammaticCompletion {
                    id,
                    owner_token: format!("owner-{id}"),
                    outcome: ProgrammaticTerminalOutcome::Cancelled,
                },
            );
        }
        assert_eq!(completions.len(), PROGRAMMATIC_COMPLETION_LIMIT);
        assert_eq!(completions.front().map(|completion| completion.id), Some(2));
    }

    #[test]
    fn a_programmatic_dictation_leaves_escape_with_its_owner() {
        // A queued hotkey job or the capture picker would otherwise claim
        // Escape that the listener then ignores.
        assert!(!claims_escape(true, false, true, || true));
        assert!(claims_escape(false, false, false, || true));
        assert!(claims_escape(false, false, true, || false));
        assert!(claims_escape(false, true, false, || false));
        assert!(!claims_escape(false, false, false, || false));
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
            last_level: Instant::now(),
            lease_deadline: Instant::now() + PROGRAMMATIC_LEASE,
        };
        let finished_error = |completions: &VecDeque<ProgrammaticCompletion>, id: u64| {
            completions
                .iter()
                .find(|completion| completion.matches_owner(id, &format!("owner-{id}")))
                .and_then(|completion| match &completion.outcome {
                    ProgrammaticTerminalOutcome::Finished(Err(code)) => Some(code.clone()),
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
}
