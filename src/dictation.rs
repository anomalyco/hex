use std::collections::VecDeque;
use std::time::{Duration, Instant};

use rubato::{FftFixedIn, Resampler};

#[cfg(target_os = "macos")]
use crate::recording_environment::{RecordingEnvironmentController, RecordingEnvironmentSession};

const RING_BUFFER_DURATION: Duration = Duration::from_secs(1);
const PRE_ROLL_DURATION: Duration = Duration::from_millis(450);
pub const MINIMUM_HOLD_DURATION: Duration = Duration::from_millis(300);
const MINIMUM_PARAKEET_DURATION: Duration = Duration::from_millis(1500);
const INITIAL_RECORDING_CAPACITY: Duration = Duration::from_secs(10);
const PARAKEET_SAMPLE_RATE: u32 = 16_000;

pub const DICTATION_START_PHRASES: &[&str] = &["dictate start", "start dictating"];

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

pub fn dictation_control_suffix(text: &str) -> Option<(DictationControl, usize)> {
    let text = text
        .trim_end()
        .trim_end_matches(|character: char| character.is_ascii_punctuation())
        .trim_end();
    let lowercase = text.to_ascii_lowercase();
    for (suffix, control) in [
        ("dictate cancel", DictationControl::Cancel),
        ("dictates cancel", DictationControl::Cancel),
        ("dictate stop", DictationControl::Stop),
        ("dictates stop", DictationControl::Stop),
        ("dictate send", DictationControl::Send),
        ("dictates send", DictationControl::Send),
        ("dictate end", DictationControl::Send),
        ("dictates end", DictationControl::Send),
        ("dictate and", DictationControl::Send),
        ("dictates and", DictationControl::Send),
    ] {
        if let Some(prefix) = lowercase.strip_suffix(suffix)
            && (prefix.is_empty()
                || prefix.ends_with(char::is_whitespace)
                || prefix.ends_with(|character: char| character.is_ascii_punctuation()))
        {
            return Some((control, prefix.len()));
        }
    }
    None
}

pub fn dictation_start_prefix(text: &str) -> Option<usize> {
    control_prefix(
        text,
        &[
            "dictate start",
            "dictates start",
            "dictate starts",
            "dictates starts",
            "start dictating",
        ],
    )
}

pub fn strip_dictation_protocol(text: &str) -> String {
    let mut text = text.trim();
    while let Some((_, prefix_end)) = dictation_control_suffix(text) {
        text = trim_control_end(&text[..prefix_end]);
    }
    if let Some(prefix_end) = dictation_start_prefix(text) {
        text = trim_control_start(&text[prefix_end..]);
    }
    text.to_string()
}

fn control_prefix(text: &str, prefixes: &[&str]) -> Option<usize> {
    let text = text.trim_start();
    let lowercase = text.to_ascii_lowercase();
    prefixes.iter().find_map(|prefix| {
        lowercase.strip_prefix(prefix).and_then(|remainder| {
            (remainder.is_empty()
                || remainder.starts_with(char::is_whitespace)
                || remainder.starts_with(|character: char| character.is_ascii_punctuation()))
            .then_some(text.len() - remainder.len())
        })
    })
}

fn trim_control_start(text: &str) -> &str {
    text.trim_start_matches(|character: char| {
        character.is_whitespace() || character.is_ascii_punctuation()
    })
}

fn trim_control_end(text: &str) -> &str {
    text.trim_end_matches(|character: char| {
        character.is_whitespace() || character.is_ascii_punctuation()
    })
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
    let mut recording = Recording::try_new(Instant::now(), source_rate)?;
    recording.push(samples);
    Ok(recording.finish())
}

pub struct DictationCapture {
    sample_rate: u32,
    ring: VecDeque<f32>,
    recording: Option<Recording>,
    #[cfg(target_os = "macos")]
    recording_environment: Option<RecordingEnvironmentController>,
}

struct Recording {
    started_at: Instant,
    samples: Vec<f32>,
    source_samples: usize,
    source_rate: u32,
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

    fn new(started_at: Instant, source_rate: u32) -> Self {
        Self::try_new(started_at, source_rate).expect("microphone sample rate must be resampleable")
    }

    fn try_new(started_at: Instant, source_rate: u32) -> Result<Self, String> {
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
            samples: Vec::with_capacity(samples_for(
                INITIAL_RECORDING_CAPACITY,
                PARAKEET_SAMPLE_RATE,
            )),
            source_samples: 0,
            source_rate,
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

    fn finish(mut self) -> Vec<f32> {
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
        }
        self.samples
    }
}

impl DictationCapture {
    pub fn new(sample_rate: u32) -> Self {
        Self {
            sample_rate,
            ring: VecDeque::with_capacity(samples_for(RING_BUFFER_DURATION, sample_rate)),
            recording: None,
            #[cfg(target_os = "macos")]
            recording_environment: None,
        }
    }

