//! The Hex keyboard's platform-independent logic, shared by iOS and Android. It never reads a
//! clock, a file or the OS: the host passes in times, key geometry and data, and the FFI
//! crate (`hex-keyboard-ffi`) maps the model files.
#![forbid(unsafe_code)]

pub mod decoder;
pub mod model;
pub mod never_suggest;
pub mod swipe;
