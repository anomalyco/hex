use super::*;

#[test]
fn normalizes_spoken_numbers_and_punctuation() {
    assert_eq!(normalize_spoken("Move LEFT, three!"), "move left 3");
    assert_eq!(normalize_spoken("Ä ä A a ONE one"), "Ä ä a a 1 1");
}

#[test]
fn a_chunks_times_move_onto_the_clips_timeline_by_sample_position() {
    // Offsets come from where a range starts, never from how many chunks
    // produced text.
    assert_eq!(sample_offset_ms(0), 0);
    assert_eq!(sample_offset_ms(2 * 30 * 16_000), 60_000);
    assert_eq!(sample_offset_ms(1_600), 100);

    let shifted = timed_spans(
        transcribe_cpp::Transcript {
            text: "hello".into(),
            segments: vec![transcribe_cpp::Segment {
                text: " hello ".into(),
                t0_ms: 0,
                t1_ms: 480,
                ..Default::default()
            }],
            words: vec![
                transcribe_cpp::Word {
                    text: "hello".into(),
                    t0_ms: 10,
                    t1_ms: 480,
                    ..Default::default()
                },
                transcribe_cpp::Word {
                    text: "  ".into(),
                    t0_ms: 500,
                    t1_ms: 600,
                    ..Default::default()
                },
            ],
            ..Default::default()
        },
        sample_offset_ms(2 * 30 * 16_000),
    );
    assert_eq!(shifted.segments.len(), 1);
    assert_eq!(
        (
            shifted.segments[0].start_ms,
            shifted.segments[0].end_ms,
            shifted.segments[0].text.as_str()
        ),
        (60_000, 60_480, "hello")
    );
    assert_eq!(shifted.words.len(), 1, "an empty word is dropped");
    assert_eq!(
        (shifted.words[0].start_ms, shifted.words[0].end_ms),
        (60_010, 60_480)
    );
}

#[test]
fn long_clip_ranges_cover_every_sample_once_within_the_limit() {
    let samples = vec![0.2_f32; 3 * 16_000 + 123];
    let max_samples = 16_000;
    let ranges = chunk_ranges(&samples, max_samples);
    assert_eq!(ranges.first().map(|range| range.start), Some(0));
    assert_eq!(ranges.last().map(|range| range.end), Some(samples.len()));
    for pair in ranges.windows(2) {
        assert_eq!(pair[0].end, pair[1].start, "the ranges are contiguous");
    }
    for range in &ranges {
        assert!(!range.is_empty(), "{range:?}");
        assert!(range.len() <= max_samples, "{range:?}");
    }
    assert_eq!(
        ranges.iter().map(|range| range.len()).sum::<usize>(),
        samples.len()
    );
}

#[test]
fn a_long_clip_is_cut_inside_a_silence_near_the_window_end() {
    // Cohere's 35 s window, and a silent second inside the 5 s a cut is
    // searched for at the end of the first window.
    let max_samples = 35 * 16_000;
    let mut samples = vec![0.25_f32; 44 * 16_000];
    let gap = 31 * 16_000..32 * 16_000;
    samples[gap.clone()].fill(0.0);
    let ranges = chunk_ranges(&samples, max_samples);
    assert_eq!(ranges.len(), 2);
    let cut = ranges[0].end;
    assert!(gap.contains(&cut), "cut at {cut} is outside {gap:?}");
    assert_eq!(ranges[1].start, cut);
}

#[test]
fn a_clip_without_silence_still_cuts_in_the_search_region() {
    let max_samples = 35 * 16_000;
    let samples = vec![0.25_f32; 40 * 16_000];
    let ranges = chunk_ranges(&samples, max_samples);
    assert_eq!(ranges.len(), 2);
    let cut = ranges[0].end;
    assert!(
        (max_samples - QUIET_SEARCH_SAMPLES..max_samples).contains(&cut),
        "cut at {cut} is outside the last 5 s of the first window"
    );
    assert_eq!(
        cut,
        max_samples - QUIET_SPAN_SAMPLES / 2,
        "every span is equally quiet, so the latest one wins"
    );
    assert!(ranges[1].len() <= max_samples);
}

