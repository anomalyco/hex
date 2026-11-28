//! The resolver against a simulated call app.
//!
//! The call app here is the model the research simulation ran over real
//! meeting speech (reported in `measurements-indicator-sim.md`), ported
//! rule for rule: one person holds
//! the mark at a time, it comes on 0.3 s into a spurt of speech, the holder
//! keeps it while they talk even if others talk over them, it passes at the
//! end of the holder's spurt to someone who has been talking for 0.3 s, and
//! it goes off 1.5 s after the holder stops. The reader looks every 0.5 s.
//!
//! A scripted call gives every word a true speaker, so each test can count
//! the words named right, named wrong and left unnamed.

use super::SpeakerName;
use super::calibrate::{Calibration, calibrate};
use super::evidence::{Correction, Evidence, Interval, Participant, ParticipantKey, Untrusted};
use super::floor::local_floor;
use super::resolve::{Attribution, Said, Strictness, Why, WhyNot, each_word, resolve};

const D_ON: u64 = 300;
const D_OFF: u64 = 1_500;
const POLL: u64 = 500;
const PHASES: [u64; 4] = [0, 137, 250, 433];
const EVERY: [Strictness; 3] = [
    Strictness::AsSeen,
    Strictness::SteadyWindow,
    Strictness::WholeSegment,
];

/// A scripted call: when each person talked. Person 0 is the user, whose
/// words are on the microphone; everyone else shares the remote track.
struct Call {
    spurts: Vec<Vec<(u64, u64)>>,
}

fn key(person: usize) -> ParticipantKey {
    ParticipantKey::numbered(person as u32 + 1)
}

impl Call {
    fn of(people: usize) -> Self {
        Self {
            spurts: vec![Vec::new(); people],
        }
    }

    fn says(mut self, person: usize, start_ms: u64, end_ms: u64) -> Self {
        self.spurts[person].push((start_ms, end_ms));
        self.spurts[person].sort_unstable();
        self
    }

    fn end_ms(&self) -> u64 {
        let ends = self.spurts.iter().flatten().map(|(_, end)| *end);
        ends.max().unwrap_or_default()
    }

    /// When the mark changed hands, and to whom: the simulation's model.
    fn indicator(&self) -> Vec<(u64, Option<usize>)> {
        // The first moment from `from` on at which this person's spurt has
        // lasted D_ON, with the spurt's start, the person and the spurt.
        let earliest = |person: usize, from: u64| {
            let spurts = self.spurts[person].iter().enumerate();
            spurts
                .filter(|(_, (_, end))| *end >= from)
                .find_map(|(index, (start, end))| {
                    let at = (start + D_ON).max(from);
                    (at <= *end).then_some((at, *start, person, index))
                })
        };
        let people = 0..self.spurts.len();
        let mut changes = Vec::new();
        let mut holder: Option<(usize, usize)> = None;
        let mut from = 0;
        loop {
            match holder {
                None => {
                    let first = people.clone().filter_map(|person| earliest(person, from));
                    let Some((at, _, person, spurt)) = first.min() else {
                        break;
                    };
                    holder = Some((person, spurt));
                    changes.push((at, Some(person)));
                }
                Some((person, spurt)) => {
                    let end = self.spurts[person][spurt].1;
                    let next = self.spurts[person]
                        .get(spurt + 1)
                        .map_or(u64::MAX, |next| next.0);
                    let release = end + D_OFF;
                    let others = people.clone().filter(|other| *other != person);
                    let taker = others
                        .filter_map(|other| earliest(other, end))
                        .filter(|(at, ..)| *at < next.min(release))
                        .min();
                    if let Some((at, _, other, spurt)) = taker {
                        holder = Some((other, spurt));
                        changes.push((at, Some(other)));
                    } else if next <= release {
                        holder = Some((person, spurt + 1));
                    } else {
                        holder = None;
                        from = release;
                        changes.push((release, None));
                    }
                }
            }
        }
        changes
    }

