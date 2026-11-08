//! The macOS side of Labs › Caps Lock dictation: the keyboard LED output
//! elements, the `IOHIDSystem` Caps Lock lock state, and the one worker
//! thread that owns both. Detection arrives from the event tap and the raw
//! HID tracker; no IOKit call runs on either callback thread or the audio
//! thread.
//!
//! The Caps Lock light is an output element on each keyboard (HID usage
//! page 0x08, usage 0x02). Writing it does not change the software Caps Lock
//! state, so Hex can use the light while typing stays lowercase. The lock
//! state is separately reset through `IOHIDSetModifierLockState`.
//!
//! The LED manager is not scheduled on a run loop. It is recreated at each
//! bounded refresh instead, so keyboards plugged in later are discovered
//! rather than trusting a device list that never updates.

use std::ffi::{c_char, c_void};
use std::ptr;
use std::sync::atomic::Ordering;
use std::sync::mpsc::{self, Receiver, Sender, SyncSender};
use std::sync::{Mutex, MutexGuard};
use std::thread::{self, JoinHandle};
use std::time::{Duration, Instant};

use objc2_core_foundation::{CFDictionary, CFNumber, CFRetained, CFString};

use super::{ENABLED, LED_PATTERN, LedModel, LedPattern};
use crate::audio::CaptureInstant;
use crate::dictation::indicator::DictationIndicatorEvent;

/// `kHIDPage_LEDs`.
const LED_USAGE_PAGE: u32 = 0x08;
/// `kHIDUsage_LED_CapsLock`.
const CAPS_LOCK_LED_USAGE: u32 = 0x02;
/// `kHIDPage_GenericDesktop`.
const GENERIC_DESKTOP_USAGE_PAGE: i32 = 0x01;
/// `kHIDUsage_GD_Keyboard`.
const KEYBOARD_USAGE: i32 = 0x06;
/// `kIOHIDOptionsTypeNone`: open without seizing the keyboard.
const HID_OPTIONS_NONE: u32 = 0;
/// `kIOHIDRequestTypeListenEvent`.
const IOHID_REQUEST_TYPE_LISTEN_EVENT: u32 = 1;
/// `kIOHIDAccessTypeGranted`.
const IOHID_ACCESS_TYPE_GRANTED: u32 = 0;
/// `kIOHIDParamConnectType`.
const IOHID_PARAM_CONNECT_TYPE: u32 = 1;
/// `kIOHIDCapsLockState`.
const CAPS_LOCK_LOCK_STATE: i32 = 1;
/// `kIOMainPortDefault`.
const MAIN_PORT_DEFAULT: u32 = 0;
/// `kIOReturnSuccess`.
const IO_RETURN_SUCCESS: i32 = 0;
/// Wakes the worker at least this often while the light is steady.
const IDLE_WAIT: Duration = Duration::from_secs(5);
/// Re-read the keyboards this often, so a keyboard plugged in later joins.
const REFRESH_INTERVAL: Duration = Duration::from_secs(5);
/// A stuck IOKit call must not keep the listener from turning the lab off.
const SHUTDOWN_TIMEOUT: Duration = Duration::from_secs(2);

#[link(name = "IOKit", kind = "framework")]
unsafe extern "C" {
    fn IOHIDManagerCreate(allocator: *const c_void, options: u32) -> *mut c_void;
    fn IOHIDManagerSetDeviceMatching(manager: *mut c_void, matching: *const c_void);
    fn IOHIDManagerOpen(manager: *mut c_void, options: u32) -> i32;
    fn IOHIDManagerClose(manager: *mut c_void, options: u32) -> i32;
    fn IOHIDManagerCopyDevices(manager: *mut c_void) -> *mut c_void;
    fn IOHIDDeviceCopyMatchingElements(
        device: *mut c_void,
        matching: *const c_void,
        options: u32,
    ) -> *mut c_void;
    fn IOHIDElementGetUsagePage(element: *mut c_void) -> u32;
    fn IOHIDElementGetUsage(element: *mut c_void) -> u32;
    fn IOHIDDeviceSetValue(device: *mut c_void, element: *mut c_void, value: *mut c_void) -> i32;
    fn IOHIDValueCreateWithIntegerValue(
        allocator: *const c_void,
        element: *mut c_void,
        timestamp: u64,
        value: isize,
    ) -> *mut c_void;
    fn IOHIDCheckAccess(request_type: u32) -> u32;
    fn IOServiceMatching(name: *const c_char) -> *mut c_void;
    fn IOServiceGetMatchingService(main_port: u32, matching: *mut c_void) -> u32;
    fn IOServiceOpen(
        service: u32,
        owning_task: u32,
        connect_type: u32,
        connection: *mut u32,
    ) -> i32;
    fn IOServiceClose(connection: u32) -> i32;
    fn IOObjectRelease(object: u32) -> i32;
    fn IOHIDSetModifierLockState(handle: u32, lock_state: i32, state: bool) -> i32;
}

