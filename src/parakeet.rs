use std::collections::{BTreeMap, HashSet};
use std::sync::Arc;
use std::sync::Mutex;
use std::sync::Once;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{self, Receiver, SyncSender, TrySendError};
use std::thread;
use std::time::Instant;

use color_eyre::eyre::{Result, WrapErr, eyre};
use transcribe_cpp::{
    Backend, ExtSlot, Model, ModelOptions, RunExtension, RunOptions, Session, TimestampKind,
    Transcript, WhisperRunOptions, sys::TRANSCRIBE_EXT_KIND_WHISPER_RUN,
};

use crate::context::ContextSnapshot;
use crate::dictation::{DictationClip, pad_for_parakeet, strip_dictation_protocol};
use crate::dictation_processor::ProcessingObservation;
use crate::meeting::{self, TranscriptEntry, TranscriptPublication};
use crate::paste::Paster;
use crate::suppression::InputActivity;
#[cfg(test)]
use crate::text_replacements::ReplacementSet;
use crate::transcription::WarmTranscriber;
use crate::transcription_models::{TranscriptionSelection, model_path, validate};

pub struct Parakeet {
    session: Session,
    options: RunOptions,
    name: String,
    selection: Option<TranscriptionSelection>,
    max_audio_samples: Option<usize>,
}

static TRANSCRIBE_LOGGING: Once = Once::new();

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
    MeetingDelta,
}

#[derive(Clone, Copy, Debug)]
pub enum TranscriptionTarget {
    Paste,
    Send,
    Edit,
}

struct InferenceJob {
    job_id: DictationJobId,
    control: Arc<JobControl>,
    submitted_at: Instant,
    clip: DictationClip,
    target: TranscriptionTarget,
    context: ContextSnapshot,
    selection: TranscriptionSelection,
}

enum InferenceCommand {
    Reload(TranscriptionSelection),
    Transcribe(Box<InferenceJob>),
}

struct ProcessorJob {
    job_id: DictationJobId,
    control: Arc<JobControl>,
    target: TranscriptionTarget,
    text: String,
    context: ContextSnapshot,
    total_started: Instant,
    queue_ms: u128,
    audio_ms: u64,
    prepare_ms: u128,
    inference_ms: u128,
}

struct CompletedTranscript {
    text: String,
    total_started: Instant,
    queue_ms: u128,
    audio_ms: u64,
    prepare_ms: u128,
    inference_ms: u128,
    processing: Option<ProcessingObservation>,
    edit_source: Option<EditSource>,
}

struct EditSource {
    application: Option<String>,
    window_title: Option<String>,
    selected_text: String,
    input_revision: u64,
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
        kind: PasteKind,
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