    /// What the reader saw: everyone on the call, then a look every POLL
    /// from `phase_ms` until the mark has gone off for good.
    fn evidence(&self, phase_ms: u64) -> Vec<Evidence> {
        let mut evidence = (0..self.spurts.len())
            .map(|person| {
                Evidence::Participant(Participant {
                    key: key(person),
                    name: SpeakerName::parse(&format!("Person {person}")),
                    is_local: person == 0,
                    at_ms: 0,
                })
            })
            .collect::<Vec<_>>();
        let changes = self.indicator();
        let mut at_ms = phase_ms;
        while at_ms <= self.end_ms() + D_OFF + 2 * POLL {
            let shown = changes.iter().rev().find(|(changed, _)| *changed <= at_ms);
            evidence.push(Evidence::Floor {
                at_ms,
                holder: shown.and_then(|(_, holder)| holder.map(key)),
            });
            at_ms += POLL;
        }
        evidence
    }

    /// The remote track's words in order, and who truly said each: 0.2 s
    /// words 0.05 s apart for as long as each spurt lasts.
    fn words(&self) -> (Vec<Interval>, Vec<usize>) {
        let mut words = Vec::new();
        for (person, spurts) in self.spurts.iter().enumerate().skip(1) {
            for (start, end) in spurts {
                let mut at = *start;
                while at + 200 <= *end {
                    words.push((at, at + 200, person));
                    at += 250;
                }
            }
        }
        words.sort_unstable();
        let spoken = words
            .iter()
            .map(|(start, end, _)| Interval::new(*start, *end));
        (spoken.collect(), words.iter().map(|word| word.2).collect())
    }

    fn microphone(&self) -> Vec<Interval> {
        let spurts = self.spurts[0].iter();
        spurts
            .map(|(start, end)| Interval::new(*start, *end))
            .collect()
    }

    /// Resolves the call word by word on the assumed timing.
    fn heard(&self, phase_ms: u64, strictness: Strictness) -> Vec<Said> {
        let evidence = self.evidence(phase_ms);
        each_word(
            &self.words().0,
            &evidence,
            Calibration::Unproven,
            strictness,
        )
    }

    fn score(&self, said: &[Said]) -> Score {
        let mut score = Score::default();
        for (said, person) in said.iter().zip(self.words().1) {
            match said {
                Said::By(who, _) if *who == key(person) => score.right += 1,
                Said::By(..) => score.wrong += 1,
                Said::ByNobody(_) => score.unnamed += 1,
            }
        }
        score
    }
}

#[derive(Debug, Default, Eq, PartialEq)]
struct Score {
    right: usize,
    wrong: usize,
    unnamed: usize,
}

fn all_right(words: usize) -> Score {
    Score {
        right: words,
        ..Score::default()
    }
}

/// Three remote people taking turns, with a few seconds between them.
fn turn_taking() -> Call {
    Call::of(4)
        .says(1, 1_000, 6_000)
        .says(2, 9_000, 13_000)
        .says(3, 16_000, 19_000)
        .says(1, 22_000, 24_000)
}

/// One person, then the next a tenth of a second later: no pause the
/// remote track could be cut at.
fn run_on() -> Call {
    Call::of(4).says(1, 1_000, 5_000).says(2, 5_100, 9_000)
}

// MARK: Fixtures from the simulation

#[test]
fn people_taking_turns_are_each_named_for_every_word() {
    let call = turn_taking();
    let words = call.words().0.len();
    assert_eq!(words, 56);
    for phase in PHASES {
        for strictness in [Strictness::SteadyWindow, Strictness::WholeSegment] {
            let said = call.heard(phase, strictness);
            assert_eq!(
                call.score(&said),
                all_right(words),
                "{phase} {strictness:?}"
            );
        }
        // As seen, a word can fall between two looks; none is ever wrong.
        let seen = call.score(&call.heard(phase, Strictness::AsSeen));
        assert_eq!(seen.wrong, 0, "{phase}");
        assert!(seen.right >= words - 8, "{phase} {seen:?}");
    }
}

