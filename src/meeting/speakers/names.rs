//! From who said what to what a transcript shows: the meeting's speaker
//! table and the speaker on each line.
//!
//! Both are derived. [`name_lines`] builds them whole from the attributions
//! and the evidence and takes nothing from what the lines carried before,
//! so running it again after a correction, or under another rule, replaces
//! every name at once and can leave no half-renamed meeting behind.

use super::evidence::{Correction, Evidence, ParticipantKey};
use super::floor::Roster;
use super::resolve::{Attribution, Said};
use super::{NameSource, SpeakerId, SpeakerName, Speakers};
use crate::meeting::{MeetingSource, TranscriptEntry};

/// A transcript with its speakers named: the table for the manifest, and
/// the same lines, each carrying an id from that table or none.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Named {
    pub speakers: Speakers,
    pub entries: Vec<TranscriptEntry>,
}

/// Names `entries` from `attributions`.
///
/// A remote line takes a speaker only when every stretch of speech it
/// overlaps was said by the same person, the call app showed that person
/// as someone other than the user, and they have a usable name. A
/// microphone line never takes one. The table lists the people who ended
/// up on a line, in the order they first speak, each under the name the
/// user gave them in this meeting or else the name the call app showed.
pub fn name_lines(
    entries: &[TranscriptEntry],
    attributions: &[Attribution],
    evidence: &[Evidence],
) -> Named {
    let roster = Roster::of(evidence);
    let renamed = |who: ParticipantKey| {
        let mut renames = evidence.iter().filter_map(|line| match line {
            Evidence::Correction(Correction::Named { key, name }) if *key == who => Some(name),
            _ => None,
        });
        renames.next_back()
    };
    let shown = |who: ParticipantKey| -> Option<(SpeakerName, NameSource)> {
        let person = roster.person(who).filter(|person| !person.is_local)?;
        match renamed(who) {
            Some(name) => Some((name.clone(), NameSource::User)),
            None => Some((person.name.clone()?, NameSource::CallApp)),
        }
    };

    let mut speakers = Speakers::default();
    let mut ids: Vec<(ParticipantKey, Option<SpeakerId>)> = Vec::new();
    let entries = entries
        .iter()
        .map(|entry| {
            let who = (entry.source == MeetingSource::System)
                .then(|| said_by(entry, attributions))
                .flatten();
            let speaker = who.and_then(|who| {
                if let Some((_, id)) = ids.iter().find(|(known, _)| *known == who) {
                    return id.clone();
                }
                let id = shown(who).map(|(name, from)| speakers.add(name, from));
                ids.push((who, id.clone()));
                id
            });
            TranscriptEntry {
                speaker,
                ..entry.clone()
            }
        })
        .collect();
    Named { speakers, entries }
}

