//! The microphone Hex captures from: the inputs Settings offers, the one
//! stream it keeps open and reopens, and the routes that say when the OS's
//! devices changed.

#[cfg(target_os = "macos")]
pub(crate) mod browser_media;
mod call_input;
#[cfg(target_os = "windows")]
mod default_endpoint;
mod devices;
pub(crate) mod effect_holds;
mod fixture;
#[cfg_attr(any(target_os = "linux", target_os = "windows"), allow(dead_code))]
pub(crate) mod hex_microphone;
pub(crate) mod idle_sleep;
mod input_format;
pub(crate) mod input_level;
pub(crate) mod instant;
mod interruption;
#[cfg(target_os = "macos")]
mod lid;
pub(crate) mod linger;
#[cfg(any(target_os = "windows", test))]
mod mix_format;
mod open;
pub(crate) mod output_ducking;
pub(crate) mod output_route;
mod overloads;
#[cfg(target_os = "macos")]
pub(crate) mod recording_environment;
mod recovering;
mod recovery;
mod routes;
mod stream;
pub(crate) mod system_idle;
mod transport;
#[cfg_attr(any(target_os = "linux", target_os = "windows"), allow(dead_code))]
pub(crate) mod voice_filter;
#[cfg(target_os = "macos")]
mod voice_processing;
#[cfg(target_os = "windows")]
mod wasapi_capture;

use std::time::Duration;

#[cfg(any(target_os = "linux", target_os = "windows"))]
pub use devices::input_device_names;
pub use devices::{InputDevices, input_devices};
/// A dictation press kept across a microphone reopen (the Linux and Windows
/// listeners read it; macOS has no such reopen).
#[cfg(any(target_os = "linux", target_os = "windows"))]
pub(crate) use interruption::capture_idle;
#[cfg(target_os = "macos")]
pub use open::open_dictation_input;
pub use open::open_meeting_input;
#[cfg(target_os = "macos")]
pub use open::open_named_input;
pub(crate) use open::{LID_CLOSED_ERROR, NO_MICROPHONE_ERROR};
pub use recovering::{RecoveringAudioInput, RecoveringAudioInputEvent};
#[cfg(target_os = "macos")]
pub use routes::notify_audio_service_restarted;
pub use routes::{
    audio_service_restart_generation, input_route_generation, output_route_generation,
    set_avoid_bluetooth_microphone,
};
pub(crate) use stream::STREAM_STALL_TIMEOUT;
pub use stream::{AudioInput, AudioInputEvent, Opened, Selection};

pub use instant::{CaptureInstant, duration_for_frames};

/// The outputs the tone and playback paths try on Windows, best first: the
/// default endpoint's listed device, then every other listed output. Never
/// cpal's default handle, whose monitor leaves thread-pool threads STA
/// (`default_endpoint`).
#[cfg(target_os = "windows")]
pub(crate) fn default_output_candidates() -> Vec<cpal::Device> {
    use cpal::traits::{DeviceTrait, HostTrait};

    let host = cpal::default_host();
    let mut devices = Vec::new();
    if let Some(device) = default_endpoint::default_output(&host) {
        devices.push(device);
    }
    let Ok(listed) = host.output_devices() else {
        return devices;
    };
    for device in listed {
        let id = device.id().ok().map(|id| id.id().to_owned());
        let duplicate = id.as_deref().is_some_and(|id| {
            devices
                .iter()
                .any(|chosen| chosen.id().is_ok_and(|chosen| chosen.id() == id))
        });
        if !duplicate {
            devices.push(device);
        }
    }
    devices
}

/// Opens the system default output as a rodio sink. Windows tries the
/// endpoint's listed device and then every other listed output, never cpal's
/// default handle, whose monitor leaves thread-pool threads STA; every other
/// OS keeps rodio's default handle and fallback.
pub(crate) fn open_default_output_sink()
-> std::result::Result<rodio::MixerDeviceSink, rodio::DeviceSinkError> {
    #[cfg(target_os = "windows")]
    {
        let mut last_error = None;
        for device in default_output_candidates() {
            match rodio::DeviceSinkBuilder::from_device(device)
                .and_then(|builder| builder.open_stream())
            {
                Ok(sink) => return Ok(sink),
                Err(error) => last_error = Some(error),
            }
        }
        Err(last_error.unwrap_or(rodio::DeviceSinkError::NoDevice))
    }
    #[cfg(not(target_os = "windows"))]
    {
        rodio::DeviceSinkBuilder::open_default_sink()
    }
}

/// Whether `HEX_AUDIO_FIXTURE` names a WAV file that stands in for every
/// microphone (benchmarks and tests; see `audio/fixture.rs`).
pub fn uses_fixture() -> bool {
    fixture::path().is_some()
}

/// How long ago a microphone stream last reported an overload it continued
/// through, for diagnosing captures.
pub fn since_last_stream_glitch() -> Option<Duration> {
    overloads::since_last()
}
