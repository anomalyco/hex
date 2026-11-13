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

use std::collections::VecDeque;
use std::fs::{self, File, OpenOptions};
use std::io::{self, BufReader};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::time::{Duration, Instant};

use rodio::Source;

use super::models::{TranscriptionSelection, is_installed, validate};
use super::pieces::{BLOCK_FRAMES, PieceBuffer, PieceError, PieceSource, Pieces, next_speechful};
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

#[cfg(target_os = "macos")]
const FORMATS: &str = "WAV, MP3, M4A, AIFF, or CAF";
#[cfg(not(target_os = "macos"))]
const FORMATS: &str = "WAV or MP3";

/// The file extensions this build reads, without the dot: the window's drop
/// target, its "Transcribe a file…" dialog, and their copy all name exactly
/// these, and [`format_of`] accepts exactly these.
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
/// far, from 0 to 1, on this thread; `cancel` stops between pieces and while
/// one waits for its transcript and is polled between decode blocks.
pub fn transcribe_audio_file(
    source: &Path,
    selection: TranscriptionSelection,
    cancel: Arc<AtomicBool>,
    mut progress: impl FnMut(f32),
) -> Result<PathBuf, FileTranscriptError> {
    if !crate::labs::drop_transcribe() {
        return Err(FileTranscriptError::Disabled);
    }
    let model = validate(&selection).map_err(model_unavailable)?;
    if !is_installed(model, &selection.language) {
        return Err(model_unavailable(format!(
            "{} is not installed; choose another model or install it",
            model.name
        )));
    }
    let mut decoded = Decoded::open(source)?;
    let total = decoded.total_samples().filter(|total| *total > 0);
    let (mut service, handle) =
        TranscriptionService::start().map_err(|error| transcription_failed(&error))?;
    // The worker runs until every sender is gone; the handle holds one, so
    // keeping it would make the shutdown wait forever after the result.
    let result = transcribe_pieces(
        &mut decoded,
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
    let path = save_transcript(source, &text)?;
    progress(1.0);
    Ok(path)
}

/// Decodes and transcribes every speechful piece in order, joining their
/// text with one space.
fn transcribe_pieces(
    decoded: &mut Decoded,
    handle: &TranscriptionServiceHandle,
    selection: &TranscriptionSelection,
    source: &Path,
    total: Option<usize>,
    cancel: &AtomicBool,
    progress: &mut impl FnMut(f32),
) -> Result<String, FileTranscriptError> {
    let mut texts = Vec::new();
    progress(0.0);
    loop {
        if cancel.load(Ordering::Acquire) {
            return Err(FileTranscriptError::Cancelled);
        }
        let Some(piece) = next_speechful(decoded).map_err(|error| decoded_error(source, error))?
        else {
            break;
        };
        let text = transcribe_piece(handle, selection, piece, cancel)?;
        if !text.is_empty() {
            texts.push(text);
        }
        let fraction = total
            .map(|total| (decoded.position() as f32 / total as f32).clamp(0.0, 1.0))
            .unwrap_or(0.0);
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

/// One dropped file's pieces, whatever format it is.
enum Decoded {
    Wav(Pieces),
    Mp3(Mp3Pieces),
    #[cfg(target_os = "macos")]
    Bridged(Bridged),
}

impl Decoded {
    fn open(path: &Path) -> Result<Self, FileTranscriptError> {
        match format_of(path) {
            Some(Format::Wav) => Pieces::open(path)
                .map(Self::Wav)
                .map_err(|error| decoded_error(path, error)),
            Some(Format::Mp3) => Mp3Pieces::open(path)
                .map(Self::Mp3)
                .map_err(|error| decoded_error(path, error)),
            #[cfg(target_os = "macos")]
            Some(Format::Bridged) => Bridged::open(path).map(Self::Bridged),
            None => Err(FileTranscriptError::UnsupportedFormat {
                path: path.to_path_buf(),
            }),
        }
    }

    fn total_samples(&self) -> Option<usize> {
        match self {
            Self::Wav(pieces) => Some(pieces.total_samples()),
            Self::Mp3(pieces) => pieces.total,
            #[cfg(target_os = "macos")]
            Self::Bridged(bridged) => Some(bridged.pieces.total_samples()),
        }
    }
}

impl PieceSource for Decoded {
    fn next_piece(&mut self) -> Result<Option<Vec<f32>>, PieceError> {
        match self {
            Self::Wav(pieces) => pieces.next_piece(),
            Self::Mp3(pieces) => pieces.next_piece(),
            #[cfg(target_os = "macos")]
            Self::Bridged(bridged) => bridged.pieces.next_piece(),
        }
    }

    fn position(&self) -> usize {
        match self {
            Self::Wav(pieces) => pieces.position(),
            Self::Mp3(pieces) => pieces.position(),
            #[cfg(target_os = "macos")]
            Self::Bridged(bridged) => bridged.pieces.position(),
        }
    }
}

/// The formats this build reads.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum Format {
    Wav,
    Mp3,
    #[cfg(target_os = "macos")]
    Bridged,
}

fn format_of(path: &Path) -> Option<Format> {
    let extension = path.extension()?.to_str()?.to_ascii_lowercase();
    if !EXTENSIONS.contains(&extension.as_str()) {
        return None;
    }
    #[cfg(target_os = "macos")]
    match extension.as_str() {
        "wav" => Some(Format::Wav),
        "mp3" => Some(Format::Mp3),
        _ => Some(Format::Bridged),
    }
    #[cfg(not(target_os = "macos"))]
    match extension.as_str() {
        "wav" => Some(Format::Wav),
        "mp3" => Some(Format::Mp3),
        _ => None,
    }
}

fn decoded_error(path: &Path, error: PieceError) -> FileTranscriptError {
    match error {
        PieceError::Empty => FileTranscriptError::NoSpeech {
            path: path.to_path_buf(),
        },
        PieceError::Incomplete | PieceError::Changed { .. } => FileTranscriptError::Corrupt {
            path: path.to_path_buf(),
        },
        PieceError::Io(_) | PieceError::Audio(_) => FileTranscriptError::Unreadable {
            path: path.to_path_buf(),
        },
    }
}

/// An MP3 read through rodio on every OS: streaming downmix to mono, 16 kHz
/// resampling, and the same 30 s pieces a WAV takes.
struct Mp3Pieces {
    decoder: rodio::Decoder<BufReader<File>>,
    channels: usize,
    resampler: Option<crate::dictation::StreamingResampler>,
    buffer: PieceBuffer,
    ready: VecDeque<Vec<f32>>,
    done: bool,
    /// The file's length at 16 kHz, when its header says.
    total: Option<usize>,
}

impl Mp3Pieces {
    fn open(path: &Path) -> Result<Self, PieceError> {
        let file = File::open(path).map_err(PieceError::Io)?;
        let decoder = rodio::Decoder::new(BufReader::new(file))
            .map_err(|_| PieceError::Audio(AudioError::Invalid))?;
        let channels = usize::from(decoder.channels().get()).max(1);
        let resampler = crate::dictation::StreamingResampler::new(decoder.sample_rate().get())
            .map_err(|_| PieceError::Audio(AudioError::Unsupported))?;
        let total = decoder
            .total_duration()
            .map(|duration| (duration.as_secs_f64() * 16_000.0) as usize)
            .filter(|total| *total > 0);
        Ok(Self {
            decoder,
            channels,
            resampler: Some(resampler),
            buffer: PieceBuffer::default(),
            ready: VecDeque::new(),
            done: false,
            total,
        })
    }

    /// Decodes one block and queues the pieces it completed. The end of the
    /// file flushes the resampler's tail and then the last piece.
    fn fill(&mut self) -> Result<(), PieceError> {
        let wanted = BLOCK_FRAMES * self.channels;
        let mut raw = Vec::with_capacity(wanted);
        while raw.len() < wanted {
            let Some(sample) = self.decoder.next() else {
                break;
            };
            raw.push(sample);
        }
        if raw.is_empty() {
            self.done = true;
            let tail = self
                .resampler
                .take()
                .map(crate::dictation::StreamingResampler::finish)
                .unwrap_or_default();
            self.ready.extend(self.buffer.push(&tail));
            return Ok(());
        }
        let mut mono = Vec::with_capacity(raw.len() / self.channels);
        for frame in raw.chunks_exact(self.channels) {
            let sum: f32 = frame.iter().copied().sum();
            let average = sum / self.channels as f32;
            if !average.is_finite() {
                return Err(PieceError::Audio(AudioError::Invalid));
            }
            mono.push(average);
        }
        let resampled = match self.resampler.as_mut() {
            Some(resampler) => resampler.push(&mono),
            None => mono,
        };
        self.ready.extend(self.buffer.push(&resampled));
        Ok(())
    }
}

impl PieceSource for Mp3Pieces {
    fn next_piece(&mut self) -> Result<Option<Vec<f32>>, PieceError> {
        loop {
            if let Some(piece) = self.ready.pop_front() {
                return Ok(Some(piece));
            }
            if self.done {
                return Ok(self.buffer.take_rest());
            }
            self.fill()?;
        }
    }

    fn position(&self) -> usize {
        self.buffer.position()
    }
}

/// An M4A, AIFF, or CAF file decoded to a temporary 16 kHz mono WAV by the
/// meeting import bridge, then read like any WAV. The temporary file is
/// removed with this.
#[cfg(target_os = "macos")]
struct Bridged {
    pieces: Pieces,
    wav: PathBuf,
}

#[cfg(target_os = "macos")]
impl Bridged {
    fn open(path: &Path) -> Result<Self, FileTranscriptError> {
        let wav = temporary_wav_path();
        if let Err(error) = crate::meeting::import::decode_to_wav(path, &wav) {
            let _ = fs::remove_file(&wav);
            return Err(transcription_failed(error));
        }
        match Pieces::open(&wav) {
            Ok(pieces) => Ok(Self { pieces, wav }),
            Err(error) => {
                let _ = fs::remove_file(&wav);
                Err(decoded_error(path, error))
            }
        }
    }
}

#[cfg(target_os = "macos")]
impl Drop for Bridged {
    fn drop(&mut self) {
        let _ = fs::remove_file(&self.wav);
    }
}

#[cfg(target_os = "macos")]
fn temporary_wav_path() -> PathBuf {
    static SEQUENCE: AtomicU64 = AtomicU64::new(0);
    std::env::temp_dir().join(format!(
        "hex-transcribe-{}-{}.wav",
        std::process::id(),
        SEQUENCE.fetch_add(1, Ordering::Relaxed)
    ))
}

/// Writes `text` beside `source`, or in Downloads when `source`'s own folder
/// can't take it, and returns the file.
fn save_transcript(source: &Path, text: &str) -> Result<PathBuf, FileTranscriptError> {
    let stem = transcript_stem(source);
    let source_directory = parent_directory(source);
    let downloads = crate::app_paths::downloads_dir().ok_or_else(|| {
        FileTranscriptError::WriteFailed("the Downloads folder is unavailable".into())
    })?;
    let directory = transcript_directory(
        source,
        &std::env::temp_dir(),
        &downloads,
        directory_is_writable(&source_directory),
    );
    let contents = format!("{text}\n");
    match write_transcript(&directory, &stem, &contents) {
        Err(FileTranscriptError::WriteFailed(_)) if directory != downloads => {
            write_transcript(&downloads, &stem, &contents)
        }
        result => result,
    }
}

/// Where a transcript for `source` goes: its own folder, unless that folder
/// is unwritable or the file sits in the OS temp folder (a promised drop's
/// copy), in which case `downloads`.
fn transcript_directory(source: &Path, temp: &Path, downloads: &Path, writable: bool) -> PathBuf {
    let directory = parent_directory(source);
    if !writable || inside(&directory, temp) {
        downloads.to_path_buf()
    } else {
        directory
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
/// created and removed, so an unwritable folder is found before decoding
/// waits on it.
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

    fn temp_directory(name: &str) -> PathBuf {
        let directory =
            std::env::temp_dir().join(format!("hex-transcribe-file-{name}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&directory);
        fs::create_dir_all(&directory).unwrap();
        directory
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
                Path::new("/home/a/Downloads"),
                true,
            ),
            PathBuf::from("/home/a/notes")
        );
    }

    #[test]
    fn an_unwritable_folder_sends_the_transcript_to_downloads() {
        assert_eq!(
            transcript_directory(
                Path::new("/home/a/notes/talk.wav"),
                Path::new("/tmp"),
                Path::new("/home/a/Downloads"),
                false,
            ),
            PathBuf::from("/home/a/Downloads")
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
                    Path::new("/home/a/Downloads"),
                    true,
                ),
                PathBuf::from(expected),
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
            transcript_directory(&real.join("talk.wav"), &alias, downloads, true),
            downloads
        );
        assert_eq!(
            transcript_directory(
                &alias.join("gone").join("talk.mp3"),
                &alias,
                downloads,
                true
            ),
            downloads
        );
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn only_the_formats_this_build_reads_are_accepted() {
        for extension in extensions() {
            let path = PathBuf::from(format!("talk.{extension}"));
            assert!(format_of(&path).is_some(), "{extension}");
            let upper = PathBuf::from(format!("talk.{}", extension.to_uppercase()));
            assert_eq!(format_of(&upper), format_of(&path), "{extension}");
        }
        assert_eq!(format_of(Path::new("talk.wav")), Some(Format::Wav));
        assert_eq!(format_of(Path::new("talk.WAV")), Some(Format::Wav));
        assert_eq!(format_of(Path::new("talk.mp3")), Some(Format::Mp3));
        for refused in ["talk.flac", "talk.ogg", "talk.txt", "talk", "talk.mp4"] {
            assert_eq!(format_of(Path::new(refused)), None, "{refused}");
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
        let error = match Decoded::open(Path::new("/nonexistent/talk.flac")) {
            Err(error) => error,
            Ok(_) => panic!("a FLAC file must be refused"),
        };
        let message = error.to_string();
        assert!(message.contains("not a kind of audio file"), "{message}");
        assert!(message.contains(FORMATS), "{message}");
    }

    #[test]
    fn the_bundled_mp3_decodes_to_sixteen_kilohertz_mono() {
        let path = Path::new(env!("CARGO_MANIFEST_DIR")).join("resources/audio/funDemo.mp3");
        let mut pieces = Mp3Pieces::open(&path).unwrap();
        let total = pieces.total.expect("the mp3 header says how long it is");
        let mut decoded = Vec::new();
        while let Some(piece) = pieces.next_piece().unwrap() {
            assert!(
                piece.len() <= super::super::pieces::PIECE_SAMPLES,
                "a piece is at most 30 s"
            );
            decoded.extend(piece);
        }
        assert!(!decoded.is_empty());
        assert!(
            decoded
                .iter()
                .all(|sample| sample.is_finite() && (-1.0..=1.0).contains(sample)),
            "every sample is mono float audio"
        );
        assert_eq!(
            decoded.len(),
            total,
            "the whole file is resampled to 16 kHz, not cut short"
        );
        assert_eq!(pieces.position(), total);
    }
}
