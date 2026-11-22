//! The `transcribe-cpp` session owner shared by the macOS, Linux, and Windows
//! GGUF transcribers. It recreates the native session around every run (and
//! every live stream) so retained scheduler scratch never outlives the
//! inference that sized it.

use std::borrow::Cow;
use std::cell::RefCell;
use std::ops::Range;
use std::path::Path;
use std::time::{Duration, Instant};

use color_eyre::eyre::{Result, WrapErr, eyre};

use crate::transcription::models::{
    AUTO_LANGUAGE, ModelDefinition, ModelRuntime, TranscriptionSelection,
};
use crate::transcription::stream_tuning::{StreamCache, StreamMenus, StreamTuning};
use crate::transcription::transcript::TimestampLevel;

/// 8 s of audio reaches every stream window shape a dictation can use (the
/// left context grows to 5.6 s), so the GPU driver compiles those kernels
/// during launch. With a 1 s probe, a new build's first dictation compiled
/// them in its finish step and took 3-4 s on an RTX 5070 Ti.
const STREAM_PREWARM_SAMPLES: usize = 8 * 16_000;

thread_local! {
    static JOB_CANCEL: RefCell<Option<transcribe_cpp::CancelToken>> = const { RefCell::new(None) };
}

/// Runs `work` with `cancel` installed on every offline run it makes on this
/// thread, so cancelling one engine job aborts its model between decode steps
/// and between chunks. The engine's offline closures reach the session through
/// model types that know nothing of jobs.
pub(crate) fn with_job_cancel<R>(
    cancel: &transcribe_cpp::CancelToken,
    work: impl FnOnce() -> R,
) -> R {
    struct Restore(Option<transcribe_cpp::CancelToken>);
    impl Drop for Restore {
        fn drop(&mut self) {
            JOB_CANCEL.with(|slot| *slot.borrow_mut() = self.0.take());
        }
    }
    let _restore = Restore(JOB_CANCEL.with(|slot| slot.borrow_mut().replace(cancel.clone())));
    work()
}

pub(crate) fn job_cancel() -> Option<transcribe_cpp::CancelToken> {
    JOB_CANCEL.with(|slot| slot.borrow().clone())
}

pub(crate) fn validate_gguf_artifact(
    path: &Path,
    definition: &ModelDefinition,
    architecture: &str,
    variant: &str,
    selection: &TranscriptionSelection,
    supports_language_detect: bool,
) -> Result<Option<String>> {
    let ModelRuntime::Gguf(artifact) = definition.runtime else {
        return Err(eyre!(
            "{} is not a GGUF transcription model",
            definition.name
        ));
    };
    if architecture != artifact.architecture || variant != artifact.variant {
        return Err(eyre!(
            "{} contains {architecture}/{variant}, expected {}/{}",
            path.display(),
            artifact.architecture,
            artifact.variant
        ));
    }
    // Auto gates a hint, so it needs detection only from a model that takes
    // one: a no-hint head (Parakeet v3, Granite) hears the language itself,
    // whatever its file's lang_detect metadata says.
    if selection.language == AUTO_LANGUAGE
        && definition.accepts_language_hint
        && !supports_language_detect
    {
        return Err(eyre!(
            "{} does not advertise automatic language detection",
            definition.name
        ));
    }
    Ok(definition
        .runtime_language_hint(&selection.language)
        .map(str::to_string))
}

// On GPU devices only the per-token decoder runs on CPU; extra spin-waiting ggml
// workers (default up to 8) stall it under load, so keep it on the calling thread.
fn session_options(device_type: transcribe_cpp::DeviceType) -> transcribe_cpp::SessionOptions {
    match device_type {
        transcribe_cpp::DeviceType::Gpu | transcribe_cpp::DeviceType::Igpu => {
            transcribe_cpp::SessionOptions {
                n_threads: 1,
                ..Default::default()
            }
        }
        _ => transcribe_cpp::SessionOptions::default(),
    }
}

/// Why one whole-clip run happens.
#[derive(Clone, Copy)]
enum RunKind {
    /// A transcript for dictation, a meeting, or the local API.
    Transcript,
    /// A warm-up: the weights come back onto the device before audio arrives.
    Warmup,
}

impl RunKind {
    /// Whether this run adds the configured vocabulary. A warm-up must not:
    /// silence plus a long term list makes Qwen3-ASR read the list back to
    /// its 256-token cap on every warm-up, which costs seconds and warms
    /// nothing extra (#1568). Live streams already skip the prompt.
    fn takes_vocabulary(self) -> bool {
        matches!(self, Self::Transcript)
    }

    /// Whether a decode that stopped at its output limit still counts as a
    /// run. A warm-up ran the whole graph on the device either way, so a
    /// follow-up warm-up need not repeat it; a failed transcript does not
    /// count.
    fn counts_when_truncated(self) -> bool {
        matches!(self, Self::Warmup)
    }
}

