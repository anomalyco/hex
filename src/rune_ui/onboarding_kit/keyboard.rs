//! A 3D perspective-tilted Apple-style low-profile keyboard: a machined
//! rounded chassis with a recessed key well and 64 perspective-projected
//! rounded keycaps whose top faces physically sink along the Z axis as keys
//! are pressed.

use rune::{Anim, Color, El, LineCap, Shape, canvas, el, text};

use super::super::controls::{KeyMaterial, Paint};

/// One key: its id, its legend, and how many key units it spans across its row.
struct Cap(&'static str, &'static str, f32);

const fn k(id: &'static str) -> Cap {
    Cap(id, id, 1.0)
}

const ROWS: [&[Cap]; 5] = [
    &[
        k("`"),
        k("1"),
        k("2"),
        k("3"),
        k("4"),
        k("5"),
        k("6"),
        k("7"),
        k("8"),
        k("9"),
        k("0"),
        k("-"),
        k("="),
        Cap("delete", "delete", 1.5),
    ],
    &[
        Cap("tab", "tab", 1.5),
        k("Q"),
        k("W"),
        k("E"),
        k("R"),
        k("T"),
        k("Y"),
        k("U"),
        k("I"),
        k("O"),
        k("P"),
        k("["),
        k("]"),
        k("\\"),
    ],
    &[
        Cap("caps", "caps", 1.75),
        k("A"),
        k("S"),
        k("D"),
        k("F"),
        k("G"),
        k("H"),
        k("J"),
        k("K"),
        k("L"),
        k(";"),
        k("'"),
        Cap("return", "return", 1.75),
    ],
    &[
        Cap("shift", "shift", 2.25),
        k("Z"),
        k("X"),
        k("C"),
        k("V"),
        k("B"),
        k("N"),
        k("M"),
        k(","),
        k("."),
        k("/"),
        Cap("shift-right", "shift", 2.25),
    ],
    &[
        Cap("fn", "fn", 1.0),
        Cap("control", "⌃", 1.0),
        Cap("option", "⌥", 1.0),
        Cap("command", "⌘", 1.25),
        Cap("space", "", 5.0),
        Cap("command-right", "⌘", 1.25),
        Cap("option-right", "⌥", 1.0),
        Cap("left", "←", 1.0),
        Cap("updown", "", 1.0),
        Cap("right", "→", 1.0),
    ],
];

const CANVAS_W: f32 = 548.0;
const CANVAS_H: f32 = 162.0;
const FRONT_W: f32 = 532.0;
const TOP_Y: f32 = 8.0;
const DEPTH_H: f32 = 142.0;
/// Perspective foreshortening: back edge narrows to ~85% of front width.
const TILT: f32 = 0.175;
/// Resting keycap height above the recessed well, in points.
const KEY_Z: f32 = 3.2;
const CHASSIS_LIP: f32 = 4.2;
const UNITS: f32 = 14.5;

/// The keyboard's stage bounding size in points.
pub(crate) fn size() -> (f32, f32) {
    (CANVAS_W, CANVAS_H)
}

/// Perspective scale factor at depth `v` (`0.0` back edge -> `1.0` front edge).
fn persp(v: f32) -> f32 {
    1.0 / (1.0 + TILT * (1.0 - v))
}

/// Projects deck coordinates `(u, v, z)` (`u` in `-0.5..=0.5`, `v` in `0..=1`,
/// `z` height in points) into canvas `[x, y]`.
fn proj(u: f32, v: f32, z: f32) -> [f32; 2] {
    let s = persp(v);
    let x = CANVAS_W * 0.5 + u * FRONT_W * s;
    let depth_t = v * s / persp(1.0);
    let y = TOP_Y + DEPTH_H * depth_t - z * (0.68 + 0.32 * v);
    [x, y]
}

