//! The microphone Hex captures from: the inputs Settings offers, the one
//! stream it keeps open and reopens, and the routes that say when the OS's
//! devices changed.

#[cfg(target_os = "macos")]
pub(crate) mod browser_media;
mod call_input;
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
