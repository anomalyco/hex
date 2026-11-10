//! The `transcribe-cpp` session owner shared by the macOS, Linux, and Windows
//! GGUF transcribers. It recreates the native session around every run (and
//! every live stream) so retained scheduler scratch never outlives the
//! inference that sized it.

use std::borrow::Cow;
use std::cell::RefCell;
use std::path::Path;
use std::time::{Duration, Instant};

use color_eyre::eyre::{Result, WrapErr, eyre};

use crate::transcription::models::{
    AUTO_LANGUAGE, ModelDefinition, ModelRuntime, TranscriptionSelection,
};
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
#[derive(Clone, Copy)]
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
        Ok(Self {
            model,
            session: Some(session),
            options,
            last_run: None,
            vocabulary,
        })
    }

    /// `options` with the vocabulary added, read now so a settings change
    /// applies from the next run. Parakeet biases toward it
    /// (`crate::dictation::vocabulary::transcription_bias`): the engine writes a term in
    /// place of the heard words only when the audio supports it (an RNN-T
    /// whole-clip run decodes the term alongside the greedy path; streams and
    /// TDT heads rescore the greedy transcript). Whisper and Qwen3-ASR read it
    /// as their context prompt (`crate::dictation::vocabulary::transcription_prompt`),
    /// in whole-clip runs only.
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
    /// [`STREAM_PREWARM_SAMPLES`]). Without an extension
    /// Parakeet buffers with its largest left, chunk, and right context
    /// (Unified English: 5.6 s, 1.04 s, 1.04 s), its most accurate and least
    /// compute-intensive setting. `HEX_STREAMING=0` keeps the model offline
    /// and `HEX_STREAMING=1` streams on any device (diagnostics and CI);
    /// `HEX_STREAM_CHUNK_MS` overrides the chunk and right context.
    pub(crate) fn probe_stream(
        &mut self,
        options: &transcribe_cpp::RunOptions,
        architecture: &str,
    ) -> Option<transcribe_cpp::StreamOptions> {
        use transcribe_cpp::DeviceType;
        let forced = match std::env::var("HEX_STREAMING").as_deref() {
            Ok("0") => return None,
            Ok("1") => true,
            _ => false,
        };
        if !self
            .model
            .capabilities()
            .is_ok_and(|capabilities| capabilities.supports_streaming)
        {
            return None;
        }
        let device_type = self.model.device().ok()?.device_type;
        if !forced && !matches!(device_type, DeviceType::Gpu | DeviceType::Igpu) {
            return None;
        }
        let chunk_ms = std::env::var("HEX_STREAM_CHUNK_MS")
            .ok()
            .and_then(|value| value.parse::<i32>().ok());
        let stream_options = transcribe_cpp::StreamOptions {
            family: (architecture == "parakeet" && chunk_ms.is_some()).then_some(
                transcribe_cpp::StreamExtension::ParakeetBuffered(
                    transcribe_cpp::ParakeetBufferedStreamOptions {
                        left_ms: None,
                        chunk_ms,
                        right_ms: chunk_ms,
                    },
                ),
            ),
            ..Default::default()
        };
        let started = std::time::Instant::now();
        let probe = self.with_stream(
            options,
            &stream_options,
            &transcribe_cpp::CancelToken::new(),
            |stream| {
                stream.feed(&vec![0.0; STREAM_PREWARM_SAMPLES])?;
                stream.finalize().map(|_| ())
            },
        );
        match probe {
            Ok(Ok(())) => {
                tracing::info!(
                    ?device_type,
                    chunk_ms,
                    prewarm_ms = started.elapsed().as_millis(),
                    "streaming transcription is available"
                );
                Some(stream_options)
            }
            Ok(Err(error)) | Err(error) => {
                tracing::warn!(%error, "model advertises streaming but could not stream");
                None
            }
        }
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
        let result =
            keep_repetition_partial(session.run(samples, &options)).map(|mut transcript| {
                // Only the vocabulary sets a prompt, and a warm-up leaves it
                // out, so it has no echo to drop; reading the effective
                // options keeps that true by construction.
                if let Some(prompt) = &options.prompt {
                    drop_prompt_echo(&mut transcript, prompt);
                }
                drop_subtitle_credit(&mut transcript);
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

    pub(crate) fn transcribe_chunked(
        &mut self,
        samples: &[f32],
        options: &transcribe_cpp::RunOptions,
        max_audio_samples: Option<usize>,
    ) -> Result<String> {
        let Some(max_samples) =
            max_audio_samples.filter(|&max_samples| samples.len() > max_samples)
        else {
            return self
                .run(samples, options)
                .map(|transcript| transcript.text)
                .wrap_err("transcription failed");
        };
        let mut output = Vec::new();
        for samples in samples.chunks(max_samples) {
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
        let Some(max_samples) =
            max_audio_samples.filter(|&max_samples| samples.len() > max_samples)
        else {
            let transcript = self
                .run(samples, options)
                .wrap_err("transcription failed")?;
            return Ok(timed(transcript, 0));
        };
        let mut timed = crate::transcription::transcript::Transcript::default();
        let mut texts = Vec::new();
        for (index, samples) in samples.chunks(max_samples).enumerate() {
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
            // The offset comes from where the chunk sits in the clip, never
            // from how many chunks produced text.
            let spans = timed_spans(transcript, chunk_offset_ms(index, max_samples));
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

/// Where one chunk starts in the clip, in whole milliseconds.
pub(crate) fn chunk_offset_ms(index: usize, chunk_samples: usize) -> i64 {
    i64::try_from(index.saturating_mul(chunk_samples) / 16).unwrap_or(i64::MAX)
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

/// Whether one chunk of a long clip goes to the model. The clip as a whole
/// held speech, but a chunk of it can be only silence (a locked dictation
/// left waiting before the words), and models such as Cohere invent long,
/// repeating text for that (#1194). Without the detector every chunk runs.
fn chunk_may_hold_speech(samples: &[f32]) -> bool {
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
mod tests {
    use super::*;

    #[test]
    fn normalizes_spoken_numbers_and_punctuation() {
        assert_eq!(normalize_spoken("Move LEFT, three!"), "move left 3");
        assert_eq!(normalize_spoken("Ä ä A a ONE one"), "Ä ä a a 1 1");
    }

    #[test]
    fn a_chunks_times_move_onto_the_clips_timeline_by_sample_position() {
        // 30 s chunks at 16 kHz, and a short final chunk: offsets come from
        // where each chunk starts, never from how many produced text.
        assert_eq!(chunk_offset_ms(0, 30 * 16_000), 0);
        assert_eq!(chunk_offset_ms(1, 30 * 16_000), 30_000);
        assert_eq!(chunk_offset_ms(2, 30 * 16_000), 60_000);
        assert_eq!(chunk_offset_ms(3, 1_600), 300);

        let shifted = timed_spans(
            transcribe_cpp::Transcript {
                text: "hello".into(),
                segments: vec![transcribe_cpp::Segment {
                    text: " hello ".into(),
                    t0_ms: 0,
                    t1_ms: 480,
                    ..Default::default()
                }],
                words: vec![
                    transcribe_cpp::Word {
                        text: "hello".into(),
                        t0_ms: 10,
                        t1_ms: 480,
                        ..Default::default()
                    },
                    transcribe_cpp::Word {
                        text: "  ".into(),
                        t0_ms: 500,
                        t1_ms: 600,
                        ..Default::default()
                    },
                ],
                ..Default::default()
            },
            chunk_offset_ms(2, 30 * 16_000),
        );
        assert_eq!(shifted.segments.len(), 1);
        assert_eq!(
            (
                shifted.segments[0].start_ms,
                shifted.segments[0].end_ms,
                shifted.segments[0].text.as_str()
            ),
            (60_000, 60_480, "hello")
        );
        assert_eq!(shifted.words.len(), 1, "an empty word is dropped");
        assert_eq!(
            (shifted.words[0].start_ms, shifted.words[0].end_ms),
            (60_010, 60_480)
        );
    }

    #[test]
    fn a_models_timestamp_kind_names_its_finest_level() {
        for (kind, level) in [
            (transcribe_cpp::TimestampKind::Word, TimestampLevel::Word),
            (transcribe_cpp::TimestampKind::Token, TimestampLevel::Word),
            (
                transcribe_cpp::TimestampKind::Segment,
                TimestampLevel::Segment,
            ),
            (transcribe_cpp::TimestampKind::Auto, TimestampLevel::Segment),
            (transcribe_cpp::TimestampKind::None, TimestampLevel::None),
        ] {
            assert_eq!(timestamp_level(kind), level, "{kind:?}");
        }
    }

    #[test]
    fn only_a_completed_run_inside_the_window_counts_as_recent() {
        let now = Instant::now();
        let window = Duration::from_secs(60);
        assert!(!ran_within(None, now, window), "a fresh load has not run");
        assert!(ran_within(Some(now), now, window));
        assert!(ran_within(Some(now - Duration::from_secs(59)), now, window));
        assert!(!ran_within(
            Some(now - Duration::from_secs(60)),
            now,
            window
        ));
    }

    #[test]
    fn a_decode_stopped_for_repeating_keeps_its_text_and_truncation_still_fails() {
        let transcript = |text: &str| {
            Some(Box::new(transcribe_cpp::Transcript {
                text: text.into(),
                ..Default::default()
            }))
        };
        let repeated = keep_repetition_partial(Err(transcribe_cpp::Error::OutputRepetition {
            message: "loop".into(),
            partial: transcript("send it now"),
        }));
        assert_eq!(repeated.unwrap().text, "send it now");
        let truncated = keep_repetition_partial(Err(transcribe_cpp::Error::OutputTruncated {
            message: "budget".into(),
            partial: transcript("send it"),
        }));
        assert!(matches!(
            truncated,
            Err(transcribe_cpp::Error::OutputTruncated { .. })
        ));
    }

    #[test]
    fn a_warmup_leaves_the_vocabulary_out_and_counts_when_truncated() {
        assert!(
            RunKind::Transcript.takes_vocabulary(),
            "a transcript still gets the vocabulary"
        );
        assert!(
            !RunKind::Warmup.takes_vocabulary(),
            "silence plus a term list makes Qwen3-ASR read the list back (#1568)"
        );
        assert!(!RunKind::Transcript.counts_when_truncated());
        assert!(RunKind::Warmup.counts_when_truncated());
    }

    #[test]
    fn a_decode_stopped_at_its_output_limit_is_recognised_through_its_context() {
        let truncated: Result<()> = Err(transcribe_cpp::Error::OutputTruncated {
            message: "budget".into(),
            partial: None,
        })
        .wrap_err("transcription failed");
        assert!(output_truncated(&truncated.unwrap_err()));
        let repeating: Result<()> = Err(transcribe_cpp::Error::OutputRepetition {
            message: "loop".into(),
            partial: None,
        })
        .wrap_err("transcription failed");
        assert!(!output_truncated(&repeating.unwrap_err()));
    }

    #[test]
    fn a_long_clip_skips_only_chunks_the_detector_hears_no_speech_in() {
        use crate::transcription::speech_detector::override_for_test;
        let chunk = [0.0; 16_000];
        {
            let _silent = override_for_test(Some(false));
            assert!(!chunk_may_hold_speech(&chunk));
        }
        {
            let _speech = override_for_test(Some(true));
            assert!(chunk_may_hold_speech(&chunk));
        }
        let _unavailable = override_for_test(None);
        assert!(
            chunk_may_hold_speech(&chunk),
            "no detector keeps every chunk"
        );
    }

    #[test]
    fn gpu_sessions_bound_decoder_threads_and_cpu_sessions_keep_library_default() {
        use transcribe_cpp::DeviceType;
        assert_eq!(session_options(DeviceType::Gpu).n_threads, 1);
        assert_eq!(session_options(DeviceType::Igpu).n_threads, 1);
        assert_eq!(session_options(DeviceType::Cpu).n_threads, 0);
        assert_eq!(session_options(DeviceType::Accel).n_threads, 0);
    }

    fn segment(text: &str, first_word: i32, n_words: i32) -> transcribe_cpp::Segment {
        transcribe_cpp::Segment {
            text: text.into(),
            first_word,
            n_words,
            ..Default::default()
        }
    }

    fn transcript(segments: &[&str]) -> transcribe_cpp::Transcript {
        let mut transcript = transcribe_cpp::Transcript::default();
        let mut first_word = 0;
        for text in segments {
            let n_words = text.split_whitespace().count() as i32;
            transcript.segments.push(segment(text, first_word, n_words));
            for word in text.split_whitespace() {
                transcript.words.push(transcribe_cpp::Word {
                    text: word.into(),
                    ..Default::default()
                });
            }
            first_word += n_words;
        }
        transcript.text = segments.join(" ");
        transcript
    }

    fn words(transcript: &transcribe_cpp::Transcript) -> Vec<&str> {
        transcript
            .words
            .iter()
            .map(|word| word.text.as_str())
            .collect()
    }

    #[test]
    fn prompt_echo_is_dropped() {
        let mut echo = transcript(&["T3 Code, Ottobit."]);
        drop_prompt_echo(&mut echo, "t3 code, ottobit");
        assert_eq!(echo.text, "");
        assert!(echo.segments.is_empty());
        assert!(echo.words.is_empty());

        let mut unsegmented = transcribe_cpp::Transcript {
            text: " T3 Code, Ottobit. ".into(),
            ..Default::default()
        };
        drop_prompt_echo(&mut unsegmented, "T3 Code, Ottobit");
        assert_eq!(unsegmented.text, "");
    }

    #[test]
    fn speech_containing_the_hints_is_kept() {
        let mut speech = transcript(&["We shipped T3 Code today"]);
        let before = speech.clone();
        drop_prompt_echo(&mut speech, "T3 Code");
        assert_eq!(speech, before);
    }

    #[test]
    fn only_the_echo_segment_is_removed() {
        let mut mixed = transcript(&["Let's ship it.", "T3 Code, Ottobit."]);
        drop_prompt_echo(&mut mixed, "T3 Code, Ottobit");
        assert_eq!(mixed.text, "Let's ship it.");
        assert_eq!(mixed.segments.len(), 1);
        assert_eq!(words(&mixed), ["Let's", "ship", "it."]);

        let mut leading = transcript(&["T3 Code, Ottobit.", "Let's ship it."]);
        drop_prompt_echo(&mut leading, "T3 Code, Ottobit");
        assert_eq!(leading.text, "Let's ship it.");
        assert_eq!(words(&leading), ["Let's", "ship", "it."]);
        assert_eq!(
            (leading.segments[0].first_word, leading.segments[0].n_words),
            (0, 3)
        );
    }

    #[test]
    fn single_hint_dictation_is_kept() {
        let mut dictation = transcript(&["Ottobit."]);
        let before = dictation.clone();
        drop_prompt_echo(&mut dictation, "Ottobit");
        assert_eq!(dictation, before);

        let mut spaced = transcript(&["T3 Code Ottobit"]);
        let before = spaced.clone();
        drop_prompt_echo(&mut spaced, "T3 Code Ottobit");
        assert_eq!(spaced, before);
    }

    #[test]
    fn hints_without_spaces_after_commas_are_matched() {
        let mut echo = transcript(&["T3 Code, Ottobit."]);
        drop_prompt_echo(&mut echo, "T3 Code,Ottobit");
        assert_eq!(echo.text, "");

        let mut joined = transcript(&["T3 Code;Ottobit"]);
        drop_prompt_echo(&mut joined, "T3 Code\nOttobit");
        assert_eq!(joined.text, "");
    }

    #[test]
    fn a_trailing_subtitle_credit_is_dropped_from_text_segments_and_words() {
        let mut own = transcript(&["On se voit demain.", "Sous-titrage Société Radio-Canada"]);
        drop_subtitle_credit(&mut own);
        assert_eq!(own.text, "On se voit demain.");
        assert_eq!(own.segments.len(), 1);
        assert_eq!(words(&own), ["On", "se", "voit", "demain."]);

        let mut joined = transcript(&["On se voit demain. Sous-titrage Société Radio-Canada"]);
        drop_subtitle_credit(&mut joined);
        assert_eq!(joined.text, "On se voit demain.");
        assert_eq!(joined.segments[0].text, "On se voit demain.");
        assert_eq!(joined.segments[0].n_words, 4);
        assert_eq!(words(&joined), ["On", "se", "voit", "demain."]);

        let mut speech = transcript(&["Sous-titrage Société Radio-Canada, c'est la fin."]);
        let before = speech.clone();
        drop_subtitle_credit(&mut speech);
        assert_eq!(speech, before);
    }

    #[test]
    fn an_empty_prompt_drops_nothing() {
        let mut silence = transcribe_cpp::Transcript::default();
        drop_prompt_echo(&mut silence, "  ");
        assert_eq!(silence, transcribe_cpp::Transcript::default());
    }

    fn validate_auto(
        model: crate::transcription::models::TranscriptionModelId,
        supports_language_detect: bool,
    ) -> Result<Option<String>> {
        let definition = crate::transcription::models::definition(model);
        let ModelRuntime::Gguf(artifact) = definition.runtime else {
            panic!("{} is not a GGUF model", definition.name);
        };
        validate_gguf_artifact(
            Path::new("catalog.gguf"),
            definition,
            artifact.architecture,
            artifact.variant,
            &TranscriptionSelection {
                model,
                language: AUTO_LANGUAGE.into(),
            },
            supports_language_detect,
        )
    }

    #[test]
    fn auto_needs_detection_only_from_models_that_take_a_language_hint() {
        use crate::transcription::models::TranscriptionModelId;

        // Parakeet v2 takes the language as a hint and its file detects none.
        assert!(validate_auto(TranscriptionModelId::ParakeetV2, false).is_err());
        // A no-hint model never receives Auto as a hint, so the file's
        // lang_detect metadata does not gate it (Granite's pinned file says 0).
        for model in [
            TranscriptionModelId::ParakeetV3,
            TranscriptionModelId::ParakeetUltra,
            TranscriptionModelId::Orukeet,
            TranscriptionModelId::GraniteSpeech5,
        ] {
            assert_eq!(validate_auto(model, false).unwrap(), None, "{model:?}");
        }
        // A hint-taking model that detects still runs Auto with no hint.
        assert_eq!(
            validate_auto(TranscriptionModelId::WhisperLargeV3Turbo, true).unwrap(),
            None
        );
    }

    #[test]
    fn a_job_cancel_token_is_installed_only_inside_its_scope() {
        let outer = transcribe_cpp::CancelToken::new();
        let inner = transcribe_cpp::CancelToken::new();
        assert!(job_cancel().is_none());
        with_job_cancel(&outer, || {
            with_job_cancel(&inner, || {
                inner.cancel();
                assert!(job_cancel().unwrap().is_cancelled());
            });
            assert!(
                !job_cancel().unwrap().is_cancelled(),
                "the outer job is restored"
            );
        });
        assert!(job_cancel().is_none());
        let _ = std::panic::catch_unwind(|| with_job_cancel(&outer, || panic!("run failed")));
        assert!(
            job_cancel().is_none(),
            "a panicking run still uninstalls its token"
        );
    }
}
