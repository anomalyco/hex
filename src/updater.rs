//! Hex's in-app updater on every OS, a client of the Anomaly release service
//! (`docs/plans/release-service.md`). One worker polls the channel's manifest
//! 10 seconds after launch, then hourly or on "Check for updates", decides with the
//! protocol's client rules, and for a direct install downloads the target,
//! checks its size, SHA-256, and OS code signature, and stages it until the
//! user restarts. Checks continue while an update is staged, so a newer
//! release replaces it, and Restart to update checks once more before it
//! switches over. Versions are display text and never compared: whenever the
//! channel's current release is not the running build (its target SHA-256,
//! else its exact version), Hex takes it, and switching channels does so
//! after the user confirms. Package-managed installs only learn the command to run;
//! source builds, test builds, and installs whose administrator set
//! `DisableUpdates` never check. Manifests are not signed: HTTPS, the
//! manifest's SHA-256, and (macOS, Windows) the code signature are the
//! integrity checks.

#[cfg(any(target_os = "linux", target_os = "windows"))]
pub mod legacy;
pub(crate) mod notes;
#[cfg_attr(target_os = "linux", path = "updater/linux.rs")]
#[cfg_attr(target_os = "macos", path = "updater/macos.rs")]
#[cfg_attr(target_os = "windows", path = "updater/windows.rs")]
mod platform;
mod presentation;
pub mod protocol;
#[cfg(any(target_os = "macos", test))]
mod staging;
mod transport;

use std::path::{Path, PathBuf};
use std::sync::{Arc, Condvar, Mutex};
use std::time::Duration;

use color_eyre::eyre::{Result, WrapErr, eyre};

use crate::app_settings::{AppSettings, UpdateChannel};
use crate::desktop::host::{DesktopAction, DesktopUpdateStatus, UpdateNotes};
use protocol::{Decision, Install};

use platform::Staged;
#[cfg(target_os = "macos")]
pub use platform::finish_on_launch;
#[cfg(any(target_os = "macos", test))]
pub(crate) use presentation::menu_title;
#[cfg_attr(target_os = "macos", allow(unused_imports))]
pub(crate) use presentation::{UpdateOffer, offer, offer_hold};

const APP: &str = "hex";
const ARTIFACT: &str = "desktop";
const ORIGIN: &str = "https://anoma.ly/release";
const STARTUP_DELAY: Duration = Duration::from_secs(10);
const CHECK_INTERVAL: Duration = Duration::from_secs(60 * 60);
/// Dev builds land on every push to dev, so the Dev channel checks often.
const DEV_CHECK_INTERVAL: Duration = Duration::from_secs(10 * 60);
/// Opening or focusing the window checks the Dev channel at most this often.
const FOCUS_CHECK_GAP: Duration = Duration::from_secs(60);
/// Failed checks back off, but never past hourly: the release service counts
/// a running install from at least one check an hour.
const MAX_BACKOFF: Duration = CHECK_INTERVAL;
/// The manifest fetch of an ordinary check.
const MANIFEST_TIMEOUT: Duration = Duration::from_secs(60);
/// The final check behind Restart to update: an unreachable service must
/// not hold the restart for long.
const RESTART_MANIFEST_TIMEOUT: Duration = Duration::from_secs(15);

/// How this install receives updates, which decides what the updater does.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum Method {
    /// Our installer's receipt (or, on macOS, a writable `Hex.app`): Hex
    /// downloads and stages updates itself on this channel.
    Direct { channel: String },
    /// A direct install Hex cannot replace (macOS: a read-only or translocated
    /// location); updates link to the download.
    DownloadOnly,
    /// A package manager (`aur`, `deb-x64`, `rpm-arm64`, `nix`, `brew`, …) updates
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

/// The channel the Release workflow published this build on, stamped at
/// build time beside its version (`HEX_BUILD_CHANNEL`).
const BUILD_CHANNEL: Option<&str> = option_env!("HEX_BUILD_CHANNEL");

/// The channel this build belongs to: the one it was published on, else
/// (builds from before the stamp) `dev` for a dev version, else the
/// install's own channel. Package managers follow `stable`.
fn home_channel(method: &Method, built: Option<&str>, version: &str) -> String {
    if matches!(method, Method::Package(_)) {
        return method.channel().into();
    }
    match built.filter(|channel| ["stable", "beta", "dev"].contains(channel)) {
        Some(channel) => channel.into(),
        None if is_dev_version(version) => UpdateChannel::Dev.name().into(),
        None => method.channel().into(),
    }
}

/// The channel to check: the `updates.channel` setting, else the build's
/// own. Package managers follow `stable`. A setting other than the build's
/// channel makes the decision a switch, which waits for the user.
fn channel(method: &Method, setting: Option<UpdateChannel>, home: &str) -> String {
    match setting {
        Some(channel) if !matches!(method, Method::Package(_)) => channel.name().into(),
        _ => home.into(),
    }
}

/// Dev builds are `0.0.0-dev-<run>` (`X.Y.Z-dev.N` before 2026-10-27).
fn is_dev_version(version: &str) -> bool {
    version.contains("-dev")
}

/// The Update channel Settings shows when none is saved: this build's own.
pub fn build_channel() -> UpdateChannel {
    let home = home_channel(&Method::Unknown, BUILD_CHANNEL, crate::VERSION);
    if home == UpdateChannel::Dev.name() {
        UpdateChannel::Dev
    } else {
        UpdateChannel::Stable
    }
}

#[derive(Clone, Debug)]
enum Phase {
    Disabled(DesktopUpdateStatus),
    Idle,
    /// Checking the channel's manifest. `confirmed_switch` holds the SHA-256
    /// of another channel's release the user just agreed to switch to;
    /// `begin_check` consumes it for that single check, and any transition out
    /// of `Checking` clears it.
    Checking {
        confirmed_switch: Option<String>,
    },
    Available(Available),
    Downloading,
    /// `Core::staged` waits for Restart to update.
    Staged,
    /// Switched over (by Restart to update or quit); terminal so no later
    /// check or download can ever un-apply it.
    Applied,
    Error,
}

#[derive(Clone, Debug, Eq, PartialEq)]
enum Available {
    /// A newer release this install cannot apply itself.
    Download { url: String },
    /// The setting names another channel, whose current release waits for
    /// the user to confirm the switch.
    Switch {
        channel: String,
        version: String,
        sha256: String,
    },
    /// The package manager serves another version.
    Command { command: String },
}

