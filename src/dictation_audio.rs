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

impl DictationAudio {
    pub fn open(
        device_override: Option<&str>,
        selection_revision: u64,
        selected_device: Option<&str>,
        recording_environment: RecordingEnvironmentController,
        pending_input: PendingInputEvents,
    ) -> Result<Self> {
        let input =
            RecoveringAudioInput::open(device_override, selection_revision, selected_device)?;
        let state = Arc::new(State {
            sample_rate: AtomicU64::new(u64::from(input.sample_rate())),
            captured_through: AtomicU64::new(0),
            device_name: Mutex::new(input.device_name().to_owned()),
            recording: AtomicBool::new(false),
            recovering: AtomicBool::new(false),
            recognition_generation: AtomicU64::new(0),
            outstanding_recognition_frames: Arc::new(AtomicU64::new(0)),
            capture_generation: AtomicU64::new(0),
        });
        let mut capture = DictationCapture::new(input.sample_rate());
        capture.enable_recording_environment(recording_environment.clone());
        let (command_sender, commands) = mpsc::channel();
        let (recognition_sender, recognition) = mpsc::sync_channel(RECOGNITION_QUEUE_CAPACITY);
        let (event_sender, events) = mpsc::channel();
        let owner_state = state.clone();
        let worker = thread::Builder::new()
            .name("dictation-audio".into())
            .spawn(move || {
                Owner {
                    input,
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

    pub fn start(&self, at: CaptureInstant) -> Result<()> {
        let _ = self.start_capture(at, false, None)?;
        Ok(())
    }

    pub fn start_voice(&self, at: CaptureInstant, recognition_generation: u64) -> Result<bool> {
        self.start_capture(at, true, Some(recognition_generation))
    }

    fn start_capture(
        &self,
        at: CaptureInstant,
        voice: bool,
        expected_recognition_generation: Option<u64>,
    ) -> Result<bool> {
        self.call(|reply| Command::Start {
            at,
            voice,
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
        self.events.try_recv().ok()
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
                    continue;
                }
                Err(TryRecvError::Disconnected) => break,
                Err(TryRecvError::Empty) => {}
            }
            let event = self
                .input
                .recv_timeout(Duration::from_millis(10), self.capture_idle());
            self.handle_input(event);
        }
    }

    fn handle(&mut self, command: Command) -> bool {
        match command {
            Command::Start {
                at,
                voice,
                expected_recognition_generation,
                reply,
            } => {
                if expected_recognition_generation.is_some_and(|generation| {
                    generation != self.recognition_generation
                        || self.pending_input.oldest().is_some()
                }) {
                    let _ = reply.send(false);
                    return true;
                }
                if expected_recognition_generation.is_some() {
                    self.next_recognition_generation();
                    self.recognition_overflowed = false;
                }
                self.drain_through(at);
                self.state.capture_generation.fetch_add(1, Ordering::AcqRel);
                if voice {
                    self.capture.start_voice_at(at)
                } else {
                    self.capture.start_at(at)
                }
                self.state.recording.store(true, Ordering::Release);
                let _ = reply.send(true);
            }
            Command::BecomeIntentional { at, reply } => {
                self.drain_through(at);
                let _ = reply.send(self.capture.become_intentional(at));
            }
            Command::Finish { at, reply } => {
                self.drain_through(at);
                let finish = self.capture.finish(at);
                self.state.recording.store(false, Ordering::Release);
                let _ = reply.send(finish);
            }
            Command::Cancel { reply } => {
                self.capture.cancel();
                self.state.recording.store(false, Ordering::Release);
                let _ = reply.send(());
            }
            Command::SelectMicrophone { revision, device } => {
                self.input.request_selection(revision, device.as_deref());
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
            let event = self.input.recv_timeout(
                remaining.min(Duration::from_millis(10)),
                self.capture_idle(),
            );
            self.handle_input(event);
        }
    }

    fn capture_idle(&self) -> bool {
        !self.capture.is_recording() && self.pending_input.oldest().is_none()
    }

    fn handle_input(&mut self, event: RecoveringAudioInputEvent) {
        match event {
            RecoveringAudioInputEvent::Chunk {
                samples,
                captured_through,
            } => {
                let chunk_nanos =
                    samples.len() as u128 * 1_000_000_000 / u128::from(self.input.sample_rate());
                let captured_from = captured_through
                    .checked_sub(Duration::from_nanos(chunk_nanos as u64))
                    .unwrap_or(captured_through);
                if let Some(previous) = self.last_captured_through
                    && captured_from.duration_since(previous) > Duration::from_millis(50)
                {
                    let gap = captured_from.duration_since(previous);
                    let was_recording = self.capture.is_recording();
                    let capture_generation = self.state.capture_generation.load(Ordering::Acquire);
                    self.capture = DictationCapture::new(self.input.sample_rate());
                    self.capture
                        .enable_recording_environment(self.recording_environment.clone());
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
            }
            RecoveringAudioInputEvent::Reopened => {
                self.last_captured_through = None;
                self.next_recognition_generation();
                self.recognition_overflowed = false;
                self.capture = DictationCapture::new(self.input.sample_rate());
                self.capture
                    .enable_recording_environment(self.recording_environment.clone());
                self.state
                    .sample_rate
                    .store(u64::from(self.input.sample_rate()), Ordering::Release);
                self.state.captured_through.store(0, Ordering::Release);
                *self
                    .state
                    .device_name
                    .lock()
                    .unwrap_or_else(|error| error.into_inner()) =
                    self.input.device_name().to_owned();
                self.state.recording.store(false, Ordering::Release);
                self.state.recovering.store(false, Ordering::Release);
                let _ = self.events.send(DictationAudioEvent::Reopened);
            }
        }
    }

    fn forward_recognition(&mut self, samples: Vec<f32>, captured_through: CaptureInstant) {
        let frames = samples.len() as u64;
        let max_frames = u64::from(self.input.sample_rate())
            * MAX_RECOGNITION_BACKLOG.as_millis() as u64
            / 1_000;
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
            sample_rate: self.input.sample_rate(),
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
}
