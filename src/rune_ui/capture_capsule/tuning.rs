//! The capsule's timings and springs in one place. The app reads
//! [`DEFAULT`].

use std::sync::RwLock;

use rune::Spring;

use crate::rune_hud::spring::{self as hud, Spec};

/// One of the HUD family's springs as Rune's.
pub(crate) const fn spring(spec: Spec) -> Spring {
    Spring::new(spec.duration, spec.bounce)
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct Tuning {
    /// The Undo window: a send, a rule's or a pick's, can be taken back
    /// for this long after it goes.
    pub(crate) undo_s: f64,
    /// The finished capsule leaves by itself after this long, unless its
    /// list is open.
    pub(crate) done_hold_s: f64,
    /// "Taken back" and errors that need nothing more leave sooner.
    pub(crate) brief_hold_s: f64,
    /// A recording's chips close by themselves this long after a tap.
    pub(crate) chips_open_s: f64,
    /// The walls, and every piece that stays and moves, on every change:
    /// the dictation HUD's morph.
    pub(crate) island: Spring,
    /// The shell's rise from its origin pose on entry and its fall back on
    /// exit; it fades over the same time with no bounce, as the HUD does.
    pub(crate) island_scale: Spring,
    /// The selection highlight trailing a mode change.
    pub(crate) highlight: Spring,
    /// Content coming in: all of it at once, over the walls' move, never
    /// waiting for them or for what leaves.
    pub(crate) content_in: Spring,
    /// Content going, at the same moment, a little quicker.
    pub(crate) content_out: Spring,
    /// How blurred (pt) and how small, toward the capsule's middle, content
    /// is while it comes and goes. Rune keeps blur within 4 pt and scale
    /// within 0.9.
    pub(crate) content_blur: f32,
    pub(crate) content_scale: f32,
}

pub(crate) const DEFAULT: Tuning = Tuning {
    undo_s: 5.0,
    done_hold_s: 25.0,
    brief_hold_s: 4.0,
    chips_open_s: 3.5,
    island: spring(hud::MORPH),
    island_scale: spring(hud::MORPH),
    highlight: Spring::new(0.25, 0.0),
    content_in: Spring::new(0.30, 0.10),
    content_out: Spring::new(0.24, 0.0),
    content_blur: 3.0,
    content_scale: 0.92,
};

static CURRENT: RwLock<Tuning> = RwLock::new(DEFAULT);

/// The values in use: [`DEFAULT`].
pub(crate) fn get() -> Tuning {
    *CURRENT.read().unwrap_or_else(|e| e.into_inner())
}

#[cfg(test)]
pub(crate) fn set(tuning: Tuning) {
    *CURRENT.write().unwrap_or_else(|e| e.into_inner()) = tuning;
}
