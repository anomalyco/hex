//! The frontmost application on Linux, for dictation modes. X11 reports the
//! active window's process. Wayland compositors don't expose the focused
//! window, so modes fall back to the global settings there.

use x11rb::connection::Connection;
use x11rb::protocol::xproto::{AtomEnum, ConnectionExt};

use crate::context::ContextSnapshot;

pub(crate) fn capture() -> ContextSnapshot {
    ContextSnapshot {
        application: frontmost_application(),
        ..ContextSnapshot::default()
    }
}

fn frontmost_application() -> Option<String> {
    if crate::linux_session::LinuxSession::detect().is_wayland() {
        return None;
    }
    let (connection, screen) = x11rb::connect(None).ok()?;
    let root = connection.setup().roots.get(screen)?.root;
    let active = intern(&connection, b"_NET_ACTIVE_WINDOW")?;
    let pid = intern(&connection, b"_NET_WM_PID")?;
    let window = property(&connection, root, active, AtomEnum::WINDOW)?;
    if window == 0 {
        return None;
    }
    process_name(property(&connection, window, pid, AtomEnum::CARDINAL)?)
}

fn intern(connection: &impl Connection, name: &[u8]) -> Option<u32> {
    Some(connection.intern_atom(true, name).ok()?.reply().ok()?.atom)
}

fn property(
    connection: &impl Connection,
    window: u32,
    property: u32,
    kind: AtomEnum,
) -> Option<u32> {
    connection
        .get_property(false, window, property, kind, 0, 1)
        .ok()?
        .reply()
        .ok()?
        .value32()?
        .next()
}

/// The process's executable name, lowercased, as the application catalog
/// names applications ("code", "firefox").
fn process_name(pid: u32) -> Option<String> {
    let executable = std::fs::read_link(format!("/proc/{pid}/exe")).ok();
    executable
        .as_deref()
        .and_then(|path| path.file_name())
        .and_then(|name| name.to_str())
        .map(str::to_owned)
        .or_else(|| std::fs::read_to_string(format!("/proc/{pid}/comm")).ok())
        .map(|name| name.trim().to_lowercase())
        .filter(|name| !name.is_empty())
}

#[cfg(test)]
mod tests {
    #[test]
    fn the_current_process_has_a_name() {
        assert!(super::process_name(std::process::id()).is_some());
    }
}
