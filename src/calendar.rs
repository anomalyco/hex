//! Calendar meetings for reminders: which events count as meetings, the call
//! link in an invitation, and when each reminder is due. The EventKit bridge
//! (`calendar.swift`) is macOS-only; the model here is plain data so Settings
//! fixtures render on every OS.
//!
//! Hex asks for calendar access only when the user clicks Connect calendar,
//! reads events locally through EventKit, and makes no network request.

use std::collections::HashMap;

use serde::Deserialize;
use url::Url;

/// How late a reminder may still show, for a Mac that wakes after the
/// meeting started.
pub const LATE_GRACE_MS: i64 = 5 * 60 * 1_000;
/// How long a reminder stays up after its meeting starts.
pub const REMINDER_AFTER_START_MS: i64 = 60 * 1_000;
/// The shortest time a reminder stays up, for one shown late.
pub const REMINDER_MIN_MS: i64 = 30 * 1_000;
/// How far ahead Hex reads events: enough for the reminder poll and the
/// Settings list.
pub const LOOKAHEAD_MS: i64 = 48 * 60 * 60 * 1_000;
/// The most events one read returns.
pub const MAX_EVENTS: i32 = 200;
/// The meetings Settings lists.
pub const UPCOMING_SHOWN: usize = 3;

/// macOS calendar access, as EventKit reports it.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub enum CalendarAccess {
    #[default]
    NotDetermined,
    Restricted,
    Denied,
    FullAccess,
    /// Add-only access, which cannot read events.
    WriteOnly,
}

impl CalendarAccess {
    pub fn from_code(code: i32) -> Self {
        match code {
            0 => Self::NotDetermined,
            1 => Self::Restricted,
            3 => Self::FullAccess,
            4 => Self::WriteOnly,
            _ => Self::Denied,
        }
    }

    /// Access was refused or limited, so only System Settings can grant it.
    pub fn blocked(self) -> bool {
        matches!(self, Self::Restricted | Self::Denied | Self::WriteOnly)
    }
}

/// macOS's answer to Connect calendar.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct AccessAnswer {
    /// EventKit's completion said yes.
    pub granted: bool,
    /// EventKit's status read right after the answer. It can still say
    /// "not determined" in the process that asked, until Hex relaunches.
    pub status: CalendarAccess,
    /// The request's error, such as a missing usage description.
    pub error: Option<String>,
}

impl AccessAnswer {
    /// The access the answer leaves: a grant is full access whatever the
    /// stale status says, and a refusal without an error is a denial even
    /// while the status still says "not determined".
    pub fn access(&self) -> CalendarAccess {
        match (self.granted, self.status) {
            (true, _) | (false, CalendarAccess::FullAccess) => CalendarAccess::FullAccess,
            (false, CalendarAccess::NotDetermined) if self.error.is_some() => {
                CalendarAccess::NotDetermined
            }
            (false, CalendarAccess::NotDetermined) => CalendarAccess::Denied,
            (false, status) => status,
        }
    }
}

/// One calendar the user can include or leave out.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq)]
pub struct CalendarInfo {
    pub id: String,
    pub title: String,
    /// The account it belongs to, such as iCloud or a Google address.
    #[serde(default)]
    pub source: String,
}

/// One event occurrence as EventKit reports it.
#[derive(Clone, Debug, Default, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct CalendarEvent {
    pub id: String,
    pub calendar: String,
    pub title: String,
    pub start_ms: f64,
    pub end_ms: f64,
    pub all_day: bool,
    /// Shown as free time.
    pub free: bool,
    pub cancelled: bool,
    /// The user declined the invitation.
    pub declined: bool,
    /// Everyone else invited, by name.
    pub attendees: Vec<String>,
    pub location: Option<String>,
    pub notes: Option<String>,
    pub url: Option<String>,
}

/// A video call service Hex recognizes in an invitation.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub enum CallProvider {
    Zoom,
    GoogleMeet,
    Teams,
    Slack,
    Webex,
}

