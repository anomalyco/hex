//! The displays a capsule's drag is measured on.
//!
//! Rune places an overlay's window on the primary display's edge, unless the
//! overlay is pinned to a display with [`DisplayTarget::Native`]. A release
//! lands in the corner nearest the window's centre (the throw in `throw`), so
//! the size wanted here is that display's in points: [`rune::Drag::dx`] and a
//! capsule's window size are both in points, device pixels over the window's
//! scale. That scale is Rune's own, read from the view's [`rune::Cx::env`], so
//! a capsule measures the display the way Rune laid it out.
//!
//! By default every capsule stays on the primary display, which is all
//! [`primary_screen_size`] measures. With Labs › Indicator on any display on
//! (`crate::labs::meeting_indicator_display`), [`displays`] lists every
//! display with the pointer's first, [`release`] picks the corner of the
//! display a throw lands on, and [`target`] resolves the display saved in
//! settings for the next show. macOS and Windows have this; a native Wayland
//! capsule has no display to measure, so Linux keeps the primary display.

use rune::DisplayTarget;

use crate::app_settings::MeetingIndicatorCorner;

use super::throw;

/// Where a capsule rests: a corner of a display.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub(super) struct Anchor {
    pub(super) corner: MeetingIndicatorCorner,
    /// `meetings.recording_indicator_display`: the OS's own name for the
    /// display, or `None` for the primary display.
    pub(super) display: Option<String>,
}

/// One display, as a release and a placement measure it.
#[derive(Clone, Debug, PartialEq)]
pub(super) struct Display {
    /// Rune's target for it.
    pub(super) target: DisplayTarget,
    /// `meetings.recording_indicator_display`'s value: the OS's own stable
    /// name for the display.
    pub(super) id: String,
    /// Its whole frame in the native global space — `x`, `y`, width and
    /// height, desktop pixels on Windows and points on macOS — which is where
    /// the pointer is compared.
    #[cfg_attr(not(any(target_os = "macos", target_os = "windows")), allow(dead_code))]
    pub(super) frame: (f32, f32, f32, f32),
    /// The top-left of the point space Rune places a window in, in that same
    /// native space: the display's own x, and on Windows its work area's top.
    pub(super) origin: (f32, f32),
    /// That point space's size in points, which is what a corner is measured
    /// in.
    pub(super) size: (f32, f32),
    /// Native units per point in `origin`'s space. macOS has one point space
    /// over every display, so its scale is 1.
    pub(super) scale: f32,
    /// Whether the OS calls it the primary display.
    pub(super) primary: bool,
}

impl Display {
    /// Where `local` points into this display sit in `origin`'s native space.
    fn global(&self, local: (f32, f32)) -> (f32, f32) {
        (
            self.origin.0 + local.0 * self.scale,
            self.origin.1 + local.1 * self.scale,
        )
    }

    /// Whether `point` in the native space is on this display, by the same
    /// half-open rule Rune resolves `DisplayTarget::UnderPointer` with.
    #[cfg_attr(not(any(target_os = "macos", target_os = "windows")), allow(dead_code))]
    fn holds(&self, point: (f32, f32)) -> bool {
        let (x, y, width, height) = self.frame;
        point.0 >= x && point.1 >= y && point.0 < x + width && point.1 < y + height
    }

    /// The corner nearest `point`, in the native space, by the halves
    /// [`throw::nearest_corner`] compares within one display.
    fn nearest(&self, point: (f32, f32)) -> MeetingIndicatorCorner {
        let centre = self.global((self.size.0 / 2.0, self.size.1 / 2.0));
        match (point.1 < centre.1, point.0 >= centre.0) {
            (true, true) => MeetingIndicatorCorner::TopRight,
            (true, false) => MeetingIndicatorCorner::TopLeft,
            (false, true) => MeetingIndicatorCorner::BottomRight,
            (false, false) => MeetingIndicatorCorner::BottomLeft,
        }
    }
}

/// Where a released capsule lands while Labs › Indicator on any display is
/// on.
#[derive(Clone, Debug, PartialEq)]
pub(super) struct Landing {
    /// The display it lands on, which Rune is told to place the window on.
    pub(super) target: DisplayTarget,
    /// `meetings.recording_indicator_display`'s new value.
    pub(super) display: String,
    pub(super) corner: MeetingIndicatorCorner,
    /// The landing display's size in points, for a glide within it.
    pub(super) size: (f32, f32),
    /// It landed on another display than the one it was dragged on: the window
    /// jumps there rather than gliding across, since a glide is measured
    /// within one display.
    pub(super) elsewhere: bool,
}

