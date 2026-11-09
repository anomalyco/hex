//! Screen Recording as this running Hex can use it.
//!
//! `CGPreflightScreenCaptureAccess` keeps its first answer for the life of
//! the process, so a grant made while Hex runs never shows through it: the
//! pane kept saying "Required", Allow opened System Settings on a switch
//! already on, and a capture failed with "Screen Recording is off". Once the
//! preflight says no, a fresh `scrawl-helper` (which macOS counts as Hex)
//! reads the grant, and a real one-frame capture tells whether this process
//! can use it or must restart first.

use std::sync::Mutex;
use std::time::{Duration, Instant};

use scrawl_capture::permissions::{Permission, Status};

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum ScreenAccess {
    #[default]
    Off,
    /// Granted in System Settings, but this process can't record until
    /// Hex restarts.
    NeedsRestart,
    Allowed,
}

/// How long a read stands while nothing shows it: every window's
/// once-a-second refresh asks, and a denial that spawns a helper each time
/// costs a process for nothing.
const IDLE_FOR: Duration = Duration::from_secs(60);

/// How long a read stands while a permission surface shows it.
const WATCHED_FOR: Duration = Duration::from_secs(2);

struct Cached {
    access: ScreenAccess,
    at: Option<Instant>,
    /// Generation of the background read in flight, if any.
    reading: Option<u64>,
    /// Newest generation handed to a check.
    issued: u64,
    /// Newest generation whose result stands, or the latest invalidation.
    floor: u64,
}

static CACHED: Mutex<Cached> = Mutex::new(Cached {
    access: ScreenAccess::Off,
    at: None,
    reading: None,
    issued: 0,
    floor: 0,
});

/// Never blocks: the preflight, else the last read, starting another on its
/// own thread when that one is older than [`IDLE_FOR`]. An idle window keeps
/// asking once a second without paying a helper process every time.
pub fn screen_access() -> ScreenAccess {
    cached_read(IDLE_FOR)
}

/// [`screen_access`] for a surface that shows the permission right now, so
/// a grant made in System Settings shows within [`WATCHED_FOR`].
pub fn screen_access_watched() -> ScreenAccess {
    cached_read(WATCHED_FOR)
}

fn cached_read(fresh_for: Duration) -> ScreenAccess {
    if crate::meeting::screen_recording_permitted() {
        return ScreenAccess::Allowed;
    }
    let mut cached = CACHED.lock().unwrap_or_else(|e| e.into_inner());
    let stale = is_stale(cached.at, Instant::now(), fresh_for);
    if stale && cached.reading.is_none() {
        let generation = next_generation(&mut cached);
        cached.reading = Some(generation);
        let spawned = std::thread::Builder::new()
            .name("screen-access".into())
            .spawn(move || {
                let _reading = Reading(generation);
                let access = read();
                let mut cached = CACHED.lock().unwrap_or_else(|e| e.into_inner());
                accept(&mut cached, generation, access, Instant::now());
            });
        if spawned.is_err() {
            cached.reading = None;
        }
    }
    cached.access
}

/// Whether a read from `at` is older than `fresh_for`; one never read is.
fn is_stale(at: Option<Instant>, now: Instant, fresh_for: Duration) -> bool {
    at.is_none_or(|at| now.saturating_duration_since(at) >= fresh_for)
}

/// Takes the generation a check starting now writes under.
fn next_generation(cached: &mut Cached) -> u64 {
    cached.issued += 1;
    cached.issued
}

/// Records a read unless something newer already stands, so a slow check can
/// neither replace a newer one nor undo an invalidation.
fn accept(cached: &mut Cached, generation: u64, access: ScreenAccess, now: Instant) {
    if generation <= cached.floor {
        return;
    }
    cached.access = access;
    cached.at = Some(now);
    cached.floor = generation;
}

/// Frees the in-flight slot if this generation still owns it.
fn finish_reading(cached: &mut Cached, generation: u64) {
    if cached.reading == Some(generation) {
        cached.reading = None;
    }
}

/// Drops the standing result and fences out every check issued so far.
fn invalidate(cached: &mut Cached) {
    cached.at = None;
    cached.floor = cached.issued;
}

