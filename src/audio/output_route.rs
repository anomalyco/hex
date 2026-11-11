//! Whether the default output plays through speakers or headphones, for
//! Labs › Only on speakers. Anything Hex can't classify counts as speakers,
//! so "While dictating" applies as before.

/// What the default output is, as far as "While dictating" cares.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum OutputRoute {
    /// Built-in or external speakers, or anything Hex can't classify.
    Speakers,
    /// Wired or Bluetooth headphones.
    Headphones,
    /// Hex couldn't read the default output at all; counts as speakers.
    Unknown,
}

/// Whether "While dictating" applies to `route`: headphones are left alone.
pub(crate) fn applies(route: OutputRoute) -> bool {
    route != OutputRoute::Headphones
}

/// CoreAudio's Bluetooth transport types, and its built-in one.
#[cfg(target_os = "macos")]
use objc2_core_audio::{
    kAudioDeviceTransportTypeBluetooth as BLUETOOTH,
    kAudioDeviceTransportTypeBluetoothLE as BLUETOOTH_LE,
    kAudioDeviceTransportTypeBuiltIn as BUILT_IN,
};
/// The same transport types on an OS without CoreAudio, so `classify` stays
/// testable here.
#[cfg(all(test, not(target_os = "macos")))]
const BLUETOOTH: u32 = u32::from_be_bytes(*b"blue");
#[cfg(all(test, not(target_os = "macos")))]
const BLUETOOTH_LE: u32 = u32::from_be_bytes(*b"blea");
#[cfg(all(test, not(target_os = "macos")))]
const BUILT_IN: u32 = u32::from_be_bytes(*b"bltn");
/// A built-in output's headphone-jack data source (`kAudioDevicePropertyDataSource`).
#[cfg(any(target_os = "macos", test))]
const HEADPHONES_JACK: u32 = u32::from_be_bytes(*b"hdpn");

/// The route a CoreAudio output reports from its transport type, selected
/// data source, and UID: Bluetooth is headphones; a built-in device is
/// headphones while its jack data source is selected, or when it is Apple
/// silicon's `BuiltInHeadphoneOutputDevice`. Everything else, including a
/// data source or UID Hex couldn't read, is speakers.
#[cfg(any(target_os = "macos", test))]
fn classify(transport: u32, data_source: Option<u32>, uid: Option<&str>) -> OutputRoute {
    if transport == BLUETOOTH || transport == BLUETOOTH_LE {
        return OutputRoute::Headphones;
    }
    if transport == BUILT_IN && data_source == Some(HEADPHONES_JACK) {
        return OutputRoute::Headphones;
    }
    if uid.is_some_and(|uid| uid.contains("BuiltInHeadphoneOutputDevice")) {
        return OutputRoute::Headphones;
    }
    OutputRoute::Speakers
}

/// The route of the default output now.
#[cfg(target_os = "macos")]
pub(crate) fn default_output_route() -> OutputRoute {
    use std::ffi::c_void;
    use std::mem::size_of;
    use std::ptr::NonNull;

    use objc2_core_audio::{
        AudioObjectGetPropertyData, AudioObjectPropertyAddress, kAudioDevicePropertyDataSource,
        kAudioDevicePropertyDeviceUID, kAudioDevicePropertyTransportType,
        kAudioHardwarePropertyDefaultOutputDevice, kAudioObjectPropertyElementMain,
        kAudioObjectPropertyScopeGlobal, kAudioObjectPropertyScopeOutput, kAudioObjectSystemObject,
    };
    use objc2_core_foundation::{CFRetained, CFString};

    fn read_u32(object: u32, selector: u32, scope: u32) -> Option<u32> {
        let mut address = AudioObjectPropertyAddress {
            mSelector: selector,
            mScope: scope,
            mElement: kAudioObjectPropertyElementMain,
        };
        let mut value = 0_u32;
        let mut size = size_of::<u32>() as u32;
        // SAFETY: All pointers reference initialized, correctly sized stack values.
        let status = unsafe {
            AudioObjectGetPropertyData(
                object,
                NonNull::from(&mut address),
                0,
                std::ptr::null(),
                NonNull::from(&mut size),
                NonNull::from(&mut value).cast::<c_void>(),
            )
        };
        (status == 0 && size as usize == size_of::<u32>()).then_some(value)
    }

    fn read_uid(device: u32) -> Option<String> {
        let mut address = AudioObjectPropertyAddress {
            mSelector: kAudioDevicePropertyDeviceUID,
            mScope: kAudioObjectPropertyScopeGlobal,
            mElement: kAudioObjectPropertyElementMain,
        };
        let mut raw: *const CFString = std::ptr::null();
        let mut size = size_of::<*const CFString>() as u32;
        // SAFETY: `raw` and `size` are a correctly sized stack slot for the
        // property's CFString, which CoreAudio retains for us.
        let status = unsafe {
            AudioObjectGetPropertyData(
                device,
                NonNull::from(&mut address),
                0,
                std::ptr::null(),
                NonNull::from(&mut size),
                NonNull::from(&mut raw).cast::<c_void>(),
            )
        };
        if status != 0 || size as usize != size_of::<*const CFString>() {
            return None;
        }
        // SAFETY: CoreAudio returned a retained CFString in `raw`; taking it
        // once into a CFRetained releases it when this string drops.
        let string = unsafe { CFRetained::from_raw(NonNull::new(raw.cast_mut())?) };
        Some(string.to_string())
    }

    let Some(device) = read_u32(
        kAudioObjectSystemObject as u32,
        kAudioHardwarePropertyDefaultOutputDevice,
        kAudioObjectPropertyScopeGlobal,
    )
    .filter(|device| *device != 0) else {
        return OutputRoute::Unknown;
    };
    let Some(transport) = read_u32(
        device,
        kAudioDevicePropertyTransportType,
        kAudioObjectPropertyScopeGlobal,
    ) else {
        return OutputRoute::Unknown;
    };
    let data_source = read_u32(
        device,
        kAudioDevicePropertyDataSource,
        kAudioObjectPropertyScopeOutput,
    );
    classify(transport, data_source, read_uid(device).as_deref())
}

