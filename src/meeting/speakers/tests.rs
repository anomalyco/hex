use std::fs;
use std::time::{Duration, Instant};

use super::*;
use crate::meeting::generator::GeneratedNotes;
use crate::meeting::query::{Library, ListQuery, OffsetRange, SearchQuery, TranscriptQuery};
use crate::meeting::{chapter_entries, markdown, notes, section_markdown, transcript_markdown};

const APP: NameSource = NameSource::CallApp;

/// A manifest exactly as a build without speakers wrote it.
const OLD_MANIFEST: &str = r#"{
  "schema_version": 1,
  "id": "1790000000000-1001",
  "title": "Zoom call — 2:30 PM",
  "status": "complete",
  "created_at_ms": 1790000000000,
  "ended_at_ms": 1790000060000,
  "duration_ms": 60000,
  "system": {
    "file": "system.wav",
    "sample_rate": 16000,
    "first_pts_us": 0,
    "last_pts_us": 60000000,
    "first_arrival_us": 0,
    "samples": 960000,
    "dropped_packets": 0
  },
  "microphone": {
    "file": "microphone.wav",
    "sample_rate": 16000,
    "first_pts_us": 0,
    "last_pts_us": 60000000,
    "first_arrival_us": 0,
    "samples": 960000,
    "dropped_packets": 0
  },
  "live_transcription_error": null,
  "error": null,
  "app": "Zoom",
  "export_path": null,
  "export_error": null,
  "echo_suppressed_utterances": 0,
  "provisional_title": null,
  "notes": null,
  "notes_error": null,
  "calendar": {
    "title": "Launch sync",
    "attendees": [
      "Ana Ruiz",
      "Ben Ito"
    ]
  },
  "title_source": "default"
}
"#;

/// Transcript lines exactly as a build without speakers wrote them.
const OLD_TRANSCRIPT: &str = concat!(
    r#"{"source":"microphone","start_ms":0,"end_ms":900,"text":"Can we still ship Thursday?"}"#,
    "\n",
    r#"{"source":"system","start_ms":1000,"end_ms":1900,"text":"Thursday works."}"#,
    "\n",
);

fn manifest() -> MeetingManifest {
    serde_json::from_str(OLD_MANIFEST).unwrap()
}

fn written(manifest: &MeetingManifest) -> String {
    format!("{}\n", serde_json::to_string_pretty(manifest).unwrap())
}

fn lines(entries: &[TranscriptEntry]) -> String {
    entries
        .iter()
        .map(|entry| format!("{}\n", serde_json::to_string(entry).unwrap()))
        .collect()
}

fn said(
    source: MeetingSource,
    speaker: Option<&SpeakerId>,
    second: u64,
    text: &str,
) -> TranscriptEntry {
    TranscriptEntry {
        speaker: speaker.cloned(),
        ..TranscriptEntry::new(source, second * 1_000, second * 1_000 + 900, text)
    }
}

/// A call where Hex named two of the remote speakers and was unsure of one
/// line.
fn named_call() -> (MeetingManifest, Vec<TranscriptEntry>) {
    let mut manifest = manifest();
    let priya = manifest.speakers.add("Priya Nair", APP).unwrap();
    let marcus = manifest.speakers.add("Marcus Webb", APP).unwrap();
    let entries = vec![
        said(
            MeetingSource::Microphone,
            None,
            5,
            "Can we still ship Thursday?",
        ),
        said(MeetingSource::System, Some(&priya), 12, "Thursday works."),
        said(MeetingSource::System, None, 19, "Yeah."),
        said(
            MeetingSource::System,
            Some(&marcus),
            21,
            "I can take the migration.",
        ),
    ];
    (manifest, entries)
}

// MARK: What is saved

#[test]
fn an_old_manifest_and_transcript_round_trip_byte_for_byte() {
    let manifest = manifest();
    assert!(manifest.speakers.is_empty());
    assert_eq!(written(&manifest), OLD_MANIFEST);

    let entries = OLD_TRANSCRIPT
        .lines()
        .map(|line| serde_json::from_str::<TranscriptEntry>(line).unwrap())
        .collect::<Vec<_>>();
    assert!(entries.iter().all(|entry| entry.speaker.is_none()));
    assert_eq!(lines(&entries), OLD_TRANSCRIPT);
}

