//! Labs › Use your own speech model: GGUF files the person imports from
//! disk, beside the pinned catalog. An import is checked from its header
//! alone (architecture, variant, languages, and tensor types every build runs
//! on Metal and Vulkan) before transcribe.cpp ever loads it, then copied into
//! `models/custom/` under its SHA-256, with a receipt of its size and digest
//! and a manifest of what its metadata declared. Its ID (`custom-` and the
//! digest's first 16 hex digits) is stable across launches and machines.
//!
//! The digest identifies the file Hex copied; it is not a publisher's
//! checksum and Hex vouches for nothing in it. A saved choice of an import
//! that is missing, changed since it was imported, or behind its Labs toggle
//! stays saved, and the listener runs the default model until it is fixed
//! ([`runnable`]).

use std::collections::HashMap;
use std::fs::{self, File};
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{LazyLock, Mutex, MutexGuard};
use std::time::{SystemTime, UNIX_EPOCH};

use color_eyre::eyre::{Result, WrapErr, bail, eyre};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

use super::gguf_header::{self, GgufHeader};
use super::models::{
    GgufArtifact, LANGUAGES, ModelDefinition, ModelRuntime, TranscriptionModelId,
    TranscriptionSelection, is_known_language, known_language, models_dir,
};

const PREFIX: &str = "custom-";
const DIGITS: usize = 16;
const MAX_NAME_CHARS: usize = 64;

/// The model families Hex's runtime handles, with the names people know.
const ARCHITECTURES: [(&str, &str); 5] = [
    ("parakeet", "Parakeet"),
    ("whisper", "Whisper"),
    ("qwen3_asr", "Qwen3-ASR"),
    ("cohere_asr", "Cohere Transcribe"),
    ("sensevoice", "SenseVoice"),
];

/// An imported model's stable ID: `custom-` and the first 16 hex digits of
/// the file's SHA-256.
#[derive(Clone, Copy, Eq, Hash, PartialEq)]
pub struct CustomModelId([u8; PREFIX.len() + DIGITS]);

impl CustomModelId {
    fn from_sha256(sha256: &str) -> Self {
        Self::parse(&format!("{PREFIX}{}", &sha256[..DIGITS])).expect("a SHA-256 is hex")
    }

    pub fn parse(text: &str) -> Option<Self> {
        let digits = text.strip_prefix(PREFIX)?;
        (digits.len() == DIGITS && digits.bytes().all(|byte| matches!(byte, b'0'..=b'9' | b'a'..=b'f')))
            .then(|| {
                let mut id = [0_u8; PREFIX.len() + DIGITS];
                id.copy_from_slice(text.as_bytes());
                Self(id)
            })
    }

    pub fn as_str(&self) -> &str {
        std::str::from_utf8(&self.0).expect("a custom model ID is ASCII")
    }

    fn digits(&self) -> &str {
        &self.as_str()[PREFIX.len()..]
    }
}

impl std::fmt::Debug for CustomModelId {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str(self.as_str())
    }
}

/// What the import's metadata declared, saved beside the file.
#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
struct Manifest {
    /// The name the picker shows; starts as `metadata_name`, editable.
    name: String,
    metadata_name: String,
    architecture: String,
    variant: String,
    languages: Vec<String>,
    detects_language: bool,
    bytes: u64,
    sha256: String,
    /// The imported file's name, for provenance; the original stays where it was.
    source: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    license: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    repository: Option<String>,
    imported_at: u64,
}

/// The receipt every model file has: the size and digest Hex checked, and
/// the modification time that proves the file is still that one.
#[derive(Deserialize, Serialize)]
struct Receipt {
    bytes: u64,
    sha256: String,
    modified_ns: u64,
}

/// Whether an imported model can run.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Status {
    Ready,
    /// Its file or manifest is gone.
    Missing,
    /// Its file is not the one imported (size or modification time).
    Changed,
    /// Labs › Use your own speech model is off.
    Off,
}

/// One imported model, as the picker, `hex model list`, and the local API
/// show it.
#[derive(Clone)]
pub struct Imported {
    pub definition: &'static ModelDefinition,
    pub architecture: &'static str,
    pub sha256: String,
    pub source: String,
    pub license: Option<String>,
    pub repository: Option<String>,
}

