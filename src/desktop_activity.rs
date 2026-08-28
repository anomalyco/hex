use crate::events::{DictationPhase, EventReader, TranscriptPhase, VoiceEvent, VoiceState};

const TRANSCRIPT_LIMIT: usize = 8;

#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub(crate) struct DesktopActivity {
    pub(crate) session_started_at: Option<u64>,
    pub(crate) state: Option<VoiceState>,
    pub(crate) device: Option<String>,
    pub(crate) transcripts: Vec<String>,
    pub(crate) error: Option<String>,
    pub(crate) last_failure: Option<(u64, String)>,
}

impl DesktopActivity {
    pub(crate) fn refresh(&mut self, reader: &mut EventReader) {
        if let Err(error) = reader.refresh() {
            tracing::warn!(%error, path = %reader.path().display(), "could not refresh desktop activity");
            self.error = Some(error.to_string());
            return;
        }
        *self = Self::from_events(reader.events().iter().rev());
    }

    pub(crate) fn state_label(&self) -> Option<&'static str> {
        self.state.map(|state| match state {
            VoiceState::Listening => "Listening",
            VoiceState::Sleeping => "Sleeping",
            VoiceState::Dictating => "Dictating",
            VoiceState::Transcribing => "Transcribing",
            VoiceState::Stopping => "Stopping",
        })
    }

    fn from_events<'a>(events: impl Iterator<Item = &'a VoiceEvent>) -> Self {
        let mut activity = Self::default();
        for event in events {
            match event {
                VoiceEvent::SessionStarted { timestamp_ms } => {
                    activity.session_started_at = Some(*timestamp_ms);
                    break;
                }
                VoiceEvent::State { state, device, .. } if activity.state.is_none() => {
                    activity.state = Some(*state);
                    activity.device = Some(device.clone());
                }
                VoiceEvent::Transcript {
                    phase: TranscriptPhase::Completed,
                    text,
                    ..
                } if activity.transcripts.len() < TRANSCRIPT_LIMIT && !text.trim().is_empty() => {
                    activity.transcripts.push(text.clone());
                }
                VoiceEvent::Dictation {
                    timestamp_ms,
                    phase: DictationPhase::Failed(message),
                    ..
                } if activity.last_failure.is_none() => {
                    activity.last_failure = Some((*timestamp_ms, message.clone()));
                }
                _ => {}
            }
            if activity.session_started_at.is_some()
                && activity.state.is_some()
                && activity.transcripts.len() == TRANSCRIPT_LIMIT
            {
                break;
            }
        }
        activity
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn projects_latest_state_and_completed_transcripts() {
        let mut events = vec![
            VoiceEvent::SessionStarted { timestamp_ms: 1 },
            VoiceEvent::State {
                timestamp_ms: 2,
                state: VoiceState::Listening,
                device: "Old microphone".into(),
            },
        ];
        events.extend((0..10).map(|index| VoiceEvent::Transcript {
            timestamp_ms: index + 3,
            phase: TranscriptPhase::Completed,
            latency_ms: 10,
            text: format!("Transcript {index}"),
        }));
        events.push(VoiceEvent::State {
            timestamp_ms: 13,
            state: VoiceState::Dictating,
            device: "Current microphone".into(),
        });
        events.push(VoiceEvent::Transcript {
            timestamp_ms: 14,
            phase: TranscriptPhase::Updated,
            latency_ms: 10,
            text: "Partial".into(),
        });

        let activity = DesktopActivity::from_events(events.iter().rev());

        assert_eq!(activity.session_started_at, Some(1));
        assert_eq!(activity.state, Some(VoiceState::Dictating));
        assert_eq!(activity.device.as_deref(), Some("Current microphone"));
        assert_eq!(activity.transcripts.len(), TRANSCRIPT_LIMIT);
        assert_eq!(activity.transcripts[0], "Transcript 9");
        assert_eq!(activity.transcripts[7], "Transcript 2");
    }

    #[test]
    fn session_boundary_excludes_older_transcripts() {
        let events = [
            VoiceEvent::Transcript {
                timestamp_ms: 1,
                phase: TranscriptPhase::Completed,
                latency_ms: 10,
                text: "Old session".into(),
            },
            VoiceEvent::SessionStarted { timestamp_ms: 2 },
            VoiceEvent::State {
                timestamp_ms: 3,
                state: VoiceState::Listening,
                device: "Microphone".into(),
            },
            VoiceEvent::Transcript {
                timestamp_ms: 4,
                phase: TranscriptPhase::Completed,
                latency_ms: 10,
                text: "Current session".into(),
            },
        ];

        let activity = DesktopActivity::from_events(events.iter().rev());

        assert_eq!(activity.transcripts, ["Current session"]);
    }

    #[test]
    fn projects_the_latest_failure_only_from_the_current_session() {
        let mut events = vec![
            VoiceEvent::SessionStarted { timestamp_ms: 1 },
            VoiceEvent::Dictation {
                timestamp_ms: 2,
                phase: DictationPhase::Failed("first".into()),
                text: String::new(),
                processing: None,
            },
            VoiceEvent::Dictation {
                timestamp_ms: 3,
                phase: DictationPhase::Failed("second".into()),
                text: String::new(),
                processing: None,
            },
        ];
        assert_eq!(
            DesktopActivity::from_events(events.iter().rev()).last_failure,
            Some((3, "second".into()))
        );
        events.push(VoiceEvent::SessionStarted { timestamp_ms: 4 });
        assert!(
            DesktopActivity::from_events(events.iter().rev())
                .last_failure
                .is_none()
        );
    }
}
