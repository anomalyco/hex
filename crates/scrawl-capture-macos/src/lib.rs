//! macOS platform capture adapter for Scrawl (`scrawl-core` traits).
//!
//! Owns `ScreenCaptureKit` display/window/region capture (`SCRecordingOutput` H.264 MP4 +
//! `SCScreenshotManager` candidate frames), `CGWindowListCopyWindowInfo` window stacking and
//! `NSScreen` Dock exclusion via [`MacosWindowProbe`], and session-scoped mouse/click telemetry via
//! [`MacosInputSource`]. Global shortcut matching (`⌥⇧S`) and microphone capture/transcription
//! stay in Hex's existing `suppression`, `dictation_audio`, and `parakeet`/`transcription` owners.

use std::path::PathBuf;

use scrawl_core::{RgbaFrame, TargetInfo};

/// Captured screen frames and optional MP4 recording produced by a [`ScreenCaptureSession`].
#[derive(Clone, Debug)]
pub struct ScreenRecording {
    pub frames: Vec<RgbaFrame>,
    pub video: Option<PathBuf>,
    pub duration_ms: u64,
    /// When `recording.mp4` started relative to the capture session's clock (ms).
    pub video_offset_ms: u64,
}

/// Which display to record: the one under `point`, else the one holding the focused window's
/// centre, else the first. Frames and points are global, top-left origin.
pub fn pick_display(
    displays: &[[f64; 4]],
    point: Option<[f64; 2]>,
    focused: Option<[f64; 4]>,
) -> usize {
    let inside = |p: [f64; 2]| {
        displays
            .iter()
            .position(|&[x, y, w, h]| p[0] >= x && p[0] < x + w && p[1] >= y && p[1] < y + h)
    };
    point
        .and_then(inside)
        .or_else(|| focused.and_then(|[x, y, w, h]| inside([x + w / 2.0, y + h / 2.0])))
        .unwrap_or(0)
}

/// The capture size for `target` with its long edge at most `long_edge` px (and at most 2× its
/// size in points).
pub fn capture_size(target: &TargetInfo, long_edge: f64) -> (u32, u32) {
    let max_dim = target.frame.width.max(target.frame.height).max(1.0);
    let scale = (long_edge / max_dim).min(2.0);
    (
        ((target.frame.width * scale).round() as u32).max(48) & !1,
        ((target.frame.height * scale).round() as u32).max(48) & !1,
    )
}

pub fn selectable_window_at(windows: &[TargetInfo], x: f64, y: f64) -> Option<&TargetInfo> {
    windows.iter().find(|window| {
        let f = window.frame;
        x >= f.x && x <= f.x + f.width && y >= f.y && y <= f.y + f.height
    })
}

pub fn clean_label(input: &str) -> String {
    let squashed = input.split_whitespace().collect::<Vec<_>>().join(" ");
    let trimmed = squashed.trim();
    if trimmed.chars().count() > 44 {
        let prefix: String = trimmed.chars().take(43).collect();
        format!("{}…", prefix.trim_end())
    } else {
        trimmed.to_string()
    }
}

#[cfg(target_os = "macos")]
mod imp {
    use std::ffi::c_void;
    use std::path::{Path, PathBuf};
    use std::ptr;
    use std::sync::atomic::{AtomicBool, AtomicI32, AtomicU64, Ordering};
    use std::sync::{Arc, Mutex};
    use std::thread::{self, JoinHandle};
    use std::time::{Duration, Instant};

    use anyhow::{Result, bail};
    use core_foundation::array::{CFArray, CFArrayRef};
    use core_foundation::base::{CFType, TCFType};
    use core_foundation::dictionary::{CFDictionary, CFDictionaryRef};
    use core_foundation::number::CFNumber;
    use core_foundation::string::CFString;
    use screencapturekit::prelude::*;
    use screencapturekit::recording_output::{
        SCRecordingOutput, SCRecordingOutputCodec, SCRecordingOutputConfiguration,
        SCRecordingOutputFileType,
    };
    use screencapturekit::screenshot_manager::{CGImageExt, SCScreenshotManager};

    use scrawl_core::render::render_synthetic_demo_frame;
    use scrawl_core::windows::WindowLogger;
    use scrawl_core::{
        AppMeta, CANDIDATE_BUDGET_BYTES, CANDIDATE_EDGE, CANDIDATE_EDGE_NO_VIDEO, CandidateBuffer,
        CaptureMode, FRAME_SAMPLE_INTERVAL, FrameSource, InputSource, InteractionEvent, RawWindow,
        RgbaFrame, ScrawlInputBatch, Stroke, TargetFrame, TargetInfo, WindowProbe,
    };

    use super::{ScreenRecording, capture_size, clean_label, pick_display, selectable_window_at};

    type EventRef = *mut c_void;

    #[repr(C)]
    #[derive(Clone, Copy, Debug)]
    pub struct CGPoint {
        pub x: f64,
        pub y: f64,
    }

    #[link(name = "CoreGraphics", kind = "framework")]
    unsafe extern "C" {
        fn CGEventCreate(source: *const c_void) -> EventRef;
        fn CGEventGetLocation(event: EventRef) -> CGPoint;
        fn CGEventSourceFlagsState(state_id: u32) -> u64;
        fn CGEventSourceButtonState(state_id: u32, button: u32) -> bool;
        fn CGMainDisplayID() -> u32;
        fn CGDisplayPixelsWide(display: u32) -> usize;
        fn CGDisplayPixelsHigh(display: u32) -> usize;
        fn CGPreflightScreenCaptureAccess() -> bool;
        fn CGRequestScreenCaptureAccess() -> bool;
        fn CGWindowListCopyWindowInfo(option: u32, relative_to_window: u32) -> CFArrayRef;
    }