/// Rounded rectangle in `(u, v)` deck coordinates projected at height `z`.
fn projected_rrect([u0, v0, u1, v1]: [f32; 4], ru: f32, rv: f32, z: f32) -> Vec<[f32; 2]> {
    let ru = ru.min((u1 - u0) * 0.45);
    let rv = rv.min((v1 - v0) * 0.45);
    let corners = [
        ([u1 - ru, v0 + rv], -std::f32::consts::FRAC_PI_2, 0.0),
        ([u1 - ru, v1 - rv], 0.0, std::f32::consts::FRAC_PI_2),
        (
            [u0 + ru, v1 - rv],
            std::f32::consts::FRAC_PI_2,
            std::f32::consts::PI,
        ),
        (
            [u0 + ru, v0 + rv],
            std::f32::consts::PI,
            3.0 * std::f32::consts::FRAC_PI_2,
        ),
    ];
    let mut pts = Vec::with_capacity(20);
    for ([cu, cv], a0, a1) in corners {
        for step in 0..=3 {
            let t = step as f32 / 3.0;
            let a = a0 + (a1 - a0) * t;
            pts.push(proj(cu + ru * a.cos(), cv + rv * a.sin(), z));
        }
    }
    pts
}

/// The id of the key that types the lowercase `letter`.
pub(crate) fn letter(letter: char) -> Option<&'static str> {
    let upper = letter.to_ascii_uppercase();
    ROWS.iter()
        .flat_map(|row| row.iter())
        .find(|cap| cap.0.len() == 1 && cap.0.starts_with(upper))
        .map(|cap| cap.0)
}

/// The key a shortcut keycap names (`⌥`, `R⌘`, `fn`), if drawn on the board.
pub(crate) fn modifier(cap: &str) -> Option<&'static str> {
    let right = cap.starts_with('R');
    let key = [
        ("⌥", "option"),
        ("Alt", "option"),
        ("⌘", "command"),
        ("⌃", "control"),
        ("Ctrl", "control"),
        ("⇧", "shift"),
        ("Shift", "shift"),
        ("fn", "fn"),
    ]
    .into_iter()
    .find(|(legend, _)| cap.contains(legend))?
    .1;
    Some(match (key, right) {
        ("option", true) => "option-right",
        ("command", true) => "command-right",
        ("shift", true) => "shift-right",
        (key, _) => key,
    })
}

struct KeySlot {
    id: &'static str,
    legend: &'static str,
    uv: [f32; 4],
}

/// All individual key slots (splitting `updown` into `up` and `down`) in `(u, v)`
/// deck space inside the recessed well.
fn key_slots() -> Vec<KeySlot> {
    const PAD_U: f32 = 0.024;
    const PAD_V: f32 = 0.055;
    const GAP_U: f32 = 0.0085;
    const GAP_V: f32 = 0.026;

    let inner_w = 1.0 - 2.0 * PAD_U;
    let inner_h = 1.0 - 2.0 * PAD_V;
    let row_h = (inner_h - (ROWS.len() - 1) as f32 * GAP_V) / ROWS.len() as f32;
    let unit_w = (inner_w - 13.0 * GAP_U) / UNITS;

    let mut out = Vec::with_capacity(65);
    for (r, row) in ROWS.iter().enumerate() {
        let v0 = PAD_V + r as f32 * (row_h + GAP_V);
        let v1 = v0 + row_h;
        let total_gaps = (row.len().saturating_sub(1)) as f32 * GAP_U;
        let row_unit = (inner_w - total_gaps) / UNITS;
        let _ = unit_w;
        let mut u = -0.5 + PAD_U;
        for cap in *row {
            let w = cap.2 * row_unit;
            if cap.0 == "updown" {
                let mid = (v0 + v1) * 0.5;
                let split = GAP_V * 0.35;
                out.push(KeySlot {
                    id: "up",
                    legend: "↑",
                    uv: [u, v0, u + w, mid - split],
                });
                out.push(KeySlot {
                    id: "down",
                    legend: "↓",
                    uv: [u, mid + split, u + w, v1],
                });
            } else {
                out.push(KeySlot {
                    id: cap.0,
                    legend: cap.1,
                    uv: [u, v0, u + w, v1],
                });
            }
            u += w + GAP_U;
        }
    }
    out
}

