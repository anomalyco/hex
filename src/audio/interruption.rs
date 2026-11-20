//! Whether a listener's capture may let go of the microphone, and when a
//! stream counts as stalled. Shared by the Linux and Windows listeners, which
//! own their capture loops; macOS reports its richer event from
//! [`crate::dictation::audio`].

use std::sync::Mutex;
use std::time::{Duration, Instant};

/// Whether a stream has gone `after` without delivering a chunk. The stall
/// watchdog reports it as a failure; a shortcut press asks first, so it never
/// starts a capture on a stream about to be reported.
pub(super) fn stalled(stall_after: Option<Duration>, last_delivery: &Mutex<Instant>) -> bool {
    stall_after.is_some_and(|after| {
        last_delivery
            .lock()
            .unwrap_or_else(|error| error.into_inner())
            .elapsed()
            >= after
    })
}

/// Whether a listener's capture is idle, so [`super::RecoveringAudioInput`]
/// may switch devices. A pending start or a release's tail still needs the
/// open stream: replacing it would discard the capture.
#[cfg(any(target_os = "linux", target_os = "windows", test))]
pub fn capture_idle(recording: bool, start_pending: bool, finishing: bool) -> bool {
    !recording && !start_pending && !finishing
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_an_idle_capture_lets_the_microphone_switch() {
        assert!(capture_idle(false, false, false));
        // Recording, a press waiting for its stream, or a release's tail.
        assert!(!capture_idle(true, false, false));
        assert!(!capture_idle(false, true, false));
        assert!(!capture_idle(false, false, true));
    }
}