    #[link(name = "CoreFoundation", kind = "framework")]
    unsafe extern "C" {
        fn CFRelease(cf: *const c_void);
    }

    const FLAG_SHIFT: u64 = 1 << 17;
    const KW_OPTION_ON_SCREEN_ONLY: u32 = 1 << 0;
    const KW_OPTION_EXCLUDE_DESKTOP_ELEMENTS: u32 = 1 << 4;

    static LAST_EXTERNAL_APP_PID: AtomicI32 = AtomicI32::new(0);

    pub fn screen_capture_granted() -> bool {
        unsafe { CGPreflightScreenCaptureAccess() }
    }

    pub fn request_screen_capture_access() -> bool {
        unsafe { CGRequestScreenCaptureAccess() }
    }

    pub fn current_mouse_location() -> Option<CGPoint> {
        let ev = unsafe { CGEventCreate(ptr::null()) };
        if ev.is_null() {
            return None;
        }
        let loc = unsafe { CGEventGetLocation(ev) };
        unsafe {
            CFRelease(ev.cast());
        }
        Some(loc)
    }

    pub fn record_external_frontmost_app() {
        use objc2_app_kit::NSWorkspace;
        let workspace = NSWorkspace::sharedWorkspace();
        if let Some(app) = workspace.frontmostApplication() {
            let pid = app.processIdentifier();
            if pid > 0 && pid != std::process::id() as i32 {
                LAST_EXTERNAL_APP_PID.store(pid, Ordering::Release);
            }
        }
    }

    pub fn frontmost_external_pid() -> Option<i32> {
        use objc2_app_kit::NSWorkspace;
        let our_pid = std::process::id() as i32;
        let pid = NSWorkspace::sharedWorkspace()
            .frontmostApplication()
            .map_or(0, |a| a.processIdentifier());
        if pid > 0 && pid != our_pid {
            return Some(pid);
        }
        let fallback = LAST_EXTERNAL_APP_PID.load(Ordering::Acquire);
        (fallback > 0 && fallback != our_pid).then_some(fallback)
    }

    pub fn frontmost_app_info() -> Option<(i32, String, Option<String>)> {
        use objc2_app_kit::{NSRunningApplication, NSWorkspace};
        let our_pid = std::process::id() as i32;
        let workspace = NSWorkspace::sharedWorkspace();
        if let Some(app) = workspace.frontmostApplication() {
            let pid = app.processIdentifier();
            if pid != our_pid {
                LAST_EXTERNAL_APP_PID.store(pid, Ordering::Release);
                let name = app.localizedName()?.to_string();
                let bundle_id = app.bundleIdentifier().map(|b| b.to_string());
                return Some((pid, name, bundle_id));
            }
        }
        let fallback_pid = LAST_EXTERNAL_APP_PID.load(Ordering::Acquire);
        if fallback_pid > 0 && fallback_pid != our_pid {
            let app = NSRunningApplication::runningApplicationWithProcessIdentifier(fallback_pid)?;
            let name = app.localizedName()?.to_string();
            let bundle_id = app.bundleIdentifier().map(|b| b.to_string());
            return Some((fallback_pid, name, bundle_id));
        }
        None
    }

    /// Discovers the current capture target (either a specific window/app or the display under the pointer).
    pub fn discover_target(window_query: Option<&str>) -> TargetInfo {
        discover_target_at(window_query, current_mouse_location().map(|p| [p.x, p.y]))
    }

    /// Like [`discover_target`], recording the display under `point` (see [`pick_display`]).
    pub fn discover_target_at(window_query: Option<&str>, point: Option<[f64; 2]>) -> TargetInfo {
        let frontmost = frontmost_app_info();
        if let Ok(content) = SCShareableContent::get() {
            let our_pid = std::process::id() as i32;
            if let Some(query) = window_query {
                let q = query.to_lowercase();
                if let Some(win) = content.windows().into_iter().find(|w| {
                    w.is_on_screen()
                        && w.window_layer() == 0
                        && w.owning_application()
                            .as_ref()
                            .is_none_or(|a| a.process_id() != our_pid)
                        && (w
                            .title()
                            .as_deref()
                            .is_some_and(|t| t.to_lowercase().contains(&q))
                            || w.owning_application()
                                .as_ref()
                                .is_some_and(|a| a.application_name().to_lowercase().contains(&q)))
                }) {
                    let frame = win.frame();
                    let owner = win.owning_application();
                    let display_id = content
                        .displays()
                        .first()
                        .map(|d| d.display_id())
                        .unwrap_or_else(|| unsafe { CGMainDisplayID() });
                    return TargetInfo {
                        mode: CaptureMode::Window,
                        app: owner.as_ref().map(|a| a.application_name()),
                        bundle_id: owner.as_ref().map(|a| a.bundle_identifier()),
                        window_title: win.title(),
                        window_id: Some(win.window_id()),
                        display_id,
                        frame: TargetFrame {
                            x: frame.origin.x,
                            y: frame.origin.y,
                            width: frame.size.width.max(1.0),
                            height: frame.size.height.max(1.0),
                            scale: 2.0,
                        },
                    };
                }
            }

            let displays = content.displays();
            let front_window = frontmost.as_ref().and_then(|(pid, _, _)| {
                content.windows().into_iter().find(|w| {
                    w.is_on_screen()
                        && w.window_layer() == 0
                        && w.owning_application()
                            .as_ref()
                            .is_some_and(|a| a.process_id() == *pid)
                })
            });
            let rects: Vec<[f64; 4]> = displays
                .iter()
                .map(|d| {
                    let f = d.frame();
                    [f.origin.x, f.origin.y, f.size.width, f.size.height]
                })
                .collect();
            let focused = front_window.as_ref().map(|w| {
                let f = w.frame();
                [f.origin.x, f.origin.y, f.size.width, f.size.height]
            });
            if let Some(display) = displays.get(pick_display(&rects, point, focused)) {
                let frame = display.frame();
                let window_title = front_window.as_ref().and_then(|w| w.title());
                return TargetInfo {
                    mode: CaptureMode::Display,
                    app: frontmost.as_ref().map(|(_, name, _)| name.clone()),
                    bundle_id: frontmost.and_then(|(_, _, bid)| bid),
                    window_title,
                    window_id: None,
                    display_id: display.display_id(),
                    frame: TargetFrame {
                        x: frame.origin.x,
                        y: frame.origin.y,
                        width: frame.size.width.max(1.0),
                        height: frame.size.height.max(1.0),
                        scale: 2.0,
                    },
                };
            }
        }

        let display_id = unsafe { CGMainDisplayID() };
        let width = (unsafe { CGDisplayPixelsWide(display_id) } as f64).max(1440.0);
        let height = (unsafe { CGDisplayPixelsHigh(display_id) } as f64).max(900.0);
        TargetInfo {
            mode: CaptureMode::Display,
            app: frontmost.as_ref().map(|(_, name, _)| name.clone()),
            bundle_id: frontmost.and_then(|(_, _, bid)| bid),
            window_title: None,
            window_id: None,
            display_id,
            frame: TargetFrame {
                x: 0.0,
                y: 0.0,
                width,
                height,
                scale: 2.0,
            },
        }
    }

