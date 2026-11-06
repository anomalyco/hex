//! The notch the notch flow's capsule grows out of: a MacBook's, measured
//! on the screen under the pointer as the dictation HUD measures it, or on
//! a flat display one the size Hex pretends it has, as `hud_placement`'s
//! `Screen::pretending_notch` does (185 pt wide, the menu bar's height).
//! The capsule hangs from the notch's bottom edge; its walls run up past
//! the screen's top, so it covers the notch as it grows and shrinks back
//! into it.

// Only the macOS playground and notch reads much of this.
#![cfg_attr(not(target_os = "macos"), allow(dead_code))]

use rune::{Color, El, Path, Shape, Spring, canvas, el};

use super::surface::CX;

/// The notch's width: a 14-inch MacBook Pro's, and the pretend one's.
pub(crate) const WIDTH: f32 = 185.0;
/// Its bottom corners.
pub(crate) const RADIUS: f32 = 8.0;
/// The concave fillets where it meets the screen's top edge.
pub(crate) const EAR: f32 = 6.0;
/// How far the capsule's walls run up past the notch's bottom edge: past
/// the screen's top under any notch or menu bar.
pub(crate) const REACH: f32 = 48.0;
/// The capsule's room either side of the notch at its narrowest.
const SIDE: f32 = 12.0;
/// The narrowest the pretend notch's capsule gets, so the notch never shows
/// beside it.
pub(crate) const MIN_WIDTH: f32 = WIDTH + 2.0 * SIDE;

/// A notch in the capsule's panel: its left and right edges in panel
/// points, measured on the screen under the pointer, or the pretend notch
/// when none is read.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct Geometry {
    pub(crate) left: f32,
    pub(crate) right: f32,
}

impl Default for Geometry {
    /// The pretend notch: [`WIDTH`] wide, centred on the panel's [`CX`], as
    /// every build without a measured notch has always drawn it.
    fn default() -> Self {
        Self {
            left: CX - WIDTH * 0.5,
            right: CX + WIDTH * 0.5,
        }
    }
}

impl Geometry {
    pub(crate) fn width(self) -> f32 {
        self.right - self.left
    }

    pub(crate) fn centre(self) -> f32 {
        (self.left + self.right) * 0.5
    }

    /// The capsule's narrowest resting width: the notch with [`SIDE`] pt
    /// either side.
    pub(crate) fn min_width(self) -> f32 {
        self.width() + 2.0 * SIDE
    }

    /// The shell tucked into the notch, in panel points (the panel's top on
    /// the screen's top): where it grows from and shrinks back to.
    pub(crate) fn tucked(self) -> [f32; 4] {
        [self.left, -REACH, self.width(), REACH]
    }
}

/// A measured notch fitted to the panel: its edges in panel points, and
/// where the panel's left edge goes on the screen it was measured on.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct Measured {
    pub(crate) geometry: Geometry,
    /// The panel's left edge, in points from the display's left.
    pub(crate) panel_x: f32,
}

/// The notch spanning `left`..`right` (points from the display's left),
/// with its edges and the panel's origin snapped to `scale`'s backing-pixel
/// grid, so the shell's walls are whole pixels. The panel's [`CX`] lands on
/// the notch's middle, so its edges are known panel points.
pub(crate) fn fitted(left: f32, right: f32, scale: f32) -> Measured {
    let snap = |points: f32| (points * scale).round() / scale;
    let (left, right) = (snap(left), snap(right));
    let panel_x = snap((left + right) * 0.5 - CX);
    Measured {
        geometry: Geometry {
            left: left - panel_x,
            right: right - panel_x,
        },
        panel_x,
    }
}

/// The notch of the screen under the pointer, fitted to the panel; `None`
/// when it can't be read, which keeps the pretend one.
#[cfg(target_os = "macos")]
pub(crate) fn measured() -> Option<Measured> {
    let (screen, _, scale) = crate::rune_hud::pointer_screen(objc2::MainThreadMarker::new()?)?;
    // The HUD's pretend notch on a flat display, the hardware one otherwise.
    let screen = screen.pretending_notch();
    let notch = screen.notch()?;
    let left = (notch.x - notch.width * 0.5 - screen.frame.x) as f32;
    Some(fitted(left, left + notch.width as f32, scale as f32))
}

/// A screen the capsule's notch flow shows on, in points.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct Screen {
    pub(crate) name: &'static str,
    pub(crate) size: (f32, f32),
    /// The menu bar's height.
    pub(crate) bar: f32,
    /// The notch's height: the hardware's, or the menu bar's when Hex
    /// pretends one.
    pub(crate) notch: f32,
    /// Whether the notch is hardware.
    pub(crate) real: bool,
}

pub(crate) const MACBOOK_PRO_14: Screen = Screen {
    name: "14-inch MacBook Pro",
    size: (1512.0, 982.0),
    bar: 32.0,
    notch: 32.0,
    real: true,
};

pub(crate) const DISPLAY_27: Screen = Screen {
    name: "27-inch display",
    size: (2560.0, 1440.0),
    bar: 24.0,
    notch: 24.0,
    real: false,
};

