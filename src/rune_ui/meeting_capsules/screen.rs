//! The screen a capsule's drag is measured on.
//!
//! Rune places an overlay's window on the primary display's edge, and a
//! release lands in the corner nearest the window's centre (the throw in
//! `throw`), so the size wanted here is that display's in points:
//! [`rune::Drag::dx`] and a capsule's window size are both in points, device
//! pixels over the window's scale. That scale is Rune's own, read from the
//! view's [`rune::Cx::env`], so a capsule measures the display the way Rune
//! laid it out.

/// The primary screen in points, where Rune places the capsules. macOS
/// already answers in points.
#[cfg(target_os = "macos")]
pub(super) fn primary_screen_size(_scale: f32) -> Option<(f32, f32)> {
    let mtm = objc2::MainThreadMarker::new()?;
    let screen = objc2_app_kit::NSScreen::screens(mtm).iter().next()?;
    let size = screen.frame().size;
    Some((size.width as f32, size.height as f32))
}

/// Windows places an overlay's `x` on the whole primary monitor and its top
/// or bottom edge inside that monitor's work area, so the height is the work
/// area's.
#[cfg(target_os = "windows")]
pub(super) fn primary_screen_size(scale: f32) -> Option<(f32, f32)> {
    let monitor = scrawl_capture::win::primary_monitor()?;
    Some((
        points(monitor.rect[2] as f32, scale),
        points(monitor.work[3] as f32, scale),
    ))
}

/// X11's primary RandR monitor in points, which is what Rune places an X11 or
/// XWayland overlay against. A capsule on a native Wayland surface has no
/// RandR behind it, so that is not supported on Wayland yet and its drag
/// returns to the corner it started in.
#[cfg(target_os = "linux")]
pub(super) fn primary_screen_size(scale: f32) -> Option<(f32, f32)> {
    use x11rb::connection::Connection as _;
    use x11rb::protocol::randr::ConnectionExt as _;

    // A capsule floating on XWayland is an X11 window placed by RandR, even
    // though the session is Wayland.
    if crate::linux::session::LinuxSession::detect().is_wayland()
        && !crate::linux::meeting_capsules::on_xwayland()
    {
        return None;
    }
    let (connection, screen) = x11rb::connect(None).ok()?;
    let root = &connection.setup().roots[screen];
    // RandR 1.5 names the primary monitor; without it the root window spans
    // every display. Only the size decides a corner, not the origin.
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
    Some((
        points(f32::from(width), scale),
        points(f32::from(height), scale),
    ))
}

/// Device pixels to points. A scale Rune hasn't set yet (0) counts as 1.
#[cfg_attr(not(any(target_os = "windows", target_os = "linux")), allow(dead_code))]
fn points(pixels: f32, scale: f32) -> f32 {
    pixels / if scale > 0.0 { scale } else { 1.0 }
}

#[cfg(test)]
mod tests {
    use super::points;

    #[test]
    fn device_pixels_become_points() {
        assert_eq!(points(2880.0, 2.0), 1440.0);
        assert_eq!(points(1920.0, 1.0), 1920.0);
        // Before Rune sets the view's scale, points are pixels.
        assert_eq!(points(1920.0, 0.0), 1920.0);
    }
}
