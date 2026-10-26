//! Linux installs: the user-local version layout our installer receipts
//! (`<support>/install.json`) updates itself by activating a new version
//! directory and switching the `current` symlink atomically. Source builds
//! (the `source-build` marker) and package-managed executables (pacman,
//! Nix) never replace themselves. Linux has no OS code signature to check:
//! HTTPS and the manifest's SHA-256 are the only integrity checks.

use std::fs::{self, File, OpenOptions};
use std::os::unix::fs::{MetadataExt, OpenOptionsExt, PermissionsExt, symlink};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

use color_eyre::eyre::{Result, WrapErr, bail};
use fs2::FileExt;

use super::Method;
use super::protocol::Version;

pub const TARGET: &str = "linux-x64";
pub const DOWNLOAD_TARGET: &str = "linux-x64";

/// The marker `scripts/install-linux.sh` leaves beside an executable it
/// built from a checkout: that install updates from its checkout.
const SOURCE_MARKER: &str = "source-build";

/// The activated executable, behind the switched `current` link.
#[derive(Clone, Debug)]
pub struct Staged(PathBuf);

pub fn method() -> Method {
    // geteuid has no preconditions and does not mutate process state.
    let user = unsafe { libc::geteuid() };
    let Ok(executable) = std::env::current_exe().and_then(|path| path.canonicalize()) else {
        return Method::Unknown;
    };
    if let Some((support, version_dir)) = installed_layout(&executable, user) {
        if version_dir.join(SOURCE_MARKER).exists() {
            return Method::Source;
        }
        return match super::receipt_channel(&support) {
            Some(channel) => Method::Direct { channel },
            None => Method::Unknown,
        };
    }
    if executable.starts_with("/nix/store") {
        return Method::Package("nix");
    }
    if fs::metadata(&executable).is_ok_and(|metadata| metadata.uid() != user)
        && Command::new("pacman")
            .arg("-Qqo")
            .arg(&executable)
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status()
            .is_ok_and(|status| status.success())
    {
        return Method::Package("aur");
    }
    Method::Unknown
}

pub fn updates_disabled_by_policy() -> bool {
    false
}

/// The support directory and this executable's version directory, when it
/// runs from the user-local version layout and no other user can write it.
fn installed_layout(executable: &Path, user: u32) -> Option<(PathBuf, PathBuf)> {
    if user == 0 {
        return None;
    }
    let support = crate::app_paths::support_dir().ok()?.canonicalize().ok()?;
    let versions = support.join("versions").canonicalize().ok()?;
    let parent = executable.parent()?;
    (executable.starts_with(&versions)
        && [support.as_path(), versions.as_path(), parent]
            .into_iter()
            .all(|path| safe_owned_directory(path, user)))
    .then(|| (support.clone(), parent.to_path_buf()))
}

fn safe_owned_directory(path: &Path, user: u32) -> bool {
    fs::metadata(path).is_ok_and(|metadata| metadata.uid() == user && metadata.mode() & 0o022 == 0)
}

pub fn stage(file: &Path, version: &str) -> Result<Staged> {
    let support = crate::app_paths::support_dir()?;
    let lock = OpenOptions::new()
        .create(true)
        .truncate(false)
        .read(true)
        .write(true)
        .mode(0o600)
        .open(support.join("updates").join("update.lock"))?;
    lock.try_lock_exclusive()
        .wrap_err("another Linux update is already in progress")?;
    let staging = support.join("updates").join("hex.staging");
    let _ = fs::remove_file(&staging);
    fs::copy(file, &staging)?;
    let activation =
        validate_executable(&staging, version).and_then(|()| activate(&support, version, &staging));
    if activation.is_err() {
        let _ = fs::remove_file(&staging);
    }
    activation.map(Staged)
}

pub fn restart_into(staged: &Staged) -> Result<()> {
    if !staged.0.is_file() {
        bail!("the prepared Hex update is no longer installed");
    }
    crate::linux_service::schedule_restart()
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
        bail!("the downloaded Linux update reports the wrong version");
    }
    Ok(())
}

/// Stages the executable into its version directory and switches `current`
/// atomically; the running version's files are never touched, and versions
/// older than the previous one are pruned.
fn activate(support: &Path, version: &str, staging: &Path) -> Result<PathBuf> {
    let versions = support.join("versions");
    let version_dir = versions.join(version);
    fs::create_dir_all(&version_dir)?;
    fs::set_permissions(&versions, fs::Permissions::from_mode(0o700))?;
    fs::set_permissions(&version_dir, fs::Permissions::from_mode(0o700))?;
    let executable = version_dir.join("hex");
    let staged = version_dir.join(".hex.partial");
    let _ = fs::remove_file(&staged);
    fs::rename(staging, &staged)?;
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
                .is_some_and(|name| Version::parse(name).is_some())
            && let Err(error) = fs::remove_dir_all(entry.path())
        {
            tracing::warn!(%error, path = %entry.path().display(), "could not prune an old Hex version");
        }
    }
    Ok(current.join("hex"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn activation_switches_current_without_removing_the_old_version() {
        let root = std::env::temp_dir().join(format!("hex-linux-activate-{}", std::process::id()));
        let _ = fs::remove_dir_all(&root);
        fs::create_dir_all(root.join("versions/1.0.0")).unwrap();
        fs::write(root.join("versions/1.0.0/hex"), b"old").unwrap();
        fs::create_dir_all(root.join("versions/0.9.0")).unwrap();
        fs::write(root.join("versions/0.9.0/hex"), b"older").unwrap();
        symlink("versions/1.0.0", root.join("current")).unwrap();
        let staging = root.join("hex.staging");
        fs::write(&staging, b"new").unwrap();

        let executable = activate(&root, "1.1.0", &staging).unwrap();
        assert_eq!(fs::read(executable).unwrap(), b"new");
        assert_eq!(fs::read(root.join("versions/1.0.0/hex")).unwrap(), b"old");
        assert!(!root.join("versions/0.9.0").exists());
        let _ = fs::remove_dir_all(root);
    }
}
