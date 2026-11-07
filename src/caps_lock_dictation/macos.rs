//! The macOS side of Labs › Caps Lock dictation: the keyboard LED output
//! elements, the `IOHIDSystem` Caps Lock lock state, and the one worker
//! thread that owns both. Detection arrives as commands from the event tap
//! and the raw HID tracker; no IOKit call runs on either callback thread or
//! the audio thread.
//!
//! The Caps Lock light is an output element on each keyboard (HID usage
//! page 0x08, usage 0x02). Writing it does not change the software Caps Lock
//! state, so Hex can use the light while typing stays lowercase. The lock
//! state is separately reset through `IOHIDSetModifierLockState`.

use std::ffi::{c_char, c_void};
use std::ptr;
use std::sync::atomic::Ordering;
use std::sync::mpsc::{self, Receiver, SyncSender};
use std::sync::{Mutex, MutexGuard};
use std::thread::{self, JoinHandle};
use std::time::{Duration, Instant};

use objc2_core_foundation::{CFDictionary, CFNumber, CFRetained, CFString};

use super::{ENABLED, LedModel, LedPattern, PressDetector, send};
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
/// At most a handful of presses or state changes wait for the worker.
const COMMAND_QUEUE: usize = 8;
/// A press wait that matters is a start or finish; losing one because the
/// listener was briefly busy would strand a capture.
const PRESS_QUEUE: usize = 4;
/// On and off are each this long, a visible but calm blink.
const BLINK_INTERVAL: Duration = Duration::from_millis(350);
/// Wakes the worker at least this often while the light is steady.
const IDLE_WAIT: Duration = Duration::from_secs(5);
/// Re-read the keyboards this often, so a keyboard plugged in later joins.
const REFRESH_INTERVAL: Duration = Duration::from_secs(5);

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

/// What the event tap, HID tracker, and listener send to the worker.
#[derive(Clone, Copy, Debug)]
pub(super) enum Command {
    /// A physical Caps Lock key-down below the keyboard filter, stamped
    /// where it was observed.
    Press {
        at: Instant,
    },
    /// An event tap AlphaShift change. Only an off → on edge starts a
    /// capture; every on edge resets the software lock off.
    Flags {
        shift: bool,
        at: Instant,
    },
    /// One dictation lifecycle event, also offered to the HUD by the
    /// listener.
    Indicator(DictationIndicatorEvent),
    Shutdown,
}

struct LedElement {
    device: *mut c_void,
    element: *mut c_void,
}

/// The keyboard LED elements and the `IOHIDSystem` connection. Created and
/// used only on the worker thread.
struct LedOutput {
    manager: *mut c_void,
    system: Option<u32>,
    elements: Vec<LedElement>,
    refreshed_at: Instant,
    missing_logged: bool,
}

impl LedOutput {
    fn open() -> Self {
        // SAFETY: a read-only access query; Hex never requests Input
        // Monitoring or prompts for it.
        let access = unsafe { IOHIDCheckAccess(IOHID_REQUEST_TYPE_LISTEN_EVENT) };
        let granted = access == IOHID_ACCESS_TYPE_GRANTED;
        if !granted {
            tracing::warn!(
                access,
                "Caps Lock dictation can't drive the keyboard light without Input Monitoring; the Caps Lock light stays under macOS's control"
            );
        }
        // Hex never requests Input Monitoring: opening a keyboard manager
        // without it could prompt, so the light stays off instead.
        let manager = if granted {
            // SAFETY: IOKit returns an owned manager; every use stays on this
            // worker thread, and it is released once when the output drops.
            unsafe { IOHIDManagerCreate(ptr::null(), HID_OPTIONS_NONE) }
        } else {
            ptr::null_mut()
        };
        if granted && manager.is_null() {
            tracing::warn!("Caps Lock dictation could not create a keyboard LED manager");
        } else if granted {
            let matching = keyboard_matching();
            // SAFETY: `manager` is live and `matching` is a valid
            // CFDictionary that set-device-matching retains.
            unsafe {
                IOHIDManagerSetDeviceMatching(
                    manager,
                    (&*matching as *const CFDictionary<CFString, CFNumber>).cast(),
                );
                let status = IOHIDManagerOpen(manager, HID_OPTIONS_NONE);
                if status != IO_RETURN_SUCCESS {
                    tracing::warn!(
                        status = format_args!("0x{:08x}", status as u32),
                        "Caps Lock dictation could not open the keyboard LED manager"
                    );
                }
            }
        }
        let system = system_connection();
        let mut output = Self {
            manager,
            system,
            elements: Vec::new(),
            refreshed_at: Instant::now(),
            missing_logged: false,
        };
        output.refresh();
        output.lock_off();
        output.write(false);
        output
    }

