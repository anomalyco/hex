//! Live transcription of another app's audio (`/streams` in the local API;
//! contract in `docs/streams.md`). The app records and pushes 16 kHz mono
//! audio; Hex transcribes it on the dictation engine's loaded model and
//! reports partial text, one final segment per window, and the final text.
//!
//! Dictation always comes first. Audio is cut into windows at pauses (at most
//! [`MAX_WINDOW_SAMPLES`]). Each window streams through a
//! [`YieldingStream`], which any dictation request closes, and the stream
//! feeds nothing while dictation is busy (from the shortcut press through
//! inference): its audio waits in a bounded backlog and catches up after. A
//! window whose stream dictation closed, or any window on a model that
//! cannot stream, is transcribed offline as one foreground request once
//! dictation is idle: after the dictation already sent, before meeting
//! chunks, and never inside a dictation stream.

use std::collections::VecDeque;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Condvar, Mutex, MutexGuard, Weak};
use std::thread;
use std::time::{Duration, Instant};

use serde::Serialize;

use crate::pause_segmenter::{FRAME_SAMPLES, PauseSegmenter};
use crate::transcription_engine::{Partial, TranscriptionEngine, YieldingStream};
use crate::transcription_models::TranscriptionSelection;

pub(crate) const SAMPLE_RATE: usize = 16_000;
/// How long a stream lives without audio or a heartbeat.
pub(crate) const LEASE: Duration = Duration::from_secs(10);
/// Audio received but not yet transcribed, such as while a long dictation
/// holds the engine. Past it, pushes are refused rather than buffered.
pub(crate) const MAX_BACKLOG_SAMPLES: usize = 120 * SAMPLE_RATE;
/// The longest window; the shared engine transcribes at most 30 s at once.
pub(crate) const MAX_WINDOW_SAMPLES: usize = 25 * SAMPLE_RATE;
/// Audio sent to the engine that it has not consumed yet. A dictation
/// request queues behind at most this much streaming work.
const MAX_IN_FLIGHT_SAMPLES: usize = SAMPLE_RATE * 2 / 5;
/// The smallest feed, so the engine is not sent a command per frame.
const FEED_SAMPLES: usize = SAMPLE_RATE / 10;
/// How long a window's stream may take to finalize once asked.
const STREAM_FINISH_DEADLINE: Duration = Duration::from_secs(3);
const POLL: Duration = Duration::from_millis(50);

/// Why a stream was refused or ended, as the API's error codes.
pub(crate) const MODEL_CHANGED: &str = "model-changed";

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum SampleFormat {
    F32le,
    S16le,
}

impl SampleFormat {
    pub(crate) fn parse(value: &str) -> Option<Self> {
        match value {
            "f32le" => Some(Self::F32le),
            "s16le" => Some(Self::S16le),
            _ => None,
        }
    }

    pub(crate) fn name(self) -> &'static str {
        match self {
            Self::F32le => "f32le",
            Self::S16le => "s16le",
        }
    }

    fn bytes_per_sample(self) -> usize {
        match self {
            Self::F32le => 4,
            Self::S16le => 2,
        }
    }

    /// Decodes little-endian samples; `None` for a partial sample or a
    /// non-finite float.
    pub(crate) fn decode(self, bytes: &[u8]) -> Option<Vec<f32>> {
        if !bytes.len().is_multiple_of(self.bytes_per_sample()) {
            return None;
        }
        match self {
            Self::F32le => bytes
                .as_chunks::<4>()
                .0
                .iter()
                .map(|sample| {
                    let value = f32::from_le_bytes(*sample);
                    value.is_finite().then(|| value.clamp(-1.0, 1.0))
                })
                .collect(),
            Self::S16le => Some(
                bytes
                    .as_chunks::<2>()
                    .0
                    .iter()
                    .map(|sample| f32::from(i16::from_le_bytes(*sample)) / 32_768.0)
                    .collect(),
            ),
        }
    }
}

/// The engine as a stream uses it. Production is [`SharedEngine`]; tests
/// substitute a fake to drive dictation preemption.
pub(crate) trait Engine: Send + 'static {
    fn dictation_busy(&self) -> bool;
    /// Whether the loaded model can stream, so windows report partials.
    fn streams(&self) -> bool;
    /// Whether the engine still holds the stream's model.
    fn holds_model(&self) -> bool;
    /// A window stream that gives way to dictation, when the model streams.
    fn begin(&self) -> Option<Box<dyn Live>>;
    /// Transcribes one window offline once dictation is idle. `stop` ends
    /// the wait early.
    fn transcribe(&self, window: Vec<f32>, stop: &dyn Fn() -> bool) -> Result<String, String>;
    fn quitting(&self) -> bool;
}