    /// Lists all on-screen normal (`window_layer == 0`) windows in front-to-back order.
    pub fn list_selectable_windows() -> Vec<TargetInfo> {
        let Ok(content) = SCShareableContent::get() else {
            return Vec::new();
        };
        let our_pid = std::process::id() as i32;
        let display_id = content
            .displays()
            .first()
            .map(|d| d.display_id())
            .unwrap_or_else(|| unsafe { CGMainDisplayID() });

        content
            .windows()
            .into_iter()
            .filter(|w| {
                let f = w.frame();
                w.is_on_screen()
                    && w.window_layer() == 0
                    && f.size.width >= 80.0
                    && f.size.height >= 80.0
                    && w.owning_application()
                        .as_ref()
                        .is_some_and(|a| a.process_id() != our_pid)
            })
            .map(|w| {
                let f = w.frame();
                let owner = w.owning_application();
                TargetInfo {
                    mode: CaptureMode::Window,
                    app: owner
                        .as_ref()
                        .map(|a| clean_label(&a.application_name()))
                        .filter(|s| !s.is_empty()),
                    bundle_id: owner.as_ref().map(|a| a.bundle_identifier()),
                    window_title: w.title().map(|t| clean_label(&t)).filter(|t| !t.is_empty()),
                    window_id: Some(w.window_id()),
                    display_id,
                    frame: TargetFrame {
                        x: f.origin.x,
                        y: f.origin.y,
                        width: f.size.width.max(1.0),
                        height: f.size.height.max(1.0),
                        scale: 2.0,
                    },
                }
            })
            .collect()
    }

    pub fn region_target(
        x: f64,
        y: f64,
        width: f64,
        height: f64,
        windows: &[TargetInfo],
    ) -> TargetInfo {
        let mid_x = x + width * 0.5;
        let mid_y = y + height * 0.5;
        let hit = selectable_window_at(windows, mid_x, mid_y);
        let frontmost = frontmost_app_info();
        let display_id = unsafe { CGMainDisplayID() };
        TargetInfo {
            mode: CaptureMode::Region,
            app: hit
                .and_then(|window| window.app.clone())
                .or_else(|| frontmost.as_ref().map(|(_, name, _)| name.clone())),
            bundle_id: hit
                .and_then(|window| window.bundle_id.clone())
                .or_else(|| frontmost.and_then(|(_, _, bid)| bid)),
            window_title: hit.and_then(|window| window.window_title.clone()),
            window_id: None,
            display_id,
            frame: TargetFrame {
                x,
                y,
                width: width.max(32.0),
                height: height.max(32.0),
                scale: 2.0,
            },
        }
    }

    fn build_sck_filter_and_config(
        target: &TargetInfo,
        excluded_window_ids: &[u32],
    ) -> Option<(SCContentFilter, SCStreamConfiguration)> {
        let content = SCShareableContent::get().ok()?;
        let our_pid = std::process::id() as i32;

        let filter = if let Some(window_id) = target.window_id
            && let Some(window) = content
                .windows()
                .into_iter()
                .find(|w| w.window_id() == window_id)
        {
            SCContentFilter::create().with_window(&window).build().ok()
        } else if let Some(display) = content
            .displays()
            .into_iter()
            .find(|d| d.display_id() == target.display_id)
            .or_else(|| content.displays().into_iter().next())
        {
            let excluded_overlays: Vec<SCWindow> = content
                .windows()
                .into_iter()
                .filter(|w| {
                    excluded_window_ids.contains(&w.window_id())
                        || (w
                            .owning_application()
                            .as_ref()
                            .is_some_and(|a| a.process_id() == our_pid)
                            && w.window_layer() != 0)
                })
                .collect();
            let excluded_refs: Vec<&SCWindow> = excluded_overlays.iter().collect();
            SCContentFilter::create()
                .with_display(&display)
                .with_excluding_windows(&excluded_refs)
                .build()
                .ok()
        } else {
            None
        }?;

        let long_edge = if target.mode == CaptureMode::Display {
            2560.0
        } else {
            1280.0
        };
        Some((filter, sck_config(target, long_edge)))
    }

