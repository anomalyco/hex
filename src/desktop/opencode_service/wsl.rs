//! OpenCode in WSL2 (Windows): its registration lives in the default
//! distribution's home, which Windows reaches through `\\wsl.localhost\` or
//! `\\wsl$\` without running `wsl.exe`. WSL2 forwards loopback, so the
//! registered URL already answers from Windows; only the registration's
//! files and the workspace header need translating.
//!
//! Opening a distribution's files can start the WSL VM and take seconds, so
//! every lookup runs on a helper thread and the caller waits only for its
//! own deadline (or a short cap); a timeout, Escape, or a zero budget is
//! "OpenCode isn't running", and a later try finds a distribution that has
//! finished starting.

use std::fs;
#[cfg(target_os = "windows")]
use std::path::Path;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{Receiver, RecvTimeoutError};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use color_eyre::eyre::{Result, WrapErr};

use super::{Missing, Service};

/// `/etc/passwd` is tiny; never read more than this.
#[cfg(target_os = "windows")]
const MAX_PASSWD_BYTES: u64 = 1024 * 1024;
/// Both ways Windows exposes a distribution's filesystem; `\\wsl$` is the
/// older Windows 10 name, tried when `\\wsl.localhost` isn't reachable.
#[cfg_attr(not(target_os = "windows"), allow(dead_code))]
const WSL_PREFIXES: [&str; 2] = [r"\\wsl.localhost", r"\\wsl$"];
/// Settings and availability checks have no deadline of their own. A cold
/// WSL boot can take a few seconds; after this they see "OpenCode isn't
/// running" and a later try finds a distribution that has finished starting.
#[cfg_attr(not(target_os = "windows"), allow(dead_code))]
const WSL_LOOKUP_CAP: Duration = Duration::from_secs(3);
/// The home found last, so later lookups skip the registry and `/etc/passwd`.
static CACHED_HOME: Mutex<Option<WslHome>> = Mutex::new(None);

/// The default distribution's home, as Windows reads it and as its server
/// sees it.
#[derive(Clone, Debug)]
pub(super) struct WslHome {
    /// `\\wsl.localhost\<distribution><home>` or `\\wsl$\<distribution><home>`,
    /// the same folder Windows can read without running anything itself.
    pub(super) unc: PathBuf,
    /// The home's Linux path, such as `/home/dax`.
    pub(super) linux: String,
    /// Set once Hex's workspace was created through `unc`; clones share it,
    /// so one success is remembered by every request.
    workspace_ready: Arc<AtomicBool>,
}

impl PartialEq for WslHome {
    fn eq(&self, other: &Self) -> bool {
        self.unc == other.unc && self.linux == other.linux
    }
}

impl Eq for WslHome {}

