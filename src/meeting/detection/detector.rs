use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{Duration, Instant};

use crate::app_settings::{MeetingPolicy, MeetingProviderChoice};

use super::{
    ABSENT_FOR_RESET, CALL_ENDED_AFTER, DROPOUT_GRACE, Detection, MeetingCandidate, MeetingSource,
    Run, SWITCHED_CALL_MEMORY,
};

static NEXT_OFFER_SEQ: AtomicU64 = AtomicU64::new(1);

/// Opaque identity of one Ask offer presentation. A monotonic sequence ensures
/// two offers minted within the same `Instant` remain distinct.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct OfferId {
    seq: u64,
    #[allow(dead_code)]
    at: Instant,
}

impl OfferId {
    pub fn at(now: Instant) -> Self {
        Self {
            seq: NEXT_OFFER_SEQ.fetch_add(1, Ordering::Relaxed),
            at: now,
        }
    }
}

/// Identity and last-seen time of the currently snoozed call.
struct SnoozedCall {
    key: String,
    application: Option<String>,
    /// The offered call's meeting code, when its page named one: another
    /// code in the same app is a different call, not this one returning.
    call: Option<String>,
    /// When a candidate was last on the microphone (or known to be, while
    /// recording); absence is counted from here, gaps included.
    last_present: Instant,
}

impl SnoozedCall {
    fn from_candidate(candidate: &MeetingCandidate, now: Instant) -> Self {
        Self {
            key: candidate.key.clone(),
            application: candidate.application.clone(),
            call: candidate.call.clone(),
            last_present: now,
        }
    }

    fn is_application_snoozed(&self, canonical_bundle_id: &str) -> bool {
        self.application.as_deref() == Some(canonical_bundle_id) && self.call.is_none()
    }

    fn snoozes(&self, candidate: &MeetingCandidate) -> bool {
        if self.key == candidate.key {
            return true;
        }
        candidate.application.is_some()
            && self.application == candidate.application
            && !(self.call.is_some() && candidate.call.is_some() && self.call != candidate.call)
    }
}

/// Lifecycle of the snoozed call and its optional open Ask offer panel.
#[derive(Default)]
enum Snooze {
    #[default]
    Clear,
    /// An Ask presentation can still answer or time out.
    Offered { call: SnoozedCall, id: OfferId },
    /// Answered, dismissed, auto-recorded, or covered by a calendar reminder:
    /// snoozed until absent for [`ABSENT_FOR_RESET`].
    Kept(SnoozedCall),
    /// The call's recording stopped (#1113): snoozed until absent for
    /// [`CALL_ENDED_AFTER`].
    Stopped(SnoozedCall),
    /// The Ask capsule closed unanswered on timeout (#1267): snooze ends as
    /// soon as the call leaves the microphone.
    TimedOut(SnoozedCall),
}

impl Snooze {
    fn open_offer(&self) -> Option<OfferId> {
        match self {
            Self::Offered { id, .. } => Some(*id),
            Self::Clear | Self::Kept(_) | Self::Stopped(_) | Self::TimedOut(_) => None,
        }
    }

    fn call(&self) -> Option<&SnoozedCall> {
        match self {
            Self::Clear => None,
            Self::Offered { call, .. }
            | Self::Kept(call)
            | Self::Stopped(call)
            | Self::TimedOut(call) => Some(call),
        }
    }

    fn call_mut(&mut self) -> Option<&mut SnoozedCall> {
        match self {
            Self::Clear => None,
            Self::Offered { call, .. }
            | Self::Kept(call)
            | Self::Stopped(call)
            | Self::TimedOut(call) => Some(call),
        }
    }

    fn absence_grace(&self) -> Option<Duration> {
        match self {
            Self::Clear => None,
            Self::Offered { .. } | Self::Kept(_) => Some(ABSENT_FOR_RESET),
            Self::Stopped(_) => Some(CALL_ENDED_AFTER),
            Self::TimedOut(_) => Some(Duration::ZERO),
        }
    }
}

/// Call-switch memory: either waiting to substitute a switched-to call while
/// the browser still names the ended one, or holding the ended call's code
/// ready to hand off once to the next recording.
#[derive(Default)]
enum SwitchedState {
    #[default]
    Nothing,
    Pending(SwitchedCall),
    HandedOff(String),
}

#[derive(Default)]
pub struct MeetingDetector {
    pending: Option<Pending>,
    snooze: Snooze,
    switched: SwitchedState,
    calendar: Option<CalendarSnooze>,
    /// Offer IDs retired since the controller last drained them.
    withdrawn_offers: Vec<OfferId>,
}

/// A candidate that has been on the microphone but is not yet stable.
struct Pending {
    key: String,
    application: Option<String>,
    /// A browser call whose page is not known (yet).
    unresolved: bool,
    run: Run,
}