/// How a session feeds its model the vocabulary.
#[derive(Clone, Copy, Debug)]
enum VocabularySlot {
    /// Parakeet's transducer heads, as a run extension (Labs).
    Bias,
    /// The context prompt (Whisper, Qwen3-ASR), bounded to this many characters.
    Prompt(usize),
    None,
}

impl VocabularySlot {
    fn of(model: &transcribe_cpp::Model) -> Self {
        let architecture = model.arch();
        if architecture == "parakeet"
            && model.accepts_ext(
                transcribe_cpp::ExtSlot::Run,
                transcribe_cpp::sys::TRANSCRIBE_EXT_KIND_PARAKEET_VOCABULARY,
            )
        {
            return Self::Bias;
        }
        if !model.supports(transcribe_cpp::Feature::ContextPrompt) {
            return Self::None;
        }
        crate::dictation::vocabulary::prompt_chars(&architecture).map_or(Self::None, Self::Prompt)
    }
}

/// Whether a finished run corrects the listed "sounds like" spellings after
/// it: only a whole-clip run on a model that read the vocabulary as a prompt
/// (Whisper, Qwen3-ASR). Parakeet's bias already swaps the exact spelling in
/// the engine, and a model with no vocabulary slot never read the list.
fn corrects_sounds_like(vocabulary: VocabularySlot, kind: RunKind) -> bool {
    kind.takes_vocabulary() && matches!(vocabulary, VocabularySlot::Prompt(_))
}

/// A run this recent leaves the weights resident (ggml keeps them so for three
/// minutes), so a prewarm after it would only repeat work.
const RECENT_RUN: Duration = Duration::from_secs(60);

pub(crate) struct OfflineGgufSession {
    model: transcribe_cpp::Model,
    session: Option<transcribe_cpp::Session>,
    options: transcribe_cpp::SessionOptions,
    /// When the last run or live stream completed; `None` until the first.
    last_run: Option<Instant>,
    /// How runs take the vocabulary.
    vocabulary: VocabularySlot,
    /// The file's architecture, read once: Whisper trims its near-silent
    /// edges before a run.
    architecture: String,
    /// What this model streams with, and the tuning it was probed for. Its
    /// tuning stays unset until a probe ran past this session's gates.
    streaming: StreamCache,
    /// The Parakeet chunk menus the file carries, read when a tuning needs
    /// them; `None` until then, and when a model has none.
    stream_menus: Option<StreamMenus>,
    /// Whether `stream_menus` was read, so a lab turned on later reads the
    /// header lazily instead of on every load.
    stream_menus_read: bool,
    /// The model file's path, kept for that lazy read.
    stream_path: Option<std::path::PathBuf>,
}

impl OfflineGgufSession {
    #[cfg(all(test, target_os = "macos"))]
    pub(crate) fn set_threads_for_test(&mut self, n_threads: i32) -> transcribe_cpp::Result<()> {
        self.options.n_threads = n_threads;
        self.session = None;
        self.session = Some(self.model.session_with(&self.options)?);
        Ok(())
    }

    pub(crate) fn new(model: transcribe_cpp::Model) -> transcribe_cpp::Result<Self> {
        let options = session_options(model.device()?.device_type);
        let session = model.session_with(&options)?;
        let vocabulary = VocabularySlot::of(&model);
        let architecture = model.arch();
        Ok(Self {
            model,
            session: Some(session),
            options,
            last_run: None,
            vocabulary,
            architecture,
            streaming: StreamCache::default(),
            stream_menus: None,
            stream_menus_read: false,
            stream_path: None,
        })
    }