impl CallProvider {
    pub fn label(self) -> &'static str {
        match self {
            Self::Zoom => "Zoom",
            Self::GoogleMeet => "Google Meet",
            Self::Teams => "Microsoft Teams",
            Self::Slack => "Slack",
            Self::Webex => "Webex",
        }
    }

    /// The Detected calls key whose Ask / Always record / Never offer
    /// choice applies to this service's calls.
    pub fn choice_key(self) -> Option<&'static str> {
        match self {
            Self::Zoom => Some("us.zoom.xos"),
            Self::GoogleMeet => Some("meet.google.com"),
            Self::Teams => Some("com.microsoft.teams2"),
            Self::Slack => Some("com.tinyspeck.slackmacgap"),
            Self::Webex => None,
        }
    }

    /// The desktop app that joins these calls, whose microphone use ends
    /// the call.
    pub fn application(self) -> Option<&'static str> {
        match self {
            Self::Zoom => Some("us.zoom.xos"),
            Self::Teams => Some("com.microsoft.teams2"),
            Self::Slack => Some("com.tinyspeck.slackmacgap"),
            Self::GoogleMeet | Self::Webex => None,
        }
    }

    /// Every app bundle ID and website host a detected call from this
    /// service can carry. A website matches its subdomains too.
    pub fn detection_keys(self) -> &'static [&'static str] {
        match self {
            Self::Zoom => &["us.zoom.xos", "zoom.us", "zoomgov.com"],
            Self::GoogleMeet => &["meet.google.com"],
            Self::Teams => &[
                "com.microsoft.teams2",
                "teams.microsoft.com",
                "teams.live.com",
            ],
            Self::Slack => &["com.tinyspeck.slackmacgap", "app.slack.com"],
            Self::Webex => &["webex.com"],
        }
    }
}

/// The call an invitation links to.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct CallLink {
    pub provider: CallProvider,
    pub url: String,
}

/// A calendar event that counts as a meeting.
#[derive(Clone, Debug, PartialEq)]
pub struct CalendarMeeting {
    pub id: String,
    pub title: String,
    pub start_ms: i64,
    pub end_ms: i64,
    pub attendees: Vec<String>,
    pub link: Option<CallLink>,
}

impl CalendarMeeting {
    /// One occurrence: a moved meeting is a new one to remind about.
    pub fn occurrence(&self) -> String {
        format!("{}@{}", self.id, self.start_ms)
    }

    /// The title, or a stand-in for an untitled event.
    pub fn display_title(&self) -> String {
        let title = self.title.trim();
        if title.is_empty() {
            "Meeting".into()
        } else {
            title.into()
        }
    }

    pub fn from_event(event: &CalendarEvent) -> Self {
        Self {
            id: event.id.clone(),
            title: event.title.trim().to_string(),
            start_ms: event.start_ms as i64,
            end_ms: event.end_ms as i64,
            attendees: event
                .attendees
                .iter()
                .map(|name| name.trim().to_string())
                .filter(|name| !name.is_empty())
                .collect(),
            link: call_link(event),
        }
    }
}

/// A meeting is an event with other people or a call link that the user
/// will attend: not all-day, declined, cancelled, or shown as free.
pub fn is_meeting(event: &CalendarEvent) -> bool {
    if event.all_day || event.declined || event.cancelled || event.free {
        return false;
    }
    if event.end_ms <= event.start_ms {
        return false;
    }
    event.attendees.iter().any(|name| !name.trim().is_empty()) || call_link(event).is_some()
}

/// The included meetings among `events`, soonest first.
pub fn meetings(events: &[CalendarEvent], includes: impl Fn(&str) -> bool) -> Vec<CalendarMeeting> {
    let mut meetings = events
        .iter()
        .filter(|event| includes(&event.calendar) && is_meeting(event))
        .map(CalendarMeeting::from_event)
        .collect::<Vec<_>>();
    meetings.sort_by_key(|meeting| meeting.start_ms);
    meetings
}

/// The next meetings that haven't ended, for Settings.
pub fn upcoming(meetings: &[CalendarMeeting], now_ms: i64) -> Vec<CalendarMeeting> {
    meetings
        .iter()
        .filter(|meeting| meeting.end_ms > now_ms)
        .take(UPCOMING_SHOWN)
        .cloned()
        .collect()
}

/// The first video call link in the event's URL, location, or notes.
pub fn call_link(event: &CalendarEvent) -> Option<CallLink> {
    [&event.url, &event.location, &event.notes]
        .into_iter()
        .flatten()
        .find_map(|text| links(text).find_map(|url| classify(&url)))
}

/// The http(s) URLs in free text, in order.
fn links(text: &str) -> impl Iterator<Item = Url> + '_ {
    let mut rest = text;
    std::iter::from_fn(move || {
        loop {
            let start = rest.find("http")?;
            let candidate = &rest[start..];
            let end = candidate
                .find(|character: char| {
                    character.is_whitespace() || "<>\"'()[]{}|\\^`".contains(character)
                })
                .unwrap_or(candidate.len());
            let token = candidate[..end].trim_end_matches(['.', ',', ';', ':', '!', '?']);
            if !(token.starts_with("https://") || token.starts_with("http://")) {
                // A link may follow inside this token, as in `http_link:https://…`.
                rest = &candidate[4..];
                continue;
            }
            rest = &candidate[end..];
            if let Ok(url) = Url::parse(token) {
                return Some(url);
            }
        }
    })
}

fn host_is(host: &str, domain: &str) -> bool {
    host == domain
        || host
            .strip_suffix(domain)
            .is_some_and(|prefix| prefix.ends_with('.'))
}

