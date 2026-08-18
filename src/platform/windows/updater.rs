//! Updates for the Windows alpha. A managed install (running from the
//! support directory's `versions` layout) self-updates through the same
//! ed25519-signed feed contract as Linux: verified download, staged
//! activation into a new version directory, a pointer-file switch, and a
//! restart. A source build only checks the public GitHub releases feed
//! and links to the release page.

use std::fs::{self, OpenOptions};
use std::os::windows::process::CommandExt;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::Duration;

use color_eyre::eyre::{Result, WrapErr, bail};
use fs2::FileExt;
use semver::Version;

use crate::self_update::{
    MAX_FEED_BYTES, PUBLIC_KEY, RELEASE_BASE, UpdateTarget, download, read_bounded,
    validate_manifest, verify_artifact, verify_feed,
};

const RELEASES_API: &str = "https://api.github.com/repos/anomalyco/hex/releases/latest";
const RELEASES_PAGE: &str = "https://github.com/anomalyco/hex/releases/latest";
const STARTUP_DELAY: Duration = Duration::from_secs(10);
const CHECK_INTERVAL: Duration = Duration::from_secs(6 * 60 * 60);
const CREATE_NO_WINDOW: u32 = 0x0800_0000;
const TARGET: UpdateTarget = UpdateTarget {
    target: "x86_64-pc-windows-msvc",
    artifact_suffix: "x86_64-windows.exe",
};

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum UpdateCheck {
    Checking,
    Current,
    Available {
        version: String,
        url: String,
    },
    ReadyToRestart {
        version: String,
        executable: PathBuf,
    },
    Failed,
}

#[derive(Clone)]
pub struct WindowsUpdater {
    state: Arc<Mutex<UpdateCheck>>,
    stop: Arc<AtomicBool>,
    check_now: Arc<AtomicBool>,
}

impl WindowsUpdater {
    pub fn start() -> Self {
        let state = Arc::new(Mutex::new(UpdateCheck::Checking));
        let stop = Arc::new(AtomicBool::new(false));
        let check_now = Arc::new(AtomicBool::new(false));
        if std::env::var_os("HEX_MOCK_UPDATE").is_some() {
            *state.lock().unwrap_or_else(|error| error.into_inner()) = UpdateCheck::Available {
                version: "9.9.9".into(),
                url: RELEASES_PAGE.into(),
            };
            return Self {
                state,
                stop,
                check_now,
            };
        }
        let worker_state = state.clone();
        let worker_stop = stop.clone();
        let worker_check_now = check_now.clone();
        let managed = managed_install();
        let _ = thread::Builder::new()
            .name("windows-update-check".into())
            .spawn(move || {
                wait_for_next_check(&worker_stop, &worker_check_now, STARTUP_DELAY);
                while !worker_stop.load(Ordering::Relaxed) {
                    worker_check_now.store(false, Ordering::Relaxed);
                    // A staged update stays ready until the restart; there
                    // is nothing further to check.
                    let ready = matches!(
                        &*worker_state
                            .lock()
                            .unwrap_or_else(|error| error.into_inner()),
                        UpdateCheck::ReadyToRestart { .. }
                    );
                    if !ready {
                        let next = if managed {
                            self_update_check()
                        } else {
                            check_latest_release()
                        };
                        *worker_state
                            .lock()
                            .unwrap_or_else(|error| error.into_inner()) = next;
                    }
                    wait_for_next_check(&worker_stop, &worker_check_now, CHECK_INTERVAL);
                }
            });
        Self {
            state,
            stop,
            check_now,
        }
    }

    /// Run a check immediately instead of waiting out the interval. A
    /// staged update stays ready; only a restart consumes it.
    pub fn request_check(&self) {
        let mut state = self.state.lock().unwrap_or_else(|error| error.into_inner());
        if matches!(&*state, UpdateCheck::ReadyToRestart { .. }) {
            return;
        }
        *state = UpdateCheck::Checking;
        drop(state);
        self.check_now.store(true, Ordering::Relaxed);
    }

