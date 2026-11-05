//! Labs › Speakers in meetings: after a meeting's final publication, its
//! system audio ("Them") is diarized so each of those lines names the voice
//! that said it ("Speaker 1", "Speaker 2", …), and the user can rename them
//! per meeting. The microphone stays "You".
//!
//! Diarization runs on the meeting's `system.wav` in overlapping chunks,
//! since the model's memory grows with the audio it is given. Speaker ids are
//! matched across chunks by who talks in the overlap. It is background work:
//! it yields to dictation before each chunk and runs at utility priority.

use std::collections::{BTreeMap, BTreeSet, HashMap};
use std::path::Path;
use std::sync::Mutex;
use std::sync::atomic::{AtomicBool, Ordering};
use std::thread;
use std::time::{Duration, Instant};

use color_eyre::eyre::{Result, eyre};
use serde::{Deserialize, Serialize};

use super::{
    MeetingManifest, MeetingSource, TranscriptEntry, final_transcript_exists, read_manifest,
    read_ndjson, root, write_final_files, write_manifest,
};
use crate::transcription::models as transcription_models;

/// Audio the model sees at once, and how much of it repeats the previous
/// chunk so its speakers can be matched.
const CHUNK_MS: u64 = 10 * 60 * 1_000;
const OVERLAP_MS: u64 = 60 * 1_000;
const RATE: u64 = 16_000;
/// Talking together for less than this in an overlap is no match.
const MIN_MATCH_MS: u64 = 500;
/// A line no segment covers takes the nearest speaker this close.
const NEAREST_MS: u64 = 1_000;
/// Longest a chunk waits for dictation to finish before it runs anyway.
const DICTATION_WAIT: Duration = Duration::from_secs(120);

/// The other voices told apart in one meeting.
#[derive(Clone, Debug, Default, Deserialize, Eq, PartialEq, Serialize)]
pub struct MeetingSpeakers {
    /// The diarization model that told them apart.
    pub model: String,
    /// How many voices the lines name, numbered from 1.
    pub count: u32,
    /// Names the user gave, by speaker number.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub names: BTreeMap<u32, String>,
}

impl MeetingSpeakers {
    /// The user's name for `speaker`, else "Speaker N".
    pub fn name(&self, speaker: u32) -> String {
        self.names
            .get(&speaker)
            .cloned()
            .unwrap_or_else(|| format!("Speaker {speaker}"))
    }
}

/// Who said a line, as Markdown, the CLI, and MCP show it: "You" for the
/// microphone, the speaker's name or number for a told-apart system line,
/// and "Them" otherwise.
pub fn speaker_label(
    source: MeetingSource,
    speaker: Option<u32>,
    speakers: Option<&MeetingSpeakers>,
) -> String {
    match (source, speaker) {
        (MeetingSource::System, Some(speaker)) => match speakers {
            Some(speakers) => speakers.name(speaker),
            None => format!("Speaker {speaker}"),
        },
        (source, _) => source.label().to_string(),
    }
}

/// One stretch of one voice, in milliseconds.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) struct Segment {
    pub start_ms: u64,
    pub end_ms: u64,
    pub speaker: u32,
}

#[cfg_attr(test, allow(dead_code))]
static DOWNLOAD_STARTED: AtomicBool = AtomicBool::new(false);
static INSTALLED: AtomicBool = AtomicBool::new(false);

/// Called while the Lab is on: downloads the speaker model once, so the
/// first meeting after turning it on is ready.
pub(crate) fn prepare() {
    if INSTALLED.load(Ordering::Relaxed) {
        return;
    }
    if transcription_models::speaker_model_installed() {
        INSTALLED.store(true, Ordering::Relaxed);
    } else {
        start_download();
    }
}

fn start_download() {
    #[cfg(not(test))]
    if !DOWNLOAD_STARTED.swap(true, Ordering::Relaxed) {
        let _ = thread::Builder::new()
            .name("hex-speaker-model-download".into())
            .spawn(|| {
                if let Err(error) =
                    transcription_models::download_speaker_model(&AtomicBool::new(false))
                {
                    tracing::warn!(%error, "could not download the speaker model");
                    // The next pass of the meeting controller tries again.
                    thread::sleep(Duration::from_secs(60));
                    DOWNLOAD_STARTED.store(false, Ordering::Relaxed);
                }
            });
    }
}

