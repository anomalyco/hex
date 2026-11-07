//! The physical keyboard state below macOS's HID keyboard filter.
//!
//! When a user remaps Caps Lock to Control, `IOHIDKeyboardFilter` rewrites
//! its usage (0x39) to Right Control (0xE4) before CoreGraphics sees it, so
//! an event tap cannot tell a remapped Caps Lock from a real Right Control
//! press. The raw HID values below the filter still report the physical
//! usage. This tracker opens an `IOHIDManager` matching keyboards, records
//! which tracked keys are held across every keyboard, and publishes the
//! union where the event tap and the shortcut recorder can read it without
//! blocking.
//!
//! The manager opens with `kIOHIDOptionsTypeNone` and never seizes a device,
//! so every key still reaches the foreground application. Hex never requests
//! Input Monitoring, so the tracker runs only when Hex already has it: a user
//! can turn Hex on under System Settings › Privacy & Security › Input
//! Monitoring and restart Hex. `IOHIDCheckAccess` gates the tracker before it
//! opens; without that permission it stays inactive without opening or
//! prompting, and matching keeps using CoreGraphics flags unchanged, including
//! Right Control starting from a remapped Caps Lock.

use std::collections::HashMap;
use std::ffi::c_void;
use std::ptr;
use std::sync::atomic::{AtomicBool, AtomicPtr, AtomicU16, AtomicU64, Ordering};
use std::sync::mpsc::{self, SyncSender};
use std::sync::{Arc, Mutex};
use std::thread::{self, JoinHandle};
use std::time::Duration;

use objc2_core_foundation::{CFDictionary, CFNumber, CFRetained, CFString};

use crate::app_settings::{CAPS_LOCK_USAGE, PhysicalModifierKeys, physical_key_bit};
use crate::audio::CaptureInstant;

const KEYBOARD_USAGE_PAGE: u32 = 0x07;
const GENERIC_DESKTOP_USAGE_PAGE: i32 = 0x01;
const KEYBOARD_USAGE: i32 = 0x06;
/// `kIOHIDOptionsTypeNone`: open the device without seizing it.
const HID_OPTIONS_NONE: u32 = 0;
/// `kIOHIDRequestTypeListenEvent` for `IOHIDCheckAccess`.
const IOHID_REQUEST_TYPE_LISTEN_EVENT: u32 = 1;
/// `kIOHIDAccessTypeGranted`.
const IOHID_ACCESS_TYPE_GRANTED: u32 = 0;
/// How long opening the manager may take before the input monitor gives up
/// rather than block dictation startup.
const START_TIMEOUT: Duration = Duration::from_secs(2);
/// The run loop wakes at least this often to observe the stop flag, closing
/// the race between checking it and entering the run loop.
const STOP_POLL_SECONDS: f64 = 0.5;
/// `kCFRunLoopRunFinished`: the run loop has no sources or timers.
const CFRUN_LOOP_RUN_FINISHED: i32 = 1;

type IohidValueCallback = unsafe extern "C" fn(
    context: *mut c_void,
    result: i32,
    sender: *mut c_void,
    value: *mut c_void,
);
type IohidDeviceCallback = unsafe extern "C" fn(
    context: *mut c_void,
    result: i32,
    sender: *mut c_void,
    device: *mut c_void,
);

#[link(name = "IOKit", kind = "framework")]
unsafe extern "C" {
    fn IOHIDManagerCreate(allocator: *const c_void, options: u32) -> *mut c_void;
    fn IOHIDManagerSetDeviceMatching(manager: *mut c_void, matching: *const c_void);
    fn IOHIDManagerRegisterInputValueCallback(
        manager: *mut c_void,
        callback: IohidValueCallback,
        context: *mut c_void,
    );
    fn IOHIDManagerRegisterDeviceRemovalCallback(
        manager: *mut c_void,
        callback: IohidDeviceCallback,
        context: *mut c_void,
    );
    fn IOHIDManagerScheduleWithRunLoop(
        manager: *mut c_void,
        run_loop: *mut c_void,
        mode: *const c_void,
    );
    fn IOHIDManagerUnscheduleFromRunLoop(
        manager: *mut c_void,
        run_loop: *mut c_void,
        mode: *const c_void,
    );
    fn IOHIDManagerOpen(manager: *mut c_void, options: u32) -> i32;
    fn IOHIDManagerClose(manager: *mut c_void, options: u32) -> i32;
    fn IOHIDCheckAccess(request_type: u32) -> u32;
    fn IOHIDValueGetElement(value: *mut c_void) -> *mut c_void;
    fn IOHIDElementGetDevice(element: *mut c_void) -> *mut c_void;
    fn IOHIDElementGetUsagePage(element: *mut c_void) -> u32;
    fn IOHIDElementGetUsage(element: *mut c_void) -> u32;
    fn IOHIDValueGetTimeStamp(value: *mut c_void) -> u64;
    fn IOHIDValueGetIntegerValue(value: *mut c_void) -> isize;
}