    pub fn state(&self) -> UpdateCheck {
        self.state
            .lock()
            .unwrap_or_else(|error| error.into_inner())
            .clone()
    }

    pub fn stop(&self) {
        self.stop.store(true, Ordering::Relaxed);
    }
}

/// A managed install runs from `<support>\versions\<version>\hex.exe`
/// inside the user's own profile; only that layout self-updates. The
/// profile requirement is the Windows analog of the Linux updater's
/// ownership checks: default profile ACLs deny other users write access,
/// while a redirected or machine-wide tree offers no such guarantee, so
/// those installs keep the release-page link behavior, like source
/// builds and unpacked zips.
pub fn managed_install() -> bool {
    let Ok(executable) = std::env::current_exe().and_then(|path| path.canonicalize()) else {
        return false;
    };
    let Ok(support) = crate::app_paths::support_dir() else {
        return false;
    };
    let Some(profile) = dirs::config_dir().and_then(|profile| profile.canonicalize().ok()) else {
        return false;
    };
    let Ok(support) = support.canonicalize() else {
        return false;
    };
    let Ok(versions) = support.join("versions").canonicalize() else {
        return false;
    };
    support.starts_with(&profile) && executable.starts_with(&versions)
}

fn self_update_check() -> UpdateCheck {
    match install_latest() {
        Ok(Some(update)) => UpdateCheck::ReadyToRestart {
            version: update.version,
            executable: update.executable,
        },
        Ok(None) => UpdateCheck::Current,
        Err(error) => {
            tracing::warn!(%error, "Windows self-update failed");
            UpdateCheck::Failed
        }
    }
}

struct InstalledUpdate {
    version: String,
    executable: PathBuf,
}