pub(crate) trait Live: Send {
    fn open(&self) -> bool;
    fn feed(&mut self, samples: &[f32]);
    fn in_flight(&self) -> usize;
    fn partial(&self) -> Option<Partial>;
    fn finish(self: Box<Self>) -> Result<String, String>;
}

impl Live for YieldingStream {
    fn open(&self) -> bool {
        YieldingStream::open(self)
    }

    fn feed(&mut self, samples: &[f32]) {
        YieldingStream::feed(self, samples);
    }

    fn in_flight(&self) -> usize {
        YieldingStream::in_flight(self)
    }

    fn partial(&self) -> Option<Partial> {
        YieldingStream::partial(self)
    }

    fn finish(self: Box<Self>) -> Result<String, String> {
        YieldingStream::finish(*self, STREAM_FINISH_DEADLINE)
    }
}

impl Engine for Box<dyn Engine> {
    fn dictation_busy(&self) -> bool {
        (**self).dictation_busy()
    }

    fn streams(&self) -> bool {
        (**self).streams()
    }

    fn holds_model(&self) -> bool {
        (**self).holds_model()
    }

    fn begin(&self) -> Option<Box<dyn Live>> {
        (**self).begin()
    }

    fn transcribe(&self, window: Vec<f32>, stop: &dyn Fn() -> bool) -> Result<String, String> {
        (**self).transcribe(window, stop)
    }

    fn quitting(&self) -> bool {
        (**self).quitting()
    }
}

/// The engine a new stream runs on and the model it holds, or the API code
/// for why there is none.
pub(crate) type EngineSource =
    Arc<dyn Fn() -> Result<(Box<dyn Engine>, TranscriptionSelection), &'static str> + Send + Sync>;

/// The running app's shared dictation engine and its loaded GGUF model.
pub(crate) fn shared_engine() -> Result<(Box<dyn Engine>, TranscriptionSelection), &'static str> {
    let engine = TranscriptionEngine::shared().ok_or("engine-unavailable")?;
    let selection = engine.held_selection().ok_or("model-not-loaded")?;
    Ok((
        Box::new(SharedEngine::new(&engine, selection.clone())),
        selection,
    ))
}

/// The running app's dictation engine, held weakly so a stream never keeps
/// the model alive past quitting.
pub(crate) struct SharedEngine {
    engine: Weak<TranscriptionEngine>,
    selection: TranscriptionSelection,
}

impl SharedEngine {
    pub(crate) fn new(
        engine: &Arc<TranscriptionEngine>,
        selection: TranscriptionSelection,
    ) -> Self {
        Self {
            engine: Arc::downgrade(engine),
            selection,
        }
    }
}

impl Engine for SharedEngine {
    fn dictation_busy(&self) -> bool {
        crate::parakeet::dictation_busy()
    }

    fn streams(&self) -> bool {
        self.engine.upgrade().is_some_and(|engine| engine.streams())
    }

    fn holds_model(&self) -> bool {
        self.engine
            .upgrade()
            .is_some_and(|engine| engine.holds(&self.selection))
    }

    fn begin(&self) -> Option<Box<dyn Live>> {
        let stream = self.engine.upgrade()?.begin_yielding()?;
        Some(Box::new(stream))
    }

    fn transcribe(&self, window: Vec<f32>, stop: &dyn Fn() -> bool) -> Result<String, String> {
        while crate::parakeet::dictation_busy() {
            if stop() {
                return Err("the stream ended".into());
            }
            thread::sleep(POLL);
        }
        let selection = self.selection.clone();
        let engine = self
            .engine
            .upgrade()
            .ok_or_else(|| "the transcription engine stopped".to_string())?;
        engine
            .foreground(
                window,
                Box::new(move |model, samples| {
                    // Never load or switch a model for a stream.
                    let transcriber = model
                        .active()
                        .filter(|transcriber| transcriber.matches_selection(&selection))
                        .ok_or_else(|| MODEL_CHANGED.to_string())?;
                    transcriber
                        .transcribe_segments(samples)
                        .map(|transcript| transcript.text)
                        .map_err(|error| format!("{error:#}"))
                }),
            )
            .map_err(|error| format!("{error:#}"))?
            .wait()
            .map(|transcribed| transcribed.text)
    }

