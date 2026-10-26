use std::collections::{BTreeMap, HashSet};
use std::fs;
use std::io::{BufWriter, Write};
use std::path::{Path, PathBuf};
use std::time::Instant;

use color_eyre::eyre::{Result, WrapErr, bail, eyre};
use hound::{SampleFormat, WavReader};
use serde::{Deserialize, Serialize};
use serde_json::json;

use crate::dictation::{pad_for_parakeet, resample_for_parakeet};
use crate::parakeet::Parakeet;

pub enum Backend {
    Onnx,
    TranscribeCpp { model: PathBuf },
}

enum LoadedBackend {
    TranscribeCpp(Box<Parakeet>),
    #[cfg(feature = "onnx-benchmark")]
    Onnx(transcribe_rs::onnx::parakeet::ParakeetModel),
}

impl LoadedBackend {
    fn load(backend: Backend) -> Result<Self> {
        match backend {
            Backend::Onnx => load_onnx(),
            Backend::TranscribeCpp { model } => Ok(Self::TranscribeCpp(Box::new(
                Parakeet::load_for_benchmark(&model)?,
            ))),
        }
    }

    fn name(&self) -> &str {
        match self {
            Self::TranscribeCpp(model) => model.name(),
            #[cfg(feature = "onnx-benchmark")]
            Self::Onnx(_) => "transcribe-rs-onnx-int8",
        }
    }

    fn transcribe(&mut self, samples: &[f32]) -> Result<String> {
        match self {
            Self::TranscribeCpp(model) => model.transcribe(samples),
            #[cfg(feature = "onnx-benchmark")]
            Self::Onnx(model) => {
                use transcribe_rs::onnx::parakeet::{ParakeetParams, TimestampGranularity};

                model
                    .transcribe_with(
                        samples,
                        &ParakeetParams {
                            timestamp_granularity: Some(TimestampGranularity::Segment),
                            ..Default::default()
                        },
                    )
                    .map(|result| result.text)
                    .map_err(Into::into)
            }
        }
    }
}

#[cfg(feature = "onnx-benchmark")]
fn load_onnx() -> Result<LoadedBackend> {
    use transcribe_rs::onnx::Quantization;
    use transcribe_rs::onnx::parakeet::ParakeetModel;

    let model_path = crate::app_paths::support_dir()?.join("models/parakeet-tdt-0.6b-v2-int8");
    Ok(LoadedBackend::Onnx(
        ParakeetModel::load(&model_path, &Quantization::Int8).wrap_err_with(|| {
            format!(
                "could not load ONNX Parakeet v2 from {}",
                model_path.display()
            )
        })?,
    ))
}

#[cfg(not(feature = "onnx-benchmark"))]
fn load_onnx() -> Result<LoadedBackend> {
    Err(eyre!("the ONNX backend requires --features onnx-benchmark"))
}

#[derive(Deserialize)]
struct Manifest {
    /// Scoring vocabulary, not recognition hints. Never passed to the model.
    #[serde(default)]
    vocabulary: Vec<String>,
    clips: Vec<ClipDefinition>,
}

#[derive(Deserialize)]
struct ClipDefinition {
    id: String,
    audio: PathBuf,
    expected: String,
    #[serde(default)]
    expected_terms: Vec<String>,
    #[serde(default = "ordinary_group")]
    group: String,
}

fn ordinary_group() -> String {
    "ordinary".into()
}

struct Clip {
    id: String,
    expected: String,
    samples: Vec<f32>,
    audio_ms: f64,
    expected_terms: Vec<String>,
    group: String,
}

#[derive(Default, Serialize)]
struct Quality {
    clips: usize,
    word_errors: usize,
    reference_words: usize,
    expected_terms: usize,
    recalled_terms: usize,
    false_replacements: usize,
    clips_with_false_replacements: usize,
}

impl Quality {
    fn record(&mut self, clip: &Clip, actual: &str, vocabulary: &[String]) {
        let (errors, words) = word_errors(&clip.expected, actual);
        self.clips += 1;
        self.word_errors += errors;
        self.reference_words += words;
        self.expected_terms += clip.expected_terms.len();
        self.recalled_terms += clip
            .expected_terms
            .iter()
            .filter(|term| term_count(actual, term) > 0)
            .count();
        let false_replacements = vocabulary
            .iter()
            .map(|term| term_count(actual, term).saturating_sub(term_count(&clip.expected, term)))
            .sum::<usize>();
        self.false_replacements += false_replacements;
        self.clips_with_false_replacements += usize::from(false_replacements > 0);
    }

