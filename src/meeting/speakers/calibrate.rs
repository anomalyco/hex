//! How the call app's speaking mark behaves on this call, measured on the
//! one person Hex can check: the user.
//!
//! Hex hears the user's microphone and sees the user's own tile. Comparing
//! the two gives how long the mark takes to come on after someone starts
//! talking and how long it lingers after they stop, and it shows whether
//! the reader is reading the call at all. If the user's tile lights while
//! the microphone is silent, or never lights while the user talks, nothing
//! the reader saw is used.

use super::evidence::Interval;
use super::floor::POLL_MS;

/// The mark's delays. Until a call has shown enough, Hex assumes the slow
/// end of what call apps were described as doing: half a second to come on
/// as the reader sees it, a second and a half to go off.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct Timing {
    /// From the start of speech to the first look that shows the mark.
    pub onset_lag_ms: u64,
    /// From the end of speech to the last look that shows the mark.
    pub release_tail_ms: u64,
}

impl Default for Timing {
    fn default() -> Self {
        Self {
            onset_lag_ms: 500,
            release_tail_ms: 1_500,
        }
    }
}

/// What a call has shown about its own speaking mark.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Calibration {
    /// Too little to judge. Names are still worked out, on the assumed
    /// timing.
    Unproven,
    /// The user's tile follows the user's microphone, with this timing.
    Healthy(Timing),
    /// It does not. Nobody is named from what the reader saw.
    Unhealthy,
}

impl Calibration {
    /// The timing to place the floor with, or none when the floor is not
    /// to be believed.
    pub fn timing(self) -> Option<Timing> {
        match self {
            Self::Unproven => Some(Timing::default()),
            Self::Healthy(timing) => Some(timing),
            Self::Unhealthy => None,
        }
    }
}

/// How many times the user's tile must light before the call is judged.
const ENOUGH_TURNS: usize = 5;
/// How many measurements a delay needs before it replaces the assumed one.
const ENOUGH_SAMPLES: usize = 3;
/// A mark that comes on later than this after speech was not set off by it.
const LONGEST_LAG_MS: u64 = 2_000;
/// A mark that outlasts speech by more than this was not kept on by it.
const LONGEST_TAIL_MS: u64 = 4_000;
/// How close the end of speech may be to the mark coming on and still be
/// what set it off: one look.
const LOOK_MS: u64 = POLL_MS;
/// The user talking this long, this many times, with the tile never
/// lighting, is a reader that cannot see the tile.
const UNSEEN_TURN_MS: u64 = 2_000;
const UNSEEN_TURNS: usize = 8;

/// Judges a call from when the user's tile was marked
/// ([`local_floor`](super::floor::local_floor)) and when the user's
/// microphone heard speech, both on the recording's clock.
pub fn calibrate(local_floor: &[Interval], microphone_speech: &[Interval]) -> Calibration {
    let mut explained = 0;
    let mut lags = Vec::new();
    let mut tails = Vec::new();
    for lit in local_floor {
        let spoke_near = microphone_speech.iter().any(|speech| {
            speech.start_ms() <= lit.end_ms() && speech.end_ms() + LONGEST_LAG_MS >= lit.start_ms()
        });
        if !spoke_near {
            continue;
        }
        explained += 1;
        // The speech under way when the mark came on set it off.
        let started = microphone_speech
            .iter()
            .filter(|speech| {
                speech.start_ms() <= lit.start_ms() && speech.end_ms() + LOOK_MS >= lit.start_ms()
            })
            .map(|speech| lit.start_ms() - speech.start_ms())
            .min();
        lags.extend(started.filter(|lag| *lag <= LONGEST_LAG_MS));
        // The last speech to end under the mark is what it outlasted,
        // unless the user was still talking when it went.
        let still_talking = microphone_speech
            .iter()
            .any(|speech| speech.contains(lit.end_ms()) && speech.end_ms() > lit.end_ms());
        let stopped = microphone_speech
            .iter()
            .filter(|speech| lit.contains(speech.end_ms()))
            .map(|speech| lit.end_ms() - speech.end_ms())
            .min();
        tails.extend(stopped.filter(|tail| !still_talking && *tail <= LONGEST_TAIL_MS));
    }

    if local_floor.len() < ENOUGH_TURNS {
        let unseen = microphone_speech
            .iter()
            .filter(|speech| speech.end_ms() - speech.start_ms() >= UNSEEN_TURN_MS)
            .count();
        return if explained == 0 && unseen >= UNSEEN_TURNS {
            Calibration::Unhealthy
        } else {
            Calibration::Unproven
        };
    }
    if explained * 5 < local_floor.len() * 4 {
        return Calibration::Unhealthy;
    }
    let assumed = Timing::default();
    Calibration::Healthy(Timing {
        // The middle lag: a turn begun over someone else lights late.
        onset_lag_ms: ranked(&mut lags, 1, 2).unwrap_or(assumed.onset_lag_ms),
        // A long tail: one cut short by the next speaker reads short.
        release_tail_ms: ranked(&mut tails, 4, 5).unwrap_or(assumed.release_tail_ms),
    })
}