    fn is_cancelled(&self) -> bool {
        self.cancelled.load(Ordering::Acquire)
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
    inference_jobs: Option<SyncSender<InferenceCommand>>,
    output_jobs: Option<SyncSender<OutputJob>>,
    events: Receiver<WorkerEvent>,
    state: Arc<Mutex<WorkerState>>,
    inference_worker: Option<thread::JoinHandle<()>>,
    processor_workers: Vec<thread::JoinHandle<()>>,
    output_worker: Option<thread::JoinHandle<()>>,
}

struct WorkerState {
    output_available: bool,
    next_sequence: u64,
    jobs: BTreeMap<DictationJobId, Arc<JobControl>>,
    pending_pastes: usize,
}

impl WorkerState {
    fn cancel_latest(&self) -> Option<DictationJobId> {
        self.jobs
            .iter()
            .rev()
            .find_map(|(&job_id, control)| control.cancel().then_some(job_id))
    }
}

impl DictationWorker {
    pub fn start(activity: InputActivity) -> Self {
        const PROCESSOR_WORKERS: usize = 2;
        let (inference_jobs, inference_receiver) = mpsc::sync_channel::<InferenceCommand>(2);
        let (processor_jobs, processor_receiver) = mpsc::sync_channel::<ProcessorJob>(4);
        let (output_jobs, output_receiver) = mpsc::sync_channel::<OutputJob>(8);
        let (event_sender, events) = mpsc::channel();
        let state = Arc::new(Mutex::new(WorkerState {
            output_available: true,
            next_sequence: 0,
            jobs: BTreeMap::new(),
            pending_pastes: 0,
        }));

        let output_state = state.clone();
        let output_events = event_sender.clone();
        let output_worker = thread::spawn(move || {
            let mut paster = match Paster::new(activity) {
                Ok(paster) => paster,
                Err(error) => {
                    let mut state = output_state
                        .lock()
                        .unwrap_or_else(|error| error.into_inner());
                    state.output_available = false;
                    state.jobs.clear();
                    state.pending_pastes = 0;
                    let _ = output_events.send(WorkerEvent::ModelFailed(error.to_string()));
                    return;
                }
            };
            let mut last_transcript = None;
            let mut meeting_cursor = MeetingPasteCursor::default();
            let mut ordered = OrderedOutputs::default();
            while let Ok(job) = output_receiver.recv() {
                if matches!(job, OutputJob::PreparePaste) {
                    paster.prepare();
                    continue;
                }
                for job in ordered.push(job) {
                    let event =
                        finish_output(job, &mut paster, &mut last_transcript, &mut meeting_cursor);
                    let mut state = output_state
                        .lock()
                        .unwrap_or_else(|error| error.into_inner());
                    match &event {
                        WorkerEvent::Completed { job_id, .. }
                        | WorkerEvent::Cancelled { job_id } => {
                            state.jobs.remove(job_id);
                        }
                        WorkerEvent::Pasted { .. } => {
                            state.pending_pastes = state.pending_pastes.saturating_sub(1);
                        }
                        WorkerEvent::ModelFailed(_) | WorkerEvent::Stage { .. } => {}
                    }
                    drop(state);
                    if output_events.send(event).is_err() {
                        return;
                    }
                }
            }
        });

        let processor_receiver = Arc::new(Mutex::new(processor_receiver));
        let mut processor_workers = Vec::with_capacity(PROCESSOR_WORKERS);
        for _ in 0..PROCESSOR_WORKERS {
            let processor_receiver = processor_receiver.clone();
            let processor_output = output_jobs.clone();
            let processor_events = event_sender.clone();
            processor_workers.push(thread::spawn(move || {
                loop {
                    let job = {
                        processor_receiver
                            .lock()
                            .unwrap_or_else(|error| error.into_inner())
                            .recv()
                    };
                    let Ok(job) = job else { break };
                    if job.control.is_cancelled() {
                        let _ = processor_output.send(OutputJob::Cancelled { job_id: job.job_id });
                        continue;
                    }
                    let profiles = crate::config::dictation_profiles();
                    if matches!(job.target, TranscriptionTarget::Edit)
                        || profiles.processes(&job.context)
                    {
                        let _ = processor_events.send(WorkerEvent::Stage {
                            job_id: job.job_id,
                            stage: DictationJobStage::Processing,
                        });
                    }
                    let processed = if matches!(job.target, TranscriptionTarget::Edit) {
                        profiles.process_edit_cancellable(
                            &job.text,
                            job.context.selected_text.as_deref().unwrap_or_default(),
                            &job.context,
                            &job.control.cancelled,
                        )
                    } else {
                        profiles.process_cancellable(
                            &job.text,
                            &job.context,
                            &job.control.cancelled,
                        )
                    };
                    if job.control.is_cancelled() {
                        let _ = processor_output.send(OutputJob::Cancelled { job_id: job.job_id });
                        continue;
                    }
                    if let Some(observation) = &processed.observation
                        && let Some(error) = &observation.fallback
                    {
                        tracing::warn!(
                            profile = observation.profile,
                            %error,
                            "dictation post-processing fell back to raw transcript"
                        );
                    }
                    let edit_source =
                        matches!(job.target, TranscriptionTarget::Edit).then(|| EditSource {
                            application: job.context.application.clone(),
                            window_title: job.context.window_title.clone(),
                            selected_text: job.context.selected_text.clone().unwrap_or_default(),
                            input_revision: job.context.input_revision.unwrap_or(u64::MAX),
                        });
                    if processor_output
                        .send(OutputJob::Completed {
                            job_id: job.job_id,
                            control: job.control,
                            target: job.target,
                            result: Box::new(Ok(CompletedTranscript {
                                text: processed.text,
                                total_started: job.total_started,
                                queue_ms: job.queue_ms,
                                audio_ms: job.audio_ms,
                                prepare_ms: job.prepare_ms,
                                inference_ms: job.inference_ms,
                                processing: processed.observation,
                                edit_source,
                            })),
                        })
                        .is_err()
                    {
                        break;
                    }
                }
            }));
        }

        let inference_events = event_sender.clone();
        let inference_output = output_jobs.clone();
        let inference_worker = thread::spawn(move || {
            prioritize_inference_thread();
            let mut transcriber = match WarmTranscriber::load() {
                Ok(transcriber) => {
                    tracing::info!("transcription model loaded");
                    transcriber
                }
                Err(error) => {
                    let _ = inference_events.send(WorkerEvent::ModelFailed(error.to_string()));
                    WarmTranscriber::default()
                }
            };
            while let Ok(command) = inference_receiver.recv() {
                let job = match command {
                    InferenceCommand::Reload(selection) => {
                        if let Err(error) = transcriber.activate(&selection) {
                            let _ =
                                inference_events.send(WorkerEvent::ModelFailed(error.to_string()));
                        }
                        continue;
                    }
                    InferenceCommand::Transcribe(job) => *job,
                };
                if job.control.is_cancelled() {
                    let _ = inference_output.send(OutputJob::Cancelled { job_id: job.job_id });
                    continue;
                }
                let _ = inference_events.send(WorkerEvent::Stage {
                    job_id: job.job_id,
                    stage: DictationJobStage::Transcribing,
                });
                let transcriber = match transcriber.activate(&job.selection) {
                    Ok(transcriber) => transcriber,
                    Err(error) => {
                        let _ = inference_output.send(OutputJob::Completed {
                            job_id: job.job_id,
                            control: job.control,
                            target: job.target,
                            result: Box::new(Err(error.to_string())),
                        });
                        continue;
                    }
                };
                let total_started = job.submitted_at;
                let queue_ms = total_started.elapsed().as_millis();
                let audio_ms = job.clip.duration_ms();
                let prepare_started = Instant::now();
                let samples = transcriber.prepare_samples(job.clip.into_transcription_samples());
                let prepare_ms = prepare_started.elapsed().as_millis();
                let inference_started = Instant::now();
                let result = transcriber
                    .transcribe(&samples)
                    .map(|text| {
                        let corrected = prepare_transcript(&text, job.target);
                        tracing::debug!(
                            raw_transcript = text,
                            corrected_transcript = corrected,
                            "prepared local transcript"
                        );
                        corrected
                    })
                    .map_err(|error| error.to_string());
                let inference_ms = inference_started.elapsed().as_millis();
                if job.control.is_cancelled() {
                    let _ = inference_output.send(OutputJob::Cancelled { job_id: job.job_id });
                    continue;
                }
                match result {
                    Ok(text)
                        if matches!(
                            job.target,
                            TranscriptionTarget::Paste
                                | TranscriptionTarget::Send
                                | TranscriptionTarget::Edit
                        ) && !text.trim().is_empty() =>
                    {
                        if processor_jobs
                            .send(ProcessorJob {
                                job_id: job.job_id,
                                control: job.control,
                                target: job.target,
                                text,
                                context: job.context,
                                total_started,
                                queue_ms,
                                audio_ms,
                                prepare_ms,
                                inference_ms,
                            })
                            .is_err()
                        {
                            break;
                        }
                    }
                    result => {
                        let result = result.map(|text| CompletedTranscript {
                            text,
                            total_started,
                            queue_ms,
                            audio_ms,
                            prepare_ms,
                            inference_ms,
                            processing: None,
                            edit_source: None,
                        });
                        if inference_output
                            .send(OutputJob::Completed {
                                job_id: job.job_id,
                                control: job.control,
                                target: job.target,
                                result: Box::new(result),
                            })
                            .is_err()
                        {
                            break;
                        }
                    }
                }
            }
        });

        Self {
            inference_jobs: Some(inference_jobs),
            output_jobs: Some(output_jobs),
            events,
            state,
            inference_worker: Some(inference_worker),
            processor_workers,
            output_worker: Some(output_worker),
        }
    }

