//! Meetings Hex left unfinished, and the launch recovery that finishes them.
//!
//! A quit or crash can kill the recorder before it finalises its WAVs: their
//! `data` chunk still says 0 bytes and the manifest still says Recording. The
//! pending-finals pass repairs each track from its file length, fills the
//! manifest's sample counts and duration, and runs the ordinary final pass, so
//! the meeting is published and exported as if it had stopped normally.

use std::fs::{self, File, OpenOptions};
use std::io::{Read, Seek, SeekFrom, Write};
use std::path::Path;
use std::sync::mpsc::SyncSender;
use std::thread::{self, JoinHandle};
use std::time::{Duration, Instant};

use color_eyre::eyre::{Result, WrapErr, eyre};

use super::watcher::{ControllerEvent, start_notes};
use super::{
    ActiveMeeting, FINAL_PENDING_FILE, MeetingManifest, MeetingStatus, TrackManifest,
    final_transcript_exists, has_recorded_audio, meeting_is_active, now_ms, read_manifest,
    stamp_ended, write_manifest,
};

/// Beside a meeting whose launch recovery started: a process that dies during
/// the pass leaves it, and the next launch never hands the same audio to Hex
/// again.
const RECOVERY_ATTEMPT_FILE: &str = "recovery.attempt";

/// How long finishing pending transcripts waits for the dictation engine
/// before loading its own model, as a recording without one does.
const ENGINE_GRACE: Duration = Duration::from_secs(30);

/// Finishes, once per launch, the transcripts of meetings Hex quit while
/// recording or that a crash left unfinished. It waits for the dictation
/// engine so the work borrows its model rather than loading a second copy.
pub(crate) struct PendingFinals {
    since: Instant,
    /// The launch sweep for abandoned import folders ran.
    swept: bool,
    checked: bool,
    worker: Option<JoinHandle<()>>,
}

impl PendingFinals {
    pub(crate) fn new() -> Self {
        Self {
            since: Instant::now(),
            swept: false,
            checked: false,
            worker: None,
        }
    }

    pub(crate) fn poll(
        &mut self,
        enabled: bool,
        recording: bool,
        events: &SyncSender<ControllerEvent>,
    ) {
        // Before the recovery pass, and whatever the Meetings opt-in, remove
        // folders a crashed or quitting import left without a manifest.
        if !self.swept {
            self.swept = true;
            super::sweep::remove_orphaned_imports();
        }
        if self.checked
            || !enabled
            || recording
            || (crate::transcription::engine::TranscriptionEngine::shared().is_none()
                && self.since.elapsed() < ENGINE_GRACE)
        {
            return;
        }
        self.checked = true;
        let ids = stranded();
        if ids.is_empty() {
            return;
        }
        let events = events.clone();
        self.worker = thread::Builder::new()
            .name("meeting-pending-finals".into())
            .spawn(move || {
                for id in ids {
                    if crate::desktop::process_exit::quitting() {
                        return;
                    }
                    match recover(&id) {
                        Ok(Some(manifest))
                            if manifest.status == MeetingStatus::Complete =>
                        {
                            tracing::info!(meeting = id, "finished a meeting left unfinished");
                            if super::notes_wanted() {
                                start_notes(manifest.id, &events);
                            }
                        }
                        Ok(Some(manifest)) => {
                            tracing::info!(
                                meeting = id,
                                status = ?manifest.status,
                                "left an unfinished meeting for the next launch"
                            );
                        }
                        Ok(None) => {}
                        Err(error) => {
                            tracing::warn!(%error, meeting = id, "could not finish a meeting transcript");
                        }
                    }
                }
            })
            .map_err(|error| tracing::error!(%error, "could not start meeting recovery"))
            .ok();
    }

    pub(crate) fn join(self) {
        if let Some(worker) = self.worker
            && worker.join().is_err()
        {
            tracing::error!("meeting recovery panicked");
        }
    }
}

/// Meetings a quit or crash left unfinished, oldest first: their folder's lock
/// is free and no final transcript was published.
pub(super) fn stranded() -> Vec<String> {
    super::root().map_or_else(|_| Vec::new(), |root| stranded_in(&root))
}