/// All mutable updater state under one lock so phase transitions, staging,
/// switch confirmation, restart requests, and wake signals are atomic.
struct Core {
    phase: Phase,
    /// The newest staged update. Checks continue while one waits, and a
    /// different current release replaces it before the restart.
    staged: Option<StagedUpdate>,
    /// What the staged dev build changed, when the dev notes said.
    notes: Option<UpdateNotes>,
    /// The channel and start of the latest check.
    last_check: Option<(String, std::time::Instant)>,
    restart: Restart,
    /// Set by "Check for updates", Dev window focus, switch confirmation, or
    /// a check arriving mid-download; cleared when the worker wakes.
    check_requested: bool,
    stop: bool,
}

struct CheckStart {
    restarting: bool,
    staged: Option<StagedUpdate>,
    confirmed_switch: Option<String>,
}

enum AdoptOutcome {
    Adopted,
    Discarded(Phase),
}

impl Core {
    fn new(phase: Phase) -> Self {
        Self {
            phase,
            staged: None,
            notes: None,
            last_check: None,
            restart: Restart::Idle,
            check_requested: false,
            stop: false,
        }
    }

    fn is_restarting(&self) -> bool {
        matches!(self.restart, Restart::Requested)
    }

    fn status(&self, packaged: bool) -> DesktopUpdateStatus {
        match &self.phase {
            Phase::Disabled(status) => status.clone(),
            Phase::Applied | Phase::Staged => DesktopUpdateStatus::ReadyToRestart,
            // A staged update stays offered while a background check or
            // download looks for a newer release, unless Restart to update
            // is running its final check.
            Phase::Checking { .. } | Phase::Downloading
                if self.staged.is_some() && !self.is_restarting() =>
            {
                DesktopUpdateStatus::ReadyToRestart
            }
            // A package manager updates Hex: its own checks only look for
            // the manager's command, so they show nothing of their own.
            Phase::Checking { .. } | Phase::Idle | Phase::Error if packaged => {
                DesktopUpdateStatus::PackageManager
            }
            Phase::Checking { .. } => DesktopUpdateStatus::Checking,
            Phase::Idle => DesktopUpdateStatus::Current,
            Phase::Error => DesktopUpdateStatus::Failed,
            Phase::Downloading => DesktopUpdateStatus::Downloading,
            Phase::Available(Available::Download { .. }) => DesktopUpdateStatus::Available,
            Phase::Available(Available::Switch {
                channel, version, ..
            }) => DesktopUpdateStatus::SwitchAvailable {
                channel: channel.clone(),
                version: version.clone(),
            },
            Phase::Available(Available::Command { command }) => {
                DesktopUpdateStatus::PackageUpdate {
                    command: command.clone(),
                }
            }
        }
    }

    fn notes(&self, packaged: bool) -> Option<UpdateNotes> {
        (self.status(packaged) == DesktopUpdateStatus::ReadyToRestart)
            .then(|| self.notes.clone())
            .flatten()
    }

    /// Requests an immediate check. Mid-download requests record
    /// `check_requested` without clobbering `Phase::Downloading`, so the
    /// worker runs the check as soon as the download completes.
    fn request_check(&mut self) -> bool {
        match &self.phase {
            Phase::Disabled(_) | Phase::Applied => return false,
            Phase::Staged | Phase::Downloading | Phase::Checking { .. } => {}
            Phase::Idle | Phase::Error | Phase::Available(_) => {
                self.phase = Phase::Checking {
                    confirmed_switch: None,
                };
            }
        }
        self.check_requested = true;
        true
    }

    fn confirm_switch(&mut self) -> bool {
        let Phase::Available(Available::Switch { sha256, .. }) = &self.phase else {
            return false;
        };
        let sha256 = sha256.clone();
        self.phase = Phase::Checking {
            confirmed_switch: Some(sha256),
        };
        self.check_requested = true;
        true
    }

    fn request_restart(&mut self, hold: Option<&'static str>) -> Result<bool> {
        if let Some(hold) = hold {
            return Err(eyre!(hold));
        }
        // A second click while the restart is under way changes nothing.
        if matches!(self.restart, Restart::Requested) || matches!(self.phase, Phase::Applied) {
            return Ok(false);
        }
        let ready = matches!(self.phase, Phase::Staged)
            || (self.staged.is_some()
                && matches!(self.phase, Phase::Checking { .. } | Phase::Downloading));
        if !ready {
            return Err(eyre!("no update is ready"));
        }
        self.restart = Restart::Requested;
        if matches!(self.phase, Phase::Staged) {
            self.phase = Phase::Checking {
                confirmed_switch: None,
            };
        }
        self.check_requested = true;
        Ok(true)
    }

    fn take_restart(&mut self) -> Option<Result<()>> {
        if !matches!(self.restart, Restart::Done(_)) {
            return None;
        }
        match std::mem::take(&mut self.restart) {
            Restart::Done(result) => Some(result.map_err(|error| eyre!(error))),
            Restart::Idle | Restart::Requested => None,
        }
    }

    fn begin_check(&mut self) -> CheckStart {
        let restarting = matches!(self.restart, Restart::Requested);
        let staged = self.staged.clone();
        let confirmed_switch = match &mut self.phase {
            Phase::Checking { confirmed_switch } => confirmed_switch.take(),
            Phase::Disabled(_) | Phase::Applied => None,
            _ => {
                self.phase = Phase::Checking {
                    confirmed_switch: None,
                };
                None
            }
        };
        CheckStart {
            restarting,
            staged,
            confirmed_switch,
        }
    }

    fn begin_download(&mut self) {
        if !matches!(self.phase, Phase::Applied | Phase::Disabled(_)) {
            self.phase = Phase::Downloading;
        }
    }

    /// Adopts a newly staged release unless a quit is installing (or already
    /// installed) another. On discard, an older staged update stays
    /// installable (`Staged`), an installed one stays `Applied`, and nothing
    /// staged yields `Idle`.
    fn adopt_staged(&mut self, update: StagedUpdate, quitting: bool) -> AdoptOutcome {
        if quitting || matches!(self.phase, Phase::Applied | Phase::Disabled(_)) {
            let next = if matches!(self.phase, Phase::Applied) {
                Phase::Applied
            } else if self.staged.is_some() {
                Phase::Staged
            } else {
                Phase::Idle
            };
            return AdoptOutcome::Discarded(next);
        }
        self.staged = Some(update);
        self.notes = None;
        AdoptOutcome::Adopted
    }

    fn finish_check(&mut self, next: Phase) {
        // Restart to update was pressed during this check: the next check,
        // which starts at once, finishes it.
        if matches!(self.restart, Restart::Requested) {
            return;
        }
        if !matches!(self.phase, Phase::Applied | Phase::Disabled(_)) {
            self.phase = next;
        }
    }

