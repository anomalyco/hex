//! OpenCode in WSL2 (Windows): its registration lives in the default
//! distribution's home, which Windows reaches through `\\wsl.localhost\` or
//! `\\wsl$\` without running `wsl.exe`. WSL2 forwards loopback, so the
//! registered URL already answers from Windows; only the registration's
//! files and the workspace header need translating.
//!
//! Opening a distribution's files can start the WSL VM and take seconds, so
//! one lookup runs at a time on a helper thread; every caller waits for that
//! one answer only as long as its own deadline (or a short cap). A timeout,
//! Escape, or a zero budget is "OpenCode isn't running", and a later try
//! finds a distribution that has finished starting.

use std::fs;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Condvar, Mutex};
use std::time::{Duration, Instant};

use color_eyre::eyre::{Result, WrapErr};

use super::{Missing, Service};

/// `/etc/passwd` and `/etc/wsl.conf` are tiny; never read more than this.
const MAX_ETC_BYTES: u64 = 1024 * 1024;
/// Both ways Windows exposes a distribution's filesystem; `\\wsl$` is the
/// older Windows 10 name, tried when `\\wsl.localhost` isn't reachable.
#[cfg_attr(not(target_os = "windows"), allow(dead_code))]
const WSL_PREFIXES: [&str; 2] = [r"\\wsl.localhost", r"\\wsl$"];
/// Settings and availability checks have no deadline of their own. A cold
/// WSL boot can take a few seconds; after this they see "OpenCode isn't
/// running" and a later try finds a distribution that has finished starting.
#[cfg_attr(not(target_os = "windows"), allow(dead_code))]
const WSL_LOOKUP_CAP: Duration = Duration::from_secs(3);
/// A lookup that ran into its cap without finding a service makes new
/// lookups wait this long, so rewrites during a hung WSL stop paying the
/// wait over and over.
const WSL_SKIP_AFTER_TIMEOUT: Duration = Duration::from_secs(10);

/// The home last resolved, with the default distribution and uid it came
/// from; a changed default rebuilds it from `/etc/passwd`.
static CACHED_HOME: Mutex<Option<CachedHome>> = Mutex::new(None);
/// No new lookup starts before this instant after one timed out.
static SKIP_UNTIL: Mutex<Option<Instant>> = Mutex::new(None);
/// The one lookup in flight, shared by every caller.
static IN_FLIGHT: Mutex<Option<Arc<Pending>>> = Mutex::new(None);

struct CachedHome {
    distribution: String,
    uid: u32,
    home: WslHome,
}

struct Pending {
    started: Instant,
    result: Mutex<Option<Result<Service, Missing>>>,
    ready: Condvar,
}

impl Pending {
    fn new() -> Self {
        Self {
            started: Instant::now(),
            result: Mutex::new(None),
            ready: Condvar::new(),
        }
    }
}

