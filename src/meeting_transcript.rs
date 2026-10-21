//! Final meeting transcription. Each track streams through a 16 kHz resampler
//! into pause-aware chunks that are transcribed at utility QoS whenever
//! dictation is idle: during the recording (rolling) or from the WAV after
//! Stop. Hard cuts overlap and are deduplicated at the seam. The joined
//! transcript drops microphone utterances that echo the remote audio, and a
//! Markdown copy is published to the user's meeting folder.

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
use crate::pause_segmenter::PauseSegmenter;
use crate::transcription::{Transcriber, Transcript, TranscriptSegment};
use crate::transcription_models::TranscriptionSelection;

use super::echo::{EchoEnvelope, Signal};
use super::{
    AudioPacket, MeetingManifest, MeetingSource, TranscriptEntry, coalesce_transcript,
    format_duration, set_owner_only, sync_directory,
};

const RATE: usize = 16_000;
const FRAME: usize = RATE / 50;
/// Pause cuts closer than this to the chunk start are ignored.
const CHUNK_MIN: usize = RATE * 10;
/// The first pause after this length ends a chunk.
const CHUNK_TARGET: usize = RATE * 20;
/// Without a pause, a chunk is cut here and the next one overlaps it.
const CHUNK_MAX: usize = RATE * 30;
const CHUNK_OVERLAP: usize = RATE * 2;
const MIN_TAIL: usize = RATE * 3 / 10;
/// About five minutes of one-second blocks for both sources.
const ROLLING_QUEUE_CAPACITY: usize = 600;
/// Chunks slower than this are logged; a stalled worker loses the queue.
const SLOW_CHUNK: Duration = Duration::from_secs(5);
/// A source whose untranscribed backlog exceeds this falls back to the WAV.
const MAX_PENDING_CHUNKS: usize = 24;
const DICTATION_WAIT: Duration = Duration::from_secs(20);
const ECHO_WINDOW_MS: u64 = 1_500;
/// Word-level segments closer than this are judged together.
const UTTERANCE_GAP_MS: u64 = 700;
/// Entries with at least this many words are also judged on their own.
const SENTENCE_WORDS: usize = 3;
const ECHO_SIMILARITY: f32 = 0.6;
/// A pause this long between timed words starts a new sentence.
const SENTENCE_PAUSE_MS: i64 = 600;
const SEAM_MAX_WORDS: usize = 12;

pub(super) const ROLLING_FILE: &str = "transcript.rolling.ndjson";

