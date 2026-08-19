//! The shared desktop shell layer used by every OS shell: the host trait,
//! shared chrome and widgets, the transcription picker, and the text input.

pub mod activity;
#[cfg(any(target_os = "linux", target_os = "windows"))]
pub mod activity_pane;
pub mod host;
#[cfg(any(target_os = "linux", target_os = "windows"))]
pub mod hud_lab;
#[cfg(any(target_os = "linux", target_os = "windows"))]
pub mod i18n;
#[cfg(any(target_os = "linux", target_os = "windows"))]
pub mod indicator_model;
#[cfg(any(target_os = "linux", target_os = "windows"))]
pub mod indicator_shader;
#[cfg(any(target_os = "linux", target_os = "windows"))]
pub mod model_catalog;
#[cfg(any(target_os = "linux", target_os = "windows"))]
pub mod onboarding;
pub mod transcription_picker;
#[cfg_attr(target_os = "linux", allow(dead_code))]
pub mod ui;

#[cfg(any(target_os = "macos", target_os = "windows"))]
pub mod text_input;
