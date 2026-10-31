//! Linux capture: the screen-cast portal picks the screen or window and hands over a PipeWire
//! stream, VA-API scales and encodes it on the GPU (`crate::linux`), and `ffmpeg` muxes the
//! movie. Wayland gives an app no global pointer, key state, window list, or synthetic input,
//! so those answer nothing here: the host (Hex) owns shortcuts and pasting, and the portal's
//! own picker stands in for the window list. X11 sessions use the same portal.
//!
//! Frames are pixels; a recording is never upscaled (`MAX_CAPTURE_SCALE`).

use std::sync::Arc;
use std::sync::atomic::Ordering;
use std::time::{Duration, Instant};

use anyhow::{Context, Result, bail};

use super::{
    CANDIDATE_EDGE, CARBON_CMD_KEY, CARBON_CONTROL_KEY, CARBON_OPTION_KEY, CARBON_SHIFT_KEY,
    Sampler, SharedCaptureTelemetry, ShortcutConfig, ShortcutKind, shortcut_label,
};
use crate::linux::media::{FPS, Writer};
use crate::linux::pipewire::{self, Frame, PixelFormat, Stream, VideoFormat};
use crate::linux::portal::{self, Source};
use crate::linux::vaapi::Encoder;
use crate::log::scrawl_log;
use crate::model::{CaptureMode, RgbaFrame, TargetFrame, TargetInfo};

pub const MAX_CAPTURE_SCALE: f64 = 1.0;

pub(super) const MOUSE_SOURCE: &str = "no pointer (Linux)";
pub(super) const KEY_SOURCE: &str = "no key tap (Linux)";
pub(super) const NO_KEY_SOURCE: &str = "no key tap (Linux)";

pub const KEY_S: u16 = 0x53;
pub const KEY_V: u16 = 0x56;

/// How long a started stream may take to agree on a format.
const FORMAT_TIMEOUT: Duration = Duration::from_secs(5);
/// Long edges of the movie: a screen, and a window.
const SCREEN_EDGE: u32 = 2560;
const WINDOW_EDGE: u32 = 1920;

pub fn main_display_id() -> u32 {
    0
}

pub fn key_code_for_char(ch: char) -> Option<u16> {
    ch.is_ascii_alphanumeric()
        .then(|| ch.to_ascii_uppercase() as u16)
}

pub fn layout_s_keycode() -> u16 {
    KEY_S
}

pub fn layout_v_keycode() -> u16 {
    KEY_V
}

/// The portal asks when a capture starts; there is nothing to grant ahead of time.
pub fn screen_capture_granted() -> bool {
    true
}

pub fn screen_capture_usable(_probe: bool) -> bool {
    true
}

pub fn screen_capture_probe() -> Result<(u32, u32), String> {
    Err("the screen is picked through the desktop's portal when a capture starts".into())
}

pub fn request_screen_capture() {}

pub fn accessibility_granted() -> bool {
    true
}

pub fn modifier_flags() -> u64 {
    0
}

pub fn shortcut_from_key_and_mods(
    key: &str,
    cmd: bool,
    ctrl: bool,
    alt: bool,
    shift: bool,
) -> Option<ShortcutConfig> {
    let ch = key.chars().next().filter(|_| key.chars().count() == 1)?;
    let key_code = key_code_for_char(ch)?;
    if !cmd && !ctrl && !alt {
        return None;
    }
    let mut carbon_modifiers = 0_u32;
    for (held, bit) in [
        (ctrl, CARBON_CONTROL_KEY),
        (alt, CARBON_OPTION_KEY),
        (shift, CARBON_SHIFT_KEY),
        (cmd, CARBON_CMD_KEY),
    ] {
        if held {
            carbon_modifiers |= bit;
        }
    }
    Some(ShortcutConfig {
        key_code: u32::from(key_code),
        carbon_modifiers,
        label: shortcut_label(carbon_modifiers, &ch.to_ascii_uppercase().to_string()),
    })
}

pub fn record_external_frontmost_app() {}

pub fn ensure_external_app_active() {}

pub fn frontmost_external_pid() -> Option<i32> {
    None
}

