//! Stop when the call ends, in a browser that never releases the microphone
//! between calls (#1775): whether a recording watches its page for another
//! call, the stop its page arms, and the notice that goes with it.

use std::sync::atomic::Ordering;
use std::sync::mpsc::SyncSender;

use crate::meeting::detection::{AutoStop, CallSwitch, MeetingDetector};

use super::{ActiveRecording, ControllerEvent, withdraw_stop};

/// The page names the recorded call again (or one the user kept): whatever
/// change was counting is over, so its stop never fires and its notice goes.
/// The call the page named before the stop is forgotten with it.
pub(super) fn withdraw_call_switch_stop(
    switch: &CallSwitch,
    stop: &mut AutoStop,
    other: &AutoStop,
    detector: &mut MeetingDetector,
    events: &SyncSender<ControllerEvent>,
) {
    if !switch.changed() && withdraw_stop(stop, other, events) {
        detector.forget_switched_call();
        tracing::info!("the page named the recorded call again; its pending stop is withdrawn");
    }
}

/// Drops an armed call-switch stop when nothing watches the page: a deadline
/// counted under a setting that is now off must not carry over.
pub(super) fn disarm_call_stop_unless_watching(
    recording: &mut Option<ActiveRecording>,
    stop_on_end: bool,
    detector: &mut MeetingDetector,
    events: &SyncSender<ControllerEvent>,
) {
    if let Some(active) = recording.as_mut()
        && !watching_call_switch(Some(active), stop_on_end)
        && withdraw_stop(&mut active.call_stop, &active.auto_stop, events)
    {
        // A recording already stopping (Stop, or the worker transcribing)
        // keeps the call its page named: the next recording starts it.
        if !active.stop.load(Ordering::Relaxed) {
            detector.forget_switched_call();
        }
        tracing::info!("the page is no longer watched; its pending stop is dropped");
    }
}

/// Whether a recording watches its browser's page for another call (Stop
/// when the call ends) instead of dropping the page while it records:
/// only one Hex started on its own, for a call whose page named its code, in
/// a browser, and that has not been told to stop yet. Every other recording
/// keeps the browser watch reset, offered and by-hand ones included: the lab
/// stops those on their call app leaving the microphone alone.
pub(super) fn watching_call_switch(recording: Option<&ActiveRecording>, stop_on_end: bool) -> bool {
    recording.is_some_and(|active| {
        !active.stop.load(Ordering::Relaxed)
            && active.call_switch.recorded().is_some()
            && active.application.as_deref().is_some_and(|application| {
                crate::microphone_activity::is_browser_id(Some(application))
            })
            && active.automatic
            && stop_on_end
    })
}
