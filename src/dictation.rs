use std::collections::{HashMap, VecDeque};
use std::time::Duration;

use rubato::audioadapter_buffers::direct::InterleavedSlice;
use rubato::{Fft, FixedSync, Indexing, Resampler, WindowFunction};

use crate::audio::CaptureInstant;
use crate::transcription_engine::{LiveStream, TranscriptionEngine};

#[cfg(target_os = "macos")]
use crate::recording_environment::{RecordingEnvironmentController, RecordingEnvironmentSession};

const VOICE_PRE_ROLL_DURATION: Duration = Duration::from_secs(1);
const TIMELINE_BUFFER_DURATION: Duration = Duration::from_secs(10);
const PRE_ROLL_DURATION: Duration = Duration::from_millis(450);
pub const MINIMUM_HOLD_DURATION: Duration = Duration::from_millis(300);
/// Audio kept after a shortcut release that finishes a capture, because a
/// release often lands while the last word is still sounding.
pub const POST_RELEASE_TAIL: Duration = Duration::from_millis(200);
const MINIMUM_PARAKEET_DURATION: Duration = Duration::from_millis(1500);
const INITIAL_RECORDING_CAPACITY: Duration = Duration::from_secs(10);
const PARAKEET_SAMPLE_RATE: u32 = 16_000;
/// A capture whose loudest 30 ms window stays below this level is treated as
/// empty rather than transcribed, because models hallucinate closing phrases
/// such as "thank you" on silence. Deliberately conservative: whispered speech
/// sits well above it, so only a quiet room's noise floor is discarded.
const SILENCE_THRESHOLD_DBFS: f32 = -50.0;
const SILENCE_WINDOW_SAMPLES: usize = 480;
/// Live audio is handed to a capture's stream in blocks of at least this many
/// 16 kHz samples (160 ms), well under the model's chunk, to keep per-feed
/// overhead low.
const STREAM_FEED_SAMPLES: usize = 2_560;

use crate::spoken_text::normalize;

const MAX_PROTOCOL_PHRASES: usize = 16;
const MAX_PROTOCOL_PHRASE_BYTES: usize = 256;

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct DictationProtocol {
    start: Vec<String>,
    stop: Vec<String>,
    send: Vec<String>,
    cancel: Vec<String>,
}

impl Default for DictationProtocol {
    fn default() -> Self {
        Self {
            start: [
                "dictate start",
                "dictates start",
                "dictate starts",
                "dictates starts",
                "start dictating",
            ]
            .map(Into::into)
            .into(),
            stop: ["dictate stop", "dictates stop"].map(Into::into).into(),
            send: [
                "dictate send",
                "dictates send",
                "dictate end",
                "dictates end",
                "dictate and",
                "dictates and",
            ]
            .map(Into::into)
            .into(),
            cancel: ["dictate cancel", "dictates cancel"].map(Into::into).into(),
        }
    }
}

impl DictationProtocol {
    pub fn try_new(
        start: Vec<String>,
        stop: Vec<String>,
        send: Vec<String>,
        cancel: Vec<String>,
    ) -> Result<Self, String> {
        let protocol = Self {
            start,
            stop,
            send,
            cancel,
        };
        let groups = [
            ("start", protocol.start.as_slice()),
            ("stop", protocol.stop.as_slice()),
            ("send", protocol.send.as_slice()),
            ("cancel", protocol.cancel.as_slice()),
        ];
        let mut normalized = HashMap::<String, &str>::new();
        for (name, phrases) in groups {
            if phrases.is_empty() || phrases.len() > MAX_PROTOCOL_PHRASES {
                return Err(format!(
                    "dictation.{name} must contain 1 through {MAX_PROTOCOL_PHRASES} phrases"
                ));
            }
            for phrase in phrases {
                if phrase.len() > MAX_PROTOCOL_PHRASE_BYTES {
                    return Err(format!(
                        "dictation.{name} phrases must be at most {MAX_PROTOCOL_PHRASE_BYTES} bytes"
                    ));
                }
                let phrase = normalize(phrase);
                if phrase.is_empty() {
                    return Err(format!("dictation.{name} phrases must not be empty"));
                }
                if let Some(existing) = normalized.insert(phrase.clone(), name) {
                    return Err(format!(
                        "dictation.{name} phrase overlaps dictation.{existing}: {phrase}"
                    ));
                }
            }
        }
        validate_boundary_overlaps("start", &protocol.start, Boundary::Prefix)?;
        let suffixes = protocol
            .stop
            .iter()
            .chain(&protocol.send)
            .chain(&protocol.cancel)
            .cloned()
            .collect::<Vec<_>>();
        validate_boundary_overlaps("control", &suffixes, Boundary::Suffix)?;
        Ok(protocol)
    }

    pub fn start_phrases(&self) -> &[String] {
        &self.start
    }

    pub fn control_phrases(&self) -> impl Iterator<Item = &String> {
        self.stop.iter().chain(&self.send).chain(&self.cancel)
    }

    pub fn control_suffix_word_count(&self, words: &[String]) -> Option<usize> {
        let words = words.iter().map(|word| normalize(word)).collect::<Vec<_>>();
        self.control_phrases().find_map(|phrase| {
            let control = spoken_words(phrase)
                .into_iter()
                .map(|word| word.normalized)
                .collect::<Vec<_>>();
            words.ends_with(&control).then_some(control.len())
        })
    }

    pub fn start_prefix(&self, text: &str) -> Option<usize> {
        control_prefix(text, &self.start)
    }

    pub fn control_suffix(&self, text: &str) -> Option<(DictationControl, usize)> {
        let words = spoken_words(text);
        for (phrases, control) in [
            (&self.cancel, DictationControl::Cancel),
            (&self.stop, DictationControl::Stop),
            (&self.send, DictationControl::Send),
        ] {
            if let Some(prefix_end) = control_suffix(&words, phrases) {
                return Some((control, prefix_end));
            }
        }
        None
    }

    pub fn strip(&self, text: &str) -> String {
        let mut text = text.trim();
        while let Some((_, prefix_end)) = self.control_suffix(text) {
            text = trim_control_end(&text[..prefix_end]);
        }
        if let Some(prefix_end) = self.start_prefix(text) {
            text = trim_control_start(&text[prefix_end..]);
            return uppercase_initial(text);
        }
        text.to_string()
    }
}

#[derive(Clone, Copy)]
enum Boundary {
    Prefix,
    Suffix,
}

fn validate_boundary_overlaps(
    name: &str,
    phrases: &[String],
    boundary: Boundary,
) -> Result<(), String> {
    let normalized = phrases
        .iter()
        .map(|phrase| normalize(phrase))
        .collect::<Vec<_>>();
    for (index, left) in normalized.iter().enumerate() {
        let left = left.split_whitespace().collect::<Vec<_>>();
        for right in &normalized[index + 1..] {
            let right = right.split_whitespace().collect::<Vec<_>>();
            let overlaps = match boundary {
                Boundary::Prefix => left.starts_with(&right) || right.starts_with(&left),
                Boundary::Suffix => left.ends_with(&right) || right.ends_with(&left),
            };
            if overlaps {
                return Err(format!(
                    "dictation {name} phrases overlap at their streaming boundary"
                ));
            }
        }
    }
    Ok(())
}