/// The call a browser's page named just before a call-switch stop (#1775):
/// the browser keeps the microphone, and its next reads can still name the
/// ended call or none, so detection uses this candidate until the page names
/// a call of its own or the call app leaves.
struct SwitchedCall {
    /// The code of the call whose recording stopped, when its page named one.
    ended: Option<String>,
    candidate: MeetingCandidate,
    /// When the page named it; the memory lapses
    /// [`SWITCHED_CALL_MEMORY`] later if no detection uses it.
    noted: Instant,
    /// The substitution was logged: a page that keeps naming the ended call
    /// says so once per switch, not every 500 ms pass.
    logged: bool,
}

/// A call the user already answered a calendar reminder for: recorded it,
/// or dismissed it.
#[derive(Clone, Debug, Eq, PartialEq)]
struct CalendarSnooze {
    /// App bundle IDs and website hosts of the meeting's call service;
    /// empty when the invitation had no call link, which covers the next
    /// call alone.
    keys: Vec<String>,
    /// When the snooze lapses, in wall-clock milliseconds: the meeting's
    /// end, or shortly after its start when it has no call link.
    until_ms: i64,
}

impl CalendarSnooze {
    fn covers(&self, candidate: &MeetingCandidate) -> bool {
        self.keys.is_empty()
            || [candidate.site.as_deref(), candidate.application.as_deref()]
                .into_iter()
                .flatten()
                .any(|key| {
                    self.keys
                        .iter()
                        .any(|known| crate::calendar::key_matches(key, known))
                })
    }
}

impl MeetingDetector {
    /// A calendar reminder was answered: its call, when detected before
    /// `until_ms`, is treated as already offered. `keys` names the call
    /// service; empty covers the next call detected.
    pub fn snooze_calendar_call(&mut self, keys: Vec<String>, until_ms: i64) {
        self.calendar = Some(CalendarSnooze { keys, until_ms });
    }

    /// The call that was offered is still on the microphone, as a running
    /// recording's call app is: its snooze must last from now, not from the
    /// last detection pass.
    pub fn keep_snooze_alive(&mut self) {
        self.keep_snooze_alive_at(Instant::now());
    }

    fn set_snooze(&mut self, next: Snooze) {
        if let Snooze::Offered { id, .. } = std::mem::replace(&mut self.snooze, next) {
            self.withdrawn_offers.push(id);
        }
    }

    pub(super) fn keep_snooze_alive_at(&mut self, now: Instant) {
        if let Some(call) = self.snooze.call_mut() {
            call.last_present = now;
        }
    }

    /// The snoozed call's recording stopped: by hand, or on its own once its
    /// call ended (Stop when the call ends). Its app being off the
    /// microphone is no longer that call, so the next call in the same app is
    /// offered, or recorded under Always record (#1113). A call already gone
    /// for [`CALL_ENDED_AFTER`] when its recording stops ends its snooze at
    /// once, so the app retaking the microphone is a new call rather than the
    /// snoozed one returning.
    pub fn recording_stopped(&mut self) {
        self.recording_stopped_at(Instant::now());
    }

    pub(super) fn recording_stopped_at(&mut self, now: Instant) {
        // An automatic stop waits out the call's absence: the short absence
        // that ends a hand-stopped call's snooze has already lasted, so there
        // is nothing left to wait for.
        if self.snooze.call().is_some_and(|call| {
            now.saturating_duration_since(call.last_present) >= CALL_ENDED_AFTER
        }) {
            self.end_snooze();
            return;
        }
        match std::mem::take(&mut self.snooze) {
            Snooze::Clear => {}
            Snooze::Offered { call, id } => {
                self.snooze = Snooze::Stopped(call);
                self.withdrawn_offers.push(id);
            }
            Snooze::Kept(call) | Snooze::Stopped(call) | Snooze::TimedOut(call) => {
                self.snooze = Snooze::Stopped(call);
            }
        }
    }

    /// The page named `candidate` just before the recording of the call whose
    /// code is `ended` stopped: the browser never left the microphone, so
    /// detection starts this call even while its reads still name the ended
    /// call or none.
    pub fn call_switched(&mut self, ended: Option<String>, candidate: MeetingCandidate) {
        self.call_switched_at(ended, candidate, Instant::now());
    }

    pub(super) fn call_switched_at(
        &mut self,
        ended: Option<String>,
        candidate: MeetingCandidate,
        now: Instant,
    ) {
        self.switched = SwitchedState::Pending(SwitchedCall {
            ended,
            candidate,
            noted: now,
            logged: false,
        });
    }