/// The sample `numerator / denominator` of the way up, once there are
/// enough to trust.
fn ranked(samples: &mut [u64], numerator: usize, denominator: usize) -> Option<u64> {
    samples.sort_unstable();
    (samples.len() >= ENOUGH_SAMPLES)
        .then(|| samples[(samples.len() * numerator / denominator).min(samples.len() - 1)])
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The user says something for four seconds every ten.
    fn turns(count: u64) -> Vec<Interval> {
        (0..count)
            .map(|turn| Interval::new(turn * 10_000, turn * 10_000 + 4_000))
            .collect()
    }

    /// A tile that comes on `lag` after each turn starts and stays `tail`
    /// after it ends.
    fn tile(turns: &[Interval], lag: u64, tail: u64) -> Vec<Interval> {
        turns
            .iter()
            .map(|turn| Interval::new(turn.start_ms() + lag, turn.end_ms() + tail))
            .collect()
    }

    #[test]
    fn a_call_that_has_shown_too_little_runs_on_the_assumed_timing() {
        let spoken = turns(4);
        for floor in [Vec::new(), tile(&spoken, 700, 1_300)] {
            let calibration = calibrate(&floor, &spoken);
            assert_eq!(calibration, Calibration::Unproven);
            assert_eq!(calibration.timing(), Some(Timing::default()));
        }
        assert_eq!(
            Timing::default(),
            Timing {
                onset_lag_ms: 500,
                release_tail_ms: 1_500
            }
        );
    }

    #[test]
    fn a_tile_that_follows_the_microphone_is_healthy_and_gives_its_timing() {
        let spoken = turns(6);
        let calibration = calibrate(&tile(&spoken, 700, 1_300), &spoken);
        let timing = Timing {
            onset_lag_ms: 700,
            release_tail_ms: 1_300,
        };
        assert_eq!(calibration, Calibration::Healthy(timing));
        assert_eq!(calibration.timing(), Some(timing));
    }

    #[test]
    fn odd_turns_do_not_move_the_timing() {
        let spoken = turns(7);
        let mut floor = tile(&spoken, 600, 1_400);
        // One turn begun over someone else lit late, and one was cut off by
        // the next speaker as soon as it ended.
        floor[2] = Interval::new(spoken[2].start_ms() + 1_900, spoken[2].end_ms() + 1_400);
        floor[4] = Interval::new(spoken[4].start_ms() + 600, spoken[4].end_ms() + 100);
        assert_eq!(
            calibrate(&floor, &spoken),
            Calibration::Healthy(Timing {
                onset_lag_ms: 600,
                release_tail_ms: 1_400
            })
        );
    }

    #[test]
    fn a_delay_seen_too_few_times_stays_assumed() {
        // The user is still talking each time the mark goes, so no tail
        // can be measured; the lag can.
        let spoken = turns(6);
        let floor = spoken
            .iter()
            .map(|turn| Interval::new(turn.start_ms() + 800, turn.end_ms() - 1_000))
            .collect::<Vec<_>>();
        assert_eq!(
            calibrate(&floor, &spoken),
            Calibration::Healthy(Timing {
                onset_lag_ms: 800,
                release_tail_ms: Timing::default().release_tail_ms
            })
        );
    }

    #[test]
    fn a_tile_that_lights_while_the_microphone_is_silent_is_unhealthy() {
        let spoken = turns(6);
        // Someone else's tile: lit in the user's silences.
        let floor = spoken
            .iter()
            .map(|turn| Interval::new(turn.end_ms() + 2_500, turn.end_ms() + 5_000))
            .collect::<Vec<_>>();
        let calibration = calibrate(&floor, &spoken);
        assert_eq!(calibration, Calibration::Unhealthy);
        assert_eq!(calibration.timing(), None);
        // One stray light in six is still healthy; two are not.
        let mut mostly = tile(&spoken, 700, 1_300);
        mostly[0] = floor[0];
        assert!(matches!(
            calibrate(&mostly, &spoken),
            Calibration::Healthy(_)
        ));
        mostly[1] = floor[1];
        assert_eq!(calibrate(&mostly, &spoken), Calibration::Unhealthy);
    }

    #[test]
    fn a_tile_that_never_lights_while_the_user_talks_is_unhealthy() {
        assert_eq!(calibrate(&[], &turns(8)), Calibration::Unhealthy);
        assert_eq!(calibrate(&[], &turns(7)), Calibration::Unproven);
        // Short remarks may never take the floor, so they prove nothing.
        let remarks = (0..20)
            .map(|turn| Interval::new(turn * 5_000, turn * 5_000 + 900))
            .collect::<Vec<_>>();
        assert_eq!(calibrate(&[], &remarks), Calibration::Unproven);
        assert_eq!(calibrate(&[], &[]), Calibration::Unproven);
    }
}
