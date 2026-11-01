use std::collections::BTreeMap;
use std::sync::Arc;
use std::sync::Mutex;
use std::sync::Once;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::mpsc::{self, Receiver, SyncSender, TrySendError};
use std::thread;
use std::time::Instant;

use color_eyre::eyre::{Result, WrapErr, eyre};
use transcribe_cpp::{Backend, Model, ModelOptions, RunOptions, TimestampKind, Transcript};

use crate::dictation::context::ContextSnapshot;
use crate::dictation::processor::ProcessingObservation;
use crate::dictation::{DictationClip, pad_for_parakeet};
use crate::history::{History, HistoryDraft, HistoryKind};
use crate::meeting;
use crate::paste::{PasteMode, Paster};
use crate::suppression::InputActivity;
use crate::transcription::WarmTranscriber;
use crate::transcription::engine::{PendingTranscript, TranscriptionEngine};
use crate::transcription::gguf_session::OfflineGgufSession;
use crate::transcription::models::{
    TranscriptionModelId, TranscriptionSelection, model_path, validate,
};

pub struct Parakeet {
    session: OfflineGgufSession,
    options: RunOptions,
    name: String,
    selection: Option<TranscriptionSelection>,
    max_audio_samples: Option<usize>,
    stream: Option<transcribe_cpp::StreamOptions>,
    _weights: procwatch::Lease,
}

/// Loaded model weights, estimated from the model file size (they live in Metal buffers).
static MODEL_WEIGHTS: procwatch::Gauge = procwatch::Gauge::new("speech.model_weights");
static TRANSCRIBE_LOGGING: Once = Once::new();
#[cfg(hex_capture)]
const TRANSCRIPTION_SAMPLES_PER_MS: usize = 16;
const MAX_PENDING_OUTPUTS: usize = 16;

pub enum WorkerEvent {
    ModelFailed(String),
    Completed {
        job_id: DictationJobId,
        target: TranscriptionTarget,
        result: Result<String, String>,
        processing: Option<ProcessingObservation>,
    },
    Stage {
        job_id: DictationJobId,
        stage: DictationJobStage,
    },
    Cancelled {
        job_id: DictationJobId,
    },
    Pasted {
        kind: PasteKind,
        result: Result<String, String>,
    },
}

#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub struct DictationJobId(u64);

impl DictationJobId {
    pub const fn value(self) -> u64 {
        self.0
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum DictationJobStage {
    Transcribing,
    Processing,
}

#[derive(Clone, Copy, Debug)]
pub enum PasteKind {
    LastTranscript,
    Picked,
    MeetingDelta,
}

enum PasteRequest {
    LastTranscript,
    Picked(String),
    MeetingDelta,
}

impl PasteRequest {
    fn kind(&self) -> PasteKind {
        match self {
            Self::LastTranscript => PasteKind::LastTranscript,
            Self::Picked(_) => PasteKind::Picked,
            Self::MeetingDelta => PasteKind::MeetingDelta,
        }
    }
}

#[derive(Clone, Copy, Debug)]
pub enum TranscriptionTarget {
    Paste,
    /// Dictate without processing: pasted as heard, skipping the Output
    /// pane's steps.
    RawPaste,
    Service,
    /// `hex dictation stop --print`: processed as a paste, then handed to
    /// the waiting command instead of pasted.
    Print,
    /// A printing stop of a raw dictation: the transcript as heard.
    RawPrint,
}

struct InferenceJob {
    job_id: DictationJobId,
    control: Arc<JobControl>,
    submitted_at: Instant,
    transcript: PendingTranscript,
    audio_ms: u64,
    prepare_ms: u128,
    target: TranscriptionTarget,
    context: ContextSnapshot,
}

struct ProcessorJob {
    job_id: DictationJobId,
    control: Arc<JobControl>,
    target: TranscriptionTarget,
    text: String,
    context: ContextSnapshot,
    timings: JobTimings,
}

/// Pipeline timings carried from inference through output for the final log.
#[derive(Clone, Copy)]
struct JobTimings {
    total_started: Instant,
    queue_ms: u128,
    audio_ms: u64,
    prepare_ms: u128,
    inference_ms: u128,
}

struct CompletedTranscript {
    text: String,
    /// Corrected local transcript before mode processing.
    raw: String,
    application: Option<String>,
    timings: JobTimings,
    processing: Option<ProcessingObservation>,
}

enum OutputJob {
    PreparePaste,
    Completed {
        job_id: DictationJobId,
        control: Arc<JobControl>,
        target: TranscriptionTarget,
        result: Box<Result<CompletedTranscript, String>>,
    },
    Cancelled {
        job_id: DictationJobId,
    },
    Paste {
        sequence: u64,
        request: PasteRequest,
    },
}

impl OutputJob {
    fn sequence(&self) -> u64 {
        match self {
            Self::PreparePaste => u64::MAX,
            Self::Completed { job_id, .. } | Self::Cancelled { job_id } => job_id.0,
            Self::Paste { sequence, .. } => *sequence,
        }
    }
}

#[derive(Default)]
struct JobControl {
    cancelled: AtomicBool,
    output_started: Mutex<bool>,
    /// A programmatic dictation's job: its owner app cancels it through the
    /// API, so Escape never does.
    owner_cancels: bool,
    /// A printing stop's reply; dropping the job unanswered tells it the
    /// dictation was cancelled or failed.
    print: Mutex<Option<crate::control::PrintReply>>,
}

impl JobControl {
    fn cancel(&self) -> bool {
        let output_started = self
            .output_started
            .lock()
            .unwrap_or_else(|error| error.into_inner());
        if *output_started {
            return false;
        }
        !self.cancelled.swap(true, Ordering::AcqRel)
    }

    /// Whether [`JobControl::cancel`] would cancel it now.
    fn cancellable(&self) -> bool {
        let output_started = self
            .output_started
            .lock()
            .unwrap_or_else(|error| error.into_inner());
        !*output_started && !self.is_cancelled()
    }

    fn is_cancelled(&self) -> bool {
        self.cancelled.load(Ordering::Acquire)
    }

    fn answer_print(&self, transcript: crate::control::PrintedTranscript) {
        if let Some(reply) = self
            .print
            .lock()
            .unwrap_or_else(|error| error.into_inner())
            .take()
        {
            reply.send(transcript);
        }
    }

    fn begin_output(&self) -> bool {
        let mut output_started = self
            .output_started
            .lock()
            .unwrap_or_else(|error| error.into_inner());
        if self.is_cancelled() {
            return false;
        }
        *output_started = true;
        true
    }
}

#[derive(Default)]
struct OrderedOutputs {
    waiting: BTreeMap<u64, OutputJob>,
    next_sequence: u64,
}

impl OrderedOutputs {
    fn push(&mut self, job: OutputJob) -> Vec<OutputJob> {
        if job.sequence() < self.next_sequence {
            return Vec::new();
        }
        self.waiting.insert(job.sequence(), job);
        let mut ready = Vec::new();
        while let Some(job) = self.waiting.remove(&self.next_sequence) {
            ready.push(job);
            self.next_sequence += 1;
        }
        ready
    }
}

pub struct DictationWorker {
    engine: Option<Arc<TranscriptionEngine>>,
    inference_jobs: Option<SyncSender<InferenceJob>>,
    output_jobs: Option<SyncSender<OutputJob>>,
    events: Receiver<WorkerEvent>,
    event_sender: mpsc::Sender<WorkerEvent>,
    state: Arc<Mutex<WorkerState>>,
    inference_worker: Option<thread::JoinHandle<()>>,
    processor_workers: Vec<thread::JoinHandle<()>>,
    output_worker: Option<thread::JoinHandle<()>>,
}

struct WorkerState {
    next_sequence: u64,
    jobs: BTreeMap<DictationJobId, Arc<JobControl>>,
    pending_pastes: usize,
}

impl WorkerState {
    fn next_output_sequence(&self) -> Result<u64, &'static str> {
        // Count accepted work even after it leaves a channel for ordered buffering.
        if self.jobs.len() + self.pending_pastes >= MAX_PENDING_OUTPUTS {
            return Err("dictation queue is full");
        }
        Ok(self.next_sequence)
    }

    fn cancel_latest(&self) -> Option<DictationJobId> {
        self.jobs
            .iter()
            .rev()
            .filter(|(_, control)| !control.owner_cancels)
            .find_map(|(&job_id, control)| control.cancel().then_some(job_id))
    }

    fn has_escape_cancellable_job(&self) -> bool {
        self.jobs
            .values()
            .any(|control| !control.owner_cancels && control.cancellable())
    }
}

impl DictationWorker {
    pub fn start(
        activity: InputActivity,
        transformations: Arc<crate::personal_commands::TransformationClient>,
        history: Option<History>,
    ) -> Self {
        const PROCESSOR_WORKERS: usize = 2;
        // Admission caps accepted jobs, so a submitted job always has a slot.
        let (inference_jobs, inference_receiver) =
            mpsc::sync_channel::<InferenceJob>(MAX_PENDING_OUTPUTS);
        let (processor_jobs, processor_receiver) = mpsc::sync_channel::<ProcessorJob>(4);
        let (output_jobs, output_receiver) = mpsc::sync_channel::<OutputJob>(8);
        let (event_sender, events) = mpsc::channel();
        let state = Arc::new(Mutex::new(WorkerState {
            next_sequence: 0,
            jobs: BTreeMap::new(),
            pending_pastes: 0,
        }));

        let engine = match TranscriptionEngine::start(WarmTranscriber::default()) {
            Ok(engine) => {
                let events = event_sender.clone();
                let _ = engine.run(move |model| {
                    prioritize_inference_thread();
                    match WarmTranscriber::load() {
                        Ok(loaded) => {
                            tracing::info!("transcription model loaded");
                            *model = loaded;
                        }
                        Err(error) => {
                            let _ = events.send(WorkerEvent::ModelFailed(error.to_string()));
                        }
                    }
                });
                let engine = Arc::new(engine);
                engine.share();
                Some(engine)
            }
            Err(error) => {
                let _ = event_sender.send(WorkerEvent::ModelFailed(error.to_string()));
                None
            }
        };

        let output_worker = thread::spawn({
            let state = state.clone();
            let events = event_sender.clone();
            move || run_output_worker(output_receiver, activity, history, &state, &events)
        });

        let processor_receiver = Arc::new(Mutex::new(processor_receiver));
        let processor_workers = (0..PROCESSOR_WORKERS)
            .map(|_| {
                let jobs = processor_receiver.clone();
                let output = output_jobs.clone();
                let events = event_sender.clone();
                let transformations = transformations.clone();
                thread::spawn(move || {
                    run_processor_worker(&jobs, &output, &events, &transformations)
                })
            })
            .collect();

        let inference_worker = thread::spawn({
            let output = output_jobs.clone();
            let events = event_sender.clone();
            move || run_inference_worker(inference_receiver, &processor_jobs, &output, &events)
        });

        Self {
            engine,
            inference_jobs: Some(inference_jobs),
            output_jobs: Some(output_jobs),
            events,
            event_sender,
            state,
            inference_worker: Some(inference_worker),
            processor_workers,
            output_worker: Some(output_worker),
        }
    }

