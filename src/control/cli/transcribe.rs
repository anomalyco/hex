//! `hex transcribe <file>` (Labs › Transcribe files): a saved WAV through
//! the running Hex's local API, or in this process on Linux and Windows when
//! Hex is stopped. Nothing is pasted, saved to History, or processed.
//!
//! A file of any length is read in pieces of at most 30 s ([`pieces`]) and
//! their text is joined with one space: a piece that short can run on the
//! running Hex's dictation engine's loaded copy when it holds the model,
//! instead of its service loading a second one, and Hex keeps one piece
//! rather than the whole file in memory.

mod pieces;

use std::path::{Path, PathBuf};
use std::time::Duration;

use serde::Deserialize;

use super::{Output, print};
use crate::control::{ControlReply, model_selection};
use crate::labs::Feature;
use crate::transcription::models::TranscriptionSelection;
use crate::transcription::service::AudioError;
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

/// The lab's gate, the file, and the model, all checked before Hex is asked
/// or a model is loaded. The samples are read piece by piece later.
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
    // Every sample is read once here and dropped, so a truncated or corrupt
    // file is refused before Hex is asked or a model loads, rather than after
    // minutes of transcription.
    open(file)?
        .validate()
        .map_err(|error| refused(file, error))?;
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

/// The refusal for a file Hex cannot read, before any transcription runs, or
/// for one that changed while it was being read.
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
        PieceError::Empty => ControlReply::error(
            "no-speech",
            format!("{name} has no audio; nothing to transcribe"),
        ),
        PieceError::Incomplete => ControlReply::error(
            "unsupported",
            format!("{name} is truncated or corrupt; Hex couldn't read all of its audio"),
        ),
        PieceError::Audio(AudioError::Unsupported) => ControlReply::error(
            "unsupported",
            format!(
                "{name} isn't a WAV Hex can decode; convert it to 16 kHz mono WAV, \
                 for example with ffmpeg: ffmpeg -i <file> -ar 16000 -ac 1 <file>.wav"
            ),
        ),
        PieceError::Audio(_) => ControlReply::error(
            "unsupported",
            format!(
                "{name} isn't a WAV file Hex can read; convert it to WAV first, \
                 for example with ffmpeg: ffmpeg -i <file> <file>.wav"
            ),
        ),
    }
}

/// A duration as `m:ss`.
fn minute_second(ms: u64) -> String {
    format!("{}:{:02}", ms / 60_000, ms / 1_000 % 60)
}

fn transcribe(job: &Job) -> ControlReply {
    if crate::local_api::running_pid().is_some() {
        return in_running_hex(job);
    }
    offline(job)
}

/// The running Hex transcribes one piece at a time with the model the
/// request names. A piece of at most 30 s runs on its dictation engine's
/// loaded copy when it holds that model, so no second copy loads and
/// dictation can run between pieces; a piece of only silence is skipped
/// before it is sent once the speech detector is installed.
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
        let samples = match next_speechful(&mut pieces) {
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

/// The next piece that may hold speech, skipping pieces of only silence as a
/// long GGUF clip's silent chunks are skipped: models invent text for silence
/// (#1194) or run to their generation cap (#968).
///
/// The check runs only when the speech detector is already installed: a
/// one-shot command must not start its background model download.
fn next_speechful(pieces: &mut Pieces) -> std::result::Result<Option<Vec<f32>>, PieceError> {
    let detector = crate::transcription::models::speech_detector_installed();
    next_speechful_with(pieces, detector)
}

/// [`next_speechful`] with the detector's availability passed in, so a test
/// needs no installed model.
fn next_speechful_with(
    pieces: &mut Pieces,
    detector: bool,
) -> std::result::Result<Option<Vec<f32>>, PieceError> {
    loop {
        match pieces.next()? {
            Some(samples) if !keeps_piece(&samples, detector) => continue,
            piece => return Ok(piece),
        }
    }
}

/// Whether a piece goes to the model: only silence is skipped, and only when
/// the detector is installed and hears none in it.
fn keeps_piece(samples: &[f32], detector: bool) -> bool {
    !detector || crate::transcription::gguf_session::chunk_may_hold_speech(samples)
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
        let samples = match next_speechful(&mut pieces) {
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
        let path = temp("wav");
        std::fs::write(&path, wav()).unwrap();
        let job = prepare(&settings, &path, None, None).unwrap();
        let _ = std::fs::remove_file(&path);
        assert_eq!(job.file, path);
        assert_eq!(job.selection.model, settings.transcription.model);
        assert_eq!(job.selection.language, settings.transcription.language);
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
        let mut settings = AppSettings::default();
        settings.labs.transcribe_files = true;
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
        let reply = prepare(&settings, &path, None, None).unwrap_err();
        let _ = std::fs::remove_file(&path);
        assert!(
            matches!(reply, ControlReply::Error { ref code, .. } if code == "no-speech"),
            "{reply:?}"
        );
    }

    #[test]
    fn a_truncated_file_is_refused_before_any_transcription() {
        let mut settings = AppSettings::default();
        settings.labs.transcribe_files = true;
        let path = wav_at("truncated", 3.0);
        let bytes = std::fs::read(&path).unwrap();
        std::fs::write(&path, &bytes[..bytes.len() / 2]).unwrap();

        let reply = prepare(&settings, &path, None, None).unwrap_err();
        let _ = std::fs::remove_file(&path);
        let (code, message) = error(reply);
        assert_eq!(code, "unsupported");
        assert!(message.contains("truncated or corrupt"), "{message}");
    }

    #[test]
    fn a_piece_is_kept_without_the_detector_and_it_is_not_consulted() {
        let path = wav_at("silent", 1.0);
        let mut pieces = open(&path).unwrap();
        {
            // The override says "no speech": a kept piece proves the check did
            // not run, so the detector cannot have started its download.
            let _silent = crate::transcription::speech_detector::override_for_test(Some(false));
            assert!(next_speechful_with(&mut pieces, false).unwrap().is_some());
        }
        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn an_installed_detector_skips_a_piece_it_hears_nothing_in() {
        let path = wav_at("kept", 1.0);
        {
            let mut pieces = open(&path).unwrap();
            let _silent = crate::transcription::speech_detector::override_for_test(Some(false));
            assert!(next_speechful_with(&mut pieces, true).unwrap().is_none());
        }
        {
            let mut pieces = open(&path).unwrap();
            let _speech = crate::transcription::speech_detector::override_for_test(Some(true));
            assert!(next_speechful_with(&mut pieces, true).unwrap().is_some());
        }
        let _ = std::fs::remove_file(&path);
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
    fn an_unsupported_wav_asks_for_sixteen_kilohertz_mono() {
        let (code, message) = error(refused(
            Path::new("wide.wav"),
            PieceError::Audio(crate::transcription::service::AudioError::Unsupported),
        ));
        assert_eq!(code, "unsupported");
        assert!(message.contains("16 kHz mono"), "{message}");
        assert!(message.contains("-ar 16000 -ac 1"), "{message}");
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
}
