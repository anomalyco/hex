//! The catalog's model definitions and the handle readers hold.
//!
//! A catalog entry is a compile-time ModelDefinition in MODELS; an imported
//! or found GGUF gets an owned definition built from its manifest
//! (custom_models). ModelRef carries either, so a reader that took a snapshot
//! keeps it valid after the import is renamed or removed, and the definition
//! drops once the last reader does.

use std::borrow::Cow;
use std::ops::Deref;
use std::sync::Arc;

use super::artifacts::{
    COHERE_ARTIFACT, COHERE_LANGUAGES, FLUID_SPEECH_PRO_ARTIFACT, GIGAAM_V3_E2E_RNNT_ARTIFACT,
    GRANITE_SPEECH_5_ARTIFACT, GRANITE_SPEECH_5_LANGUAGES, IVRIT_WHISPER_ARTIFACT,
    KB_WHISPER_MEDIUM_ARTIFACT, NB_WHISPER_MEDIUM_ARTIFACT, NEMOTRON_3_5_ASR_ARTIFACT,
    NEMOTRON_3_5_LANGUAGES, NEMOTRON_3_5_LOCALES, ORUKEET_ARTIFACT, PARAKEET_ULTRA_ARTIFACT,
    PARAKEET_UNIFIED_ENGLISH_ARTIFACT, PARAKEET_V2_ARTIFACT, PARAKEET_V3_ARTIFACT,
    PARAKEET_V3_LANGUAGES, QWEN_ARTIFACT, QWEN_LANGUAGES, QWENCLEO_ARTIFACT, SENSEVOICE_ARTIFACT,
    UNDERDOG_BARK_3_CLEAN_ARTIFACT, UNDERDOG_BARK_3_PRO_ARTIFACT, WHISPER_ARTIFACT,
};
use super::languages::LANGUAGES;
use super::models::TranscriptionModelId;

/// A model definition: the catalog's, or one built from an import's manifest.
///
/// Catalog entries keep every string 'static. An import owns the strings its
/// manifest supplies (its name, the file it uses, its digest, and its
/// variant), so a rename or an in-place change builds a new definition and
/// the old one drops with its last reader.
#[derive(Clone)]
pub struct ModelDefinition {
    pub id: TranscriptionModelId,
    pub name: Cow<'static, str>,
    pub realtime: &'static str,
    /// Word error rate on the model's own benchmark, in percent;
    /// `None` when the system manages the model and publishes none.
    pub word_error_rate: Option<f64>,
    /// The pinned artifact advertises streaming, so dictation can transcribe
    /// while the shortcut is held. It still streams only on a GPU device and
    /// after its load probe succeeds; otherwise it transcribes the whole clip.
    pub streams: bool,
    pub runtime: ModelRuntime,
    pub languages: Cow<'static, [&'static str]>,
    pub accepts_language_hint: bool,
    pub supports_language_detection: bool,
}

/// One model definition, borrowed from the catalog or shared with imports.
///
/// Readers hold this rather than a &'static ModelDefinition: a rename,
/// remove, or in-place change replaces the registry's Arc, and the old
/// definition stays valid for whoever still holds it, then is freed.
#[derive(Clone)]
pub enum ModelRef {
    Static(&'static ModelDefinition),
    Custom(Arc<ModelDefinition>),
}

impl Deref for ModelRef {
    type Target = ModelDefinition;

    fn deref(&self) -> &ModelDefinition {
        match self {
            Self::Static(definition) => definition,
            Self::Custom(definition) => definition,
        }
    }
}

/// How a model's transcription takes the vocabulary.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum VocabularyUse {
    /// Parakeet's transducer heads bias their decode toward the terms
    /// (Labs › Vocabulary help).
    Bias,
    /// Whisper and Qwen3-ASR read the terms as their context prompt.
    Prompt,
    None,
}

#[derive(Clone)]
pub enum ModelRuntime {
    /// A GGUF file: the catalog's pinned artifact, or an import's own.
    Gguf(Cow<'static, GgufArtifact>),
    AppleSpeech,
}

impl ModelRuntime {
    pub fn is_gguf(&self) -> bool {
        matches!(self, Self::Gguf(_))
    }
}

#[derive(Clone)]
pub struct GgufArtifact {
    /// The file name in its repository, which every download URL uses.
    pub filename: Cow<'static, str>,
    /// The name Hex stores it under in its one flat models folder, when that
    /// must differ from `filename`. `None` keeps `filename`. Only a collision
    /// needs one: KBLab's and NbAiLab's Whisper medium files are both called
    /// `ggml-model-q5_0.bin` and are both 539,212,484 bytes, and the folder's
    /// installed check compares size alone.
    pub stored_as: Option<&'static str>,
    pub revision: &'static str,
    pub repository: &'static str,
    pub bytes: u64,
    pub sha256: Cow<'static, str>,
    /// Every import is checked against `custom_models::ARCHITECTURES` when it
    /// is added, so it is always one of the catalog's names, or "unknown" for
    /// a hand-edited manifest.
    pub architecture: &'static str,
    pub variant: Cow<'static, str>,
}

impl GgufArtifact {
    pub(super) fn download_url(&self) -> String {
        format!(
            "https://huggingface.co/{}/resolve/{}/{}",
            self.repository, self.revision, self.filename
        )
    }

