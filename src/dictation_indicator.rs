use std::collections::BTreeMap;
use std::ffi::c_void;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{self, Receiver, Sender};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use core_graphics_types::geometry::CGSize;
use core_video::base::{CVOptionFlags, CVTimeStamp, CVTimeStampFlags};
use core_video::display_link::{CVDisplayLink, CVDisplayLinkRef};
use core_video::r#return::CVReturn;
use metal::{
    CommandQueue, Device, MTLLoadAction, MTLPixelFormat, MTLPrimitiveType, MTLStoreAction,
    MetalLayer, RenderPassDescriptor, RenderPipelineDescriptor, RenderPipelineState,
};
use objc2::MainThreadMarker;
use objc2::rc::Retained;
use objc2_app_kit::{
    NSBackingStoreType, NSColor, NSEvent, NSPanel, NSScreen, NSStatusBar, NSStatusWindowLevel,
    NSView,
    NSWindowCollectionBehavior, NSWindowStyleMask,
};
use objc2_foundation::{NSPoint, NSRect, NSSize};
use objc2_quartz_core::CALayer;

use crate::app_settings::IndicatorPosition;
use crate::dot_scope::{self, Stage};
use crate::hud_placement;
use crate::spring::{REDUCED_FADE, Spec, Spring, reduced_motion};

mod dot_scope_view;
use dot_scope_view::DotScopeView;

#[link(name = "CoreGraphics", kind = "framework")]
unsafe extern "C" {
    static kCGColorSpaceSRGB: *const c_void;
    fn CGColorSpaceCreateWithName(name: *const c_void) -> *mut c_void;
    fn CGColorSpaceRelease(space: *mut c_void);
    fn CGMainDisplayID() -> u32;
}

/// A `CGColorSpaceRef` as Objective-C encodes it, so `setColorspace:` passes
/// the method-signature check debug builds of objc2 run.
#[repr(transparent)]
struct ColorSpaceRef(*mut c_void);

// SAFETY: a transparent CGColorSpace pointer, encoded as one.
unsafe impl objc2::Encode for ColorSpaceRef {
    const ENCODING: objc2::Encoding =
        objc2::Encoding::Pointer(&objc2::Encoding::Struct("CGColorSpace", &[]));
}

const WINDOW_WIDTH: f32 = 112.0;
const WINDOW_HEIGHT: f32 = 64.0;
const CAPSULE_WIDTH: f32 = 56.0;
const CAPSULE_HEIGHT: f32 = 16.0;
const ENTRANCE_ANGULAR_FREQUENCY: f32 = 24.0;
const EXIT_ANGULAR_FREQUENCY: f32 = 24.0;
const GEOMETRY_ANGULAR_FREQUENCY: f32 = 24.0;
const HIDDEN_SCALE: f32 = 0.82;
const HIDDEN_SOFTNESS: f32 = 4.0;
const PROCESSING_MORPH_DURATION: Duration = Duration::from_millis(250);
const COMPLETED_VISIBLE_DURATION: Duration = Duration::from_millis(240);
const RECORDING_FLASH_HALF_LIFE: Duration = Duration::from_millis(280);
/// Dot scope's loudness before any meter arrives, dBFS.
const SILENT: f32 = -120.0;
/// Dot scope's done wipe (0.3 s) and a beat of the dark plate after it.
const DOT_SCOPE_COMPLETED_VISIBLE_DURATION: Duration = Duration::from_millis(340);

/// The HUD window and the capsule it centres.
#[derive(Clone, Copy, Debug, PartialEq)]
struct Geometry {
    width: f32,
    height: f32,
    capsule_height: f32,
}

impl Geometry {
    const CAPSULE: Self = Self {
        width: WINDOW_WIDTH,
        height: WINDOW_HEIGHT,
        capsule_height: CAPSULE_HEIGHT,
    };
    /// Room for Dot scope's widest plate and its drop shadow, and for an
    /// island grown out of the widest notch.
    const DOT_SCOPE: Self = Self {
        width: 320.0,
        height: 128.0,
        capsule_height: dot_scope::PLATE_HEIGHT,
    };
}

/// Dot scope's window while its experiment is on, else the capsule's.
fn geometry() -> Geometry {
    if dot_scope_enabled() {
        Geometry::DOT_SCOPE
    } else {
        Geometry::CAPSULE
    }
}

fn dot_scope_enabled() -> bool {
    crate::experiments::enabled(crate::experiments::Experiment::DotScope)
}

#[derive(Clone, Copy, Debug)]
pub enum DictationIndicatorEvent {
    Reset,
    Configure(HudTuning),
    /// Dot scope's tuning (experiment `dot-scope`).
    ConfigureDotScope(dot_scope::Tuning),
    /// HUD Lab: Dot scope follows a synthetic voice instead of the meter
    /// until the next `Reset`.
    DemoVoice,
    Started,
    EditingStarted,
    PromotedToVoiceAction,
    Meter {
        average: f32,
        peak: f32,
    },
    Submitted {
        job_id: u64,
    },
    Transcribing {
        job_id: u64,
    },
    Processing {
        job_id: u64,
    },
    Discarded,
    Cancelled,
    JobCompleted {
        job_id: u64,
    },
    JobCancelled {
        job_id: u64,
    },
    JobFailed {
        job_id: u64,
    },
    Failed,
}

#[derive(Clone, Copy, Debug)]
pub struct HudTuning {
    pub style: f32,
    pub line_count: f32,
    pub curvature: f32,
    pub speed: f32,
    pub sharpness: f32,
    pub glow: f32,
    pub depth: f32,
    pub light_angle: f32,
    pub outline: f32,
}

impl Default for HudTuning {
    fn default() -> Self {
        Self {
            style: 1.0,
            line_count: 2.0,
            curvature: 0.47,
            speed: 17.08,
            sharpness: 0.29,
            glow: 0.77,
            depth: 0.65,
            light_angle: 0.35,
            outline: 1.0,
        }
    }
}

#[derive(Clone)]
pub struct DictationIndicatorSender(Sender<DictationIndicatorEvent>);

impl DictationIndicatorSender {
    pub fn send(&self, event: DictationIndicatorEvent) {
        let _ = self.0.send(event);
    }

