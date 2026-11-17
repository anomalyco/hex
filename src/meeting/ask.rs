//! Labs › Ask meetings: one question about a meeting's transcript, answered
//! by the writer Meetings › Summaries names (Apple Intelligence on this Mac,
//! or OpenCode), from a bounded excerpt of what was said. Read-only: the
//! answer is shown in the pane and never written to the manifest, the
//! Markdown, or anywhere else.

use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;

use color_eyre::eyre::{Result, eyre};

use super::generator::{self, GenerationError};
use super::{TranscriptEntry, TranscriptReader, format_duration};

/// The most transcript one question carries. The latest speech is kept when
/// it has to be trimmed, since a question about a recording is usually about
/// what was just said.
const EXCERPT_CHARS: usize = 12_000;
/// One excerpt line at most, so a monologue can't fill it.
const LINE_CHARS: usize = 600;
/// One question takes one call, bounded like a notes summary's.
pub(super) const TIMEOUT: Duration = Duration::from_secs(60);

const INSTRUCTIONS: &str = "You answer questions about a meeting transcript. Use only what the \
transcript says, and keep the answer short: one or two sentences. Each line starts with a \
timestamp and a speaker label: You is the person who recorded the meeting, and Them is everyone \
else. If the transcript doesn't answer the question, say that it isn't in the transcript so far, \
and don't guess.";

/// Answers `question` from the meeting's current transcript: the published
/// one, or the rolling draft while it records. Blocks; call off the UI
/// thread.
pub(crate) fn answer(id: &str, question: &str, cancelled: &AtomicBool) -> Result<String> {
    let question = question.trim();
    if question.is_empty() {
        return Err(eyre!("Type a question first"));
    }
    let entries = TranscriptReader::default().read(id)?;
    if entries.is_empty() {
        return Err(eyre!("This meeting has no transcript to ask about yet"));
    }
    if cancelled.load(Ordering::Acquire) {
        return Err(eyre!("The question was given up"));
    }
    let budget = super::notes::excerpt_budget(generator::context_tokens(), EXCERPT_CHARS);
    let excerpt = recent_excerpt(&entries, budget);
    if excerpt.is_empty() {
        return Err(eyre!("This meeting has no speech to ask about yet"));
    }
    let prompt =
        format!("Meeting transcript, most recent last:\n{excerpt}\n\nQuestion: {question}");
    let answer =
        generator::answer_question(INSTRUCTIONS, &prompt, TIMEOUT, cancelled).map_err(|error| {
            match error {
                GenerationError::ContextWindow => {
                    eyre!("The meeting is too long for the model to read in one answer")
                }
                GenerationError::Refused => eyre!("The model wouldn't answer that question"),
                GenerationError::Failed(message) => eyre!("{message}"),
            }
        })?;
    let answer = answer.trim();
    if answer.is_empty() {
        return Err(eyre!("The model answered with nothing"));
    }
    Ok(answer.to_owned())
}

/// The newest lines that fit in `max_chars`, oldest first, so a question
/// about a recording sees what was just said. Every line carries its
/// timestamp and speaker, like the notes prompt's.
fn recent_excerpt(entries: &[TranscriptEntry], max_chars: usize) -> String {
    let mut lines: Vec<String> = Vec::new();
    let mut used = 0;
    for entry in entries.iter().rev() {
        let text = entry.text.trim();
        if text.is_empty() {
            continue;
        }
        let line = truncate(
            format!(
                "[{}] {}: {text}",
                format_duration(entry.start_ms),
                entry.source.label()
            ),
            LINE_CHARS,
        );
        // Each line but the first also needs its newline.
        let cost = line.len() + usize::from(!lines.is_empty());
        if used + cost > max_chars && !lines.is_empty() {
            break;
        }
        used += cost;
        lines.push(line);
    }
    lines.reverse();
    lines.join("\n")
}

/// `text` cut at a character boundary to `max` bytes, marked as cut.
fn truncate(text: String, max: usize) -> String {
    if text.len() <= max {
        return text;
    }
    let mut end = max;
    while !text.is_char_boundary(end) {
        end -= 1;
    }
    format!("{}…", &text[..end])
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::meeting::MeetingSource;

    fn line(source: MeetingSource, seconds: u64, text: &str) -> TranscriptEntry {
        TranscriptEntry {
            source,
            start_ms: seconds * 1_000,
            end_ms: seconds * 1_000 + 1_000,
            text: text.into(),
        }
    }

    #[test]
    fn the_excerpt_keeps_the_latest_speech_when_it_must_trim() {
        let entries = [
            line(MeetingSource::System, 10, "Who owns the contract?"),
            line(MeetingSource::Microphone, 20, "Sam does."),
            line(MeetingSource::System, 30, "Client X signs on Friday."),
        ];
        let text = recent_excerpt(&entries, 60);
        assert!(text.contains("Client X signs on Friday."), "{text}");
        assert!(!text.contains("Who owns the contract?"), "{text}");
        assert!(text.ends_with("Client X signs on Friday."), "{text}");
    }

    #[test]
    fn a_line_carries_its_time_and_speaker() {
        let entries = [line(MeetingSource::Microphone, 65, "Sam owns it.")];
        let text = recent_excerpt(&entries, EXCERPT_CHARS);
        assert_eq!(text, "[01:05] You: Sam owns it.");
    }

    #[test]
    fn a_monologue_is_cut_inside_its_line() {
        let entries = [line(MeetingSource::System, 0, &"word ".repeat(400))];
        let text = recent_excerpt(&entries, EXCERPT_CHARS);
        assert!(text.ends_with('…'), "{}", text.len());
        assert!(text.len() <= LINE_CHARS + 8, "{}", text.len());
    }

    #[test]
    fn the_whole_transcript_fits_when_it_is_short() {
        let entries = [
            line(MeetingSource::System, 0, "One."),
            line(MeetingSource::Microphone, 5, "Two."),
        ];
        assert_eq!(
            recent_excerpt(&entries, EXCERPT_CHARS),
            "[00:00] Them: One.\n[00:05] You: Two."
        );
    }

    #[test]
    fn an_empty_question_is_refused_before_reading_the_store() {
        let cancelled = AtomicBool::new(false);
        let error = answer("no-such-meeting", "   ", &cancelled).unwrap_err();
        assert!(error.to_string().contains("Type a question"), "{error}");
    }
}
