//! The input and output routes Hex follows: the counters that advance when the
//! OS's devices or its default input change, the CoreAudio and lid listeners
//! that advance them, and the Bluetooth-avoidance setting that moves the input
//! route.

use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};

#[cfg(target_os = "macos")]
use super::lid;

static AUDIO_SERVICE_RESTART_GENERATION: AtomicU64 = AtomicU64::new(0);
static INPUT_ROUTE_GENERATION: AtomicU64 = AtomicU64::new(0);
static AVOID_BLUETOOTH_MICROPHONE: AtomicBool = AtomicBool::new(true);

/// Whether System default passes over a Bluetooth default input, whose
/// microphone drops the headset into low-quality call audio. A change
/// resolves the input again once capture is idle.
pub fn set_avoid_bluetooth_microphone(avoid: bool) {
    if AVOID_BLUETOOTH_MICROPHONE.swap(avoid, Ordering::AcqRel) != avoid {
        INPUT_ROUTE_GENERATION.fetch_add(1, Ordering::AcqRel);
    }
}

pub(super) fn avoid_bluetooth_microphone() -> bool {
    AVOID_BLUETOOTH_MICROPHONE.load(Ordering::Acquire)
}

pub fn notify_audio_service_restarted() {
    AUDIO_SERVICE_RESTART_GENERATION.fetch_add(1, Ordering::AcqRel);
}

pub fn audio_service_restart_generation() -> u64 {
    AUDIO_SERVICE_RESTART_GENERATION.load(Ordering::Acquire)
}

/// Advances when the OS's default input or its list of inputs changes, or
/// a MacBook's lid opens or closes, so an input following the system
/// default, or standing in for a missing saved microphone, resolves again.
/// macOS reports these through CoreAudio listeners and an IOKit interest
/// notification; Windows reports a default change through its stream.
pub fn input_route_generation() -> u64 {
    INPUT_ROUTE_GENERATION.load(Ordering::Acquire)
}

#[cfg(any(target_os = "macos", target_os = "windows"))]
pub(super) fn notify_input_route_changed() {
    INPUT_ROUTE_GENERATION.fetch_add(1, Ordering::AcqRel);
}

static OUTPUT_ROUTE_GENERATION: AtomicU64 = AtomicU64::new(0);

/// Advances when the OS's default output or its list of devices changes, or
/// the audio service restarts, so a warm feedback output opened on an older
/// route reopens on the current default before its next tone. Only macOS
/// reports these, through the same CoreAudio listeners as the input route.
pub fn output_route_generation() -> u64 {
    OUTPUT_ROUTE_GENERATION
        .load(Ordering::Acquire)
        .wrapping_add(audio_service_restart_generation())
}

#[cfg(target_os = "macos")]
fn notify_output_route_changed() {
    OUTPUT_ROUTE_GENERATION.fetch_add(1, Ordering::AcqRel);
}

/// Registers the CoreAudio listeners behind [`audio_service_restart_generation`]
/// [`input_route_generation`], and [`output_route_generation`]. Their callbacks run on CoreAudio's
/// notification thread, never the audio callback, and only bump a counter.
#[cfg(target_os = "macos")]
pub fn watch_input_devices() {
    use std::ffi::c_void;
    use std::sync::Once;

    #[repr(C)]
    struct AudioObjectPropertyAddress {
        m_selector: u32,
        m_scope: u32,
        m_element: u32,
    }

    type PropertyListenerProc =
        unsafe extern "C" fn(u32, u32, *const AudioObjectPropertyAddress, *mut c_void) -> i32;

    unsafe extern "C" {
        fn AudioObjectAddPropertyListener(
            in_object_id: u32,
            in_address: *const AudioObjectPropertyAddress,
            in_listener: PropertyListenerProc,
            in_client_data: *mut c_void,
        ) -> i32;
    }

    unsafe extern "C" fn on_service_restarted(
        _in_object_id: u32,
        _in_number_addresses: u32,
        _in_addresses: *const AudioObjectPropertyAddress,
        _in_client_data: *mut c_void,
    ) -> i32 {
        tracing::warn!("CoreAudio daemon (coreaudiod) restarted; recovering microphone stream");
        notify_audio_service_restarted();
        0
    }

    unsafe extern "C" fn on_inputs_changed(
        _in_object_id: u32,
        _in_number_addresses: u32,
        _in_addresses: *const AudioObjectPropertyAddress,
        _in_client_data: *mut c_void,
    ) -> i32 {
        notify_input_route_changed();
        0
    }

    unsafe extern "C" fn on_outputs_changed(
        _in_object_id: u32,
        _in_number_addresses: u32,
        _in_addresses: *const AudioObjectPropertyAddress,
        _in_client_data: *mut c_void,
    ) -> i32 {
        notify_output_route_changed();
        0
    }

    unsafe extern "C" fn on_devices_changed(
        _in_object_id: u32,
        _in_number_addresses: u32,
        _in_addresses: *const AudioObjectPropertyAddress,
        _in_client_data: *mut c_void,
    ) -> i32 {
        notify_input_route_changed();
        notify_output_route_changed();
        0
    }

    static REGISTER: Once = Once::new();
    REGISTER.call_once(|| {
        const SYSTEM_OBJECT: u32 = 1;
        const SERVICE_RESTARTED_SELECTOR: u32 = u32::from_be_bytes(*b"srst");
        const DEFAULT_INPUT_SELECTOR: u32 = u32::from_be_bytes(*b"dIn ");
        const DEFAULT_OUTPUT_SELECTOR: u32 = u32::from_be_bytes(*b"dOut");
        const DEVICES_SELECTOR: u32 = u32::from_be_bytes(*b"dev#");
        const GLOBAL_SCOPE: u32 = u32::from_be_bytes(*b"glob");
        let listeners: [(u32, PropertyListenerProc); 4] = [
            (SERVICE_RESTARTED_SELECTOR, on_service_restarted),
            (DEFAULT_INPUT_SELECTOR, on_inputs_changed),
            (DEFAULT_OUTPUT_SELECTOR, on_outputs_changed),
            (DEVICES_SELECTOR, on_devices_changed),
        ];
        for (selector, listener) in listeners {
            let addr = AudioObjectPropertyAddress {
                m_selector: selector,
                m_scope: GLOBAL_SCOPE,
                m_element: 0,
            };
            let status = unsafe {
                AudioObjectAddPropertyListener(SYSTEM_OBJECT, &addr, listener, std::ptr::null_mut())
            };
            if status != 0 {
                tracing::warn!(status, "could not watch CoreAudio input devices");
            }
        }
        // Closing the lid switches the built-in microphone off without
        // changing CoreAudio's device list or default input.
        lid::watch(notify_input_route_changed);
    });
}