fn stranded_in(root: &Path) -> Vec<String> {
    let Ok(entries) = fs::read_dir(root) else {
        return Vec::new();
    };
    let mut ids = entries
        .flatten()
        .filter(|entry| entry.path().is_dir())
        .filter(|entry| {
            let directory = entry.path();
            if meeting_is_active(&directory) != Some(false) {
                return false;
            }
            read_manifest(&directory).is_ok_and(|manifest| wants_recovery(&directory, &manifest))
        })
        .filter_map(|entry| entry.file_name().into_string().ok())
        .collect::<Vec<_>>();
    ids.sort();
    ids
}

/// The same selection over manifests an earlier scan already read, so
/// retention does not read every folder twice.
pub(super) fn stranded_among(meetings: &[MeetingManifest]) -> Vec<String> {
    let Ok(root) = super::root() else {
        return Vec::new();
    };
    let mut ids = meetings
        .iter()
        .filter(|manifest| {
            let directory = root.join(&manifest.id);
            meeting_is_active(&directory) == Some(false) && wants_recovery(&directory, manifest)
        })
        .map(|manifest| manifest.id.clone())
        .collect::<Vec<_>>();
    ids.sort();
    ids
}

/// Whether the launch pass could finish this meeting now. Retention only
/// waits for unfinished meetings while it can: with Meetings off or no
/// installed model they expire like any other.
pub(super) fn can_recover() -> bool {
    crate::app_settings::meetings_enabled() && model_installed()
}

fn model_installed() -> bool {
    let (_, selection) = crate::app_settings::transcription_selection();
    crate::transcription::models::validate(&selection)
        .is_ok_and(|model| crate::transcription::models::is_installed(model, &selection.language))
}

/// Whether one folder is waiting for the launch pass, for read-only readers
/// that decide retention without repairing anything.
pub(super) fn awaiting(directory: &Path) -> bool {
    read_manifest(directory).is_ok_and(|manifest| wants_recovery(directory, &manifest))
}

fn wants_recovery(directory: &Path, manifest: &MeetingManifest) -> bool {
    if manifest.status == MeetingStatus::Failed {
        return false;
    }
    // A recovery that died before clearing its marker, or a final pass that
    // stopped before removing `final.pending`, is finished or settled even
    // when a transcript is already published.
    if directory.join(RECOVERY_ATTEMPT_FILE).exists() || directory.join(FINAL_PENDING_FILE).exists()
    {
        return true;
    }
    if final_transcript_exists(directory) {
        return false;
    }
    match manifest.status {
        MeetingStatus::Starting | MeetingStatus::Recording | MeetingStatus::Transcribing => true,
        // A final pass that failed leaves its error: only retry a draft with
        // none, so a broken model or audio never loops.
        MeetingStatus::Interrupted => manifest.error.is_none(),
        MeetingStatus::Complete | MeetingStatus::Failed => false,
    }
}