    pub fn transcribe(
        &self,
        clip: DictationClip,
        target: TranscriptionTarget,
        context: ContextSnapshot,
    ) -> Result<DictationJobId, &'static str> {
        let mut state = self.state.lock().unwrap_or_else(|error| error.into_inner());
        if !state.output_available {
            return Err("dictation worker is unavailable");
        }
        let job_id = DictationJobId(state.next_sequence);
        let control = Arc::new(JobControl::default());
        let (_, selection) = crate::app_settings::transcription_selection();
        self.inference_jobs
            .as_ref()
            .ok_or("dictation worker is unavailable")?
            .try_send(InferenceCommand::Transcribe(Box::new(InferenceJob {
                job_id,
                control: control.clone(),
                submitted_at: Instant::now(),
                clip,
                target,
                context,
                selection,
            })))
            .map(|()| {
                state.next_sequence += 1;
                state.jobs.insert(job_id, control);
                job_id
            })
            .map_err(queue_error)
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

    pub fn reload(&self, selection: TranscriptionSelection) -> Result<(), &'static str> {
        self.inference_jobs
            .as_ref()
            .ok_or("dictation worker is unavailable")?
            .try_send(InferenceCommand::Reload(selection))
            .map_err(queue_error)
    }

    pub fn paste_last(&self) -> Result<(), &'static str> {
        self.submit_paste(PasteKind::LastTranscript)
    }

    pub fn paste_meeting(&self) -> Result<(), &'static str> {
        self.submit_paste(PasteKind::MeetingDelta)
    }

    fn submit_paste(&self, kind: PasteKind) -> Result<(), &'static str> {
        let mut state = self.state.lock().unwrap_or_else(|error| error.into_inner());
        if !state.output_available {
            return Err("dictation worker is unavailable");
        }
        let sequence = state.next_sequence;
        self.output_jobs
            .as_ref()
            .ok_or("dictation worker is unavailable")?
            .try_send(OutputJob::Paste { sequence, kind })
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

