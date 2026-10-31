use super::*;
use crate::meeting::{MeetingSource, MeetingStatus};
use std::fs;
use std::sync::atomic::AtomicUsize;

fn entry(source: MeetingSource, start_ms: u64, text: &str) -> TranscriptEntry {
    TranscriptEntry {
        source,
        start_ms,
        end_ms: start_ms + 5_000,
        text: text.into(),
    }
}

/// One line every 30 s, alternating speakers so each is its own turn: the
/// line at `k * 30 s` is `L{k + 1}` in the boundary pass.
fn every_30s(lines: usize) -> Vec<TranscriptEntry> {
    (0..lines)
        .map(|k| {
            let source = if k % 2 == 0 {
                MeetingSource::System
            } else {
                MeetingSource::Microphone
            };
            entry(source, k as u64 * 30_000, &format!("line number {k} here"))
        })
        .collect()
}

fn window(start_ms: u64, end_ms: u64) -> PassWindow {
    PassWindow {
        text: "[00:00] Them: words".into(),
        skipped: false,
        start_ms,
        end_ms,
    }
}

fn said(title: &str, same_topic: bool) -> GeneratedChapter {
    GeneratedChapter {
        title: title.into(),
        summary: format!("About {}.", title.to_lowercase()),
        same_topic,
    }
}

fn chapter(title: &str, start_ms: u64) -> Chapter {
    Chapter {
        title: title.into(),
        summary: String::new(),
        start_ms,
    }
}

fn titles(state: &Chaptering) -> Vec<(&str, u64)> {
    state
        .chapters
        .iter()
        .map(|chapter| (chapter.title.as_str(), chapter.start_ms))
        .collect()
}

#[test]
fn passes_wait_for_three_minutes_and_enough_words_and_back_off_on_failure() {
    let state = Chaptering::default();
    let mut plan = PassPlan::default();
    assert!(!plan.due(&state, 179_999, 500));
    assert!(!plan.due(&state, 180_000, MIN_NEW_WORDS - 1));
    assert!(plan.due(&state, 180_000, MIN_NEW_WORDS));

    plan.record(false, 180_000);
    assert!(!plan.due(&state, 239_999, 500));
    assert!(plan.due(&state, 240_000, 500));
    plan.record(false, 240_000);
    assert!(!plan.due(&state, 359_999, 500));
    assert!(plan.due(&state, 360_000, 500));
    for _ in 0..10 {
        plan.record(false, 360_000);
    }
    assert!(plan.due(&state, 360_000 + MAX_RETRY_MS, 500));
    plan.record(true, 400_000);
    assert!(plan.due(&state, 180_000, 500));

    let read = Chaptering {
        covered_ms: 180_000,
        ..Chaptering::default()
    };
    assert!(!plan.due(&read, 359_999, 500));
    assert!(plan.due(&read, 360_000, 500));
}

#[test]
fn the_first_name_opens_a_chapter_at_the_start_of_the_meeting() {
    let mut state = Chaptering::default();
    assert!(state.merge(said("Image storage", false), &window(4_000, 180_000)));
    assert_eq!(titles(&state), [("Image storage", 0)]);
    assert_eq!(state.chapters[0].summary, "About image storage.");
    assert_eq!((state.covered_ms, state.passes), (180_000, 1));
}

#[test]
fn a_new_chapter_needs_two_passes_away_and_opens_where_the_first_began() {
    let mut state = Chaptering::default();
    state.merge(said("Image storage", false), &window(0, 180_000));
    assert!(!state.merge(said("Motion lint", false), &window(181_000, 360_000)));
    assert_eq!(state.chapters.len(), 1);
    assert!(state.merge(said("Motion lint rules", false), &window(361_000, 540_000)));
    assert_eq!(
        titles(&state),
        [("Image storage", 0), ("Motion lint", 181_000)]
    );

    // One pass away, then back: the detour is forgotten.
    assert!(!state.merge(said("Chat app name", false), &window(541_000, 720_000)));
    assert!(!state.merge(said("Lint thresholds", true), &window(721_000, 900_000)));
    assert!(!state.merge(said("Pricing", false), &window(901_000, 1_080_000)));
    assert_eq!(state.chapters.len(), 2);
    assert_eq!(state.passes, 6);
}

