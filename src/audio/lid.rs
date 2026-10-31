use std::ffi::{c_char, c_void};
use std::sync::OnceLock;

use objc2_core_foundation::CFString;

const MAIN_PORT_DEFAULT: u32 = 0;
/// `kIOPMMessageClamshellStateChange`:
/// `iokit_family_msg(sub_iokit_powermanagement, 0x100)`.
const CLAMSHELL_STATE_CHANGE: u32 = 0xE003_4100;

type InterestCallback = unsafe extern "C" fn(*mut c_void, u32, u32, *mut c_void);

#[link(name = "IOKit", kind = "framework")]
unsafe extern "C" {
    fn IOServiceMatching(name: *const c_char) -> *mut c_void;
    fn IOServiceGetMatchingService(main_port: u32, matching: *mut c_void) -> u32;
    fn IORegistryEntryCreateCFProperty(
        entry: u32,
        key: *const c_void,
        allocator: *const c_void,
        options: u32,
    ) -> *const c_void;
    fn IONotificationPortCreate(main_port: u32) -> *mut c_void;
    fn IONotificationPortSetDispatchQueue(port: *mut c_void, queue: *mut c_void);
    fn IOServiceAddInterestNotification(
        port: *mut c_void,
        service: u32,
        interest_type: *const c_char,
        callback: InterestCallback,
        refcon: *mut c_void,
        notification: *mut u32,
    ) -> i32;
}

#[link(name = "CoreFoundation", kind = "framework")]
unsafe extern "C" {
    static kCFBooleanTrue: *const c_void;
    fn CFRelease(value: *const c_void);
}

unsafe extern "C" {
    fn dispatch_get_global_queue(identifier: isize, flags: usize) -> *mut c_void;
}

fn root_domain() -> Option<u32> {
    static ROOT: OnceLock<Option<u32>> = OnceLock::new();
    *ROOT.get_or_init(|| {
        // SAFETY: the matching dictionary is consumed by the lookup, and
        // the returned service is kept for the life of the process.
        let service = unsafe {
            let matching = IOServiceMatching(c"IOPMrootDomain".as_ptr());
            if matching.is_null() {
                return None;
            }
            IOServiceGetMatchingService(MAIN_PORT_DEFAULT, matching)
        };
        (service != 0).then_some(service)
    })
}

pub fn closed() -> bool {
    let Some(root) = root_domain() else {
        return false;
    };
    let key = CFString::from_str("AppleClamshellState");
    let key: *const CFString = &*key;
    // SAFETY: `root` is a live service and `key` a live CFString; the
    // copied property is released after the comparison.
    unsafe {
        let value = IORegistryEntryCreateCFProperty(root, key.cast(), std::ptr::null(), 0);
        if value.is_null() {
            return false;
        }
        let closed = value == kCFBooleanTrue;
        CFRelease(value);
        closed
    }
}

/// Calls `changed` on a dispatch queue whenever the lid opens or closes.
pub fn watch(changed: fn()) {
    static CHANGED: OnceLock<fn()> = OnceLock::new();
    unsafe extern "C" fn on_message(
        _refcon: *mut c_void,
        _service: u32,
        message: u32,
        _argument: *mut c_void,
    ) {
        if message == CLAMSHELL_STATE_CHANGE
            && let Some(changed) = CHANGED.get()
        {
            changed();
        }
    }
    let Some(root) = root_domain() else {
        return;
    };
    if CHANGED.set(changed).is_err() {
        return;
    }
    // SAFETY: the port and notification live for the process, and the
    // callback only reads a static.
    let status = unsafe {
        let port = IONotificationPortCreate(MAIN_PORT_DEFAULT);
        if port.is_null() {
            -1
        } else {
            IONotificationPortSetDispatchQueue(port, dispatch_get_global_queue(0, 0));
            let mut notification = 0;
            IOServiceAddInterestNotification(
                port,
                root,
                c"IOGeneralInterest".as_ptr(),
                on_message,
                std::ptr::null_mut(),
                &mut notification,
            )
        }
    };
    if status != 0 {
        tracing::warn!(status, "could not watch the lid for microphone changes");
    }
}
