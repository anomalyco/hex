//! Hex's in-app updater on every OS, a client of the Anomaly release service
//! (`docs/plans/release-service.md`). One worker polls the channel's manifest
//! 10 seconds after launch, then hourly or on "Check for updates", decides with the
//! protocol's client rules, and for a direct install downloads the target,
//! checks its size, SHA-256, and OS code signature, and stages it until the
//! user restarts. Package-managed installs only learn the command to run;
//! source builds, test builds, and installs whose administrator set
//! `DisableUpdates` never check. Manifests are not signed: HTTPS, the
//! manifest's SHA-256, and (macOS, Windows) the code signature are the
//! integrity checks.

#[cfg_attr(target_os = "linux", path = "updater/linux.rs")]
#[cfg_attr(target_os = "macos", path = "updater/macos.rs")]
#[cfg_attr(target_os = "windows", path = "updater/windows.rs")]
mod platform;
pub mod protocol;
mod transport;

use std::path::{Path, PathBuf};
use std::sync::{Arc, Condvar, Mutex};
use std::time::Duration;

use color_eyre::eyre::{Result, WrapErr, eyre};

use crate::desktop_host::{DesktopAction, DesktopUpdateStatus};
use protocol::{Decision, Install};

use platform::Staged;
#[cfg(target_os = "macos")]
pub use platform::finish_on_launch;

const APP: &str = "hex";
const ARTIFACT: &str = "desktop";
const ORIGIN: &str = "https://anoma.ly/release";
const STARTUP_DELAY: Duration = Duration::from_secs(10);
const CHECK_INTERVAL: Duration = Duration::from_secs(60 * 60);
const MAX_BACKOFF: Duration = Duration::from_secs(6 * 60 * 60);

/// How this install receives updates, which decides what the updater does.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum Method {
    /// Our installer's receipt (or, on macOS, a writable `Hex.app`): Hex
    /// downloads and stages updates itself on this channel.
    Direct { channel: String },
    /// A direct install Hex cannot replace (macOS: a read-only or translocated
    /// location); updates link to the download.
    DownloadOnly,
    /// A package manager (`aur`, `nix`, `brew`, `winget`, `scoop`) updates
    /// it; Hex shows its command once it serves a newer version.
    #[cfg_attr(not(target_os = "linux"), allow(dead_code))]
    Package(&'static str),
    /// Built from a checkout, which updates it.
    Source,
    /// Anything else, such as a downloaded executable run in place.
    #[cfg_attr(target_os = "macos", allow(dead_code))]
    Unknown,
}

impl Method {
    fn distribution(&self) -> &str {
        match self {
            Self::Direct { .. } | Self::DownloadOnly => "direct",
            Self::Package(kind) => kind,
            Self::Source | Self::Unknown => "unknown",
        }
    }

    fn channel(&self) -> &str {
        match self {
            Self::Direct { channel } => channel,
            _ => "stable",
        }
    }
}

#[derive(Clone, Debug)]
enum State {
    Disabled(DesktopUpdateStatus),
    Checking,
    Current,
    Failed,
    Downloading,
    Ready(Staged),
    /// Switched over; the restart is under way.
    Applied,
    /// A newer release this install cannot apply itself.
    Download {
        url: String,
    },
    /// The package manager serves a newer version.
    Command {
        command: String,
    },
}

struct Shared {
    state: Mutex<State>,
    /// A package manager installed Hex, so "up to date" is its to say.
    packaged: bool,
    /// Set by "Check for updates" and `stop`; the worker waits on it between checks.
    wake: Mutex<Wake>,
    signal: Condvar,
}

#[derive(Default)]
struct Wake {
    check: bool,
    stop: bool,
}

#[derive(Clone)]
pub struct Updater {
    shared: Arc<Shared>,
}

impl Updater {
    pub fn start() -> Self {
        let method = platform::method();
        let disabled = if option_env!("HEX_TEST_BUILD") == Some("1") {
            Some(DesktopUpdateStatus::Unavailable)
        } else if managed_by_policy() {
            Some(DesktopUpdateStatus::Managed)
        } else if method == Method::Source {
            Some(DesktopUpdateStatus::SourceBuild)
        } else {
            None
        };
        let checks = disabled.is_none();
        let shared = Arc::new(Shared {
            state: Mutex::new(match disabled {
                Some(status) => State::Disabled(status),
                None => State::Checking,
            }),
            packaged: matches!(method, Method::Package(_)),
            wake: Mutex::new(Wake::default()),
            signal: Condvar::new(),
        });
        if checks {
            let worker = shared.clone();
            if let Err(error) = std::thread::Builder::new()
                .name("updater".into())
                .spawn(move || run(&worker, &method))
            {
                tracing::error!(%error, "could not start the updater");
                *lock(&shared.state) = State::Failed;
            }
        }
        Self { shared }
    }