    /// The code of the call whose page named the one detection just started,
    /// taken once by the watcher: the new recording keeps it, so the page
    /// still naming it never ends the new one.
    pub fn take_switched_from(&mut self) -> Option<String> {
        match std::mem::take(&mut self.switched) {
            SwitchedState::HandedOff(ended) => Some(ended),
            other => {
                self.switched = other;
                None
            }
        }
    }

    /// The page came back to the recorded call, or the user kept recording:
    /// nothing starts on the switched call's behalf.
    pub fn forget_switched_call(&mut self) {
        self.switched = SwitchedState::Nothing;
    }

    /// The call a page named before a call-switch stop, while it is
    /// remembered.
    #[cfg(test)]
    pub(crate) fn switched_call(&self) -> Option<&MeetingCandidate> {
        match &self.switched {
            SwitchedState::Pending(switched) => Some(&switched.candidate),
            SwitchedState::Nothing | SwitchedState::HandedOff(_) => None,
        }
    }

    /// The offer's capsule closed on its timeout, unanswered.
    pub fn offer_timed_out(&mut self, id: OfferId) {
        match std::mem::take(&mut self.snooze) {
            Snooze::Offered { call, id: open } if open == id => {
                self.snooze = Snooze::TimedOut(call);
                self.withdrawn_offers.push(open);
            }
            other => self.snooze = other,
        }
    }

    pub fn open_offer(&self) -> Option<OfferId> {
        self.snooze.open_offer()
    }

    /// A recording or Meetings being turned off retires the Ask panel while
    /// keeping the call's snooze intact.
    pub fn withdraw_offer(&mut self) -> Option<OfferId> {
        match std::mem::take(&mut self.snooze) {
            Snooze::Offered { call, id } => {
                self.snooze = Snooze::Kept(call);
                self.withdrawn_offers.push(id);
                Some(id)
            }
            other => {
                self.snooze = other;
                None
            }
        }
    }

    /// Drains offer IDs retired since the last call, in retirement order.
    pub fn take_withdrawn_offers(&mut self) -> Vec<OfferId> {
        std::mem::take(&mut self.withdrawn_offers)
    }

    /// Whether the call app is on the microphone, seen while a recording is
    /// transcribing and detection doesn't run: an absence then still ends
    /// the snooze.
    pub fn observe_call_app(&mut self, present: bool) {
        self.observe_call_app_at(present, Instant::now());
    }

    pub(super) fn observe_call_app_at(&mut self, present: bool, now: Instant) {
        if present {
            self.keep_snooze_alive_at(now);
        } else {
            self.absent_at(now);
        }
    }

    /// Whether the app is snoozed whatever page it shows, so its tabs need
    /// not be read. A snoozed call with a meeting code isn't: the page can
    /// show a different call.
    pub fn is_application_snoozed(&self, canonical_bundle_id: &str) -> bool {
        self.snooze
            .call()
            .is_some_and(|call| call.is_application_snoozed(canonical_bundle_id))
    }

    /// Whether `candidate` is the offered call: the same key, or the same
    /// app unless both name different meeting codes.
    fn snoozes(&self, candidate: &MeetingCandidate) -> bool {
        self.snooze
            .call()
            .is_some_and(|call| call.snoozes(candidate))
    }

    /// After a call-switch stop the browser can keep naming the ended call
    /// (or none) while it stays on the microphone: the call it named before
    /// the stop is the one to detect. A different application, a page that
    /// names a call of its own, or a memory older than
    /// [`SWITCHED_CALL_MEMORY`] is left alone.
    fn substitute_switched(
        &mut self,
        candidate: Option<MeetingCandidate>,
        now: Instant,
    ) -> Option<MeetingCandidate> {
        if matches!(
            &self.switched,
            SwitchedState::Pending(switched)
                if now.saturating_duration_since(switched.noted) >= SWITCHED_CALL_MEMORY
        ) {
            self.switched = SwitchedState::Nothing;
        }
        let candidate = candidate?;
        let SwitchedState::Pending(switched) = &mut self.switched else {
            return Some(candidate);
        };
        if candidate.application.is_none()
            || candidate.application != switched.candidate.application
        {
            return Some(candidate);
        }
        if candidate.call.is_some() && candidate.call != switched.ended {
            self.switched = SwitchedState::Nothing;
            return Some(candidate);
        }
        if !switched.logged {
            tracing::info!(
                application = switched.candidate.application.as_deref(),
                ended = switched.ended.as_deref().unwrap_or_default(),
                call = switched.candidate.call.as_deref().unwrap_or_default(),
                "the page still names the ended call; detecting the call it named before the stop"
            );
            switched.logged = true;
        }
        Some(switched.candidate.clone())
    }