#[test]
fn a_chapter_lasts_at_least_its_minimum_before_the_next_opens() {
    let mut state = Chaptering::default();
    state.merge(said("Image storage", false), &window(0, 40_000));
    state.merge(said("Motion lint", false), &window(41_000, 90_000));
    // The departure began 41 s in: too soon, so this pass starts over.
    assert!(!state.merge(said("Motion lint", false), &window(91_000, 130_000)));
    assert_eq!(state.chapters.len(), 1);
    assert!(state.merge(said("Motion lint", false), &window(131_000, 170_000)));
    assert_eq!(
        titles(&state),
        [("Image storage", 0), ("Motion lint", 91_000)]
    );
}

#[test]
fn a_name_like_one_already_heard_stays_in_the_chapter_whatever_the_model_says() {
    let mut state = Chaptering::default();
    state.merge(said("Image storage", false), &window(0, 180_000));
    state.merge(
        said("Image storage costs", false),
        &window(181_000, 360_000),
    );
    state.merge(said("Storage for images", false), &window(361_000, 540_000));
    assert_eq!(titles(&state), [("Image storage", 0)]);
    // The model calling an unrelated name the same topic is not enough.
    state.merge(said("Frame detection", true), &window(541_000, 600_000));
    assert_eq!(
        state.pending.as_ref().map(|pending| pending.title.as_str()),
        Some("Frame detection")
    );
    state.merge(said("Storage limits", true), &window(601_000, 660_000));
    assert_eq!(state.pending, None);
    // An empty name decides nothing but still counts as read.
    assert!(!state.merge(said("  ", false), &window(661_000, 720_000)));
    assert_eq!((state.covered_ms, state.passes), (720_000, 6));
}

#[test]
fn a_refused_passage_counts_as_read_and_a_failure_does_not() {
    let mut state = Chaptering::default();
    let outcome = run_pass(&mut state, &window(0, 180_000), &[], |_, _, _| {
        Err(GenerationError::Refused)
    });
    assert_eq!(outcome, Ok(PassOutcome::Refused));
    assert_eq!((state.covered_ms, state.passes), (180_000, 1));

    let outcome = run_pass(&mut state, &window(181_000, 360_000), &[], |_, _, _| {
        Err(GenerationError::Failed("timed out".into()))
    });
    assert!(outcome.is_err());
    assert_eq!((state.covered_ms, state.passes), (180_000, 1));

    let outcome = run_pass(&mut state, &window(181_000, 360_000), &[], |_, _, _| {
        Ok(said("Image storage", false))
    });
    assert_eq!(outcome, Ok(PassOutcome::Changed));
}

#[test]
fn the_prompt_names_the_current_chapter_terms_and_only_new_lines() {
    let terms = ["Rune".to_string(), " ".into(), "Psychopomp".into()];
    let mut seen = String::new();
    let mut state = Chaptering::default();
    state.merge(said("Image storage", false), &window(0, 180_000));
    let skipped = PassWindow {
        skipped: true,
        ..window(181_000, 360_000)
    };
    run_pass(&mut state, &skipped, &terms, |instructions, prompt, _| {
        assert_eq!(instructions, INSTRUCTIONS);
        seen = prompt.into();
        Ok(said("Image storage", true))
    })
    .unwrap();
    assert_eq!(
        seen,
        "Names and terms that may appear: Rune, Psychopomp\n\n\
         New transcript:\n[…]\n[00:00] Them: words\n\nCurrent chapter: Image storage"
    );
    assert!(prompt(&window(0, 1), None, &[]).ends_with("\n\nCurrent chapter: none yet"));
}

#[test]
fn a_confirming_pass_on_yet_another_topic_waits_for_its_own_confirmation() {
    let mut state = Chaptering::default();
    state.merge(said("Image storage", false), &window(0, 180_000));
    state.merge(said("Motion lint", false), &window(181_000, 360_000));
    // Away again, but somewhere else: lint opens, the app name waits.
    assert!(state.merge(said("Chat app name", false), &window(361_000, 540_000)));
    assert_eq!(
        titles(&state),
        [("Image storage", 0), ("Motion lint", 181_000)]
    );
    assert!(state.merge(said("App name ideas", false), &window(541_000, 720_000)));
    assert_eq!(
        titles(&state),
        [
            ("Image storage", 0),
            ("Motion lint", 181_000),
            ("Chat app name", 361_000)
        ]
    );
}