    /// An updater that never checks, for tests of the hosts that own one.
    #[cfg(all(test, target_os = "linux"))]
    pub fn idle() -> Self {
        Self {
            shared: Arc::new(Shared {
                state: Mutex::new(State::Disabled(DesktopUpdateStatus::Unavailable)),
                packaged: false,
                wake: Mutex::new(Wake::default()),
                signal: Condvar::new(),
            }),
        }
    }

    /// Checks now instead of at the next interval. A staged update stays
    /// ready until the restart.
    pub fn check_now(&self) {
        let mut state = lock(&self.shared.state);
        if matches!(
            &*state,
            State::Disabled(_) | State::Ready(_) | State::Applied | State::Downloading
        ) {
            return;
        }
        *state = State::Checking;
        drop(state);
        lock(&self.shared.wake).check = true;
        self.shared.signal.notify_all();
    }

    pub fn stop(&self) {
        lock(&self.shared.wake).stop = true;
        self.shared.signal.notify_all();
    }

    pub fn status(&self) -> DesktopUpdateStatus {
        match &*lock(&self.shared.state) {
            State::Disabled(status) => status.clone(),
            State::Checking => DesktopUpdateStatus::Checking,
            State::Current if self.shared.packaged => DesktopUpdateStatus::PackageManager,
            State::Current => DesktopUpdateStatus::Current,
            State::Failed => DesktopUpdateStatus::Failed,
            State::Downloading => DesktopUpdateStatus::Downloading,
            State::Ready(_) | State::Applied => DesktopUpdateStatus::ReadyToRestart,
            State::Download { .. } => DesktopUpdateStatus::Available,
            State::Command { command } => DesktopUpdateStatus::PackageUpdate {
                command: command.clone(),
            },
        }
    }

    /// Carries out an update control: check now, open the download, or
    /// switch to the staged update. True when Hex must now quit so the
    /// update can start.
    pub fn act(&self, action: &DesktopAction) -> Result<bool> {
        match action {
            DesktopAction::CheckForUpdates => self.check_now(),
            DesktopAction::OpenUpdate => self.open_download(),
            DesktopAction::RestartIntoUpdate => {
                self.restart_into_update()?;
                return Ok(true);
            }
            _ => {}
        }
        Ok(false)
    }

    /// The action of a single "update" menu item, which follows the status.
    #[cfg(target_os = "macos")]
    pub fn menu_action(&self) -> DesktopAction {
        match self.status() {
            DesktopUpdateStatus::ReadyToRestart => DesktopAction::RestartIntoUpdate,
            DesktopUpdateStatus::Available => DesktopAction::OpenUpdate,
            _ => DesktopAction::CheckForUpdates,
        }
    }

    /// Opens the download an install that cannot update itself needs.
    fn open_download(&self) {
        let State::Download { url } = lock(&self.shared.state).clone() else {
            return;
        };
        if let Err(error) = crate::commands::execute(crate::commands::Action::OpenUrl(url)) {
            tracing::warn!(%error, "could not open the Hex download");
        }
    }

    /// Quitting with an update ready installs it for the next launch; the
    /// other platforms activate it when it is staged.
    #[cfg(target_os = "macos")]
    pub fn install_on_quit(&self) {
        let mut state = lock(&self.shared.state);
        if let State::Ready(staged) = &*state {
            platform::install_on_quit(staged);
            *state = State::Applied;
        }
    }

    /// Switches to the staged update and starts it once this process exits;
    /// the caller quits Hex afterwards.
    fn restart_into_update(&self) -> Result<()> {
        let mut state = lock(&self.shared.state);
        let State::Ready(staged) = &*state else {
            return Err(eyre!("no update is ready"));
        };
        platform::restart_into(staged)?;
        *state = State::Applied;
        Ok(())
    }
}

/// Whether an administrator turned updates off (`DisableUpdates`), read
/// once per launch so a managed value applies at the next one.
pub fn managed_by_policy() -> bool {
    static MANAGED: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
    *MANAGED.get_or_init(platform::updates_disabled_by_policy)
}

/// Shown in place of update actions when an administrator turned them off.
#[cfg(target_os = "macos")]
pub const MANAGED_LABEL: &str = "Updates Managed by Your Organization";