    /// No candidate is on the microphone: the snooze ends once that has
    /// lasted long enough.
    fn absent_at(&mut self, now: Instant) {
        let Some(grace) = self.snooze.absence_grace() else {
            return;
        };
        let Some(call) = self.snooze.call() else {
            return;
        };
        if now.saturating_duration_since(call.last_present) >= grace {
            self.end_snooze();
        }
    }

    /// Forgets the snoozed call: the next one is offered as new.
    fn end_snooze(&mut self) {
        self.set_snooze(Snooze::Clear);
        self.switched = SwitchedState::Nothing;
    }

    /// One observation under the user's answers, `default` for calls with
    /// none of their own. A Don't offer call shows nothing but still counts
    /// as present; Always record ones pass the same stability time and
    /// snooze as an offer, so a stopped or dismissed call never starts again
    /// until its app leaves the microphone.
    pub fn detect(
        &mut self,
        candidate: Option<MeetingCandidate>,
        choices: &[MeetingProviderChoice],
        default: MeetingPolicy,
    ) -> Option<Detection> {
        self.detect_at(
            candidate,
            choices,
            default,
            Instant::now(),
            crate::calendar::wall_clock_ms(),
        )
    }

    pub(super) fn detect_at(
        &mut self,
        candidate: Option<MeetingCandidate>,
        choices: &[MeetingProviderChoice],
        default: MeetingPolicy,
        now: Instant,
        now_ms: i64,
    ) -> Option<Detection> {
        let candidate = self.substitute_switched(candidate, now);
        if candidate.as_ref().is_some_and(|candidate| {
            candidate.policy(choices, default) == MeetingPolicy::NeverOffer
        }) {
            // Still on the microphone: an earlier offer stays snoozed.
            self.pending = None;
            self.keep_snooze_alive_at(now);
            return None;
        }
        let candidate = self.observe_at(candidate, now, now_ms)?;
        // A default of Always record starts only calls Hex can name;
        // a browser page it can't is offered instead of recording every page.
        let records = match candidate.own_policy(choices) {
            Some(policy) => policy == MeetingPolicy::AlwaysRecord,
            None => default == MeetingPolicy::AlwaysRecord && candidate.can_always_record(),
        };
        Some(if records {
            Detection::Record(candidate)
        } else {
            if let Snooze::Kept(call) = std::mem::take(&mut self.snooze) {
                self.snooze = Snooze::Offered {
                    call,
                    id: OfferId::at(now),
                };
            }
            Detection::Offer(candidate)
        })
    }

    fn observe_at(
        &mut self,
        candidate: Option<MeetingCandidate>,
        now: Instant,
        now_ms: i64,
    ) -> Option<MeetingCandidate> {
        let Some(candidate) = candidate else {
            if self.pending.as_ref().is_some_and(|pending| {
                now.saturating_duration_since(pending.run.last) > DROPOUT_GRACE
            }) {
                self.pending = None;
            }
            self.absent_at(now);
            return None;
        };
        self.keep_snooze_alive_at(now);
        if self.snoozes(&candidate) {
            self.pending = None;
            return None;
        }
        let unresolved = candidate.source == MeetingSource::BrowserCall && candidate.site.is_none();
        let held = match &mut self.pending {
            Some(pending) if pending.key == candidate.key => pending.run.extend(now),
            // The browser's page was found: the same browser has been on the
            // microphone all along, so the run carries over.
            Some(pending)
                if pending.unresolved
                    && candidate.site.is_some()
                    && pending.application.is_some()
                    && pending.application == candidate.application =>
            {
                pending.key = candidate.key.clone();
                pending.unresolved = unresolved;
                pending.run.extend(now)
            }
            _ => {
                self.pending = Some(Pending {
                    key: candidate.key.clone(),
                    application: candidate.application.clone(),
                    unresolved,
                    run: Run::start(now),
                });
                Duration::ZERO
            }
        };
        if held < candidate.required_stability() {
            return None;
        }
        self.pending = None;
        self.set_snooze(Snooze::Kept(SnoozedCall::from_candidate(&candidate, now)));
        if self
            .calendar
            .as_ref()
            .is_some_and(|snooze| now_ms >= snooze.until_ms)
        {
            self.calendar = None;
        }
        if let Some(snooze) = &self.calendar
            && snooze.covers(&candidate)
        {
            tracing::info!(
                source = candidate.source.label(),
                "detected call already answered from its calendar reminder"
            );
            if snooze.keys.is_empty() {
                self.calendar = None;
            }
            return None;
        }
        self.switched = match std::mem::take(&mut self.switched) {
            SwitchedState::Pending(switched) => switched
                .ended
                .map_or(SwitchedState::Nothing, SwitchedState::HandedOff),
            SwitchedState::Nothing | SwitchedState::HandedOff(_) => SwitchedState::Nothing,
        };
        Some(candidate)
    }
}
