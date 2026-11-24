//! Low-profile scissor keycaps and their recessed trays: `Small` in a row's
//! shortcut control, `Large` in the shortcut mode explainer.

use rune::{Color, Edge, El, Shadow, Spring, TextStyle, Transition, col, row, text};
use rune_kit::Kit;

use super::Paint;

/// How big a keycap is: `Small` in a row's tray, `Large` in the explainer.
///
/// Every key is a low-profile scissor key, after Apple's Magic Keyboard: a
/// flat face on a cap whose skirt shows below it. A press sinks the face
/// into the skirt; the key's bounds never change.
#[derive(Clone, Copy, PartialEq)]
pub(in crate::rune_ui) enum KeySize {
    Small,
    Large,
}

impl KeySize {
    /// Height, skirt included.
    pub(in crate::rune_ui) const fn height(self) -> f32 {
        match self {
            Self::Small => 22.0,
            Self::Large => 28.0,
        }
    }

    /// A one-glyph key is square.
    pub(in crate::rune_ui) const fn min_width(self) -> f32 {
        self.height()
    }

    /// The skirt showing below a key at rest.
    const fn skirt(self) -> f32 {
        match self {
            Self::Small => 1.5,
            Self::Large => 2.0,
        }
    }

    /// A press sinks the face most of the way, leaving a hairline of skirt.
    const fn travel(self) -> f32 {
        self.skirt() - KEY_RIM
    }

    const fn face_radius(self) -> f32 {
        match self {
            Self::Small => 4.0,
            Self::Large => 5.0,
        }
    }

    pub(in crate::rune_ui) fn legend(self) -> TextStyle {
        match self {
            Self::Small => TextStyle::new(11.0, 400.0).lh(14.0),
            Self::Large => TextStyle::new(13.0, 400.0).lh(16.0),
        }
    }

    /// Beside the legend on the face.
    const fn face_padding(self) -> f32 {
        match self {
            Self::Small => 6.0,
            Self::Large => 8.0,
        }
    }
}

/// The cap's rim beside and above the face.
const KEY_RIM: f32 = 0.5;
/// A face sinks fast and comes back up a little slower, both without bounce.
const KEY_DOWN: Spring = Spring::new(0.08, 0.0);
const KEY_UP: Spring = Spring::new(0.18, 0.0);
/// Between the keycaps of a chord.
pub(in crate::rune_ui) const KEY_GAP: f32 = 3.0;
/// Between a tray's edge and its keys: a `Small` tray is as tall as a
/// button.
pub(in crate::rune_ui) const TRAY_PADDING: f32 = 3.0;

/// The keys' material, from the theme's tokens: in light a warm off-white
/// face (the warning cream warms the neutral layers) on a cap that shades
/// to warm grey; in dark a space-black face on a cap that falls to the
/// deepest background.
pub(in crate::rune_ui) struct KeyMaterial {
    pub(in crate::rune_ui) cap_top: Color,
    pub(in crate::rune_ui) cap_bottom: Color,
    pub(in crate::rune_ui) face: Color,
    pub(in crate::rune_ui) ink: Color,
    /// The tight shadow under the cap and the soft one it casts at rest.
    pub(in crate::rune_ui) contact: f32,
    pub(in crate::rune_ui) cast: f32,
}

impl KeyMaterial {
    pub(in crate::rune_ui) fn new(k: &Kit) -> Self {
        let c = &k.c;
        if k.dark {
            let face = c.bg_base.mix(c.layer_01, 0.6);
            Self {
                cap_top: c.layer_03.mix(c.layer_04, 0.35),
                cap_bottom: c.bg_deep,
                face,
                ink: c.text_base.mix(c.text_faint, 0.15),
                contact: 0.8,
                cast: 0.5,
            }
        } else {
            let warm = c.warning.bg;
            Self {
                cap_top: c.white,
                cap_bottom: c.layer_04.mix(c.text_faint, 0.28).mix(c.warning.fg, 0.05),
                face: c.layer_02.mix(warm, 0.35),
                ink: c.text_base.mix(c.text_muted, 0.15),
                contact: 0.2,
                cast: 0.12,
            }
        }
    }
}

