//! A recording's lifecycle: start, an immediate `stop()`, and the finishing thread.
//!
//! Stopping detaches every source's recording output at once, then waits for each detached output
//! to report its result while its stream keeps running. Stopping a stream while
//! `ScreenCaptureKit` is still exporting a removed output can make the export run twice and abort,
//! losing the file or leaving it unfinished. Only after the output finishes (or fails) is the
//! stream stopped, and that stop is bounded because the capture service can stop answering.

use std::fs;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{self, Receiver};
use std::sync::{Arc, Mutex, PoisonError};
use std::thread::{self, JoinHandle};
use std::time::{Duration, Instant};

use serde::{Deserialize, Serialize};

use crate::clicks::ClickMonitor;
use crate::cursor::{
    CursorCapture, CursorClickCaptureStatus, CursorCollector, Millis, PointerSampler, Rect,
};
use crate::error::Error;
use crate::external_audio::{ExternalAudioSink, ExternalAudioWriter};
use crate::final_frame_hold::hold_final_video_frame;
use crate::frames::FrameSampler;
use crate::input::{HostInputDrain, InputLog, RecordedInput};
use crate::manifest::{
    Clock, FileEntry, FileKind, MANIFEST_VERSION, Manifest, Permission, SourceEntry, TrackKind,
    write_json,
};
use crate::plan::{Exclusions, MANIFEST_NAME, Names, Source};
use crate::system_audio::SystemAudioWriter;
use crate::time::{HostTime, MediaTime, host_delta_millis};

// ScreenCaptureKit can take well over ten seconds to finalize longer MP4s even after the file is
// complete on disk. Keep finishing bounded without cutting normal finalization short.
pub(crate) const RECORDING_CALLBACK_TIMEOUT: Duration = Duration::from_secs(60);
/// When the capture service crashes mid-stop, `ScreenCaptureKit` never calls the stop completion.
/// Bound the wait so the recording reports an error instead of finishing forever.
pub(crate) const STREAM_STOP_TIMEOUT: Duration = Duration::from_secs(10);
const STREAM_STOP_UNRESPONSIVE: &str =
    "ScreenCaptureKit stopped responding while stopping the recording";

#[derive(Clone, Debug)]
pub(crate) enum RecordingEvent {
    Started,
    Failed(String),
    StreamStopped(String),
    Finished,
}

impl RecordingEvent {
    fn failure(&self) -> Option<Error> {
        match self {
            Self::Failed(error) => Some(Error::Recording(error.clone())),
            Self::StreamStopped(error) => Some(Error::StreamStopped(error.clone())),
            Self::Started | Self::Finished => None,
        }
    }
}

/// A source's recording-output and stream events. Checking health never consumes the events
/// finishing waits for.
pub(crate) struct RecordingEvents {
    receiver: Receiver<RecordingEvent>,
    pending: std::collections::VecDeque<RecordingEvent>,
    failure: Option<RecordingEvent>,
}

impl RecordingEvents {
    pub(crate) fn new(receiver: Receiver<RecordingEvent>) -> Self {
        Self {
            receiver,
            pending: std::collections::VecDeque::new(),
            failure: None,
        }
    }

    fn health(&mut self) -> Result<(), Error> {
        if let Some(error) = self.failure.as_ref().and_then(RecordingEvent::failure) {
            return Err(error);
        }
        while let Ok(event) = self.receiver.try_recv() {
            let failure = event.failure();
            if failure.is_some() {
                self.failure = Some(event.clone());
            }
            self.pending.push_back(event);
            if let Some(error) = failure {
                return Err(error);
            }
        }
        Ok(())
    }

    fn recv_timeout(&mut self, timeout: Duration) -> Result<RecordingEvent, ()> {
        self.pending
            .pop_front()
            .map_or_else(|| self.receiver.recv_timeout(timeout).map_err(|_| ()), Ok)
    }
}

