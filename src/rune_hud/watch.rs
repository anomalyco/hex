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

/// Consecutive failure count and reason carried across a retry after its
/// backoff expires, until the new attempt resolves.
#[cfg(any(target_os = "windows", test))]
#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) struct PriorFailure {
    failures: u32,
    reason: String,
}

/// The first-frame watch behind a Windows HUD show: each start opens one
/// attempt, which resolves once and asks for at most one recovery hide. A
/// resolved attempt that presented no frame starts (or lengthens) a show
/// backoff, so a HUD that can't show is not rebuilt on every dictation.
#[cfg(any(target_os = "windows", test))]
#[derive(Debug, Default, PartialEq, Eq)]
pub(super) enum FirstFrame {
    /// No start has opened an attempt yet.
    #[default]
    Idle,
    /// A show is in flight, waiting up to [`FIRST_FRAME`] for its first present.
    /// `attempt` tags any recovery hide so a hide decided for an older attempt
    /// never runs against a HUD a later start showed (#1793).
    Waiting {
        attempt: u64,
        started: Instant,
        presents: u64,
        /// Carried from an earlier failed attempt until this one resolves.
        prior: Option<PriorFailure>,
    },
    /// The last attempt presented a frame; consecutive failures are reset.
    Presented { attempt: u64 },
    /// The last attempt presented nothing and issued its one recovery hide: its
    /// recording gets no further show (#2175), and later starts back off until
    /// `until`.
    Failed {
        attempt: u64,
        failures: u32,
        until: Instant,
        reason: String,
    },
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
        let (attempt, prior) = match std::mem::take(self) {
            Self::Idle => (1, None),
            Self::Waiting { attempt, prior, .. } => (attempt + 1, prior),
            Self::Presented { attempt } => (attempt + 1, None),
            Self::Failed {
                attempt,
                failures,
                reason,
                ..
            } => (attempt + 1, Some(PriorFailure { failures, reason })),
        };
        *self = Self::Waiting {
            attempt,
            started: at,
            presents,
            prior,
        };
        true
    }

    /// Whether the HUD is backing off a failed show at `now`: no window,
    /// swapchain or device is made until it ends.
    pub(super) fn backing_off(&self, now: Instant) -> bool {
        matches!(self, Self::Failed { until, .. } if now < *until)
    }

    /// Whether this attempt already presented nothing, so its recording gets no
    /// further show.
    pub(super) fn failed(&self) -> bool {
        matches!(self, Self::Failed { .. })
    }

    /// How many attempts in a row presented nothing, for the backoff's log.
    pub(super) fn failed_attempts(&self) -> u32 {
        match self {
            Self::Failed { failures, .. } => *failures,
            Self::Waiting {
                prior: Some(prior), ..
            } => prior.failures,
            Self::Idle | Self::Waiting { prior: None, .. } | Self::Presented { .. } => 0,
        }
    }

    fn current_attempt(&self) -> Option<u64> {
        match self {
            Self::Idle => None,
            Self::Waiting { attempt, .. }
            | Self::Presented { attempt }
            | Self::Failed { attempt, .. } => Some(*attempt),
        }
    }

    fn unpresented(&self) -> Option<&str> {
        match self {
            Self::Failed { reason, .. } => Some(reason.as_str()),
            Self::Waiting {
                prior: Some(prior), ..
            } => Some(prior.reason.as_str()),
            Self::Idle | Self::Waiting { prior: None, .. } | Self::Presented { .. } => None,
        }
    }

    /// Whether a recovery hide still applies where it lands: no start has opened
    /// a later attempt since the poll (that start's `show` owns the HUD now), and
    /// no frame has presented since it (the HUD is drawing, only late). Either
    /// one means the hide would take away a HUD that is up or on its way
    /// (#1793).
    pub(super) fn should_apply_hide(&self, hide: HideAttempt, presents: u64) -> bool {
        self.current_attempt() == Some(hide.attempt) && hide.presents == presents
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
        let ready = matches!(
            self,
            Self::Waiting { started, .. } if now.saturating_duration_since(*started) >= FIRST_FRAME
        );
        if ready {
            let Self::Waiting {
                attempt,
                presents: baseline,
                prior,
                ..
            } = std::mem::take(self)
            else {
                unreachable!();
            };
            if presents == baseline {
                let failures = prior.map_or(0, |prior| prior.failures).saturating_add(1);
                let cooldown = backoff_after(failures);
                let reason = format!(
                    "shown but presented no frame in {} ms; surface: {}",
                    FIRST_FRAME.as_millis(),
                    surface.unwrap_or("none")
                );
                *self = Self::Failed {
                    attempt,
                    failures,
                    until: now + cooldown,
                    reason: reason.clone(),
                };
                return FirstFrameCheck {
                    reason: failure.map(str::to_owned).or(Some(reason)),
                    hide: Some(HideAttempt { attempt, presents }),
                    backoff: Some(cooldown),
                };
            }
            // A frame presented: the HUD works again, so the next failure
            // starts from the first cooldown.
            *self = Self::Presented { attempt };
            return FirstFrameCheck {
                reason: failure.map(str::to_owned),
                hide: None,
                backoff: None,
            };
        }
        FirstFrameCheck {
            reason: failure
                .map(str::to_owned)
                .or_else(|| self.unpresented().map(str::to_owned)),
            hide: None,
            backoff: None,
        }
    }
}
