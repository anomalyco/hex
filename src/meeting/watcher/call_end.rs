//! Stop when the call ends for a recording Hex did not start on its own
//! (Labs › Stop every call recording when the call ends): whether one stops,
//! and the one call app a recording started by hand watches.

use crate::dictation::context::ContextSnapshot;
use crate::meeting::detection::candidate_from_microphone;
use crate::microphone_activity::{ActiveMicrophoneApplication, active_microphone_applications};

/// How a recording came to be, for whether its call ending stops it.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) enum CallEndStop {
    /// Hex started it on its own (Always record).
    Automatic,
    /// Hex started it on request: an offer's Record, a calendar reminder, or
    /// by hand while one call app held the microphone.
    OnRequest,
    /// By hand, with no call app to watch.
    Unwatched,
}

impl CallEndStop {
    /// Which kind an `automatic` recording whose watched call app is
    /// `application` is.
    pub(super) const fn of(automatic: bool, application: Option<&str>) -> Self {
        match (automatic, application) {
            (true, _) => Self::Automatic,
            (false, Some(_)) => Self::OnRequest,
            (false, None) => Self::Unwatched,
        }
    }
}

/// Whether a recording stops on its own once its call ends. Meetings ›
/// Settings' Stop when the call ends governs one Hex started (Always
/// record), and the lab governs every other recording that watches a call
/// app. One with nothing to watch never stops itself.
pub(super) const fn stops_when_call_ends(
    start: CallEndStop,
    setting_on: bool,
    lab_on: bool,
) -> bool {
    match start {
        CallEndStop::Automatic => setting_on,
        CallEndStop::OnRequest => lab_on,
        CallEndStop::Unwatched => false,
    }
}

/// The call app a recording started by hand watches: the one native call app
/// holding the microphone, or nothing when none is or when more than one is
/// and they can't be told apart. Browsers are left out, since Hex reads a
/// page only for a call its own detection named.
pub(super) fn manual_call_app() -> Option<(String, String)> {
    if !crate::labs::stop_any_call_recording() {
        return None;
    }
    let applications = active_microphone_applications().ok()?;
    only_call_app(&applications)
}

/// One observation's call app: its canonical bundle ID and the label an
/// offer would name it by.
fn only_call_app(applications: &[ActiveMicrophoneApplication]) -> Option<(String, String)> {
    let mut found: Option<(String, String)> = None;
    for application in applications
        .iter()
        .filter(|application| !application.is_browser())
    {
        let Some(candidate) = candidate_from_microphone(application, &ContextSnapshot::default())
        else {
            continue;
        };
        let Some(application) = candidate.application else {
            continue;
        };
        if found
            .as_ref()
            .is_some_and(|(watched, _)| *watched != application)
        {
            return None;
        }
        found = Some((application, candidate.source.label().to_owned()));
    }
    found
}

#[cfg(test)]
mod tests {
    use super::*;

    fn microphone(bundle_id: &str, name: &str) -> ActiveMicrophoneApplication {
        ActiveMicrophoneApplication {
            pid: 0,
            bundle_id: bundle_id.into(),
            name: name.into(),
        }
    }

    #[test]
    fn the_lab_stops_on_request_recordings_and_the_setting_keeps_always_record() {
        use CallEndStop::*;
        // Off, every kind behaves as it always did.
        assert!(!stops_when_call_ends(Automatic, false, false));
        assert!(stops_when_call_ends(Automatic, true, false));
        assert!(!stops_when_call_ends(OnRequest, true, false));
        assert!(!stops_when_call_ends(Unwatched, true, false));
        // The lab alone stops an offer's, a calendar's, or a by-hand
        // recording that carries a call app.
        assert!(stops_when_call_ends(OnRequest, false, true));
        assert!(stops_when_call_ends(OnRequest, true, true));
        // Always record keeps its own setting, and a by-hand recording of no
        // call has nothing to watch.
        assert!(!stops_when_call_ends(Automatic, false, true));
        assert!(!stops_when_call_ends(Unwatched, false, true));
    }

    #[test]
    fn a_recording_kind_knows_whether_it_watches_a_call_app() {
        assert_eq!(CallEndStop::of(true, None), CallEndStop::Automatic);
        assert_eq!(
            CallEndStop::of(true, Some("us.zoom.xos")),
            CallEndStop::Automatic
        );
        assert_eq!(
            CallEndStop::of(false, Some("us.zoom.xos")),
            CallEndStop::OnRequest
        );
        assert_eq!(CallEndStop::of(false, None), CallEndStop::Unwatched);
    }

    #[test]
    fn one_native_call_app_is_watched_and_a_browser_or_two_are_not() {
        assert_eq!(only_call_app(&[]), None);
        assert_eq!(
            only_call_app(&[microphone("us.zoom.xos", "zoom.us")]),
            Some(("us.zoom.xos".into(), "Zoom".into()))
        );
        // Slack's audio helper folds to the app that owns the huddle.
        assert_eq!(
            only_call_app(&[
                microphone("com.tinyspeck.slackmacgap.helper", "Slack Helper"),
                microphone("com.tinyspeck.slackmacgap", "Slack"),
            ]),
            Some(("com.tinyspeck.slackmacgap".into(), "Slack Huddle".into()))
        );
        // Two calls Hex can't tell apart: neither is watched.
        assert_eq!(
            only_call_app(&[
                microphone("us.zoom.xos", "zoom.us"),
                microphone("com.tinyspeck.slackmacgap", "Slack"),
            ]),
            None
        );
        // A browser keeps its own page watch, and an app Hex doesn't know is
        // not a call.
        assert_eq!(
            only_call_app(&[microphone("com.google.Chrome", "Google Chrome")]),
            None
        );
        assert_eq!(
            only_call_app(&[microphone("com.apple.VoiceMemos", "Voice Memos")]),
            None
        );
    }
}
