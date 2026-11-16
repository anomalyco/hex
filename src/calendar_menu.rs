//! Upcoming meetings in the menu bar (Labs, macOS): the rows the status
//! menu lists, built purely from the meetings the meeting controller already
//! reads, and the snapshot it publishes so the menu never touches EventKit.
//!
//! `Reminders` in `crate::meeting::watcher` reads the calendar on its own
//! thread and hands the filtered meeting list here; the status item only
//! formats that snapshot, on the main thread, at each refresh.

use std::sync::{LazyLock, RwLock};

use crate::calendar::CalendarMeeting;
use crate::calendar::date::{LocalDate, local_date};
use crate::desktop::i18n::{tr, tr_fill};

/// The most meetings the Upcoming section lists.
pub const MAX_ROWS: usize = 5;

/// The most meetings the snapshot keeps, so the menu holds a bounded list
/// however many the read returned.
const MAX_SNAPSHOT: usize = 32;

/// A snapshot older than this never shows: the controller rereads each
/// minute, so a stopped or disabled poll leaves nothing up.
const STALE_MS: i64 = 10 * 60 * 1_000;

/// One line of the Upcoming section.
#[derive(Clone, Debug, PartialEq)]
pub struct UpcomingRow {
    /// `10:30–11:00  Design sync`, `Now · 10:30–11:00  Design sync`, or
    /// `Tomorrow 10:30–11:00  Design sync`.
    pub label: String,
    /// The meeting choosing the row joins, when it has a call link; `None`
    /// leaves the row informational.
    pub meeting: Option<CalendarMeeting>,
}

/// The Upcoming section for `meetings` at `now_ms`: meetings that haven't
/// ended, soonest first, at most [`MAX_ROWS`], each labeled with `clock`
/// (the computer's local wall-clock time).
pub fn rows(
    meetings: &[CalendarMeeting],
    now_ms: i64,
    clock: impl Fn(i64) -> String,
) -> Vec<UpcomingRow> {
    rows_with(meetings, now_ms, clock, local_date)
}

/// [`rows`] with the local date supplied, so its day prefixes are testable
/// without this computer's time zone.
fn rows_with(
    meetings: &[CalendarMeeting],
    now_ms: i64,
    clock: impl Fn(i64) -> String,
    date_of: impl Fn(i64) -> Option<LocalDate>,
) -> Vec<UpcomingRow> {
    let mut upcoming = meetings
        .iter()
        .filter(|meeting| meeting.end_ms > now_ms)
        .collect::<Vec<_>>();
    upcoming.sort_by_key(|meeting| meeting.start_ms);
    upcoming.truncate(MAX_ROWS);
    let today = date_of(now_ms);
    upcoming
        .into_iter()
        .map(|meeting| {
            let range = format!(
                "{}–{}  {}",
                clock(meeting.start_ms),
                clock(meeting.end_ms),
                meeting.display_title()
            );
            let label = if meeting.start_ms <= now_ms {
                tr_fill("Now · {}", &range)
            } else {
                match day_prefix(meeting.start_ms, today, &date_of) {
                    Some(prefix) => format!("{prefix} {range}"),
                    None => range,
                }
            };
            UpcomingRow {
                label,
                meeting: meeting.link.is_some().then(|| meeting.clone()),
            }
        })
        .collect()
}

/// What starts a meeting that begins after today: `Tomorrow` for the next
/// local day, else its short weekday. `None` for today or earlier.
fn day_prefix(
    start_ms: i64,
    today: Option<LocalDate>,
    date_of: impl Fn(i64) -> Option<LocalDate>,
) -> Option<&'static str> {
    let today = today?;
    let start = date_of(start_ms)?;
    let (today, start_days) = (today.days(), start.days());
    if start_days <= today {
        return None;
    }
    if start_days == today + 1 {
        return Some(tr("Tomorrow"));
    }
    Some(weekday_name(start.weekday()))
}

fn weekday_name(weekday: usize) -> &'static str {
    match weekday {
        0 => tr("Sun"),
        1 => tr("Mon"),
        2 => tr("Tue"),
        3 => tr("Wed"),
        4 => tr("Thu"),
        5 => tr("Fri"),
        _ => tr("Sat"),
    }
}

