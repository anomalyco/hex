use std::collections::HashMap;
use std::fs::{self, File};
use std::io::{BufWriter, Read, Seek, SeekFrom, Write};
use std::path::Path;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::mpsc::Receiver;
use std::time::{Duration, Instant};

use color_eyre::eyre::Result;
use serde::{Deserialize, Serialize};

use crate::events::TranscriptPhase;
use crate::moonshine::Moonshine;

use super::{
    AudioPacket, MeetingSource, TranscriptEntry, final_transcript_exists, read_ndjson,
    recover_final_publication, root, set_owner_only,
};

pub(super) const QUEUE_CAPACITY: usize = 64;
const UPDATE_INTERVAL: Duration = Duration::from_secs(1);
const MAX_GAP_US: u64 = 500_000;

#[derive(Clone, Debug, Deserialize, Serialize)]
pub(super) struct Event {
    pub source: MeetingSource,
    pub line_id: u64,
    pub phase: TranscriptPhase,
    pub start_ms: u64,
    pub end_ms: u64,
    pub text: String,
}

#[derive(Default)]
pub struct TranscriptReader {
    meeting_id: Option<String>,
    live_offset: u64,
    live_pending: Vec<u8>,
    live_events: HashMap<(MeetingSource, u64), Event>,
    final_entries: Option<Vec<TranscriptEntry>>,
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
        Ok(project(self.live_events.values().cloned()))
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
    pub(super) fn event_count(&self) -> usize {
        self.live_events.len()
    }

    #[cfg(test)]
    pub(super) fn has_pending_bytes(&self) -> bool {
        !self.live_pending.is_empty()
    }
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
    let mut model = Moonshine::load_with_streams(project_root, 2)?;
    let started = Instant::now();
    let mut source_origins = [None, None];
    let mut next_pts = [None, None];
    let mut next_arrivals = [None, None];
    let mut model_audio_us = [0, 0];
    let mut time_adjustments = [Vec::new(), Vec::new()];
    let update_clock = Instant::now();
    let mut next_updates = [
        update_clock + UPDATE_INTERVAL,
        update_clock + UPDATE_INTERVAL + UPDATE_INTERVAL / 2,
    ];

    while let Ok(mut packet) = receiver.recv() {
        let stream = source_index(packet.source);
        source_origins[stream].get_or_insert(packet.arrival_us);
        if let Some(gap) = audio_gap(next_pts[stream], next_arrivals[stream], &packet) {
            add_silence(&mut model, stream, packet.sample_rate, gap.silence_samples)?;
            model_audio_us[stream] +=
                gap.silence_samples * 1_000_000 / u64::from(packet.sample_rate);
            if gap.skipped_us > 0 {
                let skipped = time_adjustments[stream]
                    .last()
                    .map_or(gap.skipped_us, |(_, skipped)| skipped + gap.skipped_us);
                time_adjustments[stream].push((model_audio_us[stream], skipped));
            }
        }
        normalize_microphone(packet.source, &mut packet.samples);
        model.add_audio_to(stream, &packet.samples, packet.sample_rate)?;
        let duration_us = packet.samples.len() as u64 * 1_000_000 / u64::from(packet.sample_rate);
        model_audio_us[stream] += duration_us;
        next_pts[stream] = packet
            .pts_us
            .map(|pts| pts.saturating_add(duration_us as i64));
        let packet_end = packet.arrival_us + duration_us;
        next_arrivals[stream] = Some(next_arrivals[stream].unwrap_or_default().max(packet_end));
        if Instant::now() >= next_updates[stream] {
            write_updates(
                &mut model,
                stream,
                packet.source,
                source_offset(source_origins, stream),
                &time_adjustments[stream],
                &mut writer,
                false,
            )?;
            next_updates[stream] = Instant::now() + UPDATE_INTERVAL;
        }
    }
    let mut finalization_error = None;
    for source in [MeetingSource::System, MeetingSource::Microphone] {
        let stream = source_index(source);
        if let Err(error) = write_updates(
            &mut model,
            stream,
            source,
            source_offset(source_origins, stream),
            &time_adjustments[stream],
            &mut writer,
            true,
        ) {
            tracing::warn!(?source, %error, "could not finalize live meeting source");
            finalization_error.get_or_insert(error);
        }
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

pub(super) fn read_snapshot(path: &Path) -> Result<Vec<TranscriptEntry>> {
    let events: Vec<Event> = read_ndjson(path)?;
    Ok(project(events))
}

#[derive(Debug, Eq, PartialEq)]
pub(super) struct AudioGap {
    pub(super) silence_samples: u64,
    pub(super) skipped_us: u64,
}

pub(super) fn audio_gap(
    expected_pts_us: Option<i64>,
    expected_arrival_us: Option<u64>,
    packet: &AudioPacket,
) -> Option<AudioGap> {
    let gap_us = match (expected_pts_us, packet.pts_us) {
        (Some(expected), Some(actual)) => actual.saturating_sub(expected).max(0) as u64,
        _ => packet.arrival_us.saturating_sub(expected_arrival_us?),
    };
    if gap_us < 20_000 {
        return None;
    }
    let silence_us = gap_us.min(MAX_GAP_US);
    Some(AudioGap {
        silence_samples: silence_us * u64::from(packet.sample_rate) / 1_000_000,
        skipped_us: gap_us - silence_us,
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

pub(super) fn project(events: impl IntoIterator<Item = Event>) -> Vec<TranscriptEntry> {
    let mut lines = HashMap::new();
    for event in events {
        lines.insert((event.source, event.line_id), event);
    }
    let mut transcript = lines
        .into_values()
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
    stream: usize,
    sample_rate: u32,
    mut samples: u64,
) -> Result<()> {
    let silence = [0.0; 1_600];
    while samples > 0 {
        let count = samples.min(silence.len() as u64) as usize;
        model.add_audio_to(stream, &silence[..count], sample_rate)?;
        samples -= count as u64;
    }
    Ok(())
}

fn write_updates(
    model: &mut Moonshine,
    stream: usize,
    source: MeetingSource,
    source_offset_us: u64,
    time_adjustments: &[(u64, u64)],
    writer: &mut BufWriter<File>,
    finish: bool,
) -> Result<()> {
    let updates = if finish {
        model.finish_stream(stream)?
    } else {
        model.update_stream(stream)?
    };
    for update in updates {
        let event = Event {
            source,
            line_id: update.line_id,
            phase: update.phase,
            start_ms: source_offset_us / 1_000
                + adjusted_time_ms(update.start_ms, time_adjustments),
            end_ms: source_offset_us / 1_000 + adjusted_time_ms(update.end_ms, time_adjustments),
            text: update.text,
        };
        serde_json::to_writer(&mut *writer, &event)?;
        writer.write_all(b"\n")?;
    }
    writer.flush()?;
    writer.get_ref().sync_data()?;
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
