//! Opt-in replay of a recorded meeting through every transcript layer, to
//! measure echo suppression. Recordings are private and stay outside the
//! repository:
//!
//! ```sh
//! HEX_MEETING_DIR=<meeting directory> HEX_MEETING_CACHE=<scratch directory> \
//!   HEX_MEETING_MODEL=<Unified English GGUF> HEX_MEETING_LABELS=<labels> \
//!   cargo test meeting_echo_replay -- --ignored --nocapture
//! ```
//!
//! The first run transcribes both WAVs with Parakeet and replays them through
//! the live Moonshine worker at `HEX_MEETING_LIVE_SPEED` (default 4) times
//! real time, caching both in `HEX_MEETING_CACHE`; later runs only
//! re-evaluate echo suppression. Delete `live.ndjson` there after changing
//! the live worker. Each layer reports its `You` entries, how many
//! repeat nearby remote words (echo-like), and how many labeled local
//! utterances it lost. Labels are lines of `<start s> <end s> [note]` marking
//! where the user really spoke. `HEX_MEETING_WINDOW=<from s>-<to s>` prints
//! that part of each layer's turns.

use std::collections::{HashMap, HashSet};
use std::sync::atomic::AtomicU64;
use std::sync::mpsc;
use std::thread;
use std::time::{Duration, Instant};

use super::live::{Event, live_view};
use super::transcript::{
    ReplayChunk, RollingLine, RollingView, replay_envelope, replay_resample, replay_track,
    replay_transcribe, replay_views,
};
use super::*;
use crate::transcription::Transcriber;

const ECHO_LOOKAROUND_MS: u64 = 6_000;
const ECHO_OVERLAP: f32 = 0.6;
/// A labeled utterance counts as shown when a `You` entry overlaps it
/// within this margin.
const LABEL_MARGIN_MS: u64 = 500;
/// Live packets of 10 ms, as ScreenCaptureKit delivers the microphone.
const PACKET_MS: u64 = 10;

/// Float WAVs, including one still being recorded whose sizes are unset.
fn read_float_wav(path: &Path) -> (u32, Vec<f32>) {
    let bytes = fs::read(path).unwrap();
    assert_eq!(&bytes[..4], b"RIFF");
    let field = |at: usize| u32::from_le_bytes(bytes[at..at + 4].try_into().unwrap()) as usize;
    let mut offset = 12;
    let mut rate = 0;
    loop {
        let (id, size, body) = (&bytes[offset..offset + 4], field(offset + 4), offset + 8);
        if id == b"fmt " {
            assert_eq!(field(body) >> 16, 1, "mono audio");
            assert_eq!(field(body + 12) >> 16, 32, "32-bit float audio");
            rate = field(body + 4) as u32;
        }
        if id == b"data" {
            let end = if size == 0 || body + size > bytes.len() {
                bytes.len()
            } else {
                body + size
            };
            let samples = bytes[body..end]
                .as_chunks::<4>()
                .0
                .iter()
                .map(|sample| f32::from_le_bytes(*sample))
                .collect();
            return (rate, samples);
        }
        offset = body + size + (size & 1);
    }
}

fn words(text: &str) -> Vec<String> {
    text.split(|character: char| !character.is_alphanumeric() && character != '\'')
        .filter(|word| !word.is_empty())
        .map(str::to_lowercase)
        .collect()
}

/// The share of a microphone entry's words that remote speech nearby also
/// contains: a rough echo label for reviewing results, not a verdict.
fn remote_overlap(entry: &TranscriptEntry, remote: &[TranscriptEntry]) -> f32 {
    let spoken = words(&entry.text);
    let nearby = remote
        .iter()
        .filter(|other| {
            other.start_ms < entry.end_ms + ECHO_LOOKAROUND_MS
                && entry.start_ms < other.end_ms + ECHO_LOOKAROUND_MS
        })
        .flat_map(|other| words(&other.text))
        .collect::<HashSet<_>>();
    if spoken.is_empty() {
        return 0.0;
    }
    spoken.iter().filter(|word| nearby.contains(*word)).count() as f32 / spoken.len() as f32
}

fn clock(milliseconds: u64) -> String {
    format!(
        "{:02}:{:02}",
        milliseconds / 60_000,
        milliseconds / 1_000 % 60
    )
}

