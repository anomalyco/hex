//! Who held the floor, and when, as far as the evidence shows.
//!
//! A call app marks one person as speaking at a time. The reader looks
//! about twice a second, so what Hex has is a row of looks, and this module
//! turns them into stretches of time: someone held the floor, nobody did,
//! it was passing from one to another, or Hex could not tell.
//!
//! A look vouches for the time up to the next look only when that look came
//! soon enough. Everything else is unknown, and unknown is the worst a
//! stretch can be: nothing in it is named, and it counts against naming
//! what is beside it. That is what makes thin evidence safe. At the
//! reader's pace, taking a look away, or distrusting it, can only turn
//! stretches unknown.

use super::evidence::{Evidence, Interval, Participant, ParticipantKey, Untrusted};

/// How often the reader looks at the call window.
pub(super) const POLL_MS: u64 = 500;

/// How long one look vouches for: one and a half of the reader's half
/// seconds. Two looks further apart than this have a hole between them.
pub(super) const POLL_REACH_MS: u64 = POLL_MS + POLL_MS / 2;

/// Everyone the call app showed on the call.
pub(super) struct Roster(Vec<Participant>);

impl Roster {
    /// One person for each key, as the call app last showed them. The
    /// reader writes someone again when the app shows them differently, so
    /// the last line says whether they are the user, and the last line
    /// with a usable name says what they are called: a name that arrives
    /// after the tile does is theirs, and a later look that shows no name
    /// takes none away. They joined when they were first seen.
    pub(super) fn of(evidence: &[Evidence]) -> Self {
        let mut people: Vec<Participant> = Vec::new();
        for line in evidence {
            let Evidence::Participant(shown) = line else {
                continue;
            };
            match people.iter_mut().find(|known| known.key == shown.key) {
                Some(known) => {
                    known.is_local = shown.is_local;
                    if shown.name.is_some() {
                        known.name.clone_from(&shown.name);
                    }
                }
                None => people.push(shown.clone()),
            }
        }
        Self(people)
    }

    pub(super) fn person(&self, key: ParticipantKey) -> Option<&Participant> {
        self.0.iter().find(|person| person.key == key)
    }

    /// Whether `key` is someone on the call other than the user.
    pub(super) fn is_remote(&self, key: ParticipantKey) -> bool {
        self.person(key).is_some_and(|person| !person.is_local)
    }

    /// The one remote person, when the whole recording showed exactly one.
    pub(super) fn only_remote(&self) -> Option<ParticipantKey> {
        let mut remote = self.0.iter().filter(|person| !person.is_local);
        let only = remote.next()?;
        remote.next().is_none().then_some(only.key)
    }

    fn local(&self) -> Option<ParticipantKey> {
        let local = self.0.iter().find(|person| person.is_local)?;
        Some(local.key)
    }
}

/// Why Hex cannot tell who held the floor.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) enum Blind {
    /// No look covers the time.
    Unobserved,
    /// A look does, and it could not be trusted.
    Untrusted(Untrusted),
}

/// What one look saw.
#[derive(Clone, Copy, Eq, PartialEq)]
enum Look {
    Holder(ParticipantKey),
    Nobody,
    Blind(Blind),
}

/// The floor over one stretch of time.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) enum State {
    /// Two looks running agree that this person holds it.
    Held(ParticipantKey),
    /// This person was last to hold it and has likely stopped: the mark
    /// lingers after its owner goes quiet.
    Tail(ParticipantKey),
    /// Nobody held it at one look and this person at the next.
    Onset(ParticipantKey),
    /// It passed from the first person to the second between two looks.
    Handover(ParticipantKey, ParticipantKey),
    Nobody,
    Unknown(Blind),
}

impl State {
    /// Whether this stretch says the floor was, or may have been, someone
    /// else's than `who`'s.
    pub(super) fn against(self, who: ParticipantKey) -> bool {
        match self {
            Self::Held(other) | Self::Tail(other) | Self::Onset(other) => other != who,
            Self::Handover(from, to) => from != who && to != who,
            Self::Unknown(_) => true,
            Self::Nobody => false,
        }
    }

    fn between(before: Look, after: Look) -> Self {
        match (before, after) {
            (Look::Blind(why), _) | (_, Look::Blind(why)) => Self::Unknown(why),
            (Look::Holder(from), Look::Holder(to)) if from == to => Self::Held(from),
            (Look::Holder(from), Look::Holder(to)) => Self::Handover(from, to),
            (Look::Holder(from), Look::Nobody) => Self::Tail(from),
            (Look::Nobody, Look::Holder(to)) => Self::Onset(to),
            (Look::Nobody, Look::Nobody) => Self::Nobody,
        }
    }
}

/// The floor across a whole recording: each state holds from its time until
/// the next one's. Times are signed because speech can be placed before the
/// first look.
pub(super) struct Floor(Vec<(i64, State)>);

