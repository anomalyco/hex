use super::*;
use hound::WavReader;

#[test]
fn decodes_float_and_aligned_24_bit_integer_samples() {
    let float = [0.25_f32.to_le_bytes(), (-0.5_f32).to_le_bytes()].concat();
    assert_eq!(decode_samples(&float, 2, 4, 32, 1).unwrap(), [0.25, -0.5]);

    let integer = [
        0x4000_0000_i32.to_le_bytes(),
        (-0x4000_0000_i32).to_le_bytes(),
    ]
    .concat();
    assert_eq!(decode_samples(&integer, 2, 4, 24, 20).unwrap(), [0.5, -0.5]);
}

#[test]
fn rejects_non_numeric_media_times() {
    assert_eq!(media_time_us(CMTime::new(3, 2)), Some(1_500_000));
    assert_eq!(media_time_us(CMTime::INVALID), None);
    assert_eq!(media_time_us(CMTime::positive_infinity()), None);
}

#[test]
fn transcript_entries_coalesce_into_source_turns_without_hiding_long_pauses() {
    let entry = |source, start_ms, end_ms, text: &str| TranscriptEntry {
        source,
        start_ms,
        end_ms,
        text: text.into(),
    };
    let turns = coalesce_transcript([
        entry(MeetingSource::Microphone, 0, 1_000, "First."),
        entry(MeetingSource::Microphone, 1_400, 2_000, "Second."),
        entry(MeetingSource::System, 2_000, 3_000, "Reply."),
        entry(MeetingSource::System, 7_000, 8_000, "Later."),
    ]);

    assert_eq!(turns.len(), 3);
    assert_eq!(turns[0].text, "First. Second.");
    assert_eq!(turns[1].text, "Reply.");
    assert_eq!(turns[2].text, "Later.");
}

#[test]
fn final_transcript_replaces_the_rolling_transcript_atomically() {
    let directory = std::env::temp_dir().join(format!(
        "hex-meeting-transcript-{}-{}",
        std::process::id(),
        now_ms()
    ));
    fs::create_dir_all(&directory).unwrap();
    let rolling = TranscriptEntry {
        source: MeetingSource::Microphone,
        start_ms: 0,
        end_ms: 500,
        text: "rolling transcript".into(),
    };
    fs::write(
        directory.join(transcript::ROLLING_FILE),
        format!(
            "{{\"covered_ms\":{{\"system\":null,\"microphone\":500}}}}\n{}\n",
            serde_json::to_string(&rolling).unwrap()
        ),
    )
    .unwrap();
    assert_eq!(
        transcript_entries_in(&directory).unwrap()[0].text,
        "rolling transcript"
    );

    let final_entry = TranscriptEntry {
        source: MeetingSource::Microphone,
        start_ms: 0,
        end_ms: 500,
        text: "final transcript".into(),
    };
    fs::write(
        directory.join("transcript.ndjson"),
        format!("{}\n", serde_json::to_string(&final_entry).unwrap()),
    )
    .unwrap();
    assert_eq!(
        transcript_entries_in(&directory).unwrap()[0].text,
        "rolling transcript"
    );
    fs::write(directory.join("transcript.md.tmp"), "# Final\n").unwrap();
    let active = ActiveMeeting::acquire(&directory).unwrap();
    recover_final_publication(&directory).unwrap();
    assert!(!directory.join("transcript.md").exists());
    drop(active);
    recover_final_publication(&directory).unwrap();
    assert_eq!(
        transcript_entries_in(&directory).unwrap()[0].text,
        "final transcript"
    );
    fs::remove_dir_all(directory).unwrap();
}

