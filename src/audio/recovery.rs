//! When the dictation microphone opens again. An open that fails backs off to
//! five seconds. Any stream that fails before proving steady delivery backs off
//! to a minute instead, so a microphone that never starts cannot keep waking
//! the machine every few seconds (#1681). A stream that delivered steadily
//! reopens at once, as it always has (#1206). A stream that delivered digital
//! silence reopens at once too, and that one open makes the device renegotiate
//! its sample rate first (#1575).

use std::time::{Duration, Instant};

/// The first wait after a failed open or a stream that stopped delivering.
pub(super) const MICROPHONE_RETRY_INITIAL: Duration = Duration::from_millis(250);
/// A microphone that will not open waits at most this long between attempts.
pub(super) const MICROPHONE_RETRY_MAX: Duration = Duration::from_secs(5);
/// A stream that keeps failing before it proves steady delivery waits this
/// long at most: a reopen every few seconds would only wake the machine.
pub(super) const MICROPHONE_STALL_RETRY_MAX: Duration = Duration::from_secs(60);

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) enum MicrophoneRecoveryReason {
    /// The default input or the list of inputs changed, or a missing saved
    /// microphone is due another look. The open input keeps running.
    RouteChanged,
    SelectionChanged,
    StreamFailed,
}

/// What the microphone does next, and how long to wait before it does it.
pub(super) struct MicrophoneRecovery {
    pub(super) next_attempt: Option<Instant>,
    pub(super) delay: Duration,
    /// Whether `delay` is a stream failure's backoff. It grows to
    /// [`MICROPHONE_STALL_RETRY_MAX`] and survives the reopens it schedules,
    /// so a stream that never proves steady delivery cannot start the wait
    /// over on every reopen.
    stream_stall: bool,
    /// Whether the next replacement must make the device renegotiate its
    /// sample rate before it opens. Only a stream that delivered digital
    /// silence asks for it, and the one open that follows takes it.
    renegotiate: bool,
    pub(super) reason: Option<MicrophoneRecoveryReason>,
    pub(super) target_revision: Option<u64>,
}

impl Default for MicrophoneRecovery {
    fn default() -> Self {
        Self {
            next_attempt: None,
            delay: MICROPHONE_RETRY_INITIAL,
            stream_stall: false,
            renegotiate: false,
            reason: None,
            target_revision: None,
        }
    }
}

impl MicrophoneRecovery {
    pub(super) fn request_selection_change(&mut self, revision: u64, now: Instant) {
        if self.target_revision != Some(revision) {
            self.next_attempt = Some(now);
            self.delay = MICROPHONE_RETRY_INITIAL;
            self.stream_stall = false;
            self.renegotiate = false;
            self.target_revision = Some(revision);
            if self.reason != Some(MicrophoneRecoveryReason::StreamFailed) {
                self.reason = Some(MicrophoneRecoveryReason::SelectionChanged);
            }
        }
    }

    /// Resolves the input again at `at`. A pending reopen or selection
    /// change resolves it anyway, but a failed stream's wait is cut short:
    /// the lid opening must reach the built-in microphone at once instead of
    /// waiting out up to a minute. Its backoff stays, so device churn cannot
    /// start the fast reopen loop again.
    pub(super) fn request_route_check(&mut self, at: Instant) {
        match self.reason {
            None => {
                self.reason = Some(MicrophoneRecoveryReason::RouteChanged);
                self.next_attempt = Some(at);
                // A stream that has not proven itself yet keeps the backoff
                // its failed predecessors grew.
                if !self.stream_stall {
                    self.delay = MICROPHONE_RETRY_INITIAL;
                }
            }
            Some(
                MicrophoneRecoveryReason::RouteChanged | MicrophoneRecoveryReason::StreamFailed,
            ) => {
                self.next_attempt = Some(self.next_attempt.map_or(at, |next| next.min(at)));
            }
            Some(_) => {}
        }
    }

    /// A live stream failed, or startup could not open one: reopen at once
    /// and start over, as a working microphone does.
    pub(super) fn request_stream_recovery(&mut self, now: Instant) {
        if self.reason != Some(MicrophoneRecoveryReason::StreamFailed) {
            self.next_attempt = Some(now);
            self.delay = MICROPHONE_RETRY_INITIAL;
            self.stream_stall = false;
            self.reason = Some(MicrophoneRecoveryReason::StreamFailed);
        }
    }