pub fn frontmost_app_info() -> Option<(i32, String, Option<String>)> {
    None
}

pub fn current_mouse_screen() -> Option<[f32; 2]> {
    None
}

pub fn discover_target(window_query: Option<&str>) -> TargetInfo {
    discover_target_at(window_query, None)
}

/// A screen; which one is the portal's question. The frame is a placeholder until the stream
/// reports its size.
pub fn discover_target_at(_window_query: Option<&str>, _point: Option<[f64; 2]>) -> TargetInfo {
    TargetInfo {
        mode: CaptureMode::Display,
        app: None,
        bundle_id: None,
        window_title: None,
        window_id: None,
        display_id: 0,
        frame: TargetFrame {
            x: 0.0,
            y: 0.0,
            width: 1920.0,
            height: 1080.0,
            scale: 1.0,
        },
    }
}

/// The portal picks windows itself.
pub fn list_selectable_windows() -> Vec<TargetInfo> {
    Vec::new()
}

pub(super) fn raw_windows_now() -> Vec<crate::windows::RawWindow> {
    Vec::new()
}

pub(super) fn is_helper_process(_pid: i32) -> bool {
    false
}

pub fn wait_for_modifiers_released(_max_wait_ms: u64) {}

/// Hex pastes through its own Linux paste path.
pub fn post_synthetic_cmd_v() {}

pub fn post_synthetic_click(_screen_x: f64, _screen_y: f64) {}

pub fn install_global_hotkeys(_telemetry: &Arc<SharedCaptureTelemetry>) {}

pub fn set_shortcuts(
    telemetry: &SharedCaptureTelemetry,
    record: ShortcutConfig,
    paste: ShortcutConfig,
) -> Result<(ShortcutConfig, ShortcutConfig), ShortcutKind> {
    *telemetry
        .shortcut
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner) = record.clone();
    *telemetry
        .paste_shortcut
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner) = paste.clone();
    Ok((record, paste))
}

pub fn pause_shortcuts(telemetry: &SharedCaptureTelemetry, paused: bool) {
    telemetry.shortcuts_paused.store(paused, Ordering::Release);
}

pub fn update_global_shortcut(
    telemetry: &SharedCaptureTelemetry,
    config: ShortcutConfig,
) -> Option<ShortcutConfig> {
    let (_, paste) = telemetry.shortcuts();
    set_shortcuts(telemetry, config, paste)
        .ok()
        .map(|(record, _)| record)
}

pub fn start_global_input_tap(_telemetry: Arc<SharedCaptureTelemetry>) {}

// MARK: Recording

/// `(width, height)` with the long edge at most `edge`, both multiples of `step`.
fn fit(width: u32, height: u32, edge: u32, step: u32) -> (u32, u32) {
    let scale = (f64::from(edge) / f64::from(width.max(height).max(1))).min(1.0);
    let round = |v: u32| (((f64::from(v) * scale) as u32) / step * step).max(step);
    (round(width), round(height))
}

/// How the recorder turns stream frames into the movie.
enum Encoding {
    /// On the GPU: imported, scaled and encoded there.
    Hardware {
        encoder: Box<Encoder>,
        movie: Writer,
    },
    /// On the CPU, through x264: the frame copied out of the stream.
    Software {
        pixels: Vec<u8>,
        movie: Option<Writer>,
    },
}

/// The recording thread: see `crate::capture::Sampler`. Writes its movie before it returns.
/// The recorded screen's size in pixels while a screen recording runs.
static RECORDED: std::sync::Mutex<Option<(u32, u32)>> = std::sync::Mutex::new(None);

/// The pixel size of the screen being recorded, once its stream started:
/// a host drawing over the screen in points divides it by the scale.
pub fn recorded_screen() -> Option<(u32, u32)> {
    *RECORDED
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
}

pub(super) fn run_sampler(sampler: Sampler) -> Option<super::Movie> {
    let recorded = record(&sampler);
    *RECORDED
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner) = None;
    match recorded {
        Ok(movie) => movie,
        Err(error) => {
            scrawl_log(format!("Screen capture failed: {error:#}"));
            // The host ends the capture: nothing more of the screen will come.
            if !sampler.stopping.load(Ordering::Acquire) {
                *sampler
                    .failure
                    .lock()
                    .unwrap_or_else(std::sync::PoisonError::into_inner) =
                    Some(format!("{error:#}"));
            }
            None
        }
    }
}