/// The meetings the controller last read, and when.
#[derive(Clone, Default)]
struct Snapshot {
    meetings: Vec<CalendarMeeting>,
    read_at_ms: Option<i64>,
}

impl Snapshot {
    fn is_empty(&self) -> bool {
        self.meetings.is_empty() && self.read_at_ms.is_none()
    }
}

static SNAPSHOT: LazyLock<RwLock<Snapshot>> = LazyLock::new(|| RwLock::new(Snapshot::default()));

/// Whether the meeting controller should read the calendar for the menu: the
/// lab is on and a calendar is connected, whether or not reminders are. Off
/// everywhere else.
pub fn wanted(connected: bool) -> bool {
    reads_for_menu(crate::labs::calendar_menu(), connected)
}

/// The decision behind [`wanted`], pure so it is testable everywhere.
fn reads_for_menu(lab_on: bool, connected: bool) -> bool {
    lab_on && connected
}

/// Whether the lab itself is on, whatever the calendar's state: its snapshot
/// is cleared while it is on even when nothing may read.
pub fn lab_on() -> bool {
    crate::labs::calendar_menu()
}

/// Records the meetings the controller just read; `None` means the read
/// failed or the calendar went away, and nothing may show.
pub fn publish(read: Option<&[CalendarMeeting]>, read_at_ms: i64) {
    let Some(meetings) = read else {
        clear();
        return;
    };
    let next = Snapshot {
        meetings: meetings.iter().take(MAX_SNAPSHOT).cloned().collect(),
        read_at_ms: Some(read_at_ms),
    };
    let mut snapshot = SNAPSHOT.write().unwrap_or_else(|error| error.into_inner());
    if snapshot.meetings == next.meetings && snapshot.read_at_ms == next.read_at_ms {
        return;
    }
    *snapshot = next;
}

/// Forgets the last read. The write lock is only taken when something is
/// stored, because this runs on every controller loop while the lab is on
/// with Meetings off.
pub fn clear() {
    {
        let snapshot = SNAPSHOT.read().unwrap_or_else(|error| error.into_inner());
        if snapshot.is_empty() {
            return;
        }
    }
    *SNAPSHOT.write().unwrap_or_else(|error| error.into_inner()) = Snapshot::default();
}

/// The Upcoming section now: `None` hides it (the lab off, Meetings off, no
/// calendar connected, or no fresh read); `Some` holds the rows to show,
/// empty when nothing is upcoming.
pub fn section(now_ms: i64) -> Option<Vec<UpcomingRow>> {
    // The lab load comes first: with it off, the menu never reads settings.
    if !lab_on() {
        return None;
    }
    let connected = crate::app_settings::calendar_settings().connected;
    let visible = connected && crate::app_settings::meetings_enabled();
    let snapshot = SNAPSHOT.read().unwrap_or_else(|error| error.into_inner());
    section_from(visible, &snapshot, now_ms)
}

/// [`section`] for one snapshot and an already-decided visibility, so the
/// snapshot tests need no settings or AppKit.
fn section_from(visible: bool, snapshot: &Snapshot, now_ms: i64) -> Option<Vec<UpcomingRow>> {
    if !visible {
        return None;
    }
    // No successful read yet: the first one comes within a minute.
    let read_at_ms = snapshot.read_at_ms?;
    if !fresh(read_at_ms, now_ms) {
        return None;
    }
    Some(rows(&snapshot.meetings, now_ms, local_clock))
}

/// Whether a snapshot read at `read_at_ms` may show at `now_ms`.
fn fresh(read_at_ms: i64, now_ms: i64) -> bool {
    (now_ms - read_at_ms).abs() <= STALE_MS
}

