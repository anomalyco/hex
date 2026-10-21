use std::collections::{HashMap, HashSet};
use std::fs::{self, File};
use std::io::{BufWriter, Read, Seek, SeekFrom, Write};
use std::path::Path;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::mpsc::Receiver;
use std::time::{Duration, Instant};

use color_eyre::eyre::{Result, eyre};
use serde::{Deserialize, Serialize};

use crate::dictation::StreamingResampler;
use crate::events::TranscriptPhase;
use crate::moonshine::{Moonshine, MoonshineConfig};

use super::echo::{EchoEnvelope, Signal};
use super::transcript::{RollingLine, RollingView};
use super::{
    AudioPacket, MeetingSource, TranscriptEntry, final_transcript_exists, read_ndjson,
    recover_final_publication, root, set_owner_only,
};

pub(super) const QUEUE_CAPACITY: usize = 512;
const MODEL_SAMPLE_RATE: u32 = 16_000;
const UPDATE_INTERVAL: Duration = Duration::from_secs(1);
const MAX_GAP_US: u64 = 500_000;
/// A completed microphone line is judged once the system track reaches this
/// far past its end, so the remote audio that could explain it has arrived:
/// the speaker-to-microphone lag, reverberation, and arrival jitter.
const JUDGE_AFTER_MS: u64 = 1_000;
/// A system track that stops delivering audio, as when nothing plays, holds
/// a line back for at most this much further microphone audio.
const MAX_JUDGE_DELAY_MS: u64 = 3_000;

#[derive(Clone, Debug, Deserialize, Serialize)]
pub(super) struct Event {
    pub source: MeetingSource,
    pub line_id: u64,
    pub phase: TranscriptPhase,
    pub start_ms: u64,
    pub end_ms: u64,
    pub text: String,
    /// Microphone lines: whether the audio is speaker bleed or local speech.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub signal: Option<Signal>,
}

#[derive(Default)]
pub struct TranscriptReader {
    meeting_id: Option<String>,
    live_offset: u64,
    live_pending: Vec<u8>,
    live_events: HashMap<(MeetingSource, u64), Event>,
    final_entries: Option<Vec<TranscriptEntry>>,
    rolling_modified: Option<std::time::SystemTime>,
    rolling: RollingView,
}

impl TranscriptReader {
    pub fn read(&mut self, id: &str) -> Result<Vec<TranscriptEntry>> {
        if self.meeting_id.as_deref() != Some(id) {
            *self = Self {
                meeting_id: Some(id.into()),
                ..Self::default()
            };
        }
        let directory = root()?.join(id);
        recover_final_publication(&directory)?;
        let final_path = directory.join("transcript.ndjson");
        if final_transcript_exists(&directory) {
            if self.final_entries.is_none() {
                self.final_entries = Some(read_ndjson(&final_path)?);
                self.live_events.clear();
                self.live_pending.clear();
            }
            return Ok(self.final_entries.clone().unwrap_or_default());
        }

        self.read_live_tail(&directory.join("transcript.live.ndjson"))?;
        self.read_rolling(&directory.join(super::transcript::ROLLING_FILE))?;
        Ok(live_view(&self.rolling, self.live_events.values()))
    }

    fn read_rolling(&mut self, path: &Path) -> Result<()> {
        let modified = match fs::metadata(path) {
            Ok(metadata) => metadata.modified().ok(),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(()),
            Err(error) => return Err(error.into()),
        };
        if modified.is_some() && modified == self.rolling_modified {
            return Ok(());
        }
        self.rolling = read_ndjson::<RollingLine>(path)?.into_iter().collect();
        self.rolling_modified = modified;
        Ok(())
    }

    pub(super) fn read_live_tail(&mut self, path: &Path) -> Result<()> {
        let mut file = match File::open(path) {
            Ok(file) => file,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(()),
            Err(error) => return Err(error.into()),
        };
        let length = file.metadata()?.len();
        if length < self.live_offset {
            self.live_offset = 0;
            self.live_pending.clear();
            self.live_events.clear();
        }
        file.seek(SeekFrom::Start(self.live_offset))?;
        let mut appended = Vec::new();
        file.read_to_end(&mut appended)?;
        self.live_offset += appended.len() as u64;
        self.live_pending.extend(appended);

        let complete_bytes = self
            .live_pending
            .iter()
            .rposition(|byte| *byte == b'\n')
            .map_or(0, |newline| newline + 1);
        for line in self.live_pending[..complete_bytes].split(|byte| *byte == b'\n') {
            if line.is_empty() {
                continue;
            }
            let event: Event = serde_json::from_slice(line)?;
            self.live_events
                .insert((event.source, event.line_id), event);
        }
        self.live_pending = self.live_pending.split_off(complete_bytes);
        Ok(())
    }

