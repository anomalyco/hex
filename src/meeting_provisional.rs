use std::time::Duration;

pub const FIRST_TAIL: usize = 16_000 * 2;
pub const GROWTH: usize = 16_000 * 3;
pub const SLOW_PASS: Duration = Duration::from_secs(4);

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum DraftStep {
    Wait,
    Transcribe,
    Stop,
}

#[derive(Clone, Copy, Debug)]
pub struct LastPass {
    pub elapsed: Duration,
    pub samples: usize,
}

pub fn draft_step(tail: usize, drafted: usize, stopped: bool, last: Option<LastPass>) -> DraftStep {
    if stopped || last.is_some_and(|pass| too_slow(pass.elapsed, pass.samples)) {
        return DraftStep::Stop;
    }
    let drafted = if drafted > tail { 0 } else { drafted };
    let ready = if drafted == 0 {
        tail >= FIRST_TAIL
    } else {
        let needed = GROWTH.max(last.map(|pass| samples_for(pass.elapsed)).unwrap_or(0));
        tail - drafted >= needed
    };
    if ready {
        DraftStep::Transcribe
    } else {
        DraftStep::Wait
    }
}

fn too_slow(elapsed: Duration, samples: usize) -> bool {
    elapsed > SLOW_PASS || elapsed.as_millis() > samples as u128 * 1_000 / 16_000
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
        assert_eq!(draft_step(FIRST_TAIL - 1, 0, false, None), DraftStep::Wait);
        assert_eq!(
            draft_step(FIRST_TAIL, 0, false, None),
            DraftStep::Transcribe
        );
        let fast = pass(Duration::from_millis(65), FIRST_TAIL);
        assert_eq!(
            draft_step(FIRST_TAIL + GROWTH - 1, FIRST_TAIL, false, fast),
            DraftStep::Wait
        );
        assert_eq!(
            draft_step(FIRST_TAIL + GROWTH, FIRST_TAIL, false, fast),
            DraftStep::Transcribe
        );
    }

    #[test]
    fn a_slow_pass_stops_or_waits_for_enough_new_audio() {
        let audio = FIRST_TAIL;
        let slower_than_audio = Duration::from_millis(audio as u64 * 1_000 / 16_000 + 1);
        assert_eq!(
            draft_step(audio + GROWTH, audio, false, pass(slower_than_audio, audio)),
            DraftStep::Stop
        );
        let ten_seconds = 10 * 16_000;
        assert_eq!(
            draft_step(
                ten_seconds + GROWTH,
                ten_seconds,
                false,
                pass(SLOW_PASS + Duration::from_millis(1), ten_seconds)
            ),
            DraftStep::Stop
        );
        let four_seconds = 4 * 16_000;
        assert_eq!(
            draft_step(
                ten_seconds + GROWTH,
                ten_seconds,
                false,
                pass(SLOW_PASS, ten_seconds)
            ),
            DraftStep::Wait
        );
        assert_eq!(
            draft_step(
                ten_seconds + four_seconds,
                ten_seconds,
                false,
                pass(SLOW_PASS, ten_seconds)
            ),
            DraftStep::Transcribe
        );
        assert_eq!(
            draft_step(FIRST_TAIL, FIRST_TAIL + GROWTH, false, None),
            DraftStep::Transcribe
        );
        assert_eq!(
            draft_step(audio + GROWTH, audio, true, None),
            DraftStep::Stop
        );

        let eight_seconds = 8 * 16_000;
        let elapsed = Duration::from_millis(3_500);
        let min_growth = 3_500 * 16;
        assert!(min_growth > GROWTH);
        assert_eq!(
            draft_step(
                eight_seconds + min_growth - 1,
                eight_seconds,
                false,
                pass(elapsed, eight_seconds)
            ),
            DraftStep::Wait
        );
        assert_eq!(
            draft_step(
                eight_seconds + min_growth,
                eight_seconds,
                false,
                pass(elapsed, eight_seconds)
            ),
            DraftStep::Transcribe
        );
    }
}
