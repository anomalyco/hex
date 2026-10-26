use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::mpsc::{
    self, Receiver, RecvTimeoutError, Sender, SyncSender, TryRecvError, TrySendError,
};
use std::sync::{Arc, Mutex};
use std::thread::{self, JoinHandle};
use std::time::{Duration, Instant};

use color_eyre::eyre::{Result, eyre};

use crate::audio::{CaptureInstant, RecoveringAudioInput, RecoveringAudioInputEvent};
use crate::dictation::{ClipSource, DictationCapture, Finish};
use crate::feedback::{self, Tone};
use crate::recording_environment::{CaptureMarker, RecordingEnvironmentController};
use crate::suppression::PendingInputEvents;
use crate::transcription_engine::TranscriptionEngine;

const RECOGNITION_QUEUE_CAPACITY: usize = 64;
const MAX_RECOGNITION_BACKLOG: Duration = Duration::from_millis(500);
/// How long past the tail itself a finish waits for the stream to deliver it.
const TAIL_DELIVERY_GRACE: Duration = Duration::from_millis(100);

pub struct RecognitionAudio {
    pub samples: Vec<f32>,
    captured_through: CaptureInstant,
    sample_rate: u32,
    generation: u64,
    outstanding_frames: Arc<AtomicU64>,
}

impl RecognitionAudio {
    pub fn is_current(&self, generation: u64) -> bool {
        self.generation == generation
    }

    pub fn captured_from(&self) -> CaptureInstant {
        self.captured_through
            .sub_frames(self.samples.len(), self.sample_rate)
    }
}

impl Drop for RecognitionAudio {
    fn drop(&mut self) {
        self.outstanding_frames
            .fetch_sub(self.samples.len() as u64, Ordering::AcqRel);
    }
}

/// Audio for the `capture` experiment's sink, on the dictation timeline.
#[cfg(hex_capture)]
#[derive(Debug)]
pub enum CaptureAudio {
    Samples {
        samples: Vec<f32>,
        sample_rate: u32,
        captured_through: CaptureInstant,
    },
    /// Audio the sink never received: dropped because the sink was full, or
    /// missing from the microphone stream (a gap, failure, or reopen).
    Gap {
        from: CaptureInstant,
        through: CaptureInstant,
    },
}

/// What a closed capture sink could not deliver.
#[cfg(hex_capture)]
#[derive(Debug, PartialEq, Eq)]
pub struct CaptureSinkClosed {
    /// A gap still waiting for room in the sink when it closed.
    pub pending_gap: Option<(CaptureInstant, CaptureInstant)>,
    /// The end of the audio the sink accounted for, delivered or as a gap.
    pub accounted_through: CaptureInstant,
}

/// A gap shorter than this is jitter between chunks, not missing audio.
#[cfg(hex_capture)]
const GAP_TOLERANCE: Duration = Duration::from_millis(50);

/// The capture sink inside the owner. It never blocks the timeline: a full
/// sink drops audio and records the span as a gap to send once it has room.
#[cfg(hex_capture)]
struct CaptureSink {
    sender: SyncSender<CaptureAudio>,
    /// The capture's start: only audio before it is trimmed.
    since: CaptureInstant,
    accounted_through: CaptureInstant,
    pending_gap: Option<(CaptureInstant, CaptureInstant)>,
}

#[cfg(hex_capture)]
impl CaptureSink {
    fn new(sender: SyncSender<CaptureAudio>, since: CaptureInstant) -> Self {
        Self {
            sender,
            since,
            accounted_through: since,
            pending_gap: None,
        }
    }

    /// Offers one microphone chunk; false once the receiver is gone.
    fn offer(
        &mut self,
        samples: &[f32],
        sample_rate: u32,
        captured_through: CaptureInstant,
    ) -> bool {
        if captured_through <= self.since || sample_rate == 0 {
            return true;
        }
        let mut from = captured_through.sub_frames(samples.len(), sample_rate);
        let mut samples = samples;
        if from < self.since {
            // The first chunk starts before the capture: keep only its part.
            let overlap = self.since.duration_since(from);
            let frames = (overlap.as_nanos() * u128::from(sample_rate) / 1_000_000_000) as usize;
            samples = &samples[frames.min(samples.len())..];
            from = self.since;
        } else if from > self.accounted_through
            && from.duration_since(self.accounted_through) > GAP_TOLERANCE
        {
            self.extend_gap(self.accounted_through, from);
        }
        // Timestamp jitter can make a chunk overlap the last one; every frame
        // is still real audio, so none is dropped.
        from = from.max(self.accounted_through);
        self.accounted_through = self.accounted_through.max(captured_through);
        if let Some((gap_from, gap_through)) = self.pending_gap {
            match self.sender.try_send(CaptureAudio::Gap {
                from: gap_from,
                through: gap_through,
            }) {
                Ok(()) => self.pending_gap = None,
                Err(TrySendError::Full(_)) => {
                    self.extend_gap(from, captured_through);
                    return true;
                }
                Err(TrySendError::Disconnected(_)) => return false,
            }
        }
        if samples.is_empty() {
            return true;
        }
        match self.sender.try_send(CaptureAudio::Samples {
            samples: samples.to_vec(),
            sample_rate,
            captured_through,
        }) {
            Ok(()) => true,
            Err(TrySendError::Full(_)) => {
                if self.pending_gap.is_none() {
                    tracing::warn!("capture sink is full; dropping its audio as a gap");
                }
                self.extend_gap(from, captured_through);
                true
            }
            Err(TrySendError::Disconnected(_)) => false,
        }
    }

    fn extend_gap(&mut self, from: CaptureInstant, through: CaptureInstant) {
        self.pending_gap = Some(match self.pending_gap {
            Some((start, _)) => (start, through),
            None => (from, through),
        });
    }

    fn close(self) -> CaptureSinkClosed {
        CaptureSinkClosed {
            pending_gap: self.pending_gap,
            accounted_through: self.accounted_through,
        }
    }
}

pub enum DictationAudioEvent {
    OpenFailed {
        capture_generation: u64,
        error: String,
    },
    ReadyIntentional {
        capture_generation: u64,
    },
    Interrupted {
        was_recording: bool,
        capture_generation: u64,
    },
    Reopened,
    RecognitionDiscontinuity {
        dropped_frames: u64,
    },
    CaptureDiscontinuity {
        was_recording: bool,
        capture_generation: u64,
        gap_ms: u64,
    },
}

pub struct DictationAudio {
    commands: Sender<Command>,
    recognition: Receiver<RecognitionAudio>,
    events: Receiver<DictationAudioEvent>,
    state: Arc<State>,
    worker: Option<JoinHandle<()>>,
}

struct State {
    sample_rate: AtomicU64,
    captured_through: AtomicU64,
    device_name: Mutex<String>,
    recording: AtomicBool,
    recovering: AtomicBool,
    recognition_generation: AtomicU64,
    outstanding_recognition_frames: Arc<AtomicU64>,
    capture_generation: AtomicU64,
}

#[derive(Clone, Copy)]
enum CaptureKind {
    Hotkey,
    Programmatic,
    /// Voice-delimited capture; accepted only while this recognition
    /// generation is still current and no shortcut edge is pending.
    Voice {
        recognition_generation: u64,
    },
}

enum Command {
    Start {
        at: CaptureInstant,
        kind: CaptureKind,
        reply: SyncSender<bool>,
    },
    BecomeIntentional {
        at: CaptureInstant,
        reply: SyncSender<bool>,
    },
    MarkIntentional {
        reply: SyncSender<bool>,
    },
    Finish {
        at: CaptureInstant,
        tail: Duration,
        reply: SyncSender<Finish>,
    },
    Cancel {
        reply: SyncSender<()>,
    },
    SelectMicrophone {
        revision: u64,
        device: Option<String>,
    },
    SetReleaseWhileIdle(bool),
    Shutdown,
    InvalidateRecognition {
        reply: SyncSender<u64>,
    },
    ConsumeRecognition {
        expected_generation: u64,
        reply: SyncSender<bool>,
    },
    #[cfg(hex_capture)]
    OpenCaptureSink {
        sink: SyncSender<CaptureAudio>,
        since: CaptureInstant,
        reply: SyncSender<()>,
    },
    #[cfg(hex_capture)]
    CloseCaptureSink {
        through: CaptureInstant,
        reply: SyncSender<Option<CaptureSinkClosed>>,
    },
}

struct Owner {
    input: RecoveringAudioInput,
    release_while_idle: bool,
    pending_capture: Option<PendingCapture>,
    capture: DictationCapture,
    recording_environment: RecordingEnvironmentController,
    commands: Receiver<Command>,
    recognition: SyncSender<RecognitionAudio>,
    events: Sender<DictationAudioEvent>,
    state: Arc<State>,
    dropped_recognition_frames: u64,
    recognition_overflowed: bool,
    pending_input: PendingInputEvents,
    last_captured_through: Option<CaptureInstant>,
    engine: Option<Arc<TranscriptionEngine>>,
    /// Whether the active capture streams once it is intentional.
    stream_capture: bool,
    /// When the stream last opened or reopened, while it is open.
    stream_opened_at: Option<Instant>,
    /// When the timeline last had a gap or the stream last failed.
    discontinuity_at: Option<Instant>,
    /// The `capture` experiment's sink. While set, the microphone stays open
    /// even with release-while-idle and switches only once it closes.
    #[cfg(hex_capture)]
    capture_sink: Option<CaptureSink>,
}

struct PendingCapture {
    at: CaptureInstant,
    kind: CaptureKind,
    intentional_at: Option<CaptureInstant>,
    /// Defers background work while the microphone opens; the recording
    /// takes over with its own.
    _capture: CaptureMarker,
}

