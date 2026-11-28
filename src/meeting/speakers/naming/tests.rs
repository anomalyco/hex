use std::fs;

use super::*;
use crate::meeting::Labels;
use crate::meeting::speakers::SpeakerName;
use crate::meeting::speakers::calibrate::Timing;
use crate::meeting::speakers::evidence::{Participant, ParticipantKey};

/// How often the reader looks.
const POLL: u64 = 500;

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

/// Carol, who is recording, in a huddle with Alice and Bob.
const CAROL: u32 = 1;
const ALICE: u32 = 2;
const BOB: u32 = 3;

fn huddle() -> Vec<Evidence> {
    vec![
        person(CAROL, "Carol", true),
        person(ALICE, "Alice", false),
        person(BOB, "Bob", false),
    ]
}

/// The huddle, with a look every half second until `end_ms`: each look
/// shows whoever `marked` says the app marked at that moment.
fn looks(end_ms: u64, marked: impl Fn(u64) -> Option<u32>) -> Vec<Evidence> {
    let mut evidence = huddle();
    evidence.extend((0..=end_ms / POLL).map(|look| Evidence::Floor {
        at_ms: look * POLL,
        holder: marked(look * POLL).map(key),
        local_speaking: false,
    }));
    evidence
}

fn line(source: MeetingSource, start_ms: u64, end_ms: u64, text: &str) -> TranscriptEntry {
    TranscriptEntry::new(source, start_ms, end_ms, text)
}

fn them(start_ms: u64, end_ms: u64, text: &str) -> TranscriptEntry {
    line(MeetingSource::System, start_ms, end_ms, text)
}

fn you(start_ms: u64, end_ms: u64, text: &str) -> TranscriptEntry {
    line(MeetingSource::Microphone, start_ms, end_ms, text)
}

fn labels(named: &Named) -> Vec<&str> {
    let labels = Labels::by_track(&named.speakers);
    named
        .entries
        .iter()
        .map(|entry| labels.of(entry).as_str())
        .collect()
}

fn called<'a>(speakers: &'a Speakers, entries: &[TranscriptEntry]) -> Vec<&'a str> {
    let labels = Labels::by_track(speakers);
    entries
        .iter()
        .map(|entry| labels.of(entry).as_str())
        .collect()
}

fn table(speakers: &Speakers) -> serde_json::Value {
    serde_json::to_value(speakers).unwrap()
}

fn folder(name: &str) -> std::path::PathBuf {
    let folder =
        std::env::temp_dir().join(format!("hex-speaker-naming-{name}-{}", std::process::id()));
    let _ = fs::remove_dir_all(&folder);
    fs::create_dir_all(&folder).unwrap();
    folder
}

/// Alice talks from 1.5 s to 5 s as the app shows it, and Bob from 7.5 s
/// to 10 s.
fn two_turns() -> Vec<Evidence> {
    looks(11_000, |at_ms| match at_ms {
        1_500..=5_000 => Some(ALICE),
        7_500..=10_000 => Some(BOB),
        _ => None,
    })
}

fn two_turns_heard() -> Vec<TranscriptEntry> {
    vec![
        them(
            1_200,
            3_800,
            "Thursday works if the migration lands tomorrow.",
        ),
        you(4_600, 6_000, "Who can take it?"),
        them(7_200, 8_800, "I can take the migration."),
    ]
}

#[test]
fn a_call_is_named_from_its_first_remote_line_on_the_assumed_timing() {
    // The user has said one thing and their tile has not lit at all: far
    // too little to judge the call by.
    let (named, summary) = derive(&two_turns_heard(), &two_turns());
    assert_eq!(summary.calibration, Calibration::Unproven);
    assert_eq!(summary.calibration.timing(), Some(Timing::default()));
    assert_eq!(labels(&named), ["Alice", "You", "Bob"]);
    assert_eq!(
        table(&named.speakers),
        serde_json::json!([
            {"id": "s1", "name": "Alice", "from": "call_app"},
            {"id": "s2", "name": "Bob", "from": "call_app"},
        ])
    );
    assert_eq!(
        (summary.remote_lines, summary.named_lines, summary.speakers),
        (2, 2, 2)
    );
    assert_eq!(summary.held_floor, 2);

    // And from the very first line, with nothing after it yet.
    let (first, _) = derive(&two_turns_heard()[..1], &two_turns());
    assert_eq!(labels(&first), ["Alice"]);
    // Nothing but the speaker changes on a line.
    let unnamed = named.entries.into_iter().map(|entry| TranscriptEntry {
        speaker: None,
        ..entry
    });
    assert_eq!(unnamed.collect::<Vec<_>>(), two_turns_heard());
}

