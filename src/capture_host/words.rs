//! A capture's words, on the dictation engine's loaded model or Apple's:
//! in-memory samples for a short capture, or `audio.wav` read in pieces for a
//! streamed one.

use std::path::Path;
use std::sync::Weak;
#[cfg(not(target_os = "macos"))]
use std::sync::mpsc;

use color_eyre::eyre::{Result, eyre};
use scrawl_capture::audio::RecordedAudio;
use scrawl_capture::model::TranscriptData;
#[cfg(target_os = "macos")]
use scrawl_capture::transcribe::AppleTranscriber;
use scrawl_capture::transcribe::Transcriber;

use super::{DICTATION_ENGINE, SAMPLE_RATE, TimedWord, corrected_words, word_lag};
use crate::transcription::engine::TranscriptionEngine;
use crate::transcription::pieces::{PieceError, Pieces};

/// The transcriber `capture.transcriber` names when a capture is written.
pub(super) struct ChosenTranscriber;

impl ChosenTranscriber {
    fn hex(&self) -> HexTranscriber {
        HexTranscriber {
            engine: DICTATION_ENGINE
                .lock()
                .unwrap_or_else(|error| error.into_inner())
                .clone(),
        }
    }
}

impl Transcriber for ChosenTranscriber {
    fn transcribe(&self, audio: &RecordedAudio) -> anyhow::Result<Option<TranscriptData>> {
        match crate::app_settings::capture_settings().transcriber {
            crate::app_settings::CaptureTranscriber::Parakeet => self.hex().transcribe(audio),
            #[cfg(target_os = "macos")]
            crate::app_settings::CaptureTranscriber::Apple => {
                AppleTranscriber::default().transcribe(audio)
            }
            // Apple's SpeechAnalyzer is macOS's; elsewhere it is Hex's model.
            #[cfg(not(target_os = "macos"))]
            crate::app_settings::CaptureTranscriber::Apple => self.hex().transcribe(audio),
        }
    }

    fn transcribe_wav(
        &self,
        wav_path: &Path,
        duration_ms: u64,
    ) -> anyhow::Result<Option<TranscriptData>> {
        match crate::app_settings::capture_settings().transcriber {
            crate::app_settings::CaptureTranscriber::Parakeet => {
                self.hex().transcribe_wav(wav_path, duration_ms)
            }
            #[cfg(target_os = "macos")]
            crate::app_settings::CaptureTranscriber::Apple => {
                AppleTranscriber::default().transcribe_wav(wav_path, duration_ms)
            }
            #[cfg(not(target_os = "macos"))]
            crate::app_settings::CaptureTranscriber::Apple => {
                self.hex().transcribe_wav(wav_path, duration_ms)
            }
        }
    }
}

/// Word-timed transcription on the dictation engine's loaded model. It holds
/// the engine weakly, so a capture still writing never keeps it alive.
struct HexTranscriber {
    engine: Option<Weak<TranscriptionEngine>>,
}

impl Transcriber for HexTranscriber {
    /// Word times are milliseconds from `audio.samples[0]`; the engine moves
    /// them onto the capture's clock and builds the segments.
    fn transcribe(&self, audio: &RecordedAudio) -> anyhow::Result<Option<TranscriptData>> {
        let Some(engine) = &self.engine else {
            return Ok(None);
        };
        let samples = if audio.sample_rate == SAMPLE_RATE {
            audio.samples.clone()
        } else {
            crate::dictation::try_resample_for_parakeet(&audio.samples, audio.sample_rate)
                .map_err(|error| anyhow::anyhow!(error))?
        };
        let (_, selection) = crate::app_settings::transcription_selection();
        let duration_ms = (samples.len() as u64) * 1_000 / u64::from(SAMPLE_RATE);
        let (text, words) = transcribe_words(engine, &selection, samples)
            .map_err(|error| anyhow::anyhow!("{error:#}"))?;
        Ok(Some(transcript(
            text,
            words,
            duration_ms,
            false,
            &selection,
        )))
    }