#[test]
fn a_clip_that_fits_one_window_is_not_cut() {
    let samples = vec![0.1_f32; 16_000];
    assert_eq!(chunk_ranges(&samples, 30 * 16_000), vec![0..16_000]);
    assert_eq!(chunk_ranges(&samples, 16_000), vec![0..16_000]);
    assert!(chunk_ranges(&[], 16_000).is_empty());
}

#[test]
fn a_models_timestamp_kind_names_its_finest_level() {
    for (kind, level) in [
        (transcribe_cpp::TimestampKind::Word, TimestampLevel::Word),
        (transcribe_cpp::TimestampKind::Token, TimestampLevel::Word),
        (
            transcribe_cpp::TimestampKind::Segment,
            TimestampLevel::Segment,
        ),
        (transcribe_cpp::TimestampKind::Auto, TimestampLevel::Segment),
        (transcribe_cpp::TimestampKind::None, TimestampLevel::None),
    ] {
        assert_eq!(timestamp_level(kind), level, "{kind:?}");
    }
}

#[test]
fn only_a_completed_run_inside_the_window_counts_as_recent() {
    let now = Instant::now();
    let window = Duration::from_secs(60);
    assert!(!ran_within(None, now, window), "a fresh load has not run");
    assert!(ran_within(Some(now), now, window));
    assert!(ran_within(Some(now - Duration::from_secs(59)), now, window));
    assert!(!ran_within(
        Some(now - Duration::from_secs(60)),
        now,
        window
    ));
}

#[test]
fn a_decode_stopped_for_repeating_keeps_its_text_and_truncation_still_fails() {
    let transcript = |text: &str| {
        Some(Box::new(transcribe_cpp::Transcript {
            text: text.into(),
            ..Default::default()
        }))
    };
    let repeated = keep_repetition_partial(Err(transcribe_cpp::Error::OutputRepetition {
        message: "loop".into(),
        partial: transcript("send it now"),
    }));
    assert_eq!(repeated.unwrap().text, "send it now");
    let truncated = keep_repetition_partial(Err(transcribe_cpp::Error::OutputTruncated {
        message: "budget".into(),
        partial: transcript("send it"),
    }));
    assert!(matches!(
        truncated,
        Err(transcribe_cpp::Error::OutputTruncated { .. })
    ));
}

#[test]
fn a_warmup_leaves_the_vocabulary_out_and_counts_when_truncated() {
    assert!(
        RunKind::Transcript.takes_vocabulary(),
        "a transcript still gets the vocabulary"
    );
    assert!(
        !RunKind::Warmup.takes_vocabulary(),
        "silence plus a term list makes Qwen3-ASR read the list back (#1568)"
    );
    assert!(!RunKind::Transcript.counts_when_truncated());
    assert!(RunKind::Warmup.counts_when_truncated());
}

#[test]
fn a_decode_stopped_at_its_output_limit_is_recognised_through_its_context() {
    let truncated: Result<()> = Err(transcribe_cpp::Error::OutputTruncated {
        message: "budget".into(),
        partial: None,
    })
    .wrap_err("transcription failed");
    assert!(output_truncated(&truncated.unwrap_err()));
    let repeating: Result<()> = Err(transcribe_cpp::Error::OutputRepetition {
        message: "loop".into(),
        partial: None,
    })
    .wrap_err("transcription failed");
    assert!(!output_truncated(&repeating.unwrap_err()));
}

