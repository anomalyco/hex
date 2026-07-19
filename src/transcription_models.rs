use std::fs::{self, File};
use std::io::{ErrorKind, Read};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::thread;
use std::time::Duration;

use color_eyre::eyre::{Result, WrapErr, bail};
use fs2::FileExt;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

#[derive(Clone, Copy, Debug, Default, Deserialize, Eq, Hash, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum TranscriptionModelId {
    #[default]
    ParakeetV2,
    ParakeetV3,
    WhisperLargeV3Turbo,
    Qwen3Asr06B,
    SenseVoiceSmall,
    CohereTranscribe,
    AppleSpeech,
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
            model: TranscriptionModelId::ParakeetV2,
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
    BuiltIn,
}

impl Recommendation {
    pub const fn label(self) -> &'static str {
        match self {
            Self::Recommended => "Recommended",
            Self::Fastest => "Fastest",
            Self::MostAccurate => "Most accurate",
            Self::RecognitionHints => "Recognition hints",
            Self::BuiltIn => "Built in",
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
    pub realtime_context: &'static str,
    pub quality: &'static str,
    pub quality_context: &'static str,
    pub coverage: &'static str,
    pub timestamps: &'static str,
    pub runtime: ModelRuntime,
    pub languages: &'static [&'static str],
    pub accepts_language_hint: bool,
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

impl ModelDefinition {
    fn download_url(&self) -> Result<String> {
        let ModelRuntime::Gguf(artifact) = self.runtime else {
            bail!("{} is managed by macOS and has no GGUF artifact", self.name);
        };
        Ok(format!(
            "https://huggingface.co/{}/resolve/{}/{}",
            artifact.repository, artifact.revision, artifact.filename
        ))
    }

    pub fn supports_language(&self, language: &str) -> bool {
        self.languages.contains(&"*") || self.languages.contains(&language)
    }

