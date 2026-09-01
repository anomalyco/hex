use std::collections::{HashMap, VecDeque};
use std::time::Duration;

use rubato::{FftFixedIn, Resampler};

use crate::audio::CaptureInstant;

#[cfg(target_os = "macos")]
use crate::recording_environment::{RecordingEnvironmentController, RecordingEnvironmentSession};

const VOICE_PRE_ROLL_DURATION: Duration = Duration::from_secs(1);
const TIMELINE_BUFFER_DURATION: Duration = Duration::from_secs(10);
const PRE_ROLL_DURATION: Duration = Duration::from_millis(450);
pub const MINIMUM_HOLD_DURATION: Duration = Duration::from_millis(300);
const MINIMUM_PARAKEET_DURATION: Duration = Duration::from_millis(1500);
const INITIAL_RECORDING_CAPACITY: Duration = Duration::from_secs(10);
const PARAKEET_SAMPLE_RATE: u32 = 16_000;

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
        for (phrases, control) in [
            (&self.cancel, DictationControl::Cancel),
            (&self.stop, DictationControl::Stop),
            (&self.send, DictationControl::Send),
        ] {
            if let Some(prefix_end) = control_suffix(text, phrases) {
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

    pub fn reset(&mut self) {
        self.pending = None;
    }
}

fn control_suffix(text: &str, suffixes: &[String]) -> Option<usize> {
    let words = spoken_words(text);
    for suffix in suffixes {
        let suffix = spoken_words(suffix);
        if words.len() >= suffix.len()
            && words[words.len() - suffix.len()..]
                .iter()
                .map(|word| &word.normalized)
                .eq(suffix.iter().map(|word| &word.normalized))
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
        (words.len() >= prefix.len()
            && words[..prefix.len()]
                .iter()
                .map(|word| &word.normalized)
                .eq(prefix.iter().map(|word| &word.normalized)))
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

pub struct DictationClip {
    samples: Vec<f32>,
}

impl DictationClip {
    pub fn duration_ms(&self) -> u64 {
        self.samples.len() as u64 * 1_000 / u64::from(PARAKEET_SAMPLE_RATE)
    }

    #[cfg(any(test, target_os = "linux"))]
    pub fn into_parakeet_samples(self) -> Vec<f32> {
        let mut samples = self.samples;
        pad_for_parakeet(&mut samples);
        samples
    }

    pub fn into_transcription_samples(self) -> Vec<f32> {
        self.samples
    }
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
    resampler: Option<FftFixedIn<f32>>,
    #[cfg(target_os = "macos")]
    environment: RecordingEnvironmentState,
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
                FftFixedIn::new(
                    source_rate as usize,
                    PARAKEET_SAMPLE_RATE as usize,
                    Self::CHUNK_SIZE,
                    1,
                    1,
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
            #[cfg(target_os = "macos")]
            environment: RecordingEnvironmentState::Disabled,
        })
    }

    #[cfg(target_os = "macos")]
    fn with_environment(mut self, environment: RecordingEnvironmentState) -> Self {
        self.environment = environment;
        self
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
                self.samples.extend(
                    resampler
                        .process(&[&self.input_buffer], None)
                        .expect("fixed-rate resampling must succeed")
                        .remove(0),
                );
                self.input_buffer.clear();
            }
        }
    }

    fn push_through(&mut self, samples: &[f32], captured_through: CaptureInstant) {
        self.push(samples);
        self.recorded_through = Some(captured_through);
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
                self.samples.extend(
                    resampler
                        .process_partial(Some(&[&self.input_buffer]), None)
                        .expect("fixed-rate resampling must succeed")
                        .remove(0),
                );
            }
            let output_delay = resampler.output_delay();
            for _ in 0..8 {
                if self.samples.len() >= expected_samples + output_delay {
                    break;
                }
                let flushed = resampler
                    .process_partial::<&[f32]>(None, None)
                    .expect("fixed-rate resampler flush must succeed")
                    .remove(0);
                self.samples.extend(flushed);
            }
            self.samples.drain(..output_delay.min(self.samples.len()));
            self.samples.truncate(expected_samples);
        } else {
            self.samples.truncate(self.source_samples);
        }
        self.samples
    }
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

    #[cfg(any(test, target_os = "linux"))]
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
        #[cfg(target_os = "macos")]
        let environment = match &self.recording_environment {
            Some(controller) if intentional => RecordingEnvironmentState::Active {
                _session: controller.begin(),
            },
            Some(controller) => RecordingEnvironmentState::Pending(controller.clone()),
            None => RecordingEnvironmentState::Disabled,
        };
        #[cfg(not(target_os = "macos"))]
        let _ = intentional;
        let pre_roll = samples_for(duration, self.sample_rate).min(self.ring.len());
        let recording = Recording::new(now, self.sample_rate);
        #[cfg(target_os = "macos")]
        let mut recording = recording.with_environment(environment);
        #[cfg(not(target_os = "macos"))]
        let mut recording = recording;
        recording.intentional = intentional;
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
        }
        let retention = oldest_pending
            .map(|pending| {
                captured_through
                    .saturating_duration_since(pending)
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

    pub fn cancel(&mut self) {
        self.recording = None;
    }

    pub fn finish(&mut self, now: CaptureInstant) -> Finish {
        let Some(recording) = self.recording.take() else {
            return Finish::Discard;
        };
        if !recording.intentional
            && now.duration_since(recording.started_at) < MINIMUM_HOLD_DURATION
        {
            return Finish::Discard;
        }

        Finish::Transcribe(DictationClip {
            samples: recording.finish(Some(now)),
        })
    }
}

fn samples_for(duration: Duration, sample_rate: u32) -> usize {
    (duration.as_secs_f64() * f64::from(sample_rate)).round() as usize
}

#[cfg(test)]
mod tests {
    use super::*;

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