pub enum Finish {
    Discard,
    Transcribe(DictationClip),
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum DictationControl {
    Stop,
    Send,
    Cancel,
}

#[derive(Default)]
pub struct ControlStability {
    pending: Option<DictationControl>,
}

impl ControlStability {
    pub fn observe(
        &mut self,
        control: Option<DictationControl>,
        completed: bool,
    ) -> Option<DictationControl> {
        let Some(control) = control else {
            self.pending = None;
            return None;
        };
        let accepted = matches!(control, DictationControl::Cancel)
            || completed
            || self.pending == Some(control);
        self.pending = (!accepted).then_some(control);
        accepted.then_some(control)
    }

    #[cfg(test)]
    pub fn reset(&mut self) {
        self.pending = None;
    }
}

fn words_match(words: &[SpokenWord], pattern: &[SpokenWord]) -> bool {
    words.len() == pattern.len()
        && words
            .iter()
            .map(|word| &word.normalized)
            .eq(pattern.iter().map(|word| &word.normalized))
}

fn control_suffix(words: &[SpokenWord], suffixes: &[String]) -> Option<usize> {
    for suffix in suffixes {
        let suffix = spoken_words(suffix);
        if words.len() >= suffix.len() && words_match(&words[words.len() - suffix.len()..], &suffix)
        {
            return Some(words[words.len() - suffix.len()].start);
        }
    }
    None
}

fn control_prefix(text: &str, prefixes: &[String]) -> Option<usize> {
    let words = spoken_words(text);
    prefixes.iter().find_map(|prefix| {
        let prefix = spoken_words(prefix);
        (words.len() >= prefix.len() && words_match(&words[..prefix.len()], &prefix))
            .then(|| words[prefix.len() - 1].end)
    })
}

struct SpokenWord {
    start: usize,
    end: usize,
    normalized: String,
}

fn spoken_words(text: &str) -> Vec<SpokenWord> {
    let mut words = Vec::new();
    let mut start = None;
    for (index, character) in text.char_indices() {
        if character.is_alphanumeric() || character == '\'' {
            start.get_or_insert(index);
        } else if let Some(start) = start.take() {
            words.push(SpokenWord {
                start,
                end: index,
                normalized: normalize(&text[start..index]),
            });
        }
    }
    if let Some(start) = start {
        words.push(SpokenWord {
            start,
            end: text.len(),
            normalized: normalize(&text[start..]),
        });
    }
    words
}

fn trim_control_start(text: &str) -> &str {
    text.trim_start_matches(|character: char| {
        character.is_whitespace() || character.is_ascii_punctuation()
    })
}

fn trim_control_end(text: &str) -> &str {
    text.trim_end()
        .trim_end_matches([',', ';', ':', '-', '\u{2013}', '\u{2014}'])
        .trim_end()
}

fn uppercase_initial(text: &str) -> String {
    let Some((index, character)) = text
        .char_indices()
        .find(|(_, character)| character.is_alphabetic())
    else {
        return text.to_string();
    };
    if !character.is_lowercase() {
        return text.to_string();
    }
    let mut output = String::with_capacity(text.len());
    output.push_str(&text[..index]);
    output.extend(character.to_uppercase());
    output.push_str(&text[index + character.len_utf8()..]);
    output
}

/// How the dictation shortcut controls a capture.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq, serde::Deserialize, serde::Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ShortcutMode {
    /// Hold the shortcut while speaking; release to finish.
    #[default]
    Hold,
    /// Press once to start and again to finish. Escape still cancels.
    Toggle,
}

pub struct DictationClip {
    samples: Vec<f32>,
    /// How many trailing samples were captured after the release. The stop
    /// tone sounds during them, so the silence gate ignores them.
    tail_samples: usize,
    /// The live stream fed with this clip's audio during capture, if any.
    stream: Option<LiveStream>,
    /// Keeps background work deferring until the clip is submitted.
    #[cfg(target_os = "macos")]
    capture: Option<crate::recording_environment::CaptureMarker>,
}

impl DictationClip {
    pub fn duration_ms(&self) -> u64 {
        self.samples.len() as u64 * 1_000 / u64::from(PARAKEET_SAMPLE_RATE)
    }

    /// Takes the clip's live stream; finish it with the complete clip.
    pub fn take_stream(&mut self) -> Option<LiveStream> {
        self.stream.take()
    }

    /// Takes the marker that defers background work; hold it until the clip
    /// is submitted.
    #[cfg(target_os = "macos")]
    pub fn take_capture_marker(&mut self) -> Option<crate::recording_environment::CaptureMarker> {
        self.capture.take()
    }

    #[cfg(test)]
    pub fn for_test(samples: Vec<f32>) -> Self {
        Self {
            samples,
            tail_samples: 0,
            stream: None,
            #[cfg(target_os = "macos")]
            capture: None,
        }
    }

    /// The loudest 30 ms window's DC-removed RMS level in dBFS, over the
    /// captured audio through the release (before the tail and any model
    /// padding).
    fn loudest_window_dbfs(&self) -> f32 {
        let released = self.samples.len().saturating_sub(self.tail_samples);
        loudest_window_dbfs(&self.samples[..released])
    }

    /// Whether the capture holds no speech worth transcribing: its level stays
    /// below the silence threshold or, once the speech detector is installed,
    /// the detector finds no speech in it (a key click, the start tone, or a
    /// noisy room).
    pub fn is_near_silent(&self) -> bool {
        let level_dbfs = self.loudest_window_dbfs();
        if level_dbfs < SILENCE_THRESHOLD_DBFS {
            tracing::info!(
                level_dbfs,
                threshold_dbfs = SILENCE_THRESHOLD_DBFS,
                audio_ms = self.duration_ms(),
                "discarded near-silent dictation without transcribing it"
            );
            return true;
        }
        let released = self.samples.len().saturating_sub(self.tail_samples);
        let started = std::time::Instant::now();
        match crate::speech_detector::contains_speech(&self.samples[..released]) {
            Some(false) => {
                tracing::info!(
                    level_dbfs,
                    audio_ms = self.duration_ms(),
                    vad_ms = started.elapsed().as_millis() as u64,
                    "discarded dictation without detected speech"
                );
                true
            }
            Some(true) => {
                tracing::debug!(
                    audio_ms = self.duration_ms(),
                    vad_ms = started.elapsed().as_millis() as u64,
                    "speech detected in dictation"
                );
                false
            }
            None => false,
        }
    }

    #[cfg(test)]
    pub fn into_parakeet_samples(self) -> Vec<f32> {
        let mut samples = self.samples;
        pad_for_parakeet(&mut samples);
        samples
    }

    pub fn into_transcription_samples(self) -> Vec<f32> {
        self.samples
    }
}

fn loudest_window_dbfs(samples: &[f32]) -> f32 {
    let loudest = samples
        .chunks(SILENCE_WINDOW_SAMPLES)
        .map(|window| {
            let count = window.len() as f64;
            let mean = window.iter().map(|&sample| f64::from(sample)).sum::<f64>() / count;
            let variance = window
                .iter()
                .map(|&sample| (f64::from(sample) - mean).powi(2))
                .sum::<f64>()
                / count;
            variance.sqrt()
        })
        .fold(0.0, f64::max);
    (20.0 * loudest.max(1e-6).log10()) as f32
}

pub(crate) fn pad_for_parakeet(samples: &mut Vec<f32>) {
    samples.resize(
        samples
            .len()
            .max(samples_for(MINIMUM_PARAKEET_DURATION, PARAKEET_SAMPLE_RATE)),
        0.0,
    );
}

pub fn resample_for_parakeet(samples: &[f32], source_rate: u32) -> Vec<f32> {
    try_resample_for_parakeet(samples, source_rate)
        .expect("microphone sample rate must be resampleable")
}

