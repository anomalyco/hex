use std::fs::{self, File};
use std::io::{ErrorKind, Read};
use std::path::{Path, PathBuf};
use std::process::Stdio;
use std::str::FromStr;
use std::sync::atomic::{AtomicBool, AtomicU8, AtomicU64, Ordering};
use std::thread;
use std::time::{Duration, UNIX_EPOCH};

use color_eyre::eyre::{Result, WrapErr, bail, eyre};
use fs2::FileExt;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

#[derive(Clone, Copy, Debug, Default, Deserialize, Eq, Hash, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum TranscriptionModelId {
    #[default]
    #[serde(rename = "parakeet_unified_en")]
    ParakeetUnifiedEnglish,
    ParakeetV2,
    ParakeetV3,
    WhisperLargeV3Turbo,
    Qwen3Asr06B,
    SenseVoiceSmall,
    CohereTranscribe,
    AppleSpeech,
}

impl TranscriptionModelId {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::ParakeetUnifiedEnglish => "parakeet_unified_en",
            Self::ParakeetV2 => "parakeet_v2",
            Self::ParakeetV3 => "parakeet_v3",
            Self::WhisperLargeV3Turbo => "whisper_large_v3_turbo",
            Self::Qwen3Asr06B => "qwen3_asr06_b",
            Self::SenseVoiceSmall => "sense_voice_small",
            Self::CohereTranscribe => "cohere_transcribe",
            Self::AppleSpeech => "apple_speech",
        }
    }
}

impl FromStr for TranscriptionModelId {
    type Err = color_eyre::Report;

    fn from_str(value: &str) -> Result<Self> {
        match value {
            "parakeet_unified_en" => Ok(Self::ParakeetUnifiedEnglish),
            "parakeet_v2" => Ok(Self::ParakeetV2),
            "parakeet_v3" => Ok(Self::ParakeetV3),
            "whisper_large_v3_turbo" => Ok(Self::WhisperLargeV3Turbo),
            "qwen3_asr06_b" => Ok(Self::Qwen3Asr06B),
            "sense_voice_small" => Ok(Self::SenseVoiceSmall),
            "cohere_transcribe" => Ok(Self::CohereTranscribe),
            "apple_speech" => Ok(Self::AppleSpeech),
            _ => Err(eyre!("unknown transcription model: {value}")),
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[repr(u8)]
pub enum ModelPreparationStage {
    Downloading,
    Verifying,
    Loading,
}

impl ModelPreparationStage {
    pub fn store(self, stage: &AtomicU8) {
        stage.store(self as u8, Ordering::Release);
    }

    #[cfg(not(target_os = "windows"))]
    pub fn load(stage: &AtomicU8) -> Self {
        match stage.load(Ordering::Acquire) {
            value if value == Self::Downloading as u8 => Self::Downloading,
            value if value == Self::Verifying as u8 => Self::Verifying,
            value if value == Self::Loading as u8 => Self::Loading,
            value => panic!("invalid model preparation stage {value}"),
        }
    }
}

#[derive(Deserialize, Serialize)]
struct VerificationReceipt {
    bytes: u64,
    sha256: String,
    modified_ns: u64,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(default)]
pub struct TranscriptionSelection {
    pub model: TranscriptionModelId,
    pub language: String,
    pub recognition_hints: String,
}

impl Default for TranscriptionSelection {
    fn default() -> Self {
        Self {
            model: TranscriptionModelId::default(),
            language: "en".into(),
            recognition_hints: String::new(),
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Recommendation {
    Recommended,
    Fastest,
    MostAccurate,
    RecognitionHints,
}

impl Recommendation {
    pub const fn label(self) -> &'static str {
        match self {
            Self::Recommended => "Recommended",
            Self::Fastest => "Fastest",
            Self::MostAccurate => "Most accurate",
            Self::RecognitionHints => "Recognition hints",
        }
    }
}

#[derive(Clone, Copy)]
pub struct ModelChoice {
    pub model: &'static ModelDefinition,
    pub recommendation: Recommendation,
}

pub struct ModelDefinition {
    pub id: TranscriptionModelId,
    pub name: &'static str,
    pub realtime: &'static str,
    /// Word error rate on the `quality_context` benchmark, in percent;
    /// `None` when the system manages the model and publishes none.
    pub word_error_rate: Option<f64>,
    pub quality_context: &'static str,
    pub coverage: &'static str,
    pub timestamps: &'static str,
    /// The pinned artifact advertises streaming, so dictation can transcribe
    /// while the shortcut is held. It still streams only on a GPU device and
    /// after its load probe succeeds; otherwise it transcribes the whole clip.
    pub streams: bool,
    pub runtime: ModelRuntime,
    pub languages: &'static [&'static str],
    pub accepts_language_hint: bool,
    pub supports_language_detection: bool,
    pub supports_recognition_hints: bool,
}

#[derive(Clone, Copy)]
pub enum ModelRuntime {
    Gguf(&'static GgufArtifact),
    AppleSpeech,
}

pub struct GgufArtifact {
    pub filename: &'static str,
    pub revision: &'static str,
    pub repository: &'static str,
    pub bytes: u64,
    pub sha256: &'static str,
    pub architecture: &'static str,
    pub variant: &'static str,
}

impl GgufArtifact {
    fn download_url(&self) -> String {
        format!(
            "https://huggingface.co/{}/resolve/{}/{}",
            self.repository, self.revision, self.filename
        )
    }
}

impl ModelDefinition {
    pub const fn available(&self) -> bool {
        !matches!(self.runtime, ModelRuntime::AppleSpeech)
    }

    pub fn supports_language(&self, language: &str) -> bool {
        if language == AUTO_LANGUAGE {
            self.supports_language_detection
        } else {
            self.languages.contains(&"*") || self.languages.contains(&language)
        }
    }

    pub fn runtime_language<'a>(&self, language: &'a str) -> &'a str {
        if self.id == TranscriptionModelId::WhisperLargeV3Turbo && language == "fil" {
            "tl"
        } else {
            language
        }
    }

    pub fn runtime_language_hint<'a>(&self, language: &'a str) -> Option<&'a str> {
        (language != AUTO_LANGUAGE && self.accepts_language_hint)
            .then(|| self.runtime_language(language))
    }

    /// The languages Settings offers for this model, in [`LANGUAGES`] order,
    /// Auto first when the model detects the language.
    pub fn language_choices(&self) -> Vec<&'static str> {
        LANGUAGES
            .iter()
            .map(|(code, _)| *code)
            .filter(|code| self.supports_language(code))
            .collect()
    }

    /// The language a switch to this model falls back to when it does not
    /// support the selected one: English when it can, else its first.
    pub fn default_language(&self) -> &'static str {
        if self.supports_language("en") {
            "en"
        } else {
            self.languages[0]
        }
    }

    /// `language` when this model supports it, else its default language, so
    /// a model switch always yields a valid selection.
    pub fn language_for<'a>(&self, language: &'a str) -> &'a str {
        if self.supports_language(language) {
            language
        } else {
            self.default_language()
        }
    }

    #[cfg(target_os = "macos")]
    pub fn presentation_languages(&self) -> Vec<&'static str> {
        self.languages
            .iter()
            .copied()
            .chain(self.supports_language_detection.then_some(AUTO_LANGUAGE))
            .collect()
    }

    /// Benchmark accuracy, the inverse of its word error rate: `98.4%`.
    pub fn accuracy_label(&self) -> Option<String> {
        self.word_error_rate.map(accuracy_label)
    }

    pub fn size_label(&self) -> String {
        match self.runtime {
            ModelRuntime::AppleSpeech => "Managed".into(),
            ModelRuntime::Gguf(artifact) if artifact.bytes >= 1_000_000_000 => {
                format!("{:.1} GB", artifact.bytes as f64 / 1_000_000_000.0)
            }
            ModelRuntime::Gguf(artifact) => format!("{} MB", artifact.bytes / 1_000_000),
        }
    }

    pub const fn download_bytes(&self) -> Option<u64> {
        match self.runtime {
            ModelRuntime::Gguf(artifact) => Some(artifact.bytes),
            ModelRuntime::AppleSpeech => None,
        }
    }
}

fn accuracy_label(word_error_rate: f64) -> String {
    format!("{:.1}%", (100.0 - word_error_rate).clamp(0.0, 100.0))
}

const PARAKEET_V3_LANGUAGES: &[&str] = &[
    "bg", "hr", "cs", "da", "nl", "en", "et", "fi", "fr", "de", "el", "hu", "it", "lv", "lt", "mt",
    "pl", "pt", "ro", "ru", "sk", "sl", "es", "sv", "uk",
];
const QWEN_LANGUAGES: &[&str] = &[
    "zh", "en", "yue", "ar", "de", "fr", "es", "pt", "id", "it", "ko", "ru", "th", "vi", "ja",
    "tr", "hi", "ms", "nl", "sv", "da", "fi", "pl", "cs", "fil", "fa", "el", "ro", "hu", "mk",
];
const COHERE_LANGUAGES: &[&str] = &[
    "en", "fr", "de", "es", "it", "pt", "nl", "pl", "el", "ar", "ja", "zh", "vi", "ko",
];