struct Labels(Vec<(u64, u64, String)>);

impl Labels {
    fn read() -> Self {
        let Ok(path) = std::env::var("HEX_MEETING_LABELS") else {
            return Self(Vec::new());
        };
        Self(
            fs::read_to_string(path)
                .unwrap()
                .lines()
                .filter(|line| !line.trim().is_empty() && !line.starts_with('#'))
                .map(|line| {
                    let mut fields = line.splitn(3, ' ');
                    let mut seconds = || {
                        (fields.next().unwrap().parse::<f64>().unwrap() * 1_000.0).round() as u64
                    };
                    let (start, end) = (seconds(), seconds());
                    (start, end, fields.next().unwrap_or_default().to_string())
                })
                .collect(),
        )
    }

    fn local(&self, entry: &TranscriptEntry) -> bool {
        self.0
            .iter()
            .any(|(start, end, _)| entry.start_ms < *end && *start < entry.end_ms)
    }
}

struct Score {
    you: usize,
    echo_like: usize,
    lost: Vec<String>,
}

/// Scores the `You` entries a layer showed against the remote reference
/// and the labeled local utterances.
fn score(entries: &[TranscriptEntry], remote: &[TranscriptEntry], labels: &Labels) -> Score {
    let microphone = entries
        .iter()
        .filter(|entry| entry.source == MeetingSource::Microphone)
        .collect::<Vec<_>>();
    let echo_like = microphone
        .iter()
        .filter(|entry| !labels.local(entry) && remote_overlap(entry, remote) >= ECHO_OVERLAP)
        .count();
    let lost = labels
        .0
        .iter()
        .filter(|(start, end, _)| {
            !microphone.iter().any(|entry| {
                entry.start_ms < end + LABEL_MARGIN_MS && *start < entry.end_ms + LABEL_MARGIN_MS
            })
        })
        .map(|(start, _, note)| format!("{} {note}", clock(*start)))
        .collect();
    Score {
        you: microphone.len(),
        echo_like,
        lost,
    }
}

fn report(
    label: &str,
    entries: &[TranscriptEntry],
    remote: &[TranscriptEntry],
    labels: &Labels,
    window: (u64, u64),
) {
    let score = score(entries, remote, labels);
    println!(
        "== {label}: {} You entries, {} echo-like, {}/{} labeled local lost {:?}",
        score.you,
        score.echo_like,
        score.lost.len(),
        labels.0.len(),
        score.lost
    );
    let mut entries = entries.to_vec();
    entries.sort_by_key(|entry| entry.start_ms);
    for turn in coalesce_transcript(entries) {
        if (window.0..window.1).contains(&turn.start_ms) {
            println!(
                "  {} {:<4} {}",
                clock(turn.start_ms),
                turn.source.label(),
                turn.text
            );
        }
    }
}

fn parakeet_cache(cache: &Path, tracks: &[(MeetingSource, Vec<f32>); 2]) -> [Vec<ReplayChunk>; 2] {
    let path = cache.join("parakeet.json");
    if let Ok(bytes) = fs::read(&path) {
        return serde_json::from_slice(&bytes).unwrap();
    }
    let model_path = PathBuf::from(std::env::var("HEX_MEETING_MODEL").unwrap());
    let mut model = Transcriber::Gguf(Box::new(
        crate::parakeet::load_test_model(&model_path).unwrap(),
    ));
    let started = Instant::now();
    let cpu = process_cpu_seconds();
    let chunks = [0, 1].map(|index| replay_transcribe(&mut model, &tracks[index].1));
    println!(
        "parakeet: {:.1} s, {:.1} CPU s",
        started.elapsed().as_secs_f32(),
        process_cpu_seconds() - cpu
    );
    fs::write(&path, serde_json::to_vec(&chunks).unwrap()).unwrap();
    chunks
}