/// Where a capsule window of `size` points, let go `offset` points from
/// `from`'s resting corner on the display `saved` names, lands: in the corner
/// of the display under the pointer (`displays`' first) nearest where its
/// centre would coast to rest.
///
/// `None` when there is no display to land on.
pub(super) fn release(
    displays: &[Display],
    saved: Option<&str>,
    from: MeetingIndicatorCorner,
    offset: (f32, f32),
    velocity: (f32, f32),
    size: (f32, f32),
) -> Option<Landing> {
    let landing = displays.first()?;
    // Where Rune placed the capsule: the saved display, or the primary one it
    // falls back to when that display is gone.
    let source = displays
        .iter()
        .find(|display| Some(display.id.as_str()) == saved)
        .or_else(|| displays.iter().find(|display| display.primary))
        .unwrap_or(landing);
    let (left, top) = throw::rest_origin(from, source.size, size);
    let (coast_x, coast_y) = throw::coast(velocity);
    let centre = source.global((
        left + offset.0 + size.0 / 2.0 + coast_x,
        top + offset.1 + size.1 / 2.0 + coast_y,
    ));
    Some(Landing {
        target: landing.target.clone(),
        display: landing.id.clone(),
        corner: landing.nearest(centre),
        size: landing.size,
        elsewhere: landing.id != source.id,
    })
}

/// Rune's target for the display `saved` names: the native display itself,
/// which Rune falls back to the primary display for when it is not connected.
/// `None` is the primary display, which is also what every overlay opens on.
///
/// Called when a capsule shows, so it may run off the main thread: nothing
/// here needs the UI thread.
pub(super) fn target(saved: Option<&str>) -> DisplayTarget {
    let Some(saved) = saved else {
        return DisplayTarget::Primary;
    };
    target_in(saved).unwrap_or(DisplayTarget::Primary)
}

#[cfg(target_os = "windows")]
fn target_in(saved: &str) -> Option<DisplayTarget> {
    scrawl_capture::win::monitors()
        .into_iter()
        .find(|monitor| monitor.device == saved)
        .map(|monitor| native(&monitor))
}

/// A saved display is its own `NSScreenNumber`, which Rune matches against
/// the screens it finds now; a display that is gone falls back to the primary.
#[cfg(target_os = "macos")]
fn target_in(saved: &str) -> Option<DisplayTarget> {
    let id = saved.parse::<u32>().ok()?;
    Some(DisplayTarget::Native(rune::NativeDisplayId::MacOS(id)))
}

#[cfg(target_os = "linux")]
fn target_in(_saved: &str) -> Option<DisplayTarget> {
    None
}

/// Every display, the one under the pointer first: the display a release
/// lands on while Labs › Indicator on any display is on.
///
/// `None` where a capsule has no display to measure: a native Wayland
/// surface, or a call off the main thread on macOS.
#[cfg(target_os = "macos")]
pub(super) fn displays(_scale: f32) -> Option<Vec<Display>> {
    let mtm = objc2::MainThreadMarker::new()?;
    let screens = objc2_app_kit::NSScreen::screens(mtm);
    let primary = screens.iter().next()?;
    // Rune's macOS space is AppKit's, flipped: points, from the primary
    // screen's top-left.
    let primary_top = primary.frame().origin.y + primary.frame().size.height;
    let mut displays: Vec<Display> = screens
        .iter()
        .enumerate()
        .filter_map(|(index, screen)| {
            let id = crate::rune_hud::screen_number(&screen)?;
            let frame = screen.frame();
            let origin = (
                frame.origin.x as f32,
                (primary_top - frame.origin.y - frame.size.height) as f32,
            );
            let size = (frame.size.width as f32, frame.size.height as f32);
            Some(Display {
                target: DisplayTarget::Native(rune::NativeDisplayId::MacOS(id)),
                id: id.to_string(),
                frame: (origin.0, origin.1, size.0, size.1),
                origin,
                size,
                scale: 1.0,
                primary: index == 0,
            })
        })
        .collect();
    // The screen under the pointer first. Rune resolves the pointer's screen
    // with the same half-open rule, so both pick the same display where two
    // share an edge.
    let pointer = objc2_app_kit::NSEvent::mouseLocation();
    let point = (pointer.x as f32, (primary_top - pointer.y) as f32);
    if let Some(index) = displays.iter().position(|display| display.holds(point))
        && index > 0
    {
        displays.rotate_left(index);
    }
    Some(displays)
}

