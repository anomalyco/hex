//! The audio at a capture's head a live stream must not take for speech it
//! missed: Hex's own sounds, and the window before "While dictating" output
//! suppression reaches the microphone (#2338).
use std::time::Duration;

use super::{PARAKEET_SAMPLE_RATE, STOP_TONE_MARGIN, samples_for};
use crate::audio::CaptureInstant;

/// How far a report may run past the newest audio the capture has. The
/// audio owner has usually ingested it already; beyond this the microphone
/// is stalled and the clip will end long before it.
const MAX_AHEAD: Duration = Duration::from_millis(500);

/// Clip samples at `at` on the recorded timeline: ahead of the newest audio
/// it projects forward (bounded); into audio already recorded it maps back,
/// so a report handled after more audio arrived still lands where it
/// happened.
pub(super) fn clip_position(
    recorded: usize,
    recorded_through: Option<CaptureInstant>,
    at: CaptureInstant,
) -> usize {
    let Some(through) = recorded_through else {
        return recorded;
    };
    match at.checked_duration_since(through) {
        Some(ahead) => recorded + samples_for(ahead.min(MAX_AHEAD), PARAKEET_SAMPLE_RATE),
        None => recorded.saturating_sub(samples_for(
            through.duration_since(at),
            PARAKEET_SAMPLE_RATE,
        )),
    }
}

/// How long after an effect takes effect the speakers may still reach the
/// microphone: the output-and-input path, once the ramp or write is done.
const SPEAKER_TO_MIC_MARGIN: Duration = Duration::from_millis(40);

/// Clip samples to ignore for suppression that took effect at `at`.
pub(super) fn effective_through(
    recorded: usize,
    recorded_through: Option<CaptureInstant>,
    at: CaptureInstant,
) -> usize {
    clip_position(recorded, recorded_through, at)
        + samples_for(SPEAKER_TO_MIC_MARGIN, PARAKEET_SAMPLE_RATE)
}

/// Clip samples of Hex's own audio at the capture's head: the pre-roll (a
/// retracted double-tap first tone, an earlier dictation's stop tone) through
/// the press, then the start tone and the margin after it (the grace before a
/// provisional tone sounds, plus a little latency), and everything through
/// the moment output suppression took effect. A stream that hears it has not
/// lost any speech.
pub(super) fn ignored_head(pre_press: Duration, suppressed_through: usize) -> usize {
    let tone = crate::feedback::start_tone_length();
    let after_press = if tone.is_zero() {
        Duration::ZERO
    } else {
        tone + STOP_TONE_MARGIN
    };
    let own_sounds = samples_for(pre_press, PARAKEET_SAMPLE_RATE)
        + samples_for(after_press, PARAKEET_SAMPLE_RATE);
    own_sounds.max(suppressed_through)
}

#[cfg(test)]
mod tests {
    use super::*;

    const THROUGH: CaptureInstant = CaptureInstant::from_nanos(60_000_000_000);

    #[test]
    fn a_late_report_maps_back_into_the_recorded_audio() {
        let at = THROUGH.checked_sub(Duration::from_millis(200)).unwrap();
        assert_eq!(clip_position(10_400, Some(THROUGH), at), 7_200);
        let ahead = THROUGH + Duration::from_millis(100);
        assert_eq!(clip_position(10_400, Some(THROUGH), ahead), 12_000);
        // The speaker-to-microphone margin follows the effect's moment.
        assert_eq!(
            effective_through(10_400, Some(THROUGH), ahead),
            12_000 + samples_for(SPEAKER_TO_MIC_MARGIN, PARAKEET_SAMPLE_RATE)
        );
    }
}