    /// Hands the clip to the engine at once, so its live stream finishes
    /// before any later capture begins one; the job then waits in order.
    pub fn transcribe(
        &self,
        clip: DictationClip,
        target: TranscriptionTarget,
        context: ContextSnapshot,
    ) -> Result<DictationJobId, &'static str> {
        self.submit(clip, target, context, None)
    }

    /// Like a paste, but the transcript (processed unless `raw`) goes to
    /// `print`.
    pub fn transcribe_printed(
        &self,
        clip: DictationClip,
        print: crate::control::PrintReply,
        raw: bool,
        context: ContextSnapshot,
    ) -> Result<DictationJobId, &'static str> {
        let target = if raw {
            TranscriptionTarget::RawPrint
        } else {
            TranscriptionTarget::Print
        };
        self.submit(clip, target, context, Some(print))
    }

    fn submit(
        &self,
        mut clip: DictationClip,
        target: TranscriptionTarget,
        context: ContextSnapshot,
        print: Option<crate::control::PrintReply>,
    ) -> Result<DictationJobId, &'static str> {
        let mut state = self.state.lock().unwrap_or_else(|error| error.into_inner());
        let job_id = DictationJobId(state.next_output_sequence()?);
        let (Some(engine), Some(jobs)) = (&self.engine, &self.inference_jobs) else {
            return Err("dictation worker is unavailable");
        };
        let control = Arc::new(JobControl {
            owner_cancels: matches!(target, TranscriptionTarget::Service),
            print: Mutex::new(print),
            ..JobControl::default()
        });
        let (_, selection) = crate::app_settings::transcription_selection();
        let submitted_at = Instant::now();
        let audio_ms = clip.duration_ms();
        let stream = clip.take_stream();
        let _capture = clip.take_capture_marker();
        let samples = clip.into_transcription_samples();
        crate::dictation::diagnostics::persist(&samples);
        let prepare_ms = submitted_at.elapsed().as_millis();
        let offline = {
            let control = control.clone();
            Box::new(move |model: &mut WarmTranscriber, samples: Vec<f32>| {
                if control.is_cancelled() {
                    return Err("dictation was cancelled".to_string());
                }
                let _busy = DictationInference::begin();
                let transcriber = model
                    .activate(&selection)
                    .map_err(|error| error.to_string())?;
                transcriber
                    .transcribe(samples)
                    .map_err(|error| error.to_string())
            })
        };
        let transcript = engine
            .finish_with(stream, samples, offline)
            .map_err(|_| "dictation worker is unavailable")?;
        jobs.try_send(InferenceJob {
            job_id,
            control: control.clone(),
            submitted_at,
            transcript,
            audio_ms,
            prepare_ms,
            target,
            context,
        })
        .map(|()| {
            state.next_sequence += 1;
            state.jobs.insert(job_id, control);
            job_id
        })
        .map_err(queue_error)
    }

    /// The engine that streams hotkey captures while they are recorded.
    pub fn engine(&self) -> Option<Arc<TranscriptionEngine>> {
        self.engine.clone()
    }

    pub fn prepare_paste(&self) {
        let state = self.state.lock().unwrap_or_else(|error| error.into_inner());
        if !state.jobs.is_empty() || state.pending_pastes > 0 {
            return;
        }
        drop(state);
        let Some(output_jobs) = &self.output_jobs else {
            return;
        };
        let _ = output_jobs.try_send(OutputJob::PreparePaste);
    }

    /// Switches the warm model after the work already handed to the engine.
    pub fn reload(&self, selection: TranscriptionSelection) -> Result<(), &'static str> {
        let events = self.event_sender.clone();
        self.engine
            .as_ref()
            .ok_or("dictation worker is unavailable")?
            .run(move |model| {
                let _busy = DictationInference::begin();
                if let Err(error) = model.activate(&selection) {
                    let _ = events.send(WorkerEvent::ModelFailed(error.to_string()));
                }
            })
            .map_err(|_| "dictation worker is unavailable")
    }

    pub fn paste_last(&self) -> Result<(), &'static str> {
        self.submit_paste(PasteRequest::LastTranscript)
    }

    pub fn paste_picked(&self, text: String) -> Result<(), &'static str> {
        self.submit_paste(PasteRequest::Picked(text))
    }

    pub fn paste_meeting(&self) -> Result<(), &'static str> {
        self.submit_paste(PasteRequest::MeetingDelta)
    }

    fn submit_paste(&self, request: PasteRequest) -> Result<(), &'static str> {
        let mut state = self.state.lock().unwrap_or_else(|error| error.into_inner());
        let sequence = state.next_output_sequence()?;
        self.output_jobs
            .as_ref()
            .ok_or("dictation worker is unavailable")?
            .try_send(OutputJob::Paste { sequence, request })
            .map(|()| {
                state.next_sequence += 1;
                state.pending_pastes += 1;
            })
            .map_err(queue_error)
    }

    pub fn try_recv(&self) -> Option<WorkerEvent> {
        self.events.try_recv().ok()
    }

    pub fn is_busy(&self) -> bool {
        let state = self.state.lock().unwrap_or_else(|error| error.into_inner());
        !state.jobs.is_empty() || state.pending_pastes > 0
    }

    pub fn pending_count(&self) -> usize {
        self.state
            .lock()
            .unwrap_or_else(|error| error.into_inner())
            .jobs
            .len()
    }

    /// Whether Escape has a queued job to cancel; programmatic dictations'
    /// jobs don't count.
    pub fn has_escape_cancellable_job(&self) -> bool {
        self.state
            .lock()
            .unwrap_or_else(|error| error.into_inner())
            .has_escape_cancellable_job()
    }

    pub fn cancel_latest(&self) -> Option<DictationJobId> {
        let state = self.state.lock().unwrap_or_else(|error| error.into_inner());
        let job_id = state.cancel_latest()?;
        drop(state);
        let _ = self
            .output_jobs
            .as_ref()?
            .try_send(OutputJob::Cancelled { job_id });
        Some(job_id)
    }

    fn shutdown(&mut self) {
        // Accepted jobs still finish: the inference worker drains its queue.
        self.inference_jobs.take();
        join_worker(self.inference_worker.take(), "dictation inference");
        for worker in self.processor_workers.drain(..) {
            join_worker(Some(worker), "dictation processing");
        }
        self.output_jobs.take();
        join_worker(self.output_worker.take(), "dictation output");
    }
}