pub fn try_resample_for_parakeet(samples: &[f32], source_rate: u32) -> Result<Vec<f32>, String> {
    let mut recording = Recording::try_new(CaptureInstant::ZERO, source_rate)?;
    recording.push(samples);
    Ok(recording.finish(None))
}

/// Incremental [`resample_for_parakeet`] for long recordings that must not be
/// held in memory. Output excludes the resampler delay, and the total length
/// after `finish` matches the whole-buffer conversion.
pub struct StreamingResampler {
    source_rate: u32,
    resampler: Option<Fft<f32>>,
    input: Vec<f32>,
    source_samples: usize,
    delay_remaining: usize,
    produced: usize,
}

impl StreamingResampler {
    pub fn new(source_rate: u32) -> Result<Self, String> {
        let recording = Recording::try_new(CaptureInstant::ZERO, source_rate)?;
        let delay_remaining = recording.resampler.as_ref().map_or(0, Fft::output_delay);
        Ok(Self {
            source_rate,
            resampler: recording.resampler,
            input: Vec::with_capacity(Recording::CHUNK_SIZE),
            source_samples: 0,
            delay_remaining,
            produced: 0,
        })
    }

    pub fn push(&mut self, mut samples: &[f32]) -> Vec<f32> {
        self.source_samples += samples.len();
        let Some(resampler) = &mut self.resampler else {
            self.produced += samples.len();
            return samples.to_vec();
        };
        let mut output = Vec::new();
        while !samples.is_empty() {
            let take = samples.len().min(Recording::CHUNK_SIZE - self.input.len());
            self.input.extend_from_slice(&samples[..take]);
            samples = &samples[take..];
            if self.input.len() == Recording::CHUNK_SIZE {
                output.extend(resample_chunk(resampler, &self.input, false));
                self.input.clear();
            }
        }
        self.trim_delay(output)
    }

    pub fn finish(mut self) -> Vec<f32> {
        let expected =
            self.source_samples * PARAKEET_SAMPLE_RATE as usize / self.source_rate as usize;
        let Some(mut resampler) = self.resampler.take() else {
            return Vec::new();
        };
        let mut output = Vec::new();
        if !self.input.is_empty() {
            let flushed = resample_chunk(&mut resampler, &self.input, true);
            output.extend(self.trim_delay(flushed));
        }
        for _ in 0..8 {
            if self.produced >= expected {
                break;
            }
            let flushed = resample_chunk(&mut resampler, &[], true);
            output.extend(self.trim_delay(flushed));
        }
        let excess = self.produced.saturating_sub(expected);
        output.truncate(output.len().saturating_sub(excess));
        output
    }

    fn trim_delay(&mut self, mut output: Vec<f32>) -> Vec<f32> {
        let trimmed = self.delay_remaining.min(output.len());
        output.drain(..trimmed);
        self.delay_remaining -= trimmed;
        self.produced += output.len();
        output
    }
}

pub struct DictationCapture {
    sample_rate: u32,
    ring: VecDeque<f32>,
    ring_captured_through: Option<CaptureInstant>,
    recording: Option<Recording>,
    #[cfg(target_os = "macos")]
    recording_environment: Option<RecordingEnvironmentController>,
}

struct Recording {
    started_at: CaptureInstant,
    intentional: bool,
    samples: Vec<f32>,
    source_samples: usize,
    source_rate: u32,
    recorded_through: Option<CaptureInstant>,
    input_buffer: Vec<f32>,
    resampler: Option<Fft<f32>>,
    stream: Option<LiveStream>,
    #[cfg(target_os = "macos")]
    environment: RecordingEnvironmentState,
    #[cfg(target_os = "macos")]
    capture: Option<crate::recording_environment::CaptureMarker>,
}

#[cfg(target_os = "macos")]
enum RecordingEnvironmentState {
    Disabled,
    Pending(RecordingEnvironmentController),
    Active {
        _session: RecordingEnvironmentSession,
    },
}

#[cfg(target_os = "macos")]
impl RecordingEnvironmentState {
    fn activate(&mut self) {
        if let Self::Pending(controller) = self {
            *self = Self::Active {
                _session: controller.begin(),
            };
        }
    }
}

impl Recording {
    const CHUNK_SIZE: usize = 1024;

    fn new(started_at: CaptureInstant, source_rate: u32) -> Self {
        Self::try_new(started_at, source_rate).expect("microphone sample rate must be resampleable")
    }

    fn try_new(started_at: CaptureInstant, source_rate: u32) -> Result<Self, String> {
        let resampler = (source_rate != PARAKEET_SAMPLE_RATE)
            .then(|| {
                Fft::new_custom(
                    source_rate as usize,
                    PARAKEET_SAMPLE_RATE as usize,
                    Self::CHUNK_SIZE,
                    1,
                    1,
                    WindowFunction::BlackmanHarris2,
                    FixedSync::Input,
                )
                .map_err(|error| error.to_string())
            })
            .transpose()?;
        Ok(Self {
            started_at,
            intentional: false,
            samples: Vec::with_capacity(samples_for(
                INITIAL_RECORDING_CAPACITY,
                PARAKEET_SAMPLE_RATE,
            )),
            source_samples: 0,
            source_rate,
            recorded_through: None,
            input_buffer: Vec::with_capacity(Self::CHUNK_SIZE),
            resampler,
            stream: None,
            #[cfg(target_os = "macos")]
            environment: RecordingEnvironmentState::Disabled,
            #[cfg(target_os = "macos")]
            capture: None,
        })
    }

    fn push(&mut self, mut samples: &[f32]) {
        self.source_samples += samples.len();
        let Some(resampler) = &mut self.resampler else {
            self.samples.extend_from_slice(samples);
            return;
        };

        while !samples.is_empty() {
            let take = samples
                .len()
                .min(Self::CHUNK_SIZE - self.input_buffer.len());
            self.input_buffer.extend_from_slice(&samples[..take]);
            samples = &samples[take..];
            if self.input_buffer.len() == Self::CHUNK_SIZE {
                self.samples
                    .extend(resample_chunk(resampler, &self.input_buffer, false));
                self.input_buffer.clear();
            }
        }
    }

    fn push_through(&mut self, samples: &[f32], captured_through: CaptureInstant) {
        self.push(samples);
        self.recorded_through = Some(captured_through);
    }

    /// Hands the stream every complete block of audio it has not seen, in
    /// final-clip coordinates (after the resampler delay).
    fn feed_stream(&mut self) {
        let delay = self.resampler.as_ref().map_or(0, Fft::output_delay);
        let Some(stream) = &mut self.stream else {
            return;
        };
        let recorded = &self.samples[delay.min(self.samples.len())..];
        let fresh = &recorded[stream.fed().min(recorded.len())..];
        if fresh.len() >= STREAM_FEED_SAMPLES {
            stream.feed(fresh);
        }
    }

    fn finish(mut self, ended_at: Option<CaptureInstant>) -> Vec<f32> {
        if let (Some(ended_at), Some(recorded_through)) = (ended_at, self.recorded_through)
            && let Some(post_release) = recorded_through.checked_duration_since(ended_at)
        {
            self.source_samples = self
                .source_samples
                .saturating_sub(samples_for(post_release, self.source_rate));
        }
        if let Some(resampler) = &mut self.resampler {
            let expected_samples =
                self.source_samples * PARAKEET_SAMPLE_RATE as usize / self.source_rate as usize;
            if !self.input_buffer.is_empty() {
                self.samples
                    .extend(resample_chunk(resampler, &self.input_buffer, true));
            }
            let output_delay = resampler.output_delay();
            for _ in 0..8 {
                if self.samples.len() >= expected_samples + output_delay {
                    break;
                }
                self.samples.extend(resample_chunk(resampler, &[], true));
            }
            self.samples.drain(..output_delay.min(self.samples.len()));
            self.samples.truncate(expected_samples);
        } else {
            self.samples.truncate(self.source_samples);
        }
        self.samples
    }
}

