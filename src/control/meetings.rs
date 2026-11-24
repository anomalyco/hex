//! `hex meetings record|stop` and the meeting state `hex status` prints,
//! from the recorder state the app mirrors (`meeting::watcher::MeetingMenuState`).
//! Both the macOS host and the Linux and Windows host share these rules, so a
//! refusal reads the same wherever it reaches the recorder.

use crate::control::{ControlReply, MeetingReport};
use crate::meeting::MeetingRequest;
use crate::meeting::watcher::MeetingMenuState;

/// What `record` or `stop` asks the recorder for `state`, or the reply when
/// there is nothing to ask. Commands are idempotent: one the state already
/// answers changes nothing.
///
/// A recording whose stop is under way keeps reading as a recording until it
/// finishes transcribing, so `record` answers `busy` (a non-zero exit)
/// rather than "Already recording", which would hide that nothing started.
pub(crate) fn plan(
    state: &MeetingMenuState,
    stop: bool,
    title: Option<String>,
) -> Result<MeetingRequest, ControlReply> {
    match (state, stop) {
        (MeetingMenuState::Recording { stopping: true, .. }, false) => Err(ControlReply::error(
            "busy",
            "the last meeting is stopping; a new recording can start once it finishes transcribing",
        )),
        (MeetingMenuState::Recording { title, .. }, false) => Err(ControlReply::unchanged(
            format!("Already recording “{title}”"),
        )),
        (MeetingMenuState::Starting, false) => Err(ControlReply::unchanged(
            "A meeting recording is already starting",
        )),
        (MeetingMenuState::Transcribing, false) => Err(ControlReply::error(
            "busy",
            "the last meeting is still transcribing; try again when it finishes",
        )),
        (MeetingMenuState::Idle, false) => Ok(MeetingRequest::Start {
            title: title.filter(|title| !title.trim().is_empty()),
        }),
        (MeetingMenuState::Idle, true) => Err(ControlReply::unchanged("No meeting is recording")),
        (MeetingMenuState::Transcribing, true) => Err(ControlReply::unchanged(
            "The meeting is already transcribing",
        )),
        // The stop already went to the recorder: asking again starts nothing.
        (MeetingMenuState::Recording { stopping: true, .. }, true) => {
            Err(ControlReply::unchanged("The meeting is already stopping"))
        }
        (MeetingMenuState::Starting | MeetingMenuState::Recording { .. }, true) => {
            Ok(MeetingRequest::Stop)
        }
    }
}

/// The recorder as `hex status` reports it. `stopping` is a recording whose
/// stop is under way; it turns `transcribing` or `idle` once the recorder
/// finishes with it, and only then can a new recording start.
pub(crate) fn report(state: &MeetingMenuState) -> MeetingReport {
    let plain = |state: &str| MeetingReport {
        state: state.into(),
        ..Default::default()
    };
    match state {
        MeetingMenuState::Idle => plain("idle"),
        MeetingMenuState::Starting => plain("starting"),
        MeetingMenuState::Recording {
            started,
            title,
            stopping,
            ..
        } => MeetingReport {
            // A stop under way still names the recording, and keeps its
            // elapsed time: only `transcribing` or `idle` lets a new one start.
            state: (if *stopping { "stopping" } else { "recording" }).into(),
            title: Some(title.clone()),
            elapsed_seconds: Some(started.elapsed().as_secs()),
        },
        MeetingMenuState::Transcribing => plain("transcribing"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Instant;

    fn recording(stopping: bool) -> MeetingMenuState {
        MeetingMenuState::Recording {
            started: Instant::now(),
            id: "1700000000000".into(),
            title: "Design sync".into(),
            chapters: Vec::new(),
            warning: None,
            stopping,
        }
    }

    fn code(reply: &ControlReply) -> &str {
        match reply {
            ControlReply::Error { code, .. } => code,
            ControlReply::Done { changed: false, .. } => "unchanged",
            _ => "changed",
        }
    }

    #[test]
    fn a_recording_that_is_stopping_refuses_a_new_one_with_busy() {
        // The bug: this used to say "Already recording" and exit 0, so a
        // caller couldn't tell its recording never started.
        let Err(reply) = plan(&recording(true), false, None) else {
            panic!("a stopping meeting must not start another");
        };
        assert_eq!(code(&reply), "busy");
        assert!(matches!(
            &reply,
            ControlReply::Error { message, .. } if message.contains("stopping")
        ));
        // Stopping it again is harmless: nothing is asked of the recorder.
        let Err(reply) = plan(&recording(true), true, None) else {
            panic!("a stop already under way must not ask again");
        };
        assert_eq!(code(&reply), "unchanged");
        let Err(reply) = plan(&recording(false), false, None) else {
            panic!("a recording must not start another");
        };
        assert_eq!(code(&reply), "unchanged");
    }

    #[test]
    fn each_state_asks_the_recorder_for_one_thing() {
        assert_eq!(
            plan(&MeetingMenuState::Idle, false, Some("  ".into())).unwrap(),
            MeetingRequest::Start { title: None }
        );
        assert_eq!(
            plan(&MeetingMenuState::Idle, false, Some("Design sync".into())).unwrap(),
            MeetingRequest::Start {
                title: Some("Design sync".into())
            }
        );
        assert_eq!(
            plan(&recording(false), true, None).unwrap(),
            MeetingRequest::Stop
        );
        assert_eq!(
            plan(&MeetingMenuState::Starting, true, None).unwrap(),
            MeetingRequest::Stop
        );
        assert_eq!(
            code(&plan(&MeetingMenuState::Idle, true, None).unwrap_err()),
            "unchanged"
        );
        assert_eq!(
            code(&plan(&MeetingMenuState::Transcribing, true, None).unwrap_err()),
            "unchanged"
        );
        assert_eq!(
            code(&plan(&MeetingMenuState::Transcribing, false, None).unwrap_err()),
            "busy"
        );
    }

    #[test]
    fn status_names_a_stopping_recording_and_keeps_its_title() {
        let stopping = report(&recording(true));
        assert_eq!(stopping.state, "stopping");
        assert_eq!(stopping.title.as_deref(), Some("Design sync"));
        assert_eq!(stopping.elapsed_seconds, Some(0));
        assert_eq!(report(&recording(false)).state, "recording");
        assert_eq!(report(&MeetingMenuState::Idle).state, "idle");
        assert_eq!(report(&MeetingMenuState::Starting).state, "starting");
        assert_eq!(
            report(&MeetingMenuState::Transcribing).state,
            "transcribing"
        );
    }
}
