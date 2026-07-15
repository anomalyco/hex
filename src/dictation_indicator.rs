use std::ffi::c_void;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{self, Receiver, Sender};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use core_graphics_types::geometry::CGSize;
use core_video::base::{CVOptionFlags, CVTimeStamp};
use core_video::display_link::{CVDisplayLink, CVDisplayLinkRef};
use core_video::r#return::CVReturn;
use gpui::App;
use metal::{
    CommandQueue, Device, MTLLoadAction, MTLPixelFormat, MTLPrimitiveType, MTLStoreAction,
    MetalLayer, RenderPassDescriptor, RenderPipelineDescriptor, RenderPipelineState,
};
use objc2::MainThreadMarker;
use objc2::rc::Retained;
use objc2_app_kit::{
    NSBackingStoreType, NSColor, NSEvent, NSScreen, NSStatusWindowLevel, NSView, NSWindow,
    NSWindowCollectionBehavior, NSWindowStyleMask,
};
use objc2_foundation::{NSPoint, NSRect, NSSize};
use objc2_quartz_core::CALayer;

const WINDOW_WIDTH: f32 = 112.0;
const WINDOW_HEIGHT: f32 = 64.0;
const CAPSULE_WIDTH: f32 = 56.0;
const CAPSULE_HEIGHT: f32 = 16.0;
const TOP_OFFSET: f64 = 12.0;
const ENTRANCE_ANGULAR_FREQUENCY: f32 = 32.0;
const EXIT_ANGULAR_FREQUENCY: f32 = 30.0;
const GEOMETRY_ANGULAR_FREQUENCY: f32 = 24.0;
const HIDDEN_SCALE: f32 = 0.94;
const HIDDEN_SOFTNESS: f32 = 2.0;

#[derive(Clone, Copy, Debug)]
pub enum DictationIndicatorEvent {
    Started,
    Meter { average: f32, peak: f32 },
    Transcribing,
    Discarded,
    Cancelled,
    Completed,
    Failed,
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

