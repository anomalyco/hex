//! Speech models already on this computer: GGUF files in the Hugging Face
//! hub cache (`$HF_HUB_CACHE`, else `$HF_HOME/hub`, else
//! `~/.cache/huggingface/hub`). A scan runs only when the model picker or
//! onboarding's model step opens, never in the background, and reads
//! headers alone (`gguf_header`, the same checks as an import). Each file's
//! result is kept per path, size, modification time, and file ID, so an
//! unchanged file is never read twice.
//!
//! Hex never writes or deletes inside the cache. A file whose blob name is a
//! catalog model's pinned SHA-256 (with its size) counts as that model
//! already downloaded, used in place; any other compatible file is offered
//! under "Found on this computer" and, once chosen, is hashed in full once
//! and registered as an in-place import (`custom_models`).

use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::atomic::AtomicBool;
use std::sync::{LazyLock, Mutex};
use std::time::SystemTime;

use color_eyre::eyre::{Result, bail, eyre};
use serde::{Deserialize, Serialize};

use super::custom_models::Receipt;
use super::models::{MODELS, ModelDefinition, ModelRuntime, TranscriptionModelId, models_dir};

/// One compatible GGUF found in the cache.
#[derive(Clone, Debug, PartialEq)]
pub struct Found {
    /// The snapshot path, which Hex reads in place.
    pub path: PathBuf,
    /// `owner/name` from the cache folder (`models--owner--name`).
    pub repository: String,
    /// The snapshot folder: the commit the file was downloaded at.
    pub revision: String,
    pub file_name: String,
    pub bytes: u64,
    /// The blob's name, which the hub cache gives the file's SHA-256; `None`
    /// when the snapshot holds a plain copy (Windows without symlinks).
    pub blob_sha256: Option<String>,
    /// The catalog model this file is, by its pinned SHA-256 and size (or
    /// its repository, file name, and size for a plain copy, checked in full
    /// when chosen).
    pub catalog: Option<TranscriptionModelId>,
    /// `general.name`, else the file name.
    pub name: String,
    pub architecture: String,
    pub variant: String,
    pub languages: Vec<String>,
    /// The quantization most of its weights use: Q8_0, F16, or F32.
    pub compression: &'static str,
    /// The tensor data it loads, which estimates the memory it needs.
    pub tensor_bytes: u64,
    /// Whether its weights likely won't fit this computer's GPU memory.
    pub too_big: bool,
}

impl Found {
    /// The commit, shortened as the hub shows it.
    pub fn short_revision(&self) -> &str {
        &self.revision[..self.revision.len().min(7)]
    }

    /// `owner/name@abcdef1`.
    pub fn source(&self) -> String {
        format!("{}@{}", self.repository, self.short_revision())
    }
}

/// The hub cache folder this computer uses, when it exists.
pub fn hub_dir() -> Option<PathBuf> {
    let from_env = |name: &str| {
        std::env::var_os(name)
            .filter(|value| !value.is_empty())
            .map(PathBuf::from)
    };
    let directory = from_env("HF_HUB_CACHE")
        .or_else(|| from_env("HF_HOME").map(|home| home.join("hub")))
        .or_else(|| {
            dirs::home_dir().map(|home| home.join(".cache").join("huggingface").join("hub"))
        })?;
    directory.is_dir().then_some(directory)
}

/// What one file's header said, keyed by what proves it unchanged.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
struct Stamp {
    bytes: u64,
    modified: Option<SystemTime>,
    file_id: Option<u64>,
}

#[derive(Default)]
struct Cache {
    files: HashMap<PathBuf, (Stamp, Option<Found>)>,
    last: Vec<Found>,
}

static CACHE: LazyLock<Mutex<Cache>> = LazyLock::new(Mutex::default);

/// The models the last scan found, without scanning.
pub fn last() -> Vec<Found> {
    CACHE
        .lock()
        .unwrap_or_else(|error| error.into_inner())
        .last
        .clone()
}

/// Scans the hub cache now: headers only, and only files that changed since
/// they were last read. Exact catalog matches come first.
pub fn scan() -> Vec<Found> {
    let mut found = hub_dir().map(|hub| scan_in(&hub)).unwrap_or_default();
    for model in &mut found {
        model.too_big = likely_too_big(model.tensor_bytes);
    }
    CACHE
        .lock()
        .unwrap_or_else(|error| error.into_inner())
        .last
        .clone_from(&found);
    found
}

