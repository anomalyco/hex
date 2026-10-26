//! The `capture` experiment (macOS builds with the `capture` build option):
//! Scrawl's capture engine, the `scrawl-capture` crate, hosted inside Hex.
//! Step 3 of `docs/plans/capture-in-hex.md`.
//!
//! The engine records the screen and writes the bundle; Hex owns the
//! microphone and the model. A capture copies audio from the dictation
//! timeline through a bounded sink (no second stream) and hands it to
//! `SealedCapture::finish` ([`AudioIn::External`]), and its words come from
//! the dictation engine's loaded model as foreground work. The engine runs
//! with [`SharedResources::none`]: no ingest server, delivery retries, or
//! global shortcuts. [`CaptureHost::start`] does nothing unless the
//! experiment is on.

use std::fs;
use std::fs::DirBuilder;
use std::os::unix::fs::DirBuilderExt;
use std::path::{Path, PathBuf};
use std::sync::mpsc::{self, Receiver, SyncSender, TrySendError};
use std::sync::{Arc, Mutex, Weak};
use std::thread::{self, JoinHandle};
use std::time::{Duration, Instant};

use color_eyre::eyre::{Result, WrapErr, eyre};
use scrawl_capture::audio::{GapSpan, RecordedAudio};
use scrawl_capture::capture::{
    AudioIn, ConfigSource, Event, Host, SharedResources, Stage, Start, Target,
};
use scrawl_capture::handoff::EngineConfig;
use scrawl_capture::model::{TranscriptData, WordTiming};
use scrawl_capture::transcribe::Transcriber;

use crate::audio::CaptureInstant;
use crate::control::{
    ControlReply, ScrawlCaptureAction, ScrawlCaptureOutcome, ScrawlCaptureReport,
    ScrawlCaptureState,
};
use crate::dictation::StreamingResampler;
use crate::dictation_audio::{CaptureAudio, CaptureSinkClosed, DictationAudio};
use crate::transcription_engine::TranscriptionEngine;

/// The sink's capacity in microphone chunks (about 10 ms each): seconds of
/// slack before a stalled collector starts dropping audio as a gap.
const SINK_CAPACITY: usize = 512;
const EVENT_CAPACITY: usize = 64;
const SAMPLE_RATE: u32 = 16_000;
/// A capture keeps at most this much audio; the rest is recorded as a gap.
const MAX_CAPTURE: Duration = Duration::from_secs(20 * 60);
const GAP_TOLERANCE: Duration = Duration::from_millis(50);
/// How long shutdown waits for capture threads, inside the quit deadline.
const SHUTDOWN_WAIT: Duration = Duration::from_secs(2);
/// Captures being written at once: the one finishing plus those queued
/// behind it. A start beyond this is refused.
const MAX_FINISHING: usize = 2;

// ---------------------------------------------------------------------------
// The engine, as Hex runs it.

/// What [`CaptureHost`] needs from an engine: Scrawl's in the app, and in
/// tests one that records no screen.
trait CaptureEngine: Send + Sync + 'static {
    /// Starts recording the screen into `start.bundle_dir`. Blocking: it
    /// finds the display and starts ScreenCaptureKit.
    fn start(&self, start: Start) -> anyhow::Result<Box<dyn Recording>>;
}

/// A started capture.
trait Recording: Send {
    /// Stops recording and removes the bundle.
    fn cancel(self: Box<Self>);
    /// Stops recording the screen, its windows, and input now, keeping what
    /// was recorded for [`Sealed::finish`].
    fn stop(self: Box<Self>) -> Box<dyn Sealed>;
}

/// A stopped capture, waiting to be written.
trait Sealed: Send {
    /// Removes the bundle.
    fn cancel(self: Box<Self>);
    /// Blocking: transcribes `audio`, picks keyframes, and writes the
    /// bundle.
    fn finish(self: Box<Self>, audio: RecordedAudio) -> anyhow::Result<Written>;
}

/// What a written capture reports.
struct Written {
    words: usize,
    /// Spans of its audio that went missing.
    gaps: usize,
    /// Why it has no transcript, if it has none.
    missing_transcript: Option<String>,
}

impl CaptureEngine for scrawl_capture::capture::Engine {
    fn start(&self, start: Start) -> anyhow::Result<Box<dyn Recording>> {
        Ok(Box::new(scrawl_capture::capture::Engine::start(
            self, start,
        )?))
    }
}

impl Recording for scrawl_capture::capture::Capture {
    fn cancel(self: Box<Self>) {
        scrawl_capture::capture::Capture::cancel(*self);
    }

    fn stop(self: Box<Self>) -> Box<dyn Sealed> {
        Box::new(scrawl_capture::capture::Capture::stop(*self))
    }
}

impl Sealed for scrawl_capture::capture::SealedCapture {
    fn cancel(self: Box<Self>) {
        scrawl_capture::capture::SealedCapture::cancel(*self);
    }

    fn finish(self: Box<Self>, audio: RecordedAudio) -> anyhow::Result<Written> {
        let finished = scrawl_capture::capture::SealedCapture::finish(*self, Some(audio))?;
        tracing::info!(
            bundle = %finished.dir.display(),
            duration_ms = finished.duration_ms,
            summary = %finished.summary,
            "capture written"
        );
        Ok(Written {
            words: finished.words,
            gaps: finished.gaps,
            missing_transcript: finished.missing_transcript,
        })
    }
}

/// Scrawl's engine with nothing machine-wide: no ingest server (port
/// 47777), delivery retries, or global shortcuts. Hex never calls
/// `install_shared_resources`.
fn scrawl_engine(
    events: SyncSender<Event>,
    transcriber: Arc<dyn Transcriber>,
) -> scrawl_capture::capture::Engine {
    scrawl_capture::capture::Engine::new(Arc::new(HexHost { events }), Arc::new(HexConfig))
        .with_resources(SharedResources::none())
        .with_transcriber(transcriber)
}

/// What the capture asks for: the display under the pointer, on the clock
/// the dictation timeline shares, with Hex's audio and Hex's folder.
fn start_request(t0: CaptureInstant, bundle: PathBuf) -> Start {
    Start {
        t0_boot_ns: t0.as_nanos(),
        target: Target::ActiveDisplay,
        bundle_dir: Some(bundle),
        audio: AudioIn::External,
        window_log: true,
        note: None,
    }
}

/// Forwards events to a bounded channel (the `capture-events` thread logs
/// them) and logs through tracing. Never blocks: a full channel drops the
/// event. `Engine::new` also makes it the crate's process-wide log sink, so
/// the engine's own log lines arrive here once, never in Scrawl's log file.
struct HexHost {
    events: SyncSender<Event>,
}

impl Host for HexHost {
    fn emit(&self, event: Event) {
        if let Err(TrySendError::Full(event)) = self.events.try_send(event) {
            tracing::warn!(?event, "capture event queue is full; dropping an event");
        }
    }

    fn log(&self, line: &str) {
        tracing::info!(target: "hex::capture", "{line}");
    }

    fn metric(&self, name: &str, ms: f64) {
        tracing::debug!(target: "hex::capture", name, ms, "capture metric");
    }
}

/// The engine's settings, from Hex rather than `~/.config/scrawl`. Hex has
/// no capture settings yet, so these are Scrawl's defaults, chosen here:
/// auto-crop on (it applies to display captures), and the `agentContext`
/// summary format off.
struct HexConfig;