fn join_worker(worker: Option<thread::JoinHandle<()>>, name: &str) {
    if let Some(worker) = worker
        && worker.join().is_err()
    {
        tracing::error!(worker = name, "worker panicked during shutdown");
    }
}

#[cfg(target_os = "macos")]
pub(crate) fn prioritize_inference_thread() {
    const QOS_CLASS_USER_INITIATED: u32 = 0x19;
    unsafe extern "C" {
        fn pthread_set_qos_class_self_np(qos_class: u32, relative_priority: i32) -> i32;
    }
    // SAFETY: This configures only the calling worker thread using the public
    // macOS pthread QoS API. Relative priority zero is valid for this class.
    let status = unsafe { pthread_set_qos_class_self_np(QOS_CLASS_USER_INITIATED, 0) };
    if status == 0 {
        tracing::info!("dictation inference worker uses user-initiated QoS");
    } else {
        tracing::warn!(status, "could not raise dictation inference worker QoS");
    }
}

#[cfg(not(target_os = "macos"))]
pub(crate) fn prioritize_inference_thread() {}

fn queue_error(error: TrySendError<impl Sized>) -> &'static str {
    match error {
        TrySendError::Full(_) => "dictation queue is full",
        TrySendError::Disconnected(_) => "dictation worker is unavailable",
    }
}

