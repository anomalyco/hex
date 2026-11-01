//! HTTPS through `net` (the system's proxy and trusted roots): the polled
//! manifest with `If-None-Match`, and resumable downloads checked against
//! the manifest's size and SHA-256 before any byte is used. Requests carry
//! nothing about the install beyond `User-Agent: hex/<channel>/<version>`.

use std::fs::{self, OpenOptions};
use std::io::Read;
use std::path::{Path, PathBuf};
use std::time::Duration;

use color_eyre::eyre::{Result, WrapErr, bail, eyre};

use super::protocol::Target;

pub const MAX_MANIFEST_BYTES: u64 = 256 * 1024;

/// What every request names itself to the release service: the app, the
/// channel this build belongs to and its version, e.g. `hex/dev/0.0.0-dev-171`.
pub fn user_agent(channel: &str, version: &str) -> String {
    format!("hex/{channel}/{version}")
}

/// The last manifest the service sent, revalidated with its ETag.
#[derive(Default)]
pub struct ManifestCache {
    /// The manifest the ETag and body belong to; switching channels changes it.
    url: Option<String>,
    etag: Option<String>,
    body: Option<String>,
}

impl ManifestCache {
    /// Fetches `url`, returning the manifest text: the cached one when the
    /// service answers 304. `scratch` is unused since requests run in process.
    pub fn fetch(
        &mut self,
        url: &str,
        user_agent: &str,
        _scratch: &Path,
        max_time: Duration,
    ) -> Result<String> {
        if self.url.as_deref() != Some(url) {
            *self = Self {
                url: Some(url.to_owned()),
                ..Self::default()
            };
        }
        let agent = crate::net::agent(url, |config| {
            config
                .https_only(true)
                .max_redirects(10)
                .http_status_as_error(false)
                .timeout_global(Some(max_time.max(Duration::from_secs(1))))
        });
        let mut request = agent.get(url).header("User-Agent", user_agent);
        if let (Some(etag), Some(_)) = (&self.etag, &self.body) {
            request = request.header("If-None-Match", etag.as_str());
        }
        let mut response = request
            .call()
            .map_err(|error| eyre!("{}", crate::net::unreachable(url, &error)))?;
        match response.status().as_u16() {
            304 => self
                .body
                .clone()
                .ok_or_else(|| eyre!("the update service sent 304 for nothing cached")),
            200 => {
                let etag = response
                    .headers()
                    .get("etag")
                    .and_then(|value| value.to_str().ok())
                    .map(|value| value.trim().to_owned())
                    .filter(|etag| valid_etag(etag));
                let mut text = String::new();
                response
                    .body_mut()
                    .with_config()
                    .limit(MAX_MANIFEST_BYTES + 1)
                    .reader()
                    .take(MAX_MANIFEST_BYTES + 1)
                    .read_to_string(&mut text)
                    .wrap_err("the update manifest is not UTF-8")?;
                if text.len() as u64 > MAX_MANIFEST_BYTES {
                    bail!("the update manifest is too large");
                }
                self.etag = etag;
                self.body = Some(text.clone());
                Ok(text)
            }
            status => bail!("the update manifest request failed with HTTP {status}"),
        }
    }
}

fn valid_etag(etag: &str) -> bool {
    !etag.is_empty() && !etag.chars().any(char::is_control)
}

/// Downloads `target` from `url` into `directory`, resuming an earlier
/// partial download of the same bytes, and returns the verified file. The
/// file is named after the target URL's last segment.
pub fn download(url: &str, user_agent: &str, target: &Target, directory: &Path) -> Result<PathBuf> {
    let name = url
        .rsplit('/')
        .next()
        .filter(|name| valid_file_name(name))
        .ok_or_else(|| color_eyre::eyre::eyre!("the update file name is not usable"))?;
    let folder = directory.join(&target.sha256);
    let file = folder.join(name);
    if file.is_file() && verify(&file, target).is_ok() {
        return Ok(file);
    }
    fs::create_dir_all(&folder)?;
    let partial = folder.join(format!("{name}.partial"));
    if fs::metadata(&partial).is_ok_and(|metadata| metadata.len() > target.size) {
        fs::remove_file(&partial)?;
    }
    let mut options = OpenOptions::new();
    options.create(true).append(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }
    drop(options.open(&partial)?);
    let complete = fs::metadata(&partial)?.len() == target.size;
    if !complete {
        crate::net::download(
            &crate::net::Download {
                url,
                user_agent,
                size: Some(target.size),
                canceled: None,
            },
            &partial,
            |_| {},
        )
        .wrap_err("the update download failed")?;
    }
    if let Err(error) = verify(&partial, target) {
        // A mismatch at full size can never resume into the right bytes.
        let _ = fs::remove_file(&partial);
        return Err(error);
    }
    fs::rename(&partial, &file)?;
    Ok(file)
}