    /// `options` with the vocabulary added, read now so a settings change
    /// applies from the next run. Parakeet biases toward it
    /// (`crate::dictation::vocabulary::transcription_bias`): the engine writes a term in
    /// place of the heard words only when the audio supports it (an RNN-T
    /// whole-clip run decodes the term alongside the greedy path; streams and
    /// TDT heads rescore the greedy transcript). Whisper and Qwen3-ASR read it
    /// as their context prompt (`crate::dictation::vocabulary::transcription_prompt`),
    /// in whole-clip runs only. A run that reads only the prompt has its
    /// listed "sounds like" spellings corrected after the run instead
    /// (`crate::dictation::vocabulary::correct_sounds_like`).
    fn with_vocabulary<'a>(
        &self,
        options: &'a transcribe_cpp::RunOptions,
        stream: bool,
    ) -> Cow<'a, transcribe_cpp::RunOptions> {
        match self.vocabulary {
            VocabularySlot::Bias if options.family.is_none() => {
                match crate::dictation::vocabulary::transcription_bias() {
                    Some((terms, bias)) => Cow::Owned(transcribe_cpp::RunOptions {
                        vocabulary: terms,
                        family: Some(transcribe_cpp::RunExtension::ParakeetVocabulary(bias)),
                        ..options.clone()
                    }),
                    None => Cow::Borrowed(options),
                }
            }
            VocabularySlot::Prompt(max_chars) if !stream && options.prompt.is_none() => {
                match crate::dictation::vocabulary::transcription_prompt(max_chars) {
                    Some(prompt) => Cow::Owned(transcribe_cpp::RunOptions {
                        prompt: Some(prompt),
                        ..options.clone()
                    }),
                    None => Cow::Borrowed(options),
                }
            }
            _ => Cow::Borrowed(options),
        }
    }

    /// Whether a run, live stream, or warm-up completed within
    /// [`RECENT_RUN`]. A warm-up that stopped at its output limit counts: it
    /// decoded on the device.
    pub(crate) fn ran_recently(&self) -> bool {
        ran_within(self.last_run, Instant::now(), RECENT_RUN)
    }

    /// Streams only on a GPU (Metal, Vulkan): a CPU stream re-encodes each
    /// overlapping window and would cost several times the offline compute.
    /// The probe also prewarms the stream graph (see
    /// [`STREAM_PREWARM_SAMPLES`]). Without an extension, or with one the
    /// model's menu rejects, Parakeet buffers with its largest left, chunk,
    /// and right context (Unified English: 5.6 s, 1.04 s, 1.04 s), its most
    /// accurate and least compute-intensive setting. `HEX_STREAMING=0` keeps
    /// the model offline and `HEX_STREAMING=1` streams on any device
    /// (diagnostics and CI); `HEX_STREAM_CHUNK_MS` and `HEX_STREAM_RIGHT_MS`
    /// override the geometry, and Labs › Faster live text picks a shorter
    /// tuple from the model's own menus.
    ///
    /// Passing these gates is what lets [`Self::refresh_stream`] re-probe
    /// later: a session that fails one never streams at all.
    pub(crate) fn probe_stream(&mut self, options: &transcribe_cpp::RunOptions, model_path: &Path) {
        use transcribe_cpp::DeviceType;
        let forced = match std::env::var("HEX_STREAMING").as_deref() {
            Ok("0") => return,
            Ok("1") => true,
            _ => false,
        };
        if !self
            .model
            .capabilities()
            .is_ok_and(|capabilities| capabilities.supports_streaming)
        {
            return;
        }
        let Some(device_type) = self.model.device().ok().map(|device| device.device_type) else {
            return;
        };
        if !forced && !matches!(device_type, DeviceType::Gpu | DeviceType::Igpu) {
            return;
        }
        let tuning = self.tuning();
        if self.architecture == "parakeet" {
            self.stream_path = Some(model_path.to_path_buf());
        }
        if tuning.needs_menus() {
            self.read_menus();
        }
        let started = std::time::Instant::now();
        let stream = self.probe_best(options, tuning);
        self.streaming.record(tuning, stream);
        if self.streaming.stream().is_some() {
            tracing::info!(
                ?device_type,
                ?tuning,
                menus = self.stream_menus.is_some(),
                prewarm_ms = started.elapsed().as_millis(),
                "streaming transcription is available"
            );
        }
    }

    /// The stream geometry this model streams with, as the last probe found
    /// it.
    pub(crate) fn stream_options(&self) -> Option<&transcribe_cpp::StreamOptions> {
        self.streaming.stream()
    }

    /// The tuning this model streams with: only a Parakeet buffered model has
    /// geometry to tune.
    fn tuning(&self) -> StreamTuning {
        StreamTuning::for_model(self.architecture == "parakeet", StreamTuning::current())
    }

    /// Reads the model's chunk menus once, when a tuning needs them: a load
    /// with the lab off never opens the header for them.
    fn read_menus(&mut self) {
        if self.stream_menus_read {
            return;
        }
        let Some(path) = self.stream_path.as_deref() else {
            return;
        };
        self.stream_menus_read = true;
        self.stream_menus = StreamMenus::read(path);
    }

    /// Whether this model streams with Parakeet's buffered chunk menus rather
    /// than a cache-aware window such as Nemotron's. A buffered model's token
    /// times are emission frames, so its first word starts later than its
    /// audio. Any other architecture, and a file with no menus, is not
    /// buffered.
    pub(crate) fn streams_buffered_parakeet(&mut self) -> bool {
        if self.architecture != "parakeet" {
            return false;
        }
        self.read_menus();
        matches!(self.stream_menus, Some(StreamMenus::Buffered { .. }))
    }

    /// Re-probes while nothing records, when this process's tuning changed
    /// since the last probe, so Labs › Faster live text applies to a later
    /// capture without a restart. A session whose gated probe never ran, or
    /// never passed, does nothing here.
    pub(crate) fn refresh_stream(&mut self, options: &transcribe_cpp::RunOptions) {
        let tuning = self.tuning();
        if !self.streaming.wants(tuning) {
            return;
        }
        if tuning.needs_menus() {
            self.read_menus();
        }
        let probed = self.probe_best(options, tuning);
        if !self.streaming.record(tuning, probed) {
            tracing::warn!(
                ?tuning,
                "this stream geometry could not stream here; keeping the geometry in hand"
            );
        }
    }

    /// Probes the tuning's geometry, and the model's own default when that
    /// geometry cannot stream here: an off-menu env value or a tuple the
    /// model never trained for warns and keeps streaming, rather than
    /// turning it off.
    fn probe_best(
        &mut self,
        options: &transcribe_cpp::RunOptions,
        tuning: StreamTuning,
    ) -> Option<transcribe_cpp::StreamOptions> {
        let requested = tuning.options(self.stream_menus.as_ref());
        let mut probe = self.probe_silence(options, &requested);
        let mut geometry = requested;
        if probe.is_err() && geometry.family.is_some() {
            tracing::warn!(
                ?tuning,
                "this stream geometry is outside the model's menu; using its default"
            );
            geometry = transcribe_cpp::StreamOptions::default();
            probe = self.probe_silence(options, &geometry);
        }
        match probe {
            Ok(()) => Some(geometry),
            Err(error) => {
                tracing::warn!(%error, "model advertises streaming but could not stream");
                None
            }
        }
    }

    /// Runs [`STREAM_PREWARM_SAMPLES`] of silence through one stream, which
    /// prewarms the stream graph too.
    fn probe_silence(
        &mut self,
        options: &transcribe_cpp::RunOptions,
        stream_options: &transcribe_cpp::StreamOptions,
    ) -> transcribe_cpp::Result<()> {
        self.with_stream(
            options,
            stream_options,
            &transcribe_cpp::CancelToken::new(),
            |stream| {
                stream.feed(&vec![0.0; STREAM_PREWARM_SAMPLES])?;
                stream.finalize().map(|_| ())
            },
        )?
    }

    /// Runs `drive` against one live stream on this session, then rotates the
    /// session like [`Self::run`] so the stream's buffered audio and scratch
    /// are released. `cancel` stays installed only for the stream's lifetime.
    pub(crate) fn with_stream<R>(
        &mut self,
        options: &transcribe_cpp::RunOptions,
        stream_options: &transcribe_cpp::StreamOptions,
        cancel: &transcribe_cpp::CancelToken,
        drive: impl FnOnce(&mut transcribe_cpp::Stream<'_>) -> R,
    ) -> transcribe_cpp::Result<R> {
        let mut session = match self.session.take() {
            Some(session) => session,
            None => self.model.session_with(&self.options)?,
        };
        session.set_cancel_token(cancel);
        let options = self.with_vocabulary(options, true);
        let result = session
            .stream(&options, stream_options)
            .map(|mut stream| drive(&mut stream));
        drop(session);
        if result.is_ok() {
            self.last_run = Some(Instant::now());
        }
        self.session = self.model.session_with(&self.options).ok();
        result
    }

    pub(crate) fn run(
        &mut self,
        samples: &[f32],
        options: &transcribe_cpp::RunOptions,
    ) -> transcribe_cpp::Result<transcribe_cpp::Transcript> {
        self.run_as(samples, options, RunKind::Transcript)
    }

    /// Runs `samples` exactly like [`Self::run`], but without adding the
    /// vocabulary prompt. Silence plus a long term list makes a decoder such
    /// as Qwen3-ASR read the list back to its output cap on every warm-up,
    /// seconds of work that warms nothing extra (#1568). A decode that still
    /// stops at that cap leaves the model warm and counts as a run, so a
    /// prewarm right after the load is not repeated.
    pub(crate) fn warm(
        &mut self,
        samples: &[f32],
        options: &transcribe_cpp::RunOptions,
    ) -> transcribe_cpp::Result<transcribe_cpp::Transcript> {
        self.run_as(samples, options, RunKind::Warmup)
    }

    fn run_as(
        &mut self,
        samples: &[f32],
        options: &transcribe_cpp::RunOptions,
        kind: RunKind,
    ) -> transcribe_cpp::Result<transcribe_cpp::Transcript> {
        // transcribe-cpp 0.1.x retains input-sized scheduler buffers for the
        // session lifetime. Drop each used session before creating its
        // successor so a long inference cannot pin that high-water mark or
        // overlap two sessions' persistent decoder state.
        let mut session = match self.session.take() {
            Some(session) => session,
            None => self.model.session_with(&self.options)?,
        };
        if let Some(cancel) = job_cancel() {
            session.set_cancel_token(&cancel);
        }
        let options = match kind.takes_vocabulary() {
            true => self.with_vocabulary(options, false),
            false => Cow::Borrowed(options),
        };
        // Parakeet's engine bias already writes a term in place of an exact
        // "sounds like" spelling; a prompt model needs the spelling corrected
        // after the run instead, and a model with no vocabulary slot doesn't
        // get the list at all.
        let correct_spellings = corrects_sounds_like(self.vocabulary, kind);
        let result =
            keep_repetition_partial(session.run(samples, &options)).map(|mut transcript| {
                // Only the vocabulary sets a prompt, and a warm-up leaves it
                // out, so it has no echo to drop; reading the effective
                // options keeps that true by construction.
                if let Some(prompt) = &options.prompt {
                    drop_prompt_echo(&mut transcript, prompt);
                }
                drop_subtitle_credit(&mut transcript);
                if correct_spellings {
                    crate::dictation::vocabulary::correct_sounds_like(&mut transcript);
                }
                transcript
            });
        drop(session);
        if result.is_ok()
            || (kind.counts_when_truncated()
                && matches!(&result, Err(transcribe_cpp::Error::OutputTruncated { .. })))
        {
            self.last_run = Some(Instant::now());
        }
        match self.model.session_with(&self.options) {
            Ok(session) => {
                self.session = Some(session);
                result
            }
            Err(recovery_error) => match result {
                Ok(_) => Err(recovery_error),
                Err(run_error) => Err(run_error),
            },
        }
    }

    /// The audio a whole-clip run gets, and where it starts in `samples`.
    /// Whisper decodes a whole 30 s window and fills a near-silent opening or
    /// tail with text nobody said (a Turkish "thanks for watching", #1770), so
    /// its clip keeps only the speech the detector hears plus
    /// [`WHISPER_EDGE_MARGIN_SAMPLES`]. Another model, a detector that is
    /// unavailable, and a clip it hears no speech in all keep every sample.
    fn whisper_audio<'a>(&self, samples: &'a [f32]) -> (Cow<'a, [f32]>, usize) {
        let bounds = if self.architecture == "whisper" {
            crate::transcription::speech_detector::speech_bounds(samples)
        } else {
            None
        };
        let (audio, trimmed) = trimmed_audio(samples, &self.architecture, bounds);
        if let Cow::Owned(audio) = &audio {
            tracing::info!(
                trimmed_ms = trimmed / 16,
                kept_ms = audio.len() / 16,
                "trimmed near-silent edges before a Whisper run"
            );
        }
        (audio, trimmed)
    }

    pub(crate) fn transcribe_chunked(
        &mut self,
        samples: &[f32],
        options: &transcribe_cpp::RunOptions,
        max_audio_samples: Option<usize>,
    ) -> Result<String> {
        let (audio, _) = self.whisper_audio(samples);
        let samples = audio.as_ref();
        let Some(max_samples) =
            max_audio_samples.filter(|&max_samples| samples.len() > max_samples)
        else {
            return self
                .run(samples, options)
                .map(|transcript| transcript.text)
                .wrap_err("transcription failed");
        };
        let mut output = Vec::new();
        for range in chunk_ranges(samples, max_samples) {
            let samples = &samples[range];
            if job_cancel().is_some_and(|cancel| cancel.is_cancelled()) {
                return Err(eyre!("transcription was cancelled"));
            }
            if !chunk_may_hold_speech(samples) {
                tracing::info!(
                    chunk_ms = samples.len() / 16,
                    "skipped a chunk of a long clip with no speech"
                );
                continue;
            }
            let mut chunk = samples.to_vec();
            crate::dictation::pad_for_parakeet(&mut chunk);
            let transcript = self.run(&chunk, options).wrap_err("transcription failed")?;
            let text = transcript.text.trim();
            if !text.is_empty() {
                output.push(text.to_string());
            }
        }
        Ok(output.join(" "))
    }

    /// [`Self::transcribe_chunked`] with the model's own segment and word
    /// times, moved onto the clip's timeline by each chunk's sample position.
    /// Text is joined exactly as the untimed run joins it.
    pub(crate) fn transcribe_chunked_timed(
        &mut self,
        samples: &[f32],
        options: &transcribe_cpp::RunOptions,
        max_audio_samples: Option<usize>,
    ) -> Result<crate::transcription::transcript::Transcript> {
        let (audio, trimmed) = self.whisper_audio(samples);
        let samples = audio.as_ref();
        let Some(max_samples) =
            max_audio_samples.filter(|&max_samples| samples.len() > max_samples)
        else {
            let transcript = self
                .run(samples, options)
                .wrap_err("transcription failed")?;
            return Ok(timed(transcript, chunk_offset_ms(trimmed, 0)));
        };
        let mut timed = crate::transcription::transcript::Transcript::default();
        let mut texts = Vec::new();
        for range in chunk_ranges(samples, max_samples) {
            let offset = chunk_offset_ms(trimmed, range.start);
            let samples = &samples[range];
            if job_cancel().is_some_and(|cancel| cancel.is_cancelled()) {
                return Err(eyre!("transcription was cancelled"));
            }
            if !chunk_may_hold_speech(samples) {
                tracing::info!(
                    chunk_ms = samples.len() / 16,
                    "skipped a chunk of a long clip with no speech"
                );
                continue;
            }
            let mut chunk = samples.to_vec();
            crate::dictation::pad_for_parakeet(&mut chunk);
            let transcript = self.run(&chunk, options).wrap_err("transcription failed")?;
            let text = transcript.text.trim();
            if !text.is_empty() {
                texts.push(text.to_string());
            }
            // The offset comes from where the range sits in the clip, never
            // from how many chunks produced text.
            let spans = timed_spans(transcript, offset);
            timed.segments.extend(spans.segments);
            timed.words.extend(spans.words);
        }
        timed.text = texts.join(" ");
        Ok(timed)
    }
}

