use std::fs::{self, File, OpenOptions};
use std::io::{BufWriter, Write};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::mpsc::{self, Receiver, SyncSender, TrySendError};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::{Duration, Instant};

use color_eyre::eyre::{Result, WrapErr, eyre};
use hound::{SampleFormat, WavSpec, WavWriter};
use screencapturekit::prelude::*;
use screencapturekit::stream::delegate_trait::ErrorHandler;
use serde::{Deserialize, Serialize};

use crate::events::now_ms;

const SAMPLE_RATE: u32 = 16_000;
const TRANSCRIPT_TURN_GAP_MS: u64 = 3_000;

#[path = "meeting_echo.rs"]
mod echo;
#[path = "meeting_live.rs"]
mod live;
pub use live::TranscriptReader;
#[path = "meeting_notes.rs"]
mod notes;
#[path = "meeting_query.rs"]
pub mod query;
#[path = "meeting_transcript.rs"]
mod transcript;
pub use notes::{MeetingNotes, display_title, notes_in_progress, notes_wanted, spawn_summary};
#[cfg(debug_assertions)]
pub use notes::{summarize, summarize_file};

/// What the recorder knows about a meeting when it starts.
#[derive(Clone, Debug, Default)]
pub struct MeetingStart {
    /// An explicit title; otherwise one is derived from the app and time.
    pub title: Option<String>,
    /// The detected call application, such as `Zoom`.
    pub app: Option<String>,
}

/// What the recorder reports to its owner while it runs.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum RecordingUpdate {
    /// Capture started; the title is the one the meeting shows.
    Started { title: String },
    /// A provisional title from the transcript so far.
    Retitled { title: String },
}

pub fn default_title(app: Option<&str>, created_at_ms: u64) -> String {
    let clock = transcript::LocalTime::from_ms(created_at_ms).clock();
    match app {
        Some(app) => format!("{app} call — {clock}"),
        None => format!("Meeting — {clock}"),
    }
}

unsafe extern "C" {
    fn CGPreflightScreenCaptureAccess() -> bool;
    fn CGRequestScreenCaptureAccess() -> bool;
}

/// Whether macOS currently allows Screen & System Audio Recording, which
/// meeting capture needs for remote audio.
pub fn screen_recording_permitted() -> bool {
    // SAFETY: A CoreGraphics query without arguments or side effects.
    unsafe { CGPreflightScreenCaptureAccess() }
}

/// Asks macOS to list HEX under Screen & System Audio Recording. The system
/// prompts at most once; later requests return the current state.
pub fn request_screen_recording() -> bool {
    // SAFETY: A CoreGraphics request without arguments.
    unsafe { CGRequestScreenCaptureAccess() }
}

pub fn open_screen_recording_settings() {
    if let Err(error) = std::process::Command::new("/usr/bin/open")
        .arg("x-apple.systempreferences:com.apple.preference.security?Privacy_ScreenCapture")
        .spawn()
    {
        tracing::error!(%error, "could not open Screen Recording settings");
    }
}

pub fn reveal(path: &Path) {
    if let Err(error) = std::process::Command::new("/usr/bin/open")
        .arg("-R")
        .arg(path)
        .spawn()
    {
        tracing::error!(%error, "could not reveal meeting file");
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum MeetingRequest {
    Start,
    Stop,
    /// Regenerate Apple Intelligence notes for a completed meeting.
    Summarize(String),
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
    /// The detected call application, such as `Zoom`.
    #[serde(default)]
    pub app: Option<String>,
    /// The published Markdown copy in the user's meeting folder.
    #[serde(default)]
    pub export_path: Option<PathBuf>,
    #[serde(default)]
    pub export_error: Option<String>,
    #[serde(default)]
    pub echo_suppressed_utterances: u64,
    /// A title generated from the transcript while recording; shown only
    /// while `title` is the default and no notes title exists.
    #[serde(default)]
    pub provisional_title: Option<String>,
    /// Apple Intelligence notes generated after final publication.
    #[serde(default)]
    pub notes: Option<MeetingNotes>,
    #[serde(default)]
    pub notes_error: Option<String>,
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
            Self::System => "Them",
            Self::Microphone => "You",
        }
    }
}