/// Six rounds of ten seconds: the user talks for four seconds, and Alice
/// answers from 7 s to 8.5 s, which the app shows a second late. The user's
/// own tile is lit for `own` of each round.
fn rounds(own: std::ops::RangeInclusive<u64>) -> (Vec<TranscriptEntry>, Vec<Evidence>) {
    let entries = (0..6)
        .flat_map(|round| {
            let start = round * 10_000;
            [
                you(start, start + 4_000, "Here is where we are."),
                them(start + 7_100, start + 8_400, "Sounds right."),
            ]
        })
        .collect();
    let evidence = looks(60_000, |at_ms| match at_ms % 10_000 {
        within if own.contains(&within) => Some(CAROL),
        8_000..=9_500 => Some(ALICE),
        _ => None,
    });
    (entries, evidence)
}

#[test]
fn once_the_users_own_tile_has_lit_enough_the_call_is_judged_by_it() {
    // It follows the microphone: on a second after the user starts, off a
    // second after they stop.
    let (entries, evidence) = rounds(1_000..=5_000);
    let (named, summary) = derive(&entries, &evidence);
    assert_eq!(
        summary.calibration,
        Calibration::Healthy(Timing {
            onset_lag_ms: 1_000,
            release_tail_ms: 1_000,
        })
    );
    assert_eq!((summary.remote_lines, summary.named_lines), (6, 6));
    assert!(
        labels(&named)
            .chunks(2)
            .all(|round| round == ["You", "Alice"])
    );

    // It lights in the user's silences instead: the reader is not reading
    // this call, and nothing it saw is believed.
    let (entries, evidence) = rounds(6_500..=7_500);
    let (named, summary) = derive(&entries, &evidence);
    assert_eq!(summary.calibration, Calibration::Unhealthy);
    assert_eq!((summary.remote_lines, summary.named_lines), (6, 0));
    assert_eq!((summary.unhealthy, summary.speakers), (6, 0));
    assert!(named.speakers.is_empty());
    assert!(
        labels(&named)
            .chunks(2)
            .all(|round| round == ["You", "Them"])
    );
}

#[test]
fn a_line_said_across_a_change_of_floor_stays_them() {
    // The floor passes from Alice to Bob at 5 s with nobody pausing.
    let evidence = looks(10_000, |at_ms| match at_ms {
        1_000..=5_000 => Some(ALICE),
        5_500..=9_000 => Some(BOB),
        _ => None,
    });
    let entries = [
        them(1_500, 3_500, "So that is the plan for this week."),
        them(
            3_600,
            6_400,
            "Unless anyone objects, sorry, go ahead, yes I do.",
        ),
        them(6_500, 8_000, "It will not fit in a week."),
    ];
    let (named, summary) = derive(&entries, &evidence);
    assert_eq!(labels(&named), ["Alice", "Them", "Bob"]);
    assert!(summary.floor_changed > 0);
}

#[test]
fn a_meeting_with_nothing_seen_names_nobody() {
    let entries = two_turns_heard();
    // No evidence at all, and a roster with no look at the floor.
    for evidence in [Vec::new(), huddle()] {
        let (named, summary) = derive(&entries, &evidence);
        assert_eq!(named.entries, entries);
        assert!(named.speakers.is_empty());
        assert_eq!(summary.named_lines, 0);
    }
}

#[test]
fn the_words_of_a_line_share_its_time_by_their_length() {
    let spans = |line: &TranscriptEntry| {
        let words = words(line).into_iter();
        words
            .map(|word| (word.start_ms(), word.end_ms()))
            .collect::<Vec<_>>()
    };
    // "I" and "see" with the space each is said with: two parts and four.
    assert_eq!(
        spans(&them(1_000, 1_600, "I see")),
        [(1_000, 1_200), (1_200, 1_600)]
    );
    // The words fill the line, in order, with nothing between them.
    let long = them(
        2_000,
        9_000,
        "Thursday works if the migration lands tomorrow.",
    );
    let words = spans(&long);
    assert_eq!(words.len(), 7);
    assert_eq!((words[0].0, words[6].1), (2_000, 9_000));
    assert!(words.windows(2).all(|pair| pair[0].1 == pair[1].0));
    // A line with no words, or no length, still occupies its moment.
    assert_eq!(spans(&them(500, 900, "  ")), [(500, 900)]);
    assert_eq!(spans(&them(500, 500, "Yes.")), [(500, 500)]);
    assert_eq!(spans(&them(900, 500, "Yes.")), [(900, 900)]);
}

