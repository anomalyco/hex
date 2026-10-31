use crate::events::{
    CommandOutcome, DictationPhase, EventReader, TranscriptPhase, VoiceEvent, VoiceState,
};

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
            VoiceState::Listening => "Enabled",
            VoiceState::Sleeping => "Sleeping",
            VoiceState::Dictating => "Recording",
            VoiceState::Transcribing => "Transcribing",
            VoiceState::Stopping => "Disabling…",
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

/// One line describing `event` for the Activity pane.
pub(crate) fn event_summary(event: &VoiceEvent) -> String {
    match event {
        VoiceEvent::SessionStarted { .. } => "Hex session started".into(),
        VoiceEvent::State { state, device, .. } => match state {
            VoiceState::Sleeping => "Voice commands went to sleep".into(),
            VoiceState::Listening => format!("Voice commands resumed on {device}"),
            VoiceState::Dictating => format!("Dictation started on {device}"),
            VoiceState::Transcribing => "Dictation is being transcribed".into(),
            VoiceState::Stopping => "Hex is stopping".into(),
        },
        VoiceEvent::Transcript {
            phase,
            latency_ms,
            text,
            ..
        } => match phase {
            TranscriptPhase::Started => "Speech recognition started".into(),
            TranscriptPhase::Updated => format!("Heard \"{text}\" so far"),
            TranscriptPhase::Completed => format!("Heard \"{text}\" in {latency_ms} ms"),
        },
        VoiceEvent::Command {
            heard,
            command,
            outcome,
            ..
        } => {
            let command = command.as_deref().unwrap_or("an unmatched command");
            match outcome {
                CommandOutcome::Ignored => format!("Ignored \"{heard}\""),
                CommandOutcome::Woke => format!("\"{heard}\" woke Hex"),
                CommandOutcome::Slept => format!("\"{heard}\" put voice commands to sleep"),
                CommandOutcome::Submitted => format!("Sent {command} for \"{heard}\""),
                CommandOutcome::Executed => format!("Ran {command} for \"{heard}\""),
                CommandOutcome::Failed(error) => format!("Could not run {command}: {error}"),
            }
        }
        VoiceEvent::Dictation {
            phase,
            text,
            processing,
            ..
        } => {
            let description = match phase {
                DictationPhase::Started => "Dictation started".into(),
                DictationPhase::Discarded => "A brief dictation was discarded".into(),
                DictationPhase::Cancelled => "Dictation was cancelled".into(),
                DictationPhase::Transcribing => "Dictation is being transcribed".into(),
                DictationPhase::Pasted => text_or("Dictation paste sent", text),
                DictationPhase::Printed => text_or("Dictation was printed", text),
                DictationPhase::Edited => text_or("Selected text was edited", text),
                DictationPhase::VoiceAction => text_or("Voice Action completed", text),
                DictationPhase::Logged => text_or("Journal entry was saved", text),
                DictationPhase::Repasted => text_or("The last dictation was pasted again", text),
                DictationPhase::MeetingPasted => text_or("New meeting transcript was pasted", text),
                DictationPhase::Failed(error) => format!("Dictation failed: {error}"),
            };
            processing
                .as_ref()
                .map_or(description.clone(), |processing| {
                    processing.fallback.as_ref().map_or_else(
                        || {
                            format!(
                                "{description} · {} profile · {} ms",
                                processing.profile, processing.latency_ms
                            )
                        },
                        |error| {
                            format!(
                                "{description} · {} profile fell back to raw after {} ms: {}",
                                processing.profile, processing.latency_ms, error
                            )
                        },
                    )
                })
        }
        VoiceEvent::Context {
            application,
            browser_url,
            ..
        } => match (application, browser_url) {
            (Some(application), Some(url)) => format!("Context changed to {application} / {url}"),
            (Some(application), None) => format!("Context changed to {application}"),
            _ => "Foreground context became unavailable".into(),
        },
        VoiceEvent::ApiServerStarted { port, .. } => {
            format!("Local API started on port {port}")
        }
        VoiceEvent::ApiServerStopped { .. } => "Local API stopped".into(),
        VoiceEvent::ApiAuthFailed { method, path, .. } => {
            format!("Rejected unauthenticated {method} {path}")
        }
    }
}

fn text_or(prefix: &str, text: &str) -> String {
    if text.is_empty() {
        prefix.into()
    } else {
        format!("{prefix}: \"{text}\"")
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
                reason: None,
            },
            VoiceEvent::Dictation {
                timestamp_ms: 3,
                phase: DictationPhase::Failed("second".into()),
                text: String::new(),
                processing: None,
                reason: None,
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
