//! Microphone-timeline gaps on the Linux listener. A short step between
//! chunks (a Bluetooth stream starting, #619) is filled with silence so the
//! capture keeps going; a longer one is a discontinuity, and audio from either
//! side of it must not be stitched together. The macOS owner applies the same
//! rule in [`crate::dictation::audio`].

use std::time::Duration;

use crate::audio::CaptureInstant;
use crate::dictation::{CHUNK_GAP_TOLERANCE, MAX_PADDED_GAP};

/// One chunk on the microphone timeline, relative to the chunk before it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum ChunkStep {
    /// Contiguous, or the first chunk of a stream.
    Continue,
    /// A short gap: this many frames of silence belong before the chunk.
    Pad(usize),
    /// Too long to bridge: the capture after the gap starts fresh.
    Discontinuity(Duration),
}

/// Where a chunk of `frames` ending at `captured_through` sits relative to
/// the previous chunk's end. `None` is the first chunk of a stream.
pub(super) fn chunk_step(
    previous: Option<CaptureInstant>,
    frames: usize,
    captured_through: CaptureInstant,
    sample_rate: u32,
) -> ChunkStep {
    let captured_from = captured_through.sub_frames(frames, sample_rate);
    let Some(gap) = previous
        .map(|previous| captured_from.duration_since(previous))
        .filter(|gap| *gap > CHUNK_GAP_TOLERANCE)
    else {
        return ChunkStep::Continue;
    };
    if gap <= MAX_PADDED_GAP {
        return ChunkStep::Pad((gap.as_secs_f64() * f64::from(sample_rate)).round() as usize);
    }
    ChunkStep::Discontinuity(gap)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn through() -> CaptureInstant {
        CaptureInstant::from_nanos(60_000_000_000)
    }

    fn before(duration: Duration) -> CaptureInstant {
        through()
            .checked_sub(duration)
            .expect("the fixture starts after the gap")
    }

    #[test]
    fn a_short_microphone_gap_is_padded_with_silence() {
        // A 100 ms gap before this 100 ms chunk.
        assert_eq!(
            chunk_step(
                Some(before(Duration::from_millis(200))),
                1_600,
                through(),
                16_000
            ),
            ChunkStep::Pad(1_600)
        );
        // A gap at the 250 ms limit is still bridged.
        assert_eq!(
            chunk_step(
                Some(before(Duration::from_millis(350))),
                1_600,
                through(),
                16_000
            ),
            ChunkStep::Pad(4_000)
        );
        // Jitter within the tolerance and a stream's first chunk are neither.
        assert_eq!(
            chunk_step(
                Some(before(Duration::from_millis(150))),
                1_600,
                through(),
                16_000
            ),
            ChunkStep::Continue
        );
        assert_eq!(
            chunk_step(None, 1_600, through(), 16_000),
            ChunkStep::Continue
        );
    }

    #[test]
    fn a_long_microphone_gap_is_not_stitched() {
        assert_eq!(
            chunk_step(
                Some(before(Duration::from_millis(400))),
                1_600,
                through(),
                16_000
            ),
            ChunkStep::Discontinuity(Duration::from_millis(300))
        );
    }
}
