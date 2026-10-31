//! End-to-end dictation corpus evaluation: sweeps shortcut release offsets
//! (-300 ms to +200 ms), onset offsets, and short/long holds across real and
//! synthetic WAV corpora.

use std::fs;
use std::path::{Path, PathBuf};
use std::time::Duration;

use hound::{SampleFormat, WavReader};
use serde::{Deserialize, Serialize};

use super::*;
use crate::audio::CaptureInstant;
use crate::dictation::{
    DictationCapture, Finish, MINIMUM_HOLD_DURATION, POST_RELEASE_TAIL, resample_for_parakeet,
};

const LEAD_SILENCE: Duration = Duration::from_secs(1);
const TAIL_SILENCE: Duration = Duration::from_secs(1);
const RELEASE_SWEEP_MS: [i64; 10] = [-300, -250, -200, -150, -100, -50, 0, 50, 100, 200];

#[derive(Debug, Deserialize)]
struct RealManifestEntry {
    id: String,
    duration_ms: u64,
    text: String,
}

#[derive(Debug, Deserialize)]
struct SyntheticManifestEntry {
    id: String,
    source: String,
    category: String,
    text: String,
    wav: String,
}

#[derive(Debug, Serialize)]
struct OffsetOutcome {
    offset_ms: i64,
    transcript: String,
    kept_last_word: bool,
}

#[derive(Debug, Serialize)]
struct OnsetOutcome {
    speech_start_vs_press_ms: i64,
    warm_mic: bool,
    transcript: String,
    kept_first_word: bool,
}

#[derive(Debug, Serialize)]
struct ClipReport {
    id: String,
    source: String,
    category: String,
    sample_rate: u32,
    duration_ms: u64,
    speech_end_ms: u64,
    reference_text: String,
    offline_text: String,
    target_first_word: String,
    target_last_word: String,
    offline_has_last_word: bool,
    offsets: Vec<OffsetOutcome>,
    onsets: Vec<OnsetOutcome>,
}

fn samples_for_duration(duration: Duration, rate: u32) -> usize {
    (duration.as_nanos() * u128::from(rate) / 1_000_000_000) as usize
}

fn read_mono_wav(path: &Path) -> (Vec<f32>, u32) {
    let mut reader = WavReader::open(path)
        .unwrap_or_else(|error| panic!("failed to open {}: {error}", path.display()));
    let spec = reader.spec();
    let channels = spec.channels.max(1) as usize;
    let raw: Vec<f32> = match spec.sample_format {
        SampleFormat::Float => reader.samples::<f32>().map(Result::unwrap).collect(),
        SampleFormat::Int => {
            let scale = (1u64 << (spec.bits_per_sample.saturating_sub(1))) as f32;
            reader
                .samples::<i32>()
                .map(|sample| sample.unwrap() as f32 / scale)
                .collect()
        }
    };
    let mono = if channels == 1 {
        raw
    } else {
        raw.chunks(channels)
            .map(|frame| frame.iter().sum::<f32>() / frame.len() as f32)
            .collect()
    };
    (mono, spec.sample_rate)
}

/// Acoustic speech end in source samples: end of the last 10 ms window that
/// rises at least 10 dB above the clip's 15th-percentile noise floor and stays
/// within 22 dB of the 95th-percentile speech peak (and >= -45 dBFS).
fn detect_speech_end(samples: &[f32], rate: u32) -> usize {
    let window = (rate as usize / 100).max(1);
    let db_frames: Vec<(usize, f32)> = samples
        .chunks(window)
        .enumerate()
        .map(|(index, chunk)| {
            let energy = chunk.iter().map(|s| s * s).sum::<f32>() / chunk.len() as f32;
            let db = 10.0 * energy.max(1e-12).log10();
            (index * window + chunk.len(), db)
        })
        .collect();
    if db_frames.is_empty() {
        return samples.len();
    }
    let mut sorted_dbs: Vec<f32> = db_frames.iter().map(|&(_, db)| db).collect();
    sorted_dbs.sort_by(|a, b| a.partial_cmp(b).unwrap());
    let floor_db = sorted_dbs[(sorted_dbs.len() * 15 / 100).min(sorted_dbs.len() - 1)];
    let peak_db = sorted_dbs[(sorted_dbs.len() * 95 / 100).min(sorted_dbs.len() - 1)];
    let threshold_db = (floor_db + 10.0).max(peak_db - 22.0).max(-45.0);
    db_frames
        .into_iter()
        .filter(|&(_, db)| db >= threshold_db)
        .map(|(end_sample, _)| end_sample)
        .next_back()
        .unwrap_or(samples.len())
}