    fn report(&self) -> serde_json::Value {
        let mut report = serde_json::to_value(self).expect("quality counters serialize");
        report["wer"] = json!(ratio(self.word_errors, self.reference_words));
        report["term_recall"] = json!(ratio(self.recalled_terms, self.expected_terms));
        report["false_replacement_rate"] =
            json!(ratio(self.clips_with_false_replacements, self.clips));
        report
    }
}

fn ratio(numerator: usize, denominator: usize) -> Option<f64> {
    (denominator > 0).then(|| numerator as f64 / denominator as f64)
}

/// Ignore case and internal spacing, but never match a substring of a word.
/// Thus OpenCode == open code, while Hex != hexadecimal and Kit != toolkit.
fn term_count(text: &str, term: &str) -> usize {
    let target = normalized_words(term).concat();
    if target.is_empty() {
        return 0;
    }
    let words = normalized_words(text);
    let mut count = 0;
    let mut start = 0;
    while start < words.len() {
        let mut candidate = String::new();
        let mut next = start + 1;
        for (end, word) in words.iter().enumerate().skip(start) {
            candidate.push_str(word);
            if candidate == target {
                count += 1;
                next = end + 1;
                break;
            }
            if !target.starts_with(&candidate) {
                break;
            }
        }
        start = next;
    }
    count
}

