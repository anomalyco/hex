use super::*;
use crate::events::TranscriptPhase;

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
    let packet = AudioPacket {
        source: MeetingSource::System,
        pts_us: None,
        arrival_us: 1_200_000,
        sample_rate: 16_000,
        samples: vec![0.0; 1_600],
    };
    assert_eq!(
        live::audio_gap(None, Some(1_000_000), &packet),
        Some(live::AudioGap {
            silence_samples: 3_200,
            skipped_us: 0,
        })
    );
    assert_eq!(live::source_offset([Some(500_000), Some(700_000)], 0), 0);
    assert_eq!(
        live::source_offset([Some(500_000), Some(700_000)], 1),
        200_000
    );
}

#[test]
fn live_gaps_prefer_media_time_and_bound_synthesized_silence() {
    let contiguous = AudioPacket {
        source: MeetingSource::Microphone,
        pts_us: Some(1_200_000),
        arrival_us: 2_000_000,
        sample_rate: 44_100,
        samples: vec![0.0; 8_820],
    };
    assert_eq!(
        live::audio_gap(Some(1_200_000), Some(1_000_000), &contiguous),
        None
    );

    let delayed = AudioPacket {
        pts_us: Some(3_200_000),
        ..contiguous
    };
    assert_eq!(
        live::audio_gap(Some(1_200_000), Some(1_000_000), &delayed),
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