fn classify(url: &Url) -> Option<CallLink> {
    let host = url.host_str()?.to_ascii_lowercase();
    let path = url.path().to_ascii_lowercase();
    let provider = if (host_is(&host, "zoom.us") || host_is(&host, "zoomgov.com"))
        && ["/j/", "/my/", "/w/", "/s/"]
            .iter()
            .any(|prefix| path.starts_with(prefix))
    {
        CallProvider::Zoom
    } else if host == "meet.google.com" && is_meet_path(&path) {
        CallProvider::GoogleMeet
    } else if (host_is(&host, "teams.microsoft.com") || host_is(&host, "teams.live.com"))
        && (path.starts_with("/l/meetup-join/") || path.starts_with("/meet/"))
    {
        CallProvider::Teams
    } else if host_is(&host, "slack.com") && path.contains("/huddle") {
        CallProvider::Slack
    } else if host_is(&host, "webex.com")
        && (path.starts_with("/meet/")
            || path.starts_with("/join/")
            || path.ends_with("/j.php")
            || path.contains("/wbxmjs/joinservice/"))
    {
        CallProvider::Webex
    } else {
        return None;
    };
    Some(CallLink {
        provider,
        url: url.to_string(),
    })
}

/// A meeting code such as `/abc-defg-hij`, or a nickname path; not the
/// landing or new-meeting pages.
fn is_meet_path(path: &str) -> bool {
    let path = path.trim_matches('/');
    !path.is_empty()
        && !path.contains('/')
        && !matches!(path, "landing" | "new" | "_meet")
        && path
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || byte == b'-')
}

/// What the reminder capsule should do after a poll.
#[derive(Clone, Debug, PartialEq)]
pub enum ReminderUpdate {
    Show(CalendarMeeting),
    /// The showing reminder's meeting (this occurrence) was cancelled,
    /// moved, declined, or is long past: close it.
    Withdraw(String),
}

/// Decides when each meeting's reminder shows. Times are wall-clock
/// milliseconds, so a Mac that sleeps through the lead time still reminds on
/// wake if the meeting began less than [`LATE_GRACE_MS`] ago, and a moved
/// meeting (a new start) is reminded again.
#[derive(Default)]
pub struct ReminderScheduler {
    /// Occurrences already reminded, with when they end, so each reminds once.
    reminded: HashMap<String, i64>,
    /// The reminder on screen and when its capsule closes on its own.
    showing: Option<(CalendarMeeting, i64)>,
}

/// When a reminder shown at `shown_ms` closes on its own: a minute after its
/// meeting starts, and never sooner than [`REMINDER_MIN_MS`] after showing.
pub fn reminder_until_ms(start_ms: i64, shown_ms: i64) -> i64 {
    (start_ms + REMINDER_AFTER_START_MS).max(shown_ms + REMINDER_MIN_MS)
}

impl ReminderScheduler {
    /// One reminder shows at a time: while it is up, an overlapping
    /// meeting's reminder waits until it is answered or closes.
    pub fn poll(
        &mut self,
        meetings: &[CalendarMeeting],
        now_ms: i64,
        lead_ms: i64,
    ) -> Option<ReminderUpdate> {
        // Bounded: occurrences are forgotten an hour after they end.
        self.reminded
            .retain(|_, end_ms| *end_ms + 60 * 60 * 1_000 > now_ms);
        if let Some((showing, until_ms)) = &self.showing {
            let current = meetings
                .iter()
                .any(|meeting| meeting.occurrence() == showing.occurrence());
            if !current || now_ms >= *until_ms {
                let occurrence = showing.occurrence();
                self.showing = None;
                return Some(ReminderUpdate::Withdraw(occurrence));
            }
            return None;
        }
        let due = meetings.iter().find(|meeting| {
            now_ms >= meeting.start_ms - lead_ms
                && now_ms < meeting.start_ms + LATE_GRACE_MS
                && now_ms < meeting.end_ms
                && !self.reminded.contains_key(&meeting.occurrence())
        })?;
        self.reminded.insert(due.occurrence(), due.end_ms);
        self.showing = Some((due.clone(), reminder_until_ms(due.start_ms, now_ms)));
        Some(ReminderUpdate::Show(due.clone()))
    }

    /// The user answered the reminder, or it closed.
    pub fn answered(&mut self, occurrence: &str) {
        if self
            .showing
            .as_ref()
            .is_some_and(|(showing, _)| showing.occurrence() == occurrence)
        {
            self.showing = None;
        }
    }

    /// Reminders were turned off or a recording started: forget the showing
    /// one, returning its occurrence.
    pub fn clear(&mut self) -> Option<String> {
        self.showing.take().map(|(showing, _)| showing.occurrence())
    }
}