    /// Reads `audio.wav` a piece at a time, so a capture of hours holds about
    /// one 30 s piece rather than the whole session.
    fn transcribe_wav(
        &self,
        wav_path: &Path,
        _duration_ms: u64,
    ) -> anyhow::Result<Option<TranscriptData>> {
        let Some(engine) = &self.engine else {
            return Ok(None);
        };
        let (_, selection) = crate::app_settings::transcription_selection();
        let mut pieces = match Pieces::open(wav_path) {
            Ok(pieces) => pieces,
            // The same answer `transcribe_with` gives for a capture with no audio
            // at all, rather than an empty transcript with no source.
            Err(PieceError::Empty) => {
                return Ok(Some(TranscriptData {
                    source: "none".into(),
                    ..TranscriptData::default()
                }));
            }
            Err(error) => {
                return Err(anyhow::anyhow!(
                    "could not read {}: {error:?}",
                    wav_path.display()
                ));
            }
        };
        let (text, words, duration_ms, estimated) = transcribe_pieces(&mut pieces, |piece| {
            transcribe_words(engine, &selection, piece)
        })
        .map_err(|error| anyhow::anyhow!("{error:#}"))?;
        Ok(Some(transcript(
            text,
            words,
            duration_ms,
            estimated,
            &selection,
        )))
    }
}

fn transcript(
    text: String,
    words: Vec<TimedWord>,
    duration_ms: u64,
    spread: bool,
    selection: &crate::transcription::models::TranscriptionSelection,
) -> TranscriptData {
    // A model that gives no word times (GigaAM v3 asks for none) still said
    // the text: `transcribe_pieces` spreads each untimed piece over its own
    // audio, and a clip held in memory is spread over the whole of it, as
    // `hex` does out of process. Both are estimates, never the model's times.
    let (words, estimated) = if !words.is_empty() {
        (corrected_words(words, word_lag(selection.model)), spread)
    } else if text.trim().is_empty() {
        (Vec::new(), false)
    } else {
        let (_, whole) = scrawl_capture::transcribe::spread_words(&text, 0, duration_ms);
        (whole, true)
    };
    TranscriptData {
        source: "hex".into(),
        model: Some(selection.model.as_str().into()),
        text,
        words,
        estimated,
        ..TranscriptData::default()
    }
}

/// Transcribes saved pieces in order, moving each piece's words onto the
/// capture's clock by the audio before it and joining their text with one
/// space. A piece with text but no word times is spread over that piece's own
/// audio, never the whole capture, and `true` says an estimate was made.
fn transcribe_pieces(
    pieces: &mut Pieces,
    mut transcribe: impl FnMut(Vec<f32>) -> Result<(String, Vec<TimedWord>)>,
) -> Result<(String, Vec<TimedWord>, u64, bool)> {
    let mut texts = Vec::new();
    let mut words = Vec::new();
    let mut total_frames = 0_usize;
    let mut estimated = false;
    loop {
        let piece = match pieces.next() {
            Ok(Some(piece)) => piece,
            Ok(None) => break,
            Err(error) => return Err(eyre!("could not read the capture's audio: {error:?}")),
        };
        let piece_frames = piece.len();
        // One running frame total, so a piece's truncated milliseconds never
        // accumulate into drift.
        let offset_ms = (total_frames as i64) * 1_000 / i64::from(SAMPLE_RATE);
        let (text, piece_words) = transcribe(piece)?;
        let text = text.trim();
        if !text.is_empty() {
            texts.push(text.to_string());
        }
        if piece_words.is_empty() && !text.is_empty() {
            // A model that gives no word times (GigaAM v3) still said this
            // piece: its text goes over the piece's own audio, so a long
            // capture's words stay where they were said.
            let next_ms = ((total_frames + piece_frames) as i64) * 1_000 / i64::from(SAMPLE_RATE);
            words.extend(
                scrawl_capture::transcribe::spread_words(text, 0, (next_ms - offset_ms) as u64)
                    .1
                    .into_iter()
                    .map(|word| TimedWord {
                        start_ms: word.t0 as i64 + offset_ms,
                        end_ms: word.t1 as i64 + offset_ms,
                        text: word.text,
                    }),
            );
            estimated = true;
        }
        for word in piece_words {
            words.push(TimedWord {
                start_ms: word.start_ms + offset_ms,
                end_ms: word.end_ms + offset_ms,
                text: word.text,
            });
        }
        total_frames += piece_frames;
    }
    let duration_ms = (total_frames as u64) * 1_000 / u64::from(SAMPLE_RATE);
    Ok((texts.join(" "), words, duration_ms, estimated))
}