fn scan_in(hub: &Path) -> Vec<Found> {
    let mut found = Vec::new();
    let Ok(repositories) = std::fs::read_dir(hub) else {
        return found;
    };
    for repository in repositories.flatten() {
        let folder = repository.file_name().to_string_lossy().into_owned();
        let Some(name) = folder.strip_prefix("models--") else {
            continue;
        };
        let repository_name = name.replacen("--", "/", 1);
        let Ok(snapshots) = std::fs::read_dir(repository.path().join("snapshots")) else {
            continue;
        };
        for snapshot in snapshots.flatten() {
            let revision = snapshot.file_name().to_string_lossy().into_owned();
            let Ok(files) = std::fs::read_dir(snapshot.path()) else {
                continue;
            };
            for file in files.flatten() {
                let path = file.path();
                if path.extension().is_none_or(|extension| extension != "gguf") {
                    continue;
                }
                if let Some(model) = read(&path, &repository_name, &revision) {
                    found.push(model);
                }
            }
        }
    }
    found.sort_by(|left, right| {
        left.catalog
            .is_none()
            .cmp(&right.catalog.is_none())
            .then_with(|| left.name.to_lowercase().cmp(&right.name.to_lowercase()))
            .then_with(|| left.path.cmp(&right.path))
    });
    found
}

fn stamp(path: &Path) -> Option<Stamp> {
    let metadata = std::fs::metadata(path).ok()?;
    Some(Stamp {
        bytes: metadata.len(),
        modified: metadata.modified().ok(),
        file_id: file_id(&metadata),
    })
}

/// The file's identity on its volume: the inode on Unix; Windows reports
/// none through stable std, so size and modification time stand alone there.
pub(crate) fn file_id(metadata: &std::fs::Metadata) -> Option<u64> {
    #[cfg(unix)]
    {
        use std::os::unix::fs::MetadataExt;
        Some(metadata.ino() ^ metadata.dev().rotate_left(32))
    }
    #[cfg(not(unix))]
    {
        let _ = metadata;
        None
    }
}

/// One file's result, from the cache while its stamp holds.
fn read(path: &Path, repository: &str, revision: &str) -> Option<Found> {
    let stamp = stamp(path)?;
    let cache = CACHE.lock().unwrap_or_else(|error| error.into_inner());
    if let Some((cached, found)) = cache.files.get(path)
        && *cached == stamp
    {
        return found.clone();
    }
    drop(cache);
    let found = inspect(path, repository, revision, stamp.bytes);
    CACHE
        .lock()
        .unwrap_or_else(|error| error.into_inner())
        .files
        .insert(path.to_path_buf(), (stamp, found.clone()));
    found
}

fn inspect(path: &Path, repository: &str, revision: &str, bytes: u64) -> Option<Found> {
    let file_name = path.file_name()?.to_string_lossy().into_owned();
    let blob_sha256 = std::fs::read_link(path)
        .ok()
        .and_then(|target| {
            target
                .file_name()
                .map(|name| name.to_string_lossy().into_owned())
        })
        .filter(|name| name.len() == 64 && name.bytes().all(|byte| byte.is_ascii_hexdigit()))
        .map(|name| name.to_lowercase());
    let catalog = catalog_match(blob_sha256.as_deref(), repository, &file_name, bytes);
    // The pinned digest is the proof: an exact match needs no header read.
    if let (Some(model), Some(_)) = (catalog, &blob_sha256) {
        return Some(catalog_found(
            path,
            repository,
            revision,
            file_name,
            bytes,
            blob_sha256,
            model,
        ));
    }
    let header = match super::gguf_header::read(path) {
        Ok(header) => header,
        Err(error) => {
            tracing::debug!(path = %path.display(), %error, "skipping a cached file Hex can't read");
            return None;
        }
    };
    let profile = match super::custom_models::profile_of(&header, &file_name) {
        Ok(profile) => profile,
        Err(error) => {
            tracing::debug!(path = %path.display(), %error, "skipping a cached model Hex can't run");
            return None;
        }
    };
    Some(Found {
        path: path.to_path_buf(),
        repository: repository.to_owned(),
        revision: revision.to_owned(),
        file_name,
        bytes,
        blob_sha256,
        catalog,
        name: profile.name,
        architecture: profile.architecture,
        variant: profile.variant,
        languages: profile.languages,
        compression: compression(&header),
        tensor_bytes: header.tensor_bytes,
        too_big: false,
    })
}