    /// The name Hex's models folder holds this artifact under.
    pub fn local_name(&self) -> &str {
        self.stored_as.unwrap_or(&self.filename)
    }
}

impl ModelDefinition {
    /// A catalog GGUF model, or an import that is on, present, and unchanged.
    pub fn available(&self) -> bool {
        self.available_where(crate::labs::k_quant_models())
    }

    /// [`available`] against `k_quants` rather than this process's Labs
    /// switch, so a test can run either side of Labs › Smaller model files.
    pub(crate) fn available_where(&self, k_quants: bool) -> bool {
        match (&self.runtime, self.id) {
            (ModelRuntime::AppleSpeech, _) => false,
            (ModelRuntime::Gguf(_), TranscriptionModelId::Custom(id)) => {
                super::custom_models::usable(id)
                    && (!super::custom_models::uses_k_quants(id) || k_quants)
            }
            (ModelRuntime::Gguf(_), TranscriptionModelId::ParakeetUltra) => {
                crate::labs::parakeet_ultra()
            }
            (ModelRuntime::Gguf(_), _) => true,
        }
    }

    pub fn supports_language(&self, language: &str) -> bool {
        if language == AUTO_LANGUAGE {
            self.supports_language_detection
        } else {
            self.languages.contains(&"*") || self.languages.contains(&language)
        }
    }

    pub fn runtime_language<'a>(&self, language: &'a str) -> &'a str {
        match self.id {
            TranscriptionModelId::WhisperLargeV3Turbo if language == "fil" => "tl",
            // Its prompt and advertised languages are locales: `de-DE`.
            TranscriptionModelId::Nemotron35Asr => NEMOTRON_3_5_LOCALES
                .iter()
                .find_map(|(code, locale)| (*code == language).then_some(*locale))
                .unwrap_or(language),
            _ => language,
        }
    }

    pub fn runtime_language_hint<'a>(&self, language: &'a str) -> Option<&'a str> {
        (language != AUTO_LANGUAGE && self.accepts_language_hint)
            .then(|| self.runtime_language(language))
    }

    /// The languages Settings offers for this model, in [`LANGUAGES`] order,
    /// Auto first when the model detects the language, then the other
    /// languages the model declares (an import's Norwegian, #1148).
    pub fn language_choices(&self) -> Vec<&'static str> {
        LANGUAGES
            .iter()
            .map(|(code, _)| *code)
            .filter(|code| self.supports_language(code))
            .chain(
                self.languages
                    .iter()
                    .copied()
                    .filter(|code| *code != "*" && !super::languages::is_listed(code)),
            )
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

    pub fn presentation_languages(&self) -> Vec<&'static str> {
        self.languages
            .iter()
            .copied()
            .chain(self.supports_language_detection.then_some(AUTO_LANGUAGE))
            .collect()
    }

    pub fn size_label(&self) -> String {
        match &self.runtime {
            ModelRuntime::AppleSpeech => "Managed".into(),
            ModelRuntime::Gguf(artifact) if artifact.bytes >= 1_000_000_000 => {
                format!("{:.1} GB", artifact.bytes as f64 / 1_000_000_000.0)
            }
            ModelRuntime::Gguf(artifact) => format!("{} MB", artifact.bytes / 1_000_000),
        }
    }

    /// How transcription feeds this model the vocabulary (`gguf_session`
    /// checks the model itself before it adds the list to a run). Rewriting
    /// and search use the vocabulary with every model.
    pub fn vocabulary_use(&self) -> VocabularyUse {
        match &self.runtime {
            ModelRuntime::Gguf(artifact) => {
                super::readiness::vocabulary_use_for_architecture(artifact.architecture)
            }
            ModelRuntime::AppleSpeech => VocabularyUse::None,
        }
    }

    /// How many of `entries`, first in list order, this model's
    /// transcription reads (`dictation::vocabulary::terms_read`).
    pub fn vocabulary_terms_read(
        &self,
        entries: &[crate::dictation::vocabulary::VocabularyEntry],
    ) -> usize {
        match &self.runtime {
            ModelRuntime::Gguf(artifact) => {
                crate::dictation::vocabulary::terms_read(entries, artifact.architecture)
            }
            _ => 0,
        }
    }

    pub fn uses_vocabulary(&self) -> bool {
        self.vocabulary_use() != VocabularyUse::None
    }

    pub fn download_bytes(&self) -> Option<u64> {
        match &self.runtime {
            ModelRuntime::Gguf(artifact) => Some(artifact.bytes),
            ModelRuntime::AppleSpeech => None,
        }
    }
}