    fn sck_config(target: &TargetInfo, long_edge: f64) -> SCStreamConfiguration {
        let (width, height) = capture_size(target, long_edge);
        let mut config = SCStreamConfiguration::new()
            .with_width(width)
            .with_height(height)
            .with_shows_cursor(true);
        if target.mode == CaptureMode::Region {
            config.set_source_rect(CGRect::new(
                target.frame.x,
                target.frame.y,
                target.frame.width.max(32.0),
                target.frame.height.max(32.0),
            ));
        }
        config
    }

    fn setup_sck_capture(
        target: &TargetInfo,
        video_output_path: Option<PathBuf>,
        excluded_window_ids: &[u32],
        started_at: Instant,
        video_offset_ms: &AtomicU64,
    ) -> (
        Option<SCStream>,
        Option<SCRecordingOutput>,
        Option<PathBuf>,
        Option<SCContentFilter>,
    ) {
        let Some((filter, config)) = build_sck_filter_and_config(target, excluded_window_ids)
        else {
            return (None, None, None, None);
        };

        if let Some(video_path) = video_output_path
            && let Ok(rec_cfg_builder) = SCRecordingOutputConfiguration::new()
            && let Ok(rec_cfg) = rec_cfg_builder.with_output_url(&video_path).map(|c| {
                c.with_video_codec(SCRecordingOutputCodec::H264)
                    .with_output_file_type(SCRecordingOutputFileType::MP4)
            })
            && let Some(rec_output) = SCRecordingOutput::new(&rec_cfg)
            && let Ok(stream) = SCStream::new(&filter, &config)
            && stream.add_recording_output(&rec_output).is_ok()
        {
            let before = started_at.elapsed();
            if stream.start_capture().is_ok() {
                let after = started_at.elapsed();
                video_offset_ms.store(((before + after) / 2).as_millis() as u64, Ordering::Release);
                return (
                    Some(stream),
                    Some(rec_output),
                    Some(video_path),
                    Some(filter),
                );
            }
        }

        (None, None, None, Some(filter))
    }

    fn capture_sck_frame(
        filter: &SCContentFilter,
        config: &SCStreamConfiguration,
        t_ms: u64,
    ) -> Option<RgbaFrame> {
        let image = SCScreenshotManager::capture_image(filter, config).ok()?;
        let width = u32::try_from(image.width()).ok()?;
        let height = u32::try_from(image.height()).ok()?;
        if width == 0 || height == 0 {
            return None;
        }
        let rgba = image.rgba_data().ok()?;
        if rgba.len() != (width as usize) * (height as usize) * 4 {
            return None;
        }
        Some(RgbaFrame::new(t_ms, width, height, rgba))
    }

    /// Single-shot / on-demand `FrameSource` implementation backed by `SCScreenshotManager`.
    pub struct MacosFrameSource {
        filter: Option<SCContentFilter>,
        config: SCStreamConfiguration,
    }

    impl MacosFrameSource {
        pub fn new(target: &TargetInfo, excluded_window_ids: &[u32], long_edge: f64) -> Self {
            let filter = build_sck_filter_and_config(target, excluded_window_ids).map(|(f, _)| f);
            let config = sck_config(target, long_edge);
            Self { filter, config }
        }
    }

    impl FrameSource for MacosFrameSource {
        fn capture_frame(&mut self, t_ms: u64) -> Result<Option<RgbaFrame>> {
            let Some(ref filter) = self.filter else {
                return Ok(None);
            };
            Ok(capture_sck_frame(filter, &self.config, t_ms))
        }

        fn decode_frames(
            &mut self,
            _video_path: &Path,
            _times_ms: &[u64],
            _video_offset_ms: u64,
            _max_edge: u32,
        ) -> Result<Vec<RgbaFrame>> {
            bail!("AVAssetImageGenerator frame decode is not wired in M1")
        }
    }

    /// Active screen + optional MP4 recorder using `ScreenCaptureKit`.
    pub struct ScreenCaptureSession {
        started_at: Instant,
        stopping: Arc<AtomicBool>,
        urgent_snapshot: Arc<AtomicBool>,
        frames: Arc<Mutex<CandidateBuffer>>,
        video_offset_ms: Arc<AtomicU64>,
        sampler_thread: Option<JoinHandle<Option<PathBuf>>>,
    }