#[derive(Debug)]
pub(super) struct Chunk {
    /// 16 kHz offset of `samples[0]` within the track.
    start: usize,
    /// Leading samples shared with the previous chunk.
    overlap: usize,
    samples: Vec<f32>,
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
        let begin = self.chunk_start - self.overlap;
        (self.end() >= self.chunk_start + MIN_TAIL).then(|| Chunk {
            start: begin,
            overlap: self.overlap,
            samples: self.buffer[begin - self.buffer_start..].to_vec(),
        })
    }

    fn end(&self) -> usize {
        self.buffer_start + self.buffer.len()
    }

    fn uncommitted(&self) -> (usize, &[f32]) {
        let start = self.chunk_start;
        (start, &self.buffer[start - self.buffer_start..])
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

/// One source's entries in capture-clock milliseconds.
pub(super) struct TrackTranscript {
    source: MeetingSource,
    origin_ms: u64,
    entries: Vec<TranscriptEntry>,
    /// Capture-clock end of the audio transcribed so far.
    through_ms: u64,
}

impl TrackTranscript {
    fn new(source: MeetingSource, origin_ms: u64) -> Self {
        Self {
            source,
            origin_ms,
            entries: Vec::new(),
            through_ms: origin_ms,
        }
    }

    fn accept(&mut self, chunk: &Chunk, transcript: Transcript) {
        let chunk_ms = self.origin_ms + samples_ms(chunk.start);
        let duration_ms = samples_ms(chunk.samples.len());
        self.through_ms = self.through_ms.max(chunk_ms + duration_ms);
        let (mut incoming, segmented) =
            timed_entries(self.source, chunk_ms, duration_ms, transcript);
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
}

fn timed_entries(
    source: MeetingSource,
    start_ms: u64,
    duration_ms: u64,
    transcript: Transcript,
) -> (Vec<TranscriptEntry>, bool) {
    let segments = if transcript.words.is_empty() {
        transcript.segments
    } else {
        sentences(transcript.words)
    };
    let segmented = !segments.is_empty();
    let entries = if segmented {
        segments
            .into_iter()
            .filter_map(|segment| {
                let text = segment.text.trim().to_string();
                let start = (segment.start_ms.max(0) as u64).min(duration_ms);
                let end = (segment.end_ms.max(0) as u64).min(duration_ms);
                (!text.is_empty() && end > start).then(|| TranscriptEntry {
                    source,
                    start_ms: start_ms + start,
                    end_ms: start_ms + end,
                    text,
                })
            })
            .collect()
    } else {
        let text = transcript.text.trim();
        (!text.is_empty())
            .then(|| TranscriptEntry {
                source,
                start_ms,
                end_ms: start_ms + duration_ms,
                text: text.into(),
            })
            .into_iter()
            .collect()
    };
    (entries, segmented)
}

/// Groups timed words into sentences: a group ends after sentence
/// punctuation or before a pause. Models that time words often return one
/// segment per chunk, which is too coarse to tell echo from a reply.
fn sentences(words: Vec<TranscriptSegment>) -> Vec<TranscriptSegment> {
    let mut sentences: Vec<TranscriptSegment> = Vec::new();
    let mut open = false;
    for word in words {
        let text = word.text.trim();
        if text.is_empty() {
            continue;
        }
        match sentences.last_mut() {
            Some(sentence)
                if open && word.start_ms.saturating_sub(sentence.end_ms) < SENTENCE_PAUSE_MS =>
            {
                sentence.text.push(' ');
                sentence.text.push_str(text);
                sentence.end_ms = sentence.end_ms.max(word.end_ms);
            }
            _ => sentences.push(TranscriptSegment {
                start_ms: word.start_ms,
                end_ms: word.end_ms,
                text: text.into(),
            }),
        }
        open = !text.ends_with(['.', '?', '!']);
    }
    sentences
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
    let longest = previous_words
        .len()
        .min(normalized_next.len())
        .min(SEAM_MAX_WORDS);
    let repeated = (2..=longest)
        .rev()
        .find(|&count| previous_words[previous_words.len() - count..] == normalized_next[..count])
        .unwrap_or(0);
    next_words[repeated..].join(" ")
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

/// Whether a microphone entry's words repeat remote speech around it.
pub(super) fn repeats_remote(entry: &TranscriptEntry, remote: &[&TranscriptEntry]) -> bool {
    echo_score(&tokens(&entry.text), entry.start_ms, entry.end_ms, remote) >= ECHO_SIMILARITY
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
    while crate::parakeet::dictation_busy() && started.elapsed() < DICTATION_WAIT {
        thread::sleep(Duration::from_millis(50));
    }
}

fn load_model<'a>(
    model: &'a mut Option<Transcriber>,
    selection: &TranscriptionSelection,
) -> Result<&'a mut Transcriber> {
    if model.is_none() {
        *model = Some(Transcriber::load_selection(selection)?);
    }
    model
        .as_mut()
        .ok_or_else(|| eyre!("meeting transcriber is unavailable"))
}

struct RollingTrack {
    resampler: Option<StreamingResampler>,
    sample_rate: u32,
    chunker: Chunker,
    transcript: TrackTranscript,
    /// 16 kHz samples fed to the chunker.
    fed: usize,
    pending: usize,
    failed: bool,
    provisional: Vec<TranscriptEntry>,
    drafted_samples: usize,
    provisional_stopped: bool,
    last_pass: Option<crate::meeting_provisional::LastPass>,
}

/// Transcribes completed chunks while the meeting is still recording so the
/// final transcript is ready shortly after Stop.
pub(super) struct Rolling {
    sender: Option<SyncSender<AudioPacket>>,
    lost: Arc<[AtomicBool; 2]>,
    worker: JoinHandle<RollingOutcome>,
}

pub(super) struct RollingFeed {
    sender: SyncSender<AudioPacket>,
    lost: Arc<[AtomicBool; 2]>,
    /// Per-source audio gathered into blocks of about a second, so the
    /// bounded queue holds minutes of audio rather than ScreenCaptureKit's
    /// many small packets.
    blocks: [Option<AudioPacket>; 2],
}

impl RollingFeed {
    /// Forwards without blocking; a full queue abandons that source's rolling
    /// transcript, which is then rebuilt from its WAV after Stop.
    pub(super) fn offer(&mut self, packet: &AudioPacket) {
        let index = source_index(packet.source);
        if self.lost[index].load(Ordering::Relaxed) {
            return;
        }
        if self.blocks[index]
            .as_ref()
            .is_some_and(|block| block.sample_rate != packet.sample_rate)
        {
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
    }

    /// Sends partial blocks; call once capture ends.
    pub(super) fn flush(&mut self) {
        self.send(0);
        self.send(1);
    }

    fn send(&mut self, index: usize) {
        let Some(block) = self.blocks[index].take() else {
            return;
        };
        let lost = &self.lost[index];
        let source = block.source;
        if self.sender.try_send(block).is_err() && !lost.swap(true, Ordering::Relaxed) {
            tracing::warn!(?source, "rolling meeting transcription fell behind");
        }
    }
}

pub(super) struct RollingOutcome {
    tracks: [Option<TrackTranscript>; 2],
    envelope: EchoEnvelope,
    model: Option<Transcriber>,
}

impl Rolling {
    pub(super) fn start(directory: PathBuf, selection: TranscriptionSelection) -> Self {
        let (sender, receiver) = mpsc::sync_channel(ROLLING_QUEUE_CAPACITY);
        let lost = Arc::new([AtomicBool::new(false), AtomicBool::new(false)]);
        let worker_lost = lost.clone();
        let worker = thread::Builder::new()
            .name("meeting-transcription".into())
            .spawn(move || run_rolling(&directory, &selection, receiver, &worker_lost))
            .expect("meeting transcription thread must start");
        Self {
            sender: Some(sender),
            lost,
            worker,
        }
    }

    pub(super) fn feed(&mut self) -> Option<RollingFeed> {
        self.sender.take().map(|sender| RollingFeed {
            sender,
            lost: self.lost.clone(),
            blocks: [None, None],
        })
    }

    /// Waits for the worker after the capture writer dropped its feed.
    pub(super) fn finish(mut self) -> Option<RollingOutcome> {
        self.sender.take();
        self.worker.join().ok()
    }
}

fn run_rolling(
    directory: &Path,
    selection: &TranscriptionSelection,
    receiver: Receiver<AudioPacket>,
    lost: &[AtomicBool; 2],
) -> RollingOutcome {
    crate::parakeet::deprioritize_background_thread();
    let mut tracks: [Option<RollingTrack>; 2] = [None, None];
    let mut pending: VecDeque<(usize, Chunk)> = VecDeque::new();
    let mut envelope = EchoEnvelope::default();
    let mut model = None;
    let mut model_failed = false;
    loop {
        let mut disconnected = false;
        loop {
            match receiver.try_recv() {
                Ok(packet) => ingest(&mut tracks, &mut pending, &mut envelope, lost, packet),
                Err(TryRecvError::Empty) => break,
                Err(TryRecvError::Disconnected) => {
                    disconnected = true;
                    break;
                }
            }
        }
        if disconnected {
            break;
        }
        if !pending.is_empty() && !model_failed && !crate::parakeet::dictation_busy() {
            transcribe_pending(
                &mut tracks,
                &mut pending,
                &mut model,
                &mut model_failed,
                selection,
            );
            write_rolling_snapshot(directory, &tracks, &envelope);
            continue;
        }
        if pending.is_empty()
            && !model_failed
            && !crate::parakeet::dictation_busy()
            && !crate::transcription_models::use_english_meeting_draft(selection)
            && draft_uncommitted(&mut tracks, &mut model, &mut model_failed, selection)
        {
            write_rolling_snapshot(directory, &tracks, &envelope);
            continue;
        }
        match receiver.recv_timeout(Duration::from_millis(200)) {
            Ok(packet) => ingest(&mut tracks, &mut pending, &mut envelope, lost, packet),
            Err(RecvTimeoutError::Timeout) => {}
            Err(RecvTimeoutError::Disconnected) => break,
        }
    }
    for (index, track) in tracks.iter_mut().enumerate() {
        let Some(track) = track else { continue };
        if track.failed {
            continue;
        }
        let tail = track
            .resampler
            .take()
            .map(StreamingResampler::finish)
            .unwrap_or_default();
        envelope.push(
            track.transcript.source,
            track.transcript.origin_ms + samples_ms(track.fed),
            &tail,
            RATE as u32,
        );
        track.fed += tail.len();
        for chunk in track.chunker.push(&tail) {
            track.pending += 1;
            pending.push_back((index, chunk));
        }
        let chunker = std::mem::take(&mut track.chunker);
        if let Some(chunk) = chunker.finish() {
            track.pending += 1;
            pending.push_back((index, chunk));
        }
    }
    while !pending.is_empty() && !model_failed {
        wait_for_dictation();
        transcribe_pending(
            &mut tracks,
            &mut pending,
            &mut model,
            &mut model_failed,
            selection,
        );
    }
    let [system, microphone] = tracks;
    let complete = |track: Option<RollingTrack>, index: usize| {
        track
            .filter(|track| !track.failed && !model_failed && !lost[index].load(Ordering::Relaxed))
            .map(|track| track.transcript)
    };
    RollingOutcome {
        tracks: [complete(system, 0), complete(microphone, 1)],
        envelope,
        model,
    }
}

fn ingest(
    tracks: &mut [Option<RollingTrack>; 2],
    pending: &mut VecDeque<(usize, Chunk)>,
    envelope: &mut EchoEnvelope,
    lost: &[AtomicBool; 2],
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
            transcript: TrackTranscript::new(packet.source, packet.arrival_us / 1_000),
            fed: 0,
            pending: 0,
            provisional: Vec::new(),
            drafted_samples: 0,
            provisional_stopped: false,
            last_pass: None,
        }
    });
    if track.failed
        || lost[index].load(Ordering::Relaxed)
        || track.sample_rate != packet.sample_rate
    {
        track.failed = true;
        return;
    }
    let Some(resampler) = &mut track.resampler else {
        return;
    };
    let samples = resampler.push(&packet.samples);
    envelope.push(
        packet.source,
        track.transcript.origin_ms + samples_ms(track.fed),
        &samples,
        RATE as u32,
    );
    track.fed += samples.len();
    for chunk in track.chunker.push(&samples) {
        track.pending += 1;
        pending.push_back((index, chunk));
    }
    if track.pending > MAX_PENDING_CHUNKS {
        tracing::warn!(source = ?packet.source, "rolling meeting transcription backlog is full");
        track.failed = true;
        pending.retain(|(source, _)| *source != index);
    }
}