#[link(name = "CoreFoundation", kind = "framework")]
unsafe extern "C" {
    fn CFSetGetCount(set: *const c_void) -> isize;
    fn CFSetGetValues(set: *const c_void, values: *mut *const c_void);
    fn CFArrayGetCount(array: *const c_void) -> isize;
    fn CFArrayGetValueAtIndex(array: *const c_void, index: isize) -> *const c_void;
    fn CFRetain(value: *const c_void) -> *const c_void;
    fn CFRelease(value: *const c_void);
}

/// What the listener, event tap, and HID tracker send to the worker.
#[derive(Clone, Copy, Debug)]
pub(super) enum WorkerCommand {
    /// The light pattern changed in [`LED_PATTERN`]; re-read it.
    Wake,
    /// An AlphaShift edge changed the software lock; clear it.
    LockOff,
    Shutdown,
}

struct LedElement {
    device: *mut c_void,
    element: *mut c_void,
}

impl Drop for LedElement {
    fn drop(&mut self) {
        // SAFETY: this element retains exactly one device reference and one
        // element reference, both taken when it was cached.
        unsafe {
            CFRelease(self.element.cast_const());
            CFRelease(self.device.cast_const());
        }
    }
}

/// The keyboard LED elements and the `IOHIDSystem` connection. Created and
/// used only on the worker thread.
struct LedOutput {
    manager: *mut c_void,
    system: Option<u32>,
    elements: Vec<LedElement>,
    refreshed_at: Instant,
    missing_logged: bool,
    failure_logged: bool,
}

impl LedOutput {
    fn open() -> Self {
        let system = system_connection();
        if system.is_none() {
            tracing::warn!(
                "Caps Lock dictation could not open IOHIDSystem; Caps Lock will not be cleared after a press"
            );
        }
        let mut output = Self {
            manager: ptr::null_mut(),
            system,
            elements: Vec::new(),
            refreshed_at: Instant::now(),
            missing_logged: false,
            failure_logged: false,
        };
        output.refresh();
        output.lock_off();
        output.write_now(false);
        output
    }

