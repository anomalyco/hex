//! The cleanup tagger's model on disk: which file it is, where it lives,
//! and how it gets there.
//!
//! The model is the one the iPhone app ships, a 28 MB Core ML package kept
//! out of git as a GitHub release asset (`ios/scripts/fetch-cleanup-model.sh`
//! pins the same file). It is fetched only while Labs › Clean up what I say
//! is on, the first time a dictation wants it: through `net`, into a
//! `.partial` file that a later try resumes, and unpacked only once its size
//! and SHA-256 match the pin. The package is then compiled once, and the
//! compiled model is what every later launch loads.
//!
//! Everything sits in `<models>/cleanup/`:
//! - `HexCleanupTagger-1.mlpackage`, the package as the archive holds it;
//! - `HexCleanupTagger-1.<pin>.mlmodelc`, compiled from it, named for the
//!   archive's SHA-256 so another pin never loads this one's model.
//!
//! Each arrives by a rename of a finished folder, so a crash or a failed
//! download leaves either nothing or a whole model.

use std::fs::{self, File};
use std::path::{Path, PathBuf};

use color_eyre::eyre::{Result, bail, eyre};
use fs2::FileExt;

use super::archive::{self, Limits};

/// One pinned model archive.
#[derive(Clone, Debug)]
pub(super) struct Artifact {
    /// The `anomalyco/hex` release that carries the archive.
    pub(super) tag: &'static str,
    pub(super) asset: &'static str,
    pub(super) bytes: u64,
    pub(super) sha256: &'static str,
    /// The one folder the archive holds.
    pub(super) root: &'static str,
    /// What the package and the compiled model are called here.
    pub(super) name: &'static str,
}

/// The v1 tagger: BERT L-4/H-512 with int8 weights, English only.
pub(super) const TAGGER: Artifact = Artifact {
    tag: "ios-model-cleanup-tagger-1",
    asset: "HexCleanupTagger-1.mlpackage.zip",
    bytes: 25_906_154,
    sha256: "dd681dfddaa3902a5f359056a60f94917651cc10134e465cc3063847002d52fc",
    root: "HexCleanupTagger.mlpackage",
    name: "HexCleanupTagger-1",
};

/// The package unpacks to 28.5 MB in seven entries; anything far from that
/// is not this model.
const LIMITS: Limits = Limits {
    entries: 64,
    bytes: 64 << 20,
};

impl Artifact {
    pub(super) fn url(&self) -> String {
        format!(
            "https://github.com/anomalyco/hex/releases/download/{}/{}",
            self.tag, self.asset
        )
    }

    fn package(&self, folder: &Path) -> PathBuf {
        folder.join(format!("{}.mlpackage", self.name))
    }

    /// The compiled model, named for the first sixteen digits of the pin.
    fn compiled(&self, folder: &Path) -> PathBuf {
        let pin = self.sha256.get(..16).unwrap_or(self.sha256);
        folder.join(format!("{}.{pin}.mlmodelc", self.name))
    }

    fn partial(&self, folder: &Path) -> PathBuf {
        folder.join(format!("{}.partial", self.asset))
    }
}

/// `<models>/cleanup`.
#[cfg(not(test))]
pub(super) fn folder() -> Result<PathBuf> {
    Ok(crate::transcription::models::models_dir()?.join("cleanup"))
}

/// The compiled model in `folder`, when it is installed.
pub(super) fn installed(artifact: &Artifact, folder: &Path) -> Option<PathBuf> {
    let compiled = artifact.compiled(folder);
    compiled.is_dir().then_some(compiled)
}

/// Installs the model into `folder` unless it is there already: fetches
/// the archive unless the package is unpacked, then compiles the package
/// unless the compiled model is there. `fetch` downloads the archive into
/// the partial file it is given, appending to what is there, and `compile`
/// turns a package into a compiled model at a path that does not exist yet.
pub(super) fn install(
    artifact: &Artifact,
    folder: &Path,
    fetch: impl FnOnce(&Path) -> Result<()>,
    compile: impl FnOnce(&Path, &Path) -> Result<()>,
) -> Result<PathBuf> {
    fs::create_dir_all(folder)?;
    // Another Hex may be installing the same model: wait for it, then find
    // its work done.
    let lock = File::create(folder.join(".install.lock"))?;
    lock.lock_exclusive()?;
    let compiled = artifact.compiled(folder);
    if compiled.is_dir() {
        return Ok(compiled);
    }
    let package = artifact.package(folder);
    if !package.is_dir() {
        unpack_verified(artifact, folder, &package, fetch)?;
    }
    let staging = folder.join(format!(".{}.compiling", artifact.name));
    remove_dir(&staging)?;
    let result = compile(&package, &staging).and_then(|()| Ok(fs::rename(&staging, &compiled)?));
    if let Err(error) = result {
        let _ = remove_dir(&staging);
        // A package Core ML cannot read is fetched again next time.
        let _ = remove_dir(&package);
        return Err(error);
    }
    crate::durable::sync_directory(folder)?;
    Ok(compiled)
}