impl PendingCapture {
    fn mark_intentional(&mut self, at: CaptureInstant) {
        let became_intentional = self.intentional_at.is_none()
            && at.duration_since(self.at) >= crate::dictation::MINIMUM_HOLD_DURATION;
        if became_intentional {
            self.intentional_at = Some(at);
        }
    }
}

impl DictationAudio {
    pub fn open(
        device_override: Option<&str>,
        selection_revision: u64,
        selected_device: Option<&str>,
        recording_environment: RecordingEnvironmentController,
        pending_input: PendingInputEvents,
        release_while_idle: bool,
        engine: Option<Arc<TranscriptionEngine>>,
    ) -> Result<Self> {
        let input = if release_while_idle {
            RecoveringAudioInput::closed(device_override, selection_revision, selected_device)
        } else {
            RecoveringAudioInput::open(device_override, selection_revision, selected_device)
        };
        let device_name = input
            .is_open()
            .then(|| input.device_name().to_owned())
            .or_else(|| device_override.map(str::to_owned))
            .or_else(|| selected_device.map(str::to_owned))
            .unwrap_or_else(|| "Default microphone".into());
        Self::with_input(
            input,
            device_name,
            recording_environment,
            pending_input,
            release_while_idle,
            engine,
        )
    }

    fn with_input(
        input: RecoveringAudioInput,
        device_name: String,
        recording_environment: RecordingEnvironmentController,
        pending_input: PendingInputEvents,
        release_while_idle: bool,
        engine: Option<Arc<TranscriptionEngine>>,
    ) -> Result<Self> {
        let (owner, mut audio) = Owner::new(
            input,
            device_name,
            recording_environment,
            pending_input,
            release_while_idle,
            engine,
        );
        audio.worker = Some(
            thread::Builder::new()
                .name("dictation-audio".into())
                .spawn(move || owner.run())?,
        );
        Ok(audio)
    }

    pub fn sample_rate(&self) -> u32 {
        self.state.sample_rate.load(Ordering::Acquire) as u32
    }

    pub fn device_name(&self) -> String {
        self.state
            .device_name
            .lock()
            .unwrap_or_else(|error| error.into_inner())
            .clone()
    }

    pub fn is_recording(&self) -> bool {
        self.state.recording.load(Ordering::Acquire)
    }

    /// Advances with every accepted capture start.
    pub fn capture_generation(&self) -> u64 {
        self.state.capture_generation.load(Ordering::Acquire)
    }

    pub fn captured_through(&self) -> CaptureInstant {
        CaptureInstant::from_nanos(self.state.captured_through.load(Ordering::Acquire))
    }

    pub fn is_recovering(&self) -> bool {
        self.state.recovering.load(Ordering::Acquire)
    }

    pub fn request_selection(&self, revision: u64, device: Option<&str>) {
        let _ = self.commands.send(Command::SelectMicrophone {
            revision,
            device: device.map(str::to_owned),
        });
    }

    pub fn set_release_while_idle(&self, release: bool) {
        let _ = self.commands.send(Command::SetReleaseWhileIdle(release));
    }

    pub fn recognition_generation(&self) -> u64 {
        self.state.recognition_generation.load(Ordering::Acquire)
    }

    pub fn invalidate_recognition(&self) -> Result<u64> {
        let generation = self.call(|reply| Command::InvalidateRecognition { reply })?;
        self.discard_recognition_backlog();
        Ok(generation)
    }

    pub fn consume_recognition(&self, expected_generation: u64) -> Result<bool> {
        let consumed = self.call(|reply| Command::ConsumeRecognition {
            expected_generation,
            reply,
        })?;
        if consumed {
            self.discard_recognition_backlog();
        }
        Ok(consumed)
    }

    pub fn discard_recognition_backlog(&self) {
        while self.recognition.try_recv().is_ok() {}
    }

    pub fn start(&self, at: CaptureInstant) -> Result<bool> {
        self.start_capture(at, CaptureKind::Hotkey)
    }

    pub fn start_programmatic(&self, at: CaptureInstant) -> Result<()> {
        if self.start_capture(at, CaptureKind::Programmatic)? {
            Ok(())
        } else {
            Err(eyre!("microphone-unavailable"))
        }
    }

    pub fn start_voice(&self, at: CaptureInstant, recognition_generation: u64) -> Result<bool> {
        self.start_capture(
            at,
            CaptureKind::Voice {
                recognition_generation,
            },
        )
    }

    fn start_capture(&self, at: CaptureInstant, kind: CaptureKind) -> Result<bool> {
        self.call(|reply| Command::Start { at, kind, reply })
    }

    pub fn become_intentional(&self, at: CaptureInstant) -> Result<bool> {
        self.call(|reply| Command::BecomeIntentional { at, reply })
    }

    /// Counts the active capture as intentional now, without a hold: a
    /// `hex dictation start` has no shortcut press to judge. True when the
    /// open capture just became intentional; a capture still waiting for the
    /// microphone becomes intentional when it opens and reports
    /// `ReadyIntentional`.
    pub fn mark_intentional(&self) -> Result<bool> {
        self.call(|reply| Command::MarkIntentional { reply })
    }

    pub fn finish(&self, at: CaptureInstant) -> Result<Finish> {
        self.finish_with_tail(at, Duration::ZERO)
    }

    /// Finishes at the exact `at` boundary, first waiting a bounded time for
    /// audio through `at + tail` when the capture would be transcribed.
    pub fn finish_with_tail(&self, at: CaptureInstant, tail: Duration) -> Result<Finish> {
        self.call(|reply| Command::Finish { at, tail, reply })
    }

    pub fn cancel(&self) -> Result<()> {
        self.call(|reply| Command::Cancel { reply })
    }

    #[cfg(hex_capture)]
    /// Copies the timeline's audio from `since` into `sink`, replacing any
    /// sink already open. The sink is bounded: audio it has no room for is
    /// dropped and reported as a gap, never waited on.
    pub fn open_capture_sink(
        &self,
        sink: SyncSender<CaptureAudio>,
        since: CaptureInstant,
    ) -> Result<()> {
        self.call(|reply| Command::OpenCaptureSink { sink, since, reply })
    }

    #[cfg(hex_capture)]
    /// Closes the sink once audio through `through` arrived (bounded wait),
    /// dropping its sender. `None` when no sink was open.
    pub fn close_capture_sink(&self, through: CaptureInstant) -> Result<Option<CaptureSinkClosed>> {
        self.call(|reply| Command::CloseCaptureSink { through, reply })
    }

    pub fn recv_timeout(&self, timeout: Duration) -> Result<Option<RecognitionAudio>> {
        match self.recognition.recv_timeout(timeout) {
            Ok(audio) => Ok(Some(audio)),
            Err(RecvTimeoutError::Timeout) => Ok(None),
            Err(RecvTimeoutError::Disconnected) => Err(eyre!("dictation audio owner stopped")),
        }
    }

    pub fn try_recv_event(&self) -> Option<DictationAudioEvent> {
        while let Ok(event) = self.events.try_recv() {
            let generation = match &event {
                DictationAudioEvent::OpenFailed {
                    capture_generation, ..
                }
                | DictationAudioEvent::ReadyIntentional { capture_generation }
                | DictationAudioEvent::Interrupted {
                    capture_generation, ..
                }
                | DictationAudioEvent::CaptureDiscontinuity {
                    capture_generation, ..
                } => Some(*capture_generation),
                _ => None,
            };
            // Only accepted synchronous starts advance this generation. Finish/Cancel retain
            // terminal failures. The sole control caller also consumes events, so no newer
            // capture can start between this check and use.
            if let Some(generation) = generation
                && generation != self.capture_generation()
            {
                tracing::debug!(
                    capture_generation = generation,
                    "ignored stale capture notification"
                );
                continue;
            }
            return Some(event);
        }
        None
    }

    fn call<T>(&self, command: impl FnOnce(SyncSender<T>) -> Command) -> Result<T> {
        let (reply, response) = mpsc::sync_channel(1);
        self.commands
            .send(command(reply))
            .map_err(|_| eyre!("dictation audio owner stopped"))?;
        response
            .recv()
            .map_err(|_| eyre!("dictation audio owner stopped before applying a control"))
    }
}

impl Drop for DictationAudio {
    fn drop(&mut self) {
        let _ = self.commands.send(Command::Shutdown);
        if let Some(worker) = self.worker.take() {
            let _ = worker.join();
        }
    }
}

impl Owner {
    fn new(
        input: RecoveringAudioInput,
        device_name: String,
        recording_environment: RecordingEnvironmentController,
        pending_input: PendingInputEvents,
        release_while_idle: bool,
        engine: Option<Arc<TranscriptionEngine>>,
    ) -> (Self, DictationAudio) {
        let state = Arc::new(State {
            sample_rate: AtomicU64::new(if input.is_open() {
                u64::from(input.sample_rate())
            } else {
                0
            }),
            captured_through: AtomicU64::new(0),
            device_name: Mutex::new(device_name),
            recording: AtomicBool::new(false),
            recovering: AtomicBool::new(input.is_recovering()),
            recognition_generation: AtomicU64::new(0),
            outstanding_recognition_frames: Arc::new(AtomicU64::new(0)),
            capture_generation: AtomicU64::new(0),
        });
        let capture = new_capture(
            if input.is_open() {
                input.sample_rate()
            } else {
                16_000
            },
            &recording_environment,
        );
        let (command_sender, commands) = mpsc::channel();
        let (recognition_sender, recognition) = mpsc::sync_channel(RECOGNITION_QUEUE_CAPACITY);
        let (event_sender, events) = mpsc::channel();
        let stream_opened_at = input.is_open().then(Instant::now);
        let owner = Self {
            input,
            release_while_idle,
            pending_capture: None,
            capture,
            recording_environment,
            commands,
            recognition: recognition_sender,
            events: event_sender,
            state: state.clone(),
            dropped_recognition_frames: 0,
            recognition_overflowed: false,
            pending_input,
            last_captured_through: None,
            engine,
            stream_capture: false,
            stream_opened_at,
            discontinuity_at: None,
            #[cfg(hex_capture)]
            capture_sink: None,
        };
        let audio = DictationAudio {
            commands: command_sender,
            recognition,
            events,
            state,
            worker: None,
        };
        (owner, audio)
    }

