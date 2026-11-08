//! The latest peak of the microphone audio Hex receives, for the window's
//! level meters. The audio owner stores one atomic per chunk and the window
//! reads it, so a meter never blocks or slows capture. Nothing is stored
//! while the microphone is closed, so a stale peak reads as no audio.

use std::sync::OnceLock;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::Instant;

/// The latest peak's bits (high 32) and when it arrived, in milliseconds
/// since `epoch` plus one (low 32; zero before any audio).
static LATEST: AtomicU64 = AtomicU64::new(0);
/// How long a peak stays current. Chunks arrive every 10 to 20 ms while the
/// microphone is open.
const FRESH_MS: u32 = 250;

fn epoch() -> Instant {
    static EPOCH: OnceLock<Instant> = OnceLock::new();
    *EPOCH.get_or_init(Instant::now)
}

fn clock() -> u32 {
    (epoch().elapsed().as_millis() as u32).wrapping_add(1)
}

/// Records the peak of a chunk the microphone delivered, measured around the
/// chunk's mean so a DC offset never reads as a level.
pub fn record(samples: &[f32]) {
    if samples.is_empty() {
        return;
    }
    LATEST.store(pack(chunk_peak(samples), clock()), Ordering::Relaxed);
}

/// The chunk's absolute peak around its mean; zero for no samples.
fn chunk_peak(samples: &[f32]) -> f32 {
    if samples.is_empty() {
        return 0.0;
    }
    let count = samples.len() as f32;
    let mean = samples.iter().sum::<f32>() / count;
    samples
        .iter()
        .fold(0.0_f32, |peak, sample| peak.max((sample - mean).abs()))
}

/// The peak of the audio received in the last quarter second, 0 to 1, or
/// `None` when none arrived: the microphone is closed, released while idle,
/// failed, or not allowed.
pub fn recent() -> Option<f32> {
    unpack(LATEST.load(Ordering::Relaxed), clock())
}

fn pack(peak: f32, at: u32) -> u64 {
    (u64::from(peak.to_bits()) << 32) | u64::from(at)
}

fn unpack(packed: u64, now: u32) -> Option<f32> {
    let at = packed as u32;
    if at == 0 || now.wrapping_sub(at) > FRESH_MS {
        return None;
    }
    Some(f32::from_bits((packed >> 32) as u32).clamp(0.0, 1.0))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_peak_is_current_for_a_quarter_second() {
        let packed = pack(0.5, 1_000);
        assert_eq!(unpack(packed, 1_000), Some(0.5));
        assert_eq!(unpack(packed, 1_250), Some(0.5));
        assert_eq!(unpack(packed, 1_251), None);
    }

    #[test]
    fn nothing_recorded_reads_as_no_audio() {
        assert_eq!(unpack(0, 5), None);
    }

    #[test]
    fn a_clipped_peak_reads_as_full_scale() {
        assert_eq!(unpack(pack(1.7, u32::MAX), 3), Some(1.0));
    }

    #[test]
    fn recording_a_chunk_makes_its_peak_current() {
        record(&[0.25, -0.5, 0.25]);
        assert_eq!(recent(), Some(0.5));
    }

    /// Some microphones (a Ryzen built-in digital microphone, #841) carry a
    /// DC offset louder than the speech; the bars follow the signal, not the
    /// offset, as the HUD's meter does.
    #[test]
    fn a_dc_offset_never_reads_as_a_level() {
        assert!(chunk_peak(&[0.2; 512]) < 1e-5);
        assert!((chunk_peak(&[0.7, -0.3]) - 0.5).abs() < 1e-6);
    }
}