/// Fetches the archive, checks it against the pin, and unpacks its package
/// to `package`.
fn unpack_verified(
    artifact: &Artifact,
    folder: &Path,
    package: &Path,
    fetch: impl FnOnce(&Path) -> Result<()>,
) -> Result<()> {
    let partial = artifact.partial(folder);
    // A partial file longer than the archive is not a part of it.
    if fs::metadata(&partial).is_ok_and(|metadata| metadata.len() > artifact.bytes) {
        fs::remove_file(&partial)?;
    }
    fetch(&partial)?;
    if let Err(error) = verify(artifact, &partial) {
        // Nothing of a wrong file is worth resuming.
        let _ = fs::remove_file(&partial);
        return Err(error);
    }
    let staging = folder.join(format!(".{}.unpacking", artifact.name));
    remove_dir(&staging)?;
    fs::create_dir_all(&staging)?;
    let unpacked = archive::unpack(&partial, &staging, artifact.root, LIMITS)
        .and_then(|()| Ok(fs::rename(staging.join(artifact.root), package)?));
    let _ = remove_dir(&staging);
    let _ = fs::remove_file(&partial);
    unpacked
}

/// Whether the file at `archive` is the pinned one, byte for byte.
fn verify(artifact: &Artifact, archive: &Path) -> Result<()> {
    let length = fs::metadata(archive)?.len();
    if length != artifact.bytes {
        bail!(
            "downloaded {length} bytes for the cleanup model, expected {}",
            artifact.bytes
        );
    }
    if crate::transcription::models::sha256_hex(archive, None)? != artifact.sha256 {
        bail!("checksum mismatch for the cleanup model");
    }
    Ok(())
}

fn remove_dir(path: &Path) -> Result<()> {
    match fs::remove_dir_all(path) {
        Err(error) if error.kind() != std::io::ErrorKind::NotFound => Err(eyre!(
            "could not clear {}: {error}",
            path.file_name().unwrap_or_default().to_string_lossy()
        )),
        _ => Ok(()),
    }
}

/// Downloads the archive into `partial` over Hex's own HTTPS, resuming what
/// an earlier try left there.
#[cfg(target_os = "macos")]
pub(super) fn fetch(artifact: &Artifact, partial: &Path) -> Result<()> {
    crate::net::download(
        &crate::net::Download {
            url: &artifact.url(),
            user_agent: &format!("hex/{}", crate::VERSION),
            size: Some(artifact.bytes),
            canceled: None,
        },
        partial,
        |_| {},
    )
}

/// Starts installing the model in the background unless an install is
/// running. A failed install is tried again the next time a dictation wants
/// the model.
#[cfg(all(target_os = "macos", not(test)))]
pub(super) fn start_install() {
    use std::sync::atomic::{AtomicBool, Ordering};
    static INSTALLING: AtomicBool = AtomicBool::new(false);
    if INSTALLING.swap(true, Ordering::AcqRel) {
        return;
    }
    let spawned = std::thread::Builder::new()
        .name("hex-cleanup-model".into())
        .spawn(|| {
            let started = std::time::Instant::now();
            let installed = folder().and_then(|folder| {
                install(
                    &TAGGER,
                    &folder,
                    |partial| fetch(&TAGGER, partial),
                    |package, compiled| {
                        super::tagger::compile(package, compiled).map_err(|error| eyre!(error))
                    },
                )
            });
            match installed {
                Ok(_) => tracing::info!(
                    elapsed_ms = started.elapsed().as_millis() as u64,
                    "the cleanup model is installed"
                ),
                Err(error) => {
                    tracing::warn!(%error, "could not install the cleanup model");
                }
            }
            INSTALLING.store(false, Ordering::Release);
        });
    if spawned.is_err() {
        INSTALLING.store(false, Ordering::Release);
    }
}