/// Repairs and finishes one meeting left unfinished. `None` when it is no
/// longer eligible or another process holds it. A missing model leaves it for
/// the next launch; any other failure keeps it Interrupted with its error.
pub(super) fn recover(id: &str) -> Result<Option<MeetingManifest>> {
    let directory = super::root()?.join(id);
    let Some(_active) = ActiveMeeting::try_acquire(&directory)? else {
        return Ok(None);
    };
    let mut manifest = read_manifest(&directory)?;
    if manifest.status == MeetingStatus::Failed {
        return Ok(None);
    }
    // A marker from a process that died during this meeting's last recovery
    // attempt: settle it now and never feed the same audio to Hex again.
    if settle_attempt(&directory, &mut manifest)? {
        return Ok(None);
    }
    if !wants_recovery(&directory, &manifest) {
        return Ok(None);
    }
    let (_, selection) = crate::app_settings::transcription_selection();
    let model = crate::transcription::models::validate(&selection)?;
    if !crate::transcription::models::is_installed(model, &selection.language) {
        return Err(eyre!("{} is not installed", model.name));
    }
    tracing::info!(
        meeting = id,
        status = ?manifest.status,
        "recovering a meeting left unfinished"
    );
    // Durable before the pass, so a native abort leaves evidence that this
    // audio already killed a recovery, and the next launch does not retry it.
    write_attempt(&directory)?;
    let finished = (|| -> Result<Option<MeetingManifest>> {
        if recover_timeline(&directory, &mut manifest)? {
            write_manifest(&directory, &manifest)?;
        }
        if !has_recorded_audio(&manifest) {
            manifest.status = MeetingStatus::Interrupted;
            manifest.error = Some("no meeting audio was captured".into());
            stamp_ended(&mut manifest, now_ms());
            write_manifest(&directory, &manifest)?;
            tracing::warn!(
                meeting = id,
                "a meeting left unfinished has no recoverable audio"
            );
            return Ok(Some(manifest));
        }
        manifest.status = MeetingStatus::Transcribing;
        manifest.error = None;
        manifest.export_error = None;
        write_manifest(&directory, &manifest)?;
        // So a crash during this pass is settled by the next launch, and a
        // normal quit leaves it for the next launch to finish.
        fs::write(directory.join(FINAL_PENDING_FILE), b"")?;
        super::finish::finish_transcription(&directory, manifest, &selection).map(Some)
    })();
    clear_attempt(&directory);
    finished
}

/// Whether this folder holds a recovery attempt a process never finished.
fn attempt_path(directory: &Path) -> std::path::PathBuf {
    directory.join(RECOVERY_ATTEMPT_FILE)
}

fn write_attempt(directory: &Path) -> Result<()> {
    if attempt_path(directory).exists() {
        return Ok(());
    }
    let path = attempt_path(directory);
    let mut file = File::create(&path)?;
    super::set_owner_only(&path, false)?;
    file.write_all(b"")?;
    file.sync_all()?;
    crate::durable::sync_directory(directory)?;
    Ok(())
}

fn clear_attempt(directory: &Path) {
    if fs::remove_file(attempt_path(directory)).is_ok() {
        let _ = crate::durable::sync_directory(directory);
    }
}

/// Settles a marker left by a recovery that died. The meeting keeps its audio
/// but is never handed to the model again; a finished one just loses the
/// stale marker.
fn settle_attempt(directory: &Path, manifest: &mut MeetingManifest) -> Result<bool> {
    if !attempt_path(directory).exists() {
        return Ok(false);
    }
    if manifest.status == MeetingStatus::Complete {
        let _ = fs::remove_file(directory.join(FINAL_PENDING_FILE));
        clear_attempt(directory);
        tracing::info!(
            meeting = manifest.id,
            "cleared a finished meeting's recovery attempt"
        );
        return Ok(true);
    }
    manifest.status = MeetingStatus::Interrupted;
    manifest.error = Some("recovery did not finish".into());
    stamp_ended(manifest, now_ms());
    write_manifest(directory, manifest)?;
    let _ = fs::remove_file(directory.join(FINAL_PENDING_FILE));
    clear_attempt(directory);
    tracing::warn!(
        meeting = manifest.id,
        "a meeting's recovery did not finish; it will not be retried"
    );
    Ok(true)
}

/// Repairs every track whose WAV header still says its data chunk is empty,
/// fills the manifest from the repaired audio, and returns whether this
/// meeting's stop never finalised its tracks.
fn recover_timeline(directory: &Path, manifest: &mut MeetingManifest) -> Result<bool> {
    let repaired = repair_tracks(directory, manifest)?;
    if repaired.changed {
        write_manifest(directory, manifest)?;
    }
    stamp_from_audio(manifest, repaired.unfinalised);
    Ok(repaired.unfinalised)
}

#[derive(Default)]
struct Repaired {
    changed: bool,
    unfinalised: bool,
}

