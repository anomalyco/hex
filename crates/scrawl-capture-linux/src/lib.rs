//! Linux platform capture adapter for Scrawl (`scrawl-core` traits).
//!
//! Planned backends:
//! - X11: `xcb_shm_get_image` / `XComposite`, reusing `linux_input` grabs and `_NET_CLIENT_LIST_STACKING`.
//! - Wayland: `xdg-desktop-portal` `ScreenCast` + PipeWire, reusing `linux_wayland_input` evdev
//!   button observation and `linux_desktop` `gtk-layer-shell` overlays (noting that Wayland portal
//!   sessions require user consent and do not expose global pointer coordinates or window lists).
//!
//! In M1, only macOS capture is implemented; this crate wires the `scrawl-core` platform traits
//! (`FrameSource`, `InputSource`, `WindowProbe`) and returns a clear unsupported error.

use std::path::Path;

use anyhow::{Result, bail};
use scrawl_core::{
    AppMeta, FrameSource, InputSource, RawWindow, RgbaFrame, ScrawlInputBatch, TargetInfo,
    WindowProbe,
};

pub const UNSUPPORTED_MESSAGE: &str =
    "Linux Scrawl screen capture (X11 XShm / Wayland ScreenCast portal) is not implemented yet";

#[derive(Clone, Copy, Debug, Default)]
pub struct LinuxFrameSource;

impl FrameSource for LinuxFrameSource {
    fn capture_frame(&mut self, _t_ms: u64) -> Result<Option<RgbaFrame>> {
        bail!("{UNSUPPORTED_MESSAGE}")
    }

    fn decode_frames(
        &mut self,
        _video_path: &Path,
        _times_ms: &[u64],
        _video_offset_ms: u64,
        _max_edge: u32,
    ) -> Result<Vec<RgbaFrame>> {
        bail!("{UNSUPPORTED_MESSAGE}")
    }
}

#[derive(Clone, Copy, Debug, Default)]
pub struct LinuxInputSource;

impl InputSource for LinuxInputSource {
    fn poll_events(&mut self) -> ScrawlInputBatch {
        ScrawlInputBatch::default()
    }

    fn finish(&mut self) -> ScrawlInputBatch {
        ScrawlInputBatch::default()
    }
}

#[derive(Clone, Copy, Debug, Default)]
pub struct LinuxWindowProbe;

impl WindowProbe for LinuxWindowProbe {
    fn raw_windows(&mut self) -> Vec<RawWindow> {
        Vec::new()
    }

    fn frontmost_external_pid(&mut self) -> Option<i32> {
        None
    }

    fn app_meta(&mut self, _pid: i32) -> AppMeta {
        AppMeta {
            bundle_id: None,
            regular: false,
        }
    }

    fn cursor_screen(&mut self) -> Option<[f32; 2]> {
        None
    }
}

pub fn start_capture(_target: TargetInfo) -> Result<()> {
    bail!("{UNSUPPORTED_MESSAGE}")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn linux_capture_traits_report_unsupported_cleanly() {
        let mut frames = LinuxFrameSource;
        let err = frames.capture_frame(0).unwrap_err();
        assert!(err.to_string().contains("X11 XShm / Wayland"));

        let err = start_capture(TargetInfo::default()).unwrap_err();
        assert!(err.to_string().contains("not implemented yet"));

        let mut input = LinuxInputSource;
        assert_eq!(input.poll_events(), ScrawlInputBatch::default());
        assert_eq!(input.finish(), ScrawlInputBatch::default());
    }
}