/// The meeting happening now that a detected call belongs to: one whose
/// link names the call's service, from its reminder lead until it ends.
pub fn current_for_call<'a>(
    meetings: &'a [CalendarMeeting],
    keys: &[&str],
    now_ms: i64,
    lead_ms: i64,
) -> Option<&'a CalendarMeeting> {
    meetings.iter().find(|meeting| {
        now_ms >= meeting.start_ms - lead_ms.max(5 * 60 * 1_000)
            && now_ms < meeting.end_ms
            && meeting.link.as_ref().is_some_and(|link| {
                link.provider
                    .detection_keys()
                    .iter()
                    .any(|known| keys.iter().any(|key| key_matches(key, known)))
            })
    })
}

/// Whether a detected call's app bundle ID or website host is `known`.
pub fn key_matches(key: &str, known: &str) -> bool {
    let key = key.to_ascii_lowercase();
    key == known || host_is(&key, known)
}

/// Whole minutes until `start_ms`, rounded up; zero once it has started.
pub fn minutes_until(start_ms: i64, now_ms: i64) -> i64 {
    ((start_ms - now_ms).max(0) + 59_999) / 60_000
}

/// What Settings shows of the calendar.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct CalendarSnapshot {
    pub access: CalendarAccess,
    pub calendars: Vec<CalendarInfo>,
    /// The next included meetings, read only once connected.
    pub upcoming: Vec<CalendarMeeting>,
    pub error: Option<String>,
}

/// Milliseconds since the Unix epoch. Reminders use the wall clock, which
/// keeps counting while the Mac sleeps.
pub fn wall_clock_ms() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|elapsed| elapsed.as_millis() as i64)
        .unwrap_or_default()
}

pub use native::*;

#[cfg(target_os = "macos")]
mod native {
    use std::ffi::{CStr, c_char};

    use color_eyre::eyre::{Result, eyre};

    use super::{
        AccessAnswer, CalendarAccess, CalendarEvent, CalendarInfo, CalendarMeeting,
        CalendarSnapshot, LATE_GRACE_MS, LOOKAHEAD_MS,
    };
    use crate::app_settings::CalendarSettings;

    unsafe extern "C" {
        fn hex_calendar_authorization() -> i32;
        fn hex_calendar_request_access(granted: *mut bool, error: *mut *mut c_char) -> i32;
        fn hex_calendar_change_count() -> u64;
        fn hex_calendar_calendars() -> *mut c_char;
        fn hex_calendar_events(from_ms: f64, to_ms: f64, limit: i32) -> *mut c_char;
        fn hex_calendar_free_string(string: *mut c_char);
    }

    pub fn access() -> CalendarAccess {
        // SAFETY: An EventKit status query without arguments.
        CalendarAccess::from_code(unsafe { hex_calendar_authorization() })
    }

    /// Shows the macOS prompt when access was never decided. Blocks until
    /// the user answers, so call it off the main thread.
    pub fn request_access() -> AccessAnswer {
        let mut granted = false;
        let mut error: *mut c_char = std::ptr::null_mut();
        // SAFETY: Blocks on EventKit's completion handler and writes both
        // out-pointers, which outlive the call.
        let status = CalendarAccess::from_code(unsafe {
            hex_calendar_request_access(&mut granted, &mut error)
        });
        let error = (!error.is_null()).then(|| {
            // SAFETY: The bridge returned a valid C string it allocated.
            let message = unsafe { CStr::from_ptr(error) }
                .to_string_lossy()
                .into_owned();
            // SAFETY: Freed once, by the allocator that made it.
            unsafe { hex_calendar_free_string(error) };
            message
        });
        AccessAnswer {
            granted,
            status,
            error,
        }
    }

    /// Bumps whenever the calendar database changes.
    pub fn change_count() -> u64 {
        // SAFETY: Reads a counter the bridge's notification observer bumps.
        unsafe { hex_calendar_change_count() }
    }

    pub fn calendars() -> Result<Vec<CalendarInfo>> {
        // SAFETY: Returns an owned, NUL-terminated JSON string or null.
        decode(unsafe { hex_calendar_calendars() })
    }

    pub fn events(from_ms: i64, to_ms: i64) -> Result<Vec<CalendarEvent>> {
        // SAFETY: Returns an owned, NUL-terminated JSON string or null.
        decode(unsafe { hex_calendar_events(from_ms as f64, to_ms as f64, super::MAX_EVENTS) })
    }

    /// The included meetings from a little before now through the lookahead:
    /// one bounded EventKit read. Call it off the main and audio threads.
    pub fn read_meetings(settings: &CalendarSettings, now_ms: i64) -> Result<Vec<CalendarMeeting>> {
        let events = events(now_ms - LATE_GRACE_MS, now_ms + LOOKAHEAD_MS)?;
        Ok(super::meetings(&events, |calendar| {
            settings.includes(calendar)
        }))
    }