impl Drop for DictationWorker {
    fn drop(&mut self) {
        self.shutdown();
    }
}

fn run_output_worker(
    jobs: Receiver<OutputJob>,
    activity: InputActivity,
    history: Option<History>,
    state: &Mutex<WorkerState>,
    events: &mpsc::Sender<WorkerEvent>,
) {
    let mut paster = Paster::new(activity);
    let mut last_transcript = None;
    let mut ordered = OrderedOutputs::default();
    while let Ok(job) = jobs.recv() {
        if matches!(job, OutputJob::PreparePaste) {
            paster.prepare();
            continue;
        }
        for job in ordered.push(job) {
            let event = finish_output(
                job,
                &mut |text, mode, commit| paster.paste(text, mode, commit),
                &mut last_transcript,
                history.as_ref(),
            );
            let mut state = state.lock().unwrap_or_else(|error| error.into_inner());
            match &event {
                WorkerEvent::Completed { job_id, .. } | WorkerEvent::Cancelled { job_id } => {
                    state.jobs.remove(job_id);
                }
                WorkerEvent::Pasted { .. } => {
                    state.pending_pastes = state.pending_pastes.saturating_sub(1);
                }
                WorkerEvent::ModelFailed(_) | WorkerEvent::Stage { .. } => {}
            }
            drop(state);
            if events.send(event).is_err() {
                return;
            }
        }
    }
}

fn run_processor_worker(
    jobs: &Mutex<Receiver<ProcessorJob>>,
    output: &SyncSender<OutputJob>,
    events: &mpsc::Sender<WorkerEvent>,
    transformations: &crate::personal_commands::TransformationClient,
) {
    loop {
        let job = jobs
            .lock()
            .unwrap_or_else(|error| error.into_inner())
            .recv();
        let Ok(job) = job else { break };
        if job.control.is_cancelled() {
            let _ = output.send(OutputJob::Cancelled { job_id: job.job_id });
            continue;
        }
        let mut processed =
            process_job_text(&job, crate::dictation::config::dictation_profiles, || {
                let _ = events.send(WorkerEvent::Stage {
                    job_id: job.job_id,
                    stage: DictationJobStage::Processing,
                });
            });
        processed.apply_transformations(transformations, &job.context, &job.control.cancelled);
        if job.control.is_cancelled() {
            let _ = output.send(OutputJob::Cancelled { job_id: job.job_id });
            continue;
        }
        if let Some(observation) = &processed.observation
            && let Some(error) = &observation.fallback
        {
            tracing::warn!(
                profile = observation.profile,
                %error,
                "dictation processing fell back to the previous pipeline output"
            );
        }
        if output
            .send(OutputJob::Completed {
                job_id: job.job_id,
                control: job.control,
                target: job.target,
                result: Box::new(Ok(CompletedTranscript {
                    text: processed.text,
                    raw: job.text,
                    application: job.context.application,
                    timings: job.timings,
                    processing: processed.observation,
                })),
            })
            .is_err()
        {
            break;
        }
    }
}

/// Select processing when the worker reaches the job; the stage precedes processing.
fn process_job_text(
    job: &ProcessorJob,
    profiles: impl FnOnce() -> crate::dictation::processor::Profiles,
    processing: impl FnOnce(),
) -> crate::dictation::processor::Processed {
    let profiles = profiles();
    if profiles.processes(&job.context) {
        processing();
    }
    profiles.process_cancellable(&job.text, &job.context, &job.control.cancelled)
}

/// The longest audio the capture path hands the model at once; longer
/// captures are transcribed in consecutive chunks with their times offset.
#[cfg(hex_capture)]
const WORD_CHUNK_SAMPLES: usize = 30 * 16_000;
/// How long a capture chunk waits for dictation to go idle before it is
/// queued anyway; the engine still never runs it inside a live stream.
#[cfg(hex_capture)]
const WORD_DICTATION_WAIT: std::time::Duration = std::time::Duration::from_secs(20);

/// Transcribes a capture's 16 kHz audio with word timestamps on the engine's
/// warm model (the `capture` experiment). Before each chunk it waits while
/// dictation is busy (from the shortcut press through inference); each chunk
/// is then a foreground request: after the dictation already sent, before
/// meeting chunks, and never inside a dictation live stream, which finishes
/// first. No second copy of the model is loaded, and only a weak reference
/// to the engine is kept between chunks. Quitting stops it between chunks.
/// Every chunk runs on `selection`.
#[cfg(hex_capture)]
pub fn transcribe_with_words(
    engine: &std::sync::Weak<TranscriptionEngine>,
    selection: &crate::transcription::models::TranscriptionSelection,
    samples: Vec<f32>,
) -> Result<crate::transcription::Transcript> {
    if samples.is_empty() {
        return Ok(crate::transcription::Transcript::default());
    }
    transcribe_in_chunks(samples, WORD_CHUNK_SAMPLES, |chunk| {
        wait_for_dictation_idle()?;
        let selection = selection.clone();
        let (send, transcript) = mpsc::sync_channel(1);
        let offline: crate::transcription::engine::Offline = Box::new(move |model, samples| {
            let _busy = DictationInference::begin();
            let transcriber = model
                .activate(&selection)
                .map_err(|error| format!("{error:#}"))?;
            let result = transcriber
                .transcribe_segments(samples)
                .map_err(|error| format!("{error:#}"))?;
            let text = result.text.clone();
            let _ = send.send(result);
            Ok(text)
        });
        let pending = engine
            .upgrade()
            .ok_or_else(|| eyre!("the transcription engine stopped"))?
            .foreground(chunk, offline)?;
        pending.wait().map_err(|error| eyre!(error))?;
        transcript
            .try_recv()
            .map_err(|_| eyre!("the transcription returned no words"))
    })
}

#[cfg(hex_capture)]
fn wait_for_dictation_idle() -> Result<()> {
    let started = Instant::now();
    while dictation_busy()
        && !crate::desktop::process_exit::quitting()
        && started.elapsed() < WORD_DICTATION_WAIT
    {
        thread::sleep(std::time::Duration::from_millis(50));
    }
    if crate::desktop::process_exit::quitting() {
        return Err(eyre!("Hex is quitting"));
    }
    Ok(())
}

/// Transcribes `samples` in chunks of at most `chunk_samples`, offsetting
/// each chunk's segment and word times by where it starts.
#[cfg(hex_capture)]
fn transcribe_in_chunks(
    samples: Vec<f32>,
    chunk_samples: usize,
    mut transcribe: impl FnMut(Vec<f32>) -> Result<crate::transcription::Transcript>,
) -> Result<crate::transcription::Transcript> {
    use crate::transcription::{Transcript, TranscriptSegment};
    let shift = |spans: Vec<TranscriptSegment>, offset_ms: i64| {
        spans.into_iter().filter_map(move |span| {
            let text = span.text.trim();
            let start_ms = span.start_ms.max(0);
            (!text.is_empty()).then(|| TranscriptSegment {
                start_ms: start_ms + offset_ms,
                end_ms: span.end_ms.max(start_ms) + offset_ms,
                text: text.to_string(),
            })
        })
    };
    let mut merged = Transcript::default();
    let mut texts = Vec::new();
    for (index, chunk) in samples.chunks(chunk_samples.max(1)).enumerate() {
        let offset_ms = (index * chunk_samples / TRANSCRIPTION_SAMPLES_PER_MS) as i64;
        let result = transcribe(chunk.to_vec())?;
        let text = result.text.trim();
        if !text.is_empty() {
            texts.push(text.to_string());
        }
        merged.segments.extend(shift(result.segments, offset_ms));
        merged.words.extend(shift(result.words, offset_ms));
    }
    merged.text = texts.join(" ");
    Ok(merged)
}