#[test]
fn a_change_of_floor_in_a_pause_keeps_the_words_on_both_sides() {
    // The second person starts 0.4 s after the first stops, so the mark
    // passes straight across and each look is up to half a second late.
    let call = Call::of(4).says(1, 1_000, 5_000).says(2, 5_400, 9_000);
    let words = call.words().0.len();
    for phase in PHASES {
        let said = call.heard(phase, Strictness::SteadyWindow);
        assert_eq!(call.score(&said), all_right(words), "{phase}");
    }
}

#[test]
fn a_change_of_floor_inside_continuous_speech_leaves_the_words_beside_it_unnamed() {
    let call = run_on();
    for phase in PHASES {
        let score = call.score(&call.heard(phase, Strictness::SteadyWindow));
        assert_eq!(score.wrong, 0, "{phase}");
        assert!((5..=12).contains(&score.unnamed), "{phase} {score:?}");
        assert!(score.right >= 19, "{phase} {score:?}");
    }

    // The spans, for one run: the first person up to three quarters of a
    // second before the change, nobody across it, then the second.
    let evidence = call.evidence(0);
    let words = call.words().0;
    let spans = resolve(
        &words,
        &evidence,
        Calibration::Unproven,
        Strictness::default(),
    );
    let span = |start_ms, end_ms, said| Attribution {
        span: Interval::new(start_ms, end_ms),
        said,
    };
    assert_eq!(
        spans,
        [
            span(1_000, 3_700, Said::By(key(1), Why::HeldFloor)),
            span(3_750, 5_800, Said::ByNobody(WhyNot::FloorChanged)),
            span(5_850, 8_800, Said::By(key(2), Why::HeldFloor)),
        ]
    );
    // The order the words arrive in does not matter.
    let backwards = words.iter().rev().copied().collect::<Vec<_>>();
    let again = resolve(
        &backwards,
        &evidence,
        Calibration::Unproven,
        Strictness::default(),
    );
    assert_eq!(again, spans);
}

/// The limit this design accepts. The call app marks one speaker at a time,
/// so someone who says a few words over the person holding the floor is
/// never shown, and their words take the holder's name. On the meeting
/// corpus the simulation measured, this was nearly every wrong name
/// (`measurements-indicator-sim.md`), and a per-meeting voice check did not
/// remove it (`measurements-voice-hybrid.md`). No strictness setting helps:
/// the evidence is the same whichever rule reads it.
#[test]
fn words_said_over_the_person_holding_the_floor_take_that_persons_name() {
    let call = Call::of(4).says(1, 1_000, 8_000).says(2, 4_000, 4_600);
    let (words, speakers) = call.words();
    let over = speakers.iter().filter(|person| **person == 2).count();
    assert_eq!(over, 2);
    for phase in PHASES {
        for strictness in EVERY {
            let said = call.heard(phase, strictness);
            let score = call.score(&said);
            assert_eq!(score.wrong, over, "{phase} {strictness:?} {score:?}");
            for (said, person) in said.iter().zip(&speakers) {
                if *person == 2 {
                    assert_eq!(*said, Said::By(key(1), Why::HeldFloor));
                }
            }
        }
    }
    assert_eq!(words.len(), 30);
}

