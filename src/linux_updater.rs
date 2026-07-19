use std::fs::{self, File, OpenOptions};
use std::io::Read;
use std::os::unix::fs::{MetadataExt, OpenOptionsExt, PermissionsExt, symlink};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

use base64::Engine;
use color_eyre::eyre::{Result, WrapErr, bail, eyre};
use ed25519_dalek::{Signature, VerifyingKey};
use fs2::FileExt;
use semver::Version;
use serde::Deserialize;
use sha2::{Digest, Sha256};

const UPDATE_URL: &str = "https://pub-089d681d41754031a4aefa7017d8c2fb.r2.dev/linux-update.json";
const RELEASE_ORIGIN: &str = "https://pub-089d681d41754031a4aefa7017d8c2fb.r2.dev/releases/";
const MAX_FEED_BYTES: u64 = 64 * 1024;
const MAX_ARTIFACT_BYTES: u64 = 512 * 1024 * 1024;
const PUBLIC_KEY: [u8; 32] = [
    0xbf, 0xad, 0x02, 0xe6, 0x22, 0x08, 0xff, 0x14, 0x4b, 0x5c, 0x9d, 0x21, 0xc7, 0xe7, 0x9c, 0x7c,
    0x16, 0xc6, 0x90, 0x42, 0x99, 0xa4, 0x37, 0xd8, 0x57, 0x30, 0x30, 0x07, 0xcd, 0x4f, 0xf7, 0xd8,
];

#[derive(Clone)]
pub struct InstalledUpdate {
    pub executable: PathBuf,
}

#[derive(Deserialize)]
struct SignedFeed {
    payload: String,
    signature: String,
}

#[derive(Debug, Deserialize)]
struct Manifest {
    schema_version: u32,
    channel: String,
    target: String,
    version: String,
    artifact: String,
    bytes: u64,
    sha256: String,
}

pub fn managed_install() -> bool {
    // geteuid has no preconditions and does not mutate process state.
    let user = unsafe { libc::geteuid() };
    if user == 0 {
        return false;
    }
    let Ok(executable) = std::env::current_exe().and_then(|path| path.canonicalize()) else {
        return false;
    };
    let Ok(support) = crate::app_paths::support_dir().and_then(|path| {
        path.canonicalize()
            .wrap_err("could not resolve application support")
    }) else {
        return false;
    };
    let Ok(versions) = support.join("versions").canonicalize() else {
        return false;
    };
    let Some(parent) = executable.parent() else {
        return false;
    };
    executable.starts_with(&versions)
        && [support.as_path(), versions.as_path(), parent]
            .into_iter()
            .all(|path| safe_owned_directory(path, user))
}

pub fn install_latest() -> Result<Option<InstalledUpdate>> {
    if !managed_install() {
        return Ok(None);
    }
    let support = crate::app_paths::support_dir()?;
    let updates = support.join("updates");
    fs::create_dir_all(&updates)?;
    fs::set_permissions(&updates, fs::Permissions::from_mode(0o700))?;
    let lock = OpenOptions::new()
        .create(true)
        .truncate(false)
        .read(true)
        .write(true)
        .mode(0o600)
        .open(updates.join("update.lock"))?;
    lock.try_lock_exclusive()
        .wrap_err("another Linux update is already in progress")?;
    let feed_path = updates.join(format!("linux-update-{}.partial", std::process::id()));
    download(update_url(), &feed_path, MAX_FEED_BYTES)?;
    let feed = read_bounded(&feed_path, MAX_FEED_BYTES)?;
    let _ = fs::remove_file(&feed_path);
    let manifest = verify_feed(&feed, &PUBLIC_KEY)?;
    validate_manifest(&manifest)?;
    let installed = Version::parse(env!("CARGO_PKG_VERSION"))?;
    let available = Version::parse(&manifest.version)
        .wrap_err("Linux update manifest contains an invalid version")?;
    if available < installed {
        bail!("Linux update feed attempted a downgrade");
    }
    if available == installed {
        return Ok(None);
    }

    let artifact_url = format!("{RELEASE_ORIGIN}{}", manifest.artifact);
    let partial = updates.join(format!(
        "{}-{}.download",
        manifest.version,
        std::process::id()
    ));
    download(&artifact_url, &partial, manifest.bytes)?;
    let activation = (|| {
        verify_artifact(&partial, &manifest)?;
        validate_executable(&partial, &manifest.version)?;
        activate(&support, &manifest.version, &partial)
    })();
    if activation.is_err() {
        let _ = fs::remove_file(&partial);
    }
    let executable = activation?;
    Ok(Some(InstalledUpdate { executable }))
}

