//! Meeting transcription. Each track streams through a 16 kHz resampler into
//! short pause-aware chunks that are transcribed at utility QoS whenever
//! dictation is idle: during the recording (rolling, which is also what the
//! Meetings pane and every in-call copy show) or from the WAV after Stop.
//! Hard cuts overlap and are deduplicated at the seam. The joined transcript
//! drops microphone utterances that echo the remote audio, and a Markdown
//! copy is published to the user's meeting folder.

use std::collections::VecDeque;
use std::fs::{self, File};
use std::io::{BufWriter, Write};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{self, Receiver, RecvTimeoutError, SyncSender, TryRecvError};
use std::thread::{self, JoinHandle};
use std::time::{Duration, Instant};

use color_eyre::eyre::{Result, WrapErr, eyre};
use hound::WavReader;
use serde::{Deserialize, Serialize};

use crate::dictation::StreamingResampler;
#[cfg(target_os = "macos")]
use crate::transcription::Transcriber;
#[cfg(not(target_os = "macos"))]
use crate::transcription::local_transcriber::LocalTranscriber as Transcriber;
use crate::transcription::models::TranscriptionSelection;
use crate::transcription::pause_segmenter::PauseSegmenter;
use crate::transcription::transcript::{Transcript, TranscriptSegment};

use super::echo::{EchoEnvelope, Signal};
use super::{
    AudioPacket, MeetingManifest, MeetingSource, TranscriptEntry, coalesce_transcript,
    format_duration, set_owner_only, sync_directory,
};

const RATE: usize = 16_000;
const FRAME: usize = RATE / 50;
/// Pause cuts closer than this to the chunk start are ignored.
const CHUNK_MIN: usize = RATE * 3;
/// The first pause after this length ends a chunk. Short chunks keep the
/// in-call transcript a few seconds behind the call.
const CHUNK_TARGET: usize = RATE * 6;
/// Without a pause, a chunk is cut here and the next one overlaps it.
const CHUNK_MAX: usize = RATE * 10;
const CHUNK_OVERLAP: usize = RATE * 2;
const MIN_TAIL: usize = RATE * 3 / 10;
/// A chunk with no 20 ms frame this loud is silence and is not transcribed.
const SILENT_DB: f32 = -60.0;
/// About five minutes of one-second blocks for both sources.
const ROLLING_QUEUE_CAPACITY: usize = 600;
/// Chunks slower than this are logged.
const SLOW_CHUNK: Duration = Duration::from_secs(5);
/// A source whose untranscribed backlog exceeds this skips its oldest chunks;
/// the final transcript then rebuilds it from the WAV.
const MAX_PENDING_AUDIO: usize = RATE * 120;
const DICTATION_WAIT: Duration = Duration::from_secs(20);
/// Pause before retrying a chunk that dictation preempted.
const PREEMPTED_RETRY: Duration = Duration::from_millis(250);
const ECHO_WINDOW_MS: u64 = 1_500;
/// Word-level segments closer than this are judged together.
const UTTERANCE_GAP_MS: u64 = 700;
/// Entries with at least this many words are also judged on their own.
const SENTENCE_WORDS: usize = 3;
const ECHO_SIMILARITY: f32 = 0.6;
/// A pause this long between timed words starts a new sentence.
const SENTENCE_PAUSE_MS: i64 = 600;
const SEAM_MAX_WORDS: usize = 12;
/// How far apart two chunks may place the same word and still splice on it.
const SEAM_ANCHOR_MS: i64 = 300;
/// How far the system source's capture may trail the microphone's before
/// microphone text stops waiting for it.
const SYSTEM_STALL_MS: u64 = 5_000;

pub(super) const ROLLING_FILE: &str = "transcript.rolling.ndjson";

#[derive(Debug)]
pub(super) struct Chunk {
    /// 16 kHz offset of `samples[0]` within the track.
    start: usize,
    /// Leading samples shared with the previous chunk.
    overlap: usize,
    /// Trailing samples the next chunk transcribes again.
    tail: usize,
    samples: Vec<f32>,
}

impl Chunk {
    fn is_silent(&self) -> bool {
        let threshold = 10_f32.powf(SILENT_DB / 10.0);
        self.samples.chunks(FRAME).all(|frame| {
            frame.iter().map(|sample| sample * sample).sum::<f32>() / (frame.len() as f32)
                < threshold
        })
    }
}

/// Splits a 16 kHz track into chunks that end at pauses when possible.
#[derive(Default)]
pub(super) struct Chunker {
    buffer: Vec<f32>,
    buffer_start: usize,
    chunk_start: usize,
    overlap: usize,
    segmenter: PauseSegmenter,
    pauses: VecDeque<usize>,
}

impl Chunker {
    pub(super) fn push(&mut self, samples: &[f32]) -> Vec<Chunk> {
        for frame in samples.chunks(FRAME) {
            if let Some(cut) = self.segmenter.push(frame) {
                self.pauses.push_back(cut);
            }
        }
        self.buffer.extend_from_slice(samples);
        std::iter::from_fn(|| self.next_chunk()).collect()
    }

    pub(super) fn finish(self) -> Option<Chunk> {
        self.rest(MIN_TAIL)
    }

    /// The audio not yet in a chunk, as one last chunk, when at least
    /// `min_tail` samples follow the previous chunk; leaves the chunker as
    /// it was.
    fn rest(&self, min_tail: usize) -> Option<Chunk> {
        let begin = self.chunk_start - self.overlap;
        (self.end() >= self.chunk_start + min_tail.max(1)).then(|| Chunk {
            start: begin,
            overlap: self.overlap,
            tail: 0,
            samples: self.buffer[begin - self.buffer_start..].to_vec(),
        })
    }

    fn end(&self) -> usize {
        self.buffer_start + self.buffer.len()
    }

    fn next_chunk(&mut self) -> Option<Chunk> {
        let start = self.chunk_start;
        let end = self.end();
        self.pauses.retain(|&cut| cut >= start + CHUNK_MIN);
        let limit = start + CHUNK_MAX;
        let preferred = self
            .pauses
            .iter()
            .copied()
            .find(|&cut| cut >= start + CHUNK_TARGET && cut <= limit.min(end));
        let (cut, next_overlap) = match preferred {
            Some(cut) => (cut, 0),
            None if end >= limit => self
                .pauses
                .iter()
                .copied()
                .rfind(|&cut| cut <= limit)
                .map_or((limit, CHUNK_OVERLAP), |cut| (cut, 0)),
            None => return None,
        };
        let begin = start - self.overlap;
        let chunk = Chunk {
            start: begin,
            overlap: self.overlap,
            tail: next_overlap,
            samples: self.buffer[begin - self.buffer_start..cut - self.buffer_start].to_vec(),
        };
        let keep_from = cut - next_overlap;
        self.buffer.drain(..keep_from - self.buffer_start);
        self.buffer_start = keep_from;
        self.chunk_start = cut;
        self.overlap = next_overlap;
        Some(chunk)
    }
}

/// Transcribes a chunk unless the speech detector finds no speech in it.
/// Models invent text for silence and noise, so a speechless chunk becomes an
/// empty transcript; accepting it still advances the track's coverage.
fn transcribe_speech(
    samples: &[f32],
    transcribe: impl FnOnce() -> Result<Transcript>,
) -> Result<Transcript> {
    if speechless(samples) {
        return Ok(Transcript::default());
    }
    transcribe()
}

/// Whether the speech detector heard no speech in `samples`, so they skip the
/// model.
fn speechless(samples: &[f32]) -> bool {
    let speechless = crate::transcription::speech_detector::contains_speech(samples) == Some(false);
    if speechless {
        tracing::debug!(
            audio_ms = samples_ms(samples.len()),
            "skipped meeting chunk without detected speech"
        );
    }
    speechless
}

/// One source's entries in capture-clock milliseconds.
#[derive(Clone)]
pub(super) struct TrackTranscript {
    source: MeetingSource,
    origin_ms: u64,
    entries: Vec<TranscriptEntry>,
    /// Timed words, in capture-clock milliseconds, of a sentence the next
    /// chunk can still change: one that runs into its overlap.
    words: Vec<TranscriptSegment>,
    /// Capture-clock time before which entries are final: the next chunk's
    /// seam replaces only entries that start after it.
    settled_ms: u64,
    /// Where a flush for a copy cut this track: a sentence always ends
    /// there, so the copy's last sentence and the rest of it, published
    /// later, are separate entries the paste cursor tells apart.
    breaks: Vec<i64>,
}

impl TrackTranscript {
    fn new(source: MeetingSource, origin_ms: u64) -> Self {
        Self {
            source,
            origin_ms,
            entries: Vec::new(),
            words: Vec::new(),
            settled_ms: origin_ms,
            breaks: Vec::new(),
        }
    }

    fn accept(&mut self, chunk: &Chunk, transcript: Transcript) {
        let chunk_ms = self.origin_ms + samples_ms(chunk.start);
        let duration_ms = samples_ms(chunk.samples.len());
        let next_seam = chunk_ms + duration_ms - samples_ms(chunk.tail) / 2;
        self.settled_ms = self.settled_ms.max(next_seam);
        let untimed = !transcript.segments.is_empty() || !transcript.text.trim().is_empty();
        if !transcript.words.is_empty() || (!untimed && !self.words.is_empty()) {
            self.accept_words(chunk, chunk_ms, duration_ms, transcript.words);
            self.publish_sentences(next_seam, chunk.tail == 0);
            return;
        }
        // A model without word timings (Apple Speech) took over mid-track.
        self.finish();
        let segments = transcript.segments;
        let segmented = !segments.is_empty();
        let mut incoming: Vec<TranscriptEntry> = if segmented {
            segments
                .into_iter()
                .filter_map(|segment| {
                    let text = segment.text.trim().to_string();
                    let start = (segment.start_ms.max(0) as u64).min(duration_ms);
                    let end = (segment.end_ms.max(0) as u64).min(duration_ms);
                    (!text.is_empty() && end > start).then(|| TranscriptEntry {
                        source: self.source,
                        start_ms: chunk_ms + start,
                        end_ms: chunk_ms + end,
                        text,
                    })
                })
                .collect()
        } else {
            let text = transcript.text.trim();
            (!text.is_empty())
                .then(|| TranscriptEntry {
                    source: self.source,
                    start_ms: chunk_ms,
                    end_ms: chunk_ms + duration_ms,
                    text: text.into(),
                })
                .into_iter()
                .collect()
        };
        if chunk.overlap > 0 {
            if segmented {
                let seam = chunk_ms + samples_ms(chunk.overlap) / 2;
                self.entries.retain(|entry| entry.start_ms < seam);
                incoming.retain(|entry| entry.start_ms >= seam);
            }
            if let (Some(previous), Some(first)) = (self.entries.last(), incoming.first_mut()) {
                first.text = strip_repeated_prefix(&previous.text, &first.text);
            }
            incoming.retain(|entry| !entry.text.is_empty());
        }
        self.entries.extend(incoming);
    }

    /// Joins a chunk's timed words at the seam word by word, so a sentence
    /// that runs across a hard cut keeps every word from one side or the
    /// other.
    fn accept_words(
        &mut self,
        chunk: &Chunk,
        chunk_ms: u64,
        duration_ms: u64,
        words: Vec<TranscriptSegment>,
    ) {
        let mut incoming = words
            .into_iter()
            .filter_map(|word| {
                let text = word.text.trim().to_string();
                let start = (word.start_ms.max(0) as u64).min(duration_ms);
                let end = (word.end_ms.max(0) as u64).clamp(start, duration_ms);
                (!text.is_empty()).then(|| TranscriptSegment {
                    start_ms: (chunk_ms + start) as i64,
                    end_ms: (chunk_ms + end) as i64,
                    text,
                })
            })
            .collect::<Vec<_>>();
        if chunk.overlap > 0
            && let Some((kept, skipped)) =
                seam_anchor(&self.words, &incoming, chunk_ms, samples_ms(chunk.overlap))
        {
            self.words.truncate(kept);
            incoming.drain(..skipped);
        } else if chunk.overlap > 0 {
            let seam = (chunk_ms + samples_ms(chunk.overlap) / 2) as i64;
            self.words.retain(|word| word.start_ms < seam);
            incoming.retain(|word| word.start_ms >= seam);
            let previous = if self.words.is_empty() {
                self.entries
                    .last()
                    .map(|entry| entry.text.split_whitespace().map(normalize_word).collect())
                    .unwrap_or_default()
            } else {
                self.words
                    .iter()
                    .map(|word| normalize_word(&word.text))
                    .collect::<Vec<_>>()
            };
            let next = incoming
                .iter()
                .map(|word| normalize_word(&word.text))
                .collect::<Vec<_>>();
            incoming.drain(..repeated_prefix(&previous, &next));
        }
        self.words.extend(incoming);
    }

    /// Publishes every pending word, as when no chunk follows.
    fn finish(&mut self) {
        if let Some(end) = self.words.iter().map(|word| word.end_ms).max() {
            self.settled_ms = self.settled_ms.max(end as u64 + 1);
        }
        self.publish_sentences(self.settled_ms, true);
    }

    /// Moves sentences no later chunk can change into the entries: every
    /// sentence when nothing overlaps the next chunk, else those wholly
    /// before `seam_ms` and closed by punctuation or a later sentence.
    fn publish_sentences(&mut self, seam_ms: u64, all: bool) {
        let seam_ms = seam_ms as i64;
        let groups = sentence_groups(&self.words, &self.breaks);
        let published = if all {
            groups.len()
        } else {
            groups
                .iter()
                .enumerate()
                .take_while(|(index, group)| {
                    let before_seam = |word: &TranscriptSegment| word.start_ms < seam_ms;
                    self.words[group.start..group.end].iter().all(before_seam)
                        && (self.words[group.end - 1].text.ends_with(['.', '?', '!'])
                            || groups
                                .get(index + 1)
                                .is_some_and(|next| before_seam(&self.words[next.start])))
                })
                .count()
        };
        let end = groups
            .get(published)
            .map_or(self.words.len(), |group| group.start);
        let mut words = self.words.drain(..end).collect::<Vec<_>>();
        if let Some(last) = words.last() {
            let last = word_middle(last);
            self.breaks.retain(|&cut| cut > last);
        }
        for group in groups.into_iter().take(published) {
            let group = words.drain(..group.len()).collect::<Vec<_>>();
            self.entries.push(TranscriptEntry {
                source: self.source,
                start_ms: group[0].start_ms as u64,
                end_ms: group
                    .iter()
                    .map(|word| word.end_ms)
                    .max()
                    .unwrap_or_default() as u64,
                text: group
                    .iter()
                    .map(|word| word.text.as_str())
                    .collect::<Vec<_>>()
                    .join(" "),
            });
        }
    }
}

/// The middle of a word. Word boundaries move by tens of milliseconds when
/// a later chunk hears more context, so a cut is placed by word middles.
fn word_middle(word: &TranscriptSegment) -> i64 {
    (word.start_ms + word.end_ms) / 2
}

/// Word ranges of each sentence in `words`: a sentence ends after sentence
/// punctuation, before a pause, or at one of `breaks` (the end of the last
/// word a copy took), between the words whose middles lie either side of it.
fn sentence_groups(words: &[TranscriptSegment], breaks: &[i64]) -> Vec<std::ops::Range<usize>> {
    let mut groups: Vec<std::ops::Range<usize>> = Vec::new();
    for (index, word) in words.iter().enumerate() {
        match groups.last_mut() {
            Some(group)
                if !words[group.end - 1].text.ends_with(['.', '?', '!'])
                    && !breaks.iter().any(|&cut| {
                        word_middle(&words[group.end - 1]) < cut && cut <= word_middle(word)
                    })
                    && word.start_ms.saturating_sub(
                        words[group.clone()]
                            .iter()
                            .map(|word| word.end_ms)
                            .max()
                            .unwrap_or_default(),
                    ) < SENTENCE_PAUSE_MS =>
            {
                group.end = index + 1;
            }
            _ => groups.push(index..index + 1),
        }
    }
    groups
}