/// A whole clip's segments and words, from its single run.
fn timed(
    transcript: transcribe_cpp::Transcript,
    offset_ms: i64,
) -> crate::transcription::transcript::Transcript {
    let text = transcript.text.clone();
    let spans = timed_spans(transcript, offset_ms);
    crate::transcription::transcript::Transcript {
        text,
        segments: spans.segments,
        words: spans.words,
    }
}

/// The native timing level a run options timestamp kind asks for.
pub(crate) fn timestamp_level(kind: transcribe_cpp::TimestampKind) -> TimestampLevel {
    match kind {
        transcribe_cpp::TimestampKind::Word | transcribe_cpp::TimestampKind::Token => {
            TimestampLevel::Word
        }
        transcribe_cpp::TimestampKind::Segment | transcribe_cpp::TimestampKind::Auto => {
            TimestampLevel::Segment
        }
        transcribe_cpp::TimestampKind::None => TimestampLevel::None,
    }
}

/// 100 ms at 16 kHz: the span a long clip is cut through.
const QUIET_SPAN_SAMPLES: usize = 1_600;
/// How far back from a window's end the quietest span is looked for.
const QUIET_SEARCH_SAMPLES: usize = 5 * 16_000;

/// Where a window is cut into the next one: the middle of the quietest 100 ms
/// in its last five seconds (or its last half, when that is shorter), never
/// its first sample. `samples` must hold at least `max_samples`, and only
/// those are read, so a streaming reader can cut before it has read the rest
/// of the clip.
pub(crate) fn quiet_cut(samples: &[f32], max_samples: usize) -> usize {
    let max_samples = max_samples.max(1);
    let search = QUIET_SEARCH_SAMPLES.min(max_samples / 2).max(1);
    let span = QUIET_SPAN_SAMPLES.min(search);
    let quiet =
        max_samples - search + quietest_span(&samples[max_samples - search..max_samples], span);
    quiet.max(1)
}

