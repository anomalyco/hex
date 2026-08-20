//! Cross-platform application foundation: paths, settings-adjacent state,
//! audio capture, the dictation pipeline, and durable user data.

pub mod app_paths;
#[cfg_attr(any(target_os = "linux", target_os = "windows"), allow(dead_code))]
pub mod audio;
#[cfg(any(target_os = "macos", target_os = "linux", target_os = "windows"))]
pub mod command_context;
#[cfg(any(target_os = "macos", target_os = "linux", target_os = "windows"))]
pub mod command_grammar;
#[cfg(any(target_os = "macos", target_os = "linux", target_os = "windows"))]
pub mod commands_engine;
#[cfg_attr(any(target_os = "linux", target_os = "windows"), allow(dead_code))]
pub mod dictation;
#[cfg(any(target_os = "macos", target_os = "linux", target_os = "windows"))]
pub mod dictation_processing;
#[cfg_attr(any(target_os = "linux", target_os = "windows"), allow(dead_code))]
pub mod events;
#[cfg(any(target_os = "macos", target_os = "windows"))]
pub mod feedback;
#[cfg_attr(any(target_os = "linux", target_os = "windows"), allow(dead_code))]
pub mod history;
pub mod instance;
#[cfg(any(target_os = "macos", target_os = "linux", target_os = "windows"))]
pub mod keys;
#[cfg(any(target_os = "macos", target_os = "linux", target_os = "windows"))]
pub mod local_api;
#[cfg(any(target_os = "macos", target_os = "linux", target_os = "windows"))]
pub mod opencode;
#[cfg(any(target_os = "macos", target_os = "linux", target_os = "windows"))]
pub mod personal_commands;
#[cfg(any(target_os = "linux", target_os = "windows"))]
pub mod self_update;
#[cfg(any(target_os = "macos", target_os = "linux", target_os = "windows"))]
pub mod spoken_text;
#[cfg(any(target_os = "macos", target_os = "linux", target_os = "windows"))]
pub mod text_replacements;
