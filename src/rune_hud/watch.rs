//! The Windows HUD's first-frame watch and its failure backoff: each start
//! opens one attempt, which resolves once and asks for at most one recovery
//! hide; an attempt that presented nothing backs every later show off for a
//! cooldown that doubles while attempts keep failing. It is pure so its
//! decisions are unit-tested on every OS; `HudSender` wires it to Rune only on
//! Windows (#1793, #2175).

#[cfg(any(target_os = "windows", test))]
use std::time::{Duration, Instant};

#[cfg(target_os = "windows")]
use crate::app_settings::IndicatorPosition;

/// What Windows checks of the HUD: Rune can show the overlay without Windows
/// ever drawing it (#510), so a show must also present a frame.
#[cfg(target_os = "windows")]
#[derive(Default)]
pub(super) struct Watch {
    pub(super) position: IndicatorPosition,
    /// The overlay failure last logged, so each is logged once.
    pub(super) logged: Option<String>,
    pub(super) described: bool,
    pub(super) first_frame: FirstFrame,
}

/// The first-frame watch behind a Windows HUD show: each start opens one
/// attempt, which resolves once and asks for at most one recovery hide. A
/// resolved attempt that presented no frame starts (or lengthens) a show
/// backoff, so a HUD that can't show is not rebuilt on every dictation.
#[cfg(any(target_os = "windows", test))]
#[derive(Default)]
pub(super) struct FirstFrame {
    /// The attempt a start began: when, and the presents before it.
    attempt: Option<Attempt>,
    /// How many attempts have begun. The newest one's number tags its
    /// recovery hide, so a hide decided for an older attempt never runs against
    /// a HUD a later start showed (#1793).
    attempts: u64,
    /// The last attempt that presented nothing: why, for Settings and the log.
    unpresented: Option<String>,
    /// This attempt asked for its recovery hide.
    released: bool,
    /// This attempt already presented nothing: its recording gets no further
    /// show, so a failed HUD is not rebuilt mid-dictation (#2175).
    failed: bool,
    /// Consecutive attempts that presented nothing, for the backoff.
    failures: u32,
    /// No show begins before this while the HUD backs off a failed show.
    backoff_until: Option<Instant>,
}

#[cfg(any(target_os = "windows", test))]
struct Attempt {
    started: Instant,
    presents: u64,
}

/// A recovery hide a [`FirstFrame`] poll decided for, and the presents it read.
/// Both are rechecked where the hide lands, because that is a queued update: a
/// frame may have presented since, and a later start may have taken the HUD
/// over (#1793).
#[cfg(any(target_os = "windows", test))]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) struct HideAttempt {
    /// The attempt this hide was decided for, as `FirstFrame::attempts` numbered
    /// it.
    pub(super) attempt: u64,
    /// `surface_stats().totals.presents` when the poll resolved.
    presents: u64,
}

/// What one poll of a [`FirstFrame`] found.
#[cfg(any(target_os = "windows", test))]
pub(super) struct FirstFrameCheck {
    /// Why the HUD can't show, for Settings and the log, when it can't.
    pub(super) reason: Option<String>,
    /// This attempt's one recovery hide is due:
    /// [`FirstFrame::should_apply_hide`] says whether it still applies where it
    /// lands.
    pub(super) hide: Option<HideAttempt>,
    /// The show backoff a frame-less attempt just started, with its cooldown,
    /// for the one log line that says so.
    pub(super) backoff: Option<Duration>,
}

/// How long a started HUD has to present its first frame.
#[cfg(any(target_os = "windows", test))]
pub(super) const FIRST_FRAME: Duration = Duration::from_millis(1500);

/// How long a failed HUD show waits before the HUD is shown again.
#[cfg(any(target_os = "windows", test))]
pub(super) const HUD_BACKOFF: Duration = Duration::from_secs(60);

/// The longest a failed HUD show waits, however many fail in a row.
#[cfg(any(target_os = "windows", test))]
pub(super) const HUD_BACKOFF_MAX: Duration = Duration::from_secs(30 * 60);