#[cfg(test)]
mod tests {
    use std::cell::Cell;

    use sha2::{Digest, Sha256};

    use super::super::archive::tests::{Scratch, archive, file};
    use super::*;

    fn model_zip() -> Vec<u8> {
        archive(&[
            file("Tagger.mlpackage/Manifest.json", b"{}"),
            file("Tagger.mlpackage/Data/weights/weight.bin", &[3; 4096]),
        ])
    }

    /// The pin of `zip`, as a release would publish it.
    fn pinned(zip: &[u8]) -> Artifact {
        let digest: String = Sha256::digest(zip)
            .iter()
            .map(|byte| format!("{byte:02x}"))
            .collect();
        Artifact {
            tag: "test-model-1",
            asset: "Tagger-1.mlpackage.zip",
            bytes: zip.len() as u64,
            sha256: Box::leak(digest.into_boxed_str()),
            root: "Tagger.mlpackage",
            name: "Tagger-1",
        }
    }

    /// A compiler that writes a folder naming the package it was given.
    fn compile(package: &Path, compiled: &Path) -> Result<()> {
        assert!(package.join("Manifest.json").is_file());
        assert!(!compiled.exists());
        fs::create_dir_all(compiled)?;
        fs::write(compiled.join("model.mil"), b"compiled")?;
        Ok(())
    }

    fn names(folder: &Path) -> Vec<String> {
        let mut names: Vec<String> = fs::read_dir(folder)
            .unwrap()
            .flatten()
            .map(|entry| entry.file_name().to_string_lossy().into_owned())
            .collect();
        names.sort();
        names
    }

    #[test]
    fn the_pin_names_the_release_asset_the_iphone_app_ships() {
        let script = include_str!("../../../ios/scripts/fetch-cleanup-model.sh");
        for line in [
            format!("tag={}", TAGGER.tag),
            format!("asset={}", TAGGER.asset),
            format!("sha256={}", TAGGER.sha256),
        ] {
            assert!(script.lines().any(|found| found == line), "{line}");
        }
        assert!(script.contains(&format!("$destination/{}", TAGGER.root)));
        assert_eq!(
            TAGGER.url(),
            "https://github.com/anomalyco/hex/releases/download/ios-model-cleanup-tagger-1/HexCleanupTagger-1.mlpackage.zip"
        );
        let folder = Path::new("models/cleanup");
        assert_eq!(
            TAGGER.package(folder),
            folder.join("HexCleanupTagger-1.mlpackage")
        );
        assert_eq!(
            TAGGER.compiled(folder),
            folder.join("HexCleanupTagger-1.dd681dfddaa3902a.mlmodelc")
        );
    }

    #[test]
    fn a_verified_archive_is_unpacked_and_compiled_once() {
        let zip = model_zip();
        let artifact = pinned(&zip);
        let scratch = Scratch::new("install");
        let folder = scratch.0.join("cleanup");
        assert_eq!(installed(&artifact, &folder), None);

        let fetched = Cell::new(0);
        let fetch = |partial: &Path| {
            fetched.set(fetched.get() + 1);
            Ok(fs::write(partial, &zip)?)
        };
        let compiled = install(&artifact, &folder, fetch, compile).unwrap();
        assert_eq!(installed(&artifact, &folder), Some(compiled.clone()));
        assert_eq!(fs::read(compiled.join("model.mil")).unwrap(), b"compiled");
        assert_eq!(
            fs::read(folder.join("Tagger-1.mlpackage/Data/weights/weight.bin")).unwrap(),
            [3; 4096]
        );
        assert_eq!(
            names(&folder),
            [
                ".install.lock",
                compiled.file_name().unwrap().to_str().unwrap(),
                "Tagger-1.mlpackage",
            ]
        );

        // Installed: nothing is fetched or compiled again.
        let again = install(
            &artifact,
            &folder,
            |_| panic!("fetched an installed model"),
            |_, _| panic!("compiled an installed model"),
        );
        assert_eq!(again.unwrap(), compiled);
        assert_eq!(fetched.get(), 1);

        // Only the compiled model gone: the package on disk is compiled again.
        fs::remove_dir_all(&compiled).unwrap();
        let recompiled = install(
            &artifact,
            &folder,
            |_| panic!("fetched an unpacked model"),
            compile,
        );
        assert_eq!(recompiled.unwrap(), compiled);
    }