struct Entry {
    id: CustomModelId,
    manifest: Manifest,
    definition: &'static ModelDefinition,
    receipt: Option<(u64, u64)>,
}

#[derive(Default)]
struct Registry {
    /// The folder and modification time the entries were read at.
    stamp: Option<(PathBuf, Option<SystemTime>)>,
    entries: Vec<Entry>,
    /// Definitions built so far, by ID and name: a rename or re-import
    /// builds one, and every other read reuses it.
    built: HashMap<(CustomModelId, String), &'static ModelDefinition>,
    placeholders: HashMap<CustomModelId, &'static ModelDefinition>,
}

static REGISTRY: LazyLock<Mutex<Registry>> = LazyLock::new(Mutex::default);

fn registry() -> MutexGuard<'static, Registry> {
    let mut registry = REGISTRY.lock().unwrap_or_else(|error| error.into_inner());
    registry.refresh();
    registry
}

fn invalidate() {
    REGISTRY
        .lock()
        .unwrap_or_else(|error| error.into_inner())
        .stamp = None;
}

pub fn directory() -> Result<PathBuf> {
    Ok(models_dir()?.join("custom"))
}

impl Registry {
    fn refresh(&mut self) {
        let Ok(directory) = directory() else {
            self.entries.clear();
            return;
        };
        let modified = fs::metadata(&directory)
            .and_then(|metadata| metadata.modified())
            .ok();
        if self.stamp.as_ref() == Some(&(directory.clone(), modified)) {
            return;
        }
        let mut entries = Vec::new();
        if let Ok(files) = fs::read_dir(&directory) {
            for file in files.flatten() {
                let path = file.path();
                let Some(id) = path
                    .file_stem()
                    .and_then(|stem| stem.to_str())
                    .filter(|_| path.extension().is_some_and(|extension| extension == "json"))
                    .and_then(|stem| CustomModelId::parse(&format!("{PREFIX}{stem}")))
                else {
                    continue;
                };
                let manifest = fs::read(&path)
                    .ok()
                    .and_then(|data| serde_json::from_slice::<Manifest>(&data).ok());
                let Some(manifest) = manifest else {
                    tracing::warn!(path = %path.display(), "ignoring an unreadable imported model manifest");
                    continue;
                };
                let definition = *self
                    .built
                    .entry((id, manifest.name.clone()))
                    .or_insert_with(|| build(id, &manifest));
                let receipt = fs::read(receipt_path(&model_file(&directory, id)))
                    .ok()
                    .and_then(|data| serde_json::from_slice::<Receipt>(&data).ok())
                    .filter(|receipt| receipt.sha256 == manifest.sha256)
                    .map(|receipt| (receipt.bytes, receipt.modified_ns));
                entries.push(Entry {
                    id,
                    manifest,
                    definition,
                    receipt,
                });
            }
        }
        entries.sort_by(|left, right| {
            left.manifest
                .name
                .to_lowercase()
                .cmp(&right.manifest.name.to_lowercase())
                .then(left.id.as_str().cmp(right.id.as_str()))
        });
        self.entries = entries;
        self.stamp = Some((directory, modified));
    }

    fn entry(&self, id: CustomModelId) -> Option<&Entry> {
        self.entries.iter().find(|entry| entry.id == id)
    }
}

fn model_file(directory: &Path, id: CustomModelId) -> PathBuf {
    directory.join(format!("{}.gguf", id.digits()))
}

fn manifest_file(directory: &Path, id: CustomModelId) -> PathBuf {
    directory.join(format!("{}.json", id.digits()))
}

fn receipt_path(model: &Path) -> PathBuf {
    model.with_extension("gguf.verified.json")
}