/// Resamples one mono chunk. A partial chunk is padded with silence, so an
/// empty partial chunk flushes the resampler's delay.
fn resample_chunk(resampler: &mut Fft<f32>, samples: &[f32], partial: bool) -> Vec<f32> {
    let input =
        InterleavedSlice::new(samples, 1, samples.len()).expect("mono chunk fits its slice");
    let indexing = partial.then(|| Indexing {
        partial_len: Some(samples.len()),
        ..Indexing::default()
    });
    resampler
        .process(&input, indexing.as_ref())
        .expect("fixed-rate resampling must succeed")
        .take_data()
}

impl DictationCapture {
    pub fn new(sample_rate: u32) -> Self {
        Self {
            sample_rate,
            ring: VecDeque::with_capacity(samples_for(TIMELINE_BUFFER_DURATION, sample_rate)),
            ring_captured_through: None,
            recording: None,
            #[cfg(target_os = "macos")]
            recording_environment: None,
        }
    }

    #[cfg(target_os = "macos")]
    pub fn enable_recording_environment(&mut self, controller: RecordingEnvironmentController) {
        self.recording_environment = Some(controller);
    }

    #[cfg(test)]
    pub fn keep_warm(&mut self, samples: &[f32]) {
        self.keep_warm_for(samples, TIMELINE_BUFFER_DURATION);
    }

    fn keep_warm_for(&mut self, samples: &[f32], retention: Duration) {
        let capacity = samples_for(retention, self.sample_rate);
        if samples.len() >= capacity {
            self.ring.clear();
            self.ring.extend(&samples[samples.len() - capacity..]);
            return;
        }
        self.ring.drain(
            ..self
                .ring
                .len()
                .saturating_add(samples.len())
                .saturating_sub(capacity),
        );
        self.ring.extend(samples);
    }

    pub fn is_recording(&self) -> bool {
        self.recording.is_some()
    }

    #[cfg(any(test, target_os = "linux"))]
    pub fn start(&mut self, now: CaptureInstant) {
        self.start_with_pre_roll(now, PRE_ROLL_DURATION, false);
    }

    #[cfg(test)]
    pub fn start_voice(&mut self, now: CaptureInstant) {
        self.start_with_pre_roll(now, VOICE_PRE_ROLL_DURATION, true);
    }

    pub fn start_at(&mut self, occurred_at: CaptureInstant) {
        self.start_from_timeline(occurred_at, PRE_ROLL_DURATION, false)
    }

    pub fn start_programmatic_at(&mut self, occurred_at: CaptureInstant) {
        self.start_from_timeline(occurred_at, PRE_ROLL_DURATION, true)
    }

    pub fn start_voice_at(&mut self, occurred_at: CaptureInstant) {
        self.start_from_timeline(occurred_at, VOICE_PRE_ROLL_DURATION, true)
    }

    fn start_from_timeline(
        &mut self,
        occurred_at: CaptureInstant,
        pre_roll: Duration,
        intentional: bool,
    ) {
        let delayed_audio = self
            .ring_captured_through
            .and_then(|through| through.checked_duration_since(occurred_at))
            .unwrap_or_default();
        self.start_with_pre_roll(
            occurred_at,
            pre_roll.saturating_add(delayed_audio),
            intentional,
        );
        if let (Some(recording), Some(through)) = (&mut self.recording, self.ring_captured_through)
        {
            recording.recorded_through = Some(through);
        }
    }

    fn start_with_pre_roll(&mut self, now: CaptureInstant, duration: Duration, intentional: bool) {
        let pre_roll = samples_for(duration, self.sample_rate).min(self.ring.len());
        let mut recording = Recording::new(now, self.sample_rate);
        recording.intentional = intentional;
        #[cfg(target_os = "macos")]
        {
            recording.environment = match &self.recording_environment {
                Some(controller) if intentional => RecordingEnvironmentState::Active {
                    _session: controller.begin(),
                },
                Some(controller) => RecordingEnvironmentState::Pending(controller.clone()),
                None => RecordingEnvironmentState::Disabled,
            };
            recording.capture = self
                .recording_environment
                .is_some()
                .then(crate::recording_environment::CaptureMarker::new);
        }
        let skip = self.ring.len() - pre_roll;
        let (front, back) = self.ring.as_slices();
        if skip < front.len() {
            recording.push(&front[skip..]);
            recording.push(back);
        } else {
            recording.push(&back[skip - front.len()..]);
        }
        self.recording = Some(recording);
    }

    #[cfg(test)]
    pub fn push(&mut self, samples: &[f32], now: CaptureInstant) -> bool {
        let became_intentional = self.become_intentional(now);
        if let Some(recording) = &mut self.recording {
            recording.push(samples);
        }
        became_intentional
    }

    #[cfg(any(test, target_os = "linux"))]
    pub fn ingest(&mut self, samples: &[f32], captured_through: CaptureInstant) {
        self.ingest_with_pending(samples, captured_through, None)
    }

    pub fn ingest_with_pending(
        &mut self,
        samples: &[f32],
        captured_through: CaptureInstant,
        oldest_pending: Option<CaptureInstant>,
    ) {
        if let Some(recording) = &mut self.recording {
            recording.push_through(samples, captured_through);
            recording.feed_stream();
        }
        let retention = oldest_pending
            .map(|pending| {
                captured_through
                    .duration_since(pending)
                    .saturating_add(VOICE_PRE_ROLL_DURATION)
                    .max(TIMELINE_BUFFER_DURATION)
            })
            .unwrap_or(TIMELINE_BUFFER_DURATION);
        self.keep_warm_for(samples, retention);
        self.ring_captured_through = Some(captured_through);
    }

    pub fn become_intentional(&mut self, now: CaptureInstant) -> bool {
        let Some(recording) = &mut self.recording else {
            return false;
        };
        let became_intentional = !recording.intentional
            && now.duration_since(recording.started_at) >= MINIMUM_HOLD_DURATION;
        recording.intentional |= became_intentional;
        #[cfg(target_os = "macos")]
        if became_intentional {
            recording.environment.activate();
        }
        became_intentional
    }

    /// Ends the active recording's environment (mute, paused media, idle-sleep
    /// prevention) before the capture finishes, and runs `then` once a muted
    /// output is audible again. A recording that never became intentional
    /// cannot start the environment afterwards.
    #[cfg(target_os = "macos")]
    pub fn end_recording_environment_then(&mut self, then: impl FnOnce() + Send + Sync + 'static) {
        let environment = self.recording.as_mut().map(|recording| {
            std::mem::replace(
                &mut recording.environment,
                RecordingEnvironmentState::Disabled,
            )
        });
        match environment {
            Some(RecordingEnvironmentState::Active { _session: session }) => session.end_then(then),
            _ => then(),
        }
    }

    /// The start of an active recording that has not yet passed the hold threshold.
    pub fn tentative_start(&self) -> Option<CaptureInstant> {
        self.recording
            .as_ref()
            .filter(|recording| {
                !recording.intentional
                    && recording.recorded_through.is_none_or(|through| {
                        through.duration_since(recording.started_at) < MINIMUM_HOLD_DURATION
                    })
            })
            .map(|recording| recording.started_at)
    }

    pub fn cancel(&mut self) {
        self.recording = None;
    }

