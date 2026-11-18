//! The screen a capsule's drag is measured on.
//!
//! Rune places an overlay's window on the primary display's edge, and a
//! release lands in the corner nearest the window's centre (the throw in
//! `throw`), so the size wanted here is that display's, in the points
//! `rune::Drag::dx` and the capsule's window size are in: device pixels over
//! the window's scale.

/// The primary screen in points, where Rune places the capsules.
#[cfg(target_os = "macos")]
pub(super) fn primary_screen_size() -> Option<(f32, f32)> {
    let mtm = objc2::MainThreadMarker::new()?;
    let screen = objc2_app_kit::NSScreen::screens(mtm).iter().next()?;
    let size = screen.frame().size;
    Some((size.width as f32, size.height as f32))
}

/// Windows lays an overlay out in its primary display's points, so the
/// monitor's pixels come back over its DPI scale.
#[cfg(target_os = "windows")]
pub(super) fn primary_screen_size() -> Option<(f32, f32)> {
    let monitor = scrawl_capture::win::primary_monitor()?;
    let scale = if monitor.scale.is_finite() && monitor.scale > 0.0 {
        monitor.scale
    } else {
        1.0
    };
    Some((
        (monitor.rect[2] / scale) as f32,
        (monitor.rect[3] / scale) as f32,
    ))
}

/// X11's primary RandR monitor in points (the root window's size without
/// RandR). Wayland tells an app nothing of its outputs, so a drag there still
/// returns to the corner it started in.
#[cfg(target_os = "linux")]
pub(super) fn primary_screen_size() -> Option<(f32, f32)> {
    use x11rb::connection::Connection as _;
    use x11rb::protocol::randr::ConnectionExt as _;

    if crate::linux::session::LinuxSession::detect().is_wayland() {
        return None;
    }
    let (connection, screen) = x11rb::connect(None).ok()?;
    let root = &connection.setup().roots[screen];
    let scale = xft_scale(&connection, root.root);
    // RandR 1.5 names the primary monitor; without it the root window spans
    // every display.
    let monitors = connection
        .randr_get_monitors(root.root, true)
        .ok()
        .and_then(|cookie| cookie.reply().ok())
        .map(|reply| reply.monitors)
        .unwrap_or_default();
    let (width, height) = monitors
        .iter()
        .find(|monitor| monitor.primary)
        .or_else(|| monitors.first())
        .map_or((root.width_in_pixels, root.height_in_pixels), |monitor| {
            (monitor.width, monitor.height)
        });
    Some((f32::from(width) / scale, f32::from(height) / scale))
}

/// `Xft.dpi` from the root window's resource database as a scale, the way
/// Rune's own X11 overlay reads it; 1 when unset. Matches winit's X11 window
/// scale, which the pointer's points are divided by.
#[cfg(target_os = "linux")]
fn xft_scale(connection: &impl x11rb::connection::Connection, root: u32) -> f32 {
    use x11rb::protocol::xproto::{AtomEnum, ConnectionExt as _};

    let Some(property) = connection
        .get_property(
            false,
            root,
            AtomEnum::RESOURCE_MANAGER,
            AtomEnum::STRING,
            0,
            1 << 16,
        )
        .ok()
        .and_then(|cookie| cookie.reply().ok())
    else {
        return 1.0;
    };
    let database = String::from_utf8_lossy(&property.value);
    database
        .lines()
        .find_map(|line| line.strip_prefix("Xft.dpi:")?.trim().parse::<f32>().ok())
        .map_or(1.0, |dpi| (dpi / 96.0).clamp(1.0, 4.0))
}