fn transcribe_pending(
    tracks: &mut [Option<RollingTrack>; 2],
    pending: &mut VecDeque<(usize, Chunk)>,
    model: &mut Option<Transcriber>,
    model_failed: &mut bool,
    selection: &TranscriptionSelection,
) {
    let Some((index, chunk)) = pending.pop_front() else {
        return;
    };
    let Some(track) = tracks[index].as_mut() else {
        return;
    };
    track.pending = track.pending.saturating_sub(1);
    if track.failed {
        return;
    }
    let model = match load_model(model, selection) {
        Ok(model) => model,
        Err(error) => {
            tracing::warn!(%error, "rolling meeting transcription could not load the model");
            *model_failed = true;
            return;
        }
    };
    let started = Instant::now();
    let result = model.transcribe_segments(chunk.samples.clone());
    if started.elapsed() > SLOW_CHUNK {
        tracing::warn!(
            elapsed_ms = started.elapsed().as_millis() as u64,
            audio_ms = samples_ms(chunk.samples.len()),
            "rolling meeting chunk was slow"
        );
    }
    match result {
        Ok(transcript) => {
            track.transcript.accept(&chunk, transcript);
            track.provisional.clear();
            track.drafted_samples = 0;
        }
        Err(error) => {
            tracing::warn!(%error, "rolling meeting chunk failed");
            track.failed = true;
            pending.retain(|(source, _)| *source != index);
        }
    }
}