#[test]
fn stop_opens_a_departure_waiting_for_its_second_pass_once_the_chapter_has_lasted() {
    let mut state = Chaptering::default();
    assert!(!state.close());
    state.merge(said("Image storage", false), &window(0, 40_000));
    state.merge(said("Motion lint", false), &window(41_000, 80_000));
    // Too soon after the chapter began: the departure is dropped.
    assert!(!state.close());
    assert_eq!(state.chapters.len(), 1);

    state.merge(said("Motion lint", false), &window(181_000, 360_000));
    assert!(state.close());
    assert_eq!(
        titles(&state),
        [("Image storage", 0), ("Motion lint", 181_000)]
    );
    assert!(!state.close());
}

#[test]
fn a_pass_reads_the_newest_lines_after_the_covered_time() {
    let entries = vec![
        entry(MeetingSource::System, 0, "old line"),
        entry(MeetingSource::Microphone, 200_000, "first new line"),
        entry(MeetingSource::System, 210_000, "second new line"),
    ];
    assert_eq!(fresh_words(&entries, 180_000), 6);
    let window = pass_window(&entries, 180_000, 1_000).unwrap();
    assert_eq!(
        window.text,
        "[03:20] You: first new line\n[03:30] Them: second new line"
    );
    assert_eq!((window.start_ms, window.end_ms), (200_000, 215_000));
    assert!(!window.skipped);

    let tight = pass_window(&entries, 180_000, 20).unwrap();
    assert_eq!(tight.text, "[03:30] Them: second new line");
    assert!(tight.skipped);
    assert_eq!(tight.start_ms, 200_000);
    assert!(pass_window(&entries, 215_000, 1_000).is_none());
}

fn live_three() -> [Chapter; 3] {
    [
        chapter("Image storage", 0),
        Chapter {
            summary: "Snaps in animations.".into(),
            ..chapter("Motion lint", 180_000)
        },
        chapter("Chat app name", 420_000),
    ]
}

/// Runs the boundary pass over `every_30s(21)`, answering each call in
/// turn.
fn refine_with(
    live: &[Chapter],
    timeout: Duration,
    answers: Vec<Result<Vec<i64>, GenerationError>>,
) -> Result<Refinement> {
    let mut answers = answers.into_iter();
    refine(
        &every_30s(21),
        live,
        10_000,
        timeout,
        &[],
        || true,
        |_, _, _| answers.next().expect("one answer per call"),
    )
}

fn starts(refinement: &Refinement) -> Vec<u64> {
    refinement
        .chapters
        .iter()
        .map(|chapter| chapter.start_ms)
        .collect()
}

#[test]
fn the_boundary_pass_asks_once_per_boundary_and_moves_it_to_the_best_split() {
    let live = live_three();
    let mut prompts = Vec::new();
    let mut answers = vec![(13..=21).collect::<Vec<i64>>(), (6..=13).collect()];
    let refinement = refine(
        &every_30s(21),
        &live,
        10_000,
        Duration::from_secs(1),
        &[],
        || true,
        |instructions, text, _| {
            assert_eq!(instructions, REFINE_INSTRUCTIONS);
            prompts.push(text.to_string());
            Ok(answers.pop().unwrap())
        },
    )
    .unwrap();
    assert!(prompts[0].starts_with(
        "Chapter before: Image storage\nChapter after: Motion lint (Snaps in animations.)\n\n\
         Lines:\nL2 [00:30] You: line number 1 here\n"
    ));
    // Section 1 holds lines after the first chapter starts and before the
    // third does, within three minutes of the live boundary.
    assert!(!prompts[0].contains("L1 [00:00]"));
    assert!(prompts[0].ends_with("L13 [06:00] Them: line number 12 here\n"));
    assert!(prompts[1].starts_with(
        "Chapter before: Motion lint (Snaps in animations.)\nChapter after: Chat app name\n\n\
         Lines:\nL9 [04:00] Them: line number 8 here\n"
    ));
    assert_eq!(starts(&refinement), [0, 150_000, 360_000]);
    assert_eq!((refinement.moved, refinement.calls), (2, 2));
    assert_eq!(
        refinement.prompt_chars,
        prompts.iter().map(String::len).sum::<usize>()
    );
}

