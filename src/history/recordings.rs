//! Labs › Keep recordings: a dictation saved to History keeps its 16 kHz
//! mono audio as an owner-only 16-bit WAV in `history-audio/<id>.wav`
//! beside the store, so History can transcribe it again with another model.
//!
//! Audio exists only while the toggle is on as a capture is handed to the
//! engine ([`Recording::keep`]). The store owns every file: an entry that
//! leaves History (retention, the entry and text caps, Delete, Clear all)
//! loses its file at the next save, and the recordings have their own byte
//! cap, past which the oldest entries keep their text but lose their audio.

use std::fs;
use std::io::{self, BufReader, BufWriter};
use std::path::{Path, PathBuf};
use std::time::Duration;

use hound::{SampleFormat, WavSpec, WavWriter};

use crate::transcription::models::TranscriptionSelection;
use crate::transcription::service::{
    AudioClip, AudioError, TranscriptionService, TranscriptionServiceError,
};

/// Every recording together stays under this; about 4.5 hours of audio.
pub const MAX_RECORDING_BYTES: u64 = 512 * 1024 * 1024;
const SAMPLE_RATE: u32 = 16_000;
const DIRECTORY: &str = "history-audio";
/// How long one Transcribe again may run, model load included.
const RETRANSCRIBE_DEADLINE: Duration = Duration::from_secs(10 * 60);

/// One dictation's 16 kHz mono audio, kept for History.
#[derive(Clone, Debug)]
pub struct Recording(Vec<f32>);

impl Recording {
    /// A copy of `samples` while Labs › Keep recordings is on; nothing
    /// otherwise, so dictation audio is never kept by default.
    pub fn keep(samples: &[f32]) -> Option<Self> {
        (crate::labs::keep_recordings() && !samples.is_empty()).then(|| Self(samples.to_vec()))
    }

    #[cfg(test)]
    pub fn from_samples(samples: Vec<f32>) -> Self {
        Self(samples)
    }
}

/// The recordings folder beside the history file at `history`.
pub(super) fn directory(history: &Path) -> PathBuf {
    history.with_file_name(DIRECTORY)
}

pub(super) fn path(directory: &Path, id: u64) -> PathBuf {
    directory.join(format!("{id}.wav"))
}

/// Writes entry `id`'s audio atomically and owner-only; returns its size.
pub(super) fn write(directory: &Path, id: u64, recording: &Recording) -> io::Result<u64> {
    fs::create_dir_all(directory)?;
    restrict_directory(directory)?;
    let target = path(directory, id);
    let temporary = target.with_extension("wav.tmp");
    let written = (|| {
        let file = fs::File::create(&temporary)?;
        super::restrict_to_owner(&file)?;
        let spec = WavSpec {
            channels: 1,
            sample_rate: SAMPLE_RATE,
            bits_per_sample: 16,
            sample_format: SampleFormat::Int,
        };
        let mut writer = WavWriter::new(BufWriter::new(&file), spec).map_err(io::Error::other)?;
        for sample in &recording.0 {
            let sample = (sample.clamp(-1.0, 1.0) * f32::from(i16::MAX)).round() as i16;
            writer.write_sample(sample).map_err(io::Error::other)?;
        }
        writer.finalize().map_err(io::Error::other)?;
        file.sync_all()?;
        fs::rename(&temporary, &target)?;
        Ok(fs::metadata(&target)?.len())
    })();
    if written.is_err() {
        let _ = fs::remove_file(&temporary);
    }
    written
}