    #[cfg(target_os = "macos")]
    pub fn enable_recording_environment(&mut self, controller: RecordingEnvironmentController) {
        self.recording_environment = Some(controller);
    }

    pub fn keep_warm(&mut self, samples: &[f32]) {
        let capacity = samples_for(RING_BUFFER_DURATION, self.sample_rate);
        if samples.len() >= capacity {
            self.ring.clear();
            self.ring.extend(&samples[samples.len() - capacity..]);
            return;
        }
        self.ring.drain(
            ..samples.len().min(
                self.ring
                    .len()
                    .saturating_add(samples.len())
                    .saturating_sub(capacity),
            ),
        );
        self.ring.extend(samples);
    }

    pub fn start(&mut self, now: Instant) {
        self.start_with_pre_roll(now, PRE_ROLL_DURATION, false);
    }

    pub fn start_voice(&mut self, now: Instant) {
        self.start_with_pre_roll(now, Duration::ZERO, true);
    }

    fn start_with_pre_roll(&mut self, now: Instant, duration: Duration, intentional: bool) {
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

    pub fn push(&mut self, samples: &[f32]) {
        if let Some(recording) = &mut self.recording {
            #[cfg(target_os = "macos")]
            if recording.started_at.elapsed() >= MINIMUM_HOLD_DURATION {
                recording.environment.activate();
            }
            recording.push(samples);
        }
    }

    pub fn cancel(&mut self) {
        self.recording = None;
    }

    pub fn finish(&mut self, now: Instant) -> Finish {
        let Some(recording) = self.recording.take() else {
            return Finish::Discard;
        };
        if now.duration_since(recording.started_at) < MINIMUM_HOLD_DURATION {
            return Finish::Discard;
        }

        Finish::Transcribe(DictationClip {
            samples: recording.finish(),
        })
    }
}

fn samples_for(duration: Duration, sample_rate: u32) -> usize {
    (duration.as_secs_f64() * f64::from(sample_rate)).round() as usize
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn quick_option_tap_discards() {
        let mut capture = DictationCapture::new(16_000);
        let start = Instant::now();
        capture.start(start);
        capture.push(&vec![0.5; 1_600]);
        assert!(matches!(
            capture.finish(start + Duration::from_millis(100)),
            Finish::Discard
        ));
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
        let start = Instant::now();
        capture.start(start);
        capture.push(&vec![0.5; 8_000]);
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
    fn voice_start_excludes_audio_already_seen_by_recognizer() {
        let mut capture = DictationCapture::new(16_000);
        capture.keep_warm(&[0.25; 8_000]);
        capture.keep_warm(&[0.25; 8_000]);
        let start = Instant::now();

        capture.start_voice(start);
        capture.push(&[0.5; 8_000]);

        let Finish::Transcribe(clip) = capture.finish(start + Duration::from_millis(500)) else {
            panic!("expected transcription")
        };
        let samples = clip.into_parakeet_samples();
        assert_eq!(samples[0], 0.5);
        assert!(!samples.contains(&0.25));
    }

    #[test]
    fn capture_continues_past_sixty_seconds_until_explicitly_finished() {
        let mut capture = DictationCapture::new(16_000);
        let start = Instant::now();
        capture.start(start);
        capture.push(&vec![0.5; 61 * 16_000]);

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
            let mut recording = Recording::new(Instant::now(), sample_rate);
            recording.push(&vec![0.5; input_len]);
            let samples = recording.finish();
            let expected = input_len * PARAKEET_SAMPLE_RATE as usize / sample_rate as usize;

            assert_eq!(samples.len(), expected);
            assert!(samples.last().is_some_and(|sample| *sample > 0.25));
        }
    }

    #[test]
    fn finds_resilient_controls_at_the_end_of_an_utterance() {
        assert!(matches!(
            dictation_control_suffix("How are we doing? Dictates stop."),
            Some((DictationControl::Stop, _))
        ));
        assert!(matches!(
            dictation_control_suffix("That's okay. Dictates send."),
            Some((DictationControl::Send, _))
        ));
        assert!(matches!(
            dictation_control_suffix("Dictates end."),
            Some((DictationControl::Send, 0))
        ));
        assert!(dictation_control_suffix("Please dictate this sentence").is_none());
    }

    #[test]
    fn finds_dictation_start_at_the_beginning_of_an_utterance() {
        assert_eq!(dictation_start_prefix("Dictate start."), Some(13));
        assert!(dictation_start_prefix("Dictate start as you can see").is_some());
        assert!(dictation_start_prefix("Dictates start, testing").is_some());
        assert!(dictation_start_prefix("Okay, dictate start").is_none());
    }

    #[test]
    fn strips_voice_protocols_from_transcripts() {
        assert_eq!(
            strip_dictation_protocol("Dictate start, this should keep the message. Dictates stop."),
            "this should keep the message"
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