/// A word both chunks heard inside their overlap, at nearly the same time,
/// closest to the overlap's middle: the earlier chunk keeps it and the words
/// before it, the later chunk supplies what follows. Returns how many pending
/// words to keep and how many incoming words to skip. Splicing at a shared
/// word neither drops nor repeats a word whose timestamp shifts across the
/// middle between the two chunks.
fn seam_anchor(
    previous: &[TranscriptSegment],
    incoming: &[TranscriptSegment],
    overlap_start_ms: u64,
    overlap_ms: u64,
) -> Option<(usize, usize)> {
    let start = overlap_start_ms as i64;
    let end = start + overlap_ms as i64;
    let middle = start + overlap_ms as i64 / 2;
    let in_overlap = |word: &TranscriptSegment| word.start_ms >= start && word.start_ms < end;
    let mut best: Option<(i64, usize, usize)> = None;
    for (kept, old) in previous
        .iter()
        .enumerate()
        .filter(|(_, word)| in_overlap(word))
    {
        let old_text = normalize_word(&old.text);
        if old_text.is_empty() {
            continue;
        }
        for (skipped, new) in incoming
            .iter()
            .enumerate()
            .filter(|(_, word)| in_overlap(word))
        {
            if (old.start_ms - new.start_ms).abs() <= SEAM_ANCHOR_MS
                && normalize_word(&new.text) == old_text
            {
                let distance = (old.start_ms - middle).abs();
                if best.is_none_or(|(best, _, _)| distance < best) {
                    best = Some((distance, kept + 1, skipped + 1));
                }
            }
        }
    }
    best.map(|(_, kept, skipped)| (kept, skipped))
}

/// Removes words at the start of `next` that repeat the end of `previous`.
fn strip_repeated_prefix(previous: &str, next: &str) -> String {
    let previous_words = previous
        .split_whitespace()
        .map(normalize_word)
        .collect::<Vec<_>>();
    let next_words = next.split_whitespace().collect::<Vec<_>>();
    let normalized_next = next_words
        .iter()
        .map(|word| normalize_word(word))
        .collect::<Vec<_>>();
    next_words[repeated_prefix(&previous_words, &normalized_next)..].join(" ")
}

/// How many words (two to twelve) at the start of `next` repeat the end of
/// `previous`.
fn repeated_prefix(previous: &[String], next: &[String]) -> usize {
    let longest = previous.len().min(next.len()).min(SEAM_MAX_WORDS);
    (2..=longest)
        .rev()
        .find(|&count| previous[previous.len() - count..] == next[..count])
        .unwrap_or(0)
}

fn normalize_word(word: &str) -> String {
    word.chars()
        .filter(|character| character.is_alphanumeric())
        .flat_map(char::to_lowercase)
        .collect()
}

fn tokens(text: &str) -> Vec<String> {
    text.split(|character: char| !character.is_alphanumeric())
        .filter(|token| !token.is_empty())
        .map(str::to_lowercase)
        .collect()
}

fn tokens_match(left: &str, right: &str) -> bool {
    if left == right {
        return true;
    }
    let left = left.chars().collect::<Vec<_>>();
    let right = right.chars().collect::<Vec<_>>();
    if left.len() < 3 || right.len() < 3 {
        return false;
    }
    let distance = edit_distance(&left, &right);
    1.0 - distance as f32 / left.len().max(right.len()) as f32 >= ECHO_SIMILARITY
}

fn edit_distance(left: &[char], right: &[char]) -> usize {
    let mut previous = (0..=right.len()).collect::<Vec<_>>();
    let mut current = vec![0; right.len() + 1];
    for (row, left_char) in left.iter().enumerate() {
        current[0] = row + 1;
        for (column, right_char) in right.iter().enumerate() {
            let substitution = previous[column] + usize::from(left_char != right_char);
            current[column + 1] = substitution
                .min(previous[column + 1] + 1)
                .min(current[column] + 1);
        }
        std::mem::swap(&mut previous, &mut current);
    }
    previous[right.len()]
}

/// Drops microphone utterances that echo remote speech, which happens when
/// the call plays through speakers. An utterance is echo when the signal
/// says delayed system audio explains it, or when its words repeat
/// overlapping remote speech unless the signal shows local speech
/// (double-talk). `signal` classifies a microphone time span. Returns the
/// dropped count.
pub(super) fn suppress_echo(
    entries: Vec<TranscriptEntry>,
    signal: impl Fn(u64, u64) -> Option<Signal>,
) -> (Vec<TranscriptEntry>, u64) {
    let system = entries
        .iter()
        .filter(|entry| entry.source == MeetingSource::System)
        .collect::<Vec<_>>();
    let mut microphone = entries
        .iter()
        .enumerate()
        .filter(|(_, entry)| entry.source == MeetingSource::Microphone)
        .collect::<Vec<_>>();
    microphone.sort_by_key(|(_, entry)| entry.start_ms);
    let mut utterances: Vec<Vec<(usize, &TranscriptEntry)>> = Vec::new();
    for (index, entry) in microphone {
        match utterances.last_mut() {
            Some(utterance)
                if entry
                    .start_ms
                    .saturating_sub(utterance.last().unwrap().1.end_ms)
                    <= UTTERANCE_GAP_MS =>
            {
                utterance.push((index, entry));
            }
            _ => utterances.push(vec![(index, entry)]),
        }
    }
    let is_echo = |spoken: &[String], start_ms: u64, end_ms: u64, judge_words: bool| match signal(
        start_ms, end_ms,
    ) {
        Some(Signal::Bleed) => true,
        Some(Signal::Local) => false,
        None => judge_words && echo_score(spoken, start_ms, end_ms, &system) >= ECHO_SIMILARITY,
    };
    let mut dropped = vec![false; entries.len()];
    let mut dropped_utterances = 0;
    for utterance in utterances {
        let spoken = utterance
            .iter()
            .flat_map(|(_, entry)| tokens(&entry.text))
            .collect::<Vec<_>>();
        let end = utterance
            .iter()
            .map(|(_, entry)| entry.end_ms)
            .max()
            .unwrap_or_default();
        if is_echo(&spoken, utterance[0].1.start_ms, end, true) {
            dropped_utterances += 1;
            // Bleed can run straight into the user's own sentence; a sentence
            // the signal shows is local survives its echoing neighbours.
            for (index, entry) in utterance {
                dropped[index] = tokens(&entry.text).len() < SENTENCE_WORDS
                    || signal(entry.start_ms, entry.end_ms) != Some(Signal::Local);
            }
            continue;
        }
        if utterance.len() == 1 {
            continue;
        }
        for (index, entry) in utterance {
            let spoken = tokens(&entry.text);
            if is_echo(
                &spoken,
                entry.start_ms,
                entry.end_ms,
                spoken.len() >= SENTENCE_WORDS,
            ) {
                dropped_utterances += 1;
                dropped[index] = true;
            }
        }
    }
    let kept = entries
        .into_iter()
        .zip(dropped)
        .filter_map(|(entry, dropped)| (!dropped).then_some(entry))
        .collect();
    (kept, dropped_utterances)
}

/// The share of `spoken` that remote speech within the echo window contains.
fn echo_score(spoken: &[String], start_ms: u64, end_ms: u64, remote: &[&TranscriptEntry]) -> f32 {
    let start = start_ms.saturating_sub(ECHO_WINDOW_MS);
    let end = end_ms + ECHO_WINDOW_MS;
    let remote = remote
        .iter()
        .filter(|entry| entry.start_ms < end && start < entry.end_ms)
        .flat_map(|entry| tokens(&entry.text))
        .collect::<Vec<_>>();
    if remote.is_empty() || spoken.is_empty() {
        return 0.0;
    }
    let matched = spoken
        .iter()
        .filter(|token| remote.iter().any(|remote| tokens_match(token, remote)))
        .count();
    matched as f32 / spoken.len() as f32
}

/// Removes echo from both sources' entries, then moves them onto the
/// meeting timeline. Entries and the envelope share the capture clock.
fn join_entries(
    mut entries: Vec<TranscriptEntry>,
    origin_ms: u64,
    envelope: &EchoEnvelope,
) -> (Vec<TranscriptEntry>, u64) {
    entries.sort_by_key(|entry| entry.start_ms);
    let (mut entries, dropped) = suppress_echo(entries, |start, end| envelope.assess(start, end));
    for entry in &mut entries {
        entry.start_ms = entry.start_ms.saturating_sub(origin_ms);
        entry.end_ms = entry.end_ms.saturating_sub(origin_ms);
    }
    (entries, dropped)
}

fn samples_ms(samples: usize) -> u64 {
    samples as u64 * 1_000 / RATE as u64
}

fn source_index(source: MeetingSource) -> usize {
    match source {
        MeetingSource::System => 0,
        MeetingSource::Microphone => 1,
    }
}

fn wait_for_dictation() {
    let started = Instant::now();
    while crate::transcription::engine::dictation_busy()
        && !crate::process_exit::quitting()
        && started.elapsed() < DICTATION_WAIT
    {
        thread::sleep(Duration::from_millis(50));
    }
}

/// Transcribes meeting chunks. In the app this borrows the dictation
/// engine's loaded model between dictations, at utility QoS, so a meeting
/// never loads a second copy. A process without an engine, such as the CLI,
/// loads its own.
pub(super) struct ChunkModel {
    selection: TranscriptionSelection,
    owned: Option<Transcriber>,
    unavailable: bool,
}

enum Chunked {
    Done(Transcript),
    /// A dictation request or live stream came first; retry once it is idle.
    Preempted,
    /// No model could be loaded.
    Unavailable(color_eyre::Report),
}

impl ChunkModel {
    pub(super) fn new(selection: TranscriptionSelection) -> Self {
        Self {
            selection,
            owned: None,
            unavailable: false,
        }
    }

    /// Fails with the chunk's own transcription error. A chunk without
    /// detected speech returns an empty transcript without the model.
    fn transcribe(&mut self, samples: &[f32]) -> Result<Chunked> {
        // Quitting drops the dictation engine; a model loaded now would
        // outlast the drain.
        if crate::process_exit::quitting() {
            return Ok(Chunked::Unavailable(eyre!("Hex is quitting")));
        }
        if speechless(samples) {
            return Ok(Chunked::Done(Transcript::default()));
        }
        let engine = crate::transcription::engine::TranscriptionEngine::shared();
        if let Some(engine) = &engine
            && let Some(result) = Self::transcribe_shared(engine, samples)
        {
            self.owned = None;
            return result;
        }
        // In the app, follow dictation's current choice, such as Apple Speech.
        let selection = if engine.is_some() {
            crate::app_settings::transcription_selection().1
        } else {
            self.selection.clone()
        };
        drop(engine);
        if self
            .owned
            .as_ref()
            .is_some_and(|model| !model.matches_selection(&selection))
        {
            self.owned = None;
            self.unavailable = false;
        }
        if self.unavailable {
            return Ok(Chunked::Unavailable(eyre!(
                "meeting transcriber is unavailable"
            )));
        }
        if crate::process_exit::quitting() {
            return Ok(Chunked::Unavailable(eyre!("Hex is quitting")));
        }
        let model = match load_model(&mut self.owned, &selection) {
            Ok(model) => model,
            Err(error) => {
                self.unavailable = true;
                return Ok(Chunked::Unavailable(error));
            }
        };
        model
            .transcribe_segments(samples.to_vec())
            .map(Chunked::Done)
    }

    /// `None` when the engine has no GGUF model loaded.
    fn transcribe_shared(
        engine: &crate::transcription::engine::TranscriptionEngine,
        samples: &[f32],
    ) -> Option<Result<Chunked>> {
        let samples = samples.to_vec();
        enum Shared {
            Busy,
            NoModel,
            Ran(Result<Transcript>),
        }
        let ran = engine.background(move |model| {
            // A capture that began after the meeting checked comes first.
            if crate::transcription::engine::dictation_busy() {
                return Shared::Busy;
            }
            match engine_model(model) {
                Some(transcriber) => Shared::Ran(
                    crate::transcription::background_priority::at_background_priority(|| {
                        transcriber.transcribe_segments(samples)
                    }),
                ),
                None => Shared::NoModel,
            }
        });
        match ran {
            Ok(None | Some(Shared::Busy)) => Some(Ok(Chunked::Preempted)),
            Ok(Some(Shared::Ran(result))) => Some(result.map(Chunked::Done)),
            Ok(Some(Shared::NoModel)) | Err(_) => None,
        }
    }

    /// Transcribes `samples`, waiting out dictation as often as it preempts.
    fn transcribe_blocking(&mut self, samples: &[f32]) -> Result<Transcript> {
        loop {
            wait_for_dictation();
            match self.transcribe(samples)? {
                Chunked::Done(transcript) => return Ok(transcript),
                Chunked::Preempted => thread::sleep(PREEMPTED_RETRY),
                Chunked::Unavailable(error) => return Err(error),
            }
        }
    }
}

/// The engine's model when meetings can share it. Apple Speech runs outside
/// this process, so a meeting keeps its own rather than queue dictation
/// behind it.
#[cfg(target_os = "macos")]
fn engine_model(model: &mut crate::transcription::engine::Model) -> Option<&mut Transcriber> {
    match model.active() {
        Some(transcriber @ Transcriber::Gguf(_)) => Some(transcriber),
        _ => None,
    }
}

#[cfg(not(target_os = "macos"))]
fn engine_model(model: &mut crate::transcription::engine::Model) -> Option<&mut Transcriber> {
    Some(model)
}

/// What the rolling worker transcribes chunks with: [`ChunkModel`], or a
/// fake in tests.
trait ChunkTranscriber {
    /// `chunk.start` is the chunk's 16 kHz offset in its track.
    fn transcribe_chunk(&mut self, chunk: &Chunk) -> Result<Chunked>;
}

impl ChunkTranscriber for ChunkModel {
    fn transcribe_chunk(&mut self, chunk: &Chunk) -> Result<Chunked> {
        self.transcribe(&chunk.samples)
    }
}

fn load_model<'a>(
    model: &'a mut Option<Transcriber>,
    selection: &TranscriptionSelection,
) -> Result<&'a mut Transcriber> {
    if model.is_none() {
        #[cfg(test)]
        tests::MODEL_LOADS.fetch_add(1, Ordering::SeqCst);
        #[cfg(target_os = "macos")]
        let loaded = Transcriber::load_selection(selection)?;
        #[cfg(not(target_os = "macos"))]
        let loaded = Transcriber::load(selection)?;
        *model = Some(loaded);
    }
    model
        .as_mut()
        .ok_or_else(|| eyre!("meeting transcriber is unavailable"))
}

struct RollingTrack {
    resampler: Option<StreamingResampler>,
    sample_rate: u32,
    chunker: Chunker,
    /// 16 kHz track offset where the current chunker began, after any gap.
    base: usize,
    transcript: TrackTranscript,
    /// 16 kHz samples of the track so far, gaps included.
    fed: usize,
    /// Track position, in source samples, expected of the next packet.
    next_position: u64,
    /// Untranscribed 16 kHz samples queued for this track.
    pending: usize,
    failed: bool,
}

impl RollingTrack {
    fn push(
        &mut self,
        index: usize,
        samples: &[f32],
        envelope: &mut EchoEnvelope,
        pending: &mut VecDeque<(usize, Chunk)>,
    ) {
        envelope.push(
            self.transcript.source,
            self.transcript.origin_ms + samples_ms(self.fed),
            samples,
            RATE as u32,
        );
        self.fed += samples.len();
        for mut chunk in self.chunker.push(samples) {
            chunk.start += self.base;
            self.pending += chunk.samples.len();
            pending.push_back((index, chunk));
        }
    }

    /// Queues the rest of the audio, as at Stop or before a gap.
    fn end(
        &mut self,
        index: usize,
        envelope: &mut EchoEnvelope,
        pending: &mut VecDeque<(usize, Chunk)>,
    ) {
        let tail = self
            .resampler
            .take()
            .map(StreamingResampler::finish)
            .unwrap_or_default();
        self.push(index, &tail, envelope, pending);
        if let Some(mut chunk) = std::mem::take(&mut self.chunker).finish() {
            chunk.start += self.base;
            self.pending += chunk.samples.len();
            pending.push_back((index, chunk));
        }
    }
}

/// Transcribes short chunks while the meeting is still recording. Its
/// snapshot is the in-call transcript, and the final transcript reuses it
/// shortly after Stop.
pub(super) struct Rolling {
    sender: Option<SyncSender<RollingInput>>,
    flushes: Option<Receiver<FlushRequest>>,
    flusher: RollingFlusher,
    incomplete: Arc<[AtomicBool; 2]>,
    worker: JoinHandle<RollingOutcome>,
}

/// What the capture writer sends the rolling worker, in capture order.
enum RollingInput {
    Audio(AudioPacket),
    /// Transcribe everything sent before this, for a copy.
    Flush(FlushRequest),
}

struct FlushRequest {
    deadline: Instant,
    reply: SyncSender<Vec<TranscriptEntry>>,
}

pub(super) struct RollingFeed {
    sender: SyncSender<RollingInput>,
    flushes: Receiver<FlushRequest>,
    incomplete: Arc<[AtomicBool; 2]>,
    /// Per-source audio gathered into blocks of about a second, so the
    /// bounded queue holds minutes of audio rather than ScreenCaptureKit's
    /// many small packets.
    blocks: [Option<AudioPacket>; 2],
}