#[link(name = "CoreFoundation", kind = "framework")]
unsafe extern "C" {
    fn CFRunLoopGetCurrent() -> *mut c_void;
    fn CFRunLoopRunInMode(
        mode: *const c_void,
        seconds: f64,
        return_after_source_handled: u8,
    ) -> i32;
    fn CFRunLoopStop(run_loop: *mut c_void);
    fn CFRelease(value: *const c_void);
    static kCFRunLoopCommonModes: *const c_void;
    static kCFRunLoopDefaultMode: *const c_void;
}

/// The tracked usages every matched keyboard holds right now, unioned.
static HELD: AtomicU16 = AtomicU16::new(0);
/// The generation that owns [`HELD`], or zero while no tracker is active.
/// A tracker that timed out while starting must not clear a newer one.
static OWNER: AtomicU64 = AtomicU64::new(0);
static NEXT_OWNER: AtomicU64 = AtomicU64::new(1);

/// Labs › Caps Lock dictation's listener, called on the tracker thread for
/// each physical Caps Lock key-down with its press timestamp.
static CAPS_LOCK_PRESSED: Mutex<Option<fn(CaptureInstant)>> = Mutex::new(None);

/// Registers or clears the one Caps Lock press listener. The tracker stays
/// independent of the feature: with no listener it records held keys as
/// before.
pub(crate) fn watch_caps_lock(pressed: Option<fn(CaptureInstant)>) {
    *CAPS_LOCK_PRESSED
        .lock()
        .unwrap_or_else(|error| error.into_inner()) = pressed;
}

/// The physical keys held right now, or `None` while the tracker is
/// inactive (access not granted, the open failed, or it is stopping).
pub(crate) fn snapshot() -> Option<PhysicalModifierKeys> {
    (OWNER.load(Ordering::Acquire) != 0)
        .then(|| PhysicalModifierKeys::from_bits(HELD.load(Ordering::Acquire)))
}

/// Tracks one keyboard's held keys so two keyboards cannot clear each
/// other's state. The union is published to [`HELD`] after every change.
struct TrackerContext {
    owner: u64,
    devices: Mutex<HashMap<usize, u16>>,
}

impl TrackerContext {
    fn publish(&self, devices: &HashMap<usize, u16>) {
        if OWNER.load(Ordering::Acquire) != self.owner {
            return;
        }
        let union = devices.values().fold(0, |union, held| union | held);
        HELD.store(union, Ordering::Release);
    }
}

/// Leaves the tracker inactive, unless a newer generation already owns it.
fn deactivate(owner: u64) {
    if OWNER
        .compare_exchange(owner, 0, Ordering::AcqRel, Ordering::Acquire)
        .is_ok()
    {
        HELD.store(0, Ordering::Release);
    }
}

// SAFETY: `context` is the `TrackerContext` registered below; it outlives
// the manager, whose callbacks run only on the tracker thread's run loop.
// `value` is a live IOHID value for the duration of the callback.
unsafe extern "C" fn value_changed(
    context: *mut c_void,
    _result: i32,
    _sender: *mut c_void,
    value: *mut c_void,
) {
    if context.is_null() || value.is_null() {
        return;
    }
    // SAFETY: see the callback contract above.
    let context = unsafe { &*context.cast::<TrackerContext>() };
    // SAFETY: IOKit supplied a live value and its element.
    let element = unsafe { IOHIDValueGetElement(value) };
    if element.is_null() {
        return;
    }
    // SAFETY: `element` belongs to the live value.
    if unsafe { IOHIDElementGetUsagePage(element) } != KEYBOARD_USAGE_PAGE {
        return;
    }
    // SAFETY: as above; HID usage values fit in 16 bits.
    let Ok(usage) = u16::try_from(unsafe { IOHIDElementGetUsage(element) }) else {
        return;
    };
    let Some(bit) = physical_key_bit(usage) else {
        return;
    };
    // SAFETY: as above; the element's device is the keyboard it belongs to.
    let device = unsafe { IOHIDElementGetDevice(element) };
    if device.is_null() {
        return;
    }
    // SAFETY: as above; a key element's value is 1 while held, 0 on release.
    let down = unsafe { IOHIDValueGetIntegerValue(value) } != 0;
    let mut devices = context
        .devices
        .lock()
        .unwrap_or_else(|error| error.into_inner());
    let held = devices.entry(device as usize).or_default();
    let newly_pressed = down && usage == CAPS_LOCK_USAGE && *held & (1_u16 << bit) == 0;
    if down {
        *held |= 1 << bit;
    } else {
        *held &= !(1 << bit);
    }
    context.publish(&devices);
    drop(devices);
    if newly_pressed {
        // The device's own timestamp, which is earlier than this callback and
        // shared with the tap's CGEvent clock once scaled. No IOKit runs here.
        // SAFETY: `value` is live for this callback; its timestamp is a plain
        // read.
        let raw = unsafe { IOHIDValueGetTimeStamp(value) };
        let received_at = CaptureInstant::now();
        let (pressed_at, _) = super::physical_event_instant(raw, received_at);
        let listener = *CAPS_LOCK_PRESSED
            .lock()
            .unwrap_or_else(|error| error.into_inner());
        if let Some(listener) = listener {
            listener(pressed_at);
        }
    }
}

