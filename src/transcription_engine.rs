//! The thread that owns the loaded GGUF model and runs all of its inference.
//!
//! A capture that the model can stream is fed while the shortcut is held, so
//! release only decodes the last second or two of audio. Every finish carries
//! the complete clip: a stream that failed, stalled, or produced no text is
//! replaced by an offline transcription of that clip, and a model or device
//! that cannot stream always takes the offline path. Commands are handled in
//! order on one thread, because a live stream holds the model's compute lease.

use std::collections::VecDeque;
use std::sync::atomic::{AtomicBool, AtomicU64, AtomicUsize, Ordering};
use std::sync::mpsc::{self, Receiver, RecvTimeoutError, Sender, SyncSender};
use std::sync::{Arc, Mutex};
use std::thread::{self, JoinHandle};
use std::time::{Duration, Instant};

use color_eyre::eyre::{Result, eyre};
use transcribe_cpp::CancelToken;

/// The model the engine owns: macOS keeps its warm, switchable transcriber
/// (GGUF or Apple Speech); Linux and Windows load one GGUF model.
#[cfg(target_os = "macos")]
pub(crate) type Model = crate::transcription::WarmTranscriber;
#[cfg(not(target_os = "macos"))]
pub(crate) type Model = crate::local_transcriber::LocalTranscriber;

/// Transcribes a complete 16 kHz clip offline on the engine thread.
pub(crate) type Offline = Box<dyn FnOnce(&mut Model, Vec<f32>) -> Result<String, String> + Send>;

/// Low-priority work on the model, such as a meeting chunk. It receives
/// `None` instead of the model when dictation claimed the engine first.
pub(crate) type Background = Box<dyn FnOnce(Option<&mut Model>) + Send>;

/// A healthy finish decodes at most a couple of seconds of buffered audio.
/// Past this the stream is aborted and the clip is transcribed offline.
const STREAM_FINISH_DEADLINE: Duration = Duration::from_millis(1_500);
/// Audio fed past the clip's trimmed end (a release handled late) that the
/// stream may still keep. Beyond it, the offline clip is authoritative.
const MAX_OVERFED_SAMPLES: usize = 16_000 / 4;
/// How long a job may run once the engine starts it, before the clip's own
/// share. It covers activating and prewarming the largest model inside the
/// job (macOS loads a new selection there). Time queued behind other work is
/// never counted: the deadline only catches a wedged engine, never a slow one.
const JOB_DEADLINE: Duration = Duration::from_secs(180);
/// Run time allowed per second of audio: two passes (voice-delimited
/// dictation retranscribes without its control phrase) on a CPU-only device
/// up to four times slower than real time.
const JOB_SECONDS_PER_AUDIO_SECOND: u32 = 8;
/// How often a waiter whose job has not started checks whether the job ahead
/// of it timed out, leaving the engine wedged.
const QUEUE_POLL: Duration = Duration::from_millis(250);
/// How long dropping the engine waits for its thread. A native run that cannot
/// be aborted is left to finish on its own.
const SHUTDOWN_DEADLINE: Duration = Duration::from_secs(10);

#[derive(Clone, Copy)]
struct Deadlines {
    stream_finish: Duration,
    job: Duration,
    job_seconds_per_audio_second: u32,
    shutdown: Duration,
}

impl Deadlines {
    const PRODUCTION: Self = Self {
        stream_finish: STREAM_FINISH_DEADLINE,
        job: JOB_DEADLINE,
        job_seconds_per_audio_second: JOB_SECONDS_PER_AUDIO_SECOND,
        shutdown: SHUTDOWN_DEADLINE,
    };

    fn job_for(&self, samples: usize) -> Duration {
        self.job
            + Duration::from_secs_f64(samples as f64 / 16_000.0) * self.job_seconds_per_audio_second
    }
}

pub(crate) enum Command {
    Begin {
        stream: u64,
        /// Aborts this stream only.
        cancel: CancelToken,
        /// Where the stream publishes its text after each feed, when live
        /// text is on (the `live-text` experiment).
        partials: Option<Partials>,
    },
    Feed {
        stream: u64,
        samples: Vec<f32>,
    },
    Cancel {
        stream: u64,
    },
    Finish {
        stream: Option<u64>,
        clip: Vec<f32>,
        offline: Offline,
        job: Job,
    },
    /// An offline run that waits for an open live stream to end instead of
    /// interrupting it, such as a capture's word chunk or a `/streams`
    /// window. Like a finish it is a dictation request, so background work
    /// sent before it yields.
    #[cfg_attr(not(target_os = "macos"), allow(dead_code))]
    Foreground {
        clip: Vec<f32>,
        offline: Offline,
        job: Job,
    },
    /// A live stream that gives way to dictation (`/streams`): any dictation
    /// request closes it, and while a dictation stream is open it is refused
    /// instead of interrupting it.
    #[cfg_attr(not(target_os = "macos"), allow(dead_code))]
    BeginYielding {
        stream: u64,
        cancel: CancelToken,
        yielding: Yielding,
    },
    /// Finalizes a yielding stream, or answers [`PREEMPTED`] at once when it
    /// is no longer open. It never runs anything offline.
    #[cfg_attr(not(target_os = "macos"), allow(dead_code))]
    FinishYielding {
        stream: u64,
        reply: SyncSender<Result<String, String>>,
    },
    /// Work that needs the model itself, such as loading or switching it.
    #[cfg_attr(not(target_os = "macos"), allow(dead_code))]
    Run(Box<dyn FnOnce(&mut Model) + Send>),
    #[cfg_attr(not(target_os = "macos"), allow(dead_code))]
    Background {
        /// The dictation generation when the work was submitted.
        generation: u64,
        work: Background,
    },
    Shutdown,
}

/// Why a yielding stream ended without its text: dictation needed the engine.
pub(crate) const PREEMPTED: &str = "the stream gave way to dictation";

/// A live stream's text so far, as the model reports it after each feed.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Partial {
    /// The prefix the model has committed; it only grows within one stream.
    pub stable: String,
    /// The model's volatile hypothesis after `stable`.
    pub tentative: String,
}

/// One stream's latest partial text, shared between the worker that
/// publishes it and the stream's owner. Each stream has its own, so a new
/// capture never reads an older one's text.
#[derive(Clone, Default)]
pub(crate) struct Partials(Arc<Mutex<Published>>);

#[derive(Default)]
struct Published {
    /// Bumped by each change, so an owner can tell what it has not seen.
    seq: u64,
    partial: Option<Partial>,
}

impl Partials {
    /// Publishes `partial`; returns whether it changed the text.
    pub(crate) fn publish(&self, partial: Partial) -> bool {
        let mut published = self.0.lock().unwrap_or_else(|error| error.into_inner());
        if published.partial.as_ref() == Some(&partial) {
            return false;
        }
        published.seq += 1;
        published.partial = Some(partial);
        true
    }

    fn latest(&self) -> Option<Partial> {
        self.0
            .lock()
            .unwrap_or_else(|error| error.into_inner())
            .partial
            .clone()
    }

    /// The latest partial when it is newer than `seen`, which it advances.
    fn newer_than(&self, seen: &mut u64) -> Option<Partial> {
        let published = self.0.lock().unwrap_or_else(|error| error.into_inner());
        if published.seq <= *seen {
            return None;
        }
        *seen = published.seq;
        published.partial.clone()
    }
}

/// What the worker shares with a yielding stream's owner: whether the stream
/// is still open on the engine, and its latest partial text.
#[derive(Clone)]
pub(crate) struct Yielding {
    open: Arc<AtomicBool>,
    /// Samples the model has consumed.
    consumed: Arc<AtomicUsize>,
    partials: Partials,
}

impl Yielding {
    fn new() -> Self {
        Self {
            open: Arc::new(AtomicBool::new(true)),
            consumed: Arc::default(),
            partials: Partials::default(),
        }
    }

    fn close(&self) {
        self.open.store(false, Ordering::Release);
    }
}

/// A live stream that gives way to dictation (`/streams`). Unlike a
/// [`LiveStream`] it is not a dictation request: background work sent before
/// it still runs, and any dictation request closes it. Feed it 16 kHz audio,
/// then [`Self::finish`] it or drop it to cancel.
#[cfg_attr(not(target_os = "macos"), allow(dead_code))]
pub(crate) struct YieldingStream {
    id: u64,
    cancel: CancelToken,
    yielding: Yielding,
    sent: usize,
    finished: bool,
    commands: Sender<Command>,
}

#[cfg_attr(not(target_os = "macos"), allow(dead_code))]
impl YieldingStream {
    /// Whether the stream is still open on the engine. Once dictation closes
    /// it, its audio must be transcribed some other way.
    pub(crate) fn open(&self) -> bool {
        self.yielding.open.load(Ordering::Acquire)
    }

    pub(crate) fn feed(&mut self, samples: &[f32]) {
        if samples.is_empty() || !self.open() {
            return;
        }
        self.sent += samples.len();
        let _ = self.commands.send(Command::Feed {
            stream: self.id,
            samples: samples.to_vec(),
        });
    }