/// The backoff after `failures` consecutive frame-less attempts: one minute,
/// doubling, up to [`HUD_BACKOFF_MAX`].
#[cfg(any(target_os = "windows", test))]
pub(super) fn backoff_after(failures: u32) -> Duration {
    HUD_BACKOFF
        .saturating_mul(1 << failures.saturating_sub(1).min(5))
        .min(HUD_BACKOFF_MAX)
}

#[cfg(any(target_os = "windows", test))]
impl FirstFrame {
    /// A non-Hidden start begins a fresh attempt, unless the HUD is backing off
    /// a failed show: then the start shows nothing and returns false, so a HUD
    /// that can't show is not rebuilt on every dictation (#2175). An attempt
    /// that begins has its own present baseline and clock, its own hide, and no
    /// inherited no-frame decision. A second start while the last attempt still
    /// runs restarts it.
    pub(super) fn start(&mut self, at: Instant, presents: u64) -> bool {
        if self.backing_off(at) {
            return false;
        }
        self.attempts += 1;
        self.attempt = Some(Attempt {
            started: at,
            presents,
        });
        self.released = false;
        self.failed = false;
        true
    }

    /// Whether the HUD is backing off a failed show at `now`: no window,
    /// swapchain or device is made until it ends.
    pub(super) fn backing_off(&self, now: Instant) -> bool {
        self.backoff_until.is_some_and(|until| now < until)
    }

    /// Whether this attempt already presented nothing, so its recording gets no
    /// further show.
    pub(super) fn failed(&self) -> bool {
        self.failed
    }

    /// How many attempts in a row presented nothing, for the backoff's log.
    pub(super) fn failed_attempts(&self) -> u32 {
        self.failures
    }

    /// Whether a recovery hide still applies where it lands: no start has opened
    /// a later attempt since the poll (that start's `show` owns the HUD now), and
    /// no frame has presented since it (the HUD is drawing, only late). Either
    /// one means the hide would take away a HUD that is up or on its way
    /// (#1793).
    pub(super) fn should_apply_hide(&self, hide: HideAttempt, presents: u64) -> bool {
        hide.attempt == self.attempts && hide.presents == presents
    }

    /// One poll: resolves the attempt once `FIRST_FRAME` has passed, then
    /// decides its one hide and, when it presented nothing, the show backoff.
    /// Only this attempt's own frame-less resolve hides: Rune's overlay failure
    /// is a reason for Settings and the log, never a hide of its own, since a
    /// failed overlay presents nothing and the frame-less resolve covers it. A
    /// previous attempt's `unpresented` reason is still returned for Settings
    /// and the log, but never hides anything. `surface` names what draws the
    /// HUD, for the reason.
    pub(super) fn poll(
        &mut self,
        now: Instant,
        presents: u64,
        failure: Option<&str>,
        surface: Option<&str>,
    ) -> FirstFrameCheck {
        let resolved = self
            .attempt
            .as_ref()
            .is_some_and(|attempt| now.saturating_duration_since(attempt.started) >= FIRST_FRAME);
        let attempt = if resolved { self.attempt.take() } else { None };
        let mut presented_nothing = false;
        let mut backoff = None;
        if let Some(attempt) = attempt {
            presented_nothing = presents == attempt.presents;
            self.failed = presented_nothing;
            self.unpresented = presented_nothing.then(|| {
                format!(
                    "shown but presented no frame in {} ms; surface: {}",
                    FIRST_FRAME.as_millis(),
                    surface.unwrap_or("none")
                )
            });
            if presented_nothing {
                self.failures = self.failures.saturating_add(1);
                let cooldown = backoff_after(self.failures);
                self.backoff_until = Some(now + cooldown);
                backoff = Some(cooldown);
            } else {
                // A frame presented: the HUD works again, so the next failure
                // starts from the first cooldown.
                self.failures = 0;
                self.backoff_until = None;
            }
        }
        let hide = (!self.released && presented_nothing).then_some(HideAttempt {
            attempt: self.attempts,
            presents,
        });
        if hide.is_some() {
            self.released = true;
        }
        FirstFrameCheck {
            reason: failure
                .map(str::to_owned)
                .or_else(|| self.unpresented.clone()),
            hide,
            backoff,
        }
    }
}
