use super::*;
use objc2_core_foundation::{CFRetained, CFString};
use std::ffi::c_void;
use std::mem::size_of;
use std::ptr;

type AudioObjectID = u32;
type AudioObjectPropertySelector = u32;
type AudioObjectPropertyScope = u32;
type AudioObjectPropertyElement = u32;
type OSStatus = i32;
type AudioDeviceIOProcID = *mut c_void;

const K_AUDIO_OBJECT_SYSTEM_OBJECT: AudioObjectID = 1;
const K_AUDIO_OBJECT_UNKNOWN: AudioObjectID = 0;

const K_AUDIO_OBJECT_PROPERTY_SCOPE_GLOBAL: AudioObjectPropertyScope = u32::from_be_bytes(*b"glob");
const K_AUDIO_OBJECT_PROPERTY_SCOPE_OUTPUT: AudioObjectPropertyScope = u32::from_be_bytes(*b"outp");
const K_AUDIO_OBJECT_PROPERTY_ELEMENT_MAIN: AudioObjectPropertyElement = 0;

const K_AUDIO_HARDWARE_PROPERTY_TRANSLATE_UID_TO_DEVICE: AudioObjectPropertySelector =
    u32::from_be_bytes(*b"uidd");
const K_AUDIO_DEVICE_PROPERTY_DEVICE_IS_ALIVE: AudioObjectPropertySelector =
    u32::from_be_bytes(*b"livn");
const K_AUDIO_DEVICE_PROPERTY_DEVICE_IS_RUNNING_SOMEWHERE: AudioObjectPropertySelector =
    u32::from_be_bytes(*b"gone");
const K_AUDIO_DEVICE_PROPERTY_BUFFER_FRAME_SIZE: AudioObjectPropertySelector =
    u32::from_be_bytes(*b"fsiz");

#[repr(C)]
#[derive(Clone, Copy)]
struct AudioObjectPropertyAddress {
    m_selector: AudioObjectPropertySelector,
    m_scope: AudioObjectPropertyScope,
    m_element: AudioObjectPropertyElement,
}

#[repr(C)]
struct AudioBuffer {
    m_number_channels: u32,
    m_data_byte_size: u32,
    m_data: *mut c_void,
}

#[repr(C)]
struct AudioBufferList {
    m_number_buffers: u32,
    m_buffers: [AudioBuffer; 1],
}

type AudioDeviceIOProc = unsafe extern "C" fn(
    in_device: AudioObjectID,
    in_now: *const c_void,
    in_input_data: *const AudioBufferList,
    in_input_time: *const c_void,
    out_output_data: *mut AudioBufferList,
    in_output_time: *const c_void,
    in_client_data: *mut c_void,
) -> OSStatus;

#[link(name = "CoreAudio", kind = "framework")]
unsafe extern "C" {
    fn AudioObjectGetPropertyData(
        in_object_id: AudioObjectID,
        in_address: *const AudioObjectPropertyAddress,
        in_qualifier_data_size: u32,
        in_qualifier_data: *const c_void,
        io_data_size: *mut u32,
        out_data: *mut c_void,
    ) -> OSStatus;

    fn AudioObjectSetPropertyData(
        in_object_id: AudioObjectID,
        in_address: *const AudioObjectPropertyAddress,
        in_qualifier_data_size: u32,
        in_qualifier_data: *const c_void,
        in_data_size: u32,
        in_data: *const c_void,
    ) -> OSStatus;

    fn AudioDeviceCreateIOProcID(
        in_device: AudioObjectID,
        in_proc: AudioDeviceIOProc,
        in_client_data: *mut c_void,
        out_io_proc_id: *mut AudioDeviceIOProcID,
    ) -> OSStatus;

    fn AudioDeviceDestroyIOProcID(
        in_device: AudioObjectID,
        in_io_proc_id: AudioDeviceIOProcID,
    ) -> OSStatus;

    fn AudioDeviceStart(in_device: AudioObjectID, in_proc_id: AudioDeviceIOProcID) -> OSStatus;

    fn AudioDeviceStop(in_device: AudioObjectID, in_proc_id: AudioDeviceIOProcID) -> OSStatus;
}