    /// Samples sent that the model has not consumed yet. Keeping this small
    /// keeps a dictation request from queueing behind a pile of feeds.
    pub(crate) fn in_flight(&self) -> usize {
        self.sent
            .saturating_sub(self.yielding.consumed.load(Ordering::Acquire))
    }

    /// The model's latest text for the audio fed so far.
    pub(crate) fn partial(&self) -> Option<Partial> {
        self.yielding.partials.latest()
    }

    /// Finalizes the stream within `deadline`. It fails with [`PREEMPTED`]
    /// when dictation closed it first, and is aborted when the engine does
    /// not reach it in time; either way nothing was lost but this text.
    pub(crate) fn finish(mut self, deadline: Duration) -> Result<String, String> {
        self.finished = true;
        if !self.open() {
            return Err(PREEMPTED.into());
        }
        let (reply, receiver) = mpsc::sync_channel(1);
        if self
            .commands
            .send(Command::FinishYielding {
                stream: self.id,
                reply,
            })
            .is_err()
        {
            return Err("transcription engine stopped".into());
        }
        match receiver.recv_timeout(deadline) {
            Ok(result) => result,
            Err(RecvTimeoutError::Timeout) => {
                self.cancel.cancel();
                Err("the stream missed its finish deadline".into())
            }
            Err(RecvTimeoutError::Disconnected) => Err("transcription engine stopped".into()),
        }
    }
}

impl Drop for YieldingStream {
    fn drop(&mut self) {
        if !self.finished {
            let _ = self.commands.send(Command::Cancel { stream: self.id });
        }
    }
}

/// One finish request as the worker sees it: its own cancellation, so a
/// timeout aborts this job and never a later capture, and its reply.
pub(crate) struct Job {
    cancel: CancelToken,
    reply: Sender<Reply>,
}

impl Job {
    fn start(&self) {
        let _ = self.reply.send(Reply::Started);
    }

    fn finish(self, result: Result<Transcribed, String>) {
        let _ = self.reply.send(Reply::Done(result));
    }
}

enum Reply {
    /// The engine began this job; its deadline starts now.
    Started,
    Done(Result<Transcribed, String>),
}

#[derive(Debug)]
pub struct Transcribed {
    pub text: String,
    pub source: Source,
    /// Time from the finish request to the final text.
    pub finish_ms: u64,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Source {
    Stream,
    Offline,
    /// The stream could not be used; the reason is logged with the result.
    Fallback(String),
}

impl Source {
    pub fn label(&self) -> &str {
        match self {
            Self::Stream => "stream",
            Self::Offline => "offline",
            Self::Fallback(_) => "fallback",
        }
    }
}

/// The token of the job or stream the worker is running, so dropping the
/// engine aborts it. Work entered after the engine stops starts cancelled.
#[derive(Clone, Default)]
struct Running(Arc<Mutex<RunningState>>);

#[derive(Default)]
struct RunningState {
    stopping: bool,
    cancel: Option<CancelToken>,
}

impl Running {
    fn enter(&self, cancel: &CancelToken) {
        let mut state = self.0.lock().unwrap_or_else(|error| error.into_inner());
        if state.stopping {
            cancel.cancel();
        }
        state.cancel = Some(cancel.clone());
    }

    fn stop(&self) {
        let mut state = self.0.lock().unwrap_or_else(|error| error.into_inner());
        state.stopping = true;
        if let Some(cancel) = &state.cancel {
            cancel.cancel();
        }
    }
}

pub struct TranscriptionEngine {
    commands: Sender<Command>,
    running: Running,
    /// Set when a job misses its deadline and cleared when the worker starts
    /// another: jobs queued behind a wedged run fail instead of waiting.
    stalled: Arc<AtomicBool>,
    /// Dictation requests sent so far. Background work submitted before a
    /// newer request yields to it.
    requests: Arc<AtomicU64>,
    /// Whether the loaded model can stream on its device.
    streams: Arc<AtomicBool>,
    /// Whether dictation streams publish their text as they go (the
    /// `live-text` experiment).
    live_text: bool,
    #[cfg(target_os = "macos")]
    held: Held,
    next_stream: AtomicU64,
    deadlines: Deadlines,
    thread: Option<JoinHandle<()>>,
}

/// The GGUF selection the engine's model held after its last command.
#[cfg(target_os = "macos")]
type Held = Arc<Mutex<Option<crate::transcription_models::TranscriptionSelection>>>;

/// A live stream for one capture. Feed it the capture's 16 kHz audio in order,
/// then either [`TranscriptionEngine::finish`] or drop it to cancel.
pub struct LiveStream {
    id: u64,
    cancel: CancelToken,
    fed: usize,
    finished: bool,
    commands: Sender<Command>,
    /// The stream's text so far, with live text on, and the last version
    /// [`Self::take_partial`] returned.
    partials: Option<(Partials, u64)>,
}

impl LiveStream {
    /// Total samples handed to the stream so far.
    pub fn fed(&self) -> usize {
        self.fed
    }

    /// The model's text for the audio fed so far, when it changed since the
    /// last call. Always `None` without live text. It is provisional: the
    /// finish may replace it entirely, so it is only ever shown, never
    /// pasted or kept.
    pub fn take_partial(&mut self) -> Option<Partial> {
        let (partials, seen) = self.partials.as_mut()?;
        partials.newer_than(seen)
    }

    pub fn feed(&mut self, samples: &[f32]) {
        if samples.is_empty() {
            return;
        }
        self.fed += samples.len();
        let _ = self.commands.send(Command::Feed {
            stream: self.id,
            samples: samples.to_vec(),
        });
    }
}

impl Drop for LiveStream {
    fn drop(&mut self) {
        if !self.finished {
            let _ = self.commands.send(Command::Cancel { stream: self.id });
        }
    }
}

/// A finish in flight. Waiting enforces the stream deadline from the caller's
/// side, so a stalled native stream cannot hold the paste forever, then bounds
/// the job once the engine starts it, so a wedged run fails instead.
pub struct PendingTranscript {
    reply: Receiver<Reply>,
    cancel: CancelToken,
    stream_cancel: Option<CancelToken>,
    stalled: Arc<AtomicBool>,
    /// How long the job may run once started: its allowance plus the clip's.
    job: Duration,
    stream_finish: Duration,
}

impl PendingTranscript {
    pub fn wait(self) -> Result<Transcribed, String> {
        let mut stream_deadline = self
            .stream_cancel
            .as_ref()
            .map(|_| Instant::now() + self.stream_finish);
        let mut job_deadline: Option<Instant> = None;
        loop {
            let now = Instant::now();
            let timeout = [stream_deadline, job_deadline]
                .into_iter()
                .flatten()
                .min()
                .map_or(QUEUE_POLL, |deadline| {
                    deadline.saturating_duration_since(now).min(QUEUE_POLL)
                });
            match self.reply.recv_timeout(timeout) {
                Ok(Reply::Done(result)) => return result,
                Ok(Reply::Started) => job_deadline = Some(Instant::now() + self.job),
                Err(RecvTimeoutError::Disconnected) => {
                    return Err("transcription engine stopped".into());
                }
                Err(RecvTimeoutError::Timeout) => {
                    let now = Instant::now();
                    if let (Some(deadline), Some(stream)) = (stream_deadline, &self.stream_cancel)
                        && now >= deadline
                    {
                        tracing::warn!("live stream missed its finish deadline; aborting it");
                        stream.cancel();
                        stream_deadline = None;
                    }
                    if job_deadline.is_some_and(|deadline| now >= deadline) {
                        tracing::error!(
                            deadline_ms = self.job.as_millis() as u64,
                            "transcription missed its deadline; failing the job"
                        );
                        self.stalled.store(true, Ordering::Release);
                        self.abandon();
                        return Err("transcription timed out".into());
                    }
                    if job_deadline.is_none() && self.stalled.load(Ordering::Acquire) {
                        tracing::error!("transcription engine is stuck; failing a queued job");
                        self.abandon();
                        return Err("transcription engine is stuck".into());
                    }
                }
            }
        }
    }

    /// Aborts this job's run, or skips it if the engine has not reached it.
    fn abandon(&self) {
        self.cancel.cancel();
        if let Some(stream) = &self.stream_cancel {
            stream.cancel();
        }
    }
}

impl TranscriptionEngine {
    pub fn start(model: Model) -> Result<Self> {
        let streams = Arc::new(AtomicBool::new(model.stream_options().is_some()));
        let (commands, receiver) = mpsc::channel();
        let requests = Arc::new(AtomicU64::new(0));
        let running = Running::default();
        let stalled = Arc::new(AtomicBool::new(false));
        #[cfg(target_os = "macos")]
        let held = Held::new(Mutex::new(model.gguf_selection().cloned()));
        let worker = Worker {
            model,
            running: running.clone(),
            stalled: stalled.clone(),
            streams: streams.clone(),
            requests: requests.clone(),
            #[cfg(target_os = "macos")]
            held: held.clone(),
        };
        let thread = thread::Builder::new()
            .name("transcription-engine".into())
            .spawn(move || worker.run(receiver))?;
        Ok(Self {
            commands,
            running,
            stalled,
            requests,
            streams,
            live_text: crate::experiments::enabled(crate::experiments::Experiment::LiveText),
            #[cfg(target_os = "macos")]
            held,
            next_stream: AtomicU64::new(0),
            deadlines: Deadlines::PRODUCTION,
            thread: Some(thread),
        })
    }

