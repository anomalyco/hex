//! The notch the notch flow's capsule grows out of: a MacBook's, or on a
//! flat display one the size Hex pretends it has, as `hud_placement`'s
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
/// The narrowest the capsule gets, so the notch never shows beside it.
pub(crate) const MIN_WIDTH: f32 = WIDTH + 24.0;

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

/// The capsule's shell tucked into the notch, in panel points (the panel's
/// top on the notch's bottom edge): where it grows from and shrinks back to.
pub(crate) fn tucked() -> [f32; 4] {
    [CX - WIDTH * 0.5, -REACH, WIDTH, REACH]
}

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
