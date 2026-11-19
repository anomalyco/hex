//! Labs › Transcribe dropped audio: a file with one of the common extensions
//! for containers the shared pieces reader decodes ([`EXTENSIONS`]) is
//! dropped on the window, read in pieces of at most 30 s, transcribed with
//! the selected model in this process, and saved as `<stem>.txt` beside it.
//! The reader detects the format from the file's contents, so the list names
//! the usual extension of each container rather than every one possible. The
//! same extensions are offered on every OS. Nothing is pasted, saved to
//! History, or processed.
//!
//! A file's own folder takes the transcript unless it is unwritable or the
//! file sits in the OS temp folder, where a promised drop's copy lands: then
//! the transcript goes to Downloads. A taken name gets " 2", " 3", and so on,
//! and the write itself is atomic, so a transcript Hex cannot finish never
//! replaces another one; a crash between reserving a name and the rename can
//! still leave an empty `<stem>.txt`.

use std::fs::{self, OpenOptions};
use std::io;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::time::{Duration, Instant};

use super::models::{TranscriptionSelection, is_installed, validate};
use super::pieces::{PieceError, Pieces};
use super::service::{
    AudioClip, AudioError, TranscriptionService, TranscriptionServiceError,
    TranscriptionServiceHandle,
};
use super::transcript::TimestampLevel;

/// One piece waits for a cold model behind it and for its own transcription.
const PIECE_DEADLINE: Duration = Duration::from_secs(10 * 60);
/// How often a waiting piece checks for cancellation.
const CANCEL_POLL: Duration = Duration::from_millis(100);
/// How many taken names one transcript tries before giving up.
const MAX_ATTEMPTS: usize = 1_000;

/// The common file extensions for audio containers the shared pieces reader
/// decodes, without the dot, on every OS. The reader detects the format from
/// the file's contents rather than the name, so this is the list Hex offers,
/// not an exhaustive one: what the window's drop target, its "Transcribe a
/// file…" dialog, and their copy all offer.
pub const EXTENSIONS: &[&str] = &[
    "wav", "wave", "mp3", "aac", "m4a", "m4b", "mp4", "m4v", "3gp", "mov", "aiff", "aif", "aifc",
    "caf", "flac", "ogg", "oga", "opus", "webm", "weba", "mkv", "mka",
];

/// Those kinds as the copy names them, one name per kind (`aif` and `aifc`
/// under AIFF, `oga` under OGG, `m4b` under M4A, `m4v` and `3gp` under MP4,
/// `weba` under WebM, `mka` under MKV, `wave` under WAV), for the drop hint,
/// the refusal, and the Labs card.
pub const FORMATS: &str = "WAV, MP3, AAC, M4A, MP4, MOV, AIFF, CAF, FLAC, OGG, Opus, WebM, or MKV";

/// Why a dropped file could not become a transcript.
#[derive(Debug)]
pub enum FileTranscriptError {
    /// Labs › Transcribe dropped audio is off.
    Disabled,
    /// The extension is not one this build offers.
    UnsupportedFormat { path: PathBuf },
    /// The file could not be opened, or its audio format is not supported,
    /// with the reason.
    Unreadable { path: PathBuf, reason: String },
    /// The file's audio is in a codec this build cannot decode, named.
    UnsupportedCodec { path: PathBuf, codec: String },
    /// The file is a container with no audio track, such as a silent screen
    /// recording.
    NoAudioTrack { path: PathBuf },
    /// The file holds no audio at all.
    NoAudio { path: PathBuf },
    /// The file is truncated or changed while Hex read it, with where that
    /// was found when the reader knew (a compressed file that stopped
    /// decoding partway).
    Corrupt { path: PathBuf, at_ms: Option<u64> },
    /// The file holds no speech at all.
    NoSpeech { path: PathBuf },
    /// The selected model is not installed or cannot run here.
    ModelUnavailable(String),
    /// The transcription itself failed.
    TranscriptionFailed(String),
    /// The person cancelled the drop.
    Cancelled,
    /// The transcript could not be written.
    WriteFailed(String),
}

