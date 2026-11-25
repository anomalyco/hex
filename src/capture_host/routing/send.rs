//! Explicit send, hold, delivery, and undo lifecycle for a routed capture.
//!
//! Replaces the correlated `Option` fields that previously tracked pending
//! timers, holds, destinations, undo handles, offered rules, and saved rules
//! separately. Every valid combination is one variant of [`SendState`], and
//! [`CaptureRoute::delivery`] and [`CaptureRoute::offer`] are projected from it
//! rather than stored beside it.

use std::time::{Duration, Instant};

use scrawl_capture::RuleKind;
use scrawl_capture::model::Destination;

use super::{HOLD_MAX, RESUME_LEFT, RouteDelivery, SEND_HOLD, site_of};

/// A rule saved with a send, and what its key held before.
pub(super) struct SavedRule {
    pub(super) kind: RuleKind,
    pub(super) key: String,
    pub(super) before: Option<String>,
    pub(super) was_refused: bool,
}

/// How to take a send back once OpenCode accepted it.
pub(super) struct Sent {
    pub(super) delivery_id: String,
}

/// The countdown before a saved rule's send goes out.
pub(super) enum Clock {
    Counting { due: Instant },
    Paused { since: Instant, left: Duration },
}

impl Clock {
    pub(super) fn start(now: Instant) -> Self {
        Self::Counting {
            due: now + SEND_HOLD,
        }
    }

    pub(super) fn pause(&mut self, now: Instant) {
        if let Self::Counting { due } = *self {
            *self = Self::Paused {
                since: now,
                left: due.saturating_duration_since(now),
            };
        }
    }

    /// Ends a hold: the deadline moves on by the time held, keeping what was
    /// left (at least [`RESUME_LEFT`], never more than [`SEND_HOLD`]).
    pub(super) fn resume(&mut self, now: Instant) {
        if let Self::Paused { left, .. } = *self {
            *self = Self::Counting {
                due: now + left.clamp(RESUME_LEFT, SEND_HOLD),
            };
        }
    }

    pub(super) fn is_due(&self, now: Instant) -> bool {
        match *self {
            Self::Counting { due } => now >= due,
            Self::Paused { since, .. } => now.saturating_duration_since(since) >= HOLD_MAX,
        }
    }

    #[cfg(test)]
    pub(super) fn due(&self) -> Option<Instant> {
        match *self {
            Self::Counting { due } => Some(due),
            Self::Paused { .. } => None,
        }
    }

    #[cfg(test)]
    pub(super) fn is_paused(&self) -> bool {
        matches!(self, Self::Paused { .. })
    }

    #[cfg(test)]
    pub(super) fn expire_at(&mut self, now: Instant) {
        if let Self::Counting { due } = self {
            *due = now;
        }
    }

    #[cfg(test)]
    pub(super) fn set_paused(&mut self, since: Instant, left: Duration) {
        *self = Self::Paused { since, left };
    }
}

/// What a capsule pick should do given the current [`SendState`].
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) enum PickAction {
    /// Already delivered to this same session: don't double-send.
    AlreadyThere,
    /// Delivered to another session (or queued): take back first, then send.
    Redirect,
    /// Not yet sent (or taken back / failed / un-undoable too-late): send fresh.
    SendFresh,
}

/// What [`SendState::begin_undo`] asks the worker to execute.
pub(super) enum UndoPlan {
    /// OpenCode accepted the send and gave an undo handle: ask the router to
    /// withdraw or revert it.
    Handle {
        session: String,
        to: String,
        sent: Sent,
    },
    /// Delivered later by background retry with no undo handle: already
    /// transitioned to [`SendState::TooLate`].
    RetriedNoHandle,
    /// Queued on disk: already transitioned to [`SendState::TakenBack`]; caller
    /// removes the pending marker and logs the correction.
    Queued,
    /// Nothing to take back.
    None,
}

/// Outcome of a single delivery attempt, before attaching any saved rule.
pub(super) enum DeliverOutcome {
    Delivered {
        session: String,
        to: String,
        undo: Option<Sent>,
        offer: Option<scrawl_capture::OfferRule>,
    },
    Queued {
        session: String,
        to: String,
        why: String,
    },
    Failed {
        to: String,
        why: String,
    },
}

impl DeliverOutcome {
    pub(super) fn into_state(self, saved: Option<SavedRule>) -> SendState {
        match self {
            Self::Delivered {
                session,
                to,
                undo,
                offer,
            } => SendState::Delivered {
                session,
                to,
                undo,
                offer,
                saved,
            },
            Self::Queued { session, to, why } => SendState::Queued {
                session,
                to,
                why,
                saved,
            },
            Self::Failed { to, why } => SendState::Failed { to, why },
        }
    }

    pub(super) fn succeeded_or_queued(&self) -> bool {
        matches!(self, Self::Delivered { .. } | Self::Queued { .. })
    }