    /// 16 kHz audio recorded so far, from `start` in final-clip coordinates.
    /// The resampler's tail is only flushed by finishing, so the clip may end
    /// slightly later than what this has returned.
    #[cfg(test)]
    pub fn recorded_from(&self, start: usize) -> &[f32] {
        let Some(recording) = &self.recording else {
            return &[];
        };
        let delay = recording.resampler.as_ref().map_or(0, Fft::output_delay);
        let available = &recording.samples[delay.min(recording.samples.len())..];
        &available[start.min(available.len())..]
    }

    /// Whether the active recording has passed the hold threshold (or never
    /// needed one).
    pub fn is_intentional(&self) -> bool {
        self.recording
            .as_ref()
            .is_some_and(|recording| recording.intentional)
    }

    #[cfg(test)]
    pub fn is_streaming(&self) -> bool {
        self.recording
            .as_ref()
            .is_some_and(|recording| recording.stream.is_some())
    }

    /// Opens a live stream for the active recording when the engine's model
    /// can stream. The stream receives the recording's 16 kHz audio from its
    /// first sample (pre-roll included) as it arrives, travels with the
    /// finished clip, and is cancelled with the recording.
    pub fn begin_stream(&mut self, engine: &TranscriptionEngine) {
        let Some(recording) = &mut self.recording else {
            return;
        };
        if recording.stream.is_none() {
            recording.stream = engine.begin();
            recording.feed_stream();
        }
    }

    #[cfg(test)]
    pub fn finish(&mut self, now: CaptureInstant) -> Finish {
        self.finish_with_tail(now, Duration::ZERO)
    }

    /// Whether finishing at `ended_at` would produce a clip rather than a
    /// short-tap discard.
    pub fn would_transcribe(&self, ended_at: CaptureInstant) -> bool {
        self.recording.as_ref().is_some_and(|recording| {
            recording.intentional
                || ended_at.duration_since(recording.started_at) >= MINIMUM_HOLD_DURATION
        })
    }

    /// Finishes at the exact `ended_at` event, keeping audio through
    /// `ended_at + tail`. The hold threshold is judged on `ended_at` alone.
    pub fn finish_with_tail(&mut self, ended_at: CaptureInstant, tail: Duration) -> Finish {
        if !self.would_transcribe(ended_at) {
            self.recording = None;
            return Finish::Discard;
        }
        let Some(mut recording) = self.recording.take() else {
            return Finish::Discard;
        };

        let stream = recording.stream.take();
        let kept_tail = recording
            .recorded_through
            .and_then(|through| through.checked_duration_since(ended_at))
            .map_or(Duration::ZERO, |after| after.min(tail));
        #[cfg(target_os = "macos")]
        let capture = recording.capture.take();
        let samples = recording.finish(Some(ended_at + tail));
        Finish::Transcribe(DictationClip {
            tail_samples: samples_for(kept_tail, PARAKEET_SAMPLE_RATE).min(samples.len()),
            samples,
            stream,
            #[cfg(target_os = "macos")]
            capture,
        })
    }
}

