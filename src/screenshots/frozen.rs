//! The display as it was when the screenshot picker opened, kept only
//! while another app shows a menu or popup.
//!
//! An open menu, popup, or tooltip closes on the first click outside it,
//! and the picker's click is outside it. So when one is on the display
//! under the pointer as the picker opens ([`menu_open`]), that display is
//! shot once, before any click, and an area or a full-screen pick is cut
//! from that frame instead of being shot again. Otherwise nothing is
//! frozen and the pick is shot live, as it shows when it's clicked. A
//! window pick is always shot live: it's one window alone, without
//! whatever lies over it.

use std::sync::Mutex;

use capture_core::{Exclusions, Image, Source};
use scrawl_capture::windows::RawWindow;

/// `kCGPopUpMenuWindowLevel`: menus, pop-up buttons, and `<select>` lists.
const POP_UP_MENU_LAYER: i32 = 101;
/// `kCGHelpWindowLevel`: tooltips. Higher levels (dragging, screen savers,
/// shields, assistive overlays) aren't anything a click closes.
const HELP_LAYER: i32 = 200;

/// One display's frame, taken for the picker opening `id`.
pub(super) struct Frozen {
    id: u64,
    display: u32,
    /// The display's size, in points.
    points: [f64; 2],
    image: Image,
}

static FROZEN: Mutex<Option<Frozen>> = Mutex::new(None);

fn slot() -> std::sync::MutexGuard<'static, Option<Frozen>> {
    FROZEN.lock().unwrap_or_else(|error| error.into_inner())
}

/// Whether `windows` (the on-screen list) holds a menu, popup, or tooltip
/// of an app other than `own_pid` on the display at `frame` (global
/// points): something the picker's click would close.
fn menu_open(windows: &[RawWindow], frame: [f64; 4], own_pid: i32) -> bool {
    let [fx, fy, fw, fh] = frame;
    windows.iter().any(|window| {
        let [x, y, w, h] = window.bounds;
        window.pid != own_pid
            && (POP_UP_MENU_LAYER..=HELP_LAYER).contains(&window.layer)
            && window.alpha > 0.0
            && w > 0.0
            && h > 0.0
            && x < fx + fw
            && fx < x + w
            && y < fy + fh
            && fy < y + h
    })
}

/// Shoots the display under the pointer for the picker opening `id`, with
/// `exclude` (Hex's overlays) and the pointer left out, when another app
/// shows a menu or popup there. Nothing otherwise, when there's no display
/// there, or when the shot fails: the pick is then shot live.
pub(super) fn capture(id: u64, exclude: &Exclusions) -> Option<Frozen> {
    use scrawl_capture::screens;
    let displays = screens::displays();
    let display = &displays[screens::index_at(&displays, screens::pointer()?)?];
    let own_pid = std::process::id() as i32;
    if !menu_open(
        &scrawl_capture::windows::raw_windows(),
        display.frame,
        own_pid,
    ) {
        return None;
    }
    match capture_core::screenshot_with(&Source::Display(display.id), exclude, None, false) {
        Ok(image) => Some(Frozen {
            id,
            display: display.id,
            points: [display.frame[2], display.frame[3]],
            image,
        }),
        Err(error) => {
            tracing::warn!(%error, "could not freeze the display for the picker");
            None
        }
    }
}

/// Keeps `frozen` for its picker, replacing any older one.
pub(super) fn keep(frozen: Frozen) {
    *slot() = Some(frozen);
}

/// Drops the kept frame: the picker closed without a shot.
pub(super) fn forget() {
    *slot() = None;
}

/// The frame kept for the picker opening `id`, if it's that one's.
pub(super) fn take(id: u64) -> Option<Frozen> {
    slot().take().filter(|frozen| frozen.id == id)
}

impl Frozen {
    /// What `source` shows in this frame: the whole display, or an area of
    /// it. Nothing for a window or another display.
    pub(super) fn cut(self, source: &Source) -> Option<Image> {
        match *source {
            Source::Display(id) if id == self.display => Some(self.image),
            Source::Region { display, rect } if display == self.display => crop(
                &self.image,
                self.points,
                [rect.x, rect.y, rect.width, rect.height],
            ),
            _ => None,
        }
    }
}