    #[test]
    fn an_archive_that_is_not_the_pinned_one_is_never_unpacked() {
        let zip = model_zip();
        let artifact = pinned(&zip);
        let mut other = zip.clone();
        let last = other.len() - 1;
        // The same length, one byte different.
        other[last] ^= 1;
        for (bytes, why) in [
            (other, "checksum mismatch for the cleanup model".to_owned()),
            (
                zip[..zip.len() - 1].to_vec(),
                format!(
                    "downloaded {} bytes for the cleanup model, expected {}",
                    zip.len() - 1,
                    zip.len()
                ),
            ),
        ] {
            let scratch = Scratch::new("wrong");
            let folder = scratch.0.join("cleanup");
            let error = install(
                &artifact,
                &folder,
                |partial| Ok(fs::write(partial, &bytes)?),
                |_, _| panic!("compiled an unverified model"),
            )
            .unwrap_err();
            assert_eq!(error.to_string(), why);
            assert_eq!(installed(&artifact, &folder), None);
            // The wrong file is gone too, so the next try starts over.
            assert_eq!(names(&folder), [".install.lock"]);
        }
    }

    #[test]
    fn a_failed_download_keeps_its_partial_file_and_installs_nothing() {
        let zip = model_zip();
        let artifact = pinned(&zip);
        let scratch = Scratch::new("offline");
        let folder = scratch.0.join("cleanup");
        let error = install(
            &artifact,
            &folder,
            |partial| {
                fs::write(partial, &zip[..100])?;
                bail!("github.com could not be reached")
            },
            |_, _| panic!("compiled a model that never arrived"),
        )
        .unwrap_err();
        assert_eq!(error.to_string(), "github.com could not be reached");
        assert_eq!(
            names(&folder),
            [".install.lock", "Tagger-1.mlpackage.zip.partial"]
        );

        // The next try is handed the same partial file and finishes it.
        let compiled = install(
            &artifact,
            &folder,
            |partial| {
                assert_eq!(fs::read(partial)?, &zip[..100]);
                Ok(fs::write(partial, &zip)?)
            },
            compile,
        )
        .unwrap();
        assert!(compiled.is_dir());

        // A partial file longer than the archive is thrown away first.
        let scratch = Scratch::new("long");
        let folder = scratch.0.join("cleanup");
        fs::create_dir_all(&folder).unwrap();
        fs::write(artifact.partial(&folder), vec![0; zip.len() + 1]).unwrap();
        install(
            &artifact,
            &folder,
            |partial| {
                assert!(!partial.exists());
                Ok(fs::write(partial, &zip)?)
            },
            compile,
        )
        .unwrap();
    }

    #[test]
    fn a_package_that_does_not_compile_is_fetched_again_next_time() {
        let zip = model_zip();
        let artifact = pinned(&zip);
        let scratch = Scratch::new("compile");
        let folder = scratch.0.join("cleanup");
        let error = install(
            &artifact,
            &folder,
            |partial| Ok(fs::write(partial, &zip)?),
            |_, compiled| {
                fs::create_dir_all(compiled)?;
                bail!("Core ML could not read the model")
            },
        )
        .unwrap_err();
        assert_eq!(error.to_string(), "Core ML could not read the model");
        assert_eq!(installed(&artifact, &folder), None);
        assert_eq!(names(&folder), [".install.lock"]);
    }

    #[test]
    fn an_archive_holding_another_folder_installs_nothing() {
        let zip = archive(&[file("../Tagger.mlpackage/Manifest.json", b"{}")]);
        let artifact = pinned(&zip);
        let scratch = Scratch::new("slip");
        let folder = scratch.0.join("cleanup");
        let error = install(
            &artifact,
            &folder,
            |partial| Ok(fs::write(partial, &zip)?),
            |_, _| panic!("compiled a refused archive"),
        )
        .unwrap_err();
        assert_eq!(
            error.to_string(),
            "the archive holds a file outside Tagger.mlpackage"
        );
        assert_eq!(names(&folder), [".install.lock"]);
        assert_eq!(names(&scratch.0), ["cleanup"]);
    }
}
