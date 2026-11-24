//! Stop when the call ends for a recording Hex did not start on its own
//! (Labs › Stop every call recording when the call ends): whether one stops,
//! and the one call app a recording started by hand watches.

use color_eyre::eyre::Result;

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

/// What a recording has seen of the call app it watches, so one whose app
/// joins late never counts an absence it never sat through.
#[derive(Default)]
pub(super) struct CallWatch {
    seen_present: bool,
}

impl CallWatch {
    /// Whether this pass reads the watched app's absence as the call ending,
    /// recording that a successful microphone read found the app on the
    /// input (`on_microphone`). A recording Hex started on request knows its
    /// app before that app joins — a calendar reminder records before the
    /// call is answered, and its app can be slow to launch — so its absence
    /// counts only once the app has been on the microphone. Playback and a
    /// failed read never mark it seen. `auto_stop` says whether that absence
    /// could stop it at all: with no stop to arm (the lab off, or Always
    /// record's setting off) the "left the call" notice shows every pass,
    /// exactly as before.
    pub(super) fn counts_absence(
        &mut self,
        start: CallEndStop,
        auto_stop: bool,
        on_microphone: bool,
    ) -> bool {
        self.seen_present |= on_microphone;
        match start {
            CallEndStop::Automatic => true,
            CallEndStop::OnRequest => !auto_stop || self.seen_present,
            CallEndStop::Unwatched => false,
        }
    }
}

/// One pass's reading of the call app a recording watches.
pub(super) struct WatchedPass {
    /// Whether the app counts as present: on the microphone, playing output
    /// while a stop could count it (muting keeps a call on the microphone in
    /// some apps and off it in others, but its output stays open until
    /// hang-up, #835; a browser seems to keep its output open past its call
    /// while the Mac is locked, #2685), or a read that failed, so an error
    /// never ends a recording.
    pub(super) present: bool,
    /// A successful read found it on the microphone. Only this, never
    /// playback or a failed read, marks the app seen for
    /// [`CallWatch::counts_absence`].
    pub(super) on_microphone: bool,
}

/// Reads the call app `application` for one pass. `locked` says the session
/// is locked or the Mac is asleep, where a browser's playback stops counting:
/// it seems to keep its output stream open after its call then (#2685). A
/// native call app keeps its playback counting, since some drop the
/// microphone on mute (#835). `auto_stop` says whether a stop could be
/// armed, so playback counts as present only when it could; `playing` is the
/// app's own audio-output read, made only when it could matter.
pub(super) fn watched_pass(
    application: &str,
    locked: bool,
    auto_stop: bool,
    applications: &Result<Vec<ActiveMicrophoneApplication>>,
    playing: impl FnOnce(&str) -> bool,
) -> WatchedPass {
    let on_microphone = match applications {
        Ok(applications) => Some(
            applications
                .iter()
                .any(|candidate| candidate.canonical_bundle_id() == Some(application)),
        ),
        Err(error) => {
            tracing::warn!(%error, "could not inspect microphone activity during a meeting");
            None
        }
    };
    // A browser seems to keep its output stream open after its call while the
    // Mac is locked (#2685), so its playback stops counting there. A native
    // call app keeps it: some drop the microphone on mute (#835).
    let playback_counts =
        auto_stop && !(locked && crate::microphone_activity::is_browser_id(Some(application)));
    WatchedPass {
        present: on_microphone.unwrap_or(true) || (playback_counts && playing(application)),
        on_microphone: on_microphone == Some(true),
    }
}

/// The call app a recording started by hand watches: the one native call app
/// holding the microphone, or nothing when none is or when more than one is
/// and they can't be told apart. Browsers are left out, since Hex reads a
/// page only for a call its own detection named.
pub(super) fn manual_call_app() -> Option<(String, String)> {
    manual_call_app_with(crate::labs::stop_any_call_recording(), || {
        active_microphone_applications().ok()
    })
}

