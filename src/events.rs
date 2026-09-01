use std::fs::{self, File, OpenOptions};
use std::io::{self, BufWriter, Read, Seek, SeekFrom, Write};
#[cfg(unix)]
use std::os::unix::fs::MetadataExt;
use std::path::{Path, PathBuf};
use std::sync::mpsc::{self, SyncSender, TrySendError};
use std::sync::{Arc, Mutex};
use std::thread::{self, JoinHandle};
use std::time::{SystemTime, UNIX_EPOCH};

use std::collections::VecDeque;

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
    ApiServerStarted {
        timestamp_ms: u64,
        port: u16,
    },
    ApiServerStopped {
        timestamp_ms: u64,
    },
    ApiAuthFailed {
        timestamp_ms: u64,
        method: String,
        path: String,
    },
}

impl VoiceEvent {
    pub fn timestamp_ms(&self) -> u64 {
        match self {
            Self::SessionStarted { timestamp_ms }
            | Self::State { timestamp_ms, .. }
            | Self::Transcript { timestamp_ms, .. }
            | Self::Command { timestamp_ms, .. }
            | Self::Dictation { timestamp_ms, .. }
            | Self::Context { timestamp_ms, .. }
            | Self::ApiServerStarted { timestamp_ms, .. }
            | Self::ApiServerStopped { timestamp_ms }
            | Self::ApiAuthFailed { timestamp_ms, .. } => *timestamp_ms,
        }
    }

    pub fn kind(&self) -> &'static str {
        match self {
            Self::SessionStarted { .. } => "session_started",
            Self::State { .. } => "state",
            Self::Transcript { .. } => "transcript",
            Self::Command { .. } => "command",
            Self::Dictation { .. } => "dictation",
            Self::Context { .. } => "context",
            Self::ApiServerStarted { .. } => "api_server_started",
            Self::ApiServerStopped { .. } => "api_server_stopped",
            Self::ApiAuthFailed { .. } => "api_auth_failed",
        }
    }
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
    VoiceAction,
    // Retained so activity readers can decode records written by older releases.
    Logged,
    Repasted,
    Rewritten,
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

#[derive(Clone)]
pub struct EventLog {
    inner: Arc<EventLogInner>,
}

struct EventLogInner {
    sender: Option<SyncSender<WriterMessage>>,
    error: Arc<Mutex<Option<(io::ErrorKind, String)>>>,
    worker: Option<JoinHandle<()>>,
}

enum WriterMessage {
    Event(VoiceEvent),
    Flush(mpsc::SyncSender<Option<(io::ErrorKind, String)>>),
}

pub struct EventReader {
    path: PathBuf,
    offset: u64,
    pending: Vec<u8>,
    events: VecDeque<VoiceEvent>,
    current_context: Option<(Option<String>, Option<String>)>,
    #[cfg(unix)]
    file_id: Option<(u64, u64)>,
}

const EVENT_RETENTION: usize = 1_024;
const EVENT_WRITER_CAPACITY: usize = 1_024;

