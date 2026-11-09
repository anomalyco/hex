//! OpenCode in WSL2 (Windows): its registration lives in the default
//! distribution's home, which Windows reaches through `\\wsl.localhost\`
//! without running `wsl.exe`. WSL2 forwards loopback, so the registered URL
//! already answers from Windows; only the registration's files and the
//! workspace header need translating.

use std::fs;
use std::path::PathBuf;

#[cfg(target_os = "windows")]
use std::path::Path;

use color_eyre::eyre::{Result, WrapErr};

#[cfg(target_os = "windows")]
use super::{Missing, Service};

/// `/etc/passwd` is tiny; never read more than this.
#[cfg(target_os = "windows")]
const MAX_PASSWD_BYTES: u64 = 1024 * 1024;

/// The default distribution's home, as Windows reads it and as its server
/// sees it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) struct WslHome {
    /// `\\wsl.localhost\<distribution><home>`, the same folder Windows can
    /// read without starting the distribution itself.
    pub(super) unc: PathBuf,
    /// The home's Linux path, such as `/home/dax`.
    pub(super) linux: String,
}

impl WslHome {
    /// The home `linux_home` names in `distribution`; both are checked
    /// before they become a UNC path.
    #[cfg_attr(not(target_os = "windows"), allow(dead_code))]
    pub(super) fn home(distribution: &str, linux_home: &str) -> Option<Self> {
        let distribution = distribution_component(distribution)?;
        let linux = linux_path(linux_home)?;
        Some(Self {
            unc: PathBuf::from(format!(
                r"\\wsl.localhost\{distribution}{}",
                linux.replace('/', r"\")
            )),
            linux,
        })
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

    /// Creates the workspace in the distribution and returns the path its
    /// server sees, so a request's `x-opencode-directory` names a Linux path
    /// the server can use instead of a Windows one it reads as relative.
    pub(super) fn workspace(&self) -> Result<String> {
        let directory = self.unc_workspace();
        fs::create_dir_all(&directory).wrap_err_with(|| {
            format!(
                "could not create OpenCode workspace {}",
                directory.display()
            )
        })?;
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

/// An absolute Linux home Windows can map into a distribution's UNC path:
/// no `.` or `..`, no separators, and no characters Windows refuses.
fn linux_path(value: &str) -> Option<String> {
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
    let passwd = read_bounded(&passwd_path(&name)?)?;
    WslHome::home(&name, &home_for_uid(&passwd, uid)?)
}

/// `/etc/passwd` in a distribution, through Windows.
#[cfg(target_os = "windows")]
fn passwd_path(distribution: &str) -> Option<PathBuf> {
    let distribution = distribution_component(distribution)?;
    Some(PathBuf::from(format!(
        r"\\wsl.localhost\{distribution}\etc\passwd"
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

/// A service whose registration lives in the default WSL distribution, or
/// why there is none.
#[cfg(target_os = "windows")]
pub(super) fn discover(
    budget: Option<std::time::Duration>,
    cancelled: &std::sync::atomic::AtomicBool,
) -> Result<Service, Missing> {
    use std::sync::atomic::Ordering;
    use std::time::Instant;

    if cancelled.load(Ordering::Acquire) {
        return Err(Missing::NotRunning);
    }
    // Finding the distribution and reading its passwd is Windows file I/O;
    // the caller's remaining budget covers the registrations after it.
    let started = Instant::now();
    let home = default_home();
    if cancelled.load(Ordering::Acquire) {
        return Err(Missing::NotRunning);
    }
    let Some(home) = home else {
        tracing::debug!("no OpenCode registration was found in the default WSL distribution");
        return Err(Missing::NotRunning);
    };
    let budget = budget.map(|budget| budget.saturating_sub(started.elapsed()));
    if budget.is_some_and(|budget| budget.is_zero()) {
        return Err(Missing::NotRunning);
    }
    super::discover(&home.state_dir(), budget, cancelled, Some(&home))
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
        for line in [
            "dax:x:1000",
            "dax:x:not-a-uid:1000:Dax:/home/dax:/bin/zsh",
            "dax:x:1000:1000:Dax",
            "dax:x:1000:1000:Dax:/home/../root:/bin/zsh",
            "dax:x:1000:1000:Dax:relative:/bin/zsh",
            "dax:x:1000:1000:Dax:/home/dax/:/bin/zsh",
        ] {
            assert_eq!(home_for_uid(line, 1000), None, "{line}");
        }
    }

    #[test]
    fn a_linux_home_maps_into_the_distributions_unc_path() {
        let home = WslHome::home("Ubuntu-24.04", "/home/dax").unwrap();
        assert_eq!(
            home.unc,
            PathBuf::from(r"\\wsl.localhost\Ubuntu-24.04\home\dax")
        );
        assert_eq!(home.linux, "/home/dax");
        assert_eq!(
            home.linux_workspace(),
            "/home/dax/.local/share/hex/opencode"
        );
    }

    #[test]
    fn a_distribution_or_home_windows_cant_reach_is_refused() {
        assert!(WslHome::home("Ubuntu", "/home/dax").is_some());
        assert!(WslHome::home("Ubuntu", "/root").is_some());
        for (distribution, home) in [
            ("", "/home/dax"),
            ("..", "/home/dax"),
            ("Ubu/ntu", "/home/dax"),
            ("Ubu\\ntu", "/home/dax"),
            ("Ubuntu", ""),
            ("Ubuntu", "/"),
            ("Ubuntu", "home/dax"),
            ("Ubuntu", "/home/../root"),
            ("Ubuntu", "/home/./dax"),
            ("Ubuntu", "/home//dax"),
            ("Ubuntu", "/home/dax/"),
            ("Ubuntu", "/home\\dax"),
            ("Ubuntu", "/home/da:x"),
            ("Ubuntu", "/home/dax "),
        ] {
            assert_eq!(
                WslHome::home(distribution, home),
                None,
                "{distribution:?} {home:?}"
            );
        }
    }

    #[test]
    fn the_workspace_is_created_somewhere_windows_can_write_it() {
        let root = std::env::temp_dir().join(format!("hex-wsl-workspace-{}", std::process::id()));
        let _ = fs::remove_dir_all(&root);
        let home = WslHome {
            unc: root.join("home"),
            linux: "/home/dax".into(),
        };
        assert_eq!(home.state_dir(), root.join("home/.local/state/opencode"));
        assert_eq!(
            home.workspace().unwrap(),
            "/home/dax/.local/share/hex/opencode"
        );
        assert!(root.join("home/.local/share/hex/opencode").is_dir());
        fs::remove_dir_all(root).unwrap();
    }
}