const PARAKEET_V2_ARTIFACT: GgufArtifact = GgufArtifact {
    filename: "parakeet-tdt-0.6b-v2-Q8_0.gguf",
    revision: "07cee0616125a08ef619729bb47f40ef747e4bc4",
    repository: "handy-computer/parakeet-tdt-0.6b-v2-gguf",
    bytes: 729_574_912,
    sha256: "f0d0e99cebb6d3b83f1f7069b82b5d3c2e39a54545b0da039cb4bafd9c4e5caa",
    architecture: "parakeet",
    variant: "tdt-0.6b-v2",
};
const PARAKEET_UNIFIED_ENGLISH_ARTIFACT: GgufArtifact = GgufArtifact {
    filename: "parakeet-unified-en-0.6b-Q8_0.gguf",
    revision: "7e948f21b7bdbac698d3318db9d350f1096f3b6c",
    repository: "handy-computer/parakeet-unified-en-0.6b-gguf",
    bytes: 731_357_568,
    sha256: "4b50b6dd862bf6e346929aaf4f5eaacec003bfa3f56462d6c874b41ef2f38795",
    architecture: "parakeet",
    variant: "unified-en-0.6b",
};
const PARAKEET_V3_ARTIFACT: GgufArtifact = GgufArtifact {
    filename: "parakeet-tdt-0.6b-v3-Q8_0.gguf",
    revision: "85ac09ea12fc4b1112fa76810059364bc6adc9de",
    repository: "handy-computer/parakeet-tdt-0.6b-v3-gguf",
    bytes: 739_508_576,
    sha256: "5859f77944efcd8eafa23a6350731960b2b55b2203df51f319665c807d802cc7",
    architecture: "parakeet",
    variant: "tdt-0.6b-v3",
};
const WHISPER_ARTIFACT: GgufArtifact = GgufArtifact {
    filename: "whisper-large-v3-turbo-Q8_0.gguf",
    revision: "d222c9f621c1128299248f2ded4d8a1820519780",
    repository: "handy-computer/whisper-large-v3-turbo-gguf",
    bytes: 886_381_824,
    sha256: "d5e65f2b0828802ae2c231673d31982cebe3a778c95d9494a9f3efee6bd17448",
    architecture: "whisper",
    variant: "whisper-large-v3-turbo",
};
const QWEN_ARTIFACT: GgufArtifact = GgufArtifact {
    filename: "Qwen3-ASR-0.6B-Q8_0.gguf",
    revision: "e4e16599b900eb0cb36e524514756bb92eb092b7",
    repository: "handy-computer/Qwen3-ASR-0.6B-gguf",
    bytes: 850_423_456,
    sha256: "f081b2d5e23bd669d92cc331d722a8a0681943b8e6f34b48996fd5c319b5acd8",
    architecture: "qwen3_asr",
    variant: "qwen3-asr-0.6b",
};
const SENSEVOICE_ARTIFACT: GgufArtifact = GgufArtifact {
    filename: "SenseVoiceSmall-Q8_0.gguf",
    revision: "4a08b8e900b38a977e32eb08d5d0697d6e72ba04",
    repository: "handy-computer/SenseVoiceSmall-gguf",
    bytes: 252_684_608,
    sha256: "6c759ee4c9748c9b3f7a5a60ca74f0f7e685fb9d45d1378fce7cfd62f59adf29",
    architecture: "sensevoice",
    variant: "sensevoice-small",
};
const COHERE_ARTIFACT: GgufArtifact = GgufArtifact {
    filename: "cohere-transcribe-03-2026-Q8_0.gguf",
    revision: "dfa4adebb64f3076b7b6b90b721275cc069cb421",
    repository: "handy-computer/cohere-transcribe-03-2026-gguf",
    bytes: 2_410_655_232,
    sha256: "931916663432fd895423a4291a8400221802b288967ca2d435fc5e3141c9e71e",
    architecture: "cohere_asr",
    variant: "cohere-transcribe-03-2026",
};
/// Silero VAD v6.2. Dictation and meetings skip audio it finds no speech in,
/// because models hallucinate closing phrases such as "thank you" on silence
/// and room noise. It downloads with every transcription model.
pub const SPEECH_DETECTOR_ARTIFACT: GgufArtifact = GgufArtifact {
    filename: "silero-vad-v6.2-F32.gguf",
    revision: "a9b2d61c20ae4ddf3644edb34969e8185a485448",
    repository: "handy-computer/silero-vad-v6.2-gguf",
    bytes: 1_241_056,
    sha256: "0d871a800c420fdd5d1b17a3aea4296d1c39608f0ff4a9295eb76457eb1c88e6",
    architecture: "silero_vad",
    variant: "v6.2",
};
#[cfg_attr(test, allow(dead_code))]
const SPEECH_DETECTOR_NAME: &str = "the speech detector";

pub const MODELS: &[ModelDefinition] = &[
    ModelDefinition {
        id: TranscriptionModelId::ParakeetUnifiedEnglish,
        name: "Parakeet Unified English",
        realtime: "~163x",
        word_error_rate: Some(1.60),
        quality_context: "English benchmark",
        coverage: "English",
        timestamps: "Token timestamps",
        streams: true,
        runtime: ModelRuntime::Gguf(&PARAKEET_UNIFIED_ENGLISH_ARTIFACT),
        languages: &["en"],
        accepts_language_hint: true,
        supports_language_detection: false,
        supports_recognition_hints: false,
    },
    ModelDefinition {
        id: TranscriptionModelId::ParakeetV2,
        name: "Parakeet v2",
        realtime: "~55x",
        word_error_rate: Some(1.69),
        quality_context: "English benchmark",
        coverage: "English",
        timestamps: "Token timestamps",
        streams: false,
        runtime: ModelRuntime::Gguf(&PARAKEET_V2_ARTIFACT),
        languages: &["en"],
        accepts_language_hint: true,
        supports_language_detection: false,
        supports_recognition_hints: false,
    },
    ModelDefinition {
        id: TranscriptionModelId::ParakeetV3,
        name: "Parakeet v3",
        realtime: "~33x",
        word_error_rate: Some(1.94),
        quality_context: "English benchmark",
        coverage: "25 languages",
        timestamps: "Token timestamps",
        streams: false,
        runtime: ModelRuntime::Gguf(&PARAKEET_V3_ARTIFACT),
        languages: PARAKEET_V3_LANGUAGES,
        // The pinned v3 artifact detects language from audio and has no prompt input.
        accepts_language_hint: false,
        supports_language_detection: false,
        supports_recognition_hints: false,
    },
    ModelDefinition {
        id: TranscriptionModelId::WhisperLargeV3Turbo,
        name: "Whisper large-v3-turbo",
        realtime: "~19x",
        word_error_rate: Some(2.01),
        quality_context: "English benchmark",
        coverage: "100 languages",
        timestamps: "Segment timestamps",
        streams: false,
        runtime: ModelRuntime::Gguf(&WHISPER_ARTIFACT),
        languages: &["*"],
        accepts_language_hint: true,
        supports_language_detection: true,
        supports_recognition_hints: true,
    },
    ModelDefinition {
        id: TranscriptionModelId::Qwen3Asr06B,
        name: "Qwen3-ASR 0.6B",
        realtime: "~16x",
        word_error_rate: Some(7.64),
        quality_context: "Mandarin benchmark",
        coverage: "30 languages",
        timestamps: "No timestamps",
        streams: false,
        runtime: ModelRuntime::Gguf(&QWEN_ARTIFACT),
        languages: QWEN_LANGUAGES,
        accepts_language_hint: true,
        supports_language_detection: true,
        supports_recognition_hints: false,
    },
    ModelDefinition {
        id: TranscriptionModelId::SenseVoiceSmall,
        name: "SenseVoice Small",
        realtime: "~72x",
        word_error_rate: Some(10.11),
        quality_context: "Mandarin benchmark",
        coverage: "5 languages",
        timestamps: "No timestamps",
        streams: false,
        runtime: ModelRuntime::Gguf(&SENSEVOICE_ARTIFACT),
        languages: &["zh", "yue", "en", "ja", "ko"],
        accepts_language_hint: true,
        supports_language_detection: true,
        supports_recognition_hints: false,
    },
    ModelDefinition {
        id: TranscriptionModelId::CohereTranscribe,
        name: "Cohere Transcribe",
        realtime: "73x",
        word_error_rate: Some(1.27),
        quality_context: "English benchmark",
        coverage: "14 languages",
        timestamps: "No timestamps",
        streams: false,
        runtime: ModelRuntime::Gguf(&COHERE_ARTIFACT),
        languages: COHERE_LANGUAGES,
        accepts_language_hint: true,
        supports_language_detection: false,
        supports_recognition_hints: false,
    },
    ModelDefinition {
        id: TranscriptionModelId::AppleSpeech,
        name: "Apple Speech",
        realtime: "On device",
        word_error_rate: None,
        quality_context: "Apple managed",
        coverage: "System locales",
        timestamps: "Segment timestamps",
        streams: false,
        runtime: ModelRuntime::AppleSpeech,
        languages: &["*"],
        accepts_language_hint: true,
        supports_language_detection: false,
        supports_recognition_hints: false,
    },
];