/// A `'static` definition for `manifest`, as the catalog's are. Each is
/// built once per ID and name, so the few bytes it keeps are bounded by the
/// imports and renames a person makes.
fn build(id: CustomModelId, manifest: &Manifest) -> &'static ModelDefinition {
    let leak = |text: &str| -> &'static str { Box::leak(text.to_owned().into_boxed_str()) };
    let languages: Vec<&'static str> = manifest
        .languages
        .iter()
        .filter_map(|code| known_language(code))
        .collect();
    let languages: &'static [&'static str] = if languages.is_empty() {
        &["en"]
    } else {
        Box::leak(languages.into_boxed_slice())
    };
    let architecture = ARCHITECTURES
        .iter()
        .find(|(name, _)| *name == manifest.architecture)
        .map_or_else(|| leak(&manifest.architecture), |(name, _)| *name);
    let artifact: &'static GgufArtifact = Box::leak(Box::new(GgufArtifact {
        filename: leak(&format!("custom/{}.gguf", id.digits())),
        revision: "",
        repository: "",
        bytes: manifest.bytes,
        sha256: leak(&manifest.sha256),
        architecture,
        variant: leak(&manifest.variant),
    }));
    Box::leak(Box::new(ModelDefinition {
        id: TranscriptionModelId::Custom(id),
        name: leak(&manifest.name),
        realtime: "Unknown",
        word_error_rate: None,
        quality_context: "Not benchmarked",
        coverage: leak(&coverage(languages)),
        timestamps: match architecture {
            "parakeet" => "Token timestamps",
            "whisper" => "Segment timestamps",
            _ => "No timestamps",
        },
        streams: false,
        runtime: ModelRuntime::Gguf(artifact),
        languages,
        // Parakeet heads take no language prompt (Parakeet v3's catalog
        // entry is the same); the other families do.
        accepts_language_hint: architecture != "parakeet",
        supports_language_detection: manifest.detects_language && architecture != "parakeet",
    }))
}

fn coverage(languages: &[&str]) -> String {
    match languages {
        [only] => LANGUAGES
            .iter()
            .find(|(code, _)| code == only)
            .map_or_else(|| (*only).to_owned(), |(_, name)| (*name).to_owned()),
        languages => format!("{} languages", languages.len()),
    }
}

/// The definition for `id`: its manifest's, or a placeholder that is never
/// available when nothing is imported under it (a deleted import, or an ID
/// from another machine's settings).
pub fn definition(id: CustomModelId) -> &'static ModelDefinition {
    let mut registry = registry();
    if let Some(entry) = registry.entry(id) {
        return entry.definition;
    }
    *registry.placeholders.entry(id).or_insert_with(|| {
        build(
            id,
            &Manifest {
                name: "Imported model".into(),
                metadata_name: String::new(),
                architecture: String::new(),
                variant: String::new(),
                languages: vec!["en".into()],
                detects_language: false,
                bytes: 0,
                sha256: "0".repeat(64),
                source: String::new(),
                license: None,
                repository: None,
                imported_at: 0,
            },
        )
    })
}

/// Whether `id` names an import, available or not.
pub fn is_registered(id: CustomModelId) -> bool {
    registry().entry(id).is_some()
}

pub fn status(id: CustomModelId) -> Status {
    if !crate::labs::custom_models() {
        return Status::Off;
    }
    let registry = registry();
    let Some(entry) = registry.entry(id) else {
        return Status::Missing;
    };
    let Ok(directory) = directory() else {
        return Status::Missing;
    };
    let Ok(metadata) = fs::metadata(model_file(&directory, id)) else {
        return Status::Missing;
    };
    let modified = super::models::modified_ns(&metadata);
    match entry.receipt {
        Some((bytes, receipt_modified))
            if bytes == metadata.len()
                && bytes == entry.manifest.bytes
                && modified == Some(receipt_modified) =>
        {
            Status::Ready
        }
        _ => Status::Changed,
    }
}

/// Whether `id` can be selected and run now.
pub fn usable(id: CustomModelId) -> bool {
    status(id) == Status::Ready
}