/// Waits for each accepted job's transcript in submission order, then hands
/// it to processing. The engine thread runs the inference itself.
fn run_inference_worker(
    jobs: Receiver<InferenceJob>,
    processor_jobs: &SyncSender<ProcessorJob>,
    output: &SyncSender<OutputJob>,
    events: &mpsc::Sender<WorkerEvent>,
) {
    while let Ok(job) = jobs.recv() {
        if job.control.is_cancelled() {
            let _ = output.send(OutputJob::Cancelled { job_id: job.job_id });
            continue;
        }
        let _busy = DictationInference::begin();
        let _ = events.send(WorkerEvent::Stage {
            job_id: job.job_id,
            stage: DictationJobStage::Transcribing,
        });
        let total_started = job.submitted_at;
        let queue_ms = total_started.elapsed().as_millis();
        let inference_started = Instant::now();
        let audio_ms = job.audio_ms;
        let result = job.transcript.wait().map(|transcribed| {
            tracing::info!(
                source = transcribed.source.label(),
                finish_ms = transcribed.finish_ms,
                audio_ms,
                "transcribed dictation"
            );
            let text = transcribed.text;
            let corrected = if matches!(job.target, TranscriptionTarget::Service) {
                text.clone()
            } else {
                text.trim().to_string()
            };
            tracing::debug!(
                raw_transcript = text,
                corrected_transcript = corrected,
                "prepared local transcript"
            );
            corrected
        });
        let timings = JobTimings {
            total_started,
            queue_ms,
            audio_ms,
            prepare_ms: job.prepare_ms,
            inference_ms: inference_started.elapsed().as_millis(),
        };
        if job.control.is_cancelled() {
            let _ = output.send(OutputJob::Cancelled { job_id: job.job_id });
            continue;
        }
        let target = job.target;
        match result {
            Ok(text)
                if matches!(
                    target,
                    TranscriptionTarget::Paste | TranscriptionTarget::Print
                ) && !text.trim().is_empty() =>
            {
                if processor_jobs
                    .send(ProcessorJob {
                        job_id: job.job_id,
                        control: job.control,
                        target,
                        text,
                        context: job.context,
                        timings,
                    })
                    .is_err()
                {
                    break;
                }
            }
            result => {
                let application = job.context.application;
                let result = result.map(|text| CompletedTranscript {
                    raw: text.clone(),
                    text,
                    application,
                    timings,
                    processing: None,
                });
                if output
                    .send(OutputJob::Completed {
                        job_id: job.job_id,
                        control: job.control,
                        target,
                        result: Box::new(result),
                    })
                    .is_err()
                {
                    break;
                }
            }
        }
    }
}

fn join_worker(worker: Option<thread::JoinHandle<()>>, name: &str) {
    if let Some(worker) = worker
        && worker.join().is_err()
    {
        tracing::error!(worker = name, "worker panicked during shutdown");
    }
}

#[cfg(target_os = "macos")]
pub(crate) fn prioritize_inference_thread() {
    // ggml worker pthreads start at default QoS instead of inheriting this
    // class; GGUF sessions use one decoder thread so decoding stays here.
    const QOS_CLASS_USER_INTERACTIVE: u32 = 0x21;
    unsafe extern "C" {
        fn pthread_set_qos_class_self_np(qos_class: u32, relative_priority: i32) -> i32;
    }
    // SAFETY: This configures only the calling worker thread using the public
    // macOS pthread QoS API. Relative priority zero is valid for this class.
    let status = unsafe { pthread_set_qos_class_self_np(QOS_CLASS_USER_INTERACTIVE, 0) };
    if status == 0 {
        tracing::info!("dictation inference worker uses user-interactive QoS");
    } else {
        tracing::warn!(status, "could not raise dictation inference worker QoS");
    }
}

#[cfg(not(target_os = "macos"))]
pub(crate) fn prioritize_inference_thread() {}

static DICTATION_INFERENCE: AtomicUsize = AtomicUsize::new(0);

struct DictationInference;

impl DictationInference {
    fn begin() -> Self {
        DICTATION_INFERENCE.fetch_add(1, Ordering::AcqRel);
        Self
    }
}

impl Drop for DictationInference {
    fn drop(&mut self) {
        DICTATION_INFERENCE.fetch_sub(1, Ordering::AcqRel);
    }
}

/// Makes [`dictation_busy`] hold until dropped, as a dictation would.
#[cfg(test)]
pub(crate) struct DictationBusyForTest {
    _busy: DictationInference,
}

#[cfg(test)]
pub(crate) fn dictation_busy_for_test() -> DictationBusyForTest {
    DictationBusyForTest {
        _busy: DictationInference::begin(),
    }
}

/// Whether dictation is capturing or running inference. Meeting
/// transcription starts no chunk while this holds. A capture counts from its
/// press: a chunk already running then finishes during the hold threshold
/// and release tail, before the clip needs the model.
pub(crate) fn dictation_busy() -> bool {
    DICTATION_INFERENCE.load(Ordering::Acquire) > 0
        || crate::audio::recording_environment::capture_active()
        || crate::audio::recording_environment::intentional_recording_active()
}

/// Runs `work` at utility QoS on the dictation inference thread, then
/// restores its user-interactive class.
pub(crate) fn at_background_priority<R>(work: impl FnOnce() -> R) -> R {
    const QOS_CLASS_USER_INTERACTIVE: u32 = 0x21;
    const QOS_CLASS_UTILITY: u32 = 0x11;
    unsafe extern "C" {
        fn pthread_set_qos_class_self_np(qos_class: u32, relative_priority: i32) -> i32;
    }
    // SAFETY: These configure only the calling thread using the public macOS
    // pthread QoS API. Relative priority zero is valid for both classes.
    unsafe { pthread_set_qos_class_self_np(QOS_CLASS_UTILITY, 0) };
    let result = work();
    unsafe { pthread_set_qos_class_self_np(QOS_CLASS_USER_INTERACTIVE, 0) };
    result
}

/// Runs the calling thread at utility QoS for background transcription.
pub(crate) fn deprioritize_background_thread() {
    const QOS_CLASS_UTILITY: u32 = 0x11;
    unsafe extern "C" {
        fn pthread_set_qos_class_self_np(qos_class: u32, relative_priority: i32) -> i32;
    }
    // SAFETY: This configures only the calling thread using the public macOS
    // pthread QoS API. Relative priority zero is valid for this class.
    let status = unsafe { pthread_set_qos_class_self_np(QOS_CLASS_UTILITY, 0) };
    if status != 0 {
        tracing::warn!(status, "could not lower background transcription QoS");
    }
}

fn queue_error(error: TrySendError<impl Sized>) -> &'static str {
    match error {
        TrySendError::Full(_) => "dictation queue is full",
        TrySendError::Disconnected(_) => "dictation worker is unavailable",
    }
}

fn finish_output(
    job: OutputJob,
    paste: &mut impl FnMut(&str, PasteMode, &dyn Fn() -> bool) -> Result<()>,
    last_transcript: &mut Option<String>,
    history: Option<&History>,
) -> WorkerEvent {
    match job {
        OutputJob::PreparePaste => unreachable!("paste preparation bypasses ordered output"),
        OutputJob::Completed {
            job_id,
            control,
            target,
            result,
        } if !control.is_cancelled() => {
            let result = *result;
            let processing = result
                .as_ref()
                .ok()
                .and_then(|result| result.processing.clone());
            let result = result.and_then(|completed| {
                if control.is_cancelled() {
                    return Err("dictation was cancelled".into());
                }
                let paste_started = Instant::now();
                if !completed.text.trim().is_empty() {
                    let commit = || control.begin_output();
                    match target {
                        TranscriptionTarget::Paste | TranscriptionTarget::RawPaste => {
                            // Labs › Copy dictation only, read as the output
                            // commits: dictations go to the clipboard
                            // unpasted; Paste last dictation still pastes.
                            let mode = if crate::labs::copy_only() {
                                PasteMode::CopyOnly
                            } else {
                                PasteMode::Continue
                            };
                            paste(&completed.text, mode, &commit)
                        }
                        TranscriptionTarget::Service
                        | TranscriptionTarget::Print
                        | TranscriptionTarget::RawPrint => commit()
                            .then_some(())
                            .ok_or_else(|| eyre!("dictation was cancelled")),
                    }
                    .map_err(|error| error.to_string())?;
                    if matches!(
                        target,
                        TranscriptionTarget::Paste | TranscriptionTarget::RawPaste
                    ) {
                        *last_transcript = Some(completed.text.clone());
                    }
                    if let Some(history) = history {
                        record_history(history, target, &completed);
                    }
                }
                tracing::info!(
                    audio_ms = completed.timings.audio_ms,
                    queue_ms = completed.timings.queue_ms,
                    prepare_ms = completed.timings.prepare_ms,
                    inference_ms = completed.timings.inference_ms,
                    paste_ms = paste_started.elapsed().as_millis(),
                    total_ms = completed.timings.total_started.elapsed().as_millis(),
                    "dictation pipeline completed"
                );
                Ok(completed.text)
            });
            if matches!(
                target,
                TranscriptionTarget::Print | TranscriptionTarget::RawPrint
            ) && !control.is_cancelled()
            {
                control.answer_print(match &result {
                    Ok(text) if text.trim().is_empty() => {
                        crate::control::PrintedTranscript::NoSpeech
                    }
                    Ok(text) => crate::control::PrintedTranscript::Text(text.clone()),
                    Err(error) => crate::control::PrintedTranscript::Failed(error.clone()),
                });
            }
            if control.is_cancelled() {
                WorkerEvent::Cancelled { job_id }
            } else {
                WorkerEvent::Completed {
                    job_id,
                    target,
                    result,
                    processing,
                }
            }
        }
        OutputJob::Completed { job_id, .. } | OutputJob::Cancelled { job_id } => {
            WorkerEvent::Cancelled { job_id }
        }
        OutputJob::Paste { request, .. } => {
            let kind = request.kind();
            let result = match request {
                PasteRequest::LastTranscript => last_transcript
                    .as_deref()
                    .ok_or_else(|| "no previous transcript is available".to_string())
                    .and_then(|text| {
                        paste(text, PasteMode::Continue, &|| true)
                            .map_err(|error| error.to_string())?;
                        Ok(text.to_string())
                    }),
                PasteRequest::Picked(text) => paste(&text, PasteMode::Continue, &|| true)
                    .map_err(|error| error.to_string())
                    .map(|()| text),
                PasteRequest::MeetingDelta => {
                    paste_meeting_delta(paste).map_err(|error| error.to_string())
                }
            };
            WorkerEvent::Pasted { kind, result }
        }
    }
}

