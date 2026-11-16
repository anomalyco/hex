//! A meeting chunk's loudness before it is transcribed.
//!
//! When another call app holds the built-in microphone with voice processing,
//! Hex's own microphone track can be recorded tens of dB quiet: live, but too
//! quiet for the silence gate, the speech detector and the models. Microphone
//! chunks are therefore raised to the level the models hear, while the
//! recording, the pause segmenter and the echo envelopes keep the audio
//! exactly as it was captured.

/// The frame a chunk's level is measured over: 20 ms at 16 kHz, the same
/// frame the silence gate and the pause segmenter use.
const FRAME: usize = 320;

/// Where a raised chunk lands: the 95th percentile of its 20 ms frame RMS
/// levels, in dBFS.
const TARGET_DBFS: f32 = -25.0;

/// A chunk at least this loud is left alone. A microphone no other app holds
/// records speech near here.
const NORMAL_DBFS: f32 = -35.0;

/// The most a chunk is raised, so an amplified room stays a room.
const MAX_GAIN_DB: f32 = 30.0;

/// Below this a chunk is only room noise or digital silence, and raising it
/// would turn noise into something the model may invent words for. It is left
/// to the silence gate and the speech detector.
const FLOOR_DBFS: f32 = -60.0;

/// 20 ms of digital silence reported as a level, in dBFS: an energy of 1e-12.
const SILENT_FRAME: f32 = -120.0;

/// Raises a quiet microphone chunk in place. A loud chunk, one near silence,
/// and one between the two but already above [`NORMAL_DBFS`] are untouched,
/// and a chunk is never attenuated.
pub(super) fn raise_quiet_microphone(samples: &mut [f32]) {
    let db = gain_db(frame_percentile_dbfs(samples, 0.95));
    if db == 0.0 {
        return;
    }
    let gain = 10_f32.powf(db / 20.0);
    for sample in samples.iter_mut() {
        *sample = (*sample * gain).clamp(-1.0, 1.0);
    }
}

/// How far a chunk whose frames sit at `percentile_dbfs` is raised, in dB.
/// `None` is no audio at all, and no raise.
fn gain_db(percentile_dbfs: Option<f32>) -> f32 {
    let Some(dbfs) = percentile_dbfs else {
        return 0.0;
    };
    if dbfs < FLOOR_DBFS || dbfs >= NORMAL_DBFS {
        return 0.0;
    }
    // `dbfs` is under `NORMAL_DBFS`, itself under `TARGET_DBFS`', so the lift
    // is always a raise, never an attenuation.
    (TARGET_DBFS - dbfs).min(MAX_GAIN_DB)
}

/// The `fraction` percentile of `samples`' 20 ms frame RMS levels, in dBFS.
/// A frame shorter than [`FRAME`] counts with its own length, as the silence
/// gate measures it.
pub(super) fn frame_percentile_dbfs(samples: &[f32], fraction: f32) -> Option<f32> {
    if samples.is_empty() {
        return None;
    }
    let mut levels = samples.chunks(FRAME).map(frame_dbfs).collect::<Vec<_>>();
    levels.sort_by(f32::total_cmp);
    let position = ((levels.len() - 1) as f32 * fraction).round() as usize;
    levels.get(position).copied()
}

fn frame_dbfs(frame: &[f32]) -> f32 {
    let energy = frame.iter().map(|sample| sample * sample).sum::<f32>() / frame.len() as f32;
    // 1e-12 energy is -120 dBFS, so digital silence has a level.
    (10.0 * energy.max(1e-12).log10()).max(SILENT_FRAME)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Deterministic noise whose 20 ms frames all sit at `dbfs` RMS.
    fn noise(dbfs: f32) -> Vec<f32> {
        let amplitude = 10_f32.powf(dbfs / 20.0) * 3f32.sqrt();
        let mut seed = 7u32;
        (0..FRAME * 200)
            .map(|_| {
                seed = seed.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
                amplitude * ((seed >> 8) as f32 / (1u32 << 24) as f32 * 2.0 - 1.0)
            })
            .collect()
    }

    fn level(samples: &[f32]) -> f32 {
        frame_percentile_dbfs(samples, 0.95).unwrap()
    }

    #[test]
    fn a_quiet_chunk_is_raised_to_the_target_level() {
        let mut samples = noise(-52.0);
        raise_quiet_microphone(&mut samples);
        assert!(
            (level(&samples) - TARGET_DBFS).abs() < 0.5,
            "{}",
            level(&samples)
        );
    }

    #[test]
    fn a_normal_chunk_is_left_bit_identical() {
        let original = noise(-20.0);
        let mut samples = original.clone();
        raise_quiet_microphone(&mut samples);
        assert_eq!(samples, original);
    }

    #[test]
    fn a_near_silent_chunk_is_left_bit_identical() {
        let original = noise(-70.0);
        let mut samples = original.clone();
        raise_quiet_microphone(&mut samples);
        assert_eq!(samples, original);
    }

    #[test]
    fn a_click_does_not_suppress_the_raise() {
        let mut clicked = noise(-52.0);
        clicked[FRAME * 3 + 7] = 0.9;
        let mut samples = clicked.clone();
        raise_quiet_microphone(&mut samples);
        // The click is one frame, outside the 95th percentile, so the chunk is
        // raised as if it were not there.
        assert!(
            (level(&samples) - TARGET_DBFS).abs() < 0.5,
            "{}",
            level(&samples)
        );
    }

    #[test]
    fn the_raise_stops_at_thirty_db() {
        // -58 dBFS would need 33 dB to reach the target, so it stops 3 dB
        // short of it.
        let mut samples = noise(-58.0);
        raise_quiet_microphone(&mut samples);
        assert!((level(&samples) + 28.0).abs() < 0.5, "{}", level(&samples));
    }

    #[test]
    fn a_raised_chunk_is_clamped_to_full_scale() {
        let mut samples = noise(-58.0);
        samples[FRAME * 9 + 11] = 0.9;
        raise_quiet_microphone(&mut samples);
        assert_eq!(samples[FRAME * 9 + 11], 1.0);
        assert!(samples.iter().all(|sample| (-1.0..=1.0).contains(sample)));
    }
}
