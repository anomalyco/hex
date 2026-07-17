use std::fs::{self, File, OpenOptions};
use std::io::{self, BufWriter, Write};
use std::path::Path;
use std::time::{SystemTime, UNIX_EPOCH};

use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum VoiceEvent {
    SessionStarted {
        timestamp_ms: u64,
    },
    State {
        timestamp_ms: u64,
        state: VoiceState,
        device: String,
    },
    Transcript {
        timestamp_ms: u64,
        phase: TranscriptPhase,
        latency_ms: u32,
        text: String,
    },
    Command {
        timestamp_ms: u64,
        heard: String,
        command: Option<String>,
        outcome: CommandOutcome,
        #[serde(default)]
        context: String,
    },
    Dictation {
        timestamp_ms: u64,
        phase: DictationPhase,
        #[serde(default)]
        text: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        processing: Option<DictationProcessing>,
    },
    Context {
        timestamp_ms: u64,
        application: Option<String>,
        browser_url: Option<String>,
    },
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum VoiceState {
    Sleeping,
    Listening,
    Dictating,
    Transcribing,
    Stopping,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum DictationPhase {
    Started,
    Discarded,
    Cancelled,
    Transcribing,
    Pasted,
    Edited,
    Logged,
    Repasted,
    MeetingPasted,
    Failed(String),
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct DictationProcessing {
    pub profile: String,
    pub latency_ms: u64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub fallback: Option<String>,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum CommandOutcome {
    Ignored,
    Woke,
    Slept,
    Submitted,
    Executed,
    Failed(String),
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum TranscriptPhase {
    Started,
    Updated,
    Completed,
}

pub struct EventLog {
    writer: BufWriter<File>,
}

impl EventLog {
    pub fn create(path: &Path) -> io::Result<Self> {
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent)?;
        }
        let file = OpenOptions::new().create(true).append(true).open(path)?;
        Ok(Self {
            writer: BufWriter::new(file),
        })
    }

    pub fn emit(&mut self, event: &VoiceEvent) -> io::Result<()> {
        serde_json::to_writer(&mut self.writer, event)?;
        self.writer.write_all(b"\n")?;
        self.writer.flush()
    }

    pub fn dictation(&mut self, phase: DictationPhase, text: impl Into<String>) -> io::Result<()> {
        self.emit(&VoiceEvent::Dictation {
            timestamp_ms: now_ms(),
            phase,
            text: text.into(),
            processing: None,
        })
    }

    pub fn processed_dictation(
        &mut self,
        phase: DictationPhase,
        text: impl Into<String>,
        processing: Option<DictationProcessing>,
    ) -> io::Result<()> {
        self.emit(&VoiceEvent::Dictation {
            timestamp_ms: now_ms(),
            phase,
            text: text.into(),
            processing,
        })
    }
}

pub fn now_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis() as u64
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn events_round_trip_through_ndjson() {
        let event = VoiceEvent::Transcript {
            timestamp_ms: 42,
            phase: TranscriptPhase::Completed,
            latency_ms: 73,
            text: "Open Zed".into(),
        };

        let json = serde_json::to_string(&event).unwrap();
        let decoded: VoiceEvent = serde_json::from_str(&json).unwrap();

        assert!(matches!(
            decoded,
            VoiceEvent::Transcript {
                timestamp_ms: 42,
                phase: TranscriptPhase::Completed,
                latency_ms: 73,
                ref text,
            } if text == "Open Zed"
        ));
    }

    #[test]
    fn dictation_processing_observation_round_trips() {
        let event = VoiceEvent::Dictation {
            timestamp_ms: 42,
            phase: DictationPhase::Pasted,
            text: "Processed text".into(),
            processing: Some(DictationProcessing {
                profile: "slack".into(),
                latency_ms: 321,
                fallback: Some("deadline exceeded".into()),
            }),
        };

        let decoded: VoiceEvent =
            serde_json::from_str(&serde_json::to_string(&event).unwrap()).unwrap();

        assert!(matches!(
            decoded,
            VoiceEvent::Dictation {
                processing: Some(DictationProcessing {
                    profile,
                    latency_ms: 321,
                    fallback: Some(fallback),
                }),
                ..
            } if profile == "slack" && fallback == "deadline exceeded"
        ));
    }

    #[test]
    fn reopening_log_preserves_existing_events() {
        let path = std::env::temp_dir().join(format!(
            "voice-control-events-{}-{}.ndjson",
            std::process::id(),
            now_ms()
        ));
        let event = VoiceEvent::State {
            timestamp_ms: 1,
            state: VoiceState::Listening,
            device: "test".into(),
        };
        EventLog::create(&path).unwrap().emit(&event).unwrap();
        EventLog::create(&path).unwrap().emit(&event).unwrap();

        assert_eq!(fs::read_to_string(&path).unwrap().lines().count(), 2);
        fs::remove_file(path).unwrap();
    }
}
