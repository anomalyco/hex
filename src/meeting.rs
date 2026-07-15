use std::collections::HashSet;
use std::fs::{self, File};
use std::io::{BufWriter, Write};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::mpsc::{self, SyncSender, TrySendError};
use std::sync::{Arc, Mutex, OnceLock};
use std::thread;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use color_eyre::eyre::{Result, WrapErr, eyre};
use hound::{SampleFormat, WavReader, WavSpec, WavWriter};
use screencapturekit::prelude::*;
use serde::{Deserialize, Serialize};

use crate::dictation::resample_for_parakeet;
use crate::parakeet::Parakeet;

const SAMPLE_RATE: u32 = 16_000;
const TRANSCRIPTION_CHUNK: usize = SAMPLE_RATE as usize * 30;
static ACTIVE_MEETINGS: OnceLock<Mutex<HashSet<String>>> = OnceLock::new();

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
    pub error: Option<String>,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum MeetingStatus {
    Recording,
    Transcribing,
    Complete,
    Interrupted,
    Failed,
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

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct TranscriptEntry {
    pub source: MeetingSource,
    pub start_ms: u64,
    pub end_ms: u64,
    pub text: String,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
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

struct ActiveMeeting(String);

impl ActiveMeeting {
    fn new(id: String) -> Self {
        active_meetings().lock().unwrap().insert(id.clone());
        Self(id)
    }
}

impl Drop for ActiveMeeting {
    fn drop(&mut self) {
        active_meetings().lock().unwrap().remove(&self.0);
    }
}

fn active_meetings() -> &'static Mutex<HashSet<String>> {
    ACTIVE_MEETINGS.get_or_init(|| Mutex::new(HashSet::new()))
}

pub fn root() -> Result<PathBuf> {
    Ok(dirs::data_dir()
        .ok_or_else(|| eyre!("macOS application support directory is unavailable"))?
        .join("voice-control/meetings"))
}

pub fn record(title: Option<String>, shutdown: &AtomicBool) -> Result<MeetingManifest> {
    shutdown.store(false, Ordering::Relaxed);
    let created_at_ms = now_ms();
    let id = format!("{created_at_ms}-{}", std::process::id());
    let _active = ActiveMeeting::new(id.clone());
    let meeting_dir = root()?.join(&id);
    fs::create_dir_all(&meeting_dir).wrap_err("could not create meeting directory")?;
    set_owner_only(&meeting_dir, true)?;

    let mut manifest = MeetingManifest {
        schema_version: 1,
        id,
        title: title.unwrap_or_else(|| "Untitled meeting".into()),
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
        error: None,
    };
    write_manifest(&meeting_dir, &manifest)?;

    let (sender, receiver) = mpsc::sync_channel::<AudioPacket>(128);
    let system_drops = Arc::new(AtomicU64::new(0));
    let microphone_drops = Arc::new(AtomicU64::new(0));
    let writer_dir = meeting_dir.clone();
    let writer = thread::spawn(move || write_tracks(&writer_dir, receiver));

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
    let mut stream = SCStream::new(&filter, &config);
    let capture_clock = Instant::now();
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

    let started = Instant::now();
    stream
        .start_capture()
        .wrap_err("could not start meeting capture")?;
    println!("Recording '{}' (Ctrl-C to stop)", manifest.title);
    while !shutdown.load(Ordering::Relaxed) {
        thread::sleep(Duration::from_millis(100));
    }
    stream
        .stop_capture()
        .wrap_err("could not stop meeting capture")?;
    drop(stream);
    drop(sender);

    let mut stats = writer
        .join()
        .map_err(|_| eyre!("meeting audio writer panicked"))??;
    stats.system.dropped_packets = system_drops.load(Ordering::Relaxed);
    stats.microphone.dropped_packets = microphone_drops.load(Ordering::Relaxed);
    manifest.system = stats.system;
    manifest.microphone = stats.microphone;
    manifest.ended_at_ms = Some(now_ms());
    manifest.duration_ms = Some(started.elapsed().as_millis() as u64);
    manifest.status = MeetingStatus::Transcribing;
    write_manifest(&meeting_dir, &manifest)?;

    match transcribe(&meeting_dir, &manifest) {
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

fn write_tracks(directory: &Path, receiver: mpsc::Receiver<AudioPacket>) -> Result<CaptureStats> {
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
            *writer = Some(WavWriter::create(
                directory.join(file_name),
                WavSpec {
                    channels: 1,
                    sample_rate: packet.sample_rate,
                    bits_per_sample: 32,
                    sample_format: SampleFormat::Float,
                },
            )?);
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
        for sample in packet.samples {
            writer.write_sample(sample)?;
        }
    }
    if let Some(writer) = system {
        writer.finalize()?;
    } else {
        create_empty_track(directory.join("system.wav"))?;
    }
    if let Some(writer) = microphone {
        writer.finalize()?;
    } else {
        create_empty_track(directory.join("microphone.wav"))?;
    }
    set_owner_only(&directory.join("system.wav"), false)?;
    set_owner_only(&directory.join("microphone.wav"), false)?;
    Ok(stats)
}

fn create_empty_track(path: PathBuf) -> Result<()> {
    WavWriter::create(
        path,
        WavSpec {
            channels: 1,
            sample_rate: SAMPLE_RATE,
            bits_per_sample: 32,
            sample_format: SampleFormat::Float,
        },
    )?
    .finalize()?;
    Ok(())
}

fn transcribe(directory: &Path, manifest: &MeetingManifest) -> Result<()> {
    let origin = [
        manifest.system.first_arrival_us,
        manifest.microphone.first_arrival_us,
    ]
    .into_iter()
    .flatten()
    .min()
    .unwrap_or(0);
    let mut model = Parakeet::load()?;
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

    let mut ndjson = BufWriter::new(File::create(directory.join("transcript.ndjson"))?);
    let mut markdown = BufWriter::new(File::create(directory.join("transcript.md"))?);
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
    set_owner_only(&directory.join("transcript.ndjson"), false)?;
    set_owner_only(&directory.join("transcript.md"), false)?;
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
        if let Some(segments) = result.segments {
            transcript.extend(segments.into_iter().filter_map(|segment| {
                let text = segment.text.trim().to_string();
                (!text.is_empty()).then(|| TranscriptEntry {
                    source,
                    start_ms: chunk_start_ms + (segment.start.max(0.0) * 1_000.0) as u64,
                    end_ms: chunk_start_ms + (segment.end.max(0.0) * 1_000.0) as u64,
                    text,
                })
            }));
        } else if !result.text.trim().is_empty() {
            transcript.push(TranscriptEntry {
                source,
                start_ms: chunk_start_ms,
                end_ms: chunk_start_ms + chunk.len() as u64 * 1_000 / u64::from(SAMPLE_RATE),
                text: result.text.trim().into(),
            });
        }
    }
    Ok(())
}

pub fn list() -> Result<Vec<MeetingManifest>> {
    let root = root()?;
    let active = active_meetings().lock().unwrap().clone();
    let mut meetings = Vec::new();
    for entry in match fs::read_dir(root) {
        Ok(entries) => entries,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(meetings),
        Err(error) => return Err(error.into()),
    } {
        let path = entry?.path().join("manifest.json");
        if let Ok(data) = fs::read(&path)
            && let Ok(mut manifest) = serde_json::from_slice::<MeetingManifest>(&data)
        {
            if manifest.status == MeetingStatus::Recording && !active.contains(&manifest.id) {
                manifest.status = MeetingStatus::Interrupted;
            }
            meetings.push(manifest);
        }
    }
    meetings.sort_by_key(|meeting| std::cmp::Reverse(meeting.created_at_ms));
    Ok(meetings)
}

pub fn show(id: &str) -> Result<String> {
    let path = root()?.join(id).join("transcript.md");
    fs::read_to_string(&path).wrap_err_with(|| format!("could not read {}", path.display()))
}

fn write_manifest(directory: &Path, manifest: &MeetingManifest) -> Result<()> {
    let temporary = directory.join("manifest.json.tmp");
    let final_path = directory.join("manifest.json");
    let mut file = BufWriter::new(File::create(&temporary)?);
    serde_json::to_writer_pretty(&mut file, manifest)?;
    file.write_all(b"\n")?;
    file.flush()?;
    fs::rename(&temporary, &final_path)?;
    set_owner_only(&final_path, false)
}

fn format_duration(milliseconds: u64) -> String {
    let total_seconds = milliseconds / 1_000;
    format!("{:02}:{:02}", total_seconds / 60, total_seconds % 60)
}

fn now_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis() as u64
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
mod tests {
    use super::*;

    #[test]
    fn decodes_float_and_aligned_24_bit_integer_samples() {
        let float = [0.25_f32.to_le_bytes(), (-0.5_f32).to_le_bytes()].concat();
        assert_eq!(decode_samples(&float, 2, 4, 32, 1).unwrap(), [0.25, -0.5]);

        let integer = [
            0x4000_0000_i32.to_le_bytes(),
            (-0x4000_0000_i32).to_le_bytes(),
        ]
        .concat();
        assert_eq!(decode_samples(&integer, 2, 4, 24, 20).unwrap(), [0.5, -0.5]);
    }

    #[test]
    fn rejects_non_numeric_media_times() {
        assert_eq!(media_time_us(CMTime::new(3, 2)), Some(1_500_000));
        assert_eq!(media_time_us(CMTime::INVALID), None);
        assert_eq!(media_time_us(CMTime::positive_infinity()), None);
    }
}