#[test]
fn the_boundary_pass_keeps_a_boundary_the_answer_does_not_support() {
    let live = live_three();
    // Lines outside the section, or no lines at all.
    let untouched = refine_with(
        &live,
        Duration::from_secs(1),
        vec![Ok(vec![1, -3]), Ok(vec![99])],
    )
    .unwrap();
    assert_eq!(untouched.chapters, live);
    assert_eq!((untouched.moved, untouched.calls), (0, 2));
    // A lone line far from the live boundary disagrees with the live
    // passes, which heard the chapter after from 03:00 on.
    let lone = refine_with(
        &live,
        Duration::from_secs(1),
        vec![Ok(vec![12]), Ok(vec![])],
    )
    .unwrap();
    assert_eq!(lone.chapters, live);
    // Section 2 may not start before the boundary section 1 moved to.
    let ordered = refine_with(
        &live,
        Duration::from_secs(1),
        vec![Ok((10..=13).collect()), Ok((9..=21).collect())],
    )
    .unwrap();
    assert_eq!(starts(&ordered), [0, 270_000, 300_000]);
    assert_eq!(ordered.moved, 2);
}

#[test]
fn the_boundary_pass_fails_only_when_every_call_does() {
    let live = live_three();
    let refused = || Err(GenerationError::Refused);
    assert!(refine_with(&live, Duration::from_secs(1), vec![refused(), refused()]).is_err());
    let partly = refine_with(
        &live,
        Duration::from_secs(1),
        vec![refused(), Ok((13..=21).collect())],
    )
    .unwrap();
    assert_eq!(starts(&partly), [0, 180_000, 360_000]);
    assert_eq!((partly.moved, partly.calls), (1, 2));
    // Past the deadline, or with one chapter, nothing is asked.
    let late = refine_with(&live, Duration::ZERO, Vec::new()).unwrap();
    assert_eq!((late.chapters.as_slice(), late.calls), (live.as_slice(), 0));
    let single = refine_with(&live[..1], Duration::from_secs(1), Vec::new()).unwrap();
    assert_eq!((single.moved, single.calls, single.prompt_chars), (0, 0, 0));
}

#[test]
fn a_stopped_boundary_pass_records_nothing_and_runs_again() {
    let directory = std::env::temp_dir().join(format!(
        "hex-refine-stop-{}-{}",
        std::process::id(),
        crate::desktop::events::now_ms()
    ));
    fs::create_dir_all(&directory).unwrap();
    let export = directory.join("Team sync.md");
    fs::write(
        &export,
        "# Team sync: Image storage, Motion lint, Chat app name\n",
    )
    .unwrap();
    let fixture = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures/meetings-chapters/1790003000000-1005/manifest.json");
    let mut manifest: MeetingManifest =
        serde_json::from_slice(&fs::read(fixture).unwrap()).unwrap();
    manifest.chapters = live_three().to_vec();
    manifest.chapters_refined = false;
    manifest.duration_ms = Some(630_000);
    manifest.export_path = Some(export.clone());
    write_manifest(&directory, &manifest).unwrap();
    let stored = || fs::read(directory.join("manifest.json")).unwrap();
    let before = stored();
    let mut answers = vec![(13..=21).collect::<Vec<i64>>(), (6..=13).collect()];

    // Dictation or a quit stops the pass before its second call.
    let mut checks = 0;
    let mut calls = 0;
    refine_meeting_with(
        &directory,
        &every_30s(21),
        10_000,
        || {
            checks += 1;
            checks == 1
        },
        |_, _, _| {
            calls += 1;
            Ok(answers.pop().unwrap())
        },
    );
    assert_eq!((checks, calls), (2, 1));
    assert_eq!(stored(), before, "a stopped pass leaves the manifest alone");
    assert_eq!(
        fs::read_to_string(&export).unwrap(),
        "# Team sync: Image storage, Motion lint, Chat app name\n"
    );

    // The next run redoes it and records it once.
    let mut answers = vec![(13..=21).collect::<Vec<i64>>(), (6..=13).collect()];
    refine_meeting_with(
        &directory,
        &every_30s(21),
        10_000,
        || true,
        |_, _, _| Ok(answers.pop().unwrap()),
    );
    let refined = read_manifest(&directory).unwrap();
    assert!(refined.chapters_refined);
    assert_eq!(
        refined
            .chapters
            .iter()
            .map(|chapter| chapter.start_ms)
            .collect::<Vec<_>>(),
        [0, 150_000, 360_000]
    );
    fs::remove_dir_all(directory).unwrap();
}