impl RollingFeed {
    /// Forwards without blocking. A full queue skips that block: the source
    /// continues after the gap, and the final pass rebuilds it from its WAV.
    pub(super) fn offer(&mut self, packet: &AudioPacket) {
        let index = source_index(packet.source);
        if self.blocks[index].as_ref().is_some_and(|block| {
            block.sample_rate != packet.sample_rate
                || block.position + block.samples.len() as u64 != packet.position
        }) {
            self.send(index);
        }
        let block = self.blocks[index].get_or_insert_with(|| AudioPacket {
            source: packet.source,
            pts_us: packet.pts_us,
            arrival_us: packet.arrival_us,
            position: packet.position,
            sample_rate: packet.sample_rate,
            samples: Vec::with_capacity(packet.sample_rate as usize),
        });
        block.samples.extend_from_slice(&packet.samples);
        if block.samples.len() >= block.sample_rate as usize {
            self.send(index);
        }
        self.forward_copies();
    }

    /// Sends partial blocks and any copies waiting; call once capture ends.
    pub(super) fn flush(&mut self) {
        self.send_blocks();
        self.forward_copies();
    }

    fn send_blocks(&mut self) {
        self.send(0);
        self.send(1);
    }

    /// Forwards waiting copies behind the partial blocks, so each covers
    /// every sample received so far.
    fn forward_copies(&mut self) {
        while let Ok(request) = self.flushes.try_recv() {
            self.send_blocks();
            if self.sender.try_send(RollingInput::Flush(request)).is_err() {
                tracing::warn!(
                    "rolling meeting transcription is behind; a copy gets what is ready"
                );
            }
        }
    }

    fn send(&mut self, index: usize) {
        let Some(block) = self.blocks[index].take() else {
            return;
        };
        let source = block.source;
        if self.sender.try_send(RollingInput::Audio(block)).is_err()
            && !self.incomplete[index].swap(true, Ordering::Relaxed)
        {
            tracing::warn!(
                ?source,
                "rolling meeting transcription fell behind; skipping audio"
            );
        }
    }
}

/// Asks a recording's rolling worker for its transcript up to now.
#[derive(Clone)]
pub(super) struct RollingFlusher(SyncSender<FlushRequest>);

impl RollingFlusher {
    /// Transcribes everything captured so far and returns the joined
    /// entries, as the in-call transcript will show them, or `None` when
    /// that takes longer than `wait`.
    /// Quitting ends the wait.
    pub(super) fn transcribe_up_to_now(&self, wait: Duration) -> Option<Vec<TranscriptEntry>> {
        const STEP: Duration = Duration::from_millis(50);
        if crate::process_exit::quitting() {
            return None;
        }
        let deadline = Instant::now() + wait;
        let (reply, entries) = mpsc::sync_channel(1);
        self.0.try_send(FlushRequest { deadline, reply }).ok()?;
        loop {
            let left = deadline.saturating_duration_since(Instant::now());
            match entries.recv_timeout(left.min(STEP)) {
                Ok(entries) => return Some(entries),
                Err(RecvTimeoutError::Timeout)
                    if !left.is_zero() && !crate::process_exit::quitting() => {}
                Err(_) => return None,
            }
        }
    }
}

pub(super) struct RollingOutcome {
    tracks: [Option<TrackTranscript>; 2],
    /// Where copies cut each track, so a track rebuilt from its WAV splits
    /// its sentences at the same places.
    cuts: [Vec<i64>; 2],
    envelope: EchoEnvelope,
    model: ChunkModel,
}

impl Rolling {
    pub(super) fn start(directory: PathBuf, selection: TranscriptionSelection) -> Self {
        let (sender, receiver) = mpsc::sync_channel(ROLLING_QUEUE_CAPACITY);
        let (flusher, flushes) = mpsc::sync_channel(4);
        let incomplete = Arc::new([AtomicBool::new(false), AtomicBool::new(false)]);
        let worker_incomplete = incomplete.clone();
        let worker = thread::Builder::new()
            .name("meeting-transcription".into())
            .spawn(move || run_rolling(&directory, &selection, receiver, &worker_incomplete))
            .expect("meeting transcription thread must start");
        Self {
            sender: Some(sender),
            flushes: Some(flushes),
            flusher: RollingFlusher(flusher),
            incomplete,
            worker,
        }
    }

    pub(super) fn feed(&mut self) -> Option<RollingFeed> {
        Some(RollingFeed {
            sender: self.sender.take()?,
            flushes: self.flushes.take()?,
            incomplete: self.incomplete.clone(),
            blocks: [None, None],
        })
    }

    pub(super) fn flusher(&self) -> RollingFlusher {
        self.flusher.clone()
    }

    /// Waits for the worker after the capture writer dropped its feed.
    pub(super) fn finish(mut self) -> Option<RollingOutcome> {
        self.sender.take();
        self.worker.join().ok()
    }
}

/// The recording in this process that a copy can flush.
static FLUSHERS: std::sync::Mutex<Vec<(String, RollingFlusher)>> =
    std::sync::Mutex::new(Vec::new());

/// Makes a recording's flusher reachable by its meeting id until dropped.
pub(super) struct FlushRegistration(String);

impl Drop for FlushRegistration {
    fn drop(&mut self) {
        FLUSHERS
            .lock()
            .unwrap_or_else(|error| error.into_inner())
            .retain(|(id, _)| *id != self.0);
    }
}

pub(super) fn register_flusher(id: &str, flusher: RollingFlusher) -> FlushRegistration {
    FLUSHERS
        .lock()
        .unwrap_or_else(|error| error.into_inner())
        .push((id.to_string(), flusher));
    FlushRegistration(id.to_string())
}

pub(super) enum Flushed {
    /// The transcript through the request.
    Fresh(Vec<TranscriptEntry>),
    /// The worker did not answer in time.
    TimedOut,
    /// This process is not recording that meeting.
    NotRecording,
}

/// Transcribes meeting `id` up to now when this process records it.
pub(super) fn transcribe_up_to_now(id: &str, wait: Duration) -> Flushed {
    let flusher = FLUSHERS
        .lock()
        .unwrap_or_else(|error| error.into_inner())
        .iter()
        .find(|(recording, _)| recording == id)
        .map(|(_, flusher)| flusher.clone());
    match flusher.map(|flusher| flusher.transcribe_up_to_now(wait)) {
        Some(Some(entries)) => Flushed::Fresh(entries),
        Some(None) => Flushed::TimedOut,
        None => Flushed::NotRecording,
    }
}

/// The rolling worker's state between inputs.
struct RollingWorker<'a, M> {
    directory: &'a Path,
    tracks: [Option<RollingTrack>; 2],
    pending: VecDeque<(usize, Chunk)>,
    envelope: EchoEnvelope,
    model: M,
    model_failed: bool,
    microphone_shown_ms: u64,
    /// Every copy's cut per track, capture-clock, for a track rebuilt from
    /// its WAV after Stop.
    cuts: [Vec<i64>; 2],
}

impl<'a, M: ChunkTranscriber> RollingWorker<'a, M> {
    fn new(directory: &'a Path, model: M) -> Self {
        Self {
            directory,
            tracks: [None, None],
            pending: VecDeque::new(),
            envelope: EchoEnvelope::default(),
            model,
            model_failed: false,
            microphone_shown_ms: 0,
            cuts: [Vec::new(), Vec::new()],
        }
    }

    fn ingest(&mut self, incomplete: &[AtomicBool; 2], packet: AudioPacket) {
        ingest(
            &mut self.tracks,
            &mut self.pending,
            &mut self.envelope,
            incomplete,
            packet,
        );
    }

    /// Transcribes the oldest pending chunk; `false` when dictation
    /// preempted it.
    fn transcribe_next(&mut self) -> bool {
        transcribe_pending(
            self.directory,
            &mut self.tracks,
            &mut self.pending,
            &mut self.model,
            &mut self.model_failed,
        )
    }

    fn write_snapshot(&mut self) {
        write_rolling_snapshot(
            self.directory,
            &self.tracks,
            &mut self.microphone_shown_ms,
            &self.envelope,
        );
    }

    /// Answers a copy with the transcript through everything received: the
    /// queued chunks as usual, then each track's audio not yet in a chunk,
    /// transcribed into a copy of its transcript so no later chunk is
    /// affected. Like every meeting chunk it waits while dictation is busy,
    /// and it gives up, without replying, at the request's deadline.
    fn serve_flush(&mut self, request: FlushRequest) {
        let started = Instant::now();
        let deadline = request.deadline;
        let wait = || {
            thread::sleep(PREEMPTED_RETRY.min(deadline.saturating_duration_since(Instant::now())))
        };
        let give_up = |model_failed: bool| {
            model_failed || crate::process_exit::quitting() || Instant::now() >= deadline
        };
        while !self.pending.is_empty() {
            if give_up(self.model_failed) {
                return;
            }
            if crate::transcription::engine::dictation_busy() {
                wait();
            } else if self.transcribe_next() {
                self.write_snapshot();
            } else {
                wait();
            }
        }
        let mut provisional = self.tracks.each_ref().map(|track| {
            track
                .as_ref()
                .filter(|track| !track.failed)
                .map(|track| track.transcript.clone())
        });
        for (track, copy) in self.tracks.iter().zip(&mut provisional) {
            let (Some(track), Some(copy)) = (track, copy) else {
                continue;
            };
            // Every sample up to the request, however little follows the
            // last chunk: a word straddling a hard cut is heard whole.
            if let Some(mut chunk) = track.chunker.rest(1) {
                chunk.start += track.base;
                let transcript = if chunk.is_silent() {
                    Transcript::default()
                } else {
                    loop {
                        if give_up(false) {
                            return;
                        }
                        if crate::transcription::engine::dictation_busy() {
                            wait();
                            continue;
                        }
                        match self.model.transcribe_chunk(&chunk) {
                            Ok(Chunked::Done(transcript)) => break transcript,
                            Ok(Chunked::Preempted) => wait(),
                            Ok(Chunked::Unavailable(error)) | Err(error) => {
                                tracing::warn!(%error, "could not transcribe the meeting up to a copy");
                                return;
                            }
                        }
                    }
                };
                copy.accept(&chunk, transcript);
            }
            // Everything fed is transcribed now, so microphone text waits
            // for the system track only through the audio it has.
            copy.settled_ms = copy.settled_ms.max(copy.origin_ms + samples_ms(track.fed));
            copy.finish();
        }
        let view = rolling_view(
            provisional.each_ref().map(Option::as_ref),
            system_advancing(&self.tracks),
            self.microphone_shown_ms,
            &self.envelope,
        );
        tracing::info!(
            elapsed_ms = started.elapsed().as_millis() as u64,
            entries = view.entries.len(),
            "transcribed the meeting up to a copy"
        );
        // The view is on the meeting's timeline; breaks are capture-clock.
        let origin_ms = view_origin_ms(provisional.each_ref().map(Option::as_ref));
        let cuts = [MeetingSource::System, MeetingSource::Microphone].map(|source| {
            view.entries
                .iter()
                .filter(|entry| entry.source == source)
                .map(|entry| (origin_ms + entry.end_ms) as i64)
                .max()
        });
        if request.reply.try_send(view.entries).is_err() {
            return;
        }
        // The copy ends at each source's last word; the rest of a sentence
        // running past it is new for the next copy.
        for ((track, all), cut) in self.tracks.iter_mut().zip(&mut self.cuts).zip(cuts) {
            if let (Some(track), Some(cut)) = (track, cut)
                && all.last().is_none_or(|&last| last < cut)
            {
                track.transcript.breaks.push(cut);
                all.push(cut);
            }
        }
    }
}

fn run_rolling(
    directory: &Path,
    selection: &TranscriptionSelection,
    receiver: Receiver<RollingInput>,
    incomplete: &[AtomicBool; 2],
) -> RollingOutcome {
    crate::transcription::background_priority::deprioritize_background_thread();
    let mut worker = RollingWorker::new(directory, ChunkModel::new(selection.clone()));
    loop {
        let mut disconnected = false;
        let mut flush = None;
        loop {
            match receiver.try_recv() {
                Ok(RollingInput::Audio(packet)) => worker.ingest(incomplete, packet),
                Ok(RollingInput::Flush(request)) => {
                    flush = Some(request);
                    break;
                }
                Err(TryRecvError::Empty) => break,
                Err(TryRecvError::Disconnected) => {
                    disconnected = true;
                    break;
                }
            }
        }
        if let Some(request) = flush {
            worker.serve_flush(request);
            continue;
        }
        if disconnected {
            break;
        }
        // A preempted chunk falls through to the wait below before retrying.
        if !worker.pending.is_empty()
            && !worker.model_failed
            && !crate::process_exit::quitting()
            && !crate::transcription::engine::dictation_busy()
            && worker.transcribe_next()
        {
            worker.write_snapshot();
            continue;
        }
        match receiver.recv_timeout(Duration::from_millis(200)) {
            Ok(RollingInput::Audio(packet)) => worker.ingest(incomplete, packet),
            Ok(RollingInput::Flush(request)) => worker.serve_flush(request),
            Err(RecvTimeoutError::Timeout) => {}
            Err(RecvTimeoutError::Disconnected) => break,
        }
    }
    let RollingWorker {
        mut tracks,
        mut pending,
        mut envelope,
        mut model,
        mut model_failed,
        cuts,
        ..
    } = worker;
    for (index, track) in tracks.iter_mut().enumerate() {
        if let Some(track) = track
            && !track.failed
        {
            track.end(index, &mut envelope, &mut pending);
        }
    }
    while !pending.is_empty() && !model_failed && !crate::process_exit::quitting() {
        wait_for_dictation();
        if !transcribe_pending(
            directory,
            &mut tracks,
            &mut pending,
            &mut model,
            &mut model_failed,
        ) {
            thread::sleep(PREEMPTED_RETRY);
        }
    }
    for track in tracks.iter_mut().flatten() {
        track.transcript.finish();
    }
    let [system, microphone] = tracks;
    let complete = |track: Option<RollingTrack>, index: usize| {
        track
            .filter(|track| {
                !track.failed && !model_failed && !incomplete[index].load(Ordering::Relaxed)
            })
            .map(|track| track.transcript)
    };
    RollingOutcome {
        tracks: [complete(system, 0), complete(microphone, 1)],
        cuts,
        envelope,
        model,
    }
}

fn ingest(
    tracks: &mut [Option<RollingTrack>; 2],
    pending: &mut VecDeque<(usize, Chunk)>,
    envelope: &mut EchoEnvelope,
    incomplete: &[AtomicBool; 2],
    packet: AudioPacket,
) {
    let index = source_index(packet.source);
    let track = tracks[index].get_or_insert_with(|| {
        let resampler = StreamingResampler::new(packet.sample_rate)
            .map_err(|error| tracing::warn!(%error, "meeting audio cannot be resampled"))
            .ok();
        RollingTrack {
            failed: resampler.is_none(),
            resampler,
            sample_rate: packet.sample_rate,
            chunker: Chunker::default(),
            base: 0,
            transcript: TrackTranscript::new(packet.source, packet.arrival_us / 1_000),
            fed: 0,
            next_position: packet.position,
            pending: 0,
        }
    });
    if track.failed {
        return;
    }
    if track.sample_rate != packet.sample_rate || packet.position < track.next_position {
        tracing::warn!(source = ?packet.source, "rolling meeting transcription lost its timeline");
        track.failed = true;
        pending.retain(|(source, _)| *source != index);
        return;
    }
    if packet.position > track.next_position {
        // Silence padded in the WAV after a capture outage, or a block the
        // full queue skipped: finish the chunk before the gap and start
        // again after it, at the same place on the timeline.
        track.end(index, envelope, pending);
        track.base =
            (u128::from(packet.position) * RATE as u128 / u128::from(packet.sample_rate)) as usize;
        track.fed = track.base;
        track.resampler = StreamingResampler::new(packet.sample_rate).ok();
    }
    track.next_position = packet.position + packet.samples.len() as u64;
    let Some(resampler) = &mut track.resampler else {
        track.failed = true;
        return;
    };
    let samples = resampler.push(&packet.samples);
    track.push(index, &samples, envelope, pending);
    if track.pending > MAX_PENDING_AUDIO {
        // Transcription cannot keep up: keep the in-call transcript current
        // by skipping the oldest audio; the final pass rebuilds the track.
        if !incomplete[index].swap(true, Ordering::Relaxed) {
            tracing::warn!(source = ?packet.source, "rolling meeting transcription is behind; skipping audio");
        }
        while track.pending > MAX_PENDING_AUDIO / 2 {
            let Some(position) = pending.iter().position(|(source, _)| *source == index) else {
                break;
            };
            if let Some((_, chunk)) = pending.remove(position) {
                track.pending = track.pending.saturating_sub(chunk.samples.len());
            }
        }
    }
}

