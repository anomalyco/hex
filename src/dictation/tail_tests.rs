//! Opt-in end-of-capture harness: plays recorded speech through the real
//! capture timeline, releases the shortcut at precise offsets around the last
//! word, and compares clip boundaries and Unified English transcripts.

use std::path::{Path, PathBuf};
use std::time::Duration;

use hound::WavReader;

use super::*;
use crate::audio::CaptureInstant;
use crate::dictation::{DictationCapture, Finish, POST_RELEASE_TAIL, resample_for_parakeet};

const LEAD: Duration = Duration::from_secs(1);
const PRE_ROLL: Duration = Duration::from_millis(450);
const RELEASE_OFFSETS_MS: [i64; 6] = [-150, -100, -50, 0, 50, 150];

fn samples_for(duration: Duration, rate: u32) -> usize {
    (duration.as_nanos() * u128::from(rate) / 1_000_000_000) as usize
}

fn read_wav(path: &Path) -> (Vec<f32>, u32) {
    let mut reader = WavReader::open(path).unwrap();
    let spec = reader.spec();
    let samples = match spec.sample_format {
        hound::SampleFormat::Float => reader.samples::<f32>().map(Result::unwrap).collect(),
        hound::SampleFormat::Int => reader
            .samples::<i32>()
            .map(|sample| sample.unwrap() as f32 / (1u32 << (spec.bits_per_sample - 1)) as f32)
            .collect(),
    };
    (samples, spec.sample_rate)
}

/// End of the last 10 ms window louder than -45 dBFS, in source samples.
fn speech_end(samples: &[f32], rate: u32) -> usize {
    let window = rate as usize / 100;
    samples
        .chunks(window)
        .enumerate()
        .filter(|(_, chunk)| {
            let energy =
                chunk.iter().map(|sample| sample * sample).sum::<f32>() / chunk.len() as f32;
            10.0 * energy.max(1e-12).log10() > -45.0
        })
        .map(|(index, chunk)| index * window + chunk.len())
        .next_back()
        .unwrap_or(samples.len())
}

/// Feeds 10 ms chunks of `LEAD` silence then speech, presses after the
/// silence, and releases `release` source samples into the speech while later
/// audio is already arriving, exactly like a delivered CGEvent release. The
/// clip keeps the production post-release tail.
fn capture(audio: &[f32], rate: u32, release: usize) -> DictationClip {
    let origin = CaptureInstant::from_nanos(60_000_000_000);
    let at = |samples: usize| {
        origin + Duration::from_nanos(samples as u64 * 1_000_000_000 / u64::from(rate))
    };
    let lead = samples_for(LEAD, rate);
    let pressed_at = at(lead);
    let released_at = at(lead + release);
    let mut capture = DictationCapture::new(rate);
    let mut fed = 0;
    let mut finish = None;
    for block in audio.chunks(rate as usize / 100) {
        fed += block.len();
        let through = at(fed);
        capture.ingest(block, through);
        if through == pressed_at {
            capture.start_at(pressed_at);
        }
        if through > pressed_at && finish.is_none() {
            capture.become_intentional(through.min(released_at));
            // The owner finishes once the tail has arrived, up to one buffer late.
            if through >= released_at + POST_RELEASE_TAIL + Duration::from_millis(20) {
                finish = Some(capture.finish_with_tail(released_at, POST_RELEASE_TAIL));
            }
        }
    }
    let Some(Finish::Transcribe(clip)) = finish else {
        panic!("expected transcription");
    };
    clip
}

fn run(model: &mut Parakeet, mut samples: Vec<f32>) -> String {
    // Mirror Transcriber's Unified English preparation.
    pad_for_parakeet(&mut samples);
    samples.resize(
        samples.len() + crate::transcription::UNIFIED_ENGLISH_TRAILING_SILENCE_SAMPLES,
        0.0,
    );
    model.transcribe(&samples).unwrap()
}

#[test]
#[ignore = "requires HEX_TAIL_MODEL (Unified English GGUF) and HEX_TAIL_WAVS (comma-separated)"]
fn release_boundary_harness() {
    let mut model = Parakeet::load_from(
        &PathBuf::from(std::env::var("HEX_TAIL_MODEL").unwrap()),
        false,
        Some(&TranscriptionSelection::default()),
    )
    .unwrap();
    for path in std::env::var("HEX_TAIL_WAVS").unwrap().split(',') {
        let (speech, rate) = read_wav(Path::new(path));
        let end = speech_end(&speech, rate);
        let lead = samples_for(LEAD, rate);
        let mut audio = vec![0.0; lead];
        audio.extend_from_slice(&speech);
        audio.resize(audio.len() + rate as usize, 0.0);
        let reference = resample_for_parakeet(&audio, rate);
        let reference_start = (lead - samples_for(PRE_ROLL, rate)) * 16_000 / rate as usize;
        println!(
            "== {path} rate={rate} speech_end_ms={}",
            end as u64 * 1_000 / u64::from(rate)
        );
        for offset_ms in RELEASE_OFFSETS_MS {
            let release = (end as i64 + offset_ms * i64::from(rate) / 1_000) as usize;
            let expected_len =
                (samples_for(PRE_ROLL, rate) + release + samples_for(POST_RELEASE_TAIL, rate))
                    * 16_000
                    / rate as usize;
            let samples = capture(&audio, rate, release).into_transcription_samples();
            let missing = expected_len as i64 - samples.len() as i64;
            // Largest deviation from a whole-file conversion over the final
            // 100 ms, excluding the last 2 ms that flush against silence.
            let max_tail_diff = (samples.len().saturating_sub(1_600)
                ..samples.len().saturating_sub(32))
                .map(|index| (samples[index] - reference[reference_start + index]).abs())
                .fold(0.0, f32::max);
            for threads in [0, 1] {
                model.session.set_threads_for_test(threads).unwrap();
                let started = std::time::Instant::now();
                let text = run(&mut model, samples.clone());
                let release_ms = started.elapsed().as_millis();
                println!(
                    "release{offset_ms:+}ms threads={threads} release_ms={release_ms} missing={missing} tail_diff={max_tail_diff:.5} {text:?}"
                );
            }
        }
    }
}