#[test]
fn a_title_adds_the_longest_chapters_in_meeting_order() {
    let chapters = [
        chapter("Image storage", 0),
        chapter("Hello", 300_000),
        chapter("Motion lint", 320_000),
        chapter("Chat app name", 900_000),
        chapter("Team sync", 1_200_000),
    ];
    assert_eq!(
        titled("Team sync", &chapters, Some(1_800_000)),
        "Team sync: Image storage, Motion lint, Chat app name"
    );
    assert_eq!(
        titled(" Team sync ", &chapters[..1], Some(60_000)),
        "Team sync"
    );
    // Only chapters repeating the base: nothing to add.
    assert_eq!(
        titled(
            "Team sync",
            &[chapter("Team syncs", 0), chapter("Sync team", 200_000)],
            None
        ),
        "Team sync"
    );
    let long = [
        chapter("A very long chapter name about storage buckets", 0),
        chapter("Another very long chapter name about linting", 600_000),
        chapter("Yet another long chapter name about naming", 1_200_000),
    ];
    let title = titled("Team sync", &long, Some(1_800_000));
    assert!(title.len() <= MAX_TITLE_CHARS, "{title}");
    assert_eq!(
        title,
        "Team sync: A very long chapter name about storage buckets"
    );
    // The cap holds for the first name too; a shorter one may still fit.
    let first_too_long = [
        chapter(&"Storage ".repeat(12), 0),
        chapter("Motion lint", 900_000),
    ];
    assert_eq!(
        titled("Team sync", &first_too_long, Some(1_200_000)),
        "Team sync: Motion lint"
    );
}

#[test]
fn a_recording_title_counts_the_last_chapter_until_now() {
    let fixture = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures/meetings-chapters/1790003000000-1005/manifest.json");
    let mut manifest: MeetingManifest =
        serde_json::from_slice(&fs::read(fixture).unwrap()).unwrap();
    // The last chapter has been recording for 20 min, the longest of all.
    manifest.chapters = vec![
        chapter("Image storage", 0),
        chapter("Motion lint", 100_000),
        chapter("Chat app name", 240_000),
        chapter("Launch plan", 660_000),
    ];
    manifest.status = MeetingStatus::Recording;
    manifest.duration_ms = None;
    manifest.created_at_ms = crate::desktop::events::now_ms() - 1_860_000;
    assert_eq!(
        display_title(&manifest),
        "Team sync: Motion lint, Chat app name, Launch plan"
    );
    // Stopped without a duration, the last chapter counts as empty.
    manifest.status = MeetingStatus::Complete;
    assert_eq!(
        display_title(&manifest),
        "Team sync: Image storage, Motion lint, Chat app name"
    );
}

#[test]
fn a_section_holds_its_chapters_turns_as_markdown() {
    let entries = vec![
        entry(MeetingSource::System, 0, "Where do images live?"),
        entry(MeetingSource::Microphone, 60_000, "In the bucket."),
        entry(MeetingSource::System, 200_000, "Now the lint rules."),
        entry(MeetingSource::Microphone, 230_000, "They flag snaps."),
    ];
    let chapters = [
        Chapter {
            title: "Image storage".into(),
            summary: "Where images live.".into(),
            start_ms: 0,
        },
        chapter("Motion lint", 200_000),
    ];
    assert_eq!(chapter_entries(&entries, &chapters, 0).len(), 2);
    assert_eq!(chapter_entries(&entries, &chapters, 1).len(), 2);
    assert!(chapter_entries(&entries, &chapters, 2).is_empty());
    assert_eq!(
        section_markdown(&chapters[0], &chapter_entries(&entries, &chapters, 0)),
        "## Image storage\n\nWhere images live.\n\n**Them** · 00:00\nWhere do images live?\n\n**You** · 01:00\nIn the bucket."
    );
    assert_eq!(
        section_markdown(&chapters[1], &chapter_entries(&entries, &chapters, 1)),
        "## Motion lint\n\n**Them** · 03:20\nNow the lint rules.\n\n**You** · 03:50\nThey flag snaps."
    );
}

#[test]
fn the_vocabulary_line_lists_trimmed_terms() {
    assert_eq!(vocabulary_line(&[]), "");
    assert_eq!(vocabulary_line(&[" ".into()]), "");
    assert_eq!(
        vocabulary_line(&[" Rune ".into(), "Hex\n\tMicrophone".into()]),
        "Names and terms that may appear: Rune, Hex Microphone\n\n"
    );
}

