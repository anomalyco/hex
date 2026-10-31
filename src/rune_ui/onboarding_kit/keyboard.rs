//! Flat, crisp 14.5u ANSI Apple Magic Keyboard for onboarding:
//!
//! - Exact constant-pitch 14.5u ANSI geometry (`u * PITCH - GAP`), so `space`
//!   aligns with `C..M`, `1-Q-A-Z` staggers at `0.5u / 0.25u / 0.5u`, and all
//!   five right edges align to `0.000 pt`.
//! - Every key legend is laid out directly inside its own Rune keycap `El`
//!   (Apple corner alignment on word modifiers, dead-centre on alphas/symbols).
//! - Clean 2D scissor shadows (`box_shadow` + top inset sheen) and warm `⌥`
//!   backlight halo, with zero 3D embossing on UI elements.

use rune::{Anim, Color, El, Shadow, TextStyle, el, row, text};

use super::super::controls::{KeyMaterial, Paint};
use super::gpu_stage::KEY_COUNT;

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

const DECK_W: f32 = 472.0;
const DECK_H: f32 = 172.0;
const WELL_INSET: f32 = 7.0;
const KEY_INSET: f32 = 10.5;
const INNER_W: f32 = DECK_W - 2.0 * KEY_INSET; // 451.0
const INNER_H: f32 = DECK_H - 2.0 * KEY_INSET; // 151.0
const UNITS: f32 = 14.5;
const GAP: f32 = 3.5;
pub(crate) const PITCH: f32 = (INNER_W + GAP) / UNITS;
const ROW_GAP: f32 = 3.5;
const ROW_H: f32 = (INNER_H - 4.0 * ROW_GAP) / 5.0;

pub(crate) fn size() -> (f32, f32) {
    (DECK_W, DECK_H)
}

pub(crate) fn letter(letter: char) -> Option<&'static str> {
    let upper = letter.to_ascii_uppercase();
    ROWS.iter()
        .flat_map(|row| row.iter())
        .find(|cap| cap.0.len() == 1 && cap.0.starts_with(upper))
        .map(|cap| cap.0)
}

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

#[derive(Clone, Copy)]
pub(crate) struct KeySlot {
    pub(crate) id: &'static str,
    pub(crate) legend: &'static str,
    /// `[x, y, w, h]` in deck points (`[0..DECK_W, 0..DECK_H]`).
    pub(crate) rect: [f32; 4],
}

pub(crate) fn key_slots() -> [KeySlot; KEY_COUNT] {
    let dummy = KeySlot {
        id: "",
        legend: "",
        rect: [0.0; 4],
    };
    let mut slots = [dummy; KEY_COUNT];
    let mut idx = 0;

    for (r, row_caps) in ROWS.iter().enumerate() {
        let y = KEY_INSET + r as f32 * (ROW_H + ROW_GAP);
        let mut u_acc = 0.0;
        for cap in *row_caps {
            let x = KEY_INSET + u_acc * PITCH;
            let w = cap.2 * PITCH - GAP;
            if cap.0 == "updown" {
                let split = 2.0;
                let hh = (ROW_H - split) * 0.5;
                slots[idx] = KeySlot {
                    id: "up",
                    legend: "▲",
                    rect: [x, y, w, hh],
                };
                idx += 1;
                slots[idx] = KeySlot {
                    id: "down",
                    legend: "▼",
                    rect: [x, y + hh + split, w, hh],
                };
                idx += 1;
            } else {
                slots[idx] = KeySlot {
                    id: cap.0,
                    legend: cap.1,
                    rect: [x, y, w, ROW_H],
                };
                idx += 1;
            }
            u_acc += cap.2;
        }
    }
    debug_assert_eq!(idx, KEY_COUNT);
    slots
}

pub(crate) fn ember(p: Paint) -> Color {
    rune_theme_opencode::ember(p.k.dark, 9)
}

/// How [`keyboard_lit`] lights the deck. Every amount is `0..=1`.
pub(crate) struct Lighting<'a> {
    /// How far the deck has arrived.
    pub(crate) enter: f32,
    /// How bright every key that isn't lit is, as the eye sees it.
    pub(crate) level: f32,
    /// How far each key is pressed.
    pub(crate) depth: &'a dyn Fn(&str) -> f32,
    /// How warmly each key is lit.
    pub(crate) glow: &'a dyn Fn(&str) -> f32,
    /// How red each key flashes.
    pub(crate) flash: &'a dyn Fn(&str) -> f32,
    /// How far each unlit key, by its `[x, y, w, h]` on the deck, is taken
    /// out entirely, such as behind the HUD.
    pub(crate) hollow: &'a dyn Fn([f32; 4]) -> f32,
}