    /// Switches to the newest staged update once Restart to update's final
    /// check has run, whatever its result. True when Hex now quits. A meeting
    /// that started recording since the click keeps the update staged and the
    /// restart waiting, and a quit that began during the check never swaps: the
    /// drain installs the update itself.
    fn finish_restart(&mut self, next: Phase, hold: Option<&'static str>, quitting: bool) -> bool {
        // Quitting installs the staged update itself (`install_on_quit`), and
        // it holds this lock while it does; a swap here would exchange the same
        // two bundles back and lose the update.
        if matches!(self.phase, Phase::Applied) {
            self.restart = Restart::Done(Ok(()));
            return true;
        }
        if quitting {
            // The drain installs what is staged; this check must not swap, nor
            // hold the quit for an update that is already being installed.
            self.restart = Restart::Done(Ok(()));
            tracing::info!("a quit started during Restart to update; the drain installs it");
            return true;
        }
        if let Some(hold) = hold {
            self.phase = if self.staged.is_some() {
                Phase::Staged
            } else {
                next
            };
            self.restart = Restart::Done(Err(hold.into()));
            tracing::info!(reason = hold, "Restart to update waits for a meeting");
            return false;
        }
        let result = match &self.staged {
            Some(update) => {
                platform::restart_into(&update.staged).map_err(|error| format!("{error:#}"))
            }
            None => Err("no update is ready".into()),
        };
        let restarted = result.is_ok();
        if let Err(error) = &result {
            tracing::warn!(%error, "could not restart into the update");
        }
        self.phase = if restarted {
            Phase::Applied
        } else if self.staged.is_some() {
            Phase::Staged
        } else {
            next
        };
        self.restart = Restart::Done(result);
        restarted
    }

    #[cfg(target_os = "macos")]
    fn install_on_quit(&mut self) {
        if matches!(self.phase, Phase::Applied | Phase::Disabled(_)) {
            return;
        }
        if let Some(update) = &self.staged {
            platform::install_on_quit(&update.staged);
            self.phase = Phase::Applied;
        }
    }
}

struct Shared {
    core: Mutex<Core>,
    signal: Condvar,
    /// A package manager installed Hex, so "up to date" is its to say.
    packaged: bool,
    /// Why this install can't update itself (Windows), shown with Download
    /// update.
    #[cfg_attr(not(target_os = "windows"), allow(dead_code))]
    blocked: Option<&'static str>,
}

impl Shared {
    fn new(phase: Phase, packaged: bool, blocked: Option<&'static str>) -> Self {
        Self {
            core: Mutex::new(Core::new(phase)),
            signal: Condvar::new(),
            packaged,
            blocked,
        }
    }
}

/// A staged update and the SHA-256 of the target it came from, which tells
/// a later check whether the channel still offers that release.
#[derive(Clone, Debug)]
struct StagedUpdate {
    staged: Staged,
    sha256: String,
    version: String,
}

#[derive(Debug, Default)]
enum Restart {
    #[default]
    Idle,
    /// Restart to update was pressed: the worker checks the channel once
    /// more, then switches to the newest staged update.
    Requested,
    /// The worker switched over (the host quits) or could not.
    Done(std::result::Result<(), String>),
}

#[derive(Clone)]
pub struct Updater {
    shared: Arc<Shared>,
}

impl Updater {
    pub fn start() -> Self {
        let method = platform::method();
        let disabled = if option_env!("HEX_TEST_BUILD") == Some("1") || updates_off_by_env() {
            Some(DesktopUpdateStatus::Unavailable)
        } else if crate::app_paths::is_local_build() {
            Some(DesktopUpdateStatus::SourceBuild)
        } else if managed_by_policy() {
            Some(DesktopUpdateStatus::Managed)
        // An `HEX_X86_BASELINE=ivybridge` source build targets an older processor
        // than every published artifact, so it never takes one: installing it
        // would leave a Hex that can't start. A source build never checks,
        // downloads, or installs, and its footer says so.
        } else if method == Method::Source || cfg!(hex_x86_ivybridge) {
            Some(DesktopUpdateStatus::SourceBuild)
        } else {
            None
        };
        let checks = disabled.is_none();
        let blocked = (method == Method::Unknown)
            .then(platform::blocked)
            .flatten();
        let shared = Arc::new(Shared::new(
            match disabled {
                Some(status) => Phase::Disabled(status),
                None => Phase::Checking {
                    confirmed_switch: None,
                },
            },
            matches!(method, Method::Package(_)),
            blocked,
        ));
        if checks {
            let worker = shared.clone();
            if let Err(error) = std::thread::Builder::new()
                .name("updater".into())
                .spawn(move || run(&worker, &method))
            {
                tracing::error!(%error, "could not start the updater");
                lock(&shared.core).phase = Phase::Error;
            }
        }
        Self { shared }
    }

    /// An updater with no worker in the Idle state, for tests of when it
    /// asks for a check.
    #[cfg(test)]
    fn idle_checking() -> Self {
        Self {
            shared: Arc::new(Shared::new(Phase::Idle, false, None)),
        }
    }

    /// An updater that never checks, for tests of the hosts that own one.
    #[cfg(all(test, target_os = "linux"))]
    pub fn idle() -> Self {
        Self {
            shared: Arc::new(Shared::new(
                Phase::Disabled(DesktopUpdateStatus::Unavailable),
                false,
                None,
            )),
        }
    }

    /// Checks now instead of at the next interval. A staged update stays
    /// offered while the check looks for a newer release, and a request that
    /// arrives mid-download is queued to run as soon as the download ends.
    pub fn check_now(&self) {
        let wake = lock(&self.shared.core).request_check();
        if wake {
            self.shared.signal.notify_all();
        }
    }

    /// The window opened or gained focus: on the Dev channel, check now
    /// unless a check started within the last minute.
    pub fn window_focused(&self) {
        let wake = {
            let mut core = lock(&self.shared.core);
            let recent = core
                .last_check
                .as_ref()
                .is_some_and(|(channel, at)| channel != "dev" || at.elapsed() < FOCUS_CHECK_GAP);
            !recent && core.request_check()
        };
        if wake {
            self.shared.signal.notify_all();
        }
    }

    pub fn stop(&self) {
        lock(&self.shared.core).stop = true;
        self.shared.signal.notify_all();
    }