    impl ScreenCaptureSession {
        pub fn start(
            target: TargetInfo,
            video_output_path: Option<PathBuf>,
            excluded_window_ids: Arc<Mutex<Vec<u32>>>,
        ) -> Self {
            let started_at = Instant::now();
            let stopping = Arc::new(AtomicBool::new(false));
            let urgent_snapshot = Arc::new(AtomicBool::new(true));
            let frames = Arc::new(Mutex::new(CandidateBuffer::new(CANDIDATE_BUDGET_BYTES)));
            let video_offset_ms = Arc::new(AtomicU64::new(0));

            let thread_stopping = Arc::clone(&stopping);
            let thread_urgent = Arc::clone(&urgent_snapshot);
            let thread_frames = Arc::clone(&frames);
            let thread_offset = Arc::clone(&video_offset_ms);
            let sampler_target = target;
            let sampler_thread = thread::Builder::new()
                .name("hex-scrawl-frame-sampler".into())
                .spawn(move || {
                    let initial_excluded = excluded_window_ids
                        .lock()
                        .unwrap_or_else(std::sync::PoisonError::into_inner)
                        .clone();
                    let (stream, recording_output, video_path, filter) = setup_sck_capture(
                        &sampler_target,
                        video_output_path,
                        &initial_excluded,
                        started_at,
                        &thread_offset,
                    );
                    let shot_edge = if video_path.is_some() {
                        CANDIDATE_EDGE
                    } else {
                        CANDIDATE_EDGE_NO_VIDEO
                    };
                    let shot_config = sck_config(&sampler_target, shot_edge);
                    let mut filter_and_config = filter.map(|f| (f, shot_config));
                    let mut last_sample = Instant::now()
                        .checked_sub(FRAME_SAMPLE_INTERVAL)
                        .unwrap_or_else(Instant::now);
                    let mut phase = 0_u32;
                    let mut last_excluded_len = initial_excluded.len();
                    while !thread_stopping.load(Ordering::Acquire) {
                        let cur_excluded = excluded_window_ids
                            .lock()
                            .unwrap_or_else(std::sync::PoisonError::into_inner)
                            .clone();
                        if cur_excluded.len() != last_excluded_len {
                            last_excluded_len = cur_excluded.len();
                            if let Some((new_filter, _)) =
                                build_sck_filter_and_config(&sampler_target, &cur_excluded)
                            {
                                if let Some(ref stream) = stream {
                                    let _ = stream.update_content_filter(&new_filter);
                                }
                                filter_and_config =
                                    Some((new_filter, sck_config(&sampler_target, shot_edge)));
                            }
                        }
                        let urgent = thread_urgent.swap(false, Ordering::AcqRel);
                        let interval = thread_frames
                            .lock()
                            .unwrap_or_else(std::sync::PoisonError::into_inner)
                            .interval();
                        if urgent || last_sample.elapsed() >= interval {
                            last_sample = Instant::now();
                            let t_ms = started_at.elapsed().as_millis() as u64;
                            if urgent {
                                phase = phase.saturating_add(1);
                            }
                            let frame = if let Some((ref filter, ref config)) = filter_and_config {
                                capture_sck_frame(filter, config, t_ms).unwrap_or_else(|| {
                                    render_synthetic_demo_frame(1280, 800, t_ms, phase)
                                })
                            } else {
                                render_synthetic_demo_frame(1280, 800, t_ms, phase)
                            };
                            thread_frames
                                .lock()
                                .unwrap_or_else(std::sync::PoisonError::into_inner)
                                .push(frame, urgent);
                        }
                        thread::park_timeout(Duration::from_millis(30));
                    }
                    if let Some(stream) = stream {
                        let _ = stream.stop_capture();
                        if let Some(ref output) = recording_output {
                            let _ = stream.remove_recording_output(output);
                        }
                    }
                    video_path
                })
                .ok();

            Self {
                started_at,
                stopping,
                urgent_snapshot,
                frames,
                video_offset_ms,
                sampler_thread,
            }
        }

        pub fn urgent_trigger(&self) -> Arc<AtomicBool> {
            Arc::clone(&self.urgent_snapshot)
        }

        pub fn request_immediate_frame(&self) {
            self.urgent_snapshot.store(true, Ordering::Release);
            if let Some(ref handle) = self.sampler_thread {
                handle.thread().unpark();
            }
        }

        pub fn elapsed_ms(&self) -> u64 {
            self.started_at.elapsed().as_millis() as u64
        }

        pub fn stop(mut self) -> ScreenRecording {
            let duration_ms = self.elapsed_ms();
            self.stopping.store(true, Ordering::Release);
            let video_path = self
                .sampler_thread
                .take()
                .and_then(|handle| {
                    handle.thread().unpark();
                    handle.join().ok()
                })
                .flatten();
            let buffer = std::mem::replace(
                &mut *self
                    .frames
                    .lock()
                    .unwrap_or_else(std::sync::PoisonError::into_inner),
                CandidateBuffer::new(0),
            );
            let mut collected = buffer.into_frames();
            if collected.is_empty() {
                collected.push(render_synthetic_demo_frame(1280, 800, 0, 0));
            }
            let valid_video = video_path
                .filter(|path| path.exists() && path.metadata().is_ok_and(|m| m.len() > 0));
            ScreenRecording {
                frames: collected,
                video: valid_video,
                duration_ms: duration_ms.max(100),
                video_offset_ms: self.video_offset_ms.load(Ordering::Acquire),
            }
        }
    }

    impl Drop for ScreenCaptureSession {
        fn drop(&mut self) {
            self.stopping.store(true, Ordering::Release);
            if let Some(handle) = self.sampler_thread.take() {
                handle.thread().unpark();
                let _ = handle.join();
            }
        }
    }

    /// macOS `WindowProbe` implementation over `CGWindowListCopyWindowInfo`, `NSRunningApplication`,
    /// `NSScreen::visibleFrame` (Dock exclusion), and `CGEventGetLocation`.
    #[derive(Clone, Copy, Debug, Default)]
    pub struct MacosWindowProbe;

