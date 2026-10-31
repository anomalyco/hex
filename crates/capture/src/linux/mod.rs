//! Linux platform pieces: the screen-cast portal (`portal`), its PipeWire stream (`pipewire`),
//! VA-API hardware encoding (`vaapi`), native X11 capture (`x11`), and movies through
//! `ffmpeg` (`media`). The portal speaks
//! D-Bus through `zbus`; PipeWire and VA-API are loaded when a capture starts, so the build has
//! no native dependency on either.

pub mod media;
pub mod pipewire;
pub mod portal;
pub mod vaapi;
pub mod x11;

pub use portal::set_token_path;
