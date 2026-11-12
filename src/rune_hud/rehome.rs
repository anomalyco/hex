//! Keeping the HUD's panel on every Space: how many re-homes one stranding
//! gets, how far apart, and when the budget starts over. It knows nothing of
//! AppKit, so every OS unit-tests it; [`super::macos`] supplies the panel and
//! what it says about the active Space.

use std::time::{Duration, Instant};

/// The most re-homes one stranding gets. The window server applies a
/// collection-behavior change asynchronously, but a repair that does not take
/// (or a Space AppKit keeps reporting wrongly) must not loop.
pub(crate) const MAX_REPAIRS: u8 = 3;

/// The gap between re-homes: the previous one has to reach the window server
/// before the next check says whether it took.
pub(crate) const REPAIR_SPACING: Duration = Duration::from_millis(150);

/// Where one pump tick found the HUD's panel. A tick that finds no panel at
/// all (Rune has not made it yet) has nothing to repair and does not get here.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Located {
    /// Found, but not visible: Rune applies the queued show or hide later.
    Hidden,
    /// Visible on the active Space.
    Active,
    /// Visible, but not on the active Space.
    Elsewhere,
}

/// What this tick should do with the panel.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Repair {
    /// Leave it alone.
    None,
    /// Re-apply the collection behaviors without ordering the panel: it is
    /// not visible, so it has no Space to be moved from.
    Mask,
    /// Order the panel out, re-apply the collection behaviors, and order it
    /// back in.
    Rehome,
}

/// One shown HUD's re-home budget.
#[derive(Debug, Default)]
pub(crate) struct Rehome {
    repairs: u8,
    last: Option<Instant>,
}

impl Rehome {
    /// What this tick should do. A panel visibly on another Space is re-homed
    /// at most [`MAX_REPAIRS`] times per stranding, [`REPAIR_SPACING`] apart. A
    /// panel seen on the active Space settles the stranding, so a later one
    /// gets a fresh budget. A panel Rune has not shown yet leaves the budget
    /// as it is: a hidden panel that has only been ordered out must not reset
    /// it, or a repair that never took would loop.
    pub(crate) fn decide(&mut self, now: Instant, located: Located) -> Repair {
        match located {
            Located::Hidden => Repair::None,
            Located::Active => {
                self.settle();
                Repair::None
            }
            Located::Elsewhere if self.due(now) => {
                self.count(now);
                Repair::Rehome
            }
            Located::Elsewhere => Repair::None,
        }
    }

    /// An activation-policy change: repair now, on a fresh budget, whether or
    /// not AppKit reports the panel on the active Space, since the change can
    /// leave that report wrong (#1725). A panel that is not visible only gets
    /// its behaviors re-applied, so it spends no attempt; ordering it in would
    /// undo Rune's hide.
    pub(crate) fn reassert(&mut self, now: Instant, visible: bool) -> Repair {
        self.settle();
        if !visible {
            return Repair::Mask;
        }
        self.count(now);
        Repair::Rehome
    }

    /// The HUD hid: its next show starts a fresh budget.
    pub(crate) fn hide(&mut self) {
        self.settle();
    }

    fn due(&self, now: Instant) -> bool {
        self.repairs < MAX_REPAIRS
            && self
                .last
                .is_none_or(|last| now.saturating_duration_since(last) >= REPAIR_SPACING)
    }

    fn count(&mut self, now: Instant) {
        self.repairs = self.repairs.saturating_add(1);
        self.last = Some(now);
    }