fn record(sampler: &Sampler) -> Result<Option<super::Movie>> {
    let source = match sampler.target.mode {
        CaptureMode::Window => Source::Window,
        _ => Source::Screen,
    };
    pipewire::available()?;
    let mut cast = portal::start(source, &sampler.stopping)?;
    let remote = cast
        .remote
        .take()
        .context("the portal opened no PipeWire remote")?;
    let mut stream = Stream::connect(remote, cast.node)?;
    let asked = Instant::now();
    let format = loop {
        if let Some(format) = stream.format() {
            break format;
        }
        if let Some(why) = stream.ended() {
            bail!("{why}");
        }
        if sampler.stopping.load(Ordering::Acquire) {
            return Ok(None);
        }
        if asked.elapsed() > FORMAT_TIMEOUT {
            bail!("the screen stream never started");
        }
        std::thread::sleep(Duration::from_millis(10));
    };
    if source == Source::Screen {
        *RECORDED
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner) =
            Some((format.width, format.height));
    }
    let edge = if source == Source::Window {
        WINDOW_EDGE
    } else {
        SCREEN_EDGE
    };
    let out = fit(format.width, format.height, edge, 16);
    // The video engine shrinks by at most 4× in one pass (radeonsi crashes past it), so the
    // candidates come from the movie's frame at no less than a quarter of its size.
    let small_edge = (CANDIDATE_EDGE as u32).max(out.0.max(out.1).div_ceil(4));
    let small = fit(out.0, out.1, small_edge, 2);
    scrawl_log(format!(
        "Recording {}x{} of {}x{} ({:?}, {})",
        out.0,
        out.1,
        format.width,
        format.height,
        source,
        if format.dmabuf {
            "DMA-BUF"
        } else {
            "shared memory"
        }
    ));
    let path = sampler.video_output_path.clone();
    let mut encoding = start_encoding(&format, out, small, path.as_deref());
    crate::log::start_metric("sck_ready_ms", sampler.started_at);

    let tick = Duration::from_secs(1) / FPS;
    let mut first: Option<(Instant, u64)> = None;
    let mut written = 0_u64;
    let mut have_frame = false;
    let mut last_sample: Option<Instant> = None;
    let mut failed = false;
    while !sampler.stopping.load(Ordering::Acquire) {
        if let Some(why) = stream.ended() {
            scrawl_log(format!("The screen stream stopped: {why}"));
            break;
        }
        if let Some(frame) = stream.newest() {
            match load(&mut encoding, &frame, &format) {
                Ok(()) => have_frame = true,
                Err(error) if !failed => {
                    scrawl_log(format!("Recording frame failed: {error:#}"));
                    failed = true;
                }
                Err(_) => {}
            }
        }
        if have_frame && !failed {
            let (start, _) = *first.get_or_insert_with(|| {
                (
                    Instant::now(),
                    sampler.started_at.elapsed().as_millis() as u64,
                )
            });
            let due = (start.elapsed().as_secs_f64() * f64::from(FPS)) as u64 + 1;
            while written < due {
                if let Err(error) = encode(&mut encoding) {
                    scrawl_log(format!("Encoding failed: {error:#}"));
                    failed = true;
                    break;
                }
                written += 1;
            }
        }
        let urgent = sampler.urgent.swap(false, Ordering::AcqRel);
        let interval = sampler
            .frames
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .interval();
        if have_frame && (urgent || last_sample.is_none_or(|at| at.elapsed() >= interval)) {
            last_sample = Some(Instant::now());
            let t_ms = sampler.started_at.elapsed().as_millis() as u64;
            if let Some(frame) = candidate(&encoding, &format, small, t_ms) {
                sampler
                    .frames
                    .lock()
                    .unwrap_or_else(std::sync::PoisonError::into_inner)
                    .push(frame, urgent);
            }
        }
        std::thread::park_timeout(tick / 2);
    }
    drop(stream);
    drop(cast);
    let movie = match encoding {
        Encoding::Hardware { encoder, movie } => {
            drop(encoder);
            Some(movie)
        }
        Encoding::Software { movie, .. } => movie,
    };
    let (Some(movie), Some(path), Some((_, offset_ms))) = (movie, path, first) else {
        return Ok(None);
    };
    movie.finish()?;
    Ok(Some(super::Movie::written(path, offset_ms)))
}