pub const AUTO_LANGUAGE: &str = "auto";

pub const LANGUAGES: &[(&str, &str)] = &[
    (AUTO_LANGUAGE, "Auto"),
    ("en", "English"),
    ("zh", "Mandarin Chinese"),
    ("yue", "Cantonese"),
    ("ja", "Japanese"),
    ("ko", "Korean"),
    ("es", "Spanish"),
    ("fr", "French"),
    ("de", "German"),
    ("pt", "Portuguese"),
    ("it", "Italian"),
    ("ar", "Arabic"),
    ("hi", "Hindi"),
    ("bn", "Bangla"),
    ("vi", "Vietnamese"),
    ("ru", "Russian"),
    ("uk", "Ukrainian"),
    ("nl", "Dutch"),
    ("pl", "Polish"),
    ("tr", "Turkish"),
    ("sv", "Swedish"),
    ("da", "Danish"),
    ("fi", "Finnish"),
    ("cs", "Czech"),
    ("el", "Greek"),
    ("ro", "Romanian"),
    ("hu", "Hungarian"),
    ("id", "Indonesian"),
    ("ms", "Malay"),
    ("th", "Thai"),
    ("fa", "Persian"),
    ("bg", "Bulgarian"),
    ("hr", "Croatian"),
    ("et", "Estonian"),
    ("lv", "Latvian"),
    ("lt", "Lithuanian"),
    ("mt", "Maltese"),
    ("sk", "Slovak"),
    ("sl", "Slovenian"),
    ("mk", "Macedonian"),
    ("fil", "Filipino"),
];

pub fn definition(id: TranscriptionModelId) -> &'static ModelDefinition {
    MODELS
        .iter()
        .find(|model| model.id == id)
        .expect("every transcription model id has a definition")
}

pub fn language_name(code: &str) -> &str {
    LANGUAGES
        .iter()
        .find_map(|(candidate, name)| (*candidate == code).then_some(*name))
        .unwrap_or(code)
}

pub(crate) fn choices_for_runtime(language: &str) -> Vec<ModelChoice> {
    use Recommendation::{Fastest, MostAccurate, RecognitionHints, Recommended};
    use TranscriptionModelId::{
        CohereTranscribe, ParakeetUnifiedEnglish, ParakeetV3, Qwen3Asr06B, SenseVoiceSmall,
        WhisperLargeV3Turbo,
    };
    let choices: &[(TranscriptionModelId, Recommendation)] = match language {
        AUTO_LANGUAGE => &[(WhisperLargeV3Turbo, Recommended), (Qwen3Asr06B, Fastest)],
        "en" => &[
            (ParakeetUnifiedEnglish, Recommended),
            (CohereTranscribe, MostAccurate),
            (WhisperLargeV3Turbo, RecognitionHints),
        ],
        "zh" => &[
            (Qwen3Asr06B, Recommended),
            (SenseVoiceSmall, Fastest),
            (WhisperLargeV3Turbo, RecognitionHints),
        ],
        "yue" | "ko" => &[
            (Qwen3Asr06B, Recommended),
            (WhisperLargeV3Turbo, RecognitionHints),
        ],
        "ja" => &[
            (WhisperLargeV3Turbo, Recommended),
            (SenseVoiceSmall, Fastest),
        ],
        "el" => &[
            (CohereTranscribe, Recommended),
            (WhisperLargeV3Turbo, RecognitionHints),
        ],
        "fi" | "sv" | "da" => &[(WhisperLargeV3Turbo, Recommended), (ParakeetV3, Fastest)],
        "mt" => &[(ParakeetV3, Recommended)],
        "ar" => &[
            (WhisperLargeV3Turbo, Recommended),
            (CohereTranscribe, MostAccurate),
        ],
        "tr" | "ms" | "fil" | "fa" | "mk" => &[(WhisperLargeV3Turbo, Recommended)],
        "vi" => &[
            (Qwen3Asr06B, Recommended),
            (CohereTranscribe, MostAccurate),
            (WhisperLargeV3Turbo, RecognitionHints),
        ],
        language if PARAKEET_V3_LANGUAGES.contains(&language) => &[
            (ParakeetV3, Recommended),
            (WhisperLargeV3Turbo, RecognitionHints),
        ],
        language if QWEN_LANGUAGES.contains(&language) => &[
            (Qwen3Asr06B, Recommended),
            (WhisperLargeV3Turbo, RecognitionHints),
        ],
        _ => &[(WhisperLargeV3Turbo, Recommended)],
    };
    choices
        .iter()
        .map(|&(id, recommendation)| ModelChoice {
            model: definition(id),
            recommendation,
        })
        .collect()
}

/// Every available model in the picker's order for `language`: its
/// recommendations first, each with its badge, then the other models that
/// support it, then those that would change the language.
pub(crate) fn picker_models(
    language: &str,
) -> Vec<(&'static ModelDefinition, Option<Recommendation>)> {
    let recommended = choices_for_runtime(language);
    let others = MODELS
        .iter()
        .filter(|model| model.available())
        .filter(|model| recommended.iter().all(|choice| choice.model.id != model.id));
    let (supporting, switching): (Vec<_>, Vec<_>) =
        others.partition(|model| model.supports_language(language));
    recommended
        .iter()
        .map(|choice| (choice.model, Some(choice.recommendation)))
        .chain(
            supporting
                .into_iter()
                .chain(switching)
                .map(|model| (model, None)),
        )
        .collect()
}

pub fn validate(selection: &TranscriptionSelection) -> Result<&'static ModelDefinition> {
    if !LANGUAGES
        .iter()
        .any(|(code, _)| *code == selection.language)
    {
        bail!("unsupported transcription language: {}", selection.language);
    }
    let model = definition(selection.model);
    if !model.available() {
        bail!("{} is unavailable", model.name);
    }
    if !model.supports_language(&selection.language) {
        bail!(
            "{} does not support {}",
            model.name,
            language_name(&selection.language)
        );
    }
    if !model.supports_recognition_hints && !selection.recognition_hints.trim().is_empty() {
        bail!("{} does not support recognition hints", model.name);
    }
    Ok(model)
}

pub fn models_dir() -> Result<PathBuf> {
    #[cfg(test)]
    if let Some(directory) = TEST_MODELS_DIR.with(|directory| directory.borrow().clone()) {
        return Ok(directory);
    }
    Ok(crate::app_paths::support_dir()?.join("models"))
}

#[cfg(test)]
thread_local! {
    static TEST_MODELS_DIR: std::cell::RefCell<Option<PathBuf>> = const { std::cell::RefCell::new(None) };
}

pub fn max_audio_chunk_samples(architecture: &str, max_audio_ms: i64) -> Option<usize> {
    // Cohere's encoder capacity is not its recommended transcription window.
    // The pinned model's reference processor chunks long audio at 35 seconds.
    let maximum_ms = match (architecture, max_audio_ms) {
        ("cohere_asr", limit) if limit > 0 => limit.min(35_000),
        ("cohere_asr", _) => 35_000,
        (_, limit) => limit,
    };
    usize::try_from(maximum_ms)
        .ok()
        .filter(|milliseconds| *milliseconds > 0)?
        .checked_mul(16)
}

pub fn model_path(model: &ModelDefinition) -> Result<PathBuf> {
    let ModelRuntime::Gguf(artifact) = model.runtime else {
        bail!("{} is managed by macOS and has no model path", model.name);
    };
    Ok(models_dir()?.join(artifact.filename))
}

pub fn is_installed(model: &ModelDefinition, language: &str) -> bool {
    #[cfg(not(target_os = "macos"))]
    let _ = language;
    match model.runtime {
        ModelRuntime::Gguf(artifact) => model_path(model)
            .and_then(|path| Ok(fs::metadata(path)?.len() == artifact.bytes))
            .unwrap_or(false),
        #[cfg(target_os = "macos")]
        ModelRuntime::AppleSpeech => crate::apple_speech::AppleSpeech::is_ready(language),
        #[cfg(not(target_os = "macos"))]
        ModelRuntime::AppleSpeech => false,
    }
}

#[cfg(any(target_os = "macos", target_os = "windows"))]
pub fn is_verified(model: &ModelDefinition) -> bool {
    let Ok(path) = model_path(model) else {
        return false;
    };
    verification_receipt_matches(&path, model)
}

#[cfg(any(target_os = "macos", target_os = "windows", test))]
pub fn verify_installed(model: &ModelDefinition, canceled: &AtomicBool) -> Result<PathBuf> {
    check_canceled(canceled)?;
    let path = model_path(model)?;
    verify_installed_at(&path, model, canceled)?;
    Ok(path)
}

#[cfg(any(target_os = "macos", target_os = "windows", test))]
fn verify_installed_at(path: &Path, model: &ModelDefinition, canceled: &AtomicBool) -> Result<()> {
    check_canceled(canceled)?;
    if !path.is_file() {
        bail!(
            "{} is no longer installed. Open Settings to download it.",
            model.name
        );
    }
    let _lock = lock_downloads(path.parent().expect("model directory"), canceled)?;
    if !verification_receipt_matches(path, model) {
        verify_file_with_cancel(path, model, canceled)?;
        write_verification_receipt(path, model)?;
    }
    Ok(())
}

