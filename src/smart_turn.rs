//! Smart Turn auto-stop for hands-free dictation (experiment `smart-turn`).
//!
//! While a locked, Toggle, or voice-started dictation records, its audio is
//! resampled to 16 kHz and split into 20 ms frames with the meeting pause
//! detector's speech rule. Once speech has been heard and the silence after
//! it reaches the check point, a worker thread runs Pipecat's Smart Turn v3.2
//! on the last 8 s. A probability at or over the threshold finishes the
//! capture; anything lower keeps listening until the next pause or the plain
//! silence fallback. The worker keeps only the newest request, and answers
//! for an older pause or capture are stale.

use std::collections::VecDeque;
use std::fs::{self, File};
use std::io::Read;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{self, Receiver, Sender};
use std::sync::{Arc, Condvar, Mutex, OnceLock};
use std::thread;
use std::time::{Duration, Instant};

use color_eyre::eyre::{Result, WrapErr, bail, eyre};
use fs2::FileExt;
use realfft::RealFftPlanner;

use crate::audio::CaptureInstant;
use crate::dictation::StreamingResampler;
use crate::events::{SmartTurnDecision, VoiceEvent};
use crate::experiments::{self, Experiment};
use crate::pause_segmenter::{FRAME_SAMPLES, SpeechFrames};

pub const SAMPLE_RATE: usize = 16_000;
const WINDOW_SAMPLES: usize = 8 * SAMPLE_RATE;
const FRAME: Duration = Duration::from_millis(20);
/// Speech heard before pauses count, so the shortcut's own click never arms
/// the fallback before someone talks.
const MIN_SPEECH: Duration = Duration::from_millis(200);
/// How much longer than the check point the fallback waits, at least.
const MIN_FALLBACK_LEAD: Duration = Duration::from_millis(500);
const MEL_BINS: usize = 80;
const MEL_FRAMES: usize = WINDOW_SAMPLES / HOP;
const N_FFT: usize = 400;
const HOP: usize = 160;

const MODEL_FILENAME: &str = "smart-turn-v3.2-cpu.onnx";
const MODEL_URL: &str = "https://huggingface.co/pipecat-ai/smart-turn-v3/resolve/f766f81d3cfdf7737ac64aad813d91bbfd56bf93/smart-turn-v3.2-cpu.onnx";
const MODEL_BYTES: u64 = 8_679_182;
const MODEL_SHA256: &str = "2bb026316b14a660486a75b1733cd3fbab8c2fd0314dc9af7be49f8cca967e4f";
const RUNTIME_LIBRARY: &str = "libonnxruntime.1.23.2.dylib";

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Settings {
    /// Silence after speech before the model is asked.
    pub check: Duration,
    /// Probability at or over which the turn is complete.
    pub threshold: f32,
    /// Silence after speech that finishes regardless of the model.
    pub fallback: Duration,
}

impl Settings {
    pub const DEFAULT: Self = Self {
        check: Duration::from_millis(300),
        threshold: 0.7,
        fallback: Duration::from_millis(2_000),
    };

    fn from_lookup(lookup: impl Fn(&str) -> Option<String>) -> Self {
        let mut settings = Self::DEFAULT;
        let milliseconds = |name: &str| {
            let value = lookup(name)?;
            match value.trim().parse::<u64>() {
                Ok(ms) if (20..=60_000).contains(&ms) => Some(Duration::from_millis(ms)),
                _ => {
                    tracing::warn!(name, value, "ignoring invalid Smart Turn setting");
                    None
                }
            }
        };
        if let Some(check) = milliseconds("HEX_SMART_TURN_CHECK_MS") {
            settings.check = check;
        }
        if let Some(fallback) = milliseconds("HEX_SMART_TURN_FALLBACK_MS") {
            settings.fallback = fallback;
        }
        if let Some(value) = lookup("HEX_SMART_TURN_THRESHOLD") {
            match value.trim().parse::<f32>() {
                Ok(threshold) if (0.0..=1.0).contains(&threshold) => {
                    settings.threshold = threshold;
                }
                _ => tracing::warn!(value, "ignoring invalid HEX_SMART_TURN_THRESHOLD"),
            }
        }
        // The model always gets a turn before the plain-silence stop.
        settings.fallback = settings.fallback.max(settings.check + MIN_FALLBACK_LEAD);
        settings
    }
}

/// The settings for this process, read from the environment once.
pub fn settings() -> Settings {
    static SETTINGS: OnceLock<Settings> = OnceLock::new();
    *SETTINGS.get_or_init(|| Settings::from_lookup(|name| std::env::var(name).ok()))
}

/// Whisper log-mel features as Hugging Face's
/// `WhisperFeatureExtractor(chunk_length=8)` computes them with
/// `do_normalize=True`: the last 8 s, left-padded with zeros, normalized to
/// zero mean and unit variance, then an 80-bin Slaney mel spectrogram of a
/// 400-point periodic-Hann STFT with 160-sample hops. Row-major `[80][800]`.
pub fn log_mel(audio: &[f32]) -> Vec<f32> {
    static FILTERS: OnceLock<MelFilters> = OnceLock::new();
    let filters = FILTERS.get_or_init(MelFilters::new);

    let tail = &audio[audio.len().saturating_sub(WINDOW_SAMPLES)..];
    let mut signal = vec![0.0_f64; WINDOW_SAMPLES];
    for (target, sample) in signal[WINDOW_SAMPLES - tail.len()..].iter_mut().zip(tail) {
        *target = if sample.is_finite() {
            f64::from(*sample)
        } else {
            0.0
        };
    }
    let mean = signal.iter().sum::<f64>() / WINDOW_SAMPLES as f64;
    let variance = signal.iter().map(|x| (x - mean).powi(2)).sum::<f64>() / WINDOW_SAMPLES as f64;
    let scale = (variance + 1e-7).sqrt();
    for x in &mut signal {
        // The reference normalizes in float32.
        *x = f64::from(((*x - mean) / scale) as f32);
    }

    // Centered frames with reflect padding.
    let pad = N_FFT / 2;
    let mut padded = Vec::with_capacity(WINDOW_SAMPLES + 2 * pad);
    padded.extend((1..=pad).rev().map(|index| signal[index]));
    padded.extend_from_slice(&signal);
    padded.extend((1..=pad).map(|offset| signal[WINDOW_SAMPLES - 1 - offset]));

    let fft = RealFftPlanner::<f64>::new().plan_fft_forward(N_FFT);
    let mut input = fft.make_input_vec();
    let mut spectrum = fft.make_output_vec();
    let mut power = vec![0.0_f64; N_FFT / 2 + 1];
    let mut mel = vec![0.0_f64; MEL_BINS * MEL_FRAMES];
    // The reference drops the STFT's last frame, so 800 of 801 remain.
    for frame in 0..MEL_FRAMES {
        let start = frame * HOP;
        for ((target, sample), weight) in input
            .iter_mut()
            .zip(&padded[start..start + N_FFT])
            .zip(&filters.window)
        {
            *target = sample * weight;
        }
        fft.process(&mut input, &mut spectrum)
            .expect("FFT buffers have the planned lengths");
        for (target, bin) in power.iter_mut().zip(&spectrum) {
            *target = bin.norm_sqr();
        }
        for (bin, (first, weights)) in filters.bins.iter().enumerate() {
            let energy = weights
                .iter()
                .zip(&power[*first..])
                .map(|(weight, power)| weight * power)
                .sum::<f64>();
            mel[bin * MEL_FRAMES + frame] = energy.max(1e-10).log10();
        }
    }
    let ceiling = mel.iter().copied().fold(f64::NEG_INFINITY, f64::max) - 8.0;
    mel.into_iter()
        .map(|value| ((value.max(ceiling) + 4.0) / 4.0) as f32)
        .collect()
}