    /// Why this install can't update itself, while Download update shows:
    /// an English sentence the window translates.
    #[cfg_attr(not(target_os = "windows"), allow(dead_code))]
    pub fn blocked(&self) -> Option<&'static str> {
        matches!(
            lock(&self.shared.core).phase,
            Phase::Available(Available::Download { .. })
        )
        .then_some(self.shared.blocked)
        .flatten()
    }

    /// What the staged dev build changed, while Restart to update shows.
    pub fn notes(&self) -> Option<UpdateNotes> {
        lock(&self.shared.core).notes(self.shared.packaged)
    }

    pub fn status(&self) -> DesktopUpdateStatus {
        lock(&self.shared.core).status(self.shared.packaged)
    }

    /// Carries out an update control: check now, open the download, or
    /// restart into the staged update. A restart finishes on the worker;
    /// [`Self::take_restart`] says when Hex must quit.
    pub fn act(&self, action: &DesktopAction) -> Result<()> {
        match action {
            DesktopAction::CheckForUpdates => self.check_now(),
            DesktopAction::WindowFocused => self.window_focused(),
            DesktopAction::OpenUpdate => self.open_download(),
            DesktopAction::ConfirmUpdateSwitch => self.confirm_switch(),
            DesktopAction::RestartIntoUpdate => self.request_restart(restart_hold())?,
            _ => {}
        }
        Ok(())
    }

    /// Restart to update: the worker checks the channel once more, so a
    /// release published since the update was staged replaces it first,
    /// then switches to the newest staged update. A meeting recording holds
    /// it: the relaunch would cut the recording short.
    fn request_restart(&self, hold: Option<&'static str>) -> Result<()> {
        let wake = lock(&self.shared.core).request_restart(hold)?;
        if wake {
            self.shared.signal.notify_all();
        }
        Ok(())
    }

    /// The outcome of Restart to update, once: `Ok` when the update starts
    /// as soon as Hex quits, which the caller must now do.
    pub fn take_restart(&self) -> Option<Result<()>> {
        lock(&self.shared.core).take_restart()
    }

    /// The action of a single "update" menu item, which follows the status.
    #[cfg(target_os = "macos")]
    pub fn menu_action(&self) -> DesktopAction {
        presentation::menu_action(&self.status())
    }

    /// The user agreed to switch to the other channel's release on offer:
    /// download and stage it now.
    fn confirm_switch(&self) {
        let wake = lock(&self.shared.core).confirm_switch();
        if wake {
            self.shared.signal.notify_all();
        }
    }

    /// Opens the download an install that cannot update itself needs.
    fn open_download(&self) {
        let url = match &lock(&self.shared.core).phase {
            Phase::Available(Available::Download { url }) => url.clone(),
            _ => return,
        };
        if let Err(error) = crate::desktop::opencode_catalog::open_url(&url) {
            tracing::warn!(%error, "could not open the Hex download");
        }
    }

    /// Quitting with an update ready installs it for the next launch; the
    /// other platforms activate it when it is staged.
    #[cfg(target_os = "macos")]
    pub fn install_on_quit(&self) {
        lock(&self.shared.core).install_on_quit();
    }
}

/// `HEX_DISABLE_UPDATES=1` turns this launch's updater off, as a test build
/// has it: no checks, no network, nothing offered. For copies of Hex that
/// must stay quiet, such as benchmarks (docs/perf.md).
fn updates_off_by_env() -> bool {
    std::env::var_os("HEX_DISABLE_UPDATES").is_some_and(|value| value == "1")
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
    let mut core = lock(&shared.core);
    let deadline = std::time::Instant::now() + delay;
    while !core.check_requested && !core.stop {
        let now = std::time::Instant::now();
        if now >= deadline {
            break;
        }
        core = shared
            .signal
            .wait_timeout(core, deadline - now)
            .unwrap_or_else(|error| error.into_inner())
            .0;
    }
    core.check_requested = false;
    core.stop
}

/// How often a channel is checked: Dev every few minutes, others hourly.
fn check_interval(channel: &str) -> Duration {
    if channel == "dev" {
        DEV_CHECK_INTERVAL
    } else {
        CHECK_INTERVAL
    }
}

fn run(shared: &Shared, method: &Method) {
    let mut cache = transport::ManifestCache::default();
    let mut failures = 0;
    if wait(shared, STARTUP_DELAY) {
        return;
    }
    loop {
        let start = lock(&shared.core).begin_check();
        let result = check(
            shared,
            method,
            &mut cache,
            start.staged.as_ref(),
            start.restarting,
            start.confirmed_switch.as_deref(),
        );
        let delay = {
            let mut core = lock(&shared.core);
            let interval = core
                .last_check
                .as_ref()
                .map_or(CHECK_INTERVAL, |(channel, _)| check_interval(channel));
            let (next, delay) = match result {
                Ok(next) => {
                    failures = 0;
                    (next, interval)
                }
                Err(error) => {
                    tracing::warn!(error = %format!("{error:#}"), "update check failed");
                    failures = (failures + 1).min(8);
                    let next = if core.staged.is_some() {
                        Phase::Staged
                    } else {
                        Phase::Error
                    };
                    (next, (interval * 2u32.pow(failures)).min(MAX_BACKOFF))
                }
            };
            if start.restarting {
                if core.finish_restart(
                    next,
                    restart_hold(),
                    crate::desktop::process_exit::quitting(),
                ) {
                    return;
                }
            } else {
                core.finish_check(next);
            }
            delay
        };
        if wait(shared, delay) {
            return;
        }
    }
}

/// Why Restart to update has to wait: a recording or a launch recovery is
/// still finishing a meeting, and the relaunch would cut it off.
fn restart_hold() -> Option<&'static str> {
    offer_hold(
        &DesktopUpdateStatus::ReadyToRestart,
        crate::meeting::recording(),
    )
}