impl ConfigSource for HexConfig {
    fn engine_config(&self) -> anyhow::Result<EngineConfig> {
        Ok(hex_engine_config())
    }
}

fn hex_engine_config() -> EngineConfig {
    let mut config = EngineConfig::default();
    config.autocrop = Some(serde_json::Value::Bool(true));
    config
        .experiments
        .insert(scrawl_capture::handoff::AGENT_CONTEXT.into(), false);
    config
}

/// Word-timed transcription on the dictation engine's loaded model. It holds
/// the engine weakly, so a capture still writing never keeps it alive.
struct HexTranscriber {
    engine: Option<Weak<TranscriptionEngine>>,
}

impl Transcriber for HexTranscriber {
    /// Word times are milliseconds from `audio.samples[0]`; the engine moves
    /// them onto the capture's clock and builds the segments.
    fn transcribe(&self, audio: &RecordedAudio) -> anyhow::Result<Option<TranscriptData>> {
        let Some(engine) = &self.engine else {
            return Ok(None);
        };
        let samples = if audio.sample_rate == SAMPLE_RATE {
            audio.samples.clone()
        } else {
            crate::dictation::try_resample_for_parakeet(&audio.samples, audio.sample_rate)
                .map_err(|error| anyhow::anyhow!(error))?
        };
        let transcript = crate::parakeet::transcribe_with_words(engine, samples)
            .map_err(|error| anyhow::anyhow!("{error:#}"))?;
        let (_, selection) = crate::app_settings::transcription_selection();
        let ms = |value: i64| value.max(0) as u64;
        Ok(Some(TranscriptData {
            source: "hex".into(),
            model: Some(selection.model.as_str().into()),
            text: transcript.text,
            words: transcript
                .words
                .into_iter()
                .map(|word| WordTiming {
                    t0: ms(word.start_ms),
                    t1: ms(word.end_ms),
                    text: word.text,
                })
                .collect(),
            ..TranscriptData::default()
        }))
    }
}

/// Creates `dir` and missing parents owner-only, and makes `dir` itself
/// owner-only if it already existed.
fn private_dirs(dir: &Path) -> Result<()> {
    use std::os::unix::fs::PermissionsExt;
    DirBuilder::new()
        .recursive(true)
        .mode(0o700)
        .create(dir)
        .wrap_err_with(|| format!("could not create {}", dir.display()))?;
    fs::set_permissions(dir, fs::Permissions::from_mode(0o700))
        .wrap_err_with(|| format!("could not restrict {}", dir.display()))
}

/// A fresh owner-only `<captures>/<unix ms>.scrawl` folder; never adopts one
/// that exists.
fn new_bundle_dir(captures_dir: &Path) -> Result<PathBuf> {
    private_dirs(captures_dir)?;
    let stamp = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis();
    for attempt in 0..100 {
        let name = match attempt {
            0 => format!("{stamp}.scrawl"),
            n => format!("{stamp}-{n}.scrawl"),
        };
        let dir = captures_dir.join(name);
        match DirBuilder::new().mode(0o700).create(&dir) {
            Ok(()) => return Ok(dir),
            Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => continue,
            Err(error) => {
                return Err(error).wrap_err_with(|| format!("could not create {}", dir.display()));
            }
        }
    }
    Err(eyre!(
        "could not name a new capture in {}",
        captures_dir.display()
    ))
}

/// Makes everything in `dir` owner-only. Scrawl writes its manifest, audio,
/// and frames `0600`, but the movie, the window log, and folders still get
/// the process umask.
fn restrict_bundle(dir: &Path) {
    use std::os::unix::fs::PermissionsExt;
    let mut pending = vec![dir.to_path_buf()];
    while let Some(dir) = pending.pop() {
        let Ok(entries) = fs::read_dir(&dir) else {
            continue;
        };
        for entry in entries.flatten() {
            let Ok(kind) = entry.file_type() else {
                continue;
            };
            let mode = if kind.is_dir() {
                pending.push(entry.path());
                0o700
            } else if kind.is_file() {
                0o600
            } else {
                continue;
            };
            if let Err(error) = fs::set_permissions(entry.path(), fs::Permissions::from_mode(mode))
            {
                tracing::warn!(%error, path = %entry.path().display(), "could not restrict a capture file");
            }
        }
    }
}

fn remove_bundle(dir: &Path) {
    match fs::remove_dir_all(dir) {
        Ok(()) => {}
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
        Err(error) => tracing::warn!(%error, bundle = %dir.display(), "could not remove a capture"),
    }
}

// ---------------------------------------------------------------------------
// The controller.

/// Where a capture's audio comes from: the dictation timeline.
pub trait AudioTimeline {
    fn open_sink(&self, sink: SyncSender<CaptureAudio>, since: CaptureInstant) -> Result<()>;
    fn close_sink(&self, through: CaptureInstant) -> Result<Option<CaptureSinkClosed>>;
}

impl AudioTimeline for DictationAudio {
    fn open_sink(&self, sink: SyncSender<CaptureAudio>, since: CaptureInstant) -> Result<()> {
        self.open_capture_sink(sink, since)
    }

    fn close_sink(&self, through: CaptureInstant) -> Result<Option<CaptureSinkClosed>> {
        self.close_capture_sink(through)
    }
}

/// How a capture's collector ends.
enum Seal {
    Finish {
        through: CaptureInstant,
        closed: Option<CaptureSinkClosed>,
    },
    Cancel,
}

struct Active {
    bundle: PathBuf,
    t0: CaptureInstant,
    seal: SyncSender<Seal>,
    collector: JoinHandle<()>,
}

/// A stopped capture and its audio, waiting for the finisher.
struct FinishJob {
    bundle: PathBuf,
    capture: Box<dyn Sealed>,
    audio: RecordedAudio,
}

#[derive(Default)]
struct Outcomes {
    /// Every capture from its start until its outcome is settled: starting,
    /// recording, waiting to be written, writing, or cancelling. Each is
    /// settled exactly once, by its collector or by the finisher.
    unfinished: Vec<PathBuf>,
    last: Option<ScrawlCaptureOutcome>,
}

impl Outcomes {
    fn settle(&mut self, bundle: PathBuf, stage: Stage, written: Option<Written>) {
        self.unfinished.retain(|unfinished| unfinished != &bundle);
        self.last = Some(ScrawlCaptureOutcome {
            bundle,
            stage: stage.label().into(),
            words: written.as_ref().map(|written| written.words),
            gaps: written.as_ref().map(|written| written.gaps),
            missing_transcript: written.and_then(|written| written.missing_transcript),
        });
    }
}

type SharedOutcomes = Arc<Mutex<Outcomes>>;

fn lock(outcomes: &Mutex<Outcomes>) -> std::sync::MutexGuard<'_, Outcomes> {
    outcomes.lock().unwrap_or_else(|error| error.into_inner())
}

/// The `hex capture` controller, owned by the listener beside its
/// `DictationAudio`. The recording capture gets a `capture-audio` thread
/// that starts the engine (off the listener's thread) and collects its
/// audio, and stops recording the screen the moment the capture is sealed;
/// one `capture-finish` thread then writes stopped captures one at a time.
/// One capture records at a time.
pub struct CaptureHost {
    engine: Arc<dyn CaptureEngine>,
    captures_dir: PathBuf,
    active: Option<Active>,
    outcomes: SharedOutcomes,
    finisher: Option<(SyncSender<FinishJob>, JoinHandle<()>)>,
    /// Collectors of ended captures, until they exit.
    ended: Vec<JoinHandle<()>>,
}