    impl WindowProbe for MacosWindowProbe {
        fn raw_windows(&mut self) -> Vec<RawWindow> {
            let raw = unsafe {
                CGWindowListCopyWindowInfo(
                    KW_OPTION_ON_SCREEN_ONLY | KW_OPTION_EXCLUDE_DESKTOP_ELEMENTS,
                    0,
                )
            };
            if raw.is_null() {
                return Vec::new();
            }
            let list: CFArray<*const c_void> = unsafe { CFArray::wrap_under_create_rule(raw) };
            let k_number = CFString::from_static_string("kCGWindowNumber");
            let k_pid = CFString::from_static_string("kCGWindowOwnerPID");
            let k_owner = CFString::from_static_string("kCGWindowOwnerName");
            let k_title = CFString::from_static_string("kCGWindowName");
            let k_layer = CFString::from_static_string("kCGWindowLayer");
            let k_alpha = CFString::from_static_string("kCGWindowAlpha");
            let k_bounds = CFString::from_static_string("kCGWindowBounds");
            let k_x = CFString::from_static_string("X");
            let k_y = CFString::from_static_string("Y");
            let k_w = CFString::from_static_string("Width");
            let k_h = CFString::from_static_string("Height");

            let number = |dict: &CFDictionary<CFString, CFType>, k: &CFString| -> Option<f64> {
                dict.find(k)?.downcast::<CFNumber>()?.to_f64()
            };
            let string = |dict: &CFDictionary<CFString, CFType>, k: &CFString| -> String {
                dict.find(k)
                    .and_then(|v| v.downcast::<CFString>())
                    .map(|s| s.to_string())
                    .unwrap_or_default()
            };
            let mut out = Vec::new();
            for item in list.iter() {
                let ptr = *item as CFDictionaryRef;
                if ptr.is_null() {
                    continue;
                }
                let dict: CFDictionary<CFString, CFType> =
                    unsafe { CFDictionary::wrap_under_get_rule(ptr) };
                let Some(bounds) = dict
                    .find(&k_bounds)
                    .and_then(|v| v.downcast::<CFDictionary>())
                else {
                    continue;
                };
                let bounds: CFDictionary<CFString, CFType> =
                    unsafe { CFDictionary::wrap_under_get_rule(bounds.as_concrete_TypeRef()) };
                out.push(RawWindow {
                    id: number(&dict, &k_number).unwrap_or(0.0) as u32,
                    pid: number(&dict, &k_pid).unwrap_or(0.0) as i32,
                    owner: string(&dict, &k_owner),
                    title: string(&dict, &k_title),
                    bounds: [
                        number(&bounds, &k_x).unwrap_or(0.0),
                        number(&bounds, &k_y).unwrap_or(0.0),
                        number(&bounds, &k_w).unwrap_or(0.0),
                        number(&bounds, &k_h).unwrap_or(0.0),
                    ],
                    layer: number(&dict, &k_layer).unwrap_or(1.0) as i32,
                    alpha: number(&dict, &k_alpha).unwrap_or(1.0),
                });
            }
            out
        }

        fn frontmost_external_pid(&mut self) -> Option<i32> {
            frontmost_external_pid()
        }

        fn app_meta(&mut self, pid: i32) -> AppMeta {
            use objc2_app_kit::{NSApplicationActivationPolicy, NSRunningApplication};
            let Some(app) = NSRunningApplication::runningApplicationWithProcessIdentifier(pid)
            else {
                return AppMeta {
                    bundle_id: None,
                    regular: false,
                };
            };
            let regular = app.activationPolicy() == NSApplicationActivationPolicy::Regular;
            let bundle_id = app.bundleIdentifier().map(|b| b.to_string());
            AppMeta { bundle_id, regular }
        }

        fn cursor_screen(&mut self) -> Option<[f32; 2]> {
            let p = current_mouse_location()?;
            Some([p.x as f32, p.y as f32])
        }
    }

    pub fn dock_rect(frame: TargetFrame) -> Option<[f64; 4]> {
        use objc2::MainThreadMarker;
        use objc2_app_kit::NSScreen;
        let mtm = unsafe { MainThreadMarker::new_unchecked() };
        let screens = NSScreen::screens(mtm);
        let primary_h = screens.iter().next()?.frame().size.height;
        for screen in screens {
            let sf = screen.frame();
            let sx = sf.origin.x;
            let sy = primary_h - sf.origin.y - sf.size.height;
            if (sx - frame.x).abs() > 2.0
                || (sy - frame.y).abs() > 2.0
                || (sf.size.width - frame.width).abs() > 2.0
                || (sf.size.height - frame.height).abs() > 2.0
            {
                continue;
            }
            let vf = screen.visibleFrame();
            let vx = vf.origin.x - sx;
            let vy = (primary_h - vf.origin.y - vf.size.height) - sy;
            let vw = vf.size.width;
            let vh = vf.size.height;
            let left = vx;
            let right = (frame.width - (vx + vw)).max(0.0);
            let bottom = (frame.height - (vy + vh)).max(0.0);
            if bottom > 20.0 {
                return Some([0.0, frame.height - bottom, frame.width, bottom]);
            }
            if left > 20.0 {
                return Some([0.0, 0.0, left, frame.height]);
            }
            if right > 20.0 {
                return Some([frame.width - right, 0.0, right, frame.height]);
            }
            return None;
        }
        None
    }

    /// Session-scoped macOS `InputSource` that records mouse clicks and runs a [`WindowLogger`]
    /// while a Scrawl capture is active.
    pub struct MacosInputSource {
        stopping: Arc<AtomicBool>,
        window_request: Arc<AtomicBool>,
        events: Arc<Mutex<Vec<InteractionEvent>>>,
        strokes: Arc<Mutex<Vec<Stroke>>>,
        click_worker: Option<JoinHandle<()>>,
        window_logger: Option<WindowLogger>,
    }

