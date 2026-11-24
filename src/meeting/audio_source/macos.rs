//! macOS's meeting audio sources: the Core Audio processes playing output
//! and the output devices a meeting may record.

use super::SourceEntry;

/// The applications playing output now, read from Core Audio's process
/// metadata (never audio). The id is the app's canonical bundle id, which a
/// meeting's tap matches its processes by; a helper process is named as its
/// app, once, and Hex itself is never listed.
pub(super) fn playing_apps() -> Vec<SourceEntry> {
    crate::microphone_activity::playing_output_applications()
        .into_iter()
        .map(|(id, name)| SourceEntry { id, name })
        .collect()
}

/// The output devices Core Audio lists, the default first. The id is the
/// device's Core Audio UID (`kAudioDevicePropertyDeviceUID`), which a
/// meeting's tap names, and the name is the device's own name
/// (`kAudioObjectPropertyName`).
pub(super) fn output_devices() -> Vec<SourceEntry> {
    crate::audio::output_route::meeting_output_devices()
        .into_iter()
        .map(|(id, name)| SourceEntry { id, name })
        .collect()
}
