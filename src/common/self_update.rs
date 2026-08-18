//! The signed self-update core both port shells share: an ed25519-signed
//! feed names one content-addressed artifact per platform, and every
//! byte is verified against the signed manifest before activation. The
//! platform updaters own installation layout and relaunch; this module
//! owns trust.

use std::fs::{self, File, OpenOptions};
use std::io::Read;
use std::path::Path;
use std::process::{Command, Stdio};

use base64::Engine;
use color_eyre::eyre::{Result, WrapErr, bail, eyre};
use ed25519_dalek::{Signature, VerifyingKey};
use semver::Version;
use serde::Deserialize;
use sha2::{Digest, Sha256};

pub const RELEASE_BASE: &str = "https://pub-089d681d41754031a4aefa7017d8c2fb.r2.dev";
pub const MAX_FEED_BYTES: u64 = 64 * 1024;
pub const MAX_ARTIFACT_BYTES: u64 = 512 * 1024 * 1024;

/// The HEX release signing key; releases for every platform are signed
/// with it and `scripts/release-*.sh` refuse a mismatched private key.
pub const PUBLIC_KEY: [u8; 32] = [
    0xbf, 0xad, 0x02, 0xe6, 0x22, 0x08, 0xff, 0x14, 0x4b, 0x5c, 0x9d, 0x21, 0xc7, 0xe7, 0x9c, 0x7c,
    0x16, 0xc6, 0x90, 0x42, 0x99, 0xa4, 0x37, 0xd8, 0x57, 0x30, 0x30, 0x07, 0xcd, 0x4f, 0xf7, 0xd8,
];

/// What a platform's update feed must describe for this build.
pub struct UpdateTarget {
    /// The manifest's `target` value, e.g. "x86_64-pc-windows-msvc".
    pub target: &'static str,
    /// The content-addressed artifact suffix, e.g. "x86_64-windows.exe".
    pub artifact_suffix: &'static str,
}

#[derive(Deserialize)]
struct SignedFeed {
    payload: String,
    signature: String,
}

#[derive(Debug, Deserialize)]
pub struct Manifest {
    pub schema_version: u32,
    pub channel: String,
    pub target: String,
    pub version: String,
    pub artifact: String,
    pub bytes: u64,
    pub sha256: String,
}

pub fn verify_feed(bytes: &[u8], public_key: &[u8; 32]) -> Result<Manifest> {
    let feed: SignedFeed = serde_json::from_slice(bytes).wrap_err("invalid update feed")?;
    let payload = base64::engine::general_purpose::STANDARD
        .decode(feed.payload)
        .wrap_err("invalid update payload encoding")?;
    let signature = base64::engine::general_purpose::STANDARD
        .decode(feed.signature)
        .wrap_err("invalid update signature encoding")?;
    let signature = Signature::from_slice(&signature).wrap_err("invalid update signature")?;
    let key = VerifyingKey::from_bytes(public_key).wrap_err("invalid update public key")?;
    key.verify_strict(&payload, &signature)
        .wrap_err("update signature verification failed")?;
    serde_json::from_slice(&payload).wrap_err("invalid signed update manifest")
}

pub fn validate_manifest(manifest: &Manifest, target: &UpdateTarget) -> Result<Version> {
    if manifest.schema_version != 1 {
        bail!("unsupported update schema {}", manifest.schema_version);
    }
    if manifest.channel != "stable" {
        bail!("unsupported update channel");
    }
    if manifest.target != target.target {
        bail!("the update targets the wrong platform");
    }
    if manifest.bytes == 0 || manifest.bytes > MAX_ARTIFACT_BYTES {
        bail!("the update artifact has an invalid size");
    }
    if manifest.sha256.len() != 64
        || !manifest
            .sha256
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
    {
        bail!("the update artifact has an invalid checksum");
    }
    let version =
        Version::parse(&manifest.version).wrap_err("the update manifest has an invalid version")?;
    if !version.pre.is_empty() {
        bail!("stable updates cannot be prereleases");
    }
    if manifest.artifact
        != format!(
            "HEX-{version}-{}-{}",
            manifest.sha256, target.artifact_suffix
        )
    {
        bail!("the update artifact is not content-addressed");
    }
    Ok(version)
}