#[test]
fn a_reply_under_a_lingering_mark_is_not_named_unless_read_as_seen() {
    // The mark stays on the first person for 1.5 s after they stop, and a
    // "yeah" 0.7 s in is too short to take it.
    let call = Call::of(4).says(1, 1_000, 4_000).says(2, 4_700, 4_900);
    let reply = call.words().0.len() - 1;
    for phase in PHASES {
        let said = call.heard(phase, Strictness::SteadyWindow);
        assert_eq!(call.score(&said).wrong, 0, "{phase}");
        assert_eq!(
            said[reply],
            Said::ByNobody(WhyNot::NobodyHeldFloor),
            "{phase}"
        );
    }
    let seen = call.heard(433, Strictness::AsSeen);
    assert_eq!(seen[reply], Said::By(key(1), Why::HeldFloor));

    // What is left: a look can be half a second late, so a reply that close
    // to the end of someone's speech can still take their name.
    let close = Call::of(4).says(1, 1_000, 4_000).says(2, 4_300, 4_500);
    let said = close.heard(433, Strictness::SteadyWindow);
    assert_eq!(said[reply], Said::By(key(1), Why::HeldFloor));
}

#[test]
fn remote_words_while_the_user_holds_the_floor_are_nobodys() {
    let call = Call::of(3).says(0, 1_000, 6_000).says(1, 3_000, 3_600);
    for phase in PHASES {
        for strictness in EVERY {
            let said = call.heard(phase, strictness);
            let user = Said::ByNobody(WhyNot::LocalHeldFloor);
            assert_eq!(said, [user, user], "{phase} {strictness:?}");
        }
    }
}

#[test]
fn the_only_remote_person_on_a_call_said_every_remote_word() {
    let call = Call::of(2)
        .says(1, 1_000, 3_000)
        .says(0, 2_000, 6_000)
        .says(1, 9_000, 9_500);
    let (words, _) = call.words();
    // Their name needs no look at the floor: only the list of who was there.
    let roster = &call.evidence(0)[..2];
    for strictness in EVERY {
        for evidence in [roster, &call.evidence(0)[..]] {
            let said = each_word(&words, evidence, Calibration::Unproven, strictness);
            assert!(!said.is_empty());
            for said in said {
                assert_eq!(said, Said::By(key(1), Why::OnlyRemoteParticipant));
            }
        }
    }
    // With a second remote person there, even a silent one, it is the floor
    // that decides, and nobody looked.
    let three = Call::of(3).says(1, 1_000, 3_000);
    let said = each_word(
        &three.words().0,
        &three.evidence(0)[..3],
        Calibration::Unproven,
        Strictness::default(),
    );
    assert!(
        said.iter()
            .all(|said| *said == Said::ByNobody(WhyNot::NoEvidence))
    );
}

#[test]
fn looks_that_could_not_be_trusted_count_as_no_look() {
    let call = Call::of(4).says(1, 1_000, 9_000);
    let hidden = Untrusted::WindowHidden;
    let evidence = call
        .evidence(0)
        .into_iter()
        .map(|line| match line {
            Evidence::Floor { at_ms, .. } if (4_000..=5_500).contains(&at_ms) => {
                Evidence::Untrusted {
                    at_ms,
                    reason: hidden,
                }
            }
            line => line,
        })
        .collect::<Vec<_>>();
    let (words, _) = call.words();
    let said = each_word(
        &words,
        &evidence,
        Calibration::Unproven,
        Strictness::default(),
    );
    let named = Said::By(key(1), Why::HeldFloor);
    for (word, said) in words.iter().zip(&said) {
        let expected = match word.middle_ms() {
            // Clear of the hidden stretch on either side.
            ..2_250 | 6_250.. => named,
            // Beside it.
            2_250..2_500 | 5_500..6_250 => Said::ByNobody(WhyNot::FloorChanged),
            // The last second before it: the mark may only have lingered.
            2_500..3_000 => Said::ByNobody(WhyNot::NobodyHeldFloor),
            3_000..5_500 => Said::ByNobody(WhyNot::Untrusted(hidden)),
        };
        assert_eq!(*said, expected, "{word:?}");
    }
    // As seen, the same looks cost only the words under them.
    let seen = each_word(&words, &evidence, Calibration::Unproven, Strictness::AsSeen);
    for (word, said) in words.iter().zip(&seen) {
        let under = (3_000..5_500).contains(&word.middle_ms());
        assert_eq!(*said == named, !under, "{word:?}");
    }
}