#[test]
fn a_whisper_clip_is_trimmed_to_its_speech_plus_a_margin() {
    // 10 s clip, speech from 5 s to 9 s.
    assert_eq!(
        whisper_span(160_000, "whisper", Some(80_000..144_000)),
        72_000..152_000
    );
    // Speech up to an edge keeps that edge.
    assert_eq!(
        whisper_span(160_000, "whisper", Some(0..100_000)),
        0..108_000
    );
    assert_eq!(
        whisper_span(160_000, "whisper", Some(60_000..160_000)),
        52_000..160_000
    );
    // Only Whisper trims, and only with speech bounds from the detector.
    assert_eq!(
        whisper_span(160_000, "cohere_asr", Some(80_000..144_000)),
        0..160_000
    );
    assert_eq!(whisper_span(160_000, "whisper", None), 0..160_000);
}

#[test]
fn a_whisper_clip_is_trimmed_and_padded_to_a_chunks_minimum() {
    // 10 s clip, speech from 9.0 s to 9.2 s. The 500 ms margin leaves
    // 1.2 s, which every chunk pads to the 1.5 s minimum.
    let samples = vec![0.25_f32; 160_000];
    let (audio, start) = trimmed_audio(&samples, "whisper", Some(144_000..147_200));
    assert_eq!(start, 136_000);
    assert_eq!(audio.len(), 24_000);
    assert!(audio[..19_200].iter().all(|&sample| sample == 0.25));
    assert!(audio[19_200..].iter().all(|&sample| sample == 0.0));
    // Another model, no detector, or speech reaching both edges: the clip
    // is borrowed unchanged, with no copy or padding.
    for (architecture, bounds) in [
        ("cohere_asr", Some(144_000..147_200)),
        ("whisper", None),
        ("whisper", Some(0..160_000)),
    ] {
        let label = format!("{architecture} {bounds:?}");
        let (audio, start) = trimmed_audio(&samples, architecture, bounds);
        assert!(matches!(audio, Cow::Borrowed(_)), "{label}");
        assert_eq!((start, audio.len()), (0, samples.len()));
    }
}

#[test]
fn a_trimmed_chunks_times_stay_on_the_clips_timeline() {
    // `transcribe_chunked_timed` asks `chunk_offset_ms` for the whole clip
    // and for each chunk of the trimmed audio; this clip starts 8.5 s in,
    // so its first chunk reports 8.5 s. The end-to-end timed run needs a
    // model and is not run here.
    let samples = vec![0.1_f32; 160_000];
    let (audio, start) = trimmed_audio(&samples, "whisper", Some(144_000..147_200));
    assert_eq!(start, 136_000);
    assert_eq!(chunk_offset_ms(start, 0), 8_500);
    let ranges = chunk_ranges(&audio, 16_000);
    assert_eq!(ranges.len(), 2, "the padded clip is cut into two windows");
    for range in &ranges {
        assert_eq!(
            chunk_offset_ms(start, range.start),
            8_500 + sample_offset_ms(range.start),
            "{range:?}"
        );
    }
}

#[test]
fn a_long_clip_skips_only_chunks_the_detector_hears_no_speech_in() {
    use crate::transcription::speech_detector::override_for_test;
    let chunk = [0.0; 16_000];
    {
        let _silent = override_for_test(Some(false));
        assert!(!chunk_may_hold_speech(&chunk));
    }
    {
        let _speech = override_for_test(Some(true));
        assert!(chunk_may_hold_speech(&chunk));
    }
    let _unavailable = override_for_test(None);
    assert!(
        chunk_may_hold_speech(&chunk),
        "no detector keeps every chunk"
    );
}

#[test]
fn gpu_sessions_bound_decoder_threads_and_cpu_sessions_keep_library_default() {
    use transcribe_cpp::DeviceType;
    assert_eq!(session_options(DeviceType::Gpu).n_threads, 1);
    assert_eq!(session_options(DeviceType::Igpu).n_threads, 1);
    assert_eq!(session_options(DeviceType::Cpu).n_threads, 0);
    assert_eq!(session_options(DeviceType::Accel).n_threads, 0);
}