impl CaptureHost {
    /// The host when the `capture` experiment is on; otherwise `None`, and
    /// no engine, thread, sink, or folder is created.
    pub fn start(engine: Option<Arc<TranscriptionEngine>>) -> Option<Self> {
        if !crate::experiments::enabled(crate::experiments::Experiment::Capture) {
            return None;
        }
        // scripts/validate-app.sh refuses a binary containing this line.
        tracing::info!("capture experiment on: `hex capture` is available");
        let transcriber = Arc::new(HexTranscriber {
            engine: engine.as_ref().map(Arc::downgrade),
        });
        let host = crate::app_paths::captures_dir().and_then(|captures_dir| {
            let (events, received) = mpsc::sync_channel(EVENT_CAPACITY);
            thread::Builder::new()
                .name("capture-events".into())
                .spawn(move || log_events(received))?;
            Self::with(Arc::new(scrawl_engine(events, transcriber)), captures_dir)
        });
        host.inspect_err(|error| tracing::warn!(%error, "could not start the capture host"))
            .ok()
    }

    fn with(engine: Arc<dyn CaptureEngine>, captures_dir: PathBuf) -> Result<Self> {
        let outcomes = SharedOutcomes::default();
        let (jobs, queued) = mpsc::sync_channel(MAX_FINISHING);
        let finisher = {
            let outcomes = outcomes.clone();
            thread::Builder::new()
                .name("capture-finish".into())
                .spawn(move || finish_captures(queued, &outcomes))?
        };
        Ok(Self {
            engine,
            captures_dir,
            active: None,
            outcomes,
            finisher: Some((jobs, finisher)),
            ended: Vec::new(),
        })
    }

    pub fn control(
        &mut self,
        action: ScrawlCaptureAction,
        audio: &impl AudioTimeline,
    ) -> ControlReply {
        let now = CaptureInstant::now();
        self.reap(now, audio);
        let result = match action {
            ScrawlCaptureAction::Start => self.begin(now, audio),
            ScrawlCaptureAction::Stop => self.end(now, audio, false),
            ScrawlCaptureAction::Cancel => self.end(now, audio, true),
            ScrawlCaptureAction::Status => Ok(self.report(false, None)),
        };
        result.unwrap_or_else(|error| ControlReply::error("failed", format!("{error:#}")))
    }

    /// Cancels a recording capture, as when Hex quits, and waits a bounded
    /// time for its cleanup and for captures still writing. Quitting stops
    /// their transcription, so they keep their audio without words.
    pub fn shutdown(&mut self, audio: &impl AudioTimeline) {
        let deadline = Instant::now() + SHUTDOWN_WAIT;
        if self.active.is_some() {
            let _ = self.end(CaptureInstant::now(), audio, true);
        }
        for collector in std::mem::take(&mut self.ended) {
            join_by(collector, deadline, "capture-audio");
        }
        // Dropping the queue's sender ends the finisher after its last job.
        if let Some((jobs, finisher)) = self.finisher.take() {
            drop(jobs);
            join_by(finisher, deadline, "capture-finish");
        }
    }

    /// Ends a capture whose engine failed to start: its collector exited on
    /// its own and settled the failure.
    fn reap(&mut self, now: CaptureInstant, audio: &impl AudioTimeline) {
        if self
            .active
            .as_ref()
            .is_some_and(|active| active.collector.is_finished())
            && let Some(active) = self.active.take()
        {
            let _ = audio.close_sink(now);
            self.ended.push(active.collector);
        }
    }

    fn begin(&mut self, now: CaptureInstant, audio: &impl AudioTimeline) -> Result<ControlReply> {
        if self.active.is_some() {
            return Ok(self.report(false, None));
        }
        let Some((jobs, _)) = &self.finisher else {
            return Ok(ControlReply::error("busy", "Hex is stopping"));
        };
        // Nothing records here, so every unfinished capture is stopped and
        // writing or about to be.
        let writing = lock(&self.outcomes).unfinished.len();
        if writing >= MAX_FINISHING {
            return Ok(ControlReply::error(
                "busy",
                format!("{writing} captures are still being written; try again shortly"),
            ));
        }
        self.ended.retain(|collector| !collector.is_finished());
        let bundle = new_bundle_dir(&self.captures_dir)?;
        let (sink, chunks) = mpsc::sync_channel(SINK_CAPACITY);
        if let Err(error) = audio.open_sink(sink, now) {
            remove_bundle(&bundle);
            return Err(error);
        }
        // Unfinished from here until its collector or the finisher settles
        // it, whatever stop or cancel does meanwhile.
        lock(&self.outcomes).unfinished.push(bundle.clone());
        let (seal, sealed) = mpsc::sync_channel(1);
        let collector = {
            let engine = self.engine.clone();
            let start = start_request(now, bundle.clone());
            let jobs = jobs.clone();
            let outcomes = self.outcomes.clone();
            thread::Builder::new()
                .name("capture-audio".into())
                .spawn(move || collect(&*engine, start, chunks, sealed, &jobs, &outcomes))
                .inspect_err(|_| {
                    let _ = audio.close_sink(now);
                    remove_bundle(&bundle);
                    lock(&self.outcomes)
                        .unfinished
                        .retain(|unfinished| unfinished != &bundle);
                })?
        };
        tracing::info!(bundle = %bundle.display(), "capture started");
        self.active = Some(Active {
            bundle,
            t0: now,
            seal,
            collector,
        });
        Ok(self.report(true, None))
    }

    fn end(
        &mut self,
        now: CaptureInstant,
        audio: &impl AudioTimeline,
        cancel: bool,
    ) -> Result<ControlReply> {
        let Some(active) = self.active.take() else {
            return Ok(self.report(false, None));
        };
        let closed = audio.close_sink(now).unwrap_or_else(|error| {
            tracing::warn!(%error, "could not close the capture sink");
            None
        });
        let seal = if cancel {
            Seal::Cancel
        } else {
            Seal::Finish {
                through: now,
                closed,
            }
        };
        let _ = active.seal.send(seal);
        self.ended.push(active.collector);
        tracing::info!(bundle = %active.bundle.display(), cancel, "capture ended");
        Ok(self.report(true, Some(active.bundle)))
    }

    fn report(&self, changed: bool, ended: Option<PathBuf>) -> ControlReply {
        let outcomes = lock(&self.outcomes);
        let (state, bundle, elapsed_ms) = match &self.active {
            Some(active) => (
                ScrawlCaptureState::Recording,
                Some(active.bundle.clone()),
                Some(CaptureInstant::now().duration_since(active.t0).as_millis() as u64),
            ),
            None => (ScrawlCaptureState::Idle, ended, None),
        };
        ControlReply::ScrawlCapture(Box::new(ScrawlCaptureReport {
            changed,
            state,
            bundle,
            elapsed_ms,
            finishing: outcomes
                .unfinished
                .iter()
                .filter(|unfinished| {
                    self.active
                        .as_ref()
                        .is_none_or(|active| &active.bundle != *unfinished)
                })
                .cloned()
                .collect(),
            last: outcomes.last.clone(),
        }))
    }
}

/// Waits for `thread` until `deadline`, then leaves it running.
fn join_by(thread: JoinHandle<()>, deadline: Instant, name: &str) {
    while !thread.is_finished() && Instant::now() < deadline {
        thread::sleep(Duration::from_millis(5));
    }
    if thread.is_finished() {
        let _ = thread.join();
    } else {
        tracing::warn!(
            thread = name,
            "capture thread did not stop in time; leaving it"
        );
    }
}

