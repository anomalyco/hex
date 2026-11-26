//! Linux platform pieces: the screen-cast portal (`portal`), its PipeWire stream (`pipewire`),
//! reading tiled screen buffers back through EGL (`egl`),
//! VA-API hardware encoding (`vaapi`), native X11 capture (`x11`), and movies through
//! `ffmpeg` (`media`). The portal speaks
//! D-Bus through `zbus`; PipeWire, VA-API and EGL are loaded when a capture starts, so the
//! build has no native dependency on any of them.

pub mod egl;
pub mod media;
pub mod pipewire;
pub mod portal;
pub mod vaapi;
pub mod x11;