/// Every display, the one under the pointer first. Rune places a window's x
/// on the whole monitor and its top or bottom edge inside that monitor's work
/// area, so a display's point space starts at its work area's top.
#[cfg(target_os = "windows")]
pub(super) fn displays(_scale: f32) -> Option<Vec<Display>> {
    let mut displays: Vec<Display> = scrawl_capture::win::monitors()
        .into_iter()
        .map(|monitor| Display {
            target: native(&monitor),
            id: monitor.device,
            frame: (
                monitor.rect[0] as f32,
                monitor.rect[1] as f32,
                monitor.rect[2] as f32,
                monitor.rect[3] as f32,
            ),
            origin: (monitor.rect[0] as f32, monitor.work[1] as f32),
            size: (
                points(monitor.rect[2] as f32, monitor.scale as f32),
                points(monitor.work[3] as f32, monitor.scale as f32),
            ),
            scale: monitor.scale as f32,
            primary: monitor.primary,
        })
        .collect();
    if let Some(point) = scrawl_capture::win::cursor_position()
        && let Some(index) = displays
            .iter()
            .position(|display| display.holds((point.0 as f32, point.1 as f32)))
        && index > 0
    {
        displays.rotate_left(index);
    }
    Some(displays)
}

#[cfg(target_os = "windows")]
fn native(monitor: &scrawl_capture::win::Monitor) -> DisplayTarget {
    DisplayTarget::Native(rune::NativeDisplayId::Windows(monitor.handle))
}