/// Splits a clip into consecutive ranges of at most `max_samples`, cutting
/// each window at the middle of the quietest 100 ms in its last five seconds
/// (or half the window, when that is shorter), adapted from Cohere's reference
/// processor. An exact sample cut lands mid-word, and each slice starts a
/// fresh sentence with a capital and a closing full stop, so a long dictation
/// reads as many sentences instead of one. Each cut is [`quiet_cut`]'s.
///
/// Every sample is covered exactly once by a non-empty range. Ties pick the
/// latest span, so the ranges are deterministic. The search is energy-based,
/// so continuous speech with no quieter stretch there still cuts at its
/// quietest point, which can land inside a word.
pub(crate) fn chunk_ranges(samples: &[f32], max_samples: usize) -> Vec<Range<usize>> {
    let max_samples = max_samples.max(1);
    let mut ranges = Vec::new();
    let mut start = 0;
    while start < samples.len() {
        if samples.len() - start <= max_samples {
            ranges.push(start..samples.len());
            break;
        }
        let cut = start + quiet_cut(&samples[start..], max_samples);
        ranges.push(start..cut);
        start = cut;
    }
    ranges
}

/// The midpoint of the quietest `span` samples, the latest one on a tie.
fn quietest_span(samples: &[f32], span: usize) -> usize {
    if samples.is_empty() {
        return 0;
    }
    let span = span.clamp(1, samples.len());

    let sample_energy = |sample: f32| f64::from(sample) * f64::from(sample);
    let mut energy = samples[..span]
        .iter()
        .copied()
        .map(sample_energy)
        .sum::<f64>();
    let mut quietest = energy;
    let mut quietest_start = 0;
    for next in 1..=samples.len() - span {
        let leaving = sample_energy(samples[next - 1]);
        let entering = sample_energy(samples[next + span - 1]);
        energy += entering - leaving;
        // Ties keep the latest span.
        if energy <= quietest {
            quietest = energy;
            quietest_start = next;
        }
    }
    quietest_start + span / 2
}