    #[cfg(test)]
    pub(super) fn events(&self) -> impl Iterator<Item = &Event> {
        self.live_events.values()
    }

    #[cfg(test)]
    pub(super) fn event_count(&self) -> usize {
        self.live_events.len()
    }

    #[cfg(test)]
    pub(super) fn has_pending_bytes(&self) -> bool {
        !self.live_pending.is_empty()
    }
}

/// What the pane shows while recording: rolling final-model entries, then
/// the Moonshine draft after them, without microphone lines that echo the
/// remote audio. Rolling entries are already echo-free. Each draft line is
/// one voice-activity segment and is judged on its own: by the signal the
/// live worker stamped on it, else by whether its words repeat remote
/// speech. While the remote side is speaking, an undecided line waits until
/// it completes, so a twin of remote speech never flashes up as `You`.
pub(super) fn live_view<'a>(
    rolling: &RollingView,
    events: impl IntoIterator<Item = &'a Event>,
) -> Vec<TranscriptEntry> {
    let events = events.into_iter().collect::<Vec<_>>();
    let merged = merge_rolling(rolling, project_latest(events.iter().copied()));
    let signals = events
        .iter()
        .filter(|event| event.source == MeetingSource::Microphone)
        .map(|event| ((event.start_ms, event.end_ms), (event.signal, event.phase)))
        .collect::<HashMap<_, _>>();
    let rolling_microphone = rolling
        .entries
        .iter()
        .filter(|entry| entry.source == MeetingSource::Microphone)
        .collect::<HashSet<_>>();
    let remote = merged
        .iter()
        .filter(|entry| entry.source == MeetingSource::System)
        .collect::<Vec<_>>();
    let shown = merged
        .iter()
        .map(|entry| {
            if entry.source == MeetingSource::System || rolling_microphone.contains(entry) {
                return true;
            }
            let (signal, phase) = signals
                .get(&(entry.start_ms, entry.end_ms))
                .copied()
                .unwrap_or((None, TranscriptPhase::Completed));
            match signal {
                Some(Signal::Bleed) => false,
                Some(Signal::Local) => true,
                None if phase != TranscriptPhase::Completed => !remote
                    .iter()
                    .any(|remote| remote.start_ms < entry.end_ms && entry.start_ms < remote.end_ms),
                None => !super::transcript::repeats_remote(entry, &remote),
            }
        })
        .collect::<Vec<_>>();
    let mut entries = merged
        .into_iter()
        .zip(shown)
        .filter_map(|(entry, shown)| shown.then_some(entry))
        .collect::<Vec<_>>();
    entries.sort_by_key(|entry| (entry.start_ms, source_index(entry.source)));
    entries
}

/// Rolling final-model entries replace the Moonshine draft for the time they
/// cover, including microphone audio they dropped as echo; the draft fills in
/// only after each source's rolling coverage. Snapshots without coverage end
/// it at each source's last rolling entry.
pub(super) fn merge_rolling(
    rolling: &RollingView,
    live: Vec<TranscriptEntry>,
) -> Vec<TranscriptEntry> {
    if rolling.entries.is_empty() && rolling.covered_ms == Default::default() {
        return live;
    }
    let covered = |source| {
        rolling.covered_ms.get(source).or_else(|| {
            rolling
                .entries
                .iter()
                .filter(|entry| entry.source == source)
                .map(|entry| entry.end_ms)
                .max()
        })
    };
    let covered = [
        covered(MeetingSource::System),
        covered(MeetingSource::Microphone),
    ];
    let mut entries = rolling.entries.clone();
    entries.extend(live.into_iter().filter(|entry| {
        covered[source_index(entry.source)].is_none_or(|covered| entry.start_ms >= covered)
    }));
    entries.sort_by_key(|entry| entry.start_ms);
    entries
}