/// Every import while Labs › Use your own speech model is on, ready or not,
/// by name.
pub fn imported() -> Vec<Imported> {
    if !crate::labs::custom_models() {
        return Vec::new();
    }
    registry()
        .entries
        .iter()
        .map(|entry| Imported {
            definition: entry.definition,
            architecture: match entry.definition.runtime {
                ModelRuntime::Gguf(artifact) => artifact.architecture,
                ModelRuntime::AppleSpeech => "",
            },
            sha256: entry.manifest.sha256.clone(),
            source: entry.manifest.source.clone(),
            license: entry.manifest.license.clone(),
            repository: entry.manifest.repository.clone(),
        })
        .collect()
}

/// The selection the listener runs for `selection`: the same one, unless it
/// names an import that can't run, when it is the default model. The saved
/// choice stays as it is until the person fixes it or picks another.
pub fn runnable(selection: &TranscriptionSelection) -> TranscriptionSelection {
    match selection.model {
        TranscriptionModelId::Custom(id) if !usable(id) => fallback(),
        _ => selection.clone(),
    }
}

/// The model the listener runs in place of an import that can't.
pub fn fallback() -> TranscriptionSelection {
    super::models::recommended_selection(&crate::desktop::i18n::preferred_language())
}

/// The family name people know for `architecture`.
pub fn family(architecture: &str) -> &str {
    ARCHITECTURES
        .iter()
        .find(|(name, _)| *name == architecture)
        .map_or(architecture, |(_, family)| family)
}

/// What an import declares, once its header passes every check.
#[derive(Debug, PartialEq)]
struct Profile {
    name: String,
    architecture: String,
    variant: String,
    languages: Vec<String>,
    detects_language: bool,
    license: Option<String>,
    repository: Option<String>,
}

/// Checks a header against what Hex can run, with the reason when it can't.
fn profile(header: &GgufHeader, file_name: &str) -> Result<Profile> {
    let Some(architecture) = header.string("general.architecture") else {
        bail!("it has no general.architecture, so Hex can't tell which model family it is");
    };
    if !ARCHITECTURES.iter().any(|(name, _)| *name == architecture) {
        bail!(
            "its architecture is {architecture}; Hex runs Parakeet, Whisper, Qwen3-ASR, \
             Cohere Transcribe, and SenseVoice models"
        );
    }
    let Some(variant) = header.string("stt.variant") else {
        bail!("it has no stt.variant; convert it with transcribe.cpp's converter for {}", family(architecture));
    };
    if header.tensor_types.is_empty() {
        bail!("it has no tensors");
    }
    let unsupported = header.unsupported_tensor_types();
    if !unsupported.is_empty() {
        bail!(
            "it has {} tensors; Hex runs only F32, F16, and Q8_0 weights on Metal and Vulkan. \
             Quantize it to Q8_0 with transcribe-quantize",
            unsupported.join(", ")
        );
    }
    let mut languages: Vec<String> = Vec::new();
    for language in header
        .string_arrays
        .get("general.languages")
        .into_iter()
        .flatten()
    {
        let language = language.trim().to_lowercase();
        if is_known_language(&language) && !languages.contains(&language) {
            languages.push(language);
        }
    }
    if languages.is_empty() {
        bail!("its general.languages names no language Hex knows");
    }
    Ok(Profile {
        name: header
            .string("general.name")
            .map_or_else(|| file_name.to_owned(), |name| clamp_name(name)),
        architecture: architecture.to_owned(),
        variant: variant.to_owned(),
        languages,
        detects_language: header.bool("stt.capability.lang_detect"),
        license: header.string("general.license").map(str::to_owned),
        repository: header.string("general.repo_url").map(str::to_owned),
    })
}

fn clamp_name(name: &str) -> String {
    name.split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
        .chars()
        .take(MAX_NAME_CHARS)
        .collect()
}

/// Checks `path` without copying it: what an import would say, or why it
/// would refuse.
pub fn check(path: &Path) -> Result<String> {
    let header = gguf_header::read(path).map_err(|error| eyre!("{} can't be imported: {error}", display_name(path)))?;
    let profile = profile(&header, &file_stem(path))
        .map_err(|error| eyre!("{} can't be imported: {error}", display_name(path)))?;
    Ok(format!(
        "{} ({}, {}, {} languages)",
        profile.name,
        family(&profile.architecture),
        profile.variant,
        profile.languages.len()
    ))
}