/// The call app's mark can blink off for one look while its owner draws
/// breath mid-sentence. That is still their hold, so none of their words
/// before it are read as a lingering mark.
#[test]
fn a_mark_that_blinks_off_for_one_look_keeps_its_owners_words() {
    let call = Call::of(4).says(1, 1_000, 9_000);
    let (words, _) = call.words();
    let blinked = |evidence: Vec<Evidence>, from_ms: u64, looks: u64| {
        evidence
            .into_iter()
            .map(|line| match line {
                Evidence::Floor { at_ms, .. }
                    if (from_ms..from_ms + looks * POLL).contains(&at_ms) =>
                {
                    Evidence::Floor {
                        at_ms,
                        holder: None,
                    }
                }
                line => line,
            })
            .collect::<Vec<_>>()
    };
    for phase in PHASES {
        let evidence = blinked(call.evidence(phase), 5_000, 1);
        let said = each_word(
            &words,
            &evidence,
            Calibration::Unproven,
            Strictness::default(),
        );
        assert_eq!(call.score(&said), all_right(words.len()), "{phase}");

        // Two looks of nobody are a real silence: the words before it may
        // have been said under a lingering mark.
        let evidence = blinked(call.evidence(phase), 5_000, 2);
        let said = each_word(
            &words,
            &evidence,
            Calibration::Unproven,
            Strictness::default(),
        );
        let score = call.score(&said);
        assert_eq!(score.wrong, 0, "{phase}");
        assert!(score.unnamed > 0, "{phase} {score:?}");
    }
}

#[test]
fn a_whole_segment_is_named_or_none_of_it_is() {
    // Half one person and half the next: nobody holds four words in five.
    let call = run_on();
    for phase in PHASES {
        let said = call.heard(phase, Strictness::WholeSegment);
        let score = call.score(&said);
        assert_eq!((score.right, score.wrong), (0, 0), "{phase}");
        assert!(said.contains(&Said::ByNobody(WhyNot::Mixed)), "{phase}");
    }
    // One person with a short reply over them: the whole segment is theirs,
    // the reply included.
    let call = Call::of(4).says(1, 1_000, 8_000).says(2, 4_000, 4_600);
    let said = call.heard(0, Strictness::WholeSegment);
    assert!(
        said.iter()
            .all(|said| *said == Said::By(key(1), Why::HeldFloor))
    );
}

#[test]
fn a_call_calibrates_on_the_users_own_tile_and_resolves_on_that_timing() {
    let mut call = turn_taking();
    for turn in 0..6 {
        call = call.says(0, 30_000 + turn * 8_000, 33_000 + turn * 8_000);
    }
    for phase in PHASES {
        let evidence = call.evidence(phase);
        let calibration = calibrate(&local_floor(&evidence), &call.microphone());
        let Calibration::Healthy(timing) = calibration else {
            panic!("{phase} {calibration:?}");
        };
        // The mark's own delays, as looks every half second show them.
        assert!(
            (D_ON..D_ON + POLL).contains(&timing.onset_lag_ms),
            "{timing:?}"
        );
        assert!(
            (D_OFF - POLL..=D_OFF).contains(&timing.release_tail_ms),
            "{timing:?}"
        );

        let (words, _) = call.words();
        let said = each_word(&words, &evidence, calibration, Strictness::default());
        assert_eq!(call.score(&said), all_right(words.len()), "{phase}");
    }
}