pub(super) fn transcribe(
    project_root: &Path,
    directory: &Path,
    receiver: Receiver<AudioPacket>,
    drops: &AtomicU64,
) -> Result<()> {
    let path = directory.join("transcript.live.ndjson");
    let file = File::create(&path)?;
    set_owner_only(&path, false)?;
    super::sync_directory(directory)?;
    let mut writer = BufWriter::new(file);
    let mut model = Moonshine::load_with_config(
        project_root,
        2,
        MoonshineConfig {
            transcription_interval_ms: UPDATE_INTERVAL.as_millis() as u32,
            word_timestamps: false,
        },
    )?;
    let started = Instant::now();
    let mut source_origins = [None, None];
    let mut next_positions = [0_u64, 0];
    let mut model_samples = [0_u64, 0];
    let mut time_adjustments = [Vec::new(), Vec::new()];
    let mut resamplers: [Option<StreamingResampler>; 2] = [None, None];
    let mut envelope = EchoEnvelope::default();
    let mut pending = Vec::new();
    let update_clock = Instant::now();
    let mut next_updates = [
        update_clock + UPDATE_INTERVAL,
        update_clock + UPDATE_INTERVAL + UPDATE_INTERVAL / 2,
    ];

    while let Ok(mut packet) = receiver.recv() {
        let stream = source_index(packet.source);
        let rate = u64::from(packet.sample_rate.max(1));
        let resampler = match &mut resamplers[stream] {
            Some(resampler) => resampler,
            slot @ None => slot
                .insert(StreamingResampler::new(packet.sample_rate).map_err(|error| eyre!(error))?),
        };
        // The first packet this worker sees may follow dropped ones; the
        // origin is where the recorded track began.
        source_origins[stream].get_or_insert(
            packet
                .arrival_us
                .saturating_sub(packet.position * 1_000_000 / rate),
        );
        if let Some(gap) = audio_gap(next_positions[stream], &packet) {
            add_silence(&mut model, resampler, stream, gap.silence_samples)?;
            model_samples[stream] += gap.silence_samples;
            if gap.skipped_us > 0 {
                let skipped = time_adjustments[stream]
                    .last()
                    .map_or(gap.skipped_us, |(_, skipped)| skipped + gap.skipped_us);
                time_adjustments[stream].push((model_samples[stream] * 1_000_000 / rate, skipped));
            }
        }
        envelope.push(
            packet.source,
            source_offset(source_origins, stream) / 1_000 + packet.position * 1_000 / rate,
            &packet.samples,
            packet.sample_rate,
        );
        normalize_microphone(packet.source, &mut packet.samples);
        let resampled = resampler.push(&packet.samples);
        if !resampled.is_empty() {
            model.add_audio_to(stream, &resampled, MODEL_SAMPLE_RATE)?;
        }
        model_samples[stream] += packet.samples.len() as u64;
        next_positions[stream] = packet.position + packet.samples.len() as u64;
        if Instant::now() >= next_updates[stream] {
            write_updates(
                &mut model,
                packet.source,
                source_offset(source_origins, stream),
                &time_adjustments[stream],
                &envelope,
                &mut pending,
                &mut writer,
                false,
            )?;
            next_updates[stream] = Instant::now() + UPDATE_INTERVAL;
        }
        if !pending.is_empty() {
            write_judged(&envelope, &mut pending, &mut writer, false)?;
        }
    }
    let mut finalization_error = None;
    for source in [MeetingSource::System, MeetingSource::Microphone] {
        let stream = source_index(source);
        if let Some(resampler) = resamplers[stream].take() {
            let tail = resampler.finish();
            if !tail.is_empty()
                && let Err(error) = model.add_audio_to(stream, &tail, MODEL_SAMPLE_RATE)
            {
                tracing::warn!(?source, %error, "could not flush live meeting resampler");
                finalization_error.get_or_insert(error);
                continue;
            }
        }
        if let Err(error) = write_updates(
            &mut model,
            source,
            source_offset(source_origins, stream),
            &time_adjustments[stream],
            &envelope,
            &mut pending,
            &mut writer,
            true,
        ) {
            tracing::warn!(?source, %error, "could not finalize live meeting source");
            finalization_error.get_or_insert(error);
        }
    }
    if let Err(error) = write_judged(&envelope, &mut pending, &mut writer, true) {
        finalization_error.get_or_insert(error);
    }
    writer.flush()?;
    writer.get_ref().sync_all()?;
    if let Some(error) = finalization_error {
        return Err(error);
    }
    tracing::info!(
        elapsed_ms = started.elapsed().as_millis(),
        dropped_live_packets = drops.load(Ordering::Relaxed),
        "live meeting transcription stopped"
    );
    Ok(())
}