/// Repairs every track whose WAV header still says its data chunk is empty,
/// and fills the manifest from the repaired audio.
fn repair_tracks(directory: &Path, manifest: &mut MeetingManifest) -> Result<Repaired> {
    let mut repaired = Repaired::default();
    for track in [&mut manifest.system, &mut manifest.microphone] {
        let path = directory.join(&track.file);
        if !path.is_file() {
            continue;
        }
        let Some(facts) = repair_wav(&path)? else {
            continue;
        };
        repaired.unfinalised |= facts.unfinalised;
        if track.samples != facts.samples {
            track.samples = facts.samples;
            repaired.changed = true;
        }
        if track.sample_rate != Some(facts.sample_rate) {
            track.sample_rate = Some(facts.sample_rate);
            repaired.changed = true;
        }
    }
    Ok(repaired)
}

/// A recovered track's length, from its file rather than its header.
struct WavFacts {
    samples: u64,
    sample_rate: u32,
    /// Its sizes were rewritten from the file length: the stop never ran.
    unfinalised: bool,
}

/// What a WAV's own headers say about it.
struct WavLayout {
    data_offset: u64,
    data_size: u64,
    block_align: u16,
    sample_rate: u32,
}

/// Rewrites a WAV's RIFF and data sizes from its file length when a killed
/// recorder left them at 0, and returns its sample count and rate.
fn repair_wav(path: &Path) -> Result<Option<WavFacts>> {
    let file_len = fs::metadata(path)
        .wrap_err_with(|| format!("could not measure {}", path.display()))?
        .len();
    let Some(layout) = read_wav_layout(path, file_len)? else {
        return Ok(None);
    };
    // Only an unset or overlong data size is repaired. A file with a valid
    // data chunk and something after it (another chunk, a pad byte) keeps its
    // header, so no trailing bytes become audio.
    let declared_end = layout.data_offset.saturating_add(layout.data_size);
    let unfinalised = layout.data_size == 0 || declared_end > file_len;
    let data_size = if unfinalised {
        let available = file_len.saturating_sub(layout.data_offset);
        // A recorder's data chunk is the last one it writes, so the file
        // length is the audio; whole blocks only.
        available / u64::from(layout.block_align) * u64::from(layout.block_align)
    } else {
        layout.data_size
    };
    let samples = data_size / u64::from(layout.block_align);
    if unfinalised {
        let Some(riff_size) = layout
            .data_offset
            .checked_add(data_size)
            .and_then(|end| end.checked_sub(8))
            .and_then(|size| u32::try_from(size).ok())
        else {
            return Err(eyre!("{} is too large for a WAV", path.display()));
        };
        let Ok(data_size) = u32::try_from(data_size) else {
            return Err(eyre!("{} is too large for a WAV", path.display()));
        };
        let mut file = OpenOptions::new().write(true).open(path)?;
        file.seek(SeekFrom::Start(4))?;
        file.write_all(&riff_size.to_le_bytes())?;
        file.seek(SeekFrom::Start(layout.data_offset - 4))?;
        file.write_all(&data_size.to_le_bytes())?;
        file.sync_all()?;
        tracing::info!(
            path = %path.display(),
            samples,
            "repaired a meeting WAV left unfinalised"
        );
    }
    Ok(Some(WavFacts {
        samples,
        sample_rate: layout.sample_rate,
        unfinalised,
    }))
}

