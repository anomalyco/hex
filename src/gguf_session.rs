//! The `transcribe-cpp` session owner shared by the macOS, Linux, and Windows
//! GGUF transcribers. It recreates the native session around every run (and
//! every live stream) so retained scheduler scratch never outlives the
//! inference that sized it.

use std::borrow::Cow;
use std::cell::RefCell;
use std::path::Path;
use std::time::{Duration, Instant};

use color_eyre::eyre::{Result, WrapErr, eyre};

use crate::transcription_models::{
    AUTO_LANGUAGE, ModelDefinition, ModelRuntime, TranscriptionSelection,
};

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

/// Passes the recognition hints the way `model` reads them: Whisper as its
/// initial prompt, Qwen3-ASR as the context prompt in its system message.
/// Blank hints leave `options` untouched.
pub(crate) fn apply_recognition_hints(
    options: &mut transcribe_cpp::RunOptions,
    model: &transcribe_cpp::Model,
    definition: &ModelDefinition,
    hints: &str,
) -> Result<()> {
    let hints = hints.trim();
    if hints.is_empty() {
        return Ok(());
    }
    if !definition.supports_recognition_hints {
        return Err(eyre!("{} does not take recognition hints", definition.name));
    }
    if model.accepts_ext(
        transcribe_cpp::ExtSlot::Run,
        transcribe_cpp::sys::TRANSCRIBE_EXT_KIND_WHISPER_RUN,
    ) {
        options.family = Some(transcribe_cpp::RunExtension::Whisper(
            transcribe_cpp::WhisperRunOptions {
                initial_prompt: Some(hints.to_string()),
                ..Default::default()
            },
        ));
    } else if model.supports(transcribe_cpp::Feature::ContextPrompt) {
        options.prompt = Some(hints.to_string());
    } else {
        return Err(eyre!(
            "{} has no prompt slot for recognition hints",
            definition.name
        ));
    }
    Ok(())
}

/// The recognition hints a run primes the model with, as either prompt.
fn hint_prompt(options: &transcribe_cpp::RunOptions) -> Option<&str> {
    match &options.family {
        Some(transcribe_cpp::RunExtension::Whisper(whisper)) => whisper.initial_prompt.as_deref(),
        _ => options.prompt.as_deref(),
    }
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
    if selection.language == AUTO_LANGUAGE && !supports_language_detect {
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

/// A run this recent leaves the weights resident (ggml keeps them so for three
/// minutes), so a prewarm after it would only repeat work.
const RECENT_RUN: Duration = Duration::from_secs(60);

pub(crate) struct OfflineGgufSession {
    model: transcribe_cpp::Model,
    session: Option<transcribe_cpp::Session>,
    options: transcribe_cpp::SessionOptions,
    /// When the last run or live stream completed; `None` until the first.
    last_run: Option<Instant>,
    /// Whether runs are biased toward the vocabulary: Parakeet's transducer
    /// heads (TDT, RNN-T), which take it as a run extension.
    takes_vocabulary: bool,
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
        let takes_vocabulary = model.arch() == "parakeet"
            && model.accepts_ext(
                transcribe_cpp::ExtSlot::Run,
                transcribe_cpp::sys::TRANSCRIBE_EXT_KIND_PARAKEET_VOCABULARY,
            );
        Ok(Self {
            model,
            session: Some(session),
            options,
            last_run: None,
            takes_vocabulary,
        })
    }

    /// `options` with the vocabulary (`crate::vocabulary::transcription_bias`)
    /// added, read now so a settings change applies from the next run. The
    /// engine writes a term in place of the heard words only when the audio
    /// supports it: an RNN-T whole-clip run decodes the term alongside the
    /// greedy path, and streams and TDT heads rescore the greedy transcript.
    fn with_vocabulary<'a>(
        &self,
        options: &'a transcribe_cpp::RunOptions,
    ) -> Cow<'a, transcribe_cpp::RunOptions> {
        if !self.takes_vocabulary || options.family.is_some() {
            return Cow::Borrowed(options);
        }
        match crate::vocabulary::transcription_bias() {
            Some((terms, bias)) => Cow::Owned(transcribe_cpp::RunOptions {
                vocabulary: terms,
                family: Some(transcribe_cpp::RunExtension::ParakeetVocabulary(bias)),
                ..options.clone()
            }),
            None => Cow::Borrowed(options),
        }
    }

    /// Whether a run or live stream completed within [`RECENT_RUN`].
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
        let options = self.with_vocabulary(options);
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
        let result = keep_repetition_partial(session.run(samples, &self.with_vocabulary(options)))
            .map(|mut transcript| {
                if let Some(prompt) = hint_prompt(options) {
                    drop_prompt_echo(&mut transcript, prompt);
                }
                transcript
            });
        drop(session);
        if result.is_ok() {
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

/// Drops segments that only repeat the recognition hints. Whisper (initial
/// prompt) and Qwen3-ASR (context prompt) can return them verbatim on silence
/// or noise. Speech that merely contains a hint is kept, and so
/// is a single hint: dictating the one listed name must still paste it.
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn normalizes_spoken_numbers_and_punctuation() {
        assert_eq!(normalize_spoken("Move LEFT, three!"), "move left 3");
        assert_eq!(normalize_spoken("Ä ä A a ONE one"), "Ä ä a a 1 1");
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
    fn an_empty_prompt_drops_nothing() {
        let mut silence = transcribe_cpp::Transcript::default();
        drop_prompt_echo(&mut silence, "  ");
        assert_eq!(silence, transcribe_cpp::Transcript::default());
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
