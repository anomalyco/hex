//! `hex transcribe <file>` (Labs › Transcribe files): a saved WAV through
//! the running Hex's local API, or in this process on Linux and Windows when
//! Hex is stopped. Nothing is pasted, saved to History, or processed.

use std::io::Cursor;
use std::path::Path;
use std::time::Duration;

use serde::Deserialize;

use super::{Output, print};
use crate::control::{ControlReply, model_selection};
use crate::labs::Feature;
use crate::transcription::models::TranscriptionSelection;
use crate::transcription::service::{AudioClip, AudioError};

/// A whole file waits for a cold model and a long clip.
const TIMEOUT: Duration = Duration::from_secs(600);
const MAX_RESPONSE: usize = 1024 * 1024;

#[derive(Deserialize)]
struct Transcript {
    transcript: String,
}

/// What `hex transcribe` sends once its file and model are settled.
#[derive(Debug)]
struct Job {
    wav: Vec<u8>,
    file: std::path::PathBuf,
    selection: TranscriptionSelection,
}

pub(super) fn run(
    file: std::path::PathBuf,
    model: Option<String>,
    language: Option<String>,
    output: Output,
) -> u8 {
    let settings = match crate::app_settings::AppSettings::read() {
        Ok(settings) => settings,
        Err(error) => {
            return print(
                ControlReply::error("failed", format!("couldn't read Hex's settings: {error:#}")),
                output,
            );
        }
    };
    let reply = match prepare(&settings, &file, model, language) {
        Ok(job) => transcribe(&job),
        Err(reply) => reply,
    };
    print(reply, output)
}

/// The lab's gate, the file, and the model, all checked before Hex is asked.
fn prepare(
    settings: &crate::app_settings::AppSettings,
    file: &Path,
    model: Option<String>,
    language: Option<String>,
) -> std::result::Result<Job, ControlReply> {
    if !Feature::TranscribeFiles.on(settings) {
        return Err(ControlReply::error(
            "disabled",
            "Labs › Transcribe files is off; turn it on with `hex labs on transcribe-files`",
        ));
    }
    let wav = read_wav(file)?;
    let model = model.unwrap_or_else(|| settings.transcription.model.as_str().to_owned());
    let selection = model_selection(settings, &model, language.as_deref())?;
    Ok(Job {
        wav,
        file: file.to_path_buf(),
        selection,
    })
}

/// The whole file in memory, so the size cap and WAV validation come before
/// anything is sent.
fn read_wav(path: &Path) -> std::result::Result<Vec<u8>, ControlReply> {
    let name = path.display();
    let metadata = std::fs::metadata(path)
        .map_err(|error| ControlReply::error("failed", format!("couldn't read {name}: {error}")))?;
    if metadata.len() > crate::local_api::MAX_AUDIO_BYTES as u64 {
        let limit = crate::local_api::MAX_AUDIO_BYTES / (1024 * 1024);
        return Err(ControlReply::error(
            "resource-exhausted",
            format!("{name} is larger than {limit} MiB; transcribe a shorter or lower-rate WAV"),
        ));
    }
    let wav = std::fs::read(path)
        .map_err(|error| ControlReply::error("failed", format!("couldn't read {name}: {error}")))?;
    match AudioClip::decode_wav(Cursor::new(&wav)) {
        Ok(_) => Ok(wav),
        Err(AudioError::ResourceExhausted) => Err(ControlReply::error(
            "resource-exhausted",
            format!("{name} is too long for Hex to transcribe at once"),
        )),
        Err(_) => Err(ControlReply::error(
            "unsupported",
            format!(
                "{name} isn't a WAV file Hex can read; convert it to WAV first, \
                 for example with ffmpeg: ffmpeg -i <file> <file>.wav"
            ),
        )),
    }
}

fn transcribe(job: &Job) -> ControlReply {
    if crate::local_api::running_pid().is_some() {
        return in_running_hex(job);
    }
    offline(&job.file, &job.selection)
}

/// The running Hex transcribes on its dictation engine, whatever model it
/// has loaded or can load.
fn in_running_hex(job: &Job) -> ControlReply {
    let query = url::form_urlencoded::Serializer::new(String::new())
        .append_pair("model", job.selection.model.as_str())
        .append_pair("language", &job.selection.language)
        .finish();
    let path = format!("/transcriptions?{query}");
    match crate::local_api::post_wav(&path, &job.wav, TIMEOUT, MAX_RESPONSE) {
        Ok((200, body)) => transcript(&body),
        Ok((status, body)) => {
            let code = crate::local_api::error_code(status, &body);
            refusal(status, &code, job.selection.model.as_str())
        }
        Err(error) => ControlReply::error("transport", format!("{error:#}")),
    }
}

fn transcript(body: &[u8]) -> ControlReply {
    match serde_json::from_slice::<Transcript>(body) {
        Ok(Transcript { transcript }) if transcript.trim().is_empty() => no_speech(),
        Ok(Transcript { transcript }) => ControlReply::Transcript { text: transcript },
        Err(_) => ControlReply::error("failed", "Hex sent an unreadable transcript"),
    }
}