fn detect_speech_start(samples: &[f32], rate: u32) -> usize {
    let window = (rate as usize / 100).max(1);
    let db_frames: Vec<(usize, f32)> = samples
        .chunks(window)
        .enumerate()
        .map(|(index, chunk)| {
            let energy = chunk.iter().map(|s| s * s).sum::<f32>() / chunk.len() as f32;
            let db = 10.0 * energy.max(1e-12).log10();
            (index * window, db)
        })
        .collect();
    if db_frames.is_empty() {
        return 0;
    }
    let mut sorted_dbs: Vec<f32> = db_frames.iter().map(|&(_, db)| db).collect();
    sorted_dbs.sort_by(|a, b| a.partial_cmp(b).unwrap());
    let floor_db = sorted_dbs[(sorted_dbs.len() * 15 / 100).min(sorted_dbs.len() - 1)];
    let peak_db = sorted_dbs[(sorted_dbs.len() * 95 / 100).min(sorted_dbs.len() - 1)];
    let threshold_db = (floor_db + 10.0).max(peak_db - 22.0).max(-45.0);
    db_frames
        .into_iter()
        .find(|&(_, db)| db >= threshold_db)
        .map(|(start_sample, _)| start_sample)
        .unwrap_or(0)
}

fn normalized_words(text: &str) -> Vec<String> {
    text.split_whitespace()
        .map(|word| {
            word.chars()
                .filter(|c| c.is_alphanumeric() || *c == '\'')
                .collect::<String>()
                .to_lowercase()
        })
        .filter(|word| !word.is_empty())
        .collect()
}

fn last_word_of(text: &str) -> Option<String> {
    normalized_words(text).into_iter().next_back()
}

/// Feeds 10 ms chunks through `DictationCapture`, starting at `lead` samples
/// and releasing at `lead + release_samples`. Like production, the clip keeps
/// the post-release tail and finishes once that tail plus one 20 ms buffer
/// (`CAPTURE_DRAIN_LEAD`) has arrived.
fn simulate_capture(
    padded_audio: &[f32],
    rate: u32,
    lead_samples: usize,
    release_samples: usize,
    warm_mic: bool,
) -> DictationClip {
    let origin = CaptureInstant::from_nanos(60_000_000_000);
    let at = |samples: usize| {
        origin + Duration::from_nanos(samples as u64 * 1_000_000_000 / u64::from(rate))
    };
    let pressed_at = at(lead_samples);
    let released_at = at(lead_samples + release_samples);
    let mut capture = DictationCapture::new(rate);
    let mut fed = 0usize;
    let mut finish = None;
    let chunk_len = (rate as usize / 100).max(1);

    for block in padded_audio.chunks(chunk_len) {
        let next_fed = fed + block.len();
        let through = at(next_fed);
        if warm_mic || next_fed > lead_samples {
            capture.ingest(block, through);
        }
        if fed < lead_samples && next_fed >= lead_samples {
            capture.start_at(pressed_at);
        }
        if next_fed > lead_samples && finish.is_none() {
            capture.become_intentional(through.min(released_at));
            if through >= released_at + POST_RELEASE_TAIL + Duration::from_millis(20) {
                finish = Some(capture.finish_with_tail(released_at, POST_RELEASE_TAIL));
            }
        }
        fed = next_fed;
    }
    let finish = finish.unwrap_or_else(|| capture.finish_with_tail(released_at, POST_RELEASE_TAIL));
    let Finish::Transcribe(clip) = finish else {
        panic!("expected transcription clip");
    };
    clip
}

