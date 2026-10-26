//! macOS installs: a `Hex.app` this user can replace updates itself. The
//! update ZIP is unpacked beside the installed bundle (same volume), its code
//! signature must satisfy Anomaly's Developer ID requirement for
//! `anomaly.hex` and be notarized, and on "Restart to update" (or quit) the
//! two bundles swap atomically (`renamex_np` with `RENAME_SWAP`). The old
//! bundle stays beside it until the new version launches and removes it. A
//! read-only, translocated, or root-owned location links to the DMG instead
//! of escalating. Administrators turn updates off with `DisableUpdates` in
//! the `anomaly.hex` defaults domain.

use std::ffi::CString;
use std::fs;
use std::os::unix::ffi::OsStrExt;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

use color_eyre::eyre::{Result, WrapErr, bail, eyre};
use objc2_foundation::{NSString, NSUserDefaults};

use super::Method;

pub const TARGET: &str = "macos-arm64";
pub const DOWNLOAD_TARGET: &str = "macos-arm64-dmg";
const BUNDLE_ID: &str = "anomaly.hex";
/// A Developer ID application from Anomaly's team (5NZ4Q7NXJ4), identified
/// as `anomaly.hex`, and notarized. The release build checks its artifacts
/// against the same file.
const REQUIREMENT: &str = include_str!("../../app/update-requirement.txt");
/// Unpacked updates (and, after a swap, the replaced bundle) live in
/// `.<name>-update-<version>` beside the installed bundle.
const STAGING_PREFIX: &str = ".Hex-update-";

/// An unpacked, verified bundle and the installed bundle it replaces.
#[derive(Clone, Debug)]
pub struct Staged {
    update: PathBuf,
    installed: PathBuf,
}

/// The running `Hex.app`, when this executable is inside one.
fn installed_bundle() -> Option<PathBuf> {
    let executable = std::env::current_exe().ok()?.canonicalize().ok()?;
    let bundle = executable.parent()?.parent()?.parent()?;
    (bundle
        .extension()
        .is_some_and(|extension| extension == "app")
        && bundle.join("Contents/Info.plist").is_file())
    .then(|| bundle.to_path_buf())
}

pub fn method() -> Method {
    let Some(bundle) = installed_bundle() else {
        // A bare binary from a checkout.
        return Method::Source;
    };
    if replaceable(&bundle) {
        Method::Direct {
            channel: "stable".into(),
        }
    } else {
        Method::DownloadOnly
    }
}

/// Hex replaces the bundle by renaming it inside its parent directory, so
/// both must be writable by this user, on a writable volume, and not a
/// Gatekeeper App Translocation copy.
fn replaceable(bundle: &Path) -> bool {
    let writable = |path: &Path| {
        CString::new(path.as_os_str().as_bytes())
            // access has no side effects; the string is NUL-terminated.
            .is_ok_and(|path| unsafe { libc::access(path.as_ptr(), libc::W_OK) } == 0)
    };
    !bundle
        .components()
        .any(|component| component.as_os_str() == "AppTranslocation")
        && bundle.parent().is_some_and(writable)
        && writable(bundle)
}

pub fn updates_disabled_by_policy() -> bool {
    NSUserDefaults::standardUserDefaults().boolForKey(&NSString::from_str("DisableUpdates"))
}

/// Removes what an earlier update left beside the bundle: the replaced
/// bundle once this (new) version runs, or an unpacked update never applied.
pub fn finish_on_launch() {
    let Some(parent) = installed_bundle().and_then(|bundle| bundle.parent().map(Path::to_path_buf))
    else {
        return;
    };
    remove_staging(&parent);
}

fn remove_staging(parent: &Path) {
    let Ok(entries) = fs::read_dir(parent) else {
        return;
    };
    for entry in entries.flatten() {
        if entry
            .file_name()
            .to_str()
            .is_some_and(|name| name.starts_with(STAGING_PREFIX))
            && let Err(error) = fs::remove_dir_all(entry.path())
        {
            tracing::warn!(%error, path = %entry.path().display(), "could not remove an earlier Hex update");
        }
    }
}