fn check(
    shared: &Shared,
    method: &Method,
    cache: &mut transport::ManifestCache,
    staged: Option<&StagedUpdate>,
    restarting: bool,
    confirmed_switch: Option<&str>,
) -> Result<Phase> {
    let updates = updates_dir()?;
    // Read each check, so a change from the window or `hex settings` applies
    // at the next one.
    let setting = AppSettings::saved_update_channel();
    let home = home_channel(method, BUILD_CHANNEL, crate::VERSION);
    let channel = channel(method, setting, &home);
    let user_agent = transport::user_agent(&home, crate::VERSION);
    lock(&shared.core).last_check = Some((channel.clone(), std::time::Instant::now()));
    let manifest_url = manifest_url(&channel);
    let timeout = if restarting {
        RESTART_MANIFEST_TIMEOUT
    } else {
        MANIFEST_TIMEOUT
    };
    let manifest = cache.fetch(&manifest_url, &user_agent, &updates, timeout)?;
    let install = Install {
        version: crate::VERSION.into(),
        target: platform::TARGET.into(),
        distribution: method.distribution().into(),
        rollout_bucket: rollout_bucket(&crate::app_paths::support_dir()?)?,
        sha256: match method {
            Method::Direct { .. } => running_sha256(),
            _ => None,
        },
        channel: (!matches!(method, Method::Package(_))).then_some(home),
    };
    let decision = protocol::decide(APP, ARTIFACT, &channel, &manifest, &install);
    tracing::info!(?decision, "update decision");
    match decision {
        Decision::Reject { reason } => Err(eyre!("the update manifest was rejected: {reason}")),
        Decision::None { .. } | Decision::Install { .. }
            if staged.is_some_and(|staged| !replaces_staged(&decision, &staged.sha256)) =>
        {
            // The notes may have been published after the build was staged.
            if let Some(staged) = staged
                && !restarting
                && lock(&shared.core).notes.is_none()
            {
                fetch_notes(shared, &channel, &user_agent, &updates, &staged.version);
            }
            Ok(Phase::Staged)
        }
        Decision::None { .. } => {
            transport::prune(&updates, None);
            #[cfg(target_os = "windows")]
            if matches!(method, Method::Direct { .. }) && platform::console_missing() {
                let installed = download_console(&manifest, &manifest_url, &user_agent, &updates)
                    .and_then(|file| file.map_or(Ok(()), |file| platform::install_console(&file)));
                if let Err(error) = installed {
                    tracing::warn!(error = %format!("{error:#}"), "could not add hex.com to this install");
                }
            }
            Ok(Phase::Idle)
        }
        Decision::Notify {
            command, notes_url, ..
        } => Ok(Phase::Available(match command {
            Some(command) => Available::Command { command },
            None => Available::Download {
                url: notes_url.unwrap_or_else(|| "https://anoma.ly/hex".into()),
            },
        })),
        Decision::Install {
            reason: protocol::Reason::Switch,
            version,
            target,
        } if *method != Method::DownloadOnly
            && confirmed_switch != Some(target.sha256.as_str()) =>
        {
            Ok(Phase::Available(Available::Switch {
                channel,
                version,
                sha256: target.sha256,
            }))
        }
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
                return Ok(Phase::Available(Available::Download { url }));
            }
            let url = protocol::resolve_url(&manifest_url, &target.url)
                .ok_or_else(|| eyre!("the update URL {} is not usable", target.url))?;
            // Record the download phase; `Core::status` keeps any older
            // staged update offered until this one replaces it.
            lock(&shared.core).begin_download();
            transport::prune(&updates, Some(&target.sha256));
            #[cfg(target_os = "linux")]
            verify_linux_signature(&manifest, &manifest_url, &user_agent, &target, &updates)?;
            let file = transport::download(&url, &user_agent, &target, &updates)?;
            #[cfg(target_os = "windows")]
            let staged = download_console(&manifest, &manifest_url, &user_agent, &updates)
                .and_then(|console| platform::stage(&file, console.as_deref(), &version));
            #[cfg(not(target_os = "windows"))]
            let staged = platform::stage(&file, &version);
            let staged = staged.wrap_err_with(|| format!("could not stage Hex {version}"))?;
            // A quit installs the staged update itself (`install_on_quit`)
            // under the same lock, so a release that lands afterwards is
            // discarded instead of replacing the installed one.
            #[cfg(target_os = "macos")]
            let kept = staged.clone();
            let outcome = lock(&shared.core).adopt_staged(
                StagedUpdate {
                    staged,
                    sha256: target.sha256,
                    version: version.clone(),
                },
                crate::desktop::process_exit::quitting(),
            );
            if let AdoptOutcome::Discarded(next) = outcome {
                #[cfg(target_os = "macos")]
                platform::discard(&kept);
                return Ok(next);
            }
            // Now that this release is the staged update, the unpacked folder
            // of the release it replaced is no longer needed.
            #[cfg(target_os = "macos")]
            platform::remove_superseded(&kept);
            // Restart to update's final check quits right after; notes
            // would only hold it up.
            if !restarting {
                fetch_notes(shared, &channel, &user_agent, &updates, &version);
            }
            Ok(Phase::Staged)
        }
    }
}

/// What the staged dev build changed; nothing for other channels or when
/// the dev notes don't list it yet.
fn fetch_notes(shared: &Shared, channel: &str, user_agent: &str, updates: &Path, version: &str) {
    if channel != UpdateChannel::Dev.name() {
        return;
    }
    let notes = notes::fetch(user_agent, &updates.join("notes"), crate::VERSION, version);
    let mut core = lock(&shared.core);
    if core
        .staged
        .as_ref()
        .is_some_and(|staged| staged.version == version)
    {
        core.notes = notes;
    }
}

/// Checks the manifest's `<target>-sig` file: Anomaly's release key signing
/// the SHA-256 the executable is downloaded against. Without it, whoever
/// controls the manifest could name any executable.
#[cfg(target_os = "linux")]
fn verify_linux_signature(
    manifest: &str,
    manifest_url: &str,
    user_agent: &str,
    target: &protocol::Target,
    updates: &Path,
) -> Result<()> {
    let manifest = protocol::parse_manifest(manifest)
        .ok_or_else(|| eyre!("the update manifest was rejected: malformed"))?;
    let signature = manifest
        .release
        .target(&platform::signature_target())
        .ok_or_else(|| eyre!("the Linux update has no signature"))?;
    if signature.size != 64 {
        return Err(eyre!("the Linux update's signature is malformed"));
    }
    let url = protocol::resolve_url(manifest_url, &signature.url)
        .ok_or_else(|| eyre!("the signature URL {} is not usable", signature.url))?;
    let file = transport::download(&url, user_agent, signature, updates)?;
    platform::verify_signature(&target.sha256, &std::fs::read(file)?)
}

/// The release's `hex.com` (`<target>-cli`), checked against the manifest's
/// size and SHA-256; none when the release has no console entry point.
#[cfg(target_os = "windows")]
fn download_console(
    manifest: &str,
    manifest_url: &str,
    user_agent: &str,
    updates: &Path,
) -> Result<Option<PathBuf>> {
    let Some(manifest) = protocol::parse_manifest(manifest) else {
        return Ok(None);
    };
    let Some(target) = manifest.release.target(platform::CONSOLE_TARGET) else {
        return Ok(None);
    };
    let url = protocol::resolve_url(manifest_url, &target.url)
        .ok_or_else(|| eyre!("the hex.com URL {} is not usable", target.url))?;
    transport::download(&url, user_agent, target, updates).map(Some)
}

/// Whether `decision` replaces the staged update: only a different release
/// this install takes by itself does. A rollout that has not reached this
/// install, an unconfirmed channel switch, or the same release keeps it.
fn replaces_staged(decision: &Decision, staged_sha256: &str) -> bool {
    matches!(
        decision,
        Decision::Install {
            reason: protocol::Reason::Update | protocol::Reason::Required,
            target,
            ..
        } if target.sha256 != staged_sha256
    )
}

