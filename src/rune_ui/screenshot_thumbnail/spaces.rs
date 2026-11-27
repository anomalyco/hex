//! Keeping the screenshot cards on every Space (macOS), as the dictation HUD
//! is kept (`rune_hud::macos`). Rune makes the cards' panel join every Space,
//! but an activation-policy change (the Dock icon) can tie it to one Space
//! while AppKit still reports every Space, and Rune repairs that only as the
//! panel shows; the cards stay shown, so a card taken on one Space stayed
//! there when the user switched to another (#2950). The main thread's pump
//! ticks [`CardSpaces`]: it re-asserts the panel's behaviors once per policy
//! change, and pulls a shown panel found on another Space onto the active
//! one, with `rune_hud::rehome`'s bounded budget.

use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Instant;

use objc2::MainThreadMarker;
use objc2::rc::Retained;
use objc2_app_kit::NSWindow;

use crate::rune_hud::panel_spaces::{apply_behavior, find_panel, rehome_panel};
use crate::rune_hud::rehome;

/// An activation-policy change is waiting for the cards' panel. It outlives
/// the call that made it while the panel does not exist yet.
static ACTIVATION_POLICY_CHANGED: AtomicBool = AtomicBool::new(false);

/// Records that the activation policy changed, so the pump's next tick
/// re-asserts the cards' panel's collection behaviors.
pub(crate) fn activation_policy_changed() {
    ACTIVATION_POLICY_CHANGED.store(true, Ordering::Release);
}

/// The pump's handle on the cards' panel (main thread).
#[derive(Default)]
pub(crate) struct CardSpaces {
    /// The panel and the window number it had when found: Rune makes a new
    /// window, with a new number, when it remakes the overlay.
    panel: Option<(u32, Retained<NSWindow>)>,
    /// This show's re-home attempts; a hide or a panel seen on the active
    /// Space starts the budget over.
    rehome: rehome::Rehome,
}

impl CardSpaces {
    /// One pump tick: repairs after a policy change, then keeps shown cards
    /// on the active Space. Nothing while the Screenshots overlays were never
    /// installed.
    pub(crate) fn tick(&mut self) {
        let Some(mtm) = MainThreadMarker::new() else {
            return;
        };
        let shown = super::SHOWN.load(Ordering::Acquire);
        if ACTIVATION_POLICY_CHANGED.swap(false, Ordering::AcqRel) {
            let Some((number, panel)) = self.panel(mtm) else {
                // Rune makes the panel when it first shows the overlay, so
                // the change waits for it, as the HUD's does. Without the
                // cards (the Lab was off at launch) there is none to wait for.
                if super::CARDS.get().is_some() {
                    ACTIVATION_POLICY_CHANGED.store(true, Ordering::Release);
                }
                return;
            };
            let visible = shown && panel.isVisible();
            match self.rehome.reassert(Instant::now(), visible) {
                rehome::Repair::None => {}
                rehome::Repair::Mask => apply_behavior(&panel),
                rehome::Repair::Rehome => rehome_panel(&panel),
            }
            tracing::info!(
                window = number,
                visible,
                "re-asserted the screenshot cards' collection behaviors after a Dock icon change"
            );
            return;
        }
        if !shown {
            self.rehome.hide();
            return;
        }
        self.keep_on_active_space(mtm);
    }

    /// The cards' panel, found among the app's windows once per Rune window.
    fn panel(&mut self, mtm: MainThreadMarker) -> Option<(u32, Retained<NSWindow>)> {
        let Some(number) = super::CARDS.get().and_then(|cards| cards.window_number()) else {
            self.panel = None;
            return None;
        };
        if let Some((cached, panel)) = &self.panel
            && *cached == number
        {
            return Some((number, panel.clone()));
        }
        let Some(panel) = find_panel(mtm, number) else {
            self.panel = None;
            return None;
        };
        self.panel = Some((number, panel.clone()));
        Some((number, panel))
    }

    /// Pulls shown cards found on another Space onto the active one, as the
    /// HUD's `rehome_on_active_space` does: each attempt is judged a later
    /// tick, for at most [`rehome::MAX_REPAIRS`] attempts per stranding.
    fn keep_on_active_space(&mut self, mtm: MainThreadMarker) {
        let Some((number, panel)) = self.panel(mtm) else {
            return;
        };
        let located = if !panel.isVisible() {
            rehome::Located::Hidden
        } else if panel.isOnActiveSpace() {
            rehome::Located::Active
        } else {
            rehome::Located::Elsewhere
        };
        let decision = self.rehome.decide(Instant::now(), located);
        match decision.confirmed {
            None if decision.repair == rehome::Repair::Rehome => tracing::info!(
                window = number,
                "the screenshot cards are on another Space; re-homing them"
            ),
            None => {}
            Some(rehome::Confirmed::Repaired { attempts }) => tracing::info!(
                window = number,
                attempts,
                "the screenshot cards are on the active Space again after re-homing"
            ),
            Some(rehome::Confirmed::Stranded {
                attempts,
                next: Some(next),
            }) => tracing::info!(
                window = number,
                attempts,
                next_try_ms = next.as_millis() as u64,
                "the screenshot cards are still on another Space; re-homing them again"
            ),
            Some(rehome::Confirmed::Stranded {
                attempts,
                next: None,
            }) => tracing::warn!(
                window = number,
                attempts,
                "the screenshot cards are still on another Space; giving up until they show again"
            ),
        }
        if decision.repair == rehome::Repair::Rehome {
            rehome_panel(&panel);
        }
    }
}