    pub(super) fn delivery(&self) -> RouteDelivery {
        match self {
            Self::Delivered { to, .. } => RouteDelivery::Sent { to: to.clone() },
            Self::Queued { to, why, .. } => RouteDelivery::Queued {
                to: to.clone(),
                why: why.clone(),
            },
            Self::Failed { to, why } => RouteDelivery::Failed {
                to: to.clone(),
                why: why.clone(),
            },
        }
    }
}

/// The complete send, countdown, delivery, and undo lifecycle of one routed
/// capture.
pub(super) enum SendState {
    /// No send scheduled or attempted yet.
    Idle,
    /// A saved folder rule matched locally and its Undo countdown started
    /// while background resolution is still finding the session ID.
    ResolvingRule { clock: Clock },
    /// A saved rule has its destination and is counting down its Undo window.
    Scheduled {
        destination: Destination,
        clock: Clock,
    },
    /// Currently posting to OpenCode.
    Sending { to: String },
    /// OpenCode accepted the send (directly with an `undo` handle, or later
    /// via background retry with `undo: None`).
    Delivered {
        session: String,
        to: String,
        undo: Option<Sent>,
        offer: Option<scrawl_capture::OfferRule>,
        saved: Option<SavedRule>,
    },
    /// OpenCode didn't take the send; queued on disk for [`super::Worker::retry`].
    Queued {
        session: String,
        to: String,
        why: String,
        saved: Option<SavedRule>,
    },
    /// Cancelled before sending, withdrawn/reverted/disowned, or unqueued.
    TakenBack,
    /// OpenCode already acted on it, or refused/didn't finish Undo (keeping
    /// `retry_undo` so a later redirect can retry).
    TooLate {
        session: String,
        to: String,
        retry_undo: Option<Sent>,
    },
    /// Delivery failed outright.
    Failed { to: String, why: String },
}

impl SendState {
    pub(super) fn delivery(&self) -> Option<RouteDelivery> {
        match self {
            Self::Idle | Self::ResolvingRule { .. } | Self::Scheduled { .. } => None,
            Self::Sending { to } => Some(RouteDelivery::Sending { to: to.clone() }),
            Self::Delivered { to, .. } => Some(RouteDelivery::Sent { to: to.clone() }),
            Self::Queued { to, why, .. } => Some(RouteDelivery::Queued {
                to: to.clone(),
                why: why.clone(),
            }),
            Self::TakenBack => Some(RouteDelivery::TakenBack),
            Self::TooLate { to, .. } => Some(RouteDelivery::TooLate { to: to.clone() }),
            Self::Failed { to, why } => Some(RouteDelivery::Failed {
                to: to.clone(),
                why: why.clone(),
            }),
        }
    }

    pub(super) fn offer_site(&self) -> Option<String> {
        match self {
            Self::Delivered {
                offer: Some(offer), ..
            } => Some(site_of(&(offer.kind, offer.key.clone()))),
            _ => None,
        }
    }

    pub(super) fn offered(&self) -> Option<&scrawl_capture::OfferRule> {
        match self {
            Self::Delivered { offer, .. } => offer.as_ref(),
            _ => None,
        }
    }

    pub(super) fn has_delivery(&self) -> bool {
        !matches!(
            self,
            Self::Idle | Self::ResolvingRule { .. } | Self::Scheduled { .. }
        )
    }

    /// Whether a saved rule's countdown is active (waiting to resolve or send).
    pub(super) fn is_pending_rule(&self) -> bool {
        matches!(self, Self::ResolvingRule { .. } | Self::Scheduled { .. })
    }

    pub(super) fn hold(&mut self, on: bool, now: Instant) {
        match self {
            Self::ResolvingRule { clock } | Self::Scheduled { clock, .. } => {
                if on {
                    clock.pause(now);
                } else {
                    clock.resume(now);
                }
            }
            _ => {}
        }
    }

    pub(super) fn resume(&mut self, now: Instant) {
        self.hold(false, now);
    }

    /// Schedules a saved rule's send. When `keep_clock` is true (the rule or
    /// its resolving banner was already shown), preserves any running or
    /// paused countdown; otherwise starts a fresh [`SEND_HOLD`] window.
    pub(super) fn schedule(&mut self, destination: Destination, keep_clock: bool, now: Instant) {
        let clock = if keep_clock {
            match std::mem::replace(self, Self::Idle) {
                Self::ResolvingRule { clock } | Self::Scheduled { clock, .. } => clock,
                _ => Clock::start(now),
            }
        } else {
            Clock::start(now)
        };
        *self = Self::Scheduled { destination, clock };
    }

    /// Clears a folder-rule countdown that resolved to no destination.
    pub(super) fn clear_resolving(&mut self) {
        if matches!(self, Self::ResolvingRule { .. }) {
            *self = Self::Idle;
        }
    }

