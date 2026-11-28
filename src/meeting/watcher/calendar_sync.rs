//! Calendar meetings on the meeting controller: what a recording takes from
//! an event (title, attendees, call app) and what an answered reminder
//! snoozes in call detection.

use crate::calendar::CalendarMeeting;
use crate::meeting::{self, MeetingCalendar, MeetingStart};

use super::{MeetingDetector, Start};

/// A recording for a calendar meeting: its title, attendees, and call app.
pub(super) fn calendar_start(meeting: &CalendarMeeting) -> Start {
    let provider = meeting.link.as_ref().map(|link| link.provider);
    Start {
        meeting: MeetingStart {
            title: Some(meeting.display_title()),
            app: provider.map(|provider| provider.label().to_string()),
            application: provider
                .and_then(|provider| provider.application())
                .map(str::to_string),
            calendar: Some(calendar_context(meeting)),
            title_source: meeting::TitleSource::Calendar,
        },
        title: meeting.display_title(),
        app_label: provider
            .map_or("Meeting", |provider| provider.label())
            .into(),
        call: None,
        switched_from: None,
        watched: None,
    }
}

pub(super) fn calendar_context(meeting: &CalendarMeeting) -> MeetingCalendar {
    MeetingCalendar {
        title: meeting.display_title(),
        attendees: meeting.attendees.clone(),
    }
}

/// What a calendar meeting's answered reminder covers: its call service's
/// detected-call keys until the meeting ends. Without a call link, the next
/// call of any service, but only around the scheduled start, so one
/// dismissal never silences other calls for the rest of a long event.
pub(super) fn calendar_snooze(meeting: &CalendarMeeting) -> (Vec<String>, i64) {
    match &meeting.link {
        Some(link) => (
            link.provider
                .detection_keys()
                .iter()
                .map(|key| key.to_string())
                .collect(),
            meeting.end_ms,
        ),
        None => (
            Vec::new(),
            (meeting.start_ms + crate::calendar::LATE_GRACE_MS).min(meeting.end_ms),
        ),
    }
}

pub(super) fn snooze_calendar_call(detector: &mut MeetingDetector, meeting: &CalendarMeeting) {
    let (keys, until_ms) = calendar_snooze(meeting);
    detector.snooze_calendar_call(keys, until_ms);
}
