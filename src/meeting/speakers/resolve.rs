//! Who said each stretch of remote speech, worked out from the evidence.
//!
//! Everyone else on a call arrives on one track, so Hex knows to the word
//! when remote speech happened and not who spoke. The call app knows who
//! held the floor and not which words were theirs. [`resolve`] lines the
//! two up, and names a stretch only when one person plainly held the floor
//! across it. Everything else is nobody's, which a transcript shows as
//! `Them`.
//!
//! It is a function of its four arguments and nothing else: no file, no
//! clock, no call app. The words, the evidence, the calibration and the
//! strictness are independent, so one meeting's evidence can be resolved
//! again under another rule.
//!
//! The limit it cannot get past: the call app marks one speaker at a time,
//! so a word said over the person holding the floor is given to that
//! person. No rule that reads only the mark can tell.

use std::ops::Range;

use super::calibrate::{Calibration, Timing};
use super::evidence::{Correction, Evidence, Interval, ParticipantKey, Untrusted};
use super::floor::{Blind, Floor, POLL_MS, Roster, State};

/// A pause this long in the remote track ends a span. A change of floor
/// that falls in such a pause is clean: the words either side are kept.
const SPAN_GAP_MS: u64 = 300;
/// One of the reader's looks: how far the end of a hold, or its start, can
/// be from the truth.
const LOOK_MS: u64 = POLL_MS;
/// Under [`Strictness::SteadyWindow`], how far a word must be from a change
/// of floor inside continuous speech to keep its name.
const STEADY_MARGIN_MS: u64 = 750;
/// Under [`Strictness::WholeSegment`], a pause this long ends a segment,
/// and a longer segment is judged this much at a time.
const SEGMENT_PAUSE_MS: u64 = 600;
const SEGMENT_MS: u64 = 10_000;

/// How sure Hex must be before it names a stretch. Stricter settings trade
/// names for fewer wrong ones; none removes the wrong names that speaking
/// over someone causes.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub enum Strictness {
    /// Each word takes the name the call app showed for it, moved back by
    /// the mark's lag.
    AsSeen,
    /// A span is named when one person alone held the floor across it.
    /// Where the floor changes inside continuous speech, the words beside
    /// the change go unnamed, and so do words under a lingering mark.
    #[default]
    SteadyWindow,
    /// A whole segment of speech is named, or none of it: one person on at
    /// least four words in five, and anyone else or no view of the floor on
    /// at most one in ten.
    WholeSegment,
}

/// Why a stretch has a name.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Why {
    /// The user said so.
    Corrected,
    /// The call app showed one remote person for the whole recording.
    OnlyRemoteParticipant,
    /// They were the one remote person holding the floor.
    HeldFloor,
}

/// Why a stretch has no name.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum WhyNot {
    /// The user said they are not sure.
    Corrected,
    /// The user's own tile did not follow the user's microphone, so nothing
    /// the reader saw is believed.
    Unhealthy,
    /// No look at the call covers it.
    NoEvidence,
    /// A look does, and it could not be trusted.
    Untrusted(Untrusted),
    /// The call app showed nobody speaking, or a mark that was lingering.
    NobodyHeldFloor,
    /// The call app showed the user speaking.
    LocalHeldFloor,
    /// The floor changed hands too close by, inside continuous speech.
    FloorChanged,
    /// Under [`Strictness::WholeSegment`]: no one person held enough of
    /// the segment.
    Mixed,
}

/// Who said a stretch of remote speech.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Said {
    By(ParticipantKey, Why),
    ByNobody(WhyNot),
}

/// One stretch of remote speech and who said it.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct Attribution {
    pub span: Interval,
    pub said: Said,
}