pub(super) fn persist_error(directory: &Path, error: &color_eyre::Report) {
    let path = directory.join("transcript.live.error.txt");
    let result = (|| -> Result<()> {
        fs::write(&path, format!("{error}\n"))?;
        set_owner_only(&path, false)?;
        File::open(path)?.sync_all()?;
        Ok(())
    })();
    if let Err(persist_error) = result {
        tracing::error!(%persist_error, %error, "could not persist live transcription failure");
    }
}

#[cfg(any(debug_assertions, test))]
pub(super) fn read_snapshot(path: &Path) -> Result<Vec<TranscriptEntry>> {
    Ok(live_view(
        &RollingView::default(),
        latest(read_ndjson(path)?).values(),
    ))
}

pub(super) fn read_completed_snapshot(path: &Path) -> Result<Vec<TranscriptEntry>> {
    let events: Vec<Event> = read_ndjson(path)?;
    Ok(live_view(
        &RollingView::default(),
        latest(
            events
                .into_iter()
                .filter(|event| event.phase == TranscriptPhase::Completed),
        )
        .values(),
    ))
}

/// Rolling entries of committed chunks, without the selected-model draft
/// written after them.
pub(super) fn read_committed_rolling(path: &Path) -> Result<Vec<TranscriptEntry>> {
    if !path.exists() {
        return Ok(Vec::new());
    }
    let RollingView {
        entries,
        covered_ms,
    } = read_ndjson::<RollingLine>(path)?.into_iter().collect();
    Ok(entries
        .into_iter()
        .filter(|entry| {
            covered_ms
                .get(entry.source)
                .is_none_or(|covered| entry.end_ms <= covered)
        })
        .collect())
}

#[cfg(debug_assertions)]
pub(super) fn latest_events(path: &Path) -> Result<Vec<Event>> {
    Ok(latest(read_ndjson(path)?).into_values().collect())
}

#[cfg(test)]
pub(super) fn project(events: impl IntoIterator<Item = Event>) -> Vec<TranscriptEntry> {
    live_view(&RollingView::default(), latest(events).values())
}

fn latest(events: impl IntoIterator<Item = Event>) -> HashMap<(MeetingSource, u64), Event> {
    events
        .into_iter()
        .map(|event| ((event.source, event.line_id), event))
        .collect()
}

#[derive(Debug, Eq, PartialEq)]
pub(super) struct AudioGap {
    pub(super) silence_samples: u64,
    pub(super) skipped_us: u64,
}

/// Audio missing before `packet` on its track, because this worker dropped
/// packets: up to half a second becomes model silence and the rest is skipped
/// on the timeline.
pub(super) fn audio_gap(expected_position: u64, packet: &AudioPacket) -> Option<AudioGap> {
    let missing = packet.position.checked_sub(expected_position)?;
    if missing == 0 {
        return None;
    }
    let rate = u64::from(packet.sample_rate.max(1));
    let silence_samples = missing.min(MAX_GAP_US * rate / 1_000_000);
    Some(AudioGap {
        silence_samples,
        skipped_us: (missing - silence_samples) * 1_000_000 / rate,
    })
}

pub(super) fn normalize_microphone(source: MeetingSource, samples: &mut [f32]) {
    if source != MeetingSource::Microphone || samples.is_empty() {
        return;
    }
    let rms =
        (samples.iter().map(|sample| sample * sample).sum::<f32>() / samples.len() as f32).sqrt();
    if rms < 0.0005 {
        return;
    }
    let gain = (0.035 / rms).clamp(1.0, 24.0);
    for sample in samples {
        *sample = (*sample * gain).clamp(-1.0, 1.0);
    }
}

