//! The pump's timer and wakes: every frame (16 ms) while the HUD animates,
//! otherwise every 250 ms, and whatever sends the pump work wakes it at once
//! ([`wake_pump`]), so an idle Hex barely wakes its main thread (#1378).

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{self, Receiver};
use std::thread;

use block2::RcBlock;
use objc2::rc::Retained;
use objc2_foundation::{MainThreadMarker, NSTimer};

use super::{PUMP, Request, WINDOW, refresh_keyboard_layout};

/// The main-loop timer while the HUD animates: it follows the pointer and
/// channels drain every frame.
const PUMP_INTERVAL: f64 = 0.016;
/// The main-loop timer otherwise; senders of pump work wake it at once
/// ([`wake_pump`]), so this only paces menu refreshes and housekeeping.
const PUMP_IDLE_INTERVAL: f64 = 0.25;

/// The pump runs every frame; senders needn't wake it.
static PUMP_FAST: AtomicBool = AtomicBool::new(false);
/// A wake is queued on the main thread and hasn't run yet.
static PUMP_WAKE_PENDING: AtomicBool = AtomicBool::new(false);

/// The pump's timer interval: every frame only while the HUD animates.
pub(super) fn pump_interval(fast: bool) -> f64 {
    if fast {
        PUMP_INTERVAL
    } else {
        PUMP_IDLE_INTERVAL
    }
}

/// Starts the pump's timer, every frame when `fast`, else idle.
pub(super) fn start_pump_timer(fast: bool) -> Retained<NSTimer> {
    PUMP_FAST.store(fast, Ordering::Release);
    schedule_pump(fast)
}

/// A repeating main-loop timer that runs the pump every frame when `fast`,
/// else at the idle interval.
fn schedule_pump(fast: bool) -> Retained<NSTimer> {
    let block = RcBlock::new(|_: std::ptr::NonNull<NSTimer>| pump_now());
    // SAFETY: the timer runs its block on this (main) thread's run loop.
    let timer = unsafe {
        NSTimer::scheduledTimerWithTimeInterval_repeats_block(pump_interval(fast), true, &block)
    };
    // Keep firing while a menu is open (event-tracking run loop mode).
    // SAFETY: adding a scheduled timer to the main run loop's common modes.
    unsafe {
        let run_loop = objc2_foundation::NSRunLoop::mainRunLoop();
        run_loop.addTimer_forMode(&timer, objc2_foundation::NSRunLoopCommonModes);
    }
    timer
}

/// Runs the pump on the main thread soon, from any thread: what sends it
/// work calls this, so the pump can idle between events. Wakes coalesce,
/// and while the pump already runs every frame this does nothing.
pub(crate) fn wake_pump() {
    if PUMP_FAST.load(Ordering::Acquire) || PUMP_WAKE_PENDING.swap(true, Ordering::AcqRel) {
        return;
    }
    let Some(window) = WINDOW.get() else {
        PUMP_WAKE_PENDING.store(false, Ordering::Release);
        return;
    };
    // Changes nothing the window shows, so it asks for no frame.
    window.update_if(|_, _| {
        PUMP_WAKE_PENDING.store(false, Ordering::Release);
        pump_now();
        false
    });
}

/// One pump turn on the main thread, then the timer follows the HUD:
/// every frame while it animates, slowly otherwise.
pub(super) fn pump_now() {
    PUMP.with(|slot| {
        // A turn already running (re-entered from inside it) drains anyway.
        let Ok(mut slot) = slot.try_borrow_mut() else {
            return;
        };
        let Some(state) = slot.as_mut() else {
            return;
        };
        if let Some(mtm) = MainThreadMarker::new() {
            let switching = state
                .pump
                .runtime
                .published()
                .is_some_and(|published| published.settings.keyboard_model_switching.enabled);
            if crate::keyboard_input_sources::take_layout_change(mtm) {
                refresh_keyboard_layout(mtm);
            }
            crate::keyboard_input_sources::observe(mtm, switching);
            if let Some(source) = crate::keyboard_input_sources::take_change(mtm) {
                let _ = state.pump.runtime.send(Request::InputSourceChanged(source));
            }
        }
        state.pump.tick(&mut state.indicator);
        let fast = state.indicator.animating();
        if fast != state.fast {
            PUMP_FAST.store(fast, Ordering::Release);
            state.timer.invalidate();
            state.timer = schedule_pump(fast);
            state.fast = fast;
            if !fast {
                // Work sent while the pump was still fast didn't wake it.
                wake_pump();
            }
        }
    });
}

/// Moves `receiver`'s items to a new channel of `bound` on a thread of its
/// own, waking the pump after each, so the pump can idle between them.
pub(super) fn relay<T: Send + 'static>(
    name: &str,
    receiver: Receiver<T>,
    bound: usize,
) -> std::io::Result<Receiver<T>> {
    let (sender, relayed) = mpsc::sync_channel(bound);
    thread::Builder::new().name(name.into()).spawn(move || {
        for item in receiver {
            if sender.send(item).is_err() {
                return;
            }
            wake_pump();
        }
    })?;
    Ok(relayed)
}