    fn quitting(&self) -> bool {
        crate::process_exit::quitting()
    }
}

#[derive(Clone, Debug, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct Segment {
    pub segment: usize,
    pub start_ms: u64,
    pub end_ms: u64,
    pub text: String,
}

/// How a stream ended.
#[derive(Clone, Debug, PartialEq)]
pub(crate) enum End {
    Final {
        text: String,
        segments: Vec<Segment>,
        duration_ms: u64,
    },
    Failed {
        code: &'static str,
        message: String,
    },
    Cancelled,
}

/// One event for an events subscriber, in order.
#[derive(Clone, Debug, PartialEq, Serialize)]
#[serde(tag = "type", rename_all = "camelCase")]
pub(crate) enum Event {
    Partial {
        segment: usize,
        stable: String,
        tentative: String,
    },
    Segment(Segment),
    #[serde(rename_all = "camelCase")]
    Final {
        text: String,
        segments: Vec<Segment>,
        duration_ms: u64,
    },
    Error {
        code: &'static str,
        message: String,
    },
    Cancelled,
}

impl From<&End> for Event {
    fn from(end: &End) -> Self {
        match end.clone() {
            End::Final {
                text,
                segments,
                duration_ms,
            } => Self::Final {
                text,
                segments,
                duration_ms,
            },
            End::Failed { code, message } => Self::Error { code, message },
            End::Cancelled => Self::Cancelled,
        }
    }
}

#[derive(Debug, PartialEq, Eq)]
pub(crate) enum PushError {
    Ended,
    Finishing,
    BacklogFull,
}

struct State {
    /// Audio received that the worker has not taken yet.
    pending: VecDeque<f32>,
    /// Audio received and not yet transcribed (pending plus the worker's).
    backlog: usize,
    received: usize,
    finishing: bool,
    cancelled: bool,
    lease: Duration,
    lease_deadline: Instant,
    segments: Vec<Segment>,
    /// The latest partial and its sequence number.
    partial: Option<(u64, usize, Partial)>,
    end: Option<End>,
    /// Bumped by each events subscriber; an older one stops.
    subscriber: u64,
}

struct Shared {
    state: Mutex<State>,
    changed: Condvar,
}

impl Shared {
    fn lock(&self) -> MutexGuard<'_, State> {
        self.state.lock().unwrap_or_else(|error| error.into_inner())
    }
}

/// One live transcription stream.
pub(crate) struct Stream {
    pub id: u64,
    owner_token: String,
    pub format: SampleFormat,
    pub selection: TranscriptionSelection,
    pub partials: bool,
    shared: Arc<Shared>,
}

impl Stream {
    pub(crate) fn owned_by(&self, token: &str) -> bool {
        self.owner_token == token
    }

    fn ended(&self) -> bool {
        self.shared.lock().end.is_some()
    }

    /// Queues audio for transcription and renews the lease.
    pub(crate) fn push(&self, samples: &[f32]) -> Result<(), PushError> {
        let mut state = self.shared.lock();
        if state.end.is_some() {
            return Err(PushError::Ended);
        }
        if state.finishing {
            return Err(PushError::Finishing);
        }
        if state.backlog + samples.len() > MAX_BACKLOG_SAMPLES {
            return Err(PushError::BacklogFull);
        }
        state.pending.extend(samples);
        state.backlog += samples.len();
        state.received += samples.len();
        state.lease_deadline = Instant::now() + state.lease;
        drop(state);
        self.shared.changed.notify_all();
        Ok(())
    }

    /// Renews the lease; `false` once the stream has ended.
    pub(crate) fn heartbeat(&self) -> bool {
        let mut state = self.shared.lock();
        state.lease_deadline = Instant::now() + state.lease;
        state.end.is_none()
    }

    pub(crate) fn cancel(&self) {
        self.shared.lock().cancelled = true;
        self.shared.changed.notify_all();
    }

    /// Transcribes the rest and waits up to `timeout` for the end, which is
    /// `None` if it has not come by then (the stream keeps going).
    pub(crate) fn finish(&self, timeout: Duration, stop: &dyn Fn() -> bool) -> Option<End> {
        let deadline = Instant::now() + timeout;
        let mut state = self.shared.lock();
        if state.end.is_none() {
            state.finishing = true;
            self.shared.changed.notify_all();
        }
        loop {
            if let Some(end) = &state.end {
                return Some(end.clone());
            }
            let now = Instant::now();
            if now >= deadline || stop() {
                return None;
            }
            state = self
                .shared
                .changed
                .wait_timeout(state, (deadline - now).min(POLL * 4))
                .unwrap_or_else(|error| error.into_inner())
                .0;
        }
    }