/// Transcribes the oldest pending chunk. Returns `false`, keeping the chunk,
/// when dictation preempted it.
fn transcribe_pending(
    directory: &Path,
    tracks: &mut [Option<RollingTrack>; 2],
    pending: &mut VecDeque<(usize, Chunk)>,
    model: &mut impl ChunkTranscriber,
    model_failed: &mut bool,
) -> bool {
    let Some((index, chunk)) = pending.pop_front() else {
        return true;
    };
    let Some(track) = tracks[index].as_mut() else {
        return true;
    };
    if track.failed {
        track.pending = track.pending.saturating_sub(chunk.samples.len());
        return true;
    }
    if chunk.is_silent() {
        track.pending = track.pending.saturating_sub(chunk.samples.len());
        track.transcript.accept(&chunk, Transcript::default());
        return true;
    }
    let started = Instant::now();
    let result = match model.transcribe_chunk(&chunk) {
        Ok(Chunked::Done(transcript)) => Ok(transcript),
        Ok(Chunked::Preempted) => {
            pending.push_front((index, chunk));
            return false;
        }
        Ok(Chunked::Unavailable(_)) if crate::process_exit::quitting() => {
            *model_failed = true;
            return true;
        }
        Ok(Chunked::Unavailable(error)) => {
            tracing::warn!(%error, "rolling meeting transcription could not load the model");
            persist_error(directory, &error);
            *model_failed = true;
            return true;
        }
        Err(error) => Err(error),
    };
    track.pending = track.pending.saturating_sub(chunk.samples.len());
    if started.elapsed() > SLOW_CHUNK {
        tracing::warn!(
            elapsed_ms = started.elapsed().as_millis() as u64,
            audio_ms = samples_ms(chunk.samples.len()),
            "rolling meeting chunk was slow"
        );
    }
    match result {
        Ok(transcript) => track.transcript.accept(&chunk, transcript),
        Err(error) => {
            tracing::warn!(%error, "rolling meeting chunk failed");
            persist_error(directory, &error);
            track.failed = true;
            pending.retain(|(source, _)| *source != index);
        }
    }
    true
}

/// Why the in-call transcript stopped, beside the meeting's manifest. The
/// final transcript is still made from the recording after Stop.
pub(super) const ERROR_FILE: &str = "transcript.rolling.error.txt";

fn persist_error(directory: &Path, error: &color_eyre::Report) {
    let path = directory.join(ERROR_FILE);
    let result = (|| -> Result<()> {
        fs::write(&path, format!("{error}\n"))?;
        set_owner_only(&path, false)?;
        Ok(())
    })();
    if let Err(persist_error) = result {
        tracing::warn!(%persist_error, %error, "could not record why live transcription stopped");
    }
}

/// Meeting-timeline milliseconds through which each source's rolling
/// transcript is final.
#[derive(Clone, Copy, Debug, Default, Deserialize, Eq, PartialEq, Serialize)]
pub(super) struct RollingCoverage {
    pub system: Option<u64>,
    pub microphone: Option<u64>,
}

/// One line of the rolling snapshot: its coverage, then its entries.
#[derive(Deserialize, Serialize)]
#[serde(untagged)]
pub(super) enum RollingLine {
    Coverage { covered_ms: RollingCoverage },
    Entry(TranscriptEntry),
}

/// The rolling snapshot as the Meetings pane reads it.
#[derive(Clone, Debug, Default, PartialEq)]
pub(super) struct RollingView {
    pub entries: Vec<TranscriptEntry>,
    pub covered_ms: RollingCoverage,
}

impl FromIterator<RollingLine> for RollingView {
    fn from_iter<I: IntoIterator<Item = RollingLine>>(lines: I) -> Self {
        let mut view = Self::default();
        for line in lines {
            match line {
                RollingLine::Coverage { covered_ms } => view.covered_ms = covered_ms,
                RollingLine::Entry(entry) => view.entries.push(entry),
            }
        }
        view
    }
}

/// The joined rolling transcript, with only entries no later chunk can
/// replace. A microphone entry is published only once the system track is
/// transcribed through its end, so echo is judged against the remote words
/// regardless of which chunk finished first. A system track that stopped
/// advancing is not waited for. Microphone entries through
/// `microphone_shown_ms` were already shown and stay shown.
fn rolling_view(
    tracks: [Option<&TrackTranscript>; 2],
    system_advancing: bool,
    microphone_shown_ms: u64,
    envelope: &EchoEnvelope,
) -> RollingView {
    let origin_ms = view_origin_ms(tracks);
    let [system, microphone] = tracks;
    let system_settled = system.map(|track| track.settled_ms);
    let microphone_settled = microphone.map(|track| match system_settled {
        Some(system) if system_advancing => track
            .settled_ms
            .min(system)
            .max(microphone_shown_ms.min(track.settled_ms)),
        _ => track.settled_ms,
    });
    let settled = |track: &TrackTranscript, through: Option<u64>| {
        let settled = track.settled_ms;
        track
            .entries
            .iter()
            .filter(move |entry| {
                entry.start_ms < settled && through.is_none_or(|through| entry.end_ms <= through)
            })
            .cloned()
            .collect::<Vec<_>>()
    };
    let entries = system
        .map(|track| settled(track, None))
        .into_iter()
        .flatten()
        .chain(
            microphone
                .map(|track| settled(track, microphone_settled))
                .into_iter()
                .flatten(),
        )
        .collect::<Vec<_>>();
    let (entries, _) = join_entries(entries, origin_ms, envelope);
    RollingView {
        entries,
        covered_ms: RollingCoverage {
            system: system_settled.map(|through| through.saturating_sub(origin_ms)),
            microphone: microphone_settled.map(|through| through.saturating_sub(origin_ms)),
        },
    }
}

/// Where a rolling view's timeline starts: the earliest track's first
/// capture-clock arrival.
fn view_origin_ms(tracks: [Option<&TrackTranscript>; 2]) -> u64 {
    tracks
        .iter()
        .flatten()
        .map(|track| track.origin_ms)
        .min()
        .unwrap_or_default()
}

/// Whether microphone text waits for the system track. A system source in a
/// capture outage receives nothing; microphone text does not wait for it.
fn system_advancing(tracks: &[Option<RollingTrack>; 2]) -> bool {
    let [system, microphone] = tracks;
    let captured_ms = |track: &RollingTrack| track.transcript.origin_ms + samples_ms(track.fed);
    system.as_ref().is_some_and(|system| {
        !system.failed
            && microphone.as_ref().is_none_or(|microphone| {
                captured_ms(microphone) <= captured_ms(system) + SYSTEM_STALL_MS
            })
    })
}

/// Atomically replaces the in-call transcript.
fn write_rolling_snapshot(
    directory: &Path,
    tracks: &[Option<RollingTrack>; 2],
    microphone_shown_ms: &mut u64,
    envelope: &EchoEnvelope,
) {
    let result = (|| -> Result<()> {
        let [system, microphone] = tracks;
        let view = rolling_view(
            [system, microphone].map(|track| track.as_ref().map(|track| &track.transcript)),
            system_advancing(tracks),
            *microphone_shown_ms,
            envelope,
        );
        if let (Some(covered), Some(microphone)) = (view.covered_ms.microphone, microphone) {
            let origin_ms = tracks
                .iter()
                .flatten()
                .map(|track| track.transcript.origin_ms)
                .min()
                .unwrap_or(microphone.transcript.origin_ms);
            *microphone_shown_ms = (*microphone_shown_ms).max(origin_ms + covered);
        }
        let temporary = directory.join(format!("{ROLLING_FILE}.tmp"));
        let mut file = BufWriter::new(File::create(&temporary)?);
        set_owner_only(&temporary, false)?;
        serde_json::to_writer(
            &mut file,
            &RollingLine::Coverage {
                covered_ms: view.covered_ms,
            },
        )?;
        file.write_all(b"\n")?;
        for entry in &view.entries {
            serde_json::to_writer(&mut file, entry)?;
            file.write_all(b"\n")?;
        }
        file.flush()?;
        fs::rename(&temporary, directory.join(ROLLING_FILE))?;
        Ok(())
    })();
    if let Err(error) = result {
        tracing::warn!(%error, "could not write rolling meeting transcript");
    }
}

/// What the Meetings pane and every copy read: the final transcript once
/// published, else the rolling one, re-read only when its file changes.
#[derive(Default)]
pub struct TranscriptReader {
    meeting_id: Option<String>,
    final_entries: Option<Vec<TranscriptEntry>>,
    rolling_modified: Option<std::time::SystemTime>,
    rolling: Vec<TranscriptEntry>,
}

impl TranscriptReader {
    pub fn read(&mut self, id: &str) -> Result<Vec<TranscriptEntry>> {
        if self.meeting_id.as_deref() != Some(id) {
            *self = Self {
                meeting_id: Some(id.into()),
                ..Self::default()
            };
        }
        let directory = super::root()?.join(id);
        super::recover_final_publication(&directory)?;
        if super::final_transcript_exists(&directory) {
            if self.final_entries.is_none() {
                self.final_entries =
                    Some(super::read_ndjson(&directory.join("transcript.ndjson"))?);
            }
            return Ok(self.final_entries.clone().unwrap_or_default());
        }
        let path = directory.join(ROLLING_FILE);
        let modified = match fs::metadata(&path) {
            Ok(metadata) => metadata.modified().ok(),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(Vec::new()),
            Err(error) => return Err(error.into()),
        };
        if modified.is_none() || modified != self.rolling_modified {
            self.rolling = super::read_ndjson::<RollingLine>(&path)?
                .into_iter()
                .collect::<RollingView>()
                .entries;
            self.rolling_modified = modified;
        }
        Ok(self.rolling.clone())
    }
}

/// Produces the final transcript, using complete rolling tracks and
/// streaming any remaining track from its WAV. Returns the joined entries and
/// the number of echo utterances removed.
pub(super) fn transcribe(
    directory: &Path,
    manifest: &MeetingManifest,
    selection: &TranscriptionSelection,
    rolling: Option<RollingOutcome>,
) -> Result<(Vec<TranscriptEntry>, u64)> {
    crate::transcription::background_priority::deprioritize_background_thread();
    let (mut rolled, mut cuts, mut envelope, mut model) = match rolling {
        Some(outcome) => (
            outcome.tracks,
            outcome.cuts,
            outcome.envelope,
            outcome.model,
        ),
        None => (
            [None, None],
            Default::default(),
            EchoEnvelope::default(),
            ChunkModel::new(selection.clone()),
        ),
    };
    let mut tracks = Vec::new();
    for (source, track) in [
        (MeetingSource::System, &manifest.system),
        (MeetingSource::Microphone, &manifest.microphone),
    ] {
        let origin_ms = track.first_arrival_us.unwrap_or_default() / 1_000;
        let transcript = match rolled[source_index(source)].take() {
            Some(transcript) => transcript,
            None => {
                envelope.clear(source);
                transcribe_file(
                    |chunk| model.transcribe_blocking(&chunk.samples),
                    &directory.join(&track.file),
                    TrackTranscript {
                        breaks: std::mem::take(&mut cuts[source_index(source)]),
                        ..TrackTranscript::new(source, origin_ms)
                    },
                    &mut envelope,
                )?
            }
        };
        tracks.push(transcript);
    }
    let origin_ms = [
        manifest.system.first_arrival_us,
        manifest.microphone.first_arrival_us,
    ]
    .into_iter()
    .flatten()
    .min()
    .unwrap_or_default()
        / 1_000;
    let entries = tracks.into_iter().flat_map(|track| track.entries).collect();
    Ok(join_entries(entries, origin_ms, &envelope))
}

/// Transcribes a track's WAV into `transcript`, a new one that may carry
/// the cuts copies made while recording.
fn transcribe_file(
    mut transcribe: impl FnMut(&Chunk) -> Result<Transcript>,
    path: &Path,
    mut transcript: TrackTranscript,
    envelope: &mut EchoEnvelope,
) -> Result<TrackTranscript> {
    let (source, origin_ms) = (transcript.source, transcript.origin_ms);
    let mut reader =
        WavReader::open(path).wrap_err_with(|| format!("could not read {}", path.display()))?;
    if reader.duration() == 0 {
        return Ok(transcript);
    }
    let source_rate = reader.spec().sample_rate;
    let mut resampler = StreamingResampler::new(source_rate).map_err(|error| eyre!(error))?;
    let mut chunker = Chunker::default();
    let mut samples = reader.samples::<f32>();
    let mut block = Vec::with_capacity(source_rate as usize);
    let mut fed = 0;
    let mut transcribe_chunk = |chunk: Chunk, transcript: &mut TrackTranscript| -> Result<()> {
        let result = transcribe(&chunk)?;
        transcript.accept(&chunk, result);
        Ok(())
    };
    loop {
        block.clear();
        for sample in samples.by_ref().take(source_rate as usize) {
            block.push(sample?);
        }
        if block.is_empty() {
            break;
        }
        let samples = resampler.push(&block);
        envelope.push(source, origin_ms + samples_ms(fed), &samples, RATE as u32);
        fed += samples.len();
        for chunk in chunker.push(&samples) {
            transcribe_chunk(chunk, &mut transcript)?;
        }
    }
    let tail = resampler.finish();
    envelope.push(source, origin_ms + samples_ms(fed), &tail, RATE as u32);
    for chunk in chunker.push(&tail) {
        transcribe_chunk(chunk, &mut transcript)?;
    }
    if let Some(chunk) = chunker.finish() {
        transcribe_chunk(chunk, &mut transcript)?;
    }
    transcript.finish();
    Ok(transcript)
}

/// Debug CLI `hex meeting echo <dir>`: replays a recording's tracks and
/// reports how many microphone lines echo suppression keeps, before (words
/// only, one entry per model segment) and after (signal plus words, one
/// entry per sentence). The microphone is placed `microphone_offset_ms`
/// after the system track when the manifest has no arrival times.
#[cfg(debug_assertions)]
pub(super) fn echo_report(
    directory: &Path,
    microphone_offset_ms: Option<u64>,
    transcribe: bool,
) -> Result<String> {
    use std::fmt::Write as _;

    let manifest = super::read_manifest(directory).ok();
    let offset_ms = microphone_offset_ms.unwrap_or_else(|| {
        manifest
            .as_ref()
            .and_then(|manifest| {
                Some(
                    manifest
                        .microphone
                        .first_arrival_us?
                        .saturating_sub(manifest.system.first_arrival_us?)
                        / 1_000,
                )
            })
            .unwrap_or_default()
    });
    let sources = [
        (MeetingSource::System, 0, directory.join("system.wav")),
        (
            MeetingSource::Microphone,
            offset_ms,
            directory.join("microphone.wav"),
        ),
    ];
    let mut envelope = EchoEnvelope::default();
    let mut report = String::new();
    let mut model = None;
    let selection = transcribe.then(|| crate::app_settings::transcription_selection().1);
    let mut old_entries = Vec::new();
    let mut new_entries = Vec::new();
    for (source, origin_ms, path) in &sources {
        let mut reader =
            WavReader::open(path).wrap_err_with(|| format!("could not read {}", path.display()))?;
        let rate = reader.spec().sample_rate;
        let mut resampler = StreamingResampler::new(rate).map_err(|error| eyre!(error))?;
        let mut chunker = Chunker::default();
        let mut old_track = TrackTranscript::new(*source, *origin_ms);
        let mut new_track = TrackTranscript::new(*source, *origin_ms);
        let mut fed = 0;
        let samples = reader.samples::<f32>().collect::<Result<Vec<_>, _>>()?;
        let mut chunks = Vec::new();
        for block in samples.chunks(rate as usize) {
            let resampled = resampler.push(block);
            envelope.push(
                *source,
                origin_ms + samples_ms(fed),
                &resampled,
                RATE as u32,
            );
            fed += resampled.len();
            chunks.extend(chunker.push(&resampled));
        }
        let tail = resampler.finish();
        envelope.push(*source, origin_ms + samples_ms(fed), &tail, RATE as u32);
        chunks.extend(chunker.push(&tail));
        chunks.extend(chunker.finish());
        if let Some(selection) = &selection {
            for chunk in chunks {
                let transcript = transcribe_speech(&chunk.samples, || {
                    load_model(&mut model, selection)?.transcribe_segments(chunk.samples.clone())
                })?;
                old_track.accept(
                    &chunk,
                    Transcript {
                        words: Vec::new(),
                        ..transcript.clone()
                    },
                );
                new_track.accept(&chunk, transcript);
            }
        }
        new_track.finish();
        old_entries.extend(old_track.entries);
        new_entries.extend(new_track.entries);
    }

    if transcribe {
        let count = |entries: &[TranscriptEntry]| {
            entries
                .iter()
                .filter(|entry| entry.source == MeetingSource::Microphone)
                .count()
        };
        let (old_kept, old_dropped) = suppress_echo(old_entries.clone(), |_, _| None);
        let (new_kept, new_dropped) = suppress_echo(new_entries.clone(), |start, end| {
            envelope.assess(start, end)
        });
        let (words_kept, _) = suppress_echo(new_entries.clone(), |_, _| None);
        writeln!(
            report,
            "final: before {} of {} You entries kept ({old_dropped} dropped); sentences with \
             words only {} of {}; after {} of {} kept ({new_dropped} dropped)",
            count(&old_kept),
            count(&old_entries),
            count(&words_kept),
            count(&new_entries),
            count(&new_kept),
            count(&new_entries),
        )?;
        let mut sorted = new_entries.clone();
        sorted.sort_by_key(|entry| entry.start_ms);
        for entry in sorted {
            let kept = new_kept.contains(&entry);
            let signal = (entry.source == MeetingSource::Microphone)
                .then(|| envelope.assess(entry.start_ms, entry.end_ms));
            writeln!(
                report,
                "  {} {:>6} {:<5} {:<6} {}",
                if kept { "KEEP" } else { "drop" },
                entry.start_ms,
                entry.source.label(),
                match signal {
                    Some(Some(Signal::Bleed)) => "bleed",
                    Some(Some(Signal::Local)) => "local",
                    Some(None) => "?",
                    None => "",
                },
                entry.text
            )?;
        }
    }
    Ok(report)
}