/// Warm backlit accent for a highlighted shortcut key, from theme tokens.
pub(crate) fn ember(p: Paint) -> Color {
    rune_theme_opencode::ember(p.k.dark, 9)
}

/// Draws the 3D perspective-tilted keyboard: `dim` fades unlit keys (e.g.
/// during `"With Hex"` so the lit shortcut key stands out), `depth(id)`
/// depresses keys `0..=1`, and `glow(id)` backlights shortcut keys `0..=1`.
pub(crate) fn keyboard(
    p: Paint,
    key: &str,
    dim: f32,
    depth: impl Fn(&str) -> f32,
    glow: impl Fn(&str) -> f32,
) -> El {
    let k = p.k;
    let c = &k.c;
    let m = KeyMaterial::new(k);
    let bg = c.bg_deep;
    let dim = dim.clamp(0.0, 1.0);

    let mut shape = Shape::new();

    // 1. Soft ground shadow beneath the tilted chassis.
    let shadow_col = bg.mix(c.black, if k.dark { 0.45 * dim } else { 0.10 * dim });
    let ground = projected_rrect(
        [-0.503, 0.02, 0.503, 1.015],
        0.028,
        0.075,
        -CHASSIS_LIP - 2.0,
    );
    shape = shape.fill(ground, shadow_col);

    // 2. 3D front chassis lip (thickness of the anodized keyboard body).
    let lip_base = if k.dark {
        c.layer_01.mix(c.layer_03, 0.38)
    } else {
        c.layer_03.mix(c.border_base, 0.45)
    };
    let lip_col = bg.mix(lip_base, dim);
    let deck_lower = projected_rrect([-0.5, 0.0, 0.5, 1.0], 0.026, 0.07, -CHASSIS_LIP);
    shape = shape.fill(deck_lower, lip_col);

    // 3. Top anodized chassis deck + crisp rim highlight.
    let deck_base = if k.dark {
        c.bg_base.mix(c.layer_02, 0.65)
    } else {
        c.layer_02.mix(c.white, 0.35)
    };
    let deck_col = bg.mix(deck_base, dim);
    let deck_rim = bg.mix(c.border_weak, dim);
    let deck_top = projected_rrect([-0.5, 0.0, 0.5, 1.0], 0.026, 0.07, 0.0);
    shape = shape.fill(deck_top.clone(), deck_col);
    shape = shape.stroke_with(deck_top, true, 1.0, LineCap::Round, deck_rim);

    // 4. Recessed dark key well inside the deck.
    let well_base = if k.dark {
        c.bg_deep
    } else {
        c.layer_03.mix(c.border_muted, 0.35)
    };
    let well_col = bg.mix(well_base, dim);
    let well = projected_rrect([-0.482, 0.042, 0.482, 0.958], 0.014, 0.04, 0.0);
    shape = shape.fill(well, well_col);

    // 5. Perspective keycaps (skirt + elevated top face) and crisp legends.
    let slots = key_slots();
    let mut legends = Vec::with_capacity(slots.len());
    let warm = ember(p);
    let warm_fill = rune_theme_opencode::ember(k.dark, if k.dark { 4 } else { 3 });

    for slot in &slots {
        let down = depth(slot.id).clamp(0.0, 1.0);
        let lit = glow(slot.id).clamp(0.0, 1.0);
        let vis = (dim + (1.0 - dim) * lit).clamp(0.0, 1.0);
        let z = KEY_Z * (1.0 - 0.82 * down);
        let [u0, v0, u1, v1] = slot.uv;

        // Key skirt sitting in the well (`z = 0.25`) under the elevated face.
        let skirt_raw = m
            .cap_bottom
            .mix(m.cap_top, 0.35 * (1.0 - down))
            .mix(warm, lit * 0.35);
        let skirt_col = well_col.mix(skirt_raw, vis);
        let skirt_poly = projected_rrect([u0, v0, u1, v1], 0.006, 0.022, 0.25);
        shape = shape.fill(skirt_poly, skirt_col);

        // Elevated keycap top face at height `z`.
        let inset_u = 0.0012;
        let inset_v = 0.004;
        let face_raw = m
            .face
            .mix(c.layer_03, down * 0.22)
            .mix(warm_fill, lit * if k.dark { 0.72 } else { 0.55 });
        let face_col = well_col.mix(face_raw, vis);
        let rim_raw = c
            .border_weak
            .mix(m.cap_top, 0.25 * (1.0 - down))
            .mix(warm, lit * 0.85);
        let rim_col = well_col.mix(rim_raw, vis);

        let face_poly = projected_rrect(
            [u0 + inset_u, v0 + inset_v, u1 - inset_u, v1 - inset_v],
            0.0055,
            0.020,
            z,
        );
        shape = shape.fill(face_poly.clone(), face_col);
        shape = shape.stroke_with(
            face_poly,
            true,
            if lit > 0.05 { 1.15 } else { 0.75 },
            LineCap::Round,
            rim_col,
        );

        if !slot.legend.is_empty() {
            let v_mid = (v0 + v1) * 0.52;
            let [cx, cy] = proj((u0 + u1) * 0.5, v_mid, z);
            let row_scale = persp(v_mid);
            let half_arrow = slot.id == "up" || slot.id == "down";
            let is_word = slot.legend.chars().count() > 1;
            let base_pt = if half_arrow {
                8.0
            } else if is_word {
                9.2
            } else {
                11.2
            };
            let pt = if half_arrow {
                8.0
            } else {
                (base_pt * row_scale).round().max(8.5)
            };
            let font = rune::TextStyle::new(pt, if lit > 0.2 { 540.0 } else { 460.0 }).lh(pt + 1.5);
            let (tw, th) = rune::text::measure(slot.legend, &[], font, None);
            let y_nudge = if half_arrow { -0.35 } else { 0.0 };
            let ink_raw = m.ink.mix(warm, lit * 0.75);
            let ink_col = face_col.mix(ink_raw, vis);
            legends.push(
                text(slot.legend.to_string())
                    .key(format!("lbl-{}", slot.id))
                    .font(font)
                    .color(ink_col)
                    .absolute(cx - tw * 0.5, cy - th * 0.5 + y_nudge)
                    .offset_motion(Anim::INSTANT)
                    .opacity_motion(Anim::INSTANT),
            );
        }
    }

    el().key(key)
        .size(CANVAS_W, CANVAS_H)
        .instant_changes()
        .child(canvas(shape).key("deck").size(CANVAS_W, CANVAS_H))
        .children(legends)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn perspective_narrows_the_back_edge_and_centers_the_board() {
        let [back_l, back_y] = proj(-0.5, 0.0, 0.0);
        let [back_r, _] = proj(0.5, 0.0, 0.0);
        let [front_l, front_y] = proj(-0.5, 1.0, 0.0);
        let [front_r, _] = proj(0.5, 1.0, 0.0);

        let back_w = back_r - back_l;
        let front_w = front_r - front_l;
        assert!(back_w < front_w * 0.9, "back={back_w}, front={front_w}");
        assert!(((back_l + back_r) * 0.5 - CANVAS_W * 0.5).abs() < 0.01);
        assert!(((front_l + front_r) * 0.5 - CANVAS_W * 0.5).abs() < 0.01);
        assert!(front_y > back_y + 100.0);
    }

    #[test]
    fn pressing_a_key_lowers_its_projected_top_face() {
        let [_, up_y] = proj(0.0, 0.5, KEY_Z);
        let [_, down_y] = proj(0.0, 0.5, KEY_Z * 0.18);
        assert!(down_y > up_y + 1.5, "up={up_y}, down={down_y}");
    }

    #[test]
    fn shortcut_keycaps_and_letters_find_their_keys() {
        assert_eq!(modifier("⌥"), Some("option"));
        assert_eq!(modifier("R⌘"), Some("command-right"));
        assert_eq!(modifier("L⌥"), Some("option"));
        assert_eq!(modifier("A"), None);
        assert_eq!(letter('q'), Some("Q"));
    }
}