fn install_latest() -> Result<Option<InstalledUpdate>> {
    let support = crate::app_paths::support_dir()?;
    let updates = support.join("updates");
    fs::create_dir_all(&updates)?;
    let lock = OpenOptions::new()
        .create(true)
        .truncate(false)
        .read(true)
        .write(true)
        .open(updates.join("update.lock"))?;
    lock.try_lock_exclusive()
        .wrap_err("another Windows update is already in progress")?;
    let feed_path = updates.join("windows-update.json.partial");
    download(
        &format!("{RELEASE_BASE}/windows-update.json"),
        &feed_path,
        MAX_FEED_BYTES,
    )?;
    let feed = read_bounded(&feed_path, MAX_FEED_BYTES)?;
    let _ = fs::remove_file(&feed_path);
    let manifest = verify_feed(&feed, &PUBLIC_KEY)?;
    let available = validate_manifest(&manifest, &TARGET)?;
    let installed = Version::parse(env!("CARGO_PKG_VERSION"))?;
    if available < installed {
        bail!("the Windows update feed attempted a downgrade");
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
    if let Err(error) = crate::windows_login_item::repoint(&executable) {
        tracing::warn!(%error, "could not repoint Launch at login to the updated HEX");
    }
    if let Err(error) = repoint_start_menu_shortcut(&executable) {
        tracing::warn!(%error, "could not repoint the Start Menu shortcut to the updated HEX");
    }
    Ok(Some(InstalledUpdate {
        version: manifest.version,
        executable,
    }))
}

/// Retarget the installer-created Start Menu shortcut at the activated
/// version, so pinned and Start Menu launches follow updates. Only a
/// shortcut carrying the installer's managed description is touched.
fn repoint_start_menu_shortcut(executable: &Path) -> Result<()> {
    let Some(appdata) = dirs::config_dir() else {
        return Ok(());
    };
    let shortcut = appdata.join(r"Microsoft\Windows\Start Menu\Programs\HEX.lnk");
    if !shortcut.exists() {
        return Ok(());
    }
    let quoted_link = shortcut.display().to_string().replace('\'', "''");
    let quoted_exe = executable.display().to_string().replace('\'', "''");
    let working = executable
        .parent()
        .map(|parent| parent.display().to_string().replace('\'', "''"))
        .unwrap_or_default();
    let status = Command::new("powershell.exe")
        .args([
            "-NoProfile",
            "-WindowStyle",
            "Hidden",
            "-Command",
            &format!(
                "$s = (New-Object -ComObject WScript.Shell).CreateShortcut('{quoted_link}'); \
                 if ($s.Description -eq 'HEX managed install') {{ \
                 $s.TargetPath = '{quoted_exe}'; \
                 $s.WorkingDirectory = '{working}'; \
                 $s.Save() }}"
            ),
        ])
        .creation_flags(CREATE_NO_WINDOW)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .wrap_err("could not update the Start Menu shortcut")?;
    if !status.success() {
        bail!("the Start Menu shortcut update exited with {status}");
    }
    Ok(())
}

fn validate_executable(path: &Path, version: &str) -> Result<()> {
    let output = Command::new(path)
        .arg("--version")
        .creation_flags(CREATE_NO_WINDOW)
        .stdin(Stdio::null())
        .output()
        .wrap_err("could not start the downloaded Windows update")?;
    if !output.status.success()
        || String::from_utf8_lossy(&output.stdout)
            .split_whitespace()
            .last()
            != Some(version)
    {
        bail!("the downloaded Windows update reports the wrong version");
    }
    Ok(())
}

fn sync_file(path: &Path) -> Result<()> {
    OpenOptions::new().write(true).open(path)?.sync_all()?;
    Ok(())
}

/// Stage the executable into its version directory and switch the
/// `current-version` pointer atomically; the running version's files are
/// never touched, and versions older than the previous one are pruned.
fn activate(support: &Path, version: &str, partial: &Path) -> Result<PathBuf> {
    let versions = support.join("versions");
    let version_dir = versions.join(version);
    fs::create_dir_all(&version_dir)?;
    let executable = version_dir.join("hex.exe");
    let staged = version_dir.join("hex.exe.partial");
    let _ = fs::remove_file(&staged);
    fs::rename(partial, &staged)?;
    // FlushFileBuffers requires write access on Windows, unlike fsync.
    sync_file(&staged)?;
    fs::rename(&staged, &executable)?;

    let pointer = support.join("current-version");
    let previous = fs::read_to_string(&pointer)
        .ok()
        .map(|content| content.trim().to_string());
    let pointer_partial = support.join("current-version.partial");
    fs::write(&pointer_partial, format!("{version}\n"))?;
    sync_file(&pointer_partial)?;
    fs::rename(&pointer_partial, &pointer)?;

    let running = env!("CARGO_PKG_VERSION");
    for entry in fs::read_dir(&versions)? {
        let entry = entry?;
        let name = entry.file_name();
        let Some(name) = name.to_str() else { continue };
        if name != version
            && name != running
            && previous.as_deref() != Some(name)
            && Version::parse(name).is_ok()
            && let Err(error) = fs::remove_dir_all(entry.path())
        {
            tracing::warn!(%error, path = %entry.path().display(), "could not prune old HEX version");
        }
    }
    Ok(executable)
}

/// Hand off to the staged executable once this process exits. The
/// watcher only starts the new version when the old process is actually
/// gone, so a slow shutdown can never produce two running instances.
pub fn relaunch_at(executable: &Path) -> Result<()> {
    let pid = std::process::id();
    let quoted = executable.display().to_string().replace('\'', "''");
    Command::new("powershell.exe")
        .args([
            "-NoProfile",
            "-WindowStyle",
            "Hidden",
            "-Command",
            &format!(
                "Wait-Process -Id {pid} -Timeout 300 -ErrorAction SilentlyContinue; \
                 if (-not (Get-Process -Id {pid} -ErrorAction SilentlyContinue)) {{ \
                 Start-Process -FilePath '{quoted}' -ArgumentList 'app' }}"
            ),
        ])
        .creation_flags(CREATE_NO_WINDOW)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .wrap_err("could not schedule the updated HEX version")?;
    Ok(())
}

fn wait_for_next_check(stop: &AtomicBool, check_now: &AtomicBool, total: Duration) {
    let mut remaining = total;
    while !stop.load(Ordering::Relaxed)
        && !check_now.load(Ordering::Relaxed)
        && remaining > Duration::ZERO
    {
        let step = remaining.min(Duration::from_millis(500));
        thread::sleep(step);
        remaining = remaining.saturating_sub(step);
    }
}

/// One bounded, unauthenticated query through the curl that ships with
/// Windows; any failure (offline, rate limit, private repository) reports
/// `Failed` and the next interval retries.
fn check_latest_release() -> UpdateCheck {
    let output = Command::new("curl.exe")
        .args([
            "--silent",
            "--fail",
            "--location",
            "--max-time",
            "20",
            "--header",
            "User-Agent: hex-windows-updater",
            "--header",
            "Accept: application/vnd.github+json",
            RELEASES_API,
        ])
        .creation_flags(CREATE_NO_WINDOW)
        .output();
    let Ok(output) = output else {
        return UpdateCheck::Failed;
    };
    if !output.status.success() {
        return UpdateCheck::Failed;
    }
    let Ok(release) = serde_json::from_slice::<serde_json::Value>(&output.stdout) else {
        return UpdateCheck::Failed;
    };
    let Some(tag) = release.get("tag_name").and_then(|tag| tag.as_str()) else {
        return UpdateCheck::Failed;
    };
    let latest = tag.trim_start_matches(['v', 'V']);
    let Some(latest_version) = parse_version(latest) else {
        return UpdateCheck::Failed;
    };
    let current =
        parse_version(env!("CARGO_PKG_VERSION")).expect("the crate version is well-formed");
    if latest_version > current {
        let url = release
            .get("html_url")
            .and_then(|url| url.as_str())
            .unwrap_or(RELEASES_PAGE)
            .to_string();
        UpdateCheck::Available {
            version: latest.to_string(),
            url,
        }
    } else {
        UpdateCheck::Current
    }
}

fn parse_version(text: &str) -> Option<(u64, u64, u64)> {
    let mut parts = text.split(['.', '-', '+']);
    let major = parts.next()?.parse().ok()?;
    let minor = parts.next()?.parse().ok()?;
    let patch = parts.next().and_then(|part| part.parse().ok()).unwrap_or(0);
    Some((major, minor, patch))
}

#[cfg(test)]
mod tests {
    use super::parse_version;
    use super::*;

    #[test]
    fn activation_switches_the_pointer_without_removing_the_old_version() {
        let root =
            std::env::temp_dir().join(format!("hex-win-updater-test-{}", std::process::id()));
        let _ = fs::remove_dir_all(&root);
        fs::create_dir_all(root.join("versions/1.0.0")).unwrap();
        fs::write(root.join("versions/1.0.0/hex.exe"), b"old").unwrap();
        fs::create_dir_all(root.join("versions/0.9.0")).unwrap();
        fs::write(root.join("versions/0.9.0/hex.exe"), b"older").unwrap();
        fs::write(root.join("current-version"), "1.0.0\n").unwrap();
        let partial = root.join("update.download");
        fs::write(&partial, b"new").unwrap();

        let executable = activate(&root, "1.1.0", &partial).unwrap();

        assert_eq!(fs::read(&executable).unwrap(), b"new");
        assert_eq!(
            fs::read_to_string(root.join("current-version"))
                .unwrap()
                .trim(),
            "1.1.0"
        );
        assert_eq!(
            fs::read(root.join("versions/1.0.0/hex.exe")).unwrap(),
            b"old"
        );
        assert!(!root.join("versions/0.9.0").exists());
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn versions_compare_numerically() {
        assert!(parse_version("2.10.0") > parse_version("2.9.9"));
        assert!(parse_version("3.0") > parse_version("2.99.99"));
        assert_eq!(parse_version("2.1.0"), Some((2, 1, 0)));
    }

    #[test]
    fn tags_with_suffixes_still_parse() {
        assert_eq!(parse_version("2.2.0-beta"), Some((2, 2, 0)));
        assert_eq!(parse_version("nonsense"), None);
    }
}
