//! `hex transcribe <file>`: a saved recording through the running Hex's
//! local API, or with no Hex running through an embedded service this
//! command starts and stops ([`super::embedded`]). Nothing is pasted, saved
//! to History, or processed, and this process never loads a model.
//!
//! A file of any length is read in pieces of at most 30 s ([`pieces`]) and
//! their text is joined with one space: a piece that short can run on the
//! running Hex's dictation engine's loaded copy when it holds the model,
//! instead of its service loading a second one, and Hex keeps one piece
//! rather than the whole file in memory.

mod pieces;

use std::path::Path;
use std::time::Duration;

use serde::Deserialize;

use super::{Output, embedded, prepare, print, progress};
use crate::control::{ControlReply, model_selection};
use crate::local_api::Endpoint;
use crate::transcription::models::TranscriptionSelection;
use crate::transcription::service::AudioError;
use pieces::{PieceError, Pieces};

/// One piece waits for a cold model behind it and for its own transcription.
const TIMEOUT: Duration = Duration::from_secs(600);
const MAX_RESPONSE: usize = 1024 * 1024;
/// The longest transcript printed; past it the file is refused rather than
/// held in memory.
const MAX_TRANSCRIPT: usize = 16 * 1024 * 1024;

#[derive(Deserialize)]
struct Transcript {
    transcript: String,
}

pub(super) fn run(
    file: std::path::PathBuf,
    model: Option<String>,
    language: Option<String>,
    output: Output,
) -> u8 {
    embedded::exit_on_interrupt();
    let reply = transcribe(&file, model, language, output.json);
    embedded::done();
    progress::clear();
    print(reply, output)
}

fn transcribe(
    file: &Path,
    model: Option<String>,
    language: Option<String>,
    json: bool,
) -> ControlReply {
    let settings = match crate::app_settings::AppSettings::read() {
        Ok(settings) => settings,
        Err(error) => {
            return ControlReply::error(
                "failed",
                format!("couldn't read Hex's settings: {error:#}"),
            );
        }
    };
    let (mut pieces, selection) = match prepare_job(&settings, file, model, language) {
        Ok(job) => job,
        Err(reply) => return reply,
    };
    let endpoint = match embedded::endpoint() {
        Ok(endpoint) => endpoint,
        Err(message) => return ControlReply::error("unavailable", message),
    };
    run_pieces(&endpoint, file, &mut pieces, &selection, json)
}

/// The file and the model, both checked before Hex is asked or a model is
/// loaded or downloaded: the file opens, its audio is found and its first
/// block decodes, and a WAV's every sample reads. The samples are read piece
/// by piece later.
fn prepare_job(
    settings: &crate::app_settings::AppSettings,
    file: &Path,
    model: Option<String>,
    language: Option<String>,
) -> std::result::Result<(Pieces, TranscriptionSelection), ControlReply> {
    let mut pieces = Pieces::open(file).map_err(|error| refused(file, error))?;
    pieces.validate().map_err(|error| refused(file, error))?;
    let model = model.unwrap_or_else(|| settings.transcription.model.as_str().to_owned());
    let selection = model_selection(settings, &model, language.as_deref())?;
    Ok((pieces, selection))
}

/// The refusal for a file Hex cannot read, before any transcription runs, or
/// for one that stopped reading partway.
fn refused(file: &Path, error: PieceError) -> ControlReply {
    let name = file.display();
    match error {
        PieceError::Io(error) => {
            ControlReply::error("failed", format!("couldn't read {name}: {error}"))
        }
        PieceError::Changed { at_ms } => ControlReply::error(
            "failed",
            format!(
                "couldn't read {name} past {}; it changed or could no longer be read",
                minute_second(at_ms)
            ),
        ),
        PieceError::Damaged { at_ms } => ControlReply::error(
            "unsupported",
            format!(
                "{name} is damaged at {}; Hex couldn't decode its audio past there",
                minute_second(at_ms)
            ),
        ),
        PieceError::Empty => ControlReply::error(
            "no-speech",
            format!("{name} has no audio; nothing to transcribe"),
        ),
        PieceError::Incomplete => ControlReply::error(
            "unsupported",
            format!("{name} is truncated or corrupt; Hex couldn't read all of its audio"),
        ),
        PieceError::NotMedia => ControlReply::error(
            "unsupported",
            format!("{name} isn't an audio or video file Hex can read"),
        ),
        PieceError::NoAudioTrack => {
            ControlReply::error("unsupported", format!("{name} has no audio track"))
        }
        PieceError::UnsupportedCodec(codec) => ControlReply::error(
            "unsupported",
            format!("{name}'s audio is {codec}, which Hex can't decode"),
        ),
        PieceError::Audio(AudioError::Unsupported) => ControlReply::error(
            "unsupported",
            format!("{name} is a WAV whose sample rate or channels Hex can't decode"),
        ),
        PieceError::Audio(_) => ControlReply::error(
            "unsupported",
            format!("{name} isn't a WAV file Hex can read"),
        ),
    }
}