    fn run(mut self) {
        loop {
            let next_command = match self.commands.try_recv() {
                Ok(command) => Some(command),
                Err(TryRecvError::Disconnected) => break,
                Err(TryRecvError::Empty) if !self.input.is_active() => {
                    let Ok(command) = self.commands.recv() else {
                        break;
                    };
                    Some(command)
                }
                Err(TryRecvError::Empty) => None,
            };
            if let Some(command) = next_command {
                if !self.handle(command) {
                    break;
                }
                self.reconcile_input();
                continue;
            }
            let capture_idle = self.input_idle();
            self.input.refresh_automatic_route(capture_idle);
            let event = self
                .input
                .recv_timeout(Duration::from_millis(10), capture_idle);
            self.handle_input(event);
            self.reconcile_input();
        }
    }

    fn handle(&mut self, command: Command) -> bool {
        match command {
            Command::Start { at, kind, reply } => {
                // Draining can discover a failure belonging to the previous capture or idle
                // period. Validate and advance the new capture only after that drain.
                if self.input.is_open() && !self.input.is_recovering() {
                    self.drain_through(at);
                }
                if self.input.is_recovering() {
                    let _ = reply.send(false);
                    return true;
                }
                if let CaptureKind::Voice {
                    recognition_generation,
                } = kind
                {
                    if recognition_generation != self.recognition_generation()
                        || self.pending_input.oldest().is_some()
                    {
                        let _ = reply.send(false);
                        return true;
                    }
                    self.next_recognition_generation();
                }
                self.state.capture_generation.fetch_add(1, Ordering::AcqRel);
                if self.input.is_open() {
                    self.start_capture_at(kind, at);
                } else {
                    self.pending_capture = Some(PendingCapture {
                        at,
                        kind,
                        intentional_at: None,
                        _capture: CaptureMarker::new(),
                    });
                    self.input.request_open();
                }
                self.state.recording.store(true, Ordering::Release);
                let _ = reply.send(true);
            }
            Command::BecomeIntentional { at, reply } => {
                self.drain_through(at);
                let became_intentional = if let Some(pending) = &mut self.pending_capture {
                    pending.mark_intentional(at);
                    false
                } else {
                    self.capture.become_intentional(at)
                };
                let _ = reply.send(became_intentional);
            }
            Command::MarkIntentional { reply } => {
                let became_intentional = if let Some(pending) = &mut self.pending_capture {
                    pending.mark_intentional(pending.at + crate::dictation::MINIMUM_HOLD_DURATION);
                    false
                } else {
                    self.capture.mark_intentional()
                };
                let _ = reply.send(became_intentional);
            }
            Command::Finish { at, tail, reply } => {
                self.drain_through(at);
                let announced = self.pending_capture.is_none()
                    && !tail.is_zero()
                    && self.capture.would_transcribe(at);
                if announced {
                    // A release that will transcribe sounds at once, not after
                    // its tail, and a muted output is restored first.
                    self.capture
                        .end_recording_environment_then(|| feedback::play(Tone::DictationStop));
                }
                let deferred = announced.then(|| self.drain_tail(at, tail)).flatten();
                let mut finish = if self.pending_capture.take().is_some() {
                    if !self.capture_sink_open() {
                        self.input.cancel_open();
                    }
                    Finish::Discard
                } else {
                    self.capture.finish_with_tail(at, tail)
                };
                if let Finish::Transcribe(clip) = &mut finish {
                    clip.set_source(self.clip_source());
                }
                self.state.recording.store(false, Ordering::Release);
                let _ = reply.send(finish);
                if let Some(event) = deferred {
                    self.handle_input(event);
                }
            }
            Command::Cancel { reply } => {
                self.pending_capture = None;
                if !self.input.is_open() && !self.capture_sink_open() {
                    self.input.cancel_open();
                }
                self.capture.cancel();
                self.state.recording.store(false, Ordering::Release);
                let _ = reply.send(());
            }
            Command::SelectMicrophone { revision, device } => {
                self.input.request_selection(revision, device.as_deref());
            }
            Command::SetReleaseWhileIdle(release) => {
                self.release_while_idle = release;
            }
            Command::InvalidateRecognition { reply } => {
                let _ = reply.send(self.next_recognition_generation());
            }
            Command::ConsumeRecognition {
                expected_generation,
                reply,
            } => {
                let current = expected_generation == self.recognition_generation()
                    && self.pending_input.oldest().is_none();
                if current {
                    self.next_recognition_generation();
                }
                let _ = reply.send(current);
            }
            #[cfg(hex_capture)]
            Command::OpenCaptureSink { sink, since, reply } => {
                self.capture_sink = Some(CaptureSink::new(sink, since));
                if !self.input.is_active() {
                    self.input.request_open();
                }
                let _ = reply.send(());
            }
            #[cfg(hex_capture)]
            Command::CloseCaptureSink { through, reply } => {
                if self.capture_sink.is_some() && self.input.is_open() {
                    self.drain_through(through);
                }
                let _ = reply.send(self.capture_sink.take().map(CaptureSink::close));
            }
            Command::Shutdown => return false,
        }
        true
    }

    fn drain_through(&mut self, boundary: CaptureInstant) {
        let deadline = Instant::now() + Duration::from_millis(100);
        while self.state.captured_through.load(Ordering::Acquire) < boundary.as_nanos() {
            let remaining = deadline.saturating_duration_since(Instant::now());
            if remaining.is_zero() {
                break;
            }
            let capture_idle = self.input_idle();
            if !self.input.is_open() {
                break;
            }
            let event = self
                .input
                .recv_timeout(remaining.min(Duration::from_millis(10)), capture_idle);
            self.handle_input(event);
        }
    }

    /// Waits a bounded time for audio through `ended_at + tail`. A stream
    /// failure or discontinuity inside the tail must not discard the finishing
    /// capture, so the first such event is returned for handling afterwards.
    fn drain_tail(
        &mut self,
        ended_at: CaptureInstant,
        tail: Duration,
    ) -> Option<RecoveringAudioInputEvent> {
        let boundary = ended_at + tail;
        let deadline = Instant::now() + tail + TAIL_DELIVERY_GRACE;
        while self.state.captured_through.load(Ordering::Acquire) < boundary.as_nanos() {
            let remaining = deadline.saturating_duration_since(Instant::now());
            if remaining.is_zero() || !self.input.is_open() {
                tracing::debug!("finished before the post-release tail arrived");
                break;
            }
            let capture_idle = self.input_idle();
            let event = self
                .input
                .recv_timeout(remaining.min(Duration::from_millis(10)), capture_idle);
            let continuous = match &event {
                RecoveringAudioInputEvent::Chunk {
                    samples,
                    captured_through,
                } => self.chunk_gap(samples.len(), *captured_through).is_none(),
                RecoveringAudioInputEvent::Timeout => true,
                _ => false,
            };
            if !continuous {
                tracing::warn!(
                    "microphone stream changed during the post-release tail; finishing with the audio already captured"
                );
                return Some(event);
            }
            self.handle_input(event);
        }
        None
    }

    fn chunk_gap(&self, frames: usize, captured_through: CaptureInstant) -> Option<Duration> {
        let captured_from = captured_through.sub_frames(frames, self.sample_rate());
        self.last_captured_through
            .map(|previous| captured_from.duration_since(previous))
            .filter(|gap| *gap > Duration::from_millis(50))
    }

    fn capture_idle(&self) -> bool {
        !self.capture.is_recording()
            && self.pending_capture.is_none()
            && self.pending_input.oldest().is_none()
    }

    /// Whether the microphone may switch or close: no dictation capture and
    /// no capture sink depends on it.
    fn input_idle(&self) -> bool {
        self.capture_idle() && !self.capture_sink_open()
    }

    fn capture_sink_open(&self) -> bool {
        #[cfg(hex_capture)]
        return self.capture_sink.is_some();
        #[cfg(not(hex_capture))]
        false
    }