/// The live worker's events for the recording, replayed at `speed` times
/// real time.
fn live_cache(
    cache: &Path,
    tracks: &[(MeetingSource, u32, Vec<f32>); 2],
    speed: u32,
) -> Vec<Event> {
    let path = cache.join("live.ndjson");
    if !path.exists() {
        let output = cache.join("live-run");
        fs::create_dir_all(&output).unwrap();
        let (sender, receiver) = mpsc::sync_channel(live::QUEUE_CAPACITY);
        let worker_output = output.clone();
        let worker = thread::spawn(move || {
            let drops = AtomicU64::new(0);
            live::transcribe(
                Path::new(env!("CARGO_MANIFEST_DIR")),
                &worker_output,
                receiver,
                &drops,
            )
        });
        let started = Instant::now();
        let duration_ms = tracks
            .iter()
            .map(|(_, rate, samples)| samples.len() as u64 * 1_000 / u64::from(*rate))
            .min()
            .unwrap();
        for step in 0..duration_ms / PACKET_MS {
            let due = Duration::from_millis(step * PACKET_MS) / speed;
            if let Some(wait) = due.checked_sub(started.elapsed()) {
                thread::sleep(wait);
            }
            for (source, rate, samples) in tracks {
                let per_packet = (*rate as u64 * PACKET_MS / 1_000) as usize;
                let start = step as usize * per_packet;
                sender
                    .send(AudioPacket {
                        source: *source,
                        pts_us: None,
                        arrival_us: step * PACKET_MS * 1_000,
                        position: start as u64,
                        sample_rate: *rate,
                        samples: samples[start..start + per_packet].to_vec(),
                    })
                    .unwrap();
            }
        }
        drop(sender);
        worker.join().unwrap().unwrap();
        println!("live: {:.1} s", started.elapsed().as_secs_f32());
        fs::rename(output.join("transcript.live.ndjson"), &path).unwrap();
        fs::remove_dir_all(output).unwrap();
    }
    read_ndjson(&path).unwrap()
}

fn latest_by(events: &[Event], until_ms: u64) -> HashMap<(MeetingSource, u64), Event> {
    let mut latest = HashMap::new();
    for event in events.iter().filter(|event| event.end_ms <= until_ms) {
        latest.insert((event.source, event.line_id), event.clone());
    }
    latest
}

