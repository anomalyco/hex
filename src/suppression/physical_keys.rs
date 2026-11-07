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
//! so every key still reaches the foreground application. Input Monitoring
//! permission is already required; without it the open or the callbacks
//! fail, the tracker logs once, stays inactive, and matching keeps using
//! CoreGraphics flags unchanged.

use std::collections::HashMap;
use std::ffi::c_void;
use std::ptr;
use std::sync::atomic::{AtomicBool, AtomicPtr, AtomicU16, Ordering};
use std::sync::mpsc::{self, SyncSender};
use std::sync::{Arc, Mutex};
use std::thread::{self, JoinHandle};

use objc2_core_foundation::{CFDictionary, CFNumber, CFRetained, CFString};

use crate::app_settings::{PhysicalModifierKeys, physical_key_bit};

const KEYBOARD_USAGE_PAGE: u32 = 0x07;
const GENERIC_DESKTOP_USAGE_PAGE: i32 = 0x01;
const KEYBOARD_USAGE: i32 = 0x06;
/// `kIOHIDOptionsTypeNone`: open the device without seizing it.
const HID_OPTIONS_NONE: u32 = 0;

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
    fn IOHIDValueGetElement(value: *mut c_void) -> *mut c_void;
    fn IOHIDElementGetUsagePage(element: *mut c_void) -> u32;
    fn IOHIDElementGetUsage(element: *mut c_void) -> u32;
    fn IOHIDValueGetIntegerValue(value: *mut c_void) -> isize;
}

#[link(name = "CoreFoundation", kind = "framework")]
unsafe extern "C" {
    fn CFRunLoopGetCurrent() -> *mut c_void;
    fn CFRunLoopRun();
    fn CFRunLoopStop(run_loop: *mut c_void);
    fn CFRelease(value: *const c_void);
    static kCFRunLoopCommonModes: *const c_void;
}

/// The tracked usages every matched keyboard holds right now, unioned.
static HELD: AtomicU16 = AtomicU16::new(0);
static TRACKING: AtomicBool = AtomicBool::new(false);

/// The physical keys held right now, or `None` while the tracker is
/// inactive (permission denied, the open failed, or it is stopping).
pub(crate) fn snapshot() -> Option<PhysicalModifierKeys> {
    TRACKING
        .load(Ordering::Acquire)
        .then(|| PhysicalModifierKeys::from_bits(HELD.load(Ordering::Acquire)))
}

/// Tracks one keyboard's held keys so two keyboards cannot clear each
/// other's state. The union is published to [`HELD`] after every change.
#[derive(Default)]
struct TrackerContext {
    devices: Mutex<HashMap<usize, u16>>,
}

impl TrackerContext {
    fn publish(&self, devices: &HashMap<usize, u16>) {
        let union = devices.values().fold(0, |union, held| union | held);
        HELD.store(union, Ordering::Release);
    }
}

// SAFETY: `context` is the `TrackerContext` registered below; it outlives
// the manager, whose callbacks run only on the tracker thread's run loop.
// `sender` is the device the value came from and `value` is a live IOHID
// value for the duration of the callback.
unsafe extern "C" fn value_changed(
    context: *mut c_void,
    _result: i32,
    sender: *mut c_void,
    value: *mut c_void,
) {
    if context.is_null() || sender.is_null() || value.is_null() {
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
    // SAFETY: as above; a key element's value is 1 while held, 0 on release.
    let down = unsafe { IOHIDValueGetIntegerValue(value) } != 0;
    let mut devices = context
        .devices
        .lock()
        .unwrap_or_else(|error| error.into_inner());
    let held = devices.entry(sender as usize).or_default();
    if down {
        *held |= 1 << bit;
    } else {
        *held &= !(1 << bit);
    }
    context.publish(&devices);
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
    worker: Option<JoinHandle<()>>,
}

impl PhysicalKeyMonitor {
    /// Opens the raw HID keyboards and reads them on a dedicated run loop.
    /// A failure is logged once and leaves the tracker inactive rather than
    /// failing the input monitor.
    pub(crate) fn start() -> Option<Self> {
        let run_loop = Arc::new(AtomicPtr::new(ptr::null_mut()));
        let thread_run_loop = run_loop.clone();
        let (ready_sender, ready_receiver) = mpsc::sync_channel(1);
        let worker = thread::Builder::new()
            .name("hex-physical-keys".into())
            .spawn(move || run(thread_run_loop, ready_sender));
        let worker = match worker {
            Ok(worker) => worker,
            Err(error) => {
                tracing::warn!(%error, "could not start the raw HID keyboard tracker");
                return None;
            }
        };
        if ready_receiver.recv().unwrap_or(false) {
            Some(Self {
                run_loop,
                worker: Some(worker),
            })
        } else {
            let _ = worker.join();
            None
        }
    }
}

impl Drop for PhysicalKeyMonitor {
    fn drop(&mut self) {
        let run_loop = self.run_loop.load(Ordering::Acquire);
        if !run_loop.is_null() {
            // SAFETY: Core Foundation run loops may be stopped from any
            // thread; the tracker stored this one before reporting ready.
            unsafe { CFRunLoopStop(run_loop) };
        }
        if let Some(worker) = self.worker.take() {
            let _ = worker.join();
        }
        TRACKING.store(false, Ordering::Release);
        HELD.store(0, Ordering::Release);
    }
}

fn run(run_loop_slot: Arc<AtomicPtr<c_void>>, ready: SyncSender<bool>) {
    // SAFETY: IOKit returns an owned manager; every use below stays on this
    // thread, and it is released exactly once after the run loop returns.
    let manager = unsafe { IOHIDManagerCreate(ptr::null(), HID_OPTIONS_NONE) };
    if manager.is_null() {
        tracing::warn!("could not create the raw HID keyboard tracker");
        let _ = ready.send(false);
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
    let mut context = Box::new(TrackerContext::default());
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
    if status != 0 {
        tracing::warn!(
            status = format_args!("0x{:08x}", status as u32),
            "could not open the raw HID keyboard tracker; shortcut sides fall back to Core Graphics flags"
        );
        // SAFETY: the manager was created here, is not open, and is released
        // exactly once.
        unsafe {
            IOHIDManagerUnscheduleFromRunLoop(manager, run_loop, kCFRunLoopCommonModes);
            CFRelease(manager.cast_const());
        }
        let _ = ready.send(false);
        return;
    }
    TRACKING.store(true, Ordering::Release);
    run_loop_slot.store(run_loop, Ordering::Release);
    let _ = ready.send(true);
    // SAFETY: this dedicated thread exists solely to dispatch the HID
    // manager until `PhysicalKeyMonitor` stops it.
    unsafe { CFRunLoopRun() };
    TRACKING.store(false, Ordering::Release);
    HELD.store(0, Ordering::Release);
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