pub const AUTO_LANGUAGE: &str = "auto";

/// The catalog's one shared Cow::Borrowed constructors, so its entries stay
/// compile-time constants.
const fn text(value: &'static str) -> Cow<'static, str> {
    Cow::Borrowed(value)
}

const fn languages(value: &'static [&'static str]) -> Cow<'static, [&'static str]> {
    Cow::Borrowed(value)
}

const fn gguf(artifact: &'static GgufArtifact) -> ModelRuntime {
    ModelRuntime::Gguf(Cow::Borrowed(artifact))
}

pub const MODELS: &[ModelDefinition] = &[
    ModelDefinition {
        id: TranscriptionModelId::ParakeetUnifiedEnglish,
        name: text("Parakeet Unified English"),
        realtime: "~163x",
        word_error_rate: Some(1.60),
        streams: true,
        runtime: gguf(&PARAKEET_UNIFIED_ENGLISH_ARTIFACT),
        languages: languages(&["en"]),
        accepts_language_hint: true,
        supports_language_detection: false,
    },
    ModelDefinition {
        id: TranscriptionModelId::ParakeetV2,
        name: text("Parakeet v2"),
        realtime: "~55x",
        word_error_rate: Some(1.69),
        streams: false,
        runtime: gguf(&PARAKEET_V2_ARTIFACT),
        languages: languages(&["en"]),
        accepts_language_hint: true,
        supports_language_detection: false,
    },
    ModelDefinition {
        id: TranscriptionModelId::ParakeetV3,
        name: text("Parakeet v3"),
        realtime: "~33x",
        word_error_rate: Some(1.94),
        streams: false,
        runtime: gguf(&PARAKEET_V3_ARTIFACT),
        languages: languages(PARAKEET_V3_LANGUAGES),
        // The pinned v3 artifact detects the language from audio and has no
        // prompt input, so it is offered under Auto.
        accepts_language_hint: false,
        supports_language_detection: true,
    },
    ModelDefinition {
        id: TranscriptionModelId::Orukeet,
        name: text("Orukeet"),
        // The card says it runs as fast as Parakeet v3, and transcribe.cpp's
        // catalog puts its Q8_0 file beside v3's on the same machines, so it
        // keeps v3's figure.
        realtime: "~33x",
        // LibriSpeech test-clean, transcribe.cpp's published conversion
        // benchmark (`catalog/orukeet.json`); not measured in Hex.
        word_error_rate: Some(1.86),
        streams: false,
        runtime: gguf(&ORUKEET_ARTIFACT),
        languages: languages(PARAKEET_V3_LANGUAGES),
        // Like v3, its base: it detects the language from the audio and has
        // no language input, so it is offered under Auto.
        accepts_language_hint: false,
        supports_language_detection: true,
    },
    ModelDefinition {
        id: TranscriptionModelId::FluidSpeechPro,
        name: text("Fluid Speech Pro"),
        // The same FastConformer-TDT 0.6B as Parakeet v3, plus one small
        // pre-Conformer block, so it keeps v3's figure. The card's ~3,700x is
        // one GPU's throughput, not this single-clip figure.
        realtime: "~33x",
        // The card publishes Open ASR Leaderboard and FLEURS figures, never a
        // LibriSpeech test-clean one, so the picker's single comparable number
        // stays unset; its per-language rates are in its details.
        word_error_rate: None,
        streams: false,
        runtime: gguf(&FLUID_SPEECH_PRO_ARTIFACT),
        languages: languages(PARAKEET_V3_LANGUAGES),
        // Like v3, its base: it detects the language from the audio and has
        // no language input, so it is offered under Auto.
        accepts_language_hint: false,
        supports_language_detection: true,
    },
    ModelDefinition {
        id: TranscriptionModelId::WhisperLargeV3Turbo,
        name: text("Whisper large-v3-turbo"),
        realtime: "~19x",
        word_error_rate: Some(2.01),
        streams: false,
        runtime: gguf(&WHISPER_ARTIFACT),
        languages: languages(&["*"]),
        accepts_language_hint: true,
        supports_language_detection: true,
    },
    ModelDefinition {
        id: TranscriptionModelId::Qwen3Asr06B,
        name: text("Qwen3-ASR 0.6B"),
        realtime: "~16x",
        word_error_rate: Some(7.64),
        streams: false,
        runtime: gguf(&QWEN_ARTIFACT),
        languages: languages(QWEN_LANGUAGES),
        accepts_language_hint: true,
        supports_language_detection: true,
    },
    ModelDefinition {
        id: TranscriptionModelId::QwenCleoAsr17B,
        name: text("QwenCleo-ASR 1.7B"),
        // Half Qwen3-ASR 0.6B's speed on apollo's Vulkan GPU (22x against 41x).
        realtime: "~9x",
        // The author's own 3,699-utterance evaluation, not a public test set.
        word_error_rate: Some(19.85),
        streams: false,
        runtime: gguf(&QWENCLEO_ARTIFACT),
        languages: languages(&["ar"]),
        accepts_language_hint: true,
        supports_language_detection: false,
    },
    ModelDefinition {
        id: TranscriptionModelId::IvritWhisperLargeV3Turbo,
        name: text("ivrit.ai Whisper large-v3-turbo"),
        // The same architecture and size as Whisper large-v3-turbo; on apollo
        // it ran a 9.8 s Hebrew clip as fast as the stock model (38x and 36x).
        realtime: "~19x",
        word_error_rate: Some(18.45),
        streams: false,
        runtime: gguf(&IVRIT_WHISPER_ARTIFACT),
        // ivrit.ai: the fine-tune lost language detection and translation, so
        // it always runs with the Hebrew hint.
        languages: languages(&["he"]),
        accepts_language_hint: true,
        supports_language_detection: false,
    },
    ModelDefinition {
        id: TranscriptionModelId::SenseVoiceSmall,
        name: text("SenseVoice Small"),
        realtime: "~72x",
        word_error_rate: Some(10.11),
        streams: false,
        runtime: gguf(&SENSEVOICE_ARTIFACT),
        languages: languages(&["zh", "yue", "en", "ja", "ko"]),
        accepts_language_hint: true,
        supports_language_detection: true,
    },
    ModelDefinition {
        id: TranscriptionModelId::CohereTranscribe,
        name: text("Cohere Transcribe"),
        realtime: "73x",
        word_error_rate: Some(1.27),
        streams: false,
        runtime: gguf(&COHERE_ARTIFACT),
        languages: languages(COHERE_LANGUAGES),
        accepts_language_hint: true,
        supports_language_detection: false,
    },
    ModelDefinition {
        id: TranscriptionModelId::UnderdogBark3Pro,
        name: text("Underdog Bark 3.0 Pro"),
        realtime: "~33x",
        word_error_rate: Some(1.38),
        streams: false,
        runtime: gguf(&UNDERDOG_BARK_3_PRO_ARTIFACT),
        languages: languages(&["en"]),
        // Like Parakeet v3, its base, the artifact has no language input.
        accepts_language_hint: false,
        supports_language_detection: false,
    },
    ModelDefinition {
        id: TranscriptionModelId::UnderdogBark3Clean,
        name: text("Underdog Bark 3.0 Clean"),
        realtime: "~33x",
        word_error_rate: Some(1.71),
        streams: false,
        runtime: gguf(&UNDERDOG_BARK_3_CLEAN_ARTIFACT),
        languages: languages(&["en"]),
        accepts_language_hint: false,
        supports_language_detection: false,
    },
    ModelDefinition {
        id: TranscriptionModelId::ParakeetUltra,
        name: text("Parakeet Ultra"),
        // As fast as Parakeet v3 on apollo's Vulkan GPU (191x against 211x
        // on a FLEURS English clip), so it keeps v3's figure.
        realtime: "~33x",
        // LibriSpeech test-clean, transcribe.cpp's published conversion
        // benchmark (`model_benchmarks`); not measured in Hex.
        word_error_rate: Some(1.80),
        streams: false,
        runtime: gguf(&PARAKEET_ULTRA_ARTIFACT),
        languages: languages(PARAKEET_V3_LANGUAGES),
        // Like v3, its base: it detects the language from the audio and has
        // no language input, so it is offered under Auto.
        accepts_language_hint: false,
        supports_language_detection: true,
    },
    ModelDefinition {
        id: TranscriptionModelId::Nemotron35Asr,
        name: text("Nemotron 3.5 ASR"),
        // On apollo's Vulkan GPU it ran 0.55 to 0.97 times as fast as
        // Parakeet v3 (17.9 s and 54.6 s German clips), against v3's ~33x.
        realtime: "~25x",
        // LibriSpeech test-clean, Hex's run (`model_benchmarks`). On the same
        // runs Parakeet v3 made fewer errors: 1.95% here, and on FLEURS 4.88%
        // English and 5.17% German against 7.91% and 10.39% (8.77% and
        // 10.46% with Auto). It is here to stream with the language set.
        word_error_rate: Some(3.05),
        streams: true,
        runtime: gguf(&NEMOTRON_3_5_ASR_ARTIFACT),
        languages: languages(NEMOTRON_3_5_LANGUAGES),
        // Its prompt takes the language (`runtime_language`), and Auto leaves
        // it to the model.
        accepts_language_hint: true,
        supports_language_detection: true,
    },
    ModelDefinition {
        id: TranscriptionModelId::GigaAmV3,
        name: text("GigaAM v3"),
        // On apollo's Vulkan GPU it ran 1.2 to 1.4 times as fast as Parakeet
        // v3 (the FLEURS Russian test split, and a 10 s clip of it).
        realtime: "~40x",
        // FLEURS Russian test, Hex's run (`model_benchmarks`): Parakeet v3
        // scored 6.52% on the same run and Whisper large-v3-turbo 5.36%.
        word_error_rate: Some(5.37),
        streams: false,
        runtime: gguf(&GIGAAM_V3_E2E_RNNT_ARTIFACT),
        languages: languages(&["ru"]),
        // Russian only: it has no language input.
        accepts_language_hint: false,
        supports_language_detection: false,
    },
    ModelDefinition {
        id: TranscriptionModelId::NbWhisperMedium,
        name: text("NB-Whisper Medium"),
        // On apollo's Vulkan GPU it ran FLEURS Norwegian at 0.89 times
        // Whisper large-v3-turbo's speed (37x and 41x real time).
        realtime: "~17x",
        // FLEURS Norwegian test, Hex's run (`model_benchmarks`): Whisper
        // large-v3-turbo scored 8.94% on the same run.
        word_error_rate: Some(6.68),
        streams: false,
        runtime: gguf(&NB_WHISPER_MEDIUM_ARTIFACT),
        // Measured only for Bokmål with the `no` hint, which it always gets.
        languages: languages(&["no"]),
        accepts_language_hint: true,
        supports_language_detection: false,
    },
    ModelDefinition {
        id: TranscriptionModelId::KbWhisperMedium,
        name: text("KB-Whisper Medium"),
        // Not timed in Hex. It is the same whisper-medium graph at the same
        // Q5_0 quantization as NB-Whisper Medium, which ran FLEURS Norwegian
        // at ~17x on apollo's Vulkan GPU.
        realtime: "~17x",
        // KBLab's model card for its default (standard) checkpoint on the
        // FLEURS Swedish test (`model_benchmarks`); Hex has not measured it.
        word_error_rate: Some(6.6),
        streams: false,
        runtime: gguf(&KB_WHISPER_MEDIUM_ARTIFACT),
        // The card's only language; the pinned file takes the `sv` hint.
        languages: languages(&["sv"]),
        accepts_language_hint: true,
        supports_language_detection: false,
    },
    ModelDefinition {
        id: TranscriptionModelId::GraniteSpeech5,
        name: text("Granite Speech 5.0"),
        // On apollo's Vulkan GPU it ran 2.1 to 2.4 times as fast as Whisper
        // large-v3-turbo and 0.4 to 0.64 times Parakeet v3's speed (an 11 s
        // clip, and the FLEURS English and Japanese test splits).
        realtime: "~25x",
        // FLEURS English test, Hex's run (`model_benchmarks`).
        word_error_rate: Some(4.77),
        streams: false,
        runtime: gguf(&GRANITE_SPEECH_5_ARTIFACT),
        languages: languages(GRANITE_SPEECH_5_LANGUAGES),
        // A CTC head with no language input: it transcribes whichever of
        // its seven languages it hears, so it is offered under Auto even
        // though its pinned file leaves stt.capability.lang_detect off.
        accepts_language_hint: false,
        supports_language_detection: true,
    },
    ModelDefinition {
        id: TranscriptionModelId::AppleSpeech,
        name: text("Apple Speech"),
        realtime: "On device",
        word_error_rate: None,
        streams: false,
        runtime: ModelRuntime::AppleSpeech,
        languages: languages(&["*"]),
        accepts_language_hint: true,
        supports_language_detection: false,
    },
];