fn run_parakeet(model: &mut Parakeet, mut samples: Vec<f32>) -> String {
    pad_for_parakeet(&mut samples);
    samples.resize(samples.len() + 3_200, 0.0);
    model.transcribe(&samples).unwrap()
}

fn evaluate_clip_at_release(
    model: &mut Parakeet,
    padded_audio: &[f32],
    rate: u32,
    lead_samples: usize,
    release_samples: usize,
) -> String {
    let clip = simulate_capture(padded_audio, rate, lead_samples, release_samples, true);
    run_parakeet(model, clip.into_transcription_samples())
}

#[test]
#[ignore = "requires HEX_DICTATION_CORPUS_DIR and Unified English GGUF model"]
fn evaluate_dictation_corpus() {
    let corpus_dir = PathBuf::from(
        std::env::var("HEX_DICTATION_CORPUS_DIR").expect("HEX_DICTATION_CORPUS_DIR must be set"),
    );
    let model_path = std::env::var("HEX_TAIL_MODEL")
        .map(PathBuf::from)
        .unwrap_or_else(|_| corpus_dir.join("parakeet-unified-en-0.6b-Q8_0.gguf"));
    let sweep_limit: usize = std::env::var("HEX_CORPUS_SWEEP_LIMIT")
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(24);

    let mut model =
        Parakeet::load_from(&model_path, false, Some(&TranscriptionSelection::default()))
            .expect("load Parakeet Unified English");

    let mut items: Vec<(String, String, String, PathBuf, String)> = Vec::new();

    // `pause_manifest.json` comes from `scripts/dictation-pause-fixtures.sh`.
    for manifest in [
        "say_manifest.json",
        "elevenlabs_manifest.json",
        "pause_manifest.json",
    ] {
        let manifest = corpus_dir.join("synthetic").join(manifest);
        if !manifest.exists() {
            continue;
        }
        let entries: Vec<SyntheticManifestEntry> =
            serde_json::from_slice(&fs::read(&manifest).unwrap()).unwrap();
        for e in entries {
            items.push((
                e.id,
                e.source,
                e.category,
                corpus_dir.join("synthetic").join(e.wav),
                e.text,
            ));
        }
    }

    let real_manifest = corpus_dir.join("real/manifest.json");
    if real_manifest.exists() {
        let entries: Vec<RealManifestEntry> =
            serde_json::from_slice(&fs::read(&real_manifest).unwrap()).unwrap();
        for e in entries {
            let category = if e.duration_ms >= 50_000 {
                "real_long"
            } else if e.duration_ms >= 4_500 {
                "real_multi"
            } else {
                "real_short"
            };
            items.push((
                format!("real_{}", e.id),
                "real:kit_superwhisper".into(),
                category.into(),
                corpus_dir.join("real").join(&e.id).join("output.wav"),
                e.text,
            ));
        }
    }

    let mut reports = Vec::new();
    let mut swept_count = 0usize;

    for (id, source, category, wav_path, reference_text) in items {
        if !wav_path.exists() {
            continue;
        }
        let (speech, rate) = read_mono_wav(&wav_path);
        let duration_ms = speech.len() as u64 * 1_000 / u64::from(rate);
        if duration_ms < 400 {
            continue;
        }
        let speech_end = detect_speech_end(&speech, rate);
        let speech_end_ms = speech_end as u64 * 1_000 / u64::from(rate);

        let lead = samples_for_duration(LEAD_SILENCE, rate);
        let tail = samples_for_duration(TAIL_SILENCE, rate);
        let mut padded = vec![0.0; lead];
        padded.extend_from_slice(&speech);
        padded.resize(padded.len() + tail, 0.0);

        let whole_16k = resample_for_parakeet(&speech, rate);
        let offline_text = run_parakeet(&mut model, whole_16k);
        let target_first_word = normalized_words(&offline_text)
            .into_iter()
            .next()
            .or_else(|| normalized_words(&reference_text).into_iter().next())
            .unwrap_or_default();
        let target_last_word = last_word_of(&offline_text)
            .or_else(|| last_word_of(&reference_text))
            .unwrap_or_default();
        let offline_has_last_word = !target_last_word.is_empty()
            && last_word_of(&offline_text).as_deref() == Some(target_last_word.as_str());

        let do_sweep = swept_count < sweep_limit
            && duration_ms <= 12_000
            && (category.starts_with("tail_") || category == "real_short");
        let mut offsets = Vec::new();
        if do_sweep {
            swept_count += 1;
            for offset_ms in RELEASE_SWEEP_MS {
                let rel_signed = speech_end as i64 + offset_ms * i64::from(rate) / 1_000;
                let min_rel =
                    samples_for_duration(MINIMUM_HOLD_DURATION + Duration::from_millis(20), rate)
                        as i64;
                let release_samples = rel_signed.max(min_rel) as usize;
                let transcript =
                    evaluate_clip_at_release(&mut model, &padded, rate, lead, release_samples);
                let kept_last_word = !target_last_word.is_empty()
                    && normalized_words(&transcript)
                        .last()
                        .is_some_and(|w| w == &target_last_word);
                offsets.push(OffsetOutcome {
                    offset_ms,
                    transcript,
                    kept_last_word,
                });
            }
        }

        let mut onsets = Vec::new();
        if category == "onset" {
            let speech_start = detect_speech_start(&speech, rate);
            for speech_start_vs_press_ms in [-300i64, -150, 0, 100] {
                // speech starts at `lead + speech_start` in `padded`.
                // We want `(lead + speech_start) - press_samples = speech_start_vs_press_ms`.
                let shift_samples = speech_start_vs_press_ms * i64::from(rate) / 1_000;
                let press_samples =
                    ((lead + speech_start) as i64 - shift_samples).max(rate as i64 / 2) as usize;
                let release_samples = (lead + speech.len() + rate as usize / 5)
                    .saturating_sub(press_samples)
                    .max(samples_for_duration(
                        MINIMUM_HOLD_DURATION + Duration::from_millis(20),
                        rate,
                    ));
                for warm_mic in [true, false] {
                    let clip =
                        simulate_capture(&padded, rate, press_samples, release_samples, warm_mic);
                    let transcript = run_parakeet(&mut model, clip.into_transcription_samples());
                    let kept_first_word = !target_first_word.is_empty()
                        && normalized_words(&transcript)
                            .first()
                            .is_some_and(|w| w == &target_first_word);
                    onsets.push(OnsetOutcome {
                        speech_start_vs_press_ms,
                        warm_mic,
                        transcript,
                        kept_first_word,
                    });
                }
            }
        }

        println!(
            "[{}] cat={} sr={} dur={}ms end={}ms last={:?} offline={:?}",
            id, category, rate, duration_ms, speech_end_ms, target_last_word, offline_text
        );

        reports.push(ClipReport {
            id,
            source,
            category,
            sample_rate: rate,
            duration_ms,
            speech_end_ms,
            reference_text,
            offline_text,
            target_first_word,
            target_last_word,
            offline_has_last_word,
            offsets,
            onsets,
        });
    }

    let report_path = corpus_dir.join("corpus_report.json");
    fs::write(&report_path, serde_json::to_vec_pretty(&reports).unwrap()).unwrap();
    println!(
        "Wrote {} clip reports to {}",
        reports.len(),
        report_path.display()
    );
}
