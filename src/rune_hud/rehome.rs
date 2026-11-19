//! Keeping the HUD's panel on every Space: when a stranded panel is re-homed,
//! when an attempt counts as failed, and when the budget starts over. It knows
//! nothing of AppKit, so every OS unit-tests it; [`super::macos`] supplies the
//! panel and what it says about the active Space.

use std::time::{Duration, Instant};

/// The most re-homes one stranding gets. The window server applies a
/// collection-behavior change asynchronously, so an attempt counts only once a
/// later tick still finds the panel on another Space; but a repair that never
/// takes (or a Space AppKit keeps reporting wrongly) must not loop.
pub(crate) const MAX_REPAIRS: u32 = 8;

/// How long the first re-home gets to reach the window server before a later
/// tick judges it. Each failed attempt doubles the next one's wait.
pub(crate) const REPAIR_SPACING: Duration = Duration::from_millis(150);

/// The longest wait between re-homes, so the backoff stays a few seconds.
pub(crate) const MAX_REPAIR_WAIT: Duration = Duration::from_secs(3);

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

/// What a later tick learned about the re-homes already sent.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Confirmed {
    /// Seen on the active Space after this many attempts.
    Repaired { attempts: u32 },
    /// Still on another Space after this many attempts. `next` is how long the
    /// attempt this tick sends gets, or `None` once the budget is spent.
    Stranded {
        attempts: u32,
        next: Option<Duration>,
    },
}

/// One tick's decision, and what it confirmed about earlier attempts.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct Decision {
    pub(crate) repair: Repair,
    pub(crate) confirmed: Option<Confirmed>,
}

impl Decision {
    fn new(repair: Repair, confirmed: Option<Confirmed>) -> Self {
        Self { repair, confirmed }
    }
}

/// One shown HUD's re-home budget.
#[derive(Debug, Default)]
pub(crate) struct Rehome {
    /// Attempts a later tick found had not taken.
    failed: u32,
    /// When the attempt not yet judged was sent.
    sent: Option<Instant>,
}