/// One tray of keys per shortcut: the keys sit in a well recessed into
/// the surface, lit from above so its top edge falls into shadow. The well
/// clips its keys, so while it springs to a new shortcut's width they never
/// poke out over what is beside it.
pub(in crate::rune_ui) fn key_tray(
    p: Paint,
    key: &str,
    size: KeySize,
    keys: impl IntoIterator<Item = El>,
) -> El {
    let k = p.k;
    let c = &k.c;
    let (top, bottom, rim, well) = if k.dark {
        (
            c.bg_deep,
            c.bg_base.mix(c.layer_01, 0.7),
            k.light(0.07),
            0.6,
        )
    } else {
        (
            c.layer_04.mix(c.layer_03, 0.35),
            c.layer_03.mix(c.layer_02, 0.6),
            k.dark_a(0.07),
            0.14,
        )
    };
    // Concentric with the caps inside it.
    let radius = size.face_radius() + KEY_RIM + TRAY_PADDING;
    row()
        .key(key)
        .padding(TRAY_PADDING)
        .gap(KEY_GAP)
        .center_y()
        .radius(radius)
        .continuous_corners()
        .clip_rounded(radius)
        .fill(bottom)
        .gradient(top, bottom)
        .border(p.hair.max(0.5), rim)
        .box_shadow(Shadow {
            y: 1.0,
            blur: 2.5,
            color: k.dark_a(well),
            inset: true,
            ..Shadow::default()
        })
        .children(keys)
}

/// A shortcut's keys at rest, in one `Small` tray. Keys a new shortcut
/// adds rise into the well, and keys it drops leave through its top.
pub(in crate::rune_ui) fn keycaps(p: Paint, key: &str, parts: &[String]) -> El {
    key_tray(
        p,
        key,
        KeySize::Small,
        parts.iter().enumerate().map(|(index, part)| {
            keycap(p, index, part, KeySize::Small, false).transition(tray_roll())
        }),
    )
}

/// The width [`keycaps`] draws `parts` at, with `measure` giving a
/// legend's width.
#[cfg(hex_capture)]
pub(in crate::rune_ui) fn keycaps_width(
    parts: &[String],
    measure: impl Fn(&str, TextStyle) -> f32,
) -> f32 {
    let size = KeySize::Small;
    let caps: f32 = parts
        .iter()
        .map(|part| {
            (measure(part, size.legend()) + 2.0 * (size.face_padding() + KEY_RIM))
                .max(size.min_width())
        })
        .sum();
    caps + KEY_GAP * parts.len().saturating_sub(1) as f32 + 2.0 * TRAY_PADDING
}

/// In from the well's bottom, out through its top, inside its clip.
pub(in crate::rune_ui) fn tray_roll() -> Transition {
    Transition::push(Edge::Bottom)
}

/// One key: a face on a cap whose skirt shows below it. A press sinks the
/// face into the skirt, tightens the cap's shadow and shades the face
/// toward its bottom edge.
pub(in crate::rune_ui) fn keycap(
    p: Paint,
    index: usize,
    label: &str,
    size: KeySize,
    down: bool,
) -> El {
    let k = p.k;
    let m = KeyMaterial::new(k);
    let pressed = if down { 1.0 } else { 0.0 };
    let travel = size.travel() * pressed;
    let face_height = size.height() - KEY_RIM - size.skirt();
    // Shadow offsets never spring, so the resting and pressed casts are two
    // layers that trade strength.
    let cast = |y: f32, blur: f32, strength: f32| Shadow {
        y,
        blur,
        color: k.dark_a(m.cast * strength),
        ..Shadow::default()
    };
    let cap = col()
        .key("cap")
        .px(KEY_RIM)
        .pt(KEY_RIM)
        .pb(size.skirt() - travel)
        .radius(size.face_radius() + KEY_RIM)
        .continuous_corners()
        .fill(m.cap_bottom)
        .gradient(m.cap_top, m.cap_bottom)
        .shadow(0.5, 0.5, k.dark_a(m.contact))
        .box_shadow(cast(size.skirt(), 2.0 * size.skirt(), 1.0 - pressed))
        .box_shadow(cast(KEY_RIM, 2.0, pressed))
        .child(
            row()
                .key("face")
                .min_w(size.min_width() - 2.0 * KEY_RIM)
                .h(face_height)
                .px(size.face_padding())
                .center()
                .radius(size.face_radius())
                .continuous_corners()
                .fill(m.face)
                .gradient(
                    k.light(0.03 - 0.03 * pressed),
                    k.dark_a(0.02 + 0.06 * pressed),
                )
                .child(
                    text(label.to_string())
                        .key("label")
                        .font(size.legend())
                        .color(m.ink),
                ),
        );
    col()
        .key(format!("keycap{index}"))
        .h(size.height())
        .pt(travel)
        .motion(if down { KEY_DOWN } else { KEY_UP })
        .child(cap)
}