// SAFETY: `context` is the `TrackerContext` registered below and `device`
// is the keyboard being removed; both are live for this callback.
unsafe extern "C" fn device_removed(
    context: *mut c_void,
    _result: i32,
    _sender: *mut c_void,
    device: *mut c_void,
) {
    if context.is_null() || device.is_null() {
        return;
    }
    // SAFETY: see the callback contract above.
    let context = unsafe { &*context.cast::<TrackerContext>() };
    let mut devices = context
        .devices
        .lock()
        .unwrap_or_else(|error| error.into_inner());
    devices.remove(&(device as usize));
    context.publish(&devices);
}

fn matching_dictionary() -> CFRetained<CFDictionary<CFString, CFNumber>> {
    // `kIOHIDDeviceUsagePageKey` and `kIOHIDDeviceUsageKey` are string
    // macros in IOHIDKeys.h, not exported CFString symbols; CFDictionary
    // compares CFStrings by content, so these match IOKit's own lookups.
    let page_key = CFString::from_static_str("DeviceUsagePage");
    let usage_key = CFString::from_static_str("DeviceUsage");
    let page = CFNumber::new_i32(GENERIC_DESKTOP_USAGE_PAGE);
    let usage = CFNumber::new_i32(KEYBOARD_USAGE);
    CFDictionary::from_slices(&[&page_key, &usage_key], &[&page, &usage])
}

/// Owns the tracker thread and its run loop. Dropping it stops the run
/// loop, closes the manager, and leaves [`snapshot`] inactive.
pub(crate) struct PhysicalKeyMonitor {
    run_loop: Arc<AtomicPtr<c_void>>,
    stop: Arc<AtomicBool>,
    owner: u64,
    worker: Option<JoinHandle<()>>,
}

impl PhysicalKeyMonitor {
    /// Opens the raw HID keyboards and reads them on a dedicated run loop.
    /// A failure is logged once and leaves the tracker inactive rather than
    /// failing the input monitor. A start that takes longer than
    /// [`START_TIMEOUT`] is told to give up instead of blocking the listener.
    pub(crate) fn start() -> Option<Self> {
        let run_loop = Arc::new(AtomicPtr::new(ptr::null_mut()));
        let stop = Arc::new(AtomicBool::new(false));
        let thread_run_loop = run_loop.clone();
        let thread_stop = stop.clone();
        let (ready_sender, ready_receiver) = mpsc::sync_channel(1);
        let worker = thread::Builder::new()
            .name("hex-physical-keys".into())
            .spawn(move || run(thread_run_loop, thread_stop, ready_sender));
        let worker = match worker {
            Ok(worker) => worker,
            Err(error) => {
                tracing::warn!(%error, "could not start the raw HID keyboard tracker");
                return None;
            }
        };
        match ready_receiver.recv_timeout(START_TIMEOUT) {
            Ok(Some(owner)) => Some(Self {
                run_loop,
                stop,
                owner,
                worker: Some(worker),
            }),
            Ok(None) => {
                let _ = worker.join();
                None
            }
            Err(_) => {
                // The thread may still be opening the manager. Tell it to
                // give up and detach; it observes `stop` before it runs.
                tracing::warn!(
                    "raw HID keyboard tracker start timed out; shortcut sides fall back to Core Graphics flags"
                );
                stop.store(true, Ordering::Release);
                None
            }
        }
    }
}

impl Drop for PhysicalKeyMonitor {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Release);
        let run_loop = self.run_loop.load(Ordering::Acquire);
        if !run_loop.is_null() {
            // SAFETY: Core Foundation run loops may be stopped from any
            // thread; the tracker stored this one before reporting ready.
            unsafe { CFRunLoopStop(run_loop) };
        }
        if let Some(worker) = self.worker.take() {
            let _ = worker.join();
        }
        deactivate(self.owner);
    }
}