fn display_name(path: &Path) -> String {
    path.file_name()
        .map_or_else(|| path.display().to_string(), |name| name.to_string_lossy().into_owned())
}

fn file_stem(path: &Path) -> String {
    path.file_stem()
        .map_or_else(|| "Imported model".into(), |stem| stem.to_string_lossy().into_owned())
}

/// Why an import is refused while its Labs toggle is off.
pub const OFF: &str = "Turn on Labs › Use your own speech model first: hex labs on custom-models";

/// Imports the GGUF at `source`: checks its header, copies it into
/// `models/custom/` while hashing it, checks the copy again, and records it.
/// The original is never changed or removed. Importing the same file again
/// keeps the first import (renamed to `name` when given). On failure or
/// cancellation only this import's partial copy is removed. Callers check
/// the Labs toggle ([`OFF`]).
pub fn import(source: &Path, name: Option<&str>, canceled: &AtomicBool) -> Result<CustomModelId> {
    check(source)?;
    let directory = directory()?;
    fs::create_dir_all(&directory)?;
    let partial = directory.join(format!(
        ".import-{}-{}.partial",
        std::process::id(),
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map_or(0, |elapsed| elapsed.as_nanos())
    ));
    let result = copy_and_record(source, &partial, &directory, name, canceled);
    if result.is_err() {
        let _ = fs::remove_file(&partial);
    }
    invalidate();
    result
}

fn copy_and_record(
    source: &Path,
    partial: &Path,
    directory: &Path,
    name: Option<&str>,
    canceled: &AtomicBool,
) -> Result<CustomModelId> {
    let (bytes, sha256) = copy_hashing(source, partial, canceled)
        .wrap_err_with(|| format!("could not copy {}", display_name(source)))?;
    // The copy is what runs, so the copy is what is checked.
    let header = gguf_header::read(partial)?;
    let profile = profile(&header, &file_stem(source))
        .map_err(|error| eyre!("{} can't be imported: {error}", display_name(source)))?;
    let id = CustomModelId::from_sha256(&sha256);
    let file = model_file(directory, id);
    let manifest_path = manifest_file(directory, id);
    if let Some(existing) = fs::read(&manifest_path)
        .ok()
        .and_then(|data| serde_json::from_slice::<Manifest>(&data).ok())
        .filter(|manifest| manifest.sha256 == sha256 && file.is_file())
    {
        fs::remove_file(partial)?;
        if let Some(name) = name {
            write_manifest(directory, id, &Manifest {
                name: valid_name(name)?,
                ..existing
            })?;
        }
        return Ok(id);
    }
    crate::durable::sync_file(partial)?;
    fs::rename(partial, &file)?;
    crate::durable::sync_directory(directory)?;
    let modified_ns = fs::metadata(&file)
        .ok()
        .as_ref()
        .and_then(super::models::modified_ns)
        .ok_or_else(|| eyre!("the copy's modification time is unavailable"))?;
    super::models::write_json_atomically(
        &receipt_path(&file),
        &Receipt {
            bytes,
            sha256: sha256.clone(),
            modified_ns,
        },
    )?;
    write_manifest(
        directory,
        id,
        &Manifest {
            name: match name {
                Some(name) => valid_name(name)?,
                None => profile.name.clone(),
            },
            metadata_name: profile.name,
            architecture: profile.architecture,
            variant: profile.variant,
            languages: profile.languages,
            detects_language: profile.detects_language,
            bytes,
            sha256,
            source: display_name(source),
            license: profile.license,
            repository: profile.repository,
            imported_at: SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .map_or(0, |elapsed| elapsed.as_secs()),
        },
    )?;
    tracing::info!(id = id.as_str(), bytes, "imported a speech model");
    Ok(id)
}

fn copy_hashing(source: &Path, destination: &Path, canceled: &AtomicBool) -> Result<(u64, String)> {
    let mut input = File::open(source)?;
    let mut output = File::create(destination)?;
    let mut digest = Sha256::new();
    let mut buffer = vec![0_u8; 1024 * 1024];
    let mut bytes = 0_u64;
    loop {
        if canceled.load(Ordering::Relaxed) {
            bail!("import canceled");
        }
        let read = input.read(&mut buffer)?;
        if read == 0 {
            break;
        }
        digest.update(&buffer[..read]);
        output.write_all(&buffer[..read])?;
        bytes += read as u64;
    }
    output.flush()?;
    let sha256 = digest
        .finalize()
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect();
    Ok((bytes, sha256))
}

