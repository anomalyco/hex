use std::collections::VecDeque;
use std::time::{Duration, Instant};

use rubato::{FftFixedIn, Resampler};

const RING_BUFFER_DURATION: Duration = Duration::from_secs(1);
const PRE_ROLL_DURATION: Duration = Duration::from_millis(450);
const MINIMUM_HOLD_DURATION: Duration = Duration::from_millis(300);
const MINIMUM_PARAKEET_DURATION: Duration = Duration::from_millis(1500);
const MAXIMUM_DICTATION_DURATION: Duration = Duration::from_secs(60);
const PARAKEET_SAMPLE_RATE: u32 = 16_000;

pub enum Finish {
    Discard,
    Transcribe(DictationClip),
}

pub struct DictationClip {
    samples: Vec<f32>,
}

impl DictationClip {
    pub fn into_parakeet_samples(self) -> Vec<f32> {
        let mut samples = self.samples;
        samples.resize(
            samples
                .len()
                .max(samples_for(MINIMUM_PARAKEET_DURATION, PARAKEET_SAMPLE_RATE)),
            0.0,
        );
        samples
    }
}

pub struct DictationCapture {
    sample_rate: u32,
    ring: VecDeque<f32>,
    recording: Option<Recording>,
}

struct Recording {
    started_at: Instant,
    samples: Vec<f32>,
    source_samples: usize,
    source_rate: u32,
    input_buffer: Vec<f32>,
    resampler: Option<FftFixedIn<f32>>,
}

impl Recording {
    const CHUNK_SIZE: usize = 1024;

    fn new(started_at: Instant, source_rate: u32) -> Self {
        let resampler = (source_rate != PARAKEET_SAMPLE_RATE).then(|| {
            FftFixedIn::new(
                source_rate as usize,
                PARAKEET_SAMPLE_RATE as usize,
                Self::CHUNK_SIZE,
                1,
                1,
            )
            .expect("microphone sample rate must be resampleable")
        });
        Self {
            started_at,
            samples: Vec::with_capacity(samples_for(
                MAXIMUM_DICTATION_DURATION,
                PARAKEET_SAMPLE_RATE,
            )),
            source_samples: 0,
            source_rate,
            input_buffer: Vec::with_capacity(Self::CHUNK_SIZE),
            resampler,
        }
    }

    fn push(&mut self, mut samples: &[f32]) {
        let capacity = samples_for(MAXIMUM_DICTATION_DURATION, self.source_rate);
        samples = &samples[..samples
            .len()
            .min(capacity.saturating_sub(self.source_samples))];
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
        }
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
        let pre_roll = samples_for(PRE_ROLL_DURATION, self.sample_rate).min(self.ring.len());
        let mut recording = Recording::new(now, self.sample_rate);
        let samples: Vec<_> = self
            .ring
            .iter()
            .skip(self.ring.len() - pre_roll)
            .copied()
            .collect();
        recording.push(&samples);
        self.recording = Some(recording);
    }

    pub fn start_without_pre_roll(&mut self, now: Instant) {
        self.recording = Some(Recording::new(now, self.sample_rate));
    }

    pub fn push(&mut self, samples: &[f32]) {
        if let Some(recording) = &mut self.recording {
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
        let samples = clip.into_parakeet_samples();
        assert_eq!(samples.len(), 24_000);
        assert_eq!(samples[0], 0.25);
        assert!(samples.contains(&0.5));
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
}
