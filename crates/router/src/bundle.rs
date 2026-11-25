//! The package boundary. Opening either format only reads the manifest; it
//! never builds, copies, records a destination, or claims a capture.

use std::fs;
use std::io::{Read, Write};
use std::path::{Path, PathBuf};

use anyhow::{Context, Result, ensure};

use crate::model::ScrawlBundle;

#[cfg(test)]
mod tests;

pub const EXTENSION: &str = "hexannotation";
pub const MANIFEST: &str = "annotation.json";

pub fn manifest_name(dir: &Path) -> Option<&'static str> {
    match dir.extension()?.to_str()? {
        EXTENSION => Some(MANIFEST),
        "scrawl" => Some("scrawl.json"),
        _ => None,
    }
}

#[derive(serde::Deserialize, serde::Serialize)]
#[serde(rename_all = "camelCase")]
struct Package<T> {
    format_version: u32,
    annotation: T,
    #[serde(
        default,
        flatten,
        skip_serializing_if = "std::collections::BTreeMap::is_empty"
    )]
    extra: std::collections::BTreeMap<String, serde_json::Value>,
}

fn existing_package_extra(path: &Path) -> std::collections::BTreeMap<String, serde_json::Value> {
    read_regular_bytes(path, 64 << 20)
        .ok()
        .and_then(|bytes| serde_json::from_slice::<Package<serde_json::Value>>(&bytes).ok())
        .filter(|pkg| pkg.format_version == 1)
        .map(|pkg| pkg.extra)
        .unwrap_or_default()
}

/// Publish last, after the assets are complete. Never truncate the visible
/// manifest (even a destination update must not expose half a document).
pub fn publish(dir: &Path, bundle: &ScrawlBundle) -> Result<()> {
    let name = manifest_name(dir).context("Unknown annotation package type")?;
    let target = dir.join(name);
    let bytes = if name == MANIFEST {
        serde_json::to_vec_pretty(&Package {
            format_version: 1,
            annotation: bundle,
            extra: existing_package_extra(&target),
        })?
    } else {
        serde_json::to_vec_pretty(bundle)?
    };
    replace_private(&target, &bytes)
}

/// Adjacent staging keeps the rename atomic across custom volumes and
/// replaces a destination symlink rather than writing through it.
pub fn replace_private(path: &Path, bytes: &[u8]) -> Result<()> {
    let dir = path.parent().context("Missing parent directory")?;
    let name = path
        .file_name()
        .context("Missing file name")?
        .to_string_lossy();
    let tmp = dir.join(format!(".{name}.{}.tmp", crate::util::make_bundle_id()));
    let mut options = fs::OpenOptions::new();
    options.write(true).create_new(true);
    #[cfg(unix)]
    std::os::unix::fs::OpenOptionsExt::mode(&mut options, 0o600);
    let mut file = options.open(&tmp)?;
    let result = (|| {
        file.write_all(bytes)?;
        file.sync_all()?;
        drop(file);
        fs::rename(&tmp, path)?;
        #[cfg(unix)]
        fs::File::open(dir)?.sync_all()?;
        Ok::<_, std::io::Error>(())
    })();
    if result.is_err() {
        let _ = fs::remove_file(tmp);
    }
    Ok(result?)
}

fn read_regular_bytes(path: &Path, max: u64) -> Result<Vec<u8>> {
    ensure!(
        fs::symlink_metadata(path)?.is_file(),
        "Not a regular manifest"
    );
    let mut options = fs::OpenOptions::new();
    options.read(true);
    #[cfg(unix)]
    std::os::unix::fs::OpenOptionsExt::custom_flags(
        &mut options,
        libc::O_NOFOLLOW | libc::O_NONBLOCK,
    );
    let file = options.open(path)?;
    let meta = file.metadata()?;
    ensure!(meta.is_file() && meta.len() <= max, "Invalid manifest size");
    let mut bytes = Vec::new();
    file.take(max + 1).read_to_end(&mut bytes)?;
    ensure!(bytes.len() as u64 <= max, "Manifest too large");
    Ok(bytes)
}

/// Decode the manifest without resolving an id or consulting any app state.
pub fn read_manifest(dir: &Path) -> Result<ScrawlBundle> {
    read_manifest_bounded(dir, 64 << 20)
}

/// [`read_manifest`] with a tighter caller-supplied byte ceiling (such as Paste Last).
pub fn read_manifest_bounded(dir: &Path, max: u64) -> Result<ScrawlBundle> {
    let name = manifest_name(dir).context("Unknown annotation package type")?;
    ensure!(
        fs::symlink_metadata(dir)?.is_dir(),
        "Not a package directory"
    );
    let bytes = read_regular_bytes(&dir.join(name), max)?;
    and_decode(dir, name, &bytes)
}