#[test]
fn a_reader_watching_the_wrong_tile_names_nobody() {
    let mut call = turn_taking();
    for turn in 0..8 {
        call = call.says(0, 30_000 + turn * 8_000, 33_000 + turn * 8_000);
    }
    // The reader takes person 1's tile for the user's.
    let evidence = call
        .evidence(0)
        .into_iter()
        .map(|line| match line {
            Evidence::Participant(person) => Evidence::Participant(Participant {
                is_local: person.key == key(1),
                ..person
            }),
            line => line,
        })
        .collect::<Vec<_>>();
    let calibration = calibrate(&local_floor(&evidence), &call.microphone());
    assert_eq!(calibration, Calibration::Unhealthy);
    let said = each_word(
        &call.words().0,
        &evidence,
        calibration,
        Strictness::default(),
    );
    assert!(
        said.iter()
            .all(|said| *said == Said::ByNobody(WhyNot::Unhealthy))
    );
}

// MARK: Properties, over random calls

/// A small fixed-seed generator: the same calls on every run.
struct Dice(u64);

impl Dice {
    fn roll(&mut self, sides: u64) -> u64 {
        self.0 ^= self.0 << 13;
        self.0 ^= self.0 >> 7;
        self.0 ^= self.0 << 17;
        self.0 % sides
    }

    /// Four people talking for 40 s with no regard for each other, so the
    /// call is full of overlap, short remarks and changes of floor.
    fn call(&mut self) -> Call {
        let mut call = Call::of(4);
        for person in 0..4 {
            let mut at = self.roll(4_000);
            while at < 40_000 {
                let length = if self.roll(4) == 0 {
                    150 + self.roll(400)
                } else {
                    600 + self.roll(5_000)
                };
                call = call.says(person, at, at + length);
                at += length + 300 + self.roll(7_000);
            }
        }
        call
    }

    /// What a reader with some bad looks saw of `call`.
    fn evidence(&mut self, call: &Call) -> Vec<Evidence> {
        let phase = self.roll(POLL);
        self.thinned(&call.evidence(phase), 0, 12)
    }

    /// `evidence` with one look in `drop` taken away and one in `distrust`
    /// no longer trusted (0 for none).
    fn thinned(&mut self, evidence: &[Evidence], drop: u64, distrust: u64) -> Vec<Evidence> {
        let reasons = [
            Untrusted::CutShort,
            Untrusted::WindowHidden,
            Untrusted::TwoWindows,
            Untrusted::FailedHealthCheck,
        ];
        let mut thinned = Vec::new();
        for line in evidence {
            let (Evidence::Floor { at_ms, .. } | Evidence::Untrusted { at_ms, .. }) = line else {
                thinned.push(line.clone());
                continue;
            };
            if drop > 0 && self.roll(drop) == 0 {
                continue;
            }
            if distrust > 0 && self.roll(distrust) == 0 {
                let reason = reasons[self.roll(4) as usize];
                thinned.push(Evidence::Untrusted {
                    at_ms: *at_ms,
                    reason,
                });
            } else {
                thinned.push(line.clone());
            }
        }
        thinned
    }

    fn calibration(&mut self) -> Calibration {
        [Calibration::Unproven, Calibration::Unhealthy][self.roll(2) as usize]
    }
}

fn named(said: &[Said]) -> usize {
    said.iter()
        .filter(|said| matches!(said, Said::By(..)))
        .count()
}

#[test]
fn with_no_evidence_nobody_is_named() {
    let mut dice = Dice(0x5EED_0001);
    for _ in 0..20 {
        let call = dice.call();
        let (words, _) = call.words();
        // Nothing at all, and a list of who was there with no look at the
        // floor.
        let roster = &call.evidence(0)[..4];
        for strictness in EVERY {
            for evidence in [&[][..], roster] {
                let said = each_word(&words, evidence, Calibration::Unproven, strictness);
                assert_eq!(said.len(), words.len());
                assert!(
                    said.iter()
                        .all(|said| *said == Said::ByNobody(WhyNot::NoEvidence))
                );
                let spans = resolve(&words, evidence, Calibration::Unproven, strictness);
                assert!(
                    spans
                        .iter()
                        .all(|span| matches!(span.said, Said::ByNobody(_)))
                );
            }
        }
    }
}

