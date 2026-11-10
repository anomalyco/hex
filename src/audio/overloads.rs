//! Microphone stream overloads: what CoreAudio reports on its real-time
//! thread, and the one coalesced line the audio owner writes about them.
//!
//! The error callback has already missed its cycle when it sees an overload,
//! so it only moves counters. `report` runs on the thread that drains the
//! stream, where formatting and a blocking write are harmless.

use std::sync::OnceLock;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{Duration, Instant};

/// Overloads reported since the last line, and how many of those were
/// CoreAudio refusing the stream's real-time promotion, which leaves it
/// running under more scheduling risk.
static GLITCHES: AtomicU64 = AtomicU64::new(0);
static REALTIME_DENIED: AtomicU64 = AtomicU64::new(0);
/// The last overload, and the last line written about them, in nanoseconds on
/// [`CLOCK`]. Zero means never, which a real reading cannot be: the clock
/// starts before any stream runs.
static GLITCHED_AT: AtomicU64 = AtomicU64::new(0);
static REPORTED_AT: AtomicU64 = AtomicU64::new(0);
/// The base those two are measured from, taken before a stream runs so
/// CoreAudio's real-time thread only ever reads an initialized cell.
static CLOCK: OnceLock<Instant> = OnceLock::new();

/// Starts the clock overload times are kept against, before a stream runs.
pub(super) fn start_clock() {
    CLOCK.get_or_init(Instant::now);
}

/// How long ago a microphone stream last reported an overload it continued
/// through.
pub(super) fn since_last() -> Option<Duration> {
    let glitched_at = GLITCHED_AT.load(Ordering::Relaxed);
    if glitched_at == 0 {
        return None;
    }
    Some(Duration::from_nanos(
        clock_nanos(Instant::now()).saturating_sub(glitched_at),
    ))
}

/// Records an overload from CoreAudio's real-time thread: no logging,
/// allocation, or locks, only clock and atomic reads, because the callback
/// that sees one has already missed its cycle.
pub(super) fn record(realtime_denied: bool) {
    if realtime_denied {
        REALTIME_DENIED.fetch_add(1, Ordering::Relaxed);
    }
    GLITCHES.fetch_add(1, Ordering::Relaxed);
    GLITCHED_AT.store(clock_nanos(Instant::now()), Ordering::Relaxed);
}

/// The shortest gap between two overload lines. CoreAudio reports overloads in
/// bursts of tens per second while a cold device settles, and a loaded machine
/// can keep one stream glitching for minutes, so one line per burst keeps a
/// day's log readable. A burst held back by this window is counted by the next
/// line, at most this much later.
const LOG_INTERVAL: Duration = Duration::from_secs(10);

/// Whether the overloads a poll found are reported now, given how long ago the
/// last line was written: the first burst reports at once, and a continuing
/// one at most every [`LOG_INTERVAL`].
fn report_due(since_last_report: Option<Duration>) -> bool {
    since_last_report.is_none_or(|since| since >= LOG_INTERVAL)
}

/// Reports the overloads recorded since the last line from a thread that may
/// block, as one line per burst. The pending count is taken only when a line
/// is due, so a burst this rate limit holds back is reported late rather than
/// lost.
pub(super) fn report() {
    if GLITCHES.load(Ordering::Relaxed) == 0 {
        return;
    }
    let now = clock_nanos(Instant::now());
    let reported_at = REPORTED_AT.load(Ordering::Relaxed);
    let since_last_report =
        (reported_at != 0).then(|| Duration::from_nanos(now.saturating_sub(reported_at)));
    if !report_due(since_last_report) {
        return;
    }
    // A meeting's stream and dictation's poll separately; one reports the burst.
    if REPORTED_AT
        .compare_exchange(reported_at, now, Ordering::AcqRel, Ordering::Relaxed)
        .is_err()
    {
        return;
    }
    let count = GLITCHES.swap(0, Ordering::AcqRel);
    let realtime_denied = REALTIME_DENIED.swap(0, Ordering::AcqRel);
    tracing::warn!(
        count,
        realtime_denied,
        "microphone stream glitched; continuing"
    );
}

/// `at` in nanoseconds on the clock overload times are kept against.
fn clock_nanos(at: Instant) -> u64 {
    let since = at.saturating_duration_since(*CLOCK.get_or_init(Instant::now));
    u64::try_from(since.as_nanos()).unwrap_or(u64::MAX)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// One line per burst: the first reports at once, and a continuing one
    /// waits out the window, so the count it holds is reported by the next
    /// poll rather than lost.
    #[test]
    fn a_burst_writes_one_line_and_holds_the_rest() {
        assert!(report_due(None));
        assert!(!report_due(Some(Duration::ZERO)));
        assert!(!report_due(Some(LOG_INTERVAL - Duration::from_millis(1))));
        assert!(report_due(Some(LOG_INTERVAL)));
        assert!(report_due(Some(LOG_INTERVAL * 3)));
    }
}