pub fn run(manifest_path: &Path, warmups: usize, runs: usize, backend: Backend) -> Result<()> {
    if warmups == 0 {
        bail!("benchmark requires at least one warmup run");
    }
    if runs == 0 {
        bail!("benchmark requires at least one measured run");
    }
    let manifest_data = fs::read(manifest_path)
        .wrap_err_with(|| format!("could not read {}", manifest_path.display()))?;
    let manifest: Manifest = serde_json::from_slice(&manifest_data)
        .wrap_err_with(|| format!("could not parse {}", manifest_path.display()))?;
    validate_manifest(&manifest)?;
    let vocabulary = manifest.vocabulary;
    let root = manifest_path.parent().unwrap_or_else(|| Path::new("."));
    let clips = manifest
        .clips
        .into_iter()
        .map(|definition| load_clip(root, definition))
        .collect::<Result<Vec<_>>>()?;
    let total_audio_ms = clips.iter().map(|clip| clip.audio_ms).sum::<f64>();

    let stdout = std::io::stdout();
    let mut output = BufWriter::new(stdout.lock());
    let load_started = Instant::now();
    let mut backend = LoadedBackend::load(backend)?;
    let load_ms = elapsed_ms(load_started);
    let backend_name = backend.name().to_string();
    write_json(
        &mut output,
        json!({
            "type": "model",
            "backend": backend_name,
            "load_ms": load_ms,
            "clips": clips.len(),
            "audio_ms": total_audio_ms,
            "warmups": warmups,
            "runs": runs,
            "scoring_vocabulary": vocabulary,
        }),
    )?;

    let mut first_warmup_ms = None;
    let mut first_inference_ms = None;
    for warmup in 0..warmups {
        let mut inference_ms = 0.0;
        for clip in &clips {
            let inference_started = Instant::now();
            backend.transcribe(&clip.samples)?;
            let clip_inference_ms = elapsed_ms(inference_started);
            first_inference_ms.get_or_insert(clip_inference_ms);
            inference_ms += clip_inference_ms;
            write_json(
                &mut output,
                json!({
                    "type": "warmup_clip",
                    "backend": backend_name,
                    "warmup": warmup + 1,
                    "id": clip.id,
                    "inference_ms": clip_inference_ms,
                }),
            )?;
        }
        first_warmup_ms.get_or_insert(inference_ms);
        write_json(
            &mut output,
            json!({
                "type": "warmup",
                "backend": backend_name,
                "warmup": warmup + 1,
                "inference_ms": inference_ms,
                "audio_ms": total_audio_ms,
                "rtf": inference_ms / total_audio_ms,
            }),
        )?;
    }

    let mut run_times = Vec::with_capacity(runs);
    let mut clip_times = Vec::with_capacity(runs * clips.len());
    let mut total_word_errors = 0;
    let mut total_reference_words = 0;
    let mut quality = Quality::default();
    let mut groups = BTreeMap::<String, Quality>::new();
    for run in 0..runs {
        let mut inference_ms = 0.0;
        for clip in &clips {
            let inference_started = Instant::now();
            let transcript = backend.transcribe(&clip.samples)?;
            let clip_inference_ms = elapsed_ms(inference_started);
            inference_ms += clip_inference_ms;
            clip_times.push(clip_inference_ms);
            write_json(
                &mut output,
                json!({
                    "type": "measurement",
                    "backend": backend_name,
                    "run": run + 1,
                    "id": clip.id,
                    "inference_ms": clip_inference_ms,
                }),
            )?;
            if run == 0 {
                let mut clip_quality = Quality::default();
                clip_quality.record(clip, &transcript, &vocabulary);
                quality.record(clip, &transcript, &vocabulary);
                groups.entry(clip.group.clone()).or_default().record(
                    clip,
                    &transcript,
                    &vocabulary,
                );
                let (word_errors, reference_words) = word_errors(&clip.expected, &transcript);
                total_word_errors += word_errors;
                total_reference_words += reference_words;
                write_json(
                    &mut output,
                    json!({
                        "type": "clip",
                        "backend": backend_name,
                        "id": clip.id,
                        "audio_ms": clip.audio_ms,
                        "inference_ms": clip_inference_ms,
                        "rtf": clip_inference_ms / clip.audio_ms,
                        "word_errors": word_errors,
                        "reference_words": reference_words,
                        "expected": clip.expected,
                        "transcript": transcript,
                        "group": clip.group,
                        "quality": clip_quality.report(),
                    }),
                )?;
            }
        }
        run_times.push(inference_ms);
        write_json(
            &mut output,
            json!({
                "type": "run",
                "backend": backend_name,
                "run": run + 1,
                "inference_ms": inference_ms,
                "audio_ms": total_audio_ms,
                "rtf": inference_ms / total_audio_ms,
                "x_realtime": total_audio_ms / inference_ms,
            }),
        )?;
    }

    let median_run_ms = percentile(&run_times, 0.5);
    write_json(
        &mut output,
        json!({
            "type": "summary",
            "backend": backend_name,
            "load_ms": load_ms,
            "clips": clips.len(),
            "runs": runs,
            "audio_ms": total_audio_ms,
            "first_inference_ms": first_inference_ms,
            "first_warmup_ms": first_warmup_ms,
            "median_run_ms": median_run_ms,
            "median_rtf": median_run_ms / total_audio_ms,
            "median_x_realtime": total_audio_ms / median_run_ms,
            "median_clip_ms": percentile(&clip_times, 0.5),
            "p95_clip_ms": percentile(&clip_times, 0.95),
            "max_clip_ms": percentile(&clip_times, 1.0),
            "word_errors": total_word_errors,
            "reference_words": total_reference_words,
            "wer": total_word_errors as f64 / total_reference_words as f64,
            "quality": quality.report(),
            "groups": groups.iter().map(|(name, quality)| (name, quality.report())).collect::<BTreeMap<_, _>>(),
        }),
    )?;
    output.flush()?;
    Ok(())
}

fn validate_manifest(manifest: &Manifest) -> Result<()> {
    if manifest.clips.is_empty() {
        bail!("benchmark manifest contains no clips");
    }
    let mut ids = HashSet::new();
    let vocabulary = manifest
        .vocabulary
        .iter()
        .map(|term| normalized_words(term).concat())
        .collect::<HashSet<_>>();
    if vocabulary.contains("") || vocabulary.len() != manifest.vocabulary.len() {
        bail!("scoring vocabulary must contain unique, nonempty terms");
    }
    for clip in &manifest.clips {
        if clip.id.trim().is_empty() {
            bail!("benchmark clip id cannot be empty");
        }
        if !ids.insert(&clip.id) {
            bail!("benchmark clip id is duplicated: {}", clip.id);
        }
        if normalized_words(&clip.expected).is_empty() {
            bail!("benchmark clip reference cannot be empty: {}", clip.id);
        }
        if !["target", "distractor", "ordinary", "private"].contains(&clip.group.as_str()) {
            bail!("unknown benchmark group: {}", clip.group);
        }
        if clip.group == "target" && clip.expected_terms.is_empty() {
            bail!("target clip has no expected terms: {}", clip.id);
        }
        if ["distractor", "ordinary"].contains(&clip.group.as_str())
            && !clip.expected_terms.is_empty()
        {
            bail!("guard clip cannot have expected terms: {}", clip.id);
        }
        let mut terms = HashSet::new();
        for term in &clip.expected_terms {
            let normalized = normalized_words(term).concat();
            if !vocabulary.contains(&normalized)
                || !terms.insert(normalized)
                || term_count(&clip.expected, term) == 0
            {
                bail!(
                    "expected term must be unique, in vocabulary and reference: {} ({term})",
                    clip.id
                );
            }
        }
    }
    Ok(())
}