/// Linux has no display to measure here: an X11 or XWayland capsule keeps the
/// primary RandR monitor, and a native Wayland capsule keeps returning to the
/// corner it started in ([`primary_screen_size`]).
#[cfg(target_os = "linux")]
pub(super) fn displays(_scale: f32) -> Option<Vec<Display>> {
    None
}

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
    use super::*;

    /// A 1512 × 982 point laptop display at the origin, at 2x, and a
    /// 1920 × 1080 display at 1x to its right. `pointer` names the display
    /// the pointer is on, which [`release`] takes first.
    fn laptop_and_monitor(pointer: &str) -> Vec<Display> {
        let laptop = Display {
            target: DisplayTarget::Primary,
            id: "primary".into(),
            frame: (0.0, 0.0, 3024.0, 1964.0),
            origin: (0.0, 0.0),
            size: (1512.0, 982.0),
            scale: 2.0,
            primary: true,
        };
        let monitor = Display {
            target: DisplayTarget::Native(rune::NativeDisplayId::Windows(7)),
            id: r"\\.\DISPLAY2".into(),
            frame: (3024.0, 0.0, 1920.0, 1080.0),
            origin: (3024.0, 0.0),
            size: (1920.0, 1080.0),
            scale: 1.0,
            primary: false,
        };
        let mut displays = vec![laptop, monitor];
        displays.sort_by_key(|display| display.id != pointer);
        displays
    }

    const SIZE: (f32, f32) = (260.0, 180.0);

    #[test]
    fn a_release_on_the_display_it_was_dragged_on_keeps_its_corner() {
        let displays = laptop_and_monitor("primary");
        // Dragged 40 pt left and 30 down on the laptop, let go still.
        let landing = release(
            &displays,
            Some("primary"),
            MeetingIndicatorCorner::TopRight,
            (-40.0, 30.0),
            (0.0, 0.0),
            SIZE,
        )
        .unwrap();
        assert_eq!(landing.corner, MeetingIndicatorCorner::TopRight);
        assert_eq!(landing.display, "primary");
        assert!(!landing.elsewhere);
        assert_eq!(landing.size, (1512.0, 982.0));
        // Thrown left past the middle: the far corner of the same display.
        let landing = release(
            &displays,
            Some("primary"),
            MeetingIndicatorCorner::TopRight,
            (-700.0, 0.0),
            (-400.0, 0.0),
            SIZE,
        )
        .unwrap();
        assert_eq!(landing.corner, MeetingIndicatorCorner::TopLeft);
        assert_eq!(landing.display, "primary");
        assert!(!landing.elsewhere);
    }

    #[test]
    fn a_release_over_another_display_jumps_to_its_nearest_corner() {
        let monitor = r"\\.\DISPLAY2";
        let displays = laptop_and_monitor(monitor);
        // The pointer is on the monitor to the right: the throw crosses the
        // laptop's right edge and lands on that display's left side.
        let landing = release(
            &displays,
            Some("primary"),
            MeetingIndicatorCorner::TopRight,
            (500.0, 0.0),
            (0.0, 0.0),
            SIZE,
        )
        .unwrap();
        assert_eq!(landing.corner, MeetingIndicatorCorner::TopLeft);
        assert_eq!(landing.display, monitor);
        assert_eq!(landing.size, (1920.0, 1080.0));
        assert!(landing.elsewhere);
        assert_eq!(landing.target, displays[0].target);
        // Below the monitor's middle too: its bottom corner.
        let landing = release(
            &displays,
            Some("primary"),
            MeetingIndicatorCorner::TopRight,
            (500.0, 900.0),
            (0.0, 0.0),
            SIZE,
        )
        .unwrap();
        assert_eq!(landing.corner, MeetingIndicatorCorner::BottomLeft);
        assert!(landing.elsewhere);
        // A throw carries it back: the far side of the monitor.
        let landing = release(
            &displays,
            Some("primary"),
            MeetingIndicatorCorner::TopLeft,
            (300.0, 0.0),
            (4000.0, 0.0),
            SIZE,
        )
        .unwrap();
        assert_eq!(landing.corner, MeetingIndicatorCorner::TopRight);
        assert!(landing.elsewhere);
    }

    #[test]
    fn a_saved_display_that_is_gone_measures_the_primary_one() {
        let monitor = r"\\.\DISPLAY2";
        let displays = laptop_and_monitor(monitor);
        // The display saved in settings is not connected any more: Rune puts
        // the capsule on the primary display, and the corner is measured from
        // there.
        let landing = release(
            &displays,
            Some(r"\\.\DISPLAY9"),
            MeetingIndicatorCorner::BottomLeft,
            (10.0, 10.0),
            (0.0, 0.0),
            SIZE,
        )
        .unwrap();
        assert_eq!(landing.corner, MeetingIndicatorCorner::BottomLeft);
        assert_eq!(landing.display, monitor);
        assert!(
            landing.elsewhere,
            "the pointer's display is the landing one"
        );
        // No pointer display either: nothing to land on.
        assert_eq!(
            release(
                &[],
                None,
                MeetingIndicatorCorner::TopRight,
                (0.0, 0.0),
                (0.0, 0.0),
                SIZE
            ),
            None
        );
    }

    #[test]
    fn the_primary_display_is_the_source_when_nothing_was_saved() {
        let displays = laptop_and_monitor(r"\\.\DISPLAY2");
        let landing = release(
            &displays,
            None,
            MeetingIndicatorCorner::BottomRight,
            (0.0, -10.0),
            (0.0, 0.0),
            SIZE,
        )
        .unwrap();
        // The primary is the laptop: dragged from its bottom right and let go
        // under the middle of the monitor, the corner is measured from where
        // the laptop put it, which is its bottom left; the monitor's own
        // bottom right would mean it was measured from there instead.
        assert_eq!(landing.corner, MeetingIndicatorCorner::BottomLeft);
        assert!(landing.elsewhere);
    }

    #[test]
    fn a_display_holds_the_pointer_by_its_whole_frame() {
        let displays = laptop_and_monitor("primary");
        let (laptop, monitor) = (&displays[0], &displays[1]);
        assert!(laptop.holds((0.0, 0.0)));
        assert!(laptop.holds((3023.0, 1963.0)));
        assert!(
            !laptop.holds((3024.0, 0.0)),
            "the next display's first pixel"
        );
        assert!(monitor.holds((3024.0, 0.0)));
        assert!(!monitor.holds((4944.0, 0.0)));
    }

    #[test]
    fn device_pixels_become_points() {
        assert_eq!(points(2880.0, 2.0), 1440.0);
        assert_eq!(points(1920.0, 1.0), 1920.0);
        // Before Rune sets the view's scale, points are pixels.
        assert_eq!(points(1920.0, 0.0), 1920.0);
    }

    #[test]
    fn no_saved_display_is_the_primary_display() {
        assert_eq!(target(None), DisplayTarget::Primary);
    }
}
