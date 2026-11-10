//! Microphone stream overloads: a glitch the host reports while the stream
//! keeps running, and the one coalesced line the audio owner writes about
//! them.
//!
//! Some hosts report these from the thread that runs the stream, where a log
//! line or a lock blocks an already-late cycle, so `record` only moves
//! counters. `report` runs on the thread that drains the stream, and `flush` on
//! the one that drops it, where formatting and a blocking write are harmless.

use std::sync::OnceLock;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{Duration, Instant};

/// Overloads reported since the last line.
static GLITCHES: AtomicU64 = AtomicU64::new(0);
/// The last overload, and the last line written about them, in nanoseconds on
/// [`CLOCK`]. Zero means never, which a real reading cannot be: it is clamped
/// to at least one.
static GLITCHED_AT: AtomicU64 = AtomicU64::new(0);
static REPORTED_AT: AtomicU64 = AtomicU64::new(0);
/// The base those two are measured from, taken before a stream runs so the
/// host's thread only ever reads an initialized cell.
static CLOCK: OnceLock<Instant> = OnceLock::new();

/// Starts the clock overload times are kept against, before a stream runs.
pub(super) fn start_clock() {
    CLOCK.get_or_init(Instant::now);
}

/// How long ago a stream last reported an overload it continued through.
pub(super) fn since_last() -> Option<Duration> {
    let glitched_at = GLITCHED_AT.load(Ordering::Relaxed);
    if glitched_at == 0 {
        return None;
    }
    Some(Duration::from_nanos(
        now_nanos().saturating_sub(glitched_at),
    ))
}

/// Records an overload from the thread that runs the stream: no logging,
/// allocation, or locks, only clock and atomic reads, because a callback that
/// sees one has already missed its cycle.
pub(super) fn record() {
    GLITCHES.fetch_add(1, Ordering::Relaxed);
    GLITCHED_AT.store(now_nanos(), Ordering::Relaxed);
}

/// The shortest gap between two overload lines. Overloads arrive in bursts of
/// tens per second while a cold device settles, and a loaded machine can keep
/// one stream glitching for minutes, so one line per burst keeps the log
/// readable. A burst held back by this window is counted by the next line, at
/// most this much later.
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
    let now = now_nanos();
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
    write_pending();
}

/// Writes the count a closing stream leaves behind, without the rate limit: its
/// burst ended with the stream, so it does not belong beside the next open's
/// lines.
pub(super) fn flush() {
    if GLITCHES.load(Ordering::Relaxed) == 0 {
        return;
    }
    write_pending();
}

/// Writes the pending count, if there is one, exactly once.
fn write_pending() {
    let count = GLITCHES.swap(0, Ordering::AcqRel);
    if count == 0 {
        return;
    }
    tracing::warn!(count, "microphone stream glitched; continuing");
}

/// The clock reading. Two `Instant::now()` calls can share a tick, so the
/// result is clamped to at least one, which keeps zero meaning never.
fn now_nanos() -> u64 {
    let base = *CLOCK.get_or_init(Instant::now);
    let since = Instant::now().saturating_duration_since(base);
    u64::try_from(since.as_nanos()).unwrap_or(u64::MAX).max(1)
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