struct MelFilters {
    window: Vec<f64>,
    /// Each mel bin's first nonzero FFT bin and its weights from there.
    bins: Vec<(usize, Vec<f64>)>,
}

impl MelFilters {
    fn new() -> Self {
        let window = (0..N_FFT)
            .map(|n| 0.5 - 0.5 * (2.0 * std::f64::consts::PI * n as f64 / N_FFT as f64).cos())
            .collect();
        let fft_bins = N_FFT / 2 + 1;
        let nyquist = SAMPLE_RATE as f64 / 2.0;
        let fft_frequencies = (0..fft_bins)
            .map(|bin| nyquist * bin as f64 / (fft_bins - 1) as f64)
            .collect::<Vec<_>>();
        let low = hertz_to_mel(0.0);
        let high = hertz_to_mel(nyquist);
        let edges = (0..MEL_BINS + 2)
            .map(|index| mel_to_hertz(low + (high - low) * index as f64 / (MEL_BINS + 1) as f64))
            .collect::<Vec<_>>();
        let bins = (0..MEL_BINS)
            .map(|bin| {
                let (left, center, right) = (edges[bin], edges[bin + 1], edges[bin + 2]);
                let slaney = 2.0 / (right - left);
                let weights = fft_frequencies
                    .iter()
                    .map(|frequency| {
                        let rising = (frequency - left) / (center - left);
                        let falling = (right - frequency) / (right - center);
                        rising.min(falling).max(0.0) * slaney
                    })
                    .collect::<Vec<_>>();
                let first = weights.iter().position(|w| *w > 0.0).unwrap_or(0);
                let last = weights.iter().rposition(|w| *w > 0.0).unwrap_or(first);
                (first, weights[first..=last].to_vec())
            })
            .collect();
        Self { window, bins }
    }
}

const MIN_LOG_HERTZ: f64 = 1_000.0;
const MIN_LOG_MEL: f64 = 15.0;

fn hertz_to_mel(hertz: f64) -> f64 {
    if hertz >= MIN_LOG_HERTZ {
        MIN_LOG_MEL + (hertz / MIN_LOG_HERTZ).ln() * (27.0 / 6.4_f64.ln())
    } else {
        3.0 * hertz / 200.0
    }
}

fn mel_to_hertz(mel: f64) -> f64 {
    if mel >= MIN_LOG_MEL {
        MIN_LOG_HERTZ * ((6.4_f64.ln() / 27.0) * (mel - MIN_LOG_MEL)).exp()
    } else {
        200.0 * mel / 3.0
    }
}

/// What the pause tracker wants done.
#[derive(Debug, PartialEq)]
enum Step {
    Check(Check),
    Fallback { silence: Duration },
}

#[derive(Debug, PartialEq)]
struct Check {
    pause: u64,
    silence: Duration,
    audio: Vec<f32>,
}

/// One capture's 16 kHz audio, pauses, and verdicts.
struct Turn {
    settings: Settings,
    audio: VecDeque<f32>,
    speech: SpeechFrames,
    frame: Vec<f32>,
    speech_frames: u32,
    heard_speech: bool,
    silent_frames: u32,
    /// Counts pauses; answers carry the pause they were asked about.
    pause: u64,
    checked: bool,
    stopped: bool,
    /// Whether a pause may end this capture yet: speech while a shortcut
    /// is still held counts, but only a hands-free capture stops.
    armed: bool,
}

impl Turn {
    fn new(settings: Settings) -> Self {
        Self {
            settings,
            audio: VecDeque::with_capacity(WINDOW_SAMPLES),
            speech: SpeechFrames::default(),
            frame: Vec::with_capacity(FRAME_SAMPLES),
            speech_frames: 0,
            heard_speech: false,
            silent_frames: 0,
            pause: 0,
            checked: false,
            stopped: false,
            armed: true,
        }
    }

    fn silence(&self) -> Duration {
        FRAME * self.silent_frames
    }

    fn push(&mut self, mut samples: &[f32]) -> Vec<Step> {
        let mut steps = Vec::new();
        while !samples.is_empty() && !self.stopped {
            let take = samples.len().min(FRAME_SAMPLES - self.frame.len());
            self.frame.extend_from_slice(&samples[..take]);
            samples = &samples[take..];
            if self.frame.len() == FRAME_SAMPLES {
                if let Some(step) = self.complete_frame() {
                    steps.push(step);
                }
                self.frame.clear();
            }
        }
        steps
    }