    /// Recreates the manager and re-reads the keyboards. The manager is not
    /// kept unscheduled between refreshes, so a keyboard plugged in later is
    /// discovered here; this runs at most every [`REFRESH_INTERVAL`], never
    /// on each blink.
    fn refresh(&mut self) {
        self.elements.clear();
        if !self.manager.is_null() {
            // SAFETY: the manager was created on this worker thread.
            unsafe {
                IOHIDManagerClose(self.manager, HID_OPTIONS_NONE);
                CFRelease(self.manager.cast_const());
            }
            self.manager = ptr::null_mut();
        }
        // SAFETY: a read-only access query; Hex never requests Input
        // Monitoring or prompts for it.
        let access = unsafe { IOHIDCheckAccess(IOHID_REQUEST_TYPE_LISTEN_EVENT) };
        if access != IOHID_ACCESS_TYPE_GRANTED {
            if !self.failure_logged {
                self.failure_logged = true;
                tracing::warn!(
                    access,
                    "Caps Lock dictation can't drive the keyboard light without Input Monitoring; the Caps Lock light stays under macOS's control"
                );
            }
            self.refreshed_at = Instant::now();
            return;
        }
        self.failure_logged = false;
        // Hex never requests Input Monitoring: opening a keyboard manager
        // without it could prompt, so the light stays off instead.
        // SAFETY: IOKit returns an owned manager; every use stays on this
        // worker thread, and it is released on the next refresh or drop.
        let manager = unsafe { IOHIDManagerCreate(ptr::null(), HID_OPTIONS_NONE) };
        if manager.is_null() {
            self.log_failure("Caps Lock dictation could not create a keyboard LED manager");
            self.refreshed_at = Instant::now();
            return;
        }
        let matching = keyboard_matching();
        // SAFETY: `manager` is live and `matching` is a valid CFDictionary
        // that set-device-matching retains for the manager's lifetime.
        let status = unsafe {
            IOHIDManagerSetDeviceMatching(
                manager,
                (&*matching as *const CFDictionary<CFString, CFNumber>).cast(),
            );
            IOHIDManagerOpen(manager, HID_OPTIONS_NONE)
        };
        if status != IO_RETURN_SUCCESS {
            if !self.failure_logged {
                self.failure_logged = true;
                tracing::warn!(
                    status = format_args!("0x{:08x}", status as u32),
                    "Caps Lock dictation could not open the keyboard LED manager"
                );
            }
            // SAFETY: the manager was created here and is released once.
            unsafe {
                IOHIDManagerClose(manager, HID_OPTIONS_NONE);
                CFRelease(manager.cast_const());
            }
            self.refreshed_at = Instant::now();
            return;
        }
        self.manager = manager;
        // SAFETY: the manager is live for this refresh.
        let devices = unsafe { IOHIDManagerCopyDevices(manager) };
        if devices.is_null() {
            self.refreshed_at = Instant::now();
            return;
        }
        let led_matching = led_matching();
        // SAFETY: `devices` is a live CFSet of IOHIDDeviceRefs.
        let count = unsafe { CFSetGetCount(devices) };
        if count > 0 {
            let mut values = vec![ptr::null::<c_void>(); count as usize];
            // SAFETY: `values` has room for exactly `count` pointers.
            unsafe { CFSetGetValues(devices, values.as_mut_ptr()) };
            for device in values {
                if device.is_null() {
                    continue;
                }
                // SAFETY: the LED matching dictionary copies only Caps Lock
                // LED output elements; it is live through the call.
                let elements = unsafe {
                    IOHIDDeviceCopyMatchingElements(
                        device.cast_mut(),
                        (&*led_matching as *const CFDictionary<CFString, CFNumber>).cast(),
                        0,
                    )
                };
                if elements.is_null() {
                    continue;
                }
                // SAFETY: `elements` is a live CFArray.
                let element_count = unsafe { CFArrayGetCount(elements) };
                for index in 0..element_count.max(0) {
                    // SAFETY: the index is inside the array.
                    let element = unsafe { CFArrayGetValueAtIndex(elements, index) };
                    if element.is_null() {
                        continue;
                    }
                    // Keep the defensive check even though the dictionary
                    // matches exactly: a device that ignores it still cannot
                    // make Hex write a non-LED element.
                    // SAFETY: `element` is live and its usage is a plain
                    // query.
                    if unsafe { IOHIDElementGetUsagePage(element.cast_mut()) } != LED_USAGE_PAGE
                        || unsafe { IOHIDElementGetUsage(element.cast_mut()) }
                            != CAPS_LOCK_LED_USAGE
                    {
                        continue;
                    }
                    // Each cached element owns one retain of its element and
                    // one of its device; `LedElement`'s Drop releases them.
                    // SAFETY: both are live members of `elements`/`devices`.
                    unsafe {
                        CFRetain(element);
                        CFRetain(device);
                    }
                    self.elements.push(LedElement {
                        device: device.cast_mut(),
                        element: element.cast_mut(),
                    });
                }
                // SAFETY: the array was returned owned above.
                unsafe { CFRelease(elements.cast_const()) };
            }
        }
        // SAFETY: `devices` was returned owned by CopyDevices.
        unsafe { CFRelease(devices.cast_const()) };
        self.refreshed_at = Instant::now();
        if self.elements.is_empty() && !self.missing_logged {
            self.missing_logged = true;
            tracing::info!(
                "Caps Lock dictation found no keyboard with a Caps Lock light; dictation still works"
            );
        }
    }

    fn log_failure(&mut self, message: &'static str) {
        if !self.failure_logged {
            self.failure_logged = true;
            tracing::warn!(message);
        }
    }

