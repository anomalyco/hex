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
//! The first run transcribes both WAVs with Parakeet, caching the chunks in
//! `HEX_MEETING_CACHE`; later runs only re-evaluate echo suppression. Delete
//! `parakeet.json` there after changing the chunker. Each layer reports its
//! `You` entries, how many repeat nearby remote words (echo-like), and how
//! many labeled local utterances it lost. Labels are lines of `<start s> <end s> [note]` marking
//! where the user really spoke. `HEX_MEETING_WINDOW=<from s>-<to s>` prints
//! that part of each layer's turns.

use std::collections::HashSet;
use std::thread;
use std::time::{Duration, Instant};

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

    let rolling_path = directory.join(transcript::ROLLING_FILE);
    if rolling_path.exists() {
        let rolling: RollingView = read_ndjson::<RollingLine>(&rolling_path)
            .unwrap()
            .into_iter()
            .collect();
        report(
            "pane as recorded",
            &rolling.entries,
            &remote,
            &labels,
            window,
        );
    }
    report(
        "pane, rolling caught up",
        &view.entries,
        &remote,
        &labels,
        window,
    );

    // While recording, rolling trails the call by whole chunks, and either
    // track's chunk can finish first. Everything any snapshot showed
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
            shown.extend(view.entries);
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
}

fn process_cpu_seconds() -> f64 {
    let mut usage = unsafe { std::mem::zeroed::<libc::rusage>() };
    unsafe { libc::getrusage(libc::RUSAGE_SELF, &mut usage) };
    let seconds = |time: libc::timeval| time.tv_sec as f64 + time.tv_usec as f64 / 1e6;
    seconds(usage.ru_utime) + seconds(usage.ru_stime)
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