fn finish_output(
    job: OutputJob,
    paster: &mut Paster,
    last_transcript: &mut Option<String>,
    meeting_cursor: &mut MeetingPasteCursor,
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
                if matches!(target, TranscriptionTarget::Edit)
                    && let Some(error) = completed
                        .processing
                        .as_ref()
                        .and_then(|processing| processing.fallback.clone())
                {
                    return Err(error);
                }
                if matches!(target, TranscriptionTarget::Edit) {
                    validate_edit_destination(
                        completed
                            .edit_source
                            .as_ref()
                            .ok_or_else(|| "voice edit lost its source selection".to_string())?,
                        paster.activity_revision(),
                    )?;
                }
                if control.is_cancelled() {
                    return Err("voice edit was cancelled".into());
                }
                let paste_started = Instant::now();
                if !completed.text.trim().is_empty() {
                    if !control.begin_output() {
                        return Err("dictation was cancelled".into());
                    }
                    match target {
                        TranscriptionTarget::Paste => paster
                            .paste(&completed.text)
                            .map_err(|error| error.to_string())?,
                        TranscriptionTarget::Send => paster
                            .paste_and_send(&completed.text)
                            .map_err(|error| error.to_string())?,
                        TranscriptionTarget::Edit => {
                            let source = completed.edit_source.as_ref().ok_or_else(|| {
                                "voice edit lost its source selection".to_string()
                            })?;
                            crate::selected_text::replace(
                                &source.selected_text,
                                &completed.text,
                                || paster.activity_revision() == source.input_revision,
                            )
                            .map_err(|error| error.to_string())?
                        }
                    }
                    if matches!(
                        target,
                        TranscriptionTarget::Paste | TranscriptionTarget::Send
                    ) {
                        *last_transcript = Some(completed.text.clone());
                    }
                }
                tracing::info!(
                    audio_ms = completed.audio_ms,
                    queue_ms = completed.queue_ms,
                    prepare_ms = completed.prepare_ms,
                    inference_ms = completed.inference_ms,
                    paste_ms = paste_started.elapsed().as_millis(),
                    total_ms = completed.total_started.elapsed().as_millis(),
                    "dictation pipeline completed"
                );
                Ok(completed.text)
            });
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
        OutputJob::Paste { kind, .. } => {
            let result = match kind {
                PasteKind::LastTranscript => last_transcript
                    .as_deref()
                    .ok_or_else(|| "no previous transcript is available".to_string())
                    .and_then(|text| {
                        paster.paste(text).map_err(|error| error.to_string())?;
                        Ok(text.to_string())
                    }),
                PasteKind::MeetingDelta => {
                    paste_meeting_delta(paster, meeting_cursor).map_err(|error| error.to_string())
                }
            };
            WorkerEvent::Pasted { kind, result }
        }
    }
}

fn validate_edit_destination(source: &EditSource, input_revision: u64) -> Result<(), String> {
    let context = ContextSnapshot::capture().map_err(|error| error.to_string())?;
    let selected_text = crate::selected_text::capture().map_err(|error| error.to_string())?;
    validate_edit_identity(source, &context, &selected_text, input_revision)
}

fn validate_edit_identity(
    source: &EditSource,
    context: &ContextSnapshot,
    selected_text: &str,
    input_revision: u64,
) -> Result<(), String> {
    if input_revision != source.input_revision {
        return Err("input changed before voice edit completed".into());
    }
    if context.application != source.application {
        return Err("the foreground application changed before voice edit completed".into());
    }
    if context.window_title != source.window_title {
        return Err("the foreground window changed before voice edit completed".into());
    }
    if selected_text != source.selected_text {
        return Err("the selected text changed before voice edit completed".into());
    }
    Ok(())
}

