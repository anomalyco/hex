use crate::durable::{lock_contended, sync_directory, sync_file};
use std::borrow::Cow;
use std::fs::{self, File};
use std::io::{ErrorKind, Read};
use std::path::{Path, PathBuf};
use std::str::FromStr;
use std::sync::atomic::{AtomicBool, AtomicU8, AtomicU64, Ordering};
use std::thread;
use std::time::{Duration, UNIX_EPOCH};

use color_eyre::eyre::{Result, WrapErr, bail, eyre};
use fs2::FileExt;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

use super::artifacts::{PARAKEET_V3_LANGUAGES, QWEN_LANGUAGES};
pub use super::custom_models::CustomModelId;
pub use super::definitions::{
    AUTO_LANGUAGE, GgufArtifact, MODELS, ModelDefinition, ModelRef, ModelRuntime, VocabularyUse,
};
pub use super::languages::{LANGUAGES, language_name, menu_order};
pub(crate) use super::model_benchmarks::most_accurate_is_comparable;
pub use super::model_benchmarks::{BenchmarkRecord, benchmark};

/// A catalog model, or one imported or found on this computer.
/// Saved as its wire name (`parakeet_v3`, `custom-<16 hex digits>`); a name
/// this build doesn't know fails to decode, so the lenient settings loader
/// keeps the default for it without rewriting the file (#744).
#[derive(Clone, Copy, Debug, Default, Eq, Hash, PartialEq)]
pub enum TranscriptionModelId {
    #[default]
    ParakeetUnifiedEnglish,
    ParakeetV2,
    ParakeetV3,
    Orukeet,
    FluidSpeechPro,
    WhisperLargeV3Turbo,
    Qwen3Asr06B,
    QwenCleoAsr17B,
    IvritWhisperLargeV3Turbo,
    SenseVoiceSmall,
    CohereTranscribe,
    UnderdogBark3Pro,
    UnderdogBark3Clean,
    ParakeetUltra,
    Nemotron35Asr,
    GigaAmV3,
    NbWhisperMedium,
    KbWhisperMedium,
    GraniteSpeech5,
    AppleSpeech,
    Custom(CustomModelId),
}

impl Serialize for TranscriptionModelId {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.serialize_str(self.as_str())
    }
}

impl<'de> Deserialize<'de> for TranscriptionModelId {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let name = String::deserialize(deserializer)?;
        name.parse().map_err(serde::de::Error::custom)
    }
}

impl TranscriptionModelId {
    pub fn as_str(&self) -> &str {
        match self {
            Self::ParakeetUnifiedEnglish => "parakeet_unified_en",
            Self::ParakeetV2 => "parakeet_v2",
            Self::ParakeetV3 => "parakeet_v3",
            Self::Orukeet => "orukeet",
            Self::FluidSpeechPro => "fluid_speech_pro",
            Self::WhisperLargeV3Turbo => "whisper_large_v3_turbo",
            Self::Qwen3Asr06B => "qwen3_asr06_b",
            Self::QwenCleoAsr17B => "qwencleo_asr_1_7b",
            Self::IvritWhisperLargeV3Turbo => "ivrit_whisper_large_v3_turbo",
            Self::SenseVoiceSmall => "sense_voice_small",
            Self::CohereTranscribe => "cohere_transcribe",
            Self::UnderdogBark3Pro => "underdog_bark_3_pro",
            Self::UnderdogBark3Clean => "underdog_bark_3_clean",
            Self::ParakeetUltra => "parakeet_ultra",
            Self::Nemotron35Asr => "nemotron_3_5_asr",
            Self::GigaAmV3 => "gigaam_v3",
            Self::NbWhisperMedium => "nb_whisper_medium",
            Self::KbWhisperMedium => "kb_whisper_medium",
            Self::GraniteSpeech5 => "granite_speech_5",
            Self::AppleSpeech => "apple_speech",
            Self::Custom(id) => id.as_str(),
        }
    }

    pub const fn is_custom(self) -> bool {
        matches!(self, Self::Custom(_))
    }

    /// A model behind its own Labs toggle: an import, or Parakeet Ultra. A
    /// saved choice of one stays saved while it can't run, and the listener
    /// runs a fallback (`custom_models::runnable`).
    pub const fn is_labs_gated(self) -> bool {
        matches!(self, Self::Custom(_) | Self::ParakeetUltra)
    }
}

impl FromStr for TranscriptionModelId {
    type Err = color_eyre::Report;