/// Stops a stream; run on its own thread so a hung stop cannot hold the caller.
pub(crate) type Stopper = Box<dyn FnOnce() -> Result<(), String> + Send>;

/// A source's native stream, behind a seam so the stop state machine runs against a fake.
pub(crate) trait SourceStream: Send {
    /// Removes the recording output. The output finishes its file afterwards and reports the
    /// result through the source's events.
    fn detach_output(&self) -> Result<(), String>;
    /// Removes the frame and audio handlers.
    fn remove_handlers(&mut self) -> Result<(), String>;
    fn stopper(&self) -> Stopper;
    /// What the recording output says it recorded.
    fn recorded_duration(&self) -> Option<MediaTime>;
    /// Leaves out `exclude` from now on, without restarting the stream.
    fn exclude(&self, exclude: &Exclusions) -> Result<(), Error>;
}

/// Runs `stop` with a bound. A stop that never returns leaves its thread blocked inside
/// `ScreenCaptureKit`; nothing else can reclaim it.
pub(crate) fn stop_stream_within(stop: Stopper, timeout: Duration) -> Result<(), String> {
    let (sender, receiver) = mpsc::sync_channel(1);
    thread::Builder::new()
        .name("capture-stream-stop".into())
        .spawn(move || {
            let _ = sender.send(stop());
        })
        .map_err(|error| error.to_string())?;
    receiver
        .recv_timeout(timeout)
        .unwrap_or_else(|_| Err(STREAM_STOP_UNRESPONSIVE.into()))
}

pub(crate) struct StopState {
    recording_error: Option<Error>,
    ancillary_error: Option<Error>,
    recording_output_removed: bool,
    stream_stopped: bool,
}

impl StopState {
    fn record_stream_stop(&mut self, stop: Stopper, timeout: Duration) {
        if let Err(error) = stop_stream_within(stop, timeout)
            && self.ancillary_error.is_none()
        {
            self.ancillary_error = Some(Error::Stop(error));
        }
    }

    fn lost() -> Self {
        Self {
            recording_error: Some(Error::Stop("the detach thread panicked".into())),
            ancillary_error: None,
            recording_output_removed: false,
            stream_stopped: false,
        }
    }
}

fn preferred_stop_error(recording: Option<Error>, ancillary: Option<Error>) -> Option<Error> {
    recording.or(ancillary)
}

/// Bounds for finishing; tests shorten them.
#[derive(Clone, Copy, Debug)]
pub(crate) struct Timeouts {
    pub callback: Duration,
    pub stream_stop: Duration,
}

impl Default for Timeouts {
    fn default() -> Self {
        Self {
            callback: RECORDING_CALLBACK_TIMEOUT,
            stream_stop: STREAM_STOP_TIMEOUT,
        }
    }
}

/// One source while it records.
pub(crate) struct SourceRun {
    pub source: Source,
    pub name: String,
    pub path: PathBuf,
    pub stream: Box<dyn SourceStream>,
    pub events: RecordingEvents,
    pub collector: Arc<Mutex<CursorCollector>>,
    /// When the recording output was attached.
    pub attached: HostTime,
    pub width: u32,
    pub height: u32,
    pub rect: Rect,
    pub audio_tracks: Vec<TrackKind>,
}

/// A source's part of the result.
struct SourceOutcome {
    entry: SourceEntry,
    cursor: SourceCursor,
}

impl SourceRun {
    fn detach(&mut self) -> StopState {
        let mut recording_error = self.events.health().err();
        let stream_stopped = matches!(recording_error, Some(Error::StreamStopped(_)));
        let recording_output_removed = match self.stream.detach_output() {
            Ok(()) => true,
            // A stream the system already stopped still reports its output's own result.
            Err(_) if stream_stopped => true,
            Err(error) => {
                recording_error.get_or_insert(Error::Recording(error));
                false
            }
        };
        StopState {
            recording_error,
            ancillary_error: None,
            recording_output_removed,
            stream_stopped,
        }
    }

