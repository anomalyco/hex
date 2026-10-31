//! What writes meeting titles, summaries, and live chapters: Apple
//! Intelligence on a Mac that has it and allows it, otherwise the OpenCode
//! model when the user turned that on. OpenCode receives the transcript
//! excerpt, so it is opt-in on every OS.

use std::sync::atomic::AtomicBool;
use std::time::Duration;

use serde::Deserialize;
use serde::de::DeserializeOwned;

/// OpenCode's models take far more than any excerpt Hex sends.
const OPENCODE_CONTEXT_TOKENS: usize = 32_000;

#[derive(Clone, Debug, Default, Deserialize, Eq, PartialEq)]
pub struct GeneratedNotes {
    pub title: String,
    #[serde(default)]
    pub summary: String,
}

/// One live chapter pass: what the new lines are about, and whether that is
/// still the current chapter's topic.
#[derive(Clone, Debug, Default, Deserialize, Eq, PartialEq)]
pub struct GeneratedChapter {
    pub title: String,
    #[serde(default)]
    pub summary: String,
    #[serde(default)]
    pub same_topic: bool,
}

#[derive(Deserialize)]
pub(crate) struct GeneratedChapterLines {
    #[serde(default)]
    pub(crate) lines: Vec<i64>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum GenerationError {
    /// The prompt exceeded the model's context window.
    ContextWindow,
    /// The model refused the prompt or its guardrails blocked it.
    Refused,
    Failed(String),
}

impl std::fmt::Display for GenerationError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::ContextWindow => formatter.write_str("the transcript exceeded the model context"),
            Self::Refused => formatter.write_str("the model refused the transcript"),
            Self::Failed(message) => formatter.write_str(message),
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum Writer {
    #[cfg(target_os = "macos")]
    AppleIntelligence,
    OpenCode,
}

/// The writer for a meeting's notes now: the one the user turned on, or,
/// when asked for notes directly, Apple Intelligence wherever it is
/// available, since it never leaves the Mac. Looking for OpenCode asks its
/// running service over loopback, so call this off the UI thread.
fn writer() -> Result<Writer, String> {
    if let Some(writer) = chosen() {
        return Ok(writer);
    }
    #[cfg(target_os = "macos")]
    {
        let availability = crate::apple_intelligence::availability();
        if availability.is_available() {
            return Ok(Writer::AppleIntelligence);
        }
        if !crate::app_settings::meeting_notes_with_opencode() {
            return Err(availability.description().to_string());
        }
    }
    Err(if crate::app_settings::meeting_notes_with_opencode() {
        "OpenCode isn't running".into()
    } else {
        "Turn on Summarize with OpenCode to write meeting notes".into()
    })
}

/// The writer the user turned on, when it is available.
fn chosen() -> Option<Writer> {
    #[cfg(target_os = "macos")]
    if crate::app_settings::meeting_notes_enabled()
        && crate::apple_intelligence::availability().is_available()
    {
        return Some(Writer::AppleIntelligence);
    }
    (crate::app_settings::meeting_notes_with_opencode() && crate::opencode_service::running())
        .then_some(Writer::OpenCode)
}

/// Whether finished meetings get notes, and recording ones titles,
/// without being asked.
pub(crate) fn automatic() -> bool {
    chosen().is_some()
}

/// Whether notes can be written when asked; the reason when they cannot.
pub(crate) fn available() -> Result<(), String> {
    writer().map(|_| ())
}

/// The writer's context window in tokens, or zero when none is available.
pub(crate) fn context_tokens() -> usize {
    match writer() {
        #[cfg(target_os = "macos")]
        Ok(Writer::AppleIntelligence) => crate::apple_intelligence::context_tokens(),
        Ok(Writer::OpenCode) => OPENCODE_CONTEXT_TOKENS,
        Err(_) => 0,
    }
}

/// A meeting title and a short summary.
pub(crate) fn generate_notes(
    instructions: &str,
    prompt: &str,
    timeout: Duration,
) -> Result<GeneratedNotes, GenerationError> {
    match writer().map_err(GenerationError::Failed)? {
        #[cfg(target_os = "macos")]
        Writer::AppleIntelligence => {
            crate::apple_intelligence::generate_notes(instructions, prompt, timeout)
        }
        Writer::OpenCode => opencode(
            instructions,
            prompt,
            r#"{"title": "...", "summary": "..."}"#,
            timeout,
        ),
    }
}

/// Only a meeting title.
pub(crate) fn generate_title(
    instructions: &str,
    prompt: &str,
    timeout: Duration,
) -> Result<String, GenerationError> {
    match writer().map_err(GenerationError::Failed)? {
        #[cfg(target_os = "macos")]
        Writer::AppleIntelligence => {
            crate::apple_intelligence::generate_title(instructions, prompt, timeout)
        }
        Writer::OpenCode => {
            opencode::<GeneratedNotes>(instructions, prompt, r#"{"title": "..."}"#, timeout)
                .map(|notes| notes.title)
        }
    }
}

/// One live chapter pass over new transcript lines.
pub(crate) fn generate_chapter(
    instructions: &str,
    prompt: &str,
    timeout: Duration,
) -> Result<GeneratedChapter, GenerationError> {
    match writer().map_err(GenerationError::Failed)? {
        #[cfg(target_os = "macos")]
        Writer::AppleIntelligence => {
            crate::apple_intelligence::generate_chapter(instructions, prompt, timeout)
        }
        Writer::OpenCode => opencode(
            instructions,
            prompt,
            r#"{"title": "...", "summary": "...", "same_topic": true}"#,
            timeout,
        ),
    }
}

/// The L numbers of the lines about the chapter after a boundary.
pub(crate) fn generate_chapter_lines(
    instructions: &str,
    prompt: &str,
    timeout: Duration,
) -> Result<Vec<i64>, GenerationError> {
    match writer().map_err(GenerationError::Failed)? {
        #[cfg(target_os = "macos")]
        Writer::AppleIntelligence => {
            crate::apple_intelligence::generate_chapter_lines(instructions, prompt, timeout)
        }
        Writer::OpenCode => opencode::<GeneratedChapterLines>(
            instructions,
            prompt,
            r#"{"lines": [12, 13]}"#,
            timeout,
        )
        .map(|generated| generated.lines),
    }
}

/// One OpenCode generation answered as JSON of `shape`.
fn opencode<T: DeserializeOwned>(
    instructions: &str,
    prompt: &str,
    shape: &str,
    timeout: Duration,
) -> Result<T, GenerationError> {
    let request = format!(
        "{instructions}\n\n{prompt}\n\nAnswer with only a JSON object shaped like {shape}, \
         without code fences or any other text."
    );
    let text =
        crate::dictation_processor::generate_text(&request, timeout, &AtomicBool::new(false))
            .map_err(|error| GenerationError::Failed(format!("{error:#}")))?;
    parse_json(&text)
}

/// The JSON object in a model's answer, ignoring code fences or prose
/// around it.
fn parse_json<T: DeserializeOwned>(text: &str) -> Result<T, GenerationError> {
    let start = text.find('{');
    let end = text.rfind('}');
    let object = match (start, end) {
        (Some(start), Some(end)) if start < end => &text[start..=end],
        _ => {
            return Err(GenerationError::Failed(
                "OpenCode answered without JSON".into(),
            ));
        }
    };
    serde_json::from_str(object).map_err(|error| {
        GenerationError::Failed(format!("OpenCode answered invalid JSON: {error}"))
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn answers_are_read_through_fences_and_prose() {
        let notes: GeneratedNotes = parse_json(
            "Here you go:\n```json\n{\"title\": \"Launch plan\", \"summary\": \"Dates set.\"}\n```",
        )
        .unwrap();
        assert_eq!(notes.title, "Launch plan");
        assert_eq!(notes.summary, "Dates set.");
        let lines: GeneratedChapterLines = parse_json("{\"lines\": [3, 4]}").unwrap();
        assert_eq!(lines.lines, [3, 4]);
        assert!(parse_json::<GeneratedNotes>("no json here").is_err());
    }
}