    fn handle_input(&mut self, event: RecoveringAudioInputEvent) {
        match event {
            RecoveringAudioInputEvent::Chunk {
                samples,
                captured_through,
            } => {
                if let Some(gap) = self.chunk_gap(samples.len(), captured_through) {
                    self.discontinuity_at = Some(Instant::now());
                    let was_recording = self.capture.is_recording();
                    let capture_generation = self.state.capture_generation.load(Ordering::Acquire);
                    self.capture = new_capture(self.sample_rate(), &self.recording_environment);
                    self.state.recording.store(false, Ordering::Release);
                    self.next_recognition_generation();
                    let _ = self.events.send(DictationAudioEvent::CaptureDiscontinuity {
                        was_recording,
                        capture_generation,
                        gap_ms: gap.as_millis() as u64,
                    });
                }
                self.last_captured_through = Some(captured_through);
                self.state
                    .captured_through
                    .store(captured_through.as_nanos(), Ordering::Release);
                self.capture.ingest_with_pending(
                    &samples,
                    captured_through,
                    self.pending_input.oldest(),
                );
                // Short taps and shortcut chords never reach the model; the
                // stream starts with the backlog once the hold counts.
                if self.stream_capture
                    && self.capture.is_intentional()
                    && let Some(engine) = &self.engine
                {
                    self.capture.begin_stream(engine);
                }
                #[cfg(hex_capture)]
                {
                    let sample_rate = self.sample_rate();
                    if let Some(sink) = &mut self.capture_sink
                        && !sink.offer(&samples, sample_rate, captured_through)
                    {
                        tracing::info!("capture sink disconnected; closing it");
                        self.capture_sink = None;
                    }
                }
                self.forward_recognition(samples, captured_through);
            }
            RecoveringAudioInputEvent::Timeout => {}
            RecoveringAudioInputEvent::Interrupted => {
                self.discontinuity_at = Some(Instant::now());
                self.last_captured_through = None;
                self.next_recognition_generation();
                // A cold stream that fails before the hold threshold has lost
                // at most the press's first moments. Reopen for the same press,
                // like any other pending cold capture, instead of failing it.
                if self.release_while_idle
                    && let Some(at) = self.capture.tentative_start()
                {
                    tracing::warn!(
                        "microphone stream failed before the hold threshold; reopening for the same press"
                    );
                    let capture = CaptureMarker::new();
                    self.capture.cancel();
                    self.close_input();
                    self.pending_capture = Some(PendingCapture {
                        at,
                        kind: CaptureKind::Hotkey,
                        intentional_at: None,
                        _capture: capture,
                    });
                    self.input.request_open();
                    return;
                }
                let was_recording = self.capture.is_recording();
                let capture_generation = self.state.capture_generation.load(Ordering::Acquire);
                self.capture.cancel();
                self.state.recording.store(false, Ordering::Release);
                self.state.recovering.store(true, Ordering::Release);
                let _ = self.events.send(DictationAudioEvent::Interrupted {
                    was_recording,
                    capture_generation,
                });
                if self.release_while_idle && !self.capture_sink_open() {
                    self.close_input();
                }
            }
            RecoveringAudioInputEvent::Reopened => {
                self.stream_opened_at = Some(Instant::now());
                self.last_captured_through = None;
                self.next_recognition_generation();
                let sample_rate = self.input.sample_rate();
                self.capture = new_capture(sample_rate, &self.recording_environment);
                self.state
                    .sample_rate
                    .store(u64::from(sample_rate), Ordering::Release);
                self.state.captured_through.store(0, Ordering::Release);
                *self
                    .state
                    .device_name
                    .lock()
                    .unwrap_or_else(|error| error.into_inner()) =
                    self.input.device_name().to_owned();
                self.state.recovering.store(false, Ordering::Release);
                if let Some(pending) = self.pending_capture.take() {
                    self.start_capture_at(pending.kind, pending.at);
                    if let Some(at) = pending.intentional_at {
                        let _ = self.capture.become_intentional(at);
                        let _ = self.events.send(DictationAudioEvent::ReadyIntentional {
                            capture_generation: self
                                .state
                                .capture_generation
                                .load(Ordering::Acquire),
                        });
                    }
                } else {
                    self.state.recording.store(false, Ordering::Release);
                    if self.release_while_idle && !self.capture_sink_open() {
                        self.close_input();
                    }
                }
                let _ = self.events.send(DictationAudioEvent::Reopened);
            }
            RecoveringAudioInputEvent::OpenFailed(error) => {
                let was_recording = self.pending_capture.take().is_some();
                self.state.recording.store(false, Ordering::Release);
                self.reconcile_input();
                if was_recording {
                    let _ = self.events.send(DictationAudioEvent::OpenFailed {
                        capture_generation: self.state.capture_generation.load(Ordering::Acquire),
                        error,
                    });
                }
            }
        }
    }

    fn forward_recognition(&mut self, samples: Vec<f32>, captured_through: CaptureInstant) {
        let frames = samples.len() as u64;
        let max_frames =
            u64::from(self.sample_rate()) * MAX_RECOGNITION_BACKLOG.as_millis() as u64 / 1_000;
        let outstanding = self
            .state
            .outstanding_recognition_frames
            .load(Ordering::Acquire);
        if outstanding.saturating_add(frames) > max_frames {
            self.overflow_recognition(frames);
            return;
        }
        let audio = RecognitionAudio {
            samples,
            captured_through,
            sample_rate: self.sample_rate(),
            generation: self.recognition_generation(),
            outstanding_frames: self.state.outstanding_recognition_frames.clone(),
        };
        self.state
            .outstanding_recognition_frames
            .fetch_add(frames, Ordering::AcqRel);
        match self.recognition.try_send(audio) {
            Ok(()) => {
                self.recognition_overflowed = false;
            }
            Err(TrySendError::Full(_)) => {
                self.overflow_recognition(frames);
            }
            Err(TrySendError::Disconnected(_)) => {}
        }
    }

    fn overflow_recognition(&mut self, frames: u64) {
        self.dropped_recognition_frames += frames;
        if self.recognition_overflowed {
            return;
        }
        self.next_recognition_generation();
        self.recognition_overflowed = true;
        let dropped_frames = std::mem::take(&mut self.dropped_recognition_frames);
        let _ = self
            .events
            .send(DictationAudioEvent::RecognitionDiscontinuity { dropped_frames });
    }

    fn recognition_generation(&self) -> u64 {
        self.state.recognition_generation.load(Ordering::Acquire)
    }

    /// Starts a new recognition generation. Any earlier overflow belonged to
    /// the old generation, so the next overflow reports a fresh discontinuity.
    fn next_recognition_generation(&mut self) -> u64 {
        let generation = self.recognition_generation().wrapping_add(1);
        self.state
            .recognition_generation
            .store(generation, Ordering::Release);
        self.recognition_overflowed = false;
        generation
    }

    fn sample_rate(&self) -> u32 {
        if self.input.is_open() {
            self.input.sample_rate()
        } else {
            16_000
        }
    }

    fn reconcile_input(&mut self) {
        if self.release_while_idle && !self.capture_sink_open() {
            if self.capture_idle() && self.input.is_active() {
                self.close_input();
            }
        } else if !self.input.is_active() {
            // A key release is acknowledged after its synchronous controls return;
            // waiting for that acknowledgment here can leave the owner asleep.
            self.input.request_recovery();
            self.state.recovering.store(true, Ordering::Release);
        }
    }

    fn start_capture_at(&mut self, kind: CaptureKind, at: CaptureInstant) {
        match kind {
            CaptureKind::Hotkey => self.capture.start_at(at),
            CaptureKind::Programmatic => self.capture.start_programmatic_at(at),
            CaptureKind::Voice { .. } => self.capture.start_voice_at(at),
        }
        // Only hotkey captures stream: voice-delimited clips are retranscribed
        // without their control phrase, and programmatic ones need no speed-up.
        self.stream_capture = matches!(kind, CaptureKind::Hotkey);
    }

    fn clip_source(&self) -> ClipSource {
        ClipSource {
            device: self
                .state
                .device_name
                .lock()
                .unwrap_or_else(|error| error.into_inner())
                .clone(),
            stream_age: self.stream_opened_at.map(|at| at.elapsed()),
            since_discontinuity: self.discontinuity_at.map(|at| at.elapsed()),
            since_glitch: crate::audio::since_last_stream_glitch(),
        }
    }

    fn close_input(&mut self) {
        self.input.close();
        self.stream_opened_at = None;
        self.last_captured_through = None;
        self.next_recognition_generation();
        self.state.captured_through.store(0, Ordering::Release);
        self.state.recovering.store(false, Ordering::Release);
    }
}