pub(super) fn source_offset(origins: [Option<u64>; 2], stream: usize) -> u64 {
    let origin = origins.into_iter().flatten().min().unwrap_or_default();
    origins[stream].unwrap_or(origin).saturating_sub(origin)
}

fn project_latest<'a>(events: impl IntoIterator<Item = &'a Event>) -> Vec<TranscriptEntry> {
    let mut transcript = events
        .into_iter()
        .filter_map(|event| {
            let text = event.text.trim().to_string();
            (!text.is_empty()).then_some(TranscriptEntry {
                source: event.source,
                start_ms: event.start_ms,
                end_ms: event.end_ms,
                text,
            })
        })
        .collect::<Vec<_>>();
    transcript.sort_by_key(|entry| (entry.start_ms, source_index(entry.source)));
    transcript
}

fn add_silence(
    model: &mut Moonshine,
    resampler: &mut StreamingResampler,
    stream: usize,
    mut samples: u64,
) -> Result<()> {
    let silence = [0.0; 1_600];
    while samples > 0 {
        let count = samples.min(silence.len() as u64) as usize;
        let resampled = resampler.push(&silence[..count]);
        if !resampled.is_empty() {
            model.add_audio_to(stream, &resampled, MODEL_SAMPLE_RATE)?;
        }
        samples -= count as u64;
    }
    Ok(())
}

#[allow(clippy::too_many_arguments)]
fn write_updates(
    model: &mut Moonshine,
    source: MeetingSource,
    source_offset_us: u64,
    time_adjustments: &[(u64, u64)],
    envelope: &EchoEnvelope,
    pending: &mut Vec<Event>,
    writer: &mut BufWriter<File>,
    finish: bool,
) -> Result<()> {
    let stream = source_index(source);
    let updates = if finish {
        model.finish_stream(stream)?
    } else {
        model.update_stream(stream)?
    };
    for update in updates {
        let start_ms =
            source_offset_us / 1_000 + adjusted_time_ms(update.start_ms, time_adjustments);
        let end_ms = source_offset_us / 1_000 + adjusted_time_ms(update.end_ms, time_adjustments);
        let mut event = Event {
            source,
            line_id: update.line_id,
            phase: update.phase,
            start_ms,
            end_ms,
            text: update.text,
            signal: None,
        };
        if source == MeetingSource::Microphone {
            if event.phase == TranscriptPhase::Completed {
                pending.push(event);
                continue;
            }
            event.signal = envelope.assess(start_ms, end_ms);
        }
        write_event(writer, &event)?;
    }
    writer.flush()?;
    writer.get_ref().sync_data()?;
    Ok(())
}

/// Writes completed microphone lines once the audio that judges them has
/// arrived: the system track past their end, or enough later microphone
/// audio that the system track evidently stalled. `finish` writes all.
fn write_judged(
    envelope: &EchoEnvelope,
    pending: &mut Vec<Event>,
    writer: &mut BufWriter<File>,
    finish: bool,
) -> Result<()> {
    let system = envelope.covered_ms(MeetingSource::System);
    let microphone = envelope.covered_ms(MeetingSource::Microphone);
    let mut written = false;
    let mut error = None;
    pending.retain_mut(|event| {
        let judged = finish
            || system >= event.end_ms + JUDGE_AFTER_MS
            || microphone >= event.end_ms + MAX_JUDGE_DELAY_MS;
        if !judged || error.is_some() {
            return true;
        }
        event.signal = envelope.assess(event.start_ms, event.end_ms);
        written = true;
        if let Err(write_error) = write_event(writer, event) {
            error = Some(write_error);
        }
        false
    });
    if let Some(error) = error {
        return Err(error);
    }
    if written {
        writer.flush()?;
        writer.get_ref().sync_data()?;
    }
    Ok(())
}

fn write_event(writer: &mut BufWriter<File>, event: &Event) -> Result<()> {
    serde_json::to_writer(&mut *writer, event)?;
    writer.write_all(b"\n")?;
    Ok(())
}

pub(super) fn adjusted_time_ms(model_ms: u64, adjustments: &[(u64, u64)]) -> u64 {
    let model_us = model_ms * 1_000;
    let skipped_us = adjustments
        .iter()
        .rev()
        .find_map(|(at_us, skipped_us)| (model_us >= *at_us).then_some(*skipped_us))
        .unwrap_or_default();
    model_ms + skipped_us / 1_000
}