/// The catalog model a cached file is: its blob name is the pinned SHA-256
/// and its size matches, or (a plain copy) its repository, file name, and
/// size match the pin.
fn catalog_match(
    blob_sha256: Option<&str>,
    repository: &str,
    file_name: &str,
    bytes: u64,
) -> Option<TranscriptionModelId> {
    MODELS
        .iter()
        .find(|model| {
            let ModelRuntime::Gguf(artifact) = model.runtime else {
                return false;
            };
            artifact.bytes == bytes
                && match blob_sha256 {
                    Some(sha256) => sha256 == artifact.sha256,
                    None => artifact.repository == repository && artifact.filename == file_name,
                }
        })
        .map(|model| model.id)
}

fn compression(header: &super::gguf_header::GgufHeader) -> &'static str {
    let count = |kind| header.tensor_types.get(&kind).copied().unwrap_or(0);
    if count(8) > 0 {
        "Q8_0"
    } else if count(1) > 0 {
        "F16"
    } else {
        "F32"
    }
}

/// A found file that is exactly a catalog model, described from the catalog.
fn catalog_found(
    path: &Path,
    repository: &str,
    revision: &str,
    file_name: String,
    bytes: u64,
    blob_sha256: Option<String>,
    model: TranscriptionModelId,
) -> Found {
    let definition = super::models::definition(model);
    let (architecture, variant) = match definition.runtime {
        ModelRuntime::Gguf(artifact) => (artifact.architecture, artifact.variant),
        ModelRuntime::AppleSpeech => ("", ""),
    };
    Found {
        path: path.to_path_buf(),
        repository: repository.to_owned(),
        revision: revision.to_owned(),
        file_name,
        bytes,
        blob_sha256,
        catalog: Some(model),
        name: definition.name.to_owned(),
        architecture: architecture.to_owned(),
        variant: variant.to_owned(),
        languages: definition
            .languages
            .iter()
            .map(|code| (*code).to_owned())
            .collect(),
        compression: "Q8_0",
        tensor_bytes: bytes,
        too_big: false,
    }
}

/// A catalog model Hex uses from another app's download: where it is, and
/// the receipt of the full check Hex made when it was first used.
#[derive(Deserialize, Serialize)]
struct Adopted {
    path: PathBuf,
    receipt: Receipt,
}

fn adopted_file(model: &ModelDefinition) -> Result<PathBuf> {
    Ok(models_dir()?
        .join("found")
        .join(format!("{}.json", model.id.as_str())))
}

fn adopted(model: &ModelDefinition) -> Option<Adopted> {
    let data = fs::read(adopted_file(model).ok()?).ok()?;
    serde_json::from_slice(&data).ok()
}

/// Where a catalog model already used from another app's download is, while
/// that file is still there at the pinned size.
pub fn adopted_catalog(model: &ModelDefinition) -> Option<PathBuf> {
    let ModelRuntime::Gguf(artifact) = model.runtime else {
        return None;
    };
    adopted(model)
        .map(|adopted| adopted.path)
        .filter(|path| fs::metadata(path).is_ok_and(|metadata| metadata.len() == artifact.bytes))
}

/// Catalog models the person chose to download even though a copy is on
/// this computer ("Download instead"), until Hex quits.
static DECLINED: LazyLock<Mutex<Vec<TranscriptionModelId>>> = LazyLock::new(Mutex::default);

/// Downloads `model` the usual way from now on, instead of using the copy
/// found on this computer. The found file stays where it is.
pub fn decline(model: TranscriptionModelId) {
    let mut declined = DECLINED.lock().unwrap_or_else(|error| error.into_inner());
    if !declined.contains(&model) {
        declined.push(model);
    }
}

/// Uses the copy on this computer again after [`decline`].
pub fn accept(model: TranscriptionModelId) {
    DECLINED
        .lock()
        .unwrap_or_else(|error| error.into_inner())
        .retain(|declined| *declined != model);
}

fn declined(model: TranscriptionModelId) -> bool {
    DECLINED
        .lock()
        .unwrap_or_else(|error| error.into_inner())
        .contains(&model)
}