/// Renders the crisp 2D 14.5u Apple Magic Keyboard `El`.
pub(crate) fn keyboard_lit(p: Paint, key: &str, light: Lighting<'_>) -> El {
    let k = p.k;
    let c = &k.c;
    let m = KeyMaterial::new(k);
    let bg = c.bg_deep;
    let warm = ember(p);
    let level = light.level.clamp(0.0, 1.0);
    let enter = light.enter.clamp(0.0, 1.0);
    // Colours mix in linear light, so a level as the eye sees it needs a
    // curve: toward a dark ground less ink, toward a light one more.
    let seen = |v: f32| {
        let v = v.clamp(0.0, 1.0);
        if k.dark {
            v * v
        } else {
            1.0 - (1.0 - v) * (1.0 - v)
        }
    };

    let deck_fill = if k.dark {
        c.layer_01.mix(c.layer_02, 0.55)
    } else {
        c.layer_02.mix(c.border_weak, 0.35)
    };
    let deck_rim = if k.dark {
        c.border_base.mix(c.white, 0.05)
    } else {
        c.border_base
    };
    let well_fill = if k.dark {
        c.bg_deep
    } else {
        c.layer_02.mix(c.border_base, 0.28)
    };
    let cap_base = if k.dark {
        c.surface_raised.mix(c.layer_02, 0.32)
    } else {
        c.white.mix(c.bg_base, 0.06)
    };
    // A pressed key lights up rather than sinking into the dark, so it reads
    // in peripheral vision.
    let cap_pressed = if k.dark {
        c.surface_raised.mix(c.border_strong, 0.55)
    } else {
        c.layer_02.mix(c.border_base, 0.6)
    };
    let cap_rim = if k.dark {
        c.border_weak.mix(c.white, 0.05)
    } else {
        c.border_weak
    };

    let slots = key_slots();
    let mut key_els = Vec::with_capacity(KEY_COUNT);

    for slot in slots.iter() {
        let [x, y, w, h] = slot.rect;
        // Gentle horizontal-diagonal sweep as the deck arrives (`enter: 0 -> 1`).
        let diag = ((x / DECK_W) * 0.72 + (y / DECK_H) * 0.28).clamp(0.0, 1.0);
        let rise = if enter >= 0.999 {
            1.0
        } else {
            let local = ((enter * 1.32 - diag * 0.32) / 1.0).clamp(0.0, 1.0);
            local * local * (3.0 - 2.0 * local)
        };
        let down = (light.depth)(slot.id).clamp(0.0, 1.0);
        let lit = (light.glow)(slot.id).clamp(0.0, 1.0);
        let red = (light.flash)(slot.id).clamp(0.0, 1.0);

        let base = level * (1.0 - (light.hollow)(slot.rect).clamp(0.0, 1.0));
        let vis = seen((base + (1.0 - base) * lit).max(red)) * rise;
        let lit_cap = warm.mix(c.white, if k.dark { 0.30 } else { 0.42 });
        let fill_raw = cap_base
            .mix(cap_pressed, down)
            .mix(lit_cap, lit * if k.dark { 0.72 } else { 0.62 })
            .mix(c.bg_deep.mix(c.danger.fg, 0.30), red);
        let fill = bg.mix(fill_raw, vis);
        let rim = cap_rim
            .mix(c.border_strong, down)
            .mix(lit_cap, lit * 0.82)
            .mix(c.danger.fg, red);
        let border = bg.mix(rim, vis);

        let press_dy = 2.2 * down + (1.0 - rise) * 1.5;
        let radius = if slot.id == "up" || slot.id == "down" {
            3.5
        } else {
            5.0
        };

        let drop_alpha = (if k.dark { 0.42 } else { 0.10 }) * (1.0 - 0.75 * down) * vis;
        let sheen_alpha = (if k.dark { 0.06 } else { 0.55 }) * (1.0 - 0.6 * down) * vis;
        let mut cap_el = el()
            .key(format!("cap-{}", slot.id))
            .absolute(x, y + press_dy)
            .size(w, h)
            .radius(radius)
            .continuous_corners()
            .fill(fill)
            .border(p.hair, border)
            .box_shadow(Shadow {
                x: 0.0,
                y: 1.0 * (1.0 - 0.85 * down),
                blur: 1.8 * (1.0 - 0.5 * down),
                spread: 0.0,
                color: c.bg_deep.alpha(drop_alpha),
                inset: false,
            })
            .box_shadow(Shadow {
                x: 0.0,
                y: 0.5,
                blur: 0.0,
                spread: 0.0,
                color: c.white.alpha(sheen_alpha),
                inset: true,
            })
            .offset_motion(Anim::INSTANT);

        if red > 0.02 {
            cap_el = cap_el.box_shadow(Shadow {
                x: 0.0,
                y: 0.0,
                blur: 10.0,
                spread: 0.0,
                color: c.danger.fg.alpha(0.35 * red * rise),
                inset: false,
            });
        }
        if lit > 0.02 {
            cap_el = cap_el.box_shadow(Shadow {
                x: 0.0,
                y: 1.0,
                blur: 12.0,
                spread: 0.5,
                color: warm.alpha((if k.dark { 0.42 } else { 0.28 }) * lit * rise),
                inset: false,
            });
        }

        if !slot.legend.is_empty() {
            let base_ink = if k.dark {
                c.text_base.mix(c.white, 0.18)
            } else {
                m.ink
            };
            let lit_ink = if k.dark { c.bg_deep } else { c.text_strong };
            let ink = base_ink
                .mix(c.text_strong, down)
                .mix(lit_ink, lit * 0.88)
                .mix(c.danger.fg, red);
            let ink = bg.mix(ink, vis);

            let left_word = matches!(slot.id, "tab" | "caps" | "shift" | "fn");
            let right_word = matches!(slot.id, "delete" | "return" | "shift-right");
            let half_arrow = matches!(slot.id, "up" | "down");

            if left_word || right_word {
                let font = TextStyle::new(8.2, 470.0).lh(10.0);
                let mut r = row()
                    .key("lbl-row")
                    .w_full()
                    .h_full()
                    .px(4.5)
                    .pb(3.0)
                    .items_end();
                if right_word {
                    r = r.justify_end();
                }
                cap_el = cap_el.child(r.child(text(slot.legend).key("lbl").font(font).color(ink)));
            } else {
                let pt = if half_arrow {
                    6.8
                } else if matches!(
                    slot.id,
                    "command" | "command-right" | "option" | "option-right" | "control"
                ) {
                    12.0
                } else {
                    11.2
                };
                let weight = if lit > 0.2 { 600.0 } else { 490.0 };
                let font = TextStyle::new(pt, weight).lh(pt + 2.0);
                cap_el = cap_el
                    .center()
                    .child(text(slot.legend).key("lbl").font(font).color(ink));
            }
        }
        key_els.push(cap_el);
    }

    let deck_vis = (0.12 + 0.88 * seen(level)) * enter;
    el().key(key)
        .size(DECK_W, DECK_H)
        .radius(12.0)
        .continuous_corners()
        .fill(bg.mix(deck_fill, deck_vis))
        .border(p.hair, bg.mix(deck_rim, deck_vis))
        .box_shadow(Shadow {
            x: 0.0,
            y: 6.0 * enter,
            blur: 18.0,
            spread: -2.0,
            color: c
                .bg_deep
                .alpha((if k.dark { 0.42 } else { 0.10 }) * deck_vis),
            inset: false,
        })
        .instant_changes()
        .child(
            el().key("kb-well")
                .absolute(WELL_INSET, WELL_INSET)
                .size(DECK_W - 2.0 * WELL_INSET, DECK_H - 2.0 * WELL_INSET)
                .radius(8.0)
                .continuous_corners()
                .fill(bg.mix(well_fill, deck_vis)),
        )
        .children(key_els)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ansi_14_5u_columns_and_edges_align_to_sub_micron() {
        let slots = key_slots();
        let find = |id: &str| slots.iter().find(|s| s.id == id).unwrap().rect;
        let right_edge = |id: &str| {
            let r = find(id);
            r[0] + r[2]
        };

        // 1. All five rows end on the exact same right edge (`540.0 pt`).
        let rights = ["delete", "\\", "return", "shift-right", "right"].map(right_edge);
        for r in rights {
            assert!((r - rights[0]).abs() < 1e-3, "{rights:?}");
        }

        // 2. Spacebar spans [C.x, M.right] to sub-micron accuracy.
        assert!((find("space")[0] - find("C")[0]).abs() < 1e-3);
        assert!((right_edge("space") - right_edge("M")).abs() < 1e-3);
        assert!((right_edge("command") - right_edge("X")).abs() < 1e-3);

        // 3. Standard ANSI stagger: Q - 1 == 0.5u, A - Q == 0.25u, Z - A == 0.5u.
        let cx = |id: &str| {
            let r = find(id);
            r[0] + 0.5 * r[2]
        };
        assert!(((cx("Q") - cx("1")) - 0.5 * PITCH).abs() < 1e-3);
        assert!(((cx("A") - cx("Q")) - 0.25 * PITCH).abs() < 1e-3);
        assert!(((cx("Z") - cx("A")) - 0.5 * PITCH).abs() < 1e-3);
    }

    #[test]
    fn every_keycap_stays_inside_the_recessed_well() {
        for slot in key_slots() {
            let [x, y, w, h] = slot.rect;
            assert!(
                x >= KEY_INSET - 0.01
                    && y >= KEY_INSET - 0.01
                    && x + w <= DECK_W - KEY_INSET + 0.01
                    && y + h <= DECK_H - KEY_INSET + 0.01,
                "key {} out of well: {:?}",
                slot.id,
                slot.rect
            );
        }
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