fn source_index(source: MeetingSource) -> usize {
    match source {
        MeetingSource::System => 0,
        MeetingSource::Microphone => 1,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn completed_microphone_lines_wait_for_the_system_audio_that_judges_them() {
        let path = std::env::temp_dir().join(format!(
            "voice-control-live-judged-{}-{}.ndjson",
            std::process::id(),
            crate::events::now_ms()
        ));
        let mut writer = BufWriter::new(File::create(&path).unwrap());
        let mut envelope = EchoEnvelope::default();
        let audio = |seconds: usize| vec![0.01; 16_000 * seconds];
        envelope.push(MeetingSource::System, 0, &audio(2), 16_000);
        envelope.push(MeetingSource::Microphone, 0, &audio(5), 16_000);
        let line = |line_id, end_ms| Event {
            source: MeetingSource::Microphone,
            line_id,
            phase: TranscriptPhase::Completed,
            start_ms: 1_000,
            end_ms,
            text: "hello".into(),
            signal: None,
        };
        let mut pending = vec![line(1, 1_500), line(2, 2_500)];
        // The system track stalled long enough for the first line only.
        write_judged(&envelope, &mut pending, &mut writer, false).unwrap();
        assert_eq!(pending.len(), 1);
        envelope.push(MeetingSource::System, 2_000, &audio(2), 16_000);
        write_judged(&envelope, &mut pending, &mut writer, false).unwrap();
        assert!(pending.is_empty());
        drop(writer);
        let written: Vec<Event> = read_ndjson(&path).unwrap();
        assert_eq!(
            written
                .iter()
                .map(|event| event.line_id)
                .collect::<Vec<_>>(),
            [1, 2]
        );
        fs::remove_file(path).unwrap();
    }

    #[test]
    fn incremental_projection_matches_snapshot_after_updates_and_partial_writes() {
        let path = std::env::temp_dir().join(format!(
            "voice-control-live-projection-{}-{}.ndjson",
            std::process::id(),
            crate::events::now_ms()
        ));
        let event = |source, line_id, phase, start_ms, text: &str| Event {
            source,
            line_id,
            phase,
            start_ms,
            end_ms: start_ms + 500,
            text: text.into(),
            signal: None,
        };
        let events = [
            event(
                MeetingSource::Microphone,
                1,
                TranscriptPhase::Started,
                1_000,
                "draft",
            ),
            event(
                MeetingSource::System,
                1,
                TranscriptPhase::Completed,
                1_000,
                " computer ",
            ),
            event(
                MeetingSource::Microphone,
                1,
                TranscriptPhase::Updated,
                1_000,
                " updated ",
            ),
            event(
                MeetingSource::Microphone,
                2,
                TranscriptPhase::Started,
                2_000,
                "discard me",
            ),
            event(
                MeetingSource::Microphone,
                2,
                TranscriptPhase::Completed,
                2_000,
                "  ",
            ),
            event(
                MeetingSource::Microphone,
                1,
                TranscriptPhase::Completed,
                1_000,
                " olá final ",
            ),
        ];
        let mut file = File::create(&path).unwrap();
        let mut reader = TranscriptReader::default();
        for event in events {
            let mut bytes = serde_json::to_vec(&event).unwrap();
            bytes.push(b'\n');
            let split = bytes.len() / 2;
            for part in [&bytes[..split], &bytes[split..]] {
                file.write_all(part).unwrap();
                file.flush().unwrap();
                reader.read_live_tail(&path).unwrap();
                assert_eq!(
                    live_view(&RollingView::default(), reader.live_events.values()),
                    read_snapshot(&path).unwrap(),
                );
            }
        }
        let transcript = live_view(&RollingView::default(), reader.live_events.values());
        assert_eq!(transcript.len(), 2);
        assert_eq!(transcript[0].source, MeetingSource::System);
        assert_eq!(transcript[0].text, "computer");
        assert_eq!(transcript[1].source, MeetingSource::Microphone);
        assert_eq!(transcript[1].text, "olá final");
        fs::remove_file(path).unwrap();
    }
}