pub fn stage(archive: &Path, version: &str) -> Result<Staged> {
    let installed = installed_bundle().ok_or_else(|| eyre!("Hex is not running from Hex.app"))?;
    let parent = installed
        .parent()
        .ok_or_else(|| eyre!("Hex.app has no parent folder"))?;
    let staging = parent.join(format!("{STAGING_PREFIX}{version}"));
    unpack(archive, &staging)?;
    let update = staging.join("Hex.app");
    let result = verify_bundle(&update, version, REQUIREMENT);
    if let Err(error) = result {
        let _ = fs::remove_dir_all(&staging);
        return Err(error);
    }
    Ok(Staged { update, installed })
}

fn unpack(archive: &Path, staging: &Path) -> Result<()> {
    if staging.exists() {
        fs::remove_dir_all(staging)?;
    }
    fs::create_dir(staging)?;
    let status = Command::new("/usr/bin/ditto")
        .arg("-x")
        .arg("-k")
        .arg(archive)
        .arg(staging)
        .stdin(Stdio::null())
        .status()
        .wrap_err("could not run ditto")?;
    if !status.success() {
        let _ = fs::remove_dir_all(staging);
        bail!("could not unpack the update ({status})");
    }
    Ok(())
}

/// Checks the unpacked bundle's identity, version, and code signature
/// against `requirement` before anything runs from it.
fn verify_bundle(bundle: &Path, version: &str, requirement: &str) -> Result<()> {
    let plist = bundle.join("Contents/Info.plist");
    let value = |key: &str| -> Result<String> {
        let output = Command::new("/usr/libexec/PlistBuddy")
            .arg("-c")
            .arg(format!("Print :{key}"))
            .arg(&plist)
            .stdin(Stdio::null())
            .output()?;
        if !output.status.success() {
            bail!("the update's Info.plist has no {key}");
        }
        Ok(String::from_utf8_lossy(&output.stdout).trim().to_owned())
    };
    if value("CFBundleIdentifier")? != BUNDLE_ID {
        bail!("the update is not {BUNDLE_ID}");
    }
    if value("CFBundleShortVersionString")? != version {
        bail!("the update is not version {version}");
    }
    let output = Command::new("/usr/bin/codesign")
        .args(["--verify", "--deep", "--strict"])
        .arg(format!("-R={}", requirement.trim()))
        .arg(bundle)
        .stdin(Stdio::null())
        .output()
        .wrap_err("could not run codesign")?;
    if !output.status.success() {
        bail!(
            "the update's code signature is not Anomaly's notarized Developer ID: {}",
            String::from_utf8_lossy(&output.stderr).trim()
        );
    }
    Ok(())
}

/// Swaps the two bundles in one step: afterwards the installed path holds
/// the update and the staging folder the previous version.
fn swap(staged: &Staged) -> Result<()> {
    let from = CString::new(staged.update.as_os_str().as_bytes())?;
    let to = CString::new(staged.installed.as_os_str().as_bytes())?;
    // Both paths are NUL-terminated and outlive the call.
    if unsafe { libc::renamex_np(from.as_ptr(), to.as_ptr(), libc::RENAME_SWAP) } != 0 {
        return Err(std::io::Error::last_os_error())
            .wrap_err("could not swap in the updated Hex.app");
    }
    Ok(())
}

/// Swaps in the update and opens it once this process has exited.
pub fn restart_into(staged: &Staged) -> Result<()> {
    swap(staged)?;
    Command::new("/bin/sh")
        .args([
            "-c",
            "while kill -0 \"$HEX_PID\" 2>/dev/null; do sleep 0.2; done; exec /usr/bin/open \"$HEX_BUNDLE\"",
        ])
        .env("HEX_PID", std::process::id().to_string())
        .env("HEX_BUNDLE", &staged.installed)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .wrap_err("could not schedule the updated Hex")?;
    Ok(())
}