fn log_events(events: Receiver<Event>) {
    for event in events {
        if let Event::Progress { bundle, stage } = event {
            tracing::info!(bundle = %bundle.display(), stage = stage.label(), "capture progress");
        }
    }
}

/// The `capture-finish` thread: writes stopped captures in order, one at a
/// time, and settles each outcome.
fn finish_captures(jobs: Receiver<FinishJob>, outcomes: &Mutex<Outcomes>) {
    for FinishJob {
        bundle,
        capture,
        audio,
    } in jobs
    {
        let finished = capture.finish(audio);
        restrict_bundle(&bundle);
        match finished {
            Ok(written) => {
                tracing::info!(
                    bundle = %bundle.display(),
                    words = written.words,
                    gaps = written.gaps,
                    missing_transcript = written.missing_transcript.as_deref(),
                    "capture finished"
                );
                lock(outcomes).settle(bundle, Stage::Done, Some(written));
            }
            Err(error) => {
                tracing::error!(error = format!("{error:#}"), "capture failed");
                lock(outcomes).settle(bundle, Stage::Failed, None);
            }
        }
    }
}

/// One capture's audio thread: starts the engine, collects until the sink
/// closes, then, as sealed, stops the capture at once and queues it for the
/// finisher, or cancels it. If the engine cannot start, it removes the
/// bundle and settles the failure; dropping `chunks` closes the sink.
fn collect(
    engine: &dyn CaptureEngine,
    start: Start,
    chunks: Receiver<CaptureAudio>,
    sealed: Receiver<Seal>,
    jobs: &SyncSender<FinishJob>,
    outcomes: &Mutex<Outcomes>,
) {
    let t0 = CaptureInstant::from_nanos(start.t0_boot_ns);
    let bundle = start.bundle_dir.clone().unwrap_or_default();
    let capture = match engine.start(start) {
        Ok(capture) => capture,
        Err(error) => {
            tracing::error!(error = format!("{error:#}"), "could not start the capture");
            drop(chunks);
            remove_bundle(&bundle);
            lock(outcomes).settle(bundle, Stage::Failed, None);
            return;
        }
    };
    let mut timeline = Timeline::new(t0);
    for chunk in chunks {
        match chunk {
            CaptureAudio::Samples {
                samples,
                sample_rate,
                captured_through,
            } => timeline.push(&samples, sample_rate, captured_through),
            CaptureAudio::Gap { from, through } => timeline.gap(from, through),
        }
    }
    match sealed.recv() {
        Ok(Seal::Finish { through, closed }) => {
            // The screen stops now, not when the finisher gets to it.
            let capture = capture.stop();
            let audio = timeline.finish(through, closed);
            // Starts are refused while the queue is full, so it has room
            // unless Hex is stopping.
            let job = FinishJob {
                bundle: bundle.clone(),
                capture,
                audio,
            };
            match jobs.try_send(job) {
                Ok(()) => return,
                Err(TrySendError::Full(job) | TrySendError::Disconnected(job)) => {
                    tracing::warn!("no room to write the capture; discarding it");
                    job.capture.cancel();
                }
            }
        }
        Ok(Seal::Cancel) | Err(_) => capture.cancel(),
    }
    lock(outcomes).settle(bundle, Stage::Cancelled, None);
}

/// A capture's audio at 16 kHz on its own clock: resampled as it arrives,
/// with missing spans padded with silence so positions stay on the clock.
struct Timeline {
    t0: CaptureInstant,
    /// Where `samples[0]` sits on the clock, once audio arrived.
    first: Option<CaptureInstant>,
    samples: Vec<f32>,
    resampler: Option<(u32, StreamingResampler)>,
    gaps: Vec<GapSpan>,
    /// The most samples kept (`MAX_CAPTURE`).
    limit: usize,
    /// Where audio past the limit started being dropped.
    truncated_at: Option<CaptureInstant>,
}

impl Timeline {
    fn new(t0: CaptureInstant) -> Self {
        Self::with_limit(t0, MAX_CAPTURE)
    }

    fn with_limit(t0: CaptureInstant, limit: Duration) -> Self {
        Self {
            t0,
            first: None,
            samples: Vec::new(),
            resampler: None,
            gaps: Vec::new(),
            limit: (limit.as_nanos() * u128::from(SAMPLE_RATE) / 1_000_000_000) as usize,
            truncated_at: None,
        }
    }

    fn push(&mut self, samples: &[f32], sample_rate: u32, through: CaptureInstant) {
        if samples.is_empty() || sample_rate == 0 {
            return;
        }
        let from = through.sub_frames(samples.len(), sample_rate);
        self.first.get_or_insert(from.max(self.t0));
        if self
            .resampler
            .as_ref()
            .is_none_or(|(rate, _)| *rate != sample_rate)
        {
            self.flush();
            match StreamingResampler::new(sample_rate) {
                Ok(resampler) => self.resampler = Some((sample_rate, resampler)),
                Err(error) => {
                    tracing::warn!(%error, sample_rate, "capture audio cannot be resampled");
                    self.gap(from, through);
                    return;
                }
            }
        }
        if let Some((_, resampler)) = &mut self.resampler {
            let resampled = resampler.push(samples);
            self.append(&resampled, through);
        }
    }

    fn gap(&mut self, from: CaptureInstant, through: CaptureInstant) {
        if through.duration_since(from) <= GAP_TOLERANCE {
            return;
        }
        let Some(first) = self.first else {
            // Before any audio: the recording simply starts later.
            return;
        };
        if self.truncated_at.is_some() {
            // Past the limit: `finish` records everything after it as one gap.
            return;
        }
        self.flush();
        let limit_at = first + duration_for(self.limit);
        self.gaps.push(GapSpan {
            start_ms: self.clock_ms(from.max(first).min(limit_at)),
            end_ms: self.clock_ms(through.min(limit_at)),
        });
        // Padding stops one sample past the limit, which marks the truncation.
        let end = position(first, through).min(self.limit + 1);
        if end > self.samples.len() {
            let padding = vec![0.0; end - self.samples.len()];
            self.append(&padding, through);
        }
    }

    fn finish(
        mut self,
        through: CaptureInstant,
        closed: Option<CaptureSinkClosed>,
    ) -> RecordedAudio {
        if let Some(closed) = closed {
            if let Some((from, until)) = closed.pending_gap {
                self.gap(from, until);
            }
            if closed.accounted_through < through {
                self.gap(closed.accounted_through.max(self.t0), through);
            }
        }
        self.flush();
        if let Some(truncated_at) = self.truncated_at {
            self.gaps.push(GapSpan {
                start_ms: self.clock_ms(truncated_at),
                end_ms: self.clock_ms(through),
            });
        }
        let Some(first) = self.first else {
            return RecordedAudio::new(Vec::new(), SAMPLE_RATE, self.clock_ms(through), Vec::new());
        };
        // Audio delivered after the stop belongs to no capture.
        self.samples.truncate(position(first, through));
        let start_offset_ms = self.clock_ms(first);
        RecordedAudio::new(self.samples, SAMPLE_RATE, start_offset_ms, self.gaps)
    }

