//! Where each catalog model's error rate comes from, as Labs › Benchmark
//! details shows it, and whether two figures may be compared.

use super::models::{ModelDefinition, TranscriptionModelId, choices_for_runtime};

/// The error measure a benchmark reports; lower is better for both.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ErrorMetric {
    /// Word error rate.
    Wer,
    /// Character error rate, for languages written without spaces.
    Cer,
}

impl ErrorMetric {
    pub const fn label(self) -> &'static str {
        match self {
            Self::Wer => "WER",
            Self::Cer => "CER",
        }
    }
}

/// Where one model's error rate comes from, as Labs › Benchmark details
/// shows it. Every `None` is something the source did not record and the
/// picker says is unknown; nothing is guessed. Speed figures (`realtime`)
/// have no record: they are upstream figures whose hardware and method are
/// unknown.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct BenchmarkRecord {
    pub metric: ErrorMetric,
    /// In percent.
    pub value: f64,
    /// The language code the benchmark measured.
    pub language: &'static str,
    /// The dataset and its split, such as "FLEURS test".
    pub dataset: Option<&'static str>,
    /// Plain text naming where the figure comes from.
    pub source: Option<&'static str>,
    /// The source record's revision; not an inference run.
    pub source_revision: Option<&'static str>,
    pub utterances: Option<u32>,
    pub batch_size: Option<u32>,
    /// Whether the run asked for timestamps.
    pub timestamps: Option<bool>,
    pub hardware: Option<&'static str>,
}

impl BenchmarkRecord {
    /// A record whose source, dataset, and conditions were not recorded.
    const fn unrecorded(metric: ErrorMetric, value: f64, language: &'static str) -> Self {
        Self {
            metric,
            value,
            language,
            dataset: None,
            source: None,
            source_revision: None,
            utterances: None,
            batch_size: None,
            timestamps: None,
            hardware: None,
        }
    }

    /// The raw figure: `7.57% CER`.
    pub fn rate_label(&self) -> String {
        format!("{:.2}% {}", self.value, self.metric.label())
    }

    /// Whether the two figures measure the same thing on the same data, so
    /// one may be called better than the other. An unknown dataset matches
    /// nothing.
    pub fn comparable(&self, other: &Self) -> bool {
        self.metric == other.metric
            && self.language == other.language
            && self.dataset.is_some()
            && self.dataset == other.dataset
    }
}

/// The transcription runtime's structured records for its Mandarin FLEURS
/// runs, both legacy-published with no engine revision: the original run
/// IDs and scoring or artifact revisions are unknown. The catalog's
/// `word_error_rate` keeps the narrative figures (7.64 and 10.11) as
/// alternate legacy values; cards with the Lab on show these instead.
const MANDARIN_FLEURS_SOURCE: &str = "transcribe.cpp benchmark record, legacy-published";
const MANDARIN_FLEURS_REVISION: &str = "7045d1c";

const QWEN3_ASR_RECORD: BenchmarkRecord = BenchmarkRecord {
    metric: ErrorMetric::Cer,
    value: 7.57,
    language: "zh",
    dataset: Some("FLEURS test"),
    source: Some(MANDARIN_FLEURS_SOURCE),
    source_revision: Some(MANDARIN_FLEURS_REVISION),
    utterances: Some(945),
    batch_size: Some(8),
    timestamps: Some(false),
    hardware: None,
};
const SENSEVOICE_RECORD: BenchmarkRecord = BenchmarkRecord {
    value: 10.12,
    ..QWEN3_ASR_RECORD
};
const QWENCLEO_RECORD: BenchmarkRecord = BenchmarkRecord {
    dataset: Some("Author's Egyptian Arabic test"),
    source: Some("The model author's own evaluation"),
    utterances: Some(3_699),
    ..BenchmarkRecord::unrecorded(ErrorMetric::Wer, 19.85, "ar")
};

/// Hex's own run of transcribe.cpp's WER harness (`scripts/wer/run.py` and
/// `score.py`, language `he`, greedy, no timestamps) over the full FLEURS
/// Hebrew test split on apollo. Stock Whisper large-v3-turbo scored 29.69%
/// on the same run, so the fine-tune makes 38% fewer word errors; the
/// reporter of #752 measured 18.46% against 29.51% (37%) on an RTX 3090.
const IVRIT_WHISPER_RECORD: BenchmarkRecord = BenchmarkRecord {
    metric: ErrorMetric::Wer,
    value: 18.45,
    language: "he",
    dataset: Some("FLEURS test"),
    source: Some("Hex's run of the transcribe.cpp WER harness"),
    source_revision: Some("7045d1c"),
    utterances: Some(792),
    batch_size: Some(1),
    timestamps: Some(false),
    hardware: Some("Radeon 8060S (Vulkan)"),
};

/// transcribe.cpp's published conversion benchmark for the Q8_0 file
/// (handy-computer/parakeet-ultra-gguf's card): LibriSpeech test-clean in
/// full, batch 8, no timestamps, the `en` hint, decoded on CUDA at
/// transcribe.cpp 021aa17d. The benchmark artifact's digest and the scorer
/// versions are unrecorded; not measured in Hex.
const PARAKEET_ULTRA_RECORD: BenchmarkRecord = BenchmarkRecord {
    metric: ErrorMetric::Wer,
    value: 1.80,
    language: "en",
    dataset: Some("LibriSpeech test-clean"),
    source: Some("transcribe.cpp's published conversion benchmark"),
    source_revision: Some("021aa17d"),
    utterances: Some(2_620),
    batch_size: Some(8),
    timestamps: Some(false),
    hardware: Some("CUDA"),
};

/// `model`'s benchmark record, or `None` when the system manages the model
/// and publishes no figure (Apple Speech).
pub fn benchmark(model: &ModelDefinition) -> Option<BenchmarkRecord> {
    match model.id {
        TranscriptionModelId::Qwen3Asr06B => Some(QWEN3_ASR_RECORD),
        TranscriptionModelId::SenseVoiceSmall => Some(SENSEVOICE_RECORD),
        TranscriptionModelId::QwenCleoAsr17B => Some(QWENCLEO_RECORD),
        TranscriptionModelId::IvritWhisperLargeV3Turbo => Some(IVRIT_WHISPER_RECORD),
        TranscriptionModelId::ParakeetUltra => Some(PARAKEET_ULTRA_RECORD),
        // The catalog's English figures: their dataset, source, and
        // conditions were not recorded.
        _ => model
            .word_error_rate
            .map(|rate| BenchmarkRecord::unrecorded(ErrorMetric::Wer, rate, "en")),
    }
}

/// Whether `model` may keep a "Most accurate" badge for `language` with
/// Labs › Benchmark details on: it has a record, and every other
/// recommended model for the language that has one is comparable to it.
pub(crate) fn most_accurate_is_comparable(model: &ModelDefinition, language: &str) -> bool {
    let Some(record) = benchmark(model) else {
        return false;
    };
    record.dataset.is_some()
        && choices_for_runtime(language)
            .iter()
            .filter(|choice| choice.model.id != model.id)
            .filter_map(|choice| benchmark(choice.model))
            .all(|other| record.comparable(&other))
}