#[test]
#[ignore = "requires HEX_MEETING_DIR and HEX_MEETING_CACHE; see the module documentation"]
fn meeting_echo_replay() {
    let directory = PathBuf::from(std::env::var("HEX_MEETING_DIR").unwrap());
    let cache = PathBuf::from(std::env::var("HEX_MEETING_CACHE").unwrap());
    fs::create_dir_all(&cache).unwrap();
    let window = std::env::var("HEX_MEETING_WINDOW").map_or((0, 0), |window| {
        let (from, to) = window.split_once('-').unwrap();
        (
            from.parse::<u64>().unwrap() * 1_000,
            to.parse::<u64>().unwrap() * 1_000,
        )
    });
    let labels = Labels::read();
    let recorded = [
        (MeetingSource::System, "system.wav"),
        (MeetingSource::Microphone, "microphone.wav"),
    ]
    .map(|(source, file)| {
        let (rate, samples) = read_float_wav(&directory.join(file));
        (source, rate, samples)
    });
    let duration_ms = recorded
        .iter()
        .map(|(_, rate, samples)| samples.len() as u64 * 1_000 / u64::from(*rate))
        .min()
        .unwrap();
    let resampled = recorded
        .each_ref()
        .map(|(source, rate, samples)| (*source, replay_resample(*rate, samples)));
    let envelope = replay_envelope(&resampled[0].1, &resampled[1].1);
    let chunks = parakeet_cache(&cache, &resampled);
    let speed = std::env::var("HEX_MEETING_LIVE_SPEED").map_or(4, |speed| speed.parse().unwrap());
    let events = live_cache(&cache, &recorded, speed);

    let [system, microphone] = [
        (MeetingSource::System, &chunks[0]),
        (MeetingSource::Microphone, &chunks[1]),
    ]
    .map(|(source, chunks)| replay_track(source, chunks, u64::MAX));
    let (view, kept, all) = replay_views(&system, &microphone, &envelope);
    // Remote speech as the final model heard it is the echo reference for
    // every layer.
    let remote = all
        .iter()
        .filter(|entry| entry.source == MeetingSource::System)
        .cloned()
        .collect::<Vec<_>>();

    let live_path = directory.join("transcript.live.ndjson");
    if live_path.exists() {
        let recorded_events = read_ndjson::<Event>(&live_path).unwrap();
        let rolling_path = directory.join(transcript::ROLLING_FILE);
        let rolling: RollingView = if rolling_path.exists() {
            read_ndjson::<RollingLine>(&rolling_path)
                .unwrap()
                .into_iter()
                .collect()
        } else {
            RollingView::default()
        };
        let latest = latest_by(&recorded_events, duration_ms);
        report(
            "pane as recorded",
            &live_view(&rolling, latest.values()),
            &remote,
            &labels,
            window,
        );
    }

    let latest = latest_by(&events, u64::MAX);
    report(
        "draft only (rolling unavailable)",
        &live_view(&RollingView::default(), latest.values()),
        &remote,
        &labels,
        window,
    );
    report(
        "pane, rolling caught up",
        &live_view(&view, latest.values()),
        &remote,
        &labels,
        window,
    );
    let started = Instant::now();
    for _ in 0..10 {
        std::hint::black_box(live_view(&view, latest.values()));
    }
    println!(
        "live_view over {} draft lines: {:.1} ms",
        latest.len(),
        started.elapsed().as_secs_f64() * 100.0
    );

    // While recording, rolling trails the live draft by whole chunks, and
    // either track's chunk can finish first. Everything any snapshot showed
    // counts, since the user saw it.
    for (system_lag_ms, microphone_lag_ms) in
        [(60_000, 60_000), (60_000, 25_000), (300_000, 300_000)]
    {
        let mut shown = HashSet::new();
        for now in (10_000..duration_ms).step_by(10_000) {
            let tracks = [
                replay_track(
                    MeetingSource::System,
                    &chunks[0],
                    now.saturating_sub(system_lag_ms),
                ),
                replay_track(
                    MeetingSource::Microphone,
                    &chunks[1],
                    now.saturating_sub(microphone_lag_ms),
                ),
            ];
            let (view, _, _) = replay_views(&tracks[0], &tracks[1], &envelope);
            shown.extend(live_view(&view, latest_by(&events, now).values()));
        }
        let shown = shown.into_iter().collect::<Vec<_>>();
        let score = score(&shown, &remote, &labels);
        println!(
            "== pane while recording, rolling {} s / {} s behind: {} You entries ever shown, \
             {} echo-like",
            system_lag_ms / 1_000,
            microphone_lag_ms / 1_000,
            score.you,
            score.echo_like
        );
    }
    report("final transcript", &kept, &remote, &labels, window);

    println!("== every final-model microphone entry");
    let kept = kept.into_iter().collect::<HashSet<_>>();
    let mut all = all;
    all.sort_by_key(|entry| entry.start_ms);
    for entry in all
        .iter()
        .filter(|entry| entry.source == MeetingSource::Microphone)
    {
        println!(
            "  {} {:>7.1} {:>7.1} {} {:<6} share={:<7} overlap={:.2} {}",
            if kept.contains(entry) { "KEEP" } else { "drop" },
            entry.start_ms as f64 / 1_000.0,
            entry.end_ms as f64 / 1_000.0,
            if labels.local(entry) { "L" } else { " " },
            match envelope.assess(entry.start_ms, entry.end_ms) {
                Some(echo::Signal::Bleed) => "bleed",
                Some(echo::Signal::Local) => "local",
                None => "?",
            },
            envelope
                .locality(entry.start_ms, entry.end_ms)
                .map_or("-".into(), |locality| format!(
                    "{:.2}/{:<2}",
                    locality.share, locality.burst
                )),
            remote_overlap(entry, &remote),
            entry.text
        );
    }
    println!("== every draft microphone line");
    let shown = live_view(&RollingView::default(), latest.values())
        .into_iter()
        .collect::<HashSet<_>>();
    let mut lines = latest
        .values()
        .filter(|event| event.source == MeetingSource::Microphone && !event.text.trim().is_empty())
        .collect::<Vec<_>>();
    lines.sort_by_key(|event| event.start_ms);
    for event in lines {
        let entry = TranscriptEntry {
            source: event.source,
            start_ms: event.start_ms,
            end_ms: event.end_ms,
            text: event.text.trim().into(),
        };
        println!(
            "  {} {:>7.1} {:>7.1} {} {:<6} overlap={:.2} {}",
            if shown.contains(&entry) {
                "SHOW"
            } else {
                "hide"
            },
            entry.start_ms as f64 / 1_000.0,
            entry.end_ms as f64 / 1_000.0,
            if labels.local(&entry) { "L" } else { " " },
            match event.signal {
                Some(echo::Signal::Bleed) => "bleed",
                Some(echo::Signal::Local) => "local",
                None => "?",
            },
            remote_overlap(&entry, &remote),
            entry.text
        );
    }
}