/// Record one successfully pasted result. History failures must never fail
/// the paste that already happened.
fn record_history(history: &History, target: TranscriptionTarget, completed: &CompletedTranscript) {
    let kind = match target {
        TranscriptionTarget::Paste => HistoryKind::Dictation,
        TranscriptionTarget::RawPaste => HistoryKind::Raw,
        TranscriptionTarget::Print | TranscriptionTarget::RawPrint => HistoryKind::Printed,
        TranscriptionTarget::Service => return,
    };
    let draft = HistoryDraft {
        kind,
        raw_text: completed.raw.clone(),
        final_text: completed.text.clone(),
        application: completed.application.clone(),
        processing: completed.processing.clone(),
        audio_ms: completed.timings.audio_ms,
        inference_ms: completed.timings.inference_ms as u64,
        total_ms: completed.timings.total_started.elapsed().as_millis() as u64,
    };
    if let Err(error) = history.record(draft) {
        tracing::warn!(%error, "could not record dictation history");
    }
}

fn paste_meeting_delta(
    paste: &mut impl FnMut(&str, PasteMode, &dyn Fn() -> bool) -> Result<()>,
) -> Result<String> {
    meeting::consume_active_or_latest_delta(|text| paste(text, PasteMode::Standalone, &|| true))
}

pub(crate) fn default_model_path() -> Result<std::path::PathBuf> {
    model_path(crate::transcription::models::definition(
        crate::transcription::models::TranscriptionModelId::default(),
    ))
}

impl Parakeet {
    #[cfg(test)]
    pub fn load() -> Result<Self> {
        let (_, selection) = crate::app_settings::transcription_selection();
        Self::load_selection(&selection)
    }

    pub fn load_selection(selection: &TranscriptionSelection) -> Result<Self> {
        let definition = validate(selection)?;
        if !crate::transcription::models::is_installed(definition, &selection.language) {
            return Err(eyre!("{} is not installed", definition.name));
        }
        let path = model_path(definition)?;
        let mut parakeet = Self::load_from(&path, true, Some(selection))?;
        parakeet.prewarm()?;
        let stream_run = parakeet.stream_run_options();
        let architecture = match definition.runtime {
            crate::transcription::models::ModelRuntime::Gguf(artifact) => artifact.architecture,
            crate::transcription::models::ModelRuntime::AppleSpeech => "",
        };
        parakeet.stream = parakeet.session.probe_stream(&stream_run, architecture);
        Ok(parakeet)
    }

    /// Streams decode like offline runs, at segment timestamps: only the text
    /// of a stream is used.
    fn stream_run_options(&self) -> RunOptions {
        RunOptions {
            timestamps: match self.options.timestamps {
                TimestampKind::Word | TimestampKind::Token => TimestampKind::Segment,
                timestamps => timestamps,
            },
            ..self.options.clone()
        }
    }

    pub(crate) fn stream_options(&self) -> Option<&transcribe_cpp::StreamOptions> {
        self.stream.as_ref()
    }