    fn finish(
        mut self,
        mut stop: StopState,
        deadline: Instant,
        timeouts: Timeouts,
        stop_at: HostTime,
    ) -> SourceOutcome {
        if let Err(error) = self.stream.remove_handlers() {
            stop.ancillary_error.get_or_insert(Error::Stop(error));
        }
        let stopper = if stop.stream_stopped {
            None
        } else if stop.recording_output_removed {
            // Kept running until the detached output reports its result.
            Some(self.stream.stopper())
        } else {
            stop.record_stream_stop(self.stream.stopper(), timeouts.stream_stop);
            None
        };
        let (first_frame, mut cursor) = {
            let mut collector = self
                .collector
                .lock()
                .unwrap_or_else(PoisonError::into_inner);
            (collector.first_frame_host_time(), collector.close())
        };

        if stop.recording_output_removed {
            loop {
                let remaining = deadline.saturating_duration_since(Instant::now());
                match self.events.recv_timeout(remaining) {
                    Ok(RecordingEvent::Started | RecordingEvent::StreamStopped(_)) => {}
                    Ok(RecordingEvent::Failed(error)) => {
                        stop.recording_error = Some(Error::Recording(error));
                        break;
                    }
                    Ok(RecordingEvent::Finished) => {
                        // The system stopping the stream still leaves a complete file; keep it.
                        if stop.stream_stopped {
                            stop.recording_error = None;
                        }
                        break;
                    }
                    Err(()) => {
                        stop.recording_error = Some(Error::FinishTimeout);
                        break;
                    }
                }
            }
        }
        if let Some(stopper) = stopper {
            stop.record_stream_stop(stopper, timeouts.stream_stop);
        }

        let recorded = self.stream.recorded_duration();
        let mut duration_ms = recorded
            .and_then(|recorded| recorded.ceil_to_millis())
            .or_else(|| host_delta_millis(self.attached, stop_at))
            .unwrap_or(0);
        if stop.recording_error.is_none()
            && let Some(recorded) = recorded
            && let Some(held_end_ms) = hold_final_frame(&self.path, recorded)
        {
            duration_ms = duration_ms.max(held_end_ms);
        }
        cursor.telemetry = cursor.telemetry.sanitized_for_duration(Millis(duration_ms));
        if let Err(error) = fs::metadata(&self.path) {
            stop.ancillary_error.get_or_insert(Error::Output(error));
        }
        let error = preferred_stop_error(stop.recording_error, stop.ancillary_error);
        if let Some(error) = &error {
            tracing::warn!(source = ?self.source, %error, "source did not finish cleanly");
        }
        let start_ns = first_frame
            .map_or(self.attached, HostTime::from_ticks)
            .as_nanos();
        SourceOutcome {
            entry: SourceEntry {
                source: self.source,
                file: self.name,
                start_ns,
                duration_ms,
                width: self.width,
                height: self.height,
                frame: self.rect,
                audio_tracks: self.audio_tracks,
                error: error.map(|error| error.to_string()),
            },
            cursor: SourceCursor {
                source: self.source,
                start_ns,
                capture: cursor,
            },
        }
    }
}

/// `ScreenCaptureKit` writes a frame only when the screen changes, so a recording's video track
/// ends at the last change while audio runs to the stop. Holding the final frame keeps every
/// reader at the full length. Failure keeps the original file.
fn hold_final_frame(path: &Path, recorded: MediaTime) -> Option<u64> {
    match hold_final_video_frame(path, recorded) {
        Ok(hold) => hold.end().ceil_to_millis(),
        Err(error) => {
            tracing::warn!(path = %path.display(), %error, "kept the video without holding its final frame");
            None
        }
    }
}

/// One source's cursor telemetry, as written to [`Names::cursor`].
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct SourceCursor {
    pub source: Source,
    /// When the source's video starts on the host clock. Each `source_time` counts from here.
    pub start_ns: u64,
    #[serde(flatten)]
    pub capture: CursorCapture,
}