static IN_PROGRESS: Mutex<BTreeSet<String>> = Mutex::new(BTreeSet::new());

struct InProgress(String);

impl InProgress {
    fn claim(id: &str) -> Option<Self> {
        IN_PROGRESS
            .lock()
            .unwrap_or_else(|error| error.into_inner())
            .insert(id.to_string())
            .then(|| Self(id.to_string()))
    }
}

impl Drop for InProgress {
    fn drop(&mut self) {
        IN_PROGRESS
            .lock()
            .unwrap_or_else(|error| error.into_inner())
            .remove(&self.0);
    }
}

/// Whether a meeting's speakers are being told apart now.
pub fn speakers_in_progress(id: &str) -> bool {
    IN_PROGRESS
        .lock()
        .unwrap_or_else(|error| error.into_inner())
        .contains(id)
}

/// Tells a published meeting's speakers apart on a background thread, then
/// calls `done` with the updated manifest (or `None` when nothing changed).
/// False when it is already running for that meeting.
pub(crate) fn spawn(
    id: String,
    done: impl FnOnce(Option<MeetingManifest>) + Send + 'static,
) -> bool {
    let Some(claim) = InProgress::claim(&id) else {
        return false;
    };
    thread::Builder::new()
        .name("meeting-speakers".into())
        .spawn(move || {
            crate::transcription::background_priority::deprioritize_background_thread();
            let _claim = claim;
            let result = root().and_then(|root| diarize_meeting(&root.join(&id)));
            match result {
                Ok(manifest) => done(Some(manifest)),
                Err(error) => {
                    tracing::warn!(%error, meeting = id, "could not tell the meeting's speakers apart");
                    done(record_error(&id, &error.to_string()));
                }
            }
        })
        .map_err(|error| tracing::error!(%error, "could not start meeting speakers"))
        .is_ok()
}

fn record_error(id: &str, error: &str) -> Option<MeetingManifest> {
    let directory = root().ok()?.join(id);
    let mut manifest = read_manifest(&directory).ok()?;
    manifest.speakers_error = Some(error.to_string());
    write_manifest(&directory, &manifest).ok()?;
    Some(manifest)
}

fn diarize_meeting(directory: &Path) -> Result<MeetingManifest> {
    let manifest = read_manifest(directory)?;
    if !final_transcript_exists(directory) {
        return Err(eyre!("the meeting has no final transcript"));
    }
    if !transcription_models::speaker_model_installed() {
        start_download();
        return Err(eyre!(
            "The speaker model is still downloading; later meetings will name their speakers"
        ));
    }
    let started = Instant::now();
    let segments = diarize_track(
        &transcription_models::speaker_model_path()?,
        &directory.join(&manifest.system.file),
    )?;
    let mut entries: Vec<TranscriptEntry> = read_ndjson(&directory.join("transcript.ndjson"))?;
    let count = assign_speakers(&mut entries, &segments, system_shift_ms(&manifest));
    // Re-read so a rename or notes written meanwhile are kept.
    let mut manifest = read_manifest(directory)?;
    manifest.speakers = Some(MeetingSpeakers {
        model: transcription_models::SPEAKER_MODEL_ARTIFACT.variant.into(),
        count,
        names: BTreeMap::new(),
    });
    manifest.speakers_error = None;
    publish(directory, &mut manifest, &entries)?;
    tracing::info!(
        meeting = manifest.id,
        speakers = count,
        segments = segments.len(),
        elapsed_ms = started.elapsed().as_millis() as u64,
        "told the meeting's speakers apart"
    );
    Ok(manifest)
}

/// Renames one of a meeting's speakers; an empty name returns it to its
/// number. The internal transcript follows, and so does the exported
/// Markdown while it is still the copy Hex wrote.
pub fn rename_speaker(id: &str, speaker: u32, name: &str) -> Result<MeetingManifest> {
    rename_speaker_in(&root()?.join(id), speaker, name)
}

