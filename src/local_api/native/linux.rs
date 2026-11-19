//! Linux peer identity, from what the kernel reports for the socket peer.
//!
//! `SO_PEERCRED` names the connecting process; its uid must be Hex's. The
//! process is pinned while Hex reads it: by the pidfd `SO_PEERPIDFD` hands
//! out (Linux 6.5+), which stays bound to the process that connected, and by
//! its start time in `/proc/<pid>/stat`, read before and after. Any change,
//! or a peer that has gone, refuses the connection.
//!
//! The identity is, in order:
//!
//! - a Flatpak app: `[Application] name` in `/proc/<pid>/root/.flatpak-info`,
//!   the file Flatpak puts at the root of every sandbox;
//! - a Snap app: its enforced AppArmor label `snap.<snap>.<app>`, which the
//!   kernel reports in `/proc/<pid>/attr/apparmor/current` (or
//!   `attr/current`);
//! - otherwise the executable `/proc/<pid>/exe` links to, absolute and not
//!   deleted.
//!
//! The sandbox app id or the executable path is the grant key. Nothing the
//! client sends (argv, environment, window titles, a claimed name) is read.
//! An interpreter (`node`, `python3`, an Electron development runtime) is
//! shown as itself, so a grant to it covers every script it runs.

use std::fs;
use std::io::Read;
use std::os::fd::{AsRawFd, FromRawFd, OwnedFd, RawFd};
use std::os::unix::fs::{MetadataExt, OpenOptionsExt, PermissionsExt};
use std::path::{Path, PathBuf};

use super::display;
use super::{Identity, PeerId};
use crate::local_api::consent::{SandboxKind, Verification};

mod desktop;

/// `SO_PEERPIDFD` (`<asm-generic/socket.h>`, Linux 6.5): a pidfd for the
/// peer. Older kernels answer `ENOPROTOOPT`.
const SO_PEERPIDFD: libc::c_int = 77;
/// The most Hex reads of one `/proc` or sandbox metadata file.
const SMALL_FILE: u64 = 64 * 1024;

/// Makes `path`'s folder Hex's private data folder (user-owned, not a
/// symlink, 0700) and clears a stale socket, refusing anything else at the
/// path, as `service.sock` does.
pub(super) fn prepare(path: &Path) -> std::io::Result<()> {
    let refuse = |what: &str| std::io::Error::new(std::io::ErrorKind::PermissionDenied, what);
    // SAFETY: geteuid has no preconditions.
    let user = unsafe { libc::geteuid() };
    let parent = path.parent().ok_or_else(|| refuse("no data folder"))?;
    fs::create_dir_all(parent)?;
    let folder = fs::symlink_metadata(parent)?;
    if !folder.is_dir() || folder.uid() != user {
        return Err(refuse(
            "Hex requires a user-owned, non-symlink data directory",
        ));
    }
    fs::set_permissions(parent, fs::Permissions::from_mode(0o700))?;
    if let Ok(existing) = fs::symlink_metadata(path) {
        use std::os::unix::fs::FileTypeExt;
        if !existing.file_type().is_socket() || existing.uid() != user {
            return Err(refuse("Refusing to replace an unmanaged Hex socket path"));
        }
        // The local API's instance lock already makes this the one Hex; a
        // socket left by a crashed run is stale.
        fs::remove_file(path)?;
    }
    Ok(())
}

/// The peer behind `fd` and what the kernel shows of it; `None` refuses.
pub(super) fn identify(fd: RawFd) -> Option<(PeerId, Identity)> {
    let credentials = peer_credentials(fd)?;
    // SAFETY: geteuid has no preconditions.
    let user = unsafe { libc::geteuid() };
    identify_peer(credentials, user, peer_pidfd(fd), Path::new("/proc"))
}