#[derive(Serialize)]
struct CursorFile<'a> {
    version: u32,
    sources: &'a [SourceCursor],
}

#[derive(Serialize)]
struct InputFile<'a> {
    version: u32,
    clock: Clock,
    anchor_ns: u64,
    dropped: u64,
    events: &'a [RecordedInput],
}

/// What a finished recording produced. Each source's problems are in its manifest entry, so a
/// partly failed recording still returns every file that survived.
#[derive(Clone, Debug)]
pub struct Finished {
    pub dir: PathBuf,
    pub manifest: Manifest,
    pub cursor: Vec<SourceCursor>,
    pub input: Vec<RecordedInput>,
    /// Host events dropped because the input queue was full.
    pub input_dropped: u64,
    /// Problems outside any one source: audio files, the input log, the manifest.
    pub errors: Vec<String>,
}

impl Finished {
    /// The first error any part of the recording reported.
    #[must_use]
    pub fn error(&self) -> Option<&str> {
        self.manifest
            .sources
            .iter()
            .find_map(|source| source.error.as_deref())
            .or_else(|| self.errors.first().map(String::as_str))
    }

    /// The path of the video for `sources[index]`.
    #[must_use]
    pub fn video(&self, index: usize) -> Option<PathBuf> {
        self.manifest
            .sources
            .get(index)
            .map(|source| self.dir.join(&source.file))
    }
}

/// Everything a running recording owns.
pub(crate) struct Active {
    pub dir: PathBuf,
    pub names: Names,
    pub anchor: HostTime,
    pub sources: Vec<SourceRun>,
    pub pointer: Option<PointerSampler>,
    pub clicks: Option<ClickMonitor>,
    pub host_input: Option<HostInputDrain>,
    pub input_log: Option<Arc<InputLog>>,
    pub frames: Vec<Arc<FrameSampler>>,
    pub external: Option<ExternalAudioWriter>,
    pub system_audio: Option<Arc<SystemAudioWriter>>,
    pub stopping: Arc<AtomicBool>,
    pub missing_permissions: Vec<Permission>,
    pub timeouts: Timeouts,
}

impl Active {
    pub(crate) fn new(dir: PathBuf, names: Names, anchor: HostTime) -> Self {
        Self {
            dir,
            names,
            anchor,
            sources: Vec::new(),
            pointer: None,
            clicks: None,
            host_input: None,
            input_log: None,
            frames: Vec::new(),
            external: None,
            system_audio: None,
            stopping: Arc::new(AtomicBool::new(false)),
            missing_permissions: Vec::new(),
            timeouts: Timeouts::default(),
        }
    }