/// The default distribution's home, as Windows reads it and as its server
/// sees it.
#[derive(Clone, Debug)]
pub(super) struct WslHome {
    /// `\\wsl.localhost\<distribution><home>` or `\\wsl$\<distribution><home>`,
    /// the same folder Windows can read without running anything itself.
    pub(super) unc: PathBuf,
    /// The home's Linux path, such as `/home/dax`.
    pub(super) linux: String,
    /// `/etc/wsl.conf`'s `[automount]`, read once when the home was looked
    /// up, so naming a capture folder never reads over UNC again.
    automount: Automount,
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
    fn home(
        prefix: &str,
        distribution: &str,
        linux_home: &str,
        automount: Automount,
    ) -> Option<Self> {
        let distribution = distribution_component(distribution)?;
        let linux = linux_path(linux_home)?;
        Some(Self {
            unc: PathBuf::from(format!(
                r"{prefix}\{distribution}{}",
                linux.replace('/', r"\")
            )),
            linux,
            automount,
            workspace_ready: Arc::new(AtomicBool::new(false)),
        })
    }

    #[cfg(test)]
    pub(super) fn for_test(unc: PathBuf, linux: &str) -> Self {
        Self {
            unc,
            linux: linux.to_owned(),
            automount: Automount::default(),
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

    /// Makes the next request create the workspace again after this one
    /// failed over the same path.
    pub(super) fn forget_workspace(&self) {
        self.workspace_ready.store(false, Ordering::Release);
    }

    /// Hex's workspace as the service in WSL sees it.
    fn linux_workspace(&self) -> String {
        format!("{}/.local/share/hex/opencode", self.linux)
    }

    /// `host` as a server in this distribution sees it, using the
    /// `[automount]` read when this home was looked up: `None` when drives
    /// aren't automounted or `host` isn't on one.
    pub(super) fn directory(&self, host: &Path) -> Option<String> {
        linux_directory(&self.automount, host)
    }

    /// Creates the workspace in the distribution once and returns the path
    /// its server sees, so a request's `x-opencode-directory` names a Linux
    /// path the server can use instead of a Windows one it reads as relative.
    pub(super) fn workspace(&self) -> Result<String> {
        if !self.workspace_ready.load(Ordering::Acquire) {
            let directory = self.unc_workspace();
            if let Err(error) = fs::create_dir_all(&directory) {
                self.workspace_ready.store(false, Ordering::Release);
                return Err(error).wrap_err_with(|| {
                    format!(
                        "could not create OpenCode workspace {}",
                        directory.display()
                    )
                });
            }
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

/// `<prefix>\<distribution>\etc`, where a distribution keeps `/etc/passwd` and
/// `/etc/wsl.conf`.
#[cfg_attr(not(target_os = "windows"), allow(dead_code))]
fn etc_dir(prefix: &str, distribution: &str) -> Option<PathBuf> {
    let distribution = distribution_component(distribution)?;
    Some(PathBuf::from(format!(r"{prefix}\{distribution}\etc")))
}

/// `/etc/wsl.conf`'s `[automount]` section: drives are mounted under `root`
/// unless `enabled` is false. A missing or malformed value keeps WSL's
/// default.
#[derive(Clone, Debug, PartialEq, Eq)]
struct Automount {
    enabled: bool,
    root: String,
}

impl Default for Automount {
    fn default() -> Self {
        Self {
            enabled: true,
            root: "/mnt/".into(),
        }
    }
}

/// `/etc/wsl.conf`'s `[automount]`, read once with the home.
#[cfg_attr(not(target_os = "windows"), allow(dead_code))]
fn automount_at(etc: &Path) -> Automount {
    let conf = read_bounded(&etc.join("wsl.conf"));
    automount(conf.as_deref().unwrap_or_default())
}

/// The automount settings of `distribution`, read through `prefix` on the
/// same lookup that reads `/etc/passwd`.
#[cfg(target_os = "windows")]
fn read_automount(prefix: &str, distribution: &str) -> Automount {
    etc_dir(prefix, distribution).map_or_else(Automount::default, |etc| automount_at(&etc))
}

fn automount(conf: &str) -> Automount {
    let mut automount = Automount::default();
    let mut in_automount = false;
    for line in conf.lines() {
        let line = line.trim();
        if line.is_empty() || line.starts_with(['#', ';']) {
            continue;
        }
        if let Some(section) = line
            .strip_prefix('[')
            .and_then(|line| line.strip_suffix(']'))
        {
            in_automount = section.trim().eq_ignore_ascii_case("automount");
            continue;
        }
        if !in_automount {
            continue;
        }
        let Some((key, value)) = line.split_once('=') else {
            continue;
        };
        let key = key.trim();
        let value = ini_value(value);
        if key.eq_ignore_ascii_case("enabled") {
            automount.enabled = !value.eq_ignore_ascii_case("false");
        } else if key.eq_ignore_ascii_case("root")
            && let Some(root) = automount_root(value)
        {
            automount.root = root;
        }
    }
    automount
}

/// An INI value with one matching pair of surrounding quotes dropped and an
/// inline ` #` comment removed outside quotes.
fn ini_value(value: &str) -> &str {
    let mut quote = None;
    let mut end = value.len();
    for (index, character) in value.char_indices() {
        match (quote, character) {
            (None, '"' | '\'') => quote = Some(character),
            (Some(open), character) if character == open => quote = None,
            (None, '#')
                if value[..index]
                    .chars()
                    .next_back()
                    .is_some_and(char::is_whitespace) =>
            {
                end = index;
                break;
            }
            _ => {}
        }
    }
    let value = value[..end].trim();
    let bytes = value.as_bytes();
    if value.len() >= 2 && matches!(bytes[0], b'"' | b'\'') && bytes[value.len() - 1] == bytes[0] {
        return value[1..value.len() - 1].trim();
    }
    value
}

/// `root = /mnt/` as a mount point with exactly one trailing `/`; a value
/// that isn't an absolute Linux path is ignored.
fn automount_root(value: &str) -> Option<String> {
    if !value.starts_with('/')
        || value.contains(['\\', '\0'])
        || value.chars().any(char::is_control)
    {
        return None;
    }
    Some(format!("{}/", value.trim_end_matches('/')))
}

/// A Windows path as a server in the distribution sees it: `X:\a\b` at the
/// cached `[automount]` root (`/mnt/x/a/b` by default), or `None` when drives
/// aren't automounted or `host` isn't on one (a UNC share or a relative path).
fn linux_directory(automount: &Automount, host: &Path) -> Option<String> {
    if !automount.enabled {
        return None;
    }
    let text = host.to_string_lossy();
    let text = text.strip_prefix(r"\\?\").unwrap_or(&text);
    let bytes = text.as_bytes();
    let drive = bytes.first().copied().filter(u8::is_ascii_alphabetic)?;
    if bytes.get(1) != Some(&b':') {
        return None;
    }
    let rest = &text[2..];
    if !rest.is_empty() && !rest.starts_with(['\\', '/']) {
        return None;
    }
    let rest = rest.replace('\\', "/");
    let rest = rest.trim_end_matches('/');
    Some(format!(
        "{}{}{rest}",
        automount.root,
        char::from(drive.to_ascii_lowercase())
    ))
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

/// The cached home only when it belongs to the current default distribution
/// and uid.
fn matching_home(cache: &Option<CachedHome>, distribution: &str, uid: u32) -> Option<WslHome> {
    cache
        .as_ref()
        .filter(|cached| cached.distribution == distribution && cached.uid == uid)
        .map(|cached| cached.home.clone())
}

#[cfg_attr(not(target_os = "windows"), allow(dead_code))]
fn cached_home(distribution: &str, uid: u32) -> Option<WslHome> {
    let cache = CACHED_HOME
        .lock()
        .unwrap_or_else(|poison| poison.into_inner());
    matching_home(&cache, distribution, uid)
}

#[cfg_attr(not(target_os = "windows"), allow(dead_code))]
fn remember_home(distribution: &str, uid: u32, home: &WslHome) {
    let mut cache = CACHED_HOME
        .lock()
        .unwrap_or_else(|poison| poison.into_inner());
    *cache = Some(CachedHome {
        distribution: distribution.to_owned(),
        uid,
        home: home.clone(),
    });
}

fn clear_cached_home() {
    let mut cache = CACHED_HOME
        .lock()
        .unwrap_or_else(|poison| poison.into_inner());
    *cache = None;
}

/// Drops the remembered home, called whenever a lookup runs with the lab
/// off and when a cached home's registrations can't be read.
#[cfg_attr(not(target_os = "windows"), allow(dead_code))]
pub(super) fn clear_cache() {
    clear_cached_home();
}

/// Whether this instant is still inside the pause after a timed-out lookup.
fn still_deferred(until: Option<Instant>, now: Instant) -> bool {
    until.is_some_and(|until| now < until)
}

fn skip_active() -> bool {
    let until = *SKIP_UNTIL
        .lock()
        .unwrap_or_else(|poison| poison.into_inner());
    still_deferred(until, Instant::now())
}

fn defer_lookups() {
    let mut until = SKIP_UNTIL
        .lock()
        .unwrap_or_else(|poison| poison.into_inner());
    *until = Some(Instant::now() + WSL_SKIP_AFTER_TIMEOUT);
}

/// Waits as a caller for the shared lookup's answer. A short caller returns
/// "isn't running" without disturbing the lookup the others share.
#[cfg_attr(not(target_os = "windows"), allow(dead_code))]
fn wait_for_pending(
    pending: &Pending,
    cap: Duration,
    cancelled: &AtomicBool,
) -> Result<Service, Missing> {
    let started = Instant::now();
    let mut result = pending
        .result
        .lock()
        .unwrap_or_else(|poison| poison.into_inner());
    loop {
        if let Some(result) = result.as_ref() {
            return result.clone();
        }
        if cancelled.load(Ordering::Acquire) {
            return Err(Missing::NotRunning);
        }
        let left = cap.saturating_sub(started.elapsed());
        if left.is_zero() {
            return Err(Missing::NotRunning);
        }
        let (next, _) = pending
            .ready
            .wait_timeout(result, left.min(super::WAIT_SLICE))
            .unwrap_or_else(|poison| poison.into_inner());
        result = next;
    }
}

/// Publishes the helper's answer and wakes every caller waiting on it.
fn publish(pending: &Pending, result: Result<Service, Missing>) {
    let mut slot = pending
        .result
        .lock()
        .unwrap_or_else(|poison| poison.into_inner());
    *slot = Some(result);
    drop(slot);
    pending.ready.notify_all();
}

/// Clears the in-flight slot if it is still this lookup.
fn finish(pending: &Arc<Pending>) {
    let mut slot = IN_FLIGHT
        .lock()
        .unwrap_or_else(|poison| poison.into_inner());
    if slot
        .as_ref()
        .is_some_and(|in_flight| Arc::ptr_eq(in_flight, pending))
    {
        *slot = None;
    }
}

/// Clears the shared slot if the helper unwinds before finishing.
struct ClearOnDrop<'a>(&'a Arc<Pending>);

impl Drop for ClearOnDrop<'_> {
    fn drop(&mut self) {
        finish(self.0);
    }
}

/// Runs `lookup` on the one helper thread and publishes its answer. A caller
/// joins an existing lookup instead of spawning a second one; its own
/// timeout or cancellation only stops its wait.
#[cfg_attr(not(target_os = "windows"), allow(dead_code))]
fn run_shared_lookup(
    lookup: impl FnOnce() -> Result<Service, Missing> + Send + 'static,
    lookup_cap: Duration,
    budget: Option<Duration>,
    cancelled: &AtomicBool,
) -> Result<Service, Missing> {
    let own_cap = budget.unwrap_or(lookup_cap);
    let (pending, spawn) = {
        let mut slot = IN_FLIGHT
            .lock()
            .unwrap_or_else(|poison| poison.into_inner());
        match slot.as_ref() {
            Some(pending) => (Arc::clone(pending), false),
            None if skip_active() => return Err(Missing::NotRunning),
            None => {
                let pending = Arc::new(Pending::new());
                *slot = Some(Arc::clone(&pending));
                (pending, true)
            }
        }
    };
    let wait_cap = if spawn {
        own_cap
    } else {
        // A joiner never waits for longer than the shared lookup has left.
        let left = lookup_cap.saturating_sub(pending.started.elapsed());
        if left.is_zero() {
            return Err(Missing::NotRunning);
        }
        own_cap.min(left)
    };
    if spawn {
        let helper = Arc::clone(&pending);
        let spawned = std::thread::Builder::new()
            .name("hex-wsl-opencode".into())
            .spawn(move || {
                let _clear = ClearOnDrop(&helper);
                let started = Instant::now();
                let result = lookup();
                let timed_out = started.elapsed() >= lookup_cap;
                let failed = result.is_err();
                publish(&helper, result);
                if timed_out && failed {
                    defer_lookups();
                }
                finish(&helper);
            });
        if spawned.is_err() {
            finish(&pending);
            return Err(Missing::NotRunning);
        }
    }
    wait_for_pending(&pending, wait_cap, cancelled)
}

/// The default distribution's name and uid from WSL's registry entry. Local
/// and cheap; it never opens the distribution's files.
#[cfg(target_os = "windows")]
fn default_identity() -> Option<(String, u32)> {
    use winreg::RegKey;
    use winreg::enums::{HKEY_CURRENT_USER, KEY_READ};

    let lxss = RegKey::predef(HKEY_CURRENT_USER)
        .open_subkey_with_flags(r"Software\Microsoft\Windows\CurrentVersion\Lxss", KEY_READ)
        .ok()?;
    let distribution: String = lxss.get_value("DefaultDistribution").ok()?;
    let distribution = lxss.open_subkey_with_flags(&distribution, KEY_READ).ok()?;
    let name: String = distribution.get_value("DistributionName").ok()?;
    let uid: u32 = distribution.get_value("DefaultUid").ok()?;
    Some((name, uid))
}

/// The default distribution's home, and whether it came from the cache.
#[cfg(target_os = "windows")]
fn default_home() -> Option<(WslHome, bool)> {
    let (distribution, uid) = default_identity()?;
    if let Some(home) = cached_home(&distribution, uid) {
        return Some((home, true));
    }
    let (prefix, passwd) = read_passwd(&distribution)?;
    let home = home_for_uid(&passwd, uid)?;
    let automount = read_automount(prefix, &distribution);
    let home = WslHome::home(prefix, &distribution, &home, automount)?;
    remember_home(&distribution, uid, &home);
    Some((home, false))
}

/// The first of `\\wsl.localhost` and `\\wsl$` whose `/etc/passwd` opens.
#[cfg(target_os = "windows")]
fn read_passwd(distribution: &str) -> Option<(&'static str, String)> {
    for prefix in WSL_PREFIXES {
        let Some(path) = passwd_path(prefix, distribution) else {
            continue;
        };
        if let Some(passwd) = read_bounded(&path) {
            return Some((prefix, passwd));
        }
    }
    None
}

/// `/etc/passwd` in a distribution, through Windows.
#[cfg(target_os = "windows")]
fn passwd_path(prefix: &str, distribution: &str) -> Option<PathBuf> {
    Some(etc_dir(prefix, distribution)?.join("passwd"))
}

/// Reads at most [`MAX_ETC_BYTES`]; a larger file is refused.
fn read_bounded(path: &Path) -> Option<String> {
    use std::io::Read;

    let mut file = fs::File::open(path).ok()?;
    let mut bytes = Vec::new();
    (&mut file)
        .take(MAX_ETC_BYTES + 1)
        .read_to_end(&mut bytes)
        .ok()?;
    if bytes.len() as u64 > MAX_ETC_BYTES {
        return None;
    }
    String::from_utf8(bytes).ok()
}

/// The helper's whole lookup: the current default's home, then its
/// registrations. A cached home whose registrations can't be read is
/// forgotten so the next lookup rebuilds it.
#[cfg(target_os = "windows")]
fn lookup_home_and_service(budget: Option<Duration>) -> Result<Service, Missing> {
    let Some((home, from_cache)) = default_home() else {
        tracing::debug!("the default WSL distribution or its home could not be found");
        return Err(Missing::NotRunning);
    };
    let state = home.state_dir();
    let result = super::discover(&state, budget, &AtomicBool::new(false), Some(&home));
    if result.is_err() && from_cache && fs::read_dir(&state).is_err() {
        clear_cached_home();
    }
    result
}

/// Whether a lookup may start: a zero budget or a cancelled caller touches
/// nothing, including the registry and any WSL path.
#[cfg_attr(not(target_os = "windows"), allow(dead_code))]
fn should_lookup(budget: Option<Duration>, cancelled: &AtomicBool) -> bool {
    !cancelled.load(Ordering::Acquire) && !budget.is_some_and(|budget| budget.is_zero())
}

/// A service whose registration lives in the default WSL distribution, or
/// why there is none. One lookup is shared by every caller; each waits only
/// for its own deadline, Escape, or the Settings cap.
#[cfg(target_os = "windows")]
pub(super) fn discover(
    budget: Option<Duration>,
    cancelled: &AtomicBool,
) -> Result<Service, Missing> {
    if !should_lookup(budget, cancelled) {
        return Err(Missing::NotRunning);
    }
    run_shared_lookup(
        || lookup_home_and_service(Some(WSL_LOOKUP_CAP)),
        WSL_LOOKUP_CAP,
        budget,
        cancelled,
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::AtomicUsize;
    use std::sync::mpsc;

    /// Serializes the tests that share the in-flight and skip statics.
    static LOOKUP_TEST_LOCK: Mutex<()> = Mutex::new(());

    fn clear_lookup_state() {
        *IN_FLIGHT
            .lock()
            .unwrap_or_else(|poison| poison.into_inner()) = None;
        *SKIP_UNTIL
            .lock()
            .unwrap_or_else(|poison| poison.into_inner()) = None;
    }

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
        let localhost = WslHome::home(
            WSL_PREFIXES[0],
            "Ubuntu-24.04",
            "/home/dax",
            Automount::default(),
        )
        .unwrap();
        assert_eq!(
            localhost.unc,
            PathBuf::from(r"\\wsl.localhost\Ubuntu-24.04\home\dax")
        );
        let dollar = WslHome::home(
            WSL_PREFIXES[1],
            "Ubuntu-24.04",
            "/home/dax",
            Automount::default(),
        )
        .unwrap();
        assert_eq!(dollar.unc, PathBuf::from(r"\\wsl$\Ubuntu-24.04\home\dax"));
        assert_eq!(localhost.linux, dollar.linux);
        let trimmed = WslHome::home(
            WSL_PREFIXES[0],
            "Ubuntu-24.04",
            "/home/dax/",
            Automount::default(),
        )
        .unwrap();
        assert_eq!(trimmed.unc, localhost.unc);
        assert_eq!(trimmed.linux, "/home/dax");
        assert_eq!(
            localhost.linux_workspace(),
            "/home/dax/.local/share/hex/opencode"
        );
    }

    #[test]
    fn a_distribution_or_home_windows_cant_reach_is_refused() {
        assert!(
            WslHome::home(WSL_PREFIXES[0], "Ubuntu", "/home/dax", Automount::default()).is_some()
        );
        assert!(WslHome::home(WSL_PREFIXES[0], "Ubuntu", "/root", Automount::default()).is_some());
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
                WslHome::home(prefix, distribution, home, Automount::default()),
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
    fn the_cache_follows_the_default_distribution_and_uid() {
        let first = WslHome::for_test(PathBuf::from("/cache/one"), "/home/one");
        let second = WslHome::for_test(PathBuf::from("/cache/two"), "/home/two");
        let cache = Some(CachedHome {
            distribution: "Ubuntu".into(),
            uid: 1000,
            home: first.clone(),
        });
        assert_eq!(matching_home(&cache, "Ubuntu", 1000), Some(first.clone()));
        assert_eq!(matching_home(&cache, "Debian", 1000), None);
        assert_eq!(matching_home(&cache, "Ubuntu", 1001), None);
        assert_eq!(matching_home(&None, "Ubuntu", 1000), None);

        clear_cached_home();
        remember_home("Ubuntu", 1000, &first);
        assert_eq!(cached_home("Ubuntu", 1000), Some(first.clone()));
        assert_eq!(cached_home("Debian", 1000), None);
        assert_eq!(cached_home("Ubuntu", 1001), None);
        remember_home("Debian", 1001, &second);
        assert_eq!(cached_home("Ubuntu", 1000), None);
        assert_eq!(cached_home("Debian", 1001), Some(second.clone()));
        clear_cache();
        assert_eq!(cached_home("Debian", 1001), None);
    }

    #[test]
    fn one_lookup_serves_every_caller_and_a_short_wait_stops() {
        let _guard = LOOKUP_TEST_LOCK
            .lock()
            .unwrap_or_else(|poison| poison.into_inner());
        clear_lookup_state();
        let (started, started_rx) = mpsc::channel();
        let (release, release_rx) = mpsc::channel();
        let calls = Arc::new(AtomicUsize::new(0));
        let service = super::super::tests::fixture("http://127.0.0.1:1/");
        let first = {
            let service = service.clone();
            let calls = Arc::clone(&calls);
            std::thread::spawn(move || {
                run_shared_lookup(
                    move || {
                        calls.fetch_add(1, Ordering::SeqCst);
                        started.send(()).unwrap();
                        release_rx.recv().unwrap();
                        Ok(service)
                    },
                    WSL_LOOKUP_CAP,
                    Some(Duration::from_secs(10)),
                    &AtomicBool::new(false),
                )
            })
        };
        started_rx.recv().unwrap();

        let short = Instant::now();
        let short_calls = Arc::clone(&calls);
        assert_eq!(
            run_shared_lookup(
                move || {
                    short_calls.fetch_add(1, Ordering::SeqCst);
                    Err(Missing::NotRunning)
                },
                WSL_LOOKUP_CAP,
                Some(Duration::from_millis(50)),
                &AtomicBool::new(false),
            ),
            Err(Missing::NotRunning),
            "a short caller stops while the shared lookup runs"
        );
        assert!(short.elapsed() < Duration::from_secs(1));

        let (third_tx, third_rx) = mpsc::channel();
        let third = {
            let calls = Arc::clone(&calls);
            std::thread::spawn(move || {
                let result = run_shared_lookup(
                    move || {
                        calls.fetch_add(1, Ordering::SeqCst);
                        Err(Missing::NotRunning)
                    },
                    WSL_LOOKUP_CAP,
                    Some(Duration::from_secs(10)),
                    &AtomicBool::new(false),
                );
                third_tx.send(result).unwrap();
            })
        };
        assert!(
            third_rx.recv_timeout(Duration::from_millis(100)).is_err(),
            "the second long caller waits for the first lookup"
        );

        release.send(()).unwrap();
        let first_result = first.join().unwrap();
        let third_result = third_rx.recv_timeout(Duration::from_secs(5)).unwrap();
        third.join().unwrap();
        assert_eq!(first_result, Ok(service.clone()));
        assert_eq!(third_result, Ok(service));
        assert_eq!(
            calls.load(Ordering::SeqCst),
            1,
            "only the first caller's lookup ran"
        );
        clear_lookup_state();
    }

    #[test]
    fn a_joiner_does_not_wait_for_a_lookup_older_than_its_cap() {
        let _guard = LOOKUP_TEST_LOCK
            .lock()
            .unwrap_or_else(|poison| poison.into_inner());
        clear_lookup_state();
        let cap = Duration::from_millis(100);
        let (started, started_rx) = mpsc::channel();
        let (release, release_rx) = mpsc::channel();
        let calls = Arc::new(AtomicUsize::new(0));
        let service = super::super::tests::fixture("http://127.0.0.1:1/");
        let first = {
            let service = service.clone();
            let calls = Arc::clone(&calls);
            std::thread::spawn(move || {
                run_shared_lookup(
                    move || {
                        calls.fetch_add(1, Ordering::SeqCst);
                        started.send(()).unwrap();
                        release_rx.recv().unwrap();
                        Ok(service)
                    },
                    cap,
                    Some(Duration::from_secs(10)),
                    &AtomicBool::new(false),
                )
            })
        };
        started_rx.recv().unwrap();
        std::thread::sleep(cap + Duration::from_millis(50));

        let second_started = Instant::now();
        let second_calls = Arc::clone(&calls);
        assert_eq!(
            run_shared_lookup(
                move || {
                    second_calls.fetch_add(1, Ordering::SeqCst);
                    Err(Missing::NotRunning)
                },
                cap,
                Some(Duration::from_secs(30)),
                &AtomicBool::new(false),
            ),
            Err(Missing::NotRunning),
            "a lookup past its cap is not joined"
        );
        assert!(
            second_started.elapsed() < Duration::from_millis(500),
            "the joiner returned without waiting: {:?}",
            second_started.elapsed()
        );
        assert_eq!(
            calls.load(Ordering::SeqCst),
            1,
            "the stale lookup's caller still owns the only lookup"
        );

        release.send(()).unwrap();
        assert_eq!(first.join().unwrap(), Ok(service));
        clear_lookup_state();
    }

    #[test]
    fn a_timed_out_lookup_defers_the_next_one() {
        let _guard = LOOKUP_TEST_LOCK
            .lock()
            .unwrap_or_else(|poison| poison.into_inner());
        clear_lookup_state();
        let now = Instant::now();
        assert!(still_deferred(Some(now + WSL_SKIP_AFTER_TIMEOUT), now));
        assert!(!still_deferred(Some(now), now));
        assert!(!still_deferred(None, now));

        defer_lookups();
        let calls = Arc::new(AtomicUsize::new(0));
        let called = Arc::clone(&calls);
        assert_eq!(
            run_shared_lookup(
                move || {
                    called.fetch_add(1, Ordering::SeqCst);
                    Err(Missing::NotRunning)
                },
                WSL_LOOKUP_CAP,
                Some(Duration::from_secs(1)),
                &AtomicBool::new(false),
            ),
            Err(Missing::NotRunning)
        );
        assert_eq!(
            calls.load(Ordering::SeqCst),
            0,
            "the paused window starts nothing"
        );
        clear_lookup_state();
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
    fn the_automount_section_gives_the_root_and_can_be_off() {
        assert_eq!(automount(""), Automount::default());
        assert_eq!(
            automount("[boot]\ncommand = /bin/true\nroot = /ignored/\n"),
            Automount::default(),
            "a root outside [automount] is ignored"
        );
        assert_eq!(
            automount("[automount]\nroot = /windir/\n"),
            Automount {
                enabled: true,
                root: "/windir/".into()
            }
        );
        assert_eq!(
            automount("[automount]\nroot = /windir\nenabled = false\n"),
            Automount {
                enabled: false,
                root: "/windir/".into()
            }
        );
        assert_eq!(
            automount("[AUTOMOUNT]\nROOT = /x/\nENABLED = TRUE\n"),
            Automount {
                enabled: true,
                root: "/x/".into()
            },
            "section and key names are case-insensitive"
        );
        assert_eq!(
            automount("[automount]\n# root = /comment/\nroot = /a/\n; root = /b/\nroot = /c/\n"),
            Automount {
                enabled: true,
                root: "/c/".into()
            },
            "the last root wins and comments are skipped"
        );
        for quoted in [
            "[automount]\nroot = \"/windir/\"\n",
            "[automount]\nroot = '/windir/'\n",
        ] {
            assert_eq!(
                automount(quoted),
                Automount {
                    enabled: true,
                    root: "/windir/".into()
                },
                "{quoted:?}"
            );
        }
        assert_eq!(
            automount("[automount]\nroot = /windir/ # a comment\n"),
            Automount {
                enabled: true,
                root: "/windir/".into()
            },
            "an inline comment is dropped"
        );
        assert_eq!(
            automount("[automount]\nroot = \"/windir/ # not a comment\"\n"),
            Automount {
                enabled: true,
                root: "/windir/ # not a comment/".into()
            },
            "a # inside quotes is part of the root"
        );
        for invalid in ["relative/", r"\\mnt\\", "mnt"] {
            assert_eq!(
                automount(&format!("[automount]\nroot = {invalid}\n")),
                Automount::default(),
                "{invalid:?}"
            );
        }
    }

    #[test]
    fn windows_drives_become_paths_under_the_automount_root() {
        let map = |conf: Option<&str>, path: &str| {
            linux_directory(&automount(conf.unwrap_or_default()), Path::new(path))
        };
        assert_eq!(
            map(None, r"C:\Users\dax\captures\a.scrawl").as_deref(),
            Some("/mnt/c/Users/dax/captures/a.scrawl")
        );
        assert_eq!(
            map(None, r"\\?\C:\Users\dax\captures\a.scrawl").as_deref(),
            Some("/mnt/c/Users/dax/captures/a.scrawl"),
            "the verbatim prefix canonicalize adds is dropped"
        );
        assert_eq!(
            map(None, "D:/captures/a.scrawl").as_deref(),
            Some("/mnt/d/captures/a.scrawl"),
            "forward slashes map too"
        );
        assert_eq!(map(None, r"C:\").as_deref(), Some("/mnt/c"));
        assert_eq!(map(None, "C:").as_deref(), Some("/mnt/c"));
        let custom = Some("[automount]\nroot = /windir/\n");
        assert_eq!(
            map(custom, r"C:\Users\dax\a.scrawl").as_deref(),
            Some("/windir/c/Users/dax/a.scrawl")
        );
        assert_eq!(
            map(Some("[automount]\nenabled = false\n"), r"C:\a").as_deref(),
            None,
            "no drives are mounted"
        );
        for path in [
            r"\\server\share\a",
            r"\\?\UNC\server\share\a",
            r"relative\a",
            "C:relative",
            "",
        ] {
            assert_eq!(map(None, path), None, "{path:?}");
        }
    }

    #[test]
    fn a_capture_folder_follows_the_automount_read_with_the_home() {
        let root = std::env::temp_dir().join(format!(
            "hex-wsl-directory-{}-{:?}",
            std::process::id(),
            std::thread::current().id()
        ));
        let _ = fs::remove_dir_all(&root);
        let etc = root.join("etc");
        fs::create_dir_all(&etc).unwrap();
        let conf = etc.join("wsl.conf");
        fs::write(&conf, "[automount]\nroot = \"/windir/\" # a comment\n").unwrap();

        let home =
            WslHome::home(WSL_PREFIXES[0], "Ubuntu", "/home/dax", automount_at(&etc)).unwrap();
        assert_eq!(
            home.directory(Path::new(r"C:\Users\dax\a.scrawl"))
                .as_deref(),
            Some("/windir/c/Users/dax/a.scrawl")
        );

        // The home keeps what its lookup read, so naming a capture folder is
        // pure and never opens wsl.conf over UNC again.
        fs::write(&conf, "[automount]\nenabled = false\n").unwrap();
        assert_eq!(
            home.directory(Path::new(r"C:\Users\dax\a.scrawl"))
                .as_deref(),
            Some("/windir/c/Users/dax/a.scrawl"),
            "the cached automount is used"
        );
        fs::remove_file(&conf).unwrap();
        let default =
            WslHome::home(WSL_PREFIXES[0], "Ubuntu", "/home/dax", automount_at(&etc)).unwrap();
        assert_eq!(
            default
                .directory(Path::new(r"C:\Users\dax\a.scrawl"))
                .as_deref(),
            Some("/mnt/c/Users/dax/a.scrawl"),
            "a missing wsl.conf keeps the default"
        );
        fs::remove_dir_all(root).unwrap();
    }
}