pub(super) struct LocalTime {
    year: i32,
    month: u32,
    day: u32,
    weekday: u32,
    hour: u32,
    minute: u32,
}

impl LocalTime {
    pub(super) fn from_ms(milliseconds: u64) -> Self {
        let seconds = (milliseconds / 1_000) as libc::time_t;
        // SAFETY: `localtime_r` and `localtime_s` write only into the
        // provided zeroed `tm`.
        let mut tm: libc::tm = unsafe { std::mem::zeroed() };
        #[cfg(unix)]
        unsafe {
            libc::localtime_r(&seconds, &mut tm)
        };
        #[cfg(windows)]
        unsafe {
            libc::localtime_s(&mut tm, &seconds)
        };
        Self {
            year: tm.tm_year + 1_900,
            month: (tm.tm_mon + 1) as u32,
            day: tm.tm_mday as u32,
            weekday: tm.tm_wday as u32,
            hour: tm.tm_hour as u32,
            minute: tm.tm_min as u32,
        }
    }

    pub(super) fn clock(&self) -> String {
        let hour = match self.hour % 12 {
            0 => 12,
            hour => hour,
        };
        let period = if self.hour < 12 { "AM" } else { "PM" };
        format!("{hour}:{:02} {period}", self.minute)
    }

    /// `2026-10-30T14:05:00`, as the Trash records a deletion.
    #[cfg_attr(not(target_os = "linux"), allow(dead_code))]
    pub(super) fn iso(&self) -> String {
        format!(
            "{:04}-{:02}-{:02}T{:02}:{:02}:00",
            self.year, self.month, self.day, self.hour, self.minute
        )
    }

    /// `(year, month, day)`, for grouping meetings by local day.
    pub(super) fn day(&self) -> (i32, u32, u32) {
        (self.year, self.month, self.day)
    }

    pub(super) fn date(&self) -> String {
        const WEEKDAYS: [&str; 7] = [
            "Sunday",
            "Monday",
            "Tuesday",
            "Wednesday",
            "Thursday",
            "Friday",
            "Saturday",
        ];
        const MONTHS: [&str; 12] = [
            "January",
            "February",
            "March",
            "April",
            "May",
            "June",
            "July",
            "August",
            "September",
            "October",
            "November",
            "December",
        ];
        format!(
            "{}, {} {}, {}",
            WEEKDAYS[self.weekday as usize % 7],
            MONTHS[(self.month as usize + 11) % 12],
            self.day,
            self.year
        )
    }

    fn file_stamp(&self) -> String {
        format!(
            "{:04}-{:02}-{:02} {:02}{:02}",
            self.year, self.month, self.day, self.hour, self.minute
        )
    }
}

pub(super) fn render_markdown(manifest: &MeetingManifest, entries: &[TranscriptEntry]) -> String {
    let started = LocalTime::from_ms(manifest.created_at_ms);
    let mut markdown = format!("# {}\n\n", super::display_title(manifest).trim());
    markdown.push_str(&format!(
        "- **Date:** {}, {}\n",
        started.date(),
        started.clock()
    ));
    if let Some(duration) = manifest.duration_ms {
        markdown.push_str(&format!("- **Duration:** {}\n", format_duration(duration)));
    }
    if let Some(app) = &manifest.app {
        markdown.push_str(&format!("- **App:** {app}\n"));
    }
    if let Some(calendar) = &manifest.calendar {
        if calendar.title.trim() != manifest.title.trim() && !calendar.title.trim().is_empty() {
            markdown.push_str(&format!("- **Calendar:** {}\n", calendar.title.trim()));
        }
        let attendees = calendar
            .attendees
            .iter()
            .map(|name| name.trim())
            .filter(|name| !name.is_empty())
            .collect::<Vec<_>>();
        if !attendees.is_empty() {
            markdown.push_str(&format!("- **Attendees:** {}\n", attendees.join(", ")));
        }
    }
    markdown.push_str("\n## Transcript\n\n");
    let turns = coalesce_transcript(entries.iter().cloned());
    if turns.is_empty() {
        markdown.push_str("_No speech was transcribed._\n");
    }
    for turn in turns {
        markdown.push_str(&format!(
            "**{}** · {}\n{}\n\n",
            turn.source.label(),
            format_duration(turn.start_ms),
            turn.text.trim()
        ));
    }
    markdown
}

fn file_title(title: &str) -> String {
    let cleaned = title
        .chars()
        .map(|character| match character {
            '/' | '\\' | ':' | '*' | '?' | '"' | '<' | '>' | '|' => '-',
            character if character.is_control() => ' ',
            character => character,
        })
        .collect::<String>();
    let cleaned = cleaned.split_whitespace().collect::<Vec<_>>().join(" ");
    let cleaned = cleaned.trim_matches(|character| character == '.' || character == '-');
    let truncated = cleaned.chars().take(80).collect::<String>();
    if truncated.trim().is_empty() {
        "Meeting".into()
    } else {
        truncated.trim().into()
    }
}

/// Writes the user-facing Markdown copy without replacing an existing file.
pub(super) fn export_markdown(
    folder: &Path,
    manifest: &MeetingManifest,
    entries: &[TranscriptEntry],
) -> Result<PathBuf> {
    fs::create_dir_all(folder)
        .wrap_err_with(|| format!("could not create {}", folder.display()))?;
    let stem = format!(
        "{} {}",
        LocalTime::from_ms(manifest.created_at_ms).file_stamp(),
        file_title(&super::display_title(manifest))
    );
    let path = (1..=100)
        .map(|attempt| {
            folder.join(if attempt == 1 {
                format!("{stem}.md")
            } else {
                format!("{stem} ({attempt}).md")
            })
        })
        .find(|path| !path.exists())
        .ok_or_else(|| eyre!("too many meeting files named {stem}"))?;
    let temporary = folder.join(format!(".{stem}.md.tmp"));
    let mut file = File::create(&temporary)?;
    file.write_all(render_markdown(manifest, entries).as_bytes())?;
    file.sync_all()?;
    fs::rename(&temporary, &path)?;
    sync_directory(folder)?;
    Ok(path)
}

/// One transcribed chunk, cached by the opt-in meeting replay so echo
/// suppression can be re-evaluated without the model.
#[cfg(test)]
#[derive(Deserialize, Serialize)]
pub(super) struct ReplayChunk {
    start: usize,
    overlap: usize,
    #[serde(default)]
    tail: usize,
    len: usize,
    transcript: Transcript,
}

/// Resamples a whole track to 16 kHz, as the rolling worker does.
#[cfg(test)]
pub(super) fn replay_resample(rate: u32, samples: &[f32]) -> Vec<f32> {
    let mut resampler = StreamingResampler::new(rate).unwrap();
    let mut resampled = samples
        .chunks(rate as usize)
        .flat_map(|block| resampler.push(block))
        .collect::<Vec<_>>();
    resampled.extend(resampler.finish());
    resampled
}

#[cfg(test)]
pub(super) fn replay_transcribe(model: &mut Transcriber, audio: &[f32]) -> Vec<ReplayChunk> {
    let mut chunker = Chunker::default();
    let mut chunks = audio
        .chunks(RATE)
        .flat_map(|block| chunker.push(block))
        .collect::<Vec<_>>();
    chunks.extend(chunker.finish());
    chunks
        .into_iter()
        .map(|chunk| ReplayChunk {
            start: chunk.start,
            overlap: chunk.overlap,
            tail: chunk.tail,
            len: chunk.samples.len(),
            transcript: transcribe_speech(&chunk.samples, || {
                model.transcribe_segments(chunk.samples.clone())
            })
            .unwrap(),
        })
        .collect()
}

/// The track as rolling transcription had it once audio through
/// `through_ms` was transcribed, on a timeline starting at zero.
#[cfg(test)]
pub(super) fn replay_track(
    source: MeetingSource,
    chunks: &[ReplayChunk],
    through_ms: u64,
) -> TrackTranscript {
    let mut track = TrackTranscript::new(source, 0);
    for cached in chunks {
        if samples_ms(cached.start + cached.len) > through_ms {
            break;
        }
        let chunk = Chunk {
            start: cached.start,
            overlap: cached.overlap,
            tail: cached.tail,
            samples: vec![0.0; cached.len],
        };
        track.accept(&chunk, cached.transcript.clone());
    }
    track
}

#[cfg(test)]
pub(super) fn replay_envelope(system: &[f32], microphone: &[f32]) -> EchoEnvelope {
    let mut envelope = EchoEnvelope::default();
    for (source, audio) in [
        (MeetingSource::System, system),
        (MeetingSource::Microphone, microphone),
    ] {
        for (index, block) in audio.chunks(RATE).enumerate() {
            envelope.push(source, index as u64 * 1_000, block, RATE as u32);
        }
    }
    envelope
}