    fn complete_frame(&mut self) -> Option<Step> {
        if self.audio.len() + FRAME_SAMPLES > WINDOW_SAMPLES {
            self.audio
                .drain(..self.audio.len() + FRAME_SAMPLES - WINDOW_SAMPLES);
        }
        self.audio.extend(&self.frame);
        if self.speech.is_speech(&self.frame) {
            self.speech_frames += 1;
            self.heard_speech |= FRAME * self.speech_frames >= MIN_SPEECH;
            self.silent_frames = 0;
            return None;
        }
        if !self.heard_speech {
            return None;
        }
        self.silent_frames += 1;
        if self.silent_frames == 1 {
            self.pause += 1;
            self.checked = false;
        }
        if !self.armed {
            return None;
        }
        let silence = self.silence();
        if silence >= self.settings.fallback {
            self.stopped = true;
            return Some(Step::Fallback { silence });
        }
        if !self.checked && silence >= self.settings.check {
            self.checked = true;
            return Some(Step::Check(Check {
                pause: self.pause,
                silence,
                audio: self.audio.iter().copied().collect(),
            }));
        }
        None
    }

    /// Silence before the capture became hands-free never counts toward a
    /// stop: the clock starts again at the lock.
    fn set_armed(&mut self, armed: bool) {
        if armed && !self.armed {
            self.silent_frames = 0;
            self.checked = false;
        }
        self.armed = armed;
    }

    /// The verdict on an answer about `pause`.
    fn decide(&mut self, pause: u64, probability: f32) -> SmartTurnDecision {
        if self.stopped || pause != self.pause || self.silent_frames == 0 {
            SmartTurnDecision::Stale
        } else if probability >= self.settings.threshold {
            self.stopped = true;
            SmartTurnDecision::Stop
        } else {
            SmartTurnDecision::Continue
        }
    }
}

/// Scores whether `features` (from [`log_mel`]) end a turn.
pub trait Predictor {
    fn predict(&mut self, features: &[f32]) -> Result<f32>;
}

/// Loads the predictor on the worker thread. It returns early with an error
/// once the flag says the worker closed.
type Loader = Box<dyn FnOnce(&AtomicBool) -> Result<Box<dyn Predictor>> + Send>;

struct Request {
    capture: u64,
    pause: u64,
    silence: Duration,
    audio: Vec<f32>,
}

enum Answer {
    Ready,
    Unavailable(String),
    Predicted {
        capture: u64,
        pause: u64,
        silence: Duration,
        result: Result<f32, String>,
        elapsed: Duration,
    },
}

#[derive(Default)]
struct Slot {
    request: Option<Request>,
    closed: bool,
}

#[derive(Default)]
struct Shared {
    slot: Mutex<Slot>,
    ready: Condvar,
    closed: AtomicBool,
}

impl Shared {
    fn slot(&self) -> std::sync::MutexGuard<'_, Slot> {
        self.slot.lock().unwrap_or_else(|error| error.into_inner())
    }
}

/// The inference thread: one pending request, replaced by newer ones.
struct Worker {
    shared: Arc<Shared>,
    answers: Receiver<Answer>,
    thread: Option<thread::JoinHandle<()>>,
}

impl Worker {
    fn spawn(loader: Loader) -> Result<Self> {
        let shared = Arc::new(Shared::default());
        let (sender, answers) = mpsc::channel();
        let thread_shared = shared.clone();
        let thread = thread::Builder::new()
            .name("smart-turn".into())
            .spawn(move || serve(loader, &thread_shared, &sender))?;
        Ok(Self {
            shared,
            answers,
            thread: Some(thread),
        })
    }

    fn submit(&self, request: Request) {
        self.shared.slot().request = Some(request);
        self.shared.ready.notify_one();
    }

    fn clear(&self) {
        self.shared.slot().request = None;
    }
}

impl Drop for Worker {
    fn drop(&mut self) {
        self.shared.closed.store(true, Ordering::Release);
        {
            let mut slot = self.shared.slot();
            slot.request = None;
            slot.closed = true;
        }
        self.shared.ready.notify_one();
        if let Some(thread) = self.thread.take()
            && thread.join().is_err()
        {
            tracing::error!("Smart Turn worker panicked");
        }
    }
}

fn serve(loader: Loader, shared: &Shared, answers: &Sender<Answer>) {
    let started = Instant::now();
    let mut predictor = match loader(&shared.closed) {
        Ok(predictor) => {
            tracing::info!(
                load_ms = started.elapsed().as_millis() as u64,
                "Smart Turn model ready"
            );
            if answers.send(Answer::Ready).is_err() {
                return;
            }
            predictor
        }
        Err(error) => {
            let _ = answers.send(Answer::Unavailable(format!("{error:#}")));
            return;
        }
    };
    loop {
        let request = {
            let mut slot = shared.slot();
            loop {
                if slot.closed {
                    return;
                }
                if let Some(request) = slot.request.take() {
                    break request;
                }
                slot = shared
                    .ready
                    .wait(slot)
                    .unwrap_or_else(|error| error.into_inner());
            }
        };
        let started = Instant::now();
        let result = predictor
            .predict(&log_mel(&request.audio))
            .map_err(|error| format!("{error:#}"))
            .and_then(|probability| {
                if probability.is_finite() {
                    Ok(probability)
                } else {
                    Err(format!("Smart Turn returned {probability}"))
                }
            });
        let answer = Answer::Predicted {
            capture: request.capture,
            pause: request.pause,
            silence: request.silence,
            result,
            elapsed: started.elapsed(),
        };
        if answers.send(answer).is_err() {
            return;
        }
    }
}

/// One check's outcome, for `live.ndjson`.
#[derive(Clone, Debug, PartialEq)]
pub struct Report {
    pub probability: Option<f32>,
    pub silence_ms: u64,
    pub decision: SmartTurnDecision,
    pub inference_ms: Option<u64>,
}

impl Report {
    pub fn event(&self, timestamp_ms: u64) -> VoiceEvent {
        VoiceEvent::SmartTurn {
            timestamp_ms,
            probability: self.probability,
            silence_ms: self.silence_ms,
            decision: self.decision,
            inference_ms: self.inference_ms,
        }
    }
}

/// The capture Smart Turn follows.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Capture {
    /// The audio owner's capture generation.
    pub generation: u64,
    /// Audio captured before this instant is ignored: the end of a spoken
    /// start phrase, so its tail and queued earlier audio never count.
    pub listen_from: Option<CaptureInstant>,
    /// Whether a pause may end the capture now (it records hands-free).
    pub armed: bool,
}

struct Session {
    capture: u64,
    turn: Turn,
    /// Audio before this instant was ignored or already heard.
    heard_through: Option<CaptureInstant>,
    resampler: Option<(u32, Option<StreamingResampler>)>,
    stop: bool,
}