    /// Delivers this stream's events from the start to `send` until the
    /// stream ends, a newer subscriber replaces this one, `send` fails, or
    /// `stop` holds. `idle` runs when nothing happened for `keep_alive`, so a
    /// gone subscriber is noticed.
    pub(crate) fn subscribe(
        &self,
        keep_alive: Duration,
        stop: &dyn Fn() -> bool,
        send: &mut dyn FnMut(&Event) -> bool,
        idle: &mut dyn FnMut() -> bool,
    ) {
        let mut state = self.shared.lock();
        state.subscriber += 1;
        let me = state.subscriber;
        self.shared.changed.notify_all();
        let mut sent_segments = 0;
        let mut sent_partial = 0;
        let mut last_sent = Instant::now();
        loop {
            if state.subscriber != me || stop() {
                return;
            }
            let mut events = state.segments[sent_segments..]
                .iter()
                .cloned()
                .map(Event::Segment)
                .collect::<Vec<_>>();
            sent_segments = state.segments.len();
            if let Some((seq, segment, partial)) = &state.partial
                && *seq > sent_partial
            {
                sent_partial = *seq;
                events.push(Event::Partial {
                    segment: *segment,
                    stable: partial.stable.clone(),
                    tentative: partial.tentative.clone(),
                });
            }
            let end = state.end.as_ref().map(Event::from);
            drop(state);
            for event in events.iter().chain(&end) {
                if !send(event) {
                    return;
                }
                last_sent = Instant::now();
            }
            if end.is_some() {
                return;
            }
            if last_sent.elapsed() >= keep_alive {
                if !idle() {
                    return;
                }
                last_sent = Instant::now();
            }
            state = self.shared.lock();
            let nothing_new = state.segments.len() == sent_segments
                && state
                    .partial
                    .as_ref()
                    .is_none_or(|(seq, ..)| *seq <= sent_partial)
                && state.end.is_none()
                && state.subscriber == me;
            if nothing_new {
                state = self
                    .shared
                    .changed
                    .wait_timeout(state, POLL * 4)
                    .unwrap_or_else(|error| error.into_inner())
                    .0;
            }
        }
    }
}

#[derive(Debug, PartialEq, Eq)]
pub(crate) enum CreateError {
    Busy,
    Spawn,
}

/// The local API's streams: at most one is live at a time. An ended stream
/// stays readable (its final, its events) until the next one is created.
pub(crate) struct Streams {
    current: Mutex<Option<Arc<Stream>>>,
    next_id: AtomicU64,
    lease: Duration,
}

impl Default for Streams {
    fn default() -> Self {
        Self {
            current: Mutex::default(),
            next_id: AtomicU64::new(0),
            lease: LEASE,
        }
    }
}

impl Streams {
    #[cfg(test)]
    pub(crate) fn with_lease(lease: Duration) -> Self {
        Self {
            lease,
            ..Self::default()
        }
    }

    pub(crate) fn create(
        &self,
        engine: impl Engine,
        format: SampleFormat,
        selection: TranscriptionSelection,
        owner_token: String,
    ) -> Result<Arc<Stream>, CreateError> {
        let mut current = self
            .current
            .lock()
            .unwrap_or_else(|error| error.into_inner());
        if current.as_ref().is_some_and(|stream| !stream.ended()) {
            return Err(CreateError::Busy);
        }
        let partials = engine.streams();
        let shared = Arc::new(Shared {
            state: Mutex::new(State {
                pending: VecDeque::new(),
                backlog: 0,
                received: 0,
                finishing: false,
                cancelled: false,
                lease: self.lease,
                lease_deadline: Instant::now() + self.lease,
                segments: Vec::new(),
                partial: None,
                end: None,
                subscriber: 0,
            }),
            changed: Condvar::new(),
        });
        let stream = Arc::new(Stream {
            id: self.next_id.fetch_add(1, Ordering::Relaxed) + 1,
            owner_token,
            format,
            selection,
            partials,
            shared: shared.clone(),
        });
        thread::Builder::new()
            .name(format!("transcription-stream-{}", stream.id))
            .spawn(move || Worker::new(shared, engine).run())
            .map_err(|_| CreateError::Spawn)?;
        *current = Some(stream.clone());
        Ok(stream)
    }