fn load_clip(root: &Path, definition: ClipDefinition) -> Result<Clip> {
    let path = root.join(&definition.audio);
    let mut reader =
        WavReader::open(&path).wrap_err_with(|| format!("could not read {}", path.display()))?;
    let spec = reader.spec();
    let samples = match (spec.sample_format, spec.bits_per_sample) {
        (SampleFormat::Float, 32) => reader.samples::<f32>().collect::<Result<Vec<_>, _>>()?,
        (SampleFormat::Int, 1..=8) => reader
            .samples::<i8>()
            .map(|sample| sample.map(|sample| f32::from(sample) / 128.0))
            .collect::<Result<Vec<_>, _>>()?,
        (SampleFormat::Int, 9..=16) => reader
            .samples::<i16>()
            .map(|sample| {
                sample.map(|sample| f32::from(sample) / 2_f32.powi(spec.bits_per_sample as i32 - 1))
            })
            .collect::<Result<Vec<_>, _>>()?,
        (SampleFormat::Int, 17..=32) => reader
            .samples::<i32>()
            .map(|sample| {
                sample.map(|sample| sample as f32 / 2_f32.powi(spec.bits_per_sample as i32 - 1))
            })
            .collect::<Result<Vec<_>, _>>()?,
        _ => {
            return Err(eyre!(
                "unsupported WAV format in {}: {:?}, {} bits",
                path.display(),
                spec.sample_format,
                spec.bits_per_sample
            ));
        }
    };
    let channels = usize::from(spec.channels);
    if channels == 0 || samples.is_empty() || !samples.len().is_multiple_of(channels) {
        bail!("{} does not contain complete audio frames", path.display());
    }
    let mono = samples
        .chunks_exact(channels)
        .map(|frame| frame.iter().sum::<f32>() / channels as f32)
        .collect::<Vec<_>>();
    let audio_ms = mono.len() as f64 * 1_000.0 / f64::from(spec.sample_rate);
    let mut samples = resample_for_parakeet(&mono, spec.sample_rate);
    pad_for_parakeet(&mut samples);
    Ok(Clip {
        id: definition.id,
        expected: definition.expected,
        samples,
        audio_ms,
        expected_terms: definition.expected_terms,
        group: definition.group,
    })
}

fn elapsed_ms(started: Instant) -> f64 {
    started.elapsed().as_secs_f64() * 1_000.0
}

fn percentile(values: &[f64], percentile: f64) -> f64 {
    let mut sorted = values.to_vec();
    sorted.sort_by(f64::total_cmp);
    let index = ((sorted.len() as f64 * percentile).ceil() as usize)
        .saturating_sub(1)
        .min(sorted.len() - 1);
    sorted[index]
}

fn word_errors(expected: &str, actual: &str) -> (usize, usize) {
    let expected = normalized_words(expected);
    let actual = normalized_words(actual);
    let mut previous = (0..=actual.len()).collect::<Vec<_>>();
    for (expected_index, expected_word) in expected.iter().enumerate() {
        let mut current = vec![expected_index + 1];
        for (actual_index, actual_word) in actual.iter().enumerate() {
            current.push(if expected_word == actual_word {
                previous[actual_index]
            } else {
                1 + previous[actual_index]
                    .min(previous[actual_index + 1])
                    .min(current[actual_index])
            });
        }
        previous = current;
    }
    (previous[actual.len()], expected.len())
}