fn rename_speaker_in(directory: &Path, speaker: u32, name: &str) -> Result<MeetingManifest> {
    let mut manifest = read_manifest(directory)?;
    if speakers_in_progress(&manifest.id) {
        return Err(eyre!("Wait for the speakers to be told apart"));
    }
    let speakers = manifest
        .speakers
        .as_mut()
        .filter(|speakers| (1..=speakers.count).contains(&speaker))
        .ok_or_else(|| eyre!("The meeting has no speaker {speaker}"))?;
    let name = name.split_whitespace().collect::<Vec<_>>().join(" ");
    if name.is_empty() {
        speakers.names.remove(&speaker);
    } else {
        speakers.names.insert(speaker, name);
    }
    let entries: Vec<TranscriptEntry> = read_ndjson(&directory.join("transcript.ndjson"))?;
    publish(directory, &mut manifest, &entries)?;
    Ok(manifest)
}

/// Writes the transcript files, the exported Markdown's transcript while
/// the user hasn't changed that file, and then the manifest.
fn publish(
    directory: &Path,
    manifest: &mut MeetingManifest,
    entries: &[TranscriptEntry],
) -> Result<()> {
    write_final_files(directory, manifest, entries)?;
    if let Some(path) = manifest.export_path.clone().filter(|path| path.exists()) {
        let section = super::transcript::render_transcript_section(manifest, entries);
        if let Err(error) =
            super::notes::replace_transcript_section(&path, &mut manifest.export_sha256, &section)
        {
            tracing::info!(%error, meeting = manifest.id, "left the meeting's Markdown as it is");
        }
    }
    write_manifest(directory, manifest)
}

/// How far the system track's audio starts after the meeting's timeline:
/// transcript times are measured from the earlier of the two tracks.
fn system_shift_ms(manifest: &MeetingManifest) -> u64 {
    let origin = [
        manifest.system.first_arrival_us,
        manifest.microphone.first_arrival_us,
    ]
    .into_iter()
    .flatten()
    .min()
    .unwrap_or_default();
    manifest
        .system
        .first_arrival_us
        .unwrap_or(origin)
        .saturating_sub(origin)
        / 1_000
}

fn diarize_track(model: &Path, path: &Path) -> Result<Vec<Segment>> {
    let model = load_model(model)?;
    let mut session = model.diarize_session()?;
    let mut reader = hound::WavReader::open(path)
        .map_err(|error| eyre!("could not read {}: {error}", path.display()))?;
    let rate = reader.spec().sample_rate;
    let mut resampler =
        Some(crate::dictation::StreamingResampler::new(rate).map_err(|error| eyre!(error))?);
    let mut samples = reader.samples::<f32>();
    let blocks = std::iter::from_fn(|| {
        let mut block = Vec::with_capacity(rate as usize);
        for sample in samples.by_ref().take(rate as usize) {
            match sample {
                Ok(sample) => block.push(sample),
                Err(error) => return Some(Err(eyre!(error))),
            }
        }
        if block.is_empty() {
            return resampler.take().map(|resampler| Ok(resampler.finish()));
        }
        Some(Ok(resampler.as_mut()?.push(&block)))
    });
    diarize_stream(blocks, |pcm| {
        if crate::desktop::process_exit::quitting() {
            return Err(eyre!("Hex is quitting"));
        }
        wait_for_dictation();
        let options = transcribe_cpp::DiarizeOptions::default();
        Ok(session
            .run(pcm, &options)?
            .into_iter()
            .filter(|segment| segment.t1_ms > segment.t0_ms && segment.speaker_id > 0)
            .map(|segment| Segment {
                start_ms: segment.t0_ms.max(0) as u64,
                end_ms: segment.t1_ms.max(0) as u64,
                speaker: segment.speaker_id as u32,
            })
            .collect())
    })
}

fn load_model(path: &Path) -> Result<transcribe_cpp::Model> {
    #[cfg(target_os = "windows")]
    crate::transcription::local_transcriber::init_backends()?;
    Ok(transcribe_cpp::Model::load_with(
        path,
        &transcribe_cpp::ModelOptions::default(),
    )?)
}