fn identify_peer(
    credentials: libc::ucred,
    user: libc::uid_t,
    pidfd: Option<OwnedFd>,
    proc: &Path,
) -> Option<(PeerId, Identity)> {
    if credentials.uid != user {
        tracing::debug!(
            uid = credentials.uid,
            "a native client of another user was refused"
        );
        return None;
    }
    let pid = u32::try_from(credentials.pid).ok().filter(|pid| *pid > 0)?;
    if let Some(pidfd) = &pidfd
        && pidfd_pid(pidfd.as_raw_fd()) != Some(pid)
    {
        return None;
    }
    let process = proc.join(pid.to_string());
    let (start, (identifier, verification)) = pinned(
        || start_time(&process),
        || pidfd.as_ref().is_none_or(|pidfd| alive(pidfd.as_raw_fd())),
        || verified(&process),
    )?;
    let peer = PeerId([
        pid,
        start as u32,
        (start >> 32) as u32,
        credentials.uid,
        0,
        0,
        0,
        0,
    ]);
    let key = format!("{verification:?}");
    let shown = display::shown(key, {
        let verification = verification.clone();
        move || desktop::shown(&verification)
    });
    tracing::debug!(
        %identifier,
        ?verification,
        name = ?shown.name,
        icon = ?shown.icon.as_ref().map(|icon| icon.size()),
        "identified a native client"
    );
    Some((
        peer,
        Identity {
            identifier,
            cdhash: String::new(),
            verification,
            name: shown.name,
            icon: shown.icon,
        },
    ))
}

/// `read` while the process stays the one that connected: its start time
/// the same before and after, and (with a pidfd) still alive, so its pid was
/// never reused.
fn pinned<T>(
    start: impl Fn() -> Option<u64>,
    alive: impl Fn() -> bool,
    read: impl FnOnce() -> Option<T>,
) -> Option<(u64, T)> {
    let before = start()?;
    let value = read()?;
    let after = start()?;
    (before == after && alive()).then_some((before, value))
}

fn peer_credentials(fd: RawFd) -> Option<libc::ucred> {
    // SAFETY: an all-zero `ucred` is a valid value.
    let mut credentials: libc::ucred = unsafe { std::mem::zeroed() };
    let mut length = std::mem::size_of_val(&credentials) as libc::socklen_t;
    // SAFETY: `fd` is the accepted socket, and `credentials`/`length`
    // describe a live buffer and its size.
    let result = unsafe {
        libc::getsockopt(
            fd,
            libc::SOL_SOCKET,
            libc::SO_PEERCRED,
            (&raw mut credentials).cast(),
            &mut length,
        )
    };
    (result == 0 && length as usize == std::mem::size_of::<libc::ucred>()).then_some(credentials)
}

fn peer_pidfd(fd: RawFd) -> Option<OwnedFd> {
    let mut pidfd: libc::c_int = -1;
    let mut length = std::mem::size_of::<libc::c_int>() as libc::socklen_t;
    // SAFETY: as in `peer_credentials`, for one `c_int`.
    let result = unsafe {
        libc::getsockopt(
            fd,
            libc::SOL_SOCKET,
            SO_PEERPIDFD,
            (&raw mut pidfd).cast(),
            &mut length,
        )
    };
    // SAFETY: a successful SO_PEERPIDFD returns a new descriptor this call
    // owns.
    (result == 0 && pidfd >= 0).then(|| unsafe { OwnedFd::from_raw_fd(pidfd) })
}

/// The pid a pidfd refers to, from its `fdinfo`.
fn pidfd_pid(pidfd: RawFd) -> Option<u32> {
    let info = fs::read_to_string(format!("/proc/self/fdinfo/{pidfd}")).ok()?;
    info.lines()
        .find_map(|line| line.strip_prefix("Pid:"))
        .and_then(|pid| pid.trim().parse().ok())
}

/// Whether the pidfd's process still runs (signal 0 checks, sends nothing).
fn alive(pidfd: RawFd) -> bool {
    // SAFETY: pidfd_send_signal with signal 0 and no info only checks.
    let result = unsafe {
        libc::syscall(
            libc::SYS_pidfd_send_signal,
            pidfd,
            0,
            std::ptr::null::<libc::siginfo_t>(),
            0,
        )
    };
    result == 0
}