impl Rehome {
    /// What this tick should do. A panel visibly on another Space is re-homed,
    /// and that attempt counts only once a tick [`wait`] later still finds it
    /// there; that tick sends the next attempt, whose wait is twice as long, up
    /// to [`MAX_REPAIR_WAIT`], for at most [`MAX_REPAIRS`] attempts. A panel
    /// seen on the active Space settles the stranding, so a later one gets a
    /// fresh budget. A panel Rune has not shown yet leaves the budget as it
    /// is: a hidden panel that has only been ordered out must not reset it, or
    /// a repair that never took would loop.
    pub(crate) fn decide(&mut self, now: Instant, located: Located) -> Decision {
        match located {
            Located::Hidden => Decision::new(Repair::None, None),
            Located::Active => {
                let attempts = self.failed + u32::from(self.sent.is_some());
                self.settle();
                let confirmed = (attempts > 0).then_some(Confirmed::Repaired { attempts });
                Decision::new(Repair::None, confirmed)
            }
            Located::Elsewhere => self.stranded(now),
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
        self.sent = Some(now);
        Repair::Rehome
    }

    /// The HUD hid: its next show starts a fresh budget.
    pub(crate) fn hide(&mut self) {
        self.settle();
    }

    fn stranded(&mut self, now: Instant) -> Decision {
        let Some(sent) = self.sent else {
            if self.failed > 0 {
                // Spent: wait for the panel to settle or the HUD to hide.
                return Decision::new(Repair::None, None);
            }
            self.sent = Some(now);
            return Decision::new(Repair::Rehome, None);
        };
        if now.saturating_duration_since(sent) < wait(self.failed) {
            // The window server may not have applied it yet.
            return Decision::new(Repair::None, None);
        }
        self.failed += 1;
        if self.failed >= MAX_REPAIRS {
            self.sent = None;
            let confirmed = Confirmed::Stranded {
                attempts: self.failed,
                next: None,
            };
            return Decision::new(Repair::None, Some(confirmed));
        }
        self.sent = Some(now);
        let confirmed = Confirmed::Stranded {
            attempts: self.failed,
            next: Some(wait(self.failed)),
        };
        Decision::new(Repair::Rehome, Some(confirmed))
    }

    fn settle(&mut self) {
        self.failed = 0;
        self.sent = None;
    }
}

/// How long an attempt sent after `failed` failures gets before it is judged:
/// [`REPAIR_SPACING`], doubled per failure, capped at [`MAX_REPAIR_WAIT`].
pub(crate) fn wait(failed: u32) -> Duration {
    REPAIR_SPACING
        .checked_mul(1 << failed.min(16))
        .map_or(MAX_REPAIR_WAIT, |wait| wait.min(MAX_REPAIR_WAIT))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn repair(rehome: &mut Rehome, now: Instant, located: Located) -> Repair {
        rehome.decide(now, located).repair
    }

    /// Strands the panel until the budget is spent; returns when it was.
    fn exhaust(rehome: &mut Rehome, start: Instant) -> Instant {
        let mut now = start;
        assert_eq!(repair(rehome, now, Located::Elsewhere), Repair::Rehome);
        for failed in 0..MAX_REPAIRS {
            now += wait(failed);
            rehome.decide(now, Located::Elsewhere);
        }
        assert_eq!(rehome.sent, None, "the budget is spent");
        now
    }

    #[test]
    fn the_wait_doubles_and_caps_at_a_few_seconds() {
        let waits: Vec<_> = (0..8).map(|failed| wait(failed).as_millis()).collect();
        assert_eq!(waits, [150, 300, 600, 1200, 2400, 3000, 3000, 3000]);
        assert_eq!(wait(u32::MAX), MAX_REPAIR_WAIT);
    }

    #[test]
    fn an_unconfirmed_attempt_spends_nothing() {
        let start = Instant::now();
        let mut rehome = Rehome::default();
        assert_eq!(
            repair(&mut rehome, start, Located::Elsewhere),
            Repair::Rehome
        );
        // The window server applies a behavior change asynchronously: ticks
        // before the wait neither repair again nor count the attempt.
        for ms in 1..150 {
            let now = start + Duration::from_millis(ms);
            assert_eq!(
                rehome.decide(now, Located::Elsewhere),
                Decision::new(Repair::None, None)
            );
        }
        assert_eq!(rehome.failed, 0);
        // It took: one attempt, confirmed on the later tick.
        assert_eq!(
            rehome.decide(start + REPAIR_SPACING, Located::Active),
            Decision::new(Repair::None, Some(Confirmed::Repaired { attempts: 1 }))
        );
    }

    #[test]
    fn a_stranding_backs_off_until_its_budget_is_spent() {
        let start = Instant::now();
        let mut rehome = Rehome::default();
        assert_eq!(
            repair(&mut rehome, start, Located::Elsewhere),
            Repair::Rehome
        );
        let mut now = start;
        for failed in 0..MAX_REPAIRS - 1 {
            now += wait(failed);
            assert_eq!(
                repair(
                    &mut rehome,
                    now - Duration::from_millis(1),
                    Located::Elsewhere
                ),
                Repair::None,
                "attempt {} is not judged before its wait",
                failed + 1
            );
            assert_eq!(
                rehome.decide(now, Located::Elsewhere),
                Decision::new(
                    Repair::Rehome,
                    Some(Confirmed::Stranded {
                        attempts: failed + 1,
                        next: Some(wait(failed + 1)),
                    })
                )
            );
        }
        now += wait(MAX_REPAIRS - 1);
        assert_eq!(
            rehome.decide(now, Located::Elsewhere),
            Decision::new(
                Repair::None,
                Some(Confirmed::Stranded {
                    attempts: MAX_REPAIRS,
                    next: None,
                })
            )
        );
        // Bounded: a repair that never takes cannot loop.
        assert_eq!(
            rehome.decide(now + Duration::from_secs(60), Located::Elsewhere),
            Decision::new(Repair::None, None)
        );
    }

    #[test]
    fn being_seen_on_the_active_space_starts_a_fresh_budget() {
        let start = Instant::now();
        let mut rehome = Rehome::default();
        let spent = exhaust(&mut rehome, start);
        assert_eq!(
            rehome.decide(spent + Duration::from_secs(1), Located::Active),
            Decision::new(
                Repair::None,
                Some(Confirmed::Repaired {
                    attempts: MAX_REPAIRS
                })
            )
        );
        // Back where it belongs: the next stranding starts over.
        let again = spent + Duration::from_secs(2);
        assert_eq!(
            repair(&mut rehome, again, Located::Elsewhere),
            Repair::Rehome
        );
        assert_eq!(
            rehome.decide(again + REPAIR_SPACING, Located::Elsewhere),
            Decision::new(
                Repair::Rehome,
                Some(Confirmed::Stranded {
                    attempts: 1,
                    next: Some(wait(1)),
                })
            )
        );
        // A panel that was never stranded settles silently.
        let mut settled = Rehome::default();
        assert_eq!(
            settled.decide(start, Located::Active),
            Decision::new(Repair::None, None)
        );
    }

    #[test]
    fn hiding_starts_a_fresh_budget() {
        let start = Instant::now();
        let mut rehome = Rehome::default();
        let spent = exhaust(&mut rehome, start);
        assert_eq!(repair(&mut rehome, spent, Located::Elsewhere), Repair::None);
        rehome.hide();
        assert_eq!(
            repair(&mut rehome, spent, Located::Elsewhere),
            Repair::Rehome,
            "a new show gets its own attempts"
        );
    }

    #[test]
    fn a_panel_rune_has_not_shown_leaves_the_budget_alone() {
        let start = Instant::now();
        let mut rehome = Rehome::default();
        let spent = exhaust(&mut rehome, start);
        assert_eq!(
            rehome.decide(spent, Located::Hidden),
            Decision::new(Repair::None, None)
        );
        assert_eq!(
            repair(&mut rehome, spent, Located::Elsewhere),
            Repair::None,
            "a hidden panel must not reset a budget that is spent"
        );
        // Nor does it judge an attempt still in flight.
        let mut pending = Rehome::default();
        assert_eq!(
            repair(&mut pending, start, Located::Elsewhere),
            Repair::Rehome
        );
        let later = start + Duration::from_secs(1);
        assert_eq!(
            pending.decide(later, Located::Hidden),
            Decision::new(Repair::None, None)
        );
        assert_eq!(pending.failed, 0);
        assert_eq!(pending.sent, Some(start));
    }

    #[test]
    fn an_activation_policy_change_repairs_at_once() {
        let start = Instant::now();
        let mut rehome = Rehome::default();
        let spent = exhaust(&mut rehome, start);
        // A fresh budget, and no wait: the panel may report the active Space
        // wrongly, so the change repairs whatever the check would say.
        assert_eq!(rehome.reassert(spent, true), Repair::Rehome);
        assert_eq!(
            repair(&mut rehome, spent, Located::Elsewhere),
            Repair::None,
            "the re-assert counts as this stranding's first attempt"
        );
        assert_eq!(
            rehome.decide(spent + REPAIR_SPACING, Located::Elsewhere),
            Decision::new(
                Repair::Rehome,
                Some(Confirmed::Stranded {
                    attempts: 1,
                    next: Some(wait(1)),
                })
            )
        );
        // A hidden panel is only re-applied; ordering it in would undo a hide.
        assert_eq!(rehome.reassert(spent, false), Repair::Mask);
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
            repair(&mut rehome, start, Located::Elsewhere),
            Repair::Rehome,
            "only a reorder counts as a repair attempt"
        );
    }
}