/// Deletes every recording below `below` (IDs this store has handed out or
/// read) that `keep` rejects. Higher IDs belong to another process's record
/// still on its way to the file, so they are left alone.
pub(super) fn sweep(directory: &Path, below: u64, keep: impl Fn(u64) -> bool) {
    let Ok(files) = fs::read_dir(directory) else {
        return;
    };
    for file in files.flatten() {
        let name = file.file_name();
        let Some(name) = name.to_str() else { continue };
        let Some(id) = name
            .strip_suffix(".wav")
            .or_else(|| name.strip_suffix(".wav.tmp"))
            .and_then(|id| id.parse::<u64>().ok())
        else {
            continue;
        };
        if id < below
            && (name.ends_with(".tmp") || !keep(id))
            && let Err(error) = fs::remove_file(file.path())
        {
            tracing::warn!(%error, id, "could not delete a dictation recording");
        }
    }
}

/// Transcribes the recording at `path` again with `selection`, on a
/// transcription service of its own: it runs on the dictation engine's model
/// when that is the one asked for, yielding to dictation, or loads its own
/// copy for this run. Blocks; call it off the UI thread.
pub fn retranscribe(path: &Path, selection: TranscriptionSelection) -> Result<String, String> {
    let file = fs::File::open(path).map_err(|_| "The recording is gone.".to_string())?;
    let clip = AudioClip::decode_wav(BufReader::new(file)).map_err(|error| match error {
        AudioError::Invalid | AudioError::Unsupported => "The recording could not be read.",
        AudioError::ResourceExhausted => "The recording is too long.",
    })?;
    let (mut service, handle) =
        TranscriptionService::start().map_err(|error| format!("{error:#}"))?;
    let admission = handle
        .admit_audio()
        .ok_or_else(|| "Another transcription is running.".to_string())?;
    let result = handle
        .transcribe(selection, clip, admission)
        .map_err(describe)
        .and_then(|job| match job.wait_timeout(RETRANSCRIBE_DEADLINE) {
            Ok(Some(result)) => result.map_err(describe),
            Ok(None) => Err("Transcribing took too long.".into()),
            Err(error) => Err(describe(error)),
        });
    // The worker runs until every sender is gone; the handle holds one, so
    // keeping it would make the shutdown wait forever after the result.
    drop(handle);
    service.shutdown();
    result.map(|text| text.trim().to_string())
}

fn describe(error: TranscriptionServiceError) -> String {
    match error {
        TranscriptionServiceError::QueueFull => "Another transcription is running.".into(),
        TranscriptionServiceError::Unavailable => "Transcription stopped.".into(),
        TranscriptionServiceError::Cancelled => "Transcription was cancelled.".into(),
        TranscriptionServiceError::Model(error) | TranscriptionServiceError::Inference(error) => {
            error
        }
    }
}

#[cfg(unix)]
fn restrict_directory(directory: &Path) -> io::Result<()> {
    use std::os::unix::fs::PermissionsExt;
    fs::set_permissions(directory, fs::Permissions::from_mode(0o700))
}

#[cfg(not(unix))]
fn restrict_directory(_directory: &Path) -> io::Result<()> {
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::transcription::models::{CustomModelId, TranscriptionModelId};

    #[test]
    fn transcribe_again_returns_once_the_run_ends() {
        let directory = std::env::temp_dir().join(format!(
            "hex-retranscribe-{}-{}",
            std::process::id(),
            crate::history::now_ms()
        ));
        write(
            &directory,
            1,
            &Recording::from_samples(vec![0.0; SAMPLE_RATE as usize]),
        )
        .unwrap();
        let recording = path(&directory, 1);
        // A model that is not on this computer fails at once, without loading
        // anything; the answer must still come back (#1089).
        let selection = TranscriptionSelection {
            model: TranscriptionModelId::Custom(
                CustomModelId::parse("custom-0000000000000000").unwrap(),
            ),
            language: "en".into(),
        };
        let (sender, answer) = std::sync::mpsc::channel();
        std::thread::spawn(move || {
            let _ = sender.send(retranscribe(&recording, selection));
        });
        let answered = answer.recv_timeout(Duration::from_secs(60));
        let _ = fs::remove_dir_all(&directory);
        assert!(
            answered.expect("Transcribe again never answered").is_err(),
            "a missing model cannot transcribe"
        );
    }
}