fn run(
    run_loop_slot: Arc<AtomicPtr<c_void>>,
    stop: Arc<AtomicBool>,
    ready: SyncSender<Option<u64>>,
) {
    // Hex never calls IOHIDRequestAccess: asking could prompt for Input
    // Monitoring, which Hex's onboarding deliberately never requests. The
    // tracker runs only when Hex already has Input Monitoring; without it, a
    // remapped Caps Lock keeps starting a Right Control shortcut.
    // SAFETY: a read-only access query.
    let access = unsafe { IOHIDCheckAccess(IOHID_REQUEST_TYPE_LISTEN_EVENT) };
    if access != IOHID_ACCESS_TYPE_GRANTED {
        tracing::warn!(
            access,
            "raw HID keyboard tracker skipped: Input Monitoring is not granted; shortcut sides fall back to Core Graphics flags, so a remapped Caps Lock still starts a Right Control shortcut"
        );
        let _ = ready.send(None);
        return;
    }
    let owner = NEXT_OWNER.fetch_add(1, Ordering::Relaxed);
    // SAFETY: IOKit returns an owned manager; every use below stays on this
    // thread, and it is released exactly once after the run loop returns.
    let manager = unsafe { IOHIDManagerCreate(ptr::null(), HID_OPTIONS_NONE) };
    if manager.is_null() {
        tracing::warn!("could not create the raw HID keyboard tracker");
        let _ = ready.send(None);
        return;
    }
    let matching = matching_dictionary();
    // SAFETY: `manager` is live and `matching` is a valid CFDictionary that
    // set-device-matching retains for the manager's lifetime.
    unsafe {
        IOHIDManagerSetDeviceMatching(
            manager,
            (&*matching as *const CFDictionary<CFString, CFNumber>).cast(),
        );
    }
    let mut context = Box::new(TrackerContext {
        owner,
        devices: Mutex::new(HashMap::new()),
    });
    // SAFETY: `context` lives until after the manager closes on this thread.
    let context_ptr = (&raw mut *context).cast::<c_void>();
    // SAFETY: this is the tracker thread's current run loop, used only here.
    let run_loop = unsafe { CFRunLoopGetCurrent() };
    // SAFETY: the callbacks run only on this run loop, `context_ptr` outlives
    // them, and every argument is a live Core Foundation object.
    let status = unsafe {
        IOHIDManagerRegisterInputValueCallback(manager, value_changed, context_ptr);
        IOHIDManagerRegisterDeviceRemovalCallback(manager, device_removed, context_ptr);
        IOHIDManagerScheduleWithRunLoop(manager, run_loop, kCFRunLoopCommonModes);
        IOHIDManagerOpen(manager, HID_OPTIONS_NONE)
    };
    if status != 0 || stop.load(Ordering::Acquire) {
        if status != 0 {
            tracing::warn!(
                status = format_args!("0x{:08x}", status as u32),
                "could not open the raw HID keyboard tracker; shortcut sides fall back to Core Graphics flags"
            );
        }
        // SAFETY: the manager was created here and is released exactly once;
        // close only if the open succeeded.
        unsafe {
            IOHIDManagerUnscheduleFromRunLoop(manager, run_loop, kCFRunLoopCommonModes);
            if status == 0 {
                IOHIDManagerClose(manager, HID_OPTIONS_NONE);
            }
            CFRelease(manager.cast_const());
        }
        let _ = ready.send(None);
        return;
    }
    HELD.store(0, Ordering::Release);
    OWNER.store(owner, Ordering::Release);
    run_loop_slot.store(run_loop, Ordering::Release);
    let _ = ready.send(Some(owner));
    while !stop.load(Ordering::Acquire) {
        // SAFETY: this dedicated thread exists solely to dispatch the HID
        // manager. The timeout observes `stop` even if it was set between
        // the check above and entering this call.
        let result = unsafe { CFRunLoopRunInMode(kCFRunLoopDefaultMode, STOP_POLL_SECONDS, 0) };
        if result == CFRUN_LOOP_RUN_FINISHED {
            // Nothing can arrive; stop rather than spin on a finished loop.
            break;
        }
    }
    deactivate(owner);
    run_loop_slot.store(ptr::null_mut(), Ordering::Release);
    // SAFETY: the run loop returned, so no callback is running; the manager
    // and matching dictionary are owned here and released once.
    unsafe {
        IOHIDManagerUnscheduleFromRunLoop(manager, run_loop, kCFRunLoopCommonModes);
        IOHIDManagerClose(manager, HID_OPTIONS_NONE);
        CFRelease(manager.cast_const());
    }
    drop(matching);
    drop(context);
}