/// Clears this check's in-flight slot on drop, so a panic in [`read`] cannot
/// leave the cache believing a check is still running.
struct Reading(u64);

impl Drop for Reading {
    fn drop(&mut self) {
        finish_reading(
            &mut CACHED.lock().unwrap_or_else(|e| e.into_inner()),
            self.0,
        );
    }
}

/// Marks the cached Screen Recording state stale so the next [`screen_access`]
/// read spawns a fresh check; one already running lands nowhere.
pub fn invalidate_screen_access() {
    let mut cached = CACHED.lock().unwrap_or_else(|e| e.into_inner());
    invalidate(&mut cached);
}

/// Reads it now, blocking for up to the helper's 5 s. Off the UI thread.
pub fn screen_access_now() -> ScreenAccess {
    if crate::meeting::screen_recording_permitted() {
        return ScreenAccess::Allowed;
    }
    let generation = next_generation(&mut CACHED.lock().unwrap_or_else(|e| e.into_inner()));
    let access = read();
    let mut cached = CACHED.lock().unwrap_or_else(|e| e.into_inner());
    accept(&mut cached, generation, access, Instant::now());
    access
}

fn read() -> ScreenAccess {
    let granted = match scrawl_capture::helper::permissions() {
        Ok(statuses) => statuses.get(&Permission::ScreenRecording) == Some(&Status::Granted),
        Err(error) => {
            tracing::debug!(error = %format!("{error:#}"), "could not read Screen Recording afresh");
            false
        }
    };
    if !granted {
        ScreenAccess::Off
    } else if scrawl_capture::capture::screen_capture_usable(true) {
        ScreenAccess::Allowed
    } else {
        tracing::info!("Screen Recording is granted but needs Hex to restart");
        ScreenAccess::NeedsRestart
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn cached() -> Cached {
        Cached {
            access: ScreenAccess::Off,
            at: None,
            reading: None,
            issued: 0,
            floor: 0,
        }
    }

    #[test]
    fn a_late_older_read_does_not_replace_a_newer_one() {
        let mut cached = cached();
        let start = Instant::now();
        let older = next_generation(&mut cached);
        let newer = next_generation(&mut cached);
        accept(&mut cached, newer, ScreenAccess::NeedsRestart, start);
        accept(
            &mut cached,
            older,
            ScreenAccess::Off,
            start + Duration::from_secs(1),
        );
        assert_eq!(cached.access, ScreenAccess::NeedsRestart);
        assert_eq!(cached.at, Some(start));
    }

    #[test]
    fn invalidation_drops_a_read_that_started_before_it() {
        let mut cached = cached();
        let start = Instant::now();
        let previous = next_generation(&mut cached);
        accept(&mut cached, previous, ScreenAccess::Allowed, start);
        let in_flight = next_generation(&mut cached);
        invalidate(&mut cached);
        accept(
            &mut cached,
            in_flight,
            ScreenAccess::Off,
            start + Duration::from_secs(1),
        );
        assert_eq!(cached.access, ScreenAccess::Allowed);
        assert_eq!(cached.at, None);
    }

    #[test]
    fn an_older_worker_does_not_clear_a_newer_workers_slot() {
        let mut cached = cached();
        let older = next_generation(&mut cached);
        let newer = next_generation(&mut cached);
        cached.reading = Some(newer);
        finish_reading(&mut cached, older);
        assert_eq!(cached.reading, Some(newer));
        finish_reading(&mut cached, newer);
        assert_eq!(cached.reading, None);
    }

    #[test]
    fn an_idle_read_stands_longer_than_a_watched_one() {
        let at = Instant::now();
        assert!(is_stale(None, at, WATCHED_FOR));
        assert!(!is_stale(
            Some(at),
            at + Duration::from_secs(1),
            WATCHED_FOR
        ));
        assert!(is_stale(Some(at), at + WATCHED_FOR, WATCHED_FOR));
        assert!(!is_stale(
            Some(at),
            at + IDLE_FOR - Duration::from_secs(1),
            IDLE_FOR
        ));
        assert!(is_stale(Some(at), at + IDLE_FOR, IDLE_FOR));
    }
}