/// `HEX_RELEASE_BASE_URL` (HTTPS only) points a build at another service
/// origin, for local tests against a fixture service. Nothing defaults to the
/// service's dev stage, which only tests the service itself.
fn manifest_url(channel: &str) -> String {
    let origin = std::env::var("HEX_RELEASE_BASE_URL")
        .ok()
        .filter(|origin| origin.starts_with("https://"));
    format!(
        "{}/{APP}/{ARTIFACT}/{channel}.json",
        origin.as_deref().unwrap_or(ORIGIN).trim_end_matches('/')
    )
}

/// This build's target SHA-256, when the platform can tell (the target is
/// the executable itself); hashed once per launch.
fn running_sha256() -> Option<String> {
    static SHA256: std::sync::OnceLock<Option<String>> = std::sync::OnceLock::new();
    SHA256.get_or_init(platform::running_sha256).clone()
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
    let bytes = std::fs::read(root.join("install.json")).ok()?;
    // Tolerate a byte order mark: some UTF-8 writers add one.
    let bytes = bytes.strip_prefix(b"\xEF\xBB\xBF").unwrap_or(&bytes);
    let receipt: Receipt = serde_json::from_slice(bytes).ok()?;
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

    fn staged_update(version: &str, sha: char) -> StagedUpdate {
        StagedUpdate {
            staged: Staged::dummy(),
            sha256: sha.to_string().repeat(64),
            version: version.into(),
        }
    }

    #[test]
    fn a_package_manager_install_never_shows_its_own_checks() {
        let updater = |phase: Phase, packaged: bool| Updater {
            shared: Arc::new(Shared::new(phase, packaged, None)),
        };
        for phase in [
            Phase::Checking {
                confirmed_switch: None,
            },
            Phase::Idle,
            Phase::Error,
        ] {
            assert_eq!(
                updater(phase, true).status(),
                DesktopUpdateStatus::PackageManager
            );
        }
        assert_eq!(
            updater(
                Phase::Checking {
                    confirmed_switch: None,
                },
                false,
            )
            .status(),
            DesktopUpdateStatus::Checking
        );
        assert_eq!(
            updater(Phase::Error, false).status(),
            DesktopUpdateStatus::Failed
        );
    }

    #[test]
    fn a_build_belongs_to_the_channel_it_was_published_on() {
        let direct = Method::Direct {
            channel: "beta".into(),
        };
        assert_eq!(home_channel(&direct, Some("dev"), "0.0.0-dev-40"), "dev");
        assert_eq!(home_channel(&direct, Some("stable"), "2.4.0"), "stable");
        // Builds from before the stamp: a dev version, else the receipt.
        assert_eq!(home_channel(&direct, None, "2.3.2-dev.29"), "dev");
        assert_eq!(home_channel(&direct, None, "0.0.0-dev-40"), "dev");
        assert_eq!(home_channel(&direct, None, "2.4.0"), "beta");
        assert_eq!(home_channel(&direct, Some("nightly"), "2.4.0"), "beta");
        assert_eq!(
            home_channel(&Method::Package("aur"), Some("dev"), "0.0.0-dev-40"),
            "stable"
        );
    }

    #[test]
    fn the_setting_picks_the_channel_unless_a_package_manager_updates_hex() {
        let direct = Method::Direct {
            channel: "stable".into(),
        };
        assert_eq!(channel(&direct, None, "dev"), "dev");
        assert_eq!(
            channel(&direct, Some(UpdateChannel::Stable), "dev"),
            "stable"
        );
        assert_eq!(channel(&direct, Some(UpdateChannel::Dev), "stable"), "dev");
        assert_eq!(
            channel(&Method::DownloadOnly, Some(UpdateChannel::Dev), "stable"),
            "dev"
        );
        assert_eq!(
            channel(&Method::Package("aur"), Some(UpdateChannel::Dev), "stable"),
            "stable"
        );
    }

    fn manifest(channel: &str, version: &str, sha: char) -> String {
        let sha = sha.to_string().repeat(64);
        serde_json::json!({
            "schema": 1,
            "app": "hex",
            "artifact": "desktop",
            "channel": channel,
            "sequence": 1,
            "published_at": "2026-10-27T00:00:00Z",
            "release": {
                "version": version,
                "rollout_percent": 100,
                "revoked_versions": [],
                "distributions": [{ "kind": "direct" }],
                "targets": {
                    "linux-x64": {
                        "url": format!("/release/hex/files/{sha}/hex"),
                        "sha256": sha,
                        "size": 1
                    }
                }
            }
        })
        .to_string()
    }

    fn install(version: &str, sha: char, channel: &str) -> Install {
        Install {
            version: version.into(),
            target: "linux-x64".into(),
            distribution: "direct".into(),
            rollout_bucket: 0,
            sha256: Some(sha.to_string().repeat(64)),
            channel: Some(channel.into()),
        }
    }

    #[test]
    fn versions_are_never_compared_across_or_within_channels() {
        let dev = install("0.0.0-dev-40", 'a', "dev");
        // A legacy dev version above the new numbering still moves to it.
        let legacy = install("2.3.2-dev.29", 'f', "dev");
        let next = manifest("dev", "0.0.0-dev-41", 'b');
        for running in [&dev, &legacy] {
            assert!(matches!(
                protocol::decide(APP, ARTIFACT, "dev", &next, running),
                Decision::Install {
                    reason: protocol::Reason::Update,
                    ..
                }
            ));
        }
        let same = manifest("dev", "0.0.0-dev-40", 'a');
        assert!(matches!(
            protocol::decide(APP, ARTIFACT, "dev", &same, &dev),
            Decision::None { .. }
        ));
        // Stable 2.3.1 is a switch from dev, never "older".
        let stable = manifest("stable", "2.3.1", 'c');
        assert!(matches!(
            protocol::decide(APP, ARTIFACT, "stable", &stable, &dev),
            Decision::Install {
                reason: protocol::Reason::Switch,
                ..
            }
        ));
    }

    #[test]
    fn a_switch_waits_for_the_user_to_confirm_and_clears_after_being_consumed_or_superseded() {
        let updater = Updater::idle_checking();
        let sha = "c".repeat(64);
        lock(&updater.shared.core).phase = Phase::Available(Available::Switch {
            channel: "stable".into(),
            version: "2.3.1".into(),
            sha256: sha.clone(),
        });
        assert_eq!(
            updater.status(),
            DesktopUpdateStatus::SwitchAvailable {
                channel: "stable".into(),
                version: "2.3.1".into()
            }
        );

        updater.act(&DesktopAction::ConfirmUpdateSwitch).unwrap();
        assert_eq!(updater.status(), DesktopUpdateStatus::Checking);
        {
            let core = lock(&updater.shared.core);
            assert!(core.check_requested, "confirming checks now");
            assert!(matches!(
                &core.phase,
                Phase::Checking {
                    confirmed_switch: Some(confirmed),
                } if confirmed == &sha
            ));
        }

        // Starting the check consumes the confirmation for that check alone;
        // a failed or superseded check never keeps it set for later offers.
        let start = lock(&updater.shared.core).begin_check();
        assert_eq!(start.confirmed_switch.as_deref(), Some(sha.as_str()));
        lock(&updater.shared.core).finish_check(Phase::Error);
        assert_eq!(updater.status(), DesktopUpdateStatus::Failed);

        let next_start = lock(&updater.shared.core).begin_check();
        assert_eq!(
            next_start.confirmed_switch, None,
            "a previous failed switch confirmation must not carry over"
        );

        // Confirming again and then transitioning away (e.g. a manual check
        // after switching channels, or adopting another state) clears it too.
        lock(&updater.shared.core).phase = Phase::Available(Available::Switch {
            channel: "stable".into(),
            version: "2.3.1".into(),
            sha256: sha,
        });
        updater.act(&DesktopAction::ConfirmUpdateSwitch).unwrap();
        lock(&updater.shared.core).finish_check(Phase::Idle);
        let after_reset = lock(&updater.shared.core).begin_check();
        assert_eq!(after_reset.confirmed_switch, None);
    }

    #[test]
    fn check_now_arriving_mid_download_is_queued_without_clobbering_downloading() {
        let updater = Updater::idle_checking();
        lock(&updater.shared.core).begin_download();
        assert_eq!(updater.status(), DesktopUpdateStatus::Downloading);

        updater.check_now();
        assert_eq!(
            updater.status(),
            DesktopUpdateStatus::Downloading,
            "a mid-download check keeps the Downloading status"
        );
        assert!(
            lock(&updater.shared.core).check_requested,
            "the request is recorded instead of silently dropped"
        );

        // Once the download finishes, wait() returns immediately to run the
        // queued check rather than sleeping for the full interval.
        lock(&updater.shared.core).finish_check(Phase::Staged);
        let started = std::time::Instant::now();
        assert!(
            !wait(&updater.shared, Duration::from_secs(60)),
            "queued check wakes wait() without stopping"
        );
        assert!(
            started.elapsed() < Duration::from_secs(1),
            "wait() returned immediately for the queued check"
        );
        assert!(!lock(&updater.shared.core).check_requested);
    }

    #[test]
    fn staged_update_stays_offered_during_background_check_and_download() {
        let updater = Updater::idle_checking();
        {
            let mut core = lock(&updater.shared.core);
            assert!(matches!(
                core.adopt_staged(staged_update("0.0.0-dev-41", 'b'), false),
                AdoptOutcome::Adopted
            ));
            core.finish_check(Phase::Staged);
        }
        assert_eq!(updater.status(), DesktopUpdateStatus::ReadyToRestart);

        // Background check keeps the staged update offered.
        let start = lock(&updater.shared.core).begin_check();
        assert!(!start.restarting);
        assert_eq!(
            start.staged.as_ref().map(|s| s.version.as_str()),
            Some("0.0.0-dev-41")
        );
        assert_eq!(updater.status(), DesktopUpdateStatus::ReadyToRestart);

        // Background download of a newer release also keeps the older staged
        // update offered, and a check_now during it is queued.
        lock(&updater.shared.core).begin_download();
        assert_eq!(updater.status(), DesktopUpdateStatus::ReadyToRestart);
        updater.check_now();
        assert!(lock(&updater.shared.core).check_requested);

        // Clicking Restart to update while a background download is running
        // switches status to Downloading for the restart flow.
        updater.act(&DesktopAction::RestartIntoUpdate).unwrap();
        assert_eq!(updater.status(), DesktopUpdateStatus::Downloading);
    }

    #[test]
    fn only_a_different_release_this_install_takes_replaces_a_staged_update() {
        let running = install("0.0.0-dev-40", 'a', "dev");
        let staged = "b".repeat(64);
        let decide = |channel: &str, version: &str, sha: char| {
            protocol::decide(
                APP,
                ARTIFACT,
                channel,
                &manifest(channel, version, sha),
                &running,
            )
        };
        assert!(!replaces_staged(
            &decide("dev", "0.0.0-dev-41", 'b'),
            &staged
        ));
        assert!(replaces_staged(
            &decide("dev", "0.0.0-dev-42", 'c'),
            &staged
        ));
        // The channel rolled back to the running build: nothing to install.
        assert!(!replaces_staged(
            &decide("dev", "0.0.0-dev-40", 'a'),
            &staged
        ));
        // Another channel's release waits for the user to confirm it.
        assert!(!replaces_staged(&decide("stable", "2.3.1", 'c'), &staged));
    }

    #[test]
    fn a_restart_after_a_quit_installed_the_update_does_not_swap_again() {
        let updater = Updater::idle_checking();
        // Quitting installed the staged update and set Applied; the final
        // restart check finishes afterwards.
        {
            let mut core = lock(&updater.shared.core);
            core.phase = Phase::Applied;
            core.restart = Restart::Requested;
            assert!(
                core.finish_restart(Phase::Staged, None, false),
                "the quit already installed the update; Hex still quits"
            );
            assert!(matches!(core.restart, Restart::Done(Ok(()))));
            assert!(matches!(core.phase, Phase::Applied));
        }
    }

    #[test]
    fn a_restart_check_that_races_a_quit_never_swaps() {
        let updater = Updater::idle_checking();
        {
            let mut core = lock(&updater.shared.core);
            core.phase = Phase::Staged;
            core.restart = Restart::Requested;
            // A quit began during the final check: the drain installs whatever
            // is staged, so this check must not swap (nor report "no update").
            assert!(core.finish_restart(Phase::Staged, None, true));
            assert!(matches!(core.restart, Restart::Done(Ok(()))));
            assert!(matches!(core.phase, Phase::Staged));
        }
    }

    #[test]
    fn a_release_discarded_at_quit_leaves_the_older_one_installable() {
        // v1 is staged and a quit begins while v2 finishes downloading: v2 is
        // discarded, but the state must stay Staged so install_on_quit (which
        // skips only Applied) still installs v1.
        let mut core = Core::new(Phase::Downloading);
        core.staged = Some(staged_update("0.0.0-dev-41", 'b'));
        assert!(matches!(
            core.adopt_staged(staged_update("0.0.0-dev-42", 'c'), true),
            AdoptOutcome::Discarded(Phase::Staged)
        ));
        assert_eq!(
            core.staged.as_ref().map(|u| u.version.as_str()),
            Some("0.0.0-dev-41"),
            "the older staged update is preserved"
        );

        // A quit that already installed the update keeps Applied.
        let mut core = Core::new(Phase::Applied);
        core.staged = Some(staged_update("0.0.0-dev-41", 'b'));
        assert!(matches!(
            core.adopt_staged(staged_update("0.0.0-dev-42", 'c'), true),
            AdoptOutcome::Discarded(Phase::Applied)
        ));

        // Nothing staged: a plain Idle result, not a claim of installation.
        let mut core = Core::new(Phase::Downloading);
        assert!(matches!(
            core.adopt_staged(staged_update("0.0.0-dev-42", 'c'), true),
            AdoptOutcome::Discarded(Phase::Idle)
        ));
    }

    #[test]
    fn applied_and_disabled_are_terminal_across_transitions() {
        let mut applied = Core::new(Phase::Applied);
        assert!(!applied.request_check());
        assert!(!applied.check_requested);
        applied.begin_check();
        applied.begin_download();
        applied.finish_check(Phase::Idle);
        assert!(matches!(applied.phase, Phase::Applied));

        let mut disabled = Core::new(Phase::Disabled(DesktopUpdateStatus::Unavailable));
        assert!(!disabled.request_check());
        assert!(!disabled.check_requested);
        disabled.begin_check();
        disabled.begin_download();
        disabled.finish_check(Phase::Idle);
        assert!(matches!(
            disabled.phase,
            Phase::Disabled(DesktopUpdateStatus::Unavailable)
        ));

        let mut staged = Core::new(Phase::Staged);
        staged.finish_check(Phase::Idle);
        assert!(matches!(staged.phase, Phase::Idle));
    }

    #[test]
    fn restart_to_update_waits_while_a_meeting_records() {
        let updater = Updater::idle_checking();
        lock(&updater.shared.core).phase = Phase::Staged;
        let hold = offer_hold(&DesktopUpdateStatus::ReadyToRestart, true);
        assert!(hold.is_some());
        let error = updater.request_restart(hold).unwrap_err();
        assert!(error.to_string().contains("meeting"));
        {
            let mut core = lock(&updater.shared.core);
            assert!(matches!(core.phase, Phase::Staged));
            assert!(
                matches!(core.restart, Restart::Idle),
                "a refused restart is not left pending"
            );

            // A meeting that started after the click keeps the restart waiting.
            // Nothing is staged in this test, so the state follows the check's
            // result rather than claiming an update is ready.
            assert!(!core.finish_restart(Phase::Idle, hold, false));
            assert!(matches!(core.phase, Phase::Idle));
            assert!(matches!(core.restart, Restart::Done(Err(_))));
        }
    }

    #[test]
    fn a_release_that_finishes_staging_after_a_quit_installed_one_is_discarded() {
        // Hex is still running: the release becomes the staged update.
        for phase in [
            Phase::Staged,
            Phase::Checking {
                confirmed_switch: None,
            },
            Phase::Downloading,
            Phase::Idle,
        ] {
            let mut core = Core::new(phase);
            assert!(matches!(
                core.adopt_staged(staged_update("0.0.0-dev-42", 'c'), false),
                AdoptOutcome::Adopted
            ));
            assert_eq!(
                core.staged.as_ref().map(|u| u.version.as_str()),
                Some("0.0.0-dev-42")
            );
        }
        // Quitting installs what was staged (`install_on_quit`), so a release
        // that lands later must not replace it: the folder it unpacked into
        // holds the bundle the installed update replaced.
        for (phase, quitting) in [
            (Phase::Staged, true),
            (Phase::Downloading, true),
            (Phase::Applied, false),
            (Phase::Applied, true),
        ] {
            let mut core = Core::new(phase);
            assert!(matches!(
                core.adopt_staged(staged_update("0.0.0-dev-42", 'c'), quitting),
                AdoptOutcome::Discarded(_)
            ));
            assert!(core.staged.is_none());
        }
    }

    #[test]
    fn restart_to_update_checks_once_more_and_reports_once() {
        let updater = Updater::idle_checking();
        assert!(
            updater.act(&DesktopAction::RestartIntoUpdate).is_err(),
            "nothing is staged"
        );
        lock(&updater.shared.core).phase = Phase::Staged;
        updater.check_now();
        assert_eq!(
            updater.status(),
            DesktopUpdateStatus::ReadyToRestart,
            "a check keeps the staged update offered"
        );
        assert!(
            lock(&updater.shared.core).check_requested,
            "a staged update still checks"
        );
        lock(&updater.shared.core).check_requested = false;

        updater.act(&DesktopAction::RestartIntoUpdate).unwrap();
        assert_eq!(updater.status(), DesktopUpdateStatus::Checking);
        assert!(
            lock(&updater.shared.core).check_requested,
            "the final check starts at once"
        );
        updater
            .act(&DesktopAction::RestartIntoUpdate)
            .expect("a second click changes nothing");
        assert!(
            updater.take_restart().is_none(),
            "the final check is running"
        );

        lock(&updater.shared.core).restart = Restart::Done(Ok(()));
        assert!(matches!(updater.take_restart(), Some(Ok(()))));
        assert!(updater.take_restart().is_none(), "reported once");
    }

    #[test]
    fn the_dev_channel_checks_every_ten_minutes_and_others_hourly() {
        assert_eq!(check_interval("dev"), Duration::from_secs(600));
        assert_eq!(check_interval("stable"), Duration::from_secs(3600));
        assert_eq!(check_interval("beta"), Duration::from_secs(3600));
    }

    #[test]
    fn focusing_the_window_checks_the_dev_channel_at_most_once_a_minute() {
        let updater = Updater::idle_checking();
        updater.window_focused();
        assert!(
            lock(&updater.shared.core).check_requested,
            "the first focus checks"
        );
        lock(&updater.shared.core).check_requested = false;
        lock(&updater.shared.core).last_check = Some(("dev".into(), std::time::Instant::now()));
        updater.window_focused();
        assert!(
            !lock(&updater.shared.core).check_requested,
            "a check just started"
        );
        lock(&updater.shared.core).last_check = Some((
            "dev".into(),
            std::time::Instant::now() - FOCUS_CHECK_GAP - Duration::from_secs(1),
        ));
        updater.window_focused();
        assert!(
            lock(&updater.shared.core).check_requested,
            "the last check is over a minute old"
        );
        lock(&updater.shared.core).check_requested = false;
        lock(&updater.shared.core).last_check =
            Some(("stable".into(), std::time::Instant::now() - CHECK_INTERVAL));
        updater.window_focused();
        assert!(
            !lock(&updater.shared.core).check_requested,
            "Stable keeps its hourly checks"
        );
    }

    #[test]
    fn the_manifest_url_names_app_artifact_and_channel() {
        assert_eq!(
            manifest_url("stable"),
            "https://anoma.ly/release/hex/desktop/stable.json"
        );
    }
}