fn process_cpu_seconds() -> f64 {
    let mut usage = unsafe { std::mem::zeroed::<libc::rusage>() };
    unsafe { libc::getrusage(libc::RUSAGE_SELF, &mut usage) };
    let seconds = |time: libc::timeval| time.tv_sec as f64 + time.tv_usec as f64 / 1e6;
    seconds(usage.ru_utime) + seconds(usage.ru_stime)
}

/// CPU the live worker spends per second of meeting audio and how late its
/// lines appear, replaying the first `HEX_MEETING_LIVE_SECONDS` (default
/// 120) in real time. With `HEX_MEETING_CACHE`, the completed remote lines'
/// text is written to `live-system.txt` there.
#[test]
#[ignore = "requires HEX_MEETING_DIR; see the module documentation"]
fn meeting_live_cpu() {
    let directory = PathBuf::from(std::env::var("HEX_MEETING_DIR").unwrap());
    let seconds: u64 =
        std::env::var("HEX_MEETING_LIVE_SECONDS").map_or(120, |value| value.parse().unwrap());
    let tracks = [
        (MeetingSource::System, "system.wav"),
        (MeetingSource::Microphone, "microphone.wav"),
    ]
    .map(|(source, file)| {
        let (rate, mut samples) = read_float_wav(&directory.join(file));
        samples.truncate(rate as usize * seconds as usize);
        (source, rate, samples)
    });
    let scratch = std::env::temp_dir().join(format!("hex-live-cpu-{}", std::process::id()));
    fs::create_dir_all(&scratch).unwrap();
    let (sender, receiver) = mpsc::sync_channel(live::QUEUE_CAPACITY);
    let worker_scratch = scratch.clone();
    let worker = thread::spawn(move || {
        live::transcribe(
            Path::new(env!("CARGO_MANIFEST_DIR")),
            &worker_scratch,
            receiver,
            &AtomicU64::new(0),
        )
    });
    let path = scratch.join("transcript.live.ndjson");
    let mut reader = live::TranscriptReader::default();
    let mut first_seen = HashMap::new();
    let before = process_cpu_seconds();
    let started = Instant::now();
    let mut observe = |reader: &mut live::TranscriptReader| {
        reader.read_live_tail(&path).unwrap();
        let now_ms = started.elapsed().as_millis() as u64;
        for event in reader.events() {
            if event.source == MeetingSource::System {
                first_seen
                    .entry((
                        event.line_id,
                        event.phase == crate::events::TranscriptPhase::Completed,
                    ))
                    .or_insert((now_ms, event.end_ms));
            }
        }
    };
    for step in 0..seconds * 1_000 / PACKET_MS {
        let due = Duration::from_millis(step * PACKET_MS);
        if let Some(wait) = due.checked_sub(started.elapsed()) {
            thread::sleep(wait);
        }
        for (source, rate, samples) in &tracks {
            let per_packet = (*rate as u64 * PACKET_MS / 1_000) as usize;
            let start = step as usize * per_packet;
            sender
                .send(AudioPacket {
                    source: *source,
                    pts_us: None,
                    arrival_us: step * PACKET_MS * 1_000,
                    position: start as u64,
                    sample_rate: *rate,
                    samples: samples[start..start + per_packet].to_vec(),
                })
                .unwrap();
        }
        if step % 10 == 0 {
            observe(&mut reader);
        }
    }
    drop(sender);
    worker.join().unwrap().unwrap();
    let cpu = process_cpu_seconds() - before;
    observe(&mut reader);
    let lag = |completed: bool| {
        let mut lags = first_seen
            .iter()
            .filter(|((_, done), _)| *done == completed)
            .map(|(_, (seen, end))| seen.saturating_sub(*end) as f64 / 1_000.0)
            .collect::<Vec<_>>();
        lags.sort_by(f64::total_cmp);
        let at = |fraction: f64| lags[((lags.len() - 1) as f64 * fraction) as usize];
        format!("p50 {:.2} s, p95 {:.2} s", at(0.5), at(0.95))
    };
    println!(
        "live worker: {seconds} s of audio, {cpu:.1} CPU s ({:.0}% of one core); first text \
         after its audio {}; completed line after its end {}",
        cpu * 100.0 / seconds as f64,
        lag(false),
        lag(true)
    );
    if let Ok(cache) = std::env::var("HEX_MEETING_CACHE") {
        let mut lines = reader
            .events()
            .filter(|event| {
                event.source == MeetingSource::System
                    && event.phase == crate::events::TranscriptPhase::Completed
            })
            .collect::<Vec<_>>();
        lines.sort_by_key(|event| event.start_ms);
        let text = lines
            .iter()
            .map(|event| event.text.trim())
            .collect::<Vec<_>>()
            .join("\n");
        fs::write(PathBuf::from(cache).join("live-system.txt"), text).unwrap();
    }
    fs::remove_dir_all(scratch).unwrap();
}