impl std::fmt::Display for FileTranscriptError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Disabled => {
                write!(formatter, "Labs › Transcribe dropped audio is off")
            }
            Self::UnsupportedFormat { path } => write!(
                formatter,
                "{} is not a kind of audio file Hex can transcribe; drop a {FORMATS} file",
                path.display()
            ),
            Self::Unreadable { path, reason } => {
                write!(formatter, "{} could not be read: {reason}", path.display())
            }
            Self::UnsupportedCodec { path, codec } => write!(
                formatter,
                "{} holds audio in a codec Hex cannot decode ({codec})",
                path.display()
            ),
            Self::NoAudioTrack { path } => {
                write!(formatter, "{} has no audio track", path.display())
            }
            Self::NoAudio { path } => {
                write!(formatter, "{} has no audio", path.display())
            }
            Self::Corrupt {
                path,
                at_ms: Some(at_ms),
            } => write!(
                formatter,
                "{} is damaged at {}",
                path.display(),
                clock(*at_ms)
            ),
            Self::Corrupt { path, at_ms: None } => {
                write!(formatter, "{} is truncated or corrupt", path.display())
            }
            Self::NoSpeech { path } => {
                write!(formatter, "No speech was heard in {}", path.display())
            }
            Self::ModelUnavailable(message) | Self::TranscriptionFailed(message) => {
                formatter.write_str(message)
            }
            Self::Cancelled => formatter.write_str("The transcription was cancelled"),
            Self::WriteFailed(message) => {
                write!(formatter, "The transcript could not be saved: {message}")
            }
        }
    }
}

impl std::error::Error for FileTranscriptError {}

/// Transcribes `source` and writes `<stem>.txt` beside it (or in Downloads),
/// returning the file it wrote. Blocks, model load included: call it on a
/// worker thread. `progress` receives the fraction of the file decoded so
/// far, from 0 to 1, on this thread, or `None` when the file's length is not
/// known (a compressed file whose header gives no duration). `cancel` stops
/// between pieces, every [`CANCEL_POLL`] while a piece waits for its
/// transcript, and once more before the transcript is saved.
pub fn transcribe_audio_file(
    source: &Path,
    selection: TranscriptionSelection,
    cancel: Arc<AtomicBool>,
    mut progress: impl FnMut(Option<f32>),
) -> Result<PathBuf, FileTranscriptError> {
    if !crate::labs::drop_transcribe() {
        return Err(FileTranscriptError::Disabled);
    }
    check_extension(source)?;
    let model = validate(&selection).map_err(model_unavailable)?;
    if !is_installed(model, &selection.language) {
        return Err(model_unavailable(format!(
            "{} is not installed; choose another model or install it",
            model.name
        )));
    }
    let mut pieces = Pieces::open(source).map_err(|error| file_error(source, error))?;
    // A saved file is read whole once before a model loads, as
    // `hex transcribe` does, so a truncated WAV is refused rather than
    // failing after minutes of transcription. Compressed audio decodes once,
    // as it transcribes, and damage found later names the position where it
    // is (`12:34` into the file).
    pieces
        .validate()
        .map_err(|error| file_error(source, error))?;
    let total = pieces.duration_ms().filter(|total| *total > 0);
    let (mut service, handle) =
        TranscriptionService::start().map_err(|error| transcription_failed(&error))?;
    // The worker runs until every sender is gone; the handle holds one, so
    // keeping it would make the shutdown wait forever after the result.
    let result = transcribe_pieces(
        &mut pieces,
        &handle,
        &selection,
        source,
        total,
        &cancel,
        &mut progress,
    );
    drop(handle);
    service.shutdown();
    let text = result?;
    still_wanted(&cancel)?;
    let path = save_transcript(source, &text)?;
    progress(Some(1.0));
    Ok(path)
}

/// Whether this build's drop target offers `path`'s extension.
fn extension_accepted(path: &Path) -> bool {
    path.extension()
        .and_then(|extension| extension.to_str())
        .map(|extension| extension.to_ascii_lowercase())
        .is_some_and(|extension| EXTENSIONS.contains(&extension.as_str()))
}

/// Refuses a file whose kind this build doesn't offer, before a model loads.
fn check_extension(path: &Path) -> Result<(), FileTranscriptError> {
    if extension_accepted(path) {
        Ok(())
    } else {
        Err(FileTranscriptError::UnsupportedFormat {
            path: path.to_path_buf(),
        })
    }
}

/// Whether a finished transcript may still be written: a cancel that lands
/// while the last piece runs stops the file from being saved.
fn still_wanted(cancel: &AtomicBool) -> Result<(), FileTranscriptError> {
    if cancel.load(Ordering::Acquire) {
        return Err(FileTranscriptError::Cancelled);
    }
    Ok(())
}

