//! A meeting track's recent loudness, and the gain a microphone chunk gets.
//!
//! When another call app holds the built-in microphone with voice processing,
//! Hex's own microphone track can be recorded tens of dB quiet: live, but too
//! quiet for the speech detector and the models. The decision is the track's,
//! not the chunk's. A microphone that reached a normal level anywhere in the
//! last minute is transcribed exactly as captured, so a quiet word between
//! louder ones, and a chunk between words, are never raised. Only a track
//! that stayed under that level throughout the window is raised.
//!
//! Each one-second block of the window keeps the level two of its 20 ms
//! frames reached, so one click cannot make a quiet microphone look normal
//! while a short word, many frames long, still does.
//!
//! The recording, the pause segmenter and the echo envelopes keep the
//! captured audio. The raise happens when a chunk is made, and it does not
//! change the -60 dB silence gate's decision: a chunk quiet enough to be
//! skipped gets no gain. It is there so the speech detector and the model
//! hear the words.

use std::collections::VecDeque;

/// The frame a level is measured over: 20 ms at 16 kHz, the same frame the
/// silence gate and the pause segmenter use.
const FRAME: usize = 320;

/// One second of 16 kHz samples: the block the window remembers.
const BLOCK: usize = 16_000;

/// Blocks remembered: the track's last minute.
const WINDOW: usize = 60;

/// Where a quiet track's loud level is raised to, in dBFS.
const TARGET_DBFS: f32 = -20.0;

/// A track whose loud level reached this anywhere in the window is normal and
/// is never raised. A microphone no other app holds records speech above it.
const NORMAL_DBFS: f32 = -35.0;

/// A track quieter than this throughout the window is only room noise or
/// digital silence; raising it would hand the speech detector amplified noise.
const FLOOR_DBFS: f32 = -60.0;

/// The most a track is raised.
const MAX_GAIN_DB: f32 = 30.0;

/// The loudest a raised chunk's sample may be: -1 dBFS, so nothing clips.
const MAX_CHUNK_PEAK: f32 = 0.891;

/// 20 ms of digital silence reported as a level, in dBFS: an energy of 1e-12.
const SILENT_FRAME: f32 = -120.0;

/// Two loudest 20 ms frames of one second, in dBFS.
#[derive(Clone, Copy)]
struct Block {
    loudest: f32,
    second: f32,
}

impl Default for Block {
    fn default() -> Self {
        Self {
            loudest: f32::NEG_INFINITY,
            second: f32::NEG_INFINITY,
        }
    }
}

impl Block {
    fn hear(&mut self, dbfs: f32) {
        if dbfs > self.loudest {
            self.second = self.loudest;
            self.loudest = dbfs;
        } else if dbfs > self.second {
            self.second = dbfs;
        }
    }

    /// The loudest level at least two of its frames reached, so a single
    /// click's frame is not a level the track carries.
    fn level(&self) -> f32 {
        self.second
    }
}

/// The loudness of a track's recent speech, a second at a time.
#[derive(Default)]
pub(super) struct TrackLevel {
    blocks: VecDeque<Block>,
    current: Block,
    samples: usize,
}

impl TrackLevel {
    /// Takes the next 20 ms frame of the track's own, captured audio.
    pub(super) fn hear(&mut self, frame: &[f32]) {
        self.current.hear(frame_dbfs(frame));
        self.samples += frame.len();
        if self.samples >= BLOCK {
            self.samples -= BLOCK;
            if self.blocks.len() == WINDOW {
                self.blocks.pop_front();
            }
            self.blocks.push_back(std::mem::take(&mut self.current));
        }
    }

    /// The loudest level the track's speech reached in the window, the block
    /// still filling included, or `None` before two frames were heard.
    pub(super) fn loud_dbfs(&self) -> Option<f32> {
        self.blocks
            .iter()
            .chain(std::iter::once(&self.current))
            .map(Block::level)
            .filter(|dbfs| dbfs.is_finite())
            .reduce(f32::max)
    }

    /// The gain a chunk of this track gets: one when the track is normal, near
    /// silence, or not heard yet; otherwise up to [`MAX_GAIN_DB`], which is
    /// always a raise.
    pub(super) fn gain(&self) -> f32 {
        let Some(loud) = self.loud_dbfs() else {
            return 1.0;
        };
        if loud >= NORMAL_DBFS || loud < FLOOR_DBFS {
            return 1.0;
        }
        let db = (TARGET_DBFS - loud).min(MAX_GAIN_DB);
        10_f32.powf(db / 20.0)
    }
}