fn validate_manifest(manifest: &Manifest) -> Result<()> {
    if manifest.schema_version != 1 {
        bail!(
            "unsupported Linux update schema {}",
            manifest.schema_version
        );
    }
    if manifest.channel != "stable" {
        bail!("unsupported Linux update channel");
    }
    if manifest.target != "x86_64-unknown-linux-gnu" {
        bail!("Linux update targets the wrong platform");
    }
    validate_artifact_name(&manifest.artifact)?;
    if manifest.bytes == 0 || manifest.bytes > MAX_ARTIFACT_BYTES {
        bail!("Linux update artifact has an invalid size");
    }
    if manifest.sha256.len() != 64
        || !manifest
            .sha256
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
    {
        bail!("Linux update artifact has an invalid checksum");
    }
    if manifest.artifact != format!("HEX-{}-{}-x86_64-linux", manifest.version, manifest.sha256) {
        bail!("Linux update artifact is not content-addressed");
    }
    Version::parse(&manifest.version)
        .wrap_err("Linux update manifest contains an invalid version")?
        .pre
        .is_empty()
        .then_some(())
        .ok_or_else(|| eyre!("stable Linux updates cannot be prereleases"))?;
    Ok(())
}

pub fn relaunch(update: &InstalledUpdate) -> Result<()> {
    let pid = std::process::id().to_string();
    Command::new("/bin/sh")
        .args([
            "-c",
            "i=0; while kill -0 \"$1\" 2>/dev/null && [ \"$i\" -lt 300 ]; do sleep 0.1; i=$((i + 1)); done; exec \"$2\" app",
            "hex-restart",
            &pid,
        ])
        .arg(&update.executable)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .wrap_err("could not schedule the updated HEX version")?;
    Ok(())
}

fn update_url() -> &'static str {
    option_env!("HEX_LINUX_UPDATE_URL").unwrap_or(UPDATE_URL)
}

fn verify_feed(bytes: &[u8], public_key: &[u8; 32]) -> Result<Manifest> {
    let feed: SignedFeed = serde_json::from_slice(bytes).wrap_err("invalid Linux update feed")?;
    let payload = base64::engine::general_purpose::STANDARD
        .decode(feed.payload)
        .wrap_err("invalid Linux update payload encoding")?;
    let signature = base64::engine::general_purpose::STANDARD
        .decode(feed.signature)
        .wrap_err("invalid Linux update signature encoding")?;
    let signature = Signature::from_slice(&signature).wrap_err("invalid Linux update signature")?;
    let key = VerifyingKey::from_bytes(public_key).wrap_err("invalid Linux update public key")?;
    key.verify_strict(&payload, &signature)
        .wrap_err("Linux update signature verification failed")?;
    serde_json::from_slice(&payload).wrap_err("invalid signed Linux update manifest")
}

fn validate_artifact_name(name: &str) -> Result<()> {
    if name.is_empty()
        || name.contains("..")
        || name
            .bytes()
            .any(|byte| !byte.is_ascii_alphanumeric() && !matches!(byte, b'-' | b'_' | b'.'))
    {
        bail!("Linux update artifact name is invalid");
    }
    Ok(())
}