fn samples_for(duration: Duration, sample_rate: u32) -> usize {
    (duration.as_secs_f64() * f64::from(sample_rate)).round() as usize
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tone(amplitude: f32, samples: usize) -> impl Iterator<Item = f32> {
        (0..samples).map(move |index| {
            amplitude * (2.0 * std::f32::consts::PI * 220.0 * index as f32 / 16_000.0).sin()
        })
    }

    #[test]
    fn streaming_resampler_matches_whole_buffer_conversion() {
        for rate in [16_000, 44_100, 48_000] {
            let source: Vec<f32> = (0..rate as usize * 3 + 777)
                .map(|index| (index as f32 * 0.013).sin() * 0.4)
                .collect();
            let whole = resample_for_parakeet(&source, rate);
            let mut streaming = StreamingResampler::new(rate).unwrap();
            let mut output = Vec::new();
            for block in source.chunks(1_337) {
                output.extend(streaming.push(block));
            }
            output.extend(streaming.finish());
            assert_eq!(output.len(), whole.len(), "rate {rate}");
            assert!(
                output
                    .iter()
                    .zip(&whole)
                    .all(|(left, right)| (left - right).abs() < 1e-5),
                "rate {rate}"
            );
        }
    }

    #[test]
    fn recorded_audio_is_a_prefix_of_the_finished_clip() {
        for rate in [16_000, 44_100, 48_000] {
            let mut capture = DictationCapture::new(rate);
            let origin = CaptureInstant::from_nanos(1_000_000_000);
            capture.start(origin);
            let source: Vec<f32> = (0..rate as usize * 2)
                .map(|index| (index as f32 * 0.021).sin() * 0.3)
                .collect();
            let mut streamed = Vec::new();
            for (index, block) in source.chunks(480).enumerate() {
                let through = origin + crate::audio::duration_for_frames((index + 1) * 480, rate);
                capture.ingest(block, through);
                let fresh = capture.recorded_from(streamed.len()).to_vec();
                streamed.extend(fresh);
            }
            let Finish::Transcribe(clip) = capture.finish(origin + Duration::from_secs(5)) else {
                panic!("a two-second hold transcribes");
            };
            let clip = clip.into_transcription_samples();
            assert!(streamed.len() <= clip.len(), "rate {rate}");
            assert!(clip.len() - streamed.len() < 1_024, "rate {rate}");
            assert_eq!(&clip[..streamed.len()], &streamed[..], "rate {rate}");
        }
    }

    #[test]
    fn silence_gate_discards_only_near_silent_captures() {
        let clip = |samples: Vec<f32>| DictationClip::for_test(samples);

        assert!(clip(vec![0.0; 16_000]).is_near_silent());
        assert!(clip(Vec::new()).is_near_silent());
        // A -66 dBFS noise floor and a constant DC offset carry no speech.
        assert!(clip(tone(0.000_7, 16_000).collect()).is_near_silent());
        assert!(clip(vec![0.2; 16_000]).is_near_silent());

        // 100 ms of whisper-level (-43 dBFS RMS) audio inside a second of
        // silence is kept.
        let mut quiet_speech = vec![0.0; 8_000];
        quiet_speech.extend(tone(0.01, 1_600));
        quiet_speech.extend(vec![0.0; 6_400]);
        let quiet_speech = clip(quiet_speech);
        assert!(!quiet_speech.is_near_silent());
        assert!((quiet_speech.loudest_window_dbfs() - -43.0).abs() < 0.5);
        assert!(!clip(tone(0.1, 16_000).collect()).is_near_silent());
    }

    #[test]
    fn speech_detector_discards_loud_captures_without_speech() {
        let clip = |samples: Vec<f32>| DictationClip::for_test(samples);
        {
            let _detector = crate::speech_detector::override_for_test(Some(false));
            assert!(clip(tone(0.1, 16_000).collect()).is_near_silent());
        }
        let _detector = crate::speech_detector::override_for_test(Some(true));
        assert!(!clip(tone(0.1, 16_000).collect()).is_near_silent());
        // The level gate runs first: a silent capture never reaches the
        // detector.
        assert!(clip(vec![0.0; 16_000]).is_near_silent());
    }

    #[test]
    fn speech_detector_cost_is_logged_when_speech_passes() {
        #[derive(Clone, Default)]
        struct Log(std::sync::Arc<std::sync::Mutex<Vec<u8>>>);
        impl std::io::Write for Log {
            fn write(&mut self, bytes: &[u8]) -> std::io::Result<usize> {
                self.0.lock().unwrap().extend_from_slice(bytes);
                Ok(bytes.len())
            }
            fn flush(&mut self) -> std::io::Result<()> {
                Ok(())
            }
        }
        let log = Log::default();
        let writer = log.clone();
        let subscriber = tracing_subscriber::fmt()
            .with_max_level(tracing::Level::DEBUG)
            .with_ansi(false)
            .with_writer(move || writer.clone())
            .finish();
        let _detector = crate::speech_detector::override_for_test(Some(true));
        tracing::subscriber::with_default(subscriber, || {
            // Parallel tests may have cached this callsite as disabled while
            // no subscriber was set on their thread.
            tracing::callsite::rebuild_interest_cache();
            assert!(!DictationClip::for_test(tone(0.1, 16_000).collect()).is_near_silent());
        });
        let output = String::from_utf8(log.0.lock().unwrap().clone()).unwrap();
        assert!(output.contains("audio_ms=1000"), "{output}");
        assert!(output.contains("vad_ms="), "{output}");
    }

    fn capture_time() -> CaptureInstant {
        CaptureInstant::from_nanos(60_000_000_000)
    }

    #[test]
    fn quick_option_tap_discards() {
        let mut capture = DictationCapture::new(16_000);
        let start = capture_time();
        capture.start(start);
        let _ = capture.push(&vec![0.5; 1_600], start + Duration::from_millis(100));
        assert!(matches!(
            capture.finish(start + Duration::from_millis(100)),
            Finish::Discard
        ));
    }

    #[test]
    fn explicit_programmatic_capture_is_not_a_shortcut_tap() {
        let mut capture = DictationCapture::new(16_000);
        let start = capture_time();
        capture.start_programmatic_at(start);
        capture.ingest(&[0.5; 1_600], start + Duration::from_millis(100));

        let Finish::Transcribe(clip) = capture.finish(start + Duration::from_millis(100)) else {
            panic!("expected transcription")
        };
        assert_eq!(clip.duration_ms(), 100);
    }

    #[test]
    fn modifier_capture_becomes_intentional_only_after_the_hold_threshold() {
        let mut capture = DictationCapture::new(16_000);
        let start = capture_time();
        capture.start(start);

        assert!(!capture.push(&[0.5; 80], start + Duration::from_millis(299)));
        assert!(capture.push(&[0.5; 80], start + MINIMUM_HOLD_DURATION));
        assert!(!capture.push(&[0.5; 80], start + Duration::from_millis(301)));
    }

    #[test]
    fn release_can_commit_the_intentional_transition_between_audio_chunks() {
        let mut capture = DictationCapture::new(16_000);
        let start = capture_time();
        capture.start(start);

        assert!(!capture.push(&[0.5; 80], start + Duration::from_millis(299)));
        assert!(capture.become_intentional(start + MINIMUM_HOLD_DURATION));
        assert!(!capture.become_intentional(start + Duration::from_millis(301)));
    }

    #[test]
    fn delayed_press_reconstructs_audio_from_the_original_boundary() {
        let mut capture = DictationCapture::new(16_000);
        let origin = capture_time();
        for index in 1..=16 {
            capture.ingest(
                &vec![index as f32; 1_600],
                origin + Duration::from_millis(index * 100),
            );
        }

        capture.start_at(origin + Duration::from_secs(1));
        for index in 17..=20 {
            capture.ingest(
                &vec![index as f32; 1_600],
                origin + Duration::from_millis(index * 100),
            );
        }

        let Finish::Transcribe(clip) = capture.finish(origin + Duration::from_millis(1_800)) else {
            panic!("expected transcription")
        };
        assert_eq!(clip.duration_ms(), 1_250);
        assert_eq!(clip.samples.first(), Some(&6.0));
    }

    #[test]
    fn delayed_release_removes_audio_captured_after_the_physical_release() {
        let mut capture = DictationCapture::new(16_000);
        let origin = capture_time();
        capture.ingest(&vec![0.25; 16_000], origin);
        capture.start_at(origin);
        for index in 1..=12 {
            capture.ingest(
                &vec![index as f32; 1_600],
                origin + Duration::from_millis(index * 100),
            );
        }

        let Finish::Transcribe(clip) = capture.finish(origin + Duration::from_secs(1)) else {
            panic!("expected transcription")
        };
        assert_eq!(clip.duration_ms(), 1_450);
        assert!(!clip.samples.contains(&11.0));
        assert!(!clip.samples.contains(&12.0));
    }

    #[test]
    fn release_tail_keeps_audio_through_the_tail_and_no_further() {
        let mut capture = DictationCapture::new(16_000);
        let origin = capture_time();
        capture.ingest(&vec![0.25; 16_000], origin);
        capture.start_at(origin);
        for index in 1..=14 {
            capture.ingest(
                &vec![index as f32; 1_600],
                origin + Duration::from_millis(index * 100),
            );
        }

        let Finish::Transcribe(clip) =
            capture.finish_with_tail(origin + Duration::from_secs(1), POST_RELEASE_TAIL)
        else {
            panic!("expected transcription")
        };
        assert_eq!(clip.duration_ms(), 1_650);
        assert_eq!(clip.samples.last(), Some(&12.0));
        assert!(!clip.samples.contains(&13.0));
        assert_eq!(clip.tail_samples, 3_200);
    }

    #[test]
    fn the_stop_tone_in_the_tail_does_not_make_silence_transcribe() {
        let mut capture = DictationCapture::new(16_000);
        let origin = capture_time();
        capture.start_at(origin);
        let released = origin + Duration::from_secs(1);
        capture.ingest(&vec![0.0; 16_000], released);
        capture.ingest(
            &tone(0.1, 3_200).collect::<Vec<_>>(),
            released + POST_RELEASE_TAIL,
        );
        let Finish::Transcribe(clip) = capture.finish_with_tail(released, POST_RELEASE_TAIL) else {
            panic!("expected transcription")
        };
        assert_eq!(clip.duration_ms(), 1_200);
        assert!(clip.is_near_silent());
    }

    #[test]
    fn short_tap_discards_on_its_physical_release_despite_the_tail() {
        let origin = capture_time();
        let mut capture = DictationCapture::new(16_000);
        capture.start_at(origin);
        capture.ingest(&vec![0.5; 16_000], origin + Duration::from_secs(1));
        // The tail would carry the boundary past the hold threshold.
        assert!(!capture.would_transcribe(origin + Duration::from_millis(299)));
        assert!(matches!(
            capture.finish_with_tail(origin + Duration::from_millis(299), POST_RELEASE_TAIL),
            Finish::Discard
        ));
        assert!(!capture.is_recording());
    }

    #[test]
    fn minimum_hold_uses_source_event_times_after_a_processing_stall() {
        let origin = capture_time();
        let mut short = DictationCapture::new(16_000);
        short.start_at(origin);
        short.ingest(&vec![0.5; 16_000], origin + Duration::from_secs(2));
        assert!(matches!(
            short.finish(origin + Duration::from_millis(299)),
            Finish::Discard
        ));

        let mut accepted = DictationCapture::new(16_000);
        accepted.start_at(origin);
        accepted.ingest(&vec![0.5; 16_000], origin + Duration::from_secs(2));
        assert!(matches!(
            accepted.finish(origin + MINIMUM_HOLD_DURATION),
            Finish::Transcribe(_)
        ));
    }

    #[test]
    fn pending_input_extends_history_for_an_arbitrary_coordinator_stall() {
        let mut capture = DictationCapture::new(16_000);
        let origin = capture_time();
        capture.ingest(&vec![0.25; 16_000], origin + Duration::from_secs(1));
        let pressed_at = origin + Duration::from_secs(1);
        for second in 2..=31 {
            capture.ingest_with_pending(
                &vec![second as f32; 16_000],
                origin + Duration::from_secs(second),
                Some(pressed_at),
            );
        }

        capture.start_at(pressed_at);
        capture.ingest(&vec![32.0; 16_000], origin + Duration::from_secs(32));
        assert_eq!(capture.ring.len(), 160_000);
        assert_eq!(capture.ring.front(), Some(&23.0));
        assert_eq!(capture.ring.back(), Some(&32.0));

        let Finish::Transcribe(clip) = capture.finish(origin + Duration::from_secs(31)) else {
            panic!("expected transcription")
        };
        assert_eq!(clip.duration_ms(), 30_450);
        assert_eq!(clip.samples.first(), Some(&0.25));
    }

    #[test]
    fn warm_buffer_accepts_chunks_before_reaching_capacity() {
        let mut capture = DictationCapture::new(16_000);
        capture.keep_warm(&[0.25; 480]);
        capture.keep_warm(&[0.5; 480]);
        assert_eq!(capture.ring.len(), 960);
    }

    #[test]
    fn intentional_hold_includes_pre_roll_and_parakeet_padding() {
        let mut capture = DictationCapture::new(16_000);
        capture.keep_warm(&vec![0.25; 16_000]);
        let start = capture_time();
        capture.start(start);
        let _ = capture.push(&vec![0.5; 8_000], start + Duration::from_millis(500));
        let Finish::Transcribe(clip) = capture.finish(start + Duration::from_millis(500)) else {
            panic!("expected transcription")
        };
        assert_eq!(clip.duration_ms(), 950);
        let samples = clip.into_parakeet_samples();
        assert_eq!(samples.len(), 24_000);
        assert_eq!(samples[0], 0.25);
        assert!(samples.contains(&0.5));
    }

    #[test]
    fn voice_start_recovers_audio_consumed_while_recognizing_the_trigger() {
        let mut capture = DictationCapture::new(16_000);
        capture.keep_warm(&[0.25; 8_000]);
        capture.keep_warm(&[0.25; 8_000]);
        let start = capture_time();

        capture.start_voice(start);
        let _ = capture.push(&[0.5; 8_000], start + Duration::from_millis(500));

        let Finish::Transcribe(clip) = capture.finish(start + Duration::from_millis(500)) else {
            panic!("expected transcription")
        };
        let samples = clip.into_parakeet_samples();
        assert_eq!(samples[0], 0.25);
        assert!(samples.contains(&0.5));
    }

    #[test]
    fn capture_continues_past_sixty_seconds_until_explicitly_finished() {
        let mut capture = DictationCapture::new(16_000);
        let start = capture_time();
        capture.start(start);
        let _ = capture.push(&vec![0.5; 61 * 16_000], start + Duration::from_secs(61));

        let Finish::Transcribe(clip) = capture.finish(start + Duration::from_secs(61)) else {
            panic!("expected transcription")
        };
        assert_eq!(clip.duration_ms(), 61_000);
    }

    #[test]
    fn resampling_flushes_the_end_of_the_recording() {
        for (sample_rate, input_len) in [
            (44_100, 1_337),
            (44_100, 4_410),
            (48_000, 1_024),
            (48_000, 4_800),
        ] {
            let mut recording = Recording::new(capture_time(), sample_rate);
            recording.push(&vec![0.5; input_len]);
            let samples = recording.finish(None);
            let expected = input_len * PARAKEET_SAMPLE_RATE as usize / sample_rate as usize;

            assert_eq!(samples.len(), expected);
            assert!(samples.last().is_some_and(|sample| *sample > 0.25));
        }
    }

    /// Speech with pauses ending near 4.3 s and 9.3 s, at `sample_rate`.
    fn paused_speech(sample_rate: u32) -> Vec<f32> {
        let speech = crate::pause_segmenter::tests::utterance(
            &[(4_000, 1_000), (4_000, 1_000), (2_000, 0)],
            -70.0,
        );
        let ratio = f64::from(PARAKEET_SAMPLE_RATE) / f64::from(sample_rate);
        let frames = speech.len() * sample_rate as usize / PARAKEET_SAMPLE_RATE as usize;
        (0..frames)
            .map(|frame| {
                let position = frame as f64 * ratio;
                let index = position as usize;
                let next = speech.get(index + 1).copied().unwrap_or(speech[index]);
                let fraction = (position - index as f64) as f32;
                speech[index] * (1.0 - fraction) + next * fraction
            })
            .collect()
    }

    /// Audio handed to streams, in order, and the streams cancelled.
    fn stream_traffic(
        commands: &std::sync::mpsc::Receiver<crate::transcription_engine::Command>,
    ) -> (Vec<f32>, usize, usize) {
        use crate::transcription_engine::Command;
        let (mut fed, mut begun, mut cancelled) = (Vec::new(), 0, 0);
        for command in commands.try_iter() {
            match command {
                Command::Begin { .. } => begun += 1,
                Command::Feed { samples, .. } => fed.extend(samples),
                Command::Cancel { .. } => cancelled += 1,
                _ => {}
            }
        }
        (fed, begun, cancelled)
    }

    #[test]
    fn resampled_clips_keep_the_last_word_through_a_delayed_release() {
        for sample_rate in [44_100, 48_000] {
            let audio = paused_speech(sample_rate);
            // Release 300 ms before the audio ends, inside the final word,
            // after the chunk carrying that audio and its tail has arrived.
            let released_ms = audio.len() as u64 * 1_000 / u64::from(sample_rate) - 300;
            let kept_ms = released_ms + POST_RELEASE_TAIL.as_millis() as u64;
            let kept = kept_ms as usize * sample_rate as usize / 1_000;
            let expected = resample_for_parakeet(&audio[..kept], sample_rate);
            let mut clips = Vec::new();
            for streamed in [false, true] {
                let (engine, commands) = TranscriptionEngine::for_test();
                let mut capture = DictationCapture::new(sample_rate);
                let start = capture_time();
                capture.start(start);
                assert!(capture.become_intentional(start + MINIMUM_HOLD_DURATION));
                if streamed {
                    capture.begin_stream(&engine);
                }
                let mut through = start;
                for chunk in audio.chunks(sample_rate as usize / 100) {
                    through = through + Duration::from_millis(10);
                    capture.ingest(chunk, through);
                }
                let Finish::Transcribe(mut clip) = capture.finish_with_tail(
                    start + Duration::from_millis(released_ms),
                    POST_RELEASE_TAIL,
                ) else {
                    panic!("expected transcription")
                };
                let stream = clip.take_stream();
                assert_eq!(stream.is_some(), streamed);
                let (fed, begun, cancelled) = stream_traffic(&commands);
                drop(stream);
                assert_eq!((begun, cancelled), (usize::from(streamed), 0));
                // The stream saw exactly the clip's audio, in order, plus at
                // most the audio captured after the release's tail.
                let shared = fed.len().min(clip.samples.len());
                assert_eq!(fed[..shared], clip.samples[..shared], "{sample_rate} Hz");
                assert!(fed.len() <= clip.samples.len() + 16_000 / 4);
                assert_eq!(fed.len() > clip.samples.len() / 2, streamed);
                assert_eq!(clip.samples.len(), expected.len(), "{sample_rate} Hz");
                // Only the final 10 ms may differ: the clip's resampler saw the
                // real audio after the tail where the reference saw silence.
                let settled = expected.len() - 160;
                assert!(
                    clip.samples[..settled]
                        .iter()
                        .zip(&expected[..settled])
                        .all(|(clip, expected)| (clip - expected).abs() < 1e-4),
                    "{sample_rate} Hz streamed={streamed} lost audio before the tail ended"
                );
                clips.push(clip.samples);
            }
            assert!(
                clips[0] == clips[1],
                "{sample_rate} Hz: streaming changed the clip"
            );
        }
    }

    #[test]
    fn a_stream_starts_with_the_backlog_and_feeds_in_blocks() {
        let (engine, commands) = TranscriptionEngine::for_test();
        let mut capture = DictationCapture::new(16_000);
        capture.keep_warm(&vec![0.25; 16_000]);
        let start = capture_time();
        capture.start(start);
        assert!(!capture.is_streaming());
        // Toggle and Linux open the stream before the hold threshold.
        capture.begin_stream(&engine);
        capture.begin_stream(&engine);
        assert!(capture.is_streaming());
        let (fed, begun, _) = stream_traffic(&commands);
        assert_eq!(begun, 1, "a recording opens one stream");
        assert_eq!(fed.len(), 7_200, "the 450 ms pre-roll is fed at once");
        capture.ingest(&[0.5; 1_600], start + Duration::from_millis(100));
        assert!(
            stream_traffic(&commands).0.is_empty(),
            "100 ms waits for a block"
        );
        capture.ingest(&[0.5; 1_600], start + Duration::from_millis(200));
        assert_eq!(stream_traffic(&commands).0.len(), 3_200);
    }

    #[test]
    fn short_taps_cancellation_and_unstreamable_models_end_the_stream() {
        let (engine, commands) = TranscriptionEngine::for_test();
        let start = capture_time();
        let mut short = DictationCapture::new(16_000);
        short.start(start);
        short.begin_stream(&engine);
        short.ingest(&[0.5; 1_600], start + Duration::from_millis(100));
        assert!(matches!(
            short.finish(start + Duration::from_millis(100)),
            Finish::Discard
        ));
        assert_eq!(stream_traffic(&commands).2, 1, "a short tap cancels");

        let mut cancelled = DictationCapture::new(16_000);
        cancelled.start(start);
        assert!(cancelled.become_intentional(start + MINIMUM_HOLD_DURATION));
        cancelled.begin_stream(&engine);
        cancelled.ingest(&[0.5; 16_000], start + Duration::from_secs(1));
        cancelled.cancel();
        assert!(!cancelled.is_streaming());
        let (_, _, cancels) = stream_traffic(&commands);
        assert_eq!(cancels, 1, "Escape cancels");

        let (offline, commands) = TranscriptionEngine::for_test_without_streaming();
        let mut capture = DictationCapture::new(16_000);
        capture.start(start);
        capture.begin_stream(&offline);
        assert!(!capture.is_streaming());
        capture.ingest(&[0.5; 16_000], start + Duration::from_secs(1));
        let Finish::Transcribe(mut clip) = capture.finish(start + Duration::from_secs(1)) else {
            panic!("expected transcription")
        };
        assert!(clip.take_stream().is_none());
        assert!(commands.try_iter().next().is_none());
    }

    #[test]
    fn finds_resilient_controls_at_the_end_of_an_utterance() {
        let protocol = DictationProtocol::default();
        assert!(matches!(
            protocol.control_suffix("How are we doing? Dictates stop."),
            Some((DictationControl::Stop, _))
        ));
        assert!(matches!(
            protocol.control_suffix("That's okay. Dictates send."),
            Some((DictationControl::Send, _))
        ));
        assert!(matches!(
            protocol.control_suffix("Dictates end."),
            Some((DictationControl::Send, 0))
        ));
        assert!(
            protocol
                .control_suffix("Please dictate this sentence")
                .is_none()
        );
    }

    #[test]
    fn finds_dictation_start_at_the_beginning_of_an_utterance() {
        let protocol = DictationProtocol::default();
        assert_eq!(protocol.start_prefix("Dictate start."), Some(13));
        assert!(
            protocol
                .start_prefix("Dictate start as you can see")
                .is_some()
        );
        assert!(protocol.start_prefix("Dictates start, testing").is_some());
        assert!(protocol.start_prefix("Okay, dictate start").is_none());
    }

    #[test]
    fn strips_voice_protocols_from_transcripts() {
        assert_eq!(
            DictationProtocol::default()
                .strip("Dictate start, this should keep the message. Dictates stop."),
            "This should keep the message."
        );
    }

    #[test]
    fn configured_protocol_is_exact_and_rejects_streaming_ambiguity() {
        let protocol = DictationProtocol::try_new(
            vec!["begin note".into()],
            vec!["finish note".into()],
            vec!["send note".into()],
            vec!["discard note".into()],
        )
        .unwrap();
        assert!(protocol.start_prefix("Begin note, hello").is_some());
        assert!(protocol.start_prefix("Dictate start, hello").is_none());
        assert!(matches!(
            protocol.control_suffix("hello, finish note"),
            Some((DictationControl::Stop, _))
        ));
        assert!(protocol.control_suffix("hello, dictates stop").is_none());
        assert_eq!(
            protocol.strip("Begin note, what are you talking about? Finish note."),
            "What are you talking about?"
        );
        assert_eq!(
            protocol.strip("what are you talking about"),
            "what are you talking about"
        );

        assert!(
            DictationProtocol::try_new(
                vec!["begin".into(), "begin note".into()],
                vec!["finish".into()],
                vec!["send".into()],
                vec!["cancel".into()],
            )
            .is_err()
        );
        assert!(
            DictationProtocol::try_new(
                vec!["begin".into()],
                vec!["note".into()],
                vec!["finish note".into()],
                vec!["cancel".into()],
            )
            .is_err()
        );
    }

    #[test]
    fn configured_protocol_tolerates_transcription_punctuation() {
        let protocol = DictationProtocol::try_new(
            vec!["say".into()],
            vec!["say paste".into(), "say stop".into()],
            vec!["say send".into()],
            vec!["say cancel".into(), "never mind".into()],
        )
        .unwrap();

        assert!(matches!(
            protocol.control_suffix("What are you talking about? Say, stop."),
            Some((DictationControl::Stop, _))
        ));
        assert!(matches!(
            protocol.control_suffix("Meet me at five; say... send!"),
            Some((DictationControl::Send, _))
        ));
        assert_eq!(
            protocol.strip("Say, what are you talking about? Say, stop."),
            "What are you talking about?"
        );
        assert_eq!(
            protocol.strip("Say, meet me at five, say stop."),
            "Meet me at five"
        );
        assert!(
            protocol
                .control_suffix("Say paste and then keep talking")
                .is_none()
        );
        assert!(protocol.control_suffix("Essay stop").is_none());
        assert_eq!(
            protocol.control_suffix_word_count(&[
                "Meet".into(),
                "me".into(),
                "Say,".into(),
                "stop.".into(),
            ]),
            Some(2)
        );
    }

    #[test]
    fn control_requires_stability_or_completion() {
        let mut stability = ControlStability::default();
        assert_eq!(stability.observe(Some(DictationControl::Send), false), None);
        assert_eq!(stability.observe(None, false), None);
        assert_eq!(stability.observe(Some(DictationControl::Send), false), None);
        assert_eq!(
            stability.observe(Some(DictationControl::Send), false),
            Some(DictationControl::Send)
        );
        stability.reset();
        assert_eq!(
            stability.observe(Some(DictationControl::Stop), true),
            Some(DictationControl::Stop)
        );
        assert_eq!(
            stability.observe(Some(DictationControl::Cancel), false),
            Some(DictationControl::Cancel)
        );
    }
}
