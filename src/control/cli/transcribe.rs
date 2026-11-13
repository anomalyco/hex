//! `hex transcribe <file>` (Labs › Transcribe files): a saved WAV through
//! the running Hex's local API, or in this process on Linux and Windows when
//! Hex is stopped. Nothing is pasted, saved to History, or processed.
//!
//! A file of any length is read in pieces of at most 30 s ([`pieces`]) and
//! their text is joined with one space: a piece that short runs on the
//! running Hex's dictation engine, never needs a second model copy loaded,
//! and keeps one piece rather than the whole file in memory.

mod pieces;

use std::path::{Path, PathBuf};
use std::time::Duration;

use serde::Deserialize;

use super::{Output, print};
use crate::control::{ControlReply, model_selection};
use crate::labs::Feature;
use crate::transcription::models::TranscriptionSelection;
use pieces::{PieceError, Pieces};

/// One piece waits for a cold model behind it and for its own transcription.
const TIMEOUT: Duration = Duration::from_secs(600);
const MAX_RESPONSE: usize = 1024 * 1024;

#[derive(Deserialize)]
struct Transcript {
    transcript: String,
}

/// What `hex transcribe` sends once its file and model are settled.
#[derive(Debug)]
struct Job {
    file: PathBuf,
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

/// The lab's gate, the file's header, and the model, all checked before Hex
/// is asked or a model is loaded. The samples are read piece by piece later.
fn prepare(
    settings: &crate::app_settings::AppSettings,
    file: &Path,
    model: Option<String>,
    language: Option<String>,
) -> std::result::Result<Job, ControlReply> {
    if !Feature::TranscribeFiles.on(settings) {
        let message = match Feature::TranscribeFiles.managed() {
            Some(lock) => lock.refusal().to_owned(),
            None => {
                "Labs › Transcribe files is off; turn it on with `hex labs on transcribe-files`"
                    .to_owned()
            }
        };
        return Err(ControlReply::error("disabled", message));
    }
    // The header and format are refused here, before anything runs; this
    // reads no samples.
    open(file)?;
    let model = model.unwrap_or_else(|| settings.transcription.model.as_str().to_owned());
    let selection = model_selection(settings, &model, language.as_deref())?;
    Ok(Job {
        file: file.to_path_buf(),
        selection,
    })
}

fn open(file: &Path) -> std::result::Result<Pieces, ControlReply> {
    Pieces::open(file).map_err(|error| refused(file, error))
}

/// The refusal for a file Hex cannot read, before any transcription runs.
fn refused(file: &Path, error: PieceError) -> ControlReply {
    let name = file.display();
    match error {
        PieceError::Io(error) => {
            ControlReply::error("failed", format!("couldn't read {name}: {error}"))
        }
        PieceError::Audio(_) => ControlReply::error(
            "unsupported",
            format!(
                "{name} isn't a WAV file Hex can read; convert it to WAV first, \
                 for example with ffmpeg: ffmpeg -i <file> <file>.wav"
            ),
        ),
    }
}

fn transcribe(job: &Job) -> ControlReply {
    if crate::local_api::running_pid().is_some() {
        return in_running_hex(job);
    }
    offline(job)
}

/// The running Hex transcribes one piece at a time with the model the
/// request names: a piece of at most 30 s runs on its dictation engine's
/// loaded copy, so no second copy loads and dictation can run between pieces.
fn in_running_hex(job: &Job) -> ControlReply {
    let query = url::form_urlencoded::Serializer::new(String::new())
        .append_pair("model", job.selection.model.as_str())
        .append_pair("language", &job.selection.language)
        .finish();
    let path = format!("/transcriptions?{query}");
    let mut pieces = match open(&job.file) {
        Ok(pieces) => pieces,
        Err(reply) => return reply,
    };
    let mut texts = Vec::new();
    loop {
        let samples = match pieces.next() {
            Ok(Some(samples)) => samples,
            Ok(None) => break,
            Err(error) => return refused(&job.file, error),
        };
        let wav = match pieces::wav_bytes(&samples) {
            Ok(wav) => wav,
            Err(error) => return refused(&job.file, error),
        };
        match crate::local_api::post_wav(&path, &wav, TIMEOUT, MAX_RESPONSE) {
            Ok((200, body)) => match transcript(&body) {
                // A piece that heard no speech is skipped: only a file with
                // nothing to say answers no-speech.
                Ok(text) if text.is_empty() => {}
                Ok(text) => texts.push(text),
                Err(reply) => return reply,
            },
            Ok((status, body)) => {
                let code = crate::local_api::error_code(status, &body);
                return refusal(status, &code, job.selection.model.as_str());
            }
            Err(error) => return ControlReply::error("transport", format!("{error:#}")),
        }
    }
    joined(texts)
}

/// One piece's transcript, trimmed, or the refusal for an answer Hex cannot
/// read.
fn transcript(body: &[u8]) -> std::result::Result<String, ControlReply> {
    match serde_json::from_slice::<Transcript>(body) {
        Ok(Transcript { transcript }) => Ok(transcript.trim().to_owned()),
        Err(_) => Err(ControlReply::error(
            "failed",
            "Hex sent an unreadable transcript",
        )),
    }
}

/// Every piece's text in order, trimmed and joined with one space; a file
/// whose pieces all heard nothing is `no-speech`, as an empty transcript was.
fn joined(texts: Vec<String>) -> ControlReply {
    let text = texts
        .iter()
        .map(|text| text.trim())
        .filter(|text| !text.is_empty())
        .collect::<Vec<_>>()
        .join(" ");
    if text.is_empty() {
        return no_speech();
    }
    ControlReply::Transcript { text }
}

fn refusal(status: u16, code: &str, model: &str) -> ControlReply {
    match code {
        "unknown-model" => ControlReply::error(
            "unknown-model",
            format!("the running Hex doesn't know `{model}`; run `hex model list`"),
        ),
        "unsupported-model" => ControlReply::error(
            "model-unavailable",
            format!("the running Hex can't use `{model}`; run `hex model list` to see what it has"),
        ),
        "invalid-request" => ControlReply::error("failed", "Hex refused the request; update Hex"),
        "unsupported-audio" | "invalid-audio" => ControlReply::error(
            "unsupported",
            "Hex couldn't read the file as WAV; convert it to WAV first",
        ),
        "length-required" => ControlReply::error("failed", "Hex refused the upload; update Hex"),
        "resource-exhausted" => ControlReply::error(
            "resource-exhausted",
            "Hex says the file is too large or too long to transcribe",
        ),
        "model-not-ready" => ControlReply::error("model-unavailable", not_ready(model)),
        "queue-full" => {
            ControlReply::error("busy", "Hex is busy with another transcription; try again")
        }
        "upload-timeout" => {
            ControlReply::error("failed", "Hex timed out reading the file; try again")
        }
        "cancelled" => ControlReply::error("failed", "Hex cancelled the transcription; try again"),
        "service-gone" => ControlReply::error(
            "unavailable",
            "Hex's transcription service stopped; restart Hex",
        ),
        "model-load-failed" => ControlReply::error(
            "failed",
            format!("Hex couldn't load `{model}`; try again or pick another model"),
        ),
        "transcription-failed" => {
            ControlReply::error("failed", "Hex couldn't transcribe it; try again")
        }
        _ => match status {
            401 => ControlReply::error("failed", "Hex rejected the request; restart Hex"),
            408 => ControlReply::error("failed", "Hex timed out reading the file; try again"),
            409 => ControlReply::error("model-unavailable", not_ready(model)),
            413 => ControlReply::error(
                "resource-exhausted",
                "Hex says the file is too large to transcribe",
            ),
            415 => ControlReply::error(
                "unsupported",
                "Hex couldn't read the file as WAV; convert it to WAV first",
            ),
            429 => ControlReply::error("busy", "Hex is busy with another transcription; try again"),
            _ if code.is_empty() || code.starts_with("http-") => {
                ControlReply::error("failed", format!("Hex answered HTTP {status}"))
            }
            _ => ControlReply::error("failed", format!("Hex answered HTTP {status} ({code})")),
        },
    }
}

/// An imported model has no `hex model install` id, so it is checked where it
/// was imported instead.
fn not_ready(model: &str) -> String {
    if model.starts_with("custom-") {
        format!("`{model}` isn't ready in the running Hex; check it in Hex's model picker")
    } else {
        format!("`{model}` isn't ready in the running Hex; run `hex model install {model}`")
    }
}

fn no_speech() -> ControlReply {
    ControlReply::error(
        "no-speech",
        "no speech was heard in the file; nothing to print",
    )
}

/// Without a running Hex, Linux and Windows load the model here once and run
/// every piece through it as `hex model transcribe` runs a file; macOS runs
/// models only inside the app.
#[cfg(any(target_os = "linux", target_os = "windows"))]
fn offline(job: &Job) -> ControlReply {
    use crate::transcription::local_transcriber::LocalTranscriber;

    let mut pieces = match open(&job.file) {
        Ok(pieces) => pieces,
        Err(reply) => return reply,
    };
    let mut transcriber = match LocalTranscriber::load(&job.selection) {
        Ok(transcriber) => transcriber,
        Err(error) => {
            return ControlReply::error("failed", format!("couldn't transcribe it: {error:#}"));
        }
    };
    let mut texts = Vec::new();
    loop {
        let samples = match pieces.next() {
            Ok(Some(samples)) => samples,
            Ok(None) => break,
            Err(error) => return refused(&job.file, error),
        };
        match transcriber.transcribe_clip(samples) {
            Ok(text) if text.trim().is_empty() => {}
            Ok(text) => texts.push(text),
            Err(error) => {
                return ControlReply::error("failed", format!("couldn't transcribe it: {error:#}"));
            }
        }
    }
    joined(texts)
}

#[cfg(target_os = "macos")]
fn offline(_: &Job) -> ControlReply {
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
        assert_eq!(job.file, path);
        assert_eq!(job.selection.model, settings.transcription.model);
        assert_eq!(job.selection.language, settings.transcription.language);
    }