/// At the reader's pace of a look every half second. Looks packed closer
/// than half of what one look vouches for are outside this promise: taking
/// one of those away leaves no hole.
#[test]
fn taking_evidence_away_never_adds_a_name() {
    let mut dice = Dice(0x5EED_0002);
    let mut names = 0;
    let mut lost = 0;
    for _ in 0..60 {
        let call = dice.call();
        let (words, _) = call.words();
        let evidence = dice.evidence(&call);
        let (drop, distrust) = [(4, 0), (0, 4), (3, 6), (12, 12)][dice.roll(4) as usize];
        let less = dice.thinned(&evidence, drop, distrust);
        for strictness in EVERY {
            let before = each_word(&words, &evidence, Calibration::Unproven, strictness);
            let after = each_word(&words, &less, Calibration::Unproven, strictness);
            for (index, (before, after)) in before.iter().zip(&after).enumerate() {
                if let Said::By(who, _) = after {
                    assert!(
                        matches!(before, Said::By(was, _) if was == who),
                        "{strictness:?} word {index} {:?}: {before:?} became {after:?}",
                        words[index]
                    );
                }
            }
            names += named(&before);
            lost += named(&before) - named(&after);
        }
    }
    // The calls did name people, and thinning did cost names.
    assert!(names > 5_000 && lost > 500, "{names} {lost}");
}

#[test]
fn an_unhealthy_calibration_names_nobody() {
    let mut dice = Dice(0x5EED_0003);
    for round in 0..20 {
        // Every other call has one remote person, who would otherwise be
        // given every word.
        let call = if round % 2 == 0 {
            dice.call()
        } else {
            Call::of(2).says(1, 1_000, 9_000)
        };
        let (words, _) = call.words();
        let evidence = dice.evidence(&call);
        for strictness in EVERY {
            let said = each_word(&words, &evidence, Calibration::Unhealthy, strictness);
            assert!(
                said.iter()
                    .all(|said| *said == Said::ByNobody(WhyNot::Unhealthy))
            );
        }
    }
}

#[test]
fn a_correction_always_wins() {
    let mut dice = Dice(0x5EED_0004);
    let mut corrected = 0;
    for round in 0..40 {
        let call = if round % 4 == 3 {
            Call::of(2).says(1, 1_000, 30_000)
        } else {
            dice.call()
        };
        let (words, _) = call.words();
        let mut evidence = dice.evidence(&call);
        let start = dice.roll(30_000);
        let during = Interval::new(start, start + 500 + dice.roll(8_000));
        // Somebody on the call, the user, somebody the call app never
        // showed, or "not sure".
        let by = [Some(key(2)), Some(key(0)), Some(key(8)), None][dice.roll(4) as usize];
        // An earlier correction over the same words loses to the later one.
        let earlier = Correction::Said {
            during,
            by: Some(key(3)),
        };
        evidence.insert(
            dice.roll(evidence.len() as u64) as usize,
            Evidence::Correction(earlier),
        );
        evidence.push(Evidence::Correction(Correction::Said { during, by }));
        let less = dice.thinned(&evidence, 3, 6);

        let expected = match by {
            Some(who) => Said::By(who, Why::Corrected),
            None => Said::ByNobody(WhyNot::Corrected),
        };
        for strictness in EVERY {
            for evidence in [&evidence, &less] {
                let said = each_word(&words, evidence, dice.calibration(), strictness);
                for (word, said) in words.iter().zip(&said) {
                    if during.contains(word.middle_ms()) {
                        assert_eq!(*said, expected, "{strictness:?} {word:?}");
                        corrected += 1;
                    } else {
                        assert!(!matches!(said, Said::By(_, Why::Corrected)));
                        assert_ne!(*said, Said::ByNobody(WhyNot::Corrected));
                    }
                }
            }
        }
    }
    assert!(corrected > 1_000, "{corrected}");
}