/// A duration as `m:ss`.
fn minute_second(ms: u64) -> String {
    format!("{}:{:02}", ms / 60_000, ms / 1_000 % 60)
}

/// Each piece goes to the endpoint in turn with the model the request names.
/// A piece of at most 30 s runs on the running Hex's dictation engine's
/// loaded copy when it holds that model, so no second copy loads and
/// dictation can run between pieces. A model that isn't downloaded is
/// offered once at a terminal, then the same piece is sent again.
fn run_pieces(
    endpoint: &Endpoint,
    file: &Path,
    pieces: &mut Pieces,
    selection: &TranscriptionSelection,
    json: bool,
) -> ControlReply {
    let query = url::form_urlencoded::Serializer::new(String::new())
        .append_pair("model", selection.model.as_str())
        .append_pair("language", &selection.language)
        .finish();
    let path = format!("/transcriptions?{query}");
    let name = file.file_name().map_or_else(
        || file.display().to_string(),
        |name| name.to_string_lossy().into_owned(),
    );
    let model = selection.model.as_str();
    let mut texts = Vec::new();
    let mut length = 0;
    let mut prepared = false;
    loop {
        show_progress(&name, pieces);
        let samples = match pieces.next() {
            Ok(Some(samples)) => samples,
            Ok(None) => break,
            Err(error) => return refused(file, error),
        };
        let wav = match pieces::wav_bytes(&samples) {
            Ok(wav) => wav,
            Err(error) => return refused(file, error),
        };
        drop(samples);
        let text = loop {
            match endpoint.post(&path, "audio/wav", &wav, TIMEOUT, MAX_RESPONSE) {
                Ok((200, body)) => match transcript(&body) {
                    Ok(text) => break text,
                    Err(reply) => return reply,
                },
                Ok((status, body)) => {
                    let code = crate::local_api::error_code(status, &body);
                    if code == "model-not-ready"
                        && !prepared
                        && !model.starts_with("custom-")
                        && prepare::consent(selection, json)
                    {
                        prepared = true;
                        if let Err(reply) = prepare::prepare(endpoint, selection) {
                            return reply;
                        }
                        show_progress(&name, pieces);
                        continue;
                    }
                    return refusal(status, &code, model);
                }
                Err(error) => return ControlReply::error("transport", format!("{error:#}")),
            }
        };
        // A piece that heard no speech adds nothing: only a file with
        // nothing to say answers no-speech.
        if !text.is_empty() {
            length += text.len() + 1;
            if length > MAX_TRANSCRIPT {
                return ControlReply::error(
                    "resource-exhausted",
                    format!("{name}'s transcript is longer than Hex prints"),
                );
            }
            texts.push(text);
        }
    }
    joined(texts)
}

fn show_progress(name: &str, pieces: &Pieces) {
    let at = minute_second(pieces.position_ms());
    progress::show(&match pieces.duration_ms() {
        Some(total) => format!("Transcribing {name}… {at} of {}", minute_second(total)),
        None => format!("Transcribing {name}… {at}"),
    });
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
            format!("Hex doesn't know `{model}`; run `hex model list`"),
        ),
        "unsupported-model" => ControlReply::error(
            "model-unavailable",
            format!("Hex can't use `{model}`; run `hex model list` to see what it has"),
        ),
        "invalid-request" => ControlReply::error("failed", "Hex refused the request; update Hex"),
        "unsupported-audio" | "invalid-audio" => ControlReply::error(
            "failed",
            "Hex couldn't read the audio it was sent; update Hex",
        ),
        "length-required" => ControlReply::error("failed", "Hex refused the upload; update Hex"),
        "resource-exhausted" => ControlReply::error(
            "resource-exhausted",
            "Hex says the audio is too large or too long to transcribe",
        ),
        "model-not-ready" => ControlReply::error("model-unavailable", not_ready(model)),
        "queue-full" => {
            ControlReply::error("busy", "Hex is busy with another transcription; try again")
        }
        "upload-timeout" => {
            ControlReply::error("failed", "Hex timed out reading the audio; try again")
        }
        "cancelled" => ControlReply::error("failed", "Hex cancelled the transcription; try again"),
        "service-gone" => ControlReply::error(
            "unavailable",
            "Hex's transcription service stopped; try again",
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
            408 => ControlReply::error("failed", "Hex timed out reading the audio; try again"),
            409 => ControlReply::error("model-unavailable", not_ready(model)),
            413 => ControlReply::error(
                "resource-exhausted",
                "Hex says the audio is too large to transcribe",
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
        format!("`{model}` isn't ready; check it in Hex's model picker")
    } else {
        format!("`{model}` isn't downloaded; run `hex model install {model}`")
    }
}