/// Deletes a downloaded model with its verification receipt and any partial
/// download. Refuses instead of waiting while a download owns the directory.
pub fn remove_installed(model: &ModelDefinition) -> Result<()> {
    if !matches!(model.runtime, ModelRuntime::Gguf(_)) {
        bail!("{} is managed by macOS and cannot be deleted", model.name);
    }
    remove_installed_at(&model_path(model)?)
}

fn remove_installed_at(path: &Path) -> Result<()> {
    let directory = path.parent().expect("model directory");
    if !directory.is_dir() {
        return Ok(());
    }
    let lock = File::create(directory.join(".download.lock"))?;
    match lock.try_lock_exclusive() {
        Ok(()) => {}
        Err(error) if error.kind() == ErrorKind::WouldBlock => {
            bail!("A model download is in progress. Try again when it finishes.")
        }
        Err(error) => return Err(error.into()),
    }
    delete_set_aside_in(directory);
    for file in [
        path.to_path_buf(),
        verification_receipt_path(path),
        path.with_extension("gguf.partial"),
    ] {
        let removed = match fs::remove_file(&file) {
            Err(error) if is_mapped(&error) => set_aside(&file),
            removed => removed,
        };
        match removed {
            Ok(()) => {}
            Err(error) if error.kind() == ErrorKind::NotFound => {}
            Err(error) => {
                return Err(error).wrap_err_with(|| format!("could not delete {}", file.display()));
            }
        }
    }
    sync_directory(directory)?;
    Ok(())
}

/// Windows refuses to delete or replace a file while any view of it is
/// mapped, and transcribe.cpp maps the loaded model's weights. The file can
/// still be renamed, so it is set aside and deleted once nothing maps it.
fn is_mapped(error: &std::io::Error) -> bool {
    cfg!(windows) && error.kind() == ErrorKind::PermissionDenied
}

const SET_ASIDE_SUFFIX: &str = ".set-aside";

fn set_aside(file: &Path) -> std::io::Result<()> {
    let nanos = std::time::SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |elapsed| elapsed.as_nanos());
    let mut name = file.file_name().unwrap_or_default().to_os_string();
    name.push(format!(".{nanos}{SET_ASIDE_SUFFIX}"));
    fs::rename(file, file.with_file_name(name))
}

/// Moves `from` over `to`, setting a mapped `to` aside first.
fn replace_file(from: &Path, to: &Path) -> std::io::Result<()> {
    match fs::rename(from, to) {
        Err(error) if is_mapped(&error) && to.exists() => {
            set_aside(to)?;
            fs::rename(from, to)
        }
        renamed => renamed,
    }
}

/// Deletes models set aside while they were loaded; one still mapped stays
/// for a later sweep.
#[cfg(target_os = "windows")]
pub fn delete_set_aside() {
    if let Ok(directory) = models_dir() {
        delete_set_aside_in(&directory);
    }
}

fn delete_set_aside_in(directory: &Path) {
    let Ok(entries) = fs::read_dir(directory) else {
        return;
    };
    for entry in entries.flatten() {
        if entry
            .file_name()
            .to_string_lossy()
            .ends_with(SET_ASIDE_SUFFIX)
        {
            let _ = fs::remove_file(entry.path());
        }
    }
}

fn lock_downloads(directory: &Path, canceled: &AtomicBool) -> Result<File> {
    let lock = File::create(directory.join(".download.lock"))?;
    loop {
        check_canceled(canceled)?;
        match lock.try_lock_exclusive() {
            Ok(()) => return Ok(lock),
            Err(error) if error.kind() == ErrorKind::WouldBlock => {
                thread::sleep(Duration::from_millis(100));
            }
            Err(error) => return Err(error.into()),
        }
    }
}

#[cfg(any(target_os = "macos", target_os = "windows", test))]
fn verification_receipt_matches(path: &Path, model: &ModelDefinition) -> bool {
    let ModelRuntime::Gguf(artifact) = model.runtime else {
        return false;
    };
    artifact_receipt_matches(path, artifact)
}

fn artifact_receipt_matches(path: &Path, artifact: &GgufArtifact) -> bool {
    let Ok(metadata) = fs::metadata(path) else {
        return false;
    };
    let Ok(receipt) = fs::read(verification_receipt_path(path))
        .and_then(|data| serde_json::from_slice::<VerificationReceipt>(&data).map_err(Into::into))
    else {
        return false;
    };
    metadata.len() == artifact.bytes
        && receipt.bytes == artifact.bytes
        && receipt.sha256 == artifact.sha256
        && modified_ns(&metadata).is_some_and(|modified| modified == receipt.modified_ns)
}

pub fn download_with_progress(
    model: &ModelDefinition,
    canceled: &AtomicBool,
    downloaded_bytes: &AtomicU64,
) -> Result<PathBuf> {
    download_with_stage_progress(
        model,
        canceled,
        downloaded_bytes,
        &AtomicU8::new(ModelPreparationStage::Downloading as u8),
    )
}

pub fn download_with_stage_progress(
    model: &ModelDefinition,
    canceled: &AtomicBool,
    downloaded_bytes: &AtomicU64,
    stage: &AtomicU8,
) -> Result<PathBuf> {
    let ModelRuntime::Gguf(artifact) = model.runtime else {
        bail!("{} is installed and managed by macOS", model.name);
    };
    let path = download_artifact(artifact, model.name, canceled, downloaded_bytes, stage)?;
    // In the background, so an installed model activates at once even
    // offline. Until the detector lands, dictation and meetings keep their
    // level-only silence handling.
    if !speech_detector_installed() {
        crate::speech_detector::start_download();
    }
    Ok(path)
}

/// Downloads the speech detector unless a verified copy is installed.
#[cfg_attr(test, allow(dead_code))]
pub fn download_speech_detector(canceled: &AtomicBool) -> Result<PathBuf> {
    download_artifact(
        &SPEECH_DETECTOR_ARTIFACT,
        SPEECH_DETECTOR_NAME,
        canceled,
        &AtomicU64::new(0),
        &AtomicU8::new(ModelPreparationStage::Downloading as u8),
    )
}

pub fn speech_detector_path() -> Result<PathBuf> {
    Ok(models_dir()?.join(SPEECH_DETECTOR_ARTIFACT.filename))
}

/// Whether the speech detector is installed at its pinned size. Its checksum
/// is checked when it downloads.
pub fn speech_detector_installed() -> bool {
    speech_detector_path()
        .and_then(|path| Ok(fs::metadata(path)?.len() == SPEECH_DETECTOR_ARTIFACT.bytes))
        .unwrap_or(false)
}

fn download_artifact(
    artifact: &GgufArtifact,
    name: &str,
    canceled: &AtomicBool,
    downloaded_bytes: &AtomicU64,
    stage: &AtomicU8,
) -> Result<PathBuf> {
    ModelPreparationStage::Downloading.store(stage);
    check_canceled(canceled)?;
    let directory = models_dir()?;
    fs::create_dir_all(&directory)?;
    let _lock = lock_downloads(&directory, canceled)?;
    check_canceled(canceled)?;
    delete_set_aside_in(&directory);
    let destination = directory.join(artifact.filename);
    if destination.exists() {
        if artifact_receipt_matches(&destination, artifact) {
            downloaded_bytes.store(artifact.bytes, Ordering::Relaxed);
            return Ok(destination);
        }
        ModelPreparationStage::Verifying.store(stage);
        match verify_artifact(&destination, artifact, name, canceled) {
            Ok(()) => {
                write_artifact_receipt(&destination, artifact)?;
                downloaded_bytes.store(artifact.bytes, Ordering::Relaxed);
                return Ok(destination);
            }
            Err(error) if canceled.load(Ordering::Relaxed) => return Err(error),
            Err(error) => {
                tracing::warn!(%error, path = %destination.display(), "will replace invalid transcription model after verifying its replacement");
            }
        }
    }
    let partial = destination.with_extension("gguf.partial");
    let mut partial_len = fs::metadata(&partial).ok().map(|metadata| metadata.len());
    if partial_len > Some(artifact.bytes) {
        fs::remove_file(&partial)?;
        partial_len = None;
    }
    downloaded_bytes.store(partial_len.unwrap_or(0), Ordering::Relaxed);
    if partial_len == Some(artifact.bytes) {
        match verify_and_promote_partial(
            &partial,
            &destination,
            &directory,
            artifact,
            name,
            canceled,
            downloaded_bytes,
            stage,
        ) {
            Ok(()) => return Ok(destination),
            Err(error) if canceled.load(Ordering::Relaxed) => return Err(error),
            Err(error) => {
                tracing::warn!(%error, path = %partial.display(), "replacing invalid partial transcription model");
                fs::remove_file(&partial)?;
                downloaded_bytes.store(0, Ordering::Relaxed);
            }
        }
    }
    ModelPreparationStage::Downloading.store(stage);
    let mut child = crate::background_process::command("curl")
        .args([
            "--fail",
            "--location",
            "--retry",
            "3",
            "--connect-timeout",
            "15",
            "--speed-limit",
            "1024",
            "--speed-time",
            "60",
            "--continue-at",
            "-",
            "--silent",
            "--show-error",
            "--output",
        ])
        .arg(&partial)
        .arg(artifact.download_url())
        .stderr(Stdio::piped())
        .spawn()
        .wrap_err("could not start model download")?;
    let status = loop {
        downloaded_bytes.store(
            fs::metadata(&partial).map_or(0, |metadata| metadata.len()),
            Ordering::Relaxed,
        );
        if canceled.load(Ordering::Relaxed) {
            let _ = child.kill();
            let _ = child.wait();
            bail!("model download canceled");
        }
        if let Some(status) = child.try_wait()? {
            break status;
        }
        thread::sleep(Duration::from_millis(100));
    };
    if !status.success() {
        let mut stderr = String::new();
        if let Some(mut pipe) = child.stderr.take() {
            pipe.read_to_string(&mut stderr)?;
        }
        bail!(
            "Could not download the model from huggingface.co or its download CDN. \
             Check your connection, then retry in Settings. On a managed network, \
             ask IT to allow Hugging Face model downloads. \
             The app download uses a separate host. Download progress is kept for retry. \
             Details ({status}): {}",
            stderr.trim()
        );
    }
    check_canceled(canceled)?;
    if let Err(error) = verify_and_promote_partial(
        &partial,
        &destination,
        &directory,
        artifact,
        name,
        canceled,
        downloaded_bytes,
        stage,
    ) {
        if !canceled.load(Ordering::Relaxed) {
            let _ = fs::remove_file(&partial);
        }
        return Err(error);
    }
    Ok(destination)
}