/// Download through the platform curl with HTTPS pinned, refusing bytes
/// past the signed limit. The destination is removed on any failure.
pub fn download(url: &str, path: &Path, max_bytes: u64) -> Result<()> {
    let _ = fs::remove_file(path);
    let result = (|| {
        let mut options = OpenOptions::new();
        options.create_new(true).write(true);
        #[cfg(unix)]
        {
            use std::os::unix::fs::OpenOptionsExt;
            options.mode(0o600);
        }
        let mut destination = options.open(path)?;
        let mut command = Command::new(curl_executable());
        command.args([
            "--fail",
            "--location",
            "--proto",
            "=https",
            "--proto-redir",
            "=https",
            "--silent",
            "--connect-timeout",
            "10",
            "--max-time",
            "600",
        ]);
        #[cfg(windows)]
        {
            use std::os::windows::process::CommandExt;
            const CREATE_NO_WINDOW: u32 = 0x0800_0000;
            command.creation_flags(CREATE_NO_WINDOW);
        }
        let mut child = command
            .arg(url)
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .spawn()
            .wrap_err("could not start the update download")?;
        let copied = std::io::copy(
            &mut child
                .stdout
                .take()
                .ok_or_else(|| eyre!("the update download has no output"))?
                .take(max_bytes + 1),
            &mut destination,
        )?;
        if copied > max_bytes {
            let _ = child.kill();
            let _ = child.wait();
            bail!("the update download exceeded its signed size limit");
        }
        let status = child.wait()?;
        if !status.success() {
            bail!("the update download failed with {status}");
        }
        destination.sync_all()?;
        Ok(())
    })();
    if result.is_err() {
        let _ = fs::remove_file(path);
    }
    result
}

fn curl_executable() -> &'static str {
    #[cfg(windows)]
    {
        "curl.exe"
    }
    #[cfg(not(windows))]
    {
        "curl"
    }
}

pub fn read_bounded(path: &Path, max_bytes: u64) -> Result<Vec<u8>> {
    let mut bytes = Vec::new();
    File::open(path)?
        .take(max_bytes + 1)
        .read_to_end(&mut bytes)?;
    if bytes.len() as u64 > max_bytes {
        bail!("the update feed is too large");
    }
    Ok(bytes)
}

pub fn verify_artifact(path: &Path, manifest: &Manifest) -> Result<()> {
    let metadata = fs::metadata(path)?;
    if metadata.len() != manifest.bytes {
        bail!(
            "the update has {} bytes, expected {}",
            metadata.len(),
            manifest.bytes
        );
    }
    let actual = sha256(path)?;
    if actual != manifest.sha256 {
        bail!("update checksum verification failed");
    }
    Ok(())
}

pub fn sha256(path: &Path) -> Result<String> {
    let mut file = File::open(path)?;
    let mut hasher = Sha256::new();
    let mut buffer = [0_u8; 64 * 1024];
    loop {
        let read = file.read(&mut buffer)?;
        if read == 0 {
            break;
        }
        hasher.update(&buffer[..read]);
    }
    const HEX: &[u8; 16] = b"0123456789abcdef";
    let mut result = String::with_capacity(64);
    for &byte in hasher.finalize().iter() {
        result.push(HEX[usize::from(byte >> 4)] as char);
        result.push(HEX[usize::from(byte & 0x0f)] as char);
    }
    Ok(result)
}

#[cfg(test)]
mod tests {
    use super::*;
    use ed25519_dalek::{Signer, SigningKey};

    const LINUX: UpdateTarget = UpdateTarget {
        target: "x86_64-unknown-linux-gnu",
        artifact_suffix: "x86_64-linux",
    };
    const WINDOWS: UpdateTarget = UpdateTarget {
        target: "x86_64-pc-windows-msvc",
        artifact_suffix: "x86_64-windows.exe",
    };