fn segment(text: &str, first_word: i32, n_words: i32) -> transcribe_cpp::Segment {
    transcribe_cpp::Segment {
        text: text.into(),
        first_word,
        n_words,
        ..Default::default()
    }
}

fn transcript(segments: &[&str]) -> transcribe_cpp::Transcript {
    let mut transcript = transcribe_cpp::Transcript::default();
    let mut first_word = 0;
    for text in segments {
        let n_words = text.split_whitespace().count() as i32;
        transcript.segments.push(segment(text, first_word, n_words));
        for word in text.split_whitespace() {
            transcript.words.push(transcribe_cpp::Word {
                text: word.into(),
                ..Default::default()
            });
        }
        first_word += n_words;
    }
    transcript.text = segments.join(" ");
    transcript
}

fn words(transcript: &transcribe_cpp::Transcript) -> Vec<&str> {
    transcript
        .words
        .iter()
        .map(|word| word.text.as_str())
        .collect()
}

#[test]
fn prompt_echo_is_dropped() {
    let mut echo = transcript(&["T3 Code, Ottobit."]);
    drop_prompt_echo(&mut echo, "t3 code, ottobit");
    assert_eq!(echo.text, "");
    assert!(echo.segments.is_empty());
    assert!(echo.words.is_empty());

    let mut unsegmented = transcribe_cpp::Transcript {
        text: " T3 Code, Ottobit. ".into(),
        ..Default::default()
    };
    drop_prompt_echo(&mut unsegmented, "T3 Code, Ottobit");
    assert_eq!(unsegmented.text, "");
}

#[test]
fn speech_containing_the_hints_is_kept() {
    let mut speech = transcript(&["We shipped T3 Code today"]);
    let before = speech.clone();
    drop_prompt_echo(&mut speech, "T3 Code");
    assert_eq!(speech, before);
}

#[test]
fn only_the_echo_segment_is_removed() {
    let mut mixed = transcript(&["Let's ship it.", "T3 Code, Ottobit."]);
    drop_prompt_echo(&mut mixed, "T3 Code, Ottobit");
    assert_eq!(mixed.text, "Let's ship it.");
    assert_eq!(mixed.segments.len(), 1);
    assert_eq!(words(&mixed), ["Let's", "ship", "it."]);

    let mut leading = transcript(&["T3 Code, Ottobit.", "Let's ship it."]);
    drop_prompt_echo(&mut leading, "T3 Code, Ottobit");
    assert_eq!(leading.text, "Let's ship it.");
    assert_eq!(words(&leading), ["Let's", "ship", "it."]);
    assert_eq!(
        (leading.segments[0].first_word, leading.segments[0].n_words),
        (0, 3)
    );
}

#[test]
fn single_hint_dictation_is_kept() {
    let mut dictation = transcript(&["Ottobit."]);
    let before = dictation.clone();
    drop_prompt_echo(&mut dictation, "Ottobit");
    assert_eq!(dictation, before);

    let mut spaced = transcript(&["T3 Code Ottobit"]);
    let before = spaced.clone();
    drop_prompt_echo(&mut spaced, "T3 Code Ottobit");
    assert_eq!(spaced, before);
}

#[test]
fn hints_without_spaces_after_commas_are_matched() {
    let mut echo = transcript(&["T3 Code, Ottobit."]);
    drop_prompt_echo(&mut echo, "T3 Code,Ottobit");
    assert_eq!(echo.text, "");

    let mut joined = transcript(&["T3 Code;Ottobit"]);
    drop_prompt_echo(&mut joined, "T3 Code\nOttobit");
    assert_eq!(joined.text, "");
}

