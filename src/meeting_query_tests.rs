use std::fs;
use std::path::{Path, PathBuf};
use std::time::SystemTime;

use super::*;

const NOW: u64 = 1_790_100_000_000;
const WEEKLY: &str = "1790000000000-1001";
const ROADMAP: &str = "1790000500000-1002";
const STANDUP: &str = "1790001000000-1003";

fn fixtures() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/meetings")
}

fn library() -> Library {
    Library::new(fixtures())
}

fn list(library: &Library, include_in_progress: bool) -> String {
    library
        .list(
            &ListQuery {
                since: None,
                until: None,
                query: None,
                limit: 20,
                include_in_progress,
            },
            NOW,
        )
        .unwrap()
}

fn search(query: &str, fuzzy: bool) -> String {
    library()
        .search(
            &SearchQuery {
                query: query.into(),
                since: None,
                until: None,
                meeting_id: None,
                limit: 10,
                fuzzy,
                include_in_progress: true,
                vocabulary: Vec::new(),
            },
            NOW,
        )
        .unwrap()
}

fn transcript(id: &str, configure: impl FnOnce(&mut TranscriptQuery)) -> String {
    let mut query = TranscriptQuery {
        meeting_id: id.into(),
        start: None,
        end: None,
        speaker: None,
        max_chars: 20_000,
        cursor: None,
    };
    configure(&mut query);
    library().transcript(&query, NOW).unwrap()
}

fn hit_lines(output: &str) -> Vec<&str> {
    output
        .lines()
        .filter(|line| line.starts_with('['))
        .collect()
}

#[test]
fn lists_meetings_skipping_unreadable_folders_and_labeling_unfinished_ones() {
    let output = list(&library(), true);
    let lines = output.lines().collect::<Vec<_>>();
    assert!(lines[0].starts_with("3 of 3 meetings"), "{output}");
    // Newest first.
    assert!(lines[1].starts_with(&format!("- {STANDUP}")), "{output}");
    assert!(lines[1].contains("interrupted · rolling transcript"));
    let weekly = lines.iter().find(|line| line.contains(WEEKLY)).unwrap();
    assert!(weekly.contains("\"Weekly sync\""));
    assert!(weekly.contains("· 00:40 · complete · final transcript"));
    assert!(weekly.contains("participants: You, Them (Zoom)"));
    assert!(weekly.contains("notes: yes"));
    assert!(output.contains("warning: skipped 1 unreadable lines in transcript.ndjson"));
    assert!(output.contains(
        "Skipped 1 unreadable folders:\n- 1790001500000-1004: could not decode manifest.json"
    ));
}

#[test]
fn list_filters_by_title_and_time() {
    let library = library();
    let query = |since: Option<&str>, until: Option<&str>, title: Option<&str>| {
        library
            .list(
                &ListQuery {
                    since: since.map(Into::into),
                    until: until.map(Into::into),
                    query: title.map(Into::into),
                    limit: 20,
                    include_in_progress: true,
                },
                NOW,
            )
            .unwrap()
    };
    let roadmap = query(None, None, Some("ROADMAP"));
    assert!(
        roadmap.starts_with("1 of 1 meetings") && roadmap.contains(ROADMAP),
        "{roadmap}"
    );
    let after = query(Some("1790000500000"), None, None);
    assert!(
        !after.contains(WEEKLY) && after.contains(ROADMAP),
        "{after}"
    );
    let before = query(None, Some("2026-09-21T00:00:00Z"), None);
    assert!(before.starts_with("No meetings match"), "{before}");
}

#[test]
fn search_ranks_names_and_tolerates_recognition_misspellings() {
    let output = search("Dax", true);
    let hits = hit_lines(&output);
    assert!(hits[0].contains(WEEKLY), "{output}");
    assert!(
        output.contains("hit: Sure. **Dax**, can you walk"),
        "{output}"
    );
    assert!(
        hits.iter().any(|line| line.contains("matched dack~dax")),
        "Dacks should match Dax: {output}"
    );
    let exact_rank = hits
        .iter()
        .position(|line| line.contains("matched dax"))
        .unwrap();
    let fuzzy_rank = hits
        .iter()
        .position(|line| line.contains("dack~dax"))
        .unwrap();
    assert!(exact_rank < fuzzy_rank, "{output}");

    let exact = search("Dacks", false);
    assert_eq!(hit_lines(&exact).len(), 1, "{exact}");
    assert!(exact.contains("**Dacks** thinks"));
}

