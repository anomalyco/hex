//! Linux installs: the user-local version layout our installer receipts
//! (`<support>/install.json`) updates itself by activating a new version
//! directory and switching the `current` symlink atomically. Source builds
//! (the `source-build` marker) and package-managed executables (pacman,
//! dpkg, rpm, Nix) never replace themselves. Linux has no OS code signature to check:
//! HTTPS and the manifest's SHA-256 are the only integrity checks.

use std::fs::{self, File, OpenOptions};
use std::os::unix::fs::{MetadataExt, OpenOptionsExt, PermissionsExt, symlink};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

use color_eyre::eyre::{Result, WrapErr, bail};
use fs2::FileExt;

use super::Method;
use super::protocol::is_version_name;

#[cfg(target_arch = "x86_64")]
pub const TARGET: &str = "linux-x64";
#[cfg(target_arch = "aarch64")]
pub const TARGET: &str = "linux-arm64";
pub const DOWNLOAD_TARGET: &str = TARGET;

/// The release service's distribution kinds for this architecture's `.deb`
/// and `.rpm` packages: their update commands download this architecture's
/// package.
#[cfg(target_arch = "x86_64")]
const PACKAGES: [(&str, &[&str], &str); 3] = [
    ("pacman", &["-Qqo"], "aur"),
    ("dpkg-query", &["-S"], "deb-x64"),
    ("rpm", &["-qf"], "rpm-x64"),
];
#[cfg(target_arch = "aarch64")]
const PACKAGES: [(&str, &[&str], &str); 3] = [
    ("pacman", &["-Qqo"], "aur"),
    ("dpkg-query", &["-S"], "deb-arm64"),
    ("rpm", &["-qf"], "rpm-arm64"),
];

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
        && let Some((_, _, kind)) = PACKAGES.iter().find(|(manager, arguments, _)| {
            Command::new(manager)
                .args(*arguments)
                .arg(&executable)
                .stdin(Stdio::null())
                .stdout(Stdio::null())
                .stderr(Stdio::null())
                .status()
                .is_ok_and(|status| status.success())
        })
    {
        return Method::Package(kind);
    }
    Method::Unknown
}

pub fn updates_disabled_by_policy() -> bool {
    false
}

/// The Linux targets are the bare executable, so this build is the file
/// whose SHA-256 it has.
pub fn running_sha256() -> Option<String> {
    crate::transcription_models::sha256_hex(&std::env::current_exe().ok()?, None).ok()
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
/// atomically; the running version's files are never touched, and every
/// version but the new, running, and previous one is pruned. A newer
/// release can replace a staged one, so the previous one is not always the
/// one running.
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
            && name.to_str() != Some(crate::VERSION)
            && previous.as_ref() != Some(&name)
            && name.to_str().is_some_and(is_version_name)
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

    #[test]
    fn replacing_a_staged_update_keeps_the_running_version() {
        let root = std::env::temp_dir().join(format!("hex-linux-restage-{}", std::process::id()));
        let _ = fs::remove_dir_all(&root);
        let running = root.join("versions").join(crate::VERSION);
        fs::create_dir_all(&running).unwrap();
        fs::write(running.join("hex"), b"running").unwrap();
        symlink(
            Path::new("versions").join(crate::VERSION),
            root.join("current"),
        )
        .unwrap();
        for (version, bytes) in [("1.1.0", b"first"), ("1.2.0", b"newer")] {
            let staging = root.join("hex.staging");
            fs::write(&staging, bytes).unwrap();
            activate(&root, version, &staging).unwrap();
        }
        assert_eq!(fs::read(root.join("current/hex")).unwrap(), b"newer");
        assert_eq!(fs::read(running.join("hex")).unwrap(), b"running");
        let _ = fs::remove_dir_all(root);
    }
}
