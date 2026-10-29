//! A Mac keyboard drawn behind the try screen, its keys sinking as
//! they're pressed. Keys use the shortcut keycaps' material; the caller
//! says how far each key is down, so a typing script or a held shortcut
//! plays on it frame by frame.

use rune::{Anim, Color, El, Shadow, el, text};

use super::super::controls::{KeyMaterial, Paint};

/// One key: its id, its legend, and how many key widths it spans.
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
        Cap("caps", "caps lock", 1.75),
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

const UNIT: f32 = 34.0;
const GAP: f32 = 5.0;
const KEY_H: f32 = 30.0;
const ROW_GAP: f32 = 5.0;
const SKIRT: f32 = 2.5;
const RIM: f32 = 0.5;
const RADIUS: f32 = 5.0;
const LEGEND: rune::TextStyle = rune::TextStyle::new(11.0, 450.0).lh(14.0);
const LETTER: rune::TextStyle = rune::TextStyle::new(12.0, 450.0).lh(14.0);

/// The keyboard's size in points.
pub(crate) fn size() -> (f32, f32) {
    let w = 14.5 * (UNIT + GAP) - GAP;
    let h = ROWS.len() as f32 * (KEY_H + ROW_GAP) - ROW_GAP;
    (w, h)
}

/// The id of the key that types the lowercase `letter`.
pub(crate) fn letter(letter: char) -> Option<&'static str> {
    let upper = letter.to_ascii_uppercase();
    ROWS.iter()
        .flat_map(|row| row.iter())
        .find(|cap| cap.0.len() == 1 && cap.0.starts_with(upper))
        .map(|cap| cap.0)
}

/// The key a shortcut keycap names (`⌥`, `R⌘`, `fn`), if it's one of the
/// modifiers drawn.
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

/// Every key with its rect `[x, y, w, h]` in the keyboard's points.
fn rects() -> impl Iterator<Item = (&'static Cap, [f32; 4])> {
    ROWS.iter().enumerate().flat_map(|(r, row)| {
        let y = r as f32 * (KEY_H + ROW_GAP);
        let mut x = 0.0;
        row.iter().map(move |cap| {
            let w = cap.2 * (UNIT + GAP) - GAP;
            let rect = [x, y, w, KEY_H];
            x += w + GAP;
            (cap, rect)
        })
    })
}

/// Where `id` sits, `[x, y, w, h]` in the keyboard's points.
#[cfg(test)]
fn rect(id: &str) -> Option<[f32; 4]> {
    rects().find(|(cap, _)| cap.0 == id).map(|(_, r)| r)
}

/// The keyboard, each key `depth(id)` down (0 up, 1 pressed) and
/// `glow(id)` lit (0 to 1), for the key a shortcut holds. Unlit keys fade
/// to `dim`, so a lit one stands out.
pub(crate) fn keyboard(
    p: Paint,
    key: &str,
    dim: f32,
    depth: impl Fn(&str) -> f32,
    glow: impl Fn(&str) -> f32,
) -> El {
    let (w, h) = size();
    let mut board = el().key(key).size(w, h).instant_changes();
    for (cap, [x, y, cw, ch]) in rects() {
        if cap.0 == "updown" {
            // Two half-height arrows share one key's place.
            let half = (ch - 1.0) / 2.0;
            board = board
                .child(keycap(p, "up", "↑", [x, y, cw, half], depth("up"), 0.0).opacity(dim))
                .child(
                    keycap(
                        p,
                        "down",
                        "↓",
                        [x, y + half + 1.0, cw, half],
                        depth("down"),
                        0.0,
                    )
                    .opacity(dim),
                );
            continue;
        }
        board = board.child(
            keycap(p, cap.0, cap.1, [x, y, cw, ch], depth(cap.0), glow(cap.0))
                .opacity(dim + (1.0 - dim) * glow(cap.0)),
        );
    }
    board
}

/// The glow a held key gives off, from the theme's warm accent.
pub(crate) fn ember(p: Paint) -> Color {
    p.k.c.warning.icon
}

fn keycap(p: Paint, id: &str, legend: &str, [x, y, w, h]: [f32; 4], down: f32, glow: f32) -> El {
    let k = p.k;
    let m = KeyMaterial::new(k);
    let skirt = if h < KEY_H { 1.5 } else { SKIRT };
    let travel = (skirt - RIM) * down;
    let face_h = h - RIM - skirt;
    let font = if legend.chars().count() == 1 && legend.chars().all(char::is_alphanumeric) {
        LETTER
    } else {
        LEGEND
    };
    let ink = m.ink.mix(ember(p), glow * 0.8);
    el().key(id.to_string())
        .absolute(x, y + travel)
        .size(w, h - travel)
        .px(RIM)
        .pt(RIM)
        .pb(skirt - travel)
        .radius(RADIUS + RIM)
        .continuous_corners()
        .fill(m.cap_bottom)
        .gradient(m.cap_top, m.cap_bottom)
        .shadow(0.5, 0.5, k.dark_a(m.contact))
        .box_shadow(Shadow {
            y: skirt,
            blur: 2.0 * skirt,
            color: k.dark_a(m.cast * (1.0 - down)),
            ..Shadow::default()
        })
        .box_shadow(Shadow {
            blur: 14.0,
            color: ember(p).alpha(0.55 * glow),
            ..Shadow::default()
        })
        .offset_motion(Anim::INSTANT)
        .child(
            el().key("face")
                .w_full()
                .h(face_h)
                .center()
                .radius(RADIUS)
                .continuous_corners()
                .fill(m.face.mix(ember(p), glow * if k.dark { 0.18 } else { 0.3 }))
                .gradient(k.light(0.03 - 0.03 * down), k.dark_a(0.02 + 0.06 * down))
                .children(
                    (!legend.is_empty())
                        .then(|| text(legend.to_string()).key("legend").font(font).color(ink)),
                ),
        )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_row_is_as_wide_as_the_keyboard() {
        let (w, _) = size();
        for row in ROWS {
            let units: f32 = row.iter().map(|c| c.2).sum();
            assert!((units - 14.5).abs() < 1e-3, "{units}");
        }
        let right = rect("delete").unwrap();
        assert!((right[0] + right[2] - w).abs() < 0.01);
    }

    #[test]
    fn shortcut_keycaps_find_their_keys() {
        assert_eq!(modifier("⌥"), Some("option"));
        assert_eq!(modifier("R⌘"), Some("command-right"));
        assert_eq!(modifier("L⌥"), Some("option"));
        assert_eq!(modifier("A"), None);
    }

    #[test]
    fn letters_find_their_keys() {
        assert_eq!(letter('q'), Some("Q"));
        assert_eq!(rect("option").map(|r| r[1]), Some(4.0 * (KEY_H + ROW_GAP)));
    }
}