pub(super) fn translate_uid_to_device(uid: &str) -> Option<AudioObjectID> {
    let cf_uid = CFString::from_str(uid);
    let raw_cf = CFRetained::as_ptr(&cf_uid).as_ptr().cast::<c_void>();
    let address = AudioObjectPropertyAddress {
        m_selector: K_AUDIO_HARDWARE_PROPERTY_TRANSLATE_UID_TO_DEVICE,
        m_scope: K_AUDIO_OBJECT_PROPERTY_SCOPE_GLOBAL,
        m_element: K_AUDIO_OBJECT_PROPERTY_ELEMENT_MAIN,
    };
    let mut device_id: AudioObjectID = K_AUDIO_OBJECT_UNKNOWN;
    let mut data_size = size_of::<AudioObjectID>() as u32;
    let status = unsafe {
        AudioObjectGetPropertyData(
            K_AUDIO_OBJECT_SYSTEM_OBJECT,
            &address,
            size_of::<*const c_void>() as u32,
            (&raw const raw_cf).cast::<c_void>(),
            &mut data_size,
            (&raw mut device_id).cast::<c_void>(),
        )
    };
    if status == 0 && device_id != K_AUDIO_OBJECT_UNKNOWN {
        Some(device_id)
    } else {
        None
    }
}

fn device_flag(device_id: AudioObjectID, selector: AudioObjectPropertySelector) -> bool {
    let address = AudioObjectPropertyAddress {
        m_selector: selector,
        m_scope: K_AUDIO_OBJECT_PROPERTY_SCOPE_GLOBAL,
        m_element: K_AUDIO_OBJECT_PROPERTY_ELEMENT_MAIN,
    };
    let mut value: u32 = 0;
    let mut size = size_of::<u32>() as u32;
    let status = unsafe {
        AudioObjectGetPropertyData(
            device_id,
            &address,
            0,
            ptr::null(),
            &mut size,
            (&raw mut value).cast::<c_void>(),
        )
    };
    status == 0 && value != 0
}

pub(super) fn device_is_alive(device_id: AudioObjectID) -> bool {
    device_flag(device_id, K_AUDIO_DEVICE_PROPERTY_DEVICE_IS_ALIVE)
}

pub(super) fn device_is_running_somewhere(device_id: AudioObjectID) -> bool {
    device_flag(
        device_id,
        K_AUDIO_DEVICE_PROPERTY_DEVICE_IS_RUNNING_SOMEWHERE,
    )
}

fn set_preferred_buffer_size(device_id: AudioObjectID, frames: u32) {
    let address = AudioObjectPropertyAddress {
        m_selector: K_AUDIO_DEVICE_PROPERTY_BUFFER_FRAME_SIZE,
        m_scope: K_AUDIO_OBJECT_PROPERTY_SCOPE_OUTPUT,
        m_element: K_AUDIO_OBJECT_PROPERTY_ELEMENT_MAIN,
    };
    let _ = unsafe {
        AudioObjectSetPropertyData(
            device_id,
            &address,
            0,
            ptr::null(),
            size_of::<u32>() as u32,
            (&raw const frames).cast::<c_void>(),
        )
    };
}

/// How often a streaming sink logs new ring underflows and overflows.
const RING_REPORT_INTERVAL: Duration = Duration::from_secs(60);

struct IoContext {
    engine: HexMicEngine,
}

unsafe extern "C" fn sink_io_proc(
    _in_device: AudioObjectID,
    _in_now: *const c_void,
    _in_input_data: *const AudioBufferList,
    _in_input_time: *const c_void,
    out_output_data: *mut AudioBufferList,
    _in_output_time: *const c_void,
    in_client_data: *mut c_void,
) -> OSStatus {
    if out_output_data.is_null() || in_client_data.is_null() {
        return 0;
    }
    let ctx = unsafe { &mut *(in_client_data.cast::<IoContext>()) };
    let abl = unsafe { &mut *out_output_data };
    if abl.m_number_buffers == 0 {
        return 0;
    }
    let buf = &mut abl.m_buffers[0];
    if buf.m_data.is_null() || buf.m_data_byte_size == 0 {
        return 0;
    }
    let channels = (buf.m_number_channels as usize).max(1);
    let total_samples = (buf.m_data_byte_size as usize) / size_of::<f32>();
    let frames = total_samples / channels;
    if frames == 0 {
        return 0;
    }

    let out_slice =
        unsafe { std::slice::from_raw_parts_mut(buf.m_data.cast::<f32>(), total_samples) };

    if channels == 1 {
        render(&mut ctx.engine, out_slice);
    } else {
        let mut mono_buf = [0.0_f32; 2048];
        let count = frames.min(mono_buf.len());
        render(&mut ctx.engine, &mut mono_buf[..count]);
        for f in 0..count {
            let s = mono_buf[f];
            for c in 0..channels {
                out_slice[f * channels + c] = s;
            }
        }
        if count < frames {
            out_slice[count * channels..].fill(0.0);
        }
    }
    0
}

