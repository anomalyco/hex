//! Speech-to-text: the model catalog, the local GGUF runtime, and the
//! platform speech engines.

#[cfg(target_os = "macos")]
pub mod apple_speech;
#[cfg(any(target_os = "linux", target_os = "windows"))]
#[cfg_attr(any(target_os = "linux", target_os = "windows"), allow(dead_code))]
pub mod local_transcriber;
#[cfg(any(target_os = "macos", target_os = "linux"))]
#[cfg_attr(target_os = "linux", allow(dead_code))]
pub mod moonshine;
#[cfg(all(target_os = "macos", debug_assertions))]
pub mod moonshine_lab;
#[cfg(target_os = "macos")]
pub mod parakeet;
#[cfg(target_os = "macos")]
pub mod recognition;
#[cfg(target_os = "macos")]
pub mod transcription;
#[cfg(target_os = "macos")]
pub mod transcription_benchmark;
#[cfg_attr(any(target_os = "linux", target_os = "windows"), allow(dead_code))]
pub mod transcription_models;
#[cfg(target_os = "macos")]
pub mod transcription_service;