    /// Access, the calendars, and the next meetings, for Settings. Reads
    /// events only once the user connected a calendar.
    pub fn snapshot(settings: &CalendarSettings, now_ms: i64) -> CalendarSnapshot {
        let access = access();
        let mut snapshot = CalendarSnapshot {
            access,
            ..CalendarSnapshot::default()
        };
        if access != CalendarAccess::FullAccess || !settings.connected {
            return snapshot;
        }
        let read = calendars().and_then(|calendars| {
            let meetings = read_meetings(settings, now_ms)?;
            Ok((calendars, super::upcoming(&meetings, now_ms)))
        });
        match read {
            Ok((calendars, upcoming)) => {
                snapshot.calendars = calendars;
                snapshot.upcoming = upcoming;
            }
            Err(error) => snapshot.error = Some(format!("{error:#}")),
        }
        snapshot
    }

    fn decode<T: serde::de::DeserializeOwned>(string: *mut c_char) -> Result<T> {
        if string.is_null() {
            return Err(eyre!("calendar access is not granted"));
        }
        // SAFETY: The bridge returned a valid C string it allocated.
        let json = unsafe { CStr::from_ptr(string) }.to_bytes().to_vec();
        // SAFETY: Freed once, by the allocator that made it.
        unsafe { hex_calendar_free_string(string) };
        Ok(serde_json::from_slice(&json)?)
    }

    pub fn open_privacy_settings() {
        if let Err(error) = std::process::Command::new("/usr/bin/open")
            .arg("x-apple.systempreferences:com.apple.preference.security?Privacy_Calendars")
            .spawn()
        {
            tracing::warn!(%error, "could not open Calendars privacy settings");
        }
    }

    /// Opens a meeting's call link in its app or the browser.
    pub fn open_link(url: &str) {
        if let Err(error) = std::process::Command::new("/usr/bin/open").arg(url).spawn() {
            tracing::warn!(%error, "could not open the meeting link");
        }
    }
}

/// Linux and Windows read no calendar: reminders stay off there (Settings
/// has no Connect calendar), and only call links open.
#[cfg(not(target_os = "macos"))]
mod native {
    use color_eyre::eyre::Result;

    use super::CalendarMeeting;
    use crate::app_settings::CalendarSettings;

    pub fn change_count() -> u64 {
        0
    }

    pub fn read_meetings(
        _settings: &CalendarSettings,
        _now_ms: i64,
    ) -> Result<Vec<CalendarMeeting>> {
        Ok(Vec::new())
    }

