use std::fs::{self, File, OpenOptions};
use std::io::{BufWriter, Write};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::mpsc::{self, Receiver, SyncSender};
use std::sync::{Arc, Mutex, OnceLock};
use std::thread;
use std::time::{Duration, Instant};

use color_eyre::eyre::{Result, WrapErr, eyre};
use hound::{SampleFormat, WavSpec, WavWriter};
use screencapturekit::prelude::*;
use serde::{Deserialize, Serialize};

use crate::events::now_ms;

const SAMPLE_RATE: u32 = 16_000;
const TRANSCRIPT_TURN_GAP_MS: u64 = 3_000;
/// Beside the manifest of a meeting Hex quit while recording: its audio is
/// saved and its final transcript runs at the next launch.
const FINAL_PENDING_FILE: &str = "final.pending";

#[path = "meeting_capture.rs"]
mod capture;
#[path = "meeting_chapters.rs"]
mod chapters;
pub(crate) use capture::create_tap_aggregate;
pub use capture::{
    SystemAudioAuthorization, request_system_audio_access, system_audio_authorization,
};
#[path = "meeting_echo.rs"]
mod echo;
#[path = "meeting_notes.rs"]
mod notes;
#[path = "meeting_query.rs"]
pub mod query;
#[path = "meeting_transcript.rs"]
mod transcript;
#[cfg(debug_assertions)]
pub use chapters::chapters_file;
pub use chapters::{Chapter, chapter_entries, section_for_id, section_markdown};
pub use notes::{MeetingNotes, display_title, notes_in_progress, notes_wanted, spawn_summary};
#[cfg(debug_assertions)]
pub use notes::{summarize, summarize_file};
pub use transcript::TranscriptReader;

/// What the recorder knows about a meeting when it starts.
#[derive(Clone, Debug, Default)]
pub struct MeetingStart {
    /// An explicit title; otherwise one is derived from the app and time.
    pub title: Option<String>,
    /// The detected call application, such as `Zoom`.
    pub app: Option<String>,
    /// The calendar event the recording was started for.
    pub calendar: Option<MeetingCalendar>,
    /// Where `title` came from; ignored without a title.
    pub title_source: TitleSource,
}

/// Where a meeting's title came from. Only a title the user typed is never
/// extended or replaced by generated ones.
#[derive(Clone, Copy, Debug, Default, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum TitleSource {
    /// The app-and-time title Hex made up.
    #[default]
    Default,
    /// The detected call's own name, such as a browser tab's.
    Call,
    /// The calendar event the recording was started for.
    Calendar,
    /// Typed by the user when recording started or in a rename.
    User,
}

/// The calendar event behind a meeting: its title and the other attendees,
/// kept as context for the transcript and its notes.
#[derive(Clone, Debug, Default, Deserialize, Eq, PartialEq, Serialize)]
pub struct MeetingCalendar {
    pub title: String,
    #[serde(default)]
    pub attendees: Vec<String>,
}

/// What the recorder reports to its owner while it runs.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum RecordingUpdate {
    /// Capture started; `id` identifies the meeting folder and `title` is the one it shows.
    Started { id: String, title: String },
    /// A provisional title from the transcript so far.
    Retitled { title: String },
    /// The live chapters so far (Labs).
    Chapters(Vec<Chapter>),
    /// Capture stopped; final transcription is running.
    Transcribing,
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

/// Asks macOS to list Hex under Screen & System Audio Recording. The system
/// prompts at most once; later requests return the current state.
pub fn request_screen_recording() -> bool {
    // SAFETY: A CoreGraphics request without arguments.
    unsafe { CGRequestScreenCaptureAccess() }
}

/// Whether meeting capture can hear the other participants: through the
/// System Audio Recording consent process taps need, or Screen Recording for
/// the ScreenCaptureKit fallback.
pub fn system_audio_permitted() -> bool {
    system_audio_ready() || screen_recording_permitted()
}