impl Floor {
    /// The floor as the reader saw it, on the call app's own timing. A look
    /// that names someone the call app never showed is no look.
    pub(super) fn seen(evidence: &[Evidence], roster: &Roster) -> Self {
        let mut looks = evidence
            .iter()
            .filter_map(|line| match line {
                Evidence::Floor { at_ms, holder } => Some((
                    *at_ms,
                    match holder {
                        Some(holder) if roster.person(*holder).is_some() => Look::Holder(*holder),
                        Some(_) => Look::Blind(Blind::Unobserved),
                        None => Look::Nobody,
                    },
                )),
                Evidence::Untrusted { at_ms, reason } => {
                    Some((*at_ms, Look::Blind(Blind::Untrusted(*reason))))
                }
                Evidence::Participant(_) | Evidence::Correction(_) => None,
            })
            .collect::<Vec<_>>();
        looks.sort_by_key(|(at_ms, _)| *at_ms);

        let mut floor = Self(vec![(i64::MIN, State::Unknown(Blind::Unobserved))]);
        for (index, (at_ms, look)) in looks.iter().enumerate() {
            let state = match looks.get(index + 1) {
                Some((next_ms, next)) if next_ms - at_ms <= POLL_REACH_MS => {
                    State::between(*look, *next)
                }
                _ => State::Unknown(Blind::Unobserved),
            };
            floor.push(*at_ms as i64, state);
        }
        floor
    }

    /// The floor as it bears on what was heard. The call app marks a
    /// speaker `lag_ms` after they start, so every stretch moves that much
    /// earlier. And where a hold ends without passing straight to someone
    /// else, its last `trim_ms` may be the mark lingering, so that much of
    /// it becomes a tail (none when `trim_ms` is no more than `lag_ms`).
    pub(super) fn heard(&self, lag_ms: u64, trim_ms: u64) -> Self {
        let (lag, trim) = (lag_ms as i64, trim_ms as i64);
        let mut heard = Self(Vec::new());
        for (index, (start, end, state)) in self.stretches().enumerate() {
            let start = start.saturating_sub(lag);
            heard.push(start, state);
            if let State::Held(who) = state {
                let passed_on = matches!(self.0.get(index + 1), Some((_, State::Handover(..))));
                let lingers_from = end.saturating_sub(trim.max(lag)).max(start);
                if !passed_on && lingers_from < end.saturating_sub(lag) {
                    heard.push(lingers_from, State::Tail(who));
                }
            }
        }
        heard
    }

    /// The floor at one moment.
    pub(super) fn at(&self, ms: u64) -> State {
        let after = self.0.partition_point(|(start, _)| *start <= ms as i64);
        self.0[after.saturating_sub(1)].1
    }

    /// Every state the floor was in between two moments, ends included.
    pub(super) fn within(&self, from_ms: u64, to_ms: u64) -> impl Iterator<Item = State> + '_ {
        let (from, to) = (from_ms as i64, to_ms as i64);
        self.stretches()
            .filter(move |(start, end, _)| *start <= to && *end > from)
            .map(|(_, _, state)| state)
    }

    /// Each stretch with its start and end. One that lasts no time at all
    /// still appears: it is skipped by every question about a moment.
    fn stretches(&self) -> impl Iterator<Item = (i64, i64, State)> + '_ {
        self.0.iter().enumerate().map(|(index, (start, state))| {
            let end = self.0.get(index + 1).map_or(i64::MAX, |(next, _)| *next);
            (*start, end, *state)
        })
    }

    fn push(&mut self, start: i64, state: State) {
        if self.0.last().is_none_or(|(_, last)| *last != state) {
            self.0.push((start, state));
        }
    }
}

/// When the user's own tile was marked as speaking, on the call app's
/// timing: what [`calibrate`](super::calibrate::calibrate) compares with
/// the microphone.
pub fn local_floor(evidence: &[Evidence]) -> Vec<Interval> {
    let roster = Roster::of(evidence);
    let Some(local) = roster.local() else {
        return Vec::new();
    };
    Floor::seen(evidence, &roster)
        .stretches()
        .filter(|(_, _, state)| *state == State::Held(local))
        .map(|(start, end, _)| Interval::new(start.max(0) as u64, end.max(0) as u64))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::meeting::SpeakerName;

    fn key(number: u32) -> ParticipantKey {
        ParticipantKey::numbered(number)
    }

    fn shown(number: u32, name: &str, is_local: bool, at_ms: u64) -> Evidence {
        Evidence::Participant(Participant {
            key: key(number),
            name: SpeakerName::parse(name),
            is_local,
            at_ms,
        })
    }

    fn name_of(roster: &Roster, number: u32) -> Option<&str> {
        let name = roster.person(key(number))?.name.as_ref()?;
        Some(name.as_str())
    }

    #[test]
    fn someone_written_again_keeps_the_latest_usable_name() {
        let evidence = [
            // The tile came before its name did.
            shown(1, "", false, 0),
            shown(2, "Bob", false, 0),
            shown(3, "Carol", true, 0),
            shown(1, "Alice", false, 500),
            // The app comes to show another name, and then for a look none.
            shown(2, "Bob B.", false, 1_000),
            shown(2, "", false, 1_500),
            // And shows someone else as the user.
            shown(3, "Carol", false, 2_000),
        ];
        let roster = Roster::of(&evidence);
        assert_eq!(name_of(&roster, 1), Some("Alice"));
        assert_eq!(name_of(&roster, 2), Some("Bob B."));
        assert_eq!(name_of(&roster, 3), Some("Carol"));
        assert_eq!(name_of(&roster, 4), None);
        // Each is one person, who joined when first seen, and is the user
        // or not as last shown.
        assert_eq!(roster.0.len(), 3);
        assert_eq!(roster.person(key(1)).unwrap().at_ms, 0);
        assert!(roster.is_remote(key(3)) && roster.local().is_none());
        assert_eq!(roster.only_remote(), None);

        // Someone never shown with a usable name stays without one.
        let nameless = Roster::of(&[shown(1, "", false, 0), shown(1, "You", false, 500)]);
        assert_eq!(name_of(&nameless, 1), None);
        assert_eq!(nameless.only_remote(), Some(key(1)));
    }
}