struct AudioPacket {
    source: MeetingSource,
    pts_us: Option<i64>,
    arrival_us: u64,
    /// Samples of this source recorded before the packet: its place in the
    /// WAV, set by the writer. Consumers that drop packets use it to keep
    /// later audio on the recording's timeline.
    position: u64,
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
    Ok(crate::app_paths::support_dir()?.join("meetings"))
}

pub fn record(
    start: MeetingStart,
    shutdown: &AtomicBool,
    project_root: &Path,
    updates: Option<SyncSender<RecordingUpdate>>,
) -> Result<MeetingManifest> {
    let (_, transcription_selection) = crate::app_settings::transcription_selection();
    let transcription_model = crate::transcription_models::validate(&transcription_selection)?;
    if !crate::transcription_models::is_installed(
        transcription_model,
        &transcription_selection.language,
    ) {
        return Err(eyre!("{} is not installed", transcription_model.name));
    }
    if !screen_recording_permitted() {
        request_screen_recording();
        return Err(eyre!(
            "Allow HEX in System Settings › Privacy & Security › Screen & System Audio Recording, then try again"
        ));
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
        title: start
            .title
            .filter(|title| !title.trim().is_empty())
            .unwrap_or_else(|| default_title(start.app.as_deref(), created_at_ms)),
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
        app: start.app,
        export_path: None,
        export_error: None,
        echo_suppressed_utterances: 0,
        provisional_title: None,
        notes: None,
        notes_error: None,
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
            .build()
            .wrap_err("could not build meeting capture filter")?;
        let config = SCStreamConfiguration::new()
            .with_width(2)
            .with_height(2)
            .with_captures_audio(true)
            .with_captures_microphone(true)
            .wrap_err("could not enable meeting microphone capture")?
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
        let mut rolling =
            transcript::Rolling::start(meeting_dir.clone(), transcription_selection.clone());
        let rolling_feed = rolling.feed();
        let writer_dir = meeting_dir.clone();
        let writer = thread::spawn(move || {
            write_tracks(
                &writer_dir,
                receiver,
                Some(live_sender),
                rolling_feed,
                &live_drops,
            )
        });
        let stream_error = Arc::new(Mutex::new(None::<String>));
        let delegate_error = stream_error.clone();
        let cleanup_failed_setup =
            |stream: Option<SCStream>,
             sender: SyncSender<AudioPacket>,
             writer: thread::JoinHandle<Result<CaptureStats>>,
             live_worker: thread::JoinHandle<Result<()>>| {
                drop(stream);
                drop(sender);
                if let Err(cleanup_error) = join_capture_workers(writer, live_worker) {
                    tracing::error!(%cleanup_error, "could not finalize failed meeting setup");
                }
            };
        let mut stream = match SCStream::new_with_delegate(
            &filter,
            &config,
            ErrorHandler::new(move |error| {
                *delegate_error
                    .lock()
                    .unwrap_or_else(|error| error.into_inner()) = Some(error.to_string());
            }),
        ) {
            Ok(stream) => stream,
            Err(error) => {
                cleanup_failed_setup(None, sender, writer, live_worker);
                return Err(eyre!(error).wrap_err("could not create meeting capture stream"));
            }
        };
        let capture_clock = Instant::now();
        let sleep_prevention = crate::recording_environment::prevent_sleep();
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
            cleanup_failed_setup(Some(stream), sender, writer, live_worker);
            return Err(error);
        }
        manifest.status = MeetingStatus::Recording;
        if let Err(error) = write_manifest(&meeting_dir, &manifest) {
            if let Err(stop_error) = stream.stop_capture() {
                tracing::error!(%stop_error, "could not stop meeting capture after manifest failure");
            }
            cleanup_failed_setup(Some(stream), sender, writer, live_worker);
            return Err(error);
        }
        if let Some(updates) = &updates {
            let _ = updates.send(RecordingUpdate::Started {
                title: display_title(&manifest),
            });
        }

        let started = Instant::now();
        let titles = notes::ProvisionalTitles::start(manifest.id.clone());
        println!("Recording '{}' (Ctrl-C to stop)", manifest.title);
        while !shutdown.load(Ordering::Relaxed)
            && stream_error
                .lock()
                .unwrap_or_else(|error| error.into_inner())
                .is_none()
            && !writer.is_finished()
        {
            if let Some(title) = titles.take() {
                record_provisional_title(&meeting_dir, &mut manifest, title, updates.as_ref());
            }
            thread::sleep(Duration::from_millis(100));
        }
        if let Some(title) = titles.take() {
            record_provisional_title(&meeting_dir, &mut manifest, title, updates.as_ref());
        }
        drop(titles);
        let stop_result = stream.stop_capture();
        drop(stream);
        drop(sender);
        drop(sleep_prevention);

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

        let rolling = rolling.finish();
        match transcribe(
            &meeting_dir,
            &mut manifest,
            &transcription_selection,
            rolling,
        ) {
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

fn record_provisional_title(
    directory: &Path,
    manifest: &mut MeetingManifest,
    title: String,
    updates: Option<&SyncSender<RecordingUpdate>>,
) {
    manifest.provisional_title = Some(title);
    if let Err(error) = write_manifest(directory, manifest) {
        tracing::warn!(%error, meeting = manifest.id, "could not record provisional meeting title");
        return;
    }
    if let Some(updates) = updates {
        let _ = updates.try_send(RecordingUpdate::Retitled {
            title: display_title(manifest),
        });
    }
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

fn stamp_ended(manifest: &mut MeetingManifest, ended_at_ms: u64) {
    manifest.ended_at_ms.get_or_insert(ended_at_ms);
    manifest
        .duration_ms
        .get_or_insert(ended_at_ms.saturating_sub(manifest.created_at_ms));
}

fn persist_meeting_failure(directory: &Path, error: &color_eyre::Report) {
    let result = (|| -> Result<()> {
        let mut manifest = read_manifest(directory)?;
        if manifest.status != MeetingStatus::Complete {
            manifest.status = MeetingStatus::Failed;
            stamp_ended(&mut manifest, now_ms());
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
                        buffers = buffers.as_ref().ok().map(|buffers| buffers.num_buffers()),
                        buffer_bytes = ?buffers.as_ref().ok().map(|buffers| buffers.iter().map(|buffer| buffer.data_byte_size()).collect::<Vec<_>>()),
                        buffer_channels = ?buffers.as_ref().ok().map(|buffers| buffers.iter().map(|buffer| buffer.number_channels).collect::<Vec<_>>()),
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
                position: 0,
                sample_rate,
                samples,
            };
            if matches!(sender.try_send(packet), Err(TrySendError::Full(_))) {
                drops.fetch_add(1, Ordering::Relaxed);
            }
        },
        output_type,
    );
    registered
        .map(|_| ())
        .map_err(|error| eyre!("ScreenCaptureKit rejected {output_type} output: {error}"))
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
    let buffers = sample.audio_buffer_list().ok()?;
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
    mut rolling: Option<transcript::RollingFeed>,
    live_drops: &AtomicU64,
) -> Result<CaptureStats> {
    let mut system = None;
    let mut microphone = None;
    let mut stats = CaptureStats::default();
    stats.system.file = "system.wav".into();
    stats.microphone.file = "microphone.wav".into();
    while let Ok(mut packet) = receiver.recv() {
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
        packet.position = track.samples;
        track.samples += packet.samples.len() as u64;
        for &sample in &packet.samples {
            writer.write_sample(sample)?;
        }
        if let Some(rolling) = &mut rolling {
            rolling.offer(&packet);
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
    if let Some(rolling) = &mut rolling {
        rolling.flush();
    }
    finalize_track(directory.join("system.wav"), system)?;
    finalize_track(directory.join("microphone.wav"), microphone)?;
    Ok(stats)
}

fn finalize_track(path: PathBuf, writer: Option<WavWriter<BufWriter<File>>>) -> Result<()> {
    if let Some(writer) = writer {
        writer.finalize()?;
        File::open(path)?.sync_all()?;
        Ok(())
    } else {
        create_empty_track(path)
    }
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
    manifest: &mut MeetingManifest,
    selection: &crate::transcription_models::TranscriptionSelection,
    rolling: Option<transcript::RollingOutcome>,
) -> Result<()> {
    let (transcript, echo_suppressed) =
        transcript::transcribe(directory, manifest, selection, rolling)?;
    manifest.echo_suppressed_utterances = echo_suppressed;

    let ndjson_temporary = directory.join("transcript.ndjson.tmp");
    let markdown_temporary = directory.join("transcript.md.tmp");
    let mut ndjson = BufWriter::new(File::create(&ndjson_temporary)?);
    let mut markdown = BufWriter::new(File::create(&markdown_temporary)?);
    set_owner_only(&ndjson_temporary, false)?;
    set_owner_only(&markdown_temporary, false)?;
    writeln!(markdown, "# {}\n", display_title(manifest))?;
    for entry in &transcript {
        serde_json::to_writer(&mut ndjson, entry)?;
        ndjson.write_all(b"\n")?;
        writeln!(markdown, "{}", markdown_line(entry))?;
    }
    ndjson.flush()?;
    markdown.flush()?;
    ndjson.get_ref().sync_all()?;
    markdown.get_ref().sync_all()?;
    sync_directory(directory)?;
    fs::rename(&ndjson_temporary, directory.join("transcript.ndjson"))?;
    fs::rename(&markdown_temporary, directory.join("transcript.md"))?;
    sync_directory(directory)?;

    match transcript::export_markdown(
        &crate::app_settings::meeting_output_folder(),
        manifest,
        &transcript,
    ) {
        Ok(path) => {
            tracing::info!(path = %path.display(), "published meeting transcript");
            manifest.export_path = Some(path);
        }
        Err(error) => {
            tracing::warn!(%error, "could not publish meeting Markdown");
            manifest.export_error = Some(error.to_string());
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
            manifest.live_transcription_error =
                fs::read_to_string(directory.join("transcript.live.error.txt"))
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
                        MeetingStatus::Interrupted => stamp_ended(&mut manifest, now_ms()),
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

/// The local clock time of a timestamp, such as `2:30 PM`.
pub fn clock(milliseconds: u64) -> String {
    transcript::LocalTime::from_ms(milliseconds).clock()
}

/// `Today`, `Yesterday`, or a date such as `Thursday, October 15, 2026`.
pub fn day_label(milliseconds: u64, now_ms: u64) -> String {
    let day = transcript::LocalTime::from_ms(milliseconds);
    if day.day() == transcript::LocalTime::from_ms(now_ms).day() {
        "Today".into()
    } else if day.day() == transcript::LocalTime::from_ms(now_ms.saturating_sub(86_400_000)).day() {
        "Yesterday".into()
    } else {
        day.date()
    }
}

/// The notes as Markdown, for copying.
pub fn notes_markdown(notes: &MeetingNotes) -> String {
    notes::render_notes_block(notes)
        .lines()
        .filter(|line| !line.starts_with("<!--") && !line.starts_with("_Written on this Mac"))
        .collect::<Vec<_>>()
        .join("\n")
        .trim()
        .to_string()
}

/// Speaker turns as Markdown, for copying.
pub fn transcript_markdown(entries: &[TranscriptEntry]) -> String {
    coalesce_transcript(entries.iter().cloned())
        .into_iter()
        .map(|turn| {
            format!(
                "**{}** · {}\n{}",
                turn.source.label(),
                format_duration(turn.start_ms),
                turn.text.trim()
            )
        })
        .collect::<Vec<_>>()
        .join("\n\n")
}

/// Lowercased title, app, notes, and final transcript, for search.
pub fn search_text(manifest: &MeetingManifest) -> String {
    let mut text = format!(
        "{} {} {}",
        display_title(manifest),
        manifest.title,
        manifest.app.as_deref().unwrap_or_default()
    );
    if let Some(notes) = &manifest.notes {
        text.push(' ');
        text.push_str(&notes_markdown(notes));
    }
    if manifest.status == MeetingStatus::Complete
        && let Ok(directory) = root().map(|root| root.join(&manifest.id))
        && let Ok(entries) = read_ndjson::<TranscriptEntry>(&directory.join("transcript.ndjson"))
    {
        for entry in entries {
            text.push(' ');
            text.push_str(&entry.text);
        }
    }
    text.to_lowercase()
}

/// Moves a finished meeting to the Trash: its exported Markdown first, then
/// its recording folder. Refuses while it records, transcribes, or writes
/// notes. A missing Markdown file is skipped.
pub fn trash(id: &str) -> Result<()> {
    trash_meeting(&root()?.join(id), move_to_trash)
}

fn trash_meeting(
    directory: &Path,
    mut move_to_trash: impl FnMut(&Path) -> Result<()>,
) -> Result<()> {
    let manifest = read_manifest(directory)?;
    if meeting_is_active(directory) != Some(false) {
        return Err(eyre!("Stop the recording before deleting it"));
    }
    if notes_in_progress(&manifest.id) {
        return Err(eyre!("Wait for the notes to finish before deleting"));
    }
    if let Some(path) = manifest.export_path.filter(|path| path.exists()) {
        move_to_trash(&path)?;
    }
    move_to_trash(directory)
}

fn move_to_trash(path: &Path) -> Result<()> {
    use objc2_foundation::{NSFileManager, NSString, NSURL};
    let url = NSURL::fileURLWithPath(&NSString::from_str(&path.to_string_lossy()));
    NSFileManager::defaultManager()
        .trashItemAtURL_resultingItemURL_error(&url, None)
        .map_err(|error| {
            eyre!(
                "could not move {} to the Trash: {}",
                path.display(),
                error.localizedDescription()
            )
        })
}

/// Renames a meeting. The Markdown heading follows only while it still reads
/// the previous title, and the file keeps its name, as with generated titles.
pub fn rename(id: &str, title: &str) -> Result<MeetingManifest> {
    rename_meeting(&root()?.join(id), title)
}

fn rename_meeting(directory: &Path, title: &str) -> Result<MeetingManifest> {
    let title = title.split_whitespace().collect::<Vec<_>>().join(" ");
    if title.is_empty() {
        return Err(eyre!("The title is empty"));
    }
    let id = directory
        .file_name()
        .map(|name| name.to_string_lossy().into_owned())
        .unwrap_or_default();
    let mut manifest = read_manifest(directory)?;
    let previous = display_title(&manifest);
    manifest.title = title;
    let next = display_title(&manifest);
    let mut markdown_error = None;
    if previous != next
        && let Some(path) = manifest.export_path.clone().filter(|path| path.exists())
        && let Err(error) = notes::retitle_markdown_file(&path, &previous, &next)
    {
        tracing::warn!(%error, meeting = id, "could not retitle meeting Markdown");
        markdown_error = Some(error);
    }
    write_manifest(directory, &manifest)?;
    match markdown_error {
        Some(error) => Err(error.wrap_err("Renamed, but the Markdown heading was not updated")),
        None => Ok(manifest),
    }
}

/// Debug CLI: replays a recording's tracks through echo suppression.
#[cfg(debug_assertions)]
pub fn echo_report(
    directory: &Path,
    microphone_offset_ms: Option<u64>,
    transcribe: bool,
) -> Result<String> {
    transcript::echo_report(directory, microphone_offset_ms, transcribe)
}

#[cfg(debug_assertions)]
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
        markdown.push_str(&markdown_line(&entry));
        markdown.push('\n');
    }
    Ok(markdown)
}

fn markdown_line(entry: &TranscriptEntry) -> String {
    format!(
        "**{} {}**  {}",
        format_duration(entry.start_ms),
        entry.source.label(),
        entry.text
    )
}

/// Callers recover the final publication first; `show` does so before its
/// `final_transcript_exists` check.
#[cfg(any(debug_assertions, test))]
fn transcript_entries_in(directory: &Path) -> Result<Vec<TranscriptEntry>> {
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

fn set_owner_only(path: &Path, directory: bool) -> Result<()> {
    use std::os::unix::fs::PermissionsExt;
    fs::set_permissions(
        path,
        fs::Permissions::from_mode(if directory { 0o700 } else { 0o600 }),
    )?;
    Ok(())
}

#[cfg(test)]
#[path = "meeting_tests.rs"]
mod tests;

#[cfg(test)]
#[path = "meeting_echo_replay_tests.rs"]
mod echo_replay_tests;