    #[test]
    fn a_file_with_no_audio_is_refused_like_an_unreadable_one() {
        let mut settings = AppSettings::default();
        settings.labs.transcribe_files = true;
        let path =
            std::env::temp_dir().join(format!("hex-transcribe-empty-{}.wav", std::process::id()));
        let spec = hound::WavSpec {
            channels: 1,
            sample_rate: 16_000,
            bits_per_sample: 16,
            sample_format: hound::SampleFormat::Int,
        };
        hound::WavWriter::create(&path, spec)
            .unwrap()
            .finalize()
            .unwrap();
        let reply = prepare(&settings, &path, None, None).unwrap_err();
        let _ = std::fs::remove_file(&path);
        assert!(
            matches!(reply, ControlReply::Error { ref code, .. } if code == "unsupported"),
            "{reply:?}"
        );
    }

    #[test]
    fn every_piece_hearing_nothing_is_no_speech() {
        assert!(matches!(
            joined(Vec::new()),
            ControlReply::Error { ref code, .. } if code == "no-speech"
        ));
        assert!(matches!(
            joined(vec![String::new(), "  ".into()]),
            ControlReply::Error { ref code, .. } if code == "no-speech"
        ));
    }

    #[test]
    fn pieces_join_with_one_space_in_order() {
        let reply = joined(vec![" one ".into(), "two".into()]);
        assert!(
            matches!(reply, ControlReply::Transcript { ref text } if text == "one two"),
            "{reply:?}"
        );
    }
    fn error(reply: ControlReply) -> (String, String) {
        match reply {
            ControlReply::Error { code, message } => (code, message),
            other => panic!("expected an error, got {other:?}"),
        }
    }