    pub(crate) fn get(&self, id: u64) -> Option<Arc<Stream>> {
        self.current
            .lock()
            .unwrap_or_else(|error| error.into_inner())
            .as_ref()
            .filter(|stream| stream.id == id)
            .cloned()
    }
}

/// The audio of the window being transcribed.
struct Window {
    /// Where the window starts in the stream, in samples.
    start: usize,
    audio: Vec<f32>,
    live: Option<Box<dyn Live>>,
    /// How much of `audio` the live stream was sent.
    fed: usize,
}

impl Window {
    fn at(start: usize) -> Self {
        Self {
            start,
            audio: Vec::new(),
            live: None,
            fed: 0,
        }
    }
}

struct Worker<E> {
    shared: Arc<Shared>,
    engine: E,
    window: Window,
    segmenter: PauseSegmenter,
    /// The latest partial's sequence number.
    partial_seq: u64,
}

impl<E: Engine> Worker<E> {
    fn new(shared: Arc<Shared>, engine: E) -> Self {
        Self {
            shared,
            engine,
            window: Window::at(0),
            segmenter: PauseSegmenter::default(),
            partial_seq: 0,
        }
    }

    fn run(mut self) {
        let end = self.transcribe();
        self.window.live = None;
        let mut state = self.shared.lock();
        state.end = Some(end);
        state.pending.clear();
        state.partial = None;
        drop(state);
        self.shared.changed.notify_all();
    }

    fn transcribe(&mut self) -> End {
        loop {
            let busy = self.engine.dictation_busy();
            let (audio, finishing) = {
                let mut state = self.shared.lock();
                if state.cancelled {
                    return End::Cancelled;
                }
                if self.engine.quitting() {
                    return failed("hex-quitting", "Hex is quitting");
                }
                if !state.finishing && Instant::now() >= state.lease_deadline {
                    return failed("lease-expired", "no audio or heartbeat within the lease");
                }
                if busy {
                    // Dictation goes first; audio waits in the backlog.
                    (Vec::new(), false)
                } else {
                    let whole = if state.finishing {
                        state.pending.len()
                    } else {
                        state.pending.len() / FRAME_SAMPLES * FRAME_SAMPLES
                    };
                    (state.pending.drain(..whole).collect(), state.finishing)
                }
            };
            if busy {
                self.publish_partial();
                self.wait(true);
                continue;
            }
            if let Err(end) = self.take(&audio) {
                return end;
            }
            if finishing && self.shared.lock().pending.is_empty() {
                if let Err(end) = self.close_window() {
                    return end;
                }
                let state = self.shared.lock();
                let segments = state.segments.clone();
                let text = segments
                    .iter()
                    .map(|segment| segment.text.as_str())
                    .collect::<Vec<_>>()
                    .join(" ");
                return End::Final {
                    text,
                    segments,
                    duration_ms: samples_ms(state.received),
                };
            }
            self.feed(finishing);
            self.publish_partial();
            if audio.is_empty() {
                self.wait(false);
            }
        }
    }

    /// Waits for audio, a finish, or a cancel; while dictation is busy, for
    /// one poll whatever arrives.
    fn wait(&self, busy: bool) {
        let state = self.shared.lock();
        let ready = state.pending.len() >= FRAME_SAMPLES || state.finishing;
        if !state.cancelled && (busy || !ready) {
            let _ = self.shared.changed.wait_timeout(state, POLL);
        }
    }

    /// Adds audio to windows frame by frame, closing a window at each pause
    /// cut or when it is full.
    fn take(&mut self, audio: &[f32]) -> Result<(), End> {
        for frame in audio.chunks(FRAME_SAMPLES) {
            self.window.audio.extend_from_slice(frame);
            let cut = self.segmenter.push(frame).is_some();
            if cut || self.window.audio.len() >= MAX_WINDOW_SAMPLES {
                self.close_window()?;
            }
        }
        Ok(())
    }

    /// Sends the window's unsent audio to its live stream, opening one if
    /// none is open, while the engine keeps up.
    fn feed(&mut self, finishing: bool) {
        let window = &mut self.window;
        if window.live.as_ref().is_some_and(|live| !live.open()) {
            // Dictation closed it; a new stream starts the window over.
            window.live = None;
            window.fed = 0;
        }
        if window.audio.is_empty() {
            return;
        }
        if window.live.is_none() {
            if !self.engine.holds_model() {
                return;
            }
            window.live = self.engine.begin();
            window.fed = 0;
        }
        let Some(live) = window.live.as_mut() else {
            return;
        };
        while window.fed < window.audio.len() && live.in_flight() < MAX_IN_FLIGHT_SAMPLES {
            let unsent = window.audio.len() - window.fed;
            if unsent < FEED_SAMPLES && !finishing {
                break;
            }
            let take = unsent.min(MAX_IN_FLIGHT_SAMPLES - live.in_flight()).max(1);
            live.feed(&window.audio[window.fed..window.fed + take]);
            window.fed += take;
        }
    }