/// Raises `samples` by `gain`, limited so the chunk's own peak stays at
/// [`MAX_CHUNK_PEAK`]. A chunk that already peaks louder is left alone, so a
/// chunk is never attenuated; the clamp is only a safety net.
pub(super) fn raise(samples: &mut [f32], gain: f32) {
    let peak = samples
        .iter()
        .fold(0.0_f32, |peak, sample| peak.max(sample.abs()));
    let gain = if peak > 0.0 {
        gain.min(MAX_CHUNK_PEAK / peak)
    } else {
        gain
    };
    if gain <= 1.0 {
        return;
    }
    for sample in samples.iter_mut() {
        *sample = (*sample * gain).clamp(-1.0, 1.0);
    }
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
    fn noise(dbfs: f32, samples: usize) -> Vec<f32> {
        let amplitude = 10_f32.powf(dbfs / 20.0) * 3f32.sqrt();
        let mut seed = 7u32;
        (0..samples)
            .map(|_| {
                seed = seed.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
                amplitude * ((seed >> 8) as f32 / (1u32 << 24) as f32 * 2.0 - 1.0)
            })
            .collect()
    }

    fn feed(level: &mut TrackLevel, seconds: f32, dbfs: f32) {
        for frame in noise(dbfs, (seconds * BLOCK as f32) as usize).chunks(FRAME) {
            level.hear(frame);
        }
    }

    fn gain_db(gain: f32) -> f32 {
        20.0 * gain.log10()
    }

    /// The loud level of `samples`, measured as the track measures it.
    fn loud(samples: &[f32]) -> f32 {
        let mut level = TrackLevel::default();
        for frame in samples.chunks(FRAME) {
            level.hear(frame);
        }
        level.loud_dbfs().unwrap()
    }

    #[test]
    fn a_normal_track_is_left_alone() {
        // A 300 ms word at -20 dBFS in 10 s of -50 dBFS background: the word
        // is many frames long, so it is a level the track carries.
        let mut level = TrackLevel::default();
        feed(&mut level, 4.0, -50.0);
        feed(&mut level, 0.3, -20.0);
        feed(&mut level, 5.7, -50.0);
        assert_eq!(level.gain(), 1.0);
    }

    #[test]
    fn background_after_a_normal_word_stays_alone() {
        let mut level = TrackLevel::default();
        feed(&mut level, 4.0, -50.0);
        feed(&mut level, 0.3, -20.0);
        feed(&mut level, 30.0, -50.0);
        assert_eq!(level.gain(), 1.0);
    }

    #[test]
    fn a_loud_word_leaves_the_window_after_a_minute() {
        let mut level = TrackLevel::default();
        feed(&mut level, 4.0, -50.0);
        feed(&mut level, 0.3, -20.0);
        feed(&mut level, 61.0, -50.0);
        // The word's block is out of the window, so the quiet track is raised
        // again, near the 30 dB cap.
        assert!(gain_db(level.gain()) > 25.0, "{}", gain_db(level.gain()));
    }

    #[test]
    fn a_quiet_track_is_raised_to_the_target() {
        let mut level = TrackLevel::default();
        feed(&mut level, 10.0, -48.0);
        let gain = level.gain();
        assert!((gain_db(gain) - 28.0).abs() < 1.0, "{}", gain_db(gain));
        let mut samples = noise(-48.0, BLOCK);
        raise(&mut samples, gain);
        assert!(
            (loud(&samples) - TARGET_DBFS).abs() < 1.0,
            "{}",
            loud(&samples)
        );
    }

    #[test]
    fn a_near_silent_track_is_left_bit_identical() {
        let mut level = TrackLevel::default();
        feed(&mut level, 10.0, -70.0);
        assert_eq!(level.gain(), 1.0);
        let original = noise(-70.0, BLOCK);
        let mut samples = original.clone();
        raise(&mut samples, level.gain());
        assert_eq!(samples, original);
    }

    #[test]
    fn the_raise_stops_at_thirty_db() {
        // -56 dBFS would need 36 dB to reach the target, so it stops 6 dB
        // short of it.
        let mut level = TrackLevel::default();
        feed(&mut level, 10.0, -56.0);
        assert!((gain_db(level.gain()) - MAX_GAIN_DB).abs() < 0.5);
    }

    #[test]
    fn a_single_click_does_not_make_a_quiet_track_normal() {
        let quiet = noise(-48.0, 10 * BLOCK);
        let mut clicked = quiet.clone();
        clicked[FRAME * 3 + 7] = 0.9;
        let decision = |samples: &[f32]| {
            let mut level = TrackLevel::default();
            for frame in samples.chunks(FRAME) {
                level.hear(frame);
            }
            gain_db(level.gain())
        };
        let (without, with) = (decision(&quiet), decision(&clicked));
        // The click is one frame, so the block's level is still the
        // background's and the track is raised as if it were not there.
        assert!(with > 25.0, "{with}");
        assert!((with - without).abs() < 1.0, "{without} {with}");
    }

    #[test]
    fn a_chunk_peak_stays_under_minus_one_dbfs() {
        let mut level = TrackLevel::default();
        feed(&mut level, 10.0, -48.0);
        let mut samples = noise(-48.0, BLOCK / 2);
        samples[3] = 0.3;
        raise(&mut samples, level.gain());
        let peak = samples
            .iter()
            .fold(0.0_f32, |peak, sample| peak.max(sample.abs()));
        assert!((peak - MAX_CHUNK_PEAK).abs() < 1e-4, "{peak}");
        assert_eq!(samples[3], peak);
    }

    #[test]
    fn an_already_loud_chunk_is_not_attenuated() {
        let original = [0.95_f32, -0.95];
        let mut samples = original;
        raise(&mut samples, 10.0);
        assert_eq!(samples, original);
    }
}