/// The one person who said everything `entry` overlaps, if there is one.
fn said_by(entry: &TranscriptEntry, attributions: &[Attribution]) -> Option<ParticipantKey> {
    // A line or a stretch with no length still occupies its moment.
    let entry_end = entry.end_ms.max(entry.start_ms + 1);
    let mut overlapping = attributions.iter().filter(|attribution| {
        let span = attribution.span;
        span.start_ms() < entry_end && entry.start_ms < span.end_ms().max(span.start_ms() + 1)
    });
    let Said::By(who, _) = overlapping.next()?.said else {
        return None;
    };
    overlapping
        .all(|attribution| matches!(attribution.said, Said::By(other, _) if other == who))
        .then_some(who)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::meeting::Labels;
    use crate::meeting::speakers::evidence::{Interval, Participant};
    use crate::meeting::speakers::resolve::{Why, WhyNot};

    fn key(number: u32) -> ParticipantKey {
        ParticipantKey::numbered(number)
    }

    fn person(number: u32, name: &str, is_local: bool) -> Evidence {
        Evidence::Participant(Participant {
            key: key(number),
            name: SpeakerName::parse(name),
            is_local,
            at_ms: 0,
        })
    }

    /// Kit on the microphone, with Priya, Marcus and someone the call app
    /// gave no usable name.
    fn call() -> Vec<Evidence> {
        vec![
            person(1, "Kit", true),
            person(2, "Priya Nair", false),
            person(3, "Marcus Webb", false),
            person(4, "", false),
        ]
    }

    fn by(number: u32, start_ms: u64, end_ms: u64) -> Attribution {
        Attribution {
            span: Interval::new(start_ms, end_ms),
            said: Said::By(key(number), Why::HeldFloor),
        }
    }

    fn by_nobody(start_ms: u64, end_ms: u64) -> Attribution {
        Attribution {
            span: Interval::new(start_ms, end_ms),
            said: Said::ByNobody(WhyNot::FloorChanged),
        }
    }

    fn line(source: MeetingSource, start_ms: u64, end_ms: u64, text: &str) -> TranscriptEntry {
        TranscriptEntry::new(source, start_ms, end_ms, text)
    }

    fn them(start_ms: u64, end_ms: u64, text: &str) -> TranscriptEntry {
        line(MeetingSource::System, start_ms, end_ms, text)
    }

    fn labels(named: &Named) -> Vec<&str> {
        let labels = Labels::by_track(&named.speakers);
        named
            .entries
            .iter()
            .map(|entry| labels.of(entry).as_str())
            .collect()
    }

    #[test]
    fn lines_take_the_names_of_the_people_who_said_them() {
        let entries = [
            line(
                MeetingSource::Microphone,
                5_000,
                7_000,
                "Can we still ship Thursday?",
            ),
            them(
                12_000,
                15_000,
                "Thursday works if the migration lands tomorrow.",
            ),
            them(19_000, 19_400, "Yeah."),
            them(21_000, 23_000, "I can take the migration."),
            them(24_000, 25_000, "Thanks."),
        ];
        let attributions = [
            // Two stretches of Priya's make up her one line.
            by(2, 12_000, 13_400),
            by(2, 13_800, 15_000),
            by_nobody(19_000, 19_400),
            by(3, 21_000, 23_000),
            by(2, 24_000, 25_000),
        ];
        let named = name_lines(&entries, &attributions, &call());
        assert_eq!(
            labels(&named),
            ["You", "Priya Nair", "Them", "Marcus Webb", "Priya Nair"]
        );
        // The table holds who spoke, in the order they first did, under
        // ids of its own: no key from the evidence reaches the manifest.
        assert_eq!(
            serde_json::to_value(&named.speakers).unwrap(),
            serde_json::json!([
                {"id": "s1", "name": "Priya Nair", "from": "call_app"},
                {"id": "s2", "name": "Marcus Webb", "from": "call_app"},
            ])
        );
        // Nothing but the speaker changes on a line.
        let unnamed = named.entries.iter().map(|entry| TranscriptEntry {
            speaker: None,
            ..entry.clone()
        });
        assert_eq!(unnamed.collect::<Vec<_>>(), entries);
    }

    #[test]
    fn a_line_two_people_share_or_nobody_owns_all_of_stays_unnamed() {
        let entries = [
            them(1_000, 4_000, "Right, and then, sorry, go ahead."),
            them(5_000, 8_000, "So the plan is, yeah, the plan."),
            them(9_000, 9_500, "Fine."),
        ];
        let attributions = [
            by(2, 1_000, 2_500),
            by(3, 2_600, 4_000),
            by(2, 5_000, 6_000),
            by_nobody(6_100, 6_500),
            by(2, 6_600, 8_000),
            // Nothing was resolved for the last line at all.
        ];
        let named = name_lines(&entries, &attributions, &call());
        assert_eq!(labels(&named), ["Them", "Them", "Them"]);
        assert!(named.speakers.is_empty());
    }

    #[test]
    fn the_names_are_rebuilt_whole_and_keep_nothing_from_before() {
        let mut stale = Speakers::default();
        let old = stale.add(
            SpeakerName::parse("Someone Else").unwrap(),
            NameSource::User,
        );
        let entries = [
            TranscriptEntry {
                speaker: Some(old.clone()),
                ..line(MeetingSource::Microphone, 0, 900, "Hello.")
            },
            TranscriptEntry {
                speaker: Some(old.clone()),
                ..them(1_000, 2_000, "Hi.")
            },
            TranscriptEntry {
                speaker: Some(old),
                ..them(3_000, 4_000, "Shall we start?")
            },
        ];
        let named = name_lines(&entries, &[by(3, 3_000, 4_000)], &call());
        assert_eq!(labels(&named), ["You", "Them", "Marcus Webb"]);
        assert!(named.entries[0].speaker.is_none() && named.entries[1].speaker.is_none());
        // Run again on its own output, it says the same.
        assert_eq!(
            name_lines(&named.entries, &[by(3, 3_000, 4_000)], &call()),
            named
        );
    }

    #[test]
    fn a_name_the_user_gave_replaces_the_call_apps() {
        let rename = |number, name: &str| {
            Evidence::Correction(Correction::Named {
                key: key(number),
                name: SpeakerName::parse(name).unwrap(),
            })
        };
        let mut evidence = call();
        evidence.extend([
            rename(2, "Pri"),
            rename(2, "Priya"),
            // The call app showed no usable name for this one; the user's
            // is the first they have.
            rename(4, "Sam"),
            // Nobody the call app showed.
            rename(9, "Ghost"),
        ]);
        let entries = [
            them(0, 900, "One."),
            them(1_000, 1_900, "Two."),
            them(2_000, 2_900, "Three."),
        ];
        let attributions = [by(2, 0, 900), by(4, 1_000, 1_900), by(9, 2_000, 2_900)];
        let named = name_lines(&entries, &attributions, &evidence);
        assert_eq!(labels(&named), ["Priya", "Sam", "Them"]);
        assert_eq!(
            serde_json::to_value(&named.speakers).unwrap(),
            serde_json::json!([
                {"id": "s1", "name": "Priya", "from": "user"},
                {"id": "s2", "name": "Sam", "from": "user"},
            ])
        );
    }

    #[test]
    fn the_user_and_the_nameless_never_name_a_line() {
        let entries = [
            them(0, 900, "Mine, heard back."),
            them(1_000, 1_900, "Whose?"),
        ];
        // Even a correction that says so: a remote line cannot become You.
        let mine = Attribution {
            span: Interval::new(0, 900),
            said: Said::By(key(1), Why::Corrected),
        };
        let named = name_lines(&entries, &[mine, by(4, 1_000, 1_900)], &call());
        assert_eq!(labels(&named), ["Them", "Them"]);
        assert!(named.speakers.is_empty());
        assert_eq!(name_lines(&entries, &[], &[]).entries, entries);
    }
}
