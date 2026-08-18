//! The shared desktop shell layer used by every OS shell: the host trait,
//! shared chrome and widgets, the transcription picker, and the text input.

pub mod activity;
pub mod host;
#[cfg(any(target_os = "linux", target_os = "windows"))]
pub mod i18n;
#[cfg(any(target_os = "linux", target_os = "windows"))]
pub mod model_catalog;
pub mod transcription_picker;
pub mod ui;

#[cfg(any(target_os = "macos", target_os = "windows"))]
pub mod text_input;