#[allow(clippy::too_many_arguments)]
fn verify_and_promote_partial(
    partial: &Path,
    destination: &Path,
    directory: &Path,
    artifact: &GgufArtifact,
    name: &str,
    canceled: &AtomicBool,
    downloaded_bytes: &AtomicU64,
    stage: &AtomicU8,
) -> Result<()> {
    ModelPreparationStage::Verifying.store(stage);
    verify_artifact(partial, artifact, name, canceled)?;
    sync_file(partial)?;
    replace_file(partial, destination)?;
    sync_directory(directory)?;
    write_artifact_receipt(destination, artifact)?;
    downloaded_bytes.store(artifact.bytes, Ordering::Relaxed);
    Ok(())
}

fn verification_receipt_path(model_path: &Path) -> PathBuf {
    model_path.with_extension("gguf.verified.json")
}

/// The best starting selection for a dictation language: its first
/// recommended runtime model. Unknown languages fall back to English, so an
/// English system starts with Parakeet Unified English.
pub fn recommended_selection(language: &str) -> TranscriptionSelection {
    let language = LANGUAGES
        .iter()
        .find(|(code, _)| *code == language)
        .map_or("en", |(code, _)| *code);
    let choices = choices_for_runtime(language);
    let model = choices
        .iter()
        .find(|choice| choice.recommendation == Recommendation::Recommended)
        .or_else(|| choices.first())
        .map_or(TranscriptionModelId::ParakeetUnifiedEnglish, |choice| {
            choice.model.id
        });
    TranscriptionSelection {
        model,
        language: language.into(),
        recognition_hints: String::new(),
    }
}

pub(crate) fn modified_ns(metadata: &fs::Metadata) -> Option<u64> {
    metadata
        .modified()
        .ok()?
        .duration_since(UNIX_EPOCH)
        .ok()?
        .as_nanos()
        .try_into()
        .ok()
}

/// Publish `value` as JSON at `path` so a crash leaves either the previous
/// file or the complete new one, never a partial write.
/// Flushes a file's data to disk. Windows only flushes through a writable handle.
fn sync_file(path: &Path) -> std::io::Result<()> {
    fs::OpenOptions::new().write(true).open(path)?.sync_all()
}

/// Persists a rename or removal in `directory`. Windows cannot open a
/// directory as a file, and NTFS journals the entry change itself.
pub(crate) fn sync_directory(directory: &Path) -> std::io::Result<()> {
    #[cfg(unix)]
    File::open(directory)?.sync_all()?;
    #[cfg(windows)]
    let _ = directory;
    Ok(())
}

pub(crate) fn write_json_atomically(path: &Path, value: &impl Serialize) -> Result<()> {
    let parent = path
        .parent()
        .ok_or_else(|| color_eyre::eyre::eyre!("{} has no parent", path.display()))?;
    let temporary = path.with_extension("json.tmp");
    fs::write(&temporary, serde_json::to_vec(value)?)?;
    sync_file(&temporary)?;
    fs::rename(temporary, path)?;
    sync_directory(parent)?;
    Ok(())
}

/// Stream `path` through SHA-256 and return the lowercase hex digest,
/// honoring `canceled` between chunks.
pub(crate) fn sha256_hex(path: &Path, canceled: Option<&AtomicBool>) -> Result<String> {
    let mut file = File::open(path)?;
    let mut digest = Sha256::new();
    // Heap-allocated: a 1 MB array overflows Windows' 1 MB main-thread stack.
    let mut buffer = vec![0_u8; 1024 * 1024];
    loop {
        if let Some(canceled) = canceled {
            check_canceled(canceled)?;
        }
        let read = file.read(&mut buffer)?;
        if read == 0 {
            break;
        }
        digest.update(&buffer[..read]);
    }
    Ok(digest
        .finalize()
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect())
}

#[cfg(any(target_os = "macos", target_os = "windows", test))]
fn write_verification_receipt(path: &Path, model: &ModelDefinition) -> Result<()> {
    let ModelRuntime::Gguf(artifact) = model.runtime else {
        bail!("{} has no artifact verification receipt", model.name);
    };
    write_artifact_receipt(path, artifact)
}

fn write_artifact_receipt(path: &Path, artifact: &GgufArtifact) -> Result<()> {
    let metadata = fs::metadata(path)?;
    let receipt = VerificationReceipt {
        bytes: artifact.bytes,
        sha256: artifact.sha256.into(),
        modified_ns: modified_ns(&metadata)
            .ok_or_else(|| color_eyre::eyre::eyre!("model modification time is unavailable"))?,
    };
    write_json_atomically(&verification_receipt_path(path), &receipt)
}

fn check_canceled(canceled: &AtomicBool) -> Result<()> {
    if canceled.load(Ordering::Relaxed) {
        bail!("model download canceled");
    }
    Ok(())
}

#[cfg(test)]
fn verify_file(path: &Path, model: &ModelDefinition) -> Result<()> {
    verify_file_with_cancel(path, model, &AtomicBool::new(false))
}

#[cfg(any(target_os = "macos", target_os = "windows", test))]
fn verify_file_with_cancel(
    path: &Path,
    model: &ModelDefinition,
    canceled: &AtomicBool,
) -> Result<()> {
    let ModelRuntime::Gguf(artifact) = model.runtime else {
        bail!("{} has no artifact to verify", model.name);
    };
    verify_artifact(path, artifact, model.name, canceled)
}

