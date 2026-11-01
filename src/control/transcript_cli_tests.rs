use super::*;
use clap::Parser;
use std::fs;
use std::path::PathBuf;

const NOW: u64 = 1_792_700_000_000;

/// The shared command tree every OS's `hex` flattens.
#[derive(Parser)]
struct Cli {
    #[command(subcommand)]
    command: Option<ControlCli>,
}

use crate::control::cli::ControlCli;
const FIXTURE: &str = include_str!("../../tests/fixtures/history.json");

fn args(extra: &[&str]) -> HistoryArgs {
    let cli =
        Cli::try_parse_from(["hex", "history"].into_iter().chain(extra.iter().copied())).unwrap();
    match cli.command.unwrap() {
        ControlCli::History(args) => args,
        _ => panic!("expected history"),
    }
}

fn temp() -> PathBuf {
    let root = std::env::temp_dir().join(format!(
        "hex-transcript-cli-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    fs::create_dir_all(&root).unwrap();
    root
}

fn entries() -> Vec<HistoryEntry> {
    serde_json::from_value(
        serde_json::from_str::<serde_json::Value>(FIXTURE).unwrap()["entries"].clone(),
    )
    .unwrap()
}

#[test]
fn history_filters_and_order_apply_before_the_limit() {
    let entries = entries();
    let query = args(&[
        "--since",
        "3d",
        "--app",
        "opencode beta",
        "--grep",
        "PROBLEMS",
        "--limit",
        "1",
    ]);
    let output = history_output(&entries, &query, HistoryRetention::Week, NOW, &[]).unwrap();
    assert_eq!(output.entries.iter().map(|e| e.id).collect::<Vec<_>>(), [2]);
    assert!(!output.truncated);
    let output = history_output(
        &entries,
        &args(&["--oldest-first", "--limit", "1"]),
        HistoryRetention::Week,
        NOW,
        &[],
    )
    .unwrap();
    assert_eq!(output.entries[0].id, 2);
    assert!(output.truncated);
    let output = history_output(
        &entries,
        &args(&["--grep", "needle"]),
        HistoryRetention::Week,
        NOW,
        &[],
    )
    .unwrap();
    assert_eq!(output.entries[0].id, 4);
    assert!(output.entries[0].raw_text.is_none());
    let output = history_output(
        &entries,
        &args(&["--until", "1792700000000"]),
        HistoryRetention::Week,
        NOW,
        &[],
    )
    .unwrap();
    assert_eq!(
        output.entries.iter().map(|e| e.id).collect::<Vec<_>>(),
        [3, 2]
    );
    assert!(
        history_output(
            &entries,
            &args(&["--since", "now", "--until", "1d"]),
            HistoryRetention::Week,
            NOW,
            &[]
        )
        .is_err()
    );
}

#[test]
fn history_stats_parse_sum_retained_entries_and_need_the_labs_toggle() {
    let query = args(&["stats", "--since", "3d", "--json"]);
    assert!(matches!(
        query.action,
        Some(HistoryAction::Stats { ref since, json: true, .. }) if since.as_deref() == Some("3d")
    ));
    const DAY: u64 = 86_400_000;
    let entry = |id: u64, age_days: u64, raw: &str, final_text: &str| HistoryEntry {
        id,
        timestamp_ms: NOW - age_days * DAY,
        kind: HistoryKind::Dictation,
        raw_text: raw.into(),
        final_text: final_text.into(),
        application: Some("Zed".into()),
        processing: None,
        audio_ms: 6_000,
        inference_ms: 0,
        total_ms: 0,
    };
    let entries = [
        entry(1, 10, "expired by retention", "expired by retention"),
        entry(2, 1, "so um yes", "Yes."),
        entry(3, 0, "ten words", "ten words"),
    ];
    let utc = |ms: u64| (ms / DAY) as i64;
    let output = stats_output(&entries, None, None, HistoryRetention::Week, NOW, utc).unwrap();
    assert_eq!(output.stats.dictations, 2);
    assert_eq!(output.stats.words, 3);
    assert_eq!(output.stats.words_per_minute, Some(15));
    assert_eq!(output.stats.changed_by_output, 1);
    assert_eq!(output.stats.streak_days, 2);
    assert_eq!(output.stats.top_apps[0].dictations, 2);
    let json = serde_json::to_value(&output).unwrap();
    assert_eq!(json["version"], 1);
    assert_eq!(json["words"], 3);
    let today = stats_output(
        &entries,
        Some("today"),
        None,
        HistoryRetention::Week,
        NOW,
        utc,
    );
    assert!(today.unwrap().stats.dictations <= 1);
    assert!(stats_text(&HistoryStats::default()).starts_with("No retained"));
    assert!(require_history_stats(false).is_err());
    assert!(require_history_stats(true).is_ok());
}

#[test]
fn history_search_ranks_fuzzy_stemmed_matches_within_retention() {
    let mut entries = entries();
    entries.push(HistoryEntry {
        id: 5,
        timestamp_ms: 1_792_650_000_000,
        final_text: "Board games tonight.".into(),
        ..entries[2].clone()
    });
    let output = history_output(
        &entries,
        &args(&["--search", "problem bord"]),
        HistoryRetention::Week,
        NOW,
        &[],
    )
    .unwrap();
    // Entry 2 matches both words; 5 only one; 1 has expired; 3 and 4 miss.
    assert_eq!(
        output.entries.iter().map(|e| e.id).collect::<Vec<_>>(),
        [2, 5]
    );
    assert!(output.entries[0].score.unwrap() > output.entries[1].score.unwrap());
    assert_eq!(
        output.entries[0].snippet.as_deref(),
        Some("The **Problems** **board**.")
    );
    // The raw transcript is searched too, and filters still apply first.
    let output = history_output(
        &entries,
        &args(&["--search", "needle", "--app", "opencode beta"]),
        HistoryRetention::Week,
        NOW,
        &[],
    )
    .unwrap();
    assert_eq!(output.entries[0].id, 4);
    assert_eq!(
        output.entries[0].snippet.as_deref(),
        Some("raw only **NEEDLE**")
    );
    assert!(
        history_output(
            &entries,
            &args(&["--search", "\"\""]),
            HistoryRetention::Week,
            NOW,
            &[]
        )
        .is_err()
    );
    assert!(Cli::try_parse_from(["hex", "history", "--search", "x", "--oldest-first"]).is_err());
}

#[test]
fn history_search_counts_raw_and_final_text_as_one_document() {
    let template = entries()[2].clone();
    let entry = |id: u64, final_text: &str, raw_text: &str| HistoryEntry {
        id,
        timestamp_ms: NOW - id * 1_000,
        final_text: final_text.into(),
        raw_text: raw_text.into(),
        ..template.clone()
    };
    // Two entries say `dax`, each in both texts: still fewer than three
    // documents, so the sound-alike `Dacks` is found.
    let entries = [
        entry(1, "Ask Dax about the release.", "ask dax about the release"),
        entry(2, "Dax shipped the fix.", "dax shipped the fix"),
        entry(3, "Dacks reviewed it.", "Dacks reviewed it."),
    ];
    let output = history_output(
        &entries,
        &args(&["--search", "Dax"]),
        HistoryRetention::Week,
        NOW,
        &[],
    )
    .unwrap();
    let mut ids = output.entries.iter().map(|e| e.id).collect::<Vec<_>>();
    assert_eq!(ids.pop(), Some(3), "the fuzzy match ranks last");
    ids.sort_unstable();
    assert_eq!(ids, [1, 2]);
}

#[test]
fn history_json_contract_snapshot() {
    let entries = entries();
    let output = history_output(
        &entries,
        &args(&["--grep", "problems", "--raw"]),
        HistoryRetention::Week,
        NOW,
        &[],
    )
    .unwrap();
    assert_eq!(
        serde_json::to_value(output).unwrap(),
        serde_json::json!({
            "version":1,"retention":"week","truncated":false,"entries":[{
                "id":2,"timestamp_ms":1792540000000_u64,"application":"OpenCode Beta","mode":"Engineering",
                "kind":"dictation","final_text":"The Problems board.","raw_text":"the problems board"
            }]
        })
    );
    let mut entries = entries;
    entries[3].application = None;
    let output = history_output(
        &entries,
        &args(&["--limit", "1"]),
        HistoryRetention::Week,
        NOW,
        &[],
    )
    .unwrap();
    assert_eq!(
        serde_json::to_value(output).unwrap(),
        serde_json::json!({
            "version":1,"retention":"week","truncated":true,"entries":[{
                "id":4,"timestamp_ms":1792700000000_u64,"application":null,"mode":null,
                "kind":"send","final_text":"Recent result"
            }]
        })
    );
}

#[test]
fn retention_defaults_fails_closed_and_off_preserves_existing_entries() {
    let root = temp();
    assert_eq!(retention(&root).unwrap(), HistoryRetention::Week);
    fs::write(root.join("settings.json"), "{}").unwrap();
    assert_eq!(retention(&root).unwrap(), HistoryRetention::Week);
    fs::write(root.join("settings.json"), r#"{"history_retention":"day"}"#).unwrap();
    let entries = entries();
    let output = history_output(&entries, &args(&[]), retention(&root).unwrap(), NOW, &[]).unwrap();
    assert_eq!(output.entries.iter().map(|e| e.id).collect::<Vec<_>>(), [4]);
    let output = history_output(&entries, &args(&[]), HistoryRetention::Off, NOW, &[]).unwrap();
    assert_eq!(output.entries.len(), 4);
    fs::write(root.join("settings.json"), "{broken").unwrap();
    assert!(retention(&root).is_err());
    assert_eq!(
        fs::read_to_string(root.join("settings.json")).unwrap(),
        "{broken"
    );
    fs::remove_dir_all(root).unwrap();
}

#[test]
fn atomic_replace_read_never_observes_partial_history() {
    let root = temp();
    let path = root.join("history.json");
    fs::write(&path, FIXTURE).unwrap();
    let writer_path = path.clone();
    let barrier = std::sync::Arc::new(std::sync::Barrier::new(2));
    let writer_barrier = barrier.clone();
    let writer = std::thread::spawn(move || {
        writer_barrier.wait();
        for index in 0..100 {
            let content = if index % 2 == 0 {
                FIXTURE.to_owned()
            } else {
                FIXTURE.replace("Recent result", "replacement")
            };
            fs::write(writer_path.with_extension("tmp"), content).unwrap();
            fs::rename(writer_path.with_extension("tmp"), &writer_path).unwrap();
        }
    });
    barrier.wait();
    for _ in 0..100 {
        let entries = crate::history::read_snapshot(&path).unwrap();
        assert_eq!(entries.len(), 4);
        assert!(matches!(
            entries[3].final_text.as_str(),
            "Recent result" | "replacement"
        ));
    }
    writer.join().unwrap();
    fs::write(&path, "{broken").unwrap();
    assert!(crate::history::read_snapshot(&path).is_err());
    assert_eq!(fs::read_to_string(&path).unwrap(), "{broken");
    assert!(!path.with_extension("json.corrupt").exists());
    fs::remove_dir_all(root).unwrap();
}

#[test]
fn meeting_json_contract_snapshot() {
    let library =
        Library::new(Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/meetings"));
    let meeting = library.meeting("1790000500000-1002").unwrap();
    let json = meeting_json(&meeting, OffsetRange::default(), NOW).unwrap();
    let expected: serde_json::Value =
        serde_json::from_str(include_str!("../../tests/fixtures/meeting-cli.json")).unwrap();
    assert_eq!(json, expected);
    let scan = crate::meeting::query::Scan {
        meetings: vec![meeting],
        skipped: vec![],
    };
    assert_eq!(
        meeting_list_json(&scan, 20, NOW),
        serde_json::json!({
            "version": 1, "meetings": [expected["meeting"]], "skipped": [], "truncated": false,
        })
    );
}

#[test]
fn meeting_show_ranges_filter_json_entries_and_markdown() {
    let library =
        Library::new(Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/meetings"));
    let meeting = library.meeting("1790000500000-1002").unwrap();
    let range = OffsetRange::parse(Some("0:06"), Some("14")).unwrap();
    let json = meeting_json(&meeting, range, NOW).unwrap();
    assert_eq!(json["version"], 1);
    assert_eq!(
        json["range"],
        serde_json::json!({"from_ms": 6000, "to_ms": 14000})
    );
    let starts = json["entries"]
        .as_array()
        .unwrap()
        .iter()
        .map(|entry| entry["start_ms"].as_u64().unwrap())
        .collect::<Vec<_>>();
    assert_eq!(starts, [5500, 13000]);
    let open = meeting_json(
        &meeting,
        OffsetRange::parse(Some("1:00"), None).unwrap(),
        NOW,
    )
    .unwrap();
    assert_eq!(
        open["range"],
        serde_json::json!({"from_ms": 60000, "to_ms": null})
    );
    assert_eq!(open["entries"].as_array().unwrap().len(), 1);
    let late = OffsetRange::parse(Some("1:05"), None).unwrap();
    assert!(meeting_json(&meeting, late, NOW).is_err());
    assert!(library.markdown(&meeting.id, late, NOW).is_err());

    let markdown = library.markdown(&meeting.id, range, NOW).unwrap();
    assert!(markdown.contains("- Range: 00:06–00:14\n"), "{markdown}");
    assert!(markdown.contains("**You** · 00:05\nPerformance profiling"));
    assert!(markdown.contains("**Them** · 00:13\nAgreed."));
    assert!(!markdown.contains("Let's review") && !markdown.contains("release notes"));

    assert!(OffsetRange::parse(Some("0:14"), Some("0:06")).is_err());
    assert!(OffsetRange::parse(Some("0:06"), Some("0:06")).is_err());
    let cli = Cli::try_parse_from(["hex", "meetings", "show", "latest", "--to", "0"]).unwrap();
    let Some(ControlCli::Meetings {
        command: MeetingsCommand::Show { from, to, .. },
    }) = cli.command
    else {
        panic!("expected meetings show");
    };
    assert_eq!(
        OffsetRange::parse(from.as_deref(), to.as_deref())
            .unwrap_err()
            .to_string(),
        "The range start 00:00 must be earlier than its end 00:00."
    );
    let cli = Cli::try_parse_from([
        "hex", "meetings", "show", "latest", "--from", "1:53:03", "--to", "120:43", "--json",
    ])
    .unwrap();
    let Some(ControlCli::Meetings {
        command: MeetingsCommand::Show { from, to, .. },
    }) = cli.command
    else {
        panic!("expected meetings show");
    };
    let range = OffsetRange::parse(from.as_deref(), to.as_deref()).unwrap();
    assert_eq!(range.from_ms, Some(6_783_000));
    assert_eq!(range.to_ms, Some(7_243_000));
}

#[test]
fn meeting_chapters_list_and_select_a_chapter_in_json_and_markdown() {
    let library = Library::new(
        Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/meetings-chapters"),
    );
    let meeting = resolve(&library, "latest").unwrap();
    assert_eq!(meeting.id, "1790003000000-1005");
    let chapters = serde_json::json!([
        {"number": 1, "title": "Image storage",
         "summary": "Where uploaded images live and how long they are kept.",
         "start_ms": 0, "end_ms": 180000},
        {"number": 2, "title": "Motion lint",
         "summary": "A lint that flags animations which snap instead of easing.",
         "start_ms": 180000, "end_ms": 390000},
        {"number": 3, "title": "Chat app name", "summary": "Picking a name for the chat app.",
         "start_ms": 390000, "end_ms": 540000},
    ]);
    let listed = meeting_chapters_json(&meeting, NOW);
    assert_eq!(listed["version"], 1);
    assert_eq!(listed["chapters"], chapters);
    assert_eq!(listed["refined"], true);
    // A calendar title is extended by the chapters.
    assert_eq!(
        listed["meeting"]["title"],
        "Team sync: Image storage, Motion lint, Chat app name"
    );

    let shown = meeting_json(&meeting, OffsetRange::default(), NOW).unwrap();
    assert_eq!(shown["chapters"], chapters);
    assert_eq!(shown["entries"].as_array().unwrap().len(), 8);
    assert!(shown.get("chapter").is_none());

    let second = meeting_chapter_json(&meeting, 2, NOW).unwrap();
    assert_eq!(second["chapter"], chapters[1]);
    assert_eq!(second["chapters"], chapters);
    let starts = second["entries"]
        .as_array()
        .unwrap()
        .iter()
        .map(|entry| entry["start_ms"].as_u64().unwrap())
        .collect::<Vec<_>>();
    assert_eq!(starts, [181000, 240000, 300000]);
    for missing in [0, 4] {
        assert_eq!(
            meeting_chapter_json(&meeting, missing, NOW)
                .unwrap_err()
                .to_string(),
            format!(
                "Meeting 1790003000000-1005 has chapters 1 to 3; there is no chapter {missing}."
            )
        );
    }

    let section = library.chapter_markdown(&meeting.id, 2, NOW).unwrap();
    assert!(
        section.starts_with("# Team sync: Image storage, Motion lint, Chat app name\n"),
        "{section}"
    );
    assert!(
        section.contains("- Chapter: 2 of 3 · 03:00–06:30\n"),
        "{section}"
    );
    assert!(
        section.ends_with(
            "\n## Motion lint\n\nA lint that flags animations which snap instead of easing.\n\n\
             **You** · 03:01\nNext, the motion lint. It flags animations that snap.\n\n\
             **Them** · 04:00\nCan it run in CI on every pull request?\n\n\
             **You** · 05:00\nYes, it reads the recorded traces.\n"
        ),
        "{section}"
    );

    let full = library
        .markdown(&meeting.id, OffsetRange::default(), NOW)
        .unwrap();
    assert!(
        full.contains(
            "\n## Chapters\n\n\
             1. 00:00 · Image storage — Where uploaded images live and how long they are kept.\n\
             2. 03:00 · Motion lint — A lint that flags animations which snap instead of easing.\n\
             3. 06:30 · Chat app name — Picking a name for the chat app.\n\n## Transcript\n"
        ),
        "{full}"
    );

    // Meetings without chapters: an empty list, and no field on `show`.
    let plain = Library::new(Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/meetings"));
    let roadmap = plain.meeting("1790000500000-1002").unwrap();
    let listed = meeting_chapters_json(&roadmap, NOW);
    assert_eq!(listed["chapters"], serde_json::json!([]));
    assert_eq!(listed["refined"], false);
    assert!(
        meeting_json(&roadmap, OffsetRange::default(), NOW)
            .unwrap()
            .get("chapters")
            .is_none()
    );
    assert_eq!(
        meeting_chapter_json(&roadmap, 1, NOW)
            .unwrap_err()
            .to_string(),
        "Meeting 1790000500000-1002 has no chapters."
    );
    assert!(
        !plain
            .markdown(&roadmap.id, OffsetRange::default(), NOW)
            .unwrap()
            .contains("## Chapters")
    );
}

#[test]
fn subtitles_render_speaker_turns_as_srt_and_vtt_cues() {
    use crate::meeting::{MeetingSource, TranscriptEntry};
    let entries = [
        TranscriptEntry {
            source: MeetingSource::Microphone,
            start_ms: 5_500,
            end_ms: 8_000,
            text: "Hello <there> & welcome.".into(),
        },
        TranscriptEntry {
            source: MeetingSource::System,
            start_ms: 3_725_042,
            end_ms: 3_725_000,
            text: "First line\n\n  \nsecond --> line".into(),
        },
        TranscriptEntry {
            source: MeetingSource::System,
            start_ms: 3_800_000,
            end_ms: 3_801_000,
            text: " \n ".into(),
        },
    ];
    assert_eq!(
        render_subtitles(&entries, SubtitleFormat::Srt),
        "1\n00:00:05,500 --> 00:00:08,000\nYou: Hello <there> & welcome.\n\n\
         2\n01:02:05,042 --> 01:02:05,042\nThem: First line\nsecond -> line\n\n"
    );
    assert_eq!(
        render_subtitles(&entries, SubtitleFormat::Vtt),
        "WEBVTT\n\n\
         00:00:05.500 --> 00:00:08.000\n<v You>Hello &lt;there&gt; &amp; welcome.\n\n\
         01:02:05.042 --> 01:02:05.042\n<v Them>First line\nsecond -&gt; line\n\n"
    );
    assert_eq!(subtitle_time(360_000_000, ','), "100:00:00,000");
}

#[test]
fn subtitles_follow_the_range_and_need_the_labs_toggle() {
    let library =
        Library::new(Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/meetings"));
    let meeting = library.meeting("1790000500000-1002").unwrap();
    let range = OffsetRange::parse(Some("0:06"), Some("14")).unwrap();
    let srt = render_subtitles(meeting.entries_in(range, NOW).unwrap(), SubtitleFormat::Srt);
    assert!(srt.starts_with("1\n00:00:05,500 --> "), "{srt}");
    assert!(srt.contains("\n2\n00:00:13,000 --> "), "{srt}");
    assert!(srt.contains("You: Performance profiling"), "{srt}");
    assert!(srt.contains("Them: Agreed."), "{srt}");
    assert!(!srt.contains("\n3\n"), "{srt}");

    assert!(require_subtitle_export(true).is_ok());
    assert!(
        require_subtitle_export(false)
            .unwrap_err()
            .to_string()
            .contains("hex labs on subtitle-export")
    );
    assert_eq!(SubtitleFormat::from_flags(false, false), None);
    assert_eq!(
        SubtitleFormat::from_flags(false, true),
        Some(SubtitleFormat::Vtt)
    );
    assert!(Cli::try_parse_from(["hex", "meetings", "show", "latest", "--srt"]).is_ok());
    assert!(
        Cli::try_parse_from([
            "hex",
            "meetings",
            "show",
            "latest",
            "--vtt",
            "--chapter",
            "1"
        ])
        .is_ok()
    );
    for conflict in [["--srt", "--vtt"], ["--srt", "--json"], ["--vtt", "--md"]] {
        let command = ["hex", "meetings", "show", "latest"]
            .into_iter()
            .chain(conflict);
        assert!(Cli::try_parse_from(command).is_err(), "{conflict:?}");
    }
}

#[test]
fn parses_release_commands_and_rejects_invalid_limits_and_times() {
    for command in [
        vec!["hex", "history"],
        vec!["hex", "meetings", "list", "--json"],
        vec!["hex", "meetings", "show", "latest", "--md"],
        vec!["hex", "meetings", "search", "problems"],
        vec!["hex", "meetings", "chapters", "latest", "--json"],
        vec![
            "hex",
            "meetings",
            "show",
            "latest",
            "--chapter",
            "2",
            "--json",
        ],
    ] {
        assert!(Cli::try_parse_from(command).is_ok());
    }
    for command in [
        vec!["hex", "history", "--limit", "0"],
        vec!["hex", "history", "--limit", "10001"],
        vec!["hex", "meetings", "show", "latest", "--md", "--json"],
        vec![
            "hex",
            "meetings",
            "show",
            "latest",
            "--chapter",
            "2",
            "--from",
            "1:00",
        ],
        vec!["hex", "meetings", "show", "latest", "--chapter", "two"],
    ] {
        assert!(Cli::try_parse_from(command).is_err());
    }
    for invalid in [
        "2026-02-30",
        "2026-00-01",
        "2026-10-20T23:00:99Z",
        "2026-10-20T23:00+99:00",
        "18446744073709551615d",
    ] {
        assert!(
            crate::meeting::query::parse_time(invalid, NOW, false).is_err(),
            "{invalid}"
        );
    }
    let start = crate::meeting::query::parse_time("2026-10-20", NOW, false).unwrap();
    let end = crate::meeting::query::parse_time("2026-10-20", NOW, true).unwrap();
    let range = TimeRange::parse(Some("2026-10-20"), Some("2026-10-20"), NOW).unwrap();
    assert!(range.contains(start));
    assert!(range.contains(end - 1));
    assert!(!range.contains(end));
    let second = crate::meeting::query::parse_time("2026-10-20T12:00:00Z", NOW, false).unwrap();
    assert_eq!(
        crate::meeting::query::parse_time("2026-10-20T12:00:00.123456Z", NOW, false).unwrap(),
        second + 123
    );
}
