//! Where a shortcut finish's tail stands, and the poll a capture loop that
//! reads microphone chunks does while it waits for the tail's audio.

use std::time::Duration;
#[cfg(not(target_os = "macos"))]
use std::time::Instant;

#[cfg(not(target_os = "macos"))]
use crate::audio::CaptureInstant;
#[cfg(not(target_os = "macos"))]
use crate::dictation::DictationCapture;
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
    /// The longest tail a wait in this state may still end with.
    pub fn bound(self) -> Duration {
        match self {
            Self::Waiting { growing: true } => MAX_RELEASE_TAIL,
            Self::Waiting { growing: false } => POST_RELEASE_TAIL,
            Self::Through(tail) => tail,
        }
    }
}

/// A shortcut release waiting for its tail, for capture loops that poll
/// between microphone chunks (Linux and Windows; the macOS audio owner
/// drains its tail directly).
#[cfg(not(target_os = "macos"))]
pub struct ReleaseWait {
    ended_at: CaptureInstant,
    started: Instant,
    tone: Duration,
    waits: bool,
}

#[cfg(not(target_os = "macos"))]
impl ReleaseWait {
    /// How long past a tail itself the loop waits for its audio.
    const GRACE: Duration = Duration::from_millis(100);

    /// A release at `ended_at`. Only one that will transcribe waits; its
    /// stop tone (sounding from now) is never taken for speech.
    pub fn new(capture: &DictationCapture, ended_at: CaptureInstant) -> Self {
        let waits = capture.would_transcribe(ended_at);
        Self {
            ended_at,
            started: Instant::now(),
            tone: if waits {
                crate::feedback::stop_tone_length()
            } else {
                Duration::ZERO
            },
            waits,
        }
    }

    pub fn ended_at(&self) -> CaptureInstant {
        self.ended_at
    }

    pub fn waits(&self) -> bool {
        self.waits
    }

    /// Finishes with the audio already captured, as after a lost microphone.
    pub fn stop_waiting(&mut self) {
        self.waits = false;
    }

    /// The tail to finish with once it is decided, or once the wait for it
    /// is over (the base tail plus the grace, or the longest tail plus the
    /// grace while speech grows it); `None` while it is still coming.
    pub fn tail(&self, capture: &DictationCapture, now: Instant) -> Option<Duration> {
        if !self.waits {
            return Some(POST_RELEASE_TAIL);
        }
        match capture.release_tail(self.ended_at, self.tone) {
            ReleaseTail::Through(tail) => Some(tail),
            waiting if now >= self.started + waiting.bound() + Self::GRACE => Some(waiting.bound()),
            ReleaseTail::Waiting { .. } => None,
        }
    }
}