    fn publish_partial(&mut self) {
        if let Some(live) = self.window.live.as_deref() {
            publish_partial(&self.shared, &mut self.partial_seq, live);
        }
    }

    /// Streams the rest of a complete window through its live stream (or a
    /// new one), no more than [`MAX_IN_FLIGHT_SAMPLES`] ahead of the engine,
    /// so dictation never queues behind it. `None` when dictation took the
    /// engine or the model cannot stream; the window then runs offline.
    fn stream_window(&mut self, window: &mut Window) -> Option<String> {
        if window.live.as_ref().is_some_and(|live| !live.open()) {
            window.live = None;
        }
        if window.live.is_none() {
            if self.engine.dictation_busy() {
                return None;
            }
            window.live = self.engine.begin();
            window.fed = 0;
        }
        let mut live = window.live.take()?;
        while window.fed < window.audio.len() {
            if !live.open()
                || self.engine.dictation_busy()
                || self.engine.quitting()
                || self.shared.lock().cancelled
            {
                return None;
            }
            let room = MAX_IN_FLIGHT_SAMPLES.saturating_sub(live.in_flight());
            if room == 0 {
                thread::sleep(Duration::from_millis(5));
                continue;
            }
            let take = room.min(window.audio.len() - window.fed);
            live.feed(&window.audio[window.fed..window.fed + take]);
            window.fed += take;
            publish_partial(&self.shared, &mut self.partial_seq, &*live);
        }
        live.finish().ok()
    }

    /// Finishes the window: through its live stream when that is still open,
    /// otherwise offline once dictation is idle.
    fn close_window(&mut self) -> Result<(), End> {
        let next = Window::at(self.window.start + self.window.audio.len());
        let mut window = std::mem::replace(&mut self.window, next);
        if window.audio.is_empty() {
            return Ok(());
        }
        if !self.engine.holds_model() {
            return Err(failed(
                MODEL_CHANGED,
                "Hex's dictation model changed during the stream",
            ));
        }
        let text = match self.stream_window(&mut window) {
            Some(text) => text,
            None => {
                let shared = self.shared.clone();
                let engine_quitting = || self.engine.quitting();
                let stop = || shared.lock().cancelled || engine_quitting();
                match self.engine.transcribe(window.audio.clone(), &stop) {
                    Ok(text) => text,
                    Err(_) if self.shared.lock().cancelled => return Err(End::Cancelled),
                    Err(message) if message == MODEL_CHANGED => {
                        return Err(failed(
                            MODEL_CHANGED,
                            "Hex's dictation model changed during the stream",
                        ));
                    }
                    Err(message) => return Err(failed("transcription-failed", &message)),
                }
            }
        };
        let text = text.trim().to_string();
        let mut state = self.shared.lock();
        state.backlog = state.backlog.saturating_sub(window.audio.len());
        state.partial = None;
        if !text.is_empty() {
            let segment = Segment {
                segment: state.segments.len(),
                start_ms: samples_ms(window.start),
                end_ms: samples_ms(window.start + window.audio.len()),
                text,
            };
            state.segments.push(segment);
        }
        drop(state);
        self.shared.changed.notify_all();
        Ok(())
    }
}

fn publish_partial(shared: &Shared, seq: &mut u64, live: &dyn Live) {
    let Some(partial) = live.open().then(|| live.partial()).flatten() else {
        return;
    };
    let mut state = shared.lock();
    let segment = state.segments.len();
    if state
        .partial
        .as_ref()
        .is_some_and(|(_, at, current)| *at == segment && *current == partial)
    {
        return;
    }
    *seq += 1;
    state.partial = Some((*seq, segment, partial));
    drop(state);
    shared.changed.notify_all();
}

fn failed(code: &'static str, message: &str) -> End {
    End::Failed {
        code,
        message: message.into(),
    }
}

fn samples_ms(samples: usize) -> u64 {
    (samples * 1_000 / SAMPLE_RATE) as u64
}

#[cfg(test)]
#[path = "transcription_stream_tests.rs"]
pub(crate) mod tests;