#[test]
fn reading_a_transcript_ignores_an_in_flight_trailing_write() {
    let path = std::env::temp_dir().join(format!(
        "hex-transcript-tail-{}-{}.ndjson",
        std::process::id(),
        now_ms()
    ));
    let complete = TranscriptEntry {
        source: MeetingSource::System,
        start_ms: 0,
        end_ms: 500,
        text: "complete".into(),
    };
    fs::write(
        &path,
        format!(
            "{}\n{{\"source\":\"system\"",
            serde_json::to_string(&complete).unwrap()
        ),
    )
    .unwrap();

    let events: Vec<TranscriptEntry> = read_ndjson(&path).unwrap();
    assert_eq!(events.len(), 1);
    assert_eq!(events[0].text, "complete");

    let mut bytes = fs::read(&path).unwrap();
    bytes.extend_from_slice(&[0xe2, 0x82]);
    fs::write(&path, bytes).unwrap();
    let events: Vec<TranscriptEntry> = read_ndjson(&path).unwrap();
    assert_eq!(events.len(), 1);
    fs::remove_file(path).unwrap();
}

#[test]
fn capture_failures_are_persisted_instead_of_looking_interrupted() {
    let directory = std::env::temp_dir().join(format!(
        "hex-meeting-failure-{}-{}",
        std::process::id(),
        now_ms()
    ));
    fs::create_dir_all(&directory).unwrap();
    let manifest = MeetingManifest {
        schema_version: 1,
        id: "failure-test".into(),
        title: "Failure test".into(),
        status: MeetingStatus::Recording,
        created_at_ms: now_ms(),
        ended_at_ms: None,
        duration_ms: None,
        system: TrackManifest::default(),
        microphone: TrackManifest::default(),
        live_transcription_error: None,
        error: None,
        app: None,
        export_path: None,
        export_error: None,
        echo_suppressed_utterances: 0,
        provisional_title: None,
        notes: None,
        notes_error: None,
        calendar: None,
        title_source: None,
        chapters: Vec::new(),
        chapters_refined: false,
    };
    write_manifest(&directory, &manifest).unwrap();

    persist_meeting_failure(&directory, &eyre!("capture setup failed"));

    let manifest = read_manifest(&directory).unwrap();
    assert_eq!(manifest.status, MeetingStatus::Failed);
    assert_eq!(manifest.error.as_deref(), Some("capture setup failed"));
    assert!(manifest.ended_at_ms.is_some());
    assert!(manifest.duration_ms.is_some());
    fs::remove_dir_all(directory).unwrap();
}

#[test]
fn mid_meeting_capture_loss_proceeds_to_transcription_when_audio_was_recorded() {
    let directory = std::env::temp_dir().join(format!(
        "hex-meeting-capture-loss-{}-{}",
        std::process::id(),
        now_ms()
    ));
    fs::create_dir_all(&directory).unwrap();
    let mut manifest = MeetingManifest {
        schema_version: 1,
        id: "capture-loss-test".into(),
        title: "Capture loss test".into(),
        status: MeetingStatus::Recording,
        created_at_ms: now_ms(),
        ended_at_ms: Some(now_ms()),
        duration_ms: Some(60_000),
        system: TrackManifest::default(),
        microphone: TrackManifest::default(),
        live_transcription_error: None,
        error: None,
        app: None,
        export_path: None,
        export_error: None,
        echo_suppressed_utterances: 0,
        provisional_title: None,
        notes: None,
        notes_error: None,
        calendar: None,
        title_source: None,
        chapters: Vec::new(),
        chapters_refined: false,
    };
    let (sender, receiver) = mpsc::sync_channel(4);

    let empty_error = prepare_for_transcription(
        &directory,
        &mut manifest,
        Some("SCStream error (No capture source provided)".into()),
        Ok(()),
        Some(&sender),
    )
    .unwrap_err();
    assert!(
        empty_error
            .to_string()
            .contains("No capture source provided")
    );
    assert!(receiver.try_recv().is_err());

    manifest.system.samples = 16_000 * 60;
    prepare_for_transcription(
        &directory,
        &mut manifest,
        Some(
            "SCStream error (No capture source provided): Failed to find any displays or windows to capture"
                .into(),
        ),
        Err(eyre!("stream already stopped")),
        Some(&sender),
    )
    .unwrap();
    assert_eq!(manifest.status, MeetingStatus::Transcribing);
    assert_eq!(manifest.error, None);
    assert_eq!(receiver.try_recv().unwrap(), RecordingUpdate::Transcribing);
    let persisted = read_manifest(&directory).unwrap();
    assert_eq!(persisted.status, MeetingStatus::Transcribing);
    assert_eq!(persisted.error, None);
    fs::remove_dir_all(directory).unwrap();
}