/// Whether system audio uses the display-independent process tap.
pub fn system_audio_ready() -> bool {
    matches!(
        system_audio_authorization(),
        SystemAudioAuthorization::Granted | SystemAudioAuthorization::Unknown
    )
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
    /// Record a meeting now; `title` names it instead of the default.
    Start {
        title: Option<String>,
    },
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
    /// Why the in-call transcript stopped, read from beside the manifest.
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
    /// The calendar event the recording was started for, if any.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub calendar: Option<MeetingCalendar>,
    /// Where `title` came from. Older manifests have none: a title other
    /// than the default one counts as the user's.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub title_source: Option<TitleSource>,
    /// Topic chapters in time order (Labs).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub chapters: Vec<Chapter>,
    /// The chapter boundaries were corrected over the whole transcript.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub chapters_refined: bool,
}

impl MeetingManifest {
    /// Where `title` came from, inferred for manifests that predate the field.
    pub fn title_source(&self) -> TitleSource {
        self.title_source.unwrap_or_else(|| {
            if self.title == default_title(self.app.as_deref(), self.created_at_ms) {
                TitleSource::Default
            } else {
                TitleSource::User
            }
        })
    }
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
    /// Stretches where this source captured nothing while the meeting kept
    /// recording, filled with silence so the track stays on the timeline.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub gaps: Vec<TrackGap>,
}

#[derive(Clone, Debug, Default, Deserialize, Eq, PartialEq, Serialize)]
pub struct TrackGap {
    /// Where the silence starts, in milliseconds of this track.
    pub start_ms: u64,
    pub duration_ms: u64,
}

/// A source that resumes after at least this much silence is padded back
/// onto the timeline. Ordinary callback jitter and clock drift stay below it.
const GAP_PAD_THRESHOLD_MS: u64 = 1_000;

/// Samples missing before `packet`, judged from its arrival on the shared
/// capture clock against the samples this track already holds. Packets are
/// stamped as they finish, so the packet's own length is subtracted; the
/// estimate errs toward padding slightly too little rather than too much.
fn missing_samples(track: &TrackManifest, packet: &AudioPacket) -> u64 {
    let Some(first_arrival_us) = track.first_arrival_us else {
        return 0;
    };
    let elapsed_us = packet.arrival_us.saturating_sub(first_arrival_us);
    let expected_start = (u128::from(elapsed_us) * u128::from(packet.sample_rate) / 1_000_000)
        .saturating_sub(packet.samples.len() as u128) as u64;
    let missing = expected_start.saturating_sub(track.samples);
    if missing * 1_000 >= GAP_PAD_THRESHOLD_MS * u64::from(packet.sample_rate) {
        missing
    } else {
        0
    }
}

#[derive(Clone, Debug, Deserialize, Eq, Hash, PartialEq, Serialize)]
pub struct TranscriptEntry {
    pub source: MeetingSource,
    pub start_ms: u64,
    pub end_ms: u64,
    pub text: String,
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
    /// `None` while another process holds the meeting.
    fn try_acquire(directory: &Path) -> Result<Option<Self>> {
        let file = OpenOptions::new()
            .create(true)
            .truncate(false)
            .read(true)
            .write(true)
            .open(directory.join("active.lock"))?;
        match fs2::FileExt::try_lock_exclusive(&file) {
            Ok(()) => Ok(Some(Self { _file: file })),
            Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => Ok(None),
            Err(error) => Err(error.into()),
        }
    }

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
    if !system_audio_permitted() {
        if !request_system_audio_access() {
            request_screen_recording();
        }
        return Err(eyre!(
            "Allow Hex in System Settings › Privacy & Security › Screen & System Audio Recording, then try again"
        ));
    }
    let created_at_ms = now_ms();
    let id = format!("{created_at_ms}-{}", std::process::id());
    let meeting_dir = root()?.join(&id);
    fs::create_dir_all(&meeting_dir).wrap_err("could not create meeting directory")?;
    set_owner_only(&meeting_dir, true)?;
    let _active = ActiveMeeting::acquire(&meeting_dir)?;

