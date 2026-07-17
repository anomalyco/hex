use std::fs::{self, File, OpenOptions};
use std::io::{BufWriter, Write};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::mpsc::{self, Receiver, SyncSender, TrySendError};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::{Duration, Instant};

use color_eyre::eyre::{Result, WrapErr, eyre};
use hound::{SampleFormat, WavReader, WavSpec, WavWriter};
use screencapturekit::prelude::*;
use screencapturekit::stream::delegate_trait::ErrorHandler;
use serde::{Deserialize, Serialize};

use crate::dictation::resample_for_parakeet;
use crate::events::now_ms;
use crate::parakeet::Parakeet;

const SAMPLE_RATE: u32 = 16_000;
const TRANSCRIPTION_CHUNK: usize = SAMPLE_RATE as usize * 30;
const TRANSCRIPT_TURN_GAP_MS: u64 = 3_000;

#[path = "meeting_live.rs"]
mod live;
pub use live::TranscriptReader;

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum MeetingRequest {
    Start,
    Stop,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct MeetingManifest {
    pub schema_version: u8,
    pub id: String,
    pub title: String,
    pub status: MeetingStatus,
    pub created_at_ms: u64,
    pub ended_at_ms: Option<u64>,
    pub duration_ms: Option<u64>,
    pub system: TrackManifest,
    pub microphone: TrackManifest,
    #[serde(default)]
    pub live_dropped_packets: u64,
    #[serde(default)]
    pub live_transcription_error: Option<String>,
    pub error: Option<String>,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum MeetingStatus {
    Starting,
    Recording,
    Transcribing,
    Complete,
    Interrupted,
    Failed,
}

impl MeetingStatus {
    pub fn is_active(self) -> bool {
        matches!(self, Self::Starting | Self::Recording | Self::Transcribing)
    }
}

pub fn active(meetings: &[MeetingManifest]) -> Option<&MeetingManifest> {
    meetings.iter().find(|meeting| meeting.status.is_active())
}

pub fn active_or_latest(meetings: &[MeetingManifest]) -> Option<&MeetingManifest> {
    active(meetings).or_else(|| meetings.first())
}

#[derive(Clone, Debug, Default, Deserialize, Serialize)]
pub struct TrackManifest {
    pub file: String,
    pub sample_rate: Option<u32>,
    pub first_pts_us: Option<i64>,
    pub last_pts_us: Option<i64>,
    pub first_arrival_us: Option<u64>,
    pub samples: u64,
    pub dropped_packets: u64,
}

#[derive(Clone, Debug, Deserialize, Eq, Hash, PartialEq, Serialize)]
pub struct TranscriptEntry {
    pub source: MeetingSource,
    pub start_ms: u64,
    pub end_ms: u64,
    pub text: String,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum TranscriptPublication {
    Live,
    Final,
}

pub struct CompletedTranscript {
    pub publication: TranscriptPublication,
    pub entries: Vec<TranscriptEntry>,
}

pub fn coalesce_transcript(
    entries: impl IntoIterator<Item = TranscriptEntry>,
) -> Vec<TranscriptEntry> {
    let mut turns: Vec<TranscriptEntry> = Vec::new();
    for entry in entries {
        if let Some(turn) = turns.last_mut()
            && turn.source == entry.source
            && entry.start_ms.saturating_sub(turn.end_ms) <= TRANSCRIPT_TURN_GAP_MS
        {
            if !turn.text.ends_with(char::is_whitespace) {
                turn.text.push(' ');
            }
            turn.text.push_str(entry.text.trim());
            turn.end_ms = turn.end_ms.max(entry.end_ms);
        } else {
            turns.push(entry);
        }
    }
    turns
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, Hash, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum MeetingSource {
    System,
    Microphone,
}

impl MeetingSource {
    pub fn label(self) -> &'static str {
        match self {
            Self::System => "Computer",
            Self::Microphone => "You",
        }
    }
}

struct AudioPacket {
    source: MeetingSource,
    pts_us: Option<i64>,
    arrival_us: u64,
    sample_rate: u32,
    samples: Vec<f32>,
}

#[derive(Default)]
struct CaptureStats {
    system: TrackManifest,
    microphone: TrackManifest,
}

struct ActiveMeeting {
    _file: File,
}

impl ActiveMeeting {
    fn acquire(directory: &Path) -> Result<Self> {
        let path = directory.join("active.lock");
        let file = OpenOptions::new()
            .create(true)
            .truncate(false)
            .read(true)
            .write(true)
            .open(&path)?;
        set_owner_only(&path, false)?;
        fs2::FileExt::lock_exclusive(&file)?;
        Ok(Self { _file: file })
    }
}

pub fn root() -> Result<PathBuf> {
    Ok(dirs::data_dir()
        .ok_or_else(|| eyre!("macOS application support directory is unavailable"))?
        .join("voice-control/meetings"))
}

pub fn record(
    title: Option<String>,
    shutdown: &AtomicBool,
    project_root: &Path,
    started_sender: Option<SyncSender<()>>,
) -> Result<MeetingManifest> {
    let _sleep_prevention = crate::recording_environment::prevent_sleep();
    let (_, transcription_selection) = crate::app_settings::transcription_selection();
    let transcription_model = crate::transcription_models::validate(&transcription_selection)?;
    if !crate::transcription_models::is_installed(transcription_model) {
        return Err(eyre!("{} is not installed", transcription_model.name));
    }
    let created_at_ms = now_ms();
    let id = format!("{created_at_ms}-{}", std::process::id());
    let meeting_dir = root()?.join(&id);
    fs::create_dir_all(&meeting_dir).wrap_err("could not create meeting directory")?;
    set_owner_only(&meeting_dir, true)?;
    let _active = ActiveMeeting::acquire(&meeting_dir)?;

    let mut manifest = MeetingManifest {
        schema_version: 1,
        id,
        title: title.unwrap_or_else(|| "Untitled meeting".into()),
        status: MeetingStatus::Starting,
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
        live_dropped_packets: 0,
        live_transcription_error: None,
        error: None,
    };
    write_manifest(&meeting_dir, &manifest)?;

    let result = (|| -> Result<MeetingManifest> {
        if shutdown.load(Ordering::Relaxed) {
            return Err(eyre!("meeting recording stopped before capture started"));
        }
        let content = SCShareableContent::get().wrap_err("could not enumerate screen content")?;
        let display = content
            .displays()
            .into_iter()
            .next()
            .ok_or_else(|| eyre!("no display is available for system audio capture"))?;
        let filter = SCContentFilter::create()
            .with_display(&display)
            .with_excluding_windows(&[])
            .build();
        let config = SCStreamConfiguration::new()
            .with_width(2)
            .with_height(2)
            .with_captures_audio(true)
            .with_captures_microphone(true)
            .with_excludes_current_process_audio(true)
            .with_sample_rate(SAMPLE_RATE as i32)
            .with_channel_count(1);
        let (sender, receiver) = mpsc::sync_channel::<AudioPacket>(128);
        let (live_sender, live_receiver) = mpsc::sync_channel(live::QUEUE_CAPACITY);
        let system_drops = Arc::new(AtomicU64::new(0));
        let microphone_drops = Arc::new(AtomicU64::new(0));
        let live_drops = Arc::new(AtomicU64::new(0));
        let manifest_live_drops = live_drops.clone();
        let live_worker_dir = meeting_dir.clone();
        let live_worker_root = project_root.to_path_buf();
        let live_worker_drops = live_drops.clone();
        let live_worker = thread::spawn(move || {
            let result = live::transcribe(
                &live_worker_root,
                &live_worker_dir,
                live_receiver,
                &live_worker_drops,
            );
            if let Err(error) = &result {
                live::persist_error(&live_worker_dir, error);
            }
            result
        });
        let writer_dir = meeting_dir.clone();
        let writer = thread::spawn(move || {
            write_tracks(&writer_dir, receiver, Some(live_sender), &live_drops)
        });
        let stream_error = Arc::new(Mutex::new(None::<String>));
        let delegate_error = stream_error.clone();
        let mut stream = SCStream::new_with_delegate(
            &filter,
            &config,
            ErrorHandler::new(move |error| {
                *delegate_error
                    .lock()
                    .unwrap_or_else(|error| error.into_inner()) = Some(error.to_string());
            }),
        );
        let capture_clock = Instant::now();
        let start_result = (|| -> Result<()> {
            add_audio_handler(
                &mut stream,
                sender.clone(),
                MeetingSource::System,
                system_drops.clone(),
                SCStreamOutputType::Audio,
                capture_clock,
            )?;
            add_audio_handler(
                &mut stream,
                sender.clone(),
                MeetingSource::Microphone,
                microphone_drops.clone(),
                SCStreamOutputType::Microphone,
                capture_clock,
            )?;
            if shutdown.load(Ordering::Relaxed) {
                return Err(eyre!("meeting recording stopped before capture started"));
            }
            stream
                .start_capture()
                .wrap_err("could not start meeting capture")
        })();
        if let Err(error) = start_result {
            drop(stream);
            drop(sender);
            if let Err(cleanup_error) = join_capture_workers(writer, live_worker) {
                tracing::error!(%cleanup_error, "could not finalize failed meeting setup");
            }
            return Err(error);
        }
        manifest.status = MeetingStatus::Recording;
        if let Err(error) = write_manifest(&meeting_dir, &manifest) {
            if let Err(stop_error) = stream.stop_capture() {
                tracing::error!(%stop_error, "could not stop meeting capture after manifest failure");
            }
            drop(stream);
            drop(sender);
            if let Err(cleanup_error) = join_capture_workers(writer, live_worker) {
                tracing::error!(%cleanup_error, "could not finalize failed meeting setup");
            }
            return Err(error);
        }
        if let Some(started_sender) = started_sender {
            let _ = started_sender.send(());
        }

        let started = Instant::now();
        println!("Recording '{}' (Ctrl-C to stop)", manifest.title);
        while !shutdown.load(Ordering::Relaxed)
            && stream_error
                .lock()
                .unwrap_or_else(|error| error.into_inner())
                .is_none()
            && !writer.is_finished()
        {
            thread::sleep(Duration::from_millis(100));
        }
        let stop_result = stream.stop_capture();
        drop(stream);
        drop(sender);

        manifest.ended_at_ms = Some(now_ms());
        manifest.duration_ms = Some(started.elapsed().as_millis() as u64);
        let capture_error = stream_error
            .lock()
            .unwrap_or_else(|error| error.into_inner())
            .take();
        let mut stats = match join_audio_writer(writer) {
            Ok(stats) => stats,
            Err(error) => {
                let _ = finish_live_worker(live_worker);
                return Err(error);
            }
        };
        stats.system.dropped_packets = system_drops.load(Ordering::Relaxed);
        stats.microphone.dropped_packets = microphone_drops.load(Ordering::Relaxed);
        manifest.system = stats.system;
        manifest.microphone = stats.microphone;
        manifest.live_dropped_packets = manifest_live_drops.load(Ordering::Relaxed);
        let transition_result = if capture_error.is_none() && stop_result.is_ok() {
            manifest.status = MeetingStatus::Transcribing;
            write_manifest(&meeting_dir, &manifest)
        } else {
            Ok(())
        };
        manifest.live_transcription_error = finish_live_worker(live_worker);
        transition_result?;
        if let Some(error) = capture_error {
            write_manifest(&meeting_dir, &manifest)?;
            return Err(eyre!("meeting capture stopped unexpectedly: {error}"));
        }
        if let Err(error) = stop_result {
            write_manifest(&meeting_dir, &manifest)?;
            return Err(error).wrap_err("could not stop meeting capture");
        }
        write_manifest(&meeting_dir, &manifest)?;

        match transcribe(&meeting_dir, &manifest, &transcription_selection) {
            Ok(()) => manifest.status = MeetingStatus::Complete,
            Err(error) => {
                manifest.status = MeetingStatus::Failed;
                manifest.error = Some(error.to_string());
                write_manifest(&meeting_dir, &manifest)?;
                return Err(error);
            }
        }
        write_manifest(&meeting_dir, &manifest)?;
        println!("Saved meeting {}", meeting_dir.display());
        Ok(manifest)
    })();
    if let Err(error) = &result {
        persist_meeting_failure(&meeting_dir, error);
    }
    result
}

fn join_capture_workers(
    writer: thread::JoinHandle<Result<CaptureStats>>,
    live_worker: thread::JoinHandle<Result<()>>,
) -> Result<CaptureStats> {
    let stats = join_audio_writer(writer);
    let _ = finish_live_worker(live_worker);
    stats
}

fn join_audio_writer(writer: thread::JoinHandle<Result<CaptureStats>>) -> Result<CaptureStats> {
    writer
        .join()
        .map_err(|_| eyre!("meeting audio writer panicked"))
        .and_then(|result| result)
}

fn finish_live_worker(live_worker: thread::JoinHandle<Result<()>>) -> Option<String> {
    match live_worker.join() {
        Ok(Ok(())) => None,
        Ok(Err(error)) => {
            tracing::warn!(%error, "live meeting transcription stopped");
            Some(error.to_string())
        }
        Err(_) => {
            tracing::warn!("live meeting transcription panicked");
            Some("live meeting transcription panicked".into())
        }
    }
}

fn persist_meeting_failure(directory: &Path, error: &color_eyre::Report) {
    let result = (|| -> Result<()> {
        let mut manifest = read_manifest(directory)?;
        if manifest.status != MeetingStatus::Complete {
            let ended_at_ms = now_ms();
            manifest.status = MeetingStatus::Failed;
            manifest.ended_at_ms.get_or_insert(ended_at_ms);
            manifest
                .duration_ms
                .get_or_insert(ended_at_ms.saturating_sub(manifest.created_at_ms));
            manifest.error = Some(error.to_string());
            write_manifest(directory, &manifest)?;
        }
        Ok(())
    })();
    if let Err(persist_error) = result {
        tracing::error!(%persist_error, %error, "could not persist meeting failure");
    }
}

fn add_audio_handler(
    stream: &mut SCStream,
    sender: SyncSender<AudioPacket>,
    source: MeetingSource,
    drops: Arc<AtomicU64>,
    output_type: SCStreamOutputType,
    capture_clock: Instant,
) -> Result<()> {
    let reported_format_error = AtomicBool::new(false);
    let registered = stream.add_output_handler(
        move |sample: CMSampleBuffer, _| {
            let pts = sample.presentation_timestamp();
            let pts_us = media_time_us(pts);
            let Some(format) = sample.format_description() else {
                drops.fetch_add(1, Ordering::Relaxed);
                return;
            };
            let Some(sample_rate) = format.audio_sample_rate().map(|rate| rate.round() as u32)
            else {
                drops.fetch_add(1, Ordering::Relaxed);
                return;
            };
            let Some(samples) = decode_audio(&sample) else {
                if !reported_format_error.swap(true, Ordering::Relaxed) {
                    let buffers = sample.audio_buffer_list();
                    tracing::error!(
                        ?source,
                        sample_rate = ?format.audio_sample_rate(),
                        channels = ?format.audio_channel_count(),
                        bits = ?format.audio_bits_per_channel(),
                        bytes_per_frame = ?format.audio_bytes_per_frame(),
                        flags = ?format.audio_format_flags(),
                        sample_count = sample.num_samples(),
                        buffers = buffers.as_ref().map(|buffers| buffers.num_buffers()),
                        buffer_bytes = ?buffers.as_ref().map(|buffers| buffers.iter().map(|buffer| buffer.data_byte_size()).collect::<Vec<_>>()),
                        buffer_channels = ?buffers.as_ref().map(|buffers| buffers.iter().map(|buffer| buffer.number_channels).collect::<Vec<_>>()),
                        "unsupported meeting audio format"
                    );
                }
                drops.fetch_add(1, Ordering::Relaxed);
                return;
            };
            let packet = AudioPacket {
                source,
                pts_us,
                arrival_us: capture_clock.elapsed().as_micros() as u64,
                sample_rate,
                samples,
            };
            if matches!(sender.try_send(packet), Err(TrySendError::Full(_))) {
                drops.fetch_add(1, Ordering::Relaxed);
            }
        },
        output_type,
    );
    if registered.is_some() {
        Ok(())
    } else {
        Err(eyre!("ScreenCaptureKit rejected {output_type} output"))
    }
}

fn media_time_us(time: CMTime) -> Option<i64> {
    if !time.is_valid()
        || time.is_indefinite()
        || time.is_positive_infinity()
        || time.is_negative_infinity()
    {
        return None;
    }
    let microseconds = time.as_seconds()? * 1_000_000.0;
    microseconds
        .is_finite()
        .then(|| microseconds.round() as i64)
}

fn decode_audio(sample: &CMSampleBuffer) -> Option<Vec<f32>> {
    let format = sample.format_description()?;
    if format.audio_is_big_endian() {
        return None;
    }
    let channels = format.audio_channel_count()? as usize;
    let bytes_per_frame = format.audio_bytes_per_frame()? as usize;
    let bytes_per_sample = bytes_per_frame.checked_div(channels)?;
    let bits_per_channel = format.audio_bits_per_channel()?;
    let flags = format.audio_format_flags()?;
    let frames = usize::try_from(sample.num_samples()).ok()?;
    let buffers = sample.audio_buffer_list()?;
    if buffers.num_buffers() == channels && channels > 1 {
        let channel_samples: Vec<Vec<f32>> = buffers
            .iter()
            .map(|buffer| {
                decode_samples(
                    buffer.data(),
                    frames,
                    bytes_per_sample,
                    bits_per_channel,
                    flags,
                )
            })
            .collect::<Option<_>>()?;
        return Some(
            (0..frames)
                .map(|frame| {
                    channel_samples
                        .iter()
                        .map(|samples| samples[frame])
                        .sum::<f32>()
                        / channels as f32
                })
                .collect(),
        );
    }
    let buffer = buffers.get(0)?;
    let available_channels = usize::try_from(buffer.number_channels).ok()?.max(1);
    let samples = decode_samples(
        buffer.data(),
        frames.checked_mul(available_channels)?,
        bytes_per_sample,
        bits_per_channel,
        flags,
    )?;
    Some(
        samples
            .chunks_exact(available_channels)
            .map(|frame| frame.iter().sum::<f32>() / available_channels as f32)
            .collect(),
    )
}

fn decode_samples(
    bytes: &[u8],
    samples: usize,
    bytes_per_sample: usize,
    bits_per_channel: u32,
    flags: u32,
) -> Option<Vec<f32>> {
    let required = samples.checked_mul(bytes_per_sample)?;
    if bytes.len() < required {
        return None;
    }
    let is_float = flags & 1 != 0;
    let is_signed_integer = flags & 4 != 0;
    bytes[..required]
        .chunks_exact(bytes_per_sample)
        .map(
            |sample| match (is_float, is_signed_integer, bytes_per_sample) {
                (true, _, 4) => Some(f32::from_le_bytes(sample.try_into().ok()?)),
                (_, true, 2) => {
                    Some(f32::from(i16::from_le_bytes(sample.try_into().ok()?)) / 32_768.0)
                }
                (_, true, 3) => {
                    let raw = i32::from_le_bytes([
                        sample[0],
                        sample[1],
                        sample[2],
                        if sample[2] & 0x80 == 0 { 0 } else { 0xff },
                    ]);
                    Some(raw as f32 / 8_388_608.0)
                }
                (_, true, 4) if bits_per_channel <= 32 => {
                    Some(i32::from_le_bytes(sample.try_into().ok()?) as f32 / 2_147_483_648.0)
                }
                _ => None,
            },
        )
        .collect()
}

fn write_tracks(
    directory: &Path,
    receiver: Receiver<AudioPacket>,
    mut live_sender: Option<SyncSender<AudioPacket>>,
    live_drops: &AtomicU64,
) -> Result<CaptureStats> {
    let mut system = None;
    let mut microphone = None;
    let mut stats = CaptureStats::default();
    stats.system.file = "system.wav".into();
    stats.microphone.file = "microphone.wav".into();
    while let Ok(packet) = receiver.recv() {
        let (writer, track, file_name) = match packet.source {
            MeetingSource::System => (&mut system, &mut stats.system, "system.wav"),
            MeetingSource::Microphone => (&mut microphone, &mut stats.microphone, "microphone.wav"),
        };
        if let Some(rate) = track.sample_rate
            && rate != packet.sample_rate
        {
            return Err(eyre!(
                "{file_name} sample rate changed from {rate} to {}",
                packet.sample_rate
            ));
        }
        track.sample_rate = Some(packet.sample_rate);
        if writer.is_none() {
            let path = directory.join(file_name);
            let created = WavWriter::create(
                &path,
                WavSpec {
                    channels: 1,
                    sample_rate: packet.sample_rate,
                    bits_per_sample: 32,
                    sample_format: SampleFormat::Float,
                },
            )?;
            set_owner_only(&path, false)?;
            *writer = Some(created);
        }
        let writer = writer
            .as_mut()
            .ok_or_else(|| eyre!("could not initialize {file_name}"))?;
        if let Some(pts_us) = packet.pts_us {
            track.first_pts_us.get_or_insert(pts_us);
            track.last_pts_us = Some(pts_us);
        }
        track.first_arrival_us.get_or_insert(packet.arrival_us);
        track.samples += packet.samples.len() as u64;
        for &sample in &packet.samples {
            writer.write_sample(sample)?;
        }
        if let Some(sender) = &live_sender {
            match sender.try_send(packet) {
                Ok(()) => {}
                Err(TrySendError::Full(_)) => {
                    live_drops.fetch_add(1, Ordering::Relaxed);
                }
                Err(TrySendError::Disconnected(_)) => live_sender = None,
            }
        }
    }
    if let Some(writer) = system {
        writer.finalize()?;
        File::open(directory.join("system.wav"))?.sync_all()?;
    } else {
        create_empty_track(directory.join("system.wav"))?;
    }
    if let Some(writer) = microphone {
        writer.finalize()?;
        File::open(directory.join("microphone.wav"))?.sync_all()?;
    } else {
        create_empty_track(directory.join("microphone.wav"))?;
    }
    set_owner_only(&directory.join("system.wav"), false)?;
    set_owner_only(&directory.join("microphone.wav"), false)?;
    Ok(stats)
}

fn create_empty_track(path: PathBuf) -> Result<()> {
    WavWriter::create(
        &path,
        WavSpec {
            channels: 1,
            sample_rate: SAMPLE_RATE,
            bits_per_sample: 32,
            sample_format: SampleFormat::Float,
        },
    )?
    .finalize()?;
    set_owner_only(&path, false)?;
    File::open(path)?.sync_all()?;
    Ok(())
}

fn transcribe(
    directory: &Path,
    manifest: &MeetingManifest,
    selection: &crate::transcription_models::TranscriptionSelection,
) -> Result<()> {
    let origin = [
        manifest.system.first_arrival_us,
        manifest.microphone.first_arrival_us,
    ]
    .into_iter()
    .flatten()
    .min()
    .unwrap_or(0);
    let mut model = Parakeet::load_selection(selection)?;
    let mut transcript = Vec::new();
    transcribe_track(
        &mut model,
        directory.join(&manifest.system.file),
        MeetingSource::System,
        manifest.system.first_arrival_us.unwrap_or(origin) - origin,
        &mut transcript,
    )?;
    transcribe_track(
        &mut model,
        directory.join(&manifest.microphone.file),
        MeetingSource::Microphone,
        manifest.microphone.first_arrival_us.unwrap_or(origin) - origin,
        &mut transcript,
    )?;
    transcript.sort_by_key(|entry| entry.start_ms);

    let ndjson_temporary = directory.join("transcript.ndjson.tmp");
    let markdown_temporary = directory.join("transcript.md.tmp");
    let mut ndjson = BufWriter::new(File::create(&ndjson_temporary)?);
    let mut markdown = BufWriter::new(File::create(&markdown_temporary)?);
    set_owner_only(&ndjson_temporary, false)?;
    set_owner_only(&markdown_temporary, false)?;
    writeln!(markdown, "# {}\n", manifest.title)?;
    for entry in &transcript {
        serde_json::to_writer(&mut ndjson, entry)?;
        ndjson.write_all(b"\n")?;
        writeln!(
            markdown,
            "**{} {}**  {}",
            format_duration(entry.start_ms),
            entry.source.label(),
            entry.text
        )?;
    }
    ndjson.flush()?;
    markdown.flush()?;
    ndjson.get_ref().sync_all()?;
    markdown.get_ref().sync_all()?;
    sync_directory(directory)?;
    fs::rename(&ndjson_temporary, directory.join("transcript.ndjson"))?;
    fs::rename(&markdown_temporary, directory.join("transcript.md"))?;
    sync_directory(directory)?;
    Ok(())
}

fn transcribe_track(
    model: &mut Parakeet,
    path: PathBuf,
    source: MeetingSource,
    source_offset_us: u64,
    transcript: &mut Vec<TranscriptEntry>,
) -> Result<()> {
    let mut reader =
        WavReader::open(&path).wrap_err_with(|| format!("could not read {}", path.display()))?;
    let source_rate = reader.spec().sample_rate;
    let source_samples: Vec<f32> = reader.samples::<f32>().collect::<Result<_, _>>()?;
    let samples = resample_for_parakeet(&source_samples, source_rate);
    let source_offset_ms = source_offset_us / 1_000;
    for (chunk_index, chunk) in samples.chunks(TRANSCRIPTION_CHUNK).enumerate() {
        if chunk.is_empty() {
            continue;
        }
        let chunk_start_ms = chunk_index as u64 * 30_000 + source_offset_ms;
        let result = model.transcribe_segments(chunk)?;
        if !result.segments.is_empty() {
            transcript.extend(result.segments.into_iter().filter_map(|segment| {
                let text = segment.text.trim().to_string();
                (!text.is_empty()).then(|| TranscriptEntry {
                    source,
                    start_ms: chunk_start_ms + segment.t0_ms.max(0) as u64,
                    end_ms: chunk_start_ms + segment.t1_ms.max(0) as u64,
                    text,
                })
            }));
        } else if !result.text.trim().is_empty() {
            transcript.push(TranscriptEntry {
                source,
                start_ms: chunk_start_ms,
                end_ms: chunk_start_ms + chunk.len() as u64 * 1_000 / u64::from(SAMPLE_RATE),
                text: result.text.trim().to_string(),
            });
        }
    }
    Ok(())
}

pub fn list() -> Result<Vec<MeetingManifest>> {
    let root = root()?;
    let mut meetings = Vec::new();
    for entry in match fs::read_dir(root) {
        Ok(entries) => entries,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(meetings),
        Err(error) => return Err(error.into()),
    } {
        let directory = entry?.path();
        let path = directory.join("manifest.json");
        if let Ok(data) = fs::read(&path)
            && let Ok(mut manifest) = serde_json::from_slice::<MeetingManifest>(&data)
        {
            manifest.live_transcription_error = fs::read_to_string(
                path.parent()
                    .unwrap_or_else(|| Path::new("."))
                    .join("transcript.live.error.txt"),
            )
            .ok()
            .map(|error| error.trim().to_string());
            let process_is_active = meeting_is_active(&directory);
            if process_is_active == Some(false) {
                if let Err(error) = recover_final_publication(&directory) {
                    tracing::warn!(%error, meeting = manifest.id, "could not recover final transcript publication");
                }
                let recovered =
                    recovered_status(manifest.status, false, final_transcript_exists(&directory));
                if recovered != manifest.status {
                    manifest.status = recovered;
                    match recovered {
                        MeetingStatus::Complete => manifest.error = None,
                        MeetingStatus::Interrupted => {
                            let ended_at_ms = now_ms();
                            manifest.ended_at_ms.get_or_insert(ended_at_ms);
                            manifest
                                .duration_ms
                                .get_or_insert(ended_at_ms.saturating_sub(manifest.created_at_ms));
                        }
                        MeetingStatus::Starting
                        | MeetingStatus::Recording
                        | MeetingStatus::Transcribing
                        | MeetingStatus::Failed => {}
                    }
                    if let Err(error) = write_manifest(&directory, &manifest) {
                        tracing::warn!(%error, meeting = manifest.id, "could not persist recovered meeting status");
                    }
                }
            }
            meetings.push(manifest);
        }
    }
    meetings.sort_by_key(|meeting| std::cmp::Reverse(meeting.created_at_ms));
    Ok(meetings)
}

fn meeting_is_active(directory: &Path) -> Option<bool> {
    let file = match OpenOptions::new()
        .read(true)
        .write(true)
        .open(directory.join("active.lock"))
    {
        Ok(file) => file,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Some(false),
        Err(error) => {
            tracing::warn!(%error, directory = %directory.display(), "could not open meeting lock");
            return None;
        }
    };
    match fs2::FileExt::try_lock_exclusive(&file) {
        Ok(()) => {
            let _ = fs2::FileExt::unlock(&file);
            Some(false)
        }
        Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => Some(true),
        Err(error) => {
            tracing::warn!(%error, directory = %directory.display(), "could not inspect meeting lock");
            None
        }
    }
}

fn recovered_status(
    status: MeetingStatus,
    process_is_active: bool,
    final_transcript_exists: bool,
) -> MeetingStatus {
    match (status, process_is_active, final_transcript_exists) {
        (
            MeetingStatus::Transcribing | MeetingStatus::Interrupted | MeetingStatus::Failed,
            false,
            true,
        ) => MeetingStatus::Complete,
        (MeetingStatus::Complete, false, false) => MeetingStatus::Interrupted,
        (
            MeetingStatus::Starting | MeetingStatus::Recording | MeetingStatus::Transcribing,
            false,
            _,
        ) => MeetingStatus::Interrupted,
        _ => status,
    }
}

#[cfg_attr(not(debug_assertions), allow(dead_code))]
pub fn show(id: &str) -> Result<String> {
    let directory = root()?.join(id);
    let path = directory.join("transcript.md");
    recover_final_publication(&directory)?;
    if final_transcript_exists(&directory) {
        return fs::read_to_string(&path)
            .wrap_err_with(|| format!("could not read {}", path.display()));
    }
    let manifest = read_manifest(&directory)?;
    let mut markdown = format!("# {}\n\n", manifest.title);
    for entry in transcript_entries_in(&directory)? {
        markdown.push_str(&format!(
            "**{} {}**  {}\n",
            format_duration(entry.start_ms),
            entry.source.label(),
            entry.text
        ));
    }
    Ok(markdown)
}

#[cfg_attr(not(debug_assertions), allow(dead_code))]
fn transcript_entries_in(directory: &Path) -> Result<Vec<TranscriptEntry>> {
    recover_final_publication(directory)?;
    let final_path = directory.join("transcript.ndjson");
    if final_transcript_exists(directory) {
        return read_ndjson(&final_path);
    }
    let live_path = directory.join("transcript.live.ndjson");
    if !live_path.exists() {
        return Ok(Vec::new());
    }
    live::read_snapshot(&live_path)
}

pub fn completed_transcript(
    id: &str,
    publication: Option<TranscriptPublication>,
) -> Result<CompletedTranscript> {
    let directory = root()?.join(id);
    recover_final_publication(&directory)?;
    let final_path = directory.join("transcript.ndjson");
    let live_path = directory.join("transcript.live.ndjson");
    let publication = publication.unwrap_or_else(|| {
        if final_transcript_exists(&directory) {
            TranscriptPublication::Final
        } else {
            TranscriptPublication::Live
        }
    });
    let entries = match publication {
        TranscriptPublication::Final => read_ndjson(&final_path)?,
        TranscriptPublication::Live if live_path.exists() => {
            let mut entries = live::read_completed_snapshot(&live_path)?;
            if final_transcript_exists(&directory) {
                supplement_live_transcript(&mut entries, read_ndjson(&final_path)?);
            }
            entries
        }
        TranscriptPublication::Live => Vec::new(),
    };
    Ok(CompletedTranscript {
        publication,
        entries,
    })
}

fn supplement_live_transcript(
    live_entries: &mut Vec<TranscriptEntry>,
    final_entries: Vec<TranscriptEntry>,
) {
    let supplements = final_entries
        .into_iter()
        .filter(|final_entry| {
            !live_entries.iter().any(|live_entry| {
                live_entry.source == final_entry.source
                    && live_entry.start_ms < final_entry.end_ms
                    && final_entry.start_ms < live_entry.end_ms
            })
        })
        .collect::<Vec<_>>();
    live_entries.extend(supplements);
    live_entries.sort_by_key(|entry| entry.start_ms);
}

fn read_ndjson<T: for<'de> Deserialize<'de>>(path: &Path) -> Result<Vec<T>> {
    let contents = fs::read(path).wrap_err_with(|| format!("could not read {}", path.display()))?;
    let complete = contents
        .iter()
        .rposition(|byte| *byte == b'\n')
        .map_or(&[][..], |newline| &contents[..newline]);
    complete
        .split(|byte| *byte == b'\n')
        .filter(|line| !line.is_empty())
        .map(|line| serde_json::from_slice(line).map_err(Into::into))
        .collect()
}

fn final_transcript_exists(directory: &Path) -> bool {
    directory.join("transcript.ndjson").exists() && directory.join("transcript.md").exists()
}

fn recover_final_publication(directory: &Path) -> Result<()> {
    if meeting_is_active(directory) != Some(false) {
        return Ok(());
    }
    let ndjson = directory.join("transcript.ndjson");
    let markdown = directory.join("transcript.md");
    let markdown_temporary = directory.join("transcript.md.tmp");
    if ndjson.exists() && !markdown.exists() && markdown_temporary.exists() {
        fs::rename(markdown_temporary, markdown)?;
        sync_directory(directory)?;
    }
    Ok(())
}

fn read_manifest(directory: &Path) -> Result<MeetingManifest> {
    let path = directory.join("manifest.json");
    let data = fs::read(&path).wrap_err_with(|| format!("could not read {}", path.display()))?;
    serde_json::from_slice(&data).wrap_err("could not decode meeting manifest")
}

fn write_manifest(directory: &Path, manifest: &MeetingManifest) -> Result<()> {
    let temporary = directory.join("manifest.json.tmp");
    let final_path = directory.join("manifest.json");
    let mut file = BufWriter::new(File::create(&temporary)?);
    set_owner_only(&temporary, false)?;
    serde_json::to_writer_pretty(&mut file, manifest)?;
    file.write_all(b"\n")?;
    file.flush()?;
    file.get_ref().sync_all()?;
    fs::rename(&temporary, &final_path)?;
    sync_directory(directory)
}

fn sync_directory(directory: &Path) -> Result<()> {
    File::open(directory)?.sync_all()?;
    Ok(())
}

pub fn format_duration(milliseconds: u64) -> String {
    let total_seconds = milliseconds / 1_000;
    format!("{:02}:{:02}", total_seconds / 60, total_seconds % 60)
}

#[cfg(unix)]
fn set_owner_only(path: &Path, directory: bool) -> Result<()> {
    use std::os::unix::fs::PermissionsExt;
    fs::set_permissions(
        path,
        fs::Permissions::from_mode(if directory { 0o700 } else { 0o600 }),
    )?;
    Ok(())
}

#[cfg(not(unix))]
fn set_owner_only(_path: &Path, _directory: bool) -> Result<()> {
    Ok(())
}

#[cfg(test)]
#[path = "meeting_tests.rs"]
mod tests;