/// Smart Turn for the recognition loop: tracks the active capture, feeds
/// its audio, and reports checks and stop decisions.
pub struct SmartTurn {
    settings: Settings,
    loader: Option<Loader>,
    worker: Option<Worker>,
    ready: bool,
    unavailable: bool,
    session: Option<Session>,
    reports: Vec<Report>,
}

impl SmartTurn {
    /// `None` unless the `smart-turn` experiment is on. Nothing downloads,
    /// loads, or runs until the first capture.
    pub fn start(project_root: &Path) -> Option<Self> {
        Self::start_if(
            experiments::enabled(Experiment::SmartTurn),
            settings,
            || {
                let project_root = project_root.to_path_buf();
                Box::new(move |closed| load_onnx(&project_root, closed))
            },
        )
    }

    /// Reads nothing and allocates nothing while the experiment is off.
    fn start_if(
        enabled: bool,
        settings: impl FnOnce() -> Settings,
        loader: impl FnOnce() -> Loader,
    ) -> Option<Self> {
        enabled.then(|| Self::new(settings(), loader()))
    }

    fn new(settings: Settings, loader: Loader) -> Self {
        tracing::info!(?settings, "Smart Turn experiment is on");
        Self {
            settings,
            loader: Some(loader),
            worker: None,
            ready: false,
            unavailable: false,
            session: None,
            reports: Vec::new(),
        }
    }

    /// Follows the capture that records now, or `None` when none does. The
    /// first capture starts loading the model; captures are followed only
    /// once it is ready, so nothing stops a capture before the model can.
    pub fn track(&mut self, capture: Option<Capture>) {
        let Some(capture) = capture.filter(|_| !self.unavailable) else {
            if self.session.take().is_some()
                && let Some(worker) = &self.worker
            {
                worker.clear();
            }
            return;
        };
        if let Some(loader) = self.loader.take() {
            match Worker::spawn(loader) {
                Ok(worker) => self.worker = Some(worker),
                Err(error) => {
                    tracing::error!(%error, "could not start Smart Turn");
                    self.unavailable = true;
                    return;
                }
            }
        }
        if !self.ready {
            return;
        }
        match &mut self.session {
            Some(session) if session.capture == capture.generation => {
                session.turn.set_armed(capture.armed);
            }
            _ => {
                if let Some(worker) = &self.worker {
                    worker.clear();
                }
                let mut turn = Turn::new(self.settings);
                turn.set_armed(capture.armed);
                self.session = Some(Session {
                    capture: capture.generation,
                    turn,
                    heard_through: capture.listen_from,
                    resampler: None,
                    stop: false,
                });
            }
        }
    }

    /// Feeds microphone audio that began at `captured_from`. Audio before
    /// the capture's listening point, or already heard, is skipped.
    pub fn push(&mut self, samples: &[f32], sample_rate: u32, captured_from: CaptureInstant) {
        let Some(session) = &mut self.session else {
            return;
        };
        let skip = session.heard_through.map_or(0, |heard| {
            let early = heard
                .checked_duration_since(captured_from)
                .unwrap_or_default();
            (early.as_secs_f64() * f64::from(sample_rate)).round() as usize
        });
        let captured_through = captured_from.checked_add(crate::audio::duration_for_frames(
            samples.len(),
            sample_rate,
        ));
        if let Some(through) = captured_through
            && session.heard_through.is_none_or(|heard| through > heard)
        {
            session.heard_through = Some(through);
        }
        let Some(samples) = samples.get(skip..).filter(|samples| !samples.is_empty()) else {
            return;
        };
        if session
            .resampler
            .as_ref()
            .is_none_or(|(rate, _)| *rate != sample_rate)
        {
            let resampler = if sample_rate as usize == SAMPLE_RATE {
                None
            } else {
                match StreamingResampler::new(sample_rate) {
                    Ok(resampler) => Some(resampler),
                    Err(error) => {
                        tracing::warn!(%error, sample_rate, "Smart Turn cannot resample this microphone");
                        self.session = None;
                        return;
                    }
                }
            };
            session.resampler = Some((sample_rate, resampler));
        }
        let steps = match &mut session.resampler {
            Some((_, Some(resampler))) => {
                let resampled = resampler.push(samples);
                session.turn.push(&resampled)
            }
            _ => session.turn.push(samples),
        };
        for step in steps {
            match step {
                Step::Check(check) => {
                    if let Some(worker) = &self.worker {
                        worker.submit(Request {
                            capture: session.capture,
                            pause: check.pause,
                            silence: check.silence,
                            audio: check.audio,
                        });
                    }
                }
                Step::Fallback { silence } => {
                    session.stop = true;
                    self.reports.push(Report {
                        probability: None,
                        silence_ms: silence.as_millis() as u64,
                        decision: SmartTurnDecision::Fallback,
                        inference_ms: None,
                    });
                }
            }
        }
    }

    /// Checks finished since the last poll, oldest first.
    pub fn poll(&mut self) -> Vec<Report> {
        while let Some(answer) = self
            .worker
            .as_ref()
            .and_then(|worker| worker.answers.try_recv().ok())
        {
            match answer {
                Answer::Ready => self.ready = true,
                Answer::Unavailable(error) => {
                    tracing::error!(%error, "Smart Turn is unavailable; dictation continues without it");
                    self.unavailable = true;
                    self.worker = None;
                    self.session = None;
                }
                Answer::Predicted {
                    capture,
                    pause,
                    silence,
                    result,
                    elapsed,
                } => {
                    let (probability, decision) = match result {
                        Ok(probability) => (
                            Some(probability),
                            match &mut self.session {
                                Some(session) if session.capture == capture => {
                                    let decision = session.turn.decide(pause, probability);
                                    session.stop |= decision == SmartTurnDecision::Stop;
                                    decision
                                }
                                _ => SmartTurnDecision::Stale,
                            },
                        ),
                        Err(error) => {
                            tracing::warn!(%error, "Smart Turn inference failed");
                            (None, SmartTurnDecision::Failed)
                        }
                    };
                    self.reports.push(Report {
                        probability,
                        silence_ms: silence.as_millis() as u64,
                        decision,
                        inference_ms: Some(elapsed.as_millis() as u64),
                    });
                }
            }
        }
        std::mem::take(&mut self.reports)
    }