    /// Everything after the cut: runs on the finishing thread.
    #[allow(clippy::too_many_lines)]
    pub(crate) fn finish(mut self, stop_at: HostTime, discard: bool) -> Finished {
        let mut errors = Vec::new();
        for sampler in &self.frames {
            sampler.flush();
        }
        if let Some(mut clicks) = self.clicks.take()
            && !clicks.stop()
        {
            errors.push("the click monitor thread panicked".to_owned());
        }
        let input_dropped = self.host_input.take().map_or(0, |mut drain| drain.stop());

        // Cut every source before waiting on any of them.
        let deadline = Instant::now() + self.timeouts.callback;
        let stops = in_parallel(
            "capture-detach",
            self.sources.iter_mut().collect(),
            SourceRun::detach,
        );
        if let Some(mut pointer) = self.pointer.take()
            && !pointer.stop()
        {
            errors.push("the pointer sampler thread panicked".to_owned());
        }
        let anchor = self.anchor;
        let timeouts = self.timeouts;
        let placeholders = self
            .sources
            .iter()
            .map(|run| {
                (
                    run.source,
                    run.name.clone(),
                    run.width,
                    run.height,
                    run.rect,
                )
            })
            .collect::<Vec<_>>();
        let outcomes = in_parallel(
            "capture-finish-source",
            self.sources
                .drain(..)
                .zip(stops)
                .map(|(run, stop)| (run, stop.unwrap_or_else(StopState::lost)))
                .collect(),
            |(run, stop)| run.finish(stop, deadline, timeouts, stop_at),
        );
        let mut sources = Vec::with_capacity(outcomes.len());
        let mut cursor = Vec::with_capacity(outcomes.len());
        for (outcome, (source, file, width, height, rect)) in outcomes.into_iter().zip(placeholders)
        {
            match outcome {
                Some(outcome) => {
                    sources.push(outcome.entry);
                    cursor.push(outcome.cursor);
                }
                None => sources.push(SourceEntry {
                    source,
                    file,
                    start_ns: anchor.as_nanos(),
                    duration_ms: 0,
                    width,
                    height,
                    frame: rect,
                    audio_tracks: Vec::new(),
                    error: Some("the finishing thread panicked".into()),
                }),
            }
        }

        let mut files = sources
            .iter()
            .enumerate()
            .map(|(index, source)| FileEntry {
                name: source.file.clone(),
                kind: FileKind::Video { source: index },
                start_ns: Some(source.start_ns),
            })
            .collect::<Vec<_>>();
        let mut audio_gaps = Vec::new();
        if let Some(external) = self.external.take() {
            let summary = external.finish(stop_at);
            if let Some(error) = summary.error {
                errors.push(format!("microphone audio: {error}"));
            }
            audio_gaps = summary.gaps;
            if let Some(name) = &self.names.microphone {
                files.push(FileEntry {
                    name: name.clone(),
                    kind: FileKind::Microphone {
                        sample_rate: summary.sample_rate,
                    },
                    start_ns: Some(anchor.as_nanos()),
                });
            }
        }
        if let Some(system) = self.system_audio.take() {
            let summary = system.finish();
            if let Some(error) = summary.error {
                errors.push(format!("system audio: {error}"));
            }
            if summary.dropped > 0 {
                tracing::warn!(
                    dropped = summary.dropped,
                    "system audio buffers were dropped"
                );
            }
            if let Some(name) = self.names.system_audio.as_ref().filter(|_| summary.written) {
                files.push(FileEntry {
                    name: name.clone(),
                    kind: FileKind::SystemAudio,
                    start_ns: summary.start_ns,
                });
            }
        }
        let input = self
            .input_log
            .take()
            .map(|log| log.take())
            .unwrap_or_default();
        if let Some(name) = &self.names.cursor {
            let file = CursorFile {
                version: 1,
                sources: &cursor,
            };
            match write_json(&self.dir, name, &file) {
                Ok(()) => files.push(FileEntry {
                    name: name.clone(),
                    kind: FileKind::Cursor,
                    start_ns: None,
                }),
                Err(error) => errors.push(format!("cursor telemetry: {error}")),
            }
        }
        if let Some(name) = &self.names.input {
            let file = InputFile {
                version: 1,
                clock: Clock::MachHostNs,
                anchor_ns: anchor.as_nanos(),
                dropped: input_dropped,
                events: &input,
            };
            match write_json(&self.dir, name, &file) {
                Ok(()) => files.push(FileEntry {
                    name: name.clone(),
                    kind: FileKind::Input,
                    start_ns: None,
                }),
                Err(error) => errors.push(format!("input log: {error}")),
            }
        }
        let manifest = Manifest {
            version: MANIFEST_VERSION,
            clock: Clock::MachHostNs,
            anchor_ns: anchor.as_nanos(),
            sources,
            files,
            audio_gaps,
            missing_permissions: std::mem::take(&mut self.missing_permissions),
        };
        if discard {
            self.remove_files();
        } else if let Err(error) = manifest.write(&self.dir) {
            errors.push(format!("manifest: {error}"));
        }
        for error in &errors {
            tracing::warn!(error, "recording finished with an error");
        }
        Finished {
            dir: self.dir,
            manifest,
            cursor,
            input,
            input_dropped,
            errors,
        }
    }

