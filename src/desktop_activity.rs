use crate::events::{DictationPhase, EventReader, VoiceEvent, VoiceState};

#[derive(Clone, Debug, Default, Eq, PartialEq, serde::Serialize, serde::Deserialize)]
pub(crate) struct DesktopActivity {
    pub(crate) session_started_at: Option<u64>,
    pub(crate) state: Option<VoiceState>,
    pub(crate) device: Option<String>,
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
                VoiceEvent::Dictation {
                    timestamp_ms,
                    phase: DictationPhase::Failed(message),
                    ..
                } if activity.last_failure.is_none() => {
                    activity.last_failure = Some((*timestamp_ms, message.clone()));
                }
                _ => {}
            }
        }
        activity
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn projects_latest_state_and_device() {
        let events = [
            VoiceEvent::SessionStarted { timestamp_ms: 1 },
            VoiceEvent::State {
                timestamp_ms: 2,
                state: VoiceState::Listening,
                device: "Old microphone".into(),
            },
            VoiceEvent::State {
                timestamp_ms: 3,
                state: VoiceState::Dictating,
                device: "Current microphone".into(),
            },
        ];

        let activity = DesktopActivity::from_events(events.iter().rev());

        assert_eq!(activity.session_started_at, Some(1));
        assert_eq!(activity.state, Some(VoiceState::Dictating));
        assert_eq!(activity.device.as_deref(), Some("Current microphone"));
    }

    #[test]
    fn session_boundary_excludes_older_state_and_device() {
        let mut events = vec![
            VoiceEvent::State {
                timestamp_ms: 1,
                state: VoiceState::Dictating,
                device: "Old microphone".into(),
            },
            VoiceEvent::SessionStarted { timestamp_ms: 2 },
        ];
        let activity = DesktopActivity::from_events(events.iter().rev());
        assert_eq!(activity.session_started_at, Some(2));
        assert_eq!(activity.state, None);
        assert_eq!(activity.device, None);

        events.push(VoiceEvent::State {
            timestamp_ms: 3,
            state: VoiceState::Listening,
            device: "Current microphone".into(),
        });
        let activity = DesktopActivity::from_events(events.iter().rev());
        assert_eq!(activity.state, Some(VoiceState::Listening));
        assert_eq!(activity.device.as_deref(), Some("Current microphone"));
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