/// Walks a WAV's chunks for its format and data headers. `None` for a file
/// that is not a readable RIFF/WAVE with both.
fn read_wav_layout(path: &Path, file_len: u64) -> Result<Option<WavLayout>> {
    let mut file =
        File::open(path).wrap_err_with(|| format!("could not read {}", path.display()))?;
    let mut header = [0u8; 12];
    if file.read_exact(&mut header).is_err() || &header[..4] != b"RIFF" || &header[8..] != b"WAVE" {
        return Ok(None);
    }
    let mut offset = 12u64;
    let mut sample_rate = 0;
    let mut block_align = 0;
    loop {
        if offset + 8 > file_len {
            return Ok(None);
        }
        file.seek(SeekFrom::Start(offset))?;
        let mut chunk = [0u8; 8];
        if file.read_exact(&mut chunk).is_err() {
            return Ok(None);
        }
        let size = u64::from(u32::from_le_bytes(
            chunk[4..8].try_into().unwrap_or_default(),
        ));
        let body = offset + 8;
        match &chunk[..4] {
            b"fmt " => {
                if size < 16 || body + 16 > file_len {
                    return Ok(None);
                }
                let mut format = [0u8; 16];
                file.seek(SeekFrom::Start(body))?;
                if file.read_exact(&mut format).is_err() {
                    return Ok(None);
                }
                sample_rate = u32::from_le_bytes(format[4..8].try_into().unwrap_or_default());
                block_align = u16::from_le_bytes(format[12..14].try_into().unwrap_or_default());
            }
            b"data" => {
                if sample_rate == 0 || block_align == 0 {
                    return Ok(None);
                }
                return Ok(Some(WavLayout {
                    data_offset: body,
                    data_size: size,
                    block_align,
                    sample_rate,
                }));
            }
            _ => {}
        }
        offset = body + size + size % 2;
    }
}