#[test]
fn speakers_are_saved_as_a_table_and_an_id_on_each_named_line() {
    let (manifest, entries) = named_call();
    let saved: serde_json::Value = serde_json::from_str(&written(&manifest)).unwrap();
    assert_eq!(
        saved["speakers"],
        serde_json::json!([
            {"id": "s1", "name": "Priya Nair", "from": "call_app"},
            {"id": "s2", "name": "Marcus Webb", "from": "call_app"},
        ])
    );
    assert_eq!(
        serde_json::to_string(&entries[1]).unwrap(),
        r#"{"source":"system","start_ms":12000,"end_ms":12900,"text":"Thursday works.","speaker":"s1"}"#
    );
    // A line Hex was not sure of is saved as it always was.
    assert_eq!(
        serde_json::to_string(&entries[2]).unwrap(),
        r#"{"source":"system","start_ms":19000,"end_ms":19900,"text":"Yeah."}"#
    );

    let read: MeetingManifest = serde_json::from_str(&written(&manifest)).unwrap();
    assert_eq!(read.speakers, manifest.speakers);
    assert_eq!(written(&read), written(&manifest));
}

#[test]
fn ids_are_minted_per_meeting_and_never_reused_within_one() {
    let mut speakers = Speakers::default();
    let ids = ["Ana", "Ben", "Cy"].map(|name| speakers.add(name, APP).unwrap());
    assert_eq!(
        serde_json::to_value(&ids).unwrap(),
        serde_json::json!(["s1", "s2", "s3"])
    );

    // A table read with a gap still hands out an id nobody holds.
    let mut read: Speakers = serde_json::from_value(serde_json::json!([
        {"id": "s2", "name": "Ben", "from": "calendar"},
    ]))
    .unwrap();
    let ana = read.add("Ana", NameSource::User).unwrap();
    assert_eq!(serde_json::to_value(&ana).unwrap(), "s1");
    assert_eq!(read.names().collect::<Vec<_>>(), ["Ben", "Ana"]);
}

// MARK: Names are untrusted

#[test]
fn a_hostile_name_comes_out_clean() {
    let markup = "  Priya\n**Nair**\t`rm`\u{202E} <b>[x](http://evil)\\\u{E0041}\u{0} ";
    let hostile = format!("{markup}{}", "a".repeat(500));
    let mut speakers = Speakers::default();
    let id = speakers.add(&hostile, APP).unwrap();
    let name = speakers.name(&id).unwrap();

    assert!(
        name.starts_with("Priya Nair rm bx(http://evil) aaa"),
        "{name}"
    );
    assert_eq!(name.chars().count(), NAME_CHARS);
    assert_eq!(name.lines().count(), 1);
    assert!(!name.contains(MARKUP) && !name.contains("  "), "{name}");
    assert!(name.chars().all(|c| !c.is_control() && !is_invisible(c)));

    // So a turn heading stays one heading, and reads back as one.
    let line = said(MeetingSource::System, Some(&id), 1, "Hello.");
    let heading = format!("**{name}** · 00:01\nHello.");
    assert_eq!(
        markdown::transcript_markdown_with(&[line], Labels::by_track(&speakers)),
        heading
    );
    assert_eq!(markdown::entries_from_markdown(&heading).len(), 1);
}

#[test]
fn a_name_keeps_what_real_names_need() {
    let mut speakers = Speakers::default();
    for name in [
        "Sam's iPhone",
        "José Ñ. (he/him)",
        "田中 太郎",
        "kit_l@work",
        "Tom & Ana 👩‍💻",
    ] {
        let id = speakers.add(name, APP).unwrap();
        assert_eq!(speakers.name(&id), Some(name));
    }
}

#[test]
fn a_name_can_never_be_one_of_hexs_own_labels_or_nothing() {
    let mut speakers = Speakers::default();
    for name in [
        "You", " you ", "**You**", "THEM", "Speaker", "notes", "", " \n\t", "***", "\u{200B}",
    ] {
        assert_eq!(speakers.add(name, APP), None, "{name:?}");
    }
    assert!(speakers.is_empty());
}

#[test]
fn a_table_read_from_disk_is_cleaned_like_one_built_here() {
    let speakers: Speakers = serde_json::from_value(serde_json::json!([
        {"id": "s1", "name": "Priya\n**Nair**", "from": "call_app"},
        {"id": "s1", "name": "A second s1", "from": "user"},
        {"id": "s2", "name": "You", "from": "call_app"},
        {"id": "s3", "name": "  ", "from": "calendar"},
    ]))
    .unwrap();
    assert_eq!(speakers.names().collect::<Vec<_>>(), ["Priya Nair"]);
    // The dropped rows' lines fall back to the track's label.
    let orphan = said(
        MeetingSource::System,
        Some(&SpeakerId("s2".into())),
        0,
        "Hi.",
    );
    assert_eq!(Labels::by_track(&speakers).of(&orphan).as_str(), "Them");
}