    /// Pops the scheduled destination when its countdown has elapsed (and is
    /// not held).
    pub(super) fn take_due(&mut self, now: Instant) -> Option<Destination> {
        let due = matches!(self, Self::Scheduled { clock, .. } if clock.is_due(now));
        if !due {
            return None;
        }
        let Self::Scheduled { destination, .. } = std::mem::replace(self, Self::Idle) else {
            unreachable!()
        };
        Some(destination)
    }

    /// Cancels a pending rule send before it goes out.
    pub(super) fn cancel_scheduled(&mut self) -> bool {
        if self.is_pending_rule() {
            *self = Self::TakenBack;
            true
        } else {
            false
        }
    }

    pub(super) fn cancel_pending_for_pick(&mut self) {
        if self.is_pending_rule() {
            *self = Self::Idle;
        }
    }

    /// Decides what picking `session` should do in the current state.
    pub(super) fn pick_action(&self, session: &str) -> PickAction {
        match self {
            Self::Delivered { session: sent, .. }
            | Self::TooLate {
                session: sent,
                retry_undo: Some(_),
                ..
            } => {
                if sent == session {
                    PickAction::AlreadyThere
                } else {
                    PickAction::Redirect
                }
            }
            Self::Queued { .. } => PickAction::Redirect,
            Self::Idle
            | Self::ResolvingRule { .. }
            | Self::Scheduled { .. }
            | Self::Sending { .. }
            | Self::TakenBack
            | Self::TooLate {
                retry_undo: None, ..
            }
            | Self::Failed { .. } => PickAction::SendFresh,
        }
    }

    pub(super) fn attach_saved_rule(&mut self, rule: SavedRule) {
        if let Self::Delivered { saved, .. } | Self::Queued { saved, .. } = self {
            *saved = Some(rule);
        }
    }

    pub(super) fn take_saved_rule(&mut self) -> Option<SavedRule> {
        match self {
            Self::Delivered { saved, .. } | Self::Queued { saved, .. } => saved.take(),
            _ => None,
        }
    }

    /// Transitions a queued send that succeeded on background retry into a
    /// delivered send with no undo handle (preserving its session ID and any
    /// saved rule).
    pub(super) fn mark_retried(&mut self) -> bool {
        if !matches!(self, Self::Queued { .. }) {
            return false;
        }
        let Self::Queued {
            session, to, saved, ..
        } = std::mem::replace(self, Self::Idle)
        else {
            unreachable!()
        };
        *self = Self::Delivered {
            session,
            to,
            undo: None,
            offer: None,
            saved,
        };
        true
    }

    /// Begins taking back a delivered or queued send.
    pub(super) fn begin_undo(&mut self) -> UndoPlan {
        match std::mem::replace(self, Self::TakenBack) {
            Self::Delivered {
                session,
                to,
                undo: Some(sent),
                ..
            }
            | Self::TooLate {
                session,
                to,
                retry_undo: Some(sent),
            } => UndoPlan::Handle { session, to, sent },
            Self::Delivered {
                session,
                to,
                undo: None,
                ..
            } => {
                *self = Self::TooLate {
                    session,
                    to,
                    retry_undo: None,
                };
                UndoPlan::RetriedNoHandle
            }
            Self::Queued { .. } => UndoPlan::Queued,
            other => {
                *self = other;
                UndoPlan::None
            }
        }
    }

    #[cfg(test)]
    pub(super) fn clock(&self) -> Option<&Clock> {
        match self {
            Self::ResolvingRule { clock } | Self::Scheduled { clock, .. } => Some(clock),
            _ => None,
        }
    }

    #[cfg(test)]
    pub(super) fn clock_mut(&mut self) -> Option<&mut Clock> {
        match self {
            Self::ResolvingRule { clock } | Self::Scheduled { clock, .. } => Some(clock),
            _ => None,
        }
    }

    #[cfg(test)]
    pub(super) fn has_undo_handle(&self) -> bool {
        matches!(
            self,
            Self::Delivered { undo: Some(_), .. }
                | Self::TooLate {
                    retry_undo: Some(_),
                    ..
                }
        )
    }
}

/// A delivery as a sentence for `hex capture send`.
pub(super) fn describe(delivery: &RouteDelivery) -> String {
    match delivery {
        RouteDelivery::Sending { to } => format!("Sending to {to}"),
        RouteDelivery::Sent { to } => format!("Sent to {to}"),
        RouteDelivery::Queued { to, why } => format!("Queued for {to}: {why}"),
        RouteDelivery::TakenBack => "Taken back".into(),
        RouteDelivery::TooLate { to } => format!("Already read in {to}"),
        RouteDelivery::Failed { to, why } => format!("Not sent to {to}: {why}"),
    }
}

pub(super) fn delivery_kind(delivery: &RouteDelivery) -> &'static str {
    match delivery {
        RouteDelivery::Sending { .. } => "sending",
        RouteDelivery::Sent { .. } => "sent",
        RouteDelivery::Queued { .. } => "queued",
        RouteDelivery::TakenBack => "taken-back",
        RouteDelivery::TooLate { .. } => "too-late",
        RouteDelivery::Failed { .. } => "failed",
    }
}