fn safe_owned_directory(path: &Path, user: u32) -> bool {
    fs::metadata(path).is_ok_and(|metadata| metadata.uid() == user && metadata.mode() & 0o022 == 0)
}

fn download(url: &str, path: &Path, max_bytes: u64) -> Result<()> {
    let _ = fs::remove_file(path);
    let result = (|| {
        let mut destination = OpenOptions::new()
            .create_new(true)
            .write(true)
            .mode(0o600)
            .open(path)?;
        let mut child = Command::new("curl")
            .args([
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
            ])
            .arg(url)
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .spawn()
            .wrap_err("could not start Linux update download")?;
        let copied = std::io::copy(
            &mut child
                .stdout
                .take()
                .ok_or_else(|| eyre!("Linux update download has no output"))?
                .take(max_bytes + 1),
            &mut destination,
        )?;
        if copied > max_bytes {
            let _ = child.kill();
            let _ = child.wait();
            bail!("Linux update download exceeded its signed size limit");
        }
        let status = child.wait()?;
        if !status.success() {
            bail!("Linux update download failed with {status}");
        }
        destination.sync_all()?;
        Ok(())
    })();
    if result.is_err() {
        let _ = fs::remove_file(path);
    }
    result
}

fn read_bounded(path: &Path, max_bytes: u64) -> Result<Vec<u8>> {
    let mut bytes = Vec::new();
    File::open(path)?
        .take(max_bytes + 1)
        .read_to_end(&mut bytes)?;
    if bytes.len() as u64 > max_bytes {
        bail!("Linux update feed is too large");
    }
    Ok(bytes)
}

fn verify_artifact(path: &Path, manifest: &Manifest) -> Result<()> {
    let metadata = fs::metadata(path)?;
    if metadata.len() != manifest.bytes {
        bail!(
            "Linux update has {} bytes, expected {}",
            metadata.len(),
            manifest.bytes
        );
    }
    let actual = sha256(path)?;
    if actual != manifest.sha256 {
        bail!("Linux update checksum verification failed");
    }
    Ok(())
}