    fn flush(&mut self) {
        if let Some((_, resampler)) = self.resampler.take() {
            let rest = resampler.finish();
            let through = self
                .first
                .map(|first| first + duration_for(self.samples.len() + rest.len()))
                .unwrap_or(self.t0);
            self.append(&rest, through);
        }
    }

    fn append(&mut self, samples: &[f32], through: CaptureInstant) {
        let limit = self.limit;
        let room = limit.saturating_sub(self.samples.len());
        if samples.len() > room && self.truncated_at.is_none() {
            tracing::warn!("capture reached its length limit; dropping the rest of its audio");
            self.truncated_at = Some(
                self.first
                    .map_or(through, |first| first + duration_for(limit)),
            );
        }
        self.samples
            .extend_from_slice(&samples[..samples.len().min(room)]);
    }

    fn clock_ms(&self, at: CaptureInstant) -> u64 {
        at.duration_since(self.t0).as_millis() as u64
    }
}

fn position(first: CaptureInstant, at: CaptureInstant) -> usize {
    (at.duration_since(first).as_nanos() * u128::from(SAMPLE_RATE) / 1_000_000_000) as usize
}

fn duration_for(samples: usize) -> Duration {
    crate::audio::duration_for_frames(samples, SAMPLE_RATE)
}

/// `hex dev capture-replay`: runs a capture through [`CaptureHost`] on a
/// fresh transcription engine with the selected model, feeding `wavs` in
/// place of the dictation timeline, and returns the finished bundle. Scrawl's
/// engine records the screen meanwhile, which needs Screen Recording.
#[cfg(debug_assertions)]
pub fn replay(wavs: &[PathBuf]) -> Result<PathBuf> {
    let mut samples = Vec::new();
    let mut sample_rate = None;
    for wav in wavs {
        let (audio, rate) = crate::voice_filter::read_wav_mono_f32(wav)
            .map_err(|error| eyre!("{}: {error}", wav.display()))?;
        if *sample_rate.get_or_insert(rate) != rate {
            return Err(eyre!(
                "{} is not {} Hz",
                wav.display(),
                sample_rate.unwrap_or(rate)
            ));
        }
        if !samples.is_empty() {
            samples.extend(std::iter::repeat_n(0.0, rate as usize * 3 / 10));
        }
        samples.extend(audio);
    }
    let sample_rate = sample_rate.ok_or_else(|| eyre!("no WAV files"))?;
    let engine = Arc::new(TranscriptionEngine::start(Default::default())?);
    let mut host = CaptureHost::start(Some(engine.clone())).ok_or_else(|| {
        eyre!("the `capture` experiment is off; run with HEX_EXPERIMENTS=capture")
    })?;
    let timeline = WavTimeline {
        samples: Arc::new(samples),
        sample_rate,
        feeder: Mutex::new(None),
    };
    let ControlReply::ScrawlCapture(started) = host.control(ScrawlCaptureAction::Start, &timeline)
    else {
        return Err(eyre!("the capture did not start"));
    };
    let bundle = started
        .bundle
        .ok_or_else(|| eyre!("the capture has no bundle"))?;
    let ends = CaptureInstant::from_nanos(started_at(&host)?)
        + crate::audio::duration_for_frames(timeline.samples.len(), sample_rate);
    while CaptureInstant::now() < ends + Duration::from_millis(20) {
        thread::sleep(Duration::from_millis(20));
    }
    host.control(ScrawlCaptureAction::Stop, &timeline);
    for _ in 0..60_000 {
        let ControlReply::ScrawlCapture(status) =
            host.control(ScrawlCaptureAction::Status, &timeline)
        else {
            break;
        };
        if let Some(last) = status.last.filter(|last| last.bundle == bundle) {
            if last.stage != "done" {
                return Err(eyre!("the capture {}", last.stage));
            }
            return Ok(bundle);
        }
        thread::sleep(Duration::from_millis(10));
    }
    Err(eyre!("the capture never finished"))
}

#[cfg(debug_assertions)]
fn started_at(host: &CaptureHost) -> Result<u64> {
    host.active
        .as_ref()
        .map(|active| active.t0.as_nanos())
        .ok_or_else(|| eyre!("no capture is recording"))
}

/// Recorded audio standing in for the dictation timeline: the whole file is
/// delivered at once, stamped as if captured from the capture's start.
#[cfg(debug_assertions)]
struct WavTimeline {
    samples: Arc<Vec<f32>>,
    sample_rate: u32,
    feeder: Mutex<Option<(thread::JoinHandle<()>, CaptureInstant)>>,
}

#[cfg(debug_assertions)]
impl AudioTimeline for WavTimeline {
    fn open_sink(&self, sink: SyncSender<CaptureAudio>, since: CaptureInstant) -> Result<()> {
        let samples = self.samples.clone();
        let sample_rate = self.sample_rate;
        let chunk = (sample_rate as usize / 100).max(1);
        let feeder = thread::spawn(move || {
            for (index, part) in samples.chunks(chunk).enumerate() {
                let through = since
                    + crate::audio::duration_for_frames(index * chunk + part.len(), sample_rate);
                let audio = CaptureAudio::Samples {
                    samples: part.to_vec(),
                    sample_rate,
                    captured_through: through,
                };
                if sink.send(audio).is_err() {
                    return;
                }
            }
        });
        let ends = since + crate::audio::duration_for_frames(self.samples.len(), sample_rate);
        *self
            .feeder
            .lock()
            .unwrap_or_else(|error| error.into_inner()) = Some((feeder, ends));
        Ok(())
    }