fn prepare_transcript(text: &str, target: TranscriptionTarget) -> String {
    let stripped = strip_transcript_protocol(text, target);
    crate::text_replacements::replace(&stripped)
}

#[cfg(test)]
fn prepare_transcript_with(
    text: &str,
    target: TranscriptionTarget,
    replacements: &ReplacementSet,
) -> String {
    replacements.replace(&strip_transcript_protocol(text, target))
}

fn strip_transcript_protocol(text: &str, target: TranscriptionTarget) -> String {
    match target {
        TranscriptionTarget::Paste | TranscriptionTarget::Send | TranscriptionTarget::Edit => {
            strip_dictation_protocol(text)
        }
    }
}

#[derive(Default)]
struct MeetingPasteCursor {
    meeting_id: Option<String>,
    publication: Option<TranscriptPublication>,
    seen: HashSet<TranscriptEntry>,
}

fn paste_meeting_delta(paster: &mut Paster, cursor: &mut MeetingPasteCursor) -> Result<String> {
    let meetings = meeting::list()?;
    let selected = meeting::active_or_latest(&meetings)
        .ok_or_else(|| eyre!("no meeting transcript is available"))?;
    // Live and final ASR use different segment boundaries, so switching a
    // cursor between them could repeat or skip an overlapping segment.
    let same_meeting = cursor.meeting_id.as_deref() == Some(&selected.id);
    let empty = HashSet::new();
    let (publication, seen) = if same_meeting {
        (cursor.publication, &cursor.seen)
    } else {
        (None, &empty)
    };
    let transcript = meeting::completed_transcript(&selected.id, publication)?;
    let (text, pasted_entries) = prepare_meeting_delta(&transcript.entries, seen)?;
    paster.paste_standalone(&text)?;
    if !same_meeting {
        cursor.seen.clear();
    }
    cursor.meeting_id = Some(selected.id.clone());
    cursor.publication = Some(transcript.publication);
    cursor.seen.extend(pasted_entries);
    Ok(text)
}

fn prepare_meeting_delta(
    entries: &[TranscriptEntry],
    seen: &HashSet<TranscriptEntry>,
) -> Result<(String, Vec<TranscriptEntry>)> {
    let new_entries = entries
        .iter()
        .filter(|entry| !seen.contains(*entry))
        .cloned()
        .collect::<Vec<_>>();
    if new_entries.is_empty() {
        return Err(eyre!("no new completed meeting transcript is available"));
    }
    let text = meeting::coalesce_transcript(new_entries.iter().cloned())
        .into_iter()
        .map(|entry| format!("{}: {}", entry.source.label(), entry.text))
        .collect::<Vec<_>>()
        .join("\n\n");
    Ok((text, new_entries))
}

