//! How an input device connects (built in, Bluetooth, virtual), as each
//! platform reports it.

use cpal::Device;
#[cfg(any(target_os = "windows", target_os = "macos"))]
use cpal::traits::DeviceTrait;

/// How an input connects, as far as resolving it cares.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) enum Transport {
    /// The internal microphone, switched off while a MacBook's lid is
    /// closed. A built-in headphone-jack input is `Other`.
    BuiltIn,
    /// Bluetooth or Bluetooth LE: opening its microphone switches the
    /// headset into low-quality call audio.
    Bluetooth,
    /// A virtual or aggregate device, such as Microsoft Teams Audio, which
    /// may record silence outside its own app, so it never stands in.
    Virtual,
    /// An iPhone microphone offered through Continuity, wired or wireless.
    /// Opening it prompts on the phone and can fail, so it never stands in,
    /// though a Continuity default, saved microphone, or `--device` is used.
    #[cfg_attr(not(target_os = "macos"), allow(dead_code))]
    Continuity,
    Other,
}

/// CoreAudio's Continuity Capture transport types,
/// `kAudioDeviceTransportTypeContinuityCaptureWired` and `…Wireless`.
#[cfg_attr(not(target_os = "macos"), allow(dead_code))]
const CONTINUITY_CAPTURE_WIRED: u32 = u32::from_be_bytes(*b"ccwd");
#[cfg_attr(not(target_os = "macos"), allow(dead_code))]
const CONTINUITY_CAPTURE_WIRELESS: u32 = u32::from_be_bytes(*b"ccwl");

/// Whether a CoreAudio transport type is an iPhone's Continuity microphone.
#[cfg_attr(not(target_os = "macos"), allow(dead_code))]
fn is_continuity_capture(kind: u32) -> bool {
    kind == CONTINUITY_CAPTURE_WIRED || kind == CONTINUITY_CAPTURE_WIRELESS
}

/// ALSA devices carry no transport; Linux passes over a Bluetooth default
/// through the sound server instead (`linux_input_route`).
#[cfg(target_os = "linux")]
pub(super) fn transport(_device: &Device) -> Transport {
    Transport::Other
}

/// How WASAPI reports `device` connects, from its enumerator and form
/// factor. cpal names only `BTHENUM` Bluetooth; a hands-free headset
/// endpoint (`BTHHFENUM`) has no bus it knows, so a headset or handset of
/// unknown bus counts as Bluetooth too.
#[cfg(target_os = "windows")]
pub(super) fn transport(device: &Device) -> Transport {
    use cpal::{DeviceType, InterfaceType};
    let Ok(description) = device.description() else {
        return Transport::Other;
    };
    match description.interface_type() {
        InterfaceType::BuiltIn => Transport::BuiltIn,
        InterfaceType::Bluetooth => Transport::Bluetooth,
        InterfaceType::Virtual | InterfaceType::Aggregate => Transport::Virtual,
        InterfaceType::Unknown
            if matches!(
                description.device_type(),
                DeviceType::Headset | DeviceType::Handset
            ) =>
        {
            Transport::Bluetooth
        }
        _ => Transport::Other,
    }
}

/// Whether a built-in CoreAudio input is the internal microphone the lid
/// switches off, from its UID and selected input data source. A microphone
/// in the headphone jack is built in too, but stays on with the lid closed:
/// Apple silicon lists it as its own device (`BuiltInHeadphoneInputDevice`),
/// older Macs as the built-in input's external-microphone or line-in source.
#[cfg_attr(not(target_os = "macos"), allow(dead_code))]
fn is_lid_switched_microphone(uid: &str, data_source: Option<u32>) -> bool {
    const EXTERNAL_MICROPHONE: u32 = u32::from_be_bytes(*b"emic");
    const LINE_IN: u32 = u32::from_be_bytes(*b"line");
    if uid.contains("HeadphoneInput") {
        return false;
    }
    !matches!(data_source, Some(EXTERNAL_MICROPHONE | LINE_IN))
}