    /// An engine without a worker whose model always streams; tests read the
    /// commands it would have handled.
    #[cfg(test)]
    pub(crate) fn for_test() -> (Self, Receiver<Command>) {
        let (commands, receiver) = mpsc::channel();
        let engine = Self {
            commands,
            running: Running::default(),
            stalled: Arc::default(),
            requests: Arc::default(),
            streams: Arc::new(AtomicBool::new(true)),
            live_text: false,
            #[cfg(target_os = "macos")]
            held: Held::default(),
            next_stream: AtomicU64::new(0),
            deadlines: Deadlines::PRODUCTION,
            thread: None,
        };
        (engine, receiver)
    }

    /// Like [`Self::for_test`] for a model that cannot stream.
    #[cfg(test)]
    pub(crate) fn for_test_without_streaming() -> (Self, Receiver<Command>) {
        let (engine, receiver) = Self::for_test();
        engine.streams.store(false, Ordering::Release);
        (engine, receiver)
    }

    #[cfg(test)]
    fn with_deadlines(mut self, deadlines: Deadlines) -> Self {
        self.deadlines = deadlines;
        self
    }

    /// Turns live text on, as the `live-text` experiment does.
    #[cfg(test)]
    pub(crate) fn with_live_text(mut self) -> Self {
        self.live_text = true;
        self
    }

    /// Opens a live stream when the loaded model can stream on this device.
    pub fn begin(&self) -> Option<LiveStream> {
        if !self.streams.load(Ordering::Acquire) {
            return None;
        }
        let id = self.next_stream.fetch_add(1, Ordering::Relaxed) + 1;
        let cancel = CancelToken::new();
        let partials = self.live_text.then(Partials::default);
        self.requests.fetch_add(1, Ordering::AcqRel);
        self.commands
            .send(Command::Begin {
                stream: id,
                cancel: cancel.clone(),
                partials: partials.clone(),
            })
            .ok()?;
        Some(LiveStream {
            id,
            cancel,
            fed: 0,
            finished: false,
            commands: self.commands.clone(),
            partials: partials.map(|partials| (partials, 0)),
        })
    }

    /// Opens a stream that gives way to dictation, when the loaded model can
    /// stream on this device. See [`YieldingStream`].
    #[cfg_attr(not(target_os = "macos"), allow(dead_code))]
    pub(crate) fn begin_yielding(&self) -> Option<YieldingStream> {
        if !self.streams.load(Ordering::Acquire) {
            return None;
        }
        let id = self.next_stream.fetch_add(1, Ordering::Relaxed) + 1;
        let cancel = CancelToken::new();
        let yielding = Yielding::new();
        self.commands
            .send(Command::BeginYielding {
                stream: id,
                cancel: cancel.clone(),
                yielding: yielding.clone(),
            })
            .ok()?;
        Some(YieldingStream {
            id,
            cancel,
            yielding,
            sent: 0,
            finished: false,
            commands: self.commands.clone(),
        })
    }

    /// Whether the loaded model can stream on its device.
    #[cfg_attr(not(target_os = "macos"), allow(dead_code))]
    pub(crate) fn streams(&self) -> bool {
        self.streams.load(Ordering::Acquire)
    }

    /// Finishes a capture with its complete 16 kHz clip. Without a stream,
    /// or when the stream is unusable, the clip is transcribed offline.
    #[cfg(not(target_os = "macos"))]
    pub fn finish(&self, stream: Option<LiveStream>, clip: Vec<f32>) -> Result<PendingTranscript> {
        self.finish_with(
            stream,
            clip,
            Box::new(|model, mut clip| {
                crate::dictation::pad_for_parakeet(&mut clip);
                model
                    .transcribe(&clip)
                    .map_err(|error| format!("{error:#}"))
            }),
        )
    }

    /// Like `finish`, with the caller's offline transcription of the clip.
    pub fn finish_with(
        &self,
        stream: Option<LiveStream>,
        clip: Vec<f32>,
        offline: Offline,
    ) -> Result<PendingTranscript> {
        let (stream, stream_cancel) = match stream {
            Some(mut stream) => {
                stream.finished = true;
                (Some(stream.id), Some(stream.cancel.clone()))
            }
            None => (None, None),
        };
        self.request(clip.len(), stream_cancel, |job| Command::Finish {
            stream,
            clip,
            offline,
            job,
        })
    }

    /// Transcribes `clip` offline after every command already sent, without
    /// ever interrupting a live stream: one open when it arrives finishes
    /// first. Background work sent before it yields, as to a finish.
    #[cfg_attr(not(target_os = "macos"), allow(dead_code))]
    pub fn foreground(&self, clip: Vec<f32>, offline: Offline) -> Result<PendingTranscript> {
        self.request(clip.len(), None, |job| Command::Foreground {
            clip,
            offline,
            job,
        })
    }

    fn request(
        &self,
        samples: usize,
        stream_cancel: Option<CancelToken>,
        command: impl FnOnce(Job) -> Command,
    ) -> Result<PendingTranscript> {
        let (reply, receiver) = mpsc::channel();
        let cancel = CancelToken::new();
        let job = self.deadlines.job_for(samples);
        self.requests.fetch_add(1, Ordering::AcqRel);
        self.commands
            .send(command(Job {
                cancel: cancel.clone(),
                reply,
            }))
            .map_err(|_| eyre!("transcription engine stopped"))?;
        Ok(PendingTranscript {
            reply: receiver,
            cancel,
            stream_cancel,
            stalled: self.stalled.clone(),
            job,
            stream_finish: self.deadlines.stream_finish,
        })
    }

    /// Runs `work` with the model, after every command already sent. A live
    /// stream is closed first; its finish then takes the offline path.
    #[cfg(target_os = "macos")]
    pub fn run(&self, work: impl FnOnce(&mut Model) + Send + 'static) -> Result<()> {
        self.commands
            .send(Command::Run(Box::new(work)))
            .map_err(|_| eyre!("transcription engine stopped"))
    }

    /// Runs `work` with the model unless a dictation request was sent first or
    /// a live stream is open, and returns `None` when one was. A request sent
    /// while `work` runs waits for it: transcribe.cpp cannot abort a Metal run.
    #[cfg(target_os = "macos")]
    pub fn background<R: Send + 'static>(
        &self,
        work: impl FnOnce(&mut Model) -> R + Send + 'static,
    ) -> Result<Option<R>> {
        let (reply, receiver) = mpsc::channel();
        let generation = self.requests.load(Ordering::Acquire);
        self.commands
            .send(Command::Background {
                generation,
                work: Box::new(move |model| {
                    let _ = reply.send(model.map(work));
                }),
            })
            .map_err(|_| eyre!("transcription engine stopped"))?;
        receiver
            .recv()
            .map_err(|_| eyre!("transcription engine stopped"))
    }

    /// Drops the engine on its own thread, so replacing the model from the
    /// hotkey loop never waits on a run still in flight.
    #[cfg_attr(target_os = "macos", allow(dead_code))]
    pub fn retire(self) {
        let spawned = thread::Builder::new()
            .name("transcription-engine-retire".into())
            .spawn(move || drop(self));
        if let Err(error) = spawned {
            tracing::warn!(%error, "could not retire the old transcription engine in the background");
        }
    }

    /// Makes this the engine that meeting transcription and the local API share.
    #[cfg(target_os = "macos")]
    pub fn share(self: &Arc<Self>) {
        *SHARED.lock().unwrap_or_else(|error| error.into_inner()) = Arc::downgrade(self);
    }

    /// The running app's engine, when one is shared.
    #[cfg(target_os = "macos")]
    pub fn shared() -> Option<Arc<Self>> {
        SHARED
            .lock()
            .unwrap_or_else(|error| error.into_inner())
            .upgrade()
    }

    /// Whether the model holds the GGUF `selection` as of its last finished
    /// command. Readable while dictation runs, unlike the model itself.
    #[cfg(target_os = "macos")]
    pub(crate) fn holds(
        &self,
        selection: &crate::transcription_models::TranscriptionSelection,
    ) -> bool {
        self.held
            .lock()
            .unwrap_or_else(|error| error.into_inner())
            .as_ref()
            == Some(selection)
    }
}

#[cfg(target_os = "macos")]
impl TranscriptionEngine {
    /// The GGUF selection the model held after its last finished command.
    pub(crate) fn held_selection(
        &self,
    ) -> Option<crate::transcription_models::TranscriptionSelection> {
        self.held
            .lock()
            .unwrap_or_else(|error| error.into_inner())
            .clone()
    }
}