    /// Forces the software Caps Lock state off. The LED is re-applied by the
    /// caller because this also moves it.
    fn lock_off(&mut self) {
        let Some(connection) = self.system else {
            return;
        };
        super::RESETTING.store(true, Ordering::Release);
        // SAFETY: `connection` is an open IOHIDParam user client for the
        // process's lifetime.
        let status = unsafe { IOHIDSetModifierLockState(connection, CAPS_LOCK_LOCK_STATE, false) };
        if status == IO_RETURN_SUCCESS {
            // The software state is now off; the next on edge is a real press
            // even if this reset produced no event of its own.
            super::LAST_ALPHA_SHIFT.store(false, Ordering::Release);
        } else {
            // No reset edge is coming, so don't let the marker swallow a
            // later off edge.
            super::RESETTING.store(false, Ordering::Release);
            tracing::warn!(
                status = format_args!("0x{:08x}", status as u32),
                "Caps Lock dictation could not clear the Caps Lock state"
            );
        }
    }

    fn write(&mut self, on: bool) {
        if self.refreshed_at.elapsed() >= REFRESH_INTERVAL {
            self.refresh();
        }
        self.write_now(on);
    }

    fn write_now(&mut self, on: bool) {
        if self.manager.is_null() {
            return;
        }
        let mut written = false;
        let mut stale = false;
        for element in &self.elements {
            // SAFETY: the element and its device were retained by refresh.
            let value = unsafe {
                IOHIDValueCreateWithIntegerValue(
                    ptr::null(),
                    element.element,
                    0,
                    if on { 1 } else { 0 },
                )
            };
            if value.is_null() {
                continue;
            }
            // SAFETY: a live value for this element; released below.
            let status = unsafe { IOHIDDeviceSetValue(element.device, element.element, value) };
            // SAFETY: the value was created owned above.
            unsafe { CFRelease(value.cast_const()) };
            if status == IO_RETURN_SUCCESS {
                written = true;
            } else {
                stale = true;
            }
        }
        if stale && !written {
            // A keyboard went away (or its LED output is unsupported); the
            // next periodic refresh re-enumerates, not the next blink.
            self.elements.clear();
        }
    }
}

impl Drop for LedOutput {
    fn drop(&mut self) {
        // A quit or a turned-off lab leaves the light off even if the run
        // loop broke early. Do not refresh here: shutdown stays bounded.
        self.write_now(false);
        self.elements.clear();
        if !self.manager.is_null() {
            // SAFETY: the manager was created and opened on this thread.
            unsafe {
                IOHIDManagerClose(self.manager, HID_OPTIONS_NONE);
                CFRelease(self.manager.cast_const());
            }
        }
        if let Some(connection) = self.system.take() {
            // SAFETY: the connection was opened by `system_connection`.
            unsafe { IOServiceClose(connection) };
        }
    }
}

fn keyboard_matching() -> CFRetained<CFDictionary<CFString, CFNumber>> {
    // `kIOHIDDeviceUsagePageKey` and `kIOHIDDeviceUsageKey` are string macros
    // in IOHIDKeys.h, not exported symbols; CFDictionary compares CFStrings
    // by content, so these match IOKit's own lookups.
    let page_key = CFString::from_static_str("DeviceUsagePage");
    let usage_key = CFString::from_static_str("DeviceUsage");
    let page = CFNumber::new_i32(GENERIC_DESKTOP_USAGE_PAGE);
    let usage = CFNumber::new_i32(KEYBOARD_USAGE);
    CFDictionary::from_slices(&[&page_key, &usage_key], &[&page, &usage])
}

fn led_matching() -> CFRetained<CFDictionary<CFString, CFNumber>> {
    // IOHIDKeys.h defines kIOHIDElementUsagePageKey as "UsagePage" and
    // kIOHIDElementUsageKey as "Usage": the element-key siblings of the
    // device keys above.
    let page_key = CFString::from_static_str("UsagePage");
    let usage_key = CFString::from_static_str("Usage");
    let page = CFNumber::new_i32(LED_USAGE_PAGE as i32);
    let usage = CFNumber::new_i32(CAPS_LOCK_LED_USAGE as i32);
    CFDictionary::from_slices(&[&page_key, &usage_key], &[&page, &usage])
}