    pub fn handle(&mut self, event: DictationIndicatorEvent, _cx: &mut App) {
        if self.indicator.is_none() && matches!(event, DictationIndicatorEvent::Started) {
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

    pub fn follow_pointer(&mut self, _cx: &mut App) {
        if let Some(indicator) = &mut self.indicator {
            indicator.maintain();
        }
    }
}

struct MetalIndicator {
    window: Retained<NSWindow>,
    renderer: Arc<SharedRenderer>,
    display_link: CVDisplayLink,
    display_link_context: *const SharedRenderer,
    ordered: bool,
}

impl MetalIndicator {
    fn new() -> Result<Self, String> {
        let mtm = MainThreadMarker::new().ok_or("indicator must be created on the main thread")?;
        let renderer = Arc::new(SharedRenderer::new()?);
        let frame = NSRect::new(
            NSPoint::new(0.0, 0.0),
            NSSize::new(f64::from(WINDOW_WIDTH), f64::from(WINDOW_HEIGHT)),
        );
        let window = unsafe {
            NSWindow::initWithContentRect_styleMask_backing_defer(
                mtm.alloc(),
                frame,
                NSWindowStyleMask::Borderless
                    | NSWindowStyleMask::UtilityWindow
                    | NSWindowStyleMask::NonactivatingPanel
                    | NSWindowStyleMask::FullSizeContentView,
                NSBackingStoreType::Buffered,
                false,
            )
        };
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
                | NSWindowCollectionBehavior::FullScreenAuxiliary
                | NSWindowCollectionBehavior::Stationary
                | NSWindowCollectionBehavior::IgnoresCycle,
        );
        unsafe { window.setReleasedWhenClosed(false) };

        let display_link = CVDisplayLink::from_active_cg_displays()
            .map_err(|status| format!("could not create display link: {status}"))?;
        let display_link_context = Arc::into_raw(renderer.clone());
        unsafe {
            display_link.set_output_callback(
                display_link_callback,
                display_link_context.cast_mut().cast::<c_void>(),
            )
        }
        .map_err(|status| format!("could not configure display link: {status}"))?;

        let mut indicator = Self {
            window,
            renderer,
            display_link,
            display_link_context,
            ordered: false,
        };
        indicator.position_on_pointer_screen();
        Ok(indicator)
    }

    fn handle(&mut self, event: DictationIndicatorEvent) {
        self.renderer.handle(event);
        if matches!(event, DictationIndicatorEvent::Started) {
            self.position_on_pointer_screen();
            self.window.orderFrontRegardless();
            self.ordered = true;
            tracing::info!("Metal dictation indicator shown");
        }
        if !self.display_link.is_running()
            && let Err(status) = self.display_link.start()
        {
            tracing::error!(status, "could not start indicator display link");
        }
    }

    fn maintain(&mut self) {
        if self.renderer.is_active() {
            self.position_on_pointer_screen();
            if !self.ordered {
                self.window.orderFrontRegardless();
                self.ordered = true;
            }
        } else {
            if self.display_link.is_running() {
                let _ = self.display_link.stop();
            }
            if self.ordered {
                self.window.orderOut(None);
                self.ordered = false;
            }
        }
    }

    fn position_on_pointer_screen(&mut self) {
        let Some(mtm) = MainThreadMarker::new() else {
            return;
        };
        let pointer = NSEvent::mouseLocation();
        let screens = NSScreen::screens(mtm);
        let Some(screen) = screens.iter().find(|screen| {
            let frame = screen.frame();
            pointer.x >= frame.origin.x
                && pointer.x < frame.origin.x + frame.size.width
                && pointer.y >= frame.origin.y
                && pointer.y < frame.origin.y + frame.size.height
        }) else {
            return;
        };
        let visible = screen.visibleFrame();
        let capsule_margin = f64::from((WINDOW_HEIGHT - CAPSULE_HEIGHT) / 2.0);
        let top_left = NSPoint::new(
            visible.origin.x + (visible.size.width - f64::from(WINDOW_WIDTH)) / 2.0,
            visible.origin.y + visible.size.height + capsule_margin - TOP_OFFSET,
        );
        let frame = self.window.frame();
        if (frame.origin.x - top_left.x).abs() > 0.5
            || (frame.origin.y + frame.size.height - top_left.y).abs() > 0.5
        {
            self.window.setFrameTopLeftPoint(top_left);
        }
        self.renderer.set_scale(screen.backingScaleFactor() as f32);
    }
}

impl Drop for MetalIndicator {
    fn drop(&mut self) {
        let _ = self.display_link.stop();
        unsafe { drop(Arc::from_raw(self.display_link_context)) };
        self.window.close();
    }
}

extern "C" fn display_link_callback(
    _display_link: CVDisplayLinkRef,
    _now: *const CVTimeStamp,
    _output_time: *const CVTimeStamp,
    _flags_in: CVOptionFlags,
    _flags_out: *mut CVOptionFlags,
    context: *mut c_void,
) -> CVReturn {
    let renderer = unsafe { &*context.cast::<SharedRenderer>() };
    renderer.draw();
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
    fn new() -> Result<Self, String> {
        Ok(Self {
            renderer: Mutex::new(MetalRenderer::new()?),
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

    fn draw(&self) {
        if !self.active.load(Ordering::Acquire) {
            return;
        }
        let mut renderer = self.renderer.lock().unwrap();
        if !renderer.draw() {
            self.active.store(false, Ordering::Release);
        }
    }

    fn is_active(&self) -> bool {
        self.active.load(Ordering::Acquire)
    }
}

#[derive(Clone, Copy)]
enum Phase {
    Hidden,
    Recording,
    Transcribing,
    Completed,
    Cancelled,
    Failed,
}

impl Phase {
    fn visible_duration(self) -> Option<Duration> {
        match self {
            Self::Completed | Self::Cancelled => Some(Duration::ZERO),
            Self::Failed => Some(Duration::from_millis(600)),
            _ => None,
        }
    }
}

#[repr(C)]
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
}

struct MetalRenderer {
    layer: MetalLayer,
    command_queue: CommandQueue,
    pipeline: RenderPipelineState,
    phase: Phase,
    phase_started: Instant,
    render_started: Instant,
    last_frame: Instant,
    exiting: bool,
    scale: f32,
    target_average: f32,
    target_peak: f32,
    average: Spring,
    peak: Spring,
    width: Spring,
    height: Spring,
    opacity: Spring,
    visual_scale: Spring,
    softness: Spring,
    processing: Spring,
}

impl MetalRenderer {
    fn new() -> Result<Self, String> {
        let device = Device::system_default().ok_or("Metal is unavailable")?;
        let layer = MetalLayer::new();
        layer.set_device(&device);
        layer.set_pixel_format(MTLPixelFormat::BGRA8Unorm);
        layer.set_opaque(false);
        layer.set_presents_with_transaction(false);
        layer.set_display_sync_enabled(true);
        layer.set_contents_scale(2.0);
        layer.set_drawable_size(CGSize::new(
            f64::from(WINDOW_WIDTH * 2.0),
            f64::from(WINDOW_HEIGHT * 2.0),
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
        let now = Instant::now();
        Ok(Self {
            command_queue: device.new_command_queue(),
            layer,
            pipeline,
            phase: Phase::Hidden,
            phase_started: now,
            render_started: now,
            last_frame: now,
            exiting: true,
            scale: 2.0,
            target_average: 0.0,
            target_peak: 0.0,
            average: Spring::new(0.0),
            peak: Spring::new(0.0),
            width: Spring::new(CAPSULE_WIDTH),
            height: Spring::new(CAPSULE_HEIGHT),
            opacity: Spring::new(0.0),
            visual_scale: Spring::new(HIDDEN_SCALE),
            softness: Spring::new(HIDDEN_SOFTNESS),
            processing: Spring::new(0.0),
        })
    }

    fn handle(&mut self, event: DictationIndicatorEvent) {
        let now = Instant::now();
        match event {
            DictationIndicatorEvent::Started => {
                self.phase = Phase::Recording;
                self.phase_started = now;
                self.render_started = now;
                self.last_frame = now;
                self.exiting = false;
                self.target_average = 0.0;
                self.target_peak = 0.0;
                self.average.reset(0.0);
                self.peak.reset(0.0);
                self.width.reset(CAPSULE_WIDTH);
                self.height.reset(CAPSULE_HEIGHT);
                self.opacity.reset(0.0);
                self.visual_scale.reset(HIDDEN_SCALE);
                self.softness.reset(HIDDEN_SOFTNESS);
                self.processing.reset(0.0);
            }
            DictationIndicatorEvent::Meter { average, peak } => {
                self.target_average = (average * 7.0).clamp(0.0, 1.0);
                self.target_peak = (peak * 2.5).clamp(0.0, 1.0);
            }
            DictationIndicatorEvent::Transcribing => {
                self.phase = Phase::Transcribing;
                self.phase_started = now;
                self.target_average = 0.0;
                self.target_peak = 0.0;
            }
            DictationIndicatorEvent::Discarded => {
                self.phase = Phase::Hidden;
                self.freeze_visual_state();
            }
            DictationIndicatorEvent::Cancelled => {
                self.phase = Phase::Cancelled;
                self.phase_started = now;
                self.freeze_visual_state();
            }
            DictationIndicatorEvent::Completed => {
                if matches!(self.phase, Phase::Transcribing) {
                    self.phase = Phase::Completed;
                    self.phase_started = now;
                    self.freeze_visual_state();
                }
            }
            DictationIndicatorEvent::Failed => {
                self.phase = Phase::Failed;
                self.phase_started = now;
                self.freeze_visual_state();
            }
        }
    }

    fn freeze_visual_state(&mut self) {
        self.target_average = self.average.value;
        self.target_peak = self.peak.value;
        self.average.velocity = 0.0;
        self.peak.velocity = 0.0;
        self.width.velocity = 0.0;
        self.height.velocity = 0.0;
        self.processing.velocity = 0.0;
    }

    fn set_scale(&mut self, scale: f32) {
        if (self.scale - scale).abs() < f32::EPSILON {
            return;
        }
        self.scale = scale;
        self.layer.set_contents_scale(f64::from(scale));
        self.layer.set_drawable_size(CGSize::new(
            f64::from(WINDOW_WIDTH * scale),
            f64::from(WINDOW_HEIGHT * scale),
        ));
    }

    fn draw(&mut self) -> bool {
        let now = Instant::now();
        let dt = now
            .duration_since(self.last_frame)
            .as_secs_f32()
            .min(1.0 / 20.0);
        self.last_frame = now;
        let elapsed = now.duration_since(self.phase_started);
        let terminal_finished = self
            .phase
            .visible_duration()
            .is_some_and(|duration| elapsed >= duration);
        let visible = !matches!(self.phase, Phase::Hidden) && !terminal_finished;
        if !visible && !self.exiting {
            // Reversing an in-flight entrance velocity would briefly grow or sharpen the HUD.
            self.opacity.velocity = 0.0;
            self.visual_scale.velocity = 0.0;
            self.softness.velocity = 0.0;
            self.exiting = true;
        }
        let target_width = match self.phase {
            Phase::Recording => CAPSULE_WIDTH,
            Phase::Transcribing => CAPSULE_HEIGHT,
            Phase::Hidden | Phase::Completed | Phase::Cancelled | Phase::Failed => self.width.value,
        };
        let target_height = CAPSULE_HEIGHT;
        let target_processing = match self.phase {
            Phase::Recording => 0.0,
            Phase::Transcribing => 1.0,
            Phase::Hidden | Phase::Completed | Phase::Cancelled | Phase::Failed => {
                self.processing.value
            }
        };

        self.average.step_critical(self.target_average, dt, 16.0);
        self.peak.step_critical(self.target_peak, dt, 19.0);
        self.width
            .step_critical(target_width, dt, GEOMETRY_ANGULAR_FREQUENCY);
        self.height
            .step_critical(target_height, dt, GEOMETRY_ANGULAR_FREQUENCY);
        self.processing
            .step_critical(target_processing, dt, GEOMETRY_ANGULAR_FREQUENCY);
        let visibility_frequency = if visible {
            ENTRANCE_ANGULAR_FREQUENCY
        } else {
            EXIT_ANGULAR_FREQUENCY
        };
        self.opacity
            .step_critical(if visible { 1.0 } else { 0.0 }, dt, visibility_frequency);
        self.visual_scale.step_critical(
            if visible { 1.0 } else { HIDDEN_SCALE },
            dt,
            visibility_frequency,
        );
        self.softness.step_critical(
            if visible { 0.0 } else { HIDDEN_SOFTNESS },
            dt,
            visibility_frequency,
        );
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
            resolution: [WINDOW_WIDTH * self.scale, WINDOW_HEIGHT * self.scale],
            time: now.duration_since(self.render_started).as_secs_f32(),
            width: self.width.value,
            height: self.height.value,
            opacity: self.opacity.value.clamp(0.0, 1.0),
            scale: self.visual_scale.value.max(0.01),
            softness: self.softness.value.clamp(0.0, HIDDEN_SOFTNESS),
            average: self.average.value.clamp(0.0, 1.0),
            peak: self.peak.value.clamp(0.0, 1.0),
            processing: self.processing.value.clamp(0.0, 1.0),
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

        visible
            || !self.opacity.is_settled(0.0, 0.002, EXIT_ANGULAR_FREQUENCY)
            || !self
                .visual_scale
                .is_settled(HIDDEN_SCALE, 0.002, EXIT_ANGULAR_FREQUENCY)
            || !self
                .softness
                .is_settled(HIDDEN_SOFTNESS, 0.02, EXIT_ANGULAR_FREQUENCY)
    }
}

struct Spring {
    value: f32,
    velocity: f32,
}

impl Spring {
    fn new(value: f32) -> Self {
        Self {
            value,
            velocity: 0.0,
        }
    }

    fn reset(&mut self, value: f32) {
        self.value = value;
        self.velocity = 0.0;
    }

    fn step_critical(&mut self, target: f32, dt: f32, angular_frequency: f32) {
        let displacement = self.value - target;
        let coefficient = self.velocity + angular_frequency * displacement;
        let decay = (-angular_frequency * dt).exp();
        self.value = target + (displacement + coefficient * dt) * decay;
        self.velocity = (self.velocity - angular_frequency * coefficient * dt) * decay;
    }

    fn is_settled(&self, target: f32, value_epsilon: f32, angular_frequency: f32) -> bool {
        (self.value - target).abs() <= value_epsilon
            && self.velocity.abs() <= value_epsilon * angular_frequency
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
    fn entrance_reaches_recording_state_in_about_200ms_without_overshoot() {
        let simulate = |frame_rate: usize| {
            let mut opacity = Spring::new(0.0);
            let mut scale = Spring::new(HIDDEN_SCALE);
            let mut softness = Spring::new(HIDDEN_SOFTNESS);
            for _ in 0..frame_rate / 5 {
                let dt = 1.0 / frame_rate as f32;
                opacity.step_critical(1.0, dt, ENTRANCE_ANGULAR_FREQUENCY);
                scale.step_critical(1.0, dt, ENTRANCE_ANGULAR_FREQUENCY);
                softness.step_critical(0.0, dt, ENTRANCE_ANGULAR_FREQUENCY);
                assert!((0.0..=1.0).contains(&opacity.value));
                assert!((HIDDEN_SCALE..=1.0).contains(&scale.value));
                assert!((0.0..=HIDDEN_SOFTNESS).contains(&softness.value));
            }
            (opacity.value, scale.value, softness.value)
        };

        let at_60_hz = simulate(60);
        let at_120_hz = simulate(120);
        assert!((at_60_hz.0 - at_120_hz.0).abs() < 0.000_01);
        assert!((at_60_hz.1 - at_120_hz.1).abs() < 0.000_01);
        assert!((at_60_hz.2 - at_120_hz.2).abs() < 0.000_01);
        assert!(at_60_hz.0 >= 0.985);
        assert!(at_60_hz.1 >= 0.998);
        assert!(at_60_hz.2 <= 0.04);
    }

    #[test]
    fn lifecycle_exit_is_monotonic_and_visually_gone_in_about_200ms() {
        let mut opacity = Spring::new(1.0);
        let mut scale = Spring::new(1.0);
        let mut softness = Spring::new(0.0);
        let mut previous = (opacity.value, scale.value, softness.value);

        for _ in 0..12 {
            opacity.step_critical(0.0, 1.0 / 60.0, EXIT_ANGULAR_FREQUENCY);
            scale.step_critical(HIDDEN_SCALE, 1.0 / 60.0, EXIT_ANGULAR_FREQUENCY);
            softness.step_critical(HIDDEN_SOFTNESS, 1.0 / 60.0, EXIT_ANGULAR_FREQUENCY);
            assert!((0.0..=previous.0).contains(&opacity.value));
            assert!((HIDDEN_SCALE..=previous.1).contains(&scale.value));
            assert!((previous.2..=HIDDEN_SOFTNESS).contains(&softness.value));
            previous = (opacity.value, scale.value, softness.value);
        }

        assert!(opacity.value < 0.02);
        assert!((scale.value - HIDDEN_SCALE).abs() < 0.002);
        assert!((softness.value - HIDDEN_SOFTNESS).abs() < 0.04);
    }

    #[test]
    fn processing_morph_only_contracts_the_capsule() {
        let mut width = Spring::new(CAPSULE_WIDTH);
        let mut height = Spring::new(CAPSULE_HEIGHT);
        let mut previous_width = width.value;

        for _ in 0..15 {
            width.step_critical(CAPSULE_HEIGHT, 1.0 / 60.0, GEOMETRY_ANGULAR_FREQUENCY);
            height.step_critical(CAPSULE_HEIGHT, 1.0 / 60.0, GEOMETRY_ANGULAR_FREQUENCY);
            assert!((CAPSULE_HEIGHT..=previous_width).contains(&width.value));
            assert_eq!(height.value, CAPSULE_HEIGHT);
            previous_width = width.value;
        }

        assert!(width.value < CAPSULE_HEIGHT + 0.7);
    }
}