#[test]
fn abandoned_recording_and_finalization_are_recovered_as_interrupted() {
    assert_eq!(
        recovered_status(MeetingStatus::Starting, false, false),
        MeetingStatus::Interrupted
    );
    assert_eq!(
        recovered_status(MeetingStatus::Recording, false, false),
        MeetingStatus::Interrupted
    );
    assert_eq!(
        recovered_status(MeetingStatus::Transcribing, false, false),
        MeetingStatus::Interrupted
    );
    assert_eq!(
        recovered_status(MeetingStatus::Recording, true, false),
        MeetingStatus::Recording
    );
    assert_eq!(
        recovered_status(MeetingStatus::Complete, false, true),
        MeetingStatus::Complete
    );
    assert_eq!(
        recovered_status(MeetingStatus::Transcribing, false, true),
        MeetingStatus::Complete
    );
    assert_eq!(
        recovered_status(MeetingStatus::Complete, false, false),
        MeetingStatus::Interrupted
    );
}

#[test]
fn meeting_lock_reports_liveness_across_file_descriptors() {
    let directory = std::env::temp_dir().join(format!(
        "hex-meeting-lock-{}-{}",
        std::process::id(),
        now_ms()
    ));
    fs::create_dir_all(&directory).unwrap();

    let active = ActiveMeeting::acquire(&directory).unwrap();
    assert_eq!(meeting_is_active(&directory), Some(true));
    drop(active);
    assert_eq!(meeting_is_active(&directory), Some(false));

    fs::remove_dir_all(directory).unwrap();
}

fn finished_meeting(name: &str, title: &str) -> (PathBuf, PathBuf) {
    let base = std::env::temp_dir().join(format!("hex-{name}-{}-{}", std::process::id(), now_ms()));
    let directory = base.join("1760000000000-1");
    fs::create_dir_all(&directory).unwrap();
    let export = base.join("2026-10-15 1418 Meeting.md");
    fs::write(
        &export,
        format!("# {title}\n\n## Transcript\n\n**You** · 00:01\nHi.\n"),
    )
    .unwrap();
    let manifest: MeetingManifest = serde_json::from_value(serde_json::json!({
        "schema_version": 1,
        "id": "1760000000000-1",
        "title": title,
        "status": "complete",
        "created_at_ms": 1_760_000_000_000_u64,
        "ended_at_ms": 1_760_000_060_000_u64,
        "duration_ms": 60_000,
        "system": {"file": "system.wav", "samples": 0, "dropped_packets": 0},
        "microphone": {"file": "microphone.wav", "samples": 0, "dropped_packets": 0},
        "error": null,
        "export_path": export,
    }))
    .unwrap();
    write_manifest(&directory, &manifest).unwrap();
    (base, directory)
}

#[test]
fn renaming_updates_the_manifest_and_an_unedited_markdown_heading() {
    let (base, directory) = finished_meeting("rename", "Meeting — 2:18 PM");
    let export = read_manifest(&directory).unwrap().export_path.unwrap();
    let renamed = rename_meeting(&directory, "  Arcade   planning ").unwrap();
    assert_eq!(renamed.title, "Arcade planning");
    assert_eq!(read_manifest(&directory).unwrap().title, "Arcade planning");
    // Only live chapters read the title's source; with the experiment off
    // the manifest gains no field.
    let live_chapters = crate::labs::live_chapters();
    assert_eq!(
        fs::read_to_string(directory.join("manifest.json"))
            .unwrap()
            .contains("title_source"),
        live_chapters
    );
    assert!(
        fs::read_to_string(&export)
            .unwrap()
            .starts_with("# Arcade planning\n")
    );

    // A heading the user changed is theirs; only the manifest follows.
    fs::write(&export, "# My own heading\n\nNotes.\n").unwrap();
    rename_meeting(&directory, "Arcade sync").unwrap();
    assert_eq!(read_manifest(&directory).unwrap().title, "Arcade sync");
    assert_eq!(
        fs::read_to_string(&export).unwrap(),
        "# My own heading\n\nNotes.\n"
    );
    assert!(rename_meeting(&directory, "   ").is_err());
    fs::remove_dir_all(base).unwrap();
}