fn verify_artifact(
    path: &Path,
    artifact: &GgufArtifact,
    name: &str,
    canceled: &AtomicBool,
) -> Result<()> {
    let metadata = fs::metadata(path)?;
    if metadata.len() != artifact.bytes {
        bail!(
            "downloaded {} bytes for {}, expected {}",
            metadata.len(),
            name,
            artifact.bytes
        );
    }
    if sha256_hex(path, Some(canceled))? != artifact.sha256 {
        bail!("checksum mismatch for {}", name);
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use std::sync::atomic::{AtomicU64, Ordering};

    use super::*;

    static NEXT_TEMP_FILE: AtomicU64 = AtomicU64::new(0);

    /// A five-byte artifact whose checksum is `sha256("hello")`.
    fn fixture_model() -> ModelDefinition {
        const FIXTURE_ARTIFACT: GgufArtifact = GgufArtifact {
            filename: "fixture.gguf",
            revision: "fixture",
            repository: "fixture",
            bytes: 5,
            sha256: "2cf24dba5fb0a30e26e83b2ac5b9e29e1b161e5c1fa7425e73043362938b9824",
            architecture: "fixture",
            variant: "fixture",
        };
        ModelDefinition {
            runtime: ModelRuntime::Gguf(&FIXTURE_ARTIFACT),
            ..*definition(TranscriptionModelId::ParakeetV2)
        }
    }

    #[test]
    fn recommendations_are_language_specific() {
        let english = choices_for_runtime("en");
        assert_eq!(
            english[0].model.id,
            TranscriptionModelId::ParakeetUnifiedEnglish
        );
        assert_eq!(english[0].recommendation, Recommendation::Recommended);
        assert_eq!(english[1].model.id, TranscriptionModelId::CohereTranscribe);
        assert_eq!(english[1].recommendation, Recommendation::MostAccurate);
        let mandarin = choices_for_runtime("zh");
        assert_eq!(mandarin[0].model.id, TranscriptionModelId::Qwen3Asr06B);
        assert_eq!(mandarin[1].model.id, TranscriptionModelId::SenseVoiceSmall);
    }

    #[test]
    fn every_language_has_exactly_one_recommended_model() {
        for (language, _) in LANGUAGES {
            let recommended: Vec<_> = choices_for_runtime(language)
                .into_iter()
                .filter(|choice| choice.recommendation == Recommendation::Recommended)
                .map(|choice| choice.model.id)
                .collect();
            assert_eq!(recommended.len(), 1, "{language}: {recommended:?}");
            assert_eq!(
                recommended_selection(language).model,
                recommended[0],
                "{language}"
            );
        }
    }

    #[test]
    fn accuracy_is_the_inverse_of_the_word_error_rate() {
        assert_eq!(
            definition(TranscriptionModelId::ParakeetUnifiedEnglish)
                .accuracy_label()
                .as_deref(),
            Some("98.4%")
        );
        assert_eq!(
            definition(TranscriptionModelId::SenseVoiceSmall)
                .accuracy_label()
                .as_deref(),
            Some("89.9%")
        );
        assert_eq!(
            definition(TranscriptionModelId::AppleSpeech).accuracy_label(),
            None
        );
        assert_eq!(accuracy_label(0.0), "100.0%");
        assert_eq!(accuracy_label(112.5), "0.0%");
    }

    #[test]
    fn gguf_artifacts_are_q8_0_for_the_vulkan_shader_subset() {
        // Q8_0 GGUFs hold only f32, f16, and q8_0 tensors, the types the
        // Vulkan backend is built for (`CMAKE_ARGS` in .cargo/config.toml).
        // Another quantization needs its type added there, or Vulkan aborts.
        for model in MODELS {
            if let ModelRuntime::Gguf(artifact) = model.runtime {
                assert!(
                    artifact.filename.ends_with("-Q8_0.gguf"),
                    "{} needs its tensor types in GGML_VULKAN_SHADER_TYPES",
                    artifact.filename
                );
            }
        }
    }

    #[test]
    fn only_the_buffered_parakeet_artifact_is_marked_streaming() {
        // transcribe.cpp advertises streaming for Parakeet's chunked-attention
        // variants; Whisper, Qwen3-ASR, SenseVoice, Cohere, and the offline
        // TDT checkpoints transcribe whole clips.
        let streaming = MODELS
            .iter()
            .filter(|model| model.streams)
            .map(|model| model.id)
            .collect::<Vec<_>>();
        assert_eq!(streaming, [TranscriptionModelId::ParakeetUnifiedEnglish]);
        let ModelRuntime::Gguf(artifact) = definition(streaming[0]).runtime else {
            panic!("a streaming model has a GGUF artifact");
        };
        assert_eq!(artifact.variant, "unified-en-0.6b");
    }

    #[test]
    fn model_ids_have_explicit_stable_wire_names() {
        for (id, name) in [
            (
                TranscriptionModelId::ParakeetUnifiedEnglish,
                "parakeet_unified_en",
            ),
            (TranscriptionModelId::ParakeetV2, "parakeet_v2"),
            (TranscriptionModelId::ParakeetV3, "parakeet_v3"),
            (
                TranscriptionModelId::WhisperLargeV3Turbo,
                "whisper_large_v3_turbo",
            ),
            (TranscriptionModelId::Qwen3Asr06B, "qwen3_asr06_b"),
            (TranscriptionModelId::SenseVoiceSmall, "sense_voice_small"),
            (TranscriptionModelId::CohereTranscribe, "cohere_transcribe"),
            (TranscriptionModelId::AppleSpeech, "apple_speech"),
        ] {
            assert_eq!(id.as_str(), name);
            assert_eq!(name.parse::<TranscriptionModelId>().unwrap(), id);
            assert_eq!(serde_json::to_value(id).unwrap(), name);
        }
        assert!("unknown".parse::<TranscriptionModelId>().is_err());
    }

    #[test]
    fn apple_speech_is_retained_only_for_persisted_selection_migration() {
        assert!(
            choices_for_runtime("en")
                .iter()
                .all(|choice| choice.model.id != TranscriptionModelId::AppleSpeech)
        );
        let apple_speech = definition(TranscriptionModelId::AppleSpeech);
        assert!(matches!(apple_speech.runtime, ModelRuntime::AppleSpeech));
        assert!(!apple_speech.available());
        assert!(
            validate(&TranscriptionSelection {
                model: TranscriptionModelId::AppleSpeech,
                ..TranscriptionSelection::default()
            })
            .is_err()
        );
        assert_eq!(apple_speech.download_bytes(), None);
        assert!(model_path(apple_speech).is_err());
    }

    #[test]
    fn selections_require_model_language_compatibility() {
        assert!(validate(&TranscriptionSelection::default()).is_ok());
        assert!(
            validate(&TranscriptionSelection {
                model: TranscriptionModelId::ParakeetV2,
                language: "zh".into(),
                recognition_hints: String::new(),
            })
            .is_err()
        );
        assert!(
            validate(&TranscriptionSelection {
                model: TranscriptionModelId::ParakeetV2,
                language: AUTO_LANGUAGE.into(),
                recognition_hints: String::new(),
            })
            .is_err()
        );
        assert!(
            validate(&TranscriptionSelection {
                model: TranscriptionModelId::WhisperLargeV3Turbo,
                language: AUTO_LANGUAGE.into(),
                recognition_hints: String::new(),
            })
            .is_ok()
        );
    }

    #[test]
    fn automatic_language_choices_only_include_detection_models() {
        let choices = choices_for_runtime(AUTO_LANGUAGE);

        assert!(!choices.is_empty());
        assert!(
            choices
                .iter()
                .all(|choice| choice.model.supports_language_detection)
        );
        assert!(
            choices
                .iter()
                .any(|choice| choice.model.id == TranscriptionModelId::Qwen3Asr06B)
        );
        assert!(
            choices
                .iter()
                .all(|choice| choice.model.id != TranscriptionModelId::SenseVoiceSmall)
        );
    }

    fn routing(language: &str) -> Vec<(TranscriptionModelId, Recommendation)> {
        choices_for_runtime(language)
            .into_iter()
            .map(|choice| (choice.model.id, choice.recommendation))
            .collect()
    }

    #[test]
    fn weak_qwen_and_parakeet_languages_route_to_stronger_shipped_models() {
        use Recommendation::{Fastest, MostAccurate, RecognitionHints, Recommended};
        use TranscriptionModelId::{
            CohereTranscribe, ParakeetV3, Qwen3Asr06B, SenseVoiceSmall, WhisperLargeV3Turbo,
        };
        assert_eq!(
            routing("el"),
            [
                (CohereTranscribe, Recommended),
                (WhisperLargeV3Turbo, RecognitionHints)
            ]
        );
        for language in ["tr", "ms", "fil", "fa", "mk"] {
            assert_eq!(routing(language), [(WhisperLargeV3Turbo, Recommended)]);
        }
        assert_eq!(
            routing("ar"),
            [
                (WhisperLargeV3Turbo, Recommended),
                (CohereTranscribe, MostAccurate)
            ]
        );
        for language in ["fi", "sv", "da"] {
            assert_eq!(
                routing(language),
                [(WhisperLargeV3Turbo, Recommended), (ParakeetV3, Fastest)]
            );
        }
        assert_eq!(
            routing("ja"),
            [
                (WhisperLargeV3Turbo, Recommended),
                (SenseVoiceSmall, Fastest)
            ]
        );
        assert_eq!(
            routing(AUTO_LANGUAGE),
            [(WhisperLargeV3Turbo, Recommended), (Qwen3Asr06B, Fastest)]
        );
        for language in ["yue", "ko"] {
            assert_eq!(
                routing(language),
                [
                    (Qwen3Asr06B, Recommended),
                    (WhisperLargeV3Turbo, RecognitionHints)
                ]
            );
        }
        assert_eq!(
            routing("vi"),
            [
                (Qwen3Asr06B, Recommended),
                (CohereTranscribe, MostAccurate),
                (WhisperLargeV3Turbo, RecognitionHints)
            ]
        );
        assert_eq!(routing("mt"), [(ParakeetV3, Recommended)]);
        for language in ["hi", "th", "id", "zh"] {
            assert_eq!(routing(language)[0], (Qwen3Asr06B, Recommended));
        }
        assert_eq!(routing("de")[0], (ParakeetV3, Recommended));
    }

    #[test]
    fn every_language_choice_supports_its_language_and_first_run_picks_it() {
        for (language, _) in LANGUAGES {
            let choices = choices_for_runtime(language);
            assert!(!choices.is_empty(), "{language}");
            for choice in &choices {
                assert!(choice.model.available(), "{language}");
                assert!(choice.model.supports_language(language), "{language}");
            }
            let selection = recommended_selection(language);
            assert!(validate(&selection).is_ok(), "{language}");
        }
        assert_eq!(
            recommended_selection("el").model,
            TranscriptionModelId::CohereTranscribe
        );
        assert_eq!(
            recommended_selection("tr").model,
            TranscriptionModelId::WhisperLargeV3Turbo
        );
    }

    #[test]
    fn the_picker_lists_every_model_with_the_language_s_recommendations_first() {
        let ids = |language| {
            picker_models(language)
                .into_iter()
                .map(|(model, recommendation)| (model.id, recommendation))
                .collect::<Vec<_>>()
        };
        let available = MODELS.iter().filter(|model| model.available()).count();
        for (language, _) in LANGUAGES {
            assert_eq!(ids(language).len(), available, "{language}");
        }
        let greek = ids("el");
        assert_eq!(
            greek[..2],
            [
                (
                    TranscriptionModelId::CohereTranscribe,
                    Some(Recommendation::Recommended)
                ),
                (
                    TranscriptionModelId::WhisperLargeV3Turbo,
                    Some(Recommendation::RecognitionHints)
                ),
            ]
        );
        // Then Parakeet v3, which supports Greek, before every model that
        // would change the language.
        assert_eq!(greek[2], (TranscriptionModelId::ParakeetV3, None));
        let supports = greek
            .iter()
            .map(|(id, _)| definition(*id).supports_language("el"))
            .collect::<Vec<_>>();
        assert!(supports.windows(2).all(|pair| pair[0] || !pair[1]));
        assert!(!supports[supports.len() - 1]);
    }

    #[test]
    fn a_model_switch_keeps_a_supported_language_or_falls_back_to_the_model_s_default() {
        let unified = definition(TranscriptionModelId::ParakeetUnifiedEnglish);
        let whisper = definition(TranscriptionModelId::WhisperLargeV3Turbo);
        let qwen = definition(TranscriptionModelId::Qwen3Asr06B);
        let cohere = definition(TranscriptionModelId::CohereTranscribe);
        assert_eq!(unified.language_for("ja"), "en");
        assert_eq!(unified.language_for("auto"), "en");
        assert_eq!(whisper.language_for("ja"), "ja");
        assert_eq!(whisper.language_for("auto"), "auto");
        assert_eq!(qwen.language_for("auto"), "auto");
        assert_eq!(cohere.language_for("auto"), "en");
        for model in MODELS.iter().filter(|model| model.available()) {
            for (language, _) in LANGUAGES {
                let selection = TranscriptionSelection {
                    model: model.id,
                    language: model.language_for(language).into(),
                    recognition_hints: String::new(),
                };
                assert!(validate(&selection).is_ok(), "{} {language}", model.name);
            }
        }
    }

    #[test]
    fn each_model_offers_exactly_the_languages_it_supports() {
        let unified = definition(TranscriptionModelId::ParakeetUnifiedEnglish);
        assert_eq!(unified.language_choices(), ["en"]);
        let whisper = definition(TranscriptionModelId::WhisperLargeV3Turbo);
        assert_eq!(whisper.language_choices().len(), LANGUAGES.len());
        assert_eq!(whisper.language_choices()[0], AUTO_LANGUAGE);
        let sensevoice = definition(TranscriptionModelId::SenseVoiceSmall);
        assert_eq!(
            sensevoice.language_choices(),
            ["auto", "en", "zh", "yue", "ja", "ko"]
        );
        for model in MODELS.iter().filter(|model| model.available()) {
            assert!(model.language_choices().contains(&model.default_language()));
            let declared = model.languages.iter().filter(|code| **code != "*");
            for code in declared {
                assert!(
                    model.language_choices().contains(code),
                    "{} lists {code} outside LANGUAGES",
                    model.name
                );
            }
        }
    }

    #[test]
    fn runtime_language_arguments_distinguish_detection_from_hints() {
        let whisper = definition(TranscriptionModelId::WhisperLargeV3Turbo);
        let qwen = definition(TranscriptionModelId::Qwen3Asr06B);

        assert_eq!(whisper.runtime_language_hint(AUTO_LANGUAGE), None);
        assert_eq!(whisper.runtime_language_hint("fil"), Some("tl"));
        assert_eq!(qwen.runtime_language_hint(AUTO_LANGUAGE), None);
        assert_eq!(qwen.runtime_language_hint("zh"), Some("zh"));
    }

    #[test]
    fn parakeet_v3_supports_portuguese_without_language_conditioning() {
        let selection = TranscriptionSelection {
            model: TranscriptionModelId::ParakeetV3,
            language: "pt".into(),
            recognition_hints: String::new(),
        };
        let model = validate(&selection).unwrap();
        assert_eq!(model.id, TranscriptionModelId::ParakeetV3);
        assert!(
            choices_for_runtime("pt")
                .iter()
                .any(|choice| choice.model.id == model.id)
        );
        assert_eq!(model.runtime_language_hint(&selection.language), None);
        assert_eq!(
            definition(TranscriptionModelId::WhisperLargeV3Turbo)
                .runtime_language_hint(&selection.language),
            Some("pt")
        );
        assert!(
            !choices_for_runtime(AUTO_LANGUAGE)
                .iter()
                .any(|choice| choice.model.id == model.id)
        );
    }

    #[test]
    fn bangla_is_transcribed_by_whisper_with_its_language_hint() {
        let selection = recommended_selection("bn");
        assert_eq!(selection.model, TranscriptionModelId::WhisperLargeV3Turbo);
        assert_eq!(selection.language, "bn");
        let model = validate(&selection).unwrap();
        assert_eq!(model.runtime_language_hint("bn"), Some("bn"));
        let choices = choices_for_runtime("bn");
        assert_eq!(choices.len(), 1);
        assert_eq!(choices[0].recommendation, Recommendation::Recommended);
        assert!(
            validate(&TranscriptionSelection {
                model: TranscriptionModelId::Qwen3Asr06B,
                language: "bn".into(),
                recognition_hints: String::new(),
            })
            .is_err()
        );
    }

    #[test]
    fn recognition_hints_are_whisper_only() {
        let mut selection = TranscriptionSelection {
            recognition_hints: "OpenCode".into(),
            ..TranscriptionSelection::default()
        };
        assert!(validate(&selection).is_err());
        selection.model = TranscriptionModelId::WhisperLargeV3Turbo;
        assert!(validate(&selection).is_ok());
    }

    #[test]
    fn artifact_verification_checks_size_and_checksum() {
        let model = fixture_model();
        let path = std::env::temp_dir().join(format!(
            "hex-model-verification-{}-{}",
            std::process::id(),
            NEXT_TEMP_FILE.fetch_add(1, Ordering::Relaxed)
        ));
        fs::write(&path, b"hello").unwrap();
        assert!(verify_file(&path, &model).is_ok());
        fs::write(&path, b"world").unwrap();
        assert!(verify_file(&path, &model).is_err());
        let _ = fs::remove_file(path);
    }

    #[test]
    fn speech_detector_artifact_downloads_from_its_pinned_revision() {
        assert_eq!(
            SPEECH_DETECTOR_ARTIFACT.download_url(),
            "https://huggingface.co/handy-computer/silero-vad-v6.2-gguf/resolve/\
             a9b2d61c20ae4ddf3644edb34969e8185a485448/silero-vad-v6.2-F32.gguf"
        );
        assert_eq!(SPEECH_DETECTOR_ARTIFACT.bytes, 1_241_056);
    }

    #[test]
    fn artifact_verification_names_the_artifact_it_rejects() {
        let ModelRuntime::Gguf(artifact) = fixture_model().runtime else {
            unreachable!("the fixture is a GGUF model");
        };
        let path = std::env::temp_dir().join(format!(
            "hex-artifact-verification-{}-{}",
            std::process::id(),
            NEXT_TEMP_FILE.fetch_add(1, Ordering::Relaxed)
        ));
        let canceled = AtomicBool::new(false);
        fs::write(&path, b"hello!").unwrap();
        let error = verify_artifact(&path, artifact, "the fixture", &canceled).unwrap_err();
        assert!(error.to_string().contains("for the fixture, expected 5"));
        fs::write(&path, b"world").unwrap();
        let error = verify_artifact(&path, artifact, "the fixture", &canceled).unwrap_err();
        assert_eq!(error.to_string(), "checksum mismatch for the fixture");
        fs::write(&path, b"hello").unwrap();
        assert!(verify_artifact(&path, artifact, "the fixture", &canceled).is_ok());
        let _ = fs::remove_file(path);
    }

    #[test]
    fn verification_receipt_is_invalidated_when_the_artifact_changes() {
        let model = fixture_model();
        let path = std::env::temp_dir().join(format!(
            "hex-model-receipt-{}-{}",
            std::process::id(),
            NEXT_TEMP_FILE.fetch_add(1, Ordering::Relaxed)
        ));
        fs::write(&path, b"hello").unwrap();
        write_verification_receipt(&path, &model).unwrap();
        assert!(verification_receipt_matches(&path, &model));
        thread::sleep(Duration::from_millis(2));
        fs::write(&path, b"world").unwrap();
        assert!(!verification_receipt_matches(&path, &model));
        let _ = fs::remove_file(verification_receipt_path(&path));
        let _ = fs::remove_file(path);
    }

    #[test]
    fn an_installed_model_does_not_wait_for_the_speech_detector() {
        let directory = std::env::temp_dir().join(format!(
            "hex-installed-download-{}-{}",
            std::process::id(),
            NEXT_TEMP_FILE.fetch_add(1, Ordering::Relaxed),
        ));
        fs::create_dir(&directory).unwrap();
        fs::write(directory.join("fixture.gguf"), b"hello").unwrap();
        TEST_MODELS_DIR.with(|models| *models.borrow_mut() = Some(directory.clone()));
        let requests = crate::speech_detector::download_requests_for_test();
        let downloaded = download_with_progress(
            &fixture_model(),
            &AtomicBool::new(false),
            &AtomicU64::new(0),
        );
        TEST_MODELS_DIR.with(|models| *models.borrow_mut() = None);
        let detector_touched = directory.join(SPEECH_DETECTOR_ARTIFACT.filename).exists()
            || directory
                .join(SPEECH_DETECTOR_ARTIFACT.filename)
                .with_extension("gguf.partial")
                .exists();
        fs::remove_dir_all(&directory).unwrap();
        assert_eq!(downloaded.unwrap(), directory.join("fixture.gguf"));
        assert_eq!(
            crate::speech_detector::download_requests_for_test(),
            requests + 1
        );
        assert!(!detector_touched);
    }

    #[test]
    fn canceled_downloads_stop_before_io() {
        let canceled = AtomicBool::new(true);
        assert!(check_canceled(&canceled).is_err());
    }

    #[test]
    fn installed_only_verification_never_downloads_or_replaces_an_invalid_model() {
        let model = fixture_model();
        let directory = std::env::temp_dir().join(format!(
            "hex-installed-model-{}-{}",
            std::process::id(),
            NEXT_TEMP_FILE.fetch_add(1, Ordering::Relaxed),
        ));
        fs::create_dir(&directory).unwrap();
        let path = directory.join("fixture.gguf");
        let canceled = AtomicBool::new(false);
        assert!(verify_installed_at(&path, &model, &canceled).is_err());
        assert!(!path.exists());
        fs::write(&path, b"hello").unwrap();
        verify_installed_at(&path, &model, &canceled).unwrap();
        assert!(verification_receipt_matches(&path, &model));
        thread::sleep(Duration::from_millis(2));
        fs::write(&path, b"world").unwrap();
        assert!(verify_installed_at(&path, &model, &canceled).is_err());
        assert_eq!(fs::read(&path).unwrap(), b"world");
        assert!(!path.with_extension("gguf.partial").exists());
        canceled.store(true, Ordering::Relaxed);
        assert!(verify_installed_at(&path, &model, &canceled).is_err());
        fs::remove_file(verification_receipt_path(&path)).unwrap();
        fs::remove_file(path).unwrap();
        fs::remove_file(directory.join(".download.lock")).unwrap();
        fs::remove_dir(directory).unwrap();
    }

    #[test]
    #[cfg_attr(
        windows,
        ignore = "Windows uses uninstall; download locks are mandatory there"
    )]
    fn removing_a_model_deletes_its_artifacts_unless_a_download_is_active() {
        let directory = std::env::temp_dir().join(format!(
            "hex-remove-model-{}-{}",
            std::process::id(),
            NEXT_TEMP_FILE.fetch_add(1, Ordering::Relaxed),
        ));
        fs::create_dir(&directory).unwrap();
        let path = directory.join("fixture.gguf");
        let artifacts = [
            path.clone(),
            verification_receipt_path(&path),
            path.with_extension("gguf.partial"),
        ];
        for artifact in &artifacts {
            fs::write(artifact, b"model").unwrap();
        }
        let download = lock_downloads(&directory, &AtomicBool::new(false)).unwrap();
        let error = remove_installed_at(&path).unwrap_err().to_string();
        assert!(error.contains("download is in progress"), "{error}");
        assert!(artifacts.iter().all(|artifact| artifact.exists()));
        drop(download);

        // A child another test thread forks before it execs holds a copy of the
        // download lock's descriptor for a moment, so the release can take a few tries.
        let deadline = std::time::Instant::now() + Duration::from_secs(5);
        while let Err(error) = remove_installed_at(&path) {
            assert!(std::time::Instant::now() < deadline, "{error:#}");
            std::thread::sleep(Duration::from_millis(10));
        }
        assert!(artifacts.iter().all(|artifact| !artifact.exists()));
        remove_installed_at(&path).unwrap();
        remove_installed_at(&directory.join("missing").join("fixture.gguf")).unwrap();
        fs::remove_dir_all(directory).unwrap();
    }

    fn set_aside_count(directory: &Path) -> usize {
        fs::read_dir(directory)
            .unwrap()
            .filter(|entry| {
                entry
                    .as_ref()
                    .unwrap()
                    .file_name()
                    .to_string_lossy()
                    .ends_with(SET_ASIDE_SUFFIX)
            })
            .count()
    }

    #[test]
    fn set_aside_models_are_swept_and_replacing_keeps_the_new_file() {
        let directory = std::env::temp_dir().join(format!(
            "hex-set-aside-{}-{}",
            std::process::id(),
            NEXT_TEMP_FILE.fetch_add(1, Ordering::Relaxed),
        ));
        fs::create_dir(&directory).unwrap();
        let path = directory.join("fixture.gguf");
        let other = directory.join("other.gguf");
        fs::write(&path, b"old").unwrap();
        fs::write(&other, b"other").unwrap();
        set_aside(&path).unwrap();
        assert!(!path.exists());
        assert_eq!(set_aside_count(&directory), 1);
        delete_set_aside_in(&directory);
        assert_eq!(set_aside_count(&directory), 0);
        assert!(other.exists());

        let partial = path.with_extension("gguf.partial");
        fs::write(&path, b"old").unwrap();
        fs::write(&partial, b"new").unwrap();
        replace_file(&partial, &path).unwrap();
        assert_eq!(fs::read(&path).unwrap(), b"new");
        assert!(!partial.exists());
        fs::remove_dir_all(directory).unwrap();
    }

    /// The loaded model's weights are memory-mapped; Windows still lets the
    /// picker delete it and a download replace it.
    #[cfg(windows)]
    #[test]
    fn a_mapped_model_can_be_deleted_or_replaced_and_is_swept_once_unmapped() {
        use std::os::windows::io::AsRawHandle;
        use windows_sys::Win32::Foundation::{CloseHandle, HANDLE};
        use windows_sys::Win32::System::Memory::{
            CreateFileMappingW, FILE_MAP_READ, MEMORY_MAPPED_VIEW_ADDRESS, MapViewOfFile,
            PAGE_READONLY, UnmapViewOfFile,
        };
        fn map(path: &Path) -> (HANDLE, MEMORY_MAPPED_VIEW_ADDRESS) {
            let file = File::open(path).unwrap();
            // SAFETY: `file` is open for reading; the mapping keeps its own
            // reference to the file after `file` closes.
            let mapping = unsafe {
                CreateFileMappingW(
                    file.as_raw_handle(),
                    std::ptr::null(),
                    PAGE_READONLY,
                    0,
                    0,
                    std::ptr::null(),
                )
            };
            assert!(!mapping.is_null());
            // SAFETY: `mapping` is a valid read-only file mapping.
            let view = unsafe { MapViewOfFile(mapping, FILE_MAP_READ, 0, 0, 0) };
            assert!(!view.Value.is_null());
            (mapping, view)
        }
        let directory = std::env::temp_dir().join(format!(
            "hex-mapped-model-{}-{}",
            std::process::id(),
            NEXT_TEMP_FILE.fetch_add(1, Ordering::Relaxed),
        ));
        fs::create_dir(&directory).unwrap();
        let path = directory.join("fixture.gguf");
        fs::write(&path, b"model").unwrap();
        let deleted = map(&path);
        remove_installed_at(&path).unwrap();
        assert!(!path.exists());

        fs::write(&path, b"model").unwrap();
        let replaced = map(&path);
        let partial = path.with_extension("gguf.partial");
        fs::write(&partial, b"fresh").unwrap();
        replace_file(&partial, &path).unwrap();
        assert_eq!(fs::read(&path).unwrap(), b"fresh");

        for (mapping, view) in [deleted, replaced] {
            // SAFETY: each view and mapping came from `map` and is released once.
            unsafe {
                UnmapViewOfFile(view);
                CloseHandle(mapping);
            }
        }
        delete_set_aside_in(&directory);
        assert_eq!(set_aside_count(&directory), 0);
        assert_eq!(fs::read(&path).unwrap(), b"fresh");
        fs::remove_dir_all(directory).unwrap();
    }

    #[test]
    fn cohere_chunks_at_reference_window_not_encoder_capacity() {
        assert_eq!(
            max_audio_chunk_samples("cohere_asr", 400_000),
            Some(560_000)
        );
        assert_eq!(max_audio_chunk_samples("cohere_asr", 0), Some(560_000));
        assert_eq!(max_audio_chunk_samples("cohere_asr", 10_000), Some(160_000));
        assert_eq!(max_audio_chunk_samples("whisper", 0), None);
        assert_eq!(max_audio_chunk_samples("parakeet", 60_000), Some(960_000));
    }
}