/// The clip's text and words on the dictation engine's warm model.
#[cfg(target_os = "macos")]
fn transcribe_words(
    engine: &Weak<TranscriptionEngine>,
    selection: &crate::transcription::models::TranscriptionSelection,
    samples: Vec<f32>,
) -> Result<(String, Vec<TimedWord>)> {
    let transcript = crate::parakeet::transcribe_with_words(engine, selection, samples)?;
    let words = transcript
        .words
        .into_iter()
        .map(|word| TimedWord {
            start_ms: word.start_ms,
            end_ms: word.end_ms,
            text: word.text,
        })
        .collect();
    Ok((transcript.text, words))
}

/// The clip's text and words, as one foreground request on the Linux or
/// Windows listener's engine: after the dictation already sent, never inside
/// a live stream. Its model is whatever the listener has loaded.
#[cfg(not(target_os = "macos"))]
fn transcribe_words(
    engine: &Weak<TranscriptionEngine>,
    _selection: &crate::transcription::models::TranscriptionSelection,
    samples: Vec<f32>,
) -> Result<(String, Vec<TimedWord>)> {
    if samples.is_empty() {
        return Ok((String::new(), Vec::new()));
    }
    let (send, transcript) = mpsc::sync_channel(1);
    let offline: crate::transcription::engine::Offline = Box::new(move |model, samples| {
        let (text, words) = model
            .transcribe_words(&samples)
            .map_err(|error| format!("{error:#}"))?;
        let _ = send.send((text.clone(), words));
        Ok(text)
    });
    engine
        .upgrade()
        .ok_or_else(|| eyre!("the transcription engine stopped"))?
        .foreground(samples, offline)?
        .wait()
        .map_err(|error| eyre!(error))?;
    let (text, words) = transcript
        .try_recv()
        .map_err(|_| eyre!("the transcription returned no words"))?;
    let words = words
        .into_iter()
        .map(|(start_ms, end_ms, text)| TimedWord {
            start_ms,
            end_ms,
            text,
        })
        .collect();
    Ok((text, words))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A tone with one quiet 100 ms at 28.0 s: the quietest window in the first
    /// piece's last five seconds, at an odd sample, so its cut lands off a
    /// whole-millisecond boundary.
    fn paced(seconds: usize) -> Vec<f32> {
        let mut samples = vec![0.25_f32; 16_000 * seconds];
        for sample in &mut samples[448_001..448_001 + 1_600] {
            *sample = 0.0;
        }
        samples
    }

    fn wav(name: &str, samples: &[f32]) -> std::path::PathBuf {
        let path = std::env::temp_dir().join(format!(
            "hex-capture-words-{name}-{}.wav",
            std::process::id()
        ));
        let mut writer = hound::WavWriter::create(
            &path,
            hound::WavSpec {
                channels: 1,
                sample_rate: 16_000,
                bits_per_sample: 16,
                sample_format: hound::SampleFormat::Int,
            },
        )
        .unwrap();
        for sample in samples {
            writer
                .write_sample(scrawl_capture::audio::pcm16(*sample))
                .unwrap();
        }
        writer.finalize().unwrap();
        path
    }

    #[test]
    fn pieces_past_the_first_window_keep_their_place_on_the_clock() {
        let path = wav("offset", &paced(61));
        let mut pieces = Pieces::open(&path).unwrap();
        let mut frames = Vec::new();
        let (text, words, duration_ms, estimated) = transcribe_pieces(&mut pieces, |piece| {
            frames.push(piece.len());
            Ok((
                format!("piece {}", frames.len()),
                vec![TimedWord {
                    start_ms: 0,
                    end_ms: 500,
                    text: "word".into(),
                }],
            ))
        })
        .unwrap();
        let _ = std::fs::remove_file(&path);

        assert_eq!(frames.len(), 3, "61 s is three pieces");
        assert_eq!(text, "piece 1 piece 2 piece 3");
        assert_eq!(words.len(), 3);
        assert_eq!(duration_ms, 61_000, "the clip's whole length");
        assert!(!estimated, "the model timed every piece");
        // Every word sits at the total audio before its piece, whole
        // milliseconds of the running frame count: adding each piece's
        // truncated duration instead would drift whenever a cut lands off a
        // millisecond boundary.
        assert!(
            frames.iter().any(|&frames| frames % 16 != 0),
            "the fixture cuts pieces at odd sample counts: {frames:?}"
        );
        let mut total = 0_usize;
        for (index, word) in words.iter().enumerate() {
            let offset_ms = (total as i64) * 1_000 / 16_000;
            assert_eq!(word.start_ms, offset_ms, "word {index} stays on the clock");
            assert_eq!(word.end_ms, offset_ms + 500);
            total += frames[index];
        }
        assert!(
            words[1].start_ms >= 28_000,
            "the second window is past 28 s"
        );
        assert!(words[2].start_ms >= 57_000, "the third window is past 57 s");
    }

    /// A model that gives no word times (GigaAM v3) said the text, so the
    /// capture's words are spread over the clip as estimated times rather
    /// than left empty for the editor and the keyframes.
    #[test]
    fn a_model_without_word_times_spreads_its_text_over_the_clip() {
        let selection = crate::transcription::models::TranscriptionSelection {
            model: crate::transcription::models::TranscriptionModelId::GigaAmV3,
            ..Default::default()
        };
        let untimed = transcript("make it blue".into(), Vec::new(), 4_000, false, &selection);
        assert!(untimed.estimated, "spread times are estimates");
        assert_eq!(untimed.text, "make it blue");
        assert_eq!(
            untimed
                .words
                .iter()
                .map(|word| word.text.as_str())
                .collect::<Vec<_>>(),
            ["make", "it", "blue"]
        );
        assert_eq!(untimed.words[0].t0, 0);
        assert_eq!(untimed.words[2].t1, 4_000, "the words tile the clip");

        let silent = transcript("  ".into(), Vec::new(), 4_000, false, &selection);
        assert!(silent.words.is_empty() && !silent.estimated);

        // A piece the pieces path already spread is kept, never spread again.
        let pieced = transcript(
            "late words".into(),
            vec![TimedWord {
                start_ms: 31_000,
                end_ms: 32_000,
                text: "late".into(),
            }],
            61_000,
            true,
            &selection,
        );
        assert!(pieced.estimated, "the pieces' spread stays an estimate");
        assert_eq!(pieced.words.len(), 1, "already spread");

        let timed = transcript(
            "make it blue".into(),
            vec![TimedWord {
                start_ms: 100,
                end_ms: 300,
                text: "make".into(),
            }],
            4_000,
            false,
            &selection,
        );
        assert!(!timed.estimated, "a model's own times stay measured");
        assert_eq!(timed.words.len(), 1);
    }

    /// A piece with text but no word times is spread over that piece's own
    /// 30 s of audio, never the whole capture: a minute in, its words start a
    /// minute in, and a silent piece between adds nothing.
    #[test]
    fn an_untimed_piece_spreads_over_its_own_audio_only() {
        let path = wav("untimed", &paced(61));
        let mut pieces = Pieces::open(&path).unwrap();
        let mut frames = Vec::new();
        let mut said = ["first piece said", "", "late words"].into_iter();
        let (text, words, duration_ms, estimated) = transcribe_pieces(&mut pieces, |piece| {
            frames.push(piece.len());
            Ok((said.next().unwrap_or_default().to_string(), Vec::new()))
        })
        .unwrap();
        let _ = std::fs::remove_file(&path);

        assert_eq!(frames.len(), 3, "61 s is three pieces");
        assert_eq!(text, "first piece said late words", "silence adds nothing");
        assert!(estimated, "spread times are estimates");
        assert_eq!(
            words
                .iter()
                .map(|word| word.text.as_str())
                .collect::<Vec<_>>(),
            ["first", "piece", "said", "late", "words"]
        );

        let at = |frames: usize| (frames as u64) * 1_000 / 16_000;
        let first_end = at(frames[0]);
        let last_start = at(frames[0] + frames[1]);
        assert_eq!(words[0].start_ms, 0);
        assert_eq!(
            words[2].end_ms, first_end as i64,
            "the first piece's words tile its own audio"
        );
        assert_eq!(
            words[3].start_ms, last_start as i64,
            "the last piece's words start at its own offset"
        );
        assert_eq!(words[4].end_ms, duration_ms as i64);
        assert!(
            (30_000..61_000).contains(&last_start),
            "the last piece sits past the first: {last_start}"
        );
    }
}