#[test]
fn printing_a_table_for_a_log_never_prints_a_name() {
    let (manifest, _) = named_call();
    let printed = format!(
        "{manifest:?} {:?} {:?}",
        manifest.speakers,
        manifest.labels()
    );
    assert!(
        printed.contains("s1") && printed.contains("CallApp"),
        "{printed}"
    );
    assert!(
        !printed.contains("Priya") && !printed.contains("Marcus"),
        "{printed}"
    );
}

// MARK: What a line is called

#[test]
fn a_line_is_called_by_its_speakers_name_or_else_what_it_always_was() {
    use MeetingSource::{Microphone, System};
    let cases = [
        (System, Some("Priya Nair"), false, "Priya Nair"),
        (System, None, false, "Them"),
        (Microphone, None, false, "You"),
        // The microphone is the local user, whatever a line claims.
        (Microphone, Some("Priya Nair"), false, "You"),
        // An imported file has one track.
        (System, None, true, "Speaker"),
        (Microphone, None, true, "Speaker"),
        (System, Some("Priya Nair"), true, "Priya Nair"),
    ];
    for (source, name, imported, label) in cases {
        let got = Label::of(source, name, imported);
        assert_eq!(
            got.as_str(),
            label,
            "{source:?} {name:?} imported {imported}"
        );
        assert_eq!(got.to_string(), label);
        assert_eq!(got.name(), name.filter(|_| source == System));
    }
}

#[test]
fn a_speaker_the_table_does_not_know_is_called_by_the_track() {
    let (manifest, mut entries) = named_call();
    entries[1].speaker = Some(SpeakerId("s9".into()));
    let labels = entries
        .iter()
        .map(|entry| manifest.labels().of(entry).as_str())
        .collect::<Vec<_>>();
    assert_eq!(labels, ["You", "Them", "Them", "Marcus Webb"]);
    // With nobody named, every label is the track's.
    let unnamed = entries
        .iter()
        .map(|entry| Labels::default().of(entry).as_str())
        .collect::<Vec<_>>();
    assert_eq!(unnamed, ["You", "Them", "Them", "Them"]);
}

#[test]
fn a_rendered_label_reads_back_to_its_track() {
    for (text, source) in [
        ("You", MeetingSource::Microphone),
        ("Them", MeetingSource::System),
        ("Speaker", MeetingSource::System),
        ("Priya Nair", MeetingSource::System),
    ] {
        assert_eq!(Label::parse(text).source(), source);
        assert_eq!(Label::parse(text).as_str(), text);
    }
}

// MARK: Every reader asks the same place

#[test]
fn markdown_shows_the_name_in_the_export_a_section_and_the_folders_own_file() {
    let (manifest, entries) = named_call();
    let turns = "**You** · 00:05\nCan we still ship Thursday?\n\n\
                 **Priya Nair** · 00:12\nThursday works.\n\n\
                 **Them** · 00:19\nYeah.\n\n\
                 **Marcus Webb** · 00:21\nI can take the migration.";
    assert_eq!(transcript_markdown(&manifest, &entries), turns);
    assert!(markdown::render_document(&manifest, &entries).ends_with(&format!("{turns}\n\n")));

    let chapter = crate::meeting::Chapter {
        title: "Shipping".into(),
        summary: String::new(),
        start_ms: 0,
    };
    let section = section_markdown(
        &chapter,
        &chapter_entries(&entries, std::slice::from_ref(&chapter), 0),
        manifest.labels(),
    );
    assert_eq!(section, format!("## Shipping\n\n{turns}"));

    let labels = Labels::by_track(&manifest.speakers);
    assert_eq!(
        entries
            .iter()
            .map(|entry| markdown::folder_line(labels, entry))
            .collect::<Vec<_>>(),
        [
            "**00:05 You**  Can we still ship Thursday?",
            "**00:12 Priya Nair**  Thursday works.",
            "**00:19 Them**  Yeah.",
            "**00:21 Marcus Webb**  I can take the migration.",
        ]
    );
}

