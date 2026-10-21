use super::*;
use crate::events::TranscriptPhase;
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
fn live_transcript_projects_the_latest_version_of_each_source_line() {
    let event = |source, line_id, phase, start_ms, text: &str| live::Event {
        source,
        line_id,
        phase,
        start_ms,
        end_ms: start_ms + 500,
        text: text.into(),
        signal: None,
    };
    let transcript = live::project([
        event(
            MeetingSource::Microphone,
            1,
            TranscriptPhase::Started,
            1_000,
            "draft",
        ),
        event(
            MeetingSource::System,
            4,
            TranscriptPhase::Completed,
            500,
            "computer",
        ),
        event(
            MeetingSource::Microphone,
            1,
            TranscriptPhase::Completed,
            1_000,
            "final live line",
        ),
    ]);

    assert_eq!(transcript.len(), 2);
    assert_eq!(transcript[0].source, MeetingSource::System);
    assert_eq!(transcript[0].text, "computer");
    assert_eq!(transcript[1].source, MeetingSource::Microphone);
    assert_eq!(transcript[1].text, "final live line");
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
fn final_transcript_supplements_gaps_without_replacing_live_entries() {
    let entry = |source, start_ms, end_ms, text: &str| TranscriptEntry {
        source,
        start_ms,
        end_ms,
        text: text.into(),
    };
    let mut live = vec![entry(MeetingSource::Microphone, 0, 1_000, "Live words.")];
    supplement_live_transcript(
        &mut live,
        vec![
            entry(MeetingSource::Microphone, 100, 900, "Final replacement."),
            entry(MeetingSource::System, 100, 900, "Other source."),
            entry(MeetingSource::Microphone, 1_200, 2_000, "Recovered tail."),
        ],
    );

    assert_eq!(live.len(), 3);
    assert_eq!(live[0].text, "Live words.");
    assert_eq!(live[1].text, "Other source.");
    assert_eq!(live[2].text, "Recovered tail.");
}

#[test]
fn live_timestamps_preserve_dropped_packet_gaps_and_match_final_origin() {
    // A dropped 10 ms microphone packet is far below any media-time jitter
    // threshold, but the recorded position still accounts for it.
    let packet = AudioPacket {
        source: MeetingSource::Microphone,
        pts_us: None,
        arrival_us: 1_200_000,
        position: 44_100 + 441,
        sample_rate: 44_100,
        samples: vec![0.0; 441],
    };
    assert_eq!(
        live::audio_gap(44_100, &packet),
        Some(live::AudioGap {
            silence_samples: 441,
            skipped_us: 0,
        })
    );
    assert_eq!(live::audio_gap(44_100 + 441, &packet), None);
    assert_eq!(live::source_offset([Some(500_000), Some(700_000)], 0), 0);
    assert_eq!(
        live::source_offset([Some(500_000), Some(700_000)], 1),
        200_000
    );
}

#[test]
fn live_gaps_bound_synthesized_silence() {
    let delayed = AudioPacket {
        source: MeetingSource::Microphone,
        pts_us: None,
        arrival_us: 2_000_000,
        position: 44_100 * 3,
        sample_rate: 44_100,
        samples: vec![0.0; 8_820],
    };
    assert_eq!(
        live::audio_gap(44_100, &delayed),
        Some(live::AudioGap {
            silence_samples: 22_050,
            skipped_us: 1_500_000,
        })
    );
    assert_eq!(live::adjusted_time_ms(499, &[(500_000, 1_500_000)]), 499);
    assert_eq!(live::adjusted_time_ms(500, &[(500_000, 1_500_000)]), 2_000);
}

#[test]
fn live_microphone_normalization_lifts_quiet_speech_without_touching_system_audio() {
    let mut microphone = vec![0.002, -0.002, 0.001, -0.001];
    live::normalize_microphone(MeetingSource::Microphone, &mut microphone);
    assert!(microphone.iter().copied().fold(0.0_f32, f32::max) > 0.02);

    let mut system = vec![0.002, -0.002];
    live::normalize_microphone(MeetingSource::System, &mut system);
    assert_eq!(system, [0.002, -0.002]);
}

#[test]
fn final_transcript_replaces_the_live_draft_atomically() {
    let directory = std::env::temp_dir().join(format!(
        "voice-control-meeting-transcript-{}-{}",
        std::process::id(),
        now_ms()
    ));
    fs::create_dir_all(&directory).unwrap();
    let live = live::Event {
        source: MeetingSource::Microphone,
        line_id: 1,
        phase: TranscriptPhase::Completed,
        start_ms: 0,
        end_ms: 500,
        text: "live draft".into(),
        signal: None,
    };
    fs::write(
        directory.join("transcript.live.ndjson"),
        format!("{}\n", serde_json::to_string(&live).unwrap()),
    )
    .unwrap();
    assert_eq!(
        transcript_entries_in(&directory).unwrap()[0].text,
        "live draft"
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
        "live draft"
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
fn reading_live_transcript_ignores_an_in_flight_trailing_write() {
    let path = std::env::temp_dir().join(format!(
        "voice-control-live-transcript-{}-{}.ndjson",
        std::process::id(),
        now_ms()
    ));
    let complete = live::Event {
        source: MeetingSource::System,
        line_id: 1,
        phase: TranscriptPhase::Completed,
        start_ms: 0,
        end_ms: 500,
        text: "complete".into(),
        signal: None,
    };
    fs::write(
        &path,
        format!(
            "{}\n{{\"source\":\"system\"",
            serde_json::to_string(&complete).unwrap()
        ),
    )
    .unwrap();

    let events: Vec<live::Event> = read_ndjson(&path).unwrap();
    assert_eq!(events.len(), 1);
    assert_eq!(events[0].text, "complete");

    let mut bytes = fs::read(&path).unwrap();
    bytes.extend_from_slice(&[0xe2, 0x82]);
    fs::write(&path, bytes).unwrap();
    let events: Vec<live::Event> = read_ndjson(&path).unwrap();
    assert_eq!(events.len(), 1);
    fs::remove_file(path).unwrap();
}

#[test]
fn transcript_reader_only_parses_newly_appended_live_events() {
    let path = std::env::temp_dir().join(format!(
        "voice-control-live-tail-{}-{}.ndjson",
        std::process::id(),
        now_ms()
    ));
    let event = |line_id, text: &str| live::Event {
        source: MeetingSource::System,
        line_id,
        phase: TranscriptPhase::Updated,
        start_ms: line_id * 1_000,
        end_ms: line_id * 1_000 + 500,
        text: text.into(),
        signal: None,
    };
    let first = serde_json::to_string(&event(1, "first")).unwrap();
    let second = serde_json::to_string(&event(2, "second")).unwrap();
    let split = second.len() / 2;
    fs::write(&path, format!("{first}\n{}", &second[..split])).unwrap();

    let mut reader = TranscriptReader::default();
    reader.read_live_tail(&path).unwrap();
    assert_eq!(reader.event_count(), 1);
    assert!(reader.has_pending_bytes());

    let mut file = fs::OpenOptions::new().append(true).open(&path).unwrap();
    writeln!(file, "{}", &second[split..]).unwrap();
    reader.read_live_tail(&path).unwrap();
    assert_eq!(reader.event_count(), 2);
    assert!(!reader.has_pending_bytes());
    fs::remove_file(path).unwrap();
}

#[test]
fn capture_failures_are_persisted_instead_of_looking_interrupted() {
    let directory = std::env::temp_dir().join(format!(
        "voice-control-meeting-failure-{}-{}",
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
        live_dropped_packets: 0,
        live_transcription_error: None,
        error: None,
        app: None,
        export_path: None,
        export_error: None,
        echo_suppressed_utterances: 0,
        provisional_title: None,
        notes: None,
        notes_error: None,
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
        "voice-control-meeting-capture-loss-{}-{}",
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
        live_dropped_packets: 0,
        live_transcription_error: None,
        error: None,
        app: None,
        export_path: None,
        export_error: None,
        echo_suppressed_utterances: 0,
        provisional_title: None,
        notes: None,
        notes_error: None,
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
        "voice-control-meeting-lock-{}-{}",
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

#[test]
#[ignore = "requires VOICE_CONTROL_MEETING_FIXTURE and the installed Moonshine model"]
fn live_transcription_replays_both_recorded_tracks() {
    let fixture = PathBuf::from(std::env::var("VOICE_CONTROL_MEETING_FIXTURE").unwrap());
    let output = std::env::temp_dir().join(format!(
        "voice-control-live-replay-{}-{}",
        std::process::id(),
        now_ms()
    ));
    fs::create_dir_all(&output).unwrap();
    let (sender, receiver) = mpsc::sync_channel(live::QUEUE_CAPACITY);
    let worker_output = output.clone();
    let drops = Arc::new(AtomicU64::new(0));
    let worker_drops = drops.clone();
    let worker = thread::spawn(move || {
        live::transcribe(
            Path::new(env!("CARGO_MANIFEST_DIR")),
            &worker_output,
            receiver,
            &worker_drops,
        )
    });

    let tracks = [
        (MeetingSource::System, "system.wav"),
        (MeetingSource::Microphone, "microphone.wav"),
    ]
    .map(|(source, file)| {
        let mut reader = WavReader::open(fixture.join(file)).unwrap();
        let sample_rate = reader.spec().sample_rate;
        let samples = reader
            .samples::<f32>()
            .take(sample_rate as usize * 10)
            .collect::<Result<Vec<_>, _>>()
            .unwrap();
        (source, sample_rate, samples)
    });
    let chunks = tracks
        .iter()
        .map(|(_, sample_rate, samples)| samples.len().div_ceil(*sample_rate as usize / 5))
        .max()
        .unwrap_or_default();
    for index in 0..chunks {
        for (source, sample_rate, samples) in &tracks {
            let packet_samples = *sample_rate as usize / 5;
            let start = index * packet_samples;
            let end = (start + packet_samples).min(samples.len());
            if start >= end {
                continue;
            }
            sender
                .send(AudioPacket {
                    source: *source,
                    pts_us: None,
                    arrival_us: index as u64 * 200_000,
                    position: start as u64,
                    sample_rate: *sample_rate,
                    samples: samples[start..end].to_vec(),
                })
                .unwrap();
        }
        thread::sleep(Duration::from_millis(200));
    }
    drop(sender);
    worker.join().unwrap().unwrap();

    let events: Vec<live::Event> = read_ndjson(&output.join("transcript.live.ndjson")).unwrap();
    assert!(!events.is_empty());
    for source in [MeetingSource::System, MeetingSource::Microphone] {
        assert!(
            events
                .iter()
                .any(|event| { event.source == source && !event.text.trim().is_empty() }),
            "{source:?} emitted no text: {events:#?}"
        );
        assert!(
            events.iter().any(|event| {
                event.source == source && event.phase == TranscriptPhase::Completed
            }),
            "{source:?} emitted no completed line: {events:#?}"
        );
    }
    fs::remove_dir_all(output).unwrap();
}

#[test]
fn spanish_selection_shows_the_model_transcript_not_the_english_draft() {
    use crate::transcription_models::{TranscriptionModelId, TranscriptionSelection};

    let spanish = TranscriptionSelection {
        model: TranscriptionModelId::ParakeetV3,
        language: "es".into(),
        recognition_hints: String::new(),
    };
    let rolling = transcript::RollingView {
        entries: vec![TranscriptEntry {
            source: MeetingSource::System,
            start_ms: 0,
            end_ms: 20_000,
            text: "Hola a todos.".into(),
        }],
        ..Default::default()
    };
    let draft = [live_event(
        MeetingSource::System,
        1,
        TranscriptPhase::Completed,
        (1_000, 2_000),
        "Hello everyone",
        None,
    )];
    assert_eq!(
        shown(&live::shown_while_recording(
            &spanish,
            &rolling,
            draft.iter()
        )),
        [(MeetingSource::System, "Hola a todos.")]
    );

    let english = TranscriptionSelection::default();
    assert_eq!(
        shown(&live::shown_while_recording(
            &english,
            &transcript::RollingView::default(),
            draft.iter(),
        )),
        [(MeetingSource::System, "Hello everyone")]
    );
}

#[test]
fn rolling_entries_replace_the_live_draft_for_the_time_they_cover() {
    let entry = |source, start_ms, end_ms, text: &str| TranscriptEntry {
        source,
        start_ms,
        end_ms,
        text: text.into(),
    };
    let rolling = [
        entry(MeetingSource::System, 0, 20_000, "Final system."),
        entry(MeetingSource::Microphone, 1_000, 9_000, "Final mic."),
    ];
    let live = vec![
        entry(MeetingSource::System, 5_000, 6_000, "draft system"),
        entry(MeetingSource::Microphone, 8_000, 8_500, "draft overlap"),
        entry(MeetingSource::Microphone, 9_500, 10_000, "draft mic tail"),
        entry(MeetingSource::System, 21_000, 22_000, "draft system tail"),
    ];
    let rolling = transcript::RollingView {
        entries: rolling.to_vec(),
        ..Default::default()
    };
    let merged = live::merge_rolling(&rolling, live.clone());
    assert_eq!(
        merged
            .iter()
            .map(|entry| entry.text.as_str())
            .collect::<Vec<_>>(),
        [
            "Final system.",
            "Final mic.",
            "draft mic tail",
            "draft system tail"
        ]
    );
    assert_eq!(live::merge_rolling(&Default::default(), live.clone()), live);
}

#[test]
fn rolling_coverage_hides_draft_echo_that_rolling_dropped() {
    let entry = |source, start_ms, end_ms, text: &str| TranscriptEntry {
        source,
        start_ms,
        end_ms,
        text: text.into(),
    };
    // Rolling transcribed the microphone through 30 s and dropped all of it
    // as echo, so no microphone entry marks how far it got.
    let rolling = transcript::RollingView {
        entries: vec![entry(MeetingSource::System, 0, 29_000, "Final system.")],
        covered_ms: transcript::RollingCoverage {
            system: Some(30_000),
            microphone: Some(30_000),
        },
    };
    let live = vec![
        entry(MeetingSource::Microphone, 12_000, 13_000, "draft echo"),
        entry(MeetingSource::Microphone, 31_000, 32_000, "draft tail"),
    ];
    assert_eq!(
        live::merge_rolling(&rolling, live)
            .iter()
            .map(|entry| entry.text.as_str())
            .collect::<Vec<_>>(),
        ["Final system.", "draft tail"]
    );
}

fn live_event(
    source: MeetingSource,
    line_id: u64,
    phase: TranscriptPhase,
    (start_ms, end_ms): (u64, u64),
    text: &str,
    signal: Option<echo::Signal>,
) -> live::Event {
    live::Event {
        source,
        line_id,
        phase,
        start_ms,
        end_ms,
        text: text.into(),
        signal,
    }
}

fn shown(entries: &[TranscriptEntry]) -> Vec<(MeetingSource, &str)> {
    entries
        .iter()
        .map(|entry| (entry.source, entry.text.as_str()))
        .collect()
}

#[test]
fn live_view_hides_microphone_twins_of_remote_speech() {
    use TranscriptPhase::Completed;
    let events = [
        live_event(
            MeetingSource::System,
            1,
            Completed,
            (19_635, 22_707),
            "I feel like over the course of a couple of months we could make like",
            None,
        ),
        live_event(
            MeetingSource::Microphone,
            2,
            Completed,
            (19_718, 22_630),
            "I feel like over the course of a couple of months we can make by...",
            None,
        ),
        live_event(
            MeetingSource::System,
            3,
            Completed,
            (24_051, 25_811),
            "To thirty games and that's",
            None,
        ),
        live_event(
            MeetingSource::Microphone,
            4,
            Completed,
            (23_974, 25_574),
            "230 games and that's",
            None,
        ),
        // Misheard bleed with different words: only the signal catches it.
        live_event(
            MeetingSource::System,
            5,
            Completed,
            (7_763, 9_075),
            "I feel like it.",
            None,
        ),
        live_event(
            MeetingSource::Microphone,
            6,
            Completed,
            (7_846, 8_998),
            "Good luck, man.",
            Some(echo::Signal::Bleed),
        ),
        // The user talking over the remote side with matching words.
        live_event(
            MeetingSource::System,
            7,
            Completed,
            (40_000, 41_000),
            "Yeah.",
            None,
        ),
        live_event(
            MeetingSource::Microphone,
            8,
            Completed,
            (40_050, 41_000),
            "Yeah.",
            Some(echo::Signal::Local),
        ),
        live_event(
            MeetingSource::Microphone,
            9,
            Completed,
            (50_000, 52_000),
            "Apple has a new thing.",
            None,
        ),
    ];
    let entries = live::live_view(&Default::default(), events.iter());
    assert_eq!(
        shown(&entries),
        [
            (MeetingSource::System, "I feel like it."),
            (
                MeetingSource::System,
                "I feel like over the course of a couple of months we could make like"
            ),
            (MeetingSource::System, "To thirty games and that's"),
            (MeetingSource::System, "Yeah."),
            (MeetingSource::Microphone, "Yeah."),
            (MeetingSource::Microphone, "Apple has a new thing."),
        ]
    );
}

#[test]
fn an_undecided_draft_line_waits_while_the_remote_side_speaks() {
    use TranscriptPhase::{Completed, Updated};
    let system = live_event(
        MeetingSource::System,
        1,
        Updated,
        (1_000, 3_000),
        "Basically, I want",
        None,
    );
    let draft = |phase, text: &str| {
        live_event(
            MeetingSource::Microphone,
            2,
            phase,
            (1_080, 2_900),
            text,
            None,
        )
    };
    let entries = live::live_view(&Default::default(), [&system, &draft(Updated, "Basically")]);
    assert_eq!(
        shown(&entries),
        [(MeetingSource::System, "Basically, I want")]
    );

    // No twin arrived: once complete, the line is the user's.
    let entries = live::live_view(
        &Default::default(),
        [&system, &draft(Completed, "Hold on a second.")],
    );
    assert_eq!(
        shown(&entries),
        [
            (MeetingSource::System, "Basically, I want"),
            (MeetingSource::Microphone, "Hold on a second."),
        ]
    );

    // Local speech shows immediately, and a draft with no remote speech
    // around it is never held.
    let local = live_event(
        MeetingSource::Microphone,
        3,
        Updated,
        (1_200, 2_000),
        "Wait",
        Some(echo::Signal::Local),
    );
    let alone = live_event(
        MeetingSource::Microphone,
        4,
        Updated,
        (9_000, 9_800),
        "So anyway",
        None,
    );
    let entries = live::live_view(&Default::default(), [&system, &local, &alone]);
    assert_eq!(
        shown(&entries),
        [
            (MeetingSource::System, "Basically, I want"),
            (MeetingSource::Microphone, "Wait"),
            (MeetingSource::Microphone, "So anyway"),
        ]
    );
}

#[test]
fn live_view_keeps_rolling_microphone_entries_that_rolling_already_judged() {
    let entry = |source, start_ms, end_ms, text: &str| TranscriptEntry {
        source,
        start_ms,
        end_ms,
        text: text.into(),
    };
    // Rolling kept this double-talk line on signal evidence; the live view
    // has no signal for it and must not re-judge it by words.
    let rolling = [
        entry(MeetingSource::System, 0, 2_000, "Yeah, totally."),
        entry(MeetingSource::Microphone, 50, 2_000, "Yeah, totally."),
    ];
    let rolling = transcript::RollingView {
        entries: rolling.to_vec(),
        ..Default::default()
    };
    let entries = live::live_view(&rolling, std::iter::empty());
    assert_eq!(entries.len(), 2);
}

fn finished_meeting(name: &str, title: &str) -> (PathBuf, PathBuf) {
    let base = std::env::temp_dir().join(format!(
        "voice-control-{name}-{}-{}",
        std::process::id(),
        now_ms()
    ));
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
    let failed = cursor.consume_entries("meeting-a", TranscriptPublication::Live, &initial, |_| {
        Err(eyre!("clipboard unavailable"))
    });
    assert!(failed.is_err());
    assert_eq!(cursor.pending_turn_count("meeting-a", &initial), 2);
    assert_eq!(cursor.publication_for("meeting-a"), None);

    // A successful copy/paste commits the seen entries and drops the count to 0.
    let mut copied = String::new();
    let text = cursor
        .consume_entries(
            "meeting-a",
            TranscriptPublication::Live,
            &initial,
            |delta| {
                copied = delta.to_string();
                Ok(())
            },
        )
        .unwrap();
    assert_eq!(text, "You: First. Still me.\n\nThem: Got it.");
    assert_eq!(copied, text);
    assert_eq!(cursor.pending_turn_count("meeting-a", &initial), 0);
    assert_eq!(
        cursor.publication_for("meeting-a"),
        Some(TranscriptPublication::Live)
    );
    assert!(
        cursor
            .consume_entries("meeting-a", TranscriptPublication::Live, &initial, |_| Ok(
                ()
            ))
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
        .consume_entries("meeting-a", TranscriptPublication::Live, &extended, |_| {
            Ok(())
        })
        .unwrap();
    assert_eq!(delta, "You: Next point.");
    assert_eq!(cursor.pending_turn_count("meeting-a", &extended), 0);

    // Switching to a new meeting resets the seen set cleanly.
    assert_eq!(cursor.pending_turn_count("meeting-b", &initial), 2);
}
