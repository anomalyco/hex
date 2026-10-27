//! The capsule's timings and springs in one place. The
//! app reads [`DEFAULT`]; the playground (`hex capsule-playground`) changes
//! them live in its own process and prints them back as the constants.

use std::sync::RwLock;

use rune::Spring;

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
    /// The walls between phases.
    pub(crate) island: Spring,
    /// The shell's fade and scale on entry.
    pub(crate) island_scale: Spring,
    pub(crate) reveal_open: Spring,
    pub(crate) reveal_close: Spring,
    pub(crate) switch_width: Spring,
    pub(crate) switch_slide: Spring,
    /// The selection highlight trailing a mode change.
    pub(crate) highlight: Spring,
    /// Content leaves on the SwiftUI capsule's exit, 0.20 s × 0.85.
    pub(crate) presence_exit: Spring,
    /// Narrowing, the walls start this long after the content.
    pub(crate) shell_wait_s: f32,
    /// Between entering chips.
    pub(crate) stagger_s: f32,
    /// How long content takes to fade in (× 1.6 for its spring).
    pub(crate) reveal_enter_s: f32,
}

pub(crate) const DEFAULT: Tuning = Tuning {
    undo_s: 5.0,
    done_hold_s: 25.0,
    brief_hold_s: 4.0,
    chips_open_s: 3.5,
    island: Spring::new(0.32, 0.0),
    island_scale: Spring::new(0.22, 0.0),
    reveal_open: Spring::new(0.36, 0.16),
    reveal_close: Spring::new(0.32, 0.08),
    switch_width: Spring::new(0.30, 0.0),
    switch_slide: Spring::new(0.36, 0.0),
    highlight: Spring::new(0.25, 0.0),
    presence_exit: Spring::new(0.20 * 0.85, 0.0),
    shell_wait_s: 0.06,
    stagger_s: 0.008,
    reveal_enter_s: 0.16,
};

static CURRENT: RwLock<Tuning> = RwLock::new(DEFAULT);

/// The values in use: [`DEFAULT`] unless the playground changed them.
pub(crate) fn get() -> Tuning {
    *CURRENT.read().unwrap_or_else(|e| e.into_inner())
}

pub(crate) fn set(tuning: Tuning) {
    *CURRENT.write().unwrap_or_else(|e| e.into_inner()) = tuning;
}

impl Tuning {
    /// The values as `DEFAULT`'s Rust, to paste over it.
    pub(crate) fn to_rust(self) -> String {
        let spring = |s: Spring| format!("Spring::new({:.3}, {:.3})", s.duration, s.bounce);
        format!(
            "// src/rune_ui/capture_capsule/tuning.rs\n\
             pub(crate) const DEFAULT: Tuning = Tuning {{\n\
             \x20   undo_s: {:.2},\n\
             \x20   done_hold_s: {:.2},\n\
             \x20   brief_hold_s: {:.2},\n\
             \x20   chips_open_s: {:.2},\n\
             \x20   island: {},\n\
             \x20   island_scale: {},\n\
             \x20   reveal_open: {},\n\
             \x20   reveal_close: {},\n\
             \x20   switch_width: {},\n\
             \x20   switch_slide: {},\n\
             \x20   highlight: {},\n\
             \x20   presence_exit: {},\n\
             \x20   shell_wait_s: {:.3},\n\
             \x20   stagger_s: {:.3},\n\
             \x20   reveal_enter_s: {:.3},\n\
             }};\n",
            self.undo_s,
            self.done_hold_s,
            self.brief_hold_s,
            self.chips_open_s,
            spring(self.island),
            spring(self.island_scale),
            spring(self.reveal_open),
            spring(self.reveal_close),
            spring(self.switch_width),
            spring(self.switch_slide),
            spring(self.highlight),
            spring(self.presence_exit),
            self.shell_wait_s,
            self.stagger_s,
            self.reveal_enter_s,
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_copied_values_name_every_field() {
        let rust = DEFAULT.to_rust();
        for field in [
            "undo_s: 5.00",
            "island: Spring::new(0.320, 0.000)",
            "reveal_open: Spring::new(0.360, 0.160)",
            "reveal_enter_s: 0.160",
        ] {
            assert!(rust.contains(field), "{field} missing from\n{rust}");
        }
    }
}