fn no_speech() -> ControlReply {
    ControlReply::error(
        "no-speech",
        "no speech was heard in the file; nothing to print",
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::app_settings::AppSettings;

    /// A temp WAV path unique to this module: tests in other modules run in
    /// parallel in one process and share `TMPDIR`.
    fn temp(name: &str) -> std::path::PathBuf {
        std::env::temp_dir().join(format!(
            "hex-transcribe-cli-{name}-{}.wav",
            std::process::id()
        ))
    }

    fn not_a_wav() -> std::path::PathBuf {
        let path = temp("not-a-wav");
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
    fn a_file_that_is_not_media_is_refused_before_any_transcription() {
        let settings = AppSettings::default();
        let path = not_a_wav();
        let reply = prepare_job(&settings, &path, None, None).unwrap_err();
        let _ = std::fs::remove_file(&path);
        assert!(
            matches!(reply, ControlReply::Error { ref code, .. } if code == "unsupported"),
            "{reply:?}"
        );
    }

    #[test]
    fn a_wav_uses_the_selected_model_and_language() {
        let settings = AppSettings::default();
        let path = temp("wav");
        std::fs::write(&path, wav()).unwrap();
        let (_, selection) = prepare_job(&settings, &path, None, None).unwrap();
        let _ = std::fs::remove_file(&path);
        assert_eq!(selection.model, settings.transcription.model);
        assert_eq!(selection.language, settings.transcription.language);
    }

    /// A 16 kHz mono WAV of `seconds` of silence.
    fn wav_at(name: &str, seconds: f32) -> std::path::PathBuf {
        let path = temp(name);
        let spec = hound::WavSpec {
            channels: 1,
            sample_rate: 16_000,
            bits_per_sample: 16,
            sample_format: hound::SampleFormat::Int,
        };
        let mut writer = hound::WavWriter::create(&path, spec).unwrap();
        for _ in 0..(16_000.0 * seconds) as usize {
            writer.write_sample(0_i16).unwrap();
        }
        writer.finalize().unwrap();
        path
    }

    #[test]
    fn a_file_with_no_audio_answers_no_speech() {
        let settings = AppSettings::default();
        let path = temp("empty");
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
        let reply = prepare_job(&settings, &path, None, None).unwrap_err();
        let _ = std::fs::remove_file(&path);
        assert!(
            matches!(reply, ControlReply::Error { ref code, .. } if code == "no-speech"),
            "{reply:?}"
        );
    }

    #[test]
    fn a_truncated_file_is_refused_before_any_transcription() {
        let settings = AppSettings::default();
        let path = wav_at("truncated", 3.0);
        let bytes = std::fs::read(&path).unwrap();
        std::fs::write(&path, &bytes[..bytes.len() / 2]).unwrap();

        let reply = prepare_job(&settings, &path, None, None).unwrap_err();
        let _ = std::fs::remove_file(&path);
        let (code, message) = error(reply);
        assert_eq!(code, "unsupported");
        assert!(message.contains("truncated or corrupt"), "{message}");
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
        assert_eq!(code, "failed");
        assert!(message.contains("update Hex"), "{message}");

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

    #[test]
    fn a_changed_file_names_where_reading_stopped() {
        let (code, message) = error(refused(
            Path::new("talk.wav"),
            PieceError::Changed { at_ms: 125_000 },
        ));
        assert_eq!(code, "failed");
        assert!(message.contains("past 2:05"), "{message}");
        assert!(
            message.contains("changed or could no longer be read"),
            "{message}"
        );
        assert_eq!(minute_second(0), "0:00");
        assert_eq!(minute_second(59_999), "0:59");
        assert_eq!(minute_second(3_600_000), "60:00");
    }

    #[test]
    fn a_truncated_wav_says_it_is_truncated() {
        let (code, message) = error(refused(Path::new("cut.wav"), PieceError::Incomplete));
        assert_eq!(code, "unsupported");
        assert!(message.contains("truncated or corrupt"), "{message}");
        assert!(
            message.contains("couldn't read all of its audio"),
            "{message}"
        );
    }

    #[test]
    fn a_broken_wav_asks_for_a_wav() {
        let (code, message) = error(refused(
            Path::new("broken.wav"),
            PieceError::Audio(crate::transcription::service::AudioError::Invalid),
        ));
        assert_eq!(code, "unsupported");
        assert!(
            message.contains("isn't a WAV file Hex can read"),
            "{message}"
        );
    }

    #[test]
    fn each_unreadable_file_says_what_is_wrong_with_it() {
        for (piece_error, says) in [
            (PieceError::NotMedia, "isn't an audio or video file"),
            (PieceError::NoAudioTrack, "has no audio track"),
            (
                PieceError::UnsupportedCodec("Apple Lossless".into()),
                "audio is Apple Lossless, which Hex can't decode",
            ),
            (PieceError::Damaged { at_ms: 61_000 }, "damaged at 1:01"),
            (
                PieceError::Audio(AudioError::Unsupported),
                "sample rate or channels",
            ),
        ] {
            let (code, message) = error(refused(Path::new("talk.m4a"), piece_error));
            assert_eq!(code, "unsupported");
            assert!(message.contains(says), "{message}");
            assert!(!message.contains("ffmpeg"), "{message}");
        }
    }
}