    fn remove_files(&self) {
        let names = &self.names;
        for name in names
            .videos
            .iter()
            .chain(
                [
                    &names.microphone,
                    &names.system_audio,
                    &names.cursor,
                    &names.input,
                ]
                .into_iter()
                .flatten(),
            )
            .map(String::as_str)
            .chain([MANIFEST_NAME])
        {
            match fs::remove_file(self.dir.join(name)) {
                Ok(()) => {}
                Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
                Err(error) => tracing::warn!(name, %error, "could not remove a discarded file"),
            }
        }
    }
}

/// Runs `work` on every item at once, each on its own scoped thread, falling back to the calling
/// thread when one cannot be spawned. A panicking item yields `None`.
fn in_parallel<T, R>(name: &str, items: Vec<T>, work: impl Fn(T) -> R + Sync) -> Vec<Option<R>>
where
    T: Send,
    R: Send,
{
    if items.len() <= 1 {
        return items.into_iter().map(|item| Some(work(item))).collect();
    }
    let slots = items
        .into_iter()
        .map(|item| Mutex::new(Some(item)))
        .collect::<Vec<_>>();
    let take = |slot: &Mutex<Option<T>>| slot.lock().unwrap_or_else(PoisonError::into_inner).take();
    let work = &work;
    thread::scope(|scope| {
        let handles = slots
            .iter()
            .map(|slot| {
                thread::Builder::new()
                    .name(name.into())
                    .spawn_scoped(scope, move || take(slot).map(work))
                    .ok()
            })
            .collect::<Vec<_>>();
        handles
            .into_iter()
            .zip(&slots)
            .map(|(handle, slot)| match handle {
                Some(handle) => handle.join().ok().flatten(),
                None => take(slot).map(work),
            })
            .collect()
    })
}

/// A recording in progress.
pub struct Recording {
    pub(crate) active: Option<Active>,
    pub(crate) external_sink: Option<ExternalAudioSink>,
}

impl Recording {
    /// The moment the recording started: the manifest's `anchor_ns` and sample 0 of the
    /// external microphone WAV.
    #[must_use]
    pub fn anchor(&self) -> HostTime {
        self.active
            .as_ref()
            .map_or_else(HostTime::default, |active| active.anchor)
    }

    /// The sink for [`Mic::External`](crate::Mic::External) audio. Returns it once.
    pub fn external_audio(&mut self) -> Option<ExternalAudioSink> {
        self.external_sink.take()
    }

    /// Whether the core's own click tap is running ([`Input::Tap`](crate::Input::Tap)).
    #[must_use]
    pub fn click_status(&self) -> CursorClickCaptureStatus {
        self.active
            .as_ref()
            .and_then(|active| active.clicks.as_ref())
            .map_or(CursorClickCaptureStatus::NotRequested, ClickMonitor::status)
    }

    /// Checks every source for failures reported since the last check.
    ///
    /// # Errors
    ///
    /// Returns the first output failure or system stream stop.
    pub fn health(&mut self) -> Result<(), Error> {
        if let Some(active) = &mut self.active {
            for source in &mut active.sources {
                source.events.health()?;
            }
        }
        Ok(())
    }

    /// Replaces what the recording leaves out, without restarting any stream. Each display or
    /// region source gets a display filter built as [`Recorder::start`](crate::Recorder::start)
    /// builds it; a region keeps its area. A window source records only its window and is left
    /// as it is.
    ///
    /// # Errors
    ///
    /// Returns the first source's failure. Every source is still updated.
    pub fn set_exclusions(&self, exclude: &Exclusions) -> Result<(), Error> {
        let Some(active) = &self.active else {
            return Ok(());
        };
        let mut first = None;
        for run in &active.sources {
            if let Err(error) = run.stream.exclude(exclude) {
                tracing::warn!(source = ?run.source, %error, "could not update the exclusions");
                first.get_or_insert(error);
            }
        }
        first.map_or(Ok(()), Err)
    }