#[allow(deprecated)] // libc's replacement is the mach2 crate, which Hex doesn't depend on.
fn system_connection() -> Option<u32> {
    // SAFETY: `IOServiceGetMatchingService` consumes the matching dictionary;
    // the returned service is released here and the opened connection closes
    // on drop.
    unsafe {
        let matching = IOServiceMatching(c"IOHIDSystem".as_ptr());
        if matching.is_null() {
            return None;
        }
        let service = IOServiceGetMatchingService(MAIN_PORT_DEFAULT, matching);
        if service == 0 {
            return None;
        }
        let mut connection = 0;
        let status = IOServiceOpen(
            service,
            libc::mach_task_self(),
            IOHID_PARAM_CONNECT_TYPE,
            &mut connection,
        );
        IOObjectRelease(service);
        if status == IO_RETURN_SUCCESS {
            Some(connection)
        } else {
            None
        }
    }
}

/// The listener's handle. It starts one worker while the lab is on and stops
/// it (light off, software lock cleared) when the lab turns off or Hex quits.
pub(crate) struct CapsLockDictation {
    inner: Mutex<Inner>,
    presses: Sender<CaptureInstant>,
}

struct Inner {
    enabled: bool,
    /// A failed worker spawn is logged once, not on every listener pass.
    spawn_failed: bool,
    worker: Option<Worker>,
    presses: Receiver<CaptureInstant>,
    model: LedModel,
    pattern: LedPattern,
}

struct Worker {
    commands: Sender<WorkerCommand>,
    thread: JoinHandle<()>,
    done: Receiver<()>,
}

impl CapsLockDictation {
    pub(crate) fn new() -> Self {
        let (presses, press_receiver) = mpsc::channel();
        Self {
            inner: Mutex::new(Inner {
                enabled: false,
                spawn_failed: false,
                worker: None,
                presses: press_receiver,
                model: LedModel::default(),
                pattern: LedPattern::Off,
            }),
            presses,
        }
    }

    /// Starts or stops the worker at once. Called on every listener pass, so
    /// the Labs toggle applies without a restart.
    pub(crate) fn set_enabled(&self, on: bool) {
        let mut inner = self.lock();
        if on == inner.enabled {
            if !on {
                inner.spawn_failed = false;
            }
            return;
        }
        if on {
            if inner.spawn_failed {
                return;
            }
            while inner.presses.try_recv().is_ok() {}
            super::HID_GATE
                .lock()
                .unwrap_or_else(|error| error.into_inner())
                .clear();
            super::seed_alpha_shift(crate::suppression::alpha_shift_now());
            inner.model = LedModel::default();
            inner.pattern = LedPattern::Off;
            LED_PATTERN.store(LedPattern::Off.code(), Ordering::Release);
            let (commands, receiver) = mpsc::channel();
            let (done, done_receiver) = mpsc::sync_channel(1);
            *super::WORKER
                .lock()
                .unwrap_or_else(|error| error.into_inner()) = Some(commands.clone());
            *super::PRESSES
                .lock()
                .unwrap_or_else(|error| error.into_inner()) = Some(self.presses.clone());
            ENABLED.store(true, Ordering::Release);
            crate::suppression::physical_keys::watch_caps_lock(Some(super::physical_press));
            let worker = thread::Builder::new()
                .name("hex-caps-lock".into())
                .spawn(move || run(receiver, done));
            match worker {
                Ok(thread) => {
                    inner.worker = Some(Worker {
                        commands,
                        thread,
                        done: done_receiver,
                    });
                    inner.enabled = true;
                }
                Err(error) => {
                    if !inner.spawn_failed {
                        inner.spawn_failed = true;
                        tracing::warn!(%error, "could not start Caps Lock dictation's light worker");
                    }
                    ENABLED.store(false, Ordering::Release);
                    crate::suppression::physical_keys::watch_caps_lock(None);
                    *super::WORKER
                        .lock()
                        .unwrap_or_else(|error| error.into_inner()) = None;
                    *super::PRESSES
                        .lock()
                        .unwrap_or_else(|error| error.into_inner()) = None;
                }
            }
        } else {
            ENABLED.store(false, Ordering::Release);
            crate::suppression::physical_keys::watch_caps_lock(None);
            super::RESETTING.store(false, Ordering::Release);
            super::HID_GATE
                .lock()
                .unwrap_or_else(|error| error.into_inner())
                .clear();
            *super::WORKER
                .lock()
                .unwrap_or_else(|error| error.into_inner()) = None;
            *super::PRESSES
                .lock()
                .unwrap_or_else(|error| error.into_inner()) = None;
            LED_PATTERN.store(LedPattern::Off.code(), Ordering::Release);
            inner.model = LedModel::default();
            inner.pattern = LedPattern::Off;
            if let Some(worker) = inner.worker.take() {
                let _ = worker.commands.send(WorkerCommand::Shutdown);
                if worker.done.recv_timeout(SHUTDOWN_TIMEOUT).is_err() {
                    tracing::warn!(
                        "Caps Lock dictation's light worker did not stop in time; detaching it"
                    );
                }
                // A timed-out worker detaches here instead of blocking the
                // listener; it still clears the light before it exits.
                drop(worker.thread);
            }
            while inner.presses.try_recv().is_ok() {}
            inner.enabled = false;
        }
    }