    /// Re-reads the keyboards and remembers each Caps Lock LED element.
    fn refresh(&mut self) {
        for element in self.elements.drain(..) {
            // SAFETY: both were retained when discovered.
            unsafe {
                CFRelease(element.element.cast_const());
                CFRelease(element.device.cast_const());
            }
        }
        if self.manager.is_null() {
            return;
        }
        // SAFETY: the manager is live for the worker's whole life.
        let devices = unsafe { IOHIDManagerCopyDevices(self.manager) };
        if devices.is_null() {
            return;
        }
        // SAFETY: `devices` is a live CFSet of IOHIDDeviceRefs.
        let count = unsafe { CFSetGetCount(devices) };
        if count <= 0 {
            // SAFETY: `devices` is owned here.
            unsafe { CFRelease(devices.cast_const()) };
            return;
        }
        let mut values = vec![ptr::null::<c_void>(); count as usize];
        // SAFETY: `values` has room for exactly `count` pointers.
        unsafe { CFSetGetValues(devices, values.as_mut_ptr()) };
        for device in values {
            if device.is_null() {
                continue;
            }
            // SAFETY: `device` belongs to `devices`, which lives through the
            // loop; retain it for as long as its LED element is remembered.
            unsafe { CFRetain(device) };
            // SAFETY: the device is live; a null matching dictionary copies
            // every element.
            let elements =
                unsafe { IOHIDDeviceCopyMatchingElements(device.cast_mut(), ptr::null(), 0) };
            if elements.is_null() {
                // SAFETY: the retain above is ours to release.
                unsafe { CFRelease(device) };
                continue;
            }
            // SAFETY: `elements` is a live CFArray.
            let element_count = unsafe { CFArrayGetCount(elements) };
            let mut found = false;
            for index in 0..element_count.max(0) {
                // SAFETY: the index is inside the array.
                let element = unsafe { CFArrayGetValueAtIndex(elements, index) };
                if element.is_null() {
                    continue;
                }
                // SAFETY: `element` is live and its usage is a plain query.
                if unsafe { IOHIDElementGetUsagePage(element.cast_mut()) } != LED_USAGE_PAGE
                    || unsafe { IOHIDElementGetUsage(element.cast_mut()) } != CAPS_LOCK_LED_USAGE
                {
                    continue;
                }
                // SAFETY: the element belongs to `elements`; retain it and
                // the device for the cache.
                unsafe { CFRetain(element) };
                self.elements.push(LedElement {
                    device: device.cast_mut(),
                    element: element.cast_mut(),
                });
                found = true;
            }
            // SAFETY: the array was returned owned above.
            unsafe { CFRelease(elements.cast_const()) };
            if !found {
                // SAFETY: the device retain above is ours to release.
                unsafe { CFRelease(device) };
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

    /// Forces the software Caps Lock state off. The LED is re-applied by the
    /// caller because this also moves it.
    fn lock_off(&mut self) {
        let Some(connection) = self.system else {
            return;
        };
        // SAFETY: `connection` is an open IOHIDParam user client for the
        // process's lifetime.
        let status = unsafe { IOHIDSetModifierLockState(connection, CAPS_LOCK_LOCK_STATE, false) };
        if status != IO_RETURN_SUCCESS {
            tracing::warn!(
                status = format_args!("0x{:08x}", status as u32),
                "Caps Lock dictation could not clear the Caps Lock state"
            );
        }
    }

    fn write(&mut self, on: bool) {
        if self.manager.is_null() {
            return;
        }
        if self.elements.is_empty() || self.refreshed_at.elapsed() >= REFRESH_INTERVAL {
            self.refresh();
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
            // A keyboard went away (or its LED output is unsupported); look
            // again next time instead of writing a dead element forever.
            self.elements.clear();
        }
    }
}

impl Drop for LedOutput {
    fn drop(&mut self) {
        // A quit or a turned-off lab leaves the light off even if the run
        // loop broke early.
        self.write(false);
        for element in self.elements.drain(..) {
            // SAFETY: both were retained by refresh.
            unsafe {
                CFRelease(element.element.cast_const());
                CFRelease(element.device.cast_const());
            }
        }
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

#[allow(deprecated)] // libc's replacement is the mach2 crate, which Hex doesn't depend on.
fn system_connection() -> Option<u32> {
    // SAFETY: `IOServiceMatching` consumes its dictionary; the returned
    // service is released here and the opened connection closes on drop.
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
    presses: SyncSender<()>,
}

struct Inner {
    enabled: bool,
    worker: Option<Worker>,
    presses: Receiver<()>,
}

struct Worker {
    commands: SyncSender<Command>,
    thread: JoinHandle<()>,
}

impl CapsLockDictation {
    pub(crate) fn new() -> Self {
        let (presses, press_receiver) = mpsc::sync_channel(PRESS_QUEUE);
        Self {
            inner: Mutex::new(Inner {
                enabled: false,
                worker: None,
                presses: press_receiver,
            }),
            presses,
        }
    }

    /// Starts or stops the worker at once. Called on every listener pass, so
    /// the Labs toggle applies without a restart.
    pub(crate) fn set_enabled(&self, on: bool) {
        let mut inner = self.lock();
        if on == inner.enabled {
            return;
        }
        if on {
            while inner.presses.try_recv().is_ok() {}
            let (commands, receiver) = mpsc::sync_channel(COMMAND_QUEUE);
            *super::COMMANDS
                .lock()
                .unwrap_or_else(|error| error.into_inner()) = Some(commands.clone());
            ENABLED.store(true, Ordering::Release);
            crate::suppression::physical_keys::watch_caps_lock(Some(super::physical_press));
            let presses = self.presses.clone();
            let worker = thread::Builder::new()
                .name("hex-caps-lock".into())
                .spawn(move || run(receiver, presses));
            match worker {
                Ok(thread) => inner.worker = Some(Worker { commands, thread }),
                Err(error) => {
                    tracing::warn!(%error, "could not start Caps Lock dictation's light worker");
                    ENABLED.store(false, Ordering::Release);
                    *super::COMMANDS
                        .lock()
                        .unwrap_or_else(|error| error.into_inner()) = None;
                    crate::suppression::physical_keys::watch_caps_lock(None);
                    return;
                }
            }
            inner.enabled = true;
        } else {
            ENABLED.store(false, Ordering::Release);
            crate::suppression::physical_keys::watch_caps_lock(None);
            *super::COMMANDS
                .lock()
                .unwrap_or_else(|error| error.into_inner()) = None;
            if let Some(worker) = inner.worker.take() {
                let _ = worker.commands.send(Command::Shutdown);
                let _ = worker.thread.join();
            }
            while inner.presses.try_recv().is_ok() {}
            inner.enabled = false;
        }
    }

    /// One press, if the worker has seen one since the last call.
    pub(crate) fn try_recv_press(&self) -> bool {
        self.lock().presses.try_recv().is_ok()
    }

    /// Mirrors one HUD event into the light model.
    pub(crate) fn observe(&self, event: DictationIndicatorEvent) {
        if ENABLED.load(Ordering::Acquire) {
            send(Command::Indicator(event));
        }
    }

    fn lock(&self) -> MutexGuard<'_, Inner> {
        self.inner.lock().unwrap_or_else(|error| error.into_inner())
    }
}

impl Drop for CapsLockDictation {
    fn drop(&mut self) {
        self.set_enabled(false);
    }
}

fn run(commands: Receiver<Command>, presses: SyncSender<()>) {
    let mut output = LedOutput::open();
    let mut model = LedModel::default();
    let mut detector = PressDetector::default();
    let mut pattern = LedPattern::Off;
    let mut blink_on = false;
    let mut blink_at = None::<Instant>;
    apply(&mut output, pattern, blink_on);

    loop {
        let now = Instant::now();
        let wait = blink_at
            .map(|at| at.saturating_duration_since(now))
            .unwrap_or(IDLE_WAIT);
        match commands.recv_timeout(wait) {
            Ok(Command::Shutdown) | Err(mpsc::RecvTimeoutError::Disconnected) => break,
            Ok(Command::Press { at }) => {
                output.lock_off();
                apply(&mut output, pattern, blink_on);
                if detector.physical(at) {
                    send_press(&presses);
                }
            }
            Ok(Command::Flags { shift, at }) => {
                if shift {
                    output.lock_off();
                    apply(&mut output, pattern, blink_on);
                }
                if detector.flags(shift, at) {
                    send_press(&presses);
                }
            }
            Ok(Command::Indicator(event)) => {
                model.handle(event);
                let next = model.pattern();
                if next != pattern {
                    pattern = next;
                    match pattern {
                        LedPattern::Off => {
                            blink_on = false;
                            blink_at = None;
                        }
                        LedPattern::Solid => {
                            blink_on = false;
                            blink_at = None;
                        }
                        LedPattern::Blink => {
                            blink_on = true;
                            blink_at = Some(Instant::now() + BLINK_INTERVAL);
                        }
                    }
                    apply(&mut output, pattern, blink_on);
                }
            }
            Err(mpsc::RecvTimeoutError::Timeout) => {}
        }
        if pattern == LedPattern::Blink && blink_at.is_some_and(|at| Instant::now() >= at) {
            blink_on = !blink_on;
            blink_at = Some(Instant::now() + BLINK_INTERVAL);
            apply(&mut output, pattern, blink_on);
        }
    }

    apply(&mut output, LedPattern::Off, false);
    output.lock_off();
}

fn apply(output: &mut LedOutput, pattern: LedPattern, blink_on: bool) {
    output
        .write(matches!(pattern, LedPattern::Solid) || (pattern == LedPattern::Blink && blink_on));
}

fn send_press(presses: &SyncSender<()>) {
    let _ = presses.try_send(());
}