/// What the pane shows from rolling transcription, the final transcript,
/// and every entry before echo suppression.
#[cfg(test)]
pub(super) fn replay_views(
    system: &TrackTranscript,
    microphone: &TrackTranscript,
    envelope: &EchoEnvelope,
) -> (RollingView, Vec<TranscriptEntry>, Vec<TranscriptEntry>) {
    let view = rolling_view([Some(system), Some(microphone)], true, 0, envelope);
    let entries = system
        .entries
        .iter()
        .chain(&microphone.entries)
        .cloned()
        .collect::<Vec<_>>();
    let (kept, _) = join_entries(entries.clone(), 0, envelope);
    (view, kept, entries)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn entry(source: MeetingSource, start_ms: u64, end_ms: u64, text: &str) -> TranscriptEntry {
        TranscriptEntry {
            source,
            start_ms,
            end_ms,
            text: text.into(),
        }
    }

    fn speech(seconds: usize) -> Vec<f32> {
        crate::transcription::pause_segmenter::tests::utterance(&[(seconds * 1_000, 0)], -70.0)
    }

    fn chunks_of(audio: &[f32], block: usize) -> Vec<Chunk> {
        let mut chunker = Chunker::default();
        let mut chunks = audio
            .chunks(block)
            .flat_map(|block| chunker.push(block))
            .collect::<Vec<_>>();
        chunks.extend(chunker.finish());
        chunks
    }

    #[test]
    fn continuous_speech_is_hard_cut_with_overlap_and_covers_every_sample() {
        let audio = speech(25);
        let chunks = chunks_of(&audio, 4_410);
        let spans = chunks
            .iter()
            .map(|chunk| {
                (
                    chunk.start / RATE,
                    chunk.overlap / RATE,
                    chunk.samples.len() / RATE,
                )
            })
            .collect::<Vec<_>>();
        assert_eq!(spans, [(0, 0, 10), (8, 2, 12), (18, 2, 7)]);
        assert_eq!(
            chunks
                .iter()
                .map(|chunk| chunk.tail / RATE)
                .collect::<Vec<_>>(),
            [2, 2, 0]
        );
        let covered: usize = chunks
            .iter()
            .map(|chunk| chunk.samples.len() - chunk.overlap)
            .sum();
        assert_eq!(covered, audio.len());
        for chunk in &chunks {
            assert_eq!(
                chunk.samples[..],
                audio[chunk.start..chunk.start + chunk.samples.len()]
            );
        }
    }

    #[test]
    fn chunks_end_at_the_first_pause_after_the_target_length() {
        let audio = crate::transcription::pause_segmenter::tests::utterance(
            &[(4_000, 800), (3_000, 800), (5_000, 800), (2_000, 0)],
            -70.0,
        );
        let chunks = chunks_of(&audio, 1_024);
        assert_eq!(
            chunks.len(),
            2,
            "{:?}",
            chunks.iter().map(|c| c.start).collect::<Vec<_>>()
        );
        assert_eq!(chunks[0].overlap, 0);
        assert_eq!(chunks[1].overlap, 0);
        assert_eq!(chunks[0].tail, 0);
        // The cut in the 4 s pause is too early; the pause at 7.8 s ends the chunk.
        assert_eq!(chunks[1].start * 1_000 / RATE, 8_100);
    }

    #[test]
    fn short_recordings_become_one_chunk_and_silence_tails_are_dropped() {
        let chunks = chunks_of(&speech(5), 1_000);
        assert_eq!(chunks.len(), 1);
        assert_eq!(chunks[0].samples.len(), 5 * RATE);
        let mut chunker = Chunker::default();
        assert!(chunker.push(&vec![0.0; RATE / 10]).is_empty());
        assert!(chunker.finish().is_none());
    }

    #[test]
    fn seam_drops_repeated_words() {
        assert_eq!(
            strip_repeated_prefix("we should ship the beta", "ship the beta on Friday."),
            "on Friday."
        );
        assert_eq!(
            strip_repeated_prefix("Ship it.", "It works now."),
            "It works now.",
            "a single repeated word is ambiguous and kept"
        );
        assert_eq!(strip_repeated_prefix("", "Hello there"), "Hello there");
    }

    #[test]
    fn a_speechless_chunk_skips_the_shared_model() {
        let _detector = crate::transcription::speech_detector::override_for_test(Some(false));
        let mut model = ChunkModel::new(TranscriptionSelection::default());
        let Chunked::Done(transcript) = model.transcribe(&[0.1; 16_000]).unwrap() else {
            panic!("a speechless chunk must not reach the model");
        };
        assert!(transcript.text.is_empty() && transcript.segments.is_empty());
        assert!(model.owned.is_none());
    }

    #[test]
    fn speechless_chunks_skip_the_model_and_still_advance_coverage() {
        let _detector = crate::transcription::speech_detector::override_for_test(Some(false));
        let mut track = TrackTranscript::new(MeetingSource::Microphone, 1_000);
        let chunk = Chunk {
            start: 0,
            overlap: 0,
            tail: 0,
            samples: vec![0.1; 30 * RATE],
        };
        let result = transcribe_speech(&chunk.samples, || {
            panic!("a chunk without speech must not reach the model")
        })
        .unwrap();
        track.accept(&chunk, result);
        assert!(track.entries.is_empty());
        assert_eq!(track.settled_ms, 31_000);
    }

    #[test]
    fn a_speechless_chunk_after_an_overlap_keeps_earlier_entries() {
        let mut track = TrackTranscript::new(MeetingSource::System, 0);
        let first = Chunk {
            start: 0,
            overlap: 0,
            tail: 0,
            samples: vec![0.1; 30 * RATE],
        };
        let spoken = Transcript {
            text: String::new(),
            segments: vec![TranscriptSegment {
                start_ms: 28_000,
                end_ms: 29_900,
                text: "see you on Friday".into(),
            }],
            words: Vec::new(),
        };
        track.accept(
            &first,
            transcribe_speech(&first.samples, || Ok(spoken)).unwrap(),
        );
        let _detector = crate::transcription::speech_detector::override_for_test(Some(false));
        let second = Chunk {
            start: 28 * RATE,
            overlap: 2 * RATE,
            tail: 0,
            samples: vec![0.1; 10 * RATE],
        };
        let result = transcribe_speech(&second.samples, || {
            panic!("a chunk without speech must not reach the model")
        })
        .unwrap();
        track.accept(&second, result);
        assert_eq!(
            track.entries,
            [entry(
                MeetingSource::System,
                28_000,
                29_900,
                "see you on Friday"
            )]
        );
        assert_eq!(track.settled_ms, 38_000);
    }

    #[test]
    fn overlapping_chunks_join_at_the_middle_of_the_overlap() {
        let mut track = TrackTranscript::new(MeetingSource::System, 1_000);
        let first = Chunk {
            start: 0,
            overlap: 0,
            tail: 2 * RATE,
            samples: vec![0.0; 30 * RATE],
        };
        track.accept(
            &first,
            Transcript {
                text: String::new(),
                segments: vec![
                    TranscriptSegment {
                        start_ms: 20_000,
                        end_ms: 28_500,
                        text: "we should ship the beta".into(),
                    },
                    TranscriptSegment {
                        start_ms: 29_200,
                        end_ms: 30_000,
                        text: "on".into(),
                    },
                ],
                words: Vec::new(),
            },
        );
        let second = Chunk {
            start: 28 * RATE,
            overlap: 2 * RATE,
            tail: 0,
            samples: vec![0.0; 10 * RATE],
        };
        track.accept(
            &second,
            Transcript {
                text: String::new(),
                segments: vec![
                    TranscriptSegment {
                        start_ms: 100,
                        end_ms: 900,
                        text: "the beta".into(),
                    },
                    TranscriptSegment {
                        start_ms: 1_200,
                        end_ms: 3_000,
                        text: "on Friday.".into(),
                    },
                ],
                words: Vec::new(),
            },
        );
        let texts = track
            .entries
            .iter()
            .map(|entry| (entry.start_ms, entry.text.as_str()))
            .collect::<Vec<_>>();
        assert_eq!(
            texts,
            [(21_000, "we should ship the beta"), (30_200, "on Friday.")]
        );
    }

    #[test]
    fn rolling_publishes_only_entries_a_later_seam_cannot_replace() {
        let mut track = TrackTranscript::new(MeetingSource::System, 0);
        let segment = |start_ms, end_ms, text: &str| TranscriptSegment {
            start_ms,
            end_ms,
            text: text.into(),
        };
        let transcript = |segments| Transcript {
            segments,
            ..Transcript::default()
        };
        // A hard cut at 10 s: the next chunk starts at 8 s, its seam at 9 s.
        track.accept(
            &Chunk {
                start: 0,
                overlap: 0,
                tail: 2 * RATE,
                samples: vec![0.0; 10 * RATE],
            },
            transcript(vec![
                segment(1_000, 5_000, "Settled words."),
                segment(9_200, 10_000, "maybe"),
            ]),
        );
        let envelope = EchoEnvelope::default();
        let view = |track: &TrackTranscript| {
            rolling_view([Some(track), None], true, 0, &envelope)
                .entries
                .into_iter()
                .map(|entry| entry.text)
                .collect::<Vec<_>>()
        };
        assert_eq!(view(&track), ["Settled words."]);
        track.accept(
            &Chunk {
                start: 8 * RATE,
                overlap: 2 * RATE,
                tail: 0,
                samples: vec![0.0; 6 * RATE],
            },
            transcript(vec![segment(1_200, 2_500, "Maybe Friday.")]),
        );
        assert_eq!(view(&track), ["Settled words.", "Maybe Friday."]);
    }

    pub(super) static MODEL_LOADS: std::sync::atomic::AtomicUsize =
        std::sync::atomic::AtomicUsize::new(0);

    const QUIT_CHILD: &str = "HEX_MEETING_QUIT_CHILD";

    /// The child half of `quitting_mid_meeting_loads_no_model_and_finishes_in_time`.
    #[test]
    fn quitting_mid_meeting_child() {
        let Some(mode) = std::env::var_os(QUIT_CHILD) else {
            return;
        };
        let directory =
            std::env::temp_dir().join(format!("hex-meeting-quit-{}", std::process::id()));
        fs::create_dir_all(&directory).unwrap();
        let mut rolling = Rolling::start(directory.clone(), TranscriptionSelection::default());
        let mut feed = rolling.feed().unwrap();
        // Set before any audio arrives so no chunk can start a load first.
        if mode == "quit" {
            crate::process_exit::begin_quitting_for_test();
        }
        let quit = Instant::now();
        let audio = speech(30);
        for (index, block) in audio.chunks(RATE).enumerate() {
            let position = (index * RATE) as u64;
            feed.offer(&packet(MeetingSource::System, position, block.to_vec()));
            feed.offer(&packet(MeetingSource::Microphone, position, block.to_vec()));
        }
        feed.flush();
        drop(feed);
        rolling.finish().unwrap();
        println!(
            "loads={} elapsed_ms={} error_file={}",
            MODEL_LOADS.load(Ordering::SeqCst),
            quit.elapsed().as_millis(),
            directory.join(ERROR_FILE).exists(),
        );
        let _ = fs::remove_dir_all(&directory);
    }

    fn quitting_mid_meeting_report(mode: &str) -> std::collections::HashMap<String, String> {
        let output = std::process::Command::new(std::env::current_exe().unwrap())
            .args([
                "--exact",
                "meeting::transcript::tests::quitting_mid_meeting_child",
                "--nocapture",
            ])
            .env(QUIT_CHILD, mode)
            .output()
            .unwrap();
        assert!(output.status.success(), "{output:?}");
        String::from_utf8_lossy(&output.stdout)
            .split_whitespace()
            .filter_map(|field| field.split_once('='))
            .map(|(key, value)| (key.to_string(), value.to_string()))
            .collect()
    }

    #[test]
    fn quitting_mid_meeting_loads_no_model_and_finishes_in_time() {
        // Without quitting, the same audio with no engine tries to load one.
        let staying = quitting_mid_meeting_report("stay");
        assert_ne!(staying["loads"], "0", "{staying:?}");
        let quitting = quitting_mid_meeting_report("quit");
        assert_eq!(quitting["loads"], "0", "{quitting:?}");
        assert_eq!(quitting["error_file"], "false", "{quitting:?}");
        let elapsed_ms: u64 = quitting["elapsed_ms"].parse().unwrap();
        assert!(elapsed_ms < 2_000, "{quitting:?}");
    }

    fn packet(source: MeetingSource, position: u64, samples: Vec<f32>) -> AudioPacket {
        AudioPacket {
            source,
            pts_us: None,
            arrival_us: 0,
            position,
            sample_rate: RATE as u32,
            samples,
        }
    }

    const WORD_SLOT: usize = RATE / 2;
    const WORD_LENGTH: usize = RATE * 3 / 10;

    /// Hears a word in every loud 500 ms slot of the track, `w<slot>`, and
    /// every fourth ends a sentence. The same audio always gives the same
    /// words at the same track times, and a word the chunk cuts off is not
    /// heard. `preempt` chunks are refused first, as while dictation holds
    /// the model.
    #[derive(Default)]
    struct SlotWords {
        preempt: usize,
        calls: Vec<Instant>,
    }

    impl ChunkTranscriber for SlotWords {
        fn transcribe_chunk(&mut self, chunk: &Chunk) -> Result<Chunked> {
            self.calls.push(Instant::now());
            if self.preempt > 0 {
                self.preempt -= 1;
                return Ok(Chunked::Preempted);
            }
            let end = chunk.start + chunk.samples.len();
            let words = (chunk.start.div_ceil(WORD_SLOT)..)
                .map(|slot| slot * WORD_SLOT)
                .take_while(|&at| at + WORD_LENGTH <= end)
                .filter(|&at| {
                    let heard = &chunk.samples[at - chunk.start..at - chunk.start + WORD_LENGTH];
                    heard.iter().map(|sample| sample * sample).sum::<f32>() / WORD_LENGTH as f32
                        > 1e-4
                })
                .map(|at| {
                    let slot = at / WORD_SLOT;
                    TranscriptSegment {
                        start_ms: samples_ms(at - chunk.start) as i64,
                        end_ms: samples_ms(at - chunk.start + WORD_LENGTH) as i64,
                        text: format!("w{slot}{}", if slot % 4 == 3 { "." } else { "" }),
                    }
                })
                .collect();
            Ok(Chunked::Done(Transcript {
                words,
                ..Transcript::default()
            }))
        }
    }

    /// Them for the first 12 s, then You from 15 s to 30 s: far enough
    /// apart that echo suppression keeps both.
    fn call_audio(source: MeetingSource, at: usize) -> f32 {
        let speaks = match source {
            MeetingSource::System => at < 12 * RATE,
            MeetingSource::Microphone => (15 * RATE..30 * RATE).contains(&at),
        };
        if speaks {
            0.1 * (at as f32 * std::f32::consts::TAU * 220.0 / RATE as f32).sin()
        } else {
            0.0
        }
    }

    /// Every slot either source speaks a whole word in.
    fn call_slots() -> Vec<usize> {
        (0..12 * 2).chain(30..30 + 15 * 2).collect()
    }

    struct Call {
        directory: PathBuf,
        incomplete: [AtomicBool; 2],
        fed: usize,
        /// Each source's capture-clock arrival of its first packet.
        arrival_us: [u64; 2],
        audio: fn(MeetingSource, usize) -> f32,
    }

    impl Call {
        fn new(name: &str) -> Self {
            let directory = std::env::temp_dir()
                .join(format!("hex-meeting-flush-{name}-{}", std::process::id()));
            fs::create_dir_all(&directory).unwrap();
            Self {
                directory,
                incomplete: [AtomicBool::new(false), AtomicBool::new(false)],
                fed: 0,
                arrival_us: [0, 0],
                audio: call_audio,
            }
        }

        /// Captures both sources through `seconds`, in 100 ms packets.
        fn capture(&mut self, worker: &mut RollingWorker<SlotWords>, seconds: f32) {
            let until = (seconds * RATE as f32) as usize;
            while self.fed < until {
                let length = (RATE / 10).min(until - self.fed);
                for source in [MeetingSource::System, MeetingSource::Microphone] {
                    let samples = (self.fed..self.fed + length)
                        .map(|at| (self.audio)(source, at))
                        .collect();
                    worker.ingest(
                        &self.incomplete,
                        AudioPacket {
                            arrival_us: self.arrival_us[source_index(source)]
                                + samples_ms(self.fed) * 1_000,
                            ..packet(source, self.fed as u64, samples)
                        },
                    );
                }
                self.fed += length;
            }
        }

        /// What a plain read of the in-call transcript finds.
        fn snapshot(&self) -> Vec<TranscriptEntry> {
            super::super::read_ndjson::<RollingLine>(&self.directory.join(ROLLING_FILE))
                .map(|lines| lines.into_iter().collect::<RollingView>().entries)
                .unwrap_or_default()
        }
    }

    impl Drop for Call {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.directory);
        }
    }

    fn transcribe_queued(worker: &mut RollingWorker<SlotWords>) {
        while !worker.pending.is_empty() {
            assert!(worker.transcribe_next());
        }
        worker.write_snapshot();
    }

    fn flush(
        worker: &mut RollingWorker<SlotWords>,
        wait: Duration,
    ) -> Option<Vec<TranscriptEntry>> {
        let (reply, replies) = mpsc::sync_channel(1);
        worker.serve_flush(FlushRequest {
            deadline: Instant::now() + wait,
            reply,
        });
        replies.try_recv().ok()
    }

    /// Stop: the rest of each track, then everything published.
    fn stop(worker: &mut RollingWorker<SlotWords>) -> Vec<TranscriptEntry> {
        for (index, track) in worker.tracks.iter_mut().enumerate() {
            if let Some(track) = track {
                track.end(index, &mut worker.envelope, &mut worker.pending);
            }
        }
        transcribe_queued(worker);
        for track in worker.tracks.iter_mut().flatten() {
            track.transcript.finish();
        }
        rolling_view(
            worker
                .tracks
                .each_ref()
                .map(|track| track.as_ref().map(|track| &track.transcript)),
            false,
            0,
            &worker.envelope,
        )
        .entries
    }

    fn slots(text: &str) -> Vec<usize> {
        text.split_whitespace()
            .filter_map(|word| {
                word.trim_start_matches(|character: char| !character.is_ascii_digit())
                    .trim_end_matches('.')
                    .parse()
                    .ok()
            })
            .collect()
    }

    /// Copies through the shared cursor's rules, returning the slots copied.
    fn copy(
        cursor: &mut super::super::MeetingPasteCursor,
        entries: &[TranscriptEntry],
    ) -> Vec<usize> {
        let mut copied = Vec::new();
        let _ = cursor.consume_entries("call", entries, |text| {
            copied = slots(text);
            Ok(())
        });
        copied
    }

    /// The last slot a source speaks a whole word in by `seconds`.
    fn last_slot_by(seconds: f32) -> usize {
        let at = (seconds * RATE as f32) as usize;
        call_slots()
            .into_iter()
            .filter(|slot| slot * WORD_SLOT + WORD_LENGTH <= at)
            .max()
            .unwrap()
    }

    #[test]
    fn a_copy_includes_speech_up_to_the_click() {
        let mut call = Call::new("click");
        let directory = call.directory.clone();
        let mut worker = RollingWorker::new(&directory, SlotWords::default());
        let mut cursor = super::super::MeetingPasteCursor::default();
        for click in [9.3, 22.4] {
            call.capture(&mut worker, click);
            transcribe_queued(&mut worker);
            let shown = call.snapshot();
            let entries = flush(&mut worker, Duration::from_secs(3)).expect("flushed in time");
            let newest = last_slot_by(click);
            assert!(
                shown
                    .iter()
                    .all(|entry| !slots(&entry.text).contains(&newest)),
                "a plain read at {click} s already had w{newest}"
            );
            let copied = copy(&mut cursor, &entries);
            assert_eq!(
                copied.last(),
                Some(&newest),
                "copy at {click} s: {copied:?}"
            );
            // The flush is a copy: the in-call transcript is unchanged.
            assert_eq!(call.snapshot(), shown);
        }
        // Both sources are in the copy, You only once Them is covered.
        let entries = flush(&mut worker, Duration::from_secs(3)).unwrap();
        assert!(
            entries
                .iter()
                .any(|entry| entry.source == MeetingSource::System)
        );
        assert!(
            entries
                .iter()
                .any(|entry| entry.source == MeetingSource::Microphone)
        );
    }

    #[test]
    fn repeated_copies_neither_repeat_nor_drop_words_as_later_chunks_overlap_them() {
        copy_repeatedly(Call::new("repeated"));
    }

    /// On the capture clock, where each source starts at its own first
    /// arrival, not at zero.
    #[test]
    fn repeated_copies_cut_tracks_that_start_at_different_capture_times() {
        let mut call = Call::new("arrivals");
        call.arrival_us = [5_000_000_000, 5_000_040_000];
        copy_repeatedly(call);
    }

    fn copy_repeatedly(mut call: Call) {
        let directory = call.directory.clone();
        let mut worker = RollingWorker::new(&directory, SlotWords::default());
        let mut cursor = super::super::MeetingPasteCursor::default();
        let mut copied = Vec::new();
        // Mid-word, mid-sentence, right after a hard cut, in a pause, twice
        // in a row, and with chunks still queued (odd clicks skip the
        // ordinary transcription, so the flush runs the queue first).
        for (index, click) in [3.7, 9.3, 10.1, 12.75, 16.2, 16.3, 21.05, 27.6]
            .into_iter()
            .enumerate()
        {
            call.capture(&mut worker, click);
            if index % 2 == 0 {
                transcribe_queued(&mut worker);
            }
            let entries = flush(&mut worker, Duration::from_secs(3)).expect("flushed in time");
            let new = copy(&mut cursor, &entries);
            if let Some(&last) = new.last() {
                assert_eq!(last, last_slot_by(click), "copy at {click} s: {new:?}");
            }
            copied.extend(new);
        }
        call.capture(&mut worker, 35.0);
        let final_entries = stop(&mut worker);
        copied.extend(copy(&mut cursor, &final_entries));
        let mut sorted = copied.clone();
        sorted.sort_unstable();
        assert_eq!(sorted, call_slots(), "copied {copied:?}");
        assert_eq!(sorted, copied, "copies come in order");
        let final_slots = final_entries
            .iter()
            .flat_map(|entry| slots(&entry.text))
            .collect::<Vec<_>>();
        assert_eq!(
            final_slots,
            call_slots(),
            "the final transcript keeps every word once"
        );
    }

    #[test]
    fn a_copy_that_runs_out_of_time_gets_no_reply() {
        let mut call = Call::new("timeout");
        let directory = call.directory.clone();
        // Dictation keeps the model the whole time.
        let mut worker = RollingWorker::new(
            &directory,
            SlotWords {
                preempt: usize::MAX,
                ..SlotWords::default()
            },
        );
        call.capture(&mut worker, 9.3);
        let wait = Duration::from_millis(400);
        let started = Instant::now();
        assert!(flush(&mut worker, wait).is_none());
        assert!(started.elapsed() >= wait);
        assert!(
            started.elapsed() < wait + PREEMPTED_RETRY * 2,
            "{:?}",
            started.elapsed()
        );
        // Nothing was lost: the audio is still there for the next attempt.
        worker.model.preempt = 0;
        let entries = flush(&mut worker, Duration::from_secs(3)).unwrap();
        assert_eq!(
            slots(
                &entries
                    .iter()
                    .map(|entry| entry.text.as_str())
                    .collect::<Vec<_>>()
                    .join(" ")
            ),
            (0..=18).collect::<Vec<_>>()
        );
    }

    // Busy without an engine is macOS's capture marker.
    #[cfg(target_os = "macos")]
    #[test]
    fn a_copy_waits_out_dictation_and_a_preempted_chunk() {
        let mut call = Call::new("dictation");
        let directory = call.directory.clone();
        let mut worker = RollingWorker::new(
            &directory,
            SlotWords {
                preempt: 1,
                ..SlotWords::default()
            },
        );
        call.capture(&mut worker, 5.0);
        let busy = crate::parakeet::dictation_busy_for_test();
        let released = thread::spawn(move || {
            thread::sleep(Duration::from_millis(400));
            let at = Instant::now();
            drop(busy);
            at
        });
        let entries = flush(&mut worker, Duration::from_secs(3)).expect("flushed in time");
        let released = released.join().unwrap();
        assert!(!worker.model.calls.is_empty());
        assert!(
            worker.model.calls.iter().all(|&call| call >= released),
            "the model ran while dictation was busy"
        );
        assert!(
            worker.model.calls.len() >= 2,
            "the preempted chunk was retried"
        );
        assert!(!entries.is_empty());
    }

    #[test]
    fn a_copy_reaches_only_the_recording_this_process_runs() {
        assert!(matches!(
            transcribe_up_to_now("not-recording", Duration::from_secs(3)),
            Flushed::NotRecording
        ));
        // A recording whose worker never answers runs out of time.
        let (sender, _requests) = mpsc::sync_channel(4);
        let registration = register_flusher("silent", RollingFlusher(sender));
        let started = Instant::now();
        assert!(matches!(
            transcribe_up_to_now("silent", Duration::from_millis(200)),
            Flushed::TimedOut
        ));
        assert!(started.elapsed() >= Duration::from_millis(200));
        drop(registration);
        assert!(matches!(
            transcribe_up_to_now("silent", Duration::from_secs(3)),
            Flushed::NotRecording
        ));
    }

    fn tone(at: usize) -> f32 {
        0.1 * (at as f32 * std::f32::consts::TAU * 220.0 / RATE as f32).sin()
    }

    #[test]
    fn a_copy_just_after_a_hard_cut_keeps_you_while_them_is_silent() {
        let mut call = Call::new("hard-cut");
        call.audio = |source, at| match source {
            MeetingSource::System => 0.0,
            MeetingSource::Microphone => tone(at),
        };
        let directory = call.directory.clone();
        let mut worker = RollingWorker::new(&directory, SlotWords::default());
        call.capture(&mut worker, 10.1);
        transcribe_queued(&mut worker);
        // Neither track has a pause, so both were cut hard at 10 s, and the
        // system track is settled only to the middle of the overlap.
        let system = &worker.tracks[0].as_ref().unwrap().transcript;
        assert!(system.settled_ms <= 9_000, "{}", system.settled_ms);
        let entries = flush(&mut worker, Duration::from_secs(3)).expect("flushed in time");
        let copied = copy(&mut super::super::MeetingPasteCursor::default(), &entries);
        assert_eq!(copied, (0..=19).collect::<Vec<_>>());
    }

    #[test]
    fn a_copy_cut_holds_when_a_later_chunk_moves_the_words_slightly() {
        let word = |slot: i64, shift: i64| TranscriptSegment {
            start_ms: slot * 300 - shift,
            end_ms: (slot + 1) * 300 - shift,
            text: format!("w{slot}"),
        };
        let heard = |words: Vec<TranscriptSegment>, ms: usize| {
            let chunk = Chunk {
                start: 0,
                overlap: 0,
                tail: 0,
                samples: vec![0.0; RATE * ms / 1_000],
            };
            let transcript = Transcript {
                words,
                ..Transcript::default()
            };
            (chunk, transcript)
        };
        // The copy took w0 to w2, spoken back to back.
        let mut copied = TrackTranscript::new(MeetingSource::Microphone, 0);
        let (chunk, transcript) = heard((0..3).map(|slot| word(slot, 0)).collect(), 900);
        copied.accept(&chunk, transcript);
        copied.finish();
        let mut cursor = super::super::MeetingPasteCursor::default();
        assert_eq!(copy(&mut cursor, &copied.entries), [0, 1, 2]);
        // With more context the track hears every word 40 ms earlier, so w3
        // now starts before the cut at the end of w2.
        let mut track = TrackTranscript::new(MeetingSource::Microphone, 0);
        track.breaks.push(900);
        let (chunk, transcript) = heard((0..6).map(|slot| word(slot, 40)).collect(), 1_800);
        track.accept(&chunk, transcript);
        track.finish();
        assert_eq!(copy(&mut cursor, &track.entries), [3, 4, 5]);
    }

    #[test]
    fn a_track_rebuilt_from_its_wav_after_stop_keeps_the_copies_cuts() {
        let mut call = Call::new("rebuild");
        let directory = call.directory.clone();
        let mut worker = RollingWorker::new(&directory, SlotWords::default());
        let mut cursor = super::super::MeetingPasteCursor::default();
        let mut copied = Vec::new();
        // The second copy ends after the first word of a sentence.
        for click in [9.3, 22.4] {
            call.capture(&mut worker, click);
            transcribe_queued(&mut worker);
            let entries = flush(&mut worker, Duration::from_secs(3)).expect("flushed in time");
            copied.extend(copy(&mut cursor, &entries));
        }
        call.capture(&mut worker, 35.0);
        let rolled = stop(&mut worker);
        // The microphone track fell behind, so Stop rebuilds it from its WAV.
        let wav = directory.join("microphone.wav");
        let spec = hound::WavSpec {
            channels: 1,
            sample_rate: RATE as u32,
            bits_per_sample: 32,
            sample_format: hound::SampleFormat::Float,
        };
        let mut writer = hound::WavWriter::create(&wav, spec).unwrap();
        for at in 0..35 * RATE {
            writer
                .write_sample(call_audio(MeetingSource::Microphone, at))
                .unwrap();
        }
        writer.finalize().unwrap();
        let mut model = SlotWords::default();
        let cuts = std::mem::take(&mut worker.cuts[1]);
        assert_eq!(cuts.len(), 1, "{cuts:?}");
        let rebuilt = transcribe_file(
            |chunk| match model.transcribe_chunk(chunk)? {
                Chunked::Done(transcript) => Ok(transcript),
                Chunked::Preempted | Chunked::Unavailable(_) => unreachable!(),
            },
            &wav,
            TrackTranscript {
                breaks: cuts,
                ..TrackTranscript::new(MeetingSource::Microphone, 0)
            },
            &mut EchoEnvelope::default(),
        )
        .unwrap();
        let final_entries = rolled
            .into_iter()
            .filter(|entry| entry.source == MeetingSource::System)
            .chain(rebuilt.entries)
            .collect::<Vec<_>>();
        copied.extend(copy(&mut cursor, &final_entries));
        assert_eq!(copied, call_slots());
    }

    #[test]
    fn a_copy_waiting_when_capture_ends_is_still_answered() {
        let call = Call::new("feed-end");
        let mut rolling = Rolling::start(call.directory.clone(), TranscriptionSelection::default());
        let mut feed = rolling.feed().unwrap();
        feed.offer(&packet(MeetingSource::System, 0, vec![0.0; RATE / 2]));
        let flusher = rolling.flusher();
        let asking = thread::spawn(move || flusher.transcribe_up_to_now(Duration::from_secs(30)));
        thread::sleep(Duration::from_millis(100));
        // Capture ends before another packet arrives.
        feed.flush();
        drop(feed);
        assert!(asking.join().unwrap().is_some());
        rolling.finish().unwrap();
    }

    const COPY_QUIT_CHILD: &str = "HEX_MEETING_COPY_QUIT_CHILD";

    /// The child half of `a_copy_stops_waiting_when_hex_quits`.
    #[test]
    fn a_copy_stops_waiting_when_hex_quits_child() {
        if std::env::var_os(COPY_QUIT_CHILD).is_none() {
            return;
        }
        // A worker that never answers.
        let (sender, _requests) = mpsc::sync_channel(4);
        let flusher = RollingFlusher(sender);
        thread::spawn(|| {
            thread::sleep(Duration::from_millis(200));
            crate::process_exit::begin_quitting_for_test();
        });
        let started = Instant::now();
        let answered = flusher.transcribe_up_to_now(Duration::from_secs(3));
        let waited = started.elapsed();
        let started = Instant::now();
        let again = flusher.transcribe_up_to_now(Duration::from_secs(3));
        println!(
            "answered={} waited_ms={} again={} again_ms={}",
            answered.is_some(),
            waited.as_millis(),
            again.is_some(),
            started.elapsed().as_millis()
        );
    }

    #[test]
    fn a_copy_stops_waiting_when_hex_quits() {
        let output = std::process::Command::new(std::env::current_exe().unwrap())
            .args([
                "--exact",
                "meeting::transcript::tests::a_copy_stops_waiting_when_hex_quits_child",
                "--nocapture",
            ])
            .env(COPY_QUIT_CHILD, "1")
            .output()
            .unwrap();
        assert!(output.status.success(), "{output:?}");
        let report = String::from_utf8_lossy(&output.stdout)
            .split_whitespace()
            .filter_map(|field| field.split_once('='))
            .map(|(key, value)| (key.to_string(), value.to_string()))
            .collect::<std::collections::HashMap<_, _>>();
        assert_eq!(report["answered"], "false", "{report:?}");
        let waited: u64 = report["waited_ms"].parse().unwrap();
        assert!((200..1_000).contains(&waited), "{report:?}");
        assert_eq!(report["again"], "false", "{report:?}");
        let again: u64 = report["again_ms"].parse().unwrap();
        assert!(again < 50, "{report:?}");
    }

    #[test]
    fn the_feed_forwards_a_copy_after_the_audio_before_it() {
        let call = Call::new("feed");
        let directory = call.directory.clone();
        let mut rolling = Rolling::start(directory.clone(), TranscriptionSelection::default());
        let mut feed = rolling.feed().unwrap();
        let flusher = rolling.flusher();
        let asking = thread::spawn(move || flusher.transcribe_up_to_now(Duration::from_secs(30)));
        // Half a second, less than a block: only a flush sends it.
        thread::sleep(Duration::from_millis(100));
        feed.offer(&packet(MeetingSource::System, 0, vec![0.0; RATE / 2]));
        let entries = asking.join().unwrap().expect("answered");
        assert!(entries.is_empty(), "silence has no words");
        drop(feed);
        let outcome = rolling.finish().unwrap();
        let system = outcome.tracks[0].as_ref().unwrap();
        assert_eq!(system.breaks, Vec::<i64>::new(), "no words, no cut");
    }

    #[test]
    fn a_capture_gap_restarts_the_track_after_it_instead_of_dropping_it() {
        let mut tracks = [None, None];
        let mut pending = VecDeque::new();
        let mut envelope = EchoEnvelope::default();
        let incomplete = [AtomicBool::new(false), AtomicBool::new(false)];
        let audio = speech(8);
        for (index, block) in audio.chunks(RATE).enumerate() {
            let position = (index * RATE) as u64;
            let packet = packet(MeetingSource::Microphone, position, block.to_vec());
            ingest(
                &mut tracks,
                &mut pending,
                &mut envelope,
                &incomplete,
                packet,
            );
        }
        // Twenty seconds of silence padded into the WAV, then more speech.
        let resumed = (28 * RATE) as u64;
        for (index, block) in audio.chunks(RATE).enumerate() {
            let position = resumed + (index * RATE) as u64;
            let packet = packet(MeetingSource::Microphone, position, block.to_vec());
            ingest(
                &mut tracks,
                &mut pending,
                &mut envelope,
                &incomplete,
                packet,
            );
        }
        let track = tracks[1].as_mut().unwrap();
        assert!(!track.failed);
        assert!(!incomplete[1].load(Ordering::Relaxed));
        track.end(1, &mut envelope, &mut pending);
        let spans = pending
            .iter()
            .map(|(_, chunk)| (chunk.start, chunk.start + chunk.samples.len()))
            .collect::<Vec<_>>();
        assert!(
            spans.iter().any(|(_, end)| *end <= 8 * RATE)
                && spans.iter().any(|(start, _)| *start >= 28 * RATE)
                && !spans
                    .iter()
                    .any(|(start, end)| *start < 28 * RATE && *end > 8 * RATE),
            "{spans:?}"
        );
        assert!(spans.iter().all(|(_, end)| *end <= 36 * RATE), "{spans:?}");
    }

    #[test]
    fn a_backlog_skips_the_oldest_audio_and_marks_the_track_for_rebuilding() {
        let mut tracks = [None, None];
        let mut pending = VecDeque::new();
        let mut envelope = EchoEnvelope::default();
        let incomplete = [AtomicBool::new(false), AtomicBool::new(false)];
        let audio = speech(MAX_PENDING_AUDIO / RATE + 30);
        for (index, block) in audio.chunks(RATE).enumerate() {
            let packet = packet(MeetingSource::System, (index * RATE) as u64, block.to_vec());
            ingest(
                &mut tracks,
                &mut pending,
                &mut envelope,
                &incomplete,
                packet,
            );
        }
        let track = tracks[0].as_ref().unwrap();
        assert!(!track.failed);
        assert!(incomplete[0].load(Ordering::Relaxed));
        assert!(track.pending <= MAX_PENDING_AUDIO);
        // What remains is the newest audio.
        assert!(pending.iter().all(|(_, chunk)| chunk.start > 30 * RATE));
    }

    #[test]
    fn silent_chunks_are_not_transcribed() {
        let chunk = |samples| Chunk {
            start: 0,
            overlap: 0,
            tail: 0,
            samples,
        };
        assert!(chunk(vec![0.0; 6 * RATE]).is_silent());
        assert!(chunk(vec![0.0005; 6 * RATE]).is_silent());
        assert!(!chunk(speech(6)).is_silent());
    }

    #[test]
    fn speaker_echo_of_remote_speech_is_dropped() {
        let entries = vec![
            entry(
                MeetingSource::System,
                10_000,
                12_400,
                "Does it have AISDK support?",
            ),
            entry(
                MeetingSource::Microphone,
                10_300,
                12_900,
                "Does it have AICK support?",
            ),
            entry(
                MeetingSource::Microphone,
                13_100,
                15_000,
                "Yes, since version two.",
            ),
        ];
        let (kept, dropped) = suppress_echo(entries, |_, _| None);
        assert_eq!(dropped, 1);
        assert_eq!(
            kept.iter()
                .map(|entry| entry.text.as_str())
                .collect::<Vec<_>>(),
            ["Does it have AISDK support?", "Yes, since version two."]
        );
    }

    #[test]
    fn word_level_echo_is_dropped_as_one_utterance() {
        let mut entries = vec![entry(
            MeetingSource::System,
            5_000,
            7_000,
            "AICK support is coming next week",
        )];
        for (index, word) in ["AISDK", "support", "is", "coming", "next", "week"]
            .into_iter()
            .enumerate()
        {
            let start = 5_200 + index as u64 * 300;
            entries.push(entry(MeetingSource::Microphone, start, start + 250, word));
        }
        let (kept, dropped) = suppress_echo(entries, |_, _| None);
        assert_eq!(dropped, 1);
        assert_eq!(kept.len(), 1);
    }

    #[test]
    fn the_signal_overrides_word_matching_both_ways() {
        let entries = vec![
            entry(
                MeetingSource::System,
                1_000,
                3_000,
                "We could make twenty games.",
            ),
            // Misheard bleed: different words, but the signal explains it.
            entry(
                MeetingSource::Microphone,
                1_100,
                3_000,
                "We can make plenty.",
            ),
            entry(MeetingSource::System, 10_000, 11_000, "Yeah, totally."),
            // Double-talk: the same words, but the microphone is louder than
            // any bleed, so the user said them too.
            entry(MeetingSource::Microphone, 10_050, 11_000, "Yeah, totally."),
        ];
        let (kept, dropped) = suppress_echo(entries, |start, _| {
            Some(if start < 5_000 {
                Signal::Bleed
            } else {
                Signal::Local
            })
        });
        assert_eq!(dropped, 1);
        assert_eq!(
            kept.iter()
                .map(|entry| (entry.source, entry.start_ms))
                .collect::<Vec<_>>(),
            [
                (MeetingSource::System, 1_000),
                (MeetingSource::System, 10_000),
                (MeetingSource::Microphone, 10_050),
            ]
        );
    }

    #[test]
    fn synthetic_speaker_bleed_is_dropped_and_double_talk_kept() {
        use super::super::echo::tests::{bleed, envelope, speech};
        let system = speech(12.0, 11);
        let mut microphone = bleed(&system, 80, 0.1);
        for (offset, sample) in speech(3.0, 29).iter().enumerate() {
            microphone[7 * RATE + offset] += sample * 0.9;
        }
        let envelope = envelope(&system, &microphone);
        let entries = vec![
            entry(
                MeetingSource::System,
                1_000,
                4_000,
                "Ship the beta on Friday.",
            ),
            entry(
                MeetingSource::Microphone,
                1_100,
                4_000,
                "Shipped a better Friday.",
            ),
            entry(
                MeetingSource::System,
                7_000,
                10_000,
                "Then we tell everyone.",
            ),
            entry(
                MeetingSource::Microphone,
                7_000,
                10_000,
                "Wait, which beta?",
            ),
        ];
        let (kept, dropped) = suppress_echo(entries, |start, end| envelope.assess(start, end));
        assert_eq!(dropped, 1);
        assert!(kept.iter().any(|entry| entry.text == "Wait, which beta?"));
        assert!(
            !kept
                .iter()
                .any(|entry| entry.text == "Shipped a better Friday.")
        );
    }

    #[test]
    fn a_local_sentence_inside_an_echo_utterance_is_kept() {
        let entries = vec![
            entry(
                MeetingSource::System,
                1_000,
                9_000,
                "That's why I'm gonna get the plugin registry out. I wake up.",
            ),
            entry(
                MeetingSource::Microphone,
                1_100,
                4_000,
                "That's why I'm gonna get the plugin registry out.",
            ),
            entry(
                MeetingSource::Microphone,
                4_300,
                6_500,
                "Also, can you add some knobs?",
            ),
            entry(MeetingSource::Microphone, 6_800, 9_000, "Yeah, I wake up."),
        ];
        let (kept, dropped) = suppress_echo(entries, |start, end| {
            Some(if (4_300, 6_500) == (start, end) {
                Signal::Local
            } else {
                Signal::Bleed
            })
        });
        assert_eq!(dropped, 1);
        assert_eq!(
            kept.iter()
                .filter(|entry| entry.source == MeetingSource::Microphone)
                .map(|entry| entry.text.as_str())
                .collect::<Vec<_>>(),
            ["Also, can you add some knobs?"]
        );
    }

    fn track(
        source: MeetingSource,
        through_ms: u64,
        entries: &[(u64, u64, &str)],
    ) -> TrackTranscript {
        let mut track = TrackTranscript::new(source, 0);
        track.settled_ms = through_ms;
        track.entries = entries
            .iter()
            .map(|(start, end, text)| entry(source, *start, *end, text))
            .collect();
        track
    }

    #[test]
    fn microphone_entries_wait_until_the_system_track_covers_them() {
        let system = track(
            MeetingSource::System,
            10_000,
            &[(4_000, 6_000, "Does it have AISDK support?")],
        );
        // The microphone chunk finished first: its echo of the remote line
        // after 10 s cannot be judged until the system chunk arrives.
        let microphone = track(
            MeetingSource::Microphone,
            20_000,
            &[
                (4_100, 6_100, "Does it have AICK support?"),
                (8_000, 9_000, "Yes, since version two."),
                (12_000, 14_000, "Ship the beta on Friday."),
            ],
        );
        let envelope = EchoEnvelope::default();
        let view = rolling_view([Some(&system), Some(&microphone)], true, 0, &envelope);
        assert_eq!(
            view.entries
                .iter()
                .map(|entry| entry.text.as_str())
                .collect::<Vec<_>>(),
            ["Does it have AISDK support?", "Yes, since version two."]
        );
        assert_eq!(
            view.covered_ms,
            RollingCoverage {
                system: Some(10_000),
                microphone: Some(10_000),
            }
        );

        // Before any system chunk, the microphone covers nothing yet.
        let pending = track(MeetingSource::System, 0, &[]);
        let view = rolling_view([Some(&pending), Some(&microphone)], true, 0, &envelope);
        assert!(view.entries.is_empty());
        assert_eq!(view.covered_ms.microphone, Some(0));

        // A system track that stopped advancing is not waited for.
        let view = rolling_view([Some(&system), Some(&microphone)], false, 0, &envelope);
        assert_eq!(view.entries.len(), 3);
        assert_eq!(view.covered_ms.microphone, Some(20_000));
    }

    #[test]
    fn rolling_snapshots_parse_with_and_without_coverage() {
        let line = |json: &str| serde_json::from_str::<RollingLine>(json).unwrap();
        let view: RollingView = [
            line(r#"{"covered_ms":{"system":30000,"microphone":28000}}"#),
            line(r#"{"source":"system","start_ms":1,"end_ms":2,"text":"Hi."}"#),
        ]
        .into_iter()
        .collect();
        assert_eq!(view.covered_ms.microphone, Some(28_000));
        assert_eq!(view.entries.len(), 1);
        let legacy: RollingView = [line(
            r#"{"source":"microphone","start_ms":1,"end_ms":2,"text":"Hi."}"#,
        )]
        .into_iter()
        .collect();
        assert_eq!(legacy.covered_ms, RollingCoverage::default());
        assert_eq!(legacy.entries[0].source, MeetingSource::Microphone);
    }

    #[test]
    fn timed_words_become_sentences_at_punctuation_and_pauses() {
        let word = |start_ms, end_ms, text: &str| TranscriptSegment {
            start_ms,
            end_ms,
            text: text.into(),
        };
        let mut track = TrackTranscript::new(MeetingSource::System, 0);
        track.accept(
            &Chunk {
                start: 0,
                overlap: 0,
                tail: 0,
                samples: vec![0.0; 3 * RATE],
            },
            Transcript {
                words: vec![
                    word(0, 300, " Basically,"),
                    word(320, 500, "yes."),
                    word(560, 800, "Then"),
                    word(820, 1_000, "this"),
                    word(2_000, 2_300, "Later"),
                    word(2_320, 2_600, "words"),
                ],
                ..Transcript::default()
            },
        );
        assert_eq!(
            track
                .entries
                .iter()
                .map(|entry| (entry.start_ms, entry.end_ms, entry.text.as_str()))
                .collect::<Vec<_>>(),
            [
                (0, 500, "Basically, yes."),
                (560, 1_000, "Then this"),
                (2_000, 2_600, "Later words"),
            ]
        );
    }

    #[test]
    fn a_sentence_across_a_hard_cut_keeps_every_word() {
        let word = |start_ms, text: &str| TranscriptSegment {
            start_ms,
            end_ms: start_ms + 250,
            text: text.into(),
        };
        let mut track = TrackTranscript::new(MeetingSource::Microphone, 0);
        // Cut at 10 s; the next chunk starts at 8 s, so its seam is at 9 s.
        track.accept(
            &Chunk {
                start: 0,
                overlap: 0,
                tail: 2 * RATE,
                samples: vec![0.0; 10 * RATE],
            },
            Transcript {
                words: vec![
                    word(1_000, "Done."),
                    word(8_500, "Interface"),
                    word(8_800, "elements"),
                    word(9_100, "don't"),
                    word(9_700, "interf"),
                ],
                ..Transcript::default()
            },
        );
        assert_eq!(track.entries.len(), 1, "the open sentence waits");
        track.accept(
            &Chunk {
                start: 8 * RATE,
                overlap: 2 * RATE,
                tail: 0,
                samples: vec![0.0; 6 * RATE],
            },
            Transcript {
                words: vec![
                    word(500, "Interface"),
                    word(800, "elements"),
                    word(1_100, "don't"),
                    word(1_400, "pop"),
                    word(1_700, "in."),
                ],
                ..Transcript::default()
            },
        );
        assert_eq!(
            track
                .entries
                .iter()
                .map(|entry| entry.text.as_str())
                .collect::<Vec<_>>(),
            ["Done.", "Interface elements don't pop in."]
        );
    }

    fn hard_cut_track(old: &[(i64, &str)], new: &[(i64, &str)]) -> TrackTranscript {
        let words = |words: &[(i64, &str)], offset: i64| {
            words
                .iter()
                .map(|&(start_ms, text)| TranscriptSegment {
                    start_ms: start_ms - offset,
                    end_ms: start_ms - offset + 200,
                    text: text.into(),
                })
                .collect::<Vec<_>>()
        };
        let mut track = TrackTranscript::new(MeetingSource::System, 0);
        // Cut at 10 s; the next chunk starts at 8 s, so its seam is at 9 s.
        track.accept(
            &Chunk {
                start: 0,
                overlap: 0,
                tail: 2 * RATE,
                samples: vec![0.0; 10 * RATE],
            },
            Transcript {
                words: words(old, 0),
                ..Transcript::default()
            },
        );
        if !new.is_empty() {
            track.accept(
                &Chunk {
                    start: 8 * RATE,
                    overlap: 2 * RATE,
                    tail: 0,
                    samples: vec![0.0; 4 * RATE],
                },
                Transcript {
                    words: words(new, 8_000),
                    ..Transcript::default()
                },
            );
        }
        track
    }

    fn texts(track: &TrackTranscript) -> Vec<&str> {
        track
            .entries
            .iter()
            .map(|entry| entry.text.as_str())
            .collect()
    }

    #[test]
    fn a_word_whose_time_shifts_across_the_seam_appears_once() {
        // The two chunks place "nine" on opposite sides of the 9 s middle.
        let earlier = [
            (8_400, "We"),
            (8_700, "said"),
            (9_050, "nine"),
            (9_500, "times"),
        ];
        let later = [
            (8_400, "We"),
            (8_700, "said"),
            (8_950, "nine"),
            (9_500, "times."),
        ];
        assert_eq!(
            texts(&hard_cut_track(&earlier, &later)),
            ["We said nine times."]
        );
        let earlier = [
            (8_400, "We"),
            (8_700, "said"),
            (8_950, "nine"),
            (9_500, "times"),
        ];
        let later = [
            (8_400, "We"),
            (8_700, "said"),
            (9_050, "nine"),
            (9_500, "times."),
        ];
        assert_eq!(
            texts(&hard_cut_track(&earlier, &later)),
            ["We said nine times."]
        );
    }

    #[test]
    fn stopping_right_after_a_hard_cut_publishes_the_pending_words() {
        let mut track = hard_cut_track(&[(1_000, "Done."), (8_500, "One"), (9_200, "more")], &[]);
        assert_eq!(texts(&track), ["Done."]);
        track.finish();
        assert_eq!(texts(&track), ["Done.", "One more"]);
        assert!(
            track
                .entries
                .iter()
                .all(|entry| entry.start_ms < track.settled_ms)
        );
    }

    #[test]
    fn a_model_without_word_timings_does_not_discard_its_text() {
        let mut track = hard_cut_track(&[(8_500, "One"), (9_200, "more")], &[]);
        track.accept(
            &Chunk {
                start: 8 * RATE,
                overlap: 2 * RATE,
                tail: 0,
                samples: vec![0.0; 4 * RATE],
            },
            Transcript {
                text: "One more thing.".into(),
                segments: vec![TranscriptSegment {
                    start_ms: 1_500,
                    end_ms: 3_000,
                    text: "thing.".into(),
                }],
                ..Transcript::default()
            },
        );
        assert_eq!(texts(&track), ["One more", "thing."]);
    }

    #[test]
    fn microphone_text_already_shown_stays_shown() {
        let system = track(MeetingSource::System, 10_000, &[]);
        let microphone = track(
            MeetingSource::Microphone,
            20_000,
            &[(12_000, 14_000, "Ship the beta on Friday.")],
        );
        let envelope = EchoEnvelope::default();
        let view = rolling_view([Some(&system), Some(&microphone)], true, 15_000, &envelope);
        assert_eq!(view.entries.len(), 1);
        assert_eq!(view.covered_ms.microphone, Some(15_000));
    }

    #[test]
    fn distinct_or_distant_local_speech_is_kept() {
        let entries = vec![
            entry(
                MeetingSource::System,
                10_000,
                12_000,
                "Does it have AISDK support?",
            ),
            // Crosstalk with different words.
            entry(
                MeetingSource::Microphone,
                10_500,
                12_000,
                "Let me check the docs.",
            ),
            // The same words much later are a real answer, not an echo.
            entry(
                MeetingSource::Microphone,
                40_000,
                42_000,
                "Does it have AISDK support?",
            ),
        ];
        let (kept, dropped) = suppress_echo(entries, |_, _| None);
        assert_eq!(dropped, 0);
        assert_eq!(kept.len(), 3);
    }

    #[test]
    fn short_function_words_must_match_exactly() {
        assert!(tokens_match("aick", "aisdk"));
        assert!(!tokens_match("this", "that"));
        assert!(!tokens_match("to", "do"));
        assert!(tokens_match("to", "to"));
    }

    fn manifest() -> MeetingManifest {
        let mut manifest: MeetingManifest = serde_json::from_value(serde_json::json!({
            "schema_version": 1,
            "id": "1",
            "title": "Zoom call — 2:30 PM",
            "status": "complete",
            "created_at_ms": 1_760_000_000_000_u64,
            "ended_at_ms": null,
            "duration_ms": 125_000,
            "system": {"file": "system.wav", "samples": 0, "dropped_packets": 0},
            "microphone": {"file": "microphone.wav", "samples": 0, "dropped_packets": 0},
            "error": null
        }))
        .unwrap();
        manifest.app = Some("Zoom".into());
        manifest
    }

    #[test]
    fn markdown_lists_metadata_and_labeled_turns() {
        let markdown = render_markdown(
            &manifest(),
            &[
                entry(MeetingSource::Microphone, 1_000, 2_000, "Hi."),
                entry(MeetingSource::Microphone, 2_500, 3_000, "Can you hear me?"),
                entry(MeetingSource::System, 4_000, 5_000, "Loud and clear."),
            ],
        );
        assert!(markdown.starts_with("# Zoom call — 2:30 PM\n\n- **Date:** "));
        assert!(markdown.contains("- **Duration:** 02:05\n- **App:** Zoom\n"));
        assert!(markdown.contains("**You** · 00:01\nHi. Can you hear me?\n\n"));
        assert!(markdown.contains("**Them** · 00:04\nLoud and clear.\n"));
        assert!(!markdown.contains("Attendees"));
    }

    #[test]
    fn markdown_lists_the_calendar_attendees() {
        let mut manifest = manifest();
        manifest.title = "Q4 launch sync".into();
        manifest.calendar = Some(crate::meeting::MeetingCalendar {
            title: "Q4 launch sync".into(),
            attendees: vec!["Ana Ruiz".into(), "Ben Ito".into()],
        });
        let markdown = render_markdown(&manifest, &[]);
        assert!(markdown.contains("- **App:** Zoom\n- **Attendees:** Ana Ruiz, Ben Ito\n"));
        assert!(!markdown.contains("**Calendar:**"));
    }

    #[test]
    fn export_never_overwrites_and_uses_safe_names() {
        let folder = std::env::temp_dir().join(format!(
            "hex-meeting-export-{}-{}",
            std::process::id(),
            crate::events::now_ms()
        ));
        let mut manifest = manifest();
        manifest.title = "Design/sync: Q4?".into();
        let first = export_markdown(&folder, &manifest, &[]).unwrap();
        let second = export_markdown(&folder, &manifest, &[]).unwrap();
        assert_ne!(first, second);
        let name = first.file_name().unwrap().to_string_lossy().to_string();
        assert!(name.ends_with(" Design-sync- Q4.md"), "{name}");
        assert!(
            second
                .file_name()
                .unwrap()
                .to_string_lossy()
                .ends_with("Q4 (2).md")
        );
        assert!(
            fs::read_to_string(&first)
                .unwrap()
                .contains("_No speech was transcribed._")
        );
        fs::remove_dir_all(folder).unwrap();
    }
}
