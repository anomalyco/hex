//! Upcoming meetings in the menu bar (Labs, macOS): the rows the status
//! menu lists, built purely from the meetings the meeting controller already
//! reads, and the snapshot it publishes so the menu never touches EventKit.
//!
//! `Reminders` in `crate::meeting::watcher` reads the calendar on its own
//! thread and hands the filtered meeting list here; the status item only
//! formats that snapshot, on the main thread, at each refresh.

use std::sync::{LazyLock, RwLock};

use crate::calendar::CalendarMeeting;

/// The most meetings the Upcoming section lists.
pub const MAX_ROWS: usize = 5;

/// The most meetings the snapshot keeps, so the menu holds a bounded list
/// however many the read returned.
const MAX_SNAPSHOT: usize = 32;

/// A snapshot older than this never shows: the controller rereads each
/// minute, so a stopped or disabled poll leaves nothing up.
const STALE_MS: i64 = 10 * 60 * 1_000;

/// One line of the Upcoming section.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct UpcomingRow {
    /// `10:30–11:00  Design sync`, or `Now · 10:30–11:00  Design sync`.
    pub label: String,
    /// Whether choosing the row does anything: only a call link does.
    pub enabled: bool,
    /// The call link choosing it opens.
    pub link: Option<String>,
}

/// The Upcoming section for `meetings` at `now_ms`: meetings that haven't
/// ended, soonest first, at most [`MAX_ROWS`], each labeled with `clock`
/// (the computer's local wall-clock time).
pub fn rows(
    meetings: &[CalendarMeeting],
    now_ms: i64,
    clock: impl Fn(i64) -> String,
) -> Vec<UpcomingRow> {
    let mut upcoming = meetings
        .iter()
        .filter(|meeting| meeting.end_ms > now_ms)
        .collect::<Vec<_>>();
    upcoming.sort_by_key(|meeting| meeting.start_ms);
    upcoming.truncate(MAX_ROWS);
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
                format!("Now · {range}")
            } else {
                range
            };
            let link = meeting.link.as_ref().map(|link| link.url.clone());
            UpcomingRow {
                label,
                enabled: link.is_some(),
                link,
            }
        })
        .collect()
}

/// The meetings the controller last read, and when.
#[derive(Default)]
struct Snapshot {
    meetings: Vec<CalendarMeeting>,
    read_at_ms: Option<i64>,
}

static SNAPSHOT: LazyLock<RwLock<Snapshot>> = LazyLock::new(|| RwLock::new(Snapshot::default()));

/// Whether the meeting controller should read the calendar for the menu:
/// the lab is on, whether or not reminders are. Off everywhere else.
pub fn wanted() -> bool {
    crate::labs::calendar_menu()
}

/// Records the meetings the controller just read; `None` means the read
/// failed or the calendar went away, and nothing may show.
pub fn publish(read: Option<&[CalendarMeeting]>, read_at_ms: i64) {
    let next = match read {
        Some(meetings) => Snapshot {
            meetings: meetings.iter().take(MAX_SNAPSHOT).cloned().collect(),
            read_at_ms: Some(read_at_ms),
        },
        None => Snapshot::default(),
    };
    let mut snapshot = SNAPSHOT.write().unwrap_or_else(|error| error.into_inner());
    if snapshot.meetings == next.meetings && snapshot.read_at_ms == next.read_at_ms {
        return;
    }
    *snapshot = next;
}

/// The Upcoming section now: `None` hides it (the lab off, Meetings off, no
/// calendar connected, or no fresh read); `Some` holds the rows to show,
/// empty when nothing is upcoming.
pub fn section(now_ms: i64) -> Option<Vec<UpcomingRow>> {
    if !visible() {
        return None;
    }
    let snapshot = SNAPSHOT.read().unwrap_or_else(|error| error.into_inner());
    // No successful read yet: the first one comes within a minute.
    let read_at_ms = snapshot.read_at_ms?;
    if !fresh(read_at_ms, now_ms) {
        return None;
    }
    Some(rows(&snapshot.meetings, now_ms, local_clock))
}

/// Whether the menu may show the section at all: the lab is on, Meetings
/// are on, and a calendar is connected. Access itself is proven by a read:
/// without it the snapshot is empty or cleared.
fn visible() -> bool {
    wanted()
        && crate::app_settings::meetings_enabled()
        && crate::app_settings::calendar_settings().connected
}

/// Whether a snapshot read at `read_at_ms` may show at `now_ms`.
fn fresh(read_at_ms: i64, now_ms: i64) -> bool {
    (now_ms - read_at_ms).abs() <= STALE_MS
}

/// Local wall-clock `10:30` for `ms`, from this computer's time zone. The
/// menu's rows are local times, so a meeting across a zone change reads as
/// it happens.
pub fn local_clock(ms: i64) -> String {
    // `iso_local` is the one place Hex turns an instant into local wall-clock
    // time; the minutes are the `HH:MM` of its `YYYY-MM-DDTHH:MM:SS±HH:MM`.
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
        format!("{:02}:{:02}", ms / 3_600_000, ms % 3_600_000 / MINUTE)
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
        let now = rows(&meetings, 640 * MINUTE, clock);
        assert_eq!(now.len(), 1);
        assert_eq!(now[0].label, "Now · 10:30–11:00  Design sync");
        assert!(now[0].enabled);
        assert_eq!(now[0].link.as_deref(), Some("https://zoom.us/j/1"));
        // Before it starts there is no Now.
        let before = rows(&meetings, 600 * MINUTE, clock);
        assert_eq!(before[0].label, "10:30–11:00  Design sync");
    }

    #[test]
    fn ended_meetings_are_left_out() {
        let meetings = [
            meeting("Ended", 600, 30),
            meeting("Starting", 640, 30),
            meeting("Later", 700, 30),
        ];
        // The first ended at 11:00; now is 11:00 sharp.
        let listed = rows(&meetings, 660 * MINUTE, clock);
        let labels = listed
            .iter()
            .map(|row| row.label.as_str())
            .collect::<Vec<_>>();
        assert_eq!(
            labels,
            ["Now · 10:40–11:10  Starting", "11:40–12:10  Later"]
        );
        // Everything ends before now: nothing is listed.
        assert!(rows(&meetings, 800 * MINUTE, clock).is_empty());
    }

    #[test]
    fn at_most_five_meetings_show_soonest_first() {
        let meetings = (0..7)
            .map(|index| meeting("Sync", 600 + index * 10, 5))
            .collect::<Vec<_>>();
        let listed = rows(&meetings, 590 * MINUTE, clock);
        assert_eq!(listed.len(), MAX_ROWS);
        assert_eq!(listed[0].label, "10:00–10:05  Sync");
        assert_eq!(listed[4].label, "10:40–10:45  Sync");
        // A meeting without a call link stays informational.
        assert!(listed.iter().all(|row| !row.enabled && row.link.is_none()));
    }

    #[test]
    fn an_empty_section_has_no_rows() {
        assert!(rows(&[], 640 * MINUTE, clock).is_empty());
        // Only ended meetings read the same as none.
        assert!(rows(&[meeting("Ended", 600, 10)], 700 * MINUTE, clock).is_empty());
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
