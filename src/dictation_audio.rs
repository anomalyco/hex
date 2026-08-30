use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::mpsc::{
    self, Receiver, RecvTimeoutError, Sender, SyncSender, TryRecvError, TrySendError,
};
use std::sync::{Arc, Mutex};
use std::thread::{self, JoinHandle};
use std::time::{Duration, Instant};

use color_eyre::eyre::{Result, eyre};

use crate::audio::{CaptureInstant, RecoveringAudioInput, RecoveringAudioInputEvent};
use crate::dictation::{DictationCapture, Finish};
use crate::recording_environment::RecordingEnvironmentController;
use crate::suppression::PendingInputEvents;

const RECOGNITION_QUEUE_CAPACITY: usize = 64;
const MAX_RECOGNITION_BACKLOG: Duration = Duration::from_millis(500);

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
        let nanos = self.samples.len() as u128 * 1_000_000_000 / u128::from(self.sample_rate);
        self.captured_through
            .checked_sub(Duration::from_nanos(nanos as u64))
            .unwrap_or(self.captured_through)
    }
}

impl Drop for RecognitionAudio {
    fn drop(&mut self) {
        self.outstanding_frames
            .fetch_sub(self.samples.len() as u64, Ordering::AcqRel);
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

enum Command {
    Start {
        at: CaptureInstant,
        voice: bool,
        programmatic: bool,
        expected_recognition_generation: Option<u64>,
        reply: SyncSender<bool>,
    },
    BecomeIntentional {
        at: CaptureInstant,
        reply: SyncSender<bool>,
    },
    Finish {
        at: CaptureInstant,
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
    recognition_generation: u64,
    recognition_overflowed: bool,
    pending_input: PendingInputEvents,
    last_captured_through: Option<CaptureInstant>,
}

#[derive(Clone, Copy)]
struct PendingCapture {
    at: CaptureInstant,
    voice: bool,
    programmatic: bool,
    intentional_at: Option<CaptureInstant>,
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
        )
    }

    fn with_input(
        input: RecoveringAudioInput,
        device_name: String,
        recording_environment: RecordingEnvironmentController,
        pending_input: PendingInputEvents,
        release_while_idle: bool,
    ) -> Result<Self> {
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
        let owner_state = state.clone();
        let worker = thread::Builder::new()
            .name("dictation-audio".into())
            .spawn(move || {
                Owner {
                    input,
                    release_while_idle,
                    pending_capture: None,
                    capture,
                    recording_environment,
                    commands,
                    recognition: recognition_sender,
                    events: event_sender,
                    state: owner_state,
                    dropped_recognition_frames: 0,
                    recognition_generation: 0,
                    recognition_overflowed: false,
                    pending_input,
                    last_captured_through: None,
                }
                .run();
            })?;
        Ok(Self {
            commands: command_sender,
            recognition,
            events,
            state,
            worker: Some(worker),
        })
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

    fn capture_generation(&self) -> u64 {
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
        self.start_capture(at, false, false, None)
    }

    pub fn start_programmatic(&self, at: CaptureInstant) -> Result<()> {
        if self.start_capture(at, false, true, None)? {
            Ok(())
        } else {
            Err(eyre!("microphone-unavailable"))
        }
    }

    pub fn start_voice(&self, at: CaptureInstant, recognition_generation: u64) -> Result<bool> {
        self.start_capture(at, true, false, Some(recognition_generation))
    }

    fn start_capture(
        &self,
        at: CaptureInstant,
        voice: bool,
        programmatic: bool,
        expected_recognition_generation: Option<u64>,
    ) -> Result<bool> {
        self.call(|reply| Command::Start {
            at,
            voice,
            programmatic,
            expected_recognition_generation,
            reply,
        })
    }

    pub fn become_intentional(&self, at: CaptureInstant) -> Result<bool> {
        self.call(|reply| Command::BecomeIntentional { at, reply })
    }

    pub fn finish(&self, at: CaptureInstant) -> Result<Finish> {
        self.call(|reply| Command::Finish { at, reply })
    }

    pub fn cancel(&self) -> Result<()> {
        self.call(|reply| Command::Cancel { reply })
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
    fn run(mut self) {
        loop {
            match self.commands.try_recv() {
                Ok(command) => {
                    if !self.handle(command) {
                        break;
                    }
                    self.reconcile_input();
                    continue;
                }
                Err(TryRecvError::Disconnected) => break,
                Err(TryRecvError::Empty) => {}
            }
            if !self.input.is_open() && !self.input.is_opening() && !self.input.is_recovering() {
                match self.commands.recv() {
                    Ok(command) => {
                        if !self.handle(command) {
                            break;
                        }
                        self.reconcile_input();
                        continue;
                    }
                    Err(_) => break,
                }
            }
            let capture_idle = self.capture_idle();
            let event = self
                .input
                .recv_timeout(Duration::from_millis(10), capture_idle);
            self.handle_input(event);
            self.reconcile_input();
        }
    }

    fn handle(&mut self, command: Command) -> bool {
        match command {
            Command::Start {
                at,
                voice,
                programmatic,
                expected_recognition_generation,
                reply,
            } => {
                // Draining can discover a failure belonging to the previous capture or idle
                // period. Validate and advance the new capture only after that drain.
                if self.input.is_open() && !self.input.is_recovering() {
                    self.drain_through(at);
                }
                if self.input.is_recovering()
                    || expected_recognition_generation.is_some_and(|generation| {
                        generation != self.recognition_generation
                            || self.pending_input.oldest().is_some()
                    })
                {
                    let _ = reply.send(false);
                    return true;
                }
                if expected_recognition_generation.is_some() {
                    self.next_recognition_generation();
                    self.recognition_overflowed = false;
                }
                self.state.capture_generation.fetch_add(1, Ordering::AcqRel);
                if self.input.is_open() {
                    if voice {
                        self.capture.start_voice_at(at)
                    } else if programmatic {
                        self.capture.start_programmatic_at(at)
                    } else {
                        self.capture.start_at(at)
                    }
                } else {
                    self.pending_capture = Some(PendingCapture {
                        at,
                        voice,
                        programmatic,
                        intentional_at: None,
                    });
                    self.input.request_open();
                }
                self.state.recording.store(true, Ordering::Release);
                let _ = reply.send(true);
            }
            Command::BecomeIntentional { at, reply } => {
                let became_intentional = if let Some(pending) = &mut self.pending_capture {
                    pending.mark_intentional(at);
                    false
                } else {
                    self.drain_through(at);
                    self.capture.become_intentional(at)
                };
                let _ = reply.send(became_intentional);
            }
            Command::Finish { at, reply } => {
                let finish = if self.pending_capture.take().is_some() {
                    self.input.cancel_open();
                    Finish::Discard
                } else {
                    self.drain_through(at);
                    self.capture.finish(at)
                };
                self.state.recording.store(false, Ordering::Release);
                let _ = reply.send(finish);
            }
            Command::Cancel { reply } => {
                self.pending_capture = None;
                if !self.input.is_open() {
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
                let generation = self.next_recognition_generation();
                self.recognition_overflowed = false;
                let _ = reply.send(generation);
            }
            Command::ConsumeRecognition {
                expected_generation,
                reply,
            } => {
                let current = expected_generation == self.recognition_generation
                    && self.pending_input.oldest().is_none();
                if current {
                    self.next_recognition_generation();
                    self.recognition_overflowed = false;
                }
                let _ = reply.send(current);
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
            let capture_idle = self.capture_idle();
            if !self.input.is_open() {
                break;
            }
            let event = self
                .input
                .recv_timeout(remaining.min(Duration::from_millis(10)), capture_idle);
            self.handle_input(event);
        }
    }

    fn capture_idle(&self) -> bool {
        !self.capture.is_recording()
            && self.pending_capture.is_none()
            && self.pending_input.oldest().is_none()
    }

    fn handle_input(&mut self, event: RecoveringAudioInputEvent) {
        match event {
            RecoveringAudioInputEvent::Chunk {
                samples,
                captured_through,
            } => {
                let chunk_nanos =
                    samples.len() as u128 * 1_000_000_000 / u128::from(self.sample_rate());
                let captured_from = captured_through
                    .checked_sub(Duration::from_nanos(chunk_nanos as u64))
                    .unwrap_or(captured_through);
                if let Some(previous) = self.last_captured_through
                    && captured_from.duration_since(previous) > Duration::from_millis(50)
                {
                    let gap = captured_from.duration_since(previous);
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
                self.forward_recognition(samples, captured_through);
            }
            RecoveringAudioInputEvent::Timeout => {}
            RecoveringAudioInputEvent::Interrupted => {
                self.last_captured_through = None;
                self.next_recognition_generation();
                self.recognition_overflowed = false;
                let was_recording = self.capture.is_recording();
                let capture_generation = self.state.capture_generation.load(Ordering::Acquire);
                self.capture.cancel();
                self.state.recording.store(false, Ordering::Release);
                self.state.recovering.store(true, Ordering::Release);
                let _ = self.events.send(DictationAudioEvent::Interrupted {
                    was_recording,
                    capture_generation,
                });
                if self.release_while_idle {
                    self.close_input();
                }
            }
            RecoveringAudioInputEvent::Reopened => {
                self.last_captured_through = None;
                self.next_recognition_generation();
                self.recognition_overflowed = false;
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
                    if pending.voice {
                        self.capture.start_voice_at(pending.at);
                    } else if pending.programmatic {
                        self.capture.start_programmatic_at(pending.at);
                    } else {
                        self.capture.start_at(pending.at);
                    }
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
                    if self.release_while_idle {
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
            generation: self.recognition_generation,
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
        self.recognition_overflowed = true;
        self.next_recognition_generation();
        let dropped_frames = std::mem::take(&mut self.dropped_recognition_frames);
        let _ = self
            .events
            .send(DictationAudioEvent::RecognitionDiscontinuity { dropped_frames });
    }

    fn next_recognition_generation(&mut self) -> u64 {
        self.recognition_generation = self.recognition_generation.wrapping_add(1);
        self.state
            .recognition_generation
            .store(self.recognition_generation, Ordering::Release);
        self.recognition_generation
    }

    fn sample_rate(&self) -> u32 {
        if self.input.is_open() {
            self.input.sample_rate()
        } else {
            16_000
        }
    }

    fn reconcile_input(&mut self) {
        if self.release_while_idle {
            if self.capture_idle()
                && (self.input.is_open() || self.input.is_opening() || self.input.is_recovering())
            {
                self.close_input();
            }
        } else if !self.input.is_open() && !self.input.is_opening() && !self.input.is_recovering() {
            // A key release is acknowledged after its synchronous controls return;
            // waiting for that acknowledgment here can leave the owner asleep.
            self.input.request_recovery();
            self.state.recovering.store(true, Ordering::Release);
        }
    }

    fn close_input(&mut self) {
        self.input.close();
        self.last_captured_through = None;
        self.next_recognition_generation();
        self.recognition_overflowed = false;
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
        let state = Arc::new(State {
            sample_rate: AtomicU64::new(if open { 48_000 } else { 0 }),
            captured_through: AtomicU64::new(0),
            device_name: Mutex::new("Test microphone".into()),
            recording: AtomicBool::new(false),
            recovering: AtomicBool::new(false),
            recognition_generation: AtomicU64::new(0),
            outstanding_recognition_frames: Arc::new(AtomicU64::new(0)),
            capture_generation: AtomicU64::new(0),
        });
        let (commands, command_receiver) = mpsc::channel();
        let (recognition_sender, recognition) = mpsc::sync_channel(RECOGNITION_QUEUE_CAPACITY);
        let (events_sender, events) = mpsc::channel();
        let recording_environment = RecordingEnvironmentController::start();
        let owner = Owner {
            input,
            release_while_idle: false,
            pending_capture: None,
            capture: new_capture(if open { 48_000 } else { 16_000 }, &recording_environment),
            recording_environment,
            commands: command_receiver,
            recognition: recognition_sender,
            events: events_sender,
            state: state.clone(),
            dropped_recognition_frames: 0,
            recognition_generation: 0,
            recognition_overflowed: false,
            pending_input: PendingInputEvents::default(),
            last_captured_through: None,
        };
        let input = DictationAudio {
            commands,
            recognition,
            events,
            state,
            worker: None,
        };
        (owner, input, samples)
    }

    fn control<T>(owner: &mut Owner, command: impl FnOnce(SyncSender<T>) -> Command) -> T {
        let (reply, response) = mpsc::sync_channel(1);
        assert!(owner.handle(command(reply)));
        response.recv().unwrap()
    }

    fn start(owner: &mut Owner, at: CaptureInstant) {
        assert!(control(owner, |reply| Command::Start {
            at,
            voice: false,
            programmatic: false,
            expected_recognition_generation: None,
            reply,
        }));
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
                            control(&mut owner, |reply| Command::Finish { at, reply });
                        }
                        "cancel" => control(&mut owner, |reply| Command::Cancel { reply }),
                        "start" | "start-finish" => {
                            start(&mut owner, at);
                            if boundary == "start-finish" {
                                control(&mut owner, |reply| Command::Finish { at, reply });
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
                        control(&mut owner, |reply| Command::Finish { at, reply });
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
                voice: !interrupted,
                programmatic: false,
                expected_recognition_generation: (!interrupted)
                    .then_some(input.recognition_generation()),
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
            control(&mut owner, |reply| Command::Finish { at, reply });
            assert_eq!(input.capture_generation(), generation);
        }
        let generation = input.capture_generation();
        start(&mut owner, at);
        assert_ne!(input.capture_generation(), generation);
        let generation = input.capture_generation();
        control(&mut owner, |reply| Command::Finish { at, reply });
        control(&mut owner, |reply| Command::Cancel { reply });
        assert_eq!(input.capture_generation(), generation);
    }

    #[test]
    fn startup_microphone_failure_keeps_audio_owner_alive() {
        let input = DictationAudio::open(
            Some("HEX nonexistent microphone for startup recovery test"),
            0,
            None,
            RecordingEnvironmentController::start(),
            PendingInputEvents::default(),
            false,
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
                RecordingEnvironmentController::start(),
                pending_input,
                false,
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
            RecordingEnvironmentController::start(),
            PendingInputEvents::default(),
            false,
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
    fn pending_open_uses_physical_press_for_intentional_threshold() {
        let pressed_at = capture_time();
        let mut pending = PendingCapture {
            at: pressed_at,
            voice: false,
            programmatic: false,
            intentional_at: None,
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
}