/// Quitting with an update ready installs it for the next launch.
pub fn install_on_quit(staged: &Staged) {
    if let Err(error) = swap(staged) {
        tracing::warn!(error = %format!("{error:#}"), "could not install the update at quit");
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temporary(name: &str) -> PathBuf {
        let path =
            std::env::temp_dir().join(format!("hex-macos-update-{name}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&path);
        fs::create_dir_all(&path).unwrap();
        path.canonicalize().unwrap()
    }

    #[test]
    fn the_developer_id_requirement_compiles() {
        let status = Command::new("/usr/bin/csreq")
            .arg(format!("-r={}", REQUIREMENT.trim()))
            .arg("-t")
            .stdout(Stdio::null())
            .status()
            .unwrap();
        assert!(status.success());
    }

    /// A minimal ad-hoc signed `Hex.app` with `identifier` and `version`.
    fn bundle(root: &Path, identifier: &str, version: &str) -> PathBuf {
        let bundle = root.join("Hex.app");
        fs::create_dir_all(bundle.join("Contents/MacOS")).unwrap();
        fs::copy("/usr/bin/true", bundle.join("Contents/MacOS/hex")).unwrap();
        fs::write(
            bundle.join("Contents/Info.plist"),
            format!(
                "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n<plist version=\"1.0\"><dict>\
                 <key>CFBundleIdentifier</key><string>{identifier}</string>\
                 <key>CFBundleExecutable</key><string>hex</string>\
                 <key>CFBundleShortVersionString</key><string>{version}</string>\
                 </dict></plist>"
            ),
        )
        .unwrap();
        let status = Command::new("/usr/bin/codesign")
            .args(["--force", "--sign", "-", "--identifier", identifier])
            .arg(&bundle)
            .status()
            .unwrap();
        assert!(status.success());
        bundle
    }

    #[test]
    fn only_the_expected_signature_identity_and_version_pass() {
        let root = temporary("verify");
        let app = bundle(&root, BUNDLE_ID, "9.9.9");
        // An ad-hoc signature satisfies a plain identifier requirement…
        verify_bundle(&app, "9.9.9", "identifier \"anomaly.hex\"").unwrap();
        // …but never Anomaly's Developer ID requirement.
        let error = verify_bundle(&app, "9.9.9", REQUIREMENT).unwrap_err();
        assert!(error.to_string().contains("code signature"), "{error}");
        assert!(verify_bundle(&app, "9.9.8", "identifier \"anomaly.hex\"").is_err());
        let other = bundle(&root.join("other"), "com.kitlangton.hex2", "9.9.9");
        assert!(verify_bundle(&other, "9.9.9", "identifier \"com.kitlangton.hex2\"").is_err());
        // A modified bundle no longer satisfies its signature.
        fs::write(app.join("Contents/MacOS/hex"), b"tampered").unwrap();
        assert!(verify_bundle(&app, "9.9.9", "identifier \"anomaly.hex\"").is_err());
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn the_swap_exchanges_bundles_and_launch_cleanup_removes_the_old_one() {
        let root = temporary("swap");
        let installed = root.join("Hex.app");
        fs::create_dir_all(installed.join("Contents")).unwrap();
        fs::write(installed.join("Contents/version"), "old").unwrap();
        let staging = root.join(format!("{STAGING_PREFIX}9.9.9"));
        let update = staging.join("Hex.app");
        fs::create_dir_all(update.join("Contents")).unwrap();
        fs::write(update.join("Contents/version"), "new").unwrap();

        swap(&Staged {
            update: update.clone(),
            installed: installed.clone(),
        })
        .unwrap();
        assert_eq!(
            fs::read_to_string(installed.join("Contents/version")).unwrap(),
            "new"
        );
        assert_eq!(
            fs::read_to_string(update.join("Contents/version")).unwrap(),
            "old"
        );

        remove_staging(&root);
        assert!(!staging.exists());
        assert!(installed.exists());
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn a_zip_unpacks_beside_the_bundle_and_replaces_older_staging() {
        let root = temporary("unpack");
        let app = bundle(&root.join("source"), BUNDLE_ID, "9.9.9");
        let archive = root.join("Hex.zip");
        let status = Command::new("/usr/bin/ditto")
            .args(["-c", "-k", "--keepParent"])
            .arg(&app)
            .arg(&archive)
            .status()
            .unwrap();
        assert!(status.success());
        let staging = root.join(format!("{STAGING_PREFIX}9.9.9"));
        fs::create_dir_all(staging.join("stale")).unwrap();
        unpack(&archive, &staging).unwrap();
        assert!(!staging.join("stale").exists());
        verify_bundle(
            &staging.join("Hex.app"),
            "9.9.9",
            "identifier \"anomaly.hex\"",
        )
        .unwrap();
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn translocated_and_read_only_locations_are_not_replaceable() {
        let root = temporary("replaceable");
        let app = root.join("Hex.app");
        fs::create_dir_all(&app).unwrap();
        assert!(replaceable(&app));
        assert!(!replaceable(&root.join("AppTranslocation/x/Hex.app")));
        assert!(!replaceable(Path::new("/System/Applications/Hex.app")));
        let _ = fs::remove_dir_all(root);
    }
}
