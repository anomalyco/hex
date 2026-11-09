//! How `"Without Hex"` and `"With Hex"` are composed on the page: where the
//! conversation, the keyboard and the heading sit, and where the fireworks
//! have room. The run picks one layout ([`Layout::chosen`]), as it picks
//! its scene, so the stage's geometry is known without an `Onboard`; the
//! playground's layout variants set it.
//!
//! The stage keeps its 500 × 540 pt box. A layout moves and scales the
//! conversation and the keyboard as groups ([`Place`]), each about its
//! top-centre, so nothing inside them changes.

use std::sync::atomic::{AtomicU8, Ordering};

use super::cards::{CHAT_W, chat_height};
use super::keyboard;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) enum Layout {
    /// One centre column: heading, the conversation, the keyboard under
    /// it at the same width, `Skip →` under everything.
    #[default]
    Today,
    /// A: a tighter centre. One two-line heading for both scenes, a larger
    /// conversation, the keyboard smaller and tucked under its bottom edge,
    /// `Skip` in the corner.
    Centre,
    /// B: the heading as a left column (the mode, a big title, one line and
    /// the key), the conversation and its keyboard on the right.
    Split,
    /// C: one laptop. The conversation is its screen, the keyboard its
    /// deck, under the two-line heading.
    Laptop,
}

static CHOSEN: AtomicU8 = AtomicU8::new(0);

impl Layout {
    const ALL: [Layout; 4] = [Layout::Today, Layout::Centre, Layout::Split, Layout::Laptop];

    /// The layout this run plays.
    pub(crate) fn chosen() -> Layout {
        Self::ALL[usize::from(CHOSEN.load(Ordering::Relaxed)).min(3)]
    }

    pub(crate) fn choose(self) {
        let i = Self::ALL.iter().position(|&l| l == self).unwrap_or(0);
        CHOSEN.store(i as u8, Ordering::Relaxed);
    }

    /// The conversation, its thread and the words flying into it.
    pub(crate) fn card(self) -> Place {
        match self {
            Layout::Today => Place::IDENTITY,
            Layout::Centre => Place {
                k: 1.12,
                at: [0.0, 4.0],
            },
            Layout::Split => Place {
                k: 1.0,
                at: [0.0, 30.0],
            },
            Layout::Laptop => Place {
                k: 1.0,
                at: [0.0, LAPTOP_BEZEL + 2.0],
            },
        }
    }

    /// The keyboard: its top-centre's offset from where it sits today, and
    /// its scale about that point.
    pub(crate) fn keys(self) -> Place {
        let (_, kh) = keyboard::size();
        let today_top = STAGE_H - kh;
        let card = self.card();
        let card_bottom = card.at[1] + card.k * chat_height(1);
        let (k, top) = match self {
            Layout::Today => return Place::IDENTITY,
            // Tucked under the conversation's bottom edge.
            Layout::Centre => (0.8, card_bottom - 22.0),
            Layout::Split => (0.82, card_bottom - 18.0),
            // On the deck, under the hinge.
            Layout::Laptop => (0.86, self.laptop_lid()[3] + 8.0),
        };
        Place {
            k,
            at: [0.0, top - today_top],
        }
    }

    /// The heading has two lines in both scenes.
    pub(crate) fn two_lines(self) -> bool {
        self != Layout::Today
    }

    /// `Skip` sits in the window's corner rather than under the stage.
    pub(crate) fn corner_skip(self) -> bool {
        self != Layout::Today
    }

    pub(crate) fn split(self) -> bool {
        self == Layout::Split
    }

    /// The laptop's lid around the conversation, in stage points.
    pub(crate) fn laptop_lid(self) -> [f32; 4] {
        let card = Layout::Laptop.card();
        let x0 = (STAGE_W - CHAT_W) * 0.5 - LAPTOP_BEZEL;
        let y0 = card.at[1] - LAPTOP_BEZEL;
        [
            x0,
            y0,
            STAGE_W - x0,
            card.at[1] + chat_height(1) + LAPTOP_BEZEL,
        ]
    }

    /// The conversation's rect on the stage, as drawn.
    pub(crate) fn card_rect(self) -> [f32; 4] {
        let c = self.card();
        let x0 = (STAGE_W - CHAT_W) * 0.5;
        let [l, t] = c.map([x0, 0.0]);
        let [r, b] = c.map([x0 + CHAT_W, chat_height(1)]);
        [l, t, r, b]
    }
}

/// The laptop's bezel around its screen.
pub(crate) const LAPTOP_BEZEL: f32 = 12.0;
pub(crate) const STAGE_W: f32 = 500.0;
pub(crate) const STAGE_H: f32 = 540.0;

/// A group scaled `k` about the stage's top-centre, then moved `at`.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct Place {
    pub(crate) k: f32,
    pub(crate) at: [f32; 2],
}

impl Place {
    pub(crate) const IDENTITY: Place = Place {
        k: 1.0,
        at: [0.0, 0.0],
    };

    pub(crate) fn is_identity(self) -> bool {
        self == Self::IDENTITY
    }

    /// Where a point in the group lands on the stage.
    pub(crate) fn map(self, p: [f32; 2]) -> [f32; 2] {
        let cx = STAGE_W * 0.5;
        [
            cx + self.k * (p[0] - cx) + self.at[0],
            self.k * p[1] + self.at[1],
        ]
    }

    /// The group's point that lands on stage point `p`.
    pub(crate) fn unmap(self, p: [f32; 2]) -> [f32; 2] {
        let cx = STAGE_W * 0.5;
        [
            cx + (p[0] - self.at[0] - cx) / self.k,
            (p[1] - self.at[1]) / self.k,
        ]
    }
}

/// The keyboard's place for a group whose box is the keyboard itself at
/// its stage spot: [`Layout::keys`] pivots on the keyboard's top-centre,
/// which is the stage's centre line.
pub(crate) fn keys_map(place: Place, p: [f32; 2]) -> [f32; 2] {
    let (_, kh) = keyboard::size();
    let top = STAGE_H - kh;
    let cx = STAGE_W * 0.5;
    [
        cx + place.k * (p[0] - cx) + place.at[0],
        top + place.k * (p[1] - top) + place.at[1],
    ]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_layout_keeps_its_parts_on_the_stage_and_apart() {
        let (_, kh) = keyboard::size();
        for layout in Layout::ALL {
            let card = layout.card_rect();
            let keys = layout.keys();
            let top = keys_map(keys, [0.0, STAGE_H - kh])[1];
            let bottom = keys_map(keys, [0.0, STAGE_H])[1];
            assert!(card[1] >= 0.0 && bottom <= STAGE_H + 1.0, "{layout:?}");
            // The keyboard can tuck under the conversation, not cover it.
            assert!(top >= card[3] - 24.0, "{layout:?}");
            let p = [123.0, 45.0];
            let back = layout.card().unmap(layout.card().map(p));
            assert!((back[0] - p[0]).abs() < 1e-3 && (back[1] - p[1]).abs() < 1e-3);
        }
    }
}