/// The part of `image`, a display `points` wide and high, under `rect` (in
/// the display's points from its top-left corner), trimmed to the display.
/// Nothing when that part is empty or `image` isn't tightly packed.
fn crop(image: &Image, points: [f64; 2], rect: [f64; 4]) -> Option<Image> {
    let (width, height) = (image.width as usize, image.height as usize);
    if image.rgba.len() != width * height * 4 {
        return None;
    }
    let edge = |at: f64, pixels: u32, points: f64| -> Option<usize> {
        let scale = f64::from(pixels) / points;
        let at = (at * scale).round();
        if !scale.is_finite() || scale <= 0.0 || !at.is_finite() {
            return None;
        }
        #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
        Some(at.clamp(0.0, f64::from(pixels)) as usize)
    };
    let [x, y, w, h] = rect;
    let (left, right) = (
        edge(x, image.width, points[0])?,
        edge(x + w, image.width, points[0])?,
    );
    let (top, bottom) = (
        edge(y, image.height, points[1])?,
        edge(y + h, image.height, points[1])?,
    );
    if right <= left || bottom <= top {
        return None;
    }
    let mut rgba = Vec::with_capacity((right - left) * (bottom - top) * 4);
    for row in top..bottom {
        let start = (row * width + left) * 4;
        rgba.extend_from_slice(&image.rgba[start..start + (right - left) * 4]);
    }
    Some(Image {
        width: u32::try_from(right - left).ok()?,
        height: u32::try_from(bottom - top).ok()?,
        rgba,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A `width` × `height` image whose every pixel holds its own x and y.
    fn image(width: u32, height: u32) -> Image {
        let mut rgba = Vec::new();
        for y in 0..height {
            for x in 0..width {
                rgba.extend_from_slice(&[x as u8, y as u8, 0, 255]);
            }
        }
        Image {
            width,
            height,
            rgba,
        }
    }

    fn window(pid: i32, layer: i32, bounds: [f64; 4]) -> RawWindow {
        RawWindow {
            id: 1,
            pid,
            owner: String::new(),
            title: String::new(),
            bounds,
            layer,
            alpha: 1.0,
        }
    }

    #[test]
    fn freezes_only_for_another_apps_menu_on_the_pointers_display() {
        let display = [0.0, 0.0, 1440.0, 900.0];
        let menu = [100.0, 100.0, 200.0, 300.0];
        // A pop-up menu or a tooltip of another app on this display.
        assert!(menu_open(&[window(7, 101, menu)], display, 1));
        assert!(menu_open(&[window(7, 200, menu)], display, 1));
        // Hex's own overlays, normal and status-level windows, and
        // always-on system layers don't.
        assert!(!menu_open(&[window(1, 101, menu)], display, 1));
        assert!(!menu_open(&[window(7, 0, menu)], display, 1));
        assert!(!menu_open(&[window(7, 25, menu)], display, 1));
        assert!(!menu_open(&[window(7, 1000, menu)], display, 1));
        // A menu on another display, or an invisible one.
        assert!(!menu_open(
            &[window(7, 101, [1500.0, 100.0, 200.0, 300.0])],
            display,
            1
        ));
        let mut hidden = window(7, 101, menu);
        hidden.alpha = 0.0;
        assert!(!menu_open(&[hidden], display, 1));
        assert!(!menu_open(&[], display, 1));
    }

    #[test]
    fn crops_an_area_in_points_at_the_display_pixels() {
        // A 2x display, 10 × 6 points.
        let shot = crop(&image(20, 12), [10.0, 6.0], [2.0, 1.0, 3.0, 2.0]).unwrap();
        assert_eq!((shot.width, shot.height), (6, 4));
        assert_eq!(&shot.rgba[..4], &[4, 2, 0, 255]);
        assert_eq!(&shot.rgba[shot.rgba.len() - 4..], &[9, 5, 0, 255]);
    }

    #[test]
    fn trims_an_area_to_the_display_and_refuses_an_empty_one() {
        let shot = crop(&image(10, 10), [10.0, 10.0], [-5.0, 8.0, 8.0, 10.0]).unwrap();
        assert_eq!((shot.width, shot.height), (3, 2));
        assert_eq!(&shot.rgba[..4], &[0, 8, 0, 255]);
        assert!(crop(&image(10, 10), [10.0, 10.0], [12.0, 0.0, 4.0, 4.0]).is_none());
        assert!(crop(&image(10, 10), [0.0, 10.0], [0.0, 0.0, 4.0, 4.0]).is_none());
    }

    #[test]
    fn a_frame_serves_only_its_own_display_and_never_a_window() {
        let frozen = || Frozen {
            id: 1,
            display: 7,
            points: [4.0, 4.0],
            image: image(4, 4),
        };
        let rect = capture_core::Rect {
            x: 1.0,
            y: 1.0,
            width: 2.0,
            height: 2.0,
        };
        assert_eq!(frozen().cut(&Source::Display(7)), Some(image(4, 4)));
        assert_eq!(
            frozen()
                .cut(&Source::Region { display: 7, rect })
                .map(|shot| (shot.width, shot.height)),
            Some((2, 2))
        );
        assert!(frozen().cut(&Source::Region { display: 8, rect }).is_none());
        assert!(frozen().cut(&Source::Window(7)).is_none());
    }
}
