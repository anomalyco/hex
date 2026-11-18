//! Labs › Transcribe dropped audio: a WAV or MP3 file dropped on the window
//! (any OS), or an M4A, AIFF, or CAF file on macOS, is read in pieces of at
//! most 30 s, transcribed with the selected model in this process, and saved
//! as `<stem>.txt` beside it. Nothing is pasted, saved to History, or
//! processed.
//!
//! A file's own folder takes the transcript unless it is unwritable or the
//! file sits in the OS temp folder, where a promised drop's copy lands: then
//! the transcript goes to Downloads. A taken name gets " 2", " 3", and so on,
//! and the write itself is atomic, so a transcript Hex cannot finish never
//! takes the name.

use std::fs::{self, OpenOptions};
use std::io;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::time::{Duration, Instant};

use super::models::{TranscriptionSelection, is_installed, validate};
use super::pieces::{PieceError, Pieces};
use super::service::{
    AudioClip, TranscriptionService, TranscriptionServiceError, TranscriptionServiceHandle,
};
use super::transcript::TimestampLevel;

/// One piece waits for a cold model behind it and for its own transcription.
const PIECE_DEADLINE: Duration = Duration::from_secs(10 * 60);
/// How often a waiting piece checks for cancellation.
const CANCEL_POLL: Duration = Duration::from_millis(100);
/// How many taken names one transcript tries before giving up.
const MAX_ATTEMPTS: usize = 1_000;

#[cfg(target_os = "macos")]
const FORMATS: &str = "WAV, MP3, M4A, AIFF, or CAF";
#[cfg(not(target_os = "macos"))]
const FORMATS: &str = "WAV or MP3";

/// The file extensions this build reads, without the dot: the window's drop
/// target, its "Transcribe a file…" dialog, and their copy all name exactly
/// these, and [`extension_accepted`] accepts exactly these.
#[cfg(target_os = "macos")]
const EXTENSIONS: &[&str] = &["wav", "mp3", "m4a", "aiff", "aif", "caf"];
#[cfg(not(target_os = "macos"))]
const EXTENSIONS: &[&str] = &["wav", "mp3"];

/// Every extension this build reads, without the dot.
pub fn extensions() -> &'static [&'static str] {
    EXTENSIONS
}

/// Why a dropped file could not become a transcript.
#[derive(Debug)]
pub enum FileTranscriptError {
    /// Labs › Transcribe dropped audio is off.
    Disabled,
    /// The extension is not one this build reads.
    UnsupportedFormat { path: PathBuf },
    /// The file could not be opened, or its audio format is not supported.
    Unreadable { path: PathBuf },
    /// The file is truncated or changed while Hex read it.
    Corrupt { path: PathBuf },
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
            Self::Unreadable { path } => {
                write!(formatter, "{} could not be read", path.display())
            }
            Self::Corrupt { path } => {
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
    // as it transcribes, and damage found later names where it is.
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
    let Some(admission) = handle.admit_audio() else {
        return Err(transcription_failed("Another transcription is running"));
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
        PieceError::Empty => FileTranscriptError::NoSpeech {
            path: path.to_path_buf(),
        },
        PieceError::Incomplete | PieceError::Changed { .. } | PieceError::Damaged { .. } => {
            FileTranscriptError::Corrupt {
                path: path.to_path_buf(),
            }
        }
        PieceError::Io(_)
        | PieceError::Audio(_)
        | PieceError::NotMedia
        | PieceError::NoAudioTrack
        | PieceError::UnsupportedCodec(_) => FileTranscriptError::Unreadable {
            path: path.to_path_buf(),
        },
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

    /// A cancel that lands while the last piece runs stops the file before
    /// anything is written, so a cancelled drop never leaves a transcript.
    #[test]
    fn a_cancel_after_the_last_piece_stops_the_save() {
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

    #[test]
    fn only_the_formats_this_build_reads_are_accepted() {
        for extension in extensions() {
            let path = PathBuf::from(format!("talk.{extension}"));
            assert!(extension_accepted(&path), "{extension}");
            let upper = PathBuf::from(format!("talk.{}", extension.to_uppercase()));
            assert!(extension_accepted(&upper), "{extension}");
        }
        for refused in ["talk.flac", "talk.ogg", "talk.txt", "talk", "talk.mp4"] {
            assert!(!extension_accepted(Path::new(refused)), "{refused}");
        }
    }

    /// The extensions the window's drop target and its dialog offer are the
    /// ones this build decodes, and its copy names exactly those.
    #[test]
    fn the_extensions_on_offer_are_the_ones_this_build_reads() {
        for extension in extensions() {
            assert!(
                FORMATS
                    .to_ascii_uppercase()
                    .contains(&extension.to_uppercase()),
                "{extension} is readable but the copy ({FORMATS}) doesn't name it"
            );
        }
        for refused in ["flac", "ogg", "txt", "mp4", "wav2"] {
            assert!(!extensions().contains(&refused), "{refused}");
        }
    }

    #[test]
    fn an_unsupported_file_is_refused_before_anything_loads() {
        let error = check_extension(Path::new("/nonexistent/talk.flac")).unwrap_err();
        let message = error.to_string();
        assert!(message.contains("not a kind of audio file"), "{message}");
        assert!(message.contains(FORMATS), "{message}");
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