fn draft_uncommitted(
    tracks: &mut [Option<RollingTrack>; 2],
    model: &mut Option<Transcriber>,
    model_failed: &mut bool,
    selection: &TranscriptionSelection,
) -> bool {
    let mut choice: Option<(usize, usize)> = None;
    for index in 0..tracks.len() {
        let Some(track) = tracks[index].as_mut() else {
            continue;
        };
        if track.failed || track.provisional_stopped {
            continue;
        }
        let tail = track.chunker.uncommitted().1.len();
        if track.drafted_samples > tail {
            track.drafted_samples = 0;
            track.provisional.clear();
        }
        match crate::meeting_provisional::draft_step(
            tail,
            track.drafted_samples,
            track.provisional_stopped,
            track.last_pass,
        ) {
            crate::meeting_provisional::DraftStep::Stop => track.provisional_stopped = true,
            crate::meeting_provisional::DraftStep::Wait => {}
            crate::meeting_provisional::DraftStep::Transcribe => {
                let undrafted = tail.saturating_sub(track.drafted_samples);
                let replace = match &choice {
                    None => true,
                    Some((_, best)) => undrafted > *best,
                };
                if replace {
                    choice = Some((index, undrafted));
                }
            }
        }
    }
    let Some((index, _)) = choice else {
        return false;
    };
    let (audio_start, samples) = {
        let Some(track) = tracks[index].as_ref() else {
            return false;
        };
        let (start, audio) = track.chunker.uncommitted();
        (start, audio.to_vec())
    };
    let audio_len = samples.len();
    let (elapsed, result) = {
        let model = match load_model(model, selection) {
            Ok(model) => model,
            Err(error) => {
                tracing::warn!(%error, "meeting draft could not load the model");
                *model_failed = true;
                return false;
            }
        };
        let started = Instant::now();
        let result = model.transcribe_segments(samples);
        (started.elapsed(), result)
    };
    let Some(track) = tracks[index].as_mut() else {
        return false;
    };
    track.last_pass = Some(crate::meeting_provisional::LastPass {
        elapsed,
        samples: audio_len,
    });
    track.drafted_samples = audio_len;
    match result {
        Ok(transcript) => {
            let (lines, _) = timed_entries(
                track.transcript.source,
                track.transcript.origin_ms + samples_ms(audio_start),
                samples_ms(audio_len),
                transcript,
            );
            track.provisional = lines;
        }
        Err(error) => {
            tracing::warn!(%error, "meeting draft pass failed");
            track.provisional_stopped = true;
        }
    }
    if crate::meeting_provisional::draft_step(
        audio_len,
        track.drafted_samples,
        false,
        track.last_pass,
    ) == crate::meeting_provisional::DraftStep::Stop
    {
        tracing::warn!(
            elapsed_ms = elapsed.as_millis() as u64,
            audio_ms = samples_ms(audio_len),
            "meeting draft pass was slower than live audio; further drafts stopped"
        );
        track.provisional_stopped = true;
    }
    true
}