    pub fn meter(&self, samples: &[f32]) {
        if samples.is_empty() {
            return;
        }
        let mut sum_of_squares = 0.0;
        let mut peak = 0.0_f32;
        for sample in samples {
            sum_of_squares += sample * sample;
            peak = peak.max(sample.abs());
        }
        self.send(DictationIndicatorEvent::Meter {
            average: (sum_of_squares / samples.len() as f32).sqrt(),
            peak,
        });
    }
}

pub fn channel() -> (DictationIndicatorSender, Receiver<DictationIndicatorEvent>) {
    let (sender, receiver) = mpsc::channel();
    (DictationIndicatorSender(sender), receiver)
}

pub struct DictationIndicatorUi {
    indicator: Option<MetalIndicator>,
}

impl DictationIndicatorUi {
    pub fn new() -> Self {
        let started = Instant::now();
        let indicator = match MetalIndicator::new() {
            Ok(indicator) => {
                tracing::info!(
                    initialization_ms = started.elapsed().as_millis(),
                    "Metal dictation indicator prewarmed"
                );
                Some(indicator)
            }
            Err(error) => {
                tracing::error!(%error, "could not prewarm Metal dictation indicator");
                None
            }
        };
        Self { indicator }
    }

    pub fn handle(&mut self, event: DictationIndicatorEvent) {
        if self.indicator.is_none()
            && matches!(
                event,
                DictationIndicatorEvent::Started | DictationIndicatorEvent::EditingStarted
            )
        {
            match MetalIndicator::new() {
                Ok(indicator) => self.indicator = Some(indicator),
                Err(error) => {
                    tracing::error!(%error, "could not create Metal dictation indicator");
                    return;
                }
            }
        }
        if let Some(indicator) = &mut self.indicator {
            indicator.handle(event);
        }
    }

    pub fn follow_pointer(&mut self) {
        if let Some(indicator) = &mut self.indicator {
            indicator.maintain();
        }
    }
}

const DISPLAY_LINK_RETRY_INITIAL: Duration = Duration::from_millis(100);
const DISPLAY_LINK_RETRY_MAX: Duration = Duration::from_millis(2000);

struct AttachedDisplayLink {
    display_link: CVDisplayLink,
    context: *const SharedRenderer,
}

impl AttachedDisplayLink {
    fn create(renderer: &Arc<SharedRenderer>) -> Result<Self, CVReturn> {
        let display_link = CVDisplayLink::from_active_cg_displays()
            .or_else(|_| CVDisplayLink::from_cg_display(unsafe { CGMainDisplayID() }))?;
        let context = Arc::into_raw(renderer.clone());
        let configured = unsafe {
            display_link
                .set_output_callback(display_link_callback, context.cast_mut().cast::<c_void>())
        };
        if let Err(status) = configured {
            unsafe { drop(Arc::from_raw(context)) };
            return Err(status);
        }
        Ok(Self {
            display_link,
            context,
        })
    }

    fn is_running(&self) -> bool {
        self.display_link.is_running()
    }

    fn start(&mut self) -> Result<(), CVReturn> {
        self.display_link.start()
    }

    fn stop(&mut self) {
        let _ = self.display_link.stop();
    }
}