/// Where `sample` sits in the clip, in whole milliseconds.
pub(crate) fn sample_offset_ms(sample: usize) -> i64 {
    i64::try_from(sample / 16).unwrap_or(i64::MAX)
}

/// Where a chunk starting at `chunk_start` in the trimmed audio sits in the
/// original clip, in whole milliseconds. A trimmed clip's segment and word
/// times move by this, so they stay on the clip's timeline; the whole clip is
/// the chunk that starts at 0.
fn chunk_offset_ms(trim_start: usize, chunk_start: usize) -> i64 {
    sample_offset_ms(trim_start.saturating_add(chunk_start))
}

fn timed_spans(
    transcript: transcribe_cpp::Transcript,
    offset_ms: i64,
) -> crate::transcription::transcript::Transcript {
    use crate::transcription::transcript::TranscriptSegment;
    let span = |start_ms: i64, end_ms: i64, text: String| {
        let text = text.trim().to_owned();
        let start_ms = start_ms.max(0);
        (!text.is_empty()).then(|| TranscriptSegment {
            start_ms: start_ms.saturating_add(offset_ms),
            end_ms: end_ms.max(start_ms).saturating_add(offset_ms),
            text,
        })
    };
    crate::transcription::transcript::Transcript {
        segments: transcript
            .segments
            .into_iter()
            .filter_map(|segment| span(segment.t0_ms, segment.t1_ms, segment.text))
            .collect(),
        words: transcript
            .words
            .into_iter()
            .filter_map(|word| span(word.t0_ms, word.t1_ms, word.text))
            .collect(),
        ..Default::default()
    }
}