#[test]
fn a_trailing_subtitle_credit_is_dropped_from_text_segments_and_words() {
    let mut own = transcript(&["On se voit demain.", "Sous-titrage Société Radio-Canada"]);
    drop_subtitle_credit(&mut own);
    assert_eq!(own.text, "On se voit demain.");
    assert_eq!(own.segments.len(), 1);
    assert_eq!(words(&own), ["On", "se", "voit", "demain."]);

    let mut joined = transcript(&["On se voit demain. Sous-titrage Société Radio-Canada"]);
    drop_subtitle_credit(&mut joined);
    assert_eq!(joined.text, "On se voit demain.");
    assert_eq!(joined.segments[0].text, "On se voit demain.");
    assert_eq!(joined.segments[0].n_words, 4);
    assert_eq!(words(&joined), ["On", "se", "voit", "demain."]);

    let mut speech = transcript(&["Sous-titrage Société Radio-Canada, c'est la fin."]);
    let before = speech.clone();
    drop_subtitle_credit(&mut speech);
    assert_eq!(speech, before);
}

#[test]
fn an_empty_prompt_drops_nothing() {
    let mut silence = transcribe_cpp::Transcript::default();
    drop_prompt_echo(&mut silence, "  ");
    assert_eq!(silence, transcribe_cpp::Transcript::default());
}

fn validate_auto(
    model: crate::transcription::models::TranscriptionModelId,
    supports_language_detect: bool,
) -> Result<Option<String>> {
    let definition = crate::transcription::models::definition(model);
    let ModelRuntime::Gguf(artifact) = &definition.runtime else {
        panic!("{} is not a GGUF model", definition.name);
    };
    validate_gguf_artifact(
        Path::new("catalog.gguf"),
        &definition,
        artifact.architecture,
        artifact.variant.as_ref(),
        &TranscriptionSelection {
            model,
            language: AUTO_LANGUAGE.into(),
        },
        supports_language_detect,
    )
}

#[test]
fn auto_needs_detection_only_from_models_that_take_a_language_hint() {
    use crate::transcription::models::TranscriptionModelId;

    // Parakeet v2 takes the language as a hint and its file detects none.
    assert!(validate_auto(TranscriptionModelId::ParakeetV2, false).is_err());
    // A no-hint model never receives Auto as a hint, so the file's
    // lang_detect metadata does not gate it (Granite's pinned file says 0).
    for model in [
        TranscriptionModelId::ParakeetV3,
        TranscriptionModelId::ParakeetUltra,
        TranscriptionModelId::Orukeet,
        TranscriptionModelId::FluidSpeechPro,
        TranscriptionModelId::GraniteSpeech5,
    ] {
        assert_eq!(validate_auto(model, false).unwrap(), None, "{model:?}");
    }
    // A hint-taking model that detects still runs Auto with no hint.
    assert_eq!(
        validate_auto(TranscriptionModelId::WhisperLargeV3Turbo, true).unwrap(),
        None
    );
}

#[test]
fn a_job_cancel_token_is_installed_only_inside_its_scope() {
    let outer = transcribe_cpp::CancelToken::new();
    let inner = transcribe_cpp::CancelToken::new();
    assert!(job_cancel().is_none());
    with_job_cancel(&outer, || {
        with_job_cancel(&inner, || {
            inner.cancel();
            assert!(job_cancel().unwrap().is_cancelled());
        });
        assert!(
            !job_cancel().unwrap().is_cancelled(),
            "the outer job is restored"
        );
    });
    assert!(job_cancel().is_none());
    let _ = std::panic::catch_unwind(|| with_job_cancel(&outer, || panic!("run failed")));
    assert!(
        job_cancel().is_none(),
        "a panicking run still uninstalls its token"
    );
}

#[test]
fn only_a_prompt_run_corrects_sounds_like_spellings() {
    for slot in [
        VocabularySlot::Prompt(500),
        VocabularySlot::Bias,
        VocabularySlot::None,
    ] {
        let expected = matches!(slot, VocabularySlot::Prompt(_));
        assert_eq!(
            corrects_sounds_like(slot, RunKind::Transcript),
            expected,
            "{slot:?}"
        );
        assert!(
            !corrects_sounds_like(slot, RunKind::Warmup),
            "a warm-up reads no vocabulary and corrects nothing"
        );
    }
}
