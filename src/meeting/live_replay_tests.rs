//! Opt-in real-time replay of a recorded call through the production meeting
//! pipeline beside the warm dictation engine, measuring what an in-call copy
//! contains and what it costs. Recordings are private and stay outside the
//! repository; never run it against a live data folder:
//!
//! ```sh
//! HEX_APPLICATION_SUPPORT_DIR=<scratch> HEX_MEETING_DIR=<recorded meeting> \
//!   HEX_MEETING_MODELS=<folder with the selected GGUF> HEX_MEETING_REPORT=<dir> \
//!   HEX_MEETING_SECONDS=600 cargo test meeting_live_replay -- --ignored --nocapture
//! ```
//!
//! Both tracks are fed in 10 ms packets at real time, on the recording's
//! timeline, into the production writer and rolling worker; a dictation of
//! 4 s of remote speech runs through a live stream every 30 s. Every 500 ms it reads what the
//! Meetings pane and the copy paths would show, and every 60 s it consumes
//! "Copy new" through the shared paste cursor. After Stop it publishes the
//! final transcript and consumes once more. The reference for word error
//! rates is the recording's own `transcript.ndjson`.

use std::collections::HashMap;
use std::sync::Arc;
use std::sync::atomic::AtomicBool;
use std::thread;
use std::time::{Duration, Instant};

use super::*;
use crate::transcription::engine::{Offline, TranscriptionEngine};
use crate::transcription::models::TranscriptionSelection;
use crate::transcription::{Transcriber, WarmTranscriber};

const PACKET_MS: u64 = 10;
const DICTATION_EVERY: Duration = Duration::from_secs(30);
const DICTATION_SECONDS: usize = 4;
const POLL: Duration = Duration::from_millis(500);
const COPY_NEW_EVERY: Duration = Duration::from_secs(60);

fn env(name: &str) -> String {
    std::env::var(name).unwrap_or_else(|_| panic!("{name} is required"))
}

fn footprint_mb() -> f64 {
    let mut info = unsafe { std::mem::zeroed::<libc::rusage_info_v4>() };
    let status = unsafe {
        libc::proc_pid_rusage(
            std::process::id() as i32,
            libc::RUSAGE_INFO_V4,
            (&mut info as *mut libc::rusage_info_v4).cast(),
        )
    };
    assert_eq!(status, 0);
    info.ri_phys_footprint as f64 / 1_048_576.0
}

fn cpu_seconds() -> f64 {
    let mut usage = unsafe { std::mem::zeroed::<libc::rusage>() };
    unsafe { libc::getrusage(libc::RUSAGE_SELF, &mut usage) };
    let seconds = |time: libc::timeval| time.tv_sec as f64 + time.tv_usec as f64 / 1e6;
    seconds(usage.ru_utime) + seconds(usage.ru_stime)
}

fn words(text: &str) -> Vec<String> {
    text.split(|character: char| !character.is_alphanumeric() && character != '\'')
        .filter(|word| !word.is_empty())
        .map(str::to_lowercase)
        .collect()
}

/// Word error rate of `hypothesis` against `reference`, as (errors, words).
fn word_errors(reference: &[String], hypothesis: &[String]) -> (usize, usize) {
    let mut previous = (0..=hypothesis.len()).collect::<Vec<_>>();
    for (row, word) in reference.iter().enumerate() {
        let mut current = vec![row + 1; hypothesis.len() + 1];
        for (column, other) in hypothesis.iter().enumerate() {
            current[column + 1] = (previous[column] + usize::from(word != other))
                .min(previous[column + 1] + 1)
                .min(current[column] + 1);
        }
        previous = current;
    }
    (previous[hypothesis.len()], reference.len())
}

fn track_words(entries: &[TranscriptEntry], source: MeetingSource, through_ms: u64) -> Vec<String> {
    let mut entries = entries
        .iter()
        .filter(|entry| entry.source == source && entry.end_ms <= through_ms)
        .collect::<Vec<_>>();
    entries.sort_by_key(|entry| entry.start_ms);
    entries
        .iter()
        .flat_map(|entry| words(&entry.text))
        .collect()
}

