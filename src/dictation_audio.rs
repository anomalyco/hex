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
const EVENT_QUEUE_CAPACITY: usize = 16;
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
    events: SyncSender<DictationAudioEvent>,
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
            RecoveringAudioInput::open(device_override, selection_revision, selected_device)?
        };
        let state = Arc::new(State {
            sample_rate: AtomicU64::new(if input.is_open() {
                u64::from(input.sample_rate())
            } else {
                0
            }),
            captured_through: AtomicU64::new(0),
            device_name: Mutex::new(
                input
                    .is_open()
                    .then(|| input.device_name().to_owned())
                    .or_else(|| device_override.map(str::to_owned))
                    .or_else(|| selected_device.map(str::to_owned))
                    .unwrap_or_else(|| "Default microphone".into()),
            ),
            recording: AtomicBool::new(false),
            recovering: AtomicBool::new(false),
            recognition_generation: AtomicU64::new(0),
            outstanding_recognition_frames: Arc::new(AtomicU64::new(0)),
            capture_generation: AtomicU64::new(0),
        });
        let mut capture = DictationCapture::new(if input.is_open() {
            input.sample_rate()
        } else {
            16_000
        });
        capture.enable_recording_environment(recording_environment.clone());
        let (command_sender, commands) = mpsc::channel();
        let (recognition_sender, recognition) = mpsc::sync_channel(RECOGNITION_QUEUE_CAPACITY);
        let (event_sender, events) = mpsc::sync_channel(EVENT_QUEUE_CAPACITY);
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
                    self.close_if_idle();
                    continue;
                }
                Err(TryRecvError::Disconnected) => break,
                Err(TryRecvError::Empty) => {}
            }
            let capture_idle = self.capture_idle();
            let event = self
                .input
                .recv_timeout(Duration::from_millis(10), capture_idle);
            self.handle_input(event);
            self.close_if_idle();
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
                self.state.capture_generation.fetch_add(1, Ordering::AcqRel);
                if self.input.is_open() {
                    self.drain_through(at);
                    if voice {
                        self.capture.start_voice_at(at)
                    } else {
                        self.capture.start_at(at)
                    }
                } else {
                    self.pending_capture = Some(PendingCapture {
                        at,
                        voice,
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
                    self.input.close();
                    Finish::Discard
                } else {
                    self.drain_through(at);
                    self.capture.finish(at)
                };
                self.state.recording.store(false, Ordering::Release);
                let _ = reply.send(finish);
                self.close_if_idle();
            }
            Command::Cancel { reply } => {
                self.pending_capture = None;
                if !self.input.is_open() {
                    self.input.close();
                }
                self.capture.cancel();
                self.state.recording.store(false, Ordering::Release);
                let _ = reply.send(());
                self.close_if_idle();
            }
            Command::SelectMicrophone { revision, device } => {
                self.input.request_selection(revision, device.as_deref());
            }
            Command::SetReleaseWhileIdle(release) => {
                self.release_while_idle = release;
                if self.capture_idle() {
                    if release {
                        self.close_input();
                    } else if !self.input.is_open() {
                        self.input.request_open();
                    }
                }
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
                    self.capture = DictationCapture::new(self.sample_rate());
                    self.capture
                        .enable_recording_environment(self.recording_environment.clone());
                    self.state.recording.store(false, Ordering::Release);
                    self.next_recognition_generation();
                    let _ = self
                        .events
                        .try_send(DictationAudioEvent::CaptureDiscontinuity {
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
                let _ = self.events.try_send(DictationAudioEvent::Interrupted {
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
                self.capture = DictationCapture::new(sample_rate);
                self.capture
                    .enable_recording_environment(self.recording_environment.clone());
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
                    } else {
                        self.capture.start_at(pending.at);
                    }
                    if let Some(at) = pending.intentional_at {
                        let _ = self.capture.become_intentional(at);
                        let _ = self.events.try_send(DictationAudioEvent::ReadyIntentional {
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
                let _ = self.events.try_send(DictationAudioEvent::Reopened);
            }
            RecoveringAudioInputEvent::OpenFailed(error) => {
                self.pending_capture = None;
                self.state.recording.store(false, Ordering::Release);
                let _ = self.events.try_send(DictationAudioEvent::OpenFailed {
                    capture_generation: self.state.capture_generation.load(Ordering::Acquire),
                    error,
                });
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
            .try_send(DictationAudioEvent::RecognitionDiscontinuity { dropped_frames });
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

    fn close_if_idle(&mut self) {
        if self.release_while_idle
            && self.input.is_open()
            && !self.capture.is_recording()
            && self.pending_capture.is_none()
        {
            self.close_input();
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

#[cfg(test)]
mod tests {
    use super::*;

    fn capture_time() -> CaptureInstant {
        CaptureInstant::from_nanos(60_000_000_000)
    }

    #[test]
    fn pending_open_uses_physical_press_for_intentional_threshold() {
        let pressed_at = capture_time();
        let mut pending = PendingCapture {
            at: pressed_at,
            voice: false,
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