impl WslHome {
    /// The home `linux_home` names in `distribution`, reached through
    /// `prefix`; everything is checked before it becomes a UNC path.
    #[cfg_attr(not(target_os = "windows"), allow(dead_code))]
    pub(super) fn home(prefix: &str, distribution: &str, linux_home: &str) -> Option<Self> {
        let distribution = distribution_component(distribution)?;
        let linux = linux_path(linux_home)?;
        Some(Self {
            unc: PathBuf::from(format!(
                r"{prefix}\{distribution}{}",
                linux.replace('/', r"\")
            )),
            linux,
            workspace_ready: Arc::new(AtomicBool::new(false)),
        })
    }

    #[cfg(test)]
    pub(super) fn for_test(unc: PathBuf, linux: &str) -> Self {
        Self {
            unc,
            linux: linux.to_owned(),
            workspace_ready: Arc::new(AtomicBool::new(false)),
        }
    }

    /// Where this home keeps OpenCode's registrations.
    #[cfg_attr(not(target_os = "windows"), allow(dead_code))]
    fn state_dir(&self) -> PathBuf {
        self.unc.join(".local").join("state").join("opencode")
    }

    /// Hex's workspace in the same home, through Windows.
    fn unc_workspace(&self) -> PathBuf {
        self.unc
            .join(".local")
            .join("share")
            .join("hex")
            .join("opencode")
    }

    /// Hex's workspace as the service in WSL sees it.
    fn linux_workspace(&self) -> String {
        format!("{}/.local/share/hex/opencode", self.linux)
    }

    /// Creates the workspace in the distribution once and returns the path
    /// its server sees, so a request's `x-opencode-directory` names a Linux
    /// path the server can use instead of a Windows one it reads as relative.
    pub(super) fn workspace(&self) -> Result<String> {
        if !self.workspace_ready.load(Ordering::Acquire) {
            let directory = self.unc_workspace();
            fs::create_dir_all(&directory).wrap_err_with(|| {
                format!(
                    "could not create OpenCode workspace {}",
                    directory.display()
                )
            })?;
            self.workspace_ready.store(true, Ordering::Release);
        }
        Ok(self.linux_workspace())
    }
}

/// A distribution is one UNC path component: no separators, no `.` or `..`,
/// and nothing Windows can't put in a folder name.
fn distribution_component(value: &str) -> Option<&str> {
    (!value.is_empty()
        && value != "."
        && value != ".."
        && !value.contains(['\\', '/'])
        && !value.chars().any(windows_forbidden))
    .then_some(value)
}

/// An absolute Linux home Windows can map into a distribution's UNC path,
/// with one trailing `/` allowed and dropped: no `.` or `..` components, no
/// other separators, and no characters Windows refuses.
fn linux_path(value: &str) -> Option<String> {
    let value = value.strip_suffix('/').unwrap_or(value);
    let rest = value.strip_prefix('/')?;
    if rest.is_empty()
        || value.contains('\\')
        || value.chars().any(windows_forbidden)
        || rest.split('/').any(|part| {
            part.is_empty()
                || part == "."
                || part == ".."
                || part.ends_with(' ')
                || part.ends_with('.')
        })
    {
        return None;
    }
    Some(value.to_owned())
}

/// Whether Windows can't name a folder with `character`.
fn windows_forbidden(character: char) -> bool {
    character.is_control() || "<>:\"|?*".contains(character)
}

/// The home `/etc/passwd` gives `uid`, when Windows can reach it. A line is
/// `name:password:uid:gid:gecos:home:shell`.
#[cfg_attr(not(target_os = "windows"), allow(dead_code))]
pub(super) fn home_for_uid(passwd: &str, uid: u32) -> Option<String> {
    for line in passwd.lines() {
        let mut fields = line.split(':');
        let (Some(_name), Some(_password), Some(line_uid), Some(_gid), Some(_gecos), Some(home)) = (
            fields.next(),
            fields.next(),
            fields.next(),
            fields.next(),
            fields.next(),
            fields.next(),
        ) else {
            continue;
        };
        if line_uid.parse::<u32>() == Ok(uid) {
            return linux_path(home);
        }
    }
    None
}

/// The home found last, if any.
#[cfg_attr(not(target_os = "windows"), allow(dead_code))]
fn cached_home() -> Option<WslHome> {
    CACHED_HOME.lock().ok()?.clone()
}

/// Remembers a home for later lookups, which then skip the registry and
/// `/etc/passwd`.
#[cfg_attr(not(target_os = "windows"), allow(dead_code))]
fn remember_home(home: &WslHome) {
    if let Ok(mut cache) = CACHED_HOME.lock() {
        *cache = Some(home.clone());
    }
}

/// Waits in slices for the helper thread's answer, so a rewrite's deadline
/// and Escape stay responsive while the thread blocks opening WSL. A timeout
/// or a cancelled caller is "OpenCode isn't running"; the helper finishes in
/// the background and its answer is dropped.
#[cfg_attr(not(target_os = "windows"), allow(dead_code))]
fn wait_for_lookup(
    responses: &Receiver<Result<Service, Missing>>,
    cap: Duration,
    cancelled: &AtomicBool,
    abort: &AtomicBool,
) -> Result<Service, Missing> {
    let started = Instant::now();
    loop {
        if cancelled.load(Ordering::Acquire) {
            abort.store(true, Ordering::Release);
            return Err(Missing::NotRunning);
        }
        let left = cap.saturating_sub(started.elapsed());
        if left.is_zero() {
            abort.store(true, Ordering::Release);
            return Err(Missing::NotRunning);
        }
        match responses.recv_timeout(left.min(super::WAIT_SLICE)) {
            Ok(result) => return result,
            Err(RecvTimeoutError::Timeout) => {}
            Err(RecvTimeoutError::Disconnected) => return Err(Missing::NotRunning),
        }
    }
}

/// The default distribution's home from WSL's own registration and its
/// `/etc/passwd`.
#[cfg(target_os = "windows")]
fn default_home() -> Option<WslHome> {
    use winreg::RegKey;
    use winreg::enums::{HKEY_CURRENT_USER, KEY_READ};

    let lxss = RegKey::predef(HKEY_CURRENT_USER)
        .open_subkey_with_flags(r"Software\Microsoft\Windows\CurrentVersion\Lxss", KEY_READ)
        .ok()?;
    let distribution: String = lxss.get_value("DefaultDistribution").ok()?;
    let distribution = lxss.open_subkey_with_flags(&distribution, KEY_READ).ok()?;
    let name: String = distribution.get_value("DistributionName").ok()?;
    let uid: u32 = distribution.get_value("DefaultUid").ok()?;
    // `\\wsl.localhost` needs a newer Windows 10; `\\wsl$` works on older ones.
    let mut found = None;
    for prefix in WSL_PREFIXES {
        let Some(path) = passwd_path(prefix, &name) else {
            continue;
        };
        if let Some(passwd) = read_bounded(&path) {
            found = Some((prefix, passwd));
            break;
        }
    }
    let (prefix, passwd) = found?;
    WslHome::home(prefix, &name, &home_for_uid(&passwd, uid)?)
}

/// `/etc/passwd` in a distribution, through Windows.
#[cfg(target_os = "windows")]
fn passwd_path(prefix: &str, distribution: &str) -> Option<PathBuf> {
    let distribution = distribution_component(distribution)?;
    Some(PathBuf::from(format!(
        r"{prefix}\{distribution}\etc\passwd"
    )))
}

/// Reads at most [`MAX_PASSWD_BYTES`]; a larger file is refused.
#[cfg(target_os = "windows")]
fn read_bounded(path: &Path) -> Option<String> {
    use std::io::Read;

    let mut file = fs::File::open(path).ok()?;
    let mut bytes = Vec::new();
    (&mut file)
        .take(MAX_PASSWD_BYTES + 1)
        .read_to_end(&mut bytes)
        .ok()?;
    if bytes.len() as u64 > MAX_PASSWD_BYTES {
        return None;
    }
    String::from_utf8(bytes).ok()
}

/// The helper thread's whole lookup: a remembered home, else the default
/// distribution's home, then its registrations.
#[cfg(target_os = "windows")]
fn lookup_home_and_service(
    budget: Option<Duration>,
    cancelled: &AtomicBool,
) -> Result<Service, Missing> {
    if cancelled.load(Ordering::Acquire) {
        return Err(Missing::NotRunning);
    }
    let home = match cached_home() {
        Some(home) => home,
        None => {
            let Some(home) = default_home() else {
                tracing::debug!("the default WSL distribution or its home could not be found");
                return Err(Missing::NotRunning);
            };
            remember_home(&home);
            home
        }
    };
    if cancelled.load(Ordering::Acquire) {
        return Err(Missing::NotRunning);
    }
    super::discover(&home.state_dir(), budget, cancelled, Some(&home))
}

/// Whether a lookup may start: a zero budget or a cancelled caller touches
/// nothing, including the registry and any WSL path.
#[cfg_attr(not(target_os = "windows"), allow(dead_code))]
fn should_lookup(budget: Option<Duration>, cancelled: &AtomicBool) -> bool {
    !cancelled.load(Ordering::Acquire) && !budget.is_some_and(|budget| budget.is_zero())
}

/// A service whose registration lives in the default WSL distribution, or
/// why there is none. The lookup runs on a helper thread so opening WSL never
/// outlives the caller's deadline, Escape, or the Settings cap.
#[cfg(target_os = "windows")]
pub(super) fn discover(
    budget: Option<Duration>,
    cancelled: &AtomicBool,
) -> Result<Service, Missing> {
    if !should_lookup(budget, cancelled) {
        return Err(Missing::NotRunning);
    }
    let cap = budget.unwrap_or(WSL_LOOKUP_CAP);
    let abort = Arc::new(AtomicBool::new(false));
    let worker_abort = Arc::clone(&abort);
    let (sender, responses) = std::sync::mpsc::channel();
    let spawned = std::thread::Builder::new()
        .name("hex-wsl-opencode".into())
        .spawn(move || {
            let _ = sender.send(lookup_home_and_service(Some(cap), &worker_abort));
        });
    if spawned.is_err() {
        return Err(Missing::NotRunning);
    }
    let result = wait_for_lookup(&responses, cap, cancelled, &abort);
    // A timed-out or cancelled helper stops between registrations.
    abort.store(true, Ordering::Release);
    result
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_passwd_line_for_the_uid_names_its_home() {
        let passwd = "root:x:0:0:root:/root:/bin/bash\ndax:x:1000:1000:Dax:/home/dax:/bin/zsh\n";
        assert_eq!(home_for_uid(passwd, 1000), Some("/home/dax".into()));
        assert_eq!(home_for_uid(passwd, 0), Some("/root".into()));
        assert_eq!(home_for_uid(passwd, 1001), None);
        assert_eq!(home_for_uid("", 1000), None);
        assert_eq!(
            home_for_uid("dax:x:1000:1000:Dax:/home/dax/:/bin/zsh", 1000),
            Some("/home/dax".into()),
            "one trailing slash is accepted and dropped"
        );
        for line in [
            "dax:x:1000",
            "dax:x:not-a-uid:1000:Dax:/home/dax:/bin/zsh",
            "dax:x:1000:1000:Dax",
            "dax:x:1000:1000:Dax:/home/../root:/bin/zsh",
            "dax:x:1000:1000:Dax:relative:/bin/zsh",
            "dax:x:1000:1000:Dax:/home/dax//:/bin/zsh",
        ] {
            assert_eq!(home_for_uid(line, 1000), None, "{line}");
        }
    }

    #[test]
    fn both_wsl_prefixes_name_the_distribution() {
        let localhost = WslHome::home(WSL_PREFIXES[0], "Ubuntu-24.04", "/home/dax").unwrap();
        assert_eq!(
            localhost.unc,
            PathBuf::from(r"\\wsl.localhost\Ubuntu-24.04\home\dax")
        );
        let dollar = WslHome::home(WSL_PREFIXES[1], "Ubuntu-24.04", "/home/dax").unwrap();
        assert_eq!(dollar.unc, PathBuf::from(r"\\wsl$\Ubuntu-24.04\home\dax"));
        assert_eq!(localhost.linux, dollar.linux);
        let trimmed = WslHome::home(WSL_PREFIXES[0], "Ubuntu-24.04", "/home/dax/").unwrap();
        assert_eq!(trimmed.unc, localhost.unc);
        assert_eq!(trimmed.linux, "/home/dax");
        assert_eq!(
            localhost.linux_workspace(),
            "/home/dax/.local/share/hex/opencode"
        );
    }

    #[test]
    fn a_distribution_or_home_windows_cant_reach_is_refused() {
        assert!(WslHome::home(WSL_PREFIXES[0], "Ubuntu", "/home/dax").is_some());
        assert!(WslHome::home(WSL_PREFIXES[0], "Ubuntu", "/root").is_some());
        for (prefix, distribution, home) in [
            (WSL_PREFIXES[0], "", "/home/dax"),
            (WSL_PREFIXES[0], "..", "/home/dax"),
            (WSL_PREFIXES[0], "Ubu/ntu", "/home/dax"),
            (WSL_PREFIXES[0], "Ubu\\ntu", "/home/dax"),
            (WSL_PREFIXES[0], "Ubuntu", ""),
            (WSL_PREFIXES[0], "Ubuntu", "/"),
            (WSL_PREFIXES[0], "Ubuntu", "home/dax"),
            (WSL_PREFIXES[0], "Ubuntu", "/home/../root"),
            (WSL_PREFIXES[0], "Ubuntu", "/home/./dax"),
            (WSL_PREFIXES[0], "Ubuntu", "/home//dax"),
            (WSL_PREFIXES[0], "Ubuntu", "/home\\dax"),
            (WSL_PREFIXES[0], "Ubuntu", "/home/da:x"),
            (WSL_PREFIXES[0], "Ubuntu", "/home/dax "),
            (WSL_PREFIXES[0], "Ubuntu", "/home/dax//"),
        ] {
            assert_eq!(
                WslHome::home(prefix, distribution, home),
                None,
                "{prefix:?} {distribution:?} {home:?}"
            );
        }
    }

    #[test]
    fn the_workspace_is_created_somewhere_windows_can_write_it() {
        let root = std::env::temp_dir().join(format!("hex-wsl-workspace-{}", std::process::id()));
        let _ = fs::remove_dir_all(&root);
        let home = WslHome::for_test(root.join("home"), "/home/dax");
        assert_eq!(home.state_dir(), root.join("home/.local/state/opencode"));
        let workspace = root.join("home/.local/share/hex/opencode");
        assert_eq!(
            home.workspace().unwrap(),
            "/home/dax/.local/share/hex/opencode"
        );
        assert!(workspace.is_dir());
        fs::remove_dir_all(&workspace).unwrap();
        assert_eq!(
            home.workspace().unwrap(),
            "/home/dax/.local/share/hex/opencode"
        );
        assert!(
            !workspace.exists(),
            "a workspace created once is not created again"
        );
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn a_zero_budget_or_cancellation_skips_the_lookup() {
        let open = AtomicBool::new(false);
        assert!(!should_lookup(Some(Duration::ZERO), &open));
        assert!(!should_lookup(None, &AtomicBool::new(true)));
        assert!(should_lookup(None, &open));
        assert!(should_lookup(Some(Duration::from_millis(1)), &open));
    }

    #[test]
    fn a_found_home_is_cached() {
        CACHED_HOME.lock().unwrap().take();
        let root = std::env::temp_dir().join(format!("hex-wsl-cache-{}", std::process::id()));
        let _ = fs::remove_dir_all(&root);
        let home = WslHome::for_test(root.join("home"), "/home/dax");
        assert_eq!(cached_home(), None);
        remember_home(&home);
        let cached = cached_home().unwrap();
        assert_eq!(cached, home);
        // The cached copy shares the workspace success with the original.
        let workspace = root.join("home/.local/share/hex/opencode");
        cached.workspace().unwrap();
        assert!(workspace.is_dir());
        fs::remove_dir_all(&workspace).unwrap();
        home.workspace().unwrap();
        assert!(!workspace.exists());
        CACHED_HOME.lock().unwrap().take();
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn a_lookup_wait_returns_the_helpers_answer_or_stops_at_its_cap() {
        let (sender, responses) = std::sync::mpsc::channel();
        let cancelled = AtomicBool::new(false);
        let abort = AtomicBool::new(false);
        let service = super::super::tests::fixture("http://127.0.0.1:1/");
        sender.send(Ok(service.clone())).unwrap();
        assert_eq!(
            wait_for_lookup(&responses, Duration::from_secs(1), &cancelled, &abort),
            Ok(service)
        );
        sender.send(Err(Missing::Outdated)).unwrap();
        assert_eq!(
            wait_for_lookup(&responses, Duration::from_secs(1), &cancelled, &abort),
            Err(Missing::Outdated)
        );

        // Nothing answers: the wait ends at its cap, not the sender's life.
        let started = Instant::now();
        assert_eq!(
            wait_for_lookup(&responses, Duration::from_millis(100), &cancelled, &abort),
            Err(Missing::NotRunning)
        );
        assert!(started.elapsed() < Duration::from_secs(1));
        assert!(abort.load(Ordering::Acquire));
        drop(sender);
    }

    #[test]
    fn a_lookup_wait_stops_at_once_when_the_caller_cancels() {
        let (_sender, responses) = std::sync::mpsc::channel();
        let cancelled = AtomicBool::new(true);
        let abort = AtomicBool::new(false);
        let started = Instant::now();
        assert_eq!(
            wait_for_lookup(&responses, Duration::from_secs(30), &cancelled, &abort),
            Err(Missing::NotRunning)
        );
        assert!(started.elapsed() < Duration::from_secs(1));
        assert!(abort.load(Ordering::Acquire));
    }
}
