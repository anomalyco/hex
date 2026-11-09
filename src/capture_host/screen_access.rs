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
    reading: bool,
}

static CACHED: Mutex<Cached> = Mutex::new(Cached {
    access: ScreenAccess::Off,
    at: None,
    reading: false,
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
    if stale && !cached.reading {
        cached.reading = true;
        let spawned = std::thread::Builder::new()
            .name("screen-access".into())
            .spawn(|| {
                let access = read();
                let mut cached = CACHED.lock().unwrap_or_else(|e| e.into_inner());
                *cached = Cached {
                    access,
                    at: Some(Instant::now()),
                    reading: false,
                };
            });
        if spawned.is_err() {
            cached.reading = false;
        }
    }
    cached.access
}

/// Whether a read from `at` is older than `fresh_for`; one never read is.
fn is_stale(at: Option<Instant>, now: Instant, fresh_for: Duration) -> bool {
    at.is_none_or(|at| now.saturating_duration_since(at) >= fresh_for)
}

/// Marks the cached Screen Recording state stale so the next [`screen_access`]
/// read spawns a fresh check.
pub fn invalidate_screen_access() {
    CACHED.lock().unwrap_or_else(|e| e.into_inner()).at = None;
}

/// Reads it now, blocking for up to the helper's 5 s. Off the UI thread.
pub fn screen_access_now() -> ScreenAccess {
    if crate::meeting::screen_recording_permitted() {
        return ScreenAccess::Allowed;
    }
    let access = read();
    *CACHED.lock().unwrap_or_else(|e| e.into_inner()) = Cached {
        access,
        at: Some(Instant::now()),
        reading: false,
    };
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