    pub fn runtime_language<'a>(&self, language: &'a str) -> &'a str {
        if self.id == TranscriptionModelId::WhisperLargeV3Turbo && language == "fil" {
            "tl"
        } else {
            language
        }
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

pub const MODELS: &[ModelDefinition] = &[
    ModelDefinition {
        id: TranscriptionModelId::ParakeetV2,
        name: "Parakeet v2",
        realtime: "~55x",
        realtime_context: "local speed · M2 Max",
        quality: "1.69%",
        quality_context: "English benchmark",
        coverage: "English",
        timestamps: "Token timestamps",
        runtime: ModelRuntime::Gguf(&PARAKEET_V2_ARTIFACT),
        languages: &["en"],
        accepts_language_hint: true,
        supports_recognition_hints: false,
    },
    ModelDefinition {
        id: TranscriptionModelId::ParakeetV3,
        name: "Parakeet v3",
        realtime: "~33x",
        realtime_context: "local speed · M2 Max",
        quality: "1.94%",
        quality_context: "English benchmark",
        coverage: "25 languages",
        timestamps: "Token timestamps",
        runtime: ModelRuntime::Gguf(&PARAKEET_V3_ARTIFACT),
        languages: PARAKEET_V3_LANGUAGES,
        accepts_language_hint: true,
        supports_recognition_hints: false,
    },
    ModelDefinition {
        id: TranscriptionModelId::WhisperLargeV3Turbo,
        name: "Whisper large-v3-turbo",
        realtime: "~19x",
        realtime_context: "local speed · M2 Max",
        quality: "2.01%",
        quality_context: "English benchmark",
        coverage: "100 languages",
        timestamps: "Segment timestamps",
        runtime: ModelRuntime::Gguf(&WHISPER_ARTIFACT),
        languages: &["*"],
        accepts_language_hint: true,
        supports_recognition_hints: true,
    },
    ModelDefinition {
        id: TranscriptionModelId::Qwen3Asr06B,
        name: "Qwen3-ASR 0.6B",
        realtime: "~16x",
        realtime_context: "local speed · M2 Max",
        quality: "7.64%",
        quality_context: "Mandarin benchmark",
        coverage: "30 languages",
        timestamps: "No timestamps",
        runtime: ModelRuntime::Gguf(&QWEN_ARTIFACT),
        languages: QWEN_LANGUAGES,
        accepts_language_hint: false,
        supports_recognition_hints: false,
    },
    ModelDefinition {
        id: TranscriptionModelId::SenseVoiceSmall,
        name: "SenseVoice Small",
        realtime: "~72x",
        realtime_context: "local speed · M2 Max",
        quality: "10.11%",
        quality_context: "Mandarin benchmark",
        coverage: "5 languages",
        timestamps: "No timestamps",
        runtime: ModelRuntime::Gguf(&SENSEVOICE_ARTIFACT),
        languages: &["zh", "yue", "en", "ja", "ko"],
        accepts_language_hint: true,
        supports_recognition_hints: false,
    },
    ModelDefinition {
        id: TranscriptionModelId::CohereTranscribe,
        name: "Cohere Transcribe",
        realtime: "73x",
        realtime_context: "published speed · M4 Max",
        quality: "1.27%",
        quality_context: "English benchmark",
        coverage: "14 languages",
        timestamps: "No timestamps",
        runtime: ModelRuntime::Gguf(&COHERE_ARTIFACT),
        languages: COHERE_LANGUAGES,
        accepts_language_hint: true,
        supports_recognition_hints: false,
    },
    ModelDefinition {
        id: TranscriptionModelId::AppleSpeech,
        name: "Apple Speech",
        realtime: "On device",
        realtime_context: "macOS 26",
        quality: "System",
        quality_context: "Apple managed",
        coverage: "System locales",
        timestamps: "Segment timestamps",
        runtime: ModelRuntime::AppleSpeech,
        languages: &["*"],
        accepts_language_hint: true,
        supports_recognition_hints: false,
    },
];

pub const LANGUAGES: &[(&str, &str)] = &[
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

pub fn choices(language: &str) -> Vec<ModelChoice> {
    #[cfg(target_os = "macos")]
    let apple_speech_supported = crate::apple_speech::AppleSpeech::is_supported(language);
    #[cfg(not(target_os = "macos"))]
    let apple_speech_supported = false;
    choices_for_runtime(language, apple_speech_supported)
}

fn choices_for_runtime(language: &str, apple_speech_supported: bool) -> Vec<ModelChoice> {
    let choice = |id, recommendation| ModelChoice {
        model: definition(id),
        recommendation,
    };
    let mut choices = match language {
        "en" => vec![
            choice(
                TranscriptionModelId::ParakeetV2,
                Recommendation::Recommended,
            ),
            choice(
                TranscriptionModelId::CohereTranscribe,
                Recommendation::MostAccurate,
            ),
            choice(
                TranscriptionModelId::WhisperLargeV3Turbo,
                Recommendation::RecognitionHints,
            ),
        ],
        "zh" | "yue" | "ja" | "ko" => vec![
            choice(
                TranscriptionModelId::Qwen3Asr06B,
                Recommendation::Recommended,
            ),
            choice(
                TranscriptionModelId::SenseVoiceSmall,
                Recommendation::Fastest,
            ),
            choice(
                TranscriptionModelId::WhisperLargeV3Turbo,
                Recommendation::RecognitionHints,
            ),
        ],
        language if PARAKEET_V3_LANGUAGES.contains(&language) => vec![
            choice(
                TranscriptionModelId::ParakeetV3,
                Recommendation::Recommended,
            ),
            choice(
                TranscriptionModelId::WhisperLargeV3Turbo,
                Recommendation::RecognitionHints,
            ),
        ],
        language if QWEN_LANGUAGES.contains(&language) => vec![
            choice(
                TranscriptionModelId::Qwen3Asr06B,
                Recommendation::Recommended,
            ),
            choice(
                TranscriptionModelId::WhisperLargeV3Turbo,
                Recommendation::RecognitionHints,
            ),
        ],
        _ => vec![choice(
            TranscriptionModelId::WhisperLargeV3Turbo,
            Recommendation::Recommended,
        )],
    };
    if apple_speech_supported {
        choices.push(choice(
            TranscriptionModelId::AppleSpeech,
            Recommendation::BuiltIn,
        ));
    }
    choices
}

pub fn validate(selection: &TranscriptionSelection) -> Result<&'static ModelDefinition> {
    if !LANGUAGES
        .iter()
        .any(|(code, _)| *code == selection.language)
    {
        bail!("unsupported transcription language: {}", selection.language);
    }
    let model = definition(selection.model);
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
    Ok(crate::app_paths::support_dir()?.join("models"))
}

pub fn model_path(model: &ModelDefinition) -> Result<PathBuf> {
    let ModelRuntime::Gguf(artifact) = model.runtime else {
        bail!("{} is managed by macOS and has no model path", model.name);
    };
    Ok(models_dir()?.join(artifact.filename))
}

pub fn is_installed(model: &ModelDefinition, language: &str) -> bool {
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

pub fn download_with_progress(
    model: &ModelDefinition,
    canceled: &AtomicBool,
    downloaded_bytes: &AtomicU64,
) -> Result<PathBuf> {
    let ModelRuntime::Gguf(artifact) = model.runtime else {
        bail!("{} is installed and managed by macOS", model.name);
    };
    check_canceled(canceled)?;
    let directory = models_dir()?;
    fs::create_dir_all(&directory)?;
    let lock = File::create(directory.join(".download.lock"))?;
    loop {
        match lock.try_lock_exclusive() {
            Ok(()) => break,
            Err(error) if error.kind() == ErrorKind::WouldBlock => {
                check_canceled(canceled)?;
                thread::sleep(Duration::from_millis(100));
            }
            Err(error) => return Err(error.into()),
        }
    }
    check_canceled(canceled)?;
    let destination = directory.join(artifact.filename);
    if destination.exists() {
        match verify_file_with_cancel(&destination, model, canceled) {
            Ok(()) => {
                downloaded_bytes.store(artifact.bytes, Ordering::Relaxed);
                return Ok(destination);
            }
            Err(error) => {
                tracing::warn!(%error, path = %destination.display(), "replacing invalid transcription model");
                fs::remove_file(&destination)?;
            }
        }
    }
    let partial = destination.with_extension("gguf.partial");
    if fs::metadata(&partial).is_ok_and(|metadata| metadata.len() > artifact.bytes) {
        fs::remove_file(&partial)?;
    }
    downloaded_bytes.store(
        fs::metadata(&partial).map_or(0, |metadata| metadata.len()),
        Ordering::Relaxed,
    );
    if fs::metadata(&partial).is_ok_and(|metadata| metadata.len() == artifact.bytes) {
        match verify_file_with_cancel(&partial, model, canceled) {
            Ok(()) => {
                File::open(&partial)?.sync_all()?;
                fs::rename(&partial, &destination)?;
                File::open(&directory)?.sync_all()?;
                downloaded_bytes.store(artifact.bytes, Ordering::Relaxed);
                return Ok(destination);
            }
            Err(error) if canceled.load(Ordering::Relaxed) => return Err(error),
            Err(error) => {
                tracing::warn!(%error, path = %partial.display(), "replacing invalid partial transcription model");
                fs::remove_file(&partial)?;
                downloaded_bytes.store(0, Ordering::Relaxed);
            }
        }
    }
    let mut child = Command::new("curl")
        .args([
            "--fail",
            "--location",
            "--retry",
            "3",
            "--continue-at",
            "-",
            "--silent",
            "--show-error",
            "--output",
        ])
        .arg(&partial)
        .arg(model.download_url()?)
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
        bail!("model download failed with {status}: {}", stderr.trim());
    }
    check_canceled(canceled)?;
    if let Err(error) = verify_file_with_cancel(&partial, model, canceled) {
        if !canceled.load(Ordering::Relaxed) {
            let _ = fs::remove_file(&partial);
        }
        return Err(error);
    }
    File::open(&partial)?.sync_all()?;
    fs::rename(&partial, &destination)?;
    File::open(&directory)?.sync_all()?;
    downloaded_bytes.store(artifact.bytes, Ordering::Relaxed);
    Ok(destination)
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

fn verify_file_with_cancel(
    path: &Path,
    model: &ModelDefinition,
    canceled: &AtomicBool,
) -> Result<()> {
    let ModelRuntime::Gguf(artifact) = model.runtime else {
        bail!("{} has no artifact to verify", model.name);
    };
    let metadata = fs::metadata(path)?;
    if metadata.len() != artifact.bytes {
        bail!(
            "downloaded {} bytes for {}, expected {}",
            metadata.len(),
            model.name,
            artifact.bytes
        );
    }
    let mut file = File::open(path)?;
    let mut digest = Sha256::new();
    let mut buffer = [0_u8; 1024 * 1024];
    loop {
        check_canceled(canceled)?;
        let read = file.read(&mut buffer)?;
        if read == 0 {
            break;
        }
        digest.update(&buffer[..read]);
    }
    let actual = digest
        .finalize()
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect::<String>();
    if actual != artifact.sha256 {
        bail!("checksum mismatch for {}", model.name);
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use std::sync::atomic::{AtomicU64, Ordering};

    use super::*;

    static NEXT_TEMP_FILE: AtomicU64 = AtomicU64::new(0);

    #[test]
    fn recommendations_are_language_specific() {
        let english = choices_for_runtime("en", false);
        assert_eq!(english[0].model.id, TranscriptionModelId::ParakeetV2);
        assert_eq!(english[0].recommendation, Recommendation::Recommended);
        assert_eq!(english[1].model.id, TranscriptionModelId::CohereTranscribe);
        assert_eq!(english[1].recommendation, Recommendation::MostAccurate);
        let mandarin = choices_for_runtime("zh", false);
        assert_eq!(mandarin[0].model.id, TranscriptionModelId::Qwen3Asr06B);
        assert_eq!(mandarin[1].model.id, TranscriptionModelId::SenseVoiceSmall);
    }

    #[test]
    fn apple_speech_is_only_offered_when_the_runtime_supports_the_locale() {
        assert!(
            choices_for_runtime("en", false)
                .iter()
                .all(|choice| choice.model.id != TranscriptionModelId::AppleSpeech)
        );
        assert_eq!(
            choices_for_runtime("en", true).last().unwrap().model.id,
            TranscriptionModelId::AppleSpeech
        );
        let apple_speech = definition(TranscriptionModelId::AppleSpeech);
        assert!(matches!(apple_speech.runtime, ModelRuntime::AppleSpeech));
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
        const FIXTURE_ARTIFACT: GgufArtifact = GgufArtifact {
            filename: "fixture.gguf",
            revision: "fixture",
            repository: "fixture",
            bytes: 5,
            sha256: "2cf24dba5fb0a30e26e83b2ac5b9e29e1b161e5c1fa7425e73043362938b9824",
            architecture: "fixture",
            variant: "fixture",
        };
        let model = ModelDefinition {
            id: TranscriptionModelId::ParakeetV2,
            name: "fixture",
            realtime: "fixture",
            realtime_context: "fixture",
            quality: "fixture",
            quality_context: "fixture",
            coverage: "fixture",
            timestamps: "fixture",
            runtime: ModelRuntime::Gguf(&FIXTURE_ARTIFACT),
            languages: &["en"],
            accepts_language_hint: false,
            supports_recognition_hints: false,
        };
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
    fn canceled_downloads_stop_before_io() {
        let canceled = AtomicBool::new(true);
        assert!(check_canceled(&canceled).is_err());
    }
}