/// Cuts the remote track's `words` into spans and says who said each: a
/// span ends at a pause, and wherever the answer changes. Every word falls
/// in exactly one span, and the spans come back in the words' order.
///
/// In order of rank: what the user corrected stands; a call judged
/// unhealthy names nobody; a call with one remote person gives them every
/// word; otherwise the floor decides, as strictly as `strictness` asks.
pub fn resolve(
    words: &[Interval],
    evidence: &[Evidence],
    calibration: Calibration,
    strictness: Strictness,
) -> Vec<Attribution> {
    let mut words = words.to_vec();
    words.sort_by_key(|word| (word.start_ms(), word.end_ms()));
    let said = each_word(&words, evidence, calibration, strictness);

    let mut attributions: Vec<Attribution> = Vec::new();
    for span in runs(&words, SPAN_GAP_MS) {
        for index in span.clone() {
            let (word, said) = (words[index], said[index]);
            match attributions.last_mut() {
                Some(last) if index > span.start && last.said == said => {
                    last.span =
                        Interval::new(last.span.start_ms(), last.span.end_ms().max(word.end_ms()));
                }
                _ => attributions.push(Attribution { span: word, said }),
            }
        }
    }
    attributions
}

/// Who said each of `words`, which are in order of their starts.
pub(super) fn each_word(
    words: &[Interval],
    evidence: &[Evidence],
    calibration: Calibration,
    strictness: Strictness,
) -> Vec<Said> {
    let roster = Roster::of(evidence);
    let mut said = match (calibration.timing(), roster.only_remote()) {
        (None, _) => vec![Said::ByNobody(WhyNot::Unhealthy); words.len()],
        (Some(_), Some(only)) => vec![Said::By(only, Why::OnlyRemoteParticipant); words.len()],
        (Some(timing), None) => by_floor(words, evidence, &roster, timing, strictness),
    };
    for line in evidence {
        let Evidence::Correction(Correction::Said { during, by }) = line else {
            continue;
        };
        let corrected = match by {
            Some(who) => Said::By(*who, Why::Corrected),
            None => Said::ByNobody(WhyNot::Corrected),
        };
        for (word, said) in words.iter().zip(&mut said) {
            if during.contains(word.middle_ms()) {
                *said = corrected;
            }
        }
    }
    said
}

fn by_floor(
    words: &[Interval],
    evidence: &[Evidence],
    roster: &Roster<'_>,
    timing: Timing,
    strictness: Strictness,
) -> Vec<Said> {
    // A hold's end is a look, so it can be one look short of when the mark
    // went: that much of the tail is given back.
    let trim_ms = match strictness {
        Strictness::AsSeen => 0,
        Strictness::SteadyWindow | Strictness::WholeSegment => {
            timing.release_tail_ms.saturating_sub(LOOK_MS)
        }
    };
    let floor = Floor::seen(evidence, roster).heard(timing.onset_lag_ms, trim_ms);
    let states = words
        .iter()
        .map(|word| floor.at(word.middle_ms()))
        .collect::<Vec<_>>();
    let mut said = states
        .iter()
        .map(|state| as_seen(*state, roster))
        .collect::<Vec<_>>();
    match strictness {
        Strictness::AsSeen => {}
        Strictness::SteadyWindow => {
            for span in runs(words, SPAN_GAP_MS) {
                let (words, states) = (&words[span.clone()], &states[span.clone()]);
                steady(&floor, roster, words, states, &mut said[span]);
            }
        }
        Strictness::WholeSegment => {
            for segment in runs(words, SEGMENT_PAUSE_MS) {
                let start_ms = words[segment.start].start_ms();
                let part = |index: usize| (words[index].middle_ms() - start_ms) / SEGMENT_MS;
                let mut from = segment.start;
                for index in segment.start + 1..=segment.end {
                    if index == segment.end || part(index) != part(from) {
                        whole(roster, &states[from..index], &mut said[from..index]);
                        from = index;
                    }
                }
            }
        }
    }
    said
}

/// A word by the floor at its own moment and nothing else.
fn as_seen(state: State, roster: &Roster<'_>) -> Said {
    match state {
        State::Held(who) if roster.is_remote(who) => Said::By(who, Why::HeldFloor),
        State::Held(_) => Said::ByNobody(WhyNot::LocalHeldFloor),
        State::Handover(..) => Said::ByNobody(WhyNot::FloorChanged),
        State::Tail(_) | State::Onset(_) | State::Nobody => Said::ByNobody(WhyNot::NobodyHeldFloor),
        State::Unknown(Blind::Unobserved) => Said::ByNobody(WhyNot::NoEvidence),
        State::Unknown(Blind::Untrusted(why)) => Said::ByNobody(WhyNot::Untrusted(why)),
    }
}