/// A storage discussion at 02:00–02:40 amid chatter full of `made`, `blog`,
/// `Bob`, and `believe`, which are one edit or a Soundex code away from
/// `media` and `blob`. `blob` is said in three passages (the third at 03:30),
/// so it is no longer rare.
fn ranked(query: &str) -> String {
    Library::new(Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/meetings-ranking"))
        .search(
            &SearchQuery {
                query: query.into(),
                since: None,
                until: None,
                meeting_id: None,
                limit: 10,
                fuzzy: true,
                include_in_progress: true,
                vocabulary: Vec::new(),
            },
            NOW,
        )
        .unwrap()
}

fn offset(hit: &str) -> &str {
    hit.split(" · ")
        .nth(1)
        .and_then(|when| when.split(' ').next())
        .unwrap()
}

#[test]
fn exact_stems_outrank_loose_matches_to_common_words() {
    let output = ranked("images database blob storage media");
    let hits = hit_lines(&output);
    let mut top = hits
        .iter()
        .take(5)
        .map(|hit| offset(hit))
        .collect::<Vec<_>>();
    top.sort_unstable();
    assert_eq!(
        top,
        ["02:00", "02:10", "02:20", "02:30", "02:40"],
        "{output}"
    );
    // `made` is said throughout, so it never stands in for `media`.
    assert!(!output.contains("mad~media"), "{output}");
    // `blob` is spelled that way in three passages, so it no longer reaches
    // `blog`, `Bob`, or `believe`.
    assert!(!output.contains("~blob"), "{output}");
    let blob = ranked("blob");
    let blobs = hit_lines(&blob);
    assert_eq!(
        blobs.iter().map(|hit| offset(hit)).collect::<Vec<_>>(),
        ["02:00", "02:10", "03:30"],
        "{blob}"
    );
    assert!(
        blobs.iter().all(|hit| hit.ends_with("matched blob")),
        "{blob}"
    );
}

#[test]
fn a_term_found_in_fewer_passages_still_reaches_its_neighbours() {
    // The fixture's `blob` story with only two exact passages.
    let texts = [
        "keep the images in blob storage",
        "the blob store sits behind a CDN",
        "I wrote a blog post",
        "the onboarding video is done",
    ];
    let documents = texts.map(|text| vec![text]);
    let matched = |documents: &[Vec<&str>]| {
        search_texts(documents, "blob", &[])
            .unwrap()
            .into_iter()
            .map(|hit| hit.index)
            .collect::<Vec<_>>()
    };
    assert_eq!(matched(&documents), [0, 1, 2]);
    let mut common = documents.to_vec();
    common.push(vec!["one more blob"]);
    assert_eq!(matched(&common), [4, 0, 1]);
}

#[test]
fn an_all_exact_passage_outranks_a_higher_scoring_partial_one() {
    // `storage` is rare and `blob` common, so a short passage repeating
    // `storage` alone outscores a long one that has both words.
    let mut texts = vec![
        "blob first, then a long tangent about the quarter, the hiring plan, the offsite, the \
         budget review, and the roadmap before we finally got back to storage",
        "storage storage storage storage",
    ];
    texts.extend(["the blob again"; 8]);
    let documents = texts.iter().map(|text| vec![*text]).collect::<Vec<_>>();
    let hits = search_texts(&documents, "blob storage", &[]).unwrap();
    assert_eq!(hits[0].index, 0);
    let partial = hits.iter().find(|hit| hit.index == 1).unwrap();
    assert!(partial.score > hits[0].score, "{hits:?}");
}

#[test]
fn passages_matching_every_term_exactly_rank_first() {
    let output = ranked("blob storage");
    let hits = hit_lines(&output);
    assert!(hits.len() > 3, "{output}");
    let exact = hits
        .iter()
        .take_while(|hit| hit.ends_with("matched blob, storag"))
        .count();
    assert_eq!(exact, 2, "{output}");
    assert!(
        hits[exact..]
            .iter()
            .all(|hit| !hit.ends_with("matched blob, storag")),
        "{output}"
    );
}

#[test]
fn a_misspelled_rare_word_still_finds_its_passage() {
    let output = ranked("thumbnial");
    let hits = hit_lines(&output);
    assert_eq!(hits.len(), 1, "{output}");
    assert_eq!(offset(hits[0]), "02:30", "{output}");
    assert!(hits[0].ends_with("matched thumbnail~thumbnial"), "{output}");
}

#[test]
fn spelling_fallbacks_need_rare_terms_and_uncommon_candidates() {
    let common = 5.0;
    assert!(admits(MatchKind::Edit(1), 2, 4, common));
    assert!(admits(MatchKind::Edit(2), 2, 4, common));
    // The term is spelled this way often enough; neighbours are other words.
    assert!(!admits(MatchKind::Edit(1), RARE_EXACT_PASSAGES, 1, common));
    assert!(!admits(MatchKind::Sound, RARE_EXACT_PASSAGES, 1, common));
    // `made` for `media`.
    assert!(!admits(MatchKind::Sound, 1, 9, common));
    assert!(!admits(MatchKind::Edit(1), 1, 9, common));
    // A plain typo of a common word still reaches it, but only by one edit.
    assert!(admits(MatchKind::Edit(1), 0, 9, common));
    assert!(!admits(MatchKind::Edit(2), 0, 9, common));
    assert!(!admits(MatchKind::Sound, 0, 9, common));
    assert!(admits(MatchKind::Prefix, 50, 50, common));
}

#[test]
fn search_stems_folds_accents_and_requires_quoted_phrases() {
    let schedule = search("schedule", true);
    assert!(
        schedule.contains(WEEKLY) && schedule.contains(ROADMAP),
        "{schedule}"
    );
    assert!(
        schedule.contains("**schedule**, like a cron job"),
        "{schedule}"
    );

    let cafe = search("cafe", true);
    assert!(cafe.contains("the **Café** dashboard"), "{cafe}");

    let phrase = search("\"scheduled sessions\"", true);
    assert!(!phrase.contains(ROADMAP), "{phrase}");
    let hits = hit_lines(&phrase);
    assert!(hits.iter().any(|line| line.contains("Notes")), "{phrase}");
    assert!(
        phrase.contains("the **scheduled** **sessions** idea"),
        "{phrase}"
    );
    assert!(hits.iter().all(|line| line.contains(WEEKLY)), "{phrase}");

    let missing = search("\"sessions scheduled\"", true);
    assert!(missing.starts_with("0 matching passages"), "{missing}");
}

#[test]
fn search_reports_context_and_labels_unfinished_meetings() {
    let output = search("cron", true);
    assert!(
        output.contains(
            "before (You 00:04): Sure. Dax, can you walk us through the scheduled sessions idea?"
        ),
        "{output}"
    );
    assert!(
        output.contains("interrupted (rolling transcript)"),
        "{output}"
    );
    assert!(
        output.contains("Rolling text mentions **cron** again."),
        "{output}"
    );
}

#[test]
fn transcript_merges_turns_and_continues_from_a_cursor() {
    let full = transcript(WEEKLY, |_| {});
    assert!(full.contains(
        "] Them: Yeah, so the plan is that agents can run on a schedule, like a cron job, \
without anyone watching. Recurring sessions would wake up in the background and report back."
    ));
    assert!(full.ends_with("(end of range)\n"), "{full}");

    let first = transcript(WEEKLY, |query| query.max_chars = 500);
    assert!(!first.contains("Great, I'll write the spec."));
    let cursor = first
        .lines()
        .find_map(|line| line.strip_prefix("next_cursor: "))
        .and_then(|rest| rest.split_whitespace().next())
        .unwrap()
        .to_string();
    let second = transcript(WEEKLY, |query| {
        query.max_chars = 500;
        query.cursor = Some(cursor);
    });
    assert!(second.contains("Great, I'll write the spec."), "{second}");
    assert!(!second.contains("thanks for joining"), "{second}");

    let you = transcript(WEEKLY, |query| query.speaker = Some("you".into()));
    assert!(
        !you.contains("Them:") && you.contains("You: Sure. Dax"),
        "{you}"
    );
    let window = transcript(WEEKLY, |query| {
        query.start = Some("0:20".into());
        query.end = Some("30".into());
    });
    assert!(window.contains("Showing 00:16–00:31"), "{window}");
    assert!(!window.contains("thanks for joining") && !window.contains("write the spec"));
}

#[test]
fn ranges_keep_overlapping_entries_and_reject_inverted_or_late_ones() {
    let query = |start: &str, end: &str| {
        library().transcript(
            &TranscriptQuery {
                meeting_id: WEEKLY.into(),
                start: Some(start.into()),
                end: Some(end.into()),
                speaker: None,
                max_chars: 20_000,
                cursor: None,
            },
            NOW,
        )
    };
    // 00:16.5–00:22 overlaps 00:20 and is kept whole.
    let window = query("0:00:20", "30").unwrap();
    assert!(window.contains("Showing 00:16–00:31"), "{window}");
    let inverted = query("0:30", "0:20").unwrap_err().to_string();
    assert_eq!(
        inverted,
        "The range start 00:30 must be earlier than its end 00:20."
    );
    let late = query("0:41", "").unwrap_err().to_string();
    assert_eq!(
        late,
        "The range starts at 00:41, after the meeting's end at 00:40."
    );
    assert!(query("soon", "").is_err());

    // A rolling draft's Markdown, limited to the range.
    let markdown = |from: Option<&str>, to: Option<&str>| {
        library().markdown(STANDUP, OffsetRange::parse(from, to).unwrap(), NOW)
    };
    let draft = markdown(Some("0:04"), Some("9")).unwrap();
    assert!(draft.contains("- Range: 00:04–00:09\n"), "{draft}");
    assert!(
        draft.contains("Rolling text mentions cron again.") && draft.contains("I agree with that."),
        "{draft}"
    );
    assert!(!draft.contains("background worker"), "{draft}");
    assert!(
        markdown(None, Some("0:03"))
            .unwrap()
            .contains("- Range: 00:00–00:03\n")
    );
    let late = markdown(Some("0:10"), None).unwrap_err().to_string();
    assert!(late.contains("after the meeting's end at 00:10"), "{late}");
}

#[test]
fn transcript_of_an_unfinished_meeting_is_labeled_as_a_draft() {
    let output = transcript(STANDUP, |_| {});
    assert!(output.contains("No final transcript was published; this is the unfinished draft."));
    assert!(
        output.contains("Them: Rolling text about the background worker."),
        "{output}"
    );
    assert!(output.contains("You: I agree with that."), "{output}");
}

#[test]
fn notes_are_read_and_their_absence_is_explicit() {
    // An older manifest with action items and decisions reads as title and summary.
    let notes = library().notes(WEEKLY).unwrap();
    assert!(notes.contains("Title: Scheduled Sessions Plan"), "{notes}");
    assert!(notes.contains("Summary: Dax proposed scheduled sessions for agents."));
    assert!(!notes.contains("Prototype the runner") && !notes.contains("Decisions"));
    assert!(notes.contains("Read the transcript with get_transcript"));

    let missing = library().notes(ROADMAP).unwrap();
    assert!(missing.contains("No Apple Intelligence notes are stored for this meeting."));
    assert!(missing.contains("Last notes attempt failed: Apple Intelligence is not available"));
    assert!(missing.contains("Use get_transcript"));

    let unknown = library().notes("nope").unwrap_err().to_string();
    assert!(unknown.contains("list_meetings"), "{unknown}");
    // Unique prefixes resolve; path-like ids never escape the folder.
    assert!(library().notes("1790000500").is_ok());
    assert!(library().notes("../meetings").is_err());
}

#[test]
fn a_held_lock_marks_the_meeting_in_progress() {
    let root = std::env::temp_dir().join(format!(
        "hex-meeting-query-{}-{}",
        std::process::id(),
        crate::events::now_ms()
    ));
    let directory = root.join(STANDUP);
    fs::create_dir_all(&directory).unwrap();
    for entry in fs::read_dir(fixtures().join(STANDUP)).unwrap() {
        let path = entry.unwrap().path();
        fs::copy(&path, directory.join(path.file_name().unwrap())).unwrap();
    }
    let lock = File::create(directory.join("active.lock")).unwrap();
    let library = Library::new(&root);
    assert!(list(&library, true).contains("interrupted"));

    fs2::FileExt::lock_exclusive(&lock).unwrap();
    let output = list(&library, true);
    assert!(
        output.contains("recording (in progress) · rolling transcript"),
        "{output}"
    );
    assert!(list(&library, false).starts_with("No meetings match"));
    let notes = library.notes(STANDUP).unwrap();
    assert!(
        notes.contains("No notes yet: the meeting is still recording."),
        "{notes}"
    );
    // A recording in progress runs until now, past its rolling transcript.
    let ahead = library
        .markdown(
            STANDUP,
            OffsetRange::parse(Some("0:11"), Some("0:20")).unwrap(),
            NOW,
        )
        .unwrap();
    assert!(
        ahead.contains("## Transcript\n\nNo transcript entries in this range."),
        "{ahead}"
    );
    let rolling = library
        .markdown(
            STANDUP,
            OffsetRange::parse(Some("0:05"), None).unwrap(),
            NOW,
        )
        .unwrap();
    assert!(
        rolling.contains("Rolling text mentions cron again."),
        "{rolling}"
    );
    let late = library
        .markdown(
            STANDUP,
            OffsetRange::parse(Some("30:00:00"), None).unwrap(),
            NOW,
        )
        .unwrap_err()
        .to_string();
    assert_eq!(
        late,
        format!(
            "The range starts at 1800:00, after the meeting's end at {} so far.",
            format_duration(NOW - 1_790_001_000_000)
        )
    );

    fs2::FileExt::unlock(&lock).unwrap();
    assert!(list(&library, true).contains("interrupted"));
    fs::remove_dir_all(root).unwrap();
}

fn snapshot(root: &Path) -> Vec<(PathBuf, u64, SystemTime)> {
    let mut files = Vec::new();
    let mut pending = vec![root.to_path_buf()];
    while let Some(directory) = pending.pop() {
        let metadata = fs::metadata(&directory).unwrap();
        files.push((
            directory.clone(),
            metadata.len(),
            metadata.modified().unwrap(),
        ));
        for entry in fs::read_dir(&directory).unwrap() {
            let path = entry.unwrap().path();
            if path.is_dir() {
                pending.push(path);
            } else {
                let metadata = fs::metadata(&path).unwrap();
                files.push((path, metadata.len(), metadata.modified().unwrap()));
            }
        }
    }
    files.sort();
    files
}

#[test]
fn queries_never_write_to_the_meetings_folder() {
    let before = snapshot(&fixtures());
    let library = library();
    list(&library, true);
    search("scheduled sessions cron", true);
    for id in [WEEKLY, ROADMAP, STANDUP] {
        transcript(id, |_| {});
        library.notes(id).unwrap();
        library.markdown(id, OffsetRange::default(), NOW).unwrap();
    }
    assert_eq!(before, snapshot(&fixtures()));
}

#[test]
fn markdown_has_metadata_notes_and_turns() {
    let markdown = library()
        .markdown(WEEKLY, OffsetRange::default(), NOW)
        .unwrap();
    assert!(markdown.starts_with("# Weekly sync\n\n- Meeting: 1790000000000-1001\n"));
    assert!(markdown.contains("- App: Zoom"));
    assert!(markdown.contains("## Summary"));
    assert!(markdown.contains("## Transcript\n\n**Them** · 00:00\nHey Kit, thanks for joining."));
}

#[test]
fn parses_times_offsets_and_words() {
    assert_eq!(
        parse_time("2026-10-15T21:58:31Z", NOW, false).unwrap(),
        1_792_101_511_000
    );
    assert_eq!(
        parse_time("2026-10-15T14:58:31-07:00", NOW, false).unwrap(),
        1_792_101_511_000
    );
    assert_eq!(
        parse_time("1792101511335", NOW, false).unwrap(),
        1_792_101_511_335
    );
    assert_eq!(parse_time("7d", NOW, false).unwrap(), NOW - 7 * 86_400_000);
    let day = parse_time("2026-10-15", NOW, false).unwrap();
    assert_eq!(
        parse_time("2026-10-15", NOW, true).unwrap() - day,
        86_400_000
    );
    assert_eq!(
        parse_time("yesterday", NOW, true).unwrap(),
        parse_time("today", NOW, false).unwrap()
    );
    assert!(parse_time("last tuesday", NOW, false).is_err());

    assert_eq!(parse_offset("12:34").unwrap(), 754_000);
    assert_eq!(parse_offset("1:02:03").unwrap(), 3_723_000);
    assert_eq!(parse_offset("90").unwrap(), 90_000);
    assert_eq!(parse_offset("1.5m").unwrap(), 90_000);
    assert_eq!(parse_offset("1.14").unwrap(), 1_140);
    assert_eq!(parse_offset("113:03").unwrap(), 6_783_000);
    assert_eq!(parse_offset("0:59.5").unwrap(), 59_500);
    for invalid in [
        "soon", "1:-30", "-1:30", "-5", "0:99", "1:99:00", "0:inf", "inf", "NaN", "1.5:00",
        "1:2.5:00", "0:60", "1:60:00", ":30", "1:2:3:4", "1e300h",
    ] {
        assert!(parse_offset(invalid).is_err(), "{invalid}");
    }

    for (word, expected) in [
        ("scheduled", "schedul"),
        ("schedule", "schedul"),
        ("scheduling", "schedul"),
        ("sessions", "session"),
        ("recurring", "recur"),
        ("recur", "recur"),
        ("automated", "automat"),
        ("dax", "dax"),
        ("class", "class"),
    ] {
        assert_eq!(stem(word), expected, "{word}");
    }
    let vocabulary = [
        "schedul", "settl", "steal", "still", "dack", "deck", "docker",
    ]
    .map(String::from);
    let matches = |term: &str| {
        expand(term, vocabulary.iter(), true)
            .into_iter()
            .map(|(stem, _)| stem)
            .collect::<Vec<_>>()
    };
    // Sound-alikes apply to short, name-like words only.
    assert_eq!(matches("schedul"), ["schedul"]);
    assert_eq!(matches("dax"), ["dack", "deck"]);
    assert_eq!(soundex("dax"), soundex("dack"));
    assert_ne!(soundex("dax"), soundex("max"));
    assert_eq!(fold("Café DAX"), "cafe dax");
    assert_eq!(edit_distance("cron", "corn", 1), Some(1));
    assert_eq!(edit_distance("cron", "crane", 1), None);
}

fn entry(term: &str, sounds_like: &[&str]) -> VocabularyEntry {
    VocabularyEntry {
        term: term.into(),
        sounds_like: sounds_like.iter().map(|&text| text.into()).collect(),
    }
}

fn matched(texts: &[&str], query: &str, vocabulary: &[VocabularyEntry]) -> Vec<usize> {
    let documents = texts.iter().map(|text| vec![*text]).collect::<Vec<_>>();
    search_texts(&documents, query, vocabulary)
        .unwrap()
        .into_iter()
        .map(|hit| hit.index)
        .collect()
}

#[test]
fn a_vocabulary_term_finds_its_mis_transcriptions_below_exact_hits() {
    let texts = [
        "the abattoir guides cover release notes and a long list of other things",
        "avatar avatar avatar",
        "lunch was great",
        "abattoir has the skills",
    ];
    let vocabulary = [entry("abattoir", &[])];
    // Without the vocabulary, `avatar` is too far from `abattoir` to match.
    assert_eq!(matched(&texts, "abattoir", &[]), [3, 0]);
    // With it, `avatar` sounds alike, yet ranks after every exact hit even
    // though its repeated word scores higher.
    assert_eq!(matched(&texts, "abattoirs", &vocabulary), [3, 0, 1]);
    let documents = texts.map(|text| vec![text]);
    let hits = search_texts(&documents, "abattoir", &vocabulary).unwrap();
    assert_eq!(hits[2].snippet, "**avatar** **avatar** **avatar**");
}

#[test]
fn sounds_like_spellings_and_split_words_alias_a_term() {
    let texts = [
        "the open coat release went out",
        "opencode beta shipped",
        "we should open code review to everyone",
        "the coat is open",
    ];
    let vocabulary = [entry("OpenCode", &["open coat"])];
    assert_eq!(matched(&texts, "opencode", &[]), [1]);
    let hits = matched(&texts, "OpenCode", &vocabulary);
    assert_eq!(hits[0], 1);
    let mut aliased = hits[1..].to_vec();
    aliased.sort_unstable();
    assert_eq!(aliased, [0, 2], "only adjacent words alias the term");
    // Fuzzy off searches the words as written.
    let passages = texts.map(|text| Passage::new(PassageKind::Notes, 0, text.into()));
    let refs = passages.iter().collect::<Vec<_>>();
    let exact = score(
        &refs,
        &[0, 1, 2, 3],
        &ParsedQuery::parse("opencode"),
        false,
        &vocabulary,
    );
    assert_eq!(exact.iter().map(|hit| hit.passage).collect::<Vec<_>>(), [1]);
}

#[test]
fn vocabulary_never_pulls_in_common_words() {
    // `Rune` has one consonant sound, so sound-alikes come only from its
    // "sounds like" spellings: the vocabulary changes nothing here.
    let chatter = [
        "rune ships the new layout",
        "I went for a run",
        "the rain stopped",
        "the room is booked",
        "ruin the surprise",
        "rain again tomorrow",
    ];
    assert_eq!(
        matched(&chatter, "rune", &[entry("Rune", &[])]),
        matched(&chatter, "rune", &[])
    );
    // `scroll` sounds like `Scrawl`; once it is a common word here, it is
    // never treated as a mis-transcription.
    let mut texts = vec!["scrawl captures the page", "I fixed the scroll bug"];
    let vocabulary = [entry("Scrawl", &[])];
    assert_eq!(matched(&texts, "scrawl", &vocabulary), [0, 1]);
    texts.extend(["scroll down"; 5]);
    texts.extend(["the standup ran long"; 20]);
    assert_eq!(matched(&texts, "scrawl", &vocabulary), [0]);
    // A spelling the person wrote is always used.
    let written = [entry("Scrawl", &["scroll"])];
    assert_eq!(matched(&texts, "scrawl", &written).len(), 7);
    assert_eq!(matched(&texts, "scrawl", &written)[0], 0);
}

#[test]
fn sound_alikes_need_two_consonant_sounds_and_few_changes() {
    assert_eq!(sound_key("abattoir"), sound_key("avatar"));
    assert!(sounds_alike("abattoir", "avatar"));
    assert!(sounds_alike("claud", "cloud"));
    assert!(sounds_alike("scrawl", "scroll"));
    // One consonant sound matches too many words.
    assert!(!sounds_alike("rune", "rain"));
    assert!(!sounds_alike("dax", "docks"));
    // The first letter and most letters must agree.
    assert!(!sounds_alike("effect", "affect"));
    assert!(!sounds_alike("parakeet", "pirate"));
    assert!(!sounds_alike("avatar", "avatar"));
    // Soundex itself is unchanged by the uncapped key.
    assert_eq!(soundex("abattoir").as_deref(), Some("a136"));
    assert_eq!(soundex("dax").as_deref(), Some("d200"));
    assert_eq!(soundex("moonshine").as_deref(), Some("m525"));
}

#[test]
fn chapters_list_spans_and_lines_and_say_when_there_are_none() {
    let chaptered = Library::new(
        Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/meetings-chapters"),
    );
    let text = chaptered.chapters("1790003000000-1005", NOW).unwrap();
    assert!(
        text.starts_with(
            "1790003000000-1005 \"Team sync: Image storage, Motion lint, Chat app name\""
        ),
        "{text}"
    );
    assert!(
        text.ends_with(
            "\n3 chapters, boundaries corrected over the whole transcript:\n\
             1. 00:00–03:00 · Image storage\n   Where uploaded images live and how long they are kept.\n\
             2. 03:00–06:30 · Motion lint\n   A lint that flags animations which snap instead of easing.\n\
             3. 06:30–09:00 · Chat app name\n   Picking a name for the chat app.\n\
             \nModel-written names that may be wrong. Read a chapter with get_transcript (start and \
             end as shown) or `hex meetings show 1790003000000-1005 --chapter N`.\n"
        ),
        "{text}"
    );
    let meeting = chaptered.meeting("1790003000000-1005").unwrap();
    let views = meeting.chapter_views(NOW);
    assert_eq!(
        views
            .iter()
            .map(|view| (view.number, view.end_ms, view.entries.len()))
            .collect::<Vec<_>>(),
        [(1, 180_000, 3), (2, 390_000, 3), (3, 540_000, 2)]
    );
    assert!(
        views[2]
            .markdown()
            .starts_with("## Chat app name\n\nPicking a name")
    );

    let none = library().chapters(WEEKLY, NOW).unwrap();
    assert!(
        none.contains("No chapters are stored for this meeting."),
        "{none}"
    );
    assert!(chaptered.chapters("missing", NOW).is_err());
}