    /// One confirmed press and its boundary timestamp, if any.
    pub(crate) fn try_recv_press(&self) -> Option<CaptureInstant> {
        self.lock().presses.try_recv().ok()
    }

    /// Mirrors one HUD event into the light model and publishes the pattern
    /// the worker should show.
    pub(crate) fn observe(&self, event: DictationIndicatorEvent) {
        let slow_blink = crate::labs::caps_lock_slow_blink();
        let mut inner = self.lock();
        if !inner.enabled {
            return;
        }
        inner.model.handle(event);
        let pattern = inner.model.pattern(slow_blink);
        if pattern != inner.pattern {
            inner.pattern = pattern;
            LED_PATTERN.store(pattern.code(), Ordering::Release);
            if let Some(worker) = &inner.worker {
                let _ = worker.commands.send(WorkerCommand::Wake);
            }
        }
    }

    fn lock(&self) -> MutexGuard<'_, Inner> {
        self.inner.lock().unwrap_or_else(|error| error.into_inner())
    }
}

#[cfg(test)]
impl CapsLockDictation {
    /// A handle with the lab on and no worker, so a test can drive the
    /// listener's sink and read the model without touching IOKit.
    pub(crate) fn for_test() -> Self {
        let (presses, press_receiver) = mpsc::channel();
        Self {
            inner: Mutex::new(Inner {
                enabled: true,
                spawn_failed: false,
                worker: None,
                presses: press_receiver,
                model: LedModel::default(),
                pattern: LedPattern::Off,
            }),
            presses,
        }
    }

    /// The pattern the light model last decided.
    pub(crate) fn pattern_for_test(&self) -> LedPattern {
        self.lock().pattern
    }
}

impl Drop for CapsLockDictation {
    fn drop(&mut self) {
        self.set_enabled(false);
    }
}

fn run(commands: Receiver<WorkerCommand>, done: SyncSender<()>) {
    let mut output = LedOutput::open();
    let mut current = LedPattern::Off;
    let mut blink_on = false;
    let mut blink_at = None::<Instant>;

    loop {
        let desired = LedPattern::from_code(LED_PATTERN.load(Ordering::Acquire));
        if desired != current {
            current = desired;
            let interval = current.interval();
            blink_on = interval.is_some();
            blink_at = interval.map(|interval| Instant::now() + interval);
            apply(&mut output, current, blink_on);
        }
        let now = Instant::now();
        let wait = blink_at
            .map(|at| at.saturating_duration_since(now))
            .unwrap_or(IDLE_WAIT);
        match commands.recv_timeout(wait) {
            Ok(WorkerCommand::Shutdown) | Err(mpsc::RecvTimeoutError::Disconnected) => break,
            Ok(WorkerCommand::LockOff) => {
                output.lock_off();
                apply(&mut output, current, blink_on);
            }
            Ok(WorkerCommand::Wake) => {}
            Err(mpsc::RecvTimeoutError::Timeout) => {}
        }
        if let Some(interval) = current.interval()
            && blink_at.is_some_and(|at| Instant::now() >= at)
        {
            blink_on = !blink_on;
            blink_at = Some(Instant::now() + interval);
            apply(&mut output, current, blink_on);
        }
    }

    apply(&mut output, LedPattern::Off, false);
    output.lock_off();
    let _ = done.send(());
}

fn apply(output: &mut LedOutput, pattern: LedPattern, blink_on: bool) {
    output
        .write(matches!(pattern, LedPattern::Solid) || (blink_on && pattern.interval().is_some()));
}