/// A sound server sink, as `pactl -f json list sinks` reports it.
#[cfg(any(target_os = "linux", test))]
#[derive(serde::Deserialize)]
struct Sink {
    name: String,
    // One non-string value must not hide the rest, so values stay JSON and
    // only strings are read.
    #[serde(default)]
    properties: std::collections::HashMap<String, serde_json::Value>,
    #[serde(default)]
    active_port: Option<String>,
}

/// The route a sink reports: headphones while its active port names
/// headphones or a headset, or its form factor does, and a Bluetooth sink is
/// headphones unless its form factor says speakers. Everything else is
/// speakers, and a sink the sound server doesn't list is unknown.
#[cfg(any(target_os = "linux", test))]
fn classify_sink(default: &str, sinks: &[Sink]) -> OutputRoute {
    let Some(sink) = sinks.iter().find(|sink| sink.name == default) else {
        return OutputRoute::Unknown;
    };
    let property = |key: &str| sink.properties.get(key).and_then(serde_json::Value::as_str);
    let port_is_headphones = sink.active_port.as_deref().is_some_and(|port| {
        let port = port.to_ascii_lowercase();
        port.contains("headphone") || port.contains("headset")
    });
    let form_factor = property("device.form_factor");
    if port_is_headphones
        || matches!(form_factor, Some("headphone" | "headphones" | "headset"))
        || (property("device.api") == Some("bluez5") && form_factor != Some("speaker"))
    {
        OutputRoute::Headphones
    } else {
        OutputRoute::Speakers
    }
}

/// The route of the sound server's default sink now: `pactl` names it and
/// describes every sink. A session with only `wpctl`, an older `pactl`
/// without `get-default-sink`, or JSON Hex can't read is unknown.
#[cfg(target_os = "linux")]
pub(crate) fn default_output_route() -> OutputRoute {
    use crate::linux::input_route;

    let Some(default) = input_route::pactl(&["get-default-sink"]) else {
        return OutputRoute::Unknown;
    };
    let Some(json) = input_route::pactl(&["-f", "json", "list", "sinks"]) else {
        return OutputRoute::Unknown;
    };
    let sinks: Vec<Sink> = match serde_json::from_str(&json) {
        Ok(sinks) => sinks,
        Err(error) => {
            tracing::debug!(%error, "could not read the sound server's sinks");
            return OutputRoute::Unknown;
        }
    };
    classify_sink(default.trim(), &sinks)
}

/// The route a cpal device type reports: headphones, a headset, an earpiece,
/// and a handset are headphones, speakers are speakers. Anything else, or a
/// device or description Hex can't read, is unknown, which applies "While
/// dictating" as before.
#[cfg(any(target_os = "windows", test))]
fn classify_device(device_type: cpal::DeviceType) -> OutputRoute {
    match device_type {
        cpal::DeviceType::Headphones
        | cpal::DeviceType::Headset
        | cpal::DeviceType::Earpiece
        | cpal::DeviceType::Handset => OutputRoute::Headphones,
        cpal::DeviceType::Speaker => OutputRoute::Speakers,
        _ => OutputRoute::Unknown,
    }
}