/// Word error rate per source of `entries` against `reference`, each limited
/// to what `entries` covers of that source.
fn compare(reference: &[TranscriptEntry], entries: &[TranscriptEntry]) -> String {
    let mut parts = Vec::new();
    let (mut errors, mut total) = (0, 0);
    for source in [MeetingSource::System, MeetingSource::Microphone] {
        let through = entries
            .iter()
            .filter(|entry| entry.source == source)
            .map(|entry| entry.end_ms)
            .max()
            .unwrap_or_default();
        let (wrong, count) = word_errors(
            &track_words(reference, source, through + 1_000),
            &track_words(entries, source, u64::MAX),
        );
        errors += wrong;
        total += count;
        parts.push(format!(
            "{} {:.1}% of {count} through {}s",
            source.label(),
            wrong as f64 * 100.0 / count.max(1) as f64,
            through / 1_000
        ));
    }
    format!(
        "WER {:.1}% ({})",
        errors as f64 * 100.0 / total.max(1) as f64,
        parts.join(", ")
    )
}

fn percentile(values: &mut [f64], at: f64) -> f64 {
    if values.is_empty() {
        return f64::NAN;
    }
    values.sort_by(f64::total_cmp);
    values[((values.len() - 1) as f64 * at).round() as usize]
}

struct Track {
    source: MeetingSource,
    reader: hound::WavReader<std::io::BufReader<File>>,
    rate: u32,
    /// Replay offset of the track's first sample.
    offset_ms: u64,
    sent: u64,
}