/// Decodes and transcribes every piece in order, joining their text with one
/// space. A piece the speech detector hears nothing in comes back empty and
/// adds nothing, as it does for `hex transcribe`.
fn transcribe_pieces(
    pieces: &mut Pieces,
    handle: &TranscriptionServiceHandle,
    selection: &TranscriptionSelection,
    source: &Path,
    total: Option<u64>,
    cancel: &AtomicBool,
    progress: &mut impl FnMut(Option<f32>),
) -> Result<String, FileTranscriptError> {
    let mut texts = Vec::new();
    progress(total.is_some().then_some(0.0));
    loop {
        if cancel.load(Ordering::Acquire) {
            return Err(FileTranscriptError::Cancelled);
        }
        let Some(samples) = pieces.next().map_err(|error| file_error(source, error))? else {
            break;
        };
        let text = transcribe_piece(handle, selection, samples, cancel)?;
        if !text.is_empty() {
            texts.push(text);
        }
        let fraction =
            total.map(|total| (pieces.position_ms() as f32 / total as f32).clamp(0.0, 1.0));
        progress(fraction);
    }
    let text = texts.join(" ");
    if text.trim().is_empty() {
        return Err(FileTranscriptError::NoSpeech {
            path: source.to_path_buf(),
        });
    }
    Ok(text)
}

/// One piece through the service, waiting for a cold model and checking
/// `cancel` while it does.
fn transcribe_piece(
    handle: &TranscriptionServiceHandle,
    selection: &TranscriptionSelection,
    samples: Vec<f32>,
    cancel: &AtomicBool,
) -> Result<String, FileTranscriptError> {
    let duration_ms = samples.len() as u64 * 1_000 / 16_000;
    let clip = AudioClip {
        samples,
        duration_ms,
    };
    // This service is fresh for this file, so nothing else can hold its
    // audio slot: a refusal here is a bug, not a busy service.
    let Some(admission) = handle.admit_audio() else {
        debug_assert!(false, "a file's fresh service refused its first clip");
        return Err(transcription_failed(
            "Hex's transcription service refused the file's first clip",
        ));
    };
    let job = handle
        .transcribe(selection.clone(), clip, admission, TimestampLevel::None)
        .map_err(service_error)?;
    let deadline = Instant::now() + PIECE_DEADLINE;
    loop {
        if cancel.load(Ordering::Acquire) {
            job.cancel();
            return Err(FileTranscriptError::Cancelled);
        }
        match job.wait_timeout(CANCEL_POLL) {
            Ok(Some(Ok(transcript))) => return Ok(transcript.text.trim().to_owned()),
            Ok(Some(Err(error))) => return Err(service_error(error)),
            Ok(None) if Instant::now() >= deadline => {
                job.cancel();
                return Err(transcription_failed("Transcribing this file took too long"));
            }
            Ok(None) => {}
            Err(error) => return Err(service_error(error)),
        }
    }
}

fn service_error(error: TranscriptionServiceError) -> FileTranscriptError {
    match error {
        TranscriptionServiceError::QueueFull => {
            transcription_failed("Another transcription is running")
        }
        TranscriptionServiceError::Unavailable => {
            transcription_failed("Hex's transcription service stopped")
        }
        TranscriptionServiceError::Cancelled => FileTranscriptError::Cancelled,
        TranscriptionServiceError::Model(message) => FileTranscriptError::ModelUnavailable(message),
        TranscriptionServiceError::Inference(message) => {
            FileTranscriptError::TranscriptionFailed(message)
        }
        TranscriptionServiceError::Timestamps => {
            transcription_failed("The model cannot give these timings")
        }
    }
}

fn model_unavailable(error: impl std::fmt::Display) -> FileTranscriptError {
    FileTranscriptError::ModelUnavailable(error.to_string())
}

fn transcription_failed(error: impl std::fmt::Display) -> FileTranscriptError {
    FileTranscriptError::TranscriptionFailed(error.to_string())
}

fn file_error(path: &Path, error: PieceError) -> FileTranscriptError {
    match error {
        PieceError::Empty => FileTranscriptError::NoAudio {
            path: path.to_path_buf(),
        },
        PieceError::Incomplete => FileTranscriptError::Corrupt {
            path: path.to_path_buf(),
            at_ms: None,
        },
        PieceError::Changed { at_ms } | PieceError::Damaged { at_ms } => {
            FileTranscriptError::Corrupt {
                path: path.to_path_buf(),
                at_ms: Some(at_ms),
            }
        }
        PieceError::UnsupportedCodec(codec) => FileTranscriptError::UnsupportedCodec {
            path: path.to_path_buf(),
            codec,
        },
        PieceError::NoAudioTrack => FileTranscriptError::NoAudioTrack {
            path: path.to_path_buf(),
        },
        PieceError::Io(error) => FileTranscriptError::Unreadable {
            path: path.to_path_buf(),
            reason: error.to_string(),
        },
        PieceError::NotMedia => FileTranscriptError::Unreadable {
            path: path.to_path_buf(),
            reason: "it is not an audio or video file Hex can read".into(),
        },
        PieceError::Audio(error) => FileTranscriptError::Unreadable {
            path: path.to_path_buf(),
            reason: audio_reason(&error).into(),
        },
    }
}

