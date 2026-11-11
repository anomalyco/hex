//! A digital-silence finish on the Linux listener: the microphone runs but
//! delivered only zeros, and must be reopened (or, while lingering, closed so
//! the next press opens a fresh stream). Split from [`super`] to keep it under
//! the source cap.

use std::time::Duration;

use crate::audio::RecoveringAudioInput;
use crate::audio::linger::MicrophoneLinger;

/// A warm microphone whose finished dictation was exactly zero: the stream
/// runs but its device was handed to another app, and stays silent until
/// reopened (#1122). Reopen it as after a stream failure. A microphone
/// released while idle already opens fresh for the next press, unless Labs ›
/// Keep mic open after dictating keeps it lingering (`window`): then it closes
/// now, so the next press opens a fresh stream instead of the dead one.
/// Returns whether the stream went away, recovering or closed; the caller
/// must treat it as unavailable until it reopens or opens again.
pub(super) fn reopen_silent_stream(
    input: &mut RecoveringAudioInput,
    release_while_idle: bool,
    linger: &mut MicrophoneLinger,
    window: Option<Duration>,
) -> bool {
    if !input.is_open() || input.is_recovering() {
        return false;
    }
    if release_while_idle {
        if window.is_none() {
            return false;
        }
        tracing::warn!(
            device = %input.device_name(),
            "dictation audio was digital silence; closing the lingering microphone"
        );
        linger.clear();
        input.close();
        return true;
    }
    tracing::warn!(
        device = %input.device_name(),
        "dictation audio was digital silence; reopening the microphone"
    );
    input.request_recovery();
    true
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::audio::{RecoveringAudioInput, RecoveringAudioInputEvent};

    /// An input whose replacement open already answered, so it is open and
    /// receiving.
    fn open_input() -> RecoveringAudioInput {
        let (mut input, opened) = RecoveringAudioInput::pending_for_test();
        let (replacement, _samples) = crate::audio::AudioInput::channel_for_test();
        opened
            .send((0, Ok(crate::audio::Opened::Input(Box::new(replacement)))))
            .unwrap();
        assert!(matches!(
            input.recv_timeout(Duration::ZERO, true),
            RecoveringAudioInputEvent::Reopened
        ));
        input
    }

    #[test]
    fn a_warm_silent_stream_reopens_once_unless_released() {
        let mut input = open_input();
        let mut linger = MicrophoneLinger::default();
        assert!(reopen_silent_stream(&mut input, false, &mut linger, None));
        assert!(input.is_recovering());
        // A second finish cannot queue another replacement.
        assert!(!reopen_silent_stream(&mut input, false, &mut linger, None));

        // Release while idle with the lab off keeps the open stream: no
        // reopen, and it stays open for the next press.
        let mut released = open_input();
        assert!(!reopen_silent_stream(
            &mut released,
            true,
            &mut linger,
            None
        ));
        assert!(released.is_open(), "the stream stays open");
        assert!(!released.is_recovering(), "and is not reopened");
    }

    #[test]
    fn a_lingering_silent_stream_closes_instead_of_reopening() {
        let mut input = open_input();
        assert!(input.is_open());
        let mut linger = MicrophoneLinger::default();
        assert!(reopen_silent_stream(
            &mut input,
            true,
            &mut linger,
            Some(Duration::from_secs(60)),
        ));
        assert!(!input.is_open(), "the lingering microphone closes");
        assert!(!input.is_recovering(), "nothing is reopened");
        assert!(!linger.armed(), "the window ends with the stream");
    }
}