    /// Whether the followed capture should finish now. The request stays
    /// until taken or the capture ends, so a caller that must first handle
    /// queued input can take it on a later turn.
    pub fn take_stop(&mut self) -> bool {
        self.session
            .as_mut()
            .is_some_and(|session| std::mem::take(&mut session.stop))
    }
}

struct OnnxPredictor {
    session: ort::session::Session,
}

impl Predictor for OnnxPredictor {
    fn predict(&mut self, features: &[f32]) -> Result<f32> {
        let input = ort::value::Tensor::from_array((
            [1_usize, MEL_BINS, MEL_FRAMES],
            features.to_vec().into_boxed_slice(),
        ))
        .map_err(|error| eyre!("{error}"))?;
        let outputs = self
            .session
            .run(ort::inputs!["input_features" => input])
            .map_err(|error| eyre!("{error}"))?;
        let (_, probabilities) = outputs[0]
            .try_extract_tensor::<f32>()
            .map_err(|error| eyre!("{error}"))?;
        probabilities
            .first()
            .copied()
            .ok_or_else(|| eyre!("Smart Turn returned no probability"))
    }
}

fn load_onnx(project_root: &Path, closed: &AtomicBool) -> Result<Box<dyn Predictor>> {
    // Without the runtime the model is useless, so it isn't downloaded.
    init_runtime(&find_runtime(project_root)?)?;
    let model = install_model(closed)?;
    let builder_error = |error: ort::Error<ort::session::builder::SessionBuilder>| eyre!("{error}");
    let session = ort::session::Session::builder()
        .map_err(|error| eyre!("{error}"))?
        .with_optimization_level(ort::session::builder::GraphOptimizationLevel::All)
        .map_err(builder_error)?
        .with_parallel_execution(false)
        .map_err(builder_error)?
        .with_intra_threads(1)
        .map_err(builder_error)?
        .with_inter_threads(1)
        .map_err(builder_error)?
        .with_intra_op_spinning(false)
        .map_err(builder_error)?
        .commit_from_file(&model)
        .map_err(|error| eyre!("could not load {}: {error}", model.display()))?;
    Ok(Box::new(OnnxPredictor { session }))
}

/// Loads the ONNX Runtime library once per process.
fn init_runtime(library: &Path) -> Result<()> {
    static RUNTIME: OnceLock<Result<(), String>> = OnceLock::new();
    RUNTIME
        .get_or_init(|| {
            ort::init_from(library)
                .map(|environment| {
                    let _ = environment.with_name("hex-smart-turn").commit();
                })
                .map_err(|error| format!("could not load {}: {error}", library.display()))
        })
        .clone()
        .map_err(|error| eyre!(error))
}

fn find_runtime(project_root: &Path) -> Result<PathBuf> {
    if let Ok(executable) = std::env::current_exe()
        && let Some(contents) = executable.parent().and_then(Path::parent)
    {
        let bundled = contents.join("Frameworks").join(RUNTIME_LIBRARY);
        if bundled.exists() {
            return Ok(bundled);
        }
    }
    let candidate = project_root
        .join("target/onnxruntime")
        .join(RUNTIME_LIBRARY);
    if candidate.exists() {
        return Ok(candidate);
    }
    bail!("{RUNTIME_LIBRARY} not found; run ./scripts/setup.sh")
}

/// The pinned model in Application Support, downloaded and verified on
/// first use. The download stops once `closed` is set.
fn install_model(closed: &AtomicBool) -> Result<PathBuf> {
    let directory = crate::transcription_models::models_dir()?;
    fs::create_dir_all(&directory)?;
    let lock = File::create(directory.join(".smart-turn.lock"))?;
    lock.lock_exclusive()?;
    let destination = directory.join(MODEL_FILENAME);
    if verify_model(&destination).is_ok() {
        return Ok(destination);
    }
    let partial = directory.join(format!("{MODEL_FILENAME}.partial"));
    if fs::metadata(&partial).is_ok_and(|metadata| metadata.len() >= MODEL_BYTES) {
        fs::remove_file(&partial)?;
    }
    tracing::info!("downloading the Smart Turn model");
    let mut child = Command::new("/usr/bin/curl")
        .args([
            "--fail",
            "--location",
            "--retry",
            "3",
            "--connect-timeout",
            "15",
            "--max-time",
            "300",
            "--continue-at",
            "-",
            "--silent",
            "--show-error",
            "--output",
        ])
        .arg(&partial)
        .arg(MODEL_URL)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::piped())
        .spawn()
        .wrap_err("could not start the Smart Turn model download")?;
    let status = loop {
        if closed.load(Ordering::Acquire) {
            let _ = child.kill();
            let _ = child.wait();
            bail!("Smart Turn closed during the model download");
        }
        if let Some(status) = child.try_wait()? {
            break status;
        }
        thread::sleep(Duration::from_millis(100));
    };
    if !status.success() {
        let mut stderr = String::new();
        if let Some(mut pipe) = child.stderr.take() {
            let _ = pipe.read_to_string(&mut stderr);
        }
        bail!(
            "could not download the Smart Turn model ({status}): {}",
            stderr.trim()
        );
    }
    if let Err(error) = verify_model(&partial) {
        let _ = fs::remove_file(&partial);
        return Err(error);
    }
    File::open(&partial)?.sync_all()?;
    fs::rename(&partial, &destination)?;
    crate::transcription_models::sync_directory(&directory)?;
    Ok(destination)
}