#[cfg(target_os = "macos")]
static SHARED: std::sync::Mutex<std::sync::Weak<TranscriptionEngine>> =
    std::sync::Mutex::new(std::sync::Weak::new());

impl Drop for TranscriptionEngine {
    fn drop(&mut self) {
        self.running.stop();
        // A live stream's sender may outlive the engine; stop explicitly.
        let _ = self.commands.send(Command::Shutdown);
        let Some(thread) = self.thread.take() else {
            return;
        };
        let deadline = Instant::now() + self.deadlines.shutdown;
        while !thread.is_finished() && Instant::now() < deadline {
            thread::sleep(Duration::from_millis(5));
        }
        if thread.is_finished() {
            let _ = thread.join();
        } else {
            tracing::error!("transcription engine did not stop in time; leaving it running");
        }
    }
}

struct Worker {
    model: Model,
    running: Running,
    stalled: Arc<AtomicBool>,
    streams: Arc<AtomicBool>,
    requests: Arc<AtomicU64>,
    #[cfg(target_os = "macos")]
    held: Held,
}

/// How a live stream ended inside the worker.
enum StreamEnd {
    /// A finish for this stream: the final text, or why the stream is unusable.
    Finished {
        result: Result<String, String>,
        clip: Vec<f32>,
        offline: Offline,
        job: Job,
        started: Instant,
    },
    Cancelled,
    /// Another command arrived first; it is handled after the stream closes.
    Interrupted(Command),
    Disconnected,
}

impl Worker {
    fn run(mut self, commands: Receiver<Command>) {
        // Commands received while a stream was open, handled before new ones.
        let mut next = VecDeque::new();
        loop {
            let Some(command) = next.pop_front().or_else(|| commands.recv().ok()) else {
                return;
            };
            match command {
                Command::Begin {
                    stream,
                    cancel,
                    partials,
                } => {
                    self.stream(stream, &cancel, partials.as_ref(), &commands, &mut next);
                }
                Command::BeginYielding {
                    stream,
                    cancel,
                    yielding,
                } => {
                    // Anything already waiting comes first.
                    if next.is_empty() {
                        self.yielding_stream(stream, &cancel, &yielding, &commands, &mut next);
                    }
                    yielding.close();
                }
                Command::FinishYielding { reply, .. } => {
                    let _ = reply.send(Err(PREEMPTED.into()));
                }
                Command::Finish {
                    stream,
                    clip,
                    offline,
                    job,
                } => {
                    let source = match stream {
                        // The stream was already closed: its begin failed or
                        // another command interrupted it.
                        Some(_) => Source::Fallback("the live stream was not available".into()),
                        None => Source::Offline,
                    };
                    self.run_offline(clip, offline, source, job);
                }
                Command::Foreground { clip, offline, job } => {
                    self.run_offline(clip, offline, Source::Offline, job);
                }
                Command::Run(work) => {
                    work(&mut self.model);
                    self.streams
                        .store(self.model.stream_options().is_some(), Ordering::Release);
                }
                Command::Background { generation, work } => {
                    let current = self.requests.load(Ordering::Acquire) == generation;
                    work(current.then_some(&mut self.model));
                }
                Command::Shutdown => return,
                // Stale input for a stream that already ended.
                Command::Feed { .. } | Command::Cancel { .. } => {}
            }
            #[cfg(target_os = "macos")]
            self.publish_selection();
        }
    }

    /// Any command may load or switch the model (a dictation job activates
    /// its selection), so the held selection is refreshed after each.
    #[cfg(target_os = "macos")]
    fn publish_selection(&self) {
        let current = self.model.gguf_selection();
        let mut held = self.held.lock().unwrap_or_else(|error| error.into_inner());
        if held.as_ref() != current {
            *held = current.cloned();
        }
    }

    fn run_offline(&mut self, clip: Vec<f32>, offline: Offline, source: Source, job: Job) {
        if job.cancel.is_cancelled() {
            // Its waiter already gave up on it.
            job.finish(Err("transcription was abandoned".into()));
            return;
        }
        let started = Instant::now();
        self.start(&job);
        self.offline(clip, offline, source, started, job);
    }

    /// Starts a job's clock. The engine is evidently moving again.
    fn start(&self, job: &Job) {
        self.stalled.store(false, Ordering::Release);
        job.start();
    }

    /// Drives one live stream until it finishes, is cancelled, or another
    /// command interrupts it. Commands still to be handled, in order, are
    /// pushed onto `next`.
    fn stream(
        &mut self,
        id: u64,
        cancel: &CancelToken,
        partials: Option<&Partials>,
        commands: &Receiver<Command>,
        next: &mut VecDeque<Command>,
    ) {
        let Some(options) = self.model.stream_options().cloned() else {
            return;
        };
        self.running.enter(cancel);
        let begun = Instant::now();
        let mut stats = FeedStats::default();
        let stalled = &self.stalled;
        let ended = self.model.with_stream(&options, cancel, |stream| {
            let mut capped = PauseCap::new(stream);
            let ended = drive_stream(
                id,
                &mut capped,
                commands,
                stalled,
                partials,
                next,
                &mut stats,
            );
            stats.pauses_dropped_ms = capped.dropped / 16;
            ended
        });
        let FeedStats {
            feed_ms,
            feeds,
            partials: changes,
            pauses_dropped_ms,
        } = stats;
        match ended {
            Ok(StreamEnd::Finished {
                result,
                clip,
                offline,
                job,
                started,
            }) => {
                tracing::info!(
                    audio_ms = clip.len() / 16,
                    stream_ms = begun.elapsed().as_millis(),
                    feeds,
                    feed_ms,
                    partials = changes,
                    pauses_dropped_ms,
                    finish_ms = started.elapsed().as_millis(),
                    "finished live stream"
                );
                match result {
                    Ok(text) => job.finish(Ok(Transcribed {
                        text,
                        source: Source::Stream,
                        finish_ms: started.elapsed().as_millis() as u64,
                    })),
                    Err(_) if job.cancel.is_cancelled() => {
                        job.finish(Err("transcription was abandoned".into()));
                    }
                    Err(reason) => {
                        tracing::warn!(%reason, "live stream unusable; transcribing the full clip");
                        self.offline(clip, offline, Source::Fallback(reason), started, job);
                    }
                }
            }
            Ok(StreamEnd::Cancelled) | Ok(StreamEnd::Disconnected) => {}
            Ok(StreamEnd::Interrupted(command)) => {
                // The capture goes on without its stream (a model switch):
                // its text stops, so the plate hides.
                if let Some(partials) = partials {
                    partials.publish(Partial::default());
                }
                next.push_back(command);
            }
            Err(error) => tracing::warn!(%error, "could not begin a live stream"),
        }
    }

    /// Drives one yielding stream until it finishes, is cancelled, or any
    /// command that is not its own, background, or foreground work arrives;
    /// that command is handled next.
    fn yielding_stream(
        &mut self,
        id: u64,
        cancel: &CancelToken,
        yielding: &Yielding,
        commands: &Receiver<Command>,
        next: &mut VecDeque<Command>,
    ) {
        let Some(options) = self.model.stream_options().cloned() else {
            return;
        };
        if cancel.is_cancelled() {
            return;
        }
        self.running.enter(cancel);
        let ended = self.model.with_stream(&options, cancel, |stream| {
            drive_yielding(id, stream, commands, yielding, next)
        });
        match ended {
            Ok(Some(command)) => next.push_back(command),
            Ok(None) => {}
            Err(error) => tracing::warn!(%error, "could not begin a yielding stream"),
        }
    }

    fn offline(
        &mut self,
        clip: Vec<f32>,
        offline: Offline,
        source: Source,
        started: Instant,
        job: Job,
    ) {
        self.running.enter(&job.cancel);
        let model = &mut self.model;
        let result = crate::gguf_session::with_job_cancel(&job.cancel, || offline(model, clip))
            .map(|text| Transcribed {
                text: text.trim().to_string(),
                source,
                finish_ms: started.elapsed().as_millis() as u64,
            });
        // An offline run may switch the model (macOS activates a selection).
        self.streams
            .store(self.model.stream_options().is_some(), Ordering::Release);
        job.finish(result);
    }
}

/// Feeds whatever part of the clip the stream has not seen, then finalizes.
/// The clip is authoritative: a stream fed well past its trimmed end, or one
/// that produced nothing, is reported unusable so the caller falls back.
/// The live stream as the worker drives it.
trait StreamFeed {
    fn feed(&mut self, samples: &[f32]) -> Result<(), String>;
    fn finalize(&mut self) -> Result<String, String>;
    fn partial(&self) -> Partial;
}

impl StreamFeed for transcribe_cpp::Stream<'_> {
    fn feed(&mut self, samples: &[f32]) -> Result<(), String> {
        transcribe_cpp::Stream::feed(self, samples)
            .map(drop)
            .map_err(|error| error.to_string())
    }

    fn finalize(&mut self) -> Result<String, String> {
        transcribe_cpp::Stream::finalize(self).map_err(|error| error.to_string())?;
        Ok(self.text().full.trim().to_string())
    }

    fn partial(&self) -> Partial {
        let text = self.text();
        Partial {
            stable: text.committed.trim_start().to_string(),
            tentative: text.tentative,
        }
    }
}