/// A position in a file as `m:ss`, or `h:mm:ss` past an hour, for a message
/// that names where damage was found.
fn clock(ms: u64) -> String {
    let seconds = ms / 1_000;
    if seconds >= 3_600 {
        format!(
            "{}:{:02}:{:02}",
            seconds / 3_600,
            seconds / 60 % 60,
            seconds % 60
        )
    } else {
        format!("{}:{:02}", seconds / 60, seconds % 60)
    }
}

/// Why a WAV this build can't take was refused, for the unreadable message.
fn audio_reason(error: &AudioError) -> &'static str {
    match error {
        AudioError::Unsupported => "its sample rate or channels are ones Hex cannot decode",
        AudioError::Invalid => "its WAV audio is malformed",
        AudioError::ResourceExhausted => "it is too large to decode",
    }
}

/// Writes `text` beside `source`, or in Downloads when `source`'s own folder
/// can't take it, and returns the file. Downloads is only needed for that
/// fallback: a writable source folder needs no Downloads at all.
fn save_transcript(source: &Path, text: &str) -> Result<PathBuf, FileTranscriptError> {
    let stem = transcript_stem(source);
    let source_directory = parent_directory(source);
    let downloads = crate::app_paths::downloads_dir();
    let directory = transcript_directory(
        source,
        &std::env::temp_dir(),
        downloads.as_deref(),
        directory_is_writable(&source_directory),
    )
    .ok_or_else(|| {
        FileTranscriptError::WriteFailed("the Downloads folder is unavailable".into())
    })?;
    let contents = format!("{text}\n");
    let result = write_transcript(&directory, &stem, &contents);
    match result {
        Err(FileTranscriptError::WriteFailed(_)) => match downloads {
            Some(downloads) if downloads != directory => {
                write_transcript(&downloads, &stem, &contents)
            }
            _ => result,
        },
        result => result,
    }
}

/// Where a transcript for `source` goes: its own folder, unless that folder
/// is unwritable or the file sits in the OS temp folder (a promised drop's
/// copy), in which case `downloads`, or `None` when there is no Downloads
/// folder to fall back to.
fn transcript_directory(
    source: &Path,
    temp: &Path,
    downloads: Option<&Path>,
    writable: bool,
) -> Option<PathBuf> {
    let directory = parent_directory(source);
    if !writable || inside(&directory, temp) {
        downloads.map(Path::to_path_buf)
    } else {
        Some(directory)
    }
}

fn parent_directory(source: &Path) -> PathBuf {
    source
        .parent()
        .filter(|parent| !parent.as_os_str().is_empty())
        .unwrap_or(Path::new("."))
        .to_path_buf()
}

/// Whether `path` is `directory` or below it, resolving symlinks through the
/// longest part of each that exists (`/tmp` and `/private/tmp` name the same
/// temp folder on macOS, though a dropped file's own folder may not exist yet).
fn inside(path: &Path, directory: &Path) -> bool {
    resolve(path).starts_with(resolve(directory))
}

/// `path` with its longest existing prefix resolved through symlinks and the
/// rest kept as it is.
fn resolve(path: &Path) -> PathBuf {
    let mut existing = path;
    let mut rest = Vec::new();
    loop {
        if let Ok(resolved) = existing.canonicalize() {
            let mut resolved = resolved;
            for name in rest.iter().rev() {
                resolved.push(name);
            }
            return resolved;
        }
        match (existing.parent(), existing.file_name()) {
            (Some(parent), Some(name)) => {
                rest.push(name.to_os_string());
                existing = parent;
            }
            _ => return path.to_path_buf(),
        }
    }
}

/// Whether a transcript may be written into `directory`: a probe file is
/// created and removed, so an unwritable folder is found before anything is
/// written, and its transcript goes to Downloads instead.
fn directory_is_writable(directory: &Path) -> bool {
    static SEQUENCE: AtomicU64 = AtomicU64::new(0);
    let probe = directory.join(format!(
        ".hex-transcribe-{}-{}.tmp",
        std::process::id(),
        SEQUENCE.fetch_add(1, Ordering::Relaxed)
    ));
    match OpenOptions::new().write(true).create_new(true).open(&probe) {
        Ok(file) => {
            drop(file);
            let _ = fs::remove_file(&probe);
            true
        }
        Err(_) => false,
    }
}