/// How long the command recognizer's Moonshine updates take, the way
/// recognition drives it: 100 ms of microphone audio, then an update every
/// 200 ms, in real time for `HEX_MEETING_LIVE_SECONDS` (default 60).
#[test]
#[ignore = "requires HEX_MEETING_DIR; see the module documentation"]
fn moonshine_command_update_latency() {
    let directory = PathBuf::from(std::env::var("HEX_MEETING_DIR").unwrap());
    let seconds: usize =
        std::env::var("HEX_MEETING_LIVE_SECONDS").map_or(60, |value| value.parse().unwrap());
    let (rate, samples) = read_float_wav(&directory.join("microphone.wav"));
    let audio = replay_resample(rate, &samples[..rate as usize * seconds]);
    let mut model =
        crate::moonshine::Moonshine::load(Path::new(env!("CARGO_MANIFEST_DIR"))).unwrap();
    let cpu = process_cpu_seconds();
    let started = Instant::now();
    let mut durations = Vec::new();
    for (index, block) in audio.chunks(1_600).enumerate() {
        let due = Duration::from_millis(index as u64 * 100);
        if let Some(wait) = due.checked_sub(started.elapsed()) {
            thread::sleep(wait);
        }
        model.add_audio_to(0, block, 16_000).unwrap();
        if index % 2 == 1 {
            let update = Instant::now();
            model.update_stream(0).unwrap();
            durations.push(update.elapsed().as_secs_f64() * 1_000.0);
        }
    }
    durations.sort_by(f64::total_cmp);
    let at = |fraction: f64| durations[((durations.len() - 1) as f64 * fraction) as usize];
    println!(
        "command updates: p50 {:.1} ms, p95 {:.1} ms, max {:.1} ms; {:.0}% of one core",
        at(0.5),
        at(0.95),
        at(1.0),
        (process_cpu_seconds() - cpu) * 100.0 / seconds as f64
    );
}

/// Dictation latency while something else runs: transcribes seconds 5-13
/// of `HEX_MEETING_DIR/system.wav` with `HEX_MEETING_MODEL` twenty times,
/// a second apart, and prints the spread.
#[test]
#[ignore = "requires HEX_MEETING_DIR and HEX_MEETING_MODEL; see the module documentation"]
fn dictation_latency() {
    let directory = PathBuf::from(std::env::var("HEX_MEETING_DIR").unwrap());
    let model_path = PathBuf::from(std::env::var("HEX_MEETING_MODEL").unwrap());
    let (rate, samples) = read_float_wav(&directory.join("system.wav"));
    let clip = replay_resample(rate, &samples[rate as usize * 5..rate as usize * 13]);
    let mut model = Transcriber::Gguf(Box::new(
        crate::parakeet::load_test_model(&model_path).unwrap(),
    ));
    model.transcribe_segments(clip.clone()).unwrap();
    let mut durations = (0..20)
        .map(|_| {
            thread::sleep(Duration::from_secs(1));
            let started = Instant::now();
            model.transcribe_segments(clip.clone()).unwrap();
            started.elapsed().as_secs_f64() * 1_000.0
        })
        .collect::<Vec<_>>();
    durations.sort_by(f64::total_cmp);
    println!(
        "dictation of 8 s: p50 {:.0} ms, p95 {:.0} ms, max {:.0} ms",
        durations[10], durations[18], durations[19]
    );
}