    fn signed_feed(payload: &[u8]) -> (Vec<u8>, [u8; 32]) {
        let signing = SigningKey::from_bytes(&[7; 32]);
        let feed = serde_json::json!({
            "payload": base64::engine::general_purpose::STANDARD.encode(payload),
            "signature": base64::engine::general_purpose::STANDARD.encode(signing.sign(payload).to_bytes()),
        });
        (
            serde_json::to_vec(&feed).unwrap(),
            signing.verifying_key().to_bytes(),
        )
    }

    fn manifest(target: &UpdateTarget) -> Manifest {
        Manifest {
            schema_version: 1,
            channel: "stable".into(),
            target: target.target.into(),
            version: "2.1.0".into(),
            artifact: format!("HEX-2.1.0-{}-{}", "a".repeat(64), target.artifact_suffix),
            bytes: 3,
            sha256: "a".repeat(64),
        }
    }

    #[test]
    fn signed_manifest_is_verified_before_parsing() {
        let payload = br#"{"schema_version":1,"channel":"stable","target":"x86_64-unknown-linux-gnu","version":"2.1.0","artifact":"HEX-2.1.0-x86_64-linux","bytes":3,"sha256":"abc"}"#;
        let (feed, key) = signed_feed(payload);
        let manifest = verify_feed(&feed, &key).unwrap();
        assert_eq!(manifest.version, "2.1.0");

        let parsed: serde_json::Value = serde_json::from_slice(&feed).unwrap();
        let signature = parsed["signature"].as_str().unwrap();
        let tampered = serde_json::to_vec(&serde_json::json!({
            "payload": base64::engine::general_purpose::STANDARD.encode(b"{}"),
            "signature": signature,
        }))
        .unwrap();
        let error = verify_feed(&tampered, &key).unwrap_err().to_string();
        assert!(error.contains("signature verification failed"));
    }

    #[test]
    fn manifests_validate_per_target() {
        assert!(validate_manifest(&manifest(&LINUX), &LINUX).is_ok());
        assert!(validate_manifest(&manifest(&WINDOWS), &WINDOWS).is_ok());
        // A manifest for one platform never validates for the other.
        assert!(validate_manifest(&manifest(&LINUX), &WINDOWS).is_err());
        assert!(validate_manifest(&manifest(&WINDOWS), &LINUX).is_err());
    }

    #[test]
    fn manifests_reject_the_wrong_channel_artifact_and_prereleases() {
        assert!(
            validate_manifest(
                &Manifest {
                    channel: "nightly".into(),
                    ..manifest(&LINUX)
                },
                &LINUX
            )
            .is_err()
        );
        assert!(
            validate_manifest(
                &Manifest {
                    artifact: "../hex".into(),
                    ..manifest(&LINUX)
                },
                &LINUX
            )
            .is_err()
        );
        assert!(
            validate_manifest(
                &Manifest {
                    version: "2.1.0-beta.1".into(),
                    artifact: format!("HEX-2.1.0-beta.1-{}-x86_64-linux", "a".repeat(64)),
                    ..manifest(&LINUX)
                },
                &LINUX
            )
            .is_err()
        );
    }

    #[test]
    fn artifacts_require_the_signed_size_and_checksum() {
        let path = std::env::temp_dir().join(format!(
            "hex-self-update-artifact-test-{}",
            std::process::id()
        ));
        fs::write(&path, b"new").unwrap();
        let mut manifest = Manifest {
            sha256: sha256(&path).unwrap(),
            ..manifest(&LINUX)
        };
        assert!(verify_artifact(&path, &manifest).is_ok());
        manifest.sha256 = "a".repeat(64);
        assert!(verify_artifact(&path, &manifest).is_err());
        manifest.bytes = 4;
        assert!(verify_artifact(&path, &manifest).is_err());
        let _ = fs::remove_file(path);
    }
}
