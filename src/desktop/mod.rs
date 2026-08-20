//! The shared desktop shell layer used by every OS shell: the host trait,
//! shared chrome and widgets, the transcription picker, and the text input.

pub mod activity;
#[cfg(any(target_os = "linux", target_os = "windows"))]
pub mod activity_pane;
#[cfg(any(target_os = "macos", target_os = "linux", target_os = "windows"))]
pub mod history_pane;
pub mod host;
#[cfg(any(target_os = "linux", target_os = "windows"))]
pub mod hud_lab;
#[cfg(any(target_os = "linux", target_os = "windows"))]
pub mod i18n;
#[cfg(any(target_os = "macos", target_os = "linux", target_os = "windows"))]
pub mod indicator_model;
#[cfg(any(target_os = "linux", target_os = "windows"))]
pub mod indicator_shader;
#[cfg(any(target_os = "macos", target_os = "windows"))]
pub mod mode_basics;
#[cfg(any(target_os = "macos", target_os = "windows"))]
pub mod mode_list;
#[cfg(any(target_os = "macos", target_os = "windows"))]
pub mod mode_processing;
#[cfg(any(target_os = "macos", target_os = "linux", target_os = "windows"))]
pub mod mode_target;
#[cfg(any(target_os = "macos", target_os = "windows"))]
pub mod mode_transformations;
#[cfg(any(target_os = "linux", target_os = "windows"))]
pub mod model_catalog;
#[cfg(any(target_os = "linux", target_os = "windows"))]
pub mod onboarding;
#[cfg(any(target_os = "macos", target_os = "linux", target_os = "windows"))]
pub mod replacement_editor;
pub mod shell;
pub mod transcription_picker;
#[cfg_attr(target_os = "linux", allow(dead_code))]
pub mod ui;
#[cfg(any(target_os = "macos", target_os = "windows"))]
pub mod voice_action_pane;

#[cfg(any(target_os = "macos", target_os = "linux", target_os = "windows"))]
pub mod text_input;