#[test]
fn exported_markdown_parses_back_into_turns_whoever_is_named() {
    let (manifest, entries) = named_call();
    let parsed = markdown::entries_from_markdown(&markdown::render_document(&manifest, &entries));
    let turn =
        |source, start_ms, end_ms, text: &str| TranscriptEntry::new(source, start_ms, end_ms, text);
    assert_eq!(
        parsed,
        [
            turn(
                MeetingSource::Microphone,
                5_000,
                12_000,
                "Can we still ship Thursday?"
            ),
            turn(MeetingSource::System, 12_000, 19_000, "Thursday works."),
            turn(MeetingSource::System, 19_000, 21_000, "Yeah."),
            turn(
                MeetingSource::System,
                21_000,
                23_000,
                "I can take the migration."
            ),
        ]
    );
    // An imported meeting's one track reads back too.
    let imported = "**Speaker** · 00:01\nHi.\n\n**Speaker** · 00:04\nHello.\n";
    assert_eq!(markdown::entries_from_markdown(imported).len(), 2);
}

#[test]
fn the_notes_prompt_gives_each_line_its_speaker() {
    let (manifest, entries) = named_call();
    let prompt_under = |labels: Labels<'_>| {
        let mut prompt = String::new();
        notes::summarize_with_calendar(
            &entries,
            labels,
            None,
            &[],
            12_000,
            Instant::now() + Duration::from_secs(60),
            |_, text, _: Duration| {
                prompt = text.to_owned();
                Ok(GeneratedNotes {
                    title: "Launch".into(),
                    summary: "They met.".into(),
                    ..Default::default()
                })
            },
        )
        .unwrap();
        prompt
    };
    assert_eq!(
        prompt_under(Labels::by_track(&manifest.speakers)),
        "Meeting transcript:\n\n\
         [00:05] You: Can we still ship Thursday?\n\
         [00:12] Priya Nair: Thursday works.\n\
         [00:19] Them: Yeah.\n\
         [00:21] Marcus Webb: I can take the migration."
    );
    // A table that knows nobody: every remote line is Them again.
    assert_eq!(
        prompt_under(Labels::default()),
        "Meeting transcript:\n\n\
         [00:05] You: Can we still ship Thursday?\n\
         [00:12] Them: Thursday works.\n\
         [00:19] Them: Yeah.\n\
         [00:21] Them: I can take the migration."
    );
}

#[test]
fn agents_and_the_cli_read_the_name_in_transcripts_search_and_the_list() {
    const NOW: u64 = 1_790_100_000_000;
    let (manifest, entries) = named_call();
    let root = std::env::temp_dir().join(format!("hex-speaker-names-{}", std::process::id()));
    let _ = fs::remove_dir_all(&root);
    let folder = root.join(&manifest.id);
    fs::create_dir_all(&folder).unwrap();
    fs::write(folder.join("manifest.json"), written(&manifest)).unwrap();
    fs::write(folder.join("transcript.ndjson"), lines(&entries)).unwrap();
    fs::write(folder.join("transcript.md"), "").unwrap();
    let library = Library::new(&root);

    let shown = library
        .markdown(&manifest.id, OffsetRange::default(), NOW)
        .unwrap();
    assert!(
        shown.contains("**Priya Nair** · 00:12\nThursday works."),
        "{shown}"
    );

    let query = TranscriptQuery {
        meeting_id: manifest.id.clone(),
        start: None,
        end: None,
        speaker: None,
        max_chars: 20_000,
        cursor: None,
    };
    let transcript = library.transcript(&query, NOW).unwrap();
    for line in [
        "] You: Can we still ship Thursday?",
        "] Priya Nair: Thursday works.",
        "] Them: Yeah.",
        "] Marcus Webb: I can take the migration.",
    ] {
        assert!(transcript.contains(line), "{line}\n{transcript}");
    }

    let query = SearchQuery {
        query: "migration".into(),
        since: None,
        until: None,
        meeting_id: None,
        limit: 10,
        fuzzy: false,
        include_in_progress: true,
        vocabulary: Vec::new(),
    };
    let found = library.search(&query, NOW).unwrap();
    assert!(found.contains("· Marcus Webb ·"), "{found}");
    assert!(found.contains("before (Them 00:19):"), "{found}");

    let query = ListQuery {
        since: None,
        until: None,
        query: None,
        limit: 20,
        include_in_progress: true,
    };
    let listed = library.list(&query, NOW).unwrap();
    assert!(
        listed.contains("participants: You, Them, Priya Nair, Marcus Webb (Zoom)"),
        "{listed}"
    );
    fs::remove_dir_all(&root).unwrap();
}