    #[test]
    fn a_refusal_follows_hexs_code_rather_than_its_status() {
        let (code, message) = error(refusal(415, "unsupported-audio", "parakeet_v2"));
        assert_eq!(code, "unsupported");
        assert!(
            message.contains("couldn't read the file as WAV"),
            "{message}"
        );

        let (code, message) = error(refusal(400, "invalid-audio", "parakeet_v2"));
        assert_eq!(code, "unsupported");
        assert!(
            message.contains("couldn't read the file as WAV"),
            "{message}"
        );

        let (code, message) = error(refusal(400, "unknown-model", "parakeet_v2"));
        assert_eq!(code, "unknown-model");
        assert!(message.contains("hex model list"), "{message}");

        let (code, message) = error(refusal(409, "model-not-ready", "parakeet_v2"));
        assert_eq!(code, "model-unavailable");
        assert!(
            message.contains("hex model install parakeet_v2"),
            "{message}"
        );

        let (code, message) = error(refusal(409, "model-not-ready", "custom-ab12ab12ab12ab12"));
        assert_eq!(code, "model-unavailable");
        assert!(!message.contains("hex model install"), "{message}");

        let (code, message) = error(refusal(429, "queue-full", "parakeet_v2"));
        assert_eq!(code, "busy");
        assert!(
            message.contains("busy with another transcription"),
            "{message}"
        );

        let (code, _) = error(refusal(413, "resource-exhausted", "parakeet_v2"));
        assert_eq!(code, "resource-exhausted");

        let (code, _) = error(refusal(409, "cancelled", "parakeet_v2"));
        assert_eq!(code, "failed");

        let (code, _) = error(refusal(503, "service-gone", "parakeet_v2"));
        assert_eq!(code, "unavailable");

        let (code, _) = error(refusal(401, "http-401", "parakeet_v2"));
        assert_eq!(code, "failed");

        let (code, _) = error(refusal(413, "http-413", "parakeet_v2"));
        assert_eq!(code, "resource-exhausted");
    }

    #[test]
    fn a_piece_answer_is_read_trimmed() {
        assert_eq!(
            transcript(br#"{"transcript":" hello ","durationMs":1000}"#).unwrap(),
            "hello"
        );
        assert!(matches!(
            transcript(b"not json"),
            Err(ControlReply::Error { ref code, .. }) if code == "failed"
        ));
    }
}