/// The cached file this catalog model could use instead of a download: one
/// already adopted, else an exact match the last scan found.
pub fn catalog_candidate(model: &ModelDefinition) -> Option<PathBuf> {
    if declined(model.id) {
        return None;
    }
    adopted_catalog(model).or_else(|| {
        last()
            .into_iter()
            .find(|found| found.catalog == Some(model.id))
            .map(|found| found.path)
    })
}

/// Whether the adopted file still holds the receipt of Hex's full check.
pub fn catalog_verified(model: &ModelDefinition) -> bool {
    adopted(model).is_some_and(|adopted| {
        fs::metadata(&adopted.path).is_ok_and(|metadata| adopted.receipt.holds(&metadata))
    })
}

/// Uses `path`, another app's download of `model`, where it is: hashed in
/// full against the pinned SHA-256 the first time, and again only when its
/// size, modification time, or file ID change. Hex writes only its own record.
pub fn adopt_catalog(
    model: &ModelDefinition,
    path: &Path,
    canceled: &AtomicBool,
) -> Result<PathBuf> {
    let ModelRuntime::Gguf(artifact) = model.runtime else {
        bail!("{} has no file to check", model.name);
    };
    if let Some(adopted) = adopted(model)
        && adopted.path == path
        && fs::metadata(path).is_ok_and(|metadata| adopted.receipt.holds(&metadata))
    {
        return Ok(path.to_path_buf());
    }
    let metadata = fs::metadata(path)?;
    if metadata.len() != artifact.bytes {
        bail!(
            "{} found on this computer is not the size Hex pins",
            model.name
        );
    }
    let sha256 = super::models::sha256_hex(path, Some(canceled))?;
    if sha256 != artifact.sha256 {
        bail!(
            "{} found on this computer doesn't match the checksum Hex pins",
            model.name
        );
    }
    let record = adopted_file(model)?;
    fs::create_dir_all(
        record
            .parent()
            .ok_or_else(|| eyre!("no folder for {}", record.display()))?,
    )?;
    super::models::write_json_atomically(
        &record,
        &Adopted {
            path: path.to_path_buf(),
            receipt: Receipt::of(path, sha256)?,
        },
    )?;
    tracing::info!(model = model.id.as_str(), path = %path.display(), "using a speech model found on this computer");
    Ok(path.to_path_buf())
}

/// Forgets an adopted catalog model; the file stays where it is.
pub fn forget_catalog(model: &ModelDefinition) -> Result<()> {
    match fs::remove_file(adopted_file(model)?) {
        Err(error) if error.kind() != std::io::ErrorKind::NotFound => Err(error.into()),
        _ => Ok(()),
    }
}

/// The largest memory a GPU here reports, read once.
#[cfg(any(target_os = "macos", target_os = "linux", target_os = "windows"))]
pub fn gpu_memory() -> Option<u64> {
    static MEMORY: LazyLock<Option<u64>> = LazyLock::new(|| {
        transcribe_cpp::devices()
            .into_iter()
            .filter(|device| {
                matches!(
                    device.device_type,
                    transcribe_cpp::DeviceType::Gpu | transcribe_cpp::DeviceType::Igpu
                )
            })
            .map(|device| device.memory_total)
            .filter(|memory| *memory > 0)
            .max()
    });
    *MEMORY
}

#[cfg(not(any(target_os = "macos", target_os = "linux", target_os = "windows")))]
pub fn gpu_memory() -> Option<u64> {
    None
}

/// Whether loading `tensor_bytes` of weights likely won't fit the GPU: the
/// weights plus a quarter for activations exceed what it reports.
pub fn likely_too_big(tensor_bytes: u64) -> bool {
    gpu_memory().is_some_and(|memory| tensor_bytes.saturating_add(tensor_bytes / 4) > memory)
}

/// `owner/name@abcdef1` for a file in the hub cache, from its path.
pub fn describe(path: &Path) -> Option<String> {
    let mut parts = path.iter().rev().skip(1);
    let revision = parts.next()?.to_string_lossy().into_owned();
    if parts.next()? != "snapshots" {
        return None;
    }
    let repository = parts.next()?.to_string_lossy();
    let repository = repository.strip_prefix("models--")?.replacen("--", "/", 1);
    Some(format!(
        "{repository}@{}",
        &revision[..revision.len().min(7)]
    ))
}

#[cfg(all(test, unix))]
mod tests {
    use super::*;
    use crate::transcription::gguf_header::tests::gguf;