/// Pause audio a dictation stream still hears before its long pauses are
/// cut: longer than a chunk plus its lookahead (2.08 s for Unified English),
/// so the words before a pause decode during it.
const PAUSE_HEAD_SAMPLES: usize = 40_000;
/// Pause audio kept just before speech resumes after a cut.
const PAUSE_TAIL_SAMPLES: usize = 4_800;

/// A dictation stream that hears at most the head and tail of each pause.
/// Parakeet's buffered stream can drop the first word after a pause longer
/// than its 5.6 s left context (#324, #338); the clip, and with it any
/// offline fallback, keeps every sample.
struct PauseCap<'a, S> {
    stream: &'a mut S,
    speech: crate::pause_segmenter::SpeechFrames,
    /// Samples not yet a whole 20 ms frame.
    frame: Vec<f32>,
    /// Non-speech samples since the last speech frame.
    pause: usize,
    /// The newest pause samples past the head, released if speech resumes.
    tail: VecDeque<f32>,
    dropped: usize,
}

impl<'a, S: StreamFeed> PauseCap<'a, S> {
    fn new(stream: &'a mut S) -> Self {
        Self {
            stream,
            speech: Default::default(),
            frame: Vec::with_capacity(crate::pause_segmenter::FRAME_SAMPLES),
            pause: 0,
            tail: VecDeque::with_capacity(PAUSE_TAIL_SAMPLES),
            dropped: 0,
        }
    }

    fn frame(&mut self, out: &mut Vec<f32>) {
        if self.speech.is_speech(&self.frame) {
            out.extend(self.tail.drain(..));
            self.pause = 0;
            out.append(&mut self.frame);
            return;
        }
        self.pause += self.frame.len();
        if self.pause <= PAUSE_HEAD_SAMPLES {
            out.append(&mut self.frame);
            return;
        }
        self.tail.extend(self.frame.drain(..));
        let overflow = self.tail.len().saturating_sub(PAUSE_TAIL_SAMPLES);
        self.tail.drain(..overflow);
        self.dropped += overflow;
    }
}

impl<S: StreamFeed> StreamFeed for PauseCap<'_, S> {
    fn feed(&mut self, mut samples: &[f32]) -> Result<(), String> {
        let mut out = Vec::with_capacity(samples.len() + self.tail.len());
        while !samples.is_empty() {
            let take = samples
                .len()
                .min(crate::pause_segmenter::FRAME_SAMPLES - self.frame.len());
            self.frame.extend_from_slice(&samples[..take]);
            samples = &samples[take..];
            if self.frame.len() == crate::pause_segmenter::FRAME_SAMPLES {
                self.frame(&mut out);
            }
        }
        if out.is_empty() {
            return Ok(());
        }
        self.stream.feed(&out)
    }

    /// A trailing pause keeps only its head.
    fn finalize(&mut self) -> Result<String, String> {
        if self.pause <= PAUSE_HEAD_SAMPLES && !self.frame.is_empty() {
            let rest = std::mem::take(&mut self.frame);
            self.stream.feed(&rest)?;
        } else {
            self.dropped += self.frame.len();
        }
        self.dropped += self.tail.len();
        self.tail.clear();
        self.stream.finalize()
    }

    fn partial(&self) -> Partial {
        self.stream.partial()
    }
}

#[derive(Default)]
struct FeedStats {
    feed_ms: u128,
    feeds: u32,
    /// Changes of text published for live text.
    partials: u32,
    /// Pause audio the stream never heard.
    pauses_dropped_ms: usize,
}

/// Handles commands while stream `id` is open, publishing its text to
/// `partials` after each feed when live text is on. Foreground work waits in
/// `deferred` until the stream ends; background work is skipped; any other
/// command interrupts the stream.
fn drive_stream(
    id: u64,
    stream: &mut impl StreamFeed,
    commands: &Receiver<Command>,
    stalled: &AtomicBool,
    partials: Option<&Partials>,
    deferred: &mut VecDeque<Command>,
    stats: &mut FeedStats,
) -> StreamEnd {
    let mut failure: Option<String> = None;
    let mut fed = 0_usize;
    loop {
        let Ok(command) = commands.recv() else {
            return StreamEnd::Disconnected;
        };
        match command {
            Command::Feed { stream: s, samples } if s == id => {
                fed += samples.len();
                if failure.is_none() {
                    let started = Instant::now();
                    match stream.feed(&samples) {
                        Ok(()) => {
                            if partials.is_some_and(|partials| partials.publish(stream.partial())) {
                                stats.partials += 1;
                            }
                        }
                        Err(error) => {
                            failure = Some(format!("stream feed failed: {error}"));
                            // The text stops here; the plate hides rather
                            // than freeze on it while the capture goes on.
                            if let Some(partials) = partials {
                                partials.publish(Partial::default());
                            }
                        }
                    }
                    stats.feed_ms += started.elapsed().as_millis();
                    stats.feeds += 1;
                }
            }
            Command::Feed { .. } => {}
            // Background work never interrupts a capture.
            Command::Background { work, .. } => work(None),
            // Nor does foreground work; it runs once the stream ends.
            foreground @ Command::Foreground { .. } => deferred.push_back(foreground),
            // Nor does a stream that gives way to dictation; it is refused.
            Command::BeginYielding { yielding, .. } => yielding.close(),
            Command::FinishYielding { reply, .. } => {
                let _ = reply.send(Err(PREEMPTED.into()));
            }
            Command::Cancel { stream: s } if s == id => return StreamEnd::Cancelled,
            Command::Cancel { .. } => {}
            Command::Finish {
                stream: Some(s),
                clip,
                offline,
                job,
            } if s == id => {
                let started = Instant::now();
                stalled.store(false, Ordering::Release);
                job.start();
                let result = match failure {
                    Some(failure) => Err(failure),
                    None => finish_stream(stream, &clip, fed),
                };
                return StreamEnd::Finished {
                    result,
                    clip,
                    offline,
                    job,
                    started,
                };
            }
            other => return StreamEnd::Interrupted(other),
        }
    }
}

/// Handles commands while yielding stream `id` is open, publishing its
/// partial text after each feed. Background work is skipped and foreground
/// work deferred, as in a dictation stream; any other command (a dictation
/// begin or finish, model work, shutdown) closes it and is returned.
fn drive_yielding(
    id: u64,
    stream: &mut impl StreamFeed,
    commands: &Receiver<Command>,
    yielding: &Yielding,
    deferred: &mut VecDeque<Command>,
) -> Option<Command> {
    loop {
        let Ok(command) = commands.recv() else {
            return None;
        };
        match command {
            Command::Feed { stream: s, samples } if s == id => match stream.feed(&samples) {
                Ok(()) => {
                    yielding.consumed.fetch_add(samples.len(), Ordering::AcqRel);
                    yielding.partials.publish(stream.partial());
                }
                Err(error) => {
                    tracing::warn!(%error, "yielding stream feed failed");
                    return None;
                }
            },
            Command::Feed { .. } => {}
            Command::Background { work, .. } => work(None),
            foreground @ Command::Foreground { .. } => deferred.push_back(foreground),
            Command::Cancel { stream: s } if s == id => return None,
            Command::Cancel { .. } => {}
            Command::FinishYielding { stream: s, reply } if s == id => {
                let _ = reply.send(stream.finalize());
                return None;
            }
            Command::FinishYielding { reply, .. } => {
                let _ = reply.send(Err(PREEMPTED.into()));
            }
            Command::BeginYielding { yielding, .. } => yielding.close(),
            other => {
                yielding.close();
                return Some(other);
            }
        }
    }
}