/// A notch `height` tall centred on `x`, its top at `y`, with its ears.
pub(crate) fn notch(key: &str, color: Color, x: f32, y: f32, height: f32) -> El {
    el().key(key.to_owned())
        .absolute(x - WIDTH * 0.5 - EAR, y)
        .size(WIDTH + 2.0 * EAR, height)
        .child(
            el().absolute(EAR, 0.0)
                .size(WIDTH, height)
                .corners([0.0, 0.0, RADIUS, RADIUS])
                .fill(color),
        )
        .child(ear(color, false).absolute(0.0, 0.0))
        .child(ear(color, true).absolute(WIDTH + EAR, 0.0))
}

/// The ears of a shell spanning `x` to `x + w`, at the screen's top `y`,
/// moving on the shell's spring.
pub(crate) fn ears(key: &str, color: Color, x: f32, w: f32, y: f32, motion: Spring) -> [El; 2] {
    [
        ear(color, false)
            .key(format!("{key}-left"))
            .absolute(x - EAR, y)
            .motion(motion),
        ear(color, true)
            .key(format!("{key}-right"))
            .absolute(x + w, y)
            .motion(motion),
    ]
}

/// One ear: the corner between a wall and the screen's top edge, filled
/// outside a quarter circle. `right` mirrors it for the right wall.
fn ear(color: Color, right: bool) -> El {
    // A quarter circle's cubic: its control points sit this far along the
    // tangents.
    const K: f32 = 0.552;
    let r = EAR;
    let path = if right {
        Path::new()
            .move_to([r, 0.0])
            .line_to([0.0, 0.0])
            .line_to([0.0, r])
            .cubic_to([0.0, r - K * r], [r - K * r, 0.0], [r, 0.0])
    } else {
        Path::new()
            .move_to([0.0, 0.0])
            .line_to([r, 0.0])
            .line_to([r, r])
            .cubic_to([r, r - K * r], [K * r, 0.0], [0.0, 0.0])
    }
    .close();
    canvas(Shape::new().view_box(r, r).fill_path(&path, color)).size(r, r)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Without a measured notch the geometry is the one every build has
    /// drawn: 185 pt wide, the panel's middle, 12 pt either side at rest.
    #[test]
    fn the_default_geometry_is_the_pretend_notch() {
        let notch = Geometry::default();
        assert_eq!(notch.left, CX - WIDTH * 0.5);
        assert_eq!(notch.right, CX + WIDTH * 0.5);
        assert_eq!(notch.width(), WIDTH);
        assert_eq!(notch.centre(), CX);
        assert_eq!(notch.min_width(), MIN_WIDTH);
        assert_eq!(notch.tucked(), [CX - WIDTH * 0.5, -REACH, WIDTH, REACH]);
    }

    /// A notch 0.5 pt left of its 1512 pt display's middle moves the panel:
    /// its edges stay where the hardware is, not where the display centres.
    #[test]
    fn a_measured_notch_moves_the_panel_with_its_edges() {
        // aux_left 663 and aux_right 664: 185 pt wide, its middle at 755.5.
        let Measured { geometry, panel_x } = fitted(663.0, 848.0, 2.0);
        assert_eq!(panel_x, 435.5, "the panel is off the display's middle");
        assert_eq!(geometry.width(), 185.0);
        assert_eq!(geometry.centre(), CX);
        assert_eq!(
            (panel_x + geometry.left, panel_x + geometry.right),
            (663.0, 848.0),
            "the shell's walls land on the notch's edges"
        );
        assert_eq!(geometry.min_width(), 209.0);
        assert_eq!(geometry.tucked(), [CX - 92.5, -REACH, 185.0, REACH]);
        // At 1× a half point can't be kept, so the panel takes the whole
        // pixel nearest and the notch sits half a point off its middle.
        let Measured { geometry, panel_x } = fitted(663.0, 848.0, 1.0);
        assert_eq!(panel_x, 436.0);
        assert_eq!(
            (panel_x + geometry.left, panel_x + geometry.right),
            (663.0, 848.0)
        );
    }

    /// At 2× an odd-pixel notch still lands on whole pixels, its panel's
    /// origin included.
    #[test]
    fn a_two_x_notch_lands_on_whole_pixels() {
        let Measured { geometry, panel_x } = fitted(663.5, 849.0, 2.0);
        let whole = |points: f32| (points * 2.0).fract() == 0.0;
        assert!(whole(panel_x), "the panel's origin is a whole pixel");
        assert!(whole(geometry.left) && whole(geometry.right));
        assert_eq!(panel_x + geometry.left, 663.5);
        assert_eq!(panel_x + geometry.right, 849.0);
        assert_eq!(geometry.width(), 185.5, "371 physical pixels");
    }

    /// Edges between pixels are rounded onto the grid before the panel is
    /// placed, so neither wall sits half a pixel off.
    #[test]
    fn edges_between_pixels_snap_outward_or_inward() {
        let Measured { geometry, panel_x } = fitted(663.25, 848.75, 2.0);
        assert_eq!(panel_x, 436.5);
        assert_eq!(
            (panel_x + geometry.left, panel_x + geometry.right),
            (663.5, 849.0)
        );
    }
}