fn verify_model(path: &Path) -> Result<()> {
    let bytes = fs::metadata(path)?.len();
    if bytes != MODEL_BYTES {
        bail!("Smart Turn model has {bytes} bytes, expected {MODEL_BYTES}");
    }
    if crate::transcription_models::sha256_hex(path, None)? != MODEL_SHA256 {
        bail!("Smart Turn model checksum mismatch");
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::pause_segmenter::tests::utterance;

    /// The reference signal `ref.py` fed `WhisperFeatureExtractor`: 2.5 s of
    /// a modulated 220 Hz tone plus 1250 Hz, then 0.5 s of a faint 50 Hz hum.
    fn reference_signal() -> Vec<f32> {
        (0..3 * SAMPLE_RATE)
            .map(|n| {
                let t = n as f64 / SAMPLE_RATE as f64;
                let tau = 2.0 * std::f64::consts::PI;
                let value = if t < 2.5 {
                    0.3 * (tau * 220.0 * t).sin() * (0.5 + 0.5 * (tau * 3.0 * t).sin())
                        + 0.1 * (tau * 1250.0 * t).sin()
                } else {
                    0.001 * (tau * 50.0 * t).sin()
                };
                value as f32
            })
            .collect()
    }

    #[test]
    fn log_mel_matches_whisper_feature_extractor() {
        let features = log_mel(&reference_signal());
        assert_eq!(features.len(), MEL_BINS * MEL_FRAMES);
        // transformers' WhisperFeatureExtractor(chunk_length=8) on the same signal.
        let picks = [
            (2, 520, 0.712_121_6),
            (5, 600, 1.688_267),
            (10, 640, 0.636_762_7),
            (15, 700, 0.017_422_2),
            (30, 745, 1.278_610_7),
            (60, 600, -0.161_787_63),
            (79, 700, -0.161_787_63),
            (1, 780, 0.560_602_3),
            (0, 0, -0.161_787_63),
            (0, 799, 0.632_712_4),
        ];
        for (bin, frame, expected) in picks {
            let actual = features[bin * MEL_FRAMES + frame];
            assert!(
                (actual - expected).abs() < 1e-3,
                "bin {bin} frame {frame}: {actual} != {expected}"
            );
        }
        let mean = features.iter().map(|x| f64::from(*x)).sum::<f64>() / features.len() as f64;
        let std = (features
            .iter()
            .map(|x| (f64::from(*x) - mean).powi(2))
            .sum::<f64>()
            / features.len() as f64)
            .sqrt();
        assert!((mean - -0.057_343_021).abs() < 1e-4, "mean {mean}");
        assert!((std - 0.337_150_69).abs() < 1e-4, "std {std}");
    }

    #[test]
    fn settings_read_overrides_and_ignore_nonsense() {
        assert_eq!(Settings::from_lookup(|_| None), Settings::DEFAULT);
        let veto = Settings::from_lookup(|name| {
            match name {
                "HEX_SMART_TURN_CHECK_MS" => Some("1500"),
                "HEX_SMART_TURN_THRESHOLD" => Some("0.8"),
                "HEX_SMART_TURN_FALLBACK_MS" => Some("3000"),
                _ => None,
            }
            .map(Into::into)
        });
        assert_eq!(
            veto,
            Settings {
                check: Duration::from_millis(1_500),
                threshold: 0.8,
                fallback: Duration::from_millis(3_000),
            }
        );
        let nonsense = Settings::from_lookup(|name| {
            match name {
                "HEX_SMART_TURN_CHECK_MS" => Some("soon"),
                "HEX_SMART_TURN_THRESHOLD" => Some("2"),
                "HEX_SMART_TURN_FALLBACK_MS" => Some("5"),
                _ => None,
            }
            .map(Into::into)
        });
        assert_eq!(nonsense, Settings::DEFAULT);
    }

    fn checks(steps: Vec<Step>) -> Vec<Check> {
        steps
            .into_iter()
            .map(|step| match step {
                Step::Check(check) => check,
                step => panic!("expected a check, got {step:?}"),
            })
            .collect()
    }

    #[test]
    fn checks_once_per_pause_after_speech() {
        let mut turn = Turn::new(Settings::DEFAULT);
        // Speech then 290 ms of quiet: not yet.
        assert!(turn.push(&utterance(&[(1_000, 290)], -70.0)).is_empty());
        let mut seed = 1;
        let quiet = crate::pause_segmenter::tests::noise(20, -70.0, &mut seed);
        let found = checks(turn.push(&quiet));
        assert_eq!(found.len(), 1);
        assert_eq!(found[0].silence, Duration::from_millis(300));
        assert_eq!(found[0].audio.len(), 1_300 * SAMPLE_RATE / 1_000);
        // The same pause is never asked about twice.
        let quiet = crate::pause_segmenter::tests::noise(1_000, -70.0, &mut seed);
        assert!(turn.push(&quiet).is_empty());
        assert_eq!(
            turn.decide(found[0].pause, 0.2),
            SmartTurnDecision::Continue
        );
    }

    #[test]
    fn silence_alone_never_checks_or_stops() {
        let mut turn = Turn::new(Settings::DEFAULT);
        let mut seed = 2;
        let audio = crate::pause_segmenter::tests::noise(10_000, -60.0, &mut seed);
        assert!(turn.push(&audio).is_empty());
        // A click shorter than a syllable doesn't arm the fallback either.
        assert!(turn.push(&utterance(&[(100, 3_000)], -70.0)).is_empty());
    }

    #[test]
    fn a_confident_answer_stops_and_a_doubtful_one_waits_for_the_fallback() {
        let mut turn = Turn::new(Settings::DEFAULT);
        let check = checks(turn.push(&utterance(&[(1_000, 300)], -70.0))).remove(0);
        assert_eq!(turn.decide(check.pause, 0.71), SmartTurnDecision::Stop);
        // Stopped: later audio and answers do nothing.
        assert!(turn.push(&utterance(&[(500, 3_000)], -70.0)).is_empty());
        assert_eq!(turn.decide(check.pause, 0.99), SmartTurnDecision::Stale);

        let mut turn = Turn::new(Settings::DEFAULT);
        let check = checks(turn.push(&utterance(&[(1_000, 300)], -70.0))).remove(0);
        assert_eq!(turn.decide(check.pause, 0.69), SmartTurnDecision::Continue);
        let mut seed = 3;
        let steps = turn.push(&crate::pause_segmenter::tests::noise(
            2_000, -70.0, &mut seed,
        ));
        assert_eq!(
            steps,
            [Step::Fallback {
                silence: Duration::from_millis(2_000)
            }]
        );
    }

    #[test]
    fn answers_about_an_earlier_pause_are_stale() {
        let mut turn = Turn::new(Settings::DEFAULT);
        let first = checks(turn.push(&utterance(&[(1_000, 300)], -70.0))).remove(0);
        // Speech resumes before the answer arrives.
        assert!(turn.push(&utterance(&[(500, 0)], -70.0)).is_empty());
        assert_eq!(turn.decide(first.pause, 0.99), SmartTurnDecision::Stale);
        let second = checks(turn.push(&utterance(&[(0, 300)], -70.0))).remove(0);
        assert!(second.pause > first.pause);
        assert_eq!(turn.decide(first.pause, 0.99), SmartTurnDecision::Stale);
        assert_eq!(turn.decide(second.pause, 0.99), SmartTurnDecision::Stop);
    }

    #[test]
    fn the_model_is_always_asked_before_the_fallback() {
        let settings = Settings::from_lookup(|name| {
            match name {
                "HEX_SMART_TURN_CHECK_MS" => Some("1500"),
                "HEX_SMART_TURN_FALLBACK_MS" => Some("1000"),
                _ => None,
            }
            .map(Into::into)
        });
        assert_eq!(settings.fallback, Duration::from_millis(2_000));
        let mut turn = Turn::new(settings);
        let steps = turn.push(&utterance(&[(1_000, 2_000)], -70.0));
        assert_eq!(steps.len(), 2, "{steps:?}");
        assert!(matches!(&steps[0], Step::Check(check) if check.silence == settings.check));
        assert_eq!(
            steps[1],
            Step::Fallback {
                silence: Duration::from_millis(2_000)
            }
        );
    }

    /// Answers each request with the next scripted probability, only once
    /// the test opens the gate.
    struct Scripted {
        gate: Receiver<f32>,
        asked: Arc<AtomicBool>,
    }

    impl Predictor for Scripted {
        fn predict(&mut self, features: &[f32]) -> Result<f32> {
            assert_eq!(features.len(), MEL_BINS * MEL_FRAMES);
            self.asked.store(true, Ordering::SeqCst);
            Ok(self.gate.recv()?)
        }
    }

    struct Harness {
        // Dropped first, so a worker waiting on the gate can exit.
        gate: Sender<f32>,
        smart_turn: SmartTurn,
        asked: Arc<AtomicBool>,
        /// Where the next fed audio begins.
        at: CaptureInstant,
    }

    const START: CaptureInstant = CaptureInstant::from_nanos(10_000_000_000);

    fn capture(generation: u64) -> Capture {
        Capture {
            generation,
            listen_from: None,
            armed: true,
        }
    }

    /// A Smart Turn whose model loads once `loaded` receives a result.
    fn gated(loaded: Receiver<Result<()>>) -> Harness {
        let (gate, receiver) = mpsc::channel();
        let asked = Arc::new(AtomicBool::new(false));
        let predictor = Scripted {
            gate: receiver,
            asked: asked.clone(),
        };
        let smart_turn = SmartTurn::new(
            Settings::DEFAULT,
            Box::new(move |_: &AtomicBool| {
                loaded.recv()??;
                Ok(Box::new(predictor) as Box<dyn Predictor>)
            }),
        );
        Harness {
            smart_turn,
            gate,
            asked,
            at: START,
        }
    }

    fn scripted() -> Harness {
        let (load, loaded) = mpsc::channel();
        load.send(Ok(())).unwrap();
        gated(loaded)
    }

    fn eventually(what: &str, mut done: impl FnMut() -> bool) {
        let deadline = Instant::now() + Duration::from_secs(5);
        while !done() {
            assert!(Instant::now() < deadline, "timed out waiting for {what}");
            thread::sleep(Duration::from_millis(5));
        }
    }

    impl Harness {
        /// Tracks `capture` until the model is ready and a session follows it.
        fn follow(&mut self, capture: Capture) {
            let smart_turn = &mut self.smart_turn;
            eventually("the model", || {
                smart_turn.track(Some(capture));
                assert!(smart_turn.poll().is_empty());
                smart_turn.session.is_some()
            });
        }

        /// Feeds `audio` at `rate` in 10 ms chunks, continuing the timeline.
        fn feed(&mut self, audio: &[f32], rate: u32) {
            for chunk in audio.chunks(rate as usize / 100) {
                self.smart_turn.push(chunk, rate, self.at);
                self.at = self
                    .at
                    .checked_add(crate::audio::duration_for_frames(chunk.len(), rate))
                    .unwrap();
            }
        }

        fn wait_until_asked(&self) {
            eventually("a check", || self.asked.swap(false, Ordering::SeqCst));
        }

        fn reports(&mut self) -> Vec<Report> {
            let mut reports = Vec::new();
            let smart_turn = &mut self.smart_turn;
            eventually("a report", || {
                reports = smart_turn.poll();
                !reports.is_empty()
            });
            reports
        }
    }

    #[test]
    fn stops_a_hands_free_capture_when_the_model_hears_the_end() {
        let mut harness = scripted();
        harness.follow(capture(7));
        // 48 kHz microphone audio is resampled before the speech rule.
        let audio = utterance(&[(1_500, 400)], -70.0)
            .into_iter()
            .flat_map(|sample| [sample; 3])
            .collect::<Vec<_>>();
        harness.feed(&audio, 48_000);
        harness.gate.send(0.9).unwrap();
        let reports = harness.reports();
        assert_eq!(reports.len(), 1);
        assert_eq!(reports[0].decision, SmartTurnDecision::Stop);
        assert_eq!(reports[0].probability, Some(0.9));
        assert_eq!(reports[0].silence_ms, 300);
        assert!(harness.smart_turn.take_stop());
        assert!(!harness.smart_turn.take_stop());
    }

    #[test]
    fn an_answer_for_an_ended_or_newer_capture_does_nothing() {
        let mut harness = scripted();
        harness.follow(capture(1));
        harness.feed(&utterance(&[(1_000, 300)], -70.0), 16_000);
        harness.wait_until_asked();
        // The capture ends and a new one starts before the answer.
        harness.smart_turn.track(None);
        harness.smart_turn.track(Some(capture(2)));
        harness.gate.send(0.99).unwrap();
        let reports = harness.reports();
        assert_eq!(reports.len(), 1);
        assert_eq!(reports[0].decision, SmartTurnDecision::Stale);
        assert!(!harness.smart_turn.take_stop());
    }

    #[test]
    fn an_ended_capture_drops_its_waiting_check() {
        let mut harness = scripted();
        harness.follow(capture(1));
        harness.feed(&utterance(&[(1_000, 300)], -70.0), 16_000);
        harness.wait_until_asked();
        // A second pause waits behind the first, then the capture ends.
        harness.feed(&utterance(&[(500, 300)], -70.0), 16_000);
        harness.smart_turn.track(None);
        harness.gate.send(0.99).unwrap();
        harness.gate.send(0.99).unwrap();
        assert_eq!(harness.reports().len(), 1);
        thread::sleep(Duration::from_millis(100));
        assert!(harness.smart_turn.poll().is_empty());
        assert!(!harness.asked.load(Ordering::SeqCst));
    }

    #[test]
    fn nothing_is_followed_or_stopped_before_the_model_is_ready() {
        let (load, loaded) = mpsc::channel();
        let mut harness = gated(loaded);
        harness.smart_turn.track(Some(capture(1)));
        // Speech and a long pause while the model downloads.
        harness.feed(&utterance(&[(1_000, 3_000)], -70.0), 16_000);
        assert!(harness.smart_turn.poll().is_empty());
        assert!(harness.smart_turn.session.is_none());
        assert!(!harness.smart_turn.take_stop());
        load.send(Ok(())).unwrap();
        harness.follow(capture(1));
        harness.feed(&utterance(&[(1_000, 300)], -70.0), 16_000);
        harness.wait_until_asked();
    }

    #[test]
    fn a_failed_load_leaves_dictation_alone() {
        let (load, loaded) = mpsc::channel();
        let mut harness = gated(loaded);
        harness.smart_turn.track(Some(capture(1)));
        harness.feed(&utterance(&[(1_000, 3_000)], -70.0), 16_000);
        load.send(Err(eyre!("no runtime"))).unwrap();
        let smart_turn = &mut harness.smart_turn;
        eventually("the failure", || {
            assert!(smart_turn.poll().is_empty());
            smart_turn.unavailable
        });
        // No fallback either: dictation behaves as if the experiment were off.
        harness.smart_turn.track(Some(capture(1)));
        harness.feed(&utterance(&[(1_000, 3_000)], -70.0), 16_000);
        assert!(harness.smart_turn.poll().is_empty());
        assert!(!harness.smart_turn.take_stop());
        assert!(harness.smart_turn.worker.is_none());
    }

    #[test]
    fn speech_while_held_counts_but_only_a_hands_free_capture_stops() {
        let mut harness = scripted();
        let mut held = capture(1);
        held.armed = false;
        harness.follow(held);
        // Speech and a long pause while the shortcut is still down.
        harness.feed(&utterance(&[(1_000, 2_500)], -70.0), 16_000);
        assert!(harness.smart_turn.poll().is_empty());
        assert!(!harness.smart_turn.take_stop());
        assert!(!harness.asked.load(Ordering::SeqCst));
        // Locked: the speech already heard counts, the silence starts over.
        harness.smart_turn.track(Some(capture(1)));
        let mut seed = 4;
        let quiet = crate::pause_segmenter::tests::noise(280, -70.0, &mut seed);
        harness.feed(&quiet, 16_000);
        thread::sleep(Duration::from_millis(50));
        assert!(!harness.asked.load(Ordering::SeqCst));
        harness.feed(
            &crate::pause_segmenter::tests::noise(20, -70.0, &mut seed),
            16_000,
        );
        harness.wait_until_asked();
        harness.gate.send(0.9).unwrap();
        assert_eq!(harness.reports()[0].decision, SmartTurnDecision::Stop);
        assert!(harness.smart_turn.take_stop());
    }

    #[test]
    fn listening_starts_after_the_start_phrase() {
        let mut harness = scripted();
        harness.follow(Capture {
            listen_from: START.checked_add(Duration::from_millis(1_505)),
            ..capture(1)
        });
        // The start phrase and the pause after it come before the listening
        // point, so neither counts.
        harness.feed(&utterance(&[(1_000, 3_000)], -70.0), 16_000);
        thread::sleep(Duration::from_millis(50));
        assert!(harness.smart_turn.poll().is_empty());
        assert!(!harness.asked.load(Ordering::SeqCst));
        assert!(!harness.smart_turn.take_stop());
        // Replayed audio is already heard.
        let replay = harness.at;
        harness.at = START;
        harness.feed(&utterance(&[(1_000, 3_000)], -70.0), 16_000);
        assert_eq!(harness.at, replay);
        assert!(harness.smart_turn.poll().is_empty());
        // Listening began mid-frame, so frames straddle the speech's end.
        harness.feed(&utterance(&[(1_000, 400)], -70.0), 16_000);
        harness.wait_until_asked();
    }

    #[test]
    fn a_non_finite_probability_fails_the_check() {
        let mut harness = scripted();
        harness.follow(capture(1));
        harness.feed(&utterance(&[(1_000, 300)], -70.0), 16_000);
        harness.gate.send(f32::NAN).unwrap();
        let reports = harness.reports();
        assert_eq!(reports[0].decision, SmartTurnDecision::Failed);
        assert_eq!(reports[0].probability, None);
        assert!(!harness.smart_turn.take_stop());
    }

    #[test]
    fn non_finite_samples_read_as_silence() {
        let mut audio = reference_signal();
        let clean = log_mel(&audio);
        audio[100] = f32::NAN;
        audio[200] = f32::INFINITY;
        let features = log_mel(&audio);
        assert!(features.iter().all(|value| value.is_finite()));
        assert_eq!(features.len(), clean.len());
    }

    #[test]
    fn off_reads_and_loads_nothing() {
        let smart_turn = SmartTurn::start_if(
            false,
            || panic!("read settings while off"),
            || panic!("built a loader while off"),
        );
        assert!(smart_turn.is_none());
    }

    /// Runs the real model on the reference signal. Needs the ONNX Runtime
    /// dylib from `./scripts/setup.sh` and network for the first download.
    #[test]
    #[ignore = "downloads the model and needs the ONNX Runtime dylib"]
    fn real_model_matches_python_on_the_reference_signal() {
        let root = std::env::var_os("HEX_PROJECT_ROOT")
            .map_or_else(|| PathBuf::from(env!("CARGO_MANIFEST_DIR")), PathBuf::from);
        let mut predictor = load_onnx(&root, &AtomicBool::new(false)).unwrap();
        let probability = predictor.predict(&log_mel(&reference_signal())).unwrap();
        // Python onnxruntime 1.23.2 with every graph optimization, on the same
        // features: 0.8642676472663879. The quantized model's score moves with
        // the runtime version (1.31.0 gives 0.8857588768005371).
        assert!((probability - 0.864_267_6).abs() < 1e-4, "P {probability}");
    }
}