#[test]
fn microphone_lines_that_run_on_are_one_stretch_of_talking() {
    let lines = [
        you(1_000, 2_000, "One."),
        you(2_250, 3_000, "Two."),
        you(3_300, 4_000, "Three."),
        // Out of order, and inside what was already heard.
        you(1_500, 1_800, "Aside."),
    ];
    let stretches = speech(lines.iter())
        .into_iter()
        .map(|stretch| (stretch.start_ms(), stretch.end_ms()))
        .collect::<Vec<_>>();
    assert_eq!(stretches, [(1_000, 3_000), (3_300, 4_000)]);
    assert!(speech([].iter()).is_empty());
}

#[test]
fn nothing_is_named_with_the_lab_off_or_without_evidence() {
    let folder = folder("gate");
    let entries = two_turns_heard();
    let mut known = Speakers::default();

    // The lab on, and a meeting with no evidence: an imported file, a call
    // in an app Hex cannot read, a meeting from before the lab.
    let (named, summary) = named(&folder, entries.clone(), &mut known, true);
    assert_eq!((named.entries, summary), (entries.clone(), None));
    assert!(named.speakers.is_empty() && known.is_empty());

    // Evidence, and the lab off: it is not even read.
    evidence::append(&folder, &two_turns()).unwrap();
    let (named, summary) = super::named(&folder, entries.clone(), &mut known, false);
    assert_eq!((named.entries, summary), (entries.clone(), None));
    assert!(named.speakers.is_empty() && known.is_empty());

    // Both: named.
    let (named, summary) = super::named(&folder, entries, &mut known, true);
    assert_eq!(labels(&named), ["Alice", "You", "Bob"]);
    assert_eq!(summary.unwrap().named_lines, 2);
    assert_eq!(known, named.speakers);
    fs::remove_dir_all(folder).unwrap();
}

fn manifest(id: &str) -> MeetingManifest {
    let track = |file: &str| {
        serde_json::json!({
            "file": file, "sample_rate": 16_000, "first_pts_us": null, "last_pts_us": null,
            "first_arrival_us": null, "samples": 0, "dropped_packets": 0,
        })
    };
    serde_json::from_value(serde_json::json!({
        "schema_version": 1, "id": id, "title": "Slack huddle", "status": "transcribing",
        "created_at_ms": 0, "ended_at_ms": null, "duration_ms": null,
        "system": track("system.wav"), "microphone": track("microphone.wav"), "error": null,
    }))
    .unwrap()
}