fn sha256(path: &Path) -> Result<String> {
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

fn validate_executable(path: &Path, version: &str) -> Result<()> {
    fs::set_permissions(path, fs::Permissions::from_mode(0o755))?;
    let output = Command::new(path)
        .arg("--version")
        .stdin(Stdio::null())
        .output()
        .wrap_err("could not start the downloaded Linux update")?;
    if !output.status.success()
        || String::from_utf8_lossy(&output.stdout)
            .split_whitespace()
            .last()
            != Some(version)
    {
        bail!("downloaded Linux update reports the wrong version");
    }
    Ok(())
}

fn activate(support: &Path, version: &str, partial: &Path) -> Result<PathBuf> {
    let versions = support.join("versions");
    let version_dir = versions.join(version);
    fs::create_dir_all(&version_dir)?;
    fs::set_permissions(&versions, fs::Permissions::from_mode(0o700))?;
    fs::set_permissions(&version_dir, fs::Permissions::from_mode(0o700))?;
    let executable = version_dir.join("hex");
    let staged = version_dir.join(format!(".hex-{}.partial", std::process::id()));
    let _ = fs::remove_file(&staged);
    fs::rename(partial, &staged)?;
    fs::set_permissions(&staged, fs::Permissions::from_mode(0o755))?;
    OpenOptions::new().read(true).open(&staged)?.sync_all()?;
    fs::rename(&staged, &executable)?;
    File::open(&version_dir)?.sync_all()?;

    let current = support.join("current");
    let previous = fs::read_link(&current)
        .ok()
        .and_then(|path| path.file_name().map(|name| name.to_owned()));
    let next = support.join(format!(".current-{}.partial", std::process::id()));
    let _ = fs::remove_file(&next);
    symlink(Path::new("versions").join(version), &next)?;
    fs::rename(&next, &current)?;
    File::open(support)?.sync_all()?;
    for entry in fs::read_dir(&versions)? {
        let entry = entry?;
        let name = entry.file_name();
        if name.to_str() != Some(version)
            && previous.as_ref() != Some(&name)
            && name
                .to_str()
                .is_some_and(|name| Version::parse(name).is_ok())
        {
            if let Err(error) = fs::remove_dir_all(entry.path()) {
                tracing::warn!(%error, path = %entry.path().display(), "could not prune old HEX version");
            }
        }
    }
    Ok(current.join("hex"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use ed25519_dalek::{Signer, SigningKey};

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
    fn artifact_names_cannot_escape_the_release_origin() {
        for name in ["../hex", "dir/hex", "https:hex", "hex?old", ""] {
            assert!(validate_artifact_name(name).is_err(), "accepted {name:?}");
        }
        assert!(validate_artifact_name("HEX-2.1.0-x86_64-linux").is_ok());
    }

    #[test]
    fn manifest_rejects_the_wrong_channel_target_and_checksum() {
        let valid = Manifest {
            schema_version: 1,
            channel: "stable".into(),
            target: "x86_64-unknown-linux-gnu".into(),
            version: "2.1.0".into(),
            artifact: format!("HEX-2.1.0-{}-x86_64-linux", "a".repeat(64)),
            bytes: 3,
            sha256: "a".repeat(64),
        };
        assert!(validate_manifest(&valid).is_ok());
        assert!(
            validate_manifest(&Manifest {
                channel: "nightly".into(),
                ..valid
            })
            .is_err()
        );
        assert!(
            validate_manifest(&Manifest {
                schema_version: 1,
                channel: "stable".into(),
                target: "x86_64-unknown-linux-gnu".into(),
                version: "2.1.0-beta.1".into(),
                artifact: format!("HEX-2.1.0-beta.1-{}-x86_64-linux", "a".repeat(64)),
                bytes: 3,
                sha256: "a".repeat(64),
            })
            .is_err()
        );
    }

    #[test]
    fn activation_switches_current_without_removing_the_old_version() {
        let root = std::env::temp_dir().join(format!(
            "hex-updater-test-{}-{}",
            std::process::id(),
            std::thread::current().name().unwrap_or("activation")
        ));
        let _ = fs::remove_dir_all(&root);
        fs::create_dir_all(root.join("versions/1.0.0")).unwrap();
        fs::write(root.join("versions/1.0.0/hex"), b"old").unwrap();
        fs::create_dir_all(root.join("versions/0.9.0")).unwrap();
        fs::write(root.join("versions/0.9.0/hex"), b"older").unwrap();
        symlink("versions/1.0.0", root.join("current")).unwrap();
        let partial = root.join("update.download");
        fs::write(&partial, b"new").unwrap();

        let executable = activate(&root, "1.1.0", &partial).unwrap();
        assert_eq!(fs::read(executable).unwrap(), b"new");
        assert_eq!(fs::read(root.join("versions/1.0.0/hex")).unwrap(), b"old");
        assert!(!root.join("versions/0.9.0").exists());
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn artifact_requires_the_signed_size_and_checksum() {
        let path =
            std::env::temp_dir().join(format!("hex-updater-artifact-test-{}", std::process::id()));
        fs::write(&path, b"new").unwrap();
        let mut manifest = Manifest {
            schema_version: 1,
            channel: "stable".into(),
            target: "x86_64-unknown-linux-gnu".into(),
            version: "2.1.0".into(),
            artifact: format!("HEX-2.1.0-{}-x86_64-linux", "a".repeat(64)),
            bytes: 3,
            sha256: sha256(&path).unwrap(),
        };
        assert!(verify_artifact(&path, &manifest).is_ok());
        manifest.sha256 = "a".repeat(64);
        assert!(verify_artifact(&path, &manifest).is_err());
        manifest.bytes = 4;
        assert!(verify_artifact(&path, &manifest).is_err());
        let _ = fs::remove_file(path);
    }
}
