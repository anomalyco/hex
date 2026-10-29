//! Linux platform pieces: the screen-cast portal (`portal`), its PipeWire stream (`pipewire`),
//! VA-API hardware encoding (`vaapi`), and movies through `ffmpeg` (`media`). The portal speaks
//! D-Bus through `zbus`; PipeWire and VA-API are loaded when a capture starts, so the build has
//! no native dependency on either.

pub mod media;
pub mod pipewire;
pub mod portal;
pub mod vaapi;

pub use portal::set_token_path;