    fn settle(&mut self) {
        self.repairs = 0;
        self.last = None;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_stranding_gets_three_rehomes_at_least_150_ms_apart() {
        let start = Instant::now();
        let mut rehome = Rehome::default();
        assert_eq!(rehome.decide(start, Located::Elsewhere), Repair::Rehome);
        // The window server applies a behavior change asynchronously: the next
        // check waits for the previous repair instead of piling up.
        assert_eq!(
            rehome.decide(
                start + REPAIR_SPACING - Duration::from_millis(1),
                Located::Elsewhere
            ),
            Repair::None
        );
        assert_eq!(
            rehome.decide(start + REPAIR_SPACING, Located::Elsewhere),
            Repair::Rehome
        );
        assert_eq!(
            rehome.decide(start + 2 * REPAIR_SPACING, Located::Elsewhere),
            Repair::Rehome
        );
        // Bounded: a repair that never takes cannot loop.
        assert_eq!(
            rehome.decide(start + 10 * REPAIR_SPACING, Located::Elsewhere),
            Repair::None
        );
    }

    #[test]
    fn being_seen_on_the_active_space_starts_a_fresh_budget() {
        let start = Instant::now();
        let mut rehome = Rehome::default();
        for step in 0..u32::from(MAX_REPAIRS) {
            assert_eq!(
                rehome.decide(start + step * REPAIR_SPACING, Located::Elsewhere),
                Repair::Rehome
            );
        }
        assert_eq!(
            rehome.decide(start + 10 * REPAIR_SPACING, Located::Elsewhere),
            Repair::None
        );
        // Back where it belongs: the next stranding gets three again.
        let active = start + 11 * REPAIR_SPACING;
        assert_eq!(rehome.decide(active, Located::Active), Repair::None);
        assert_eq!(rehome.decide(active, Located::Elsewhere), Repair::Rehome);
        assert_eq!(
            rehome.decide(active + REPAIR_SPACING, Located::Elsewhere),
            Repair::Rehome
        );
        assert_eq!(
            rehome.decide(active + 2 * REPAIR_SPACING, Located::Elsewhere),
            Repair::Rehome
        );
        assert_eq!(
            rehome.decide(active + 3 * REPAIR_SPACING, Located::Elsewhere),
            Repair::None
        );
    }

    #[test]
    fn hiding_starts_a_fresh_budget() {
        let start = Instant::now();
        let mut rehome = Rehome::default();
        for step in 0..u32::from(MAX_REPAIRS) {
            assert_eq!(
                rehome.decide(start + step * REPAIR_SPACING, Located::Elsewhere),
                Repair::Rehome
            );
        }
        assert_eq!(
            rehome.decide(start + 4 * REPAIR_SPACING, Located::Elsewhere),
            Repair::None
        );
        rehome.hide();
        let again = start + 5 * REPAIR_SPACING;
        assert_eq!(
            rehome.decide(again, Located::Elsewhere),
            Repair::Rehome,
            "a new show gets its own attempts"
        );
    }

    #[test]
    fn a_panel_rune_has_not_shown_leaves_the_budget_alone() {
        let start = Instant::now();
        let mut rehome = Rehome::default();
        for step in 0..u32::from(MAX_REPAIRS) {
            assert_eq!(
                rehome.decide(start + step * REPAIR_SPACING, Located::Elsewhere),
                Repair::Rehome
            );
        }
        let later = start + 4 * REPAIR_SPACING;
        assert_eq!(rehome.decide(later, Located::Hidden), Repair::None);
        assert_eq!(
            rehome.decide(later, Located::Elsewhere),
            Repair::None,
            "a hidden panel must not reset a budget that is spent"
        );
    }

    #[test]
    fn an_activation_policy_change_repairs_at_once() {
        let start = Instant::now();
        let mut rehome = Rehome::default();
        for step in 0..u32::from(MAX_REPAIRS) {
            assert_eq!(
                rehome.decide(start + step * REPAIR_SPACING, Located::Elsewhere),
                Repair::Rehome
            );
        }
        // A fresh budget, and no wait: the panel may report the active Space
        // wrongly, so the change repairs whatever the check would say.
        let changed = start + 4 * REPAIR_SPACING;
        assert_eq!(rehome.reassert(changed, true), Repair::Rehome);
        assert_eq!(
            rehome.decide(changed, Located::Elsewhere),
            Repair::None,
            "the re-assert counts as this stranding's first attempt"
        );
        assert_eq!(
            rehome.decide(changed + REPAIR_SPACING, Located::Elsewhere),
            Repair::Rehome
        );
        // A hidden panel is only re-applied; ordering it in would undo a hide.
        assert_eq!(rehome.reassert(changed, false), Repair::Mask);
    }

    #[test]
    fn a_hidden_panels_reassert_spends_no_attempt() {
        let start = Instant::now();
        let mut rehome = Rehome::default();
        for _ in 0..MAX_REPAIRS + 2 {
            assert_eq!(rehome.reassert(start, false), Repair::Mask);
        }
        // Re-applying a hidden panel's mask moves nothing, so it must not use
        // up the budget a later stranding needs.
        assert_eq!(
            rehome.decide(start, Located::Elsewhere),
            Repair::Rehome,
            "only a reorder counts as a repair attempt"
        );
    }
}