    fn whisper() -> Vec<u8> {
        gguf(
            &[
                ("general.architecture", "whisper"),
                ("general.name", "Whisper Small"),
                ("stt.variant", "whisper-small"),
            ],
            &[("general.languages", &["en", "de"])],
            &[],
            &[("encoder.weight", 8, 64), ("norm", 0, 16)],
        )
    }

    fn temporary(name: &str) -> PathBuf {
        let directory =
            std::env::temp_dir().join(format!("hex-found-{name}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&directory);
        fs::create_dir_all(&directory).unwrap();
        directory
    }

    /// Puts `bytes` (or a sparse file of `length`) in a fake hub as the
    /// blob `sha256`, linked from `repository`'s snapshot `revision`.
    fn cache(
        hub: &Path,
        repository: &str,
        revision: &str,
        file: &str,
        sha256: &str,
        contents: Option<&[u8]>,
        length: u64,
    ) -> PathBuf {
        let folder = hub.join(format!("models--{}", repository.replacen('/', "--", 1)));
        let blobs = folder.join("blobs");
        let snapshot = folder.join("snapshots").join(revision);
        fs::create_dir_all(&blobs).unwrap();
        fs::create_dir_all(&snapshot).unwrap();
        let blob = blobs.join(sha256);
        match contents {
            Some(contents) => fs::write(&blob, contents).unwrap(),
            None => fs::File::create(&blob).unwrap().set_len(length).unwrap(),
        }
        let link = snapshot.join(file);
        std::os::unix::fs::symlink(format!("../../blobs/{sha256}"), &link).unwrap();
        link
    }

    fn snapshot_of(hub: &Path) -> Vec<(PathBuf, u64, Option<SystemTime>)> {
        let mut files = Vec::new();
        let mut pending = vec![hub.to_path_buf()];
        while let Some(directory) = pending.pop() {
            for entry in fs::read_dir(directory).unwrap().flatten() {
                let metadata = fs::symlink_metadata(entry.path()).unwrap();
                if metadata.is_dir() {
                    pending.push(entry.path());
                } else {
                    files.push((entry.path(), metadata.len(), metadata.modified().ok()));
                }
            }
        }
        files.sort();
        files
    }

    #[test]
    fn download_instead_stops_using_the_found_copy() {
        let model = super::super::models::definition(TranscriptionModelId::SenseVoiceSmall);
        let path = PathBuf::from("/nowhere/sense-voice.gguf");
        let found = vec![Found {
            path: path.clone(),
            repository: String::new(),
            revision: String::new(),
            file_name: String::new(),
            bytes: 0,
            blob_sha256: None,
            catalog: Some(model.id),
            name: String::new(),
            architecture: String::new(),
            variant: String::new(),
            languages: Vec::new(),
            compression: "Q8_0",
            tensor_bytes: 0,
            too_big: false,
        }];
        CACHE.lock().unwrap().last = found;
        assert_eq!(catalog_candidate(model), Some(path.clone()));
        decline(model.id);
        assert_eq!(catalog_candidate(model), None);
        accept(model.id);
        assert_eq!(catalog_candidate(model), Some(path));
        CACHE.lock().unwrap().last.clear();
    }

    #[test]
    fn a_scan_finds_catalog_matches_and_compatible_files_without_writing() {
        let hub = temporary("scan");
        let v3 = super::super::models::definition(TranscriptionModelId::ParakeetV3);
        let ModelRuntime::Gguf(artifact) = v3.runtime else {
            unreachable!("Parakeet v3 is a GGUF model")
        };
        cache(
            &hub,
            artifact.repository,
            artifact.revision,
            artifact.filename,
            artifact.sha256,
            None,
            artifact.bytes,
        );
        let contents = whisper();
        let sha256 = "ab".repeat(32);
        cache(
            &hub,
            "someone/whisper-small-gguf",
            "0123456789abcdef",
            "whisper-small-Q8_0.gguf",
            &sha256,
            Some(&contents),
            0,
        );
        // Not a model Hex can run, and not a GGUF at all.
        cache(
            &hub,
            "someone/broken",
            "1111111",
            "broken.gguf",
            &"cd".repeat(32),
            Some(b"not a gguf"),
            0,
        );
        cache(
            &hub,
            "someone/readme",
            "2222222",
            "README.md",
            &"ef".repeat(32),
            Some(b"hi"),
            0,
        );
        let before = snapshot_of(&hub);
        let found = scan_in(&hub);
        assert_eq!(found.len(), 2, "{found:#?}");
        assert_eq!(found[0].catalog, Some(TranscriptionModelId::ParakeetV3));
        assert_eq!(found[0].blob_sha256.as_deref(), Some(artifact.sha256));
        let other = &found[1];
        assert_eq!(other.catalog, None);
        assert_eq!(other.name, "Whisper Small");
        assert_eq!(other.compression, "Q8_0");
        assert_eq!(other.source(), "someone/whisper-small-gguf@0123456");
        assert_eq!(other.languages, ["en", "de"]);
        assert_eq!(
            describe(&other.path).as_deref(),
            Some("someone/whisper-small-gguf@0123456")
        );
        // A second scan answers unchanged files from the cache.
        assert_eq!(scan_in(&hub), found);
        assert_eq!(snapshot_of(&hub), before, "the cache is never written");
        fs::remove_dir_all(hub).unwrap();
    }

    #[test]
    fn found_files_are_used_and_forgotten_in_place() {
        let hub = temporary("adopt");
        let models = temporary("adopt-models");
        super::super::models::set_test_models_dir(Some(models.clone()));
        let contents = whisper();
        let staged = hub.join("staged.gguf");
        fs::write(&staged, &contents).unwrap();
        let sha256 = super::super::models::sha256_hex(&staged, None).unwrap();
        fs::remove_file(staged).unwrap();
        let path = cache(
            &hub,
            "someone/whisper-small-gguf",
            "0123456789abcdef",
            "whisper-small-Q8_0.gguf",
            &sha256,
            Some(&contents),
            0,
        );
        let before = snapshot_of(&hub);
        // A blob whose contents don't match its name is refused.
        assert!(
            super::super::custom_models::adopt_in_place(
                &path,
                Some(&"00".repeat(32)),
                "someone/whisper-small-gguf@0123456",
                &AtomicBool::new(false)
            )
            .unwrap_err()
            .to_string()
            .contains("checksum")
        );
        let id = super::super::custom_models::adopt_in_place(
            &path,
            Some(&sha256),
            "someone/whisper-small-gguf@0123456",
            &AtomicBool::new(false),
        )
        .unwrap();
        assert!(super::super::custom_models::uses_path(&path));
        assert!(super::super::custom_models::usable(id));
        let definition = super::super::custom_models::definition(id);
        assert_eq!(super::super::models::model_path(definition).unwrap(), path);
        assert_eq!(
            super::super::custom_models::verify(id, &AtomicBool::new(false)).unwrap(),
            path
        );
        super::super::custom_models::remove(id).unwrap();
        assert!(!super::super::custom_models::uses_path(&path));
        assert!(path.is_file(), "forgetting leaves the file");
        assert_eq!(snapshot_of(&hub), before, "the cache is never written");
        super::super::models::set_test_models_dir(None);
        fs::remove_dir_all(hub).unwrap();
        fs::remove_dir_all(models).unwrap();
    }

    #[test]
    fn a_catalog_match_is_checked_in_full_before_use() {
        let hub = temporary("catalog");
        let models = temporary("catalog-models");
        super::super::models::set_test_models_dir(Some(models.clone()));
        // The smallest catalog file keeps the full check quick.
        let (v3, artifact) = MODELS
            .iter()
            .filter(|model| !model.id.is_custom())
            .filter_map(|model| match model.runtime {
                ModelRuntime::Gguf(artifact) => Some((model, artifact)),
                ModelRuntime::AppleSpeech => None,
            })
            .min_by_key(|(_, artifact)| artifact.bytes)
            .unwrap();
        // The right name and size, the wrong bytes.
        let path = cache(
            &hub,
            artifact.repository,
            artifact.revision,
            artifact.filename,
            artifact.sha256,
            None,
            artifact.bytes,
        );
        let before = snapshot_of(&hub);
        assert!(
            adopt_catalog(v3, &path, &AtomicBool::new(false))
                .unwrap_err()
                .to_string()
                .contains("checksum")
        );
        assert!(adopted_catalog(v3).is_none());
        assert!(!catalog_verified(v3));
        forget_catalog(v3).unwrap();
        assert_eq!(snapshot_of(&hub), before, "the cache is never written");
        super::super::models::set_test_models_dir(None);
        fs::remove_dir_all(hub).unwrap();
        fs::remove_dir_all(models).unwrap();
    }
}