impl EventLog {
    pub fn create(path: &Path) -> io::Result<Self> {
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent)?;
        }
        let file = OpenOptions::new().create(true).append(true).open(path)?;
        let (sender, receiver) = mpsc::sync_channel(EVENT_WRITER_CAPACITY);
        let error = Arc::new(Mutex::new(None));
        let worker_error = error.clone();
        let worker = thread::Builder::new()
            .name("event-writer".into())
            .spawn(move || run_event_writer(BufWriter::new(file), receiver, worker_error))?;
        Ok(Self {
            inner: Arc::new(EventLogInner {
                sender: Some(sender),
                error,
                worker: Some(worker),
            }),
        })
    }

    pub fn emit(&self, event: &VoiceEvent) -> io::Result<()> {
        self.check_error()?;
        let sender = self
            .inner
            .sender
            .as_ref()
            .ok_or_else(|| io::Error::new(io::ErrorKind::BrokenPipe, "event writer stopped"))?;
        match sender.try_send(WriterMessage::Event(event.clone())) {
            Ok(()) => Ok(()),
            Err(TrySendError::Full(WriterMessage::Event(event))) if event.is_replaceable() => {
                Ok(())
            }
            Err(TrySendError::Full(message)) => sender
                .send(message)
                .map_err(|_| self.writer_stopped_error()),
            Err(TrySendError::Disconnected(_)) => Err(self.writer_stopped_error()),
        }
    }

    pub fn flush(&self) -> io::Result<()> {
        self.check_error()?;
        let (reply, flushed) = mpsc::sync_channel(0);
        self.inner
            .sender
            .as_ref()
            .ok_or_else(|| self.writer_stopped_error())?
            .send(WriterMessage::Flush(reply))
            .map_err(|_| self.writer_stopped_error())?;
        match flushed.recv().map_err(|_| self.writer_stopped_error())? {
            Some((kind, message)) => Err(io::Error::new(kind, message)),
            None => Ok(()),
        }
    }

    fn check_error(&self) -> io::Result<()> {
        match self
            .inner
            .error
            .lock()
            .unwrap_or_else(|error| error.into_inner())
            .as_ref()
        {
            Some((kind, message)) => Err(io::Error::new(*kind, message.clone())),
            None => Ok(()),
        }
    }

    fn writer_stopped_error(&self) -> io::Error {
        self.check_error()
            .err()
            .unwrap_or_else(|| io::Error::new(io::ErrorKind::BrokenPipe, "event writer stopped"))
    }

    pub fn dictation(&self, phase: DictationPhase, text: impl Into<String>) -> io::Result<()> {
        self.emit(&VoiceEvent::Dictation {
            timestamp_ms: now_ms(),
            phase,
            text: text.into(),
            processing: None,
        })
    }

    pub fn processed_dictation(
        &self,
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

impl VoiceEvent {
    fn is_replaceable(&self) -> bool {
        matches!(
            self,
            Self::Transcript {
                phase: TranscriptPhase::Started | TranscriptPhase::Updated,
                ..
            } | Self::Context { .. }
        )
    }
}

impl Drop for EventLogInner {
    fn drop(&mut self) {
        self.sender.take();
        if let Some(worker) = self.worker.take() {
            let _ = worker.join();
        }
    }
}

fn run_event_writer(
    mut writer: BufWriter<File>,
    receiver: mpsc::Receiver<WriterMessage>,
    error: Arc<Mutex<Option<(io::ErrorKind, String)>>>,
) {
    while let Ok(message) = receiver.recv() {
        match message {
            WriterMessage::Event(event) => {
                let result = serde_json::to_writer(&mut writer, &event)
                    .map_err(io::Error::other)
                    .and_then(|()| writer.write_all(b"\n"))
                    .and_then(|()| writer.flush());
                if let Err(write_error) = result {
                    *error.lock().unwrap_or_else(|error| error.into_inner()) =
                        Some((write_error.kind(), write_error.to_string()));
                    break;
                }
            }
            WriterMessage::Flush(reply) => {
                let failure = writer
                    .flush()
                    .err()
                    .map(|write_error| (write_error.kind(), write_error.to_string()));
                if let Some(failure) = &failure {
                    *error.lock().unwrap_or_else(|error| error.into_inner()) =
                        Some(failure.clone());
                }
                let failed = failure.is_some();
                let _ = reply.send(failure);
                if failed {
                    break;
                }
            }
        }
    }
    let _ = writer.flush();
}

impl EventReader {
    pub fn open(path: impl Into<PathBuf>) -> Self {
        Self {
            path: path.into(),
            offset: 0,
            pending: Vec::new(),
            events: VecDeque::new(),
            current_context: None,
            #[cfg(unix)]
            file_id: None,
        }
    }

    pub fn refresh(&mut self) -> io::Result<()> {
        let metadata = match fs::metadata(&self.path) {
            Ok(metadata) => metadata,
            Err(error) if error.kind() == io::ErrorKind::NotFound => {
                self.reset();
                return Ok(());
            }
            Err(error) => return Err(error),
        };
        #[cfg(unix)]
        {
            let file_id = (metadata.dev(), metadata.ino());
            if self.file_id.is_some_and(|current| current != file_id) {
                self.reset();
            }
            self.file_id = Some(file_id);
        }
        if metadata.len() < self.offset {
            self.reset();
        }
        let mut file = File::open(&self.path)?;
        file.seek(SeekFrom::Start(self.offset))?;
        let mut appended = Vec::new();
        file.read_to_end(&mut appended)?;
        self.offset += appended.len() as u64;
        self.pending.extend_from_slice(&appended);

        let Some(complete) = self
            .pending
            .iter()
            .rposition(|byte| *byte == b'\n')
            .map(|position| position + 1)
        else {
            return Ok(());
        };
        let records: Vec<_> = self.pending.drain(..complete).collect();
        for line in records.split(|byte| *byte == b'\n') {
            let line = line.strip_suffix(b"\r").unwrap_or(line);
            if line.is_empty() {
                continue;
            }
            let Ok(event) = serde_json::from_slice(line) else {
                continue;
            };
            self.push(event);
        }
        Ok(())
    }

    pub fn events(&self) -> &VecDeque<VoiceEvent> {
        &self.events
    }

    pub fn recent(&self, limit: usize) -> Vec<VoiceEvent> {
        self.events.iter().rev().take(limit).cloned().collect()
    }

    pub fn current_context(&self) -> Option<&(Option<String>, Option<String>)> {
        self.current_context.as_ref()
    }

    pub fn path(&self) -> &Path {
        &self.path
    }

    fn push(&mut self, event: VoiceEvent) {
        if matches!(event, VoiceEvent::SessionStarted { .. }) {
            self.events.clear();
            self.current_context = None;
        }
        if let VoiceEvent::Context {
            application,
            browser_url,
            ..
        } = &event
        {
            self.current_context = Some((application.clone(), browser_url.clone()));
        }
        self.events.push_back(event);
        while self.events.len() > EVENT_RETENTION {
            self.events.pop_front();
        }
    }

    fn reset(&mut self) {
        self.offset = 0;
        self.pending.clear();
        self.events.clear();
        self.current_context = None;
        #[cfg(unix)]
        {
            self.file_id = None;
        }
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

    #[test]
    fn reader_tails_complete_records_and_resets_at_session_boundaries() {
        let path = std::env::temp_dir().join(format!(
            "voice-control-event-reader-{}-{}.ndjson",
            std::process::id(),
            now_ms()
        ));
        let mut reader = EventReader::open(&path);
        fs::write(
            &path,
            concat!(
                "{\"kind\":\"context\",\"timestamp_ms\":1,\"application\":\"Zed\",\"browser_url\":null}\n",
                "{\"kind\":\"state\",\"timestamp_ms\":2,\"state\":\"listening\",\"device\":\"Test\"}"
            ),
        )
        .unwrap();

        reader.refresh().unwrap();
        assert_eq!(reader.events().len(), 1);
        assert_eq!(
            reader.current_context().cloned(),
            Some((Some("Zed".into()), None))
        );

        let mut file = OpenOptions::new().append(true).open(&path).unwrap();
        writeln!(file).unwrap();
        writeln!(file, "{{\"kind\":\"session_started\",\"timestamp_ms\":3}}",).unwrap();
        writeln!(
            file,
            "{{\"kind\":\"state\",\"timestamp_ms\":4,\"state\":\"dictating\",\"device\":\"Test\"}}"
        )
        .unwrap();

        reader.refresh().unwrap();
        assert_eq!(reader.events().len(), 2);
        assert!(matches!(
            reader.events().front(),
            Some(VoiceEvent::SessionStarted { timestamp_ms: 3 })
        ));
        assert_eq!(reader.current_context(), None);
        fs::remove_file(path).unwrap();
    }
}