/// The recovered audio's own length, for a manifest whose stop never wrote
/// one. A stop that did write them keeps its values, unless the tracks had to
/// be repaired from unfinalised headers: then the stop never ran, and a
/// relabel before this pass (such as `list_all`'s now-based stamp) is wrong.
fn stamp_from_audio(manifest: &mut MeetingManifest, unfinalised: bool) {
    let longest = [&manifest.system, &manifest.microphone]
        .into_iter()
        .map(|track: &TrackManifest| {
            let rate = track
                .sample_rate
                .filter(|rate| *rate > 0)
                .unwrap_or(super::SAMPLE_RATE);
            track.samples.saturating_mul(1_000) / u64::from(rate)
        })
        .max()
        .unwrap_or(0);
    if unfinalised && longest > 0 {
        manifest.duration_ms = Some(longest);
        manifest.ended_at_ms = Some(manifest.created_at_ms.saturating_add(longest));
        return;
    }
    manifest.duration_ms.get_or_insert(longest);
    manifest
        .ended_at_ms
        .get_or_insert(manifest.created_at_ms.saturating_add(longest));
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use hound::{SampleFormat, WavReader, WavSpec, WavWriter};

    use super::*;

    fn temporary(name: &str) -> PathBuf {
        let path = std::env::temp_dir().join(format!("hex-recovery-{name}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&path);
        fs::create_dir_all(&path).unwrap();
        path
    }

    fn manifest(id: &str, status: MeetingStatus) -> MeetingManifest {
        MeetingManifest {
            schema_version: 1,
            id: id.into(),
            title: id.into(),
            status,
            created_at_ms: now_ms(),
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
            app: None,
            export_path: None,
            export_error: None,
            echo_suppressed_utterances: 0,
            provisional_title: None,
            notes: None,
            notes_error: None,
            calendar: None,
            title_source: None,
            chapters: Vec::new(),
            chapters_refined: false,
            retention_applies: true,
            export_sha256: None,
            notes_export_path: None,
            notes_export_sha256: None,
            notes_export_removed: false,
            audio_removed_at_ms: None,
            imported: false,
        }
    }

    /// An empty 16 kHz mono float track, as a killed recorder leaves one: its
    /// data bytes written, both sizes 0.
    fn unfinalised_track(path: &Path, samples: usize) {
        let mut writer = WavWriter::create(
            path,
            WavSpec {
                channels: 1,
                sample_rate: 16_000,
                bits_per_sample: 32,
                sample_format: SampleFormat::Float,
            },
        )
        .unwrap();
        for sample in 0..samples {
            writer.write_sample(sample as f32 / 16_000.0).unwrap();
        }
        writer.flush().unwrap();
        drop(writer);
        let file_len = fs::metadata(path).unwrap().len();
        let mut file = OpenOptions::new().write(true).open(path).unwrap();
        file.seek(SeekFrom::Start(4)).unwrap();
        file.write_all(&0u32.to_le_bytes()).unwrap();
        file.seek(SeekFrom::Start(64)).unwrap();
        file.write_all(&0u32.to_le_bytes()).unwrap();
        file.sync_all().unwrap();
        assert_eq!(fs::metadata(path).unwrap().len(), file_len);
        assert_eq!(WavReader::open(path).unwrap().duration(), 0);
    }

    #[test]
    fn a_zeroed_wav_header_is_repaired_from_its_file_length() {
        let directory = temporary("wav");
        let path = directory.join("system.wav");
        unfinalised_track(&path, 16_000);
        let facts = repair_wav(&path).unwrap().unwrap();
        assert_eq!(facts.samples, 16_000);
        assert_eq!(facts.sample_rate, 16_000);
        assert!(facts.unfinalised);
        assert_eq!(WavReader::open(&path).unwrap().duration(), 16_000);
        // Reopening a repaired file leaves its sizes alone.
        let again = repair_wav(&path).unwrap().unwrap();
        assert_eq!(again.samples, 16_000);
        assert!(!again.unfinalised);
        fs::remove_dir_all(directory).unwrap();
    }

    #[test]
    fn a_finished_wav_keeps_its_header() {
        let directory = temporary("finished-wav");
        let path = directory.join("system.wav");
        let mut writer = WavWriter::create(
            &path,
            WavSpec {
                channels: 1,
                sample_rate: 16_000,
                bits_per_sample: 32,
                sample_format: SampleFormat::Float,
            },
        )
        .unwrap();
        writer.write_sample(0.5).unwrap();
        writer.finalize().unwrap();
        let before = fs::read(&path).unwrap();
        let facts = repair_wav(&path).unwrap().unwrap();
        assert_eq!(facts.samples, 1);
        assert!(!facts.unfinalised);
        assert_eq!(fs::read(&path).unwrap(), before);
        fs::remove_dir_all(directory).unwrap();
    }

    #[test]
    fn a_trailing_chunk_is_not_absorbed_into_the_audio() {
        let directory = temporary("trailing-chunk");
        let path = directory.join("system.wav");
        let mut writer = WavWriter::create(
            &path,
            WavSpec {
                channels: 1,
                sample_rate: 16_000,
                bits_per_sample: 32,
                sample_format: SampleFormat::Float,
            },
        )
        .unwrap();
        writer.write_sample(0.5).unwrap();
        writer.finalize().unwrap();
        // An odd-sized chunk after the data, with its pad byte.
        let mut bytes = fs::read(&path).unwrap();
        bytes.extend_from_slice(b"LIST");
        bytes.extend_from_slice(&3u32.to_le_bytes());
        bytes.extend_from_slice(&[1, 2, 3, 0]);
        fs::write(&path, &bytes).unwrap();
        let before = fs::read(&path).unwrap();
        let facts = repair_wav(&path).unwrap().unwrap();
        assert_eq!(facts.samples, 1);
        assert!(!facts.unfinalised);
        assert_eq!(
            fs::read(&path).unwrap(),
            before,
            "a valid data chunk keeps its header"
        );
        fs::remove_dir_all(directory).unwrap();
    }

    #[test]
    fn a_stranded_recording_is_selected_for_recovery() {
        let root = temporary("stranded");
        let folder = |id: &str| {
            let directory = root.join(id);
            fs::create_dir_all(&directory).unwrap();
            directory
        };
        let stranded = folder("stranded");
        unfinalised_track(&stranded.join("system.wav"), 1_600);
        unfinalised_track(&stranded.join("microphone.wav"), 0);
        write_manifest(&stranded, &manifest("stranded", MeetingStatus::Recording)).unwrap();

        let done = folder("done");
        fs::write(done.join("transcript.ndjson"), b"").unwrap();
        fs::write(done.join("transcript.md"), b"").unwrap();
        write_manifest(&done, &manifest("done", MeetingStatus::Complete)).unwrap();

        let failed = folder("failed");
        let mut broken = manifest("failed", MeetingStatus::Failed);
        broken.error = Some("capture failed".into());
        write_manifest(&failed, &broken).unwrap();

        // Interrupted with an error is a final pass that already failed.
        let stopped = folder("stopped");
        let mut errored = manifest("stopped", MeetingStatus::Interrupted);
        errored.error = Some("model failed".into());
        write_manifest(&stopped, &errored).unwrap();

        // Another process still records this one.
        let busy = folder("busy");
        write_manifest(&busy, &manifest("busy", MeetingStatus::Recording)).unwrap();
        let held = ActiveMeeting::acquire(&busy).unwrap();

        assert_eq!(stranded_in(&root), ["stranded"]);
        drop(held);
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn a_published_transcript_with_a_pending_marker_is_still_selected() {
        let root = temporary("published-pending");
        let directory = root.join("published");
        fs::create_dir_all(&directory).unwrap();
        fs::write(directory.join("transcript.ndjson"), b"").unwrap();
        fs::write(directory.join("transcript.md"), b"").unwrap();
        fs::write(directory.join(FINAL_PENDING_FILE), b"").unwrap();
        write_manifest(&directory, &manifest("published", MeetingStatus::Complete)).unwrap();

        // The pass stopped between publishing and removing the marker: the
        // old finish_pending would still redo its export and clear it.
        assert!(wants_recovery(
            &directory,
            &read_manifest(&directory).unwrap()
        ));
        assert_eq!(stranded_in(&root), ["published"]);
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn a_recovery_that_died_is_marked_and_never_retried() {
        let directory = temporary("attempt");
        write_manifest(&directory, &manifest("died", MeetingStatus::Recording)).unwrap();
        write_attempt(&directory).unwrap();
        assert!(attempt_path(&directory).exists());

        let mut meeting = read_manifest(&directory).unwrap();
        assert!(settle_attempt(&directory, &mut meeting).unwrap());
        let settled = read_manifest(&directory).unwrap();
        assert_eq!(settled.status, MeetingStatus::Interrupted);
        assert_eq!(settled.error.as_deref(), Some("recovery did not finish"));
        assert!(!attempt_path(&directory).exists());
        assert!(
            !wants_recovery(&directory, &settled),
            "the audio that killed a recovery is never tried again"
        );
        fs::remove_dir_all(directory).unwrap();
    }

    #[test]
    fn a_relabelled_meeting_takes_its_duration_from_the_audio() {
        let directory = temporary("relabelled");
        unfinalised_track(&directory.join("system.wav"), 32_000);
        let mut meeting = manifest("relabelled", MeetingStatus::Recording);
        write_manifest(&directory, &meeting).unwrap();
        // `list_all` relabels a stranded meeting before the launch pass runs.
        meeting.status = MeetingStatus::Interrupted;
        stamp_ended(&mut meeting, now_ms());
        write_manifest(&directory, &meeting).unwrap();

        let mut meeting = read_manifest(&directory).unwrap();
        assert!(
            recover_timeline(&directory, &mut meeting).unwrap(),
            "the stop never finalised the track"
        );
        assert_eq!(meeting.duration_ms, Some(2_000));
        assert_eq!(
            meeting.ended_at_ms,
            Some(meeting.created_at_ms + 2_000),
            "the audio's length wins over the relabel's time"
        );
        fs::remove_dir_all(directory).unwrap();
    }

    #[test]
    fn recovery_fills_the_manifest_from_the_repaired_audio() {
        let directory = temporary("repair-tracks");
        unfinalised_track(&directory.join("system.wav"), 32_000);
        unfinalised_track(&directory.join("microphone.wav"), 8_000);
        let mut meeting = manifest("tracks", MeetingStatus::Recording);
        assert!(recover_timeline(&directory, &mut meeting).unwrap());
        assert_eq!(meeting.system.samples, 32_000);
        assert_eq!(meeting.microphone.samples, 8_000);
        assert_eq!(meeting.system.sample_rate, Some(16_000));
        assert_eq!(meeting.duration_ms, Some(2_000));
        assert_eq!(
            meeting.ended_at_ms,
            Some(meeting.created_at_ms + 2_000),
            "the recovered meeting ends with its audio"
        );
        fs::remove_dir_all(directory).unwrap();
    }
}