#[test]
fn the_final_transcript_and_the_manifests_table_are_named_together() {
    let folder = folder("final");
    evidence::append(&folder, &two_turns()).unwrap();
    let mut manifest = manifest("final");

    let entries = name_transcript_when(&folder, &mut manifest, two_turns_heard(), true);
    assert_eq!(
        called(&manifest.speakers, &entries),
        ["Alice", "You", "Bob"]
    );
    assert_eq!(
        table(&manifest.speakers),
        serde_json::json!([
            {"id": "s1", "name": "Alice", "from": "call_app"},
            {"id": "s2", "name": "Bob", "from": "call_app"},
        ])
    );
    // Both are what gets saved: a line's speaker is an id in that table.
    let saved = serde_json::to_string(&entries[0]).unwrap();
    assert!(saved.ends_with(r#""speaker":"s1"}"#), "{saved}");

    // The lab turned off before the final pass: the table the recording
    // saved goes, with the names.
    let entries = name_transcript_when(&folder, &mut manifest, two_turns_heard(), false);
    assert_eq!(entries, two_turns_heard());
    assert!(manifest.speakers.is_empty());
    fs::remove_dir_all(folder).unwrap();
}

#[test]
fn the_final_pass_keeps_the_ids_the_recording_saved() {
    let folder = folder("final-ids");
    evidence::append(&folder, &two_turns()).unwrap();
    // While recording, only Bob's line had been named when the table was
    // last saved, so Bob is s1 in the manifest.
    let mut live = LiveNames::handing_to(LiveTable::default());
    let recorded = live.name_when(&folder, two_turns_heard()[2..].to_vec(), true);
    let mut manifest = manifest("final-ids");
    manifest.speakers = live.table.take().unwrap();
    assert_eq!(called(&manifest.speakers, &recorded), ["Bob"]);
    let bob = recorded[0].speaker.clone();

    // The final pass names Alice first, and Bob is still s1: the in-call
    // lines read against the final table, or the final lines against the
    // table the recording saved, never show the other's name.
    let saved = manifest.speakers.clone();
    let entries = name_transcript_when(&folder, &mut manifest, two_turns_heard(), true);
    assert_eq!(
        called(&manifest.speakers, &entries),
        ["Alice", "You", "Bob"]
    );
    assert_eq!(entries[2].speaker, bob);
    assert_eq!(called(&manifest.speakers, &recorded), ["Bob"]);
    assert_eq!(called(&saved, &entries), ["Them", "You", "Bob"]);
    fs::remove_dir_all(folder).unwrap();
}

#[test]
fn the_in_call_transcript_is_named_as_it_grows_and_its_table_handed_on_when_it_changes() {
    let folder = folder("live");
    let table = LiveTable::default();
    let mut live = LiveNames::handing_to(table.clone());
    let heard = two_turns_heard();

    // Before the reader has saved anything: nothing named, nothing to save.
    assert_eq!(
        live.name_when(&folder, heard[..1].to_vec(), true),
        heard[..1]
    );
    assert!(table.take().is_none());

    // The reader has seen Alice's turn, and her line is settled.
    let seen = two_turns();
    let so_far = seen
        .iter()
        .filter(|line| !matches!(line, Evidence::Floor { at_ms, .. } if *at_ms > 5_500))
        .cloned()
        .collect::<Vec<_>>();
    evidence::append(&folder, &so_far).unwrap();
    let first = live.name_when(&folder, heard[..1].to_vec(), true);
    let saved = table.take().unwrap();
    assert_eq!(called(&saved, &first), ["Alice"]);

    // The user's line adds nobody: the table is the same and is not handed
    // on again.
    let second = live.name_when(&folder, heard[..2].to_vec(), true);
    assert_eq!(called(&saved, &second), ["Alice", "You"]);
    assert!(table.take().is_none());

    // Bob's turn: a new table, in which Alice is who she was.
    evidence::append(&folder, &seen[so_far.len()..]).unwrap();
    let third = live.name_when(&folder, heard.clone(), true);
    let grown = table.take().unwrap();
    assert_eq!(called(&grown, &third), ["Alice", "You", "Bob"]);
    assert_eq!(called(&saved, &third), ["Alice", "You", "Them"]);
    assert_eq!(third[0].speaker, first[0].speaker);

    // Only the latest table waits for the recorder.
    table.put(saved);
    table.put(grown.clone());
    assert_eq!(table.take(), Some(grown));
    assert!(table.take().is_none());

    // The lab turned off while recording: the next transcript is unnamed
    // and an empty table goes to the manifest.
    assert_eq!(live.name_when(&folder, heard.clone(), false), heard);
    assert_eq!(table.take(), Some(Speakers::default()));
    fs::remove_dir_all(folder).unwrap();
}

#[test]
fn simultaneous_local_speech_keeps_calibration_healthy_and_names_the_sole_remote_speaker() {
    // Six rounds where Carol (local) talks for 4 s while Alice backchannels
    // inside her turn, plus a seventh round where Alice and Bob both talk over
    // Carol (which must stay Them).
    let mut entries = Vec::new();
    for round in 0..6 {
        let start = round * 10_000;
        entries.push(you(start, start + 4_000, "Walking through the rollout."));
        entries.push(them(start + 1_600, start + 2_400, "Yeah, makes sense."));
    }
    entries.push(you(60_000, 64_000, "Any other questions?"));
    entries.push(them(61_600, 62_400, "Wait, what about caching?"));

    let mut evidence = huddle();
    for look in 0..=66_000 / POLL {
        let at_ms = look * POLL;
        let round = at_ms / 10_000;
        let within = at_ms % 10_000;
        let local_speaking = (500..=5_000).contains(&within);
        let alice = (2_000..=3_500).contains(&within);
        let bob = round == 6 && (2_000..=3_500).contains(&within);
        let holder = match (alice, bob, local_speaking) {
            (true, false, _) => Some(key(ALICE)),
            (false, true, _) => Some(key(BOB)),
            (false, false, true) => Some(key(CAROL)),
            _ => None,
        };
        evidence.push(Evidence::Floor {
            at_ms,
            holder,
            local_speaking,
        });
    }

    let (named, summary) = derive(&entries, &evidence);
    assert_eq!(
        summary.calibration,
        Calibration::Healthy(Timing {
            onset_lag_ms: 500,
            release_tail_ms: 1_000,
        })
    );
    assert_eq!((summary.remote_lines, summary.named_lines), (7, 6));
    let mut expected: Vec<&str> = (0..6).flat_map(|_| ["You", "Alice"]).collect();
    expected.extend(["You", "Them"]);
    assert_eq!(labels(&named), expected);
}