#[test]
fn chapters_extend_every_title_but_one_the_user_typed() {
    let chapters = vec![
        Chapter {
            title: "Image storage".into(),
            summary: String::new(),
            start_ms: 0,
        },
        Chapter {
            title: "Motion lint".into(),
            summary: String::new(),
            start_ms: 200_000,
        },
        Chapter {
            title: "Chat app name".into(),
            summary: String::new(),
            start_ms: 500_000,
        },
    ];
    let (base, directory) = finished_meeting("chapter-title", "Team sync");
    let mut manifest = read_manifest(&directory).unwrap();
    manifest.duration_ms = Some(700_000);
    manifest.chapters = chapters;
    // Older manifests have no source: a title other than the default is
    // the user's, so chapters leave it alone.
    assert_eq!(manifest.title_source, None);
    assert_eq!(manifest.title_source(), TitleSource::User);
    assert_eq!(display_title(&manifest), "Team sync");

    for source in [TitleSource::Call, TitleSource::Calendar] {
        manifest.title_source = Some(source);
        assert_eq!(
            display_title(&manifest),
            "Team sync: Image storage, Motion lint, Chat app name"
        );
    }
    let default = default_title(None, manifest.created_at_ms);
    manifest.title = default.clone();
    manifest.title_source = None;
    assert_eq!(manifest.title_source(), TitleSource::Default);
    assert_eq!(
        display_title(&manifest),
        format!("{default}: Image storage, Motion lint, Chat app name")
    );
    manifest.provisional_title = Some("Launch prep".into());
    assert_eq!(
        display_title(&manifest),
        "Launch prep: Image storage, Motion lint, Chat app name"
    );
    write_manifest(&directory, &manifest).unwrap();

    // A rename is the user's, and stays theirs while the recorder, holding
    // its own copy of the manifest, records more chapters.
    let renamed = rename_meeting(&directory, "Budget").unwrap();
    assert_eq!(renamed.title_source, Some(TitleSource::User));
    assert_eq!(display_title(&read_manifest(&directory).unwrap()), "Budget");
    adopt_rename(&directory, &mut manifest);
    assert_eq!(manifest.title, "Budget");
    assert_eq!(manifest.title_source(), TitleSource::User);
    assert_eq!(display_title(&manifest), "Budget");

    // A generated title on disk is not adopted.
    let mut generated = read_manifest(&directory).unwrap();
    generated.title = "Calendar name".into();
    generated.title_source = Some(TitleSource::Calendar);
    write_manifest(&directory, &generated).unwrap();
    adopt_rename(&directory, &mut manifest);
    assert_eq!(manifest.title, "Budget");
    fs::remove_dir_all(base).unwrap();
}

#[test]
fn deleting_trashes_the_markdown_then_the_recording_and_refuses_while_recording() {
    let (base, directory) = finished_meeting("trash", "Design sync");
    let export = read_manifest(&directory).unwrap().export_path.unwrap();
    let mut trashed = Vec::new();

    let active = ActiveMeeting::acquire(&directory).unwrap();
    let refused = trash_meeting(&directory, |path| {
        trashed.push(path.to_path_buf());
        Ok(())
    });
    assert!(
        refused
            .unwrap_err()
            .to_string()
            .contains("Stop the recording")
    );
    assert!(trashed.is_empty());
    drop(active);

    trash_meeting(&directory, |path| {
        trashed.push(path.to_path_buf());
        Ok(())
    })
    .unwrap();
    assert_eq!(trashed, [export.clone(), directory.clone()]);

    // Without a Markdown copy only the recording moves.
    fs::remove_file(&export).unwrap();
    trashed.clear();
    trash_meeting(&directory, |path| {
        trashed.push(path.to_path_buf());
        Ok(())
    })
    .unwrap();
    assert_eq!(trashed, [directory]);
    fs::remove_dir_all(base).unwrap();
}

