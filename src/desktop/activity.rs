use crate::desktop::events::{
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

    /// The newest failure at or after `started_at`, hidden once a later
    /// dictation completed. Startup reads an event log that may still end in
    /// the previous session, so its own start time filters that out.
    #[cfg_attr(not(any(target_os = "macos", target_os = "windows")), allow(dead_code))]
    pub(crate) fn visible_failure_since(
        &self,
        started_at: u64,
        last_completed_at: Option<u64>,
    ) -> Option<String> {
        self.last_failure
            .as_ref()
            .filter(|(failed_at, _)| {
                *failed_at >= started_at
                    && last_completed_at.is_none_or(|completed| completed < *failed_at)
            })
            .map(|(_, message)| message.clone())
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

/// When the current session's newest dictation completed. `events` is oldest
/// first, as `EventReader` keeps them.
#[cfg_attr(not(any(target_os = "macos", target_os = "windows")), allow(dead_code))]
pub(crate) fn last_completed_at<'a>(
    events: impl DoubleEndedIterator<Item = &'a VoiceEvent>,
) -> Option<u64> {
    for event in events.rev() {
        match event {
            VoiceEvent::SessionStarted { .. } => return None,
            VoiceEvent::Transcript {
                timestamp_ms,
                phase: TranscriptPhase::Completed,
                ..
            } => return Some(*timestamp_ms),
            _ => {}
        }
    }
    None
}

/// A known reason a dictation failed, in words a person can act on. Unknown
/// failures stay as they were logged.
pub(crate) fn failure_explanation(message: &str) -> String {
    if message.contains(crate::audio::LID_CLOSED_ERROR) {
        crate::desktop::i18n::tr(
            "Built-in mic is off with the lid closed. Open the lid or connect a mic.",
        )
        .to_owned()
    } else if message.contains(crate::audio::NO_MICROPHONE_ERROR) {
        crate::desktop::i18n::tr("No microphone is available. Connect a microphone.").to_owned()
    } else {
        message.to_owned()
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
            TranscriptPhase::Updated if text.is_empty() => "Heard speech so far".into(),
            TranscriptPhase::Updated => format!("Heard \"{text}\" so far"),
            TranscriptPhase::Completed if text.is_empty() => {
                format!("Transcribed in {latency_ms} ms")
            }
            TranscriptPhase::Completed => format!("Heard \"{text}\" in {latency_ms} ms"),
        },
        VoiceEvent::Command {
            heard,
            command,
            outcome,
            ..
        } => {
            let command = command.as_deref().unwrap_or("an unmatched command");
            let heard = if heard.is_empty() { "speech" } else { heard };
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
            reason,
            ..
        } => {
            let description = match phase {
                DictationPhase::Started => "Dictation started".into(),
                DictationPhase::Discarded
                    if reason.as_deref()
                        == Some(crate::desktop::events::VOICE_PROMPT_NOTHING_REASON) =>
                {
                    crate::desktop::i18n::tr("The voice prompt asked for nothing").to_owned()
                }
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
        VoiceEvent::Context { application, .. } => match application {
            Some(application) => format!("Context changed to {application}"),
            None => "Foreground context became unavailable".into(),
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
    fn a_voice_prompt_nothing_discard_says_so() {
        let event = VoiceEvent::Dictation {
            timestamp_ms: 1,
            phase: DictationPhase::Discarded,
            text: String::new(),
            processing: None,
            reason: Some(crate::desktop::events::VOICE_PROMPT_NOTHING_REASON.into()),
        };
        assert_eq!(
            event_summary(&event),
            crate::desktop::i18n::tr("The voice prompt asked for nothing")
        );
    }

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

    #[test]
    fn newest_failure_shows_until_a_later_transcript_completes() {
        let activity = DesktopActivity {
            last_failure: Some((5, "OpenCode generation failed".into())),
            ..DesktopActivity::default()
        };
        assert_eq!(
            activity.visible_failure_since(0, Some(4)).as_deref(),
            Some("OpenCode generation failed")
        );
        assert_eq!(activity.visible_failure_since(0, Some(6)), None);
        assert_eq!(
            activity.visible_failure_since(0, None).as_deref(),
            Some("OpenCode generation failed")
        );
    }

    #[test]
    fn the_newest_completion_is_scoped_to_the_current_session() {
        let transcript = |timestamp_ms, text| VoiceEvent::Transcript {
            timestamp_ms,
            phase: TranscriptPhase::Completed,
            latency_ms: 10,
            text,
        };
        let events = [
            transcript(1, "Old session".into()),
            VoiceEvent::SessionStarted { timestamp_ms: 2 },
        ];
        assert_eq!(last_completed_at(events.iter()), None);
        let events = [
            VoiceEvent::SessionStarted { timestamp_ms: 2 },
            transcript(3, "First".into()),
            transcript(4, "Second".into()),
        ];
        assert_eq!(last_completed_at(events.iter()), Some(4));
    }

    #[test]
    fn a_startup_reader_ignores_the_previous_sessions_failure() {
        let activity = DesktopActivity {
            last_failure: Some((5, "Could not open microphone".into())),
            ..DesktopActivity::default()
        };
        assert_eq!(
            activity.visible_failure_since(6, None),
            None,
            "a failure before this runtime started is the previous session's"
        );
        assert_eq!(
            activity.visible_failure_since(5, None).as_deref(),
            Some("Could not open microphone")
        );
        assert_eq!(
            activity.visible_failure_since(5, Some(7)),
            None,
            "a later completed dictation still clears it"
        );
    }

    #[test]
    fn known_microphone_failures_get_an_actionable_explanation() {
        assert_eq!(
            failure_explanation(
                "Could not open microphone: the built-in microphone is off while the lid is closed"
            ),
            "Built-in mic is off with the lid closed. Open the lid or connect a mic."
        );
        assert_eq!(
            failure_explanation("Could not open microphone: no microphone is available"),
            "No microphone is available. Connect a microphone."
        );
        assert_eq!(
            failure_explanation("OpenCode generation failed"),
            "OpenCode generation failed"
        );
    }
}