fn lock<T>(mutex: &Mutex<T>) -> std::sync::MutexGuard<'_, T> {
    mutex.lock().unwrap_or_else(|error| error.into_inner())
}

/// Waits `delay` or until woken; true when the updater should stop.
fn wait(shared: &Shared, delay: Duration) -> bool {
    let mut wake = lock(&shared.wake);
    let deadline = std::time::Instant::now() + delay;
    while !wake.check && !wake.stop {
        let now = std::time::Instant::now();
        if now >= deadline {
            break;
        }
        wake = shared
            .signal
            .wait_timeout(wake, deadline - now)
            .unwrap_or_else(|error| error.into_inner())
            .0;
    }
    wake.check = false;
    wake.stop
}

fn run(shared: &Shared, method: &Method) {
    let mut cache = transport::ManifestCache::default();
    let mut backoff = CHECK_INTERVAL;
    if wait(shared, STARTUP_DELAY) {
        return;
    }
    loop {
        *lock(&shared.state) = State::Checking;
        let delay = match check(shared, method, &mut cache) {
            Ok(next) => {
                backoff = CHECK_INTERVAL;
                *lock(&shared.state) = next;
                CHECK_INTERVAL
            }
            Err(error) => {
                tracing::warn!(error = %format!("{error:#}"), "update check failed");
                *lock(&shared.state) = State::Failed;
                backoff = (backoff * 2).min(MAX_BACKOFF);
                backoff
            }
        };
        if matches!(&*lock(&shared.state), State::Ready(_)) {
            // A staged update waits for the restart; nothing further to check.
            return;
        }
        if wait(shared, delay) {
            return;
        }
    }
}

fn check(shared: &Shared, method: &Method, cache: &mut transport::ManifestCache) -> Result<State> {
    let updates = updates_dir()?;
    let manifest_url = manifest_url(method.channel());
    let manifest = cache.fetch(&manifest_url, &updates)?;
    let install = Install {
        version: env!("CARGO_PKG_VERSION").into(),
        target: platform::TARGET.into(),
        distribution: method.distribution().into(),
        rollout_bucket: rollout_bucket(&crate::app_paths::support_dir()?)?,
    };
    let decision = protocol::decide(APP, ARTIFACT, method.channel(), &manifest, &install);
    tracing::info!(?decision, "update decision");
    match decision {
        Decision::Reject { reason } => Err(eyre!("the update manifest was rejected: {reason}")),
        Decision::None { .. } => {
            transport::prune(&updates, None);
            Ok(State::Current)
        }
        Decision::Notify {
            command, notes_url, ..
        } => Ok(match command {
            Some(command) => State::Command { command },
            None => State::Download {
                url: notes_url.unwrap_or_else(|| "https://anoma.ly/hex".into()),
            },
        }),
        Decision::Install {
            version, target, ..
        } => {
            if *method == Method::DownloadOnly {
                let url = protocol::parse_manifest(&manifest)
                    .and_then(|manifest| {
                        manifest
                            .release
                            .target(platform::DOWNLOAD_TARGET)
                            .and_then(|target| protocol::resolve_url(&manifest_url, &target.url))
                            .or(manifest.release.notes_url)
                    })
                    .unwrap_or_else(|| "https://anoma.ly/hex".into());
                return Ok(State::Download { url });
            }
            let url = protocol::resolve_url(&manifest_url, &target.url)
                .ok_or_else(|| eyre!("the update URL {} is not usable", target.url))?;
            *lock(&shared.state) = State::Downloading;
            transport::prune(&updates, Some(&target.sha256));
            let file = transport::download(&url, &target, &updates)?;
            let staged = platform::stage(&file, &version)
                .wrap_err_with(|| format!("could not stage Hex {version}"))?;
            Ok(State::Ready(staged))
        }
    }
}

/// `HEX_RELEASE_BASE_URL` (HTTPS only) points a build at another service
/// origin, e.g. `https://dev.anoma.ly/release`, for dry runs.
fn manifest_url(channel: &str) -> String {
    let origin = std::env::var("HEX_RELEASE_BASE_URL")
        .ok()
        .filter(|origin| origin.starts_with("https://"));
    format!(
        "{}/{APP}/{ARTIFACT}/{channel}.json",
        origin.as_deref().unwrap_or(ORIGIN).trim_end_matches('/')
    )
}