/// Meeting-timeline milliseconds through which each source's rolling
/// transcript replaces the live draft, including entries dropped as echo.
#[derive(Clone, Copy, Debug, Default, Deserialize, Eq, PartialEq, Serialize)]
pub(super) struct RollingCoverage {
    pub system: Option<u64>,
    pub microphone: Option<u64>,
}

impl RollingCoverage {
    pub(super) fn get(&self, source: MeetingSource) -> Option<u64> {
        match source {
            MeetingSource::System => self.system,
            MeetingSource::Microphone => self.microphone,
        }
    }
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

/// The joined rolling transcript. A microphone entry is published only once
/// the system track is transcribed through its end, so echo is judged
/// against the remote words regardless of which chunk finished first. A
/// system track that stopped advancing is not waited for.
fn rolling_view(
    tracks: [Option<&TrackTranscript>; 2],
    system_advancing: bool,
    envelope: &EchoEnvelope,
) -> RollingView {
    let origin_ms = tracks
        .iter()
        .flatten()
        .map(|track| track.origin_ms)
        .min()
        .unwrap_or_default();
    let [system, microphone] = tracks;
    let system_through = system.map(|track| track.through_ms);
    let microphone_through = microphone.map(|track| match system_through {
        Some(system) if system_advancing => track.through_ms.min(system),
        _ => track.through_ms,
    });
    // A track that fell behind keeps showing what it transcribed; the live
    // draft continues after it.
    let entries = system
        .into_iter()
        .flat_map(|track| track.entries.iter().cloned())
        .chain(microphone.into_iter().flat_map(|track| {
            track
                .entries
                .iter()
                .filter(|entry| microphone_through.is_none_or(|through| entry.end_ms <= through))
                .cloned()
        }))
        .collect::<Vec<_>>();
    let (entries, _) = join_entries(entries, origin_ms, envelope);
    RollingView {
        entries,
        covered_ms: RollingCoverage {
            system: system_through.map(|through| through.saturating_sub(origin_ms)),
            microphone: microphone_through.map(|through| through.saturating_sub(origin_ms)),
        },
    }
}

fn provisional_entries(
    tracks: &[Option<RollingTrack>; 2],
    envelope: &EchoEnvelope,
) -> Vec<TranscriptEntry> {
    if tracks
        .iter()
        .flatten()
        .all(|track| track.provisional.is_empty())
    {
        return Vec::new();
    }
    let origin_ms = tracks
        .iter()
        .flatten()
        .map(|track| track.transcript.origin_ms)
        .min()
        .unwrap_or_default();
    let mut entries = Vec::new();
    for track in tracks.iter().flatten() {
        if track.transcript.source == MeetingSource::System {
            entries.extend(track.transcript.entries.iter().cloned());
        }
        entries.extend(track.provisional.iter().cloned());
    }
    let (mut entries, _) = suppress_echo(entries, |start, end| envelope.assess(start, end));
    entries.retain(|entry| {
        tracks[source_index(entry.source)]
            .as_ref()
            .is_none_or(|track| entry.start_ms >= track.transcript.through_ms)
    });
    for entry in &mut entries {
        entry.start_ms = entry.start_ms.saturating_sub(origin_ms);
        entry.end_ms = entry.end_ms.saturating_sub(origin_ms);
    }
    entries.sort_by_key(|entry| (entry.start_ms, source_index(entry.source)));
    entries
}

/// Atomically replaces the draft that the Meetings pane shows while recording.
fn write_rolling_snapshot(
    directory: &Path,
    tracks: &[Option<RollingTrack>; 2],
    envelope: &EchoEnvelope,
) {
    let result = (|| -> Result<()> {
        let [system, microphone] = tracks;
        let view = rolling_view(
            [system, microphone].map(|track| track.as_ref().map(|track| &track.transcript)),
            system.as_ref().is_some_and(|track| !track.failed),
            envelope,
        );
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
        let provisional = provisional_entries(tracks, envelope);
        for entry in view.entries.iter().chain(&provisional) {
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

/// Produces the final transcript, using complete rolling tracks and
/// streaming any remaining track from its WAV. Returns the joined entries and
/// the number of echo utterances removed.
pub(super) fn transcribe(
    directory: &Path,
    manifest: &MeetingManifest,
    selection: &TranscriptionSelection,
    rolling: Option<RollingOutcome>,
) -> Result<(Vec<TranscriptEntry>, u64)> {
    crate::parakeet::deprioritize_background_thread();
    let (mut rolled, mut envelope, mut model) = match rolling {
        Some(outcome) => (outcome.tracks, outcome.envelope, outcome.model),
        None => ([None, None], EchoEnvelope::default(), None),
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
                    &mut model,
                    selection,
                    &directory.join(&track.file),
                    source,
                    origin_ms,
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

fn transcribe_file(
    model: &mut Option<Transcriber>,
    selection: &TranscriptionSelection,
    path: &Path,
    source: MeetingSource,
    origin_ms: u64,
    envelope: &mut EchoEnvelope,
) -> Result<TrackTranscript> {
    let mut transcript = TrackTranscript::new(source, origin_ms);
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
        wait_for_dictation();
        let result = load_model(model, selection)?.transcribe_segments(chunk.samples.clone())?;
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
                let transcript = load_model(&mut model, selection)?
                    .transcribe_segments(chunk.samples.clone())?;
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
        old_entries.extend(old_track.entries);
        new_entries.extend(new_track.entries);
    }

    let live_path = directory.join("transcript.live.ndjson");
    if live_path.exists() {
        let mut events = super::live::latest_events(&live_path)?;
        let before = events
            .iter()
            .filter(|event| event.source == MeetingSource::Microphone)
            .count();
        for event in &mut events {
            if event.source == MeetingSource::Microphone {
                event.signal = envelope.assess(event.start_ms, event.end_ms);
            }
        }
        let shown = super::live::live_view(&RollingView::default(), events.iter());
        let after = shown
            .iter()
            .filter(|entry| entry.source == MeetingSource::Microphone)
            .count();
        let text_only = events
            .iter()
            .cloned()
            .map(|mut event| {
                event.signal = None;
                event
            })
            .collect::<Vec<_>>();
        let words_only = super::live::live_view(&RollingView::default(), text_only.iter())
            .iter()
            .filter(|entry| entry.source == MeetingSource::Microphone)
            .count();
        writeln!(
            report,
            "live draft (microphone offset {offset_ms} ms): {before} You lines shown before, \
             {words_only} with word matching only, {after} with signal and words"
        )?;
        events.sort_by_key(|event| event.start_ms);
        for event in events
            .iter()
            .filter(|event| event.source == MeetingSource::Microphone)
        {
            let kept = shown
                .iter()
                .any(|entry| entry.source == event.source && entry.start_ms == event.start_ms);
            writeln!(
                report,
                "  {} {:>7} {:<6} {:>4} {}",
                if kept { "KEEP" } else { "drop" },
                event.start_ms,
                match event.signal {
                    Some(Signal::Bleed) => "bleed",
                    Some(Signal::Local) => "local",
                    None => "?",
                },
                envelope
                    .locality(event.start_ms, event.end_ms)
                    .map_or("-".into(), |locality| format!("{:.2}", locality.share)),
                event.text
            )?;
        }
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
        // SAFETY: `localtime_r` writes only into the provided zeroed `tm`.
        let mut tm: libc::tm = unsafe { std::mem::zeroed() };
        unsafe { libc::localtime_r(&seconds, &mut tm) };
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
            len: chunk.samples.len(),
            transcript: model.transcribe_segments(chunk.samples).unwrap(),
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
    let view = rolling_view([Some(system), Some(microphone)], true, envelope);
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
        crate::pause_segmenter::tests::utterance(&[(seconds * 1_000, 0)], -70.0)
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
        let audio = speech(65);
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
        assert_eq!(spans, [(0, 0, 30), (28, 2, 32), (58, 2, 7)]);
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
    fn uncommitted_tail_is_audio_the_chunker_has_not_cut() {
        let audio = speech(25);
        let mut chunker = Chunker::default();
        assert!(chunker.push(&audio).is_empty());
        let (start, tail) = chunker.uncommitted();
        assert_eq!(start, 0);
        assert_eq!(tail, audio.as_slice());

        let audio = speech(40);
        let mut chunker = Chunker::default();
        let chunks = chunker.push(&audio);
        assert_eq!(chunks.len(), 1);
        let (start, tail) = chunker.uncommitted();
        assert_eq!(start, chunks[0].start + chunks[0].samples.len());
        assert_eq!(tail, &audio[start..]);
        assert!(start > chunks[0].start);
        assert_eq!(&audio[chunks[0].start..start], chunks[0].samples.as_slice());
    }

    #[test]
    fn chunks_end_at_the_first_pause_after_the_target_length() {
        let audio = crate::pause_segmenter::tests::utterance(
            &[(12_000, 800), (9_000, 800), (6_000, 800), (4_000, 0)],
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
        // The 12.3 s cut is too early; the pause at 21.8 s ends the chunk.
        assert_eq!(chunks[1].start * 1_000 / RATE, 21_950);
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
    fn overlapping_chunks_join_at_the_middle_of_the_overlap() {
        let mut track = TrackTranscript::new(MeetingSource::System, 1_000);
        let first = Chunk {
            start: 0,
            overlap: 0,
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
        track.through_ms = through_ms;
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
        let view = rolling_view([Some(&system), Some(&microphone)], true, &envelope);
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
        let view = rolling_view([Some(&pending), Some(&microphone)], true, &envelope);
        assert!(view.entries.is_empty());
        assert_eq!(view.covered_ms.microphone, Some(0));

        // A system track that stopped advancing is not waited for.
        let view = rolling_view([Some(&system), Some(&microphone)], false, &envelope);
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
        assert_eq!(view.covered_ms.get(MeetingSource::Microphone), Some(28_000));
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
        let grouped = sentences(vec![
            word(0, 300, " Basically,"),
            word(320, 500, "yes."),
            word(560, 800, "Then"),
            word(820, 1_000, "this"),
            word(2_000, 2_300, "Later"),
            word(2_320, 2_600, "words"),
        ]);
        assert_eq!(
            grouped
                .iter()
                .map(|sentence| (sentence.start_ms, sentence.end_ms, sentence.text.as_str()))
                .collect::<Vec<_>>(),
            [
                (0, 500, "Basically, yes."),
                (560, 1_000, "Then this"),
                (2_000, 2_600, "Later words"),
            ]
        );
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