fn valid_file_name(name: &str) -> bool {
    let bytes = name.as_bytes();
    !bytes.is_empty()
        && bytes.len() <= 200
        && bytes[0].is_ascii_alphanumeric()
        && bytes
            .iter()
            .all(|byte| byte.is_ascii_alphanumeric() || b"._+-".contains(byte))
}

pub fn verify(path: &Path, target: &Target) -> Result<()> {
    let size = fs::metadata(path)?.len();
    if size != target.size {
        bail!("the update has {size} bytes, expected {}", target.size);
    }
    if crate::transcription::models::sha256_hex(path, None)? != target.sha256 {
        bail!("the update's SHA-256 does not match the manifest");
    }
    Ok(())
}

/// Removes downloads other than `keep` (a target's SHA-256).
pub fn prune(directory: &Path, keep: Option<&str>) {
    let Ok(entries) = fs::read_dir(directory) else {
        return;
    };
    for entry in entries.flatten() {
        let name = entry.file_name();
        if keep != name.to_str()
            && name.len() == 64
            && entry.path().is_dir()
            && let Err(error) = fs::remove_dir_all(entry.path())
        {
            tracing::warn!(%error, path = %entry.path().display(), "could not remove an old update download");
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn requests_name_the_app_channel_and_version() {
        assert_eq!(user_agent("dev", "0.0.0-dev-171"), "hex/dev/0.0.0-dev-171");
        assert_eq!(user_agent("stable", "2.3.1"), "hex/stable/2.3.1");
    }

    #[test]
    fn etags_with_control_characters_are_dropped() {
        assert!(valid_etag("\"new\""));
        assert!(!valid_etag(""));
        assert!(!valid_etag("\"a\nb\""));
    }

    #[test]
    fn downloads_are_checked_against_size_and_sha256() {
        let directory =
            std::env::temp_dir().join(format!("hex-update-verify-{}", std::process::id()));
        fs::create_dir_all(&directory).unwrap();
        let path = directory.join("file");
        fs::write(&path, b"new").unwrap();
        let mut target = Target {
            url: "files/x/file".into(),
            size: 3,
            sha256: crate::transcription::models::sha256_hex(&path, None).unwrap(),
        };
        assert!(verify(&path, &target).is_ok());
        target.size = 4;
        assert!(verify(&path, &target).is_err());
        target.size = 3;
        target.sha256 = "a".repeat(64);
        assert!(verify(&path, &target).is_err());
        let _ = fs::remove_dir_all(directory);
    }

    #[test]
    fn a_finished_download_is_reused_without_the_network() {
        let directory =
            std::env::temp_dir().join(format!("hex-update-reuse-{}", std::process::id()));
        let _ = fs::remove_dir_all(&directory);
        let source = directory.join("source");
        fs::create_dir_all(&directory).unwrap();
        fs::write(&source, b"bytes").unwrap();
        let sha256 = crate::transcription::models::sha256_hex(&source, None).unwrap();
        let target = Target {
            url: format!("files/{sha256}/Hex.zip"),
            size: 5,
            sha256: sha256.clone(),
        };
        // A complete partial download needs no request.
        fs::create_dir_all(directory.join(&sha256)).unwrap();
        fs::copy(&source, directory.join(&sha256).join("Hex.zip.partial")).unwrap();
        let file = download(
            "https://invalid.invalid/Hex.zip",
            "hex/dev/0.0.0-dev-1",
            &target,
            &directory,
        )
        .unwrap();
        assert_eq!(fs::read(&file).unwrap(), b"bytes");
        assert_eq!(
            download(
                "https://invalid.invalid/Hex.zip",
                "hex/dev/0.0.0-dev-1",
                &target,
                &directory
            )
            .unwrap(),
            file
        );
        prune(&directory, None);
        assert!(!directory.join(&sha256).exists());
        assert!(
            !valid_file_name("../x")
                && !valid_file_name(".hidden")
                && valid_file_name("Hex-2.4.0-macos-arm64.zip")
        );
        let _ = fs::remove_dir_all(directory);
    }
}