fn new_capture(
    sample_rate: u32,
    recording_environment: &RecordingEnvironmentController,
) -> DictationCapture {
    let mut capture = DictationCapture::new(sample_rate);
    capture.enable_recording_environment(recording_environment.clone());
    capture
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::dictation::POST_RELEASE_TAIL;

    fn capture_time() -> CaptureInstant {
        CaptureInstant::from_nanos(60_000_000_000)
    }

    // Drive the real serialized owner without a worker so input/control ordering is exact.
    fn owner_for_test(open: bool) -> (Owner, DictationAudio, Sender<(Vec<f32>, CaptureInstant)>) {
        let (mut input, opened) = RecoveringAudioInput::pending_for_test();
        let (replacement, samples) = crate::audio::AudioInput::channel_for_test();
        opened.send((0, Ok(replacement))).unwrap();
        if open {
            assert!(matches!(
                input.recv_timeout(Duration::ZERO, true),
                RecoveringAudioInputEvent::Reopened
            ));
        }
        let (owner, input) = Owner::new(
            input,
            "Test microphone".into(),
            RecordingEnvironmentController::for_test(),
            PendingInputEvents::default(),
            false,
            None,
        );
        input.state.recovering.store(false, Ordering::Release);
        (owner, input, samples)
    }

    fn control<T>(owner: &mut Owner, command: impl FnOnce(SyncSender<T>) -> Command) -> T {
        let (reply, response) = mpsc::sync_channel(1);
        assert!(owner.handle(command(reply)));
        response
            .recv_timeout(Duration::from_secs(5))
            .expect("the owner replies to each command")
    }

    fn start(owner: &mut Owner, at: CaptureInstant) {
        assert!(control(owner, |reply| Command::Start {
            at,
            kind: CaptureKind::Hotkey,
            reply,
        }));
    }

    #[test]
    fn hotkey_captures_stream_once_intentional_and_others_do_not() {
        use crate::transcription_engine::Command as EngineCommand;
        for kind in [CaptureKind::Hotkey, CaptureKind::Programmatic] {
            for cancel in [false, true] {
                let hotkey = matches!(kind, CaptureKind::Hotkey);
                let (engine, commands) = TranscriptionEngine::for_test();
                let (mut owner, _input, _samples) = owner_for_test(true);
                owner.engine = Some(Arc::new(engine));
                let at = capture_time();
                assert!(control(&mut owner, |reply| Command::Start {
                    at,
                    kind,
                    reply
                }));
                let mut through = at;
                let mut chunk = |owner: &mut Owner| {
                    through = through + Duration::from_millis(10);
                    owner.handle_input(RecoveringAudioInputEvent::Chunk {
                        samples: vec![0.25; 480],
                        captured_through: through,
                    });
                };
                chunk(&mut owner);
                // A short tap or chord never reaches the model.
                assert!(commands.try_iter().next().is_none());
                control(&mut owner, |reply| Command::BecomeIntentional {
                    at: at + crate::dictation::MINIMUM_HOLD_DURATION,
                    reply,
                });
                for _ in 0..100 {
                    chunk(&mut owner);
                }
                let mut stream = None;
                if cancel {
                    control(&mut owner, |reply| Command::Cancel { reply });
                } else {
                    let Finish::Transcribe(mut clip) =
                        control(&mut owner, |reply| Command::Finish {
                            tail: Duration::ZERO,
                            at: through,
                            reply,
                        })
                    else {
                        panic!("expected transcription");
                    };
                    stream = clip.take_stream();
                }
                let received = commands.try_iter().collect::<Vec<_>>();
                let count = |matches: fn(&EngineCommand) -> bool| {
                    received.iter().filter(|command| matches(command)).count()
                };
                let begun = count(|command| matches!(command, EngineCommand::Begin { .. }));
                let feeds = count(|command| matches!(command, EngineCommand::Feed { .. }));
                let cancels = count(|command| matches!(command, EngineCommand::Cancel { .. }));
                assert_eq!(begun, usize::from(hotkey), "{cancel}");
                assert_eq!(feeds > 0, hotkey, "{cancel}");
                assert_eq!(cancels, usize::from(hotkey && cancel), "{cancel}");
                assert_eq!(stream.is_some(), hotkey && !cancel, "{cancel}");
            }
        }
    }

    #[test]
    fn marked_captures_are_intentional_without_a_hold() {
        use crate::transcription_engine::Command as EngineCommand;
        // Open microphone: intentional at once, so it streams and a stop
        // shorter than the hold threshold still transcribes.
        let (engine, commands) = TranscriptionEngine::for_test();
        let (mut owner, _input, _samples) = owner_for_test(true);
        owner.engine = Some(Arc::new(engine));
        let at = capture_time();
        start(&mut owner, at);
        assert!(control(&mut owner, |reply| Command::MarkIntentional {
            reply
        }));
        assert!(!control(&mut owner, |reply| Command::MarkIntentional {
            reply
        }));
        let through = at + Duration::from_millis(50);
        owner.handle_input(RecoveringAudioInputEvent::Chunk {
            samples: vec![0.25; 2_400],
            captured_through: through,
        });
        assert!(
            commands
                .try_iter()
                .any(|command| matches!(command, EngineCommand::Begin { .. }))
        );
        assert!(matches!(
            control(&mut owner, |reply| Command::Finish {
                tail: Duration::ZERO,
                at: through,
                reply,
            }),
            Finish::Transcribe(_)
        ));

        // Microphone still opening: it reports ReadyIntentional once open.
        let (mut owner, input, _samples) = owner_for_test(false);
        start(&mut owner, at);
        assert!(!control(&mut owner, |reply| Command::MarkIntentional {
            reply
        }));
        let event = owner.input.recv_timeout(Duration::ZERO, false);
        owner.handle_input(event);
        assert!(matches!(
            input.try_recv_event(),
            Some(DictationAudioEvent::ReadyIntentional { .. })
        ));
    }

    #[test]
    fn capture_failures_survive_terminal_controls_but_not_a_new_capture() {
        for interrupted in [false, true] {
            for recording in [false, true] {
                for boundary in ["current", "finish", "cancel", "start", "start-finish"] {
                    let (mut owner, input, _samples) = owner_for_test(true);
                    let at = capture_time();
                    owner.handle_input(RecoveringAudioInputEvent::Chunk {
                        samples: vec![0.25; 480],
                        captured_through: at,
                    });
                    if recording {
                        start(&mut owner, at);
                    }
                    if interrupted {
                        owner.handle_input(RecoveringAudioInputEvent::Interrupted);
                    } else {
                        owner.handle_input(RecoveringAudioInputEvent::Chunk {
                            samples: vec![0.25; 480],
                            captured_through: at + Duration::from_millis(110),
                        });
                    }
                    assert!(!input.is_recording());
                    match boundary {
                        "finish" => {
                            control(&mut owner, |reply| Command::Finish {
                                tail: Duration::ZERO,
                                at,
                                reply,
                            });
                        }
                        "cancel" => control(&mut owner, |reply| Command::Cancel { reply }),
                        "start" | "start-finish" => {
                            start(&mut owner, at);
                            if boundary == "start-finish" {
                                control(&mut owner, |reply| Command::Finish {
                                    tail: Duration::ZERO,
                                    at,
                                    reply,
                                });
                            }
                        }
                        _ => {}
                    }
                    let event = input.try_recv_event();
                    if matches!(boundary, "current" | "finish" | "cancel") {
                        assert!(matches!(
                            event,
                            Some(DictationAudioEvent::Interrupted { was_recording, .. }
                                | DictationAudioEvent::CaptureDiscontinuity { was_recording, .. })
                                if was_recording == recording
                        ));
                    } else {
                        assert!(
                            event.is_none(),
                            "interrupted={interrupted} recording={recording} {boundary}"
                        );
                    }
                }
            }
        }
    }

    #[test]
    fn pending_capture_notifications_follow_control_boundaries() {
        for ready in [false, true] {
            for boundary in ["current", "finish", "cancel", "start"] {
                let (mut owner, input, _samples) = owner_for_test(false);
                let at = capture_time();
                start(&mut owner, at);
                control(&mut owner, |reply| Command::BecomeIntentional {
                    at: at + crate::dictation::MINIMUM_HOLD_DURATION,
                    reply,
                });
                if ready {
                    let event = owner.input.recv_timeout(Duration::ZERO, false);
                    owner.handle_input(event);
                } else {
                    owner
                        .handle_input(RecoveringAudioInputEvent::OpenFailed("test failure".into()));
                }
                match boundary {
                    "finish" => {
                        control(&mut owner, |reply| Command::Finish {
                            tail: Duration::ZERO,
                            at,
                            reply,
                        });
                    }
                    "cancel" => control(&mut owner, |reply| Command::Cancel { reply }),
                    "start" => start(&mut owner, CaptureInstant::from_nanos(0)),
                    _ => {}
                }
                if boundary != "start" {
                    assert!(matches!(
                        (ready, input.try_recv_event()),
                        (true, Some(DictationAudioEvent::ReadyIntentional { .. }))
                            | (false, Some(DictationAudioEvent::OpenFailed { .. }))
                    ));
                }
                // Stream-wide events still pass through after a stale capture event.
                if ready {
                    assert!(matches!(
                        input.try_recv_event(),
                        Some(DictationAudioEvent::Reopened)
                    ));
                }
                assert!(input.try_recv_event().is_none(), "ready={ready} {boundary}");
            }
        }
    }

    #[test]
    fn cold_capture_restart_rejects_late_open_results_and_closes_after_finish() {
        for cancel in [false, true] {
            let (mut owner, input, _unused_samples) = owner_for_test(false);
            owner.release_while_idle = true;
            // Substitute only the asynchronous device opener, not owner events or captures.
            let (first_input, first_opened) = RecoveringAudioInput::pending_for_test();
            owner.input = first_input;
            let at = capture_time();
            start(&mut owner, at);
            owner.reconcile_input();
            let first_generation = input.capture_generation();
            assert!(owner.input.is_opening());
            if cancel {
                control(&mut owner, |reply| Command::Cancel { reply });
            } else {
                assert!(matches!(
                    control(&mut owner, |reply| Command::Finish {
                        tail: Duration::ZERO,
                        at: at + Duration::from_millis(80),
                        reply,
                    }),
                    Finish::Discard
                ));
            }
            owner.reconcile_input();
            assert!(!input.is_recording());
            assert!(!owner.input.is_opening());
            assert!(!owner.input.is_open());
            let idle_generation = input.capture_generation();
            assert_eq!(idle_generation, first_generation);

            let (second_input, second_opened) = RecoveringAudioInput::pending_for_test();
            owner.input = second_input;
            let second_press = at + Duration::from_millis(180);
            start(&mut owner, second_press);
            owner.reconcile_input();
            let second_generation = input.capture_generation();
            assert_ne!(second_generation, idle_generation);
            assert_ne!(second_generation, first_generation);

            let (late_input, late_samples) = crate::audio::AudioInput::channel_for_test();
            assert!(first_opened.send((0, Ok(late_input))).is_err());
            assert!(late_samples.send((vec![0.75; 480], second_press)).is_err());
            let event = owner.input.recv_timeout(Duration::ZERO, false);
            assert!(matches!(event, RecoveringAudioInputEvent::Timeout));
            owner.handle_input(event);
            assert!(input.try_recv_event().is_none());
            assert!(input.is_recording());

            // A locked second tap stays pending beyond its physical release. Readiness
            // occurs later, but the intentional threshold must still use this press.
            assert!(!control(&mut owner, |reply| Command::BecomeIntentional {
                at: second_press + crate::dictation::MINIMUM_HOLD_DURATION,
                reply,
            }));
            let (replacement, samples) = crate::audio::AudioInput::channel_for_test();
            second_opened.send((0, Ok(replacement))).unwrap();
            let event = owner.input.recv_timeout(Duration::ZERO, false);
            assert!(matches!(event, RecoveringAudioInputEvent::Reopened));
            owner.handle_input(event);
            owner.reconcile_input();
            assert!(owner.input.is_open());
            assert!(input.is_recording());
            assert!(matches!(
                input.try_recv_event(),
                Some(DictationAudioEvent::ReadyIntentional { capture_generation })
                    if capture_generation == second_generation
            ));
            assert!(matches!(
                input.try_recv_event(),
                Some(DictationAudioEvent::Reopened)
            ));

            let finished_at = at + Duration::from_millis(700);
            samples.send((vec![0.25; 4_800], finished_at)).unwrap();
            let finish = control(&mut owner, |reply| Command::Finish {
                tail: Duration::ZERO,
                at: finished_at,
                reply,
            });
            let Finish::Transcribe(clip) = finish else {
                panic!("second capture must preserve its original press and produce one clip");
            };
            assert_eq!(clip.duration_ms(), 100);
            owner.reconcile_input();
            assert!(!input.is_recording());
            assert!(!owner.input.is_open());
            assert!(!owner.input.is_opening());
            assert!(samples.send((vec![0.25; 480], finished_at)).is_err());
            assert!(matches!(
                control(&mut owner, |reply| Command::Finish {
                    tail: Duration::ZERO,
                    at: finished_at,
                    reply
                }),
                Finish::Discard
            ));
            assert!(input.try_recv_event().is_none());
        }
    }

    #[test]
    fn boundary_drain_preserves_finish_failure_but_does_not_fail_a_new_capture() {
        for starting in [false, true] {
            let (mut owner, input, samples) = owner_for_test(true);
            let at = capture_time();
            owner.handle_input(RecoveringAudioInputEvent::Chunk {
                samples: vec![0.25; 480],
                captured_through: at,
            });
            start(&mut owner, at);
            owner.handle_input(RecoveringAudioInputEvent::Chunk {
                samples: vec![0.25; 19_200],
                captured_through: at + Duration::from_millis(400),
            });
            let boundary = at + Duration::from_millis(510);
            samples.send((vec![0.25; 480], boundary)).unwrap();
            if starting {
                start(&mut owner, boundary);
            } else {
                assert!(matches!(
                    control(&mut owner, |reply| Command::Finish {
                        tail: Duration::ZERO,
                        at: boundary,
                        reply
                    }),
                    Finish::Discard
                ));
            }
            assert_eq!(input.is_recording(), starting);
            if starting {
                assert!(input.try_recv_event().is_none());
            } else {
                assert!(matches!(
                    input.try_recv_event(),
                    Some(DictationAudioEvent::CaptureDiscontinuity {
                        was_recording: true,
                        ..
                    })
                ));
            }
        }
    }

    /// Starts an intentional hotkey capture at `at` with 10 ms of warm audio
    /// and 400 ms of speech at 0.25, returning the release instant.
    fn intentional_capture(owner: &mut Owner, at: CaptureInstant) -> CaptureInstant {
        owner.handle_input(RecoveringAudioInputEvent::Chunk {
            samples: vec![0.25; 480],
            captured_through: at,
        });
        start(owner, at);
        let released = at + Duration::from_millis(400);
        owner.handle_input(RecoveringAudioInputEvent::Chunk {
            samples: vec![0.25; 19_200],
            captured_through: released,
        });
        assert!(control(owner, |reply| Command::BecomeIntentional {
            at: at + crate::dictation::MINIMUM_HOLD_DURATION,
            reply,
        }));
        released
    }

    fn finish_after(owner: &mut Owner, at: CaptureInstant, tail: Duration) -> Finish {
        control(owner, |reply| Command::Finish { at, tail, reply })
    }

    #[test]
    fn release_waits_for_its_tail_and_keeps_no_later_audio() {
        let (mut owner, input, samples) = owner_for_test(true);
        let released = intentional_capture(&mut owner, capture_time());
        for (index, value) in [0.5, 0.5, 0.75].into_iter().enumerate() {
            let through = released + Duration::from_millis(100 * (index as u64 + 1));
            samples.send((vec![value; 4_800], through)).unwrap();
        }
        let Finish::Transcribe(clip) = finish_after(&mut owner, released, POST_RELEASE_TAIL) else {
            panic!("expected transcription");
        };
        assert!(!input.is_recording());
        assert_eq!(clip.duration_ms(), 610);
        let source = clip
            .source()
            .expect("the owner records where a clip came from");
        assert_eq!(source.device, "Test microphone");
        assert!(source.stream_age.is_some());
        assert!(source.since_discontinuity.is_none());
        let clip = clip.into_transcription_samples();
        assert!((clip[clip.len() - 400] - 0.5).abs() < 0.05);
        assert!(clip.iter().all(|sample| *sample < 0.6));
    }

    #[test]
    fn a_press_inside_the_tail_starts_its_own_capture_after_the_old_one_finishes() {
        let (mut owner, input, samples) = owner_for_test(true);
        let released = intentional_capture(&mut owner, capture_time());
        let pressed = released + Duration::from_millis(100);
        for through_ms in [100, 200, 300] {
            let through = released + Duration::from_millis(through_ms);
            samples.send((vec![0.5; 4_800], through)).unwrap();
        }
        // The coordinator serializes the release before the newer press.
        let Finish::Transcribe(old) = finish_after(&mut owner, released, POST_RELEASE_TAIL) else {
            panic!("expected transcription");
        };
        assert_eq!(old.duration_ms(), 610);
        let old_generation = input.capture_generation();
        start(&mut owner, pressed);
        assert!(input.is_recording());
        assert_ne!(input.capture_generation(), old_generation);
        let second_release = pressed + Duration::from_millis(500);
        samples
            .send((vec![0.5; 14_400], released + Duration::from_millis(600)))
            .unwrap();
        for through_ms in [700, 800] {
            let through = released + Duration::from_millis(through_ms);
            samples.send((vec![0.5; 4_800], through)).unwrap();
        }
        assert!(control(&mut owner, |reply| Command::BecomeIntentional {
            at: pressed + crate::dictation::MINIMUM_HOLD_DURATION,
            reply,
        }));
        let Finish::Transcribe(new) = finish_after(&mut owner, second_release, POST_RELEASE_TAIL)
        else {
            panic!("expected transcription");
        };
        // Its own 450 ms pre-roll, 500 ms hold, and 200 ms tail.
        assert_eq!(new.duration_ms(), 1_150);
    }

    #[test]
    fn a_stalled_or_failed_stream_bounds_the_tail_wait() {
        for fails in [false, true] {
            let (mut owner, input, samples) = owner_for_test(true);
            let released = intentional_capture(&mut owner, capture_time());
            samples
                .send((vec![0.5; 2_400], released + Duration::from_millis(50)))
                .unwrap();
            if fails {
                drop(samples);
            }
            let started = Instant::now();
            let Finish::Transcribe(clip) = finish_after(&mut owner, released, POST_RELEASE_TAIL)
            else {
                panic!("fails={fails}: the tail must not discard the capture");
            };
            let waited = started.elapsed();
            assert_eq!(clip.duration_ms(), 460, "fails={fails}");
            assert!(waited < Duration::from_secs(2), "fails={fails}: {waited:?}");
            if fails {
                assert!(waited < POST_RELEASE_TAIL, "{waited:?}");
                assert!(matches!(
                    input.try_recv_event(),
                    Some(DictationAudioEvent::Interrupted {
                        was_recording: false,
                        ..
                    })
                ));
            } else {
                assert!(waited >= POST_RELEASE_TAIL, "{waited:?}");
                assert!(input.try_recv_event().is_none());
            }
        }
    }

    #[test]
    fn escape_and_short_taps_finish_without_waiting_for_a_tail() {
        for short_tap in [false, true] {
            let (mut owner, input, _samples) = owner_for_test(true);
            let at = capture_time();
            let started = Instant::now();
            if short_tap {
                owner.handle_input(RecoveringAudioInputEvent::Chunk {
                    samples: vec![0.25; 9_600],
                    captured_through: at + Duration::from_millis(200),
                });
                start(&mut owner, at);
                assert!(matches!(
                    finish_after(
                        &mut owner,
                        at + Duration::from_millis(200),
                        POST_RELEASE_TAIL
                    ),
                    Finish::Discard
                ));
            } else {
                intentional_capture(&mut owner, at);
                control(&mut owner, |reply| Command::Cancel { reply });
            }
            assert!(!input.is_recording());
            assert!(
                started.elapsed() < Duration::from_millis(100),
                "short_tap={short_tap}: {:?}",
                started.elapsed()
            );
        }
    }

    #[test]
    fn start_revalidates_microphone_and_voice_generation_after_boundary_drain() {
        for interrupted in [false, true] {
            let (mut owner, input, samples) = owner_for_test(true);
            let at = capture_time();
            owner.handle_input(RecoveringAudioInputEvent::Chunk {
                samples: vec![0.25; 480],
                captured_through: at,
            });
            let boundary = at + Duration::from_millis(110);
            if interrupted {
                drop(samples);
            } else {
                samples.send((vec![0.25; 480], boundary)).unwrap();
            }
            let generation = input.capture_generation();
            assert!(!control(&mut owner, |reply| Command::Start {
                at: boundary,
                kind: if interrupted {
                    CaptureKind::Hotkey
                } else {
                    CaptureKind::Voice {
                        recognition_generation: input.recognition_generation(),
                    }
                },
                reply,
            }));
            assert_eq!(input.capture_generation(), generation);
            assert!(!input.is_recording());
            assert!(input.try_recv_event().is_some());
        }
    }

    #[test]
    fn capture_generation_advances_only_when_a_new_capture_is_accepted() {
        let (mut owner, input, _samples) = owner_for_test(true);
        let at = CaptureInstant::from_nanos(0);
        for _ in 0..2 {
            let generation = input.capture_generation();
            control(&mut owner, |reply| Command::Cancel { reply });
            assert_eq!(input.capture_generation(), generation);
            let generation = input.capture_generation();
            control(&mut owner, |reply| Command::Finish {
                tail: Duration::ZERO,
                at,
                reply,
            });
            assert_eq!(input.capture_generation(), generation);
        }
        let generation = input.capture_generation();
        start(&mut owner, at);
        assert_ne!(input.capture_generation(), generation);
        let generation = input.capture_generation();
        control(&mut owner, |reply| Command::Finish {
            tail: Duration::ZERO,
            at,
            reply,
        });
        control(&mut owner, |reply| Command::Cancel { reply });
        assert_eq!(input.capture_generation(), generation);
    }

    #[test]
    fn startup_microphone_failure_keeps_audio_owner_alive() {
        let device = "HEX nonexistent microphone for startup recovery test";
        let input = DictationAudio::with_input(
            RecoveringAudioInput::failed_for_test(device),
            device.into(),
            RecordingEnvironmentController::for_test(),
            PendingInputEvents::default(),
            false,
            None,
        )
        .expect("a missing microphone should enter recovery, not stop the listener");

        assert!(input.is_recovering());
        assert_eq!(input.sample_rate(), 0);
        assert!(
            input
                .recv_timeout(Duration::from_millis(20))
                .unwrap()
                .is_none()
        );
        assert_eq!(
            input
                .start_programmatic(capture_time())
                .unwrap_err()
                .to_string(),
            "microphone-unavailable"
        );
        assert!(!input.is_recording());
        assert_eq!(input.capture_generation(), 0);
        assert!(!input.start(capture_time()).unwrap());
        assert!(!input.is_recording());
        assert!(matches!(
            input.finish(capture_time()).unwrap(),
            Finish::Discard
        ));
        input.cancel().unwrap();
        assert!(input.is_recovering());

        input.set_release_while_idle(true);
        input.cancel().unwrap();
        assert!(!input.is_recovering());
        input.set_release_while_idle(false);
        input.cancel().unwrap();
        assert!(input.is_recovering());
    }

    #[test]
    fn enabling_warm_microphone_preserves_pending_capture_and_recovers_after_cancellation() {
        for finish in [false, true] {
            let (input, _opened) = RecoveringAudioInput::pending_for_test();
            let (pending_input, acknowledge) =
                PendingInputEvents::with_pending_for_test(capture_time());
            let input = DictationAudio::with_input(
                input,
                "Test microphone".into(),
                RecordingEnvironmentController::for_test(),
                pending_input,
                false,
                None,
            )
            .unwrap();
            assert!(input.start(capture_time()).unwrap());
            assert!(input.is_recording());
            input.set_release_while_idle(true);
            input.invalidate_recognition().unwrap();

            input.set_release_while_idle(false);
            input.invalidate_recognition().unwrap();
            assert!(!input.is_recovering());
            assert!(input.is_recording());

            if finish {
                assert!(matches!(
                    input.finish(capture_time()).unwrap(),
                    Finish::Discard
                ));
            } else {
                input.cancel().unwrap();
            }
            input.invalidate_recognition().unwrap();
            assert!(!input.is_recording());
            assert!(input.is_recovering());
            acknowledge();
        }
    }

    #[test]
    fn recovered_owner_delivers_audio_and_accepts_a_new_capture() {
        let (mut input, opened) = RecoveringAudioInput::pending_for_test();
        input.request_recovery();
        let input = DictationAudio::with_input(
            input,
            "Missing microphone".into(),
            RecordingEnvironmentController::for_test(),
            PendingInputEvents::default(),
            false,
            None,
        )
        .unwrap();
        assert!(!input.start(capture_time()).unwrap());
        let (replacement, samples) = crate::audio::AudioInput::channel_for_test();
        opened.send((0, Ok(replacement))).unwrap();
        assert!(matches!(
            input.events.recv_timeout(Duration::from_secs(2)).unwrap(),
            DictationAudioEvent::Reopened
        ));
        assert!(!input.is_recovering());
        assert_eq!(input.sample_rate(), 48_000);
        assert_eq!(input.device_name(), "Test microphone");
        samples.send((vec![0.25; 480], capture_time())).unwrap();
        let audio = input.recv_timeout(Duration::from_secs(2)).unwrap().unwrap();
        assert_eq!(audio.samples, vec![0.25; 480]);
        assert!(audio.is_current(input.recognition_generation()));
        assert_eq!(input.captured_through(), capture_time());
        assert!(input.start(capture_time()).unwrap());
        assert!(input.is_recording());
        input.cancel().unwrap();
        assert!(!input.is_recording());
        assert!(!input.is_recovering());
    }

    #[test]
    fn cold_stream_failure_before_hold_threshold_reopens_for_the_same_press() {
        for mode in ["reopen", "release-pending", "release-during-drain"] {
            let (mut owner, input, first_samples) = owner_for_test(false);
            owner.release_while_idle = true;
            let pressed_at = capture_time();
            start(&mut owner, pressed_at);
            let generation = input.capture_generation();
            let event = owner.input.recv_timeout(Duration::ZERO, false);
            owner.handle_input(event);
            assert!(matches!(
                input.try_recv_event(),
                Some(DictationAudioEvent::Reopened)
            ));

            if mode == "release-during-drain" {
                drop(first_samples);
                assert!(matches!(
                    control(&mut owner, |reply| Command::Finish {
                        tail: Duration::ZERO,
                        at: pressed_at + Duration::from_millis(250),
                        reply,
                    }),
                    Finish::Discard
                ));
                owner.reconcile_input();
                assert!(!input.is_recording());
                assert!(!owner.input.is_opening());
                assert!(input.try_recv_event().is_none());
                continue;
            }

            // #130: the freshly opened stream fails ~150 ms into the held press.
            owner.handle_input(RecoveringAudioInputEvent::Interrupted);
            owner.reconcile_input();
            assert!(input.is_recording(), "the held press must survive");
            assert!(input.try_recv_event().is_none(), "no failure is reported");
            assert!(owner.input.is_opening(), "the press reopens the microphone");

            let (second_input, second_opened) = RecoveringAudioInput::pending_for_test();
            owner.input = second_input;
            if mode == "release-pending" {
                assert!(matches!(
                    control(&mut owner, |reply| Command::Finish {
                        tail: Duration::ZERO,
                        at: pressed_at + Duration::from_millis(250),
                        reply,
                    }),
                    Finish::Discard
                ));
                owner.reconcile_input();
                assert!(!input.is_recording());
                assert!(!owner.input.is_opening());
                assert!(second_opened.send((0, Err("late".into()))).is_err());
                continue;
            }
            assert!(!control(&mut owner, |reply| Command::BecomeIntentional {
                at: pressed_at + crate::dictation::MINIMUM_HOLD_DURATION,
                reply,
            }));
            let (second_stream, samples) = crate::audio::AudioInput::channel_for_test();
            second_opened.send((0, Ok(second_stream))).unwrap();
            let event = owner.input.recv_timeout(Duration::ZERO, false);
            owner.handle_input(event);
            assert!(matches!(
                input.try_recv_event(),
                Some(DictationAudioEvent::ReadyIntentional { capture_generation })
                    if capture_generation == generation
            ));
            let finished_at = pressed_at + Duration::from_millis(700);
            samples.send((vec![0.25; 4_800], finished_at)).unwrap();
            let Finish::Transcribe(clip) = control(&mut owner, |reply| Command::Finish {
                tail: Duration::ZERO,
                at: finished_at,
                reply,
            }) else {
                panic!("the reopened press must still produce its clip");
            };
            assert_eq!(clip.duration_ms(), 100);
        }
    }

    #[test]
    fn cold_stream_failure_after_hold_threshold_still_fails_the_capture() {
        for mark_intentional in [false, true] {
            let (mut owner, input, _unused_samples) = owner_for_test(false);
            owner.release_while_idle = true;
            let pressed_at = capture_time();
            start(&mut owner, pressed_at);
            let event = owner.input.recv_timeout(Duration::ZERO, false);
            owner.handle_input(event);
            let threshold_at = pressed_at + crate::dictation::MINIMUM_HOLD_DURATION;
            if mark_intentional {
                assert!(owner.capture.become_intentional(threshold_at));
            } else {
                owner.handle_input(RecoveringAudioInputEvent::Chunk {
                    samples: vec![0.25; 14_400],
                    captured_through: threshold_at,
                });
            }
            assert!(matches!(
                input.try_recv_event(),
                Some(DictationAudioEvent::Reopened)
            ));

            // Speech may already be recorded, so dropping it silently would paste a fragment.
            owner.handle_input(RecoveringAudioInputEvent::Interrupted);
            assert!(!input.is_recording());
            assert!(matches!(
                input.try_recv_event(),
                Some(DictationAudioEvent::Interrupted {
                    was_recording: true,
                    ..
                })
            ));
        }
    }

    #[test]
    fn pending_open_uses_physical_press_for_intentional_threshold() {
        let pressed_at = capture_time();
        let mut pending = PendingCapture {
            at: pressed_at,
            kind: CaptureKind::Hotkey,
            intentional_at: None,
            _capture: CaptureMarker::new(),
        };

        pending.mark_intentional(pressed_at + Duration::from_millis(299));
        assert_eq!(pending.intentional_at, None);
        pending.mark_intentional(pressed_at + crate::dictation::MINIMUM_HOLD_DURATION);
        assert_eq!(
            pending.intentional_at,
            Some(pressed_at + crate::dictation::MINIMUM_HOLD_DURATION)
        );
        pending.mark_intentional(
            pressed_at + crate::dictation::MINIMUM_HOLD_DURATION + Duration::from_millis(1),
        );
        assert_eq!(
            pending.intentional_at,
            Some(pressed_at + crate::dictation::MINIMUM_HOLD_DURATION)
        );
    }

    #[cfg(hex_capture)]
    fn chunk_at(owner: &mut Owner, through: CaptureInstant) {
        owner.handle_input(RecoveringAudioInputEvent::Chunk {
            samples: vec![0.25; 480],
            captured_through: through,
        });
    }

    #[cfg(hex_capture)]
    fn open_sink(
        owner: &mut Owner,
        capacity: usize,
        since: CaptureInstant,
    ) -> Receiver<CaptureAudio> {
        let (sink, audio) = mpsc::sync_channel(capacity);
        control(owner, |reply| Command::OpenCaptureSink {
            sink,
            since,
            reply,
        });
        audio
    }

    /// Drains the events; a hold reports readiness and nothing reads
    /// recognition here, but dictation must see no failure or gap.
    #[cfg(hex_capture)]
    fn assert_no_capture_failure(input: &DictationAudio) {
        while let Some(event) = input.try_recv_event() {
            assert!(
                !matches!(
                    event,
                    DictationAudioEvent::OpenFailed { .. }
                        | DictationAudioEvent::Interrupted { .. }
                        | DictationAudioEvent::CaptureDiscontinuity { .. }
                ),
                "dictation saw a failure or gap"
            );
        }
    }

    #[cfg(hex_capture)]
    fn sample_frames(
        audio: &Receiver<CaptureAudio>,
    ) -> (usize, Vec<(CaptureInstant, CaptureInstant)>) {
        let mut frames = 0;
        let mut gaps = Vec::new();
        for item in audio.try_iter() {
            match item {
                CaptureAudio::Samples {
                    samples,
                    sample_rate,
                    ..
                } => {
                    assert_eq!(sample_rate, 48_000);
                    frames += samples.len();
                }
                CaptureAudio::Gap { from, through } => gaps.push((from, through)),
            }
        }
        (frames, gaps)
    }

    #[cfg(hex_capture)]
    #[test]
    fn capture_sink_keeps_every_frame_through_timestamp_jitter() {
        let since = capture_time();
        let (sender, audio) = mpsc::sync_channel(16);
        let mut sink = CaptureSink::new(sender, since);
        // Straddles the start: only the part after it is kept.
        assert!(sink.offer(&[0.1; 480], 48_000, since + Duration::from_millis(5)));
        // Jitter: stamped 1 ms early, overlapping the chunk before it.
        assert!(sink.offer(&[0.2; 480], 48_000, since + Duration::from_millis(14)));
        assert!(sink.offer(&[0.3; 480], 48_000, since + Duration::from_millis(25)));
        // A late stamp inside the tolerance is not a gap either.
        assert!(sink.offer(&[0.4; 480], 48_000, since + Duration::from_millis(40)));
        let mut frames = 0;
        for item in audio.try_iter() {
            match item {
                CaptureAudio::Samples { samples, .. } => frames += samples.len(),
                CaptureAudio::Gap { .. } => panic!("jitter is not a gap"),
            }
        }
        assert_eq!(frames, 240 + 3 * 480);
        let closed = sink.close();
        assert_eq!(closed.pending_gap, None);
        assert_eq!(closed.accounted_through, since + Duration::from_millis(40));
    }

    #[cfg(hex_capture)]
    #[test]
    fn capture_sink_shares_the_timeline_with_dictation() {
        let (mut owner, input, _samples) = owner_for_test(true);
        let at = capture_time();
        chunk_at(&mut owner, at);
        // The sink starts 5 ms into the next chunk and keeps only that part.
        let since = at + Duration::from_millis(5);
        let audio = open_sink(&mut owner, 1_024, since);
        let mut through = at;
        for _ in 0..20 {
            through = through + Duration::from_millis(10);
            chunk_at(&mut owner, through);
        }
        start(&mut owner, through);
        for _ in 0..50 {
            through = through + Duration::from_millis(10);
            chunk_at(&mut owner, through);
        }
        let Finish::Transcribe(clip) = control(&mut owner, |reply| Command::Finish {
            tail: Duration::ZERO,
            at: through,
            reply,
        }) else {
            panic!("dictation still transcribes while a capture records");
        };
        assert!(clip.duration_ms() >= 450, "{}", clip.duration_ms());
        for _ in 0..10 {
            through = through + Duration::from_millis(10);
            chunk_at(&mut owner, through);
        }
        assert_eq!(
            control(&mut owner, |reply| Command::CloseCaptureSink {
                through,
                reply
            }),
            Some(CaptureSinkClosed {
                pending_gap: None,
                accounted_through: through,
            })
        );
        let (frames, gaps) = sample_frames(&audio);
        assert_eq!(frames, 240 + 79 * 480, "every frame from `since`, once");
        assert!(gaps.is_empty());
        assert_no_capture_failure(&input);
        // Closed: later audio goes nowhere, and a second close is a no-op.
        chunk_at(&mut owner, through + Duration::from_millis(10));
        assert!(audio.try_recv().is_err());
        assert_eq!(
            control(&mut owner, |reply| Command::CloseCaptureSink {
                through,
                reply
            }),
            None
        );
    }

    #[cfg(hex_capture)]
    #[test]
    fn a_full_capture_sink_drops_audio_as_a_gap_and_never_stalls_dictation() {
        let (mut owner, input, _samples) = owner_for_test(true);
        let at = capture_time();
        let audio = open_sink(&mut owner, 2, at);
        start(&mut owner, at);
        let mut through = at;
        for _ in 0..100 {
            through = through + Duration::from_millis(10);
            chunk_at(&mut owner, through);
        }
        let dropped_from = at + Duration::from_millis(20);
        let Finish::Transcribe(clip) = control(&mut owner, |reply| Command::Finish {
            tail: Duration::ZERO,
            at: through,
            reply,
        }) else {
            panic!("a full sink must not stop dictation");
        };
        assert!(clip.duration_ms() >= 990, "{}", clip.duration_ms());

        // The sink drains; the next chunk sends the gap, then its own audio.
        assert_eq!(sample_frames(&audio), (960, Vec::new()));
        through = through + Duration::from_millis(10);
        chunk_at(&mut owner, through);
        let (frames, gaps) = sample_frames(&audio);
        assert_eq!(frames, 480);
        assert_eq!(
            gaps,
            [(
                dropped_from,
                through.checked_sub(Duration::from_millis(10)).unwrap()
            )]
        );

        // A gap still pending when the sink closes comes back to the caller.
        for _ in 0..5 {
            through = through + Duration::from_millis(10);
            chunk_at(&mut owner, through);
        }
        let closed = control(&mut owner, |reply| Command::CloseCaptureSink {
            through,
            reply,
        })
        .expect("the sink was open");
        assert_eq!(
            closed.pending_gap,
            Some((
                through.checked_sub(Duration::from_millis(30)).unwrap(),
                through
            ))
        );
        assert_no_capture_failure(&input);
    }

    #[cfg(hex_capture)]
    #[test]
    fn capture_sink_records_microphone_gaps() {
        let (mut owner, _input, _samples) = owner_for_test(true);
        let at = capture_time();
        let audio = open_sink(&mut owner, 64, at);
        chunk_at(&mut owner, at + Duration::from_millis(10));
        // 200 ms of audio never arrived.
        let resumed = at + Duration::from_millis(220);
        chunk_at(&mut owner, resumed);
        let (frames, gaps) = sample_frames(&audio);
        assert_eq!(frames, 960);
        assert_eq!(
            gaps,
            [(
                at + Duration::from_millis(10),
                resumed.checked_sub(Duration::from_millis(10)).unwrap()
            )]
        );
    }

    #[cfg(hex_capture)]
    #[test]
    fn capture_sink_holds_a_released_microphone_open_until_it_closes() {
        let (mut owner, _input, _samples) = owner_for_test(true);
        owner.release_while_idle = true;
        let at = capture_time();
        let audio = open_sink(&mut owner, 64, at);
        owner.reconcile_input();
        assert!(owner.input.is_open(), "the capture keeps the stream open");
        start(&mut owner, at);
        chunk_at(&mut owner, at + Duration::from_millis(10));
        control(&mut owner, |reply| Command::Cancel { reply });
        owner.reconcile_input();
        assert!(owner.input.is_open(), "dictation ending does not close it");

        // A stream failure keeps recovering instead of closing.
        owner.handle_input(RecoveringAudioInputEvent::Interrupted);
        owner.reconcile_input();
        assert!(owner.input.is_active());

        drop(audio);
        control(&mut owner, |reply| Command::CloseCaptureSink {
            through: at,
            reply,
        });
        owner.reconcile_input();
        assert!(!owner.input.is_active(), "released once the capture ends");
    }

    #[cfg(hex_capture)]
    #[test]
    fn a_dropped_capture_receiver_closes_the_sink() {
        let (mut owner, _input, _samples) = owner_for_test(true);
        let at = capture_time();
        drop(open_sink(&mut owner, 4, at));
        chunk_at(&mut owner, at + Duration::from_millis(10));
        assert!(owner.capture_sink.is_none());
    }
}