fn transcript_stem(source: &Path) -> String {
    source
        .file_stem()
        .map(|stem| stem.to_string_lossy().trim().to_string())
        .filter(|stem| !stem.is_empty())
        .unwrap_or_else(|| "transcript".into())
}

/// The `number`th free name's file name: `<stem>.txt`, then `<stem> 2.txt`,
/// `<stem> 3.txt`, and so on.
fn transcript_name(stem: &str, number: usize) -> String {
    if number <= 1 {
        format!("{stem}.txt")
    } else {
        format!("{stem} {number}.txt")
    }
}

/// Writes `contents` to the first free `<stem>.txt` name in `directory`,
/// atomically and without ever replacing a file that is already there.
fn write_transcript(
    directory: &Path,
    stem: &str,
    contents: &str,
) -> Result<PathBuf, FileTranscriptError> {
    fs::create_dir_all(directory).map_err(write_failed)?;
    let mut reserved = None;
    for number in 1..=MAX_ATTEMPTS {
        let candidate = directory.join(transcript_name(stem, number));
        match OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&candidate)
        {
            Ok(file) => {
                drop(file);
                reserved = Some(candidate);
                break;
            }
            Err(error) if error.kind() == io::ErrorKind::AlreadyExists => continue,
            Err(error) => return Err(write_failed(error)),
        }
    }
    let path = reserved.ok_or_else(|| {
        FileTranscriptError::WriteFailed(format!(
            "too many files are named {stem}.txt in {}",
            directory.display()
        ))
    })?;
    match crate::durable::write(&path, contents.as_bytes()) {
        Ok(()) => Ok(path),
        Err(error) => {
            let _ = fs::remove_file(&path);
            Err(write_failed(error))
        }
    }
}