impl Drop for AttachedDisplayLink {
    fn drop(&mut self) {
        let _ = self.display_link.stop();
        unsafe { drop(Arc::from_raw(self.context)) };
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
struct DisplayOutputTime {
    video_seconds: f64,
    refresh_period: Option<f32>,
}

fn output_timestamp(output_time: *const CVTimeStamp) -> Option<DisplayOutputTime> {
    if output_time.is_null() {
        return None;
    }
    let ts = unsafe { &*output_time };
    let video_valid = (ts.flags & (CVTimeStampFlags::kCVTimeStampVideoTimeValid as u64)) != 0;
    if !video_valid || ts.videoTimeScale <= 0 {
        return None;
    }
    let scale = f64::from(ts.videoTimeScale);
    let video_seconds = (ts.videoTime as f64) / scale;
    let refresh_valid =
        (ts.flags & (CVTimeStampFlags::kCVTimeStampVideoRefreshPeriodValid as u64)) != 0;
    let refresh_period = (refresh_valid && ts.videoRefreshPeriod > 0)
        .then(|| ((ts.videoRefreshPeriod as f64) / scale) as f32);
    Some(DisplayOutputTime {
        video_seconds,
        refresh_period,
    })
}

struct MetalIndicator {
    window: Retained<NSPanel>,
    geometry: Geometry,
    renderer: Arc<SharedRenderer>,
    display_link: Option<AttachedDisplayLink>,
    display_link_retry_delay: Duration,
    display_link_next_retry: Instant,
    ordered: bool,
    /// Dot scope's placement and whether to pretend the screen has a
    /// notch; `None` for the capsule, which always sits below.
    placement: Option<(dot_scope::Placement, bool)>,
    placed: Option<hud_placement::Placed>,
}

impl MetalIndicator {
    fn new() -> Result<Self, String> {
        let mtm = MainThreadMarker::new().ok_or("indicator must be created on the main thread")?;
        let geometry = geometry();
        let renderer = Arc::new(SharedRenderer::new(geometry)?);
        let frame = NSRect::new(
            NSPoint::new(0.0, 0.0),
            NSSize::new(f64::from(geometry.width), f64::from(geometry.height)),
        );
        let window = NSPanel::initWithContentRect_styleMask_backing_defer(
            mtm.alloc(),
            frame,
            NSWindowStyleMask::Borderless
                | NSWindowStyleMask::UtilityWindow
                | NSWindowStyleMask::NonactivatingPanel
                | NSWindowStyleMask::FullSizeContentView,
            NSBackingStoreType::Buffered,
            false,
        );
        let view = NSView::initWithFrame(mtm.alloc(), frame);
        view.setWantsLayer(true);
        let metal_layer = renderer.layer();
        let cocoa_layer = unsafe { &*metal_layer.cast::<CALayer>() };
        cocoa_layer.setOpaque(false);
        view.setLayer(Some(cocoa_layer));
        window.setContentView(Some(&view));
        window.setBackgroundColor(Some(&NSColor::clearColor()));
        window.setOpaque(false);
        window.setIgnoresMouseEvents(true);
        window.setHasShadow(false);
        window.setHidesOnDeactivate(false);
        window.setCanHide(false);
        window.setLevel(NSStatusWindowLevel);
        window.setCollectionBehavior(
            NSWindowCollectionBehavior::CanJoinAllSpaces
                | NSWindowCollectionBehavior::CanJoinAllApplications
                | NSWindowCollectionBehavior::FullScreenAuxiliary
                | NSWindowCollectionBehavior::Stationary
                | NSWindowCollectionBehavior::IgnoresCycle,
        );
        unsafe { window.setReleasedWhenClosed(false) };

        let now = Instant::now();
        let mut indicator = Self {
            window,
            geometry,
            renderer,
            display_link: None,
            display_link_retry_delay: DISPLAY_LINK_RETRY_INITIAL,
            display_link_next_retry: now,
            ordered: false,
            placement: (geometry == Geometry::DOT_SCOPE).then(|| {
                let tuning = dot_scope::Tuning::load();
                (tuning.placement, tuning.pretend_notch)
            }),
            placed: None,
        };
        indicator.ensure_display_link(true);
        indicator.position_on_pointer_screen();
        Ok(indicator)
    }

    fn ensure_display_link(&mut self, force: bool) {
        if self.display_link.is_some() {
            return;
        }
        let now = Instant::now();
        if !force && now < self.display_link_next_retry {
            return;
        }
        match AttachedDisplayLink::create(&self.renderer) {
            Ok(link) => {
                self.display_link = Some(link);
                self.display_link_retry_delay = DISPLAY_LINK_RETRY_INITIAL;
                self.display_link_next_retry = now;
            }
            Err(status) => {
                tracing::warn!(
                    status,
                    retry_ms = self.display_link_retry_delay.as_millis(),
                    "could not attach indicator display link yet; Metal pipeline and panel remain prewarmed"
                );
                self.display_link_next_retry = now + self.display_link_retry_delay;
                self.display_link_retry_delay =
                    (self.display_link_retry_delay * 2).min(DISPLAY_LINK_RETRY_MAX);
            }
        }
    }

    fn handle(&mut self, event: DictationIndicatorEvent) {
        if matches!(
            event,
            DictationIndicatorEvent::Started | DictationIndicatorEvent::EditingStarted
        ) {
            self.renderer.set_dark(dot_scope_view::system_is_dark());
        }
        if let DictationIndicatorEvent::ConfigureDotScope(tuning) = event
            && self.placement.is_some()
        {
            self.placement = Some((tuning.placement, tuning.pretend_notch));
            if self.ordered {
                self.position_on_pointer_screen();
            }
        }
        self.renderer.handle(event);
        if matches!(
            event,
            DictationIndicatorEvent::Started | DictationIndicatorEvent::EditingStarted
        ) && crate::app_settings::indicator_position() != IndicatorPosition::Hidden
        {
            self.show_on_pointer_screen();
            tracing::info!("Metal dictation indicator shown");
        }
        self.drive_frames(true);
    }

    fn maintain(&mut self) {
        let hidden = crate::app_settings::indicator_position() == IndicatorPosition::Hidden;
        if self.renderer.is_active() && !hidden {
            self.show_on_pointer_screen();
            self.drive_frames(false);
        } else {
            if let Some(display_link) = &mut self.display_link
                && display_link.is_running()
            {
                display_link.stop();
            }
            if self.ordered {
                self.window.orderOut(None);
                self.ordered = false;
            }
        }
    }

    fn show_on_pointer_screen(&mut self) {
        self.position_on_pointer_screen();
        if !self.ordered {
            self.window.orderFrontRegardless();
            self.ordered = true;
        }
    }

    fn drive_frames(&mut self, force: bool) {
        self.ensure_display_link(force);
        if let Some(display_link) = &mut self.display_link {
            if !display_link.is_running()
                && let Err(status) = display_link.start()
                && force
            {
                tracing::error!(status, "could not start indicator display link");
            }
        } else {
            self.renderer.draw(None);
        }
    }

    fn position_on_pointer_screen(&mut self) {
        let Some(mtm) = MainThreadMarker::new() else {
            return;
        };
        let pointer = NSEvent::mouseLocation();
        let screens = NSScreen::screens(mtm);
        let Some(screen) = screens
            .iter()
            .find(|screen| {
                let frame = screen.frame();
                pointer.x >= frame.origin.x
                    && pointer.x < frame.origin.x + frame.size.width
                    && pointer.y >= frame.origin.y
                    && pointer.y < frame.origin.y + frame.size.height
            })
            .or_else(|| NSScreen::mainScreen(mtm))
        else {
            return;
        };
        let rect = |r: NSRect| hud_placement::Rect {
            x: r.origin.x,
            y: r.origin.y,
            width: r.size.width,
            height: r.size.height,
        };
        let mut metrics = hud_placement::Screen {
            frame: rect(screen.frame()),
            visible: rect(screen.visibleFrame()),
            safe_top: screen.safeAreaInsets().top,
            aux_left: screen.auxiliaryTopLeftArea().size.width,
            aux_right: screen.auxiliaryTopRightArea().size.width,
            bar: NSStatusBar::systemStatusBar().thickness(),
        };
        let (placement, pretend_notch) = self.placement.unwrap_or_default();
        if pretend_notch {
            metrics = metrics.pretending_notch();
        }
        let geometry = self.geometry;
        let placed = hud_placement::place(
            placement,
            &metrics,
            hud_placement::Content {
                window: (f64::from(geometry.width), f64::from(geometry.height)),
                plate: (
                    f64::from(geometry.width) - 24.0,
                    f64::from(geometry.capsule_height),
                ),
            },
            crate::app_settings::indicator_position() == IndicatorPosition::Bottom,
        );
        self.renderer.set_scale(screen.backingScaleFactor() as f32);
        if self.placed == Some(placed) {
            return;
        }
        if self.placed.map(|old| old.over_menu_bar) != Some(placed.over_menu_bar) {
            // Above the menu bar and its status items, below their menus.
            self.window.setLevel(if placed.over_menu_bar {
                NSStatusWindowLevel + 1
            } else {
                NSStatusWindowLevel
            });
        }
        self.placed = Some(placed);
        let (x, y) = placed.top_left;
        self.window.setFrameTopLeftPoint(NSPoint::new(x, y));
        self.renderer.set_layout(placed.layout);
    }
}

impl Drop for MetalIndicator {
    fn drop(&mut self) {
        self.display_link.take();
        self.window.close();
    }
}

extern "C" fn display_link_callback(
    _display_link: CVDisplayLinkRef,
    _now: *const CVTimeStamp,
    output_time: *const CVTimeStamp,
    _flags_in: CVOptionFlags,
    _flags_out: *mut CVOptionFlags,
    context: *mut c_void,
) -> CVReturn {
    let renderer = unsafe { &*context.cast::<SharedRenderer>() };
    renderer.draw(output_timestamp(output_time));
    0
}

struct SharedRenderer {
    renderer: Mutex<MetalRenderer>,
    active: AtomicBool,
}

// Metal command queues and layers are designed for cross-thread submission. Access to mutable
// renderer state is serialized, and AppKit objects never leave the main thread.
unsafe impl Send for SharedRenderer {}
unsafe impl Sync for SharedRenderer {}

impl SharedRenderer {
    fn new(geometry: Geometry) -> Result<Self, String> {
        Ok(Self {
            renderer: Mutex::new(MetalRenderer::new(geometry)?),
            active: AtomicBool::new(false),
        })
    }

    fn layer(&self) -> *const metal::MetalLayerRef {
        self.renderer.lock().unwrap().layer.as_ref()
    }

    fn handle(&self, event: DictationIndicatorEvent) {
        let mut renderer = self.renderer.lock().unwrap();
        renderer.handle(event);
        self.active.store(true, Ordering::Release);
    }

    fn set_scale(&self, scale: f32) {
        self.renderer.lock().unwrap().set_scale(scale);
    }

    fn set_layout(&self, layout: hud_placement::Layout) {
        if let Some(view) = &mut self.renderer.lock().unwrap().dot_scope {
            view.set_layout(layout);
        }
    }

    fn set_dark(&self, dark: bool) {
        if let Some(view) = &mut self.renderer.lock().unwrap().dot_scope {
            view.set_dark(dark);
        }
    }

    fn draw(&self, output_time: Option<DisplayOutputTime>) {
        if !self.active.load(Ordering::Acquire) {
            return;
        }
        let mut renderer = self.renderer.lock().unwrap();
        if !renderer.draw(output_time) {
            self.active.store(false, Ordering::Release);
        }
    }

    fn is_active(&self) -> bool {
        self.active.load(Ordering::Acquire)
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum Phase {
    Hidden,
    Recording,
    Transcribing,
    Completed,
    Cancelled,
    Failed,
}

fn active_phase(capturing: bool, pending_jobs: usize) -> Option<Phase> {
    if capturing {
        Some(Phase::Recording)
    } else if pending_jobs > 0 {
        Some(Phase::Transcribing)
    } else {
        None
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum JobPhase {
    Queued,
    Transcribing,
    Processing,
}

impl Phase {
    fn visible_duration(self) -> Option<Duration> {
        match self {
            Self::Completed => Some(COMPLETED_VISIBLE_DURATION),
            Self::Cancelled => Some(Duration::ZERO),
            Self::Failed => Some(Duration::from_millis(600)),
            _ => None,
        }
    }
}

#[repr(C, align(8))]
#[derive(Clone, Copy)]
struct Uniforms {
    resolution: [f32; 2],
    time: f32,
    width: f32,
    height: f32,
    opacity: f32,
    scale: f32,
    softness: f32,
    average: f32,
    peak: f32,
    processing: f32,
    post_processing: f32,
    capturing: f32,
    editing: f32,
    queued_count: f32,
    line_style: f32,
    line_count: f32,
    line_curvature: f32,
    line_speed: f32,
    line_sharpness: f32,
    line_glow: f32,
    sphere_depth: f32,
    light_angle: f32,
    sphere_outline: f32,
    completion: f32,
    recording_flash: f32,
    _padding: f32,
}

const _: () = assert!(std::mem::size_of::<Uniforms>() == 112);

fn recording_flash(elapsed: Duration) -> f32 {
    2.0_f32.powf(-elapsed.as_secs_f32() / RECORDING_FLASH_HALF_LIFE.as_secs_f32())
}

fn recording_flash_for(phase: Phase, editing: bool, elapsed: Duration) -> f32 {
    if matches!(phase, Phase::Recording) && !editing {
        recording_flash(elapsed)
    } else {
        0.0
    }
}

struct MetalRenderer {
    layer: MetalLayer,
    command_queue: CommandQueue,
    pipeline: RenderPipelineState,
    phase: Phase,
    capturing: bool,
    editing: bool,
    jobs: BTreeMap<u64, JobPhase>,
    phase_started: Instant,
    last_frame: Instant,
    last_output_seconds: Option<f64>,
    shader_time: f32,
    exiting: bool,
    completion_pending: bool,
    scale: f32,
    target_average: f32,
    target_peak: f32,
    average: Spring,
    peak: Spring,
    width: Spring,
    opacity: Spring,
    visual_scale: Spring,
    softness: Spring,
    processing: Spring,
    post_processing: Spring,
    tuning: HudTuning,
    geometry: Geometry,
    /// Drawn instead of the capsule while the `dot-scope` experiment is on.
    dot_scope: Option<DotScopeView>,
    /// The microphone's loudness for Dot scope, dBFS.
    loudness: f32,
    /// HUD Lab's synthetic voice and how far into it Dot scope is.
    demo_voice: Option<f32>,
}

impl MetalRenderer {
    fn new(geometry: Geometry) -> Result<Self, String> {
        let device = Device::system_default().ok_or("Metal is unavailable")?;
        let layer = MetalLayer::new();
        layer.set_device(&device);
        layer.set_pixel_format(MTLPixelFormat::BGRA8Unorm);
        unsafe {
            let color_space = CGColorSpaceCreateWithName(kCGColorSpaceSRGB);
            if !color_space.is_null() {
                let any_layer = &*(layer.as_ref() as *const metal::MetalLayerRef)
                    .cast::<objc2::runtime::AnyObject>();
                let _: () = objc2::msg_send![any_layer, setColorspace: ColorSpaceRef(color_space)];
                CGColorSpaceRelease(color_space);
            }
        }
        layer.set_opaque(false);
        layer.set_presents_with_transaction(false);
        layer.set_display_sync_enabled(true);
        layer.set_contents_scale(2.0);
        layer.set_drawable_size(CGSize::new(
            f64::from(geometry.width * 2.0),
            f64::from(geometry.height * 2.0),
        ));

        let library = device
            .new_library_with_data(include_bytes!(concat!(
                env!("OUT_DIR"),
                "/dictation_indicator.metallib"
            )))
            .map_err(|error| format!("could not load indicator shader: {error}"))?;
        let vertex = library
            .get_function("indicator_vertex", None)
            .map_err(|error| format!("could not load indicator vertex shader: {error}"))?;
        let fragment = library
            .get_function("indicator_fragment", None)
            .map_err(|error| format!("could not load indicator fragment shader: {error}"))?;
        let descriptor = RenderPipelineDescriptor::new();
        descriptor.set_vertex_function(Some(&vertex));
        descriptor.set_fragment_function(Some(&fragment));
        descriptor
            .color_attachments()
            .object_at(0)
            .ok_or("indicator pipeline has no color attachment")?
            .set_pixel_format(MTLPixelFormat::BGRA8Unorm);
        let pipeline = device
            .new_render_pipeline_state(&descriptor)
            .map_err(|error| format!("could not create indicator pipeline: {error}"))?;
        let dot_scope = if geometry == Geometry::DOT_SCOPE {
            Some(DotScopeView::new(
                &device,
                &library,
                dot_scope::Tuning::load(),
            )?)
        } else {
            None
        };
        let command_queue = device.new_command_queue();
        // Prewarm the command queue so the first keypress never pays driver submission setup.
        let prewarm = command_queue.new_command_buffer();
        prewarm.commit();
        let now = Instant::now();
        Ok(Self {
            command_queue,
            layer,
            pipeline,
            phase: Phase::Hidden,
            capturing: false,
            editing: false,
            jobs: BTreeMap::new(),
            phase_started: now,
            last_frame: now,
            last_output_seconds: None,
            shader_time: 0.0,
            exiting: true,
            completion_pending: false,
            scale: 2.0,
            target_average: 0.0,
            target_peak: 0.0,
            average: Spring::at(0.0),
            peak: Spring::at(0.0),
            width: Spring::at(CAPSULE_WIDTH),
            opacity: Spring::at(0.0),
            visual_scale: Spring::at(HIDDEN_SCALE),
            softness: Spring::at(HIDDEN_SOFTNESS),
            processing: Spring::at(0.0),
            post_processing: Spring::at(0.0),
            tuning: HudTuning::default(),
            geometry,
            dot_scope,
            loudness: SILENT,
            demo_voice: None,
        })
    }

    fn handle(&mut self, event: DictationIndicatorEvent) {
        let now = Instant::now();
        let editing_started = matches!(event, DictationIndicatorEvent::EditingStarted);
        match event {
            DictationIndicatorEvent::Reset => {
                self.capturing = false;
                self.editing = false;
                self.jobs.clear();
                self.phase = Phase::Hidden;
                self.completion_pending = false;
                self.last_output_seconds = None;
                self.loudness = SILENT;
                self.demo_voice = None;
                self.freeze_visual_state();
            }
            DictationIndicatorEvent::Configure(tuning) => {
                self.tuning = tuning;
            }
            DictationIndicatorEvent::ConfigureDotScope(tuning) => {
                if let Some(view) = &mut self.dot_scope {
                    view.tune(tuning);
                }
            }
            DictationIndicatorEvent::DemoVoice => {
                self.demo_voice = Some(0.0);
            }
            DictationIndicatorEvent::Started | DictationIndicatorEvent::EditingStarted => {
                // HUD Lab and the playground send `DemoVoice` after this.
                self.demo_voice = None;
                self.capturing = true;
                self.editing = editing_started;
                self.phase = Phase::Recording;
                self.phase_started = now;
                self.last_frame = now;
                self.last_output_seconds = None;
                self.shader_time = 0.0;
                self.exiting = false;
                self.completion_pending = false;
                self.target_average = 0.0;
                self.target_peak = 0.0;
                let reduce = reduced_motion();
                self.average.jump(0.0);
                self.peak.jump(0.0);
                self.width.jump(CAPSULE_WIDTH);
                self.opacity.jump(0.0);
                self.visual_scale
                    .jump(if reduce { 1.0 } else { HIDDEN_SCALE });
                self.softness
                    .jump(if reduce { 0.0 } else { HIDDEN_SOFTNESS });
                self.processing.jump(0.0);
                self.post_processing.jump(0.0);
                self.loudness = SILENT;
                if let Some(view) = &mut self.dot_scope {
                    view.restart(reduce);
                }
            }
            DictationIndicatorEvent::PromotedToVoiceAction => {
                self.editing = true;
            }
            DictationIndicatorEvent::Meter { average, peak } => {
                self.target_average = (average * 9.0).clamp(0.0, 1.0);
                self.target_peak = (peak * 3.0).clamp(0.0, 1.0);
                self.loudness = dot_scope::loudness(average);
            }
            DictationIndicatorEvent::Submitted { job_id } => {
                self.capturing = false;
                self.editing = false;
                self.jobs.insert(job_id, JobPhase::Queued);
                self.show_pipeline(now);
            }
            DictationIndicatorEvent::Transcribing { job_id } => {
                if let Some(phase) = self.jobs.get_mut(&job_id) {
                    *phase = JobPhase::Transcribing;
                    self.show_pipeline(now);
                }
            }
            DictationIndicatorEvent::Processing { job_id } => {
                if let Some(phase) = self.jobs.get_mut(&job_id) {
                    *phase = JobPhase::Processing;
                    self.show_pipeline(now);
                }
            }
            DictationIndicatorEvent::Discarded => {
                self.finish_capture(now, Phase::Hidden);
            }
            DictationIndicatorEvent::Cancelled => {
                self.finish_capture(now, Phase::Cancelled);
            }
            DictationIndicatorEvent::JobCompleted { job_id } => {
                self.finish_job(job_id, now, Phase::Completed);
            }
            DictationIndicatorEvent::JobCancelled { job_id } => {
                self.finish_job(job_id, now, Phase::Cancelled);
            }
            DictationIndicatorEvent::JobFailed { job_id } => {
                self.finish_job(job_id, now, Phase::Failed);
            }
            DictationIndicatorEvent::Failed => {
                self.finish_capture(now, Phase::Failed);
            }
        }
    }

    fn finish_capture(&mut self, now: Instant, empty_phase: Phase) {
        self.capturing = false;
        self.editing = false;
        if !matches!(empty_phase, Phase::Hidden) {
            self.phase_started = now;
        }
        self.completion_pending = false;
        self.freeze_visual_state();
        if self.jobs.is_empty() {
            self.phase = empty_phase;
        } else {
            self.show_pipeline(now);
        }
    }

    fn show_pipeline(&mut self, now: Instant) {
        let Some(phase) = active_phase(self.capturing, self.jobs.len()) else {
            return;
        };
        self.phase = phase;
        if matches!(phase, Phase::Recording) {
            return;
        }
        self.phase_started = now;
        self.completion_pending = false;
        self.target_average = 0.0;
        self.target_peak = 0.0;
    }

    fn finish_job(&mut self, job_id: u64, now: Instant, terminal: Phase) {
        if self.jobs.remove(&job_id).is_none() {
            return;
        }
        if active_phase(self.capturing, self.jobs.len()).is_some() {
            self.show_pipeline(now);
            return;
        }
        if matches!(terminal, Phase::Completed) {
            self.phase = Phase::Transcribing;
            self.phase_started = now;
            self.completion_pending = true;
        } else {
            self.phase = terminal;
            self.phase_started = now;
            self.completion_pending = false;
            self.freeze_visual_state();
        }
    }

    fn freeze_visual_state(&mut self) {
        self.target_average = self.average.x;
        self.target_peak = self.peak.x;
        self.average.v = 0.0;
        self.peak.v = 0.0;
        self.width.v = 0.0;
        self.processing.v = 0.0;
        self.post_processing.v = 0.0;
    }

    fn set_scale(&mut self, scale: f32) {
        if (self.scale - scale).abs() < f32::EPSILON {
            return;
        }
        self.scale = scale;
        self.layer.set_contents_scale(f64::from(scale));
        self.layer.set_drawable_size(CGSize::new(
            f64::from(self.geometry.width * scale),
            f64::from(self.geometry.height * scale),
        ));
    }

    fn frame_dt(&mut self, now: Instant, output_time: Option<DisplayOutputTime>) -> f32 {
        let dt = if let Some(out) = output_time {
            let delta = self
                .last_output_seconds
                .map(|prev| (out.video_seconds - prev) as f32)
                .filter(|&d| d > 0.0 && d <= 0.25)
                .or(out.refresh_period)
                .unwrap_or(1.0 / 60.0);
            self.last_output_seconds = Some(out.video_seconds);
            delta
        } else {
            now.duration_since(self.last_frame).as_secs_f32()
        };
        self.last_frame = now;
        dt.clamp(1.0 / 240.0, 1.0 / 20.0)
    }

    /// Steps and draws Dot scope; true while it still needs frames.
    fn draw_dot_scope(&mut self, dt: f32, visible: bool, reduce: bool) -> bool {
        let stage = match self.phase {
            Phase::Recording => Some(Stage::Recording),
            Phase::Transcribing => Some(Stage::Processing),
            Phase::Completed => Some(Stage::Done),
            Phase::Failed => Some(Stage::Failed),
            Phase::Hidden | Phase::Cancelled => None,
        };
        let loudness = match &mut self.demo_voice {
            Some(time) => {
                *time += dt;
                dot_scope::demo_loudness(*time)
            }
            None => self.loudness,
        };
        let resolution = [
            self.geometry.width * self.scale,
            self.geometry.height * self.scale,
        ];
        let scale = self.scale;
        let Some(view) = &mut self.dot_scope else {
            return false;
        };
        view.advance(dt, stage, visible, loudness, reduce);
        let needs_frames = visible || !view.settled();
        let Some(drawable) = self.layer.next_drawable() else {
            return needs_frames;
        };
        let descriptor = RenderPassDescriptor::new();
        let attachment = descriptor.color_attachments().object_at(0).unwrap();
        attachment.set_texture(Some(drawable.texture()));
        attachment.set_load_action(MTLLoadAction::Clear);
        attachment.set_clear_color(metal::MTLClearColor::new(0.0, 0.0, 0.0, 0.0));
        attachment.set_store_action(MTLStoreAction::Store);
        let command_buffer = self.command_queue.new_command_buffer();
        let encoder = command_buffer.new_render_command_encoder(descriptor);
        view.encode(encoder, resolution, scale);
        encoder.end_encoding();
        command_buffer.present_drawable(drawable);
        command_buffer.commit();
        needs_frames
    }

    fn draw(&mut self, output_time: Option<DisplayOutputTime>) -> bool {
        let now = Instant::now();
        if self.completion_pending
            && now.duration_since(self.phase_started) >= PROCESSING_MORPH_DURATION
        {
            self.width.jump(CAPSULE_HEIGHT);
            self.processing.jump(1.0);
            self.phase = Phase::Completed;
            self.phase_started = now;
            self.completion_pending = false;
            self.freeze_visual_state();
        }
        let dt = self.frame_dt(now, output_time);
        let reduce = reduced_motion();
        if !reduce {
            self.shader_time += dt;
        }
        let elapsed = now.duration_since(self.phase_started);
        let visible_duration = match self.phase {
            Phase::Completed if self.dot_scope.is_some() => {
                Some(DOT_SCOPE_COMPLETED_VISIBLE_DURATION)
            }
            phase => phase.visible_duration(),
        };
        let terminal_finished = visible_duration.is_some_and(|duration| elapsed >= duration);
        let visible = !matches!(self.phase, Phase::Hidden) && !terminal_finished;
        if self.dot_scope.is_some() {
            return self.draw_dot_scope(dt, visible, reduce);
        }
        if !visible && !self.exiting {
            // Reversing an in-flight entrance velocity would briefly grow or sharpen the HUD.
            self.opacity.v = 0.0;
            self.visual_scale.v = 0.0;
            self.softness.v = 0.0;
            self.exiting = true;
        }
        let (target_width, target_processing) = match self.phase {
            Phase::Recording => (CAPSULE_WIDTH, 0.0),
            Phase::Transcribing => (CAPSULE_HEIGHT, 1.0),
            Phase::Hidden | Phase::Completed | Phase::Cancelled | Phase::Failed => {
                (self.width.x, self.processing.x)
            }
        };
        let target_post_processing = if self
            .jobs
            .first_key_value()
            .is_some_and(|(_, phase)| matches!(phase, JobPhase::Processing))
        {
            1.0
        } else {
            0.0
        };

        let meter_average_spec = Spec::from_omega(22.0);
        let meter_peak_spec = Spec::from_omega(26.0);
        let geometry_spec = Spec::from_omega(GEOMETRY_ANGULAR_FREQUENCY);
        let visibility_frequency = if visible {
            ENTRANCE_ANGULAR_FREQUENCY
        } else {
            EXIT_ANGULAR_FREQUENCY
        };
        let visibility_spec = Spec::from_omega(visibility_frequency);

        self.average
            .step_to(self.target_average, meter_average_spec, dt);
        self.peak.step_to(self.target_peak, meter_peak_spec, dt);
        if reduce {
            self.width.jump(target_width);
            self.processing.jump(target_processing);
            self.post_processing.jump(target_post_processing);
            self.opacity
                .step_to(if visible { 1.0 } else { 0.0 }, REDUCED_FADE, dt);
            self.visual_scale.jump(1.0);
            self.softness.jump(0.0);
        } else {
            self.width.step_to(target_width, geometry_spec, dt);
            self.processing
                .step_to(target_processing, geometry_spec, dt);
            self.post_processing
                .step_to(target_post_processing, geometry_spec, dt);
            self.opacity
                .step_to(if visible { 1.0 } else { 0.0 }, visibility_spec, dt);
            self.visual_scale.step_to(
                if visible { 1.0 } else { HIDDEN_SCALE },
                visibility_spec,
                dt,
            );
            self.softness.step_to(
                if visible { 0.0 } else { HIDDEN_SOFTNESS },
                visibility_spec,
                dt,
            );
        }
        self.target_peak *= 0.91_f32.powf(dt * 60.0);

        let Some(drawable) = self.layer.next_drawable() else {
            return true;
        };
        let descriptor = RenderPassDescriptor::new();
        let attachment = descriptor.color_attachments().object_at(0).unwrap();
        attachment.set_texture(Some(drawable.texture()));
        attachment.set_load_action(MTLLoadAction::Clear);
        attachment.set_clear_color(metal::MTLClearColor::new(0.0, 0.0, 0.0, 0.0));
        attachment.set_store_action(MTLStoreAction::Store);

        let command_buffer = self.command_queue.new_command_buffer();
        let encoder = command_buffer.new_render_command_encoder(descriptor);
        encoder.set_render_pipeline_state(&self.pipeline);
        let uniforms = Uniforms {
            resolution: [
                self.geometry.width * self.scale,
                self.geometry.height * self.scale,
            ],
            time: self.shader_time,
            width: self.width.x,
            height: CAPSULE_HEIGHT,
            opacity: self.opacity.x.clamp(0.0, 1.0),
            scale: self.visual_scale.x.max(0.01),
            softness: self.softness.x.clamp(0.0, HIDDEN_SOFTNESS),
            average: self.average.x.clamp(0.0, 1.0),
            peak: self.peak.x.clamp(0.0, 1.0),
            processing: self.processing.x.clamp(0.0, 1.0),
            post_processing: self.post_processing.x.clamp(0.0, 1.0),
            capturing: if self.capturing { 1.0 } else { 0.0 },
            editing: if self.editing { 1.0 } else { 0.0 },
            queued_count: if self.capturing {
                self.jobs.len() as f32
            } else {
                self.jobs.len().saturating_sub(1) as f32
            },
            line_style: self.tuning.style,
            line_count: self.tuning.line_count,
            line_curvature: self.tuning.curvature,
            line_speed: if reduce { 0.0 } else { self.tuning.speed },
            line_sharpness: self.tuning.sharpness,
            line_glow: self.tuning.glow,
            sphere_depth: self.tuning.depth,
            light_angle: self.tuning.light_angle,
            sphere_outline: self.tuning.outline,
            completion: if matches!(self.phase, Phase::Completed) {
                (elapsed.as_secs_f32() / COMPLETED_VISIBLE_DURATION.as_secs_f32()).clamp(0.0, 1.0)
            } else {
                0.0
            },
            recording_flash: recording_flash_for(self.phase, self.editing, elapsed),
            _padding: 0.0,
        };
        encoder.set_fragment_bytes(
            0,
            std::mem::size_of::<Uniforms>() as u64,
            (&raw const uniforms).cast::<c_void>(),
        );
        encoder.draw_primitives(MTLPrimitiveType::Triangle, 0, 3);
        encoder.end_encoding();
        command_buffer.present_drawable(drawable);
        command_buffer.commit();

        let exit_spec = if reduce {
            REDUCED_FADE
        } else {
            Spec::from_omega(EXIT_ANGULAR_FREQUENCY)
        };
        visible
            || !self.opacity.settled(exit_spec, 0.002)
            || (!reduce && !self.visual_scale.settled(exit_spec, 0.002))
            || (!reduce && !self.softness.settled(exit_spec, 0.02))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn meter_reports_rms_and_peak() {
        let (sender, receiver) = channel();
        sender.meter(&[-0.5, 0.5]);
        let DictationIndicatorEvent::Meter { average, peak } = receiver.recv().unwrap() else {
            panic!("expected meter event");
        };
        assert!((average - 0.5).abs() < f32::EPSILON);
        assert!((peak - 0.5).abs() < f32::EPSILON);
    }

    #[test]
    fn dot_scope_window_fits_the_widest_plate() {
        let widest = dot_scope::KNOBS
            .iter()
            .find(|knob| knob.key == "dot-width")
            .unwrap()
            .max;
        assert!(widest + 24.0 <= Geometry::DOT_SCOPE.width);
    }

    #[test]
    fn entrance_reaches_recording_state_in_about_250ms_without_overshoot() {
        let spec = Spec::from_omega(ENTRANCE_ANGULAR_FREQUENCY);
        let simulate = |frame_rate: usize| {
            let mut opacity = Spring::at(0.0);
            let mut scale = Spring::at(HIDDEN_SCALE);
            let mut softness = Spring::at(HIDDEN_SOFTNESS);
            for _ in 0..frame_rate / 4 {
                let dt = 1.0 / frame_rate as f32;
                opacity.step_to(1.0, spec, dt);
                scale.step_to(1.0, spec, dt);
                softness.step_to(0.0, spec, dt);
                assert!((0.0..=1.0).contains(&opacity.x));
                assert!((HIDDEN_SCALE..=1.0).contains(&scale.x));
                assert!((0.0..=HIDDEN_SOFTNESS).contains(&softness.x));
            }
            (opacity.x, scale.x, softness.x)
        };

        let at_60_hz = simulate(60);
        let at_120_hz = simulate(120);
        assert!((at_60_hz.0 - at_120_hz.0).abs() < 0.000_01);
        assert!((at_60_hz.1 - at_120_hz.1).abs() < 0.000_01);
        assert!((at_60_hz.2 - at_120_hz.2).abs() < 0.000_01);
        assert!(at_60_hz.0 >= 0.98);
        assert!(at_60_hz.1 >= 0.996);
        assert!(at_60_hz.2 <= 0.08);
    }

    #[test]
    fn lifecycle_exit_is_monotonic_and_visually_gone_in_about_250ms() {
        let spec = Spec::from_omega(EXIT_ANGULAR_FREQUENCY);
        let mut opacity = Spring::at(1.0);
        let mut scale = Spring::at(1.0);
        let mut softness = Spring::at(0.0);
        let mut previous = (opacity.x, scale.x, softness.x);

        for _ in 0..15 {
            opacity.step_to(0.0, spec, 1.0 / 60.0);
            scale.step_to(HIDDEN_SCALE, spec, 1.0 / 60.0);
            softness.step_to(HIDDEN_SOFTNESS, spec, 1.0 / 60.0);
            assert!((0.0..=previous.0).contains(&opacity.x));
            assert!((HIDDEN_SCALE..=previous.1).contains(&scale.x));
            assert!((previous.2..=HIDDEN_SOFTNESS).contains(&softness.x));
            previous = (opacity.x, scale.x, softness.x);
        }

        assert!(opacity.x < 0.02);
        assert!((scale.x - HIDDEN_SCALE).abs() < 0.004);
        assert!((softness.x - HIDDEN_SOFTNESS).abs() < 0.08);
    }

    #[test]
    fn completion_has_a_visible_bloom_before_exit() {
        assert_eq!(
            Phase::Completed.visible_duration(),
            Some(Duration::from_millis(240))
        );
    }

    #[test]
    fn recording_flash_starts_bright_and_settles_without_a_second_pulse() {
        let samples =
            [0, 70, 280, 560, 1_120].map(|millis| recording_flash(Duration::from_millis(millis)));

        assert_eq!(samples[0], 1.0);
        assert!(samples.windows(2).all(|pair| pair[0] > pair[1]));
        assert!((samples[2] - 0.5).abs() < 0.000_1);
        assert!((samples[3] - 0.25).abs() < 0.000_1);
        assert!(samples[4] < 0.07);
    }

    #[test]
    fn voice_action_starts_green_without_the_dictation_flash() {
        assert_eq!(
            recording_flash_for(Phase::Recording, true, Duration::ZERO),
            0.0
        );
        assert_eq!(
            recording_flash_for(Phase::Recording, false, Duration::ZERO),
            1.0
        );
    }

    #[test]
    fn rust_uniform_layout_matches_metal() {
        assert_eq!(std::mem::size_of::<Uniforms>(), 112);
        assert_eq!(std::mem::align_of::<Uniforms>(), 8);
        assert_eq!(std::mem::offset_of!(Uniforms, sphere_outline), 92);
        assert_eq!(std::mem::offset_of!(Uniforms, recording_flash), 100);
        assert_eq!(std::mem::offset_of!(Uniforms, _padding), 104);
    }

    #[test]
    fn pending_completion_cannot_replace_a_new_recording_phase() {
        assert_eq!(active_phase(true, 0), Some(Phase::Recording));
        assert_eq!(active_phase(true, 2), Some(Phase::Recording));
        assert_eq!(active_phase(false, 2), Some(Phase::Transcribing));
        assert_eq!(active_phase(false, 0), None);
    }

    #[test]
    fn processing_morph_duration_matches_geometry_spring() {
        let spec = Spec::from_omega(GEOMETRY_ANGULAR_FREQUENCY);
        let mut width = Spring::at(CAPSULE_WIDTH);
        let mut previous_width = width.x;
        for _ in 0..15 {
            width.step_to(CAPSULE_HEIGHT, spec, 1.0 / 60.0);
            assert!((CAPSULE_HEIGHT..=previous_width).contains(&width.x));
            previous_width = width.x;
        }
        assert!(width.x < CAPSULE_HEIGHT + 0.7);
        assert_eq!(PROCESSING_MORPH_DURATION, Duration::from_millis(250));
    }

    #[test]
    fn output_timestamp_extracts_video_seconds_and_refresh_period() {
        use core_video::base::CVSMPTETime;
        let ts = CVTimeStamp {
            version: 0,
            videoTimeScale: 24_000,
            videoTime: 48_000,
            hostTime: 0,
            rateScalar: 1.0,
            videoRefreshPeriod: 200,
            smpteTime: CVSMPTETime {
                subframes: 0,
                subframeDivisor: 0,
                counter: 0,
                type_: 0,
                flags: 0,
                hours: 0,
                minutes: 0,
                seconds: 0,
                frames: 0,
            },
            flags: (CVTimeStampFlags::kCVTimeStampVideoTimeValid as u64)
                | (CVTimeStampFlags::kCVTimeStampVideoRefreshPeriodValid as u64),
            reserved: 0,
        };
        let parsed = output_timestamp(&raw const ts).expect("valid timestamp");
        assert!((parsed.video_seconds - 2.0).abs() < 1e-9);
        assert!((parsed.refresh_period.unwrap() - (1.0 / 120.0)).abs() < 1e-6);
    }
}
