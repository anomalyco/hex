//! One line of a meeting transcript, and how it is identified.

use serde::{Deserialize, Serialize};

use super::speakers::SpeakerId;

/// One transcribed line: the track it was heard on, its span on the
/// recording's clock, and its words.
#[derive(Clone, Debug, Deserialize, Eq, Hash, PartialEq, Serialize)]
pub struct TranscriptEntry {
    pub source: MeetingSource,
    pub start_ms: u64,
    pub end_ms: u64,
    pub text: String,
    /// Who said it, when Hex is sure: an id in the meeting's speaker table.
    /// Absent from the saved line otherwise, which is every line written
    /// before speakers existed.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub speaker: Option<SpeakerId>,
}

impl TranscriptEntry {
    /// A line as transcription hears it: nobody is named yet.
    pub fn new(source: MeetingSource, start_ms: u64, end_ms: u64, text: impl Into<String>) -> Self {
        Self {
            source,
            start_ms,
            end_ms,
            text: text.into(),
            speaker: None,
        }
    }
}

/// Identifies one published transcript line by its track and exact span,
/// not its text or its speaker: the Meetings pane carries these keys so an
/// edit deletes the lines it showed even if the words were rewritten or the
/// speaker was named meanwhile.
#[derive(Clone, Copy, Debug, Deserialize, Eq, Hash, PartialEq, Serialize)]
pub struct TranscriptEntryKey {
    pub source: MeetingSource,
    pub start_ms: u64,
    pub end_ms: u64,
}

impl TranscriptEntryKey {
    pub fn of(entry: &TranscriptEntry) -> Self {
        Self {
            source: entry.source,
            start_ms: entry.start_ms,
            end_ms: entry.end_ms,
        }
    }

    pub fn matches(self, entry: &TranscriptEntry) -> bool {
        self.source == entry.source
            && self.start_ms == entry.start_ms
            && self.end_ms == entry.end_ms
    }
}

/// The track a line was heard on: everyone else on the call, or the local
/// microphone. What a line is called is [`super::Labels`]' business.
#[derive(Clone, Copy, Debug, Deserialize, Eq, Hash, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum MeetingSource {
    System,
    Microphone,
}