/// How much audio a trimmed Whisper run keeps around the speech the detector
/// hears, on each side.
const WHISPER_EDGE_MARGIN_SAMPLES: usize = 8_000;

/// The span of a clip of `len` samples a whole-clip run keeps. A Whisper clip
/// loses everything outside its detected speech plus
/// [`WHISPER_EDGE_MARGIN_SAMPLES`]; any other model, and a clip without speech
/// bounds, keeps every sample.
fn whisper_span(len: usize, architecture: &str, bounds: Option<Range<usize>>) -> Range<usize> {
    match (architecture, bounds) {
        ("whisper", Some(bounds)) => crate::transcription::speech_detector::edge_bounds(
            len,
            bounds,
            WHISPER_EDGE_MARGIN_SAMPLES,
        ),
        _ => 0..len,
    }
}

/// `samples` for a whole-clip run: the span [`whisper_span`] keeps, padded to
/// the minimum duration every chunk and every caller pads to, and where that
/// span starts in `samples`. A clip the span covers whole is borrowed, never
/// copied or padded.
fn trimmed_audio<'a>(
    samples: &'a [f32],
    architecture: &str,
    bounds: Option<Range<usize>>,
) -> (Cow<'a, [f32]>, usize) {
    let span = whisper_span(samples.len(), architecture, bounds);
    if span.start == 0 && span.end == samples.len() {
        return (Cow::Borrowed(samples), 0);
    }
    let mut audio = samples[span.clone()].to_vec();
    crate::dictation::pad_for_parakeet(&mut audio);
    (Cow::Owned(audio), span.start)
}

/// Whether one chunk of a long clip, or one piece of a long file, goes to the
/// model. The recording as a whole held speech, but a chunk of it can be only
/// silence (a locked dictation left waiting before the words, or a pause
/// between two pieces), and models such as Cohere and Whisper invent long,
/// repeating text for that (#1194, #1770), or run to their generation cap
/// (#968). Without the detector every chunk runs.
pub(crate) fn chunk_may_hold_speech(samples: &[f32]) -> bool {
    crate::transcription::speech_detector::contains_speech(samples) != Some(false)
}

/// Whether `error` is a decode stopped at its output limit. A decoder that
/// runs to its generation cap on silence (Qwen3-ASR 1.7B, #968) still ran the
/// whole graph on the device, so a warm-up that ends this way leaves the model
/// warm: it is not a failed device, and not a failed selection.
pub(crate) fn output_truncated(error: &color_eyre::eyre::Report) -> bool {
    error.chain().any(|cause| {
        matches!(
            cause.downcast_ref::<transcribe_cpp::Error>(),
            Some(transcribe_cpp::Error::OutputTruncated { .. })
        )
    })
}

/// transcribe-cpp 0.4 stops a decoder that starts looping and reports the text
/// before the loop as an error; 0.2 returned the looped text as success. Keep
/// returning text.
fn keep_repetition_partial(
    result: transcribe_cpp::Result<transcribe_cpp::Transcript>,
) -> transcribe_cpp::Result<transcribe_cpp::Transcript> {
    match result {
        Err(transcribe_cpp::Error::OutputRepetition {
            partial: Some(partial),
            ..
        }) => Ok(*partial),
        result => result,
    }
}