    /// Ends the recording now and returns at once. Finishing (closing files, holding the final
    /// frame, writing the manifest) runs on the recording's own thread.
    #[must_use]
    pub fn stop(mut self) -> Finishing {
        self.begin(false)
    }

    /// Like [`stop`](Self::stop), then removes every file the recording wrote.
    #[must_use]
    pub fn cancel(mut self) -> Finishing {
        self.begin(true)
    }

    fn begin(&mut self, discard: bool) -> Finishing {
        let stop_at = HostTime::now();
        let Some(active) = self.active.take() else {
            return Finishing::failed(Error::FinisherLost);
        };
        active.stopping.store(true, Ordering::Release);
        if let Some(external) = &active.external {
            external.signal_stop(stop_at);
        }
        if let Some(system) = &active.system_audio {
            system.cut(stop_at);
        }
        self.external_sink = None;
        Finishing::spawn(active, stop_at, discard)
    }
}

impl Drop for Recording {
    fn drop(&mut self) {
        if self.active.is_some() {
            // Finishing is bounded; its thread ends on its own.
            drop(self.begin(false));
        }
    }
}

/// A stopped recording whose files are still being finished.
pub struct Finishing {
    state: FinishingState,
}

enum FinishingState {
    Running(JoinHandle<Finished>),
    Failed(Error),
    Taken,
}

impl Finishing {
    fn spawn(active: Active, stop_at: HostTime, discard: bool) -> Self {
        let spawned = thread::Builder::new()
            .name("capture-finish".into())
            .spawn(move || active.finish(stop_at, discard));
        match spawned {
            Ok(handle) => Self {
                state: FinishingState::Running(handle),
            },
            Err(error) => Self::failed(Error::Stop(format!(
                "could not start the finishing thread: {error}"
            ))),
        }
    }

    fn failed(error: Error) -> Self {
        Self {
            state: FinishingState::Failed(error),
        }
    }

    /// Whether [`wait`](Self::wait) would return without blocking.
    #[must_use]
    pub fn is_finished(&self) -> bool {
        match &self.state {
            FinishingState::Running(handle) => handle.is_finished(),
            FinishingState::Failed(_) | FinishingState::Taken => true,
        }
    }

    /// The result once finishing is done, without blocking. Returns `None` while it runs, and
    /// after the result was taken.
    pub fn try_wait(&mut self) -> Option<Result<Finished, Error>> {
        if !self.is_finished() || matches!(self.state, FinishingState::Taken) {
            return None;
        }
        Some(self.take())
    }

    /// Blocks until the recording's files are finished.
    ///
    /// # Errors
    ///
    /// Returns an error only when finishing itself was lost. Per-source and per-file problems are
    /// in the [`Finished`] result.
    pub fn wait(mut self) -> Result<Finished, Error> {
        self.take()
    }

    fn take(&mut self) -> Result<Finished, Error> {
        match std::mem::replace(&mut self.state, FinishingState::Taken) {
            FinishingState::Running(handle) => handle.join().map_err(|_| Error::FinisherLost),
            FinishingState::Failed(error) => Err(error),
            FinishingState::Taken => Err(Error::FinisherLost),
        }
    }
}

pub(crate) fn validate_sources(sources: &[Source]) -> Result<(), Error> {
    if sources.is_empty() {
        return Err(Error::NoSources);
    }
    if sources
        .iter()
        .enumerate()
        .any(|(index, source)| sources[..index].contains(source))
    {
        return Err(Error::DuplicateSources);
    }
    Ok(())
}

#[cfg(test)]
mod tests;
