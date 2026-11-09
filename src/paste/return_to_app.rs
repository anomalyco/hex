//! Labs › Return to app: a dictation pastes into the app its capture started
//! in. Another app can come to the front while the person is still talking (a
//! window pops up, a notification opens); this brings the starting app back
//! before the paste. When that can't be confirmed in time, nothing is pasted
//! and Paste last dictation keeps the text.

use std::thread;
use std::time::{Duration, Instant};

use color_eyre::eyre::{Report, Result, eyre};
use objc2::rc::autoreleasepool;
use objc2_app_kit::{NSApplicationActivationOptions, NSRunningApplication};

use crate::dictation::context::frontmost_pid;

/// How long Hex waits for the app to come to the front.
const RETURN_TIMEOUT: Duration = Duration::from_millis(500);
/// How often it asks which app is in front while it waits.
const RETURN_POLL: Duration = Duration::from_millis(10);
/// A short settle once it is in front, so its key window takes the paste.
const RETURN_SETTLE: Duration = Duration::from_millis(50);

/// What a dictation's paste does about the app its capture started in.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum Decision {
    /// Paste where focus is now: no app was recorded, the recorded one was
    /// Hex itself, or it is already in front.
    Paste,
    /// Bring this app to the front first.
    ReturnTo(i32),
}

/// The app in front when a capture starts, which its paste returns to.
/// `None` unless Labs › Return to app is on, and never Hex itself.
pub(crate) fn capture_start() -> Option<i32> {
    if !crate::labs::return_to_app() {
        return None;
    }
    frontmost_pid().filter(|pid| *pid > 0 && *pid != our_pid())
}

/// Which app the paste needs, from process ids alone. Hex's own, none, and
/// the app already in front need no switch.
pub(crate) fn decision(return_to: Option<i32>, frontmost: Option<i32>, our_pid: i32) -> Decision {
    match return_to {
        Some(pid) if pid > 0 && pid != our_pid && frontmost != Some(pid) => Decision::ReturnTo(pid),
        _ => Decision::Paste,
    }
}

/// Whether the app at `target` is the one in front now.
fn arrived(target: i32, frontmost: Option<i32>) -> bool {
    frontmost == Some(target)
}

/// Brings the recorded app back to the front, if it isn't there already.
/// `Err` means nothing may be pasted: the app is gone or focus couldn't be
/// confirmed in time.
pub(crate) fn bring_to_front(return_to: Option<i32>) -> Result<()> {
    if !crate::labs::return_to_app() {
        return Ok(());
    }
    let Decision::ReturnTo(pid) = decision(return_to, frontmost_pid(), our_pid()) else {
        return Ok(());
    };
    let application =
        autoreleasepool(|_| NSRunningApplication::runningApplicationWithProcessIdentifier(pid));
    let Some(application) = application else {
        tracing::warn!(pid, "the app a dictation started in is gone");
        return Err(could_not_return());
    };
    tracing::debug!(
        pid,
        "bringing the app the dictation started in back to the front"
    );
    // ActivateIgnoringOtherApps is still what raises another app over Hex's
    // own key windows from a background app.
    #[allow(deprecated)]
    let _activated = autoreleasepool(|_| {
        application.activateWithOptions(NSApplicationActivationOptions::ActivateIgnoringOtherApps)
    });
    let deadline = Instant::now() + RETURN_TIMEOUT;
    while !arrived(pid, frontmost_pid()) {
        if Instant::now() >= deadline {
            tracing::warn!(
                pid,
                "couldn't confirm the app a dictation started in came to the front"
            );
            return Err(could_not_return());
        }
        thread::sleep(RETURN_POLL);
    }
    thread::sleep(RETURN_SETTLE);
    Ok(())
}

/// What a person sees when the paste went nowhere.
fn could_not_return() -> Report {
    eyre!("couldn't switch back to the app you started in; use Paste last dictation")
}

fn our_pid() -> i32 {
    std::process::id() as i32
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_a_different_recorded_app_needs_a_switch() {
        assert_eq!(decision(None, Some(7), 1), Decision::Paste);
        assert_eq!(decision(Some(7), Some(7), 1), Decision::Paste);
        assert_eq!(decision(Some(1), Some(2), 1), Decision::Paste, "Hex itself");
        assert_eq!(decision(Some(0), Some(2), 1), Decision::Paste, "no pid");
        assert_eq!(decision(Some(7), Some(9), 1), Decision::ReturnTo(7));
        assert_eq!(decision(Some(7), None, 1), Decision::ReturnTo(7));
    }

    #[test]
    fn only_the_recorded_pid_is_the_app_that_arrived() {
        assert!(arrived(7, Some(7)));
        assert!(!arrived(7, Some(9)));
        assert!(!arrived(7, None));
    }
}