/// Local wall-clock `10:30` for `ms`, from this computer's time zone. The
/// menu's rows are local times, so a meeting across a zone change reads as
/// it happens.
pub fn local_clock(ms: i64) -> String {
    let iso = crate::meeting::query::iso_local(u64::try_from(ms).unwrap_or_default());
    iso.get(11..16).map_or(iso.clone(), str::to_owned)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::calendar::{CallLink, CallProvider};

    const MINUTE: i64 = 60_000;

    /// The test clock: minutes past midnight, so labels are predictable
    /// without this computer's zone.
    fn clock(ms: i64) -> String {
        let minute = ms / MINUTE;
        format!("{:02}:{:02}", minute / 60 % 24, minute % 60)
    }

    /// The test calendar starts on Saturday 2026-11-14 (today's date).
    fn date(day: i64) -> LocalDate {
        LocalDate {
            year: 2026,
            month: 11,
            day: 14 + day,
        }
    }

    /// Milliseconds into whole test days.
    fn date_of(ms: i64) -> Option<LocalDate> {
        Some(date(ms.div_euclid(24 * 60 * MINUTE)))
    }

    fn meeting(title: &str, start_minute: i64, minutes: i64) -> CalendarMeeting {
        CalendarMeeting {
            id: title.into(),
            title: title.into(),
            start_ms: start_minute * MINUTE,
            end_ms: (start_minute + minutes) * MINUTE,
            attendees: vec!["Ana Ruiz".into()],
            link: None,
        }
    }

    fn with_link(mut meeting: CalendarMeeting) -> CalendarMeeting {
        meeting.link = Some(CallLink {
            provider: CallProvider::Zoom,
            url: "https://zoom.us/j/1".into(),
        });
        meeting
    }

    #[test]
    fn an_in_progress_meeting_reads_now() {
        let meetings = [with_link(meeting("Design sync", 630, 30))];
        // 10:30–11:00, at 10:40.
        let now = rows_with(&meetings, 640 * MINUTE, clock, date_of);
        assert_eq!(now.len(), 1);
        assert_eq!(now[0].label, "Now · 10:30–11:00  Design sync");
        assert_eq!(
            now[0].meeting.as_ref().map(|meeting| meeting.id.as_str()),
            Some("Design sync")
        );
        // Before it starts there is no Now.
        let before = rows_with(&meetings, 600 * MINUTE, clock, date_of);
        assert_eq!(before[0].label, "10:30–11:00  Design sync");
    }

    #[test]
    fn meetings_on_later_days_read_tomorrow_or_their_weekday() {
        let meetings = [
            meeting("Today", 700, 30),
            meeting("Tomorrow", 1_500, 30),
            meeting("Later", 2_900, 30),
        ];
        let listed = rows_with(&meetings, 600 * MINUTE, clock, date_of);
        let labels = listed
            .iter()
            .map(|row| row.label.as_str())
            .collect::<Vec<_>>();
        // 11:40 today, 01:00 tomorrow (Sunday), 00:20 on Monday the 16th.
        assert_eq!(
            labels,
            [
                "11:40–12:10  Today",
                "Tomorrow 01:00–01:30  Tomorrow",
                "Mon 00:20–00:50  Later",
            ]
        );
    }

    #[test]
    fn ended_meetings_are_left_out() {
        let meetings = [
            meeting("Ended", 600, 30),
            meeting("Starting", 640, 30),
            meeting("Later", 700, 30),
        ];
        // The first ended at 11:00; now is 11:00 sharp.
        let listed = rows_with(&meetings, 660 * MINUTE, clock, date_of);
        let labels = listed
            .iter()
            .map(|row| row.label.as_str())
            .collect::<Vec<_>>();
        assert_eq!(
            labels,
            ["Now · 10:40–11:10  Starting", "11:40–12:10  Later"]
        );
        // Everything ends before now: nothing is listed.
        assert!(rows_with(&meetings, 800 * MINUTE, clock, date_of).is_empty());
    }

    #[test]
    fn at_most_five_meetings_show_soonest_first() {
        let meetings = (0..7)
            .map(|index| meeting("Sync", 600 + index * 10, 5))
            .collect::<Vec<_>>();
        let listed = rows_with(&meetings, 590 * MINUTE, clock, date_of);
        assert_eq!(listed.len(), MAX_ROWS);
        assert_eq!(listed[0].label, "10:00–10:05  Sync");
        assert_eq!(listed[4].label, "10:40–10:45  Sync");
        // A meeting without a call link stays informational.
        assert!(listed.iter().all(|row| row.meeting.is_none()));
    }

    #[test]
    fn an_empty_section_has_no_rows() {
        assert!(rows_with(&[], 640 * MINUTE, clock, date_of).is_empty());
        // Only ended meetings read the same as none.
        assert!(rows_with(&[meeting("Ended", 600, 10)], 700 * MINUTE, clock, date_of).is_empty());
    }

    #[test]
    fn a_stale_snapshot_never_shows() {
        let now = 1_000 * MINUTE;
        assert!(fresh(now, now));
        assert!(fresh(now - STALE_MS, now));
        assert!(!fresh(now - STALE_MS - 1, now));
        // A clock change far ahead is stale too, rather than negative.
        assert!(!fresh(now + STALE_MS + 1, now));
    }

    #[test]
    fn publishing_keeps_at_most_32_and_none_clears() {
        let meetings = (0..40)
            .map(|index| meeting("Sync", 600 + index, 5))
            .collect::<Vec<_>>();
        publish(Some(&meetings), 1_000);
        let stored = SNAPSHOT
            .read()
            .unwrap_or_else(|error| error.into_inner())
            .clone();
        assert_eq!(stored.meetings.len(), MAX_SNAPSHOT);
        assert_eq!(stored.meetings.as_slice(), &meetings[..MAX_SNAPSHOT]);
        assert_eq!(stored.read_at_ms, Some(1_000));

        publish(None, 2_000);
        assert!(
            SNAPSHOT
                .read()
                .unwrap_or_else(|error| error.into_inner())
                .is_empty()
        );
    }

    #[test]
    fn the_section_hides_until_visible_and_fresh() {
        let snapshot = Snapshot {
            meetings: vec![with_link(meeting("Design sync", 630, 30))],
            read_at_ms: Some(640 * MINUTE),
        };
        assert!(section_from(false, &snapshot, 640 * MINUTE).is_none());
        assert!(
            section_from(true, &snapshot, 640 * MINUTE + STALE_MS + 1).is_none(),
            "a stale read hides"
        );
        let shown = section_from(true, &snapshot, 640 * MINUTE).expect("the section shows");
        assert_eq!(shown.len(), 1);
        assert!(shown[0].meeting.is_some());
        // No read yet reads as hidden.
        assert!(section_from(true, &Snapshot::default(), 640 * MINUTE).is_none());
    }

    #[test]
    fn the_menu_read_wants_a_connected_calendar() {
        assert!(reads_for_menu(true, true));
        assert!(!reads_for_menu(true, false));
        assert!(!reads_for_menu(false, true));
        assert!(!reads_for_menu(false, false));
    }

    #[test]
    fn a_local_date_parses_the_iso_date_part() {
        assert_eq!(
            LocalDate::parse("2026-11-14T10:30:00-05:00"),
            Some(LocalDate {
                year: 2026,
                month: 11,
                day: 14,
            })
        );
        assert_eq!(LocalDate::parse("not a date"), None);
        // Only the first ten characters are the date.
        assert_eq!(
            LocalDate::parse("2026-11-14-15"),
            Some(LocalDate {
                year: 2026,
                month: 11,
                day: 14,
            })
        );
        // November 14, 2026 is a Saturday, and the next day a Sunday.
        assert_eq!(date(0).weekday(), 6);
        assert_eq!(date(1).weekday(), 0);
        assert_eq!(date(2).weekday(), 1);
        // A real instant reads as some local date.
        assert!(local_date(1_760_000_000_000).is_some());
    }

    #[test]
    fn the_local_clock_reads_hours_and_minutes() {
        // Any instant reads as this computer's wall-clock `HH:MM`.
        let clock = local_clock(1_760_000_000_000);
        assert_eq!(clock.len(), 5);
        assert_eq!(clock.as_bytes()[2], b':');
        assert!(clock.split(':').all(|part| part.parse::<u8>().is_ok()));
        // A timestamp before the epoch reads as something, not a panic.
        assert_eq!(local_clock(-1).len(), 5);
    }
}