#[test]
fn paste_cursor_counts_coalesced_turns_and_advances_only_on_success() {
    let entry = |source, start_ms, end_ms, text: &str| TranscriptEntry {
        source,
        start_ms,
        end_ms,
        text: text.into(),
    };
    let initial = vec![
        entry(MeetingSource::Microphone, 0, 1_000, "First."),
        entry(MeetingSource::Microphone, 1_200, 2_000, "Still me."),
        entry(MeetingSource::System, 2_500, 3_500, "Got it."),
    ];
    let mut cursor = MeetingPasteCursor::default();
    assert_eq!(cursor.pending_turn_count("meeting-a", &initial), 2);

    // A failed paste or clipboard write must not advance the cursor.
    let failed = cursor.consume_entries("meeting-a", &initial, |_| {
        Err(eyre!("clipboard unavailable"))
    });
    assert!(failed.is_err());
    assert_eq!(cursor.pending_turn_count("meeting-a", &initial), 2);

    // A successful copy/paste commits the entries and drops the count to 0.
    let mut copied = String::new();
    let text = cursor
        .consume_entries("meeting-a", &initial, |delta| {
            copied = delta.to_string();
            Ok(())
        })
        .unwrap();
    assert_eq!(text, "You: First. Still me.\n\nThem: Got it.");
    assert_eq!(copied, text);
    assert_eq!(cursor.pending_turn_count("meeting-a", &initial), 0);
    assert!(
        cursor
            .consume_entries("meeting-a", &initial, |_| Ok(()))
            .is_err()
    );

    // Appending a new turn makes only that turn pending for the next Copy new / Option-Control-V.
    let mut extended = initial.clone();
    extended.push(entry(
        MeetingSource::Microphone,
        6_000,
        7_000,
        "Next point.",
    ));
    assert_eq!(cursor.pending_turn_count("meeting-a", &extended), 1);
    let delta = cursor
        .consume_entries("meeting-a", &extended, |_| Ok(()))
        .unwrap();
    assert_eq!(delta, "You: Next point.");
    assert_eq!(cursor.pending_turn_count("meeting-a", &extended), 0);

    // Switching to a new meeting starts from the beginning.
    assert_eq!(cursor.pending_turn_count("meeting-b", &initial), 2);
}

#[test]
fn paste_cursor_neither_repeats_nor_drops_text_when_the_transcript_is_rewritten() {
    let entry = |source, start_ms, end_ms, text: &str| TranscriptEntry {
        source,
        start_ms,
        end_ms,
        text: text.into(),
    };
    let mut cursor = MeetingPasteCursor::default();
    let mut pasted = Vec::new();
    let mut paste = |cursor: &mut MeetingPasteCursor, entries: &[TranscriptEntry]| {
        if let Ok(text) = cursor.consume_entries("meeting", entries, |_| Ok(())) {
            pasted.push(text);
        }
    };
    let rolling = vec![
        entry(MeetingSource::System, 0, 4_000, "Kickoff is at two."),
        entry(MeetingSource::System, 4_200, 6_000, "Sound good?"),
    ];
    paste(&mut cursor, &rolling);

    // The microphone entry before the copied remote speech is published once
    // the remote side is transcribed past it, and a re-judged echo line
    // disappears; neither repeats what was copied.
    let later = vec![
        entry(MeetingSource::System, 0, 4_000, "Kickoff is at two."),
        entry(MeetingSource::Microphone, 3_000, 4_000, "Two works."),
        entry(MeetingSource::System, 4_200, 6_000, "Sound good?"),
        entry(MeetingSource::System, 6_100, 9_000, "Then let's ship it."),
    ];
    paste(&mut cursor, &later);

    // After Stop, the final transcript rebuilt a track with different
    // segmentation; only text mostly after what was copied is new.
    let published = vec![
        entry(
            MeetingSource::System,
            0,
            6_000,
            "Kickoff is at two. Sounds good?",
        ),
        entry(MeetingSource::Microphone, 3_000, 4_000, "Two works."),
        entry(MeetingSource::System, 6_100, 9_000, "Then let's ship it."),
        entry(MeetingSource::System, 8_800, 11_000, "And the tail."),
        entry(MeetingSource::Microphone, 10_000, 11_000, "Bye."),
    ];
    paste(&mut cursor, &published);
    assert!(
        cursor
            .consume_entries("meeting", &published, |_| Ok(()))
            .is_err()
    );
    assert_eq!(
        pasted,
        [
            "Them: Kickoff is at two. Sound good?",
            "You: Two works.\n\nThem: Then let's ship it.",
            "Them: And the tail.\n\nYou: Bye.",
        ]
    );
}