    /// Opens a meeting's call link in its app or the browser.
    pub fn open_link(url: &str) {
        #[cfg(target_os = "linux")]
        let opened = std::process::Command::new("xdg-open").arg(url).spawn();
        #[cfg(target_os = "windows")]
        let opened = std::process::Command::new("explorer.exe").arg(url).spawn();
        if let Err(error) = opened {
            tracing::warn!(%error, "could not open the meeting link");
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const MINUTE: i64 = 60_000;

    fn answer(granted: bool, status: CalendarAccess, error: Option<&str>) -> AccessAnswer {
        AccessAnswer {
            granted,
            status,
            error: error.map(Into::into),
        }
    }

    #[test]
    fn a_grant_is_full_access_even_while_the_status_is_stale() {
        use CalendarAccess::*;
        assert_eq!(answer(true, NotDetermined, None).access(), FullAccess);
        assert_eq!(answer(true, FullAccess, None).access(), FullAccess);
    }

    #[test]
    fn a_refusal_is_a_denial_unless_the_request_failed() {
        use CalendarAccess::*;
        assert_eq!(answer(false, NotDetermined, None).access(), Denied);
        assert_eq!(answer(false, Denied, None).access(), Denied);
        assert_eq!(answer(false, Restricted, None).access(), Restricted);
        assert_eq!(answer(false, WriteOnly, None).access(), WriteOnly);
        assert_eq!(
            answer(false, NotDetermined, Some("no usage description")).access(),
            NotDetermined
        );
        assert!(answer(false, NotDetermined, None).access().blocked());
    }

    fn event(title: &str) -> CalendarEvent {
        CalendarEvent {
            id: format!("{title}-id"),
            calendar: "work".into(),
            title: title.into(),
            start_ms: (100 * MINUTE) as f64,
            end_ms: (130 * MINUTE) as f64,
            attendees: vec!["Ana Ruiz".into()],
            ..CalendarEvent::default()
        }
    }

    fn meeting(id: &str, start_minute: i64) -> CalendarMeeting {
        CalendarMeeting {
            id: id.into(),
            title: id.into(),
            start_ms: start_minute * MINUTE,
            end_ms: (start_minute + 30) * MINUTE,
            attendees: vec!["Ana Ruiz".into()],
            link: None,
        }
    }

    #[test]
    fn meetings_need_other_people_or_a_call_link() {
        assert!(is_meeting(&event("Sync")));
        let mut solo = event("Focus time");
        solo.attendees.clear();
        assert!(!is_meeting(&solo));
        solo.notes = Some("Join: https://meet.google.com/abc-defg-hij".into());
        assert!(is_meeting(&solo));
        let mut blank = event("Blank");
        blank.attendees = vec!["  ".into()];
        assert!(!is_meeting(&blank));
    }

    #[test]
    fn all_day_declined_free_and_cancelled_events_are_skipped() {
        for skip in [
            |event: &mut CalendarEvent| event.all_day = true,
            |event: &mut CalendarEvent| event.declined = true,
            |event: &mut CalendarEvent| event.free = true,
            |event: &mut CalendarEvent| event.cancelled = true,
            |event: &mut CalendarEvent| event.end_ms = event.start_ms,
        ] {
            let mut skipped = event("Sync");
            skip(&mut skipped);
            assert!(!is_meeting(&skipped), "{skipped:?}");
        }
    }

    #[test]
    fn excluded_calendars_are_left_out_and_meetings_sort_by_start() {
        let mut later = event("Later");
        later.start_ms += (60 * MINUTE) as f64;
        later.end_ms += (60 * MINUTE) as f64;
        let mut personal = event("Dentist");
        personal.calendar = "personal".into();
        let found = meetings(&[later, event("Sooner"), personal], |calendar| {
            calendar != "personal"
        });
        let titles = found
            .iter()
            .map(|meeting| meeting.title.as_str())
            .collect::<Vec<_>>();
        assert_eq!(titles, ["Sooner", "Later"]);
    }

    fn link_in(field: &str, text: &str) -> Option<CallLink> {
        let mut event = event("Sync");
        match field {
            "url" => event.url = Some(text.into()),
            "location" => event.location = Some(text.into()),
            _ => event.notes = Some(text.into()),
        }
        call_link(&event)
    }

    #[test]
    fn call_links_are_found_in_the_url_location_or_notes() {
        let cases = [
            (
                "url",
                "https://us02web.zoom.us/j/81234567890?pwd=abc",
                CallProvider::Zoom,
            ),
            (
                "location",
                "Zoom: https://zoom.us/my/kit.",
                CallProvider::Zoom,
            ),
            (
                "notes",
                "Join with Google Meet: https://meet.google.com/abc-defg-hij\nOr dial +1 555",
                CallProvider::GoogleMeet,
            ),
            (
                "location",
                "https://meet.google.com/abc-defg-hij",
                CallProvider::GoogleMeet,
            ),
            (
                "notes",
                "Microsoft Teams meeting\nJoin: <https://teams.microsoft.com/l/meetup-join/19%3ameeting_abc%40thread.v2/0?context=%7b%7d>",
                CallProvider::Teams,
            ),
            (
                "notes",
                "https://teams.live.com/meet/9312345",
                CallProvider::Teams,
            ),
            (
                "url",
                "https://app.slack.com/huddle/T012AB3C4/C012AB3C4",
                CallProvider::Slack,
            ),
            (
                "notes",
                "(https://acme.webex.com/meet/kit)",
                CallProvider::Webex,
            ),
            (
                "location",
                "https://acme.webex.com/acme/j.php?MTID=m123",
                CallProvider::Webex,
            ),
        ];
        for (field, text, provider) in cases {
            let link = link_in(field, text).unwrap_or_else(|| panic!("no link in {text}"));
            assert_eq!(link.provider, provider, "{text}");
            assert!(
                !link.url.ends_with('.') && !link.url.ends_with('>'),
                "{}",
                link.url
            );
        }
        let teams = link_in("notes", "Join: <https://teams.microsoft.com/l/meetup-join/19%3ameeting_abc%40thread.v2/0?context=%7b%7d>").unwrap();
        assert!(
            teams
                .url
                .starts_with("https://teams.microsoft.com/l/meetup-join/19%3ameeting_abc")
        );
    }

    #[test]
    fn other_links_and_lookalikes_are_not_calls() {
        for text in [
            "https://meet.google.com/landing",
            "https://zoom.us/pricing",
            "https://notzoom.us/j/123",
            "https://zoom.us.evil.example/j/123",
            "https://docs.google.com/document/d/abc",
            "Room 4B",
            "zoom.us/j/123 without a scheme",
            "httpx://zoom.us/j/1",
        ] {
            assert_eq!(link_in("notes", text), None, "{text}");
        }
    }

    #[test]
    fn a_link_after_a_bare_http_in_the_same_word_is_found() {
        let link = link_in("notes", "http_link:https://zoom.us/j/123").unwrap();
        assert_eq!(link.provider, CallProvider::Zoom);
        assert_eq!(link.url, "https://zoom.us/j/123");
    }

    #[test]
    fn the_url_field_wins_over_the_notes() {
        let mut event = event("Sync");
        event.notes = Some("https://zoom.us/j/1".into());
        event.url = Some("https://meet.google.com/abc-defg-hij".into());
        assert_eq!(
            call_link(&event).unwrap().provider,
            CallProvider::GoogleMeet
        );
    }

    #[test]
    fn a_reminder_shows_once_at_the_lead_time() {
        let mut scheduler = ReminderScheduler::default();
        let meetings = [meeting("sync", 100)];
        assert_eq!(scheduler.poll(&meetings, 98 * MINUTE, MINUTE), None);
        assert_eq!(
            scheduler.poll(&meetings, 99 * MINUTE, MINUTE),
            Some(ReminderUpdate::Show(meetings[0].clone()))
        );
        assert_eq!(scheduler.poll(&meetings, 99 * MINUTE + 500, MINUTE), None);
        scheduler.answered(&meetings[0].occurrence());
        assert_eq!(scheduler.poll(&meetings, 100 * MINUTE, MINUTE), None);
    }

    #[test]
    fn at_start_reminds_when_the_meeting_begins() {
        let mut scheduler = ReminderScheduler::default();
        let meetings = [meeting("sync", 100)];
        assert_eq!(scheduler.poll(&meetings, 100 * MINUTE - 1, 0), None);
        assert!(matches!(
            scheduler.poll(&meetings, 100 * MINUTE, 0),
            Some(ReminderUpdate::Show(_))
        ));
    }

    #[test]
    fn waking_shortly_after_the_start_still_reminds_but_not_much_later() {
        let meetings = [meeting("sync", 100)];
        let mut woke_soon = ReminderScheduler::default();
        assert_eq!(woke_soon.poll(&meetings, 90 * MINUTE, MINUTE), None);
        // Asleep through the lead time and the start.
        assert!(matches!(
            woke_soon.poll(&meetings, 103 * MINUTE, MINUTE),
            Some(ReminderUpdate::Show(_))
        ));
        let mut woke_late = ReminderScheduler::default();
        assert_eq!(woke_late.poll(&meetings, 90 * MINUTE, MINUTE), None);
        assert_eq!(woke_late.poll(&meetings, 106 * MINUTE, MINUTE), None);
    }

    #[test]
    fn a_showing_reminder_closes_when_its_meeting_is_cancelled_or_long_started() {
        let mut scheduler = ReminderScheduler::default();
        let meetings = vec![meeting("sync", 100)];
        assert!(matches!(
            scheduler.poll(&meetings, 99 * MINUTE, MINUTE),
            Some(ReminderUpdate::Show(_))
        ));
        // Cancelled: the filter drops it from the list.
        assert_eq!(
            scheduler.poll(&[], 99 * MINUTE + 1_000, MINUTE),
            Some(ReminderUpdate::Withdraw("sync@6000000".into()))
        );
        assert_eq!(scheduler.poll(&meetings, 99 * MINUTE + 2_000, MINUTE), None);

        // Unanswered, it closes a minute after the start, with the capsule.
        let mut expiring = ReminderScheduler::default();
        expiring.poll(&meetings, 99 * MINUTE, MINUTE);
        assert_eq!(expiring.poll(&meetings, 101 * MINUTE - 1, MINUTE), None);
        assert_eq!(
            expiring.poll(&meetings, 101 * MINUTE, MINUTE),
            Some(ReminderUpdate::Withdraw("sync@6000000".into()))
        );
    }

    #[test]
    fn an_overlapping_meeting_waits_for_the_showing_reminder() {
        let mut scheduler = ReminderScheduler::default();
        let meetings = vec![meeting("first", 100), meeting("second", 100)];
        assert!(matches!(
            scheduler.poll(&meetings, 99 * MINUTE, MINUTE),
            Some(ReminderUpdate::Show(shown)) if shown.id == "first"
        ));
        // The second is due too, but never replaces the first on screen.
        for second in 1..=119 {
            assert_eq!(
                scheduler.poll(&meetings, 99 * MINUTE + second * 1_000, MINUTE),
                None
            );
        }
        assert_eq!(
            scheduler.poll(&meetings, 101 * MINUTE, MINUTE),
            Some(ReminderUpdate::Withdraw("first@6000000".into()))
        );
        assert!(matches!(
            scheduler.poll(&meetings, 101 * MINUTE + 1_000, MINUTE),
            Some(ReminderUpdate::Show(shown)) if shown.id == "second"
        ));

        // Answering the first shows the second right away.
        let mut answering = ReminderScheduler::default();
        answering.poll(&meetings, 99 * MINUTE, MINUTE);
        answering.answered("first@6000000");
        assert!(matches!(
            answering.poll(&meetings, 99 * MINUTE + 1_000, MINUTE),
            Some(ReminderUpdate::Show(shown)) if shown.id == "second"
        ));
    }

    #[test]
    fn a_late_reminder_stays_up_at_least_half_a_minute() {
        assert_eq!(reminder_until_ms(100 * MINUTE, 99 * MINUTE), 101 * MINUTE);
        assert_eq!(
            reminder_until_ms(100 * MINUTE, 104 * MINUTE),
            104 * MINUTE + 30_000
        );
    }

    #[test]
    fn a_moved_meeting_is_reminded_at_its_new_time() {
        let mut scheduler = ReminderScheduler::default();
        let original = vec![meeting("sync", 100)];
        assert!(matches!(
            scheduler.poll(&original, 99 * MINUTE, MINUTE),
            Some(ReminderUpdate::Show(_))
        ));
        let moved = vec![meeting("sync", 160)];
        assert_eq!(
            scheduler.poll(&moved, 99 * MINUTE + 500, MINUTE),
            Some(ReminderUpdate::Withdraw(original[0].occurrence()))
        );
        assert_eq!(scheduler.poll(&moved, 120 * MINUTE, MINUTE), None);
        assert_eq!(
            scheduler.poll(&moved, 159 * MINUTE, MINUTE),
            Some(ReminderUpdate::Show(moved[0].clone()))
        );
    }

    #[test]
    fn back_to_back_meetings_each_remind() {
        let mut scheduler = ReminderScheduler::default();
        let meetings = vec![meeting("first", 100), meeting("second", 130)];
        assert!(matches!(
            scheduler.poll(&meetings, 99 * MINUTE, MINUTE),
            Some(ReminderUpdate::Show(shown)) if shown.id == "first"
        ));
        scheduler.answered("first@6000000");
        assert!(matches!(
            scheduler.poll(&meetings, 129 * MINUTE, MINUTE),
            Some(ReminderUpdate::Show(shown)) if shown.id == "second"
        ));
    }

    #[test]
    fn reminded_occurrences_are_forgotten_after_they_end() {
        let mut scheduler = ReminderScheduler::default();
        let meetings = vec![meeting("sync", 100)];
        scheduler.poll(&meetings, 99 * MINUTE, MINUTE);
        scheduler.answered(&meetings[0].occurrence());
        scheduler.poll(&[], 400 * MINUTE, MINUTE);
        assert!(scheduler.reminded.is_empty());
    }

    #[test]
    fn turning_reminders_off_withdraws_the_showing_one() {
        let mut scheduler = ReminderScheduler::default();
        let meetings = vec![meeting("sync", 100)];
        scheduler.poll(&meetings, 99 * MINUTE, MINUTE);
        assert_eq!(scheduler.clear(), Some(meetings[0].occurrence()));
        assert_eq!(scheduler.clear(), None);
        // Already reminded: turning them back on doesn't show it again.
        assert_eq!(scheduler.poll(&meetings, 99 * MINUTE + 500, MINUTE), None);
    }

    #[test]
    fn minutes_until_rounds_up_and_stops_at_the_start() {
        assert_eq!(minutes_until(100 * MINUTE, 99 * MINUTE), 1);
        assert_eq!(minutes_until(100 * MINUTE, 99 * MINUTE + 1), 1);
        assert_eq!(minutes_until(100 * MINUTE, 95 * MINUTE - 1), 6);
        assert_eq!(minutes_until(100 * MINUTE, 100 * MINUTE), 0);
        assert_eq!(minutes_until(100 * MINUTE, 103 * MINUTE), 0);
    }

    #[test]
    fn a_detected_call_belongs_to_the_current_meeting_on_its_service() {
        let mut zoom = meeting("zoom", 100);
        zoom.link = Some(CallLink {
            provider: CallProvider::Zoom,
            url: "https://zoom.us/j/1".into(),
        });
        let mut meet = meeting("meet", 100);
        meet.link = Some(CallLink {
            provider: CallProvider::GoogleMeet,
            url: "https://meet.google.com/abc-defg-hij".into(),
        });
        let meetings = vec![meeting("no-link", 100), zoom, meet];
        let found = |keys: &[&str], now| {
            current_for_call(&meetings, keys, now, MINUTE).map(|meeting| meeting.id.clone())
        };
        assert_eq!(
            found(&["us.zoom.xos"], 101 * MINUTE).as_deref(),
            Some("zoom")
        );
        assert_eq!(
            found(&["com.google.Chrome", "meet.google.com"], 99 * MINUTE).as_deref(),
            Some("meet")
        );
        assert_eq!(
            found(&["us02web.zoom.us"], 101 * MINUTE).as_deref(),
            Some("zoom")
        );
        assert_eq!(found(&["com.apple.FaceTime"], 101 * MINUTE), None);
        assert_eq!(found(&["us.zoom.xos"], 131 * MINUTE), None);
        assert_eq!(found(&["us.zoom.xos"], 90 * MINUTE), None);
    }
}
