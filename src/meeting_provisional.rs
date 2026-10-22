use std::time::Duration;

pub const FIRST_TAIL: usize = 16_000 * 2;
pub const GROWTH: usize = 16_000 * 3;
pub const SLOW_PASS: Duration = Duration::from_secs(4);

#[derive(Clone, Copy, Debug)]
pub struct LastPass {
    pub elapsed: Duration,
    pub samples: usize,
}

impl LastPass {
    /// Slower than its audio, or than `SLOW_PASS`: drafting cannot keep up.
    pub fn too_slow(&self) -> bool {
        self.elapsed > SLOW_PASS || self.elapsed.as_millis() > self.samples as u128 * 1_000 / 16_000
    }
}

/// Whether an uncommitted tail of `tail` samples, `drafted` of which the
/// last pass covered, is due for another pass.
pub fn should_draft(tail: usize, drafted: usize, last: Option<LastPass>) -> bool {
    if drafted == 0 || drafted > tail {
        return tail >= FIRST_TAIL;
    }
    let needed = GROWTH.max(last.map_or(0, |pass| samples_for(pass.elapsed)));
    tail - drafted >= needed
}

fn samples_for(elapsed: Duration) -> usize {
    (elapsed.as_millis() as usize).saturating_mul(16)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn pass(elapsed: Duration, samples: usize) -> Option<LastPass> {
        Some(LastPass { elapsed, samples })
    }

    #[test]
    fn draft_waits_until_two_seconds_then_every_three_seconds_of_new_audio() {
        assert!(!should_draft(FIRST_TAIL - 1, 0, None));
        assert!(should_draft(FIRST_TAIL, 0, None));
        let fast = pass(Duration::from_millis(65), FIRST_TAIL);
        assert!(!should_draft(FIRST_TAIL + GROWTH - 1, FIRST_TAIL, fast));
        assert!(should_draft(FIRST_TAIL + GROWTH, FIRST_TAIL, fast));
        assert!(should_draft(FIRST_TAIL, FIRST_TAIL + GROWTH, None));
    }

    #[test]
    fn a_slow_pass_stops_or_waits_for_enough_new_audio() {
        let audio = FIRST_TAIL;
        let slower_than_audio = Duration::from_millis(audio as u64 * 1_000 / 16_000 + 1);
        assert!(pass(slower_than_audio, audio).unwrap().too_slow());
        let ten_seconds = 10 * 16_000;
        assert!(
            pass(SLOW_PASS + Duration::from_millis(1), ten_seconds)
                .unwrap()
                .too_slow()
        );
        assert!(!pass(SLOW_PASS, ten_seconds).unwrap().too_slow());

        let four_seconds = 4 * 16_000;
        let at_limit = pass(SLOW_PASS, ten_seconds);
        assert!(!should_draft(ten_seconds + GROWTH, ten_seconds, at_limit));
        assert!(should_draft(
            ten_seconds + four_seconds,
            ten_seconds,
            at_limit
        ));

        let eight_seconds = 8 * 16_000;
        let elapsed = pass(Duration::from_millis(3_500), eight_seconds);
        let min_growth = 3_500 * 16;
        assert!(min_growth > GROWTH);
        assert!(!should_draft(
            eight_seconds + min_growth - 1,
            eight_seconds,
            elapsed
        ));
        assert!(should_draft(
            eight_seconds + min_growth,
            eight_seconds,
            elapsed
        ));
    }
}
