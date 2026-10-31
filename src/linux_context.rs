//! The focused application and web page on Linux, for dictation modes.
//! X11 reports the active window's process and title. On Wayland the
//! compositor's foreign-toplevel list (wlroots compositors, niri,
//! Hyprland) reports the activated window's app ID and title; desktops
//! without it (GNOME, KDE) report no focused window, and modes fall back to
//! the global settings. AT-SPI then finds that window's page address.

use std::collections::HashMap;

use wayland_client::backend::ObjectId;
use wayland_client::globals::{GlobalListContents, registry_queue_init};
use wayland_client::protocol::wl_registry::{self, WlRegistry};
use wayland_client::{Connection, Dispatch, Proxy, QueueHandle, event_created_child};
use wayland_protocols_wlr::foreign_toplevel::v1::client::zwlr_foreign_toplevel_handle_v1::{
    self, ZwlrForeignToplevelHandleV1,
};
use wayland_protocols_wlr::foreign_toplevel::v1::client::zwlr_foreign_toplevel_manager_v1::{
    self, EVT_TOPLEVEL_OPCODE, ZwlrForeignToplevelManagerV1,
};
use x11rb::connection::Connection as _;
use x11rb::protocol::xproto::{AtomEnum, ConnectionExt, Window};

use crate::context::ContextSnapshot;
use crate::linux_atspi::WindowHint;

/// The focused window as the display server reports it.
#[derive(Debug, Default)]
struct FocusedWindow {
    pid: Option<u32>,
    app_id: Option<String>,
    title: Option<String>,
}

pub(crate) fn capture() -> ContextSnapshot {
    let window = if crate::linux_session::LinuxSession::detect().is_wayland() {
        wayland_focused_window()
    } else {
        x11_focused_window()
    };
    let Some(window) = window else {
        return ContextSnapshot::default();
    };
    let accessible = crate::linux_atspi::focused_window(&WindowHint {
        pid: window.pid,
        title: window.title.clone(),
    });
    let pid = window
        .pid
        .or_else(|| accessible.as_ref().and_then(|accessible| accessible.pid));
    let application = window
        .app_id
        .as_deref()
        .and_then(crate::application_catalog::desktop_application_name)
        .or_else(|| pid.and_then(process_name))
        .or_else(|| window.app_id.as_deref().map(str::to_lowercase));
    ContextSnapshot {
        application,
        browser_url: accessible.and_then(|accessible| accessible.url),
        window_title: window.title,
        input_revision: None,
    }
}

fn x11_focused_window() -> Option<FocusedWindow> {
    let (connection, screen) = x11rb::connect(None).ok()?;
    let root = connection.setup().roots.get(screen)?.root;
    let active = intern(&connection, b"_NET_ACTIVE_WINDOW")?;
    let pid = intern(&connection, b"_NET_WM_PID")?;
    let name = intern(&connection, b"_NET_WM_NAME")?;
    let utf8 = intern(&connection, b"UTF8_STRING")?;
    let window = property(&connection, root, active, AtomEnum::WINDOW.into())?;
    if window == 0 {
        return None;
    }
    Some(FocusedWindow {
        pid: property(&connection, window, pid, AtomEnum::CARDINAL.into()),
        app_id: None,
        title: text_property(&connection, window, name, utf8),
    })
}

fn intern(connection: &impl x11rb::connection::Connection, name: &[u8]) -> Option<u32> {
    Some(connection.intern_atom(true, name).ok()?.reply().ok()?.atom)
}

fn property(
    connection: &impl x11rb::connection::Connection,
    window: Window,
    property: u32,
    kind: u32,
) -> Option<u32> {
    connection
        .get_property(false, window, property, kind, 0, 1)
        .ok()?
        .reply()
        .ok()?
        .value32()?
        .next()
}

fn text_property(
    connection: &impl x11rb::connection::Connection,
    window: Window,
    property: u32,
    kind: u32,
) -> Option<String> {
    let reply = connection
        .get_property(false, window, property, kind, 0, 1024)
        .ok()?
        .reply()
        .ok()?;
    String::from_utf8(reply.value)
        .ok()
        .filter(|title| !title.is_empty())
}

#[derive(Default)]
struct Toplevel {
    app_id: Option<String>,
    title: Option<String>,
    activated: bool,
}

#[derive(Default)]
struct Toplevels(HashMap<ObjectId, Toplevel>);

impl Dispatch<WlRegistry, GlobalListContents> for Toplevels {
    fn event(
        _: &mut Self,
        _: &WlRegistry,
        _: wl_registry::Event,
        _: &GlobalListContents,
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
    }
}

impl Dispatch<ZwlrForeignToplevelManagerV1, ()> for Toplevels {
    fn event(
        _: &mut Self,
        _: &ZwlrForeignToplevelManagerV1,
        _: zwlr_foreign_toplevel_manager_v1::Event,
        _: &(),
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
    }

    event_created_child!(Toplevels, ZwlrForeignToplevelManagerV1, [
        EVT_TOPLEVEL_OPCODE => (ZwlrForeignToplevelHandleV1, ()),
    ]);
}

impl Dispatch<ZwlrForeignToplevelHandleV1, ()> for Toplevels {
    fn event(
        toplevels: &mut Self,
        handle: &ZwlrForeignToplevelHandleV1,
        event: zwlr_foreign_toplevel_handle_v1::Event,
        _: &(),
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
        let toplevel = toplevels.0.entry(handle.id()).or_default();
        match event {
            zwlr_foreign_toplevel_handle_v1::Event::AppId { app_id } => {
                toplevel.app_id = Some(app_id);
            }
            zwlr_foreign_toplevel_handle_v1::Event::Title { title } => {
                toplevel.title = Some(title);
            }
            zwlr_foreign_toplevel_handle_v1::Event::State { state } => {
                toplevel.activated = state
                    .as_chunks::<4>()
                    .0
                    .iter()
                    .map(|bytes| u32::from_ne_bytes(*bytes))
                    .any(|state| state == zwlr_foreign_toplevel_handle_v1::State::Activated as u32);
            }
            zwlr_foreign_toplevel_handle_v1::Event::Closed => {
                toplevels.0.remove(&handle.id());
                handle.destroy();
            }
            _ => {}
        }
    }
}

/// The activated toplevel from `zwlr_foreign_toplevel_manager_v1`, or
/// `None` when the compositor doesn't offer it.
fn wayland_focused_window() -> Option<FocusedWindow> {
    let connection = Connection::connect_to_env().ok()?;
    let (globals, mut queue) = registry_queue_init::<Toplevels>(&connection).ok()?;
    let manager: ZwlrForeignToplevelManagerV1 = globals.bind(&queue.handle(), 1..=3, ()).ok()?;
    let mut toplevels = Toplevels::default();
    // The first roundtrip announces each toplevel, the second delivers
    // their app IDs, titles, and states.
    queue.roundtrip(&mut toplevels).ok()?;
    queue.roundtrip(&mut toplevels).ok()?;
    manager.stop();
    let focused = toplevels
        .0
        .into_values()
        .find(|toplevel| toplevel.activated)?;
    Some(FocusedWindow {
        pid: None,
        app_id: focused.app_id.filter(|app_id| !app_id.is_empty()),
        title: focused.title.filter(|title| !title.is_empty()),
    })
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

    /// Manual probe: `cargo test linux_context_reads_the_focused_window --
    /// --ignored --nocapture` prints what modes would see right now.
    #[test]
    #[ignore]
    fn linux_context_reads_the_focused_window() {
        let started = std::time::Instant::now();
        println!("{:?} in {:?}", super::capture(), started.elapsed());
    }
}