/// [`manual_call_app`] over an explicit gate and a read given by the caller,
/// so a test can check that the lab off reads nothing.
fn manual_call_app_with(
    lab_on: bool,
    applications: impl FnOnce() -> Option<Vec<ActiveMicrophoneApplication>>,
) -> Option<(String, String)> {
    if !lab_on {
        return None;
    }
    only_call_app(&applications()?)
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
    fn a_calendar_recording_counts_no_absence_until_its_app_joins() {
        let zoom = vec![microphone("us.zoom.xos", "zoom.us")];
        let mut watch = CallWatch::default();
        // 33 s of a recording Hex started on request before its app joined
        // (a calendar reminder's Zoom call, not answered yet): every read
        // finds it off the microphone, and none counts, so nothing arms.
        for _ in 0..66 {
            assert!(
                !watch.counts_absence(CallEndStop::OnRequest, true, false),
                "an absence counted before the app was ever on the microphone"
            );
        }
        // The app plays audio — a Zoom window waiting to join, a Slack
        // notification — but has not taken the microphone: present to the
        // controller, still not seen, so 33 more seconds count nothing.
        let playing = watched_pass(
            "com.tinyspeck.slackmacgap",
            false,
            true,
            &Ok(zoom.clone()),
            |_| true,
        );
        assert!(playing.present, "playback keeps the call present");
        assert!(!playing.on_microphone);
        for _ in 0..66 {
            assert!(!watch.counts_absence(CallEndStop::OnRequest, true, playing.on_microphone));
        }
        // A read that failed is present too, and never marks the app seen.
        let failed: Result<Vec<ActiveMicrophoneApplication>> =
            Err(color_eyre::eyre::eyre!("no audio"));
        let unread = watched_pass("us.zoom.xos", false, true, &failed, |_| false);
        assert!(unread.present);
        assert!(!unread.on_microphone);
        assert!(!watch.counts_absence(CallEndStop::OnRequest, true, unread.on_microphone));

        // The app joins, then leaves: its absence counts from then on, and
        // the call coming back keeps counting after it leaves again.
        let joined = watched_pass("us.zoom.xos", false, true, &Ok(zoom), |_| false);
        assert!(joined.on_microphone);
        assert!(watch.counts_absence(CallEndStop::OnRequest, true, joined.on_microphone));
        assert!(watch.counts_absence(CallEndStop::OnRequest, true, false));
        assert!(watch.counts_absence(CallEndStop::OnRequest, true, true));
        assert!(watch.counts_absence(CallEndStop::OnRequest, true, false));

        // With no stop to arm (the lab off) the notice shows every pass, as
        // before, and Always record counts from its first pass, as before.
        let mut lab_off = CallWatch::default();
        assert!(lab_off.counts_absence(CallEndStop::OnRequest, false, false));
        let mut automatic = CallWatch::default();
        assert!(automatic.counts_absence(CallEndStop::Automatic, true, false));
        assert!(automatic.counts_absence(CallEndStop::Automatic, false, false));
    }

    #[test]
    fn a_locked_mac_stops_only_a_browser_s_playback_counting() {
        let none: Result<Vec<ActiveMicrophoneApplication>> = Ok(Vec::new());
        // A browser seems to keep its output stream open after its call while
        // the Mac is locked, and its microphone is already released: the call
        // ended.
        let mut played = 0;
        let locked = watched_pass("company.thebrowser.Browser", true, true, &none, |_| {
            played += 1;
            true
        });
        assert!(
            !locked.present,
            "a browser's playback kept its call present while the Mac was locked"
        );
        assert_eq!(played, 0, "playback was read when it could not matter");
        assert!(!locked.on_microphone);

        // A native call app keeps its playback: some drop the microphone on
        // mute (#835).
        let mut played = 0;
        let native = watched_pass("us.zoom.xos", true, true, &none, |_| {
            played += 1;
            true
        });
        assert!(
            native.present,
            "a native call app's playback stopped counting while locked"
        );
        assert_eq!(played, 1);

        // Locked with the browser still on the microphone: present, not ended.
        let held = watched_pass(
            "company.thebrowser.Browser",
            true,
            true,
            &Ok(vec![microphone("company.thebrowser.Browser", "Arc")]),
            |_| false,
        );
        assert!(held.present, "the microphone still counts while locked");
        assert!(held.on_microphone);

        // Unlocked, a browser's playback counts while a stop could be armed...
        let mut played = 0;
        let awake = watched_pass("company.thebrowser.Browser", false, true, &none, |_| {
            played += 1;
            true
        });
        assert!(awake.present);
        assert_eq!(played, 1);

        // ...and is not read when none could.
        let mut played = 0;
        let idle = watched_pass("company.thebrowser.Browser", false, false, &none, |_| {
            played += 1;
            true
        });
        assert!(!idle.present);
        assert_eq!(played, 0);
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
    fn a_by_hand_recording_reads_no_microphone_while_its_lab_is_off() {
        let mut read = false;
        assert_eq!(
            manual_call_app_with(false, || {
                read = true;
                Some(vec![microphone("us.zoom.xos", "zoom.us")])
            }),
            None
        );
        assert!(!read, "the microphone was read with the lab off");
        assert_eq!(
            manual_call_app_with(true, || Some(vec![microphone("us.zoom.xos", "zoom.us")])),
            Some(("us.zoom.xos".into(), "Zoom".into()))
        );
        // Nothing on the microphone, or a read that failed, watches nothing.
        assert_eq!(manual_call_app_with(true, || Some(Vec::new())), None);
        assert_eq!(manual_call_app_with(true, || None), None);
    }

    #[test]
    fn windows_and_linux_executables_link_to_their_canonical_call_app() {
        // The microphone adapter names a process by its executable off a Mac;
        // it maps to the same key detection and an offer use.
        for (executable, canonical, label) in [
            ("ms-teams.exe", "com.microsoft.teams2", "Microsoft Teams"),
            ("teams-for-linux", "com.microsoft.teams2", "Microsoft Teams"),
            ("zoom", "us.zoom.xos", "Zoom"),
        ] {
            assert_eq!(
                manual_call_app_with(true, || Some(vec![microphone(executable, executable)])),
                Some((canonical.into(), label.into())),
                "{executable}"
            );
        }
        // A browser keeps its own page watch, so a by-hand recording watches
        // nothing, and two different call apps can't be told apart.
        assert_eq!(
            manual_call_app_with(true, || Some(vec![microphone("chrome", "Google Chrome")])),
            None
        );
        assert_eq!(
            manual_call_app_with(true, || Some(vec![
                microphone("zoom", "zoom"),
                microphone("ms-teams.exe", "ms-teams.exe"),
            ])),
            None
        );
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
            Some(("com.tinyspeck.slackmacgap".into(), "Slack huddle".into()))
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