pub(super) fn run_sink_supervisor() {
    loop {
        if !crate::app_settings::hex_microphone_enabled() {
            SINK_STREAMING.store(false, Ordering::Release);
            thread::sleep(Duration::from_millis(500));
            continue;
        }

        let Some(sink_id) = translate_uid_to_device(HEX_MICROPHONE_SINK_UID) else {
            SINK_STREAMING.store(false, Ordering::Release);
            let wait = if super::driver_action_in_progress() {
                Duration::from_millis(200)
            } else {
                Duration::from_secs(1)
            };
            thread::sleep(wait);
            continue;
        };

        let restart_gen = crate::audio::audio_service_restart_generation();
        set_preferred_buffer_size(sink_id, 256);
        CAPTURE_RING.clear();

        let cfg = current_tuned_config();
        let mut io_ctx = Box::new(IoContext {
            engine: cfg.build_engine(),
        });

        let mut proc_id: AudioDeviceIOProcID = ptr::null_mut();
        let create_status = unsafe {
            AudioDeviceCreateIOProcID(
                sink_id,
                sink_io_proc,
                (&raw mut *io_ctx).cast::<c_void>(),
                &mut proc_id,
            )
        };
        if create_status != 0 || proc_id.is_null() {
            tracing::warn!(
                status = create_status,
                "failed to create Hex Microphone sink IOProc; retrying"
            );
            thread::sleep(Duration::from_millis(500));
            continue;
        }

        let start_status = unsafe { AudioDeviceStart(sink_id, proc_id) };
        if start_status != 0 {
            let _ = unsafe { AudioDeviceDestroyIOProcID(sink_id, proc_id) };
            tracing::warn!(
                status = start_status,
                "failed to start Hex Microphone sink IOProc; retrying"
            );
            thread::sleep(Duration::from_millis(500));
            continue;
        }

        SINK_STREAMING.store(true, Ordering::Release);
        tracing::info!(
            device_id = sink_id,
            mode = cfg.mode.id(),
            "Hex Microphone virtual sink streaming started"
        );

        let started = CAPTURE_RING.counters();
        let mut reported = started;
        let mut last_report = Instant::now();
        while crate::app_settings::hex_microphone_enabled()
            && device_is_alive(sink_id)
            && crate::audio::audio_service_restart_generation() == restart_gen
        {
            thread::sleep(Duration::from_millis(250));
            if last_report.elapsed() >= RING_REPORT_INTERVAL {
                last_report = Instant::now();
                let now = CAPTURE_RING.counters();
                // Only while an app reads Hex Microphone: an idle released
                // microphone underflows on every callback by design.
                let in_use = translate_uid_to_device(HEX_MICROPHONE_UID)
                    .is_some_and(device_is_running_somewhere);
                if in_use && now != reported {
                    tracing::info!(
                        underflows = now.0 - reported.0,
                        overflows = now.1 - reported.1,
                        interval_s = RING_REPORT_INTERVAL.as_secs(),
                        "Hex Microphone ring glitched"
                    );
                }
                reported = now;
            }
        }

        SINK_STREAMING.store(false, Ordering::Release);
        let _ = unsafe { AudioDeviceStop(sink_id, proc_id) };
        let _ = unsafe { AudioDeviceDestroyIOProcID(sink_id, proc_id) };
        let stopped = CAPTURE_RING.counters();
        CAPTURE_RING.clear();
        tracing::info!(
            device_id = sink_id,
            underflows = stopped.0 - started.0,
            overflows = stopped.1 - started.1,
            "Hex Microphone virtual sink stopped"
        );
    }
}