fn finish_stream(stream: &mut impl StreamFeed, clip: &[f32], fed: usize) -> Result<String, String> {
    if fed > clip.len() + MAX_OVERFED_SAMPLES {
        return Err(format!(
            "the stream received {} ms past the clip's end",
            (fed - clip.len()) / 16
        ));
    }
    if clip.len() > fed {
        stream
            .feed(&clip[fed..])
            .map_err(|error| format!("stream feed failed: {error}"))?;
    }
    let text = stream
        .finalize()
        .map_err(|error| format!("stream finalize failed: {error}"))?;
    if text.is_empty() {
        return Err("the stream returned no text".into());
    }
    Ok(text)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn source_labels_distinguish_fallbacks() {
        assert_eq!(Source::Stream.label(), "stream");
        assert_eq!(Source::Offline.label(), "offline");
        assert_eq!(Source::Fallback("x".into()).label(), "fallback");
    }

    #[test]
    fn streams_send_their_audio_in_order_and_cancel_when_dropped() {
        let (engine, commands) = TranscriptionEngine::for_test();
        let mut stream = engine.begin().expect("the test engine streams");
        stream.feed(&[0.1; 3]);
        stream.feed(&[]);
        stream.feed(&[0.2; 2]);
        assert_eq!(stream.fed(), 5);
        drop(stream);
        let finished = engine.begin().expect("the test engine streams");
        let pending = engine
            .finish_with(
                Some(finished),
                vec![0.0; 7],
                Box::new(|_, _| Ok(String::new())),
            )
            .unwrap();
        assert!(pending.stream_cancel.is_some());
        let received = commands.try_iter().collect::<Vec<_>>();
        assert!(matches!(
            received.as_slice(),
            [
                Command::Begin { stream: 1, .. },
                Command::Feed { stream: 1, samples: first },
                Command::Feed { stream: 1, samples: second },
                Command::Cancel { stream: 1 },
                Command::Begin { stream: 2, .. },
                Command::Finish { stream: Some(2), clip, .. },
            ] if first.len() == 3 && second.len() == 2 && clip.len() == 7
        ));
    }

    #[derive(Default)]
    struct FakeStream {
        fed: Vec<f32>,
        /// Fail every feed once this many samples are in, as a native
        /// stream error would.
        fail_after: Option<usize>,
    }

    impl StreamFeed for FakeStream {
        fn feed(&mut self, samples: &[f32]) -> Result<(), String> {
            if self.fail_after.is_some_and(|after| self.fed.len() >= after) {
                return Err("native feed failed".into());
            }
            self.fed.extend_from_slice(samples);
            Ok(())
        }

        fn finalize(&mut self) -> Result<String, String> {
            Ok(format!("streamed {}", self.fed.len()))
        }

        fn partial(&self) -> Partial {
            Partial {
                stable: format!("heard {}", self.fed.len()),
                tentative: String::new(),
            }
        }
    }

    #[test]
    fn foreground_work_waits_for_an_open_stream_instead_of_interrupting_it() {
        let (engine, commands) = TranscriptionEngine::for_test();
        let mut stream = engine.begin().unwrap();
        assert!(matches!(
            commands.try_recv().unwrap(),
            Command::Begin { stream: 1, .. }
        ));
        stream.feed(&[0.1; 4]);
        let chunk = engine
            .foreground(vec![0.0; 9], Box::new(|_, _| Ok("chunk".into())))
            .unwrap();
        stream.feed(&[0.2; 4]);
        let dictation = engine
            .finish_with(
                Some(stream),
                vec![0.0; 8],
                Box::new(|_, _| panic!("the dictation must not be retranscribed offline")),
            )
            .unwrap();

        let mut fake = FakeStream::default();
        let mut deferred = VecDeque::new();
        let ended = drive_stream(
            1,
            &mut fake,
            &commands,
            &AtomicBool::new(false),
            None,
            &mut deferred,
            &mut FeedStats::default(),
        );
        let StreamEnd::Finished { result, job, .. } = ended else {
            panic!("the stream did not finish");
        };
        // The stream got all its audio and finished as a stream.
        assert_eq!(result, Ok("streamed 8".to_string()));
        assert_eq!(fake.fed.len(), 8);
        job.finish(Ok(Transcribed {
            text: "streamed 8".into(),
            source: Source::Stream,
            finish_ms: 0,
        }));
        assert_eq!(dictation.wait().unwrap().source, Source::Stream);
        // The chunk waits to run after the stream.
        assert!(matches!(
            deferred.as_slices().0,
            [Command::Foreground { clip, .. }] if clip.len() == 9
        ));
        drop(deferred);
        assert!(chunk.wait().is_err());
    }

    fn tone(samples: usize) -> Vec<f32> {
        (0..samples)
            .map(|i| 0.3 * (i as f32 * 0.05).sin())
            .collect()
    }

    #[test]
    fn a_dictation_stream_hears_only_the_ends_of_long_pauses() {
        // Quiet first, as the pre-roll is, so the noise floor starts low.
        let mut clip = vec![0.0; 8_000];
        clip.extend(tone(16_000));
        clip.extend(vec![0.0; 16_000]);
        clip.extend(tone(16_000));
        clip.extend(vec![0.0; 160_000]);
        clip.extend(tone(16_000));
        clip.extend(vec![0.0; 80_000]);
        let mut fake = FakeStream::default();
        let mut capped = PauseCap::new(&mut fake);
        for chunk in clip.chunks(1_234) {
            capped.feed(chunk).unwrap();
        }
        capped.finalize().unwrap();
        let dropped = capped.dropped;
        // A 1 s pause passes whole; the 10 s one keeps 2.5 s and 0.3 s, and
        // the trailing 5 s one only its head.
        let long = 160_000 - PAUSE_HEAD_SAMPLES - PAUSE_TAIL_SAMPLES;
        let trailing = 80_000 - PAUSE_HEAD_SAMPLES;
        assert_eq!(dropped, long + trailing);
        assert_eq!(fake.fed.len(), clip.len() - dropped);
        assert_eq!(fake.fed[..56_000], clip[..56_000]);
        let resumed = 56_000 + PAUSE_HEAD_SAMPLES + PAUSE_TAIL_SAMPLES;
        assert_eq!(fake.fed[resumed..resumed + 16_000], clip[216_000..232_000]);
    }

    fn heard(samples: usize) -> Partial {
        Partial {
            stable: format!("heard {samples}"),
            tentative: String::new(),
        }
    }

    /// Where the worker publishes the text of the stream `commands` began.
    fn begun_partials(commands: &Receiver<Command>) -> Option<Partials> {
        match commands.try_recv() {
            Ok(Command::Begin { partials, .. }) => partials,
            _ => panic!("a stream began"),
        }
    }

    /// Drives stream 1 through the commands queued so far, then a cancel.
    fn drive_queued(
        commands: &Receiver<Command>,
        stream: &mut FakeStream,
        partials: Option<&Partials>,
    ) -> FeedStats {
        let (relay, relayed) = mpsc::channel();
        for command in commands.try_iter() {
            relay.send(command).unwrap();
        }
        relay.send(Command::Cancel { stream: 1 }).unwrap();
        let mut stats = FeedStats::default();
        let ended = drive_stream(
            1,
            stream,
            &relayed,
            &AtomicBool::new(false),
            partials,
            &mut VecDeque::new(),
            &mut stats,
        );
        assert!(matches!(ended, StreamEnd::Cancelled));
        stats
    }

    #[test]
    fn a_dictation_stream_publishes_its_text_after_each_feed_with_live_text() {
        let (engine, commands) = TranscriptionEngine::for_test();
        let engine = engine.with_live_text();
        let mut stream = engine.begin().unwrap();
        let partials = begun_partials(&commands).expect("live text gives the stream a slot");
        assert_eq!(
            stream.take_partial(),
            None,
            "nothing is published before a feed"
        );
        stream.feed(&[0.1; 4]);
        stream.feed(&[0.2; 2]);

        let stats = drive_queued(&commands, &mut FakeStream::default(), Some(&partials));
        assert_eq!(stats.partials, 2);
        // The owner reads the newest text once, then nothing until it changes.
        assert_eq!(stream.take_partial(), Some(heard(6)));
        assert_eq!(stream.take_partial(), None);
        assert!(!partials.publish(heard(6)), "the same text is not a change");
        assert_eq!(stream.take_partial(), None);
        assert!(partials.publish(heard(7)));
        assert_eq!(stream.take_partial(), Some(heard(7)));
    }

    #[test]
    fn without_live_text_a_dictation_stream_publishes_nothing() {
        let (engine, commands) = TranscriptionEngine::for_test();
        let mut stream = engine.begin().unwrap();
        assert!(begun_partials(&commands).is_none());
        stream.feed(&[0.1; 4]);
        let stats = drive_queued(&commands, &mut FakeStream::default(), None);
        assert_eq!(stats.partials, 0);
        assert_eq!(stream.take_partial(), None);
    }

    #[test]
    fn each_dictation_stream_has_its_own_text() {
        let (engine, commands) = TranscriptionEngine::for_test();
        let engine = engine.with_live_text();
        let mut first = engine.begin().unwrap();
        let first_partials = begun_partials(&commands).unwrap();
        // An older capture's stream is still finishing when a new one opens.
        let mut second = engine.begin().unwrap();
        let second_partials = begun_partials(&commands).unwrap();
        assert!(first_partials.publish(heard(4)));
        assert_eq!(
            second.take_partial(),
            None,
            "the new capture shows no old text"
        );
        assert_eq!(first.take_partial(), Some(heard(4)));
        assert!(second_partials.publish(heard(1)));
        assert_eq!(second.take_partial(), Some(heard(1)));
        assert_eq!(first.take_partial(), None);
    }

    #[test]
    fn a_failed_feed_clears_the_text_and_stops_publishing() {
        let (engine, commands) = TranscriptionEngine::for_test();
        let engine = engine.with_live_text();
        let mut stream = engine.begin().unwrap();
        let partials = begun_partials(&commands).unwrap();
        stream.feed(&[0.1; 4]);
        stream.feed(&[0.2; 4]);
        stream.feed(&[0.3; 4]);
        let mut failing = FakeStream {
            fail_after: Some(4),
            ..FakeStream::default()
        };
        let stats = drive_queued(&commands, &mut failing, Some(&partials));
        assert_eq!(stats.partials, 1, "the first feed's text, then nothing");
        assert_eq!(failing.fed.len(), 4, "a failed stream is fed no more");
        // The plate hides instead of freezing on the last words.
        assert_eq!(stream.take_partial(), Some(Partial::default()));
    }

    fn begun_yielding(commands: &Receiver<Command>) -> Yielding {
        match commands.try_recv() {
            Ok(Command::BeginYielding { yielding, .. }) => yielding,
            _ => panic!("a yielding stream began"),
        }
    }

    #[test]
    fn a_yielding_stream_reports_partials_and_finishes_through_the_stream() {
        let (engine, commands) = TranscriptionEngine::for_test();
        let mut stream = engine.begin_yielding().unwrap();
        let yielding = begun_yielding(&commands);
        stream.feed(&[0.1; 4]);
        assert_eq!(stream.in_flight(), 4);
        let finishing = thread::spawn(move || stream.finish(Duration::from_secs(5)));
        // The finish is sent from the other thread; wait for it to queue.
        let mut queued = Vec::new();
        while queued.len() < 2 {
            queued.push(commands.recv().unwrap());
        }
        let (relay, relayed) = mpsc::channel();
        for command in queued {
            relay.send(command).unwrap();
        }
        let mut fake = FakeStream::default();
        let interrupted = drive_yielding(1, &mut fake, &relayed, &yielding, &mut VecDeque::new());
        assert!(interrupted.is_none());
        assert_eq!(finishing.join().unwrap(), Ok("streamed 4".into()));
        assert_eq!(yielding.consumed.load(Ordering::Acquire), 4);
        assert_eq!(
            yielding.partials.latest(),
            Some(Partial {
                stable: "heard 4".into(),
                tentative: String::new(),
            })
        );
    }

    #[test]
    fn a_dictation_request_closes_a_yielding_stream_and_runs_next() {
        let (engine, commands) = TranscriptionEngine::for_test();
        let mut stream = engine.begin_yielding().unwrap();
        let yielding = begun_yielding(&commands);
        stream.feed(&[0.1; 4]);
        let generation = engine.requests.load(Ordering::Acquire);
        let _chunk = engine
            .foreground(vec![0.0; 3], Box::new(|_, _| Ok("chunk".into())))
            .unwrap();
        assert_eq!(
            engine.requests.load(Ordering::Acquire),
            generation + 1,
            "foreground work is a dictation-class request"
        );
        let _dictation = engine.begin().unwrap();
        stream.feed(&[0.2; 4]);

        let mut fake = FakeStream::default();
        let mut deferred = VecDeque::new();
        let interrupted = drive_yielding(1, &mut fake, &commands, &yielding, &mut deferred);
        assert!(matches!(
            interrupted,
            Some(Command::Begin { stream: 2, .. })
        ));
        assert!(!stream.open(), "dictation closed the stream");
        assert_eq!(fake.fed.len(), 4);
        assert!(matches!(
            deferred.as_slices().0,
            [Command::Foreground { clip, .. }] if clip.len() == 3
        ));
        // A closed stream answers at once; its owner transcribes offline.
        assert_eq!(stream.finish(Duration::from_secs(5)), Err(PREEMPTED.into()));
    }

    #[test]
    fn an_open_dictation_stream_refuses_a_yielding_stream_instead_of_ending() {
        let (engine, commands) = TranscriptionEngine::for_test();
        let mut dictation = engine.begin().unwrap();
        assert!(matches!(
            commands.try_recv().unwrap(),
            Command::Begin { stream: 1, .. }
        ));
        let yielding = engine.begin_yielding().unwrap();
        dictation.feed(&[0.1; 4]);
        let _pending = engine
            .finish_with(
                Some(dictation),
                vec![0.0; 4],
                Box::new(|_, _| panic!("the dictation must not be retranscribed offline")),
            )
            .unwrap();
        let mut fake = FakeStream::default();
        let ended = drive_stream(
            1,
            &mut fake,
            &commands,
            &AtomicBool::new(false),
            None,
            &mut VecDeque::new(),
            &mut FeedStats::default(),
        );
        let StreamEnd::Finished { result, .. } = ended else {
            panic!("the dictation stream did not finish");
        };
        assert_eq!(result, Ok("streamed 4".to_string()));
        assert!(!yielding.open());
    }

    const SHORT: Deadlines = Deadlines {
        stream_finish: Duration::from_millis(50),
        job: Duration::from_millis(100),
        job_seconds_per_audio_second: 0,
        shutdown: Duration::from_millis(100),
    };

    fn silent() -> Offline {
        Box::new(|_, _| Ok(String::new()))
    }

    /// Takes the next finish the test engine would have run.
    fn next_job(commands: &Receiver<Command>) -> Job {
        commands
            .try_iter()
            .find_map(|command| match command {
                Command::Finish { job, .. } => Some(job),
                _ => None,
            })
            .expect("a finish was sent")
    }

    #[test]
    fn a_started_job_that_never_ends_fails_instead_of_hanging() {
        let (engine, commands) = TranscriptionEngine::for_test();
        let engine = engine.with_deadlines(SHORT);
        for streamed in [false, true] {
            let stream = streamed.then(|| engine.begin().expect("the test engine streams"));
            let pending = engine
                .finish_with(stream, vec![0.0; 1_600], silent())
                .unwrap();
            let job = next_job(&commands);
            job.start();
            let started = Instant::now();
            assert_eq!(pending.wait().unwrap_err(), "transcription timed out");
            assert!(started.elapsed() < Duration::from_secs(2));
            assert!(job.cancel.is_cancelled(), "a timeout aborts the stuck run");
            engine.stalled.store(false, Ordering::Release);
        }
    }

    #[test]
    fn the_job_clock_does_not_count_time_queued_behind_other_work() {
        let (engine, commands) = TranscriptionEngine::for_test();
        let engine = engine.with_deadlines(SHORT);
        let pending = engine.finish_with(None, Vec::new(), silent()).unwrap();
        let job = next_job(&commands);
        let worker = thread::spawn(move || {
            // Three job deadlines pass before the engine reaches the job.
            thread::sleep(SHORT.job * 3);
            job.start();
            job.finish(Ok(Transcribed {
                text: "late but fine".into(),
                source: Source::Offline,
                finish_ms: 0,
            }));
        });
        assert_eq!(pending.wait().unwrap().text, "late but fine");
        worker.join().unwrap();
    }

    #[test]
    fn a_timed_out_job_cancels_only_itself_not_the_next_capture() {
        let (engine, commands) = TranscriptionEngine::for_test();
        let engine = engine.with_deadlines(SHORT);
        let first = engine.finish_with(None, Vec::new(), silent()).unwrap();
        let first_job = next_job(&commands);
        first_job.start();
        // Capture 2 begins while job 1 is still running.
        let second = engine.begin().expect("the test engine streams");
        assert_eq!(first.wait().unwrap_err(), "transcription timed out");
        assert!(first_job.cancel.is_cancelled());
        assert!(!second.cancel.is_cancelled(), "capture 2's stream survives");
        let Some(Command::Begin { cancel, .. }) = commands.try_iter().next() else {
            panic!("capture 2 began a stream");
        };
        assert!(!cancel.is_cancelled());
        // A capture that begins after the timeout is untouched too.
        let third = engine.begin().expect("the test engine streams");
        assert!(!third.cancel.is_cancelled());
    }

    #[test]
    fn jobs_queued_behind_a_wedged_run_fail_instead_of_waiting_forever() {
        let (engine, commands) = TranscriptionEngine::for_test();
        let engine = engine.with_deadlines(SHORT);
        let wedged = engine.finish_with(None, Vec::new(), silent()).unwrap();
        let queued = engine.finish_with(None, Vec::new(), silent()).unwrap();
        let wedged_job = next_job(&commands);
        let queued_job = next_job(&commands);
        wedged_job.start();
        assert_eq!(wedged.wait().unwrap_err(), "transcription timed out");
        let started = Instant::now();
        assert_eq!(queued.wait().unwrap_err(), "transcription engine is stuck");
        assert!(started.elapsed() < Duration::from_secs(2));
        assert!(
            queued_job.cancel.is_cancelled(),
            "the engine skips it later"
        );
    }

    #[test]
    fn the_job_deadline_grows_with_the_clip() {
        let (engine, _commands) = TranscriptionEngine::for_test_without_streaming();
        let pending = engine
            .finish_with(None, vec![0.0; 16_000 * 30], silent())
            .unwrap();
        assert_eq!(
            pending.job,
            JOB_DEADLINE + Duration::from_secs(30 * u64::from(JOB_SECONDS_PER_AUDIO_SECOND))
        );
    }

    /// A real engine thread over macOS's warm transcriber with no model
    /// loaded: it cannot stream, so every finish runs the offline closure.
    #[cfg(target_os = "macos")]
    mod worker {
        use super::*;

        fn offline_text(text: &'static str) -> Offline {
            Box::new(move |_, clip| Ok(format!("  {text} {} ", clip.len())))
        }

        #[test]
        fn a_finish_without_a_stream_transcribes_the_clip_offline() {
            let engine = TranscriptionEngine::start(Model::default()).unwrap();
            assert!(engine.begin().is_none());
            let transcribed = engine
                .finish_with(None, vec![0.0; 5], offline_text("whole"))
                .unwrap()
                .wait()
                .unwrap();
            assert_eq!(transcribed.text, "whole 5");
            assert_eq!(transcribed.source, Source::Offline);
        }

        #[test]
        fn an_unusable_stream_falls_back_to_the_complete_clip() {
            let engine = TranscriptionEngine::start(Model::default()).unwrap();
            // Claim streaming so a stream opens that the model cannot serve.
            engine.streams.store(true, Ordering::Release);
            let mut stream = engine.begin().expect("streaming was claimed");
            stream.feed(&[0.0; 3]);
            let transcribed = engine
                .finish_with(Some(stream), vec![0.0; 9], offline_text("fallback"))
                .unwrap()
                .wait()
                .unwrap();
            assert_eq!(transcribed.text, "fallback 9");
            assert!(matches!(transcribed.source, Source::Fallback(_)));
            assert!(!engine.streams.load(Ordering::Acquire));
        }

        #[test]
        fn work_runs_in_order_and_failures_reach_the_caller() {
            let engine = TranscriptionEngine::start(Model::default()).unwrap();
            let (order, ran) = mpsc::channel();
            let first = order.clone();
            engine.run(move |_| first.send("run").unwrap()).unwrap();
            let failed = engine
                .finish_with(
                    None,
                    Vec::new(),
                    Box::new(move |_, _| {
                        order.send("finish").unwrap();
                        Err("dictation was cancelled".into())
                    }),
                )
                .unwrap();
            assert_eq!(failed.wait().unwrap_err(), "dictation was cancelled");
            assert_eq!(ran.try_iter().collect::<Vec<_>>(), ["run", "finish"]);
        }

        #[test]
        fn background_work_yields_to_dictation_sent_before_it_runs() {
            let engine = TranscriptionEngine::start(Model::default()).unwrap();
            assert_eq!(engine.background(|_| 7).unwrap(), Some(7));

            let generation = engine.requests.fetch_add(1, Ordering::AcqRel);
            let (reply, ran) = mpsc::channel();
            engine
                .commands
                .send(Command::Background {
                    generation,
                    work: Box::new(move |model| reply.send(model.is_some()).unwrap()),
                })
                .unwrap();
            assert!(!ran.recv().unwrap(), "stale background work must not run");
        }

        #[test]
        fn a_wedged_run_times_out_its_job_and_later_jobs_still_finish() {
            let engine = TranscriptionEngine::start(Model::default())
                .unwrap()
                .with_deadlines(SHORT);
            let (release, wedged) = mpsc::channel::<()>();
            let stuck = engine
                .finish_with(
                    None,
                    Vec::new(),
                    Box::new(move |_, _| {
                        let _ = wedged.recv();
                        Ok("late".into())
                    }),
                )
                .unwrap();
            assert_eq!(stuck.wait().unwrap_err(), "transcription timed out");
            let next = engine
                .finish_with(None, vec![0.0; 2], offline_text("next"))
                .unwrap();
            release.send(()).unwrap();
            assert_eq!(next.wait().unwrap().text, "next 2");
        }

        #[test]
        fn dropping_a_wedged_engine_returns() {
            let engine = TranscriptionEngine::start(Model::default())
                .unwrap()
                .with_deadlines(SHORT);
            let (_release, wedged) = mpsc::channel::<()>();
            let (entered, running) = mpsc::channel();
            engine
                .run(move |_| {
                    entered.send(()).unwrap();
                    let _ = wedged.recv();
                })
                .unwrap();
            running.recv_timeout(Duration::from_secs(5)).unwrap();
            let started = Instant::now();
            drop(engine);
            assert!(started.elapsed() < Duration::from_secs(2));
        }

        #[test]
        fn a_job_queued_behind_slow_work_is_not_timed_out() {
            let engine = TranscriptionEngine::start(Model::default())
                .unwrap()
                .with_deadlines(SHORT);
            // A model load that outlasts the job deadline three times over.
            engine.run(|_| thread::sleep(SHORT.job * 3)).unwrap();
            let queued = engine
                .finish_with(None, vec![0.0; 4], offline_text("queued"))
                .unwrap();
            assert_eq!(queued.wait().unwrap().text, "queued 4");
        }

        #[test]
        fn a_timeout_aborts_the_offline_run_through_its_job_token() {
            let engine = TranscriptionEngine::start(Model::default())
                .unwrap()
                .with_deadlines(SHORT);
            let (aborted, saw_abort) = mpsc::channel();
            let stuck = engine
                .finish_with(
                    None,
                    Vec::new(),
                    Box::new(move |_, _| {
                        // What transcribe-cpp's abort callback polls.
                        let cancel = crate::gguf_session::job_cancel().expect("installed");
                        while !cancel.is_cancelled() {
                            thread::sleep(Duration::from_millis(5));
                        }
                        aborted.send(()).unwrap();
                        Err("aborted".into())
                    }),
                )
                .unwrap();
            assert_eq!(stuck.wait().unwrap_err(), "transcription timed out");
            saw_abort.recv_timeout(Duration::from_secs(5)).unwrap();
            let next = engine
                .finish_with(None, vec![0.0; 2], offline_text("next"))
                .unwrap();
            assert_eq!(next.wait().unwrap().text, "next 2");
        }

        #[test]
        fn retiring_a_wedged_engine_returns_at_once() {
            let engine = TranscriptionEngine::start(Model::default()).unwrap();
            let (release, wedged) = mpsc::channel::<()>();
            let (entered, running) = mpsc::channel();
            engine
                .run(move |_| {
                    entered.send(()).unwrap();
                    let _ = wedged.recv();
                })
                .unwrap();
            running.recv_timeout(Duration::from_secs(5)).unwrap();
            let started = Instant::now();
            engine.retire();
            assert!(started.elapsed() < Duration::from_millis(500));
            release.send(()).unwrap();
        }

        #[test]
        fn a_yielding_stream_on_a_model_that_cannot_stream_closes_at_once() {
            let engine = TranscriptionEngine::start(Model::default()).unwrap();
            engine.streams.store(true, Ordering::Release);
            let mut stream = engine.begin_yielding().expect("streaming was claimed");
            stream.feed(&[0.0; 3]);
            assert_eq!(stream.finish(Duration::from_secs(5)), Err(PREEMPTED.into()));
        }

        #[test]
        fn dropping_the_engine_stops_despite_an_open_stream() {
            let engine = TranscriptionEngine::start(Model::default()).unwrap();
            engine.streams.store(true, Ordering::Release);
            let stream = engine.begin().expect("streaming was claimed");
            drop(engine);
            drop(stream);
        }

        /// Quitting frees the model by dropping the engine; that must finish
        /// work in flight and end the thread that owns the model before
        /// returning, or ggml's Metal teardown at exit sees a live buffer.
        #[test]
        fn dropping_the_engine_ends_the_thread_that_owns_the_model_first() {
            struct ThreadExit(Arc<AtomicBool>);
            impl Drop for ThreadExit {
                fn drop(&mut self) {
                    self.0.store(true, Ordering::SeqCst);
                }
            }
            thread_local! {
                static EXIT: std::cell::RefCell<Option<ThreadExit>> =
                    const { std::cell::RefCell::new(None) };
            }

            let engine = TranscriptionEngine::start(Model::default()).unwrap();
            let exited = Arc::new(AtomicBool::new(false));
            let finished = Arc::new(AtomicBool::new(false));
            let (started, running) = mpsc::channel();
            let (on_exit, done) = (exited.clone(), finished.clone());
            engine
                .run(move |_| {
                    EXIT.with(|slot| *slot.borrow_mut() = Some(ThreadExit(on_exit)));
                    started.send(()).unwrap();
                    thread::sleep(Duration::from_millis(50));
                    done.store(true, Ordering::SeqCst);
                })
                .unwrap();
            running.recv().unwrap();
            drop(engine);
            assert!(
                finished.load(Ordering::SeqCst),
                "work in flight was abandoned"
            );
            assert!(
                exited.load(Ordering::SeqCst),
                "the engine thread outlived the engine"
            );
        }
    }

    #[test]
    fn an_engine_whose_model_cannot_stream_opens_no_stream() {
        let (engine, commands) = TranscriptionEngine::for_test();
        engine.streams.store(false, Ordering::Release);
        assert!(engine.begin().is_none());
        let pending = engine
            .finish_with(None, vec![0.0; 7], Box::new(|_, _| Ok(String::new())))
            .unwrap();
        assert!(pending.stream_cancel.is_none());
        assert!(matches!(
            commands.try_iter().collect::<Vec<_>>().as_slice(),
            [Command::Finish { stream: None, .. }]
        ));
    }
}
