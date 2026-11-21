//! The typing speed Dictation stats compares against, in words a minute:
//! time saved is what typing the dictated words at this speed would have
//! taken, less the recorded audio. Saved as `typing_wpm`, and kept inside
//! its range as it loads.

/// How many words a person types a minute by default.
pub const DEFAULT_TYPING_WPM: u32 = 40;
/// The fastest and slowest typing speed Dictation stats accepts.
pub const MIN_TYPING_WPM: u32 = 10;
pub const MAX_TYPING_WPM: u32 = 200;

/// A saved typing speed within [`MIN_TYPING_WPM`] and [`MAX_TYPING_WPM`].
pub const fn clamp(wpm: u32) -> u32 {
    if wpm < MIN_TYPING_WPM {
        MIN_TYPING_WPM
    } else if wpm > MAX_TYPING_WPM {
        MAX_TYPING_WPM
    } else {
        wpm
    }
}

pub(super) fn default_wpm() -> u32 {
    DEFAULT_TYPING_WPM
}

/// Clamps a saved typing speed as it loads, so every decode agrees with
/// the window and `hex settings`.
pub(super) fn deserialize<'de, D>(deserializer: D) -> std::result::Result<u32, D::Error>
where
    D: serde::Deserializer<'de>,
{
    use serde::Deserialize;
    u32::deserialize(deserializer).map(clamp)
}