fn normalized_words(text: &str) -> Vec<String> {
    text.split(|character: char| !character.is_alphanumeric())
        .filter(|word| !word.is_empty())
        .map(str::to_lowercase)
        .collect()
}

fn write_json(output: &mut impl Write, value: serde_json::Value) -> Result<()> {
    serde_json::to_writer(&mut *output, &value)?;
    output.write_all(b"\n")?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use std::time::{SystemTime, UNIX_EPOCH};

    use hound::{WavSpec, WavWriter};

    use super::*;

    #[test]
    fn word_error_rate_normalizes_case_and_punctuation() {
        assert_eq!(
            word_errors("Use OpenCode, please.", "use opencode please"),
            (0, 3)
        );
    }

    #[test]
    fn word_error_rate_counts_edits() {
        assert_eq!(word_errors("one two three", "one four"), (2, 3));
        assert_eq!(word_errors("one two", "zero one two three"), (2, 2));
    }

    #[test]
    fn percentile_uses_nearest_rank() {
        assert_eq!(percentile(&[9.0, 1.0, 3.0, 2.0], 0.5), 2.0);
        assert_eq!(percentile(&[9.0, 1.0, 3.0, 2.0], 0.95), 9.0);
    }

    #[test]
    fn manifest_requires_unique_ids_and_spoken_references() {
        let manifest = |clips| Manifest {
            vocabulary: vec![],
            clips,
        };
        let clip = |id: &str, expected: &str| ClipDefinition {
            id: id.into(),
            audio: "unused.wav".into(),
            expected: expected.into(),
            expected_terms: vec![],
            group: ordinary_group(),
        };

        assert!(validate_manifest(&manifest(vec![])).is_err());
        assert!(validate_manifest(&manifest(vec![clip("blank", "...")])).is_err());
        assert!(
            validate_manifest(&manifest(vec![clip("same", "one"), clip("same", "two")])).is_err()
        );
        assert!(validate_manifest(&manifest(vec![clip("valid", "one")])).is_ok());
    }

    #[test]
    fn wav_preparation_downmixes_and_pads_without_changing_duration() {
        let unique = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let directory = std::env::temp_dir().join(format!(
            "voice-control-transcription-benchmark-{}-{unique}",
            std::process::id()
        ));
        fs::create_dir(&directory).unwrap();
        let path = directory.join("stereo.wav");
        let mut writer = WavWriter::create(
            &path,
            WavSpec {
                channels: 2,
                sample_rate: 16_000,
                bits_per_sample: 32,
                sample_format: SampleFormat::Float,
            },
        )
        .unwrap();
        for sample in [1.0, -1.0, 0.5, 0.5] {
            writer.write_sample(sample).unwrap();
        }
        writer.finalize().unwrap();

        let clip = load_clip(
            &directory,
            ClipDefinition {
                id: "stereo".into(),
                audio: "stereo.wav".into(),
                expected: "reference".into(),
                expected_terms: vec![],
                group: ordinary_group(),
            },
        )
        .unwrap();

        assert_eq!(clip.audio_ms, 0.125);
        assert_eq!(clip.samples.len(), 24_000);
        assert_eq!(&clip.samples[..2], &[0.0, 0.5]);
        fs::remove_dir_all(directory).unwrap();
    }

    #[test]
    fn vocabulary_scoring_respects_word_boundaries_and_counts_insertions() {
        assert_eq!(term_count("Use OPEN CODE and OpenCode.", "OpenCode"), 2);
        assert_eq!(term_count("hexadecimal toolkit effects runes", "Hex"), 0);
        assert_eq!(term_count("hexadecimal toolkit effects runes", "Kit"), 0);
        assert_eq!(term_count("a rune carved in stone", "Rune"), 1);
        let clip = Clip {
            id: "guard".into(),
            expected: "Tom Hanks found a rune".into(),
            expected_terms: vec![],
            group: "distractor".into(),
            samples: vec![],
            audio_ms: 1000.0,
        };
        let mut quality = Quality::default();
        quality.record(
            &clip,
            "Tom Hex found a rune",
            &["Hex".into(), "Rune".into()],
        );
        assert_eq!(quality.false_replacements, 1);
        assert_eq!(quality.clips_with_false_replacements, 1);
        assert_eq!(quality.report()["term_recall"], serde_json::Value::Null);
        assert_eq!(quality.word_errors, 1);
    }
}