    /// A finished dictation was exactly zero: the stream runs but delivers
    /// digital silence, as a device a call app let go can (#1122). Reopen it
    /// like a failed stream, and have that open renegotiate the device's
    /// sample rate first: a device left on a stale clock delivers only zeros
    /// until the rate changes (#1575).
    pub(super) fn request_silence_recovery(&mut self, now: Instant) {
        self.request_stream_recovery(now);
        self.renegotiate = true;
    }

    /// Whether the next replacement must renegotiate the sample rate, taken
    /// by the open it applies to and by no other.
    pub(super) fn take_renegotiation(&mut self) -> bool {
        std::mem::take(&mut self.renegotiate)
    }

    /// A stream stopped before it proved steady delivery: wait out the stall
    /// backoff, doubling it up to [`MICROPHONE_STALL_RETRY_MAX`].
    pub(super) fn back_off_stream_recovery(&mut self, now: Instant) {
        self.reason = Some(MicrophoneRecoveryReason::StreamFailed);
        self.stream_stall = true;
        self.failed(now);
    }

    pub(super) fn blocks_audio(&self) -> bool {
        self.reason == Some(MicrophoneRecoveryReason::StreamFailed)
    }

    pub(super) fn should_attempt(&self, now: Instant) -> bool {
        self.next_attempt.is_some_and(|next| now >= next)
    }

    pub(super) fn failed(&mut self, now: Instant) {
        self.next_attempt = Some(now + self.delay);
        let ceiling = if self.stream_stall {
            MICROPHONE_STALL_RETRY_MAX
        } else {
            MICROPHONE_RETRY_MAX
        };
        self.delay = self.delay.saturating_mul(2).min(ceiling);
    }

    /// A replacement stream is open. A failed stream's backoff is kept until
    /// that stream delivers steadily; anything else starts over.
    pub(super) fn reopened(&mut self) {
        self.next_attempt = None;
        self.reason = None;
        self.renegotiate = false;
        self.target_revision = None;
        if !self.stream_stall {
            self.delay = MICROPHONE_RETRY_INITIAL;
        }
    }

    /// The installed stream has delivered steadily, so the backoff left by
    /// the failed streams before it is forgotten. A stream that never proves
    /// steady delivery never gets here.
    pub(super) fn stream_proved(&mut self) {
        if self.stream_stall {
            self.stream_stall = false;
            self.delay = MICROPHONE_RETRY_INITIAL;
        }
    }

    /// Forgets everything: the microphone is closed, or the person chose
    /// another one.
    pub(super) fn recovered(&mut self) {
        self.next_attempt = None;
        self.delay = MICROPHONE_RETRY_INITIAL;
        self.stream_stall = false;
        self.renegotiate = false;
        self.reason = None;
        self.target_revision = None;
    }

    /// A dictation press asks for a failed microphone now, keeping the
    /// backoff it has grown so repeated presses cannot restore the loop.
    pub(super) fn retry_now(&mut self, now: Instant) {
        if self.reason == Some(MicrophoneRecoveryReason::StreamFailed) {
            self.next_attempt = Some(now);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_a_silence_recovery_renegotiates_the_next_open() {
        let now = Instant::now();

        // Digital silence reopens like a failure and renegotiates once.
        let mut silent = MicrophoneRecovery::default();
        silent.request_silence_recovery(now);
        assert_eq!(silent.reason, Some(MicrophoneRecoveryReason::StreamFailed));
        assert!(silent.should_attempt(now));
        assert!(silent.take_renegotiation());
        // The one open that follows takes it; a second open does not.
        assert!(!silent.take_renegotiation());

        // A stream failure reopens without touching the device's rate.
        let mut failed = MicrophoneRecovery::default();
        failed.request_stream_recovery(now);
        assert!(!failed.take_renegotiation());

        // Neither does a stream that never proved itself stalling.
        let mut stalled = MicrophoneRecovery::default();
        stalled.back_off_stream_recovery(now);
        assert!(!stalled.take_renegotiation());
    }

    #[test]
    fn a_reopen_or_a_reset_forgets_a_renegotiation_no_open_took() {
        let now = Instant::now();
        let mut recovery = MicrophoneRecovery::default();
        recovery.request_silence_recovery(now);
        recovery.reopened();
        assert!(!recovery.renegotiate);

        recovery.request_silence_recovery(now);
        recovery.recovered();
        assert!(!recovery.renegotiate);

        // Choosing another microphone is not the stuck device any more.
        recovery.request_silence_recovery(now);
        recovery.request_selection_change(7, now);
        assert!(!recovery.renegotiate);
    }
}
