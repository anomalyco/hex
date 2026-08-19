use std::fs::{self, File, OpenOptions};
use std::os::unix::fs::{MetadataExt, OpenOptionsExt, PermissionsExt, symlink};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

use color_eyre::eyre::{Result, WrapErr, bail};
use fs2::FileExt;
use semver::Version;

use crate::self_update::{
    MAX_FEED_BYTES, PUBLIC_KEY, RELEASE_BASE, UpdateTarget, download, read_bounded,
    validate_manifest, verify_artifact, verify_feed,
};

const TARGET: UpdateTarget = UpdateTarget {
    target: "x86_64-unknown-linux-gnu",
    artifact_suffix: "x86_64-linux",
};

#[derive(Clone)]
pub struct InstalledUpdate(PathBuf);

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
    let feed_path = updates.join("linux-update.json.partial");
    download(
        &format!("{RELEASE_BASE}/linux-update.json"),
        &feed_path,
        MAX_FEED_BYTES,
    )?;
    let feed = read_bounded(&feed_path, MAX_FEED_BYTES)?;
    let _ = fs::remove_file(&feed_path);
    let manifest = verify_feed(&feed, &PUBLIC_KEY)?;
    let available = validate_manifest(&manifest, &TARGET)?;
    let installed = Version::parse(env!("CARGO_PKG_VERSION"))?;
    if available < installed {
        bail!("Linux update feed attempted a downgrade");
    }
    if available == installed {
        return Ok(None);
    }

    let artifact_url = format!("{RELEASE_BASE}/releases/{}", manifest.artifact);
    let partial = updates.join("update.download");
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
    Ok(Some(InstalledUpdate(executable)))
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
        .arg(&update.0)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .wrap_err("could not schedule the updated HEX version")?;
    Ok(())
}

fn safe_owned_directory(path: &Path, user: u32) -> bool {
    fs::metadata(path).is_ok_and(|metadata| metadata.uid() == user && metadata.mode() & 0o022 == 0)
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
    let staged = version_dir.join(".hex.partial");
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
    let next = support.join(".current.partial");
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
            && let Err(error) = fs::remove_dir_all(entry.path())
        {
            tracing::warn!(%error, path = %entry.path().display(), "could not prune old HEX version");
        }
    }
    Ok(current.join("hex"))
}

#[cfg(test)]
mod tests {
    use super::*;

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
}