    impl MacosInputSource {
        pub fn start(target: TargetInfo, urgent_snapshot: Option<Arc<AtomicBool>>) -> Self {
            let stopping = Arc::new(AtomicBool::new(false));
            let window_request = Arc::new(AtomicBool::new(false));
            let events = Arc::new(Mutex::new(Vec::new()));
            let strokes = Arc::new(Mutex::new(Vec::new()));
            let started_at = Instant::now();
            let window_logger = if target.mode == CaptureMode::Display {
                Some(WindowLogger::start(
                    target.frame,
                    dock_rect(target.frame),
                    started_at,
                    Arc::clone(&window_request),
                    MacosWindowProbe,
                ))
            } else {
                None
            };
            let worker_stopping = Arc::clone(&stopping);
            let worker_events = Arc::clone(&events);
            let worker_window_request = Arc::clone(&window_request);
            let frame = target.frame;
            let click_worker = thread::Builder::new()
                .name("hex-scrawl-click-sampler".into())
                .spawn(move || {
                    let mut prev_buttons = [false; 2];
                    while !worker_stopping.load(Ordering::Acquire) {
                        thread::sleep(Duration::from_millis(12));
                        let flags =
                            unsafe { CGEventSourceFlagsState(0) | CGEventSourceFlagsState(1) };
                        let shift = (flags & FLAG_SHIFT) != 0;
                        let buttons = [
                            unsafe {
                                CGEventSourceButtonState(0, 0) || CGEventSourceButtonState(1, 0)
                            },
                            unsafe {
                                CGEventSourceButtonState(0, 1) || CGEventSourceButtonState(1, 1)
                            },
                        ];
                        if buttons != prev_buttons {
                            let down = [
                                buttons[0] && !prev_buttons[0],
                                buttons[1] && !prev_buttons[1],
                            ];
                            prev_buttons = buttons;
                            if (down[0] || down[1])
                                && !shift
                                && let Some(loc) = current_mouse_location()
                            {
                                let nx = ((loc.x - frame.x) / frame.width.max(1.0)).clamp(0.0, 1.0)
                                    as f32;
                                let ny = ((loc.y - frame.y) / frame.height.max(1.0)).clamp(0.0, 1.0)
                                    as f32;
                                let t = started_at.elapsed().as_millis() as u64;
                                let button = if down[1] { "right" } else { "left" };
                                worker_events
                                    .lock()
                                    .unwrap_or_else(std::sync::PoisonError::into_inner)
                                    .push(InteractionEvent::Click {
                                        t,
                                        x: (nx * 100.0).round() / 100.0,
                                        y: (ny * 100.0).round() / 100.0,
                                        button: Some(button.into()),
                                        label: None,
                                        role: None,
                                    });
                                worker_window_request.store(true, Ordering::Release);
                                if let Some(ref urgent) = urgent_snapshot {
                                    urgent.store(true, Ordering::Release);
                                }
                            }
                        }
                    }
                })
                .ok();

            Self {
                stopping,
                window_request,
                events,
                strokes,
                click_worker,
                window_logger,
            }
        }

        /// Wakes the window logger immediately (e.g. when a click occurs).
        pub fn note_click(&self) {
            self.window_request.store(true, Ordering::Release);
        }
    }

    impl InputSource for MacosInputSource {
        fn poll_events(&mut self) -> ScrawlInputBatch {
            let events = std::mem::take(
                &mut *self
                    .events
                    .lock()
                    .unwrap_or_else(std::sync::PoisonError::into_inner),
            );
            let strokes = std::mem::take(
                &mut *self
                    .strokes
                    .lock()
                    .unwrap_or_else(std::sync::PoisonError::into_inner),
            );
            ScrawlInputBatch {
                events,
                strokes,
                cursor: Vec::new(),
                windows: Vec::new(),
            }
        }

        fn finish(&mut self) -> ScrawlInputBatch {
            self.stopping.store(true, Ordering::Release);
            if let Some(worker) = self.click_worker.take() {
                let _ = worker.join();
            }
            let activity = self
                .window_logger
                .take()
                .map(WindowLogger::stop)
                .unwrap_or_default();
            let events = std::mem::take(
                &mut *self
                    .events
                    .lock()
                    .unwrap_or_else(std::sync::PoisonError::into_inner),
            );
            let strokes = std::mem::take(
                &mut *self
                    .strokes
                    .lock()
                    .unwrap_or_else(std::sync::PoisonError::into_inner),
            );
            ScrawlInputBatch {
                events,
                strokes,
                cursor: activity.cursor,
                windows: activity.windows,
            }
        }
    }

    impl Drop for MacosInputSource {
        fn drop(&mut self) {
            self.stopping.store(true, Ordering::Release);
            if let Some(worker) = self.click_worker.take() {
                let _ = worker.join();
            }
            if let Some(logger) = self.window_logger.take() {
                let _ = logger.stop();
            }
        }
    }
}

#[cfg(not(target_os = "macos"))]
mod imp {
    use std::path::{Path, PathBuf};
    use std::sync::atomic::AtomicBool;
    use std::sync::{Arc, Mutex};

    use anyhow::{Result, bail};
    use scrawl_core::{
        AppMeta, FrameSource, InputSource, RawWindow, RgbaFrame, ScrawlInputBatch, TargetFrame,
        TargetInfo, WindowProbe,
    };

    use super::ScreenRecording;

    pub const UNSUPPORTED_MESSAGE: &str =
        "scrawl-capture-macos is only supported on macOS (ScreenCaptureKit)";

    pub fn screen_capture_granted() -> bool {
        false
    }

    pub fn request_screen_capture_access() -> bool {
        false
    }