pub(crate) fn default_model_path() -> Result<std::path::PathBuf> {
    model_path(crate::transcription_models::definition(
        crate::transcription_models::TranscriptionModelId::ParakeetV2,
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
        if !crate::transcription_models::is_installed(definition, &selection.language) {
            return Err(eyre!("{} is not installed", definition.name));
        }
        let path = model_path(definition)?;
        let mut parakeet = Self::load_from(&path, true, Some(selection))?;
        let prewarm_started = Instant::now();
        let mut silence = Vec::new();
        pad_for_parakeet(&mut silence);
        parakeet
            .transcribe(&silence)
            .wrap_err("could not prewarm transcription model")?;
        tracing::info!(
            prewarm_ms = prewarm_started.elapsed().as_millis(),
            "prewarmed transcription model"
        );
        Ok(parakeet)
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
                gpu_device: 0,
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
        let name = format!("transcribe-cpp-{}-{variant}", device.kind);
        let definition = selection.map(validate).transpose()?;
        if let Some(definition) = definition {
            let architecture = model.arch();
            let crate::transcription_models::ModelRuntime::Gguf(artifact) = definition.runtime
            else {
                return Err(eyre!(
                    "{} is not a GGUF transcription model",
                    definition.name
                ));
            };
            if architecture != artifact.architecture || variant != artifact.variant {
                return Err(eyre!(
                    "{} contains {architecture}/{variant}, expected {}/{}",
                    model_path.display(),
                    artifact.architecture,
                    artifact.variant
                ));
            }
        }
        tracing::info!(
            backend = device.kind,
            device = device.description,
            variant,
            "loaded transcription model"
        );
        let capabilities = model.capabilities();
        if let (Some(selection), Some(definition)) = (selection, definition) {
            let runtime_language = definition.runtime_language(&selection.language);
            if !capabilities.languages.is_empty()
                && !capabilities
                    .languages
                    .iter()
                    .any(|language| language == runtime_language)
            {
                return Err(eyre!(
                    "{} does not advertise support for {}",
                    definition.name,
                    crate::transcription_models::language_name(&selection.language)
                ));
            }
            if definition.supports_recognition_hints
                && !model.accepts_ext(ExtSlot::Run, TRANSCRIBE_EXT_KIND_WHISPER_RUN)
            {
                return Err(eyre!(
                    "{} does not accept Whisper recognition hints",
                    definition.name
                ));
            }
        }
        let session = model.session()?;
        let language = selection
            .zip(definition)
            .filter(|(_, definition)| definition.accepts_language_hint)
            .map(|(selection, definition)| {
                definition.runtime_language(&selection.language).to_string()
            });
        let family = selection
            .zip(definition)
            .and_then(|(selection, definition)| {
                definition.supports_recognition_hints.then(|| {
                    RunExtension::Whisper(WhisperRunOptions {
                        initial_prompt: (!selection.recognition_hints.trim().is_empty())
                            .then(|| selection.recognition_hints.trim().to_string()),
                        ..Default::default()
                    })
                })
            });
        Ok(Self {
            session,
            options: RunOptions {
                timestamps: if capabilities.max_timestamp_kind == TimestampKind::None {
                    TimestampKind::None
                } else {
                    TimestampKind::Segment
                },
                language,
                family,
                ..Default::default()
            },
            name,
            selection: selection.cloned(),
            max_audio_samples: (capabilities.max_audio_ms > 0)
                .then_some(capabilities.max_audio_ms as usize * 16),
        })
    }

    pub(crate) fn name(&self) -> &str {
        &self.name
    }

    pub(crate) fn matches_selection(&self, selection: &TranscriptionSelection) -> bool {
        self.selection.as_ref() == Some(selection)
    }

    pub fn transcribe(&mut self, samples: &[f32]) -> Result<String> {
        let Some(max_audio_samples) = self.max_audio_samples else {
            return self.transcribe_segments(samples).map(|result| result.text);
        };
        if samples.len() <= max_audio_samples {
            return self.transcribe_segments(samples).map(|result| result.text);
        }
        let mut text = Vec::new();
        for chunk in samples.chunks(max_audio_samples) {
            let mut chunk = chunk.to_vec();
            pad_for_parakeet(&mut chunk);
            let result = self.transcribe_segments(&chunk)?;
            let result = result.text.trim();
            if !result.is_empty() {
                text.push(result.to_string());
            }
        }
        Ok(text.join(" "))
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

    #[test]
    fn voice_edit_destination_requires_unchanged_input_app_window_and_selection() {
        let source = EditSource {
            application: Some("TextEdit".into()),
            window_title: Some("Draft".into()),
            selected_text: "original text".into(),
            input_revision: 7,
        };
        let context = ContextSnapshot {
            application: source.application.clone(),
            browser_url: None,
            window_title: source.window_title.clone(),
            selected_text: None,
            input_revision: None,
        };

        assert!(validate_edit_identity(&source, &context, "original text", 7).is_ok());
        assert!(validate_edit_identity(&source, &context, "original text", 8).is_err());
        assert!(validate_edit_identity(&source, &context, "other text", 7).is_err());
        let mut other_window = context.clone();
        other_window.window_title = Some("Other".into());
        assert!(validate_edit_identity(&source, &other_window, "original text", 7).is_err());
    }
    use crate::app_settings::TextReplacement;
    use crate::dictation::resample_for_parakeet;
    use crate::meeting::MeetingSource;

    #[test]
    fn replacements_follow_control_stripping_and_stay_corrected_through_fallback() {
        let replacements = ReplacementSet::new(&[
            TextReplacement {
                matched_phrase: "open code".into(),
                output: "OpenCode".into(),
            },
            TextReplacement {
                matched_phrase: "alpha".into(),
                output: "beta".into(),
            },
            TextReplacement {
                matched_phrase: "beta".into(),
                output: "gamma".into(),
            },
        ]);

        let corrected = prepare_transcript_with(
            "Dictate start, use open code and alpha. Dictate stop.",
            TranscriptionTarget::Paste,
            &replacements,
        );

        assert_eq!(corrected, "use OpenCode and beta");
        assert_eq!(replacements.replace("beta"), "gamma");
        assert_eq!(replacements.replace("Use OPEN CODE."), "Use OpenCode.");
    }

    #[test]
    fn meeting_delta_tracks_each_source_and_coalesces_turns() {
        let entry = |source, start_ms, end_ms, text: &str| TranscriptEntry {
            source,
            start_ms,
            end_ms,
            text: text.into(),
        };
        let entries = [
            entry(MeetingSource::Microphone, 0, 1_000, "First."),
            entry(MeetingSource::System, 500, 2_500, "Reply."),
            entry(MeetingSource::Microphone, 1_100, 2_000, "Second."),
        ];

        let mut seen = HashSet::new();
        let (text, pasted) = prepare_meeting_delta(&entries, &seen).unwrap();
        assert_eq!(text, "You: First.\n\nComputer: Reply.\n\nYou: Second.");
        seen.extend(pasted);
        assert!(prepare_meeting_delta(&entries, &seen).is_err());

        let appended = [
            entries[0].clone(),
            entries[1].clone(),
            entry(MeetingSource::System, 900, 1_050, "Late completion."),
            entries[2].clone(),
            entry(MeetingSource::Microphone, 2_600, 3_000, "Third."),
            entry(MeetingSource::System, 2_800, 3_200, "Follow-up."),
        ];
        let (text, pasted) = prepare_meeting_delta(&appended, &seen).unwrap();
        assert_eq!(
            text,
            "Computer: Late completion.\n\nYou: Third.\n\nComputer: Follow-up."
        );
        seen.extend(pasted);
        assert!(prepare_meeting_delta(&appended, &seen).is_err());
    }

    #[test]
    fn parallel_processing_results_are_released_in_submission_order() {
        let mut outputs = OrderedOutputs::default();
        let job = |sequence| OutputJob::Paste {
            sequence,
            kind: PasteKind::LastTranscript,
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
            kind: PasteKind::LastTranscript,
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
    fn repeated_cancellation_walks_back_through_pending_jobs() {
        let jobs = [0, 1, 2]
            .into_iter()
            .map(|sequence| (DictationJobId(sequence), Arc::new(JobControl::default())))
            .collect();
        let state = WorkerState {
            output_available: true,
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
    #[ignore = "requires VOICE_CONTROL_STUTTER_FIXTURE and the installed Parakeet model"]
    fn tdt_decoder_does_not_repeat_single_letter_tokens() {
        let path = PathBuf::from(std::env::var("VOICE_CONTROL_STUTTER_FIXTURE").unwrap());
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
