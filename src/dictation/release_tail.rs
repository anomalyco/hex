//! Where a shortcut finish's tail stands, and the poll a capture loop that
//! reads microphone chunks does while it waits for the tail's audio.

use std::time::Duration;
#[cfg(not(target_os = "macos"))]
use std::time::Instant;

#[cfg(not(target_os = "macos"))]
use crate::audio::CaptureInstant;
#[cfg(not(target_os = "macos"))]
use crate::dictation::DictationCapture;
#[cfg(not(target_os = "macos"))]
use crate::dictation::TAIL_DELIVERY_GRACE;
use crate::dictation::{MAX_RELEASE_TAIL, POST_RELEASE_TAIL};

/// Where a shortcut finish's tail stands, from the audio recorded so far.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ReleaseTail {
    /// More audio is needed. `growing` once speech has carried past
    /// [`POST_RELEASE_TAIL`], so the wait is bounded by
    /// [`MAX_RELEASE_TAIL`] rather than the base tail.
    Waiting { growing: bool },
    /// Finish keeping this much audio after the release.
    Through(Duration),
}

impl ReleaseTail {
    /// The longest tail a wait in this state may still end with. Callers add
    /// [`crate::dictation::TAIL_DELIVERY_GRACE`] to bound how long they wait
    /// for the microphone stream to deliver it.
    pub fn bound(self) -> Duration {
        match self {
            Self::Waiting { growing: true } => MAX_RELEASE_TAIL,
            Self::Waiting { growing: false } => POST_RELEASE_TAIL,
            Self::Through(tail) => tail,
        }
    }
}

#[cfg(not(target_os = "macos"))]
#[derive(Clone, Copy)]
enum WaitState {
    Decided,
    Waiting { started: Instant, tone: Duration },
}

/// A shortcut release waiting for its tail, for capture loops that poll
/// between microphone chunks (Linux and Windows; the macOS audio owner
/// drains its tail directly).
#[cfg(not(target_os = "macos"))]
pub struct ReleaseWait {
    ended_at: CaptureInstant,
    state: WaitState,
}

#[cfg(not(target_os = "macos"))]
impl ReleaseWait {
    /// A release at `ended_at`. Only one that will transcribe waits; its
    /// stop tone (sounding from now) is never taken for speech.
    pub fn new(capture: &DictationCapture, ended_at: CaptureInstant) -> Self {
        let state = if capture.would_transcribe(ended_at) {
            WaitState::Waiting {
                started: Instant::now(),
                tone: crate::feedback::stop_tone_length(),
            }
        } else {
            WaitState::Decided
        };
        Self { ended_at, state }
    }

    pub fn ended_at(&self) -> CaptureInstant {
        self.ended_at
    }

    pub fn waits(&self) -> bool {
        matches!(self.state, WaitState::Waiting { .. })
    }

    /// Finishes with the audio already captured, as after a lost microphone.
    pub fn stop_waiting(&mut self) {
        self.state = WaitState::Decided;
    }

    /// The tail to finish with once it is decided, or once the wait for it
    /// is over (the base tail plus [`TAIL_DELIVERY_GRACE`], or the longest
    /// tail plus the grace while speech grows it); `None` while it is still
    /// coming.
    pub fn tail(&self, capture: &DictationCapture, now: Instant) -> Option<Duration> {
        let WaitState::Waiting { started, tone } = self.state else {
            return Some(POST_RELEASE_TAIL);
        };
        match capture.release_tail(self.ended_at, tone) {
            ReleaseTail::Through(tail) => Some(tail),
            waiting if now >= started + waiting.bound() + TAIL_DELIVERY_GRACE => {
                Some(waiting.bound())
            }
            ReleaseTail::Waiting { .. } => None,
        }
    }
}