/// One span under [`Strictness::SteadyWindow`].
///
/// When the span's whole time shows one remote person and nothing against
/// them, the change of floor fell in the pauses around it, so the words
/// said while the mark was coming on or passing over are theirs too, and
/// so is the half second before it came on. When anything else shows, a
/// word keeps its name only clear of all of it.
fn steady(
    floor: &Floor,
    roster: &Roster<'_>,
    words: &[Interval],
    states: &[State],
    said: &mut [Said],
) {
    let from_ms = words[0].start_ms();
    let to_ms = words
        .iter()
        .map(|word| word.end_ms())
        .max()
        .unwrap_or(from_ms);
    let mut holders = floor
        .within(from_ms, to_ms)
        .filter_map(|state| match state {
            State::Held(who) if roster.is_remote(who) => Some(who),
            _ => None,
        })
        .collect::<Vec<_>>();
    holders.sort_unstable();
    holders.dedup();

    for who in holders {
        if !floor.within(from_ms, to_ms).any(|state| state.against(who)) {
            let theirs = |state: State| match state {
                State::Held(holder) | State::Onset(holder) => holder == who,
                State::Handover(from, to) => from == who || to == who,
                State::Tail(_) | State::Nobody | State::Unknown(_) => false,
            };
            let Some(first) = states.iter().position(|state| theirs(*state)) else {
                continue;
            };
            let first_ms = words[first].middle_ms();
            for (index, state) in states.iter().enumerate() {
                let coming_on = index < first
                    && *state == State::Nobody
                    && first_ms - words[index].middle_ms() <= LOOK_MS;
                if theirs(*state) || coming_on {
                    said[index] = Said::By(who, Why::HeldFloor);
                }
            }
        } else {
            for (index, word) in words.iter().enumerate() {
                let near_from = word
                    .middle_ms()
                    .saturating_sub(STEADY_MARGIN_MS)
                    .max(from_ms);
                let near_to = (word.middle_ms() + STEADY_MARGIN_MS).min(to_ms);
                let unsteady =
                    |state: State| state.against(who) || matches!(state, State::Handover(..));
                if states[index] == State::Held(who)
                    && floor.within(near_from, near_to).any(unsteady)
                {
                    said[index] = Said::ByNobody(WhyNot::FloorChanged);
                }
            }
        }
    }
}

/// One segment, or ten seconds of one, under [`Strictness::WholeSegment`].
fn whole(roster: &Roster<'_>, states: &[State], said: &mut [Said]) {
    let share =
        |counts: &dyn Fn(State) -> bool| states.iter().filter(|state| counts(**state)).count();
    let holder = states.iter().find_map(|state| match state {
        State::Held(who) if roster.is_remote(*who) => {
            let held = share(&|state| state == State::Held(*who));
            let against = share(&|state| match state {
                State::Held(other) | State::Tail(other) => other != *who,
                State::Unknown(_) => true,
                State::Onset(_) | State::Handover(..) | State::Nobody => false,
            });
            (held * 5 >= states.len() * 4 && against * 10 <= states.len()).then_some(*who)
        }
        _ => None,
    });
    for said in said {
        match holder {
            Some(who) => *said = Said::By(who, Why::HeldFloor),
            None if matches!(said, Said::By(..)) => *said = Said::ByNobody(WhyNot::Mixed),
            None => {}
        }
    }
}

/// The runs of `words` with no pause of `gap_ms` or more inside them.
fn runs(words: &[Interval], gap_ms: u64) -> Vec<Range<usize>> {
    let mut runs: Vec<Range<usize>> = Vec::new();
    let mut heard_until_ms = 0;
    for (index, word) in words.iter().enumerate() {
        match runs.last_mut() {
            Some(run) if word.start_ms() < heard_until_ms + gap_ms => run.end = index + 1,
            _ => runs.push(index..index + 1),
        }
        heard_until_ms = heard_until_ms.max(word.end_ms());
    }
    runs
}