    let given_title = start.title.filter(|title| !title.trim().is_empty());
    let title_source = match (&given_title, start.title_source) {
        (None, _) => TitleSource::Default,
        // A title without a known source is treated as the user's.
        (Some(_), TitleSource::Default) => TitleSource::User,
        (Some(_), source) => source,
    };
    let mut manifest = MeetingManifest {
        schema_version: 1,
        id,
        title: given_title.unwrap_or_else(|| default_title(start.app.as_deref(), created_at_ms)),
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
        live_transcription_error: None,
        error: None,
        app: start.app,
        export_path: None,
        export_error: None,
        echo_suppressed_utterances: 0,
        provisional_title: None,
        notes: None,
        notes_error: None,
        calendar: start.calendar,
        title_source: None,
        chapters: Vec::new(),
        chapters_refined: false,
    };
    set_title_source(&mut manifest, title_source);
    write_manifest(&meeting_dir, &manifest)?;

    let result = (|| -> Result<MeetingManifest> {
        if shutdown.load(Ordering::Relaxed) {
            return Err(eyre!("meeting recording stopped before capture started"));
        }
        let (sender, receiver) = mpsc::sync_channel::<AudioPacket>(128);
        let system_drops = Arc::new(AtomicU64::new(0));
        let microphone_drops = Arc::new(AtomicU64::new(0));
        let mut rolling =
            transcript::Rolling::start(meeting_dir.clone(), transcription_selection.clone());
        let rolling_feed = rolling.feed();
        let flush_registration = transcript::register_flusher(&manifest.id, rolling.flusher());
        let writer_dir = meeting_dir.clone();
        let writer = thread::spawn(move || write_tracks(&writer_dir, receiver, rolling_feed));
        let cleanup_failed_setup =
            |sender: SyncSender<AudioPacket>, writer: thread::JoinHandle<Result<CaptureStats>>| {
                drop(sender);
                if let Err(cleanup_error) = join_audio_writer(writer) {
                    tracing::error!(%cleanup_error, "could not finalize failed meeting setup");
                }
            };
        let capture_clock = Instant::now();
        let sleep_prevention = crate::recording_environment::prevent_sleep();
        let sink = |source, drops: &Arc<AtomicU64>| capture::PacketSink {
            source,
            sender: sender.clone(),
            drops: drops.clone(),
            clock: capture_clock,
        };
        // Each source recovers on its own: losing system audio (for example
        // when ScreenCaptureKit loses its display) never stops the microphone.
        let microphone_capture = capture::SourceSupervisor::microphone(sink(
            MeetingSource::Microphone,
            &microphone_drops,
        ));
        let system_capture =
            capture::SourceSupervisor::system(sink(MeetingSource::System, &system_drops));
        manifest.status = MeetingStatus::Recording;
        if let Err(error) = write_manifest(&meeting_dir, &manifest) {
            drop(system_capture);
            drop(microphone_capture);
            cleanup_failed_setup(sender, writer);
            return Err(error);
        }
        if let Some(updates) = &updates {
            let _ = updates.send(RecordingUpdate::Started {
                id: manifest.id.clone(),
                title: display_title(&manifest),
            });
        }

        let started = Instant::now();
        let titles = notes::ProvisionalTitles::start(manifest.id.clone());
        let live_chapters =
            crate::labs::enabled().then(|| chapters::LiveChapters::start(manifest.id.clone()));
        println!("Recording '{}' (Ctrl-C to stop)", manifest.title);
        let take_updates = |manifest: &mut MeetingManifest| {
            if let Some(title) = titles.take() {
                record_provisional_title(&meeting_dir, manifest, title, updates.as_ref());
            }
            if let Some(chapters) = live_chapters.as_ref().and_then(|live| live.take()) {
                record_chapters(&meeting_dir, manifest, chapters, updates.as_ref());
            }
        };
        while !shutdown.load(Ordering::Relaxed) && !writer.is_finished() {
            take_updates(&mut manifest);
            thread::sleep(Duration::from_millis(100));
        }
        take_updates(&mut manifest);
        if let Some(chapters) = live_chapters.as_ref().and_then(|live| live.finish()) {
            record_chapters(&meeting_dir, &mut manifest, chapters, updates.as_ref());
        }
        drop(titles);
        drop(live_chapters);
        keep_rename(&meeting_dir, &mut manifest);
        drop(flush_registration);
        drop(system_capture);
        drop(microphone_capture);
        drop(sender);
        drop(sleep_prevention);

        manifest.ended_at_ms = Some(now_ms());
        manifest.duration_ms = Some(started.elapsed().as_millis() as u64);
        let mut stats = join_audio_writer(writer)?;
        stats.system.dropped_packets = system_drops.load(Ordering::Relaxed);
        stats.microphone.dropped_packets = microphone_drops.load(Ordering::Relaxed);
        manifest.system = stats.system;
        manifest.microphone = stats.microphone;
        if crate::process_exit::quitting() && has_recorded_audio(&manifest) {
            // Final transcription would outlast quitting's drain.
            drop(rolling.finish());
            fs::write(meeting_dir.join(FINAL_PENDING_FILE), b"")?;
            manifest.status = MeetingStatus::Interrupted;
            write_manifest(&meeting_dir, &manifest)?;
            tracing::info!(
                meeting = manifest.id,
                "Hex quit while recording; the transcript finishes at the next launch"
            );
            return Ok(manifest);
        }
        let capture_error =
            (!has_recorded_audio(&manifest)).then(|| "no meeting audio was captured".to_string());
        prepare_for_transcription(
            &meeting_dir,
            &mut manifest,
            capture_error,
            Ok(()),
            updates.as_ref(),
        )?;

        let rolling = rolling.finish();
        match transcribe(
            &meeting_dir,
            &mut manifest,
            &transcription_selection,
            rolling,
        ) {
            Ok(()) => manifest.status = MeetingStatus::Complete,
            Err(error) => {
                keep_rename(&meeting_dir, &mut manifest);
                manifest.status = MeetingStatus::Failed;
                manifest.error = Some(error.to_string());
                write_manifest(&meeting_dir, &manifest)?;
                return Err(error);
            }
        }
        keep_rename(&meeting_dir, &mut manifest);
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
    keep_rename(directory, manifest);
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

fn record_chapters(
    directory: &Path,
    manifest: &mut MeetingManifest,
    chapters: Vec<Chapter>,
    updates: Option<&SyncSender<RecordingUpdate>>,
) {
    keep_rename(directory, manifest);
    let previous_title = display_title(manifest);
    manifest.chapters = chapters;
    if let Err(error) = write_manifest(directory, manifest) {
        tracing::warn!(%error, meeting = manifest.id, "could not record meeting chapters");
        return;
    }
    if let Some(updates) = updates {
        let _ = updates.try_send(RecordingUpdate::Chapters(manifest.chapters.clone()));
        let title = display_title(manifest);
        if title != previous_title {
            let _ = updates.try_send(RecordingUpdate::Retitled { title });
        }
    }
}

/// Records where the title came from, which only live chapters read; with
/// the experiment off, a manifest that has neither stays as it was.
fn set_title_source(manifest: &mut MeetingManifest, source: TitleSource) {
    if manifest.title_source.is_some() || !manifest.chapters.is_empty() || crate::labs::enabled() {
        manifest.title_source = Some(source);
    }
}

/// Keeps a rename made while recording (Labs).
fn keep_rename(directory: &Path, manifest: &mut MeetingManifest) {
    if crate::labs::enabled() {
        adopt_rename(directory, manifest);
    }
}

/// The recorder rewrites the manifest from memory, so it adopts a user
/// title written meanwhile.
fn adopt_rename(directory: &Path, manifest: &mut MeetingManifest) {
    if let Ok(stored) = read_manifest(directory)
        && stored.title_source() == TitleSource::User
        && (stored.title != manifest.title || manifest.title_source() != TitleSource::User)
    {
        manifest.title = stored.title;
        manifest.title_source = Some(TitleSource::User);
    }
}

fn join_audio_writer(writer: thread::JoinHandle<Result<CaptureStats>>) -> Result<CaptureStats> {
    writer
        .join()
        .map_err(|_| eyre!("meeting audio writer panicked"))
        .and_then(|result| result)
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

fn has_recorded_audio(manifest: &MeetingManifest) -> bool {
    manifest.system.samples > 0 || manifest.microphone.samples > 0
}

fn prepare_for_transcription(
    directory: &Path,
    manifest: &mut MeetingManifest,
    capture_error: Option<String>,
    stop_result: Result<()>,
    updates: Option<&SyncSender<RecordingUpdate>>,
) -> Result<()> {
    if !has_recorded_audio(manifest) {
        if let Some(error) = capture_error {
            write_manifest(directory, manifest)?;
            return Err(eyre!("meeting capture stopped unexpectedly: {error}"));
        }
        if let Err(error) = stop_result {
            write_manifest(directory, manifest)?;
            return Err(error);
        }
    } else {
        if let Some(error) = capture_error {
            tracing::warn!(
                %error,
                meeting = manifest.id,
                "meeting capture stopped early; transcribing recorded audio"
            );
        }
        if let Err(error) = stop_result {
            tracing::warn!(
                %error,
                meeting = manifest.id,
                "meeting capture stop reported an error; transcribing recorded audio"
            );
        }
    }
    if let Some(updates) = updates {
        // Never dropped: the controller drains until the recorder finishes.
        let _ = updates.send(RecordingUpdate::Transcribing);
    }
    manifest.status = MeetingStatus::Transcribing;
    manifest.error = None;
    write_manifest(directory, manifest)
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
    mut rolling: Option<transcript::RollingFeed>,
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
        let missing = missing_samples(track, &packet);
        if missing > 0 {
            let rate = u64::from(packet.sample_rate);
            track.gaps.push(TrackGap {
                start_ms: track.samples * 1_000 / rate,
                duration_ms: missing * 1_000 / rate,
            });
            tracing::warn!(
                source = ?packet.source,
                gap_ms = missing * 1_000 / rate,
                "meeting source resumed after a gap; padding silence"
            );
            for _ in 0..missing {
                writer.write_sample(0.0)?;
            }
            track.samples += missing;
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
    // Transcription takes a while; the headings use a rename made meanwhile.
    keep_rename(directory, manifest);

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
    let _ = fs::remove_file(directory.join(FINAL_PENDING_FILE));
    Ok(())
}

/// Meetings Hex quit while recording, oldest first.
pub fn pending_final_transcripts() -> Vec<String> {
    let Ok(entries) = root().and_then(|root| Ok(fs::read_dir(root)?)) else {
        return Vec::new();
    };
    let mut ids = entries
        .flatten()
        .filter(|entry| entry.path().join(FINAL_PENDING_FILE).exists())
        .filter_map(|entry| entry.file_name().into_string().ok())
        .collect::<Vec<_>>();
    ids.sort();
    ids
}

/// Transcribes and publishes a meeting Hex quit while recording, as Stop
/// would have. `None` when it is no longer pending or another process holds
/// it. Quitting again, or a missing model, leaves it pending for the next
/// launch; any other failure keeps it Interrupted with its draft.
pub fn finish_pending(id: &str) -> Result<Option<MeetingManifest>> {
    let directory = root()?.join(id);
    let Some(_active) = ActiveMeeting::try_acquire(&directory)? else {
        return Ok(None);
    };
    let pending = directory.join(FINAL_PENDING_FILE);
    if !pending.exists() {
        return Ok(None);
    }
    let (_, selection) = crate::app_settings::transcription_selection();
    let model = crate::transcription_models::validate(&selection)?;
    if !crate::transcription_models::is_installed(model, &selection.language) {
        return Err(eyre!("{} is not installed", model.name));
    }
    let mut manifest = read_manifest(&directory)?;
    tracing::info!(
        meeting = id,
        "finishing the transcript of a meeting Hex quit while recording"
    );
    match transcribe(&directory, &mut manifest, &selection, None) {
        Ok(()) => {
            manifest.status = MeetingStatus::Complete;
            manifest.error = None;
            write_manifest(&directory, &manifest)?;
            Ok(Some(manifest))
        }
        Err(error) if crate::process_exit::quitting() => Err(error),
        Err(error) => {
            manifest.status = MeetingStatus::Interrupted;
            manifest.error = Some(error.to_string());
            write_manifest(&directory, &manifest)?;
            let _ = fs::remove_file(&pending);
            Err(error)
        }
    }
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
                fs::read_to_string(directory.join(transcript::ERROR_FILE))
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
    set_title_source(&mut manifest, TitleSource::User);
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

/// Debug CLI: transcribes a finished or interrupted meeting's WAV tracks
/// and publishes its final transcript and Markdown copy.
#[cfg(debug_assertions)]
pub fn transcribe_meeting(id: &str) -> Result<MeetingManifest> {
    let (_, selection) = crate::app_settings::transcription_selection();
    let model = crate::transcription_models::validate(&selection)?;
    if !crate::transcription_models::is_installed(model, &selection.language) {
        return Err(eyre!("{} is not installed", model.name));
    }
    let directory = root()?.join(id);
    if meeting_is_active(&directory) != Some(false) {
        return Err(eyre!("Stop the recording before transcribing it"));
    }
    let mut manifest = read_manifest(&directory)?;
    if !has_recorded_audio(&manifest) {
        return Err(eyre!("meeting has no recorded audio"));
    }
    manifest.status = MeetingStatus::Transcribing;
    manifest.error = None;
    manifest.export_error = None;
    write_manifest(&directory, &manifest)?;
    match transcribe(&directory, &mut manifest, &selection, None) {
        Ok(()) => {
            manifest.status = MeetingStatus::Complete;
            manifest.error = None;
            write_manifest(&directory, &manifest)?;
            Ok(manifest)
        }
        Err(error) => {
            manifest.status = MeetingStatus::Failed;
            manifest.error = Some(error.to_string());
            write_manifest(&directory, &manifest)?;
            Err(error)
        }
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
    let rolling_path = directory.join(transcript::ROLLING_FILE);
    if !rolling_path.exists() {
        return Ok(Vec::new());
    }
    Ok(read_ndjson::<transcript::RollingLine>(&rolling_path)?
        .into_iter()
        .collect::<transcript::RollingView>()
        .entries)
}

static MEETING_PASTE_CURSOR: OnceLock<Mutex<MeetingPasteCursor>> = OnceLock::new();

pub fn shared_paste_cursor() -> &'static Mutex<MeetingPasteCursor> {
    MEETING_PASTE_CURSOR.get_or_init(Default::default)
}

/// What Option-Control-V already handed out: for each source, the capture
/// time spans of the entries pasted. The in-call transcript can still change
/// near its live edge, where echo is judged again as audio arrives, and the
/// final transcript replaces it after Stop, so the cursor follows time rather
/// than identical entries. An entry is new when its midpoint lies outside
/// every pasted span, so text the final pass recovers for audio the in-call
/// transcript skipped is still pasted later.
#[derive(Clone, Debug, Default)]
pub struct MeetingPasteCursor {
    meeting_id: Option<String>,
    copied: [Vec<(u64, u64)>; 2],
}

/// Pasted spans closer than this merge, so a sentence split differently in
/// the final transcript is not pasted again through the pause between two
/// pasted ones.
const PASTE_SPAN_JOIN_MS: u64 = 1_500;

impl MeetingPasteCursor {
    fn unseen(&self, id: &str, entries: &[TranscriptEntry]) -> Vec<TranscriptEntry> {
        let same_meeting = self.meeting_id.as_deref() == Some(id);
        let mut unseen = entries
            .iter()
            .filter(|entry| {
                let middle = (entry.start_ms + entry.end_ms) / 2;
                !same_meeting
                    || !self.copied[cursor_slot(entry.source)]
                        .iter()
                        .any(|&(start, end)| start <= middle && middle <= end)
            })
            .cloned()
            .collect::<Vec<_>>();
        unseen.sort_by_key(|entry| entry.start_ms);
        unseen
    }

    /// Number of coalesced speaker turns available since the last committed
    /// copy or paste for `id`.
    pub fn pending_turn_count(&self, id: &str, entries: &[TranscriptEntry]) -> usize {
        coalesce_transcript(self.unseen(id, entries)).len()
    }

    /// Formats unseen entries for `id`, runs `commit_output`, and advances the
    /// cursor only if `commit_output` succeeds.
    pub fn consume_entries(
        &mut self,
        id: &str,
        entries: &[TranscriptEntry],
        commit_output: impl FnOnce(&str) -> Result<()>,
    ) -> Result<String> {
        let unseen = self.unseen(id, entries);
        let text = format_meeting_delta(&unseen)?;
        commit_output(&text)?;
        if self.meeting_id.as_deref() != Some(id) {
            self.meeting_id = Some(id.to_string());
            self.copied = Default::default();
        }
        for entry in unseen {
            let spans = &mut self.copied[cursor_slot(entry.source)];
            spans.push((entry.start_ms, entry.end_ms));
            spans.sort_unstable();
            let mut merged: Vec<(u64, u64)> = Vec::with_capacity(spans.len());
            for &(start, end) in spans.iter() {
                match merged.last_mut() {
                    Some(last) if start <= last.1 + PASTE_SPAN_JOIN_MS => last.1 = last.1.max(end),
                    _ => merged.push((start, end)),
                }
            }
            *spans = merged;
        }
        Ok(text)
    }
}

fn cursor_slot(source: MeetingSource) -> usize {
    match source {
        MeetingSource::System => 0,
        MeetingSource::Microphone => 1,
    }
}

/// Speaker turns of `entries`, which must be in time order.
pub fn format_meeting_delta(entries: &[TranscriptEntry]) -> Result<String> {
    if entries.is_empty() {
        return Err(eyre!("no new meeting transcript is available"));
    }
    Ok(coalesce_transcript(entries.iter().cloned())
        .into_iter()
        .map(|entry| format!("{}: {}", entry.source.label(), entry.text))
        .collect::<Vec<_>>()
        .join("\n\n"))
}

/// How long a copy of new turns waits for the recording's audio up to the
/// click to be transcribed. That is the next audio packet (milliseconds), a
/// meeting chunk already running (well under a second on Metal), and each
/// source's last few seconds (a few hundred milliseconds each), so it
/// usually takes under a second; three seconds leaves room for a slower Mac
/// or a short backlog while still feeling like a response to the click. A
/// dictation in progress keeps the model, so the copy may run out of time
/// instead.
pub const COPY_TRANSCRIBE_WAIT: Duration = Duration::from_secs(3);

/// What a copy of new turns reads for meeting `id`: while this process
/// records it, the transcript through the click, waiting up to
/// [`COPY_TRANSCRIBE_WAIT`] for it; past that, or otherwise, the in-call
/// or final transcript as it is.
pub fn entries_for_copy(id: &str) -> Result<Vec<TranscriptEntry>> {
    match transcript::transcribe_up_to_now(id, COPY_TRANSCRIBE_WAIT) {
        transcript::Flushed::Fresh(entries) => return Ok(entries),
        transcript::Flushed::TimedOut => tracing::warn!(
            meeting = id,
            "the latest audio was not transcribed in time; copying what is ready"
        ),
        transcript::Flushed::NotRecording => {}
    }
    TranscriptReader::default().read(id)
}

/// Consumes new turns for `id` using the shared cursor, advancing only if
/// `commit_output` succeeds. While recording they come from the transcript
/// through now ([`entries_for_copy`]), afterwards from the final one.
pub fn consume_delta_for_id(
    id: &str,
    commit_output: impl FnOnce(&str) -> Result<()>,
) -> Result<String> {
    let entries = entries_for_copy(id)?;
    shared_paste_cursor()
        .lock()
        .unwrap_or_else(|error| error.into_inner())
        .consume_entries(id, &entries, commit_output)
}

/// Like [`consume_delta_for_id`] over entries the caller already read, for the
/// main thread: it fails rather than wait while Option-Control-V holds the
/// cursor through its paste.
pub fn try_consume_delta_from(
    id: &str,
    entries: &[TranscriptEntry],
    commit_output: impl FnOnce(&str) -> Result<()>,
) -> Result<String> {
    let mut cursor = match shared_paste_cursor().try_lock() {
        Ok(cursor) => cursor,
        Err(std::sync::TryLockError::Poisoned(error)) => error.into_inner(),
        Err(std::sync::TryLockError::WouldBlock) => {
            return Err(eyre!("a meeting paste is in progress"));
        }
    };
    cursor.consume_entries(id, entries, commit_output)
}

/// Speaker turns in `entries` that the shared cursor has not handed out.
pub fn pending_delta_turns(id: &str, entries: &[TranscriptEntry]) -> usize {
    shared_paste_cursor()
        .lock()
        .unwrap_or_else(|error| error.into_inner())
        .pending_turn_count(id, entries)
}

/// Consumes new completed turns for the active or most recent meeting using
/// the shared cursor, advancing only if `commit_output` succeeds.
pub fn consume_active_or_latest_delta(
    commit_output: impl FnOnce(&str) -> Result<()>,
) -> Result<String> {
    let meetings = list()?;
    let selected =
        active_or_latest(&meetings).ok_or_else(|| eyre!("no meeting transcript is available"))?;
    consume_delta_for_id(&selected.id, commit_output)
}

/// Returns the Markdown transcript for `manifest`, preferring its exported
/// Markdown when present and falling back to internal or live speaker turns.
pub fn transcript_markdown_for_manifest(manifest: &MeetingManifest) -> Result<String> {
    if let Some(path) = manifest.export_path.as_ref().filter(|path| path.exists()) {
        return fs::read_to_string(path)
            .wrap_err_with(|| format!("could not read {}", path.display()));
    }
    let internal_md = root()?.join(&manifest.id).join("transcript.md");
    if internal_md.exists() {
        return fs::read_to_string(&internal_md)
            .wrap_err_with(|| format!("could not read {}", internal_md.display()));
    }
    let entries = TranscriptReader::default().read(&manifest.id)?;
    if !entries.is_empty() {
        return Ok(transcript_markdown(&entries));
    }
    Err(eyre!("no meeting transcript is available"))
}

/// Returns the speaker-turn Markdown for `manifest` (without file headers),
/// falling back to exported Markdown if raw entries are unavailable.
pub fn speaker_turns_markdown_for_manifest(manifest: &MeetingManifest) -> Result<String> {
    if let Ok(entries) = TranscriptReader::default().read(&manifest.id)
        && !entries.is_empty()
    {
        return Ok(transcript_markdown(&entries));
    }
    transcript_markdown_for_manifest(manifest)
}

/// Returns the Markdown transcript for the active or most recent meeting.
pub fn latest_transcript_markdown() -> Result<String> {
    let meetings = list()?;
    for meeting in meetings
        .iter()
        .filter(|m| m.status.is_active())
        .chain(meetings.iter().filter(|m| !m.status.is_active()))
    {
        if let Ok(markdown) = speaker_turns_markdown_for_manifest(meeting)
            && !markdown.trim().is_empty()
        {
            return Ok(markdown);
        }
    }
    Err(eyre!("no meeting transcript is available"))
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

#[cfg(test)]
#[path = "meeting_live_replay_tests.rs"]
mod live_replay_tests;