/// Waits while dictation captures or transcribes, for at most
/// [`DICTATION_WAIT`]: a chunk can't be interrupted once it runs.
fn wait_for_dictation() {
    let started = Instant::now();
    while crate::transcription::engine::dictation_busy()
        && started.elapsed() < DICTATION_WAIT
        && !crate::desktop::process_exit::quitting()
    {
        thread::sleep(Duration::from_millis(100));
    }
}

/// Feeds 16 kHz audio to `run` in overlapping chunks and stitches what it
/// finds into one set of segments with speaker ids shared by every chunk.
fn diarize_stream(
    blocks: impl Iterator<Item = Result<Vec<f32>>>,
    mut run: impl FnMut(&[f32]) -> Result<Vec<Segment>>,
) -> Result<Vec<Segment>> {
    let chunk = samples(CHUNK_MS);
    let step = samples(CHUNK_MS - OVERLAP_MS);
    let mut buffer = Vec::with_capacity(chunk);
    let mut offset_ms = 0;
    let mut chunks = Vec::new();
    for block in blocks {
        buffer.extend(block?);
        while buffer.len() >= chunk {
            chunks.push((offset_ms, run(&buffer[..chunk])?));
            buffer.drain(..step);
            offset_ms += CHUNK_MS - OVERLAP_MS;
        }
    }
    // What is left after a full chunk is its overlap, already heard.
    if !buffer.is_empty() && (chunks.is_empty() || buffer.len() > samples(OVERLAP_MS)) {
        chunks.push((offset_ms, run(&buffer)?));
    }
    Ok(stitch(&chunks, OVERLAP_MS))
}

fn samples(ms: u64) -> usize {
    (ms * RATE / 1_000) as usize
}

/// Joins chunks that start at their offsets and repeat `overlap_ms` of the
/// chunk before. A chunk's speakers take the ids of the previous chunk's
/// speakers they talk together with most in the overlap; the rest get new
/// ids. Each chunk keeps its segments from the middle of its leading
/// overlap to the middle of the next.
fn stitch(chunks: &[(u64, Vec<Segment>)], overlap_ms: u64) -> Vec<Segment> {
    let mut stitched = Vec::new();
    let mut previous: Vec<Segment> = Vec::new();
    let mut next_id = 1;
    for (index, (offset, segments)) in chunks.iter().enumerate() {
        let segments: Vec<Segment> = segments
            .iter()
            .map(|segment| Segment {
                start_ms: segment.start_ms + offset,
                end_ms: segment.end_ms + offset,
                speaker: segment.speaker,
            })
            .collect();
        let ids = match_speakers(
            &previous,
            &segments,
            (*offset, offset + overlap_ms),
            &mut next_id,
        );
        let global: Vec<Segment> = segments
            .iter()
            .map(|segment| Segment {
                speaker: ids[&segment.speaker],
                ..*segment
            })
            .collect();
        let low = if index == 0 {
            0
        } else {
            offset + overlap_ms / 2
        };
        let high = chunks
            .get(index + 1)
            .map_or(u64::MAX, |(next, _)| next + overlap_ms / 2);
        stitched.extend(global.iter().filter_map(|segment| {
            let start_ms = segment.start_ms.max(low);
            let end_ms = segment.end_ms.min(high);
            (end_ms > start_ms).then_some(Segment {
                start_ms,
                end_ms,
                ..*segment
            })
        }));
        previous = global;
    }
    stitched.sort_by_key(|segment| (segment.start_ms, segment.speaker));
    stitched
}

/// The id each of `current`'s chunk-local speakers takes.
fn match_speakers(
    previous: &[Segment],
    current: &[Segment],
    (low, high): (u64, u64),
    next_id: &mut u32,
) -> HashMap<u32, u32> {
    let mut together: HashMap<(u32, u32), u64> = HashMap::new();
    for old in previous {
        for new in current {
            let start = old.start_ms.max(new.start_ms).max(low);
            let end = old.end_ms.min(new.end_ms).min(high);
            if end > start {
                *together.entry((old.speaker, new.speaker)).or_default() += end - start;
            }
        }
    }
    let mut pairs: Vec<_> = together.into_iter().collect();
    pairs.sort_by(|a, b| b.1.cmp(&a.1).then(a.0.cmp(&b.0)));
    let mut ids = HashMap::new();
    let mut taken = BTreeSet::new();
    for ((old, new), shared_ms) in pairs {
        if shared_ms >= MIN_MATCH_MS && !ids.contains_key(&new) && taken.insert(old) {
            ids.insert(new, old);
            *next_id = (*next_id).max(old + 1);
        }
    }
    let mut local: Vec<u32> = current.iter().map(|segment| segment.speaker).collect();
    local.sort_unstable();
    local.dedup();
    for speaker in local {
        ids.entry(speaker).or_insert_with(|| {
            let id = *next_id;
            *next_id += 1;
            id
        });
    }
    ids
}