/// The route of Windows' default output now, from its WASAPI form factor as
/// cpal reports it.
#[cfg(target_os = "windows")]
pub(crate) fn default_output_route() -> OutputRoute {
    use std::panic::{AssertUnwindSafe, catch_unwind};

    use cpal::traits::{DeviceTrait, HostTrait};

    let Some(device) = cpal::default_host().default_output_device() else {
        return OutputRoute::Unknown;
    };
    // cpal's `Device::description` expects `OpenPropertyStore` to succeed, so
    // an endpoint that vanishes between the lookup above and this read panics.
    // Catching it keeps the audio-control worker (the thread that calls this)
    // alive; a description Hex can't read is unknown either way.
    match catch_unwind(AssertUnwindSafe(|| {
        device
            .description()
            .map(|description| description.device_type())
    })) {
        Ok(Ok(device_type)) => classify_device(device_type),
        Ok(Err(error)) => {
            tracing::debug!(%error, "could not describe the default output");
            OutputRoute::Unknown
        }
        Err(_) => {
            tracing::debug!("describing the default output panicked");
            OutputRoute::Unknown
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn source(code: &[u8; 4]) -> Option<u32> {
        Some(u32::from_be_bytes(*code))
    }

    #[test]
    fn bluetooth_outputs_are_headphones() {
        assert_eq!(
            classify(BLUETOOTH, None, Some("AirPods Pro")),
            OutputRoute::Headphones
        );
        assert_eq!(
            classify(BLUETOOTH_LE, None, Some("AirPods Pro")),
            OutputRoute::Headphones
        );
    }

    #[test]
    fn a_built_in_output_on_its_headphone_jack_is_headphones() {
        assert_eq!(
            classify(
                BUILT_IN,
                source(b"hdpn"),
                Some("AppleHDAEngineOutput:1B,0,1,1:0")
            ),
            OutputRoute::Headphones
        );
        assert_eq!(
            classify(
                BUILT_IN,
                source(b"ispk"),
                Some("AppleHDAEngineOutput:1B,0,1,1:0")
            ),
            OutputRoute::Speakers
        );
        // A data source Hex couldn't read counts as speakers.
        assert_eq!(
            classify(BUILT_IN, None, Some("AppleHDAEngineOutput:1B,0,1,1:0")),
            OutputRoute::Speakers
        );
    }

    #[test]
    fn apples_wired_headphone_device_is_headphones() {
        assert_eq!(
            classify(BUILT_IN, None, Some("BuiltInHeadphoneOutputDevice")),
            OutputRoute::Headphones
        );
        assert_eq!(
            classify(
                u32::from_be_bytes(*b"usb "),
                None,
                Some("BuiltInHeadphoneOutputDevice")
            ),
            OutputRoute::Headphones
        );
    }

    #[test]
    fn everything_else_is_speakers() {
        assert_eq!(
            classify(BUILT_IN, source(b"ispk"), Some("BuiltInSpeakerDevice")),
            OutputRoute::Speakers
        );
        assert_eq!(
            classify(
                u32::from_be_bytes(*b"usb "),
                source(b"hdpn"),
                Some("USB Audio")
            ),
            OutputRoute::Speakers
        );
        assert_eq!(classify(BUILT_IN, None, None), OutputRoute::Speakers);
    }

    #[test]
    fn applies_is_false_only_for_headphones() {
        assert!(!applies(OutputRoute::Headphones));
        assert!(applies(OutputRoute::Speakers));
        assert!(applies(OutputRoute::Unknown));
    }

    #[cfg(any(target_os = "linux", test))]
    fn sinks() -> Vec<Sink> {
        serde_json::from_str(
            r#"[
              {"index": 0, "state": "RUNNING", "name": "alsa_output.pci-0000_00_1f.3.analog-stereo",
               "description": "Built-in Audio Analog Stereo", "driver": "PipeWire", "mute": false,
               "properties": {"device.api": "alsa", "device.class": "sound", "device.bus": "pci"},
               "ports": [{"name": "analog-output-speaker", "description": "Speakers", "availability": "availability unknown"},
                         {"name": "analog-output-headphones", "description": "Headphones", "availability": "not available"}],
               "active_port": "analog-output-speaker"},
              {"index": 1, "state": "SUSPENDED", "name": "alsa_output.usb-USB_Audio-00.analog-stereo",
               "description": "USB Audio Analog Stereo", "driver": "PipeWire", "mute": false,
               "properties": {"device.api": "alsa", "device.class": "sound", "device.bus": "usb"},
               "active_port": "analog-output-headphones"},
              {"index": 2, "state": "SUSPENDED", "name": "alsa_output.pci-0000_00_1f.3.hdmi-stereo",
               "description": "Built-in Audio Digital Stereo (HDMI)", "driver": "PipeWire", "mute": false,
               "properties": {"device.api": "alsa", "device.class": "sound", "device.bus": "pci"},
               "active_port": "hdmi-output-0"},
              {"index": 3, "state": "SUSPENDED", "name": "bluez_output.AA_BB_CC_DD_EE_FF.1",
               "description": "Alex Pods", "driver": "PipeWire", "mute": false,
               "properties": {"device.api": "bluez5", "device.form_factor": "headset"},
               "active_port": null},
              {"index": 4, "state": "SUSPENDED", "name": "bluez_output.11_22_33_44_55_66.1",
               "description": "Alex Speaker", "driver": "PipeWire", "mute": false,
               "properties": {"device.api": "bluez5", "device.form_factor": "speaker"},
               "active_port": null}
            ]"#,
        )
        .unwrap()
    }

    #[cfg(any(target_os = "linux", test))]
    #[test]
    fn an_analog_sink_is_headphones_only_on_a_headphone_port() {
        assert_eq!(
            classify_sink("alsa_output.pci-0000_00_1f.3.analog-stereo", &sinks()),
            OutputRoute::Speakers
        );
        assert_eq!(
            classify_sink("alsa_output.usb-USB_Audio-00.analog-stereo", &sinks()),
            OutputRoute::Headphones
        );
        assert_eq!(
            classify_sink("alsa_output.pci-0000_00_1f.3.hdmi-stereo", &sinks()),
            OutputRoute::Speakers
        );
    }

    #[cfg(any(target_os = "linux", test))]
    #[test]
    fn a_headset_port_is_headphones() {
        let sinks: Vec<Sink> = serde_json::from_str(
            r#"[{"index": 0, "state": "SUSPENDED",
                 "name": "alsa_output.pci-0000_00_1f.3-platform-skl_hda_dsp_generic.HiFi__Headphones__sink",
                 "description": "Headphones", "driver": "PipeWire", "mute": false,
                 "properties": {"device.api": "alsa", "device.class": "sound", "device.bus": "pci"},
                 "ports": [{"name": "[Out] Headset", "description": "Headset", "availability": "availability unknown"}],
                 "active_port": "[Out] Headset"}]"#,
        )
        .unwrap();
        assert_eq!(
            classify_sink(
                "alsa_output.pci-0000_00_1f.3-platform-skl_hda_dsp_generic.HiFi__Headphones__sink",
                &sinks
            ),
            OutputRoute::Headphones
        );
    }

    #[cfg(any(target_os = "linux", test))]
    #[test]
    fn bluetooth_sinks_are_headphones_unless_they_say_speakers() {
        assert_eq!(
            classify_sink("bluez_output.AA_BB_CC_DD_EE_FF.1", &sinks()),
            OutputRoute::Headphones
        );
        assert_eq!(
            classify_sink("bluez_output.11_22_33_44_55_66.1", &sinks()),
            OutputRoute::Speakers
        );
    }

    #[cfg(any(target_os = "linux", test))]
    #[test]
    fn a_non_string_property_does_not_hide_the_others() {
        let sinks: Vec<Sink> = serde_json::from_str(
            r#"[{"name": "bluez_output.AA_BB_CC_DD_EE_FF.1",
                 "properties": {"device.api": "bluez5", "device.bus": 7},
                 "active_port": null}]"#,
        )
        .unwrap();
        assert_eq!(
            classify_sink("bluez_output.AA_BB_CC_DD_EE_FF.1", &sinks),
            OutputRoute::Headphones
        );
    }

    #[cfg(any(target_os = "linux", test))]
    #[test]
    fn a_sink_the_sound_server_does_not_name_is_unknown() {
        assert_eq!(
            classify_sink("alsa_output.pci-0000_00_1f.3.analog-stereo.2", &sinks()),
            OutputRoute::Unknown
        );
    }

    #[cfg(any(target_os = "windows", test))]
    #[test]
    fn cpal_headphone_types_are_headphones() {
        use cpal::DeviceType;

        assert_eq!(
            classify_device(DeviceType::Headphones),
            OutputRoute::Headphones
        );
        assert_eq!(
            classify_device(DeviceType::Headset),
            OutputRoute::Headphones
        );
        assert_eq!(
            classify_device(DeviceType::Earpiece),
            OutputRoute::Headphones
        );
        assert_eq!(
            classify_device(DeviceType::Handset),
            OutputRoute::Headphones
        );
        assert_eq!(classify_device(DeviceType::Speaker), OutputRoute::Speakers);
        assert_eq!(classify_device(DeviceType::Dock), OutputRoute::Unknown);
        assert_eq!(classify_device(DeviceType::Unknown), OutputRoute::Unknown);
    }
}
