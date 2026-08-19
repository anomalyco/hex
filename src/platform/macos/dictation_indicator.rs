use std::ffi::c_void;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{self, Receiver, Sender};
use std::sync::{Arc, Mutex};
use std::time::Instant;

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

use crate::desktop::indicator_model::{
    CAPSULE_HEIGHT, IndicatorModel, TOP_OFFSET, WINDOW_HEIGHT, WINDOW_WIDTH,
};

pub use crate::desktop::indicator_model::{DictationIndicatorEvent, HudTuning};

#[derive(Clone)]
pub struct DictationIndicatorSender(Sender<DictationIndicatorEvent>);

impl DictationIndicatorSender {
    pub fn send(&self, event: DictationIndicatorEvent) {
        let _ = self.0.send(event);
    }

    pub fn meter(&self, samples: &[f32]) {
        if let Some(event) = DictationIndicatorEvent::meter(samples) {
            self.send(event);
        }
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
        if matches!(
            event,
            DictationIndicatorEvent::Started | DictationIndicatorEvent::EditingStarted
        ) {
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

struct MetalRenderer {
    layer: MetalLayer,
    command_queue: CommandQueue,
    pipeline: RenderPipelineState,
    scale: f32,
    model: IndicatorModel,
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
        Ok(Self {
            command_queue: device.new_command_queue(),
            layer,
            pipeline,
            scale: 2.0,
            model: IndicatorModel::new(),
        })
    }

    fn handle(&mut self, event: DictationIndicatorEvent) {
        self.model.handle(event);
    }

    fn set_scale(&mut self, scale: f32) {
        if (self.scale - scale).abs() < f32::EPSILON {
            return;
        }
        self.scale = scale;
        self.model.set_backing_scale(scale);
        self.layer.set_contents_scale(f64::from(scale));
        self.layer.set_drawable_size(CGSize::new(
            f64::from(WINDOW_WIDTH * scale),
            f64::from(WINDOW_HEIGHT * scale),
        ));
    }

    fn draw(&mut self) -> bool {
        let frame = self.model.frame(Instant::now());

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
        encoder.set_fragment_bytes(
            0,
            std::mem::size_of_val(&frame.uniforms) as u64,
            std::ptr::from_ref(&frame.uniforms).cast::<c_void>(),
        );
        encoder.draw_primitives(MTLPrimitiveType::Triangle, 0, 3);
        encoder.end_encoding();
        command_buffer.present_drawable(drawable);
        command_buffer.commit();

        frame.visible
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
}