fn updates_dir() -> Result<PathBuf> {
    let updates = crate::app_paths::support_dir()?.join("updates");
    std::fs::create_dir_all(&updates)?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&updates, std::fs::Permissions::from_mode(0o700))?;
    }
    Ok(updates)
}

/// This install's rollout bucket, 0–99: a random 32-bit number drawn once
/// from the OS's secure source and kept with the install. It never leaves
/// the device.
fn rollout_bucket(support: &Path) -> Result<u64> {
    let path = support.join("update-bucket");
    if let Some(number) = std::fs::read_to_string(&path)
        .ok()
        .and_then(|text| text.trim().parse::<u32>().ok())
    {
        return Ok(u64::from(number) % 100);
    }
    let mut bytes = [0; 4];
    getrandom::fill(&mut bytes).map_err(|error| eyre!("no secure random source: {error}"))?;
    let number = u32::from_le_bytes(bytes);
    std::fs::create_dir_all(support)?;
    let partial = path.with_extension("partial");
    std::fs::write(&partial, format!("{number}\n"))?;
    std::fs::rename(&partial, &path)?;
    Ok(u64::from(number) % 100)
}

/// The channel in our installer's receipt at `root`, when it names this app
/// and root and no other user can write the root.
#[cfg_attr(target_os = "macos", allow(dead_code))]
fn receipt_channel(root: &Path) -> Option<String> {
    #[derive(serde::Deserialize)]
    struct Receipt {
        app: String,
        artifact: String,
        channel: String,
        method: String,
        root: PathBuf,
    }
    let receipt: Receipt =
        serde_json::from_slice(&std::fs::read(root.join("install.json")).ok()?).ok()?;
    let canonical = |path: &Path| path.canonicalize().ok();
    (receipt.app == APP
        && receipt.artifact == ARTIFACT
        && receipt.method == "direct"
        && canonical(&receipt.root).is_some()
        && canonical(&receipt.root) == canonical(root)
        && ["stable", "beta", "dev"].contains(&receipt.channel.as_str()))
    .then_some(receipt.channel)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temporary(name: &str) -> PathBuf {
        let path = std::env::temp_dir().join(format!("hex-updater-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&path);
        std::fs::create_dir_all(&path).unwrap();
        path
    }

    #[test]
    fn the_rollout_bucket_is_drawn_once_and_kept() {
        let support = temporary("bucket");
        let first = rollout_bucket(&support).unwrap();
        assert!(first < 100);
        assert_eq!(rollout_bucket(&support).unwrap(), first);
        std::fs::write(support.join("update-bucket"), "4294967295\n").unwrap();
        assert_eq!(rollout_bucket(&support).unwrap(), 95);
        let _ = std::fs::remove_dir_all(support);
    }

    #[test]
    fn receipts_must_name_this_app_and_root() {
        let root = temporary("receipt");
        let write = |value: serde_json::Value| {
            std::fs::write(root.join("install.json"), value.to_string()).unwrap()
        };
        let receipt = |channel: &str, root_path: &Path| serde_json::json!({"app":"hex","artifact":"desktop","channel":channel,"method":"direct","root":root_path});
        assert_eq!(receipt_channel(&root), None);
        write(receipt("stable", &root));
        assert_eq!(receipt_channel(&root).as_deref(), Some("stable"));
        write(receipt("beta", &root));
        assert_eq!(receipt_channel(&root).as_deref(), Some("beta"));
        write(receipt("nightly", &root));
        assert_eq!(receipt_channel(&root), None);
        write(receipt("stable", &std::env::temp_dir()));
        assert_eq!(receipt_channel(&root), None);
        write(
            serde_json::json!({"app":"opencode","artifact":"desktop","channel":"stable","method":"direct","root":root}),
        );
        assert_eq!(receipt_channel(&root), None);
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn methods_map_to_protocol_distributions() {
        assert_eq!(
            Method::Direct {
                channel: "beta".into()
            }
            .distribution(),
            "direct"
        );
        assert_eq!(
            Method::Direct {
                channel: "beta".into()
            }
            .channel(),
            "beta"
        );
        assert_eq!(Method::DownloadOnly.distribution(), "direct");
        assert_eq!(Method::Package("aur").distribution(), "aur");
        assert_eq!(Method::Unknown.distribution(), "unknown");
        assert_eq!(Method::Package("aur").channel(), "stable");
    }

    #[test]
    fn the_manifest_url_names_app_artifact_and_channel() {
        assert_eq!(
            manifest_url("stable"),
            "https://anoma.ly/release/hex/desktop/stable.json"
        );
    }
}