    pub(crate) fn with_stream<R>(
        &mut self,
        stream_options: &transcribe_cpp::StreamOptions,
        cancel: &transcribe_cpp::CancelToken,
        drive: impl FnOnce(&mut transcribe_cpp::Stream<'_>) -> R,
    ) -> transcribe_cpp::Result<R> {
        let options = self.stream_run_options();
        self.session
            .with_stream(&options, stream_options, cancel, drive)
    }

    pub(crate) fn load_for_benchmark(model_path: &std::path::Path) -> Result<Self> {
        Self::load_from(model_path, false, None)
    }

    fn load_from(
        model_path: &std::path::Path,
        diagnostics: bool,
        selection: Option<&TranscriptionSelection>,
    ) -> Result<Self> {
        if diagnostics {
            TRANSCRIBE_LOGGING.call_once(transcribe_cpp::init_logging);
        } else {
            transcribe_cpp::disable_logging();
        }
        let model = Model::load_with(
            model_path,
            &ModelOptions {
                backend: Backend::Metal,
                ..Default::default()
            },
        )
        .wrap_err_with(|| {
            format!(
                "could not load transcription model from {}",
                model_path.display()
            )
        })?;
        let device = model.device()?;
        if device.kind != "metal" {
            return Err(eyre!(
                "transcription model selected {} ({}) instead of Metal",
                device.kind,
                device.name
            ));
        }
        let variant = model.variant();
        let architecture = model.arch();
        let name = format!("transcribe-cpp-{}-{variant}", device.kind);
        let definition = selection.map(validate).transpose()?;
        tracing::info!(
            backend = device.kind,
            device = device.description,
            variant,
            "loaded transcription model"
        );
        let capabilities = model.capabilities()?;
        let mut options = RunOptions {
            timestamps: match capabilities.max_timestamp_kind {
                TimestampKind::Word | TimestampKind::Token => TimestampKind::Word,
                TimestampKind::Auto | TimestampKind::Segment => TimestampKind::Segment,
                TimestampKind::None => TimestampKind::None,
            },
            ..Default::default()
        };
        if let (Some(selection), Some(definition)) = (selection, definition) {
            let runtime_language = crate::transcription::gguf_session::validate_gguf_artifact(
                model_path,
                definition,
                &architecture,
                &variant,
                selection,
                capabilities.supports_language_detect,
            )?;
            if let Some(runtime_language) = &runtime_language
                && !capabilities.languages.is_empty()
                && !capabilities
                    .languages
                    .iter()
                    .any(|language| language == runtime_language)
            {
                return Err(eyre!(
                    "{} does not advertise support for {}",
                    definition.name,
                    crate::transcription::models::language_name(&selection.language)
                ));
            }
            options.language = runtime_language;
        }
        let session = OfflineGgufSession::new(model)?;
        let weights_bytes = std::fs::metadata(model_path).map_or(0, |meta| meta.len() as usize);
        Ok(Self {
            session,
            options,
            name,
            selection: selection.cloned(),
            max_audio_samples: crate::transcription::models::max_audio_chunk_samples(
                &architecture,
                capabilities.max_audio_ms,
            ),
            stream: None,
            _weights: MODEL_WEIGHTS.track(weights_bytes),
        })
    }

    /// Runs a short silent inference. ggml keeps the Metal weights resident
    /// for only three minutes after a run; then macOS may compress or swap
    /// them out, and the next run pays to bring them back. A prewarm pays that
    /// before the audio arrives, and is skipped right after another run.
    pub(crate) fn prewarm(&mut self) -> Result<()> {
        if self.session.ran_recently() {
            return Ok(());
        }
        let started = Instant::now();
        let mut silence = Vec::new();
        pad_for_parakeet(&mut silence);
        self.transcribe(&silence)
            .wrap_err("could not prewarm transcription model")?;
        tracing::info!(
            prewarm_ms = started.elapsed().as_millis(),
            "prewarmed transcription model"
        );
        Ok(())
    }

    pub(crate) fn name(&self) -> &str {
        &self.name
    }

    pub(crate) fn matches_selection(&self, selection: &TranscriptionSelection) -> bool {
        self.selection.as_ref() == Some(selection)
    }

    pub(crate) fn selection(&self) -> Option<&TranscriptionSelection> {
        self.selection.as_ref()
    }

    pub(crate) fn model_id(&self) -> Option<TranscriptionModelId> {
        self.selection.as_ref().map(|selection| selection.model)
    }

    /// Low-level GGUF entry point over prepared audio. `Transcriber` owns the
    /// whole-clip padding; chunks only apply the minimum duration, never
    /// another model-specific trailing context.
    pub fn transcribe(&mut self, samples: &[f32]) -> Result<String> {
        self.session
            .transcribe_chunked(samples, &self.options, self.max_audio_samples)
    }

    pub fn transcribe_segments(&mut self, samples: &[f32]) -> Result<Transcript> {
        self.session
            .run(samples, &self.options)
            .wrap_err("transcription failed")
    }
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use hound::WavReader;

    use super::*;

    use crate::dictation::resample_for_parakeet;
    use crate::meeting::{MeetingSource, TranscriptEntry, format_meeting_delta};

    fn processing_job(target: TranscriptionTarget) -> ProcessorJob {
        ProcessorJob {
            job_id: DictationJobId(0),
            control: Arc::new(JobControl::default()),
            target,
            text: "raw instruction".into(),
            context: ContextSnapshot::default(),
            timings: JobTimings {
                total_started: Instant::now(),
                queue_ms: 0,
                audio_ms: 1000,
                prepare_ms: 0,
                inference_ms: 0,
            },
        }
    }

    #[test]
    fn ordinary_processing_snapshots_modes_before_stage_and_keeps_transformations() {
        use crate::dictation::processor::{Profile, Profiles};
        use std::cell::Cell;

        for transformations in [vec![], vec!["fixture-transform".to_string()]] {
            let loaded = Cell::new(false);
            let stage_sent = Cell::new(false);
            let processed = process_job_text(
                &processing_job(TranscriptionTarget::Paste),
                || {
                    assert!(!stage_sent.get());
                    loaded.set(true);
                    Profiles::new(
                        Profile::new("Global", "").transformations(transformations.clone()),
                    )
                },
                || {
                    assert!(loaded.get());
                    stage_sent.set(true);
                },
            );
            assert!(loaded.get());
            assert_eq!(stage_sent.get(), !transformations.is_empty());
            assert_eq!(processed.text, "raw instruction");
            assert_eq!(processed.transformations, transformations);
        }
    }

    #[test]
    #[ignore = "requires HEX_COHERE_MODEL and HEX_COHERE_FIXTURES synthetic audio"]
    fn cohere_long_form_keeps_all_sections_with_and_without_pauses() {
        let model_path = PathBuf::from(std::env::var_os("HEX_COHERE_MODEL").expect("model path"));
        let fixtures =
            PathBuf::from(std::env::var_os("HEX_COHERE_FIXTURES").expect("fixture path"));
        let selection = TranscriptionSelection {
            model: TranscriptionModelId::CohereTranscribe,
            language: "en".into(),
        };
        let mut model = Parakeet::load_from(&model_path, false, Some(&selection)).unwrap();
        eprintln!(
            "Cohere maximum chunk: {:?} samples",
            model.max_audio_samples
        );
        for (name, repeats) in [
            ("long-continuous", 1),
            ("long-pauses", 1),
            ("long-repeated", 3),
        ] {
            let mut reader = WavReader::open(fixtures.join(format!("{name}.wav"))).unwrap();
            assert_eq!(reader.spec().sample_rate, 16_000);
            assert_eq!(reader.spec().channels, 1);
            let samples = reader
                .samples::<i16>()
                .map(|sample| f32::from(sample.unwrap()) / 32768.0)
                .collect::<Vec<_>>();
            assert!(samples.len() > 35 * 16_000);
            let text = model.transcribe(&samples).unwrap().to_lowercase();
            for phrase in [
                "small community garden",
                "bright green door",
                "purple submarine",
            ] {
                assert_eq!(
                    text.matches(phrase).count(),
                    repeats,
                    "{name} lost {phrase:?}: {text}"
                );
            }
            assert!(
                text.split_whitespace().count() >= 160 * repeats,
                "{name}: {text}"
            );
        }
    }

    #[test]
    fn meeting_delta_coalesces_turns_in_time_order() {
        let entry = |source, start_ms, end_ms, text: &str| TranscriptEntry {
            source,
            start_ms,
            end_ms,
            text: text.into(),
        };
        let entries = [
            entry(MeetingSource::Microphone, 0, 1_000, "First."),
            entry(MeetingSource::Microphone, 1_100, 1_400, "Still me."),
            entry(MeetingSource::System, 1_500, 2_500, "Reply."),
            entry(MeetingSource::Microphone, 2_600, 3_000, "Second."),
        ];
        assert_eq!(
            format_meeting_delta(&entries).unwrap(),
            "You: First. Still me.\n\nThem: Reply.\n\nYou: Second."
        );
        assert!(format_meeting_delta(&[]).is_err());
    }

    #[test]
    fn parallel_processing_results_are_released_in_submission_order() {
        let mut outputs = OrderedOutputs::default();
        let job = |sequence| OutputJob::Paste {
            sequence,
            request: PasteRequest::LastTranscript,
        };

        assert!(outputs.push(job(1)).is_empty());
        assert_eq!(
            outputs
                .push(job(0))
                .into_iter()
                .map(|job| job.sequence())
                .collect::<Vec<_>>(),
            [0, 1]
        );
        assert_eq!(
            outputs
                .push(job(2))
                .into_iter()
                .map(|job| job.sequence())
                .collect::<Vec<_>>(),
            [2]
        );
    }

    #[test]
    fn ordered_waiting_outputs_remain_bounded_after_channel_drain() {
        let (output_sender, output_receiver) = mpsc::sync_channel(1);
        let (event_sender, events) = mpsc::channel();
        let worker = DictationWorker {
            engine: None,
            inference_jobs: None,
            output_jobs: Some(output_sender),
            events,
            event_sender,
            state: Arc::new(Mutex::new(WorkerState {
                next_sequence: 1,
                jobs: BTreeMap::from([(DictationJobId(0), Arc::new(JobControl::default()))]),
                pending_pastes: 0,
            })),
            inference_worker: None,
            processor_workers: Vec::new(),
            output_worker: None,
        };
        let mut ordered = OrderedOutputs::default();
        for _ in 1..MAX_PENDING_OUTPUTS {
            worker.paste_last().unwrap();
            assert!(ordered.push(output_receiver.try_recv().unwrap()).is_empty());
        }
        assert_eq!(worker.paste_last(), Err("dictation queue is full"));
        assert_eq!(worker.paste_meeting(), Err("dictation queue is full"));
        assert_eq!(ordered.waiting.len(), MAX_PENDING_OUTPUTS - 1);
        assert_eq!(
            worker.state.lock().unwrap().next_sequence,
            MAX_PENDING_OUTPUTS as u64
        );

        worker.state.lock().unwrap().jobs.remove(&DictationJobId(0));
        worker.paste_last().unwrap();
        assert_eq!(
            output_receiver.try_recv().unwrap().sequence(),
            MAX_PENDING_OUTPUTS as u64
        );
    }

    #[test]
    fn cancellation_replaces_waiting_output_and_ignores_late_completion() {
        let mut outputs = OrderedOutputs::default();
        let completed = |sequence| OutputJob::Completed {
            job_id: DictationJobId(sequence),
            control: Arc::new(JobControl::default()),
            target: TranscriptionTarget::Paste,
            result: Box::new(Err("late result".into())),
        };

        assert!(outputs.push(completed(1)).is_empty());
        assert!(
            outputs
                .push(OutputJob::Cancelled {
                    job_id: DictationJobId(1),
                })
                .is_empty()
        );
        let ready = outputs.push(OutputJob::Paste {
            sequence: 0,
            request: PasteRequest::LastTranscript,
        });
        assert_eq!(ready.len(), 2);
        assert!(matches!(ready[1], OutputJob::Cancelled { .. }));
        assert!(outputs.push(completed(1)).is_empty());
    }

    #[test]
    fn a_job_can_only_be_cancelled_once() {
        let control = JobControl::default();
        assert!(control.cancel());
        assert!(!control.cancel());
        assert!(control.is_cancelled());
    }

    #[test]
    fn output_commit_and_cancellation_are_mutually_exclusive() {
        let committed = JobControl::default();
        assert!(committed.begin_output());
        assert!(!committed.cancel());
        assert!(!committed.is_cancelled());

        let cancelled = JobControl::default();
        assert!(cancelled.cancel());
        assert!(!cancelled.begin_output());
    }

    #[test]
    fn output_stays_cancellable_through_preparation_but_not_after_mutation() {
        use std::cell::RefCell;
        use std::time::{Duration, SystemTime, UNIX_EPOCH};

        use crate::history::{HistoryRetention, HistoryStore};
        use crate::paste::commit_prepared_paste;

        let completed = |text: &str| CompletedTranscript {
            text: text.into(),
            raw: text.into(),
            application: None,
            timings: JobTimings {
                total_started: Instant::now(),
                queue_ms: 0,
                audio_ms: 1_000,
                prepare_ms: 0,
                inference_ms: 0,
            },
            processing: None,
        };
        for cancel_during_preparation in [true, false] {
            let path = std::env::temp_dir().join(format!(
                "hex-output-cancellation-{}-{}.json",
                std::process::id(),
                SystemTime::now()
                    .duration_since(UNIX_EPOCH)
                    .unwrap()
                    .as_nanos()
            ));
            let history = History::new(HistoryStore::open(
                path.clone(),
                HistoryRetention::Forever,
                crate::history::now_ms(),
            ));
            record_history(&history, TranscriptionTarget::Paste, &completed("previous"));
            let saved_history = std::fs::read(&path).unwrap();
            let mut last_transcript = Some("previous".into());
            let clipboard = RefCell::new("original clipboard".to_string());
            let pasted = RefCell::new(Vec::new());
            let control = Arc::new(JobControl::default());
            let (blocked, blocking) = mpsc::channel();
            let (resume, resumed) = mpsc::channel();
            let cancelling = control.clone();
            let cancellation = thread::spawn(move || {
                blocking.recv_timeout(Duration::from_secs(5)).unwrap();
                let accepted = cancelling.cancel();
                resume.send(()).unwrap();
                accepted
            });
            let pause = || {
                blocked.send(()).unwrap();
                resumed.recv_timeout(Duration::from_secs(5)).unwrap();
            };
            let event = finish_output(
                OutputJob::Completed {
                    job_id: DictationJobId(0),
                    control: control.clone(),
                    target: TranscriptionTarget::Paste,
                    result: Box::new(Ok(completed("new output"))),
                },
                &mut |text, mode, commit| {
                    assert!(matches!(mode, PasteMode::Continue));
                    commit_prepared_paste(
                        || {
                            // Model a blocked restore lock or lazy clipboard provider.
                            if cancel_during_preparation {
                                pause();
                            }
                            Ok(clipboard.borrow().clone())
                        },
                        commit,
                        |_previous| {
                            *clipboard.borrow_mut() = text.into();
                            if !cancel_during_preparation {
                                pause();
                            }
                            pasted.borrow_mut().push(text.to_string());
                            Ok(())
                        },
                    )?;
                    Ok(())
                },
                &mut last_transcript,
                Some(&history),
            );

            assert_eq!(cancellation.join().unwrap(), cancel_during_preparation);
            assert_eq!(control.is_cancelled(), cancel_during_preparation);
            if cancel_during_preparation {
                assert!(matches!(
                    event,
                    WorkerEvent::Cancelled {
                        job_id: DictationJobId(0)
                    }
                ));
                assert_eq!(*clipboard.borrow(), "original clipboard");
                assert!(pasted.borrow().is_empty());
                assert_eq!(last_transcript.as_deref(), Some("previous"));
                assert_eq!(history.search("").len(), 1);
                assert_eq!(std::fs::read(&path).unwrap(), saved_history);
            } else {
                assert!(matches!(
                    event,
                    WorkerEvent::Completed { result: Ok(ref text), .. } if text == "new output"
                ));
                assert_eq!(*clipboard.borrow(), "new output");
                assert_eq!(*pasted.borrow(), ["new output"]);
                assert_eq!(last_transcript.as_deref(), Some("new output"));
                let entries = history.search("");
                assert_eq!(entries.len(), 2);
                assert_eq!(entries[0].final_text, "new output");
                assert_eq!(entries[0].kind, HistoryKind::Dictation);
            }
            drop(history);
            std::fs::remove_file(path).unwrap();
        }
    }

    /// Replays recordings into the production engine as capture would (160 ms
    /// blocks, the last 200 ms held back as the release tail) and compares
    /// the stream's release latency and text with whole-clip transcription.
    #[test]
    #[ignore = "requires HEX_STREAM_MODEL (Unified English GGUF) and HEX_STREAM_WAVS (comma-separated)"]
    fn metal_stream_tracks_offline_transcription() {
        use crate::transcription::engine::{Offline, Source, TranscriptionEngine};
        use crate::transcription::{Transcriber, WarmTranscriber};

        let selection = TranscriptionSelection::default();
        let mut model = Parakeet::load_from(
            &PathBuf::from(std::env::var("HEX_STREAM_MODEL").unwrap()),
            false,
            Some(&selection),
        )
        .unwrap();
        model.stream = model
            .session
            .probe_stream(&model.stream_run_options(), "parakeet");
        assert!(model.stream.is_some(), "Metal must stream");
        let engine = TranscriptionEngine::start(WarmTranscriber::serving(Transcriber::Gguf(
            Box::new(model),
        )))
        .unwrap();
        // `HEX_STREAM_LIVE_TEXT=0` replays without reading live text.
        let engine = if std::env::var("HEX_STREAM_LIVE_TEXT").is_ok_and(|value| value == "0") {
            engine
        } else {
            engine.with_live_text()
        };
        let speed = std::env::var("HEX_STREAM_SPEED")
            .ok()
            .and_then(|speed| speed.parse::<f64>().ok())
            .unwrap_or(4.0);
        let offline = || -> Offline {
            let selection = selection.clone();
            Box::new(move |model, samples| {
                model
                    .activate(&selection)
                    .and_then(|model| model.transcribe(samples))
                    .map_err(|error| error.to_string())
            })
        };
        let words = |text: &str| {
            text.split_whitespace()
                .map(|word| {
                    word.chars()
                        .filter(|c| c.is_alphanumeric())
                        .flat_map(char::to_lowercase)
                        .collect::<String>()
                })
                .filter(|word| !word.is_empty())
                .collect::<Vec<_>>()
        };
        for path in std::env::var("HEX_STREAM_WAVS").unwrap().split(',') {
            let mut reader = WavReader::open(path).unwrap();
            let rate = reader.spec().sample_rate;
            let channels = usize::from(reader.spec().channels);
            let raw = match reader.spec().sample_format {
                hound::SampleFormat::Float => reader
                    .samples::<f32>()
                    .map(Result::unwrap)
                    .collect::<Vec<_>>(),
                hound::SampleFormat::Int => reader
                    .samples::<i16>()
                    .map(|sample| f32::from(sample.unwrap()) / 32_768.0)
                    .collect(),
            };
            let mono = raw
                .chunks(channels)
                .map(|frame| frame.iter().sum::<f32>() / channels as f32)
                .collect::<Vec<_>>();
            let samples = resample_for_parakeet(&mono, rate);
            // A warm app has already run inputs of this size.
            let run = |samples: Vec<f32>| {
                engine
                    .finish_with(None, samples, offline())
                    .unwrap()
                    .wait()
                    .unwrap()
            };
            run(samples.clone());
            let whole = run(samples.clone());

            let mut stream = engine.begin().expect("the model streams");
            let held = samples.len().saturating_sub(3_200);
            let feeding = Instant::now();
            // Live text as a HUD would read it: (audio fed in ms, stable
            // words, tentative words), and how often the stable text was
            // rewritten instead of extended.
            let mut timeline = Vec::new();
            let mut stable = String::new();
            let mut rewrites = 0;
            for (index, block) in samples[..held].chunks(2_560).enumerate() {
                let due = std::time::Duration::from_secs_f64(index as f64 * 0.16 / speed);
                thread::sleep(due.saturating_sub(feeding.elapsed()));
                stream.feed(block);
                if let Some(partial) = stream.take_partial() {
                    if !partial.stable.starts_with(stable.as_str()) {
                        rewrites += 1;
                    }
                    stable = partial.stable.clone();
                    timeline.push((
                        stream.fed() / 16,
                        words(&partial.stable).len(),
                        words(&partial.tentative).len(),
                    ));
                }
            }
            thread::sleep(std::time::Duration::from_secs_f64(0.2 / speed));
            let streamed = engine
                .finish_with(Some(stream), samples.clone(), offline())
                .unwrap()
                .wait()
                .unwrap();

            let (reference, hypothesis) = (words(&whole.text), words(&streamed.text));
            let mut previous = (0..=hypothesis.len()).collect::<Vec<_>>();
            for (row, expected) in reference.iter().enumerate() {
                let mut current = vec![row + 1; hypothesis.len() + 1];
                for (column, actual) in hypothesis.iter().enumerate() {
                    current[column + 1] = (previous[column + 1] + 1)
                        .min(current[column] + 1)
                        .min(previous[column] + usize::from(expected != actual));
                }
                previous = current;
            }
            let differences = previous[hypothesis.len()];
            println!(
                "{path}: audio_ms={} offline_ms={} release_ms={} ({}) word_differences={differences}/{}",
                samples.len() / 16,
                whole.finish_ms,
                streamed.finish_ms,
                streamed.source.label(),
                reference.len()
            );
            println!("  offline {}", whole.text);
            println!("  stream  {}", streamed.text);
            let first_text_ms = timeline
                .iter()
                .find(|(_, stable, tentative)| stable + tentative > 0)
                .map(|(fed_ms, ..)| *fed_ms);
            println!(
                "  live text: updates={} first_text_at_audio_ms={first_text_ms:?} stable_rewrites={rewrites} last_stable_words={}/{}",
                timeline.len(),
                timeline.last().map_or(0, |(_, stable, _)| *stable),
                reference.len()
            );
            for (fed_ms, stable, tentative) in &timeline {
                println!("    audio_ms={fed_ms} stable_words={stable} tentative_words={tentative}");
            }
            assert_eq!(streamed.source, Source::Stream);
            assert!(differences * 20 <= reference.len().max(20));
        }
    }

    #[cfg(hex_capture)]
    #[test]
    fn chunked_word_transcription_keeps_word_times_across_chunks() {
        use crate::transcription::{Transcript, TranscriptSegment};
        let span = |start_ms, end_ms, text: &str| TranscriptSegment {
            start_ms,
            end_ms,
            text: text.into(),
        };
        let mut calls = Vec::new();
        let merged = transcribe_in_chunks(vec![0.0; 16_000 * 25], 16_000 * 10, |chunk| {
            calls.push(chunk.len());
            let chunk_index = calls.len();
            Ok(Transcript {
                text: format!(" part {chunk_index} "),
                segments: vec![span(0, 900, " part ")],
                words: vec![
                    span(-5, 400, " part"),
                    span(450, 300, "two"),
                    span(500, 600, "  "),
                ],
            })
        })
        .unwrap();
        assert_eq!(calls, [160_000, 160_000, 80_000]);
        assert_eq!(merged.text, "part 1 part 2 part 3");
        let words = merged
            .words
            .iter()
            .map(|word| (word.start_ms, word.end_ms, word.text.as_str()))
            .collect::<Vec<_>>();
        assert_eq!(
            words,
            [
                (0, 400, "part"),
                (450, 450, "two"),
                (10_000, 10_400, "part"),
                (10_450, 10_450, "two"),
                (20_000, 20_400, "part"),
                (20_450, 20_450, "two"),
            ]
        );
        assert_eq!(merged.segments[2].start_ms, 20_000);
    }

    #[cfg(hex_capture)]
    #[test]
    fn word_transcription_is_a_foreground_request_on_the_shared_engine() {
        use crate::transcription::engine::Command as EngineCommand;
        let (engine, commands) = TranscriptionEngine::for_test();
        let engine = Arc::new(engine);
        let weak = Arc::downgrade(&engine);
        assert!(
            transcribe_with_words(&weak, &Default::default(), Vec::new())
                .unwrap()
                .words
                .is_empty()
        );
        assert!(
            commands.try_recv().is_err(),
            "silence never reaches the model"
        );

        // A busy dictation holds the chunk back until it is idle.
        let busy = DictationInference::begin();
        let request = {
            let weak = weak.clone();
            thread::spawn(move || {
                transcribe_with_words(
                    &weak,
                    &Default::default(),
                    vec![0.1; WORD_CHUNK_SAMPLES + 1_600],
                )
            })
        };
        assert!(
            commands
                .recv_timeout(std::time::Duration::from_millis(200))
                .is_err()
        );
        drop(busy);
        let command = commands
            .recv_timeout(std::time::Duration::from_secs(5))
            .unwrap();
        // A foreground request: after dictation already sent, counted as a
        // request so meeting chunks yield to it, and never inside a stream.
        assert!(matches!(
            command,
            EngineCommand::Foreground { ref clip, .. } if clip.len() == WORD_CHUNK_SAMPLES
        ));
        // The next chunk waits for this one, so a dictation sent meanwhile
        // runs first.
        assert!(
            commands
                .recv_timeout(std::time::Duration::from_millis(100))
                .is_err()
        );
        drop(command);
        assert!(request.join().unwrap().is_err());
        // Only a weak reference is held: once the engine is gone, a capture
        // fails instead of keeping it alive.
        drop(engine);
        assert!(transcribe_with_words(&weak, &Default::default(), vec![0.1; 16]).is_err());
    }

    #[test]
    fn repeated_cancellation_walks_back_through_pending_jobs() {
        let jobs = [0, 1, 2]
            .into_iter()
            .map(|sequence| (DictationJobId(sequence), Arc::new(JobControl::default())))
            .collect();
        let state = WorkerState {
            next_sequence: 3,
            jobs,
            pending_pastes: 0,
        };

        assert_eq!(state.cancel_latest(), Some(DictationJobId(2)));
        assert_eq!(state.cancel_latest(), Some(DictationJobId(1)));
        assert_eq!(state.cancel_latest(), Some(DictationJobId(0)));
        assert_eq!(state.cancel_latest(), None);
    }

    #[test]
    fn escape_skips_programmatic_dictation_jobs() {
        let owned = || {
            Arc::new(JobControl {
                owner_cancels: true,
                ..JobControl::default()
            })
        };
        let state = WorkerState {
            next_sequence: 2,
            jobs: BTreeMap::from([
                (DictationJobId(0), Arc::new(JobControl::default())),
                (DictationJobId(1), owned()),
            ]),
            pending_pastes: 0,
        };
        assert!(state.has_escape_cancellable_job());
        assert_eq!(state.cancel_latest(), Some(DictationJobId(0)));
        assert_eq!(state.cancel_latest(), None);
        assert!(!state.has_escape_cancellable_job());
        assert!(!state.jobs[&DictationJobId(1)].is_cancelled());

        // A job whose output has started can no longer be cancelled.
        let pasting = WorkerState {
            next_sequence: 1,
            jobs: BTreeMap::from([(DictationJobId(0), Arc::new(JobControl::default()))]),
            pending_pastes: 0,
        };
        assert!(pasting.jobs[&DictationJobId(0)].begin_output());
        assert!(!pasting.has_escape_cancellable_job());
        assert_eq!(pasting.cancel_latest(), None);

        let only_owned = WorkerState {
            next_sequence: 1,
            jobs: BTreeMap::from([(DictationJobId(0), owned())]),
            pending_pastes: 0,
        };
        assert!(!only_owned.has_escape_cancellable_job());
    }

    #[test]
    #[ignore = "requires HEX_STUTTER_FIXTURE and the installed Parakeet model"]
    fn tdt_decoder_does_not_repeat_single_letter_tokens() {
        let path = PathBuf::from(std::env::var("HEX_STUTTER_FIXTURE").unwrap());
        let mut reader = WavReader::open(path).unwrap();
        let sample_rate = reader.spec().sample_rate;
        let samples = reader
            .samples::<f32>()
            .skip(sample_rate as usize * 20)
            .take(sample_rate as usize * 25)
            .collect::<Result<Vec<_>, _>>()
            .unwrap();
        let samples = resample_for_parakeet(&samples, sample_rate);
        let text = Parakeet::load().unwrap().transcribe(&samples).unwrap();

        assert!(text.contains("sweatshirt"), "unexpected transcript: {text}");
        assert!(
            !text.contains("p p p p") && !text.contains("p-p-p-p"),
            "pathological token repetition: {text}"
        );
    }
}

#[cfg(test)]
#[path = "dictation/tail_tests.rs"]
mod dictation_tail_tests;

#[cfg(test)]
#[path = "dictation/corpus_tests.rs"]
mod dictation_corpus_tests;

/// Loads a GGUF model file directly, for opt-in harnesses that replay
/// recordings without the installed model catalog.
#[cfg(test)]
pub(crate) fn load_test_model(path: &std::path::Path) -> Result<Parakeet> {
    Parakeet::load_from(path, false, Some(&TranscriptionSelection::default()))
}