fn start_encoding(
    format: &VideoFormat,
    out: (u32, u32),
    small: (u32, u32),
    path: Option<&std::path::Path>,
) -> Encoding {
    if format.dmabuf
        && let Some(path) = path
    {
        let hardware = Encoder::new((format.width, format.height), format.pixel, out, small)
            .and_then(|encoder| Ok((encoder, Writer::h264(path)?)));
        match hardware {
            Ok((encoder, movie)) => {
                return Encoding::Hardware {
                    encoder: Box::new(encoder),
                    movie,
                };
            }
            Err(error) => scrawl_log(format!("Hardware encoding unavailable: {error:#}")),
        }
    }
    let pixel = match format.pixel {
        PixelFormat::Bgrx => "bgr0",
        PixelFormat::Rgbx => "rgb0",
        PixelFormat::Bgra => "bgra",
        PixelFormat::Rgba => "rgba",
    };
    let movie = path.and_then(|path| {
        Writer::x264(path, pixel, (format.width, format.height), out)
            .inspect_err(|error| scrawl_log(format!("Software encoding unavailable: {error:#}")))
            .ok()
    });
    Encoding::Software {
        pixels: Vec::new(),
        movie,
    }
}

fn load(encoding: &mut Encoding, frame: &Frame, format: &VideoFormat) -> Result<()> {
    match encoding {
        Encoding::Hardware { encoder, .. } => encoder.load(frame),
        Encoding::Software { pixels, .. } => {
            let row = format.width as usize * 4;
            pixels.resize(row * format.height as usize, 0);
            if frame.dmabuf {
                pipewire::sync_dmabuf(frame.fd, true);
            }
            for y in 0..format.height as usize {
                let src = unsafe {
                    std::slice::from_raw_parts(frame.rows().add(y * frame.stride as usize), row)
                };
                pixels[y * row..(y + 1) * row].copy_from_slice(src);
            }
            if frame.dmabuf {
                pipewire::sync_dmabuf(frame.fd, false);
            }
            Ok(())
        }
    }
}

fn encode(encoding: &mut Encoding) -> Result<()> {
    match encoding {
        Encoding::Hardware { encoder, movie } => encoder.encode(movie),
        Encoding::Software { pixels, movie } => match movie {
            Some(movie) => Ok(std::io::Write::write_all(movie, pixels)?),
            None => Ok(()),
        },
    }
}

fn candidate(
    encoding: &Encoding,
    format: &VideoFormat,
    (width, height): (u32, u32),
    t_ms: u64,
) -> Option<RgbaFrame> {
    match encoding {
        Encoding::Hardware { encoder, .. } => {
            let (w, h, rgba) = encoder.small_rgba()?;
            Some(RgbaFrame::new(t_ms, w, h, rgba))
        }
        Encoding::Software { pixels, .. } if !pixels.is_empty() => {
            let mut rgba = Vec::with_capacity(width as usize * height as usize * 4);
            for y in 0..height {
                let sy = (u64::from(y) * u64::from(format.height) / u64::from(height)) as usize;
                for x in 0..width {
                    let sx = (u64::from(x) * u64::from(format.width) / u64::from(width)) as usize;
                    let at = (sy * format.width as usize + sx) * 4;
                    let p = &pixels[at..at + 4];
                    if format.pixel.bgr() {
                        rgba.extend_from_slice(&[p[2], p[1], p[0], 255]);
                    } else {
                        rgba.extend_from_slice(&[p[0], p[1], p[2], 255]);
                    }
                }
            }
            Some(RgbaFrame::new(t_ms, width, height, rgba))
        }
        Encoding::Software { .. } => None,
    }
}