fn write_failed(error: impl std::fmt::Display) -> FileTranscriptError {
    FileTranscriptError::WriteFailed(error.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::transcription::pieces::PIECE_SAMPLES;

    fn temp_directory(name: &str) -> PathBuf {
        let directory =
            std::env::temp_dir().join(format!("hex-transcribe-file-{name}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&directory);
        fs::create_dir_all(&directory).unwrap();
        directory
    }

    /// A 16 kHz mono 16-bit WAV of `samples` samples at `path`.
    fn write_wav(path: &Path, samples: usize) {
        let spec = hound::WavSpec {
            channels: 1,
            sample_rate: 16_000,
            bits_per_sample: 16,
            sample_format: hound::SampleFormat::Int,
        };
        let mut writer = hound::WavWriter::create(path, spec).unwrap();
        for sample in 0..samples {
            writer.write_sample((sample % 200) as i16).unwrap();
        }
        writer.finalize().unwrap();
    }

    #[test]
    fn the_first_free_name_numbering_counts_up() {
        assert_eq!(transcript_name("talk", 1), "talk.txt");
        assert_eq!(transcript_name("talk", 2), "talk 2.txt");
        assert_eq!(transcript_name("talk", 3), "talk 3.txt");
    }

    #[test]
    fn a_transcript_never_overwrites_an_earlier_one() {
        let directory = temp_directory("names");
        let first = write_transcript(&directory, "talk", "first\n").unwrap();
        let second = write_transcript(&directory, "talk", "second\n").unwrap();
        let third = write_transcript(&directory, "talk", "third\n").unwrap();
        assert_eq!(first.file_name().unwrap(), "talk.txt");
        assert_eq!(second.file_name().unwrap(), "talk 2.txt");
        assert_eq!(third.file_name().unwrap(), "talk 3.txt");
        assert_eq!(fs::read_to_string(&first).unwrap(), "first\n");
        assert_eq!(fs::read_to_string(&second).unwrap(), "second\n");
        assert_eq!(fs::read_to_string(&third).unwrap(), "third\n");
        assert_eq!(
            fs::read_dir(&directory).unwrap().count(),
            3,
            "no staging file is left"
        );
        fs::remove_dir_all(directory).unwrap();
    }

    #[test]
    fn a_file_beside_its_source_keeps_its_own_folder() {
        assert_eq!(
            transcript_directory(
                Path::new("/home/a/notes/talk.mp3"),
                Path::new("/tmp"),
                Some(Path::new("/home/a/Downloads")),
                true,
            ),
            Some(PathBuf::from("/home/a/notes"))
        );
    }

    /// Downloads is only the fallback: a writable folder beside the source
    /// needs no Downloads folder at all.
    #[test]
    fn a_writable_source_folder_needs_no_downloads_folder() {
        assert_eq!(
            transcript_directory(
                Path::new("/home/a/notes/talk.wav"),
                Path::new("/tmp"),
                None,
                true,
            ),
            Some(PathBuf::from("/home/a/notes"))
        );
        assert_eq!(
            transcript_directory(
                Path::new("/home/a/notes/talk.wav"),
                Path::new("/tmp"),
                None,
                false,
            ),
            None,
            "an unwritable folder with no Downloads cannot be saved"
        );
        assert_eq!(
            transcript_directory(Path::new("/tmp/talk.wav"), Path::new("/tmp"), None, true),
            None,
            "a temp file with no Downloads cannot be saved"
        );
    }

    #[test]
    fn an_unwritable_folder_sends_the_transcript_to_downloads() {
        assert_eq!(
            transcript_directory(
                Path::new("/home/a/notes/talk.wav"),
                Path::new("/tmp"),
                Some(Path::new("/home/a/Downloads")),
                false,
            ),
            Some(PathBuf::from("/home/a/Downloads"))
        );
    }

    #[test]
    fn a_file_in_the_temp_folder_sends_the_transcript_to_downloads() {
        for source in [
            "/tmp/hex-drop.m4a",
            "/tmp/TemporaryItems/x/talk.mp3",
            "/tmpfoo/talk.wav",
        ] {
            let expected = if source.starts_with("/tmp/") {
                "/home/a/Downloads"
            } else {
                "/tmpfoo"
            };
            assert_eq!(
                transcript_directory(
                    Path::new(source),
                    Path::new("/tmp"),
                    Some(Path::new("/home/a/Downloads")),
                    true,
                ),
                Some(PathBuf::from(expected)),
                "{source}"
            );
        }
    }

    /// macOS names its temp folder `/tmp` but canonicalizes it to
    /// `/private/tmp`, so a file below it is found there even when its own
    /// folder has not been made yet.
    #[cfg(unix)]
    #[test]
    fn a_temp_folder_reached_through_a_symlink_still_sends_the_transcript_to_downloads() {
        let root = temp_directory("symlinked-temp");
        let real = root.join("real");
        fs::create_dir_all(&real).unwrap();
        let alias = root.join("alias");
        std::os::unix::fs::symlink(&real, &alias).unwrap();
        let downloads = Path::new("/home/a/Downloads");
        assert_eq!(
            transcript_directory(&real.join("talk.wav"), &alias, Some(downloads), true),
            Some(downloads.to_path_buf())
        );
        assert_eq!(
            transcript_directory(
                &alias.join("gone").join("talk.mp3"),
                &alias,
                Some(downloads),
                true
            ),
            Some(downloads.to_path_buf())
        );
        fs::remove_dir_all(root).unwrap();
    }

    /// The check before the save reads the cancel flag once more, so a cancel
    /// that lands while the final piece runs still stops the transcript.
    #[test]
    fn still_wanted_refuses_a_cancelled_file() {
        let cancel = AtomicBool::new(false);
        assert!(still_wanted(&cancel).is_ok());
        cancel.store(true, Ordering::Release);
        assert!(matches!(
            still_wanted(&cancel),
            Err(FileTranscriptError::Cancelled)
        ));
    }

    /// A WAV is read whole once before a model loads, as `hex transcribe`
    /// does, so a truncated file is refused up front instead of after
    /// minutes of transcription.
    #[test]
    fn a_truncated_wav_is_refused_before_anything_is_transcribed() {
        let path = temp_directory("truncated-wav").join("talk.wav");
        write_wav(&path, 16_000);
        let bytes = fs::read(&path).unwrap();
        fs::write(&path, &bytes[..bytes.len() / 2]).unwrap();

        let mut pieces = Pieces::open(&path).unwrap();
        let error = pieces.validate().unwrap_err();
        assert!(matches!(&error, PieceError::Incomplete), "{error:?}");
        assert!(matches!(
            file_error(&path, error),
            FileTranscriptError::Corrupt { .. }
        ));
        fs::remove_dir_all(path.parent().unwrap()).unwrap();
    }

    /// Damage the reader finds partway through names where it is, so a long
    /// file's message points at the part that failed; a WAV the up-front read
    /// refused has no position to name.
    #[test]
    fn damage_found_partway_says_where_it_is() {
        let path = Path::new("talk.m4a");
        let damaged = file_error(path, PieceError::Damaged { at_ms: 754_000 });
        assert_eq!(damaged.to_string(), "talk.m4a is damaged at 12:34");
        let changed = file_error(path, PieceError::Changed { at_ms: 3_723_000 });
        assert_eq!(changed.to_string(), "talk.m4a is damaged at 1:02:03");
        let incomplete = file_error(path, PieceError::Incomplete);
        assert!(matches!(
            incomplete,
            FileTranscriptError::Corrupt { at_ms: None, .. }
        ));
        assert_eq!(incomplete.to_string(), "talk.m4a is truncated or corrupt");
    }

    /// The up-front check rewinds the WAV, so the reader the transcription
    /// uses still starts at the first sample.
    #[test]
    fn a_checked_wav_still_reads_from_its_first_sample() {
        let directory = temp_directory("checked-wav");
        let path = directory.join("talk.wav");
        write_wav(&path, 32_000);

        let mut pieces = Pieces::open(&path).unwrap();
        pieces.validate().unwrap();
        let first = pieces
            .next()
            .unwrap()
            .expect("a checked file still has audio");
        assert!(!first.is_empty(), "the first piece holds its samples");
        assert_eq!(pieces.position_ms(), 2_000);
        fs::remove_dir_all(directory).unwrap();
    }

    /// Every extension this build offers, the name its copy uses for that
    /// kind (`aif` and `aifc` are named AIFF, `oga` OGG, `m4b` M4A, `m4v` and
    /// `3gp` MP4, `weba` WebM, `mka` MKV, `wave` WAV), and the fixture the
    /// shared pieces reader decodes for it.
    const KINDS: &[(&str, &str, &str)] = &[
        ("wav", "WAV", "generated.wav"),
        ("wave", "WAV", "generated.wav"),
        ("mp3", "MP3", "sine.mp3"),
        ("aac", "AAC", "sine-aac.aac"),
        ("m4a", "M4A", "sine-aac.m4a"),
        ("m4b", "M4A", "sine-aac.m4a"),
        ("mp4", "MP4", "sine-video.mp4"),
        ("m4v", "MP4", "sine-aac.m4a"),
        ("3gp", "MP4", "sine-aac.m4a"),
        ("mov", "MOV", "sine-video.mp4"),
        ("aiff", "AIFF", "sine.aiff"),
        ("aif", "AIFF", "sine.aiff"),
        ("aifc", "AIFF", "sine.aiff"),
        ("caf", "CAF", "sine-aac.caf"),
        ("flac", "FLAC", "sine.flac"),
        ("ogg", "OGG", "sine-vorbis.ogg"),
        ("oga", "OGG", "sine-vorbis.ogg"),
        ("opus", "Opus", "sine-opus.ogg"),
        ("webm", "WebM", "sine-opus.webm"),
        ("weba", "WebM", "sine-opus.webm"),
        ("mkv", "MKV", "sine-opus.webm"),
        ("mka", "MKV", "sine-opus.webm"),
    ];

    fn fixture(name: &str) -> PathBuf {
        Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("tests/fixtures/media")
            .join(name)
    }

    #[test]
    fn only_the_formats_this_build_reads_are_accepted() {
        for extension in EXTENSIONS {
            let path = PathBuf::from(format!("talk.{extension}"));
            assert!(extension_accepted(&path), "{extension}");
            let upper = PathBuf::from(format!("talk.{}", extension.to_uppercase()));
            assert!(extension_accepted(&upper), "{extension}");
        }
        // Extensions this build does not offer: nothing here is a container
        // the shared pieces reader decodes.
        for refused in [
            "talk.wma",
            "talk.amr",
            "talk.ac3",
            "talk.txt",
            "talk",
            "talk.mid",
            "talk.wav2",
        ] {
            assert!(!extension_accepted(Path::new(refused)), "{refused}");
        }
    }

    /// The extensions the window's drop target and its dialog offer are named
    /// in its copy, and every one of them is accepted.
    #[test]
    fn every_offered_extension_is_named_in_the_copy() {
        let offered: Vec<(&str, &str)> = KINDS
            .iter()
            .map(|(extension, name, _)| (*extension, *name))
            .collect();
        assert_eq!(
            EXTENSIONS.len(),
            offered.len(),
            "every kind has one extension on offer"
        );
        for (extension, name) in offered {
            assert!(
                EXTENSIONS.contains(&extension),
                "{extension} is named in the copy but not offered"
            );
            assert!(
                FORMATS.contains(name),
                "the copy ({FORMATS}) doesn't name {name}"
            );
            assert!(
                extension_accepted(Path::new(&format!("talk.{extension}"))),
                "{extension} is offered but not accepted"
            );
        }
        for refused in ["wma", "txt", "mid", "wav2"] {
            assert!(!EXTENSIONS.contains(&refused), "{refused}");
        }
    }

    /// Every extension on offer is one the shared pieces reader actually
    /// decodes, read from a fixture under that exact extension.
    #[test]
    fn every_extension_on_offer_reads_a_fixture_under_that_name() {
        let directory = temp_directory("offered");
        for (extension, _, source) in KINDS {
            let path = directory.join(format!("sine.{extension}"));
            if *source == "generated.wav" {
                write_wav(&path, 8_000);
            } else {
                fs::copy(fixture(source), &path).unwrap();
            }
            let mut pieces =
                Pieces::open(&path).unwrap_or_else(|error| panic!("{extension}: {error:?}"));
            pieces
                .validate()
                .unwrap_or_else(|error| panic!("{extension}: {error:?}"));
            let mut samples = 0;
            while let Some(piece) = pieces
                .next()
                .unwrap_or_else(|error| panic!("{extension}: {error:?}"))
            {
                samples += piece.len();
            }
            assert!(
                samples > 1_000,
                "{extension} reads half a second of audio: {samples} samples"
            );
        }
        fs::remove_dir_all(directory).unwrap();
    }

    /// The Labs card, its summary, and the CLI description all name the kinds
    /// this build reads, so no copy contradicts the drop target.
    #[test]
    fn every_labs_line_names_the_formats_this_build_reads() {
        use crate::labs::Feature;
        for copy in [
            Feature::DropTranscribe.summary(),
            Feature::DropTranscribe.description(),
        ] {
            assert!(copy.contains(FORMATS), "{copy}");
        }
    }

    #[test]
    fn an_unsupported_file_is_refused_before_anything_loads() {
        for refused in ["talk.wma", "talk.amr", "talk.ac3", "talk.txt"] {
            let error = check_extension(&Path::new("/nonexistent").join(refused)).unwrap_err();
            let message = error.to_string();
            assert!(message.contains("not a kind of audio file"), "{message}");
            assert!(message.contains(FORMATS), "{message}");
        }
    }

    /// A container with no audio track says so, a codec the reader can't
    /// decode is named, an empty file says it has no audio rather than no
    /// speech, and a WAV it can't take says why, rather than all four reading
    /// as an unreadable file.
    #[test]
    fn a_file_without_audio_and_an_unknown_codec_say_what_is_wrong() {
        let video = fixture("rgbw.mp4");
        let error = Pieces::open(&video).unwrap_err();
        let mapped = file_error(&video, error);
        assert!(
            mapped.to_string().ends_with("has no audio track"),
            "{mapped}"
        );
        assert!(matches!(mapped, FileTranscriptError::NoAudioTrack { .. }));

        let codec = file_error(
            Path::new("/recordings/talk.mp4"),
            PieceError::UnsupportedCodec("AC-3".into()),
        );
        assert!(
            codec.to_string().contains("codec Hex cannot decode (AC-3)"),
            "{codec}"
        );
        assert!(
            matches!(&codec, FileTranscriptError::UnsupportedCodec { codec, .. } if codec == "AC-3")
        );

        let empty = file_error(Path::new("/recordings/silence.wav"), PieceError::Empty);
        assert!(empty.to_string().ends_with("has no audio"), "{empty}");
        assert!(!empty.to_string().contains("No speech"), "{empty}");
        assert!(matches!(empty, FileTranscriptError::NoAudio { .. }));

        let audio = file_error(
            Path::new("/recordings/talk.wav"),
            PieceError::Audio(AudioError::Unsupported),
        );
        assert!(audio.to_string().contains("could not be read"), "{audio}");
        assert!(
            audio.to_string().contains("sample rate or channels"),
            "{audio}"
        );
        assert!(matches!(audio, FileTranscriptError::Unreadable { .. }));
    }

    /// The bundled MP3 decodes through the shared reader at 16 kHz mono, as
    /// every other container does.
    #[test]
    fn the_bundled_mp3_decodes_to_sixteen_kilohertz_mono() {
        let path = Path::new(env!("CARGO_MANIFEST_DIR")).join("resources/audio/funDemo.mp3");
        let mut pieces = Pieces::open(&path).unwrap();
        let total = pieces
            .duration_ms()
            .expect("the mp3 header says how long it is");
        let mut decoded = Vec::new();
        while let Some(piece) = pieces.next().unwrap() {
            assert!(piece.len() <= PIECE_SAMPLES, "a piece is at most 30 s");
            decoded.extend(piece);
        }
        assert!(!decoded.is_empty());
        assert!(
            decoded
                .iter()
                .all(|sample| sample.is_finite() && (-1.0..=1.0).contains(sample)),
            "every sample is mono float audio"
        );
        let expected = total as usize * 16;
        assert!(
            decoded.len().abs_diff(expected) <= 16_000,
            "the whole file is resampled to 16 kHz, not cut short: {} samples for {total} ms",
            decoded.len()
        );
    }
}