#[test]
#[ignore = "requires HEX_APPLICATION_SUPPORT_DIR, HEX_MEETING_DIR, HEX_MEETING_MODELS and HEX_MEETING_REPORT"]
fn meeting_live_replay() {
    let source_dir = PathBuf::from(env("HEX_MEETING_DIR"));
    let report = PathBuf::from(env("HEX_MEETING_REPORT"));
    let seconds: u64 =
        std::env::var("HEX_MEETING_SECONDS").map_or(600, |value| value.parse().unwrap());
    let support = crate::app_paths::support_dir().unwrap();
    assert!(
        std::env::var_os("HEX_APPLICATION_SUPPORT_DIR").is_some(),
        "isolate the data folder"
    );
    fs::create_dir_all(&report).unwrap();
    fs::create_dir_all(&support).unwrap();
    let models = support.join("models");
    if !models.exists() {
        std::os::unix::fs::symlink(env("HEX_MEETING_MODELS"), &models).unwrap();
    }

    let selection = TranscriptionSelection::default();
    let started_loading = Instant::now();
    let model = Transcriber::load_selection(&selection).unwrap();
    let engine = Arc::new(TranscriptionEngine::start(WarmTranscriber::serving(model)).unwrap());
    engine.share();
    println!("engine ready in {:?}", started_loading.elapsed());
    let offline = |selection: TranscriptionSelection| -> Offline {
        Box::new(move |model, samples| {
            model
                .activate(&selection)
                .and_then(|model| model.transcribe(samples))
                .map_err(|error| error.to_string())
        })
    };

    let source_manifest = read_manifest(&source_dir).unwrap();
    let reference: Vec<TranscriptEntry> =
        read_ndjson(&source_dir.join("transcript.ndjson")).unwrap();
    let origin_us = [
        source_manifest.system.first_arrival_us,
        source_manifest.microphone.first_arrival_us,
    ]
    .into_iter()
    .flatten()
    .min()
    .unwrap_or_default();
    let open = |source, track: &TrackManifest| {
        let reader = hound::WavReader::open(source_dir.join(&track.file)).unwrap();
        Track {
            source,
            rate: reader.spec().sample_rate,
            reader,
            offset_ms: track
                .first_arrival_us
                .unwrap_or(origin_us)
                .saturating_sub(origin_us)
                / 1_000,
            sent: 0,
        }
    };
    let mut tracks = [
        open(MeetingSource::System, &source_manifest.system),
        open(MeetingSource::Microphone, &source_manifest.microphone),
    ];
    let dictation_clip = {
        let mut reader = hound::WavReader::open(source_dir.join("system.wav")).unwrap();
        let rate = reader.spec().sample_rate;
        reader.seek(rate * 5).unwrap();
        let raw = reader
            .samples::<f32>()
            .take(rate as usize * DICTATION_SECONDS)
            .map(Result::unwrap)
            .collect::<Vec<_>>();
        crate::dictation::resample_for_parakeet(&raw, rate)
    };
    // Warm the engine with the clip size dictation uses.
    engine
        .finish_with(None, dictation_clip.clone(), offline(selection.clone()))
        .unwrap()
        .wait()
        .unwrap();

    let created_at_ms = now_ms();
    let id = format!("{created_at_ms}-{}", std::process::id());
    let directory = root().unwrap().join(&id);
    fs::create_dir_all(&directory).unwrap();
    let _active = ActiveMeeting::acquire(&directory).unwrap();
    let mut manifest = MeetingManifest {
        schema_version: 1,
        id: id.clone(),
        title: "Replay".into(),
        status: MeetingStatus::Recording,
        created_at_ms,
        ended_at_ms: None,
        duration_ms: None,
        system: TrackManifest {
            file: "system.wav".into(),
            ..TrackManifest::default()
        },
        microphone: TrackManifest {
            file: "microphone.wav".into(),
            ..TrackManifest::default()
        },
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
        retention_applies: false,
        export_sha256: None,
    };
    write_manifest(&directory, &manifest).unwrap();

    let idle_footprint = footprint_mb();
    let cpu_before = cpu_seconds();
    let (sender, receiver) = mpsc::sync_channel::<AudioPacket>(128);
    let pipeline = Pipeline::start(&directory, receiver, &selection);

    let clock = Instant::now();
    let stop = Arc::new(AtomicBool::new(false));
    let sampler = {
        let stop = stop.clone();
        thread::spawn(move || {
            let mut samples = Vec::new();
            while !stop.load(Ordering::Relaxed) {
                samples.push(footprint_mb());
                thread::sleep(Duration::from_secs(1));
            }
            samples
        })
    };
    let dictations = {
        let stop = stop.clone();
        let engine = engine.clone();
        let clip = dictation_clip.clone();
        let selection = selection.clone();
        thread::spawn(move || {
            let mut latencies = Vec::new();
            let mut next = DICTATION_EVERY;
            while !stop.load(Ordering::Relaxed) {
                if clock.elapsed() < next {
                    thread::sleep(Duration::from_millis(50));
                    continue;
                }
                next += DICTATION_EVERY;
                let marker = crate::audio::recording_environment::CaptureMarker::new();
                let mut stream = engine.begin();
                let feeding = Instant::now();
                let held = clip.len().saturating_sub(3_200);
                for (index, block) in clip[..held].chunks(2_560).enumerate() {
                    let due = Duration::from_secs_f64(index as f64 * 0.16);
                    thread::sleep(due.saturating_sub(feeding.elapsed()));
                    if let Some(stream) = &mut stream {
                        stream.feed(block);
                    }
                }
                thread::sleep(Duration::from_millis(200));
                let released = Instant::now();
                let pending = engine
                    .finish_with(stream, clip.clone(), offline(selection.clone()))
                    .unwrap();
                drop(marker);
                pending.wait().unwrap();
                latencies.push(released.elapsed().as_secs_f64() * 1_000.0);
            }
            latencies
        })
    };

    let mut first_seen_view: HashMap<(MeetingSource, u64, String), f64> = HashMap::new();
    let mut first_seen_rolling: HashMap<(MeetingSource, u64, String), f64> = HashMap::new();
    let mut pasted = String::new();
    let mut next_poll = Duration::ZERO;
    let mut next_copy = COPY_NEW_EVERY;
    let mut mid_copy = None;
    let mid = Duration::from_secs(seconds / 2);
    let mut reader = TranscriptReader::default();
    let rolling_path = directory.join(transcript::ROLLING_FILE);
    loop {
        let now_ms = clock.elapsed().as_millis() as u64;
        if now_ms >= seconds * 1_000 {
            break;
        }
        for track in &mut tracks {
            let rate = u64::from(track.rate);
            let due = now_ms.saturating_sub(track.offset_ms) * rate / 1_000;
            while track.sent + rate * PACKET_MS / 1_000 <= due {
                let samples = track
                    .reader
                    .samples::<f32>()
                    .take((rate * PACKET_MS / 1_000) as usize)
                    .map(Result::unwrap)
                    .collect::<Vec<_>>();
                if samples.is_empty() {
                    break;
                }
                track.sent += samples.len() as u64;
                sender
                    .send(AudioPacket {
                        source: track.source,
                        pts_us: None,
                        arrival_us: clock.elapsed().as_micros() as u64,
                        position: 0,
                        sample_rate: track.rate,
                        samples,
                    })
                    .unwrap();
            }
        }
        let elapsed = clock.elapsed();
        if elapsed >= next_poll {
            next_poll += POLL;
            let at = elapsed.as_secs_f64() * 1_000.0;
            if let Ok(entries) = reader.read(&id) {
                for entry in entries {
                    first_seen_view
                        .entry((entry.source, entry.start_ms, entry.text.clone()))
                        .or_insert(at - entry.end_ms as f64);
                }
            }
            if let Ok(lines) = read_ndjson::<transcript::RollingLine>(&rolling_path) {
                let view: transcript::RollingView = lines.into_iter().collect();
                for entry in view.entries {
                    first_seen_rolling
                        .entry((entry.source, entry.start_ms, entry.text.clone()))
                        .or_insert(at - entry.end_ms as f64);
                }
            }
            if elapsed >= next_copy {
                next_copy += COPY_NEW_EVERY;
                let _ = consume_delta_for_id(&id, |text| {
                    pasted.push_str(text);
                    pasted.push_str("\n\n");
                    Ok(())
                });
            }
            if mid_copy.is_none() && elapsed >= mid {
                let all = TranscriptReader::default().read(&id).unwrap_or_default();
                let new = TranscriptReader::default().read(&id).unwrap_or_default();
                mid_copy = Some((elapsed, all, new));
            }
        }
        thread::sleep(Duration::from_millis(PACKET_MS / 2));
    }
    let late_all = TranscriptReader::default().read(&id).unwrap_or_default();
    let stopped = Instant::now();
    drop(sender);
    stop.store(true, Ordering::Relaxed);
    let latencies = dictations.join().unwrap();
    let (stats, rolling) = pipeline.finish();
    manifest.system = stats.system;
    manifest.microphone = stats.microphone;
    let (final_entries, _) =
        transcript::transcribe(&directory, &manifest, &selection, rolling).unwrap();
    let final_ms = stopped.elapsed().as_millis();
    let cpu = cpu_seconds() - cpu_before;
    let wall = clock.elapsed().as_secs_f64();
    let footprints = sampler.join().unwrap();
    {
        let path = directory.join("transcript.ndjson");
        let mut file = BufWriter::new(File::create(&path).unwrap());
        for entry in &final_entries {
            serde_json::to_writer(&mut file, entry).unwrap();
            file.write_all(b"\n").unwrap();
        }
    }
    // Readers treat the transcript as published once both files exist.
    fs::write(
        directory.join("transcript.md"),
        transcript_markdown(&final_entries),
    )
    .unwrap();
    let _ = consume_delta_for_id(&id, |text| {
        pasted.push_str(text);
        Ok(())
    });

    let through = seconds * 1_000;
    let pasted_entries = pasted
        .split("\n\n")
        .filter_map(|turn| turn.split_once(": "))
        .flat_map(|(label, text)| {
            let source = if label == "You" {
                MeetingSource::Microphone
            } else {
                MeetingSource::System
            };
            words(text).into_iter().map(move |word| (source, word))
        })
        .collect::<Vec<_>>();
    let pasted_words = |source| {
        pasted_entries
            .iter()
            .filter(|(entry_source, _)| *entry_source == source)
            .map(|(_, word)| word.clone())
            .collect::<Vec<_>>()
    };
    let final_words = |source| track_words(&final_entries, source, u64::MAX);
    let (mid_at, mid_all, mid_new) = mid_copy.unwrap();
    let mut view_lags = first_seen_view.values().copied().collect::<Vec<_>>();
    let mut rolling_lags = first_seen_rolling.values().copied().collect::<Vec<_>>();
    let mut latencies = latencies;
    let peak = footprints.iter().copied().fold(0.0, f64::max);
    let mean = footprints.iter().sum::<f64>() / footprints.len().max(1) as f64;
    let mut lines = vec![
        format!(
            "replayed {seconds} s; CPU {:.1}% of a core",
            cpu * 100.0 / wall
        ),
        format!(
            "footprint idle {idle_footprint:.0} MB, meeting mean {mean:.0} MB, peak {peak:.0} MB"
        ),
        format!(
            "dictation release->text (n={}): p50 {:.0} ms, p95 {:.0} ms, max {:.0} ms",
            latencies.len(),
            percentile(&mut latencies, 0.5),
            percentile(&mut latencies, 0.95),
            percentile(&mut latencies, 1.0)
        ),
        format!(
            "pane entry lag after speech end: p50 {:.1} s, p95 {:.1} s (n={})",
            percentile(&mut view_lags, 0.5) / 1_000.0,
            percentile(&mut view_lags, 0.95) / 1_000.0,
            view_lags.len()
        ),
        format!(
            "Parakeet entry lag after speech end: p50 {:.1} s, p95 {:.1} s (n={})",
            percentile(&mut rolling_lags, 0.5) / 1_000.0,
            percentile(&mut rolling_lags, 0.95) / 1_000.0,
            rolling_lags.len()
        ),
        format!(
            "mid-call Copy all at {:.0} s: {}",
            mid_at.as_secs_f64(),
            compare(&reference, &mid_all)
        ),
        format!(
            "mid-call Copy new source at {:.0} s: {}",
            mid_at.as_secs_f64(),
            compare(&reference, &mid_new)
        ),
        format!(
            "Copy all just before Stop: {}",
            compare(&reference, &late_all)
        ),
        format!(
            "final ({final_ms} ms after Stop): {}",
            compare(&reference, &final_entries)
        ),
    ];
    for source in [MeetingSource::System, MeetingSource::Microphone] {
        let reference_words = track_words(&reference, source, through);
        let (wrong, count) = word_errors(&final_words(source), &pasted_words(source));
        let (reference_wrong, _) = word_errors(&reference_words, &pasted_words(source));
        lines.push(format!(
            "Copy new every 60 s + after Stop, {}: {} words pasted, final has {}; vs final {:.1}%, vs reference {:.1}%",
            source.label(),
            pasted_words(source).len(),
            count,
            wrong as f64 * 100.0 / count.max(1) as f64,
            reference_wrong as f64 * 100.0 / reference_words.len().max(1) as f64,
        ));
    }
    let summary = lines.join("\n");
    println!("{summary}");
    fs::write(report.join("summary.txt"), &summary).unwrap();
    fs::write(report.join("pasted.txt"), &pasted).unwrap();
    fs::write(
        report.join("mid-copy-all.md"),
        transcript_markdown(&mid_all),
    )
    .unwrap();
    fs::write(report.join("final.md"), transcript_markdown(&final_entries)).unwrap();
    fs::write(
        report.join("reference.md"),
        transcript_markdown(
            &reference
                .iter()
                .filter(|entry| entry.end_ms <= through)
                .cloned()
                .collect::<Vec<_>>(),
        ),
    )
    .unwrap();
    let _ = fs::remove_dir_all(&directory);
}

/// The recording pipeline after capture: writer and rolling.
struct Pipeline {
    writer: thread::JoinHandle<Result<CaptureStats>>,
    rolling: transcript::Rolling,
}

impl Pipeline {
    fn start(
        directory: &Path,
        receiver: Receiver<AudioPacket>,
        selection: &TranscriptionSelection,
    ) -> Self {
        let mut rolling = transcript::Rolling::start(directory.to_path_buf(), selection.clone());
        let feed = rolling.feed();
        let writer = {
            let directory = directory.to_path_buf();
            thread::spawn(move || write_tracks(&directory, receiver, feed))
        };
        Self { writer, rolling }
    }

    fn finish(self) -> (CaptureStats, Option<transcript::RollingOutcome>) {
        let stats = self.writer.join().unwrap().unwrap();
        (stats, self.rolling.finish())
    }
}