fn unreachable_pass(_: &str, _: &str, _: Duration) -> Result<GeneratedChapter, GenerationError> {
    panic!("no pass without Apple Intelligence")
}

fn always() -> bool {
    true
}

fn never() -> bool {
    false
}

/// A transcript that grows by a minute per read: storage talk for the
/// first six minutes, then lint talk.
fn growing_transcript() -> (ReadTranscript, Arc<AtomicUsize>) {
    let reads = Arc::new(AtomicUsize::new(0));
    let counter = reads.clone();
    let read: ReadTranscript = Box::new(move || {
        let minutes = counter.fetch_add(1, Ordering::Relaxed) as u64 + 1;
        Ok((0..minutes.min(20) * 6)
            .map(|k| {
                let start_ms = k * 10_000;
                let topic = if start_ms < 360_000 {
                    "storage"
                } else {
                    "lint"
                };
                entry(
                    MeetingSource::System,
                    start_ms,
                    &format!("we keep talking about the {topic} plan in some detail here"),
                )
            })
            .collect())
    });
    (read, reads)
}

fn by_topic(_: &str, prompt: &str, _: Duration) -> Result<GeneratedChapter, GenerationError> {
    let new_lines = prompt.split("New transcript:").nth(1).unwrap_or_default();
    let current = prompt
        .lines()
        .find_map(|line| line.strip_prefix("Current chapter: "))
        .unwrap_or_default();
    let title = if new_lines.contains("lint") {
        "Motion lint"
    } else {
        "Image storage"
    };
    Ok(GeneratedChapter {
        title: title.into(),
        summary: String::new(),
        same_topic: current == title,
    })
}

#[test]
fn the_worker_reports_each_new_set_of_chapters() {
    let (read, _) = growing_transcript();
    let live = LiveChapters::start_with(
        "test".into(),
        read,
        Duration::from_millis(1),
        always,
        Box::new(by_topic),
    );
    let started = Instant::now();
    let mut latest = Vec::new();
    while latest.len() < 2 && started.elapsed() < Duration::from_secs(10) {
        if let Some(chapters) = live.take() {
            latest = chapters;
        }
        thread::sleep(Duration::from_millis(5));
    }
    // Live, the chapter starts with the first pass away from storage, which
    // also read storage talk; the boundary pass after Stop corrects that.
    assert_eq!(
        latest
            .iter()
            .map(|chapter| (chapter.title.as_str(), chapter.start_ms))
            .collect::<Vec<_>>(),
        [("Image storage", 0), ("Motion lint", 240_000)]
    );
}

#[test]
fn the_worker_reads_nothing_while_chapters_are_unwanted() {
    let (read, reads) = growing_transcript();
    let live = LiveChapters::start_with(
        "test".into(),
        read,
        Duration::from_millis(1),
        never,
        Box::new(unreachable_pass),
    );
    thread::sleep(Duration::from_millis(50));
    assert_eq!(live.take(), None);
    assert_eq!(live.finish(), None);
    assert_eq!(reads.load(Ordering::Relaxed), 0);
}

#[test]
fn finishing_hands_over_chapters_not_yet_taken_and_the_departure_stop_opened() {
    let (read, _) = growing_transcript();
    let live = LiveChapters::start_with(
        "test".into(),
        read,
        Duration::from_millis(1),
        never,
        Box::new(unreachable_pass),
    );
    {
        let mut shared = live.shared();
        shared
            .state
            .merge(said("Image storage", false), &window(0, 180_000));
        shared.fresh = true;
    }
    assert_eq!(live.take(), Some(vec![chapter_with("Image storage", 0)]));
    assert_eq!(live.take(), None);
    live.shared()
        .state
        .merge(said("Motion lint", false), &window(181_000, 360_000));
    assert_eq!(
        live.finish(),
        Some(vec![
            chapter_with("Image storage", 0),
            chapter_with("Motion lint", 181_000)
        ])
    );
    assert_eq!(live.finish(), None);
}

fn chapter_with(title: &str, start_ms: u64) -> Chapter {
    Chapter {
        summary: format!("About {}.", title.to_lowercase()),
        ..chapter(title, start_ms)
    }
}
