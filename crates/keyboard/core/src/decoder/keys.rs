//! The letter keys taps are read against (`TypingDecoder.Keys`), and the two ways the host's
//! layout becomes them: `Keys::from_layout` (`Keys.init(layout:offsets:)`) and `offsets`
//! (`TypingDecoder.offsets(from:layout:)`). The host passes its layout as data: each key's
//! frame and character, and where a split layout's halves are pushed together.

use std::collections::HashMap;

/// A rectangle with a non-negative width and height, as `CGRect` keeps it.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Rect {
    pub x: f64,
    pub y: f64,
    pub width: f64,
    pub height: f64,
}

impl Rect {
    pub fn min_x(&self) -> f64 {
        self.x
    }

    pub fn max_x(&self) -> f64 {
        self.x + self.width
    }

    pub fn mid_x(&self) -> f64 {
        self.x + self.width * 0.5
    }

    pub fn min_y(&self) -> f64 {
        self.y
    }

    pub fn max_y(&self) -> f64 {
        self.y + self.height
    }

    pub fn mid_y(&self) -> f64 {
        self.y + self.height * 0.5
    }

    /// The smallest rectangle holding both (`CGRect.union`).
    pub fn union(&self, other: &Rect) -> Rect {
        let x = self.min_x().min(other.min_x());
        let y = self.min_y().min(other.min_y());
        let max_x = self.max_x().max(other.max_x());
        let max_y = self.max_y().max(other.max_y());
        Rect {
            x,
            y,
            width: max_x - x,
            height: max_y - y,
        }
    }
}

/// Swift's `max(a, b)`: `b` unless it is less than `a`.
pub(crate) fn swift_max(a: f64, b: f64) -> f64 {
    if b >= a { b } else { a }
}

/// Swift's `min(a, b)`: `b` only when it is less than `a`.
pub(crate) fn swift_min(a: f64, b: f64) -> f64 {
    if b < a { b } else { a }
}

#[derive(Debug, Clone, PartialEq)]
pub struct Keys {
    /// Each letter key's character and centre, shifted by your tap offsets when there are
    /// any. One per letter: a repeated letter's last centre counts.
    pub centres: Vec<(char, f64, f64)>,
    /// Key width plus gap across; row height plus gap down.
    pub pitch: (f64, f64),
    /// The bottom letter row's centre, below which a tap may have been meant for space.
    pub bottom_row_y: f64,
    /// The space bar's keycap as on the unsplit keyboard (both bars on a split one), or
    /// `None` when unknown: a bottom-row tap above it may have been meant for space.
    pub space: Option<Rect>,
}

/// One key of the host's letters page.
#[derive(Debug, Clone, PartialEq)]
pub struct LayoutKey {
    /// What it types (`KeyboardLayout.Key.character`), if anything.
    pub character: Option<String>,
    /// Whether it is a space bar.
    pub space: bool,
    pub frame: Rect,
}

/// Where a split layout's halves meet: x at or past `middle` moves left by `shift` when the
/// halves are pushed together (`KeyboardLayout.unsplitX`).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Split {
    pub middle: f64,
    pub shift: f64,
}

fn unsplit_x(split: Option<Split>, x: f64) -> f64 {
    match split {
        Some(split) if x >= split.middle => x - split.shift,
        _ => x,
    }
}

/// The key's letter when it types exactly one letter.
fn letter(key: &LayoutKey) -> Option<char> {
    let mut chars = crate::decoder::text::characters(key.character.as_deref()?);
    let c = chars.next()?;
    (chars.next().is_none() && c.is_alphabetic()).then_some(c)
}

/// Learn how I type's tap counts and summed offsets for one key, in keycap widths and heights
/// (`TypingJournalKeyStats`).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct KeyStats {
    pub count: u64,
    pub dx: f64,
    pub dy: f64,
}

impl Keys {
    /// The letter keys of a letters page. `pitch` is the layout's (`SplitGeometry.pitch` or
    /// the letter width plus gap across; key height plus row gap down); `offsets` (in points,
    /// by letter) moves a centre to where you tend to tap it.
    pub fn from_layout(
        keys: &[LayoutKey],
        split: Option<Split>,
        pitch: (f64, f64),
        offsets: &HashMap<char, (f64, f64)>,
    ) -> Keys {
        let mut centres: Vec<(char, f64, f64)> = Vec::new();
        let mut bottom = 0.0;
        let mut space: Option<Rect> = None;
        for key in keys.iter().filter(|k| k.space) {
            let min_x = unsplit_x(split, key.frame.min_x());
            let max_x = unsplit_x(split, key.frame.max_x());
            let bar = Rect {
                x: min_x,
                y: key.frame.min_y(),
                width: swift_max(0.0, max_x - min_x),
                height: key.frame.height,
            };
            space = Some(space.map_or(bar, |s| s.union(&bar)));
        }
        for key in keys {
            let Some(letter) = letter(key) else {
                continue;
            };
            let shift = offsets.get(&letter).copied().unwrap_or((0.0, 0.0));
            // A split layout's halves are pushed together, so a letter on both halves (G, V)
            // has one centre, where the unsplit keyboard has it.
            let centre = (
                letter,
                unsplit_x(split, key.frame.mid_x()) + shift.0,
                key.frame.mid_y() + shift.1,
            );
            match centres.iter_mut().find(|c| c.0 == letter) {
                Some(existing) => *existing = centre,
                None => centres.push(centre),
            }
            bottom = swift_max(bottom, key.frame.mid_y());
        }
        Keys {
            centres,
            pitch,
            bottom_row_y: bottom,
            space,
        }
    }
}

/// Learn how I type's average tap offsets (in keycap widths and heights) as points, for
/// letters with 20 taps or more, shrunk toward the centre while there are few. `stats` is by
/// the key's character.
pub fn offsets(keys: &[LayoutKey], stats: &HashMap<String, KeyStats>) -> HashMap<char, (f64, f64)> {
    let mut out = HashMap::new();
    for key in keys {
        let Some(letter) = letter(key) else {
            continue;
        };
        let Some(s) = key.character.as_ref().and_then(|c| stats.get(c)) else {
            continue;
        };
        if s.count < 20 {
            continue;
        }
        let weight = s.count as f64 / (s.count + 20) as f64;
        let mean = (s.dx / s.count as f64, s.dy / s.count as f64);
        out.insert(
            letter,
            (
                mean.0 * key.frame.width * weight,
                mean.1 * key.frame.height * weight,
            ),
        );
    }
    out
}
