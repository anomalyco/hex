//! The thread that owns the loaded GGUF model and runs all of its inference.
//!
//! A capture that the model can stream is fed while the shortcut is held, so
//! release only decodes the last second or two of audio. Every finish carries
//! the complete clip: a stream that failed, stalled, or produced no text is
//! replaced by an offline transcription of that clip, and a model or device
//! that cannot stream always takes the offline path. Commands are handled in
//! order on one thread, because a live stream holds the model's compute lease.

use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::mpsc::{self, Receiver, RecvTimeoutError, Sender};
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

pub(crate) enum Command {
    Begin {
        stream: u64,
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
        reply: Sender<Result<Transcribed, String>>,
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

pub struct TranscriptionEngine {
    commands: Sender<Command>,
    cancel: CancelToken,
    /// Dictation requests sent so far. Background work submitted before a
    /// newer request yields to it.
    requests: Arc<AtomicU64>,
    /// Whether the loaded model can stream on its device.
    streams: Arc<AtomicBool>,
    next_stream: AtomicU64,
    thread: Option<JoinHandle<()>>,
}

/// A live stream for one capture. Feed it the capture's 16 kHz audio in order,
/// then either [`TranscriptionEngine::finish`] or drop it to cancel.
pub struct LiveStream {
    id: u64,
    fed: usize,
    finished: bool,
    commands: Sender<Command>,
}

impl LiveStream {
    /// Total samples handed to the stream so far.
    pub fn fed(&self) -> usize {
        self.fed
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
/// side, so a stalled native stream cannot hold the paste forever.
pub struct PendingTranscript {
    reply: Receiver<Result<Transcribed, String>>,
    cancel: CancelToken,
    streamed: bool,
}

impl PendingTranscript {
    pub fn wait(self) -> Result<Transcribed, String> {
        if self.streamed {
            match self.reply.recv_timeout(STREAM_FINISH_DEADLINE) {
                Ok(result) => return result,
                Err(RecvTimeoutError::Disconnected) => {
                    return Err("transcription engine stopped".into());
                }
                Err(RecvTimeoutError::Timeout) => {
                    tracing::warn!("live stream missed its finish deadline; aborting it");
                    self.cancel.cancel();
                }
            }
        }
        self.reply
            .recv()
            .unwrap_or_else(|_| Err("transcription engine stopped".into()))
    }
}

impl TranscriptionEngine {
    pub fn start(model: Model) -> Result<Self> {
        let cancel = CancelToken::new();
        let streams = Arc::new(AtomicBool::new(model.stream_options().is_some()));
        let (commands, receiver) = mpsc::channel();
        let requests = Arc::new(AtomicU64::new(0));
        let worker = Worker {
            model,
            cancel: cancel.clone(),
            streams: streams.clone(),
            requests: requests.clone(),
        };
        let thread = thread::Builder::new()
            .name("transcription-engine".into())
            .spawn(move || worker.run(receiver))?;
        Ok(Self {
            commands,
            cancel,
            requests,
            streams,
            next_stream: AtomicU64::new(0),
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
            cancel: CancelToken::new(),
            requests: Arc::default(),
            streams: Arc::new(AtomicBool::new(true)),
            next_stream: AtomicU64::new(0),
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

    /// Opens a live stream when the loaded model can stream on this device.
    pub fn begin(&self) -> Option<LiveStream> {
        if !self.streams.load(Ordering::Acquire) {
            return None;
        }
        let id = self.next_stream.fetch_add(1, Ordering::Relaxed) + 1;
        self.requests.fetch_add(1, Ordering::AcqRel);
        self.commands.send(Command::Begin { stream: id }).ok()?;
        Some(LiveStream {
            id,
            fed: 0,
            finished: false,
            commands: self.commands.clone(),
        })
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
        let (reply, receiver) = mpsc::channel();
        let streamed = stream.is_some();
        self.requests.fetch_add(1, Ordering::AcqRel);
        let stream = stream.map(|mut stream| {
            stream.finished = true;
            stream.id
        });
        self.commands
            .send(Command::Finish {
                stream,
                clip,
                offline,
                reply,
            })
            .map_err(|_| eyre!("transcription engine stopped"))?;
        Ok(PendingTranscript {
            reply: receiver,
            cancel: self.cancel.clone(),
            streamed,
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
}

#[cfg(target_os = "macos")]
static SHARED: Mutex<std::sync::Weak<TranscriptionEngine>> = Mutex::new(std::sync::Weak::new());

impl Drop for TranscriptionEngine {
    fn drop(&mut self) {
        self.cancel.cancel();
        // A live stream's sender may outlive the engine; stop explicitly.
        let _ = self.commands.send(Command::Shutdown);
        if let Some(thread) = self.thread.take() {
            let _ = thread.join();
        }
    }
}

struct Worker {
    model: Model,
    cancel: CancelToken,
    streams: Arc<AtomicBool>,
    requests: Arc<AtomicU64>,
}

/// How a live stream ended inside the worker.
enum StreamEnd {
    /// A finish for this stream: the final text, or why the stream is unusable.
    Finished {
        result: Result<String, String>,
        clip: Vec<f32>,
        offline: Offline,
        reply: Sender<Result<Transcribed, String>>,
        started: Instant,
    },
    Cancelled,
    /// Another command arrived first; it is handled after the stream closes.
    Interrupted(Command),
    Disconnected,
}

impl Worker {
    fn run(mut self, commands: Receiver<Command>) {
        let mut next = None;
        loop {
            let Some(command) = next.take().or_else(|| commands.recv().ok()) else {
                return;
            };
            match command {
                Command::Begin { stream } => {
                    next = self.stream(stream, &commands);
                }
                Command::Finish {
                    stream,
                    clip,
                    offline,
                    reply,
                } => {
                    let started = Instant::now();
                    let source = match stream {
                        // The stream was already closed: its begin failed or
                        // another command interrupted it.
                        Some(_) => Source::Fallback("the live stream was not available".into()),
                        None => Source::Offline,
                    };
                    self.offline(clip, offline, source, started, reply);
                }
                Command::Run(work) => {
                    self.cancel.reset();
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
        }
    }

    /// Drives one live stream until it finishes, is cancelled, or another
    /// command interrupts it. Returns a command still to be handled.
    fn stream(&mut self, id: u64, commands: &Receiver<Command>) -> Option<Command> {
        let options = self.model.stream_options().cloned()?;
        self.cancel.reset();
        let begun = Instant::now();
        let mut feed_ms = 0_u128;
        let mut feeds = 0_u32;
        let ended = self.model.with_stream(&options, &self.cancel, |stream| {
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
                            if let Err(error) = stream.feed(&samples) {
                                failure = Some(format!("stream feed failed: {error}"));
                            }
                            feed_ms += started.elapsed().as_millis();
                            feeds += 1;
                        }
                    }
                    Command::Feed { .. } => {}
                    // Background work never interrupts a capture.
                    Command::Background { work, .. } => work(None),
                    Command::Cancel { stream: s } if s == id => return StreamEnd::Cancelled,
                    Command::Cancel { .. } => {}
                    Command::Finish {
                        stream: Some(s),
                        clip,
                        offline,
                        reply,
                    } if s == id => {
                        let started = Instant::now();
                        let result = match failure {
                            Some(failure) => Err(failure),
                            None => finish_stream(stream, &clip, fed),
                        };
                        return StreamEnd::Finished {
                            result,
                            clip,
                            offline,
                            reply,
                            started,
                        };
                    }
                    other => return StreamEnd::Interrupted(other),
                }
            }
        });
        match ended {
            Ok(StreamEnd::Finished {
                result,
                clip,
                offline,
                reply,
                started,
            }) => {
                tracing::info!(
                    audio_ms = clip.len() / 16,
                    stream_ms = begun.elapsed().as_millis(),
                    feeds,
                    feed_ms,
                    finish_ms = started.elapsed().as_millis(),
                    "finished live stream"
                );
                match result {
                    Ok(text) => {
                        let _ = reply.send(Ok(Transcribed {
                            text,
                            source: Source::Stream,
                            finish_ms: started.elapsed().as_millis() as u64,
                        }));
                    }
                    Err(reason) => {
                        tracing::warn!(%reason, "live stream unusable; transcribing the full clip");
                        self.offline(clip, offline, Source::Fallback(reason), started, reply);
                    }
                }
                None
            }
            Ok(StreamEnd::Cancelled) | Ok(StreamEnd::Disconnected) => None,
            Ok(StreamEnd::Interrupted(command)) => Some(command),
            Err(error) => {
                tracing::warn!(%error, "could not begin a live stream");
                None
            }
        }
    }

    fn offline(
        &mut self,
        clip: Vec<f32>,
        offline: Offline,
        source: Source,
        started: Instant,
        reply: Sender<Result<Transcribed, String>>,
    ) {
        self.cancel.reset();
        let result = offline(&mut self.model, clip).map(|text| Transcribed {
            text: text.trim().to_string(),
            source,
            finish_ms: started.elapsed().as_millis() as u64,
        });
        // An offline run may switch the model (macOS activates a selection).
        self.streams
            .store(self.model.stream_options().is_some(), Ordering::Release);
        let _ = reply.send(result);
    }
}

/// Feeds whatever part of the clip the stream has not seen, then finalizes.
/// The clip is authoritative: a stream fed well past its trimmed end, or one
/// that produced nothing, is reported unusable so the caller falls back.
fn finish_stream(
    stream: &mut transcribe_cpp::Stream<'_>,
    clip: &[f32],
    fed: usize,
) -> Result<String, String> {
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
    stream
        .finalize()
        .map_err(|error| format!("stream finalize failed: {error}"))?;
    let text = stream.text().full.trim().to_string();
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
        assert!(pending.streamed);
        let received = commands.try_iter().collect::<Vec<_>>();
        assert!(matches!(
            received.as_slice(),
            [
                Command::Begin { stream: 1 },
                Command::Feed { stream: 1, samples: first },
                Command::Feed { stream: 1, samples: second },
                Command::Cancel { stream: 1 },
                Command::Begin { stream: 2 },
                Command::Finish { stream: Some(2), clip, .. },
            ] if first.len() == 3 && second.len() == 2 && clip.len() == 7
        ));
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
        fn dropping_the_engine_stops_despite_an_open_stream() {
            let engine = TranscriptionEngine::start(Model::default()).unwrap();
            engine.streams.store(true, Ordering::Release);
            let stream = engine.begin().expect("streaming was claimed");
            drop(engine);
            drop(stream);
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
        assert!(!pending.streamed);
        assert!(matches!(
            commands.try_iter().collect::<Vec<_>>().as_slice(),
            [Command::Finish { stream: None, .. }]
        ));
    }
}