    fn from_str(value: &str) -> Result<Self> {
        match value {
            "parakeet_unified_en" => Ok(Self::ParakeetUnifiedEnglish),
            "parakeet_v2" => Ok(Self::ParakeetV2),
            "parakeet_v3" => Ok(Self::ParakeetV3),
            "orukeet" => Ok(Self::Orukeet),
            "fluid_speech_pro" => Ok(Self::FluidSpeechPro),
            "whisper_large_v3_turbo" => Ok(Self::WhisperLargeV3Turbo),
            "qwen3_asr06_b" => Ok(Self::Qwen3Asr06B),
            "qwencleo_asr_1_7b" => Ok(Self::QwenCleoAsr17B),
            "ivrit_whisper_large_v3_turbo" => Ok(Self::IvritWhisperLargeV3Turbo),
            "sense_voice_small" => Ok(Self::SenseVoiceSmall),
            "cohere_transcribe" => Ok(Self::CohereTranscribe),
            "underdog_bark_3_pro" => Ok(Self::UnderdogBark3Pro),
            "underdog_bark_3_clean" => Ok(Self::UnderdogBark3Clean),
            "parakeet_ultra" => Ok(Self::ParakeetUltra),
            "nemotron_3_5_asr" => Ok(Self::Nemotron35Asr),
            "gigaam_v3" => Ok(Self::GigaAmV3),
            "nb_whisper_medium" => Ok(Self::NbWhisperMedium),
            "kb_whisper_medium" => Ok(Self::KbWhisperMedium),
            "granite_speech_5" => Ok(Self::GraniteSpeech5),
            "apple_speech" => Ok(Self::AppleSpeech),
            _ => CustomModelId::parse(value)
                .map(Self::Custom)
                .ok_or_else(|| eyre!("unknown transcription model: {value}")),
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
}

impl Default for TranscriptionSelection {
    fn default() -> Self {
        Self {
            model: TranscriptionModelId::default(),
            language: "en".into(),
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Recommendation {
    Recommended,
    Fastest,
    MostAccurate,
    Vocabulary,
}

#[derive(Clone)]
pub struct ModelChoice {
    pub model: ModelRef,
    pub recommendation: Recommendation,
}

/// Silero VAD v6.2. Dictation and meetings skip audio it finds no speech in,
/// because models hallucinate closing phrases such as "thank you" on silence
/// and room noise. It downloads with every transcription model.
pub const SPEECH_DETECTOR_ARTIFACT: GgufArtifact = GgufArtifact {
    filename: Cow::Borrowed("silero-vad-v6.2-F32.gguf"),
    stored_as: None,
    revision: "a9b2d61c20ae4ddf3644edb34969e8185a485448",
    repository: "handy-computer/silero-vad-v6.2-gguf",
    bytes: 1_241_056,
    sha256: Cow::Borrowed("0d871a800c420fdd5d1b17a3aea4296d1c39608f0ff4a9295eb76457eb1c88e6"),
    architecture: "silero_vad",
    variant: Cow::Borrowed("v6.2"),
};
#[cfg_attr(test, allow(dead_code))]
const SPEECH_DETECTOR_NAME: &str = "the speech detector";

pub fn definition(id: TranscriptionModelId) -> ModelRef {
    if let TranscriptionModelId::Custom(id) = id {
        return super::custom_models::definition(id);
    }
    ModelRef::Static(
        MODELS
            .iter()
            .find(|model| model.id == id)
            .expect("every transcription model id has a definition"),
    )
}

/// Every catalog model, then the imports while their Labs toggle is on.
pub fn all_models() -> Vec<ModelRef> {
    MODELS
        .iter()
        .map(ModelRef::Static)
        .chain(
            super::custom_models::imported()
                .into_iter()
                .map(|import| import.definition),
        )
        .collect()
}

pub(crate) fn choices_for_runtime(language: &str) -> Vec<ModelChoice> {
    use Recommendation::{Fastest, MostAccurate, Recommended, Vocabulary};
    use TranscriptionModelId::{
        CohereTranscribe, GigaAmV3, IvritWhisperLargeV3Turbo, KbWhisperMedium, NbWhisperMedium,
        ParakeetUnifiedEnglish, ParakeetV3, Qwen3Asr06B, SenseVoiceSmall, WhisperLargeV3Turbo,
    };
    let choices: &[(TranscriptionModelId, Recommendation)] = match language {
        AUTO_LANGUAGE => &[(WhisperLargeV3Turbo, Recommended), (Qwen3Asr06B, Fastest)],
        "en" => &[
            (ParakeetUnifiedEnglish, Recommended),
            (CohereTranscribe, MostAccurate),
            (WhisperLargeV3Turbo, Vocabulary),
        ],
        "zh" => &[
            (Qwen3Asr06B, Recommended),
            (SenseVoiceSmall, Fastest),
            (WhisperLargeV3Turbo, Vocabulary),
        ],
        "yue" | "ko" => &[
            (Qwen3Asr06B, Recommended),
            (WhisperLargeV3Turbo, Vocabulary),
        ],
        // FLEURS Japanese test: Granite Speech 5.0 tied Whisper turbo
        // (5.04% CER against 5.02%) at 2.4 times its speed, without
        // vocabulary, so it stays unbadged (Hex's run).
        "ja" => &[
            (WhisperLargeV3Turbo, Recommended),
            (SenseVoiceSmall, Fastest),
        ],
        "el" => &[
            (CohereTranscribe, Recommended),
            (WhisperLargeV3Turbo, Vocabulary),
        ],
        "fi" | "da" => &[(WhisperLargeV3Turbo, Recommended), (ParakeetV3, Fastest)],
        "mt" => &[(ParakeetV3, Recommended)],
        "ar" => &[
            (WhisperLargeV3Turbo, Recommended),
            (CohereTranscribe, MostAccurate),
        ],
        "tr" | "ms" | "fil" | "fa" | "mk" => &[(WhisperLargeV3Turbo, Recommended)],
        // FLEURS Hebrew test: 18.45% WER against stock Whisper's 29.69%.
        "he" => &[
            (IvritWhisperLargeV3Turbo, Recommended),
            (WhisperLargeV3Turbo, Vocabulary),
        ],
        // FLEURS Russian test: 5.37% WER against Parakeet v3's 6.52%, and
        // Whisper turbo's 5.36% at a fifth of its speed (Hex's run).
        "ru" => &[(GigaAmV3, Recommended), (WhisperLargeV3Turbo, Vocabulary)],
        // FLEURS Norwegian test: 6.68% WER against Whisper turbo's 8.94%, at
        // 0.89 times its speed and 60% of its download (Hex's run).
        "no" => &[
            (NbWhisperMedium, Recommended),
            (WhisperLargeV3Turbo, Vocabulary),
        ],
        // KBLab's model card figure for the default (standard) checkpoint on
        // the FLEURS Swedish test at 6.6% WER (Hex hasn't measured any of the
        // three in Swedish); Parakeet v3 is the faster, less accurate pick.
        "sv" => &[
            (KbWhisperMedium, Recommended),
            (ParakeetV3, Fastest),
            (WhisperLargeV3Turbo, Vocabulary),
        ],
        "vi" => &[
            (Qwen3Asr06B, Recommended),
            (CohereTranscribe, MostAccurate),
            (WhisperLargeV3Turbo, Vocabulary),
        ],
        language if PARAKEET_V3_LANGUAGES.contains(&language) => {
            &[(ParakeetV3, Recommended), (WhisperLargeV3Turbo, Vocabulary)]
        }
        language if QWEN_LANGUAGES.contains(&language) => &[
            (Qwen3Asr06B, Recommended),
            (WhisperLargeV3Turbo, Vocabulary),
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
pub(crate) fn picker_models(language: &str) -> Vec<(ModelRef, Option<Recommendation>)> {
    let recommended = choices_for_runtime(language);
    let others = MODELS
        .iter()
        .filter(|model| model.available())
        .filter(|model| recommended.iter().all(|choice| choice.model.id != model.id));
    let (supporting, switching): (Vec<_>, Vec<_>) =
        others.partition(|model| model.supports_language(language));
    // Imports follow, ready or not, so one that can't run can still be removed.
    let imported = super::custom_models::imported()
        .into_iter()
        .map(|import| import.definition);
    recommended
        .iter()
        .map(|choice| (choice.model.clone(), Some(choice.recommendation)))
        .chain(
            supporting
                .into_iter()
                .chain(switching)
                .map(ModelRef::Static)
                .chain(imported)
                .map(|model| (model, None)),
        )
        .collect()
}

/// Whisper large-v3's language codes, for its `"*"` catalog entry: the local
/// API accepts these beyond the languages Settings lists.
const WHISPER_LANGUAGES: &[&str] = &[
    "en", "zh", "de", "es", "ru", "ko", "fr", "ja", "pt", "tr", "pl", "ca", "nl", "ar", "sv", "it",
    "id", "hi", "fi", "vi", "he", "uk", "el", "ms", "cs", "ro", "da", "hu", "ta", "no", "th", "ur",
    "hr", "bg", "lt", "la", "mi", "ml", "cy", "sk", "te", "fa", "lv", "bn", "sr", "az", "sl", "kn",
    "et", "mk", "br", "eu", "is", "hy", "ne", "mn", "bs", "kk", "sq", "sw", "gl", "mr", "pa", "si",
    "km", "sn", "yo", "so", "af", "oc", "ka", "be", "tg", "sd", "gu", "am", "yi", "lo", "uz", "fo",
    "ht", "ps", "tk", "nn", "mt", "sa", "lb", "my", "bo", "tl", "mg", "as", "tt", "haw", "ln",
    "ha", "ba", "jw", "su", "yue",
];

/// Whether `language` is a code Hex knows: one Settings lists, or one of
/// Whisper's, which only models that accept any language support.
pub fn is_known_language(language: &str) -> bool {
    known_language(language).is_some()
}

/// `language` as Hex's own `'static` code, when Hex knows it.
pub(crate) fn known_language(language: &str) -> Option<&'static str> {
    LANGUAGES
        .iter()
        .map(|(code, _)| *code)
        .chain(WHISPER_LANGUAGES.iter().copied())
        .find(|code| *code == language)
}

/// Whether a saved selection stays saved: one `validate` accepts, or an
/// import with a known language, kept while it is missing, changed, or off
/// so the choice survives until it is fixed ([`super::custom_models::runnable`]).
pub fn validate_saved(selection: &TranscriptionSelection) -> Result<()> {
    if selection.model.is_labs_gated() {
        if !is_known_language(&selection.language) {
            bail!("unsupported transcription language: {}", selection.language);
        }
        return Ok(());
    }
    validate(selection).map(drop)
}

pub fn validate(selection: &TranscriptionSelection) -> Result<ModelRef> {
    validate_where(selection, crate::labs::k_quant_models())
}

/// [`validate`] against `k_quants` rather than this process's Labs switch, so
/// a test can run either side of Labs › Smaller model files.
pub(crate) fn validate_where(
    selection: &TranscriptionSelection,
    k_quants: bool,
) -> Result<ModelRef> {
    if !is_known_language(&selection.language) {
        bail!("unsupported transcription language: {}", selection.language);
    }
    let model = definition(selection.model);
    if !model.available_where(k_quants) {
        bail!("{} is unavailable", model.name);
    }
    if !model.supports_language(&selection.language) {
        bail!(
            "{} does not support {}",
            model.name,
            language_name(&selection.language)
        );
    }
    Ok(model)
}

pub fn models_dir() -> Result<PathBuf> {
    #[cfg(test)]
    if let Some(directory) = TEST_MODELS_DIR.with(|directory| directory.borrow().clone()) {
        return Ok(directory);
    }
    crate::app_paths::models_dir()
}

#[cfg(test)]
thread_local! {
    static TEST_MODELS_DIR: std::cell::RefCell<Option<PathBuf>> = const { std::cell::RefCell::new(None) };
}

#[cfg(test)]
pub(crate) fn set_test_models_dir(directory: Option<PathBuf>) {
    TEST_MODELS_DIR.with(|models| *models.borrow_mut() = directory);
}

pub fn max_audio_chunk_samples(architecture: &str, max_audio_ms: i64) -> Option<usize> {
    // Cohere's encoder capacity is not its recommended transcription window.
    // The pinned model's reference processor chunks long audio at 35 seconds.
    // Whisper decodes one 30-second window at a time; handed a longer clip its
    // own long-form loop also decodes a near-silent remainder, which it fills
    // with invented text (#1770).
    let maximum_ms = match (architecture, max_audio_ms) {
        ("cohere_asr", limit) if limit > 0 => limit.min(35_000),
        ("cohere_asr", _) => 35_000,
        ("whisper", limit) if limit > 0 => limit.min(30_000),
        ("whisper", _) => 30_000,
        (_, limit) => limit,
    };
    usize::try_from(maximum_ms)
        .ok()
        .filter(|milliseconds| *milliseconds > 0)?
        .checked_mul(16)
}

pub fn model_path(model: &ModelDefinition) -> Result<PathBuf> {
    let ModelRuntime::Gguf(artifact) = &model.runtime else {
        bail!("{} is managed by macOS and has no model path", model.name);
    };
    let managed = models_dir()?.join(artifact.local_name());
    // A catalog model another app downloaded (one already in use, or an
    // exact match the last scan found), used where it is.
    if !model.id.is_custom()
        && !managed.exists()
        && let Some(found) = super::found_models::catalog_candidate(model)
    {
        return Ok(found);
    }
    Ok(managed)
}

/// Whether `model` runs from a file another app downloaded (Delete forgets it).
pub fn is_in_place(model: &ModelDefinition) -> bool {
    !model.id.is_custom() && model_path(model).is_ok_and(|path| in_place(&path) && path.is_file())
}

/// Whether `path` is outside Hex's own models folder: a found file Hex
/// reads in place and never writes, moves, or deletes.
fn in_place(path: &Path) -> bool {
    models_dir().is_ok_and(|directory| !path.starts_with(directory))
}

pub fn is_installed(model: &ModelDefinition, language: &str) -> bool {
    #[cfg(not(target_os = "macos"))]
    let _ = language;
    match &model.runtime {
        ModelRuntime::Gguf(artifact) => model_path(model)
            .and_then(|path| Ok(fs::metadata(path)?.len() == artifact.bytes))
            .unwrap_or(false),
        #[cfg(target_os = "macos")]
        ModelRuntime::AppleSpeech => crate::apple_speech::AppleSpeech::is_ready(language),
        #[cfg(not(target_os = "macos"))]
        ModelRuntime::AppleSpeech => false,
    }
}

pub fn is_verified(model: &ModelDefinition) -> bool {
    if let TranscriptionModelId::Custom(id) = model.id {
        return super::custom_models::status(id) == super::custom_models::Status::Ready;
    }
    let Ok(path) = model_path(model) else {
        return false;
    };
    if in_place(&path) {
        return super::found_models::catalog_verified(model);
    }
    verification_receipt_matches(&path, model)
}

pub use super::readiness::{is_ready, needs_check};

pub fn verify_installed(model: &ModelDefinition, canceled: &AtomicBool) -> Result<PathBuf> {
    check_canceled(canceled)?;
    if let TranscriptionModelId::Custom(id) = model.id {
        return super::custom_models::verify(id, canceled);
    }
    let path = model_path(model)?;
    // Receipts for found files live with Hex, never beside the file.
    if in_place(&path) {
        return super::found_models::adopt_catalog(model, &path, canceled);
    }
    verify_installed_at(&path, model, canceled)?;
    Ok(path)
}

fn verify_installed_at(path: &Path, model: &ModelDefinition, canceled: &AtomicBool) -> Result<()> {
    check_canceled(canceled)?;
    if !path.is_file() {
        if model.id.is_custom() {
            bail!("{} is missing. Import it again.", model.name);
        }
        bail!(
            "{} is no longer installed. Open Settings to download it.",
            model.name
        );
    }
    let _lock = lock_downloads(path.parent().expect("model directory"), canceled, || {})?;
    if !verification_receipt_matches(path, model) {
        verify_file_with_cancel(path, model, canceled)?;
        write_verification_receipt(path, model)?;
    }
    Ok(())
}

/// Deletes a downloaded model with its verification receipt and any partial
/// download. Refuses instead of waiting while a download owns the directory.
pub fn remove_installed(model: &ModelDefinition) -> Result<()> {
    if let TranscriptionModelId::Custom(id) = model.id {
        return super::custom_models::remove(id);
    }
    if !model.runtime.is_gguf() {
        bail!("{} is managed by macOS and cannot be deleted", model.name);
    }
    let path = model_path(model)?;
    // A found file is only forgotten.
    if in_place(&path) {
        return super::found_models::forget_catalog(model);
    }
    remove_installed_at(&path)
}

pub(crate) fn remove_installed_at(path: &Path) -> Result<()> {
    let directory = path.parent().expect("model directory");
    if !directory.is_dir() {
        return Ok(());
    }
    let lock = File::create(directory.join(".download.lock"))?;
    match lock.try_lock_exclusive() {
        Ok(()) => {}
        Err(error) if lock_contended(&error) => {
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

/// Takes the models folder's download lock. While another process holds it
/// (`hex model install` in a terminal, or another Hex), `waiting` hears every
/// attempt, so a caller can report that process's progress.
fn lock_downloads(directory: &Path, canceled: &AtomicBool, waiting: impl Fn()) -> Result<File> {
    let lock = File::create(directory.join(".download.lock"))?;
    loop {
        check_canceled(canceled)?;
        match lock.try_lock_exclusive() {
            Ok(()) => return Ok(lock),
            Err(error) if lock_contended(&error) => {
                waiting();
                thread::sleep(Duration::from_millis(100));
            }
            Err(error) => return Err(error.into()),
        }
    }
}

fn verification_receipt_matches(path: &Path, model: &ModelDefinition) -> bool {
    let ModelRuntime::Gguf(artifact) = &model.runtime else {
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

/// `hex model install` on Linux and Windows, and tests.
#[cfg_attr(target_os = "macos", allow(dead_code))]
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
    let ModelRuntime::Gguf(artifact) = &model.runtime else {
        bail!("{} is installed and managed by macOS", model.name);
    };
    // An import has nothing to download: Hex checks its own copy. A catalog
    // model another app already downloaded is checked and used where it is.
    let path = if model.id.is_custom() || model_path(model).is_ok_and(|path| in_place(&path)) {
        ModelPreparationStage::Verifying.store(stage);
        let path = verify_installed(model, canceled)?;
        downloaded_bytes.store(artifact.bytes, Ordering::Relaxed);
        path
    } else {
        download_artifact(artifact, &model.name, canceled, downloaded_bytes, stage)?
    };
    // In the background, so an installed model activates at once even
    // offline. Until the detector lands, dictation and meetings keep their
    // level-only silence handling.
    if !speech_detector_installed() {
        crate::transcription::speech_detector::start_download();
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
    Ok(models_dir()?.join(SPEECH_DETECTOR_ARTIFACT.local_name()))
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
    let destination = directory.join(artifact.local_name());
    let partial = destination.with_extension("gguf.partial");
    // Another process may be writing this same partial file: its bytes are
    // this download's progress too, so waiting shows them instead of 0.
    let _lock = lock_downloads(&directory, canceled, || {
        if let Ok(metadata) = fs::metadata(&partial) {
            downloaded_bytes.store(metadata.len().min(artifact.bytes), Ordering::Relaxed);
        }
    })?;
    check_canceled(canceled)?;
    delete_set_aside_in(&directory);
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
    // In-process HTTPS with the system's proxy and trusted roots (`net`), so
    // managed networks with a proxy or TLS inspection work (#894).
    let url = artifact.download_url();
    let fetched = crate::net::download(
        &crate::net::Download {
            url: &url,
            user_agent: &format!("hex/{}", crate::VERSION),
            size: Some(artifact.bytes),
            canceled: Some(canceled),
        },
        &partial,
        |bytes| downloaded_bytes.store(bytes, Ordering::Relaxed),
    );
    if let Err(error) = fetched {
        check_canceled(canceled)?;
        // The first line is the reason onboarding and Settings show.
        bail!(
            "{error}\n\nCheck your connection, then retry, or choose a model file. \
             On a managed network, ask IT to allow downloads from huggingface.co and \
             its download CDN. Download progress is kept for retry."
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
pub(crate) fn write_json_atomically(path: &Path, value: &impl Serialize) -> Result<()> {
    crate::durable::write(path, &serde_json::to_vec(value)?)?;
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

pub(crate) fn write_verification_receipt(path: &Path, model: &ModelDefinition) -> Result<()> {
    let ModelRuntime::Gguf(artifact) = &model.runtime else {
        bail!("{} has no artifact verification receipt", model.name);
    };
    write_artifact_receipt(path, artifact)
}

fn write_artifact_receipt(path: &Path, artifact: &GgufArtifact) -> Result<()> {
    let metadata = fs::metadata(path)?;
    let receipt = VerificationReceipt {
        bytes: artifact.bytes,
        sha256: artifact.sha256.clone().into(),
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

fn verify_file_with_cancel(
    path: &Path,
    model: &ModelDefinition,
    canceled: &AtomicBool,
) -> Result<()> {
    let ModelRuntime::Gguf(artifact) = &model.runtime else {
        bail!("{} has no artifact to verify", model.name);
    };
    verify_artifact(path, artifact, &model.name, canceled)
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
    const FIXTURE_ARTIFACT: GgufArtifact = GgufArtifact {
        filename: Cow::Borrowed("fixture.gguf"),
        stored_as: None,
        revision: "fixture",
        repository: "fixture",
        bytes: 5,
        sha256: Cow::Borrowed("2cf24dba5fb0a30e26e83b2ac5b9e29e1b161e5c1fa7425e73043362938b9824"),
        architecture: "fixture",
        variant: Cow::Borrowed("fixture"),
    };

    fn fixture_model() -> ModelDefinition {
        ModelDefinition {
            runtime: ModelRuntime::Gguf(Cow::Borrowed(&FIXTURE_ARTIFACT)),
            ..(*definition(TranscriptionModelId::ParakeetV2)).clone()
        }
    }

    #[test]
    fn orukeet_is_offered_for_exactly_parakeet_v3s_languages_and_never_badged() {
        let orukeet = definition(TranscriptionModelId::Orukeet);
        let v3 = definition(TranscriptionModelId::ParakeetV3);
        assert_eq!(orukeet.languages, PARAKEET_V3_LANGUAGES);
        assert_eq!(orukeet.language_choices(), v3.language_choices());
        assert!(!orukeet.streams);
        assert!(!orukeet.accepts_language_hint);
        assert!(orukeet.supports_language_detection);
        assert!(!orukeet.id.is_labs_gated());
        assert!(orukeet.available());
        assert!(super::super::model_details::details(orukeet.id).is_some());
        for language in v3.languages.iter().copied().chain([AUTO_LANGUAGE]) {
            assert!(
                choices_for_runtime(language)
                    .iter()
                    .all(|choice| choice.model.id != orukeet.id),
                "Orukeet is never recommended ({language})"
            );
        }
    }

    #[test]
    fn fluid_speech_pro_is_offered_for_exactly_parakeet_v3s_languages_and_never_badged() {
        let fluid = definition(TranscriptionModelId::FluidSpeechPro);
        let v3 = definition(TranscriptionModelId::ParakeetV3);
        assert_eq!(fluid.languages, PARAKEET_V3_LANGUAGES);
        assert_eq!(fluid.language_choices(), v3.language_choices());
        assert!(!fluid.streams);
        assert!(!fluid.accepts_language_hint);
        assert!(fluid.supports_language_detection);
        assert!(!fluid.id.is_labs_gated());
        assert!(fluid.available());
        // Its card publishes Open ASR Leaderboard and FLEURS figures, never a
        // LibriSpeech test-clean one, so the picker has no single number to
        // compare and the model is never badged.
        assert!(fluid.word_error_rate.is_none());
        assert!(benchmark(&fluid).is_none());
        assert!(super::super::model_details::details(fluid.id).is_some());
        for language in v3.languages.iter().copied().chain([AUTO_LANGUAGE]) {
            assert!(
                choices_for_runtime(language)
                    .iter()
                    .all(|choice| choice.model.id != fluid.id),
                "Fluid Speech Pro is never recommended ({language})"
            );
        }
    }

    #[test]
    fn parakeet_ultra_is_offered_only_with_its_lab_and_a_saved_choice_survives() {
        let ultra = definition(TranscriptionModelId::ParakeetUltra);
        assert_eq!(ultra.available(), crate::labs::parakeet_ultra());
        assert!(!ultra.streams && !ultra.accepts_language_hint);
        assert_eq!(ultra.languages, PARAKEET_V3_LANGUAGES);
        for language in [AUTO_LANGUAGE, "en", "de"] {
            assert!(
                choices_for_runtime(language)
                    .iter()
                    .all(|choice| choice.model.id != ultra.id),
                "Ultra is never recommended ({language})"
            );
        }
        let saved = TranscriptionSelection {
            model: ultra.id,
            language: "de".into(),
        };
        assert!(validate_saved(&saved).is_ok());
        if !crate::labs::parakeet_ultra() {
            assert!(validate(&saved).is_err());
            assert_eq!(
                super::super::custom_models::runnable(&saved),
                super::super::custom_models::fallback()
            );
        }
        let mut settings = crate::app_settings::AppSettings::default();
        assert!(
            crate::labs::Feature::ParakeetUltra
                .off_refusal(&settings)
                .is_none()
        );
        settings.transcription = saved;
        assert!(
            crate::labs::Feature::ParakeetUltra
                .off_refusal(&settings)
                .is_some()
        );
        assert!(
            crate::labs::Feature::BenchmarkDetails
                .off_refusal(&settings)
                .is_none()
        );
    }

    #[test]
    fn hebrew_recommends_the_ivrit_fine_tune_with_its_hint() {
        use Recommendation::{Recommended, Vocabulary};
        use TranscriptionModelId::{IvritWhisperLargeV3Turbo, WhisperLargeV3Turbo};
        let hebrew: Vec<_> = choices_for_runtime("he")
            .into_iter()
            .map(|choice| (choice.model.id, choice.recommendation))
            .collect();
        assert_eq!(
            hebrew,
            [
                (IvritWhisperLargeV3Turbo, Recommended),
                (WhisperLargeV3Turbo, Vocabulary)
            ]
        );
        let ivrit = definition(IvritWhisperLargeV3Turbo);
        assert_eq!(ivrit.language_choices(), ["he"]);
        assert_eq!(ivrit.runtime_language_hint("he"), Some("he"));
        assert!(!ivrit.supports_language(AUTO_LANGUAGE));
        assert_eq!(language_name("he"), "Hebrew");
    }

    #[test]
    fn an_import_offers_every_language_it_declares_by_name() {
        let import = ModelDefinition {
            languages: Cow::Borrowed(&["en", "nn", "no"]),
            supports_language_detection: true,
            ..(*definition(TranscriptionModelId::WhisperLargeV3Turbo)).clone()
        };
        assert_eq!(import.language_choices(), [AUTO_LANGUAGE, "en", "no", "nn"]);
        assert_eq!(language_name("nn"), "Norwegian Nynorsk");
        assert!(
            !definition(TranscriptionModelId::WhisperLargeV3Turbo)
                .language_choices()
                .contains(&"nn")
        );
    }

    #[test]
    fn norwegian_recommends_nb_whisper_with_its_hint() {
        use Recommendation::{Recommended, Vocabulary};
        use TranscriptionModelId::{NbWhisperMedium, WhisperLargeV3Turbo};
        let norwegian: Vec<_> = choices_for_runtime("no")
            .into_iter()
            .map(|choice| (choice.model.id, choice.recommendation))
            .collect();
        assert_eq!(
            norwegian,
            [
                (NbWhisperMedium, Recommended),
                (WhisperLargeV3Turbo, Vocabulary)
            ]
        );
        let nb = definition(NbWhisperMedium);
        assert_eq!(nb.language_choices(), ["no"]);
        assert_eq!(nb.runtime_language_hint("no"), Some("no"));
        assert!(!nb.supports_language(AUTO_LANGUAGE));
        assert!(
            definition(WhisperLargeV3Turbo)
                .language_choices()
                .contains(&"no")
        );
        assert_eq!(language_name("no"), "Norwegian");
    }

    #[test]
    fn swedish_recommends_kb_whisper_with_its_hint() {
        use Recommendation::{Fastest, Recommended, Vocabulary};
        use TranscriptionModelId::{KbWhisperMedium, ParakeetV3, WhisperLargeV3Turbo};
        let swedish: Vec<_> = choices_for_runtime("sv")
            .into_iter()
            .map(|choice| (choice.model.id, choice.recommendation))
            .collect();
        assert_eq!(
            swedish,
            [
                (KbWhisperMedium, Recommended),
                (ParakeetV3, Fastest),
                (WhisperLargeV3Turbo, Vocabulary)
            ]
        );
        let kb = definition(KbWhisperMedium);
        assert_eq!(kb.language_choices(), ["sv"]);
        assert_eq!(kb.runtime_language_hint("sv"), Some("sv"));
        assert!(!kb.supports_language(AUTO_LANGUAGE));
        assert!(kb.languages.iter().all(|code| *code != "no"));
        assert!(definition(ParakeetV3).supports_language("sv"));
        assert!(
            definition(WhisperLargeV3Turbo)
                .language_choices()
                .contains(&"sv")
        );
        assert_eq!(language_name("sv"), "Swedish");
    }

    #[test]
    fn russian_recommends_gigaam_without_a_hint() {
        use Recommendation::{Recommended, Vocabulary};
        use TranscriptionModelId::{GigaAmV3, WhisperLargeV3Turbo};
        let russian: Vec<_> = choices_for_runtime("ru")
            .into_iter()
            .map(|choice| (choice.model.id, choice.recommendation))
            .collect();
        assert_eq!(
            russian,
            [(GigaAmV3, Recommended), (WhisperLargeV3Turbo, Vocabulary)]
        );
        let gigaam = definition(GigaAmV3);
        assert_eq!(gigaam.language_choices(), ["ru"]);
        assert_eq!(gigaam.runtime_language_hint("ru"), None);
        assert_eq!(gigaam.vocabulary_use(), VocabularyUse::None);
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
    fn whisper_accepts_its_languages_beyond_the_settings_list() {
        for language in ["ta", "te"] {
            let whisper = TranscriptionSelection {
                model: TranscriptionModelId::WhisperLargeV3Turbo,
                language: language.into(),
            };
            assert!(validate(&whisper).is_ok(), "{language}");
            let qwen = TranscriptionSelection {
                model: TranscriptionModelId::Qwen3Asr06B,
                language: language.into(),
            };
            assert!(validate(&qwen).is_err(), "{language}");
        }
        let unknown = TranscriptionSelection {
            model: TranscriptionModelId::WhisperLargeV3Turbo,
            language: "xx".into(),
        };
        assert!(validate(&unknown).is_err());
    }

    #[test]
    fn only_figures_on_the_same_metric_language_and_dataset_compare() {
        use crate::transcription::model_benchmarks::ErrorMetric;
        let qwen = benchmark(&definition(TranscriptionModelId::Qwen3Asr06B)).unwrap();
        let sensevoice = benchmark(&definition(TranscriptionModelId::SenseVoiceSmall)).unwrap();
        assert_eq!(qwen.rate_label(), "7.57% CER");
        assert_eq!(sensevoice.rate_label(), "10.12% CER");
        assert!(qwen.comparable(&sensevoice));
        assert_eq!(sensevoice.source_revision, Some("7045d1c"));
        let parakeet = benchmark(&definition(TranscriptionModelId::ParakeetV2)).unwrap();
        let cohere = benchmark(&definition(TranscriptionModelId::CohereTranscribe)).unwrap();
        assert!(
            !parakeet.comparable(&cohere),
            "an unknown dataset compares with nothing"
        );
        assert!(!qwen.comparable(&parakeet));
        assert!(!qwen.comparable(&BenchmarkRecord {
            language: "en",
            ..qwen
        }));
        assert!(!qwen.comparable(&BenchmarkRecord {
            metric: ErrorMetric::Wer,
            ..qwen
        }));
        assert!(!most_accurate_is_comparable(
            &definition(TranscriptionModelId::CohereTranscribe),
            "en"
        ));
        assert!(benchmark(&definition(TranscriptionModelId::AppleSpeech)).is_none());
        // The catalog's legacy figures stay for the picker without the Lab.
        assert_eq!(
            definition(TranscriptionModelId::Qwen3Asr06B).word_error_rate,
            Some(7.64)
        );
    }

    #[test]
    fn gguf_artifacts_are_q8_0_for_the_vulkan_shader_subset() {
        // Q8_0 GGUFs hold only f32, f16, and q8_0 tensors, and the National
        // Libraries' Whisper files (NB-Whisper, KB-Whisper) q5_0 too: the
        // types the Vulkan backend is built for (`CMAKE_ARGS` in
        // .cargo/config.toml). Another quantization needs its type added
        // there, or Vulkan aborts.
        for model in MODELS {
            if let ModelRuntime::Gguf(artifact) = &model.runtime {
                assert!(
                    artifact.filename.ends_with("-Q8_0.gguf")
                        || artifact.filename == "ggml-model-q5_0.bin",
                    "{} needs its tensor types in GGML_VULKAN_SHADER_TYPES",
                    artifact.filename
                );
            }
        }
    }

    #[test]
    fn no_two_catalog_artifacts_share_a_file_in_the_models_folder() {
        // The folder is flat and `is_installed` compares bytes alone: two
        // artifacts stored under one name would each claim the other's file,
        // fail its checksum, and delete it. KB-Whisper Medium therefore
        // stores its copy as a different name than NB-Whisper's, which
        // publishes the same `ggml-model-q5_0.bin` at the same size.
        let mut stored = std::collections::HashMap::new();
        let artifacts = MODELS
            .iter()
            .filter_map(|model| match &model.runtime {
                ModelRuntime::Gguf(artifact) => Some(artifact.as_ref()),
                ModelRuntime::AppleSpeech => None,
            })
            .chain([&SPEECH_DETECTOR_ARTIFACT]);
        for artifact in artifacts {
            if let Some(other) = stored.insert(artifact.local_name(), artifact.filename.as_ref()) {
                panic!(
                    "{} and {} would both install as {}",
                    artifact.filename,
                    other,
                    artifact.local_name()
                );
            }
        }
        let kb = definition(TranscriptionModelId::KbWhisperMedium);
        let nb = definition(TranscriptionModelId::NbWhisperMedium);
        let (kb, nb) = (&kb, &nb);
        let (ModelRuntime::Gguf(kb), ModelRuntime::Gguf(nb)) = (&kb.runtime, &nb.runtime) else {
            unreachable!("both Whisper fine-tunes are GGUF models")
        };
        assert_eq!(kb.filename, nb.filename);
        assert_eq!(kb.bytes, nb.bytes);
        assert_eq!(kb.local_name(), "kb-whisper-medium-q5_0.bin");
        // Every model keeps the name it was installed under before the field.
        assert_eq!(nb.local_name(), &*nb.filename);
    }

    #[test]
    fn only_the_streaming_parakeet_artifacts_are_marked_streaming() {
        // transcribe.cpp advertises streaming for Parakeet's chunked-attention
        // (Unified English) and cache-aware (Nemotron 3.5) variants; Whisper,
        // Qwen3-ASR, SenseVoice, Cohere, and the offline TDT checkpoints
        // transcribe whole clips.
        let streaming = MODELS
            .iter()
            .filter(|model| model.streams)
            .map(|model| match &model.runtime {
                ModelRuntime::Gguf(artifact) => artifact.variant.to_string(),
                ModelRuntime::AppleSpeech => panic!("a streaming model has a GGUF artifact"),
            })
            .collect::<Vec<_>>();
        assert_eq!(
            streaming,
            ["unified-en-0.6b", "nemotron-3.5-asr-streaming-0.6b"]
        );
    }

    #[test]
    fn nemotron_takes_its_languages_as_locales_and_detects_with_auto() {
        let nemotron = definition(TranscriptionModelId::Nemotron35Asr);
        assert_eq!(nemotron.runtime_language_hint("de"), Some("de-DE"));
        assert_eq!(nemotron.runtime_language_hint("zh"), Some("zh-CN"));
        assert_eq!(nemotron.runtime_language_hint(AUTO_LANGUAGE), None);
        assert!(nemotron.supports_language(AUTO_LANGUAGE));
        // Every language it offers has its locale.
        for language in nemotron.languages.iter().copied() {
            assert!(
                crate::transcription::artifacts::NEMOTRON_3_5_LOCALES
                    .iter()
                    .any(|(code, _)| *code == language),
                "{language}"
            );
        }
        assert!(nemotron.languages.iter().all(|code| {
            super::super::languages::LANGUAGES
                .iter()
                .any(|(known, _)| known == code)
        }));
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
            (TranscriptionModelId::Orukeet, "orukeet"),
            (TranscriptionModelId::FluidSpeechPro, "fluid_speech_pro"),
            (
                TranscriptionModelId::WhisperLargeV3Turbo,
                "whisper_large_v3_turbo",
            ),
            (TranscriptionModelId::Qwen3Asr06B, "qwen3_asr06_b"),
            (TranscriptionModelId::QwenCleoAsr17B, "qwencleo_asr_1_7b"),
            (
                TranscriptionModelId::IvritWhisperLargeV3Turbo,
                "ivrit_whisper_large_v3_turbo",
            ),
            (TranscriptionModelId::SenseVoiceSmall, "sense_voice_small"),
            (TranscriptionModelId::CohereTranscribe, "cohere_transcribe"),
            (
                TranscriptionModelId::UnderdogBark3Pro,
                "underdog_bark_3_pro",
            ),
            (
                TranscriptionModelId::UnderdogBark3Clean,
                "underdog_bark_3_clean",
            ),
            (TranscriptionModelId::ParakeetUltra, "parakeet_ultra"),
            (TranscriptionModelId::Nemotron35Asr, "nemotron_3_5_asr"),
            (TranscriptionModelId::GigaAmV3, "gigaam_v3"),
            (TranscriptionModelId::NbWhisperMedium, "nb_whisper_medium"),
            (TranscriptionModelId::KbWhisperMedium, "kb_whisper_medium"),
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
        assert!(model_path(&apple_speech).is_err());
    }

    #[test]
    fn selections_require_model_language_compatibility() {
        assert!(validate(&TranscriptionSelection::default()).is_ok());
        assert!(
            validate(&TranscriptionSelection {
                model: TranscriptionModelId::ParakeetV2,
                language: "zh".into(),
            })
            .is_err()
        );
        assert!(
            validate(&TranscriptionSelection {
                model: TranscriptionModelId::ParakeetV2,
                language: AUTO_LANGUAGE.into(),
            })
            .is_err()
        );
        assert!(
            validate(&TranscriptionSelection {
                model: TranscriptionModelId::WhisperLargeV3Turbo,
                language: AUTO_LANGUAGE.into(),
            })
            .is_ok()
        );
    }

    #[test]
    fn models_that_detect_language_without_a_hint_offer_auto() {
        for model in [
            TranscriptionModelId::ParakeetV3,
            TranscriptionModelId::GraniteSpeech5,
        ] {
            let definition = definition(model);
            assert!(!definition.accepts_language_hint, "{model:?}");
            assert!(definition.supports_language_detection, "{model:?}");
            assert!(definition.language_choices().contains(&AUTO_LANGUAGE));
            assert_eq!(definition.language_for(AUTO_LANGUAGE), AUTO_LANGUAGE);
        }
        let v2 = definition(TranscriptionModelId::ParakeetV2);
        assert!(!v2.supports_language_detection);
        assert!(!v2.language_choices().contains(&AUTO_LANGUAGE));
        assert_eq!(v2.language_for(AUTO_LANGUAGE), "en");
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
        use Recommendation::{Fastest, MostAccurate, Recommended, Vocabulary};
        use TranscriptionModelId::{
            CohereTranscribe, ParakeetV3, Qwen3Asr06B, SenseVoiceSmall, WhisperLargeV3Turbo,
        };
        assert_eq!(
            routing("el"),
            [
                (CohereTranscribe, Recommended),
                (WhisperLargeV3Turbo, Vocabulary)
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
        for language in ["fi", "da"] {
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
                    (WhisperLargeV3Turbo, Vocabulary)
                ]
            );
        }
        assert_eq!(
            routing("vi"),
            [
                (Qwen3Asr06B, Recommended),
                (CohereTranscribe, MostAccurate),
                (WhisperLargeV3Turbo, Vocabulary)
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
                    Some(Recommendation::Vocabulary)
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
            })
            .is_err()
        );
    }

    #[test]
    fn khmer_is_transcribed_by_whisper_with_its_language_hint() {
        let selection = recommended_selection("km");
        assert_eq!(selection.model, TranscriptionModelId::WhisperLargeV3Turbo);
        assert_eq!(selection.language, "km");
        let model = validate(&selection).unwrap();
        assert_eq!(model.runtime_language_hint("km"), Some("km"));
        assert!(model.language_choices().contains(&"km"));
        let choices = choices_for_runtime("km");
        assert_eq!(choices.len(), 1);
        assert_eq!(choices[0].recommendation, Recommendation::Recommended);
        assert!(
            validate(&TranscriptionSelection {
                model: TranscriptionModelId::Qwen3Asr06B,
                language: "km".into(),
            })
            .is_err()
        );
    }

    #[test]
    fn azerbaijani_is_transcribed_by_whisper_with_its_language_hint() {
        let selection = recommended_selection("az");
        assert_eq!(selection.model, TranscriptionModelId::WhisperLargeV3Turbo);
        assert_eq!(selection.language, "az");
        let model = validate(&selection).unwrap();
        assert_eq!(model.runtime_language_hint("az"), Some("az"));
        assert!(model.language_choices().contains(&"az"));
        let choices = choices_for_runtime("az");
        assert_eq!(choices.len(), 1);
        assert_eq!(choices[0].recommendation, Recommendation::Recommended);
        assert!(
            validate(&TranscriptionSelection {
                model: TranscriptionModelId::Qwen3Asr06B,
                language: "az".into(),
            })
            .is_err()
        );
    }

    #[test]
    fn catalan_is_transcribed_by_whisper_with_its_language_hint() {
        let selection = recommended_selection("ca");
        assert_eq!(selection.model, TranscriptionModelId::WhisperLargeV3Turbo);
        assert_eq!(selection.language, "ca");
        let model = validate(&selection).unwrap();
        assert_eq!(model.runtime_language_hint("ca"), Some("ca"));
        assert!(model.language_choices().contains(&"ca"));
        let choices = choices_for_runtime("ca");
        assert_eq!(choices.len(), 1);
        assert_eq!(choices[0].recommendation, Recommendation::Recommended);
        assert!(
            validate(&TranscriptionSelection {
                model: TranscriptionModelId::Qwen3Asr06B,
                language: "ca".into(),
            })
            .is_err()
        );
    }

    #[test]
    fn whisper_and_qwen3_asr_read_the_vocabulary_as_a_prompt_and_parakeet_biases() {
        use TranscriptionModelId::*;
        let by = |kind| {
            MODELS
                .iter()
                .filter(|model| model.vocabulary_use() == kind)
                .map(|model| model.id)
                .collect::<Vec<_>>()
        };
        assert_eq!(
            by(VocabularyUse::Prompt),
            [
                WhisperLargeV3Turbo,
                Qwen3Asr06B,
                QwenCleoAsr17B,
                IvritWhisperLargeV3Turbo,
                NbWhisperMedium,
                KbWhisperMedium
            ]
        );
        assert_eq!(
            by(VocabularyUse::Bias),
            [
                ParakeetUnifiedEnglish,
                ParakeetV2,
                ParakeetV3,
                Orukeet,
                FluidSpeechPro,
                UnderdogBark3Pro,
                UnderdogBark3Clean,
                ParakeetUltra,
                Nemotron35Asr,
            ]
        );
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
        let ModelRuntime::Gguf(artifact) = &fixture_model().runtime else {
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
    fn readiness_needs_the_pinned_files_verification_receipt() {
        let model = fixture_model();
        let directory = std::env::temp_dir().join(format!(
            "hex-model-ready-{}-{}",
            std::process::id(),
            NEXT_TEMP_FILE.fetch_add(1, Ordering::Relaxed)
        ));
        fs::create_dir(&directory).unwrap();
        set_test_models_dir(Some(directory.clone()));
        let path = directory.join("fixture.gguf");
        fs::write(&path, b"hello").unwrap();
        // A file at the pinned size is installed, but not ready until Hex
        // has checked it once and written the receipt.
        assert!(is_installed(&model, "en"));
        assert!(!is_verified(&model));
        assert!(!is_ready(&model, "en"));
        write_verification_receipt(&path, &model).unwrap();
        assert!(is_ready(&model, "en"));
        set_test_models_dir(None);
        fs::remove_dir_all(directory).unwrap();
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
        let requests = crate::transcription::speech_detector::download_requests_for_test();
        let downloaded = download_with_progress(
            &fixture_model(),
            &AtomicBool::new(false),
            &AtomicU64::new(0),
        );
        TEST_MODELS_DIR.with(|models| *models.borrow_mut() = None);
        let detector_touched = directory
            .join(SPEECH_DETECTOR_ARTIFACT.local_name())
            .exists()
            || directory
                .join(SPEECH_DETECTOR_ARTIFACT.local_name())
                .with_extension("gguf.partial")
                .exists();
        fs::remove_dir_all(&directory).unwrap();
        assert_eq!(downloaded.unwrap(), directory.join("fixture.gguf"));
        assert_eq!(
            crate::transcription::speech_detector::download_requests_for_test(),
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
        let download = lock_downloads(&directory, &AtomicBool::new(false), || {}).unwrap();
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

    #[test]
    fn waiting_for_another_processs_download_reports_its_partial_bytes() {
        use std::sync::Arc;

        /// Holds the lock only until the worker is told to stop: a panic
        /// anywhere in the test cancels it before the lock can be released,
        /// so it never reaches the real download.
        struct Held {
            canceled: Arc<AtomicBool>,
            lock: Option<File>,
        }

        impl Drop for Held {
            fn drop(&mut self) {
                self.canceled.store(true, Ordering::Relaxed);
                drop(self.lock.take());
            }
        }

        let directory = std::env::temp_dir().join(format!(
            "hex-external-download-{}-{}",
            std::process::id(),
            NEXT_TEMP_FILE.fetch_add(1, Ordering::Relaxed),
        ));
        fs::create_dir(&directory).unwrap();
        let partial = directory
            .join("fixture.gguf")
            .with_extension("gguf.partial");
        fs::write(&partial, b"hel").unwrap();
        let canceled = Arc::new(AtomicBool::new(false));
        // Another process (`hex model install` in a terminal) owns the lock
        // and is still writing the partial file.
        let held = Held {
            canceled: canceled.clone(),
            lock: Some(lock_downloads(&directory, &AtomicBool::new(false), || {}).unwrap()),
        };

        let worker_canceled = canceled.clone();
        let downloaded = Arc::new(AtomicU64::new(0));
        let worker_downloaded = downloaded.clone();
        let worker_directory = directory.clone();
        let worker = std::thread::spawn(move || {
            // The fixture's models folder is thread-local.
            set_test_models_dir(Some(worker_directory));
            download_artifact(
                &FIXTURE_ARTIFACT,
                "the fixture",
                &worker_canceled,
                &worker_downloaded,
                &AtomicU8::new(ModelPreparationStage::Downloading as u8),
            )
        });
        let deadline = std::time::Instant::now() + Duration::from_secs(5);
        while downloaded.load(Ordering::Relaxed) != 3 {
            assert!(
                std::time::Instant::now() < deadline,
                "waiting never reported the other process's bytes"
            );
            std::thread::sleep(Duration::from_millis(10));
        }
        // Canceled while still waiting: it never fetched a piece or promoted
        // a file, so the test never touches the network.
        canceled.store(true, Ordering::Relaxed);
        let result = worker.join().unwrap();
        drop(held);
        let error = result.unwrap_err().to_string();
        assert!(error.contains("canceled"), "{error}");
        assert!(!directory.join("fixture.gguf").exists());
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
        assert_eq!(max_audio_chunk_samples("parakeet", 60_000), Some(960_000));
    }

    #[test]
    fn whisper_chunks_at_its_thirty_second_window() {
        assert_eq!(max_audio_chunk_samples("whisper", 0), Some(480_000));
        assert_eq!(max_audio_chunk_samples("whisper", 30_000), Some(480_000));
        assert_eq!(max_audio_chunk_samples("whisper", 45_000), Some(480_000));
        assert_eq!(max_audio_chunk_samples("whisper", 20_000), Some(320_000));
    }
}