fn refusal(status: u16, code: &str, model: &str) -> ControlReply {
    match status {
        401 => ControlReply::error("failed", "Hex rejected the request; restart Hex"),
        408 => ControlReply::error("failed", "Hex timed out reading the file; try again"),
        409 => ControlReply::error(
            "model-unavailable",
            format!("`{model}` isn't ready in Hex; run `hex model install {model}`"),
        ),
        413 => ControlReply::error(
            "resource-exhausted",
            "Hex says the file is too large to transcribe",
        ),
        429 => ControlReply::error("busy", "Hex is busy with another transcription; try again"),
        400 | 415 => ControlReply::error(
            "unsupported",
            "Hex couldn't read the file as WAV; convert it to WAV first",
        ),
        _ if code.is_empty() => {
            ControlReply::error("failed", format!("Hex answered HTTP {status}"))
        }
        _ => ControlReply::error("failed", format!("Hex answered HTTP {status} ({code})")),
    }
}

fn no_speech() -> ControlReply {
    ControlReply::error(
        "no-speech",
        "no speech was heard in the file; nothing to print",
    )
}

/// Without a running Hex, Linux and Windows load the model here as
/// `hex model transcribe` does; macOS runs models only inside the app.
#[cfg(any(target_os = "linux", target_os = "windows"))]
fn offline(file: &Path, selection: &TranscriptionSelection) -> ControlReply {
    match crate::transcription::local_transcriber::transcribe_wav_with_selection(file, selection) {
        Ok(text) if text.trim().is_empty() => no_speech(),
        Ok(text) => ControlReply::Transcript { text },
        Err(error) => ControlReply::error("failed", format!("couldn't transcribe it: {error:#}")),
    }
}

#[cfg(target_os = "macos")]
fn offline(_: &Path, _: &TranscriptionSelection) -> ControlReply {
    super::not_running()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::app_settings::AppSettings;

    fn not_a_wav() -> std::path::PathBuf {
        let path = std::env::temp_dir().join(format!(
            "hex-transcribe-not-a-wav-{}.wav",
            std::process::id()
        ));
        std::fs::write(&path, b"this is not a wav").unwrap();
        path
    }

    fn wav() -> Vec<u8> {
        let samples = [0_i16; 160];
        let data = (samples.len() * 2) as u32;
        let mut bytes = Vec::from(*b"RIFF");
        bytes.extend_from_slice(&(36 + data).to_le_bytes());
        bytes.extend_from_slice(b"WAVEfmt ");
        bytes.extend_from_slice(&16_u32.to_le_bytes());
        bytes.extend_from_slice(&1_u16.to_le_bytes());
        bytes.extend_from_slice(&1_u16.to_le_bytes());
        bytes.extend_from_slice(&16_000_u32.to_le_bytes());
        bytes.extend_from_slice(&(16_000_u32 * 2).to_le_bytes());
        bytes.extend_from_slice(&2_u16.to_le_bytes());
        bytes.extend_from_slice(&16_u16.to_le_bytes());
        bytes.extend_from_slice(b"data");
        bytes.extend_from_slice(&data.to_le_bytes());
        for sample in samples {
            bytes.extend_from_slice(&sample.to_le_bytes());
        }
        bytes
    }

    #[test]
    fn the_lab_off_refuses_and_names_its_toggle() {
        let reply = prepare(
            &AppSettings::default(),
            Path::new("recording.wav"),
            None,
            None,
        )
        .unwrap_err();
        assert!(
            matches!(reply, ControlReply::Error { ref code, ref message }
                if code == "disabled" && message.contains("hex labs on transcribe-files")),
            "{reply:?}"
        );
    }

    #[test]
    fn a_file_that_is_not_wav_is_refused_before_any_transcription() {
        let mut settings = AppSettings::default();
        settings.labs.transcribe_files = true;
        let path = not_a_wav();
        let reply = prepare(&settings, &path, None, None).unwrap_err();
        let _ = std::fs::remove_file(&path);
        assert!(
            matches!(reply, ControlReply::Error { ref code, .. } if code == "unsupported"),
            "{reply:?}"
        );
    }

    #[test]
    fn a_wav_uses_the_selected_model_and_language() {
        let mut settings = AppSettings::default();
        settings.labs.transcribe_files = true;
        let path =
            std::env::temp_dir().join(format!("hex-transcribe-wav-{}.wav", std::process::id()));
        std::fs::write(&path, wav()).unwrap();
        let job = prepare(&settings, &path, None, None).unwrap();
        let _ = std::fs::remove_file(&path);
        assert_eq!(job.selection.model, settings.transcription.model);
        assert_eq!(job.selection.language, settings.transcription.language);
    }

    #[test]
    fn an_empty_transcript_is_no_speech() {
        assert!(matches!(
            transcript(br#"{"transcript":"  ","durationMs":1000}"#),
            ControlReply::Error { ref code, .. } if code == "no-speech"
        ));
    }
}