/// How CoreAudio reports `device` connects: built in, such as a MacBook's
/// microphone, or Bluetooth (classic or LE), such as AirPods. A
/// headphone-jack microphone counts as other, since closing the lid leaves
/// it on.
#[cfg(target_os = "macos")]
pub(super) fn transport(device: &Device) -> Transport {
    use std::ffi::c_void;
    use std::ptr::NonNull;

    use objc2_core_audio::{
        AudioObjectGetPropertyData, AudioObjectID, AudioObjectPropertyAddress,
        AudioObjectPropertySelector, kAudioDevicePropertyTransportType,
        kAudioDeviceTransportTypeAggregate, kAudioDeviceTransportTypeAutoAggregate,
        kAudioDeviceTransportTypeBluetooth, kAudioDeviceTransportTypeBluetoothLE,
        kAudioDeviceTransportTypeBuiltIn, kAudioDeviceTransportTypeVirtual,
        kAudioHardwarePropertyTranslateUIDToDevice, kAudioObjectPropertyElementMain,
        kAudioObjectPropertyScopeGlobal, kAudioObjectSystemObject,
    };
    use objc2_core_foundation::CFString;

    /// `kAudioDevicePropertyDataSource` and `kAudioObjectPropertyScopeInput`.
    const DATA_SOURCE_SELECTOR: u32 = u32::from_be_bytes(*b"ssrc");
    const INPUT_SCOPE: u32 = u32::from_be_bytes(*b"inpt");

    fn read_u32(
        object: AudioObjectID,
        selector: AudioObjectPropertySelector,
        scope: u32,
        qualifier: Option<NonNull<c_void>>,
        qualifier_size: u32,
    ) -> Option<u32> {
        let mut address = AudioObjectPropertyAddress {
            mSelector: selector,
            mScope: scope,
            mElement: kAudioObjectPropertyElementMain,
        };
        let mut value = 0_u32;
        let mut size = std::mem::size_of::<u32>() as u32;
        let status = unsafe {
            AudioObjectGetPropertyData(
                object,
                NonNull::from(&mut address),
                qualifier_size,
                qualifier.map_or(std::ptr::null(), |qualifier| qualifier.as_ptr()),
                NonNull::from(&mut size),
                NonNull::from(&mut value).cast(),
            )
        };
        (status == 0 && size as usize == std::mem::size_of::<u32>()).then_some(value)
    }

    let Ok(id) = device.id() else {
        return Transport::Other;
    };
    let uid = CFString::from_str(id.id());
    let uid_ref: *const CFString = &*uid;
    let Some(device_id) = read_u32(
        kAudioObjectSystemObject as AudioObjectID,
        kAudioHardwarePropertyTranslateUIDToDevice,
        kAudioObjectPropertyScopeGlobal,
        Some(NonNull::from(&uid_ref).cast()),
        std::mem::size_of::<*const CFString>() as u32,
    )
    .filter(|device_id| *device_id != 0) else {
        return Transport::Other;
    };
    match read_u32(
        device_id,
        kAudioDevicePropertyTransportType,
        kAudioObjectPropertyScopeGlobal,
        None,
        0,
    ) {
        Some(kind) if kind == kAudioDeviceTransportTypeBuiltIn => {
            let data_source = read_u32(device_id, DATA_SOURCE_SELECTOR, INPUT_SCOPE, None, 0);
            if is_lid_switched_microphone(id.id(), data_source) {
                Transport::BuiltIn
            } else {
                Transport::Other
            }
        }
        Some(kind)
            if kind == kAudioDeviceTransportTypeBluetooth
                || kind == kAudioDeviceTransportTypeBluetoothLE =>
        {
            Transport::Bluetooth
        }
        Some(kind)
            if kind == kAudioDeviceTransportTypeVirtual
                || kind == kAudioDeviceTransportTypeAggregate
                || kind == kAudioDeviceTransportTypeAutoAggregate =>
        {
            Transport::Virtual
        }
        Some(kind) if is_continuity_capture(kind) => Transport::Continuity,
        _ => Transport::Other,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_the_internal_microphone_is_switched_off_by_the_lid() {
        let source = |code: &[u8; 4]| Some(u32::from_be_bytes(*code));
        assert!(is_lid_switched_microphone("BuiltInMicrophoneDevice", None));
        assert!(is_lid_switched_microphone(
            "AppleHDAEngineInput:1B,0,1,0:1",
            source(b"imic")
        ));
        assert!(!is_lid_switched_microphone(
            "BuiltInHeadphoneInputDevice",
            None
        ));
        assert!(!is_lid_switched_microphone(
            "AppleHDAEngineInput:1B,0,1,0:1",
            source(b"emic")
        ));
        assert!(!is_lid_switched_microphone(
            "AppleHDAEngineInput:1B,0,1,0:1",
            source(b"line")
        ));
    }

    #[test]
    fn continuity_capture_transports_are_recognised() {
        assert_eq!(CONTINUITY_CAPTURE_WIRED, 0x6363_7764);
        assert_eq!(CONTINUITY_CAPTURE_WIRELESS, 0x6363_776c);
        assert!(is_continuity_capture(u32::from_be_bytes(*b"ccwd")));
        assert!(is_continuity_capture(u32::from_be_bytes(*b"ccwl")));
        assert!(!is_continuity_capture(u32::from_be_bytes(*b"bltn")));
        assert!(!is_continuity_capture(u32::from_be_bytes(*b"virt")));
    }
}