#[test]
fn paste_cursor_pastes_text_the_final_pass_recovers_for_skipped_audio() {
    let entry = |start_ms, end_ms, text: &str| TranscriptEntry {
        source: MeetingSource::System,
        start_ms,
        end_ms,
        text: text.into(),
    };
    let mut cursor = MeetingPasteCursor::default();
    let mut paste = |entries: &[TranscriptEntry]| {
        cursor
            .consume_entries("meeting", entries, |_| Ok(()))
            .unwrap_or_default()
    };
    // The in-call transcript fell behind and skipped 10–40 s of audio.
    assert_eq!(
        paste(&[
            entry(0, 9_000, "Before the backlog."),
            entry(41_000, 45_000, "After the backlog."),
        ]),
        "Them: Before the backlog.\n\nThem: After the backlog."
    );
    // The final pass rebuilt the skipped audio; only that text is new.
    assert_eq!(
        paste(&[
            entry(0, 9_000, "Before the backlog."),
            entry(12_000, 30_000, "Recovered middle."),
            entry(41_000, 45_000, "After the backlog."),
        ]),
        "Them: Recovered middle."
    );
}

fn packet_at(source: MeetingSource, arrival_ms: u64, samples: usize) -> AudioPacket {
    AudioPacket {
        source,
        pts_us: None,
        arrival_us: arrival_ms * 1_000,
        position: 0,
        sample_rate: SAMPLE_RATE,
        samples: vec![0.25; samples],
    }
}

#[test]
fn contiguous_audio_with_jitter_is_not_padded() {
    let track = TrackManifest {
        first_arrival_us: Some(100_000),
        samples: 16_000,
        ..TrackManifest::default()
    };
    // One second of audio held, the next 100 ms packet arrives 300 ms late.
    let late = packet_at(MeetingSource::Microphone, 1_500, 1_600);
    assert_eq!(missing_samples(&track, &late), 0);
}

#[test]
fn a_source_that_resumes_after_an_outage_is_padded_back_onto_the_timeline() {
    let track = TrackManifest {
        first_arrival_us: Some(0),
        samples: 16_000,
        ..TrackManifest::default()
    };
    // The source went silent after one second and resumed at 11.1 s.
    let resumed = packet_at(MeetingSource::System, 11_100, 1_600);
    assert_eq!(missing_samples(&track, &resumed), 160_000);
}

#[test]
fn writer_fills_a_capture_gap_with_silence_and_records_it() {
    let directory = std::env::temp_dir().join(format!(
        "hex-meeting-gap-{}-{}",
        std::process::id(),
        now_ms()
    ));
    fs::create_dir_all(&directory).unwrap();
    let (sender, receiver) = mpsc::sync_channel(16);
    for (arrival_ms, source) in [
        (100, MeetingSource::System),
        (100, MeetingSource::Microphone),
        (200, MeetingSource::System),
        (200, MeetingSource::Microphone),
        // System audio drops out for five seconds; the microphone continues.
        (300, MeetingSource::Microphone),
        (5_300, MeetingSource::System),
    ] {
        sender.send(packet_at(source, arrival_ms, 1_600)).unwrap();
    }
    drop(sender);
    let stats = write_tracks(&directory, receiver, None).unwrap();
    assert_eq!(stats.microphone.samples, 4_800);
    assert!(stats.microphone.gaps.is_empty());
    assert_eq!(stats.system.gaps.len(), 1);
    assert_eq!(stats.system.gaps[0].start_ms, 200);
    assert_eq!(stats.system.gaps[0].duration_ms, 4_900);
    assert_eq!(stats.system.samples, 3_200 + 78_400 + 1_600);
    let wav = WavReader::open(directory.join("system.wav")).unwrap();
    assert_eq!(u64::from(wav.len()), stats.system.samples);
    fs::remove_dir_all(directory).unwrap();
}