    fn close_sink(&self, _through: CaptureInstant) -> Result<Option<CaptureSinkClosed>> {
        let feeder = self
            .feeder
            .lock()
            .unwrap_or_else(|error| error.into_inner())
            .take();
        Ok(feeder.map(|(feeder, ends)| {
            let _ = feeder.join();
            CaptureSinkClosed {
                pending_gap: None,
                accounted_through: ends,
            }
        }))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn t(ms: u64) -> CaptureInstant {
        CaptureInstant::from_nanos(90_000_000_000 + ms * 1_000_000)
    }

    fn temp_dir(name: &str) -> PathBuf {
        let unique = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        std::env::temp_dir().join(format!("hex-capture-{name}-{unique}"))
    }

    #[test]
    fn the_timeline_resamples_pads_gaps_and_ends_at_the_stop() {
        let mut timeline = Timeline::new(t(0));
        // Audio starts 100 ms in (a cold microphone).
        let mut through = t(100);
        for _ in 0..50 {
            through = through + Duration::from_millis(10);
            timeline.push(&[0.5; 480], 48_000, through);
        }
        // 300 ms dropped, then audio again.
        timeline.gap(through, through + Duration::from_millis(300));
        through = through + Duration::from_millis(300);
        for _ in 0..50 {
            through = through + Duration::from_millis(10);
            timeline.push(&[0.5; 160], 16_000, through);
        }
        let audio = timeline.finish(
            through.checked_sub(Duration::from_millis(100)).unwrap(),
            None,
        );
        assert_eq!(audio.sample_rate, 16_000);
        assert_eq!(audio.start_offset_ms, 100);
        assert_eq!(
            audio.gaps,
            [GapSpan {
                start_ms: 600,
                end_ms: 900
            }]
        );
        // 500 + 300 + 500 ms of audio, cut 100 ms before the last chunk.
        assert_eq!(audio.duration_ms(), 100 + 1_200);
        let resampled_silence = &audio.samples[8_000 + 100..8_000 + 4_700];
        assert!(resampled_silence.iter().all(|sample| *sample == 0.0));
    }

    #[test]
    fn a_pending_gap_and_a_stalled_tail_come_back_from_the_closed_sink() {
        let mut timeline = Timeline::new(t(0));
        timeline.push(&[0.5; 1_600], 16_000, t(100));
        let audio = timeline.finish(
            t(600),
            Some(CaptureSinkClosed {
                pending_gap: Some((t(100), t(300))),
                accounted_through: t(300),
            }),
        );
        assert_eq!(
            audio.gaps,
            [
                GapSpan {
                    start_ms: 100,
                    end_ms: 300
                },
                GapSpan {
                    start_ms: 300,
                    end_ms: 600
                }
            ]
        );
        assert_eq!(audio.samples.len(), 9_600);
    }

    #[derive(Default)]
    struct FakeTimeline {
        open: Mutex<Option<SyncSender<CaptureAudio>>>,
        /// The end of the audio fed so far.
        fed: Mutex<Option<CaptureInstant>>,
        /// Runs at the start of the next `close_sink`.
        on_close: Mutex<Option<Box<dyn FnOnce() + Send>>>,
    }

    impl AudioTimeline for FakeTimeline {
        fn open_sink(&self, sink: SyncSender<CaptureAudio>, _since: CaptureInstant) -> Result<()> {
            let mut open = self.open.lock().unwrap();
            assert!(open.is_none(), "one sink at a time");
            *open = Some(sink);
            Ok(())
        }

        fn close_sink(&self, _through: CaptureInstant) -> Result<Option<CaptureSinkClosed>> {
            let on_close = self.on_close.lock().unwrap().take();
            if let Some(on_close) = on_close {
                on_close();
            }
            let fed = self.fed.lock().unwrap().take();
            Ok(self.open.lock().unwrap().take().map(|_| CaptureSinkClosed {
                pending_gap: None,
                accounted_through: fed.unwrap_or(CaptureInstant::ZERO),
            }))
        }
    }

    impl FakeTimeline {
        fn feed(&self, samples: usize, through: CaptureInstant) {
            *self.fed.lock().unwrap() = Some(through);
            let open = self.open.lock().unwrap();
            open.as_ref()
                .expect("a sink is open")
                .send(CaptureAudio::Samples {
                    samples: vec![0.25; samples],
                    sample_rate: 16_000,
                    captured_through: through,
                })
                .unwrap();
        }
    }

    struct FakeTranscriber;

    impl Transcriber for FakeTranscriber {
        fn transcribe(&self, audio: &RecordedAudio) -> anyhow::Result<Option<TranscriptData>> {
            assert_eq!(audio.sample_rate, 16_000);
            Ok(Some(TranscriptData {
                source: "fake".into(),
                text: "hello there".into(),
                words: vec![
                    WordTiming {
                        t0: 0,
                        t1: 200,
                        text: "hello".into(),
                    },
                    WordTiming {
                        t0: 250,
                        t1: 500,
                        text: "there".into(),
                    },
                ],
                ..TranscriptData::default()
            }))
        }
    }

    /// Scrawl's engine minus the screen: it writes the bundle with Scrawl's
    /// own writer from the audio Hex hands it and no frames.
    struct FakeEngine {
        transcriber: Arc<dyn Transcriber>,
        starts: Mutex<Vec<Start>>,
        steps: Steps,
        refuse: bool,
    }

    /// What each capture did, in order: `stop`, then `finish` once its
    /// write begins.
    type Steps = Arc<Mutex<Vec<(&'static str, PathBuf)>>>;

    impl FakeEngine {
        fn new(transcriber: Arc<dyn Transcriber>) -> Arc<Self> {
            Arc::new(Self {
                transcriber,
                starts: Mutex::default(),
                steps: Steps::default(),
                refuse: false,
            })
        }
    }

    impl CaptureEngine for FakeEngine {
        fn start(&self, start: Start) -> anyhow::Result<Box<dyn Recording>> {
            self.starts.lock().unwrap().push(start.clone());
            if self.refuse {
                anyhow::bail!("no screen to record");
            }
            Ok(Box::new(FakeRecording {
                dir: start.bundle_dir.unwrap(),
                transcriber: self.transcriber.clone(),
                steps: self.steps.clone(),
            }))
        }
    }

    struct FakeRecording {
        dir: PathBuf,
        transcriber: Arc<dyn Transcriber>,
        steps: Steps,
    }

    impl Recording for FakeRecording {
        fn cancel(self: Box<Self>) {
            fs::remove_dir_all(&self.dir).unwrap();
        }

        fn stop(self: Box<Self>) -> Box<dyn Sealed> {
            self.steps.lock().unwrap().push(("stop", self.dir.clone()));
            self
        }
    }

    impl Sealed for FakeRecording {
        fn cancel(self: Box<Self>) {
            fs::remove_dir_all(&self.dir).unwrap();
        }

        fn finish(self: Box<Self>, audio: RecordedAudio) -> anyhow::Result<Written> {
            use scrawl_capture::model::{CaptureMode, TargetFrame, TargetInfo};
            self.steps
                .lock()
                .unwrap()
                .push(("finish", self.dir.clone()));
            let duration_ms = audio.duration_ms();
            let gaps = audio.gaps.len();
            let target = TargetInfo {
                mode: CaptureMode::Window,
                app: None,
                bundle_id: None,
                window_title: None,
                window_id: None,
                display_id: 0,
                frame: TargetFrame {
                    x: 0.0,
                    y: 0.0,
                    width: 800.0,
                    height: 600.0,
                    scale: 2.0,
                },
            };
            let recording = scrawl_capture::capture::ScreenRecording {
                frames: Vec::new(),
                video: None,
                duration_ms,
                video_offset_ms: 0,
            };
            let (_, bundle, _) = scrawl_capture::pipeline::finalize_capture_bundle_with_flags(
                &self.dir,
                target,
                recording,
                Vec::new(),
                Vec::new(),
                Vec::new(),
                Some(audio),
                None,
                Default::default(),
                Some(&*self.transcriber),
            )?;
            Ok(Written {
                words: bundle.transcript.words.len(),
                gaps,
                missing_transcript: bundle.transcript.missing.map(|missing| missing.reason),
            })
        }
    }

    fn mode(path: &Path) -> u32 {
        use std::os::unix::fs::PermissionsExt;
        fs::metadata(path).unwrap().permissions().mode() & 0o777
    }

    fn report(reply: ControlReply) -> ScrawlCaptureReport {
        match reply {
            ControlReply::ScrawlCapture(report) => *report,
            other => panic!("unexpected reply {other:?}"),
        }
    }

    fn wait_for_last(
        host: &mut CaptureHost,
        audio: &FakeTimeline,
        bundle: &Path,
        idle: bool,
    ) -> ScrawlCaptureOutcome {
        for _ in 0..1_000 {
            let status = report(host.control(ScrawlCaptureAction::Status, audio));
            if (!idle || status.finishing.is_empty())
                && let Some(last) = status.last
                && last.bundle == bundle
            {
                return last;
            }
            thread::sleep(Duration::from_millis(10));
        }
        panic!("the capture never finished");
    }

    #[test]
    fn controls_are_idempotent_and_stop_writes_a_scrawl_bundle() {
        let dir = temp_dir("control");
        let audio = FakeTimeline::default();
        let engine = FakeEngine::new(Arc::new(FakeTranscriber));
        let mut host = CaptureHost::with(engine.clone(), dir.clone()).unwrap();

        let idle = report(host.control(ScrawlCaptureAction::Stop, &audio));
        assert!(!idle.changed && idle.bundle.is_none());
        assert!(!report(host.control(ScrawlCaptureAction::Cancel, &audio)).changed);

        let started = report(host.control(ScrawlCaptureAction::Start, &audio));
        assert!(started.changed);
        assert_eq!(started.state, ScrawlCaptureState::Recording);
        let bundle = started.bundle.unwrap();
        let again = report(host.control(ScrawlCaptureAction::Start, &audio));
        assert!(!again.changed);
        assert_eq!(again.bundle.as_ref(), Some(&bundle));

        // Half a second of audio arrives 300 ms after the start, as from a
        // cold microphone.
        let t0 = host.active.as_ref().unwrap().t0;
        audio.feed(8_000, t0 + Duration::from_millis(800));
        while CaptureInstant::now() < t0 + Duration::from_millis(820) {
            thread::sleep(Duration::from_millis(10));
        }
        let stopped = report(host.control(ScrawlCaptureAction::Stop, &audio));
        assert!(stopped.changed);
        assert_eq!(stopped.state, ScrawlCaptureState::Idle);
        assert_eq!(stopped.bundle.as_ref(), Some(&bundle));
        assert!(!report(host.control(ScrawlCaptureAction::Stop, &audio)).changed);

        let last = wait_for_last(&mut host, &audio, &bundle, true);
        assert_eq!(last.stage, "done");
        assert_eq!(last.words, Some(2));
        // The fake audio ends before the stop, so the tail may count as a gap.
        assert!(last.gaps.is_some());
        assert_eq!(last.missing_transcript, None);

        // Hex's audio, on Hex's clock, into Hex's folder; nothing else.
        let start = engine.starts.lock().unwrap()[0].clone();
        assert_eq!(start.audio, AudioIn::External);
        assert!(matches!(start.target, Target::ActiveDisplay));
        assert_eq!(start.bundle_dir.as_ref(), Some(&bundle));
        assert_eq!(start.t0_boot_ns, t0.as_nanos());
        assert!(bundle.starts_with(&dir));

        // Scrawl's writer pads audio.wav to the capture's clock and moves
        // the words onto it.
        let wav = hound::WavReader::open(bundle.join("audio.wav")).unwrap();
        assert_eq!(wav.spec().sample_rate, 16_000);
        let leading = 300 * 16;
        let samples = wav
            .into_samples::<i16>()
            .collect::<Result<Vec<_>, _>>()
            .unwrap();
        assert!(samples.len() >= leading + 8_000);
        assert!(samples[..leading - 16].iter().all(|sample| *sample == 0));
        assert!(
            samples[leading + 160..leading + 7_800]
                .iter()
                .all(|sample| *sample != 0)
        );
        let manifest: serde_json::Value =
            serde_json::from_slice(&fs::read(bundle.join("scrawl.json")).unwrap()).unwrap();
        let words = &manifest["transcript"]["words"];
        assert_eq!(manifest["transcript"]["text"], "hello there");
        assert_eq!(words[1]["text"], "there");
        assert_eq!(words[1]["t0"], 550);
        assert_eq!(manifest["audio"], "audio.wav");
        assert_eq!(mode(&dir), 0o700);
        assert_eq!(mode(&bundle), 0o700);
        for entry in fs::read_dir(&bundle).unwrap() {
            let entry = entry.unwrap();
            let expected = if entry.file_type().unwrap().is_dir() {
                0o700
            } else {
                0o600
            };
            assert_eq!(mode(&entry.path()), expected, "{}", entry.path().display());
        }

        // Cancel discards the bundle.
        let started = report(host.control(ScrawlCaptureAction::Start, &audio));
        let cancelled_bundle = started.bundle.unwrap();
        assert!(report(host.control(ScrawlCaptureAction::Cancel, &audio)).changed);
        assert!(!report(host.control(ScrawlCaptureAction::Cancel, &audio)).changed);
        let last = wait_for_last(&mut host, &audio, &cancelled_bundle, true);
        assert_eq!(last.stage, "cancelled");
        assert!(!cancelled_bundle.exists());
        assert!(bundle.exists());
        fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn an_engine_that_cannot_start_leaves_nothing_behind() {
        let dir = temp_dir("refused");
        let audio = FakeTimeline::default();
        let engine = Arc::new(FakeEngine {
            transcriber: Arc::new(FakeTranscriber),
            starts: Mutex::default(),
            steps: Steps::default(),
            refuse: true,
        });
        let mut host = CaptureHost::with(engine, dir.clone()).unwrap();
        let bundle = report(host.control(ScrawlCaptureAction::Start, &audio))
            .bundle
            .unwrap();
        let last = wait_for_last(&mut host, &audio, &bundle, true);
        assert_eq!(last.stage, "failed");
        assert!(!bundle.exists());
        let status = report(host.control(ScrawlCaptureAction::Status, &audio));
        assert_eq!(status.state, ScrawlCaptureState::Idle);
        assert!(audio.open.lock().unwrap().is_none(), "the sink is closed");
        // The next start is a fresh capture.
        assert!(report(host.control(ScrawlCaptureAction::Start, &audio)).changed);
        host.shutdown(&audio);
        fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn scrawls_engine_claims_nothing_machine_wide() {
        let (events, _received) = mpsc::sync_channel(1);
        let engine = scrawl_engine(events, Arc::new(FakeTranscriber));
        assert_eq!(engine.resources(), SharedResources::none());
    }

    #[test]
    fn the_engine_config_comes_from_hex() {
        let config = HexConfig.engine_config().unwrap();
        assert_eq!(
            scrawl_capture::autocrop::Settings::from_engine_config(&config),
            Some(Default::default()),
            "display captures crop with Scrawl's defaults"
        );
        assert!(!config.experiment(scrawl_capture::handoff::AGENT_CONTEXT));
        assert_eq!(config.transcriber, None);
    }

    #[test]
    fn padding_past_the_length_limit_stays_bounded() {
        let mut timeline = Timeline::with_limit(t(0), Duration::from_secs(1));
        timeline.push(&[0.5; 1_600], 16_000, t(100));
        // A gap far past the limit pads only up to it.
        timeline.gap(t(100), t(3 * 60 * 60 * 1_000));
        assert!(timeline.samples.len() <= 16_000);
        timeline.gap(t(3 * 60 * 60 * 1_000), t(6 * 60 * 60 * 1_000));
        timeline.push(&[0.5; 1_600], 16_000, t(6 * 60 * 60 * 1_000 + 100));
        let audio = timeline.finish(t(6 * 60 * 60 * 1_000 + 100), None);
        assert_eq!(audio.samples.len(), 16_000);
        assert_eq!(
            audio.gaps,
            [
                GapSpan {
                    start_ms: 100,
                    end_ms: 1_000
                },
                GapSpan {
                    start_ms: 1_000,
                    end_ms: 6 * 60 * 60 * 1_000 + 100
                }
            ]
        );
    }

    /// Holds every transcription until the test lets it through.
    struct GatedTranscriber(Mutex<Receiver<()>>);

    impl Transcriber for GatedTranscriber {
        fn transcribe(&self, _audio: &RecordedAudio) -> anyhow::Result<Option<TranscriptData>> {
            self.0.lock().unwrap().recv().ok();
            Ok(None)
        }
    }

    /// Waits until the one writer has taken `bundle`. Each capture's thread
    /// hands it over on its own schedule, so tests that need the first
    /// capture written first wait for it before starting the next.
    fn wait_until_writing(engine: &FakeEngine, bundle: &Path) {
        for _ in 0..1_000 {
            if engine
                .steps
                .lock()
                .unwrap()
                .iter()
                .any(|(step, dir)| *step == "finish" && dir == bundle)
            {
                return;
            }
            thread::sleep(Duration::from_millis(5));
        }
        panic!("{} was never written", bundle.display());
    }

    #[test]
    fn captures_finish_one_at_a_time_and_starts_wait_for_room() {
        let dir = temp_dir("bounded");
        let audio = FakeTimeline::default();
        let (gate, gated) = mpsc::channel();
        let engine = FakeEngine::new(Arc::new(GatedTranscriber(Mutex::new(gated))));
        let mut host = CaptureHost::with(engine.clone(), dir.clone()).unwrap();
        let mut bundles = Vec::new();
        for _ in 0..MAX_FINISHING {
            let started = report(host.control(ScrawlCaptureAction::Start, &audio));
            let bundle = started.bundle.unwrap();
            // Some audio, so each capture has something to transcribe.
            audio.feed(
                160,
                host.active.as_ref().unwrap().t0 + Duration::from_millis(10),
            );
            thread::sleep(Duration::from_millis(20));
            host.control(ScrawlCaptureAction::Stop, &audio);
            if bundles.is_empty() {
                wait_until_writing(&engine, &bundle);
            }
            bundles.push(bundle);
        }
        assert_eq!(
            report(host.control(ScrawlCaptureAction::Status, &audio)).finishing,
            bundles
        );
        let ControlReply::Error { code, .. } = host.control(ScrawlCaptureAction::Start, &audio)
        else {
            panic!("a start past the bound must be refused");
        };
        assert_eq!(code, "busy");
        // One finishes at a time, in order; the outcome is recorded directly.
        gate.send(()).unwrap();
        let first = wait_for_last(&mut host, &audio, &bundles[0], false);
        assert_eq!(first.stage, "done");
        assert_eq!(
            report(host.control(ScrawlCaptureAction::Status, &audio)).finishing,
            bundles[1..]
        );
        gate.send(()).unwrap();
        assert_eq!(
            wait_for_last(&mut host, &audio, &bundles[1], true).stage,
            "done"
        );
        assert!(report(host.control(ScrawlCaptureAction::Start, &audio)).changed);
        host.shutdown(&audio);
        fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn a_capture_queued_behind_a_slow_finish_has_already_stopped() {
        let dir = temp_dir("stopped");
        let audio = FakeTimeline::default();
        let (gate, gated) = mpsc::channel();
        let engine = FakeEngine::new(Arc::new(GatedTranscriber(Mutex::new(gated))));
        let mut host = CaptureHost::with(engine.clone(), dir.clone()).unwrap();
        let steps = |engine: &FakeEngine| engine.steps.lock().unwrap().clone();
        let mut bundles = Vec::new();
        for _ in 0..2 {
            let bundle = report(host.control(ScrawlCaptureAction::Start, &audio))
                .bundle
                .unwrap();
            audio.feed(
                160,
                host.active.as_ref().unwrap().t0 + Duration::from_millis(10),
            );
            host.control(ScrawlCaptureAction::Stop, &audio);
            if bundles.is_empty() {
                wait_until_writing(&engine, &bundle);
            }
            bundles.push(bundle);
        }
        // The first is stuck transcribing; the second waits behind it, its
        // screen already stopped.
        let (first, second) = (&bundles[0], &bundles[1]);
        let mut expected = vec![
            ("finish", first.clone()),
            ("stop", first.clone()),
            ("stop", second.clone()),
        ];
        expected.sort();
        let sorted = |mut steps: Vec<_>| {
            steps.sort();
            steps
        };
        for _ in 0..1_000 {
            if sorted(steps(&engine)) == expected {
                break;
            }
            thread::sleep(Duration::from_millis(5));
        }
        assert_eq!(sorted(steps(&engine)), expected);
        assert_eq!(
            report(host.control(ScrawlCaptureAction::Status, &audio)).finishing,
            bundles
        );
        gate.send(()).unwrap();
        gate.send(()).unwrap();
        assert_eq!(wait_for_last(&mut host, &audio, second, true).stage, "done");
        assert_eq!(steps(&engine).last(), Some(&("finish", second.clone())));
        host.shutdown(&audio);
        fs::remove_dir_all(dir).unwrap();
    }

    /// Fails to start once released.
    struct StallingEngine(Mutex<Receiver<()>>);

    impl CaptureEngine for StallingEngine {
        fn start(&self, _start: Start) -> anyhow::Result<Box<dyn Recording>> {
            let _ = self.0.lock().unwrap().recv();
            anyhow::bail!("no screen to record")
        }
    }

    #[test]
    fn a_failed_start_raced_by_stop_never_wedges_later_starts() {
        let dir = temp_dir("raced");
        let audio = FakeTimeline::default();
        let (release, released) = mpsc::channel();
        let mut host =
            CaptureHost::with(Arc::new(StallingEngine(Mutex::new(released))), dir.clone()).unwrap();
        for _ in 0..3 {
            let bundle = report(host.control(ScrawlCaptureAction::Start, &audio))
                .bundle
                .unwrap();
            // The engine fails and its collector settles inside the stop,
            // after the stop saw it still starting.
            let (release, outcomes, failed) =
                (release.clone(), host.outcomes.clone(), bundle.clone());
            *audio.on_close.lock().unwrap() = Some(Box::new(move || {
                release.send(()).unwrap();
                while lock(&outcomes)
                    .last
                    .as_ref()
                    .is_none_or(|last| last.bundle != failed)
                {
                    thread::sleep(Duration::from_millis(5));
                }
            }));
            assert!(report(host.control(ScrawlCaptureAction::Stop, &audio)).changed);
            let status = report(host.control(ScrawlCaptureAction::Status, &audio));
            assert!(status.finishing.is_empty(), "{:?}", status.finishing);
            assert_eq!(status.last.unwrap().stage, "failed");
            assert!(!bundle.exists());
        }
        host.shutdown(&audio);
        fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn shutdown_cancels_the_recording_capture_and_waits_for_its_cleanup() {
        let dir = temp_dir("shutdown");
        let audio = FakeTimeline::default();
        let mut host =
            CaptureHost::with(FakeEngine::new(Arc::new(FakeTranscriber)), dir.clone()).unwrap();
        let bundle = report(host.control(ScrawlCaptureAction::Start, &audio))
            .bundle
            .unwrap();
        assert!(bundle.exists());
        host.shutdown(&audio);
        assert!(!bundle.exists());
        let last = report(host.control(ScrawlCaptureAction::Status, &audio))
            .last
            .unwrap();
        assert_eq!((last.bundle, last.stage.as_str()), (bundle, "cancelled"));
        let ControlReply::Error { code, .. } = host.control(ScrawlCaptureAction::Start, &audio)
        else {
            panic!("a stopped host starts nothing");
        };
        assert_eq!(code, "busy");
        fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn the_host_is_absent_while_the_experiment_is_off() {
        // Nothing turns the experiment on in tests, so no host, engine,
        // thread, or sink exists and the listener answers `feature-off`.
        if !crate::experiments::enabled(crate::experiments::Experiment::Capture) {
            assert!(CaptureHost::start(None).is_none());
        }
    }
}
