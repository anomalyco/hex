#[cfg(hex_capture)]
use std::sync::mpsc;
#[cfg(hex_capture)]
use std::thread;
#[cfg(hex_capture)]
use std::time::Instant;

#[cfg(hex_capture)]
use color_eyre::eyre::{Result, eyre};

#[cfg(hex_capture)]
use super::{DictationInference, dictation_busy};
#[cfg(hex_capture)]
use crate::transcription::engine::TranscriptionEngine;

/// The longest audio the capture path hands the engine as one foreground
/// request, shared with `pieces::PIECE_SAMPLES`. Each request runs through
/// `Transcriber::transcribe_timed`, which enforces the loaded model's own
/// `models::max_audio_chunk_samples` cap and Whisper edge trimming.
#[cfg(hex_capture)]
pub(super) const WORD_CHUNK_SAMPLES: usize = crate::transcription::pieces::PIECE_SAMPLES;
/// How long a capture chunk waits for dictation to go idle before it is
/// queued anyway; the engine still never runs it inside a live stream.
#[cfg(hex_capture)]
const WORD_DICTATION_WAIT: std::time::Duration = std::time::Duration::from_secs(20);

/// Transcribes a capture's 16 kHz audio with word timestamps on the engine's
/// warm model (the `capture` experiment). Before each chunk it waits while
/// dictation is busy (from the shortcut press through inference); each chunk
/// is then a foreground request: after the dictation already sent, before
/// meeting chunks, and never inside a dictation live stream, which finishes
/// first. No second copy of the model is loaded, and only a weak reference
/// to the engine is kept between chunks. Quitting stops it between chunks.
/// Every chunk runs on `selection`.
#[cfg(hex_capture)]
pub fn transcribe_with_words(
    engine: &std::sync::Weak<TranscriptionEngine>,
    selection: &crate::transcription::models::TranscriptionSelection,
    samples: Vec<f32>,
) -> Result<crate::transcription::Transcript> {
    if samples.is_empty() {
        return Ok(crate::transcription::Transcript::default());
    }
    transcribe_in_chunks(samples, WORD_CHUNK_SAMPLES, |chunk| {
        wait_for_dictation_idle()?;
        let selection = selection.clone();
        let (send, transcript) = mpsc::sync_channel(1);
        let offline: crate::transcription::engine::Offline = Box::new(move |model, samples| {
            let _busy = DictationInference::begin();
            let transcriber = model
                .activate(&selection)
                .map_err(|error| format!("{error:#}"))?;
            // Route through `transcribe_timed` so `max_audio_chunk_samples`
            // (Whisper 30 s, Cohere 35 s, custom `max_audio_ms`) and Whisper
            // edge trimming always apply.
            let result = transcriber
                .transcribe_timed(samples)
                .map_err(|error| format!("{error:#}"))?;
            let text = result.text.clone();
            let _ = send.send(result);
            Ok(text)
        });
        let pending = engine
            .upgrade()
            .ok_or_else(|| eyre!("the transcription engine stopped"))?
            .foreground(chunk, offline)?;
        pending.wait().map_err(|error| eyre!(error))?;
        transcript
            .try_recv()
            .map_err(|_| eyre!("the transcription returned no words"))
    })
}

#[cfg(hex_capture)]
fn wait_for_dictation_idle() -> Result<()> {
    let started = Instant::now();
    while dictation_busy()
        && !crate::desktop::process_exit::quitting()
        && started.elapsed() < WORD_DICTATION_WAIT
    {
        thread::sleep(std::time::Duration::from_millis(50));
    }
    if crate::desktop::process_exit::quitting() {
        return Err(eyre!("Hex is quitting"));
    }
    Ok(())
}

/// Transcribes `samples` in quiet-cut chunks of at most `chunk_samples`,
/// offsetting each chunk's segment and word times by where its range starts.
#[cfg(hex_capture)]
pub(super) fn transcribe_in_chunks(
    samples: Vec<f32>,
    chunk_samples: usize,
    mut transcribe: impl FnMut(Vec<f32>) -> Result<crate::transcription::Transcript>,
) -> Result<crate::transcription::Transcript> {
    use crate::transcription::gguf_session::{chunk_ranges, sample_offset_ms};
    use crate::transcription::{Transcript, TranscriptSegment};
    let shift = |spans: Vec<TranscriptSegment>, offset_ms: i64| {
        spans.into_iter().filter_map(move |span| {
            let text = span.text.trim();
            let start_ms = span.start_ms.max(0);
            (!text.is_empty()).then(|| TranscriptSegment {
                start_ms: start_ms + offset_ms,
                end_ms: span.end_ms.max(start_ms) + offset_ms,
                text: text.to_string(),
            })
        })
    };
    let mut merged = Transcript::default();
    let mut texts = Vec::new();
    for range in chunk_ranges(&samples, chunk_samples.max(1)) {
        let offset_ms = sample_offset_ms(range.start);
        let result = transcribe(samples[range].to_vec())?;
        let text = result.text.trim();
        if !text.is_empty() {
            texts.push(text.to_string());
        }
        merged.segments.extend(shift(result.segments, offset_ms));
        merged.words.extend(shift(result.words, offset_ms));
    }
    merged.text = texts.join(" ");
    Ok(merged)
}