/// Gives each system line the speaker heard most while it was said (or the
/// nearest one within [`NEAREST_MS`]), numbering speakers from 1 in order of
/// their first line. `shift_ms` moves segments from the system track onto
/// the transcript's timeline. Returns how many speakers the lines name.
fn assign_speakers(entries: &mut [TranscriptEntry], segments: &[Segment], shift_ms: u64) -> u32 {
    let mut numbers: HashMap<u32, u32> = HashMap::new();
    let mut order: Vec<usize> = (0..entries.len()).collect();
    order.sort_by_key(|&index| entries[index].start_ms);
    for index in order {
        let entry = &mut entries[index];
        entry.speaker = None;
        if entry.source != MeetingSource::System {
            continue;
        }
        let Some(speaker) = majority_speaker(entry, segments, shift_ms) else {
            continue;
        };
        let next = numbers.len() as u32 + 1;
        entry.speaker = Some(*numbers.entry(speaker).or_insert(next));
    }
    numbers.len() as u32
}

fn majority_speaker(entry: &TranscriptEntry, segments: &[Segment], shift_ms: u64) -> Option<u32> {
    let mut heard: BTreeMap<u32, u64> = BTreeMap::new();
    let mut nearest: Option<(u64, u32)> = None;
    for segment in segments {
        let (start, end) = (segment.start_ms + shift_ms, segment.end_ms + shift_ms);
        let shared = end
            .min(entry.end_ms)
            .saturating_sub(start.max(entry.start_ms));
        if shared > 0 {
            *heard.entry(segment.speaker).or_default() += shared;
        }
        let gap = start
            .saturating_sub(entry.end_ms)
            .max(entry.start_ms.saturating_sub(end));
        if gap <= NEAREST_MS && nearest.is_none_or(|(best, _)| gap < best) {
            nearest = Some((gap, segment.speaker));
        }
    }
    heard
        .into_iter()
        .max_by(|a, b| a.1.cmp(&b.1).then(b.0.cmp(&a.0)))
        .map(|(speaker, _)| speaker)
        .or(nearest.map(|(_, speaker)| speaker))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn segment(start_ms: u64, end_ms: u64, speaker: u32) -> Segment {
        Segment {
            start_ms,
            end_ms,
            speaker,
        }
    }

    fn entry(source: MeetingSource, start_ms: u64, end_ms: u64) -> TranscriptEntry {
        TranscriptEntry {
            source,
            start_ms,
            end_ms,
            text: "words".into(),
            speaker: None,
        }
    }

    #[test]
    fn lines_take_the_voice_heard_most_and_are_numbered_by_first_line() {
        let segments = [
            segment(0, 4_000, 3),
            segment(3_000, 9_000, 1),
            segment(20_000, 21_000, 2),
        ];
        let mut entries = vec![
            entry(MeetingSource::System, 0, 4_000),
            entry(MeetingSource::Microphone, 4_000, 5_000),
            entry(MeetingSource::System, 4_000, 9_000),
            entry(MeetingSource::System, 2_500, 5_000),
            // No segment within a second: it stays "Them".
            entry(MeetingSource::System, 12_000, 13_000),
            // Just past a segment's end: the nearest voice.
            entry(MeetingSource::System, 21_500, 22_000),
        ];
        assert_eq!(assign_speakers(&mut entries, &segments, 0), 3);
        let speakers: Vec<_> = entries.iter().map(|entry| entry.speaker).collect();
        assert_eq!(
            speakers,
            [Some(1), None, Some(2), Some(2), None, Some(3)],
            "the microphone is never a numbered speaker"
        );
    }

    #[test]
    fn segments_move_onto_the_transcript_timeline() {
        let line = entry(MeetingSource::System, 5_000, 6_000);
        let segments = [segment(0, 1_200, 1), segment(1_200, 3_000, 2)];
        // 5–6 s on the timeline is 1–2 s of system audio: mostly speaker 2's.
        assert_eq!(majority_speaker(&line, &segments, 4_000), Some(2));
        assert_eq!(majority_speaker(&line, &segments, 0), None);
    }

    #[test]
    fn chunks_share_ids_through_who_talks_in_the_overlap() {
        let chunks = vec![
            (
                0,
                vec![segment(0, 550_000, 1), segment(550_000, 600_000, 2)],
            ),
            // The second chunk numbers the same two voices the other way
            // round and adds a third.
            (
                540_000,
                vec![
                    segment(0, 10_000, 2),
                    segment(10_000, 60_000, 1),
                    segment(60_000, 200_000, 2),
                    segment(200_000, 300_000, 3),
                ],
            ),
        ];
        assert_eq!(
            stitch(&chunks, 60_000),
            [
                segment(0, 550_000, 1),
                segment(550_000, 570_000, 2),
                segment(570_000, 600_000, 2),
                segment(600_000, 740_000, 1),
                segment(740_000, 840_000, 3),
            ]
        );
    }

    #[test]
    fn long_audio_runs_in_overlapping_chunks_and_short_audio_once() {
        let mut lengths = Vec::new();
        let minute = vec![0.0_f32; samples(60_000)];
        // 25 minutes: chunks at 0, 9 and 18 minutes; the last holds 7.
        let blocks = (0..25).map(|_| Ok(minute.clone()));
        diarize_stream(blocks, |pcm| {
            lengths.push(pcm.len() / samples(60_000));
            Ok(Vec::new())
        })
        .unwrap();
        assert_eq!(lengths, [10, 10, 7]);

        lengths.clear();
        let blocks = (0..3).map(|_| Ok(minute.clone()));
        diarize_stream(blocks, |pcm| {
            lengths.push(pcm.len() / samples(60_000));
            Ok(Vec::new())
        })
        .unwrap();
        assert_eq!(lengths, [3]);
    }

    /// A finished meeting in a temporary folder: its transcript told apart
    /// into two voices and exported to `export.md`, as Hex wrote it.
    fn told_apart_meeting() -> (std::path::PathBuf, MeetingManifest) {
        let directory = std::env::temp_dir().join(format!(
            "hex-speakers-{}-{}",
            std::process::id(),
            crate::desktop::events::now_ms()
        ));
        std::fs::create_dir_all(&directory).unwrap();
        let mut manifest: MeetingManifest = serde_json::from_value(serde_json::json!({
            "schema_version": 1,
            "id": "speakers-test",
            "title": "Planning",
            "status": "complete",
            "created_at_ms": 1_760_000_000_000_u64,
            "ended_at_ms": null,
            "duration_ms": 10_000,
            "system": {"file": "system.wav", "samples": 0, "dropped_packets": 0},
            "microphone": {"file": "microphone.wav", "samples": 0, "dropped_packets": 0},
            "error": null
        }))
        .unwrap();
        manifest.speakers = Some(MeetingSpeakers {
            model: "test".into(),
            count: 2,
            names: BTreeMap::new(),
        });
        let mut entries = vec![
            entry(MeetingSource::System, 0, 2_000),
            entry(MeetingSource::Microphone, 2_500, 3_000),
            entry(MeetingSource::System, 3_500, 5_000),
        ];
        entries[0].speaker = Some(1);
        entries[2].speaker = Some(2);
        let export = directory.join("export.md");
        let markdown = format!(
            "# Planning\n\nMy notes stay.\n\n{}",
            super::super::transcript::render_transcript_section(&manifest, &entries)
        );
        std::fs::write(&export, &markdown).unwrap();
        manifest.export_path = Some(export);
        manifest.export_sha256 = Some(super::super::text_sha256(markdown.as_bytes()));
        publish(&directory, &mut manifest, &entries).unwrap();
        (directory, manifest)
    }

    #[test]
    fn renaming_a_speaker_rewrites_the_transcripts_hex_wrote() {
        let (directory, manifest) = told_apart_meeting();
        let export = manifest.export_path.clone().unwrap();
        assert!(
            std::fs::read_to_string(&export)
                .unwrap()
                .contains("**Speaker 2** · 00:03")
        );

        let renamed = rename_speaker_in(&directory, 2, "  Sam  Lee ").unwrap();
        assert_eq!(renamed.speakers.unwrap().name(2), "Sam Lee");
        let markdown = std::fs::read_to_string(&export).unwrap();
        assert!(markdown.starts_with("# Planning\n\nMy notes stay.\n\n## Transcript\n"));
        assert!(markdown.contains("**Speaker 1** · 00:00"));
        assert!(markdown.contains("**You** · 00:02"));
        assert!(markdown.contains("**Sam Lee** · 00:03"));
        let internal = std::fs::read_to_string(directory.join("transcript.md")).unwrap();
        assert!(internal.contains("**00:03 Sam Lee**"), "{internal}");

        // An empty name returns the speaker to its number.
        rename_speaker_in(&directory, 2, " ").unwrap();
        assert!(
            std::fs::read_to_string(&export)
                .unwrap()
                .contains("**Speaker 2** · 00:03")
        );
        assert!(rename_speaker_in(&directory, 3, "Nobody").is_err());

        // A Markdown file the user edited is left alone; the manifest and
        // Hex's own transcript still follow.
        std::fs::write(&export, "# Planning\n\nRewritten by hand.\n").unwrap();
        rename_speaker_in(&directory, 1, "Alex").unwrap();
        assert_eq!(
            std::fs::read_to_string(&export).unwrap(),
            "# Planning\n\nRewritten by hand.\n"
        );
        let internal = std::fs::read_to_string(directory.join("transcript.md")).unwrap();
        assert!(internal.contains("**00:00 Alex**"));
        std::fs::remove_dir_all(directory).unwrap();
    }

    /// The real model on a synthetic call: put the pinned speaker model and
    /// a WAV of turns by three voices (A B C A B C A C B A, each about
    /// 5.5 s; two women's voices of one TTS engine merge, so use a man's for C) in `research-tmp/`. Never commit either.
    #[test]
    #[ignore = "needs the speaker model and a synthetic call"]
    fn the_model_tells_a_synthetic_call_s_voices_apart() {
        let folder = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("research-tmp");
        let started = Instant::now();
        let segments =
            diarize_track(&folder.join("sortformer.gguf"), &folder.join("call.wav")).unwrap();
        eprintln!("{} segments in {:?}", segments.len(), started.elapsed());
        for segment in &segments {
            eprintln!("{segment:?}");
        }
        let turns = [
            0.0, 4.86, 10.38, 16.27, 21.09, 26.99, 32.13, 38.05, 44.05, 49.67, 56.37,
        ];
        let mut entries: Vec<TranscriptEntry> = turns
            .windows(2)
            .map(|span| {
                entry(
                    MeetingSource::System,
                    (span[0] * 1_000.0) as u64,
                    (span[1] * 1_000.0 - 600.0) as u64,
                )
            })
            .collect();
        assert_eq!(assign_speakers(&mut entries, &segments, 0), 3);
        let speakers: Vec<_> = entries.iter().map(|entry| entry.speaker.unwrap()).collect();
        assert_eq!(speakers, [1, 2, 3, 1, 2, 3, 1, 3, 2, 1]);
    }

    #[test]
    fn labels_name_numbered_and_renamed_speakers() {
        let mut speakers = MeetingSpeakers {
            model: "test".into(),
            count: 2,
            names: BTreeMap::new(),
        };
        speakers.names.insert(2, "Alex".into());
        let label = |source, speaker| speaker_label(source, speaker, Some(&speakers));
        assert_eq!(label(MeetingSource::Microphone, None), "You");
        assert_eq!(label(MeetingSource::System, None), "Them");
        assert_eq!(label(MeetingSource::System, Some(1)), "Speaker 1");
        assert_eq!(label(MeetingSource::System, Some(2)), "Alex");
    }
}