    pub fn record_external_frontmost_app() {}

    pub fn frontmost_external_pid() -> Option<i32> {
        None
    }

    pub fn frontmost_app_info() -> Option<(i32, String, Option<String>)> {
        None
    }

    pub fn discover_target(_window_query: Option<&str>) -> TargetInfo {
        TargetInfo::default()
    }

    pub fn discover_target_at(_window_query: Option<&str>, _point: Option<[f64; 2]>) -> TargetInfo {
        TargetInfo::default()
    }

    pub fn list_selectable_windows() -> Vec<TargetInfo> {
        Vec::new()
    }

    pub fn region_target(
        x: f64,
        y: f64,
        width: f64,
        height: f64,
        _windows: &[TargetInfo],
    ) -> TargetInfo {
        TargetInfo {
            mode: scrawl_core::CaptureMode::Region,
            frame: TargetFrame {
                x,
                y,
                width: width.max(32.0),
                height: height.max(32.0),
                scale: 2.0,
            },
            ..TargetInfo::default()
        }
    }

    pub struct MacosFrameSource;

    impl MacosFrameSource {
        pub fn new(_target: &TargetInfo, _excluded_window_ids: &[u32], _long_edge: f64) -> Self {
            Self
        }
    }

    impl FrameSource for MacosFrameSource {
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

    pub struct ScreenCaptureSession;

    impl ScreenCaptureSession {
        pub fn start(
            _target: TargetInfo,
            _video_output_path: Option<PathBuf>,
            _excluded_window_ids: Arc<Mutex<Vec<u32>>>,
        ) -> Self {
            Self
        }

        pub fn urgent_trigger(&self) -> Arc<AtomicBool> {
            Arc::new(AtomicBool::new(false))
        }

        pub fn request_immediate_frame(&self) {}

        pub fn elapsed_ms(&self) -> u64 {
            0
        }

        pub fn stop(self) -> ScreenRecording {
            ScreenRecording {
                frames: Vec::new(),
                video: None,
                duration_ms: 0,
                video_offset_ms: 0,
            }
        }
    }

    #[derive(Clone, Copy, Debug, Default)]
    pub struct MacosWindowProbe;

    impl WindowProbe for MacosWindowProbe {
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

    pub fn dock_rect(_frame: TargetFrame) -> Option<[f64; 4]> {
        None
    }

    pub struct MacosInputSource;

    impl MacosInputSource {
        pub fn start(_target: TargetInfo, _urgent_snapshot: Option<Arc<AtomicBool>>) -> Self {
            Self
        }

        pub fn note_click(&self) {}
    }

    impl InputSource for MacosInputSource {
        fn poll_events(&mut self) -> ScrawlInputBatch {
            ScrawlInputBatch::default()
        }

        fn finish(&mut self) -> ScrawlInputBatch {
            ScrawlInputBatch::default()
        }
    }
}

pub use imp::*;

#[cfg(test)]
mod tests {
    use scrawl_core::{CANDIDATE_EDGE, CaptureMode, TargetFrame, TargetInfo};

    use super::*;

    #[test]
    fn recordings_use_the_display_under_the_cursor() {
        let displays = [[0.0, 0.0, 1728.0, 1117.0], [1728.0, -200.0, 2560.0, 1440.0]];
        assert_eq!(pick_display(&displays, Some([2000.0, 300.0]), None), 1);
        assert_eq!(pick_display(&displays, Some([100.0, 100.0]), None), 0);
        let focused = Some([1900.0, 0.0, 800.0, 600.0]);
        assert_eq!(pick_display(&displays, None, focused), 1);
        assert_eq!(pick_display(&displays, Some([-50.0, 0.0]), focused), 1);
        assert_eq!(pick_display(&displays, None, None), 0);
    }

    #[test]
    fn candidates_are_small_and_recordings_keep_full_size() {
        let target = |mode, width, height| TargetInfo {
            mode,
            app: None,
            bundle_id: None,
            window_title: None,
            window_id: None,
            display_id: 1,
            frame: TargetFrame {
                x: 0.0,
                y: 0.0,
                width,
                height,
                scale: 2.0,
            },
        };
        let display = target(CaptureMode::Display, 1728.0, 1117.0);
        assert_eq!(capture_size(&display, CANDIDATE_EDGE), (480, 310));
        assert_eq!(capture_size(&display, 2560.0), (2560, 1654));
        let region = target(CaptureMode::Region, 480.0, 320.0);
        assert_eq!(capture_size(&region, CANDIDATE_EDGE), (480, 320));
        assert_eq!(capture_size(&region, 1280.0), (960, 640));
    }

    #[test]
    fn cached_window_hit_preserves_front_to_back_order_and_screen_coordinates() {
        let window = |id, x, y| TargetInfo {
            mode: CaptureMode::Window,
            app: Some(format!("App {id}")),
            bundle_id: None,
            window_title: Some(format!("Window {id}")),
            window_id: Some(id),
            display_id: 1,
            frame: TargetFrame {
                x,
                y,
                width: 200.0,
                height: 100.0,
                scale: 2.0,
            },
        };
        let windows = [window(1, -100.0, 20.0), window(2, 0.0, 0.0)];
        assert_eq!(
            selectable_window_at(&windows, 50.0, 50.0)
                .unwrap()
                .window_id,
            Some(1)
        );
        assert_eq!(
            selectable_window_at(&windows, 150.0, 50.0)
                .unwrap()
                .window_id,
            Some(2)
        );
        assert!(selectable_window_at(&windows, -101.0, 50.0).is_none());
    }
}