fn ran_within(last_run: Option<Instant>, now: Instant, window: Duration) -> bool {
    last_run.is_some_and(|last_run| now.saturating_duration_since(last_run) < window)
}

fn normalize_spoken(text: &str) -> String {
    text.split_whitespace()
        .filter_map(|word| {
            let word = word.trim_matches(|character: char| character.is_ascii_punctuation());
            if word.is_empty() {
                return None;
            }
            let word = word.to_ascii_lowercase();
            Some(match word.as_str() {
                "zero" => "0".to_string(),
                "one" => "1".to_string(),
                "two" => "2".to_string(),
                "three" => "3".to_string(),
                "four" => "4".to_string(),
                "five" => "5".to_string(),
                "six" => "6".to_string(),
                "seven" => "7".to_string(),
                "eight" => "8".to_string(),
                "nine" => "9".to_string(),
                _ => word,
            })
        })
        .collect::<Vec<_>>()
        .join(" ")
}

/// Drops segments that only repeat the vocabulary prompt. Whisper and
/// Qwen3-ASR can return their context prompt verbatim on silence or noise.
/// Speech that merely contains a term is kept, and so is a single term:
/// dictating the one listed name must still paste it.
fn drop_prompt_echo(transcript: &mut transcribe_cpp::Transcript, prompt: &str) {
    let is_separator = |character: char| matches!(character, ',' | ';' | '\n');
    let hints = prompt
        .split(is_separator)
        .filter(|hint| !hint.trim().is_empty())
        .count();
    if hints < 2 {
        return;
    }
    let normalize = |text: &str| normalize_spoken(&text.replace(is_separator, " "));
    let prompt = normalize(prompt);
    let echoes = |text: &str| normalize(text) == prompt;
    if transcript.segments.is_empty() {
        if echoes(&transcript.text) {
            transcript.text.clear();
            transcript.words.clear();
        }
        return;
    }
    if !transcript
        .segments
        .iter()
        .any(|segment| echoes(&segment.text))
    {
        return;
    }
    let words = std::mem::take(&mut transcript.words);
    let mut segments = Vec::new();
    for mut segment in std::mem::take(&mut transcript.segments) {
        if echoes(&segment.text) {
            continue;
        }
        let first = (segment.first_word.max(0) as usize).min(words.len());
        let last = (first + segment.n_words.max(0) as usize).min(words.len());
        segment.first_word = transcript.words.len() as i32;
        let index = segments.len() as i32;
        transcript
            .words
            .extend(words[first..last].iter().cloned().map(|mut word| {
                word.seg_index = index;
                word
            }));
        segments.push(segment);
    }
    transcript.text = segments
        .iter()
        .map(|segment| segment.text.trim())
        .filter(|text| !text.is_empty())
        .collect::<Vec<_>>()
        .join(" ");
    transcript.segments = segments;
}

/// Removes a subtitle credit the model appended to the end of the clip
/// (`subtitle_credits`, #1141) from the text, the last segments, and their
/// words.
fn drop_subtitle_credit(transcript: &mut transcribe_cpp::Transcript) {
    use crate::transcription::subtitle_credits::{strip, token_count};
    let Some((text, removed_from_text)) = strip(&transcript.text) else {
        return;
    };
    transcript.text = text;
    let mut removed = if transcript.segments.is_empty() {
        removed_from_text
    } else {
        0
    };
    while let Some(segment) = transcript.segments.last_mut() {
        let Some((kept, tokens)) = strip(&segment.text) else {
            break;
        };
        if kept.is_empty() {
            let first = (segment.first_word.max(0) as usize).min(transcript.words.len());
            transcript.words.truncate(first);
            transcript.segments.pop();
            continue;
        }
        segment.text = kept;
        removed = tokens;
        break;
    }
    if transcript.segments.is_empty() && transcript.text.is_empty() {
        transcript.words.clear();
        return;
    }
    // Words of a segment that kept some speech, or of an unsegmented run.
    let last_segment = transcript.segments.last_mut();
    let floor = last_segment
        .as_ref()
        .map_or(0, |segment| segment.first_word.max(0) as usize);
    let mut popped = 0;
    while removed > 0 && transcript.words.len() > floor {
        let Some(word) = transcript.words.last() else {
            break;
        };
        let tokens = token_count(&word.text);
        if tokens > removed {
            break;
        }
        removed -= tokens;
        transcript.words.pop();
        popped += 1;
    }
    if let Some(segment) = last_segment {
        segment.n_words = (segment.n_words - popped).max(0);
    }
}

#[cfg(test)]
#[path = "gguf_session_tests.rs"]
mod tests;