/// The process's start time in clock ticks since boot (`stat` field 22).
fn start_time(process: &Path) -> Option<u64> {
    parse_start_time(&read_small(&process.join("stat")).ok()?)
}

fn parse_start_time(stat: &str) -> Option<u64> {
    // The command name (field 2) may hold spaces and parentheses; the fields
    // after its last `)` start at field 3.
    let rest = &stat[stat.rfind(')')? + 1..];
    rest.split_whitespace().nth(19)?.parse().ok()
}

/// The grant key and how the kernel vouches for it.
fn verified(process: &Path) -> Option<(String, Verification)> {
    match read_small(&process.join("root/.flatpak-info")) {
        Ok(info) => {
            // A sandbox Hex cannot name is refused, never shown as the
            // executable inside it.
            let app_id = flatpak_app_id(&info)?;
            return Some((
                app_id.clone(),
                Verification::Sandbox {
                    kind: SandboxKind::Flatpak,
                    app_id,
                },
            ));
        }
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
        Err(error) => {
            tracing::debug!(%error, "a native client's root could not be read");
            return None;
        }
    }
    let label = read_small(&process.join("attr/apparmor/current"))
        .or_else(|_| read_small(&process.join("attr/current")));
    if let Some(app_id) = label.ok().as_deref().and_then(snap_app) {
        return Some((
            app_id.clone(),
            Verification::Sandbox {
                kind: SandboxKind::Snap,
                app_id,
            },
        ));
    }
    let path = executable(&fs::read_link(process.join("exe")).ok()?)?;
    Some((
        path.to_string_lossy().into_owned(),
        Verification::Executable { path },
    ))
}

/// A regular file of at most [`SMALL_FILE`] bytes, never blocking on a FIFO
/// a sandbox put in its place.
fn read_small(path: &Path) -> std::io::Result<String> {
    let file = fs::OpenOptions::new()
        .read(true)
        .custom_flags(libc::O_NONBLOCK | libc::O_NOCTTY)
        .open(path)?;
    let metadata = file.metadata()?;
    if !metadata.is_file() {
        return Err(std::io::Error::other("not a regular file"));
    }
    let mut text = String::new();
    file.take(SMALL_FILE).read_to_string(&mut text)?;
    Ok(text)
}

/// `[Application] name` of a `.flatpak-info`, when it is a valid app id.
fn flatpak_app_id(info: &str) -> Option<String> {
    let mut group = "";
    for line in info.lines().map(str::trim) {
        if line.starts_with('[') {
            group = line;
            continue;
        }
        if group != "[Application]" {
            continue;
        }
        if let Some((key, value)) = line.split_once('=')
            && key.trim() == "name"
        {
            let id = value.trim();
            let valid = (3..=255).contains(&id.len())
                && id.contains('.')
                && id
                    .bytes()
                    .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'.' | b'_' | b'-'))
                && !id.starts_with('.')
                && !id.ends_with('.');
            return valid.then(|| id.to_owned());
        }
    }
    None
}

/// `<snap>.<app>` of an enforced snap app's AppArmor label
/// (`snap.<snap>.<app> (enforce)`). Hooks, other profiles, and
/// complain-mode (devmode) snaps, which are not confined, are not apps.
fn snap_app(label: &str) -> Option<String> {
    let label = label.trim_end_matches(['\0', '\n']).trim();
    let (profile, mode) = label.split_once(" (")?;
    if mode != "enforce)" {
        return None;
    }
    let mut parts = profile.strip_prefix("snap.")?.split('.');
    let (snap, app) = (parts.next()?, parts.next()?);
    if parts.next().is_some() {
        return None;
    }
    let snap_ok = !snap.is_empty()
        && snap
            .bytes()
            .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || matches!(b, b'-' | b'_'));
    let app_ok = !app.is_empty() && app.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'-');
    (snap_ok && app_ok).then(|| format!("{snap}.{app}"))
}