fn and_decode(dir: &Path, name: &str, bytes: &[u8]) -> Result<ScrawlBundle> {
    let bundle = if name == MANIFEST {
        let p: Package<ScrawlBundle> = serde_json::from_slice(bytes)?;
        ensure!(
            p.format_version == 1,
            "Unsupported Hex Annotation version {}",
            p.format_version
        );
        p.annotation
    } else {
        serde_json::from_slice(bytes)?
    };
    ensure!(
        bundle.version == 1 && !bundle.id.trim().is_empty(),
        "Unsupported annotation payload"
    );
    // Never let a path in a package point a preview, editor or send outside it.
    for rel in bundle
        .video
        .iter()
        .chain(&bundle.audio)
        .chain(&bundle.clean_frames)
        .chain(std::iter::once(&bundle.contact_sheet).filter(|s| !s.is_empty()))
        .chain(bundle.keyframes.iter().map(|k| &k.file))
        .chain(bundle.marks.iter().filter_map(|m| m.frame.as_ref()))
        .chain(
            bundle
                .autocrop
                .iter()
                .flat_map(|c| [&c.full_frames, &c.full_contact_sheet]),
        )
    {
        let mut path = dir.to_owned();
        for part in rel.split('/') {
            ensure!(
                !matches!(part, "" | "." | "..") && !part.contains(['\\', ':']),
                "Invalid asset path"
            );
            path.push(part);
            match fs::symlink_metadata(&path) {
                Ok(m) => ensure!(m.is_dir() || m.is_file(), "Invalid asset type"),
                Err(e) if e.kind() == std::io::ErrorKind::NotFound => (), // pruned media
                Err(e) => return Err(e.into()),
            }
        }
    }
    Ok(bundle)
}

/// A bundle's folder from a path, a `scrawl.json` in it, a bare bundle ID (looked up in the
/// host's captures folder) or `latest`.
pub fn resolve_bundle_dir(path_or_id: &Path) -> Result<PathBuf> {
    resolve_bundle_dir_in(crate::env::get().dirs.captures.as_deref(), path_or_id)
}

/// [`resolve_bundle_dir`] with the captures folder given.
pub fn resolve_bundle_dir_in(captures: Option<&Path>, path_or_id: &Path) -> Result<PathBuf> {
    if path_or_id == Path::new("latest") {
        return newest_bundle(captures.context("No captures folder")?);
    }
    let mut path = path_or_id.to_owned();
    if path
        .file_name()
        .is_some_and(|name| name == "scrawl.json" || name == MANIFEST)
    {
        path = path
            .parent()
            .context("Missing bundle directory")?
            .to_owned();
        if path.as_os_str().is_empty() {
            path = PathBuf::from(".");
        }
    }
    if !path.exists()
        && path.components().count() == 1
        && let Some(captures) = captures
    {
        path = captures.join(&path);
        if path.extension().is_none() {
            path.set_extension(EXTENSION);
            if read_manifest(&path).is_err() {
                path.set_extension("scrawl");
            }
        }
    }
    ensure!(
        fs::symlink_metadata(&path).is_ok_and(|m| m.is_dir()),
        "Bundle directory not found: {}",
        path.display()
    );
    let path = plain_path(
        path.canonicalize()
            .with_context(|| format!("Could not resolve {}", path.display()))?,
    );
    ensure!(
        manifest_name(&path).is_some(),
        "Expected an annotation package: {}",
        path.display()
    );
    Ok(path)
}

pub fn newest_bundle(captures: &Path) -> Result<PathBuf> {
    let mut candidates = Vec::new();
    for entry in
        fs::read_dir(captures).with_context(|| format!("Could not read {}", captures.display()))?
    {
        let entry = entry?;
        let path = entry.path();
        if let Ok(bundle) = read_manifest(&path) {
            candidates.push((
                bundle.created_at_ms,
                manifest_name(&path) == Some(MANIFEST),
                path,
            ));
        }
    }
    candidates.sort_unstable();
    candidates
        .pop()
        .context("No completed annotations found")?
        .2
        .canonicalize()
        .map(plain_path)
        .map_err(Into::into)
}

/// `canonicalize` on Windows returns `\\?\C:\…`; agents and terminals want `C:\…`.
pub fn plain_path(path: PathBuf) -> PathBuf {
    match path.to_str().and_then(|p| p.strip_prefix(r"\\?\")) {
        Some(rest) if rest.as_bytes().get(1) == Some(&b':') => PathBuf::from(rest),
        _ => path,
    }
}

pub fn load_bundle(bundle_dir: &Path) -> Result<(PathBuf, ScrawlBundle)> {
    load_bundle_in(crate::env::get().dirs.captures.as_deref(), bundle_dir)
}

/// [`load_bundle`] with the captures folder given.
pub fn load_bundle_in(
    captures: Option<&Path>,
    bundle_dir: &Path,
) -> Result<(PathBuf, ScrawlBundle)> {
    let directory = resolve_bundle_dir_in(captures, bundle_dir)?;
    let bundle = read_manifest(&directory)
        .with_context(|| format!("Could not open {}", directory.display()))?;
    Ok((directory, bundle))
}