fn write_manifest(directory: &Path, id: CustomModelId, manifest: &Manifest) -> Result<()> {
    super::models::write_json_atomically(&manifest_file(directory, id), manifest)?;
    invalidate();
    Ok(())
}

fn valid_name(name: &str) -> Result<String> {
    let name = clamp_name(name);
    if name.is_empty() {
        bail!("an imported model's name can't be empty");
    }
    Ok(name)
}

/// Renames an import; only the picker's name changes.
pub fn rename(id: CustomModelId, name: &str) -> Result<()> {
    let directory = directory()?;
    let manifest = fs::read(manifest_file(&directory, id))
        .ok()
        .and_then(|data| serde_json::from_slice::<Manifest>(&data).ok())
        .ok_or_else(|| eyre!("no imported model is {}", id.as_str()))?;
    write_manifest(&directory, id, &Manifest {
        name: valid_name(name)?,
        ..manifest
    })
}

/// Removes Hex's copy of an import, its receipt, and its manifest. The file
/// it was imported from is not touched.
pub fn remove(id: CustomModelId) -> Result<()> {
    let directory = directory()?;
    super::models::remove_installed_at(&model_file(&directory, id))?;
    match fs::remove_file(manifest_file(&directory, id)) {
        Err(error) if error.kind() != std::io::ErrorKind::NotFound => return Err(error.into()),
        _ => {}
    }
    invalidate();
    tracing::info!(id = id.as_str(), "removed an imported speech model");
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::transcription::gguf_header::tests::gguf;

    fn parakeet(name: &str) -> Vec<u8> {
        gguf(
            &[
                ("general.architecture", "parakeet"),
                ("general.name", name),
                ("stt.variant", "tdt-0.6b-primeline"),
                ("general.license", "cc-by-4.0"),
            ],
            &[("general.languages", &["en", "de", "xx"])],
            &[("stt.capability.lang_detect", true)],
            &[("encoder.weight", 8, 64), ("norm", 0, 16)],
        )
    }

    fn temporary_models_dir() -> PathBuf {
        static NEXT: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
        let directory = std::env::temp_dir().join(format!(
            "hex-custom-models-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        fs::create_dir_all(&directory).unwrap();
        super::super::models::set_test_models_dir(Some(directory.clone()));
        directory
    }

    #[test]
    fn an_import_is_copied_recorded_renamed_and_removed_leaving_the_original() {
        let models = temporary_models_dir();
        let source = models.join("primeline-Q8_0.gguf");
        fs::write(&source, parakeet("Parakeet primeLine")).unwrap();
        let id = import(&source, None, &AtomicBool::new(false)).unwrap();
        assert!(source.is_file(), "the original stays");
        let definition = definition(id);
        assert_eq!(definition.name, "Parakeet primeLine");
        assert_eq!(definition.id, TranscriptionModelId::Custom(id));
        assert_eq!(definition.languages, ["en", "de"]);
        assert!(!definition.accepts_language_hint && !definition.streams);
        assert!(is_registered(id));
        let ModelRuntime::Gguf(artifact) = definition.runtime else {
            unreachable!("imports are GGUF files")
        };
        assert_eq!(artifact.bytes, fs::metadata(&source).unwrap().len());
        assert_eq!(
            artifact.sha256,
            super::super::models::sha256_hex(&source, None).unwrap()
        );
        assert!(super::super::models::model_path(definition).unwrap().is_file());
        // The same file again keeps the one import.
        assert_eq!(import(&source, Some("primeLine"), &AtomicBool::new(false)).unwrap(), id);
        assert_eq!(super::definition(id).name, "primeLine");
        rename(id, "  German  Parakeet ").unwrap();
        assert_eq!(super::definition(id).name, "German Parakeet");
        assert!(rename(id, "  ").is_err());
        remove(id).unwrap();
        assert!(!is_registered(id));
        assert!(source.is_file());
        assert_eq!(super::definition(id).name, "Imported model");
        assert!(!super::definition(id).available());
        super::super::models::set_test_models_dir(None);
        fs::remove_dir_all(models).unwrap();
    }

    #[test]
    fn a_refused_or_canceled_import_leaves_nothing_behind() {
        let models = temporary_models_dir();
        let source = models.join("llama.gguf");
        fs::write(&source, gguf(&[("general.architecture", "llama")], &[], &[], &[("a", 0, 4)])).unwrap();
        let error = import(&source, None, &AtomicBool::new(false)).unwrap_err();
        assert!(error.to_string().starts_with("llama.gguf can't be imported: its architecture is llama"));
        let good = models.join("good.gguf");
        fs::write(&good, parakeet("Good")).unwrap();
        assert!(import(&good, None, &AtomicBool::new(true)).is_err());
        let left: Vec<_> = fs::read_dir(directory().unwrap())
            .map(|files| files.flatten().map(|file| file.file_name()).collect())
            .unwrap_or_default();
        assert!(left.is_empty(), "{left:?}");
        super::super::models::set_test_models_dir(None);
        fs::remove_dir_all(models).unwrap();
    }

    #[test]
    fn a_saved_import_that_cannot_run_falls_back_and_stays_saved() {
        let id = CustomModelId::from_sha256(&"0f".repeat(32));
        let saved = TranscriptionSelection {
            model: TranscriptionModelId::Custom(id),
            language: "de".into(),
        };
        assert_ne!(status(id), Status::Ready);
        assert_eq!(runnable(&saved), fallback());
        let catalog = TranscriptionSelection::default();
        assert_eq!(runnable(&catalog), catalog);
    }

    #[test]
    fn ids_are_the_digest_prefix_and_parse_back() {
        let id = CustomModelId::from_sha256(&"ab12".repeat(16));
        assert_eq!(id.as_str(), "custom-ab12ab12ab12ab12");
        assert_eq!(CustomModelId::parse(id.as_str()), Some(id));
        for bad in ["custom-AB12AB12AB12AB12", "custom-ab12", "parakeet_v3", "custom-ab12ab12ab12ab1z"] {
            assert_eq!(CustomModelId::parse(bad), None, "{bad}");
        }
    }

    #[test]
    fn a_parakeet_header_gives_its_profile() {
        let header = gguf_header::tests::read_header(parakeet("Parakeet primeLine"));
        let profile = profile(&header, "file").unwrap();
        assert_eq!(profile.name, "Parakeet primeLine");
        assert_eq!(profile.architecture, "parakeet");
        assert_eq!(profile.languages, ["en", "de"]);
        assert!(profile.detects_language);
        assert_eq!(profile.license.as_deref(), Some("cc-by-4.0"));
    }

    #[test]
    fn unsupported_files_are_refused_with_a_reason() {
        let reason = |bytes: Vec<u8>| {
            profile(&gguf_header::tests::read_header(bytes), "file")
                .unwrap_err()
                .to_string()
        };
        assert!(reason(gguf(&[("general.architecture", "llama")], &[], &[], &[("a", 0, 4)])).contains("its architecture is llama"));
        assert!(reason(gguf(&[], &[], &[], &[("a", 0, 4)])).contains("no general.architecture"));
        assert!(reason(gguf(&[("general.architecture", "whisper")], &[], &[], &[("a", 0, 4)])).contains("no stt.variant"));
        let quantized = gguf(
            &[("general.architecture", "whisper"), ("stt.variant", "whisper-small")],
            &[("general.languages", &["en"])],
            &[],
            &[("a", 12, 256)],
        );
        assert!(reason(quantized).contains("Q4_K (1) tensors"));
        let no_languages = gguf(
            &[("general.architecture", "whisper"), ("stt.variant", "whisper-small")],
            &[("general.languages", &["xx"])],
            &[],
            &[("a", 0, 4)],
        );
        assert!(reason(no_languages).contains("no language Hex knows"));
    }
}