/// The executable `/proc/<pid>/exe` named, unless it is relative or was
/// deleted (the kernel then appends ` (deleted)`).
fn executable(link: &Path) -> Option<PathBuf> {
    let text = link.to_str()?;
    (link.is_absolute() && !text.ends_with(" (deleted)")).then(|| link.to_path_buf())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::os::unix::net::UnixStream;

    #[test]
    fn a_flatpak_is_named_by_its_application_id() {
        let info = "[Application]\nname=org.example.Notes\nruntime=runtime/org.gnome.Platform/x86_64/47\n\n[Instance]\ninstance-id=1\n";
        assert_eq!(flatpak_app_id(info).as_deref(), Some("org.example.Notes"));
        // Only the Application group names the app.
        assert_eq!(flatpak_app_id("[Runtime]\nname=org.gnome.Platform\n"), None);
        assert_eq!(flatpak_app_id("[Application]\nname=../../etc\n"), None);
        assert_eq!(flatpak_app_id("[Application]\nname=notes\n"), None);
    }

    #[test]
    fn a_snap_is_named_by_its_enforced_app_label() {
        assert_eq!(
            snap_app("snap.firefox.firefox (enforce)\n").as_deref(),
            Some("firefox.firefox")
        );
        assert_eq!(
            snap_app("snap.notes_beta.notes-cli (enforce)").as_deref(),
            Some("notes_beta.notes-cli")
        );
        assert_eq!(snap_app("snap.firefox.firefox (complain)"), None);
        assert_eq!(snap_app("snap.firefox.hook.configure (enforce)"), None);
        assert_eq!(snap_app("unconfined"), None);
        assert_eq!(snap_app("/usr/bin/evince (enforce)"), None);
    }

    #[test]
    fn a_deleted_or_relative_executable_is_refused() {
        assert_eq!(
            executable(Path::new("/usr/bin/node")),
            Some(PathBuf::from("/usr/bin/node"))
        );
        assert_eq!(executable(Path::new("/tmp/app (deleted)")), None);
        assert_eq!(executable(Path::new("app")), None);
    }

    #[test]
    fn the_start_time_follows_a_command_name_with_spaces() {
        let stat = "4242 (my (odd) app) S 1 4242 4242 0 -1 4194560 100 0 0 0 1 2 0 0 20 0 1 0 987654 1000 10";
        assert_eq!(parse_start_time(stat), Some(987654));
    }

    #[test]
    fn a_process_that_changed_or_went_while_read_is_refused() {
        let ticks = std::cell::Cell::new(10);
        let restarted = pinned(
            || {
                ticks.set(ticks.get() + 1);
                Some(ticks.get())
            },
            || true,
            || Some("read"),
        );
        assert_eq!(restarted, None);
        assert_eq!(pinned(|| Some(7), || false, || Some("read")), None);
        assert_eq!(
            pinned(|| Some(7), || true, || Some("read")),
            Some((7, "read"))
        );
    }

    #[test]
    fn another_users_peer_is_refused() {
        let (ours, _theirs) = UnixStream::pair().unwrap();
        let credentials = peer_credentials(ours.as_raw_fd()).unwrap();
        let someone_else = credentials.uid + 1;
        assert!(identify_peer(credentials, someone_else, None, Path::new("/proc")).is_none());
    }

    /// The peer of a socket pair is this test binary: its executable is the
    /// identity and its pid the peer.
    #[test]
    fn a_local_peer_is_named_by_its_executable() {
        let (ours, _theirs) = UnixStream::pair().unwrap();
        let (peer, identity) = identify(ours.as_raw_fd()).unwrap();
        let exe = fs::read_link("/proc/self/exe").unwrap();
        assert_eq!(peer.0[0], std::process::id());
        assert_eq!(identity.identifier, exe.to_string_lossy());
        assert_eq!(
            identity.verification,
            Verification::Executable { path: exe }
        );
        // The same process twice is the same peer.
        let (again, _) = identify(ours.as_raw_fd()).unwrap();
        assert_eq!(again, peer);
    }
}
