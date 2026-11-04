//! The dictation HUD on every OS: a click-through Rune overlay whose
//! compositor steps `dictation_indicator`'s model on Rune's frame clock (the
//! display's refresh on macOS) and draws Dot scope on the GPU with
//! `rune_hud.wgsl`. The overlay never takes
//! focus or a click: a nonactivating panel on macOS, a layer-shell surface on
//! Wayland, an override-redirect window on X11, and a topmost tool window
//! that never activates on Windows.
//!
//! Listeners report through [`HudSender`], which never waits: each event is
//! an overlay update, applied on the event thread before the next frame.
//!
//! The Sounds sheet draws its preview of the HUD with the same [`Painter`].

#[cfg_attr(not(target_os = "macos"), allow(dead_code))]
pub(crate) mod dot_scope;
#[cfg_attr(not(target_os = "macos"), allow(dead_code))]
pub(crate) mod hud_placement;
#[cfg(target_os = "macos")]
mod macos;
pub(crate) mod spring;

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use rune::compose::{Compositor, Frame};
use rune::{Cx, El, LiveRegion, OverlayHandle, OverlayOptions, OverlaySpec, ScreenEdge, el};

use crate::app_settings::IndicatorPosition;
use crate::desktop::i18n::tr;
use crate::dictation::indicator::{DictationIndicatorEvent, DotFrame, IndicatorModel};
use crate::rune_hud::dot_scope::MAX_DOTS;
#[cfg(not(target_os = "macos"))]
use crate::rune_hud::dot_scope::PLATE_HEIGHT;
use crate::rune_hud::hud_placement::{ISLAND_INSET, ISLAND_PAD, Island, Layout};
use crate::rune_ui::live_text::Spot;

#[cfg(target_os = "macos")]
pub(crate) use macos::DictationIndicatorUi;

pub(crate) type HudEvent = DictationIndicatorEvent;

/// The window's margin from the top or bottom edge on Linux and Windows.
pub(crate) const TOP_OFFSET: f32 = 12.0;
pub(crate) const BOTTOM_OFFSET: f32 = 24.0;
/// The HUD window: room for Dot scope's widest plate and drop shadow, and
/// on macOS for an island grown out of the widest notch.
#[cfg(target_os = "macos")]
pub(crate) const WINDOW: (f32, f32) = (320.0, 128.0);
#[cfg(not(target_os = "macos"))]
pub(crate) const WINDOW: (f32, f32) = (304.0, 80.0);
/// The notch's own bottom corner radius, which the island starts from.
const NOTCH_RADIUS: f32 = 8.0;
/// The open island's bottom corner radius.
const ISLAND_RADIUS: f32 = 18.0;
/// The notch indicator's bottom corner radius once its ears are out.
const EARS_RADIUS: f32 = 14.0;

/// The live text's gap from the HUD's plate.
const LIVE_TEXT_GAP: f32 = 6.0;

/// Where the live text starts, as of the HUD's last show.
static LIVE_TEXT: Mutex<Option<Spot>> = Mutex::new(None);

/// Where the live text plate starts: just past the HUD's plate, on its
/// screen; with the HUD hidden, where its plate would be.
pub(crate) fn live_text_place() -> Spot {
    let spot = *LIVE_TEXT.lock().unwrap_or_else(|error| error.into_inner());
    spot.unwrap_or(Spot::Edge(ScreenEdge::Top, 40.0))
}

fn set_live_text(spot: Spot) {
    *LIVE_TEXT.lock().unwrap_or_else(|error| error.into_inner()) = Some(spot);
}

/// How a listener reports to the HUD; sending never waits.
#[derive(Clone)]
pub(crate) struct HudSender {
    overlay: OverlayHandle<Hud>,
    #[cfg(target_os = "macos")]
    animating: Arc<AtomicBool>,
    #[cfg(target_os = "windows")]
    watch: Arc<Mutex<Watch>>,
}

/// What Windows checks of the HUD: Rune can show the overlay without
/// Windows ever drawing it (#510), so a show must also present a frame.
#[cfg(target_os = "windows")]
#[derive(Default)]
struct Watch {
    position: IndicatorPosition,
    /// The overlay failure last logged, so each is logged once.
    logged: Option<String>,
    described: bool,
    /// A dictation's start awaiting the HUD's first frame: when, and the
    /// frames presented before it.
    pending: Option<(Instant, u64)>,
    /// The last start presented nothing.
    unpresented: Option<String>,
}

/// How long a started HUD has to present its first frame.
#[cfg(target_os = "windows")]
const FIRST_FRAME: std::time::Duration = std::time::Duration::from_millis(1500);

impl HudSender {
    pub(crate) fn send(&self, event: HudEvent) {
        #[cfg(target_os = "windows")]
        if matches!(event, HudEvent::Started) {
            let presents = self.overlay.surface_stats().totals.presents;
            let mut watch = self.watch.lock().unwrap_or_else(|error| error.into_inner());
            if watch.position != IndicatorPosition::Hidden && watch.pending.is_none() {
                watch.pending = Some((Instant::now(), presents));
            }
        }
        self.overlay.update(move |hud, _| hud.handle(event));
    }

    #[cfg(not(target_os = "macos"))]
    pub(crate) fn meter(&self, samples: &[f32]) {
        if let Some(event) = DictationIndicatorEvent::meter(samples) {
            self.send(event);
        }
    }

    #[cfg(not(target_os = "macos"))]
    pub(crate) fn set_position(&self, position: IndicatorPosition) {
        #[cfg(target_os = "windows")]
        {
            self.watch
                .lock()
                .unwrap_or_else(|error| error.into_inner())
                .position = position;
        }
        self.overlay
            .update(move |hud, _| hud.set_position(position));
    }

    /// Why Windows could not show the HUD, for Settings: Rune's reason, or a
    /// started HUD that presented no frame. Logged as an error the first time
    /// each reason is seen; what draws the HUD is logged once it exists.
    #[cfg(target_os = "windows")]
    pub(crate) fn failure(&self) -> Option<String> {
        let mut watch = self.watch.lock().unwrap_or_else(|error| error.into_inner());
        let surface = self.overlay.surface_description();
        if !watch.described
            && let Some(surface) = &surface
        {
            tracing::info!(%surface, "dictation HUD surface");
            watch.described = true;
        }
        if let Some((at, before)) = watch.pending
            && at.elapsed() >= FIRST_FRAME
        {
            watch.pending = None;
            let presents = self.overlay.surface_stats().totals.presents;
            watch.unpresented = (presents == before).then(|| {
                format!(
                    "shown but presented no frame in {} ms; surface: {}",
                    FIRST_FRAME.as_millis(),
                    surface.as_deref().unwrap_or("none")
                )
            });
        }
        let reason = self
            .overlay
            .failure()
            .or_else(|| watch.unpresented.clone())?;
        if watch.logged.as_deref() != Some(reason.as_str()) {
            tracing::error!(%reason, "dictation HUD can't show");
            watch.logged = Some(reason.clone());
        }
        Some(format!(
            "The recording HUD can't show ({reason}). Dictation still works."
        ))
    }

    /// The overlay, to ask whether the platform could show it.
    #[cfg(target_os = "linux")]
    pub(crate) fn availability(&self) -> rune::Availability {
        self.overlay.availability()
    }

    /// Why Rune could not open the overlay, when it says.
    #[cfg(target_os = "linux")]
    pub(crate) fn failure(&self) -> Option<String> {
        self.overlay.failure()
    }
}

/// The overlay's state: the model and what it last drew.
pub(crate) struct Hud {
    model: IndicatorModel,
    drawn: Option<DotFrame>,
    /// Where the plate sits in the window (macOS placements).
    layout: Layout,
    position: IndicatorPosition,
    /// The overlay itself, to show and hide it.
    this: Option<OverlayHandle<Hud>>,
    shown: bool,
    /// Rune's Reduce Motion, as of the last frame.
    reduce: bool,
    /// Whether the compositor wants the display's frames.
    animating: Arc<AtomicBool>,
    #[cfg(target_os = "macos")]
    placed: Option<crate::rune_hud::hud_placement::Placed>,
    /// The position the overlay was last placed for, while it stays on that
    /// display; a capture's start or a hide picks the display again.
    #[cfg(not(target_os = "macos"))]
    edge_placed: Option<IndicatorPosition>,
    /// When the HUD last checked which display the pointer is on.
    #[cfg(not(target_os = "macos"))]
    followed: Option<Instant>,
}

/// How often a shown HUD checks which display the pointer is on (#807).
#[cfg(not(target_os = "macos"))]
const FOLLOW_EVERY: std::time::Duration = std::time::Duration::from_millis(250);

impl Hud {
    /// While shown, every `FOLLOW_EVERY`: the HUD and the live text move to
    /// the display under the pointer when it moved to another, at once and
    /// without focus. Windows and X11; a layer-shell surface stays on the
    /// compositor's focused output, and GNOME and KDE on Wayland on the
    /// primary.
    #[cfg(not(target_os = "macos"))]
    fn follow_pointer(&mut self, now: Instant) {
        if !self.shown || self.followed.is_some_and(|at| now < at + FOLLOW_EVERY) {
            return;
        }
        self.followed = Some(now);
        if let Some(overlay) = &self.this {
            overlay.refollow_pointer();
        }
        crate::rune_ui::live_text::refollow_pointer();
    }

    fn handle(&mut self, event: HudEvent) {
        // A capture's start sends the display and placement to Rune again,
        // so a placement Rune resolved wrong on another screen (#873)
        // doesn't stick.
        #[cfg(target_os = "macos")]
        if let HudEvent::ConfigureDotScope(_) | HudEvent::Started = event {
            self.placed = None;
        }
        #[cfg(not(target_os = "macos"))]
        if let HudEvent::Started = event {
            self.edge_placed = None;
        }
        self.model.handle(event, Instant::now(), self.reduce);
        self.animating.store(true, Ordering::Release);
        let starts = matches!(event, HudEvent::Started);
        if starts || (self.shown && !matches!(event, HudEvent::Meter { .. })) {
            self.show();
        }
    }

    #[cfg(not(target_os = "macos"))]
    fn set_position(&mut self, position: IndicatorPosition) {
        self.position = position;
        if self.shown {
            self.show();
        }
    }

    /// Shows the overlay where the HUD belongs, unless it is set to Hidden.
    fn show(&mut self) {
        let Some(overlay) = self.this.clone() else {
            return;
        };
        #[cfg(target_os = "macos")]
        {
            self.position = crate::app_settings::indicator_position();
        }
        if self.position == IndicatorPosition::Hidden {
            #[cfg(target_os = "macos")]
            self.place_live_text_without_hud();
            #[cfg(not(target_os = "macos"))]
            {
                let plate_start = TOP_OFFSET + (WINDOW.1 - PLATE_HEIGHT) / 2.0;
                set_live_text(Spot::Edge(ScreenEdge::Top, plate_start));
            }
            self.hide();
            return;
        }
        #[cfg(target_os = "macos")]
        self.place_on_pointer_screen(&overlay);
        #[cfg(not(target_os = "macos"))]
        {
            let (edge, margin) = match self.position {
                IndicatorPosition::Bottom => (ScreenEdge::Bottom, BOTTOM_OFFSET),
                _ => (ScreenEdge::Top, TOP_OFFSET),
            };
            // Placing picks the display again (`display`); while shown, the
            // HUD follows the pointer to another display at its own pace
            // (`follow_pointer`).
            if self.edge_placed != Some(self.position) {
                overlay.place(edge, margin);
                self.edge_placed = Some(self.position);
            }
            // The plate sits in the middle of the window.
            let past_plate = margin + (WINDOW.1 + PLATE_HEIGHT) / 2.0 + LIVE_TEXT_GAP;
            set_live_text(Spot::Edge(edge, past_plate));
        }
        if !self.shown {
            overlay.show();
            self.shown = true;
        }
    }

    fn hide(&mut self) {
        #[cfg(not(target_os = "macos"))]
        {
            self.edge_placed = None;
        }
        #[cfg(target_os = "macos")]
        {
            self.placed = None;
        }
        if std::mem::take(&mut self.shown)
            && let Some(overlay) = &self.this
        {
            overlay.hide();
        }
    }
}

/// How long the hidden HUD keeps its GPU surface: the device it holds costs
/// about 150 MB and the GPU driver's threads waking some 65 times a second.
/// Windows gives it back, window included, as soon as the HUD hides: a hidden
/// topmost window that presented with DXGI let NVIDIA's Instant Replay take
/// Hex for the running game (#1308).
#[cfg(not(target_os = "windows"))]
const GPU_IDLE: Duration = Duration::from_secs(5 * 60);
#[cfg(target_os = "windows")]
const GPU_IDLE: Duration = Duration::ZERO;

/// The format the HUD draws in on Windows and Linux (Rune drops the
/// surface's sRGB suffix for `srgb_blend`).
const HUD_FORMAT: wgpu::TextureFormat = wgpu::TextureFormat::Bgra8Unorm;

/// A painter made ahead of the HUD's first frame on a device `prepare_gpu`
/// was given.
static PREBUILT: Mutex<Option<(wgpu::Device, Painter)>> = Mutex::new(None);

/// Makes the HUD's pipeline on `device` off the UI thread, for the HUD's next
/// first frame: compiling it there takes 75 to 115 ms on Windows. Unclaimed,
/// it goes after a while, since it keeps the device alive.
#[cfg_attr(not(target_os = "windows"), allow(dead_code))]
pub(crate) fn prepare_gpu(device: &wgpu::Device) {
    let lock = || PREBUILT.lock().unwrap_or_else(|error| error.into_inner());
    if lock().as_ref().is_some_and(|(made, _)| made == device) {
        return;
    }
    let painter = Painter::new(device, HUD_FORMAT);
    *lock() = Some((device.clone(), painter));
    let device = device.clone();
    let _ = std::thread::Builder::new()
        .name("hud-prepared".into())
        .spawn(move || {
            std::thread::sleep(Duration::from_secs(70));
            let mut slot = lock();
            if slot.as_ref().is_some_and(|(made, _)| *made == device) {
                slot.take();
            }
        });
}

/// `prepare_gpu`'s painter, when it was made on `device` for `format`.
fn prebuilt(device: &wgpu::Device, format: wgpu::TextureFormat) -> Option<Painter> {
    let mut slot = PREBUILT.lock().unwrap_or_else(|error| error.into_inner());
    slot.as_ref()
        .is_some_and(|(made, painter)| made == device && painter.format == format)
        .then(|| slot.take().map(|(_, painter)| painter))
        .flatten()
}

/// Adds the HUD's overlay to the app; send its events through the returned
/// sender.
pub(crate) fn install<S: 'static>(app: &mut rune::App<S>) -> HudSender {
    let animating = Arc::new(AtomicBool::new(false));
    let now = Instant::now();
    let model = IndicatorModel::new(dot_scope::lab(), now);
    let overlay = app.overlay(
        OverlaySpec {
            name: "hex-hud",
            size: WINDOW,
            edge: ScreenEdge::Top,
            margin: 0.0,
            align: rune::Align::Center,
            inset: 0.0,
            interactive: false,
            view,
        }
        .options(OverlayOptions {
            // The first press never waits on the window: macOS keeps the
            // panel drawn at alpha zero; elsewhere only the window waits,
            // and its GPU surface comes with the show (`release_gpu_after`).
            prewarm: true,
            #[cfg(not(target_os = "macos"))]
            display: display(),
            ..OverlayOptions::default()
        })
        // The shaders write display (sRGB-encoded) values.
        .srgb_blend(true)
        // On Linux a burst of dictations keeps the GPU surface, so each HUD
        // shows at once; a Hex at rest gives the GPU device back.
        .release_gpu_after(GPU_IDLE)
        .compositor(HudCompositor {
            gpu: None,
            animating: animating.clone(),
        }),
        Hud {
            model,
            drawn: None,
            layout: Layout::default(),
            position: IndicatorPosition::Top,
            this: None,
            shown: false,
            reduce: false,
            animating: animating.clone(),
            #[cfg(target_os = "macos")]
            placed: None,
            #[cfg(not(target_os = "macos"))]
            edge_placed: None,
            #[cfg(not(target_os = "macos"))]
            followed: None,
        },
    );
    let this = overlay.clone();
    overlay.update(move |hud, _| hud.this = Some(this));
    HudSender {
        overlay,
        #[cfg(target_os = "macos")]
        animating,
        #[cfg(target_os = "windows")]
        watch: Arc::default(),
    }
}

/// The display the HUD and live text appear on, picked at each capture's
/// start: the one under the pointer, as on macOS (#486). Windows and X11 read
/// the pointer and pick its monitor; a layer-shell surface pins no output, so
/// the compositor puts it on the focused one. GNOME (whose HUD runs on
/// XWayland, blind to the pointer over Wayland windows) and KDE keep the
/// primary display on Wayland; their X11 sessions follow the pointer (#1200).
#[cfg(not(target_os = "macos"))]
pub(crate) fn display() -> rune::DisplayTarget {
    #[cfg(target_os = "linux")]
    if keeps_primary_display(
        std::env::var("XDG_CURRENT_DESKTOP").ok().as_deref(),
        crate::linux::session::LinuxSession::detect().is_wayland(),
    ) {
        return rune::DisplayTarget::Primary;
    }
    rune::DisplayTarget::UnderPointer
}

/// Whether the HUD stays on the primary display: GNOME and KDE on Wayland
/// only. On X11 the pointer is readable on every desktop.
#[cfg(any(target_os = "linux", test))]
fn keeps_primary_display(desktops: Option<&str>, wayland: bool) -> bool {
    wayland
        && desktops.is_some_and(|desktops| {
            desktops.split(':').any(|desktop| {
                ["GNOME", "KDE"]
                    .iter()
                    .any(|d| desktop.eq_ignore_ascii_case(d))
            })
        })
}

/// The HUD's one element, which the compositor draws over. Screen readers
/// read it as a status that announces Recording and Transcribing as they
/// begin; it takes no input, as the overlay is click-through.
fn view(hud: &Hud, _: &mut Cx<Hud>) -> El {
    let status = hud.model.status().map_or("", tr);
    el().key("hud")
        .w_full()
        .h_full()
        .role("status")
        .label(status)
        .description(tr("Dictation"))
        .live_region(LiveRegion::Polite)
}

/// `DotUniforms` in `rune_hud.wgsl`.
#[repr(C)]
#[derive(Clone, Copy)]
pub(crate) struct DotUniforms {
    resolution: [f32; 2],
    backing_scale: f32,
    width: f32,
    height: f32,
    opacity: f32,
    scale: f32,
    softness: f32,
    pitch: f32,
    columns: f32,
    rows: f32,
    glow: f32,
    grid: f32,
    live: f32,
    cool: f32,
    dark: f32,
    base: [f32; 4],
    core: [f32; 4],
    plate: [f32; 4],
    rim: [f32; 4],
    shadow: [f32; 4],
    plate_layout: [f32; 4],
    island: [f32; 4],
    field: [f32; 4],
    /// The notch indicator's done and failed, 0 to 1.
    ears: [f32; 4],
}

const _: () = assert!(std::mem::size_of::<DotUniforms>() == 208);
const DOT_UNIFORM_BYTES: u32 = std::mem::size_of::<DotUniforms>() as u32;
const DOT_BYTES: u64 = (MAX_DOTS * 4) as u64;
/// How `rune_hud.wgsl` declares `u` as a uniform buffer; with immediates it
/// is declared `var<immediate>` instead.
const DOT_UNIFORM_BINDING: &str = "@group(0) @binding(0) var<uniform> u: DotUniforms;";

impl DotUniforms {
    fn new(frame: &DotFrame, layout: Layout, target: Target) -> Self {
        let pose = &frame.pose;
        // An island is black in either appearance and draws no plate.
        let dark = target.dark || layout.island.is_some();
        let mut colors = dot_scope::palette(dark, pose, frame.tuning.saturation);
        if layout.island.is_some() {
            for color in [&mut colors.plate, &mut colors.rim, &mut colors.shadow] {
                color[3] = 0.0;
            }
        }
        let mut center = layout.center;
        if layout.anchor_left {
            center[0] += pose.width * layout.fit / 2.0;
        }
        let rgba = |c: [f32; 3]| [c[0], c[1], c[2], 1.0];
        Self {
            resolution: [target.viewport[2], target.viewport[3]],
            backing_scale: target.scale,
            width: pose.width,
            height: pose.height,
            opacity: pose.opacity,
            scale: pose.scale,
            softness: pose.softness,
            pitch: frame.tuning.spacing,
            columns: frame.columns as f32,
            rows: frame.rows as f32,
            glow: pose.glow,
            grid: pose.grid,
            live: pose.live,
            cool: pose.cool,
            dark: if dark { 1.0 } else { 0.0 },
            base: rgba(colors.base),
            core: rgba(colors.core),
            plate: colors.plate,
            rim: colors.rim,
            shadow: colors.shadow,
            plate_layout: [
                center[0],
                center[1],
                layout.fit,
                if layout.island.is_some() { 1.0 } else { 0.0 },
            ],
            island: layout
                .island
                .map_or([0.0; 4], |island| island_shape(island, pose)),
            // The ears draw glyphs, not the field: the notch's half width,
            // the mode, the voice, and the stage's clock.
            field: match layout.island {
                Some(island) if island.ears => [island.notch[0] / 2.0, 1.0, pose.level, pose.clock],
                _ => [field_radius(layout, pose), 0.0, 0.0, 0.0],
            },
            ears: [pose.done, pose.fail, 0.0, 0.0],
        }
    }
}

/// The island for this pose: the notch itself while hidden, grown to hold
/// the plate as it shows, bouncing with it.
fn island_shape(island: Island, pose: &dot_scope::Pose) -> [f32; 4] {
    let [notch_width, notch_height] = island.notch;
    if island.ears {
        // The notch's height throughout, widening to the island's width.
        let half_width = notch_width / 2.0;
        let grow = pose.reveal;
        return [
            half_width + (island.width / 2.0 - half_width) * grow,
            notch_height,
            island.top,
            (NOTCH_RADIUS + (EARS_RADIUS - NOTCH_RADIUS) * grow.min(1.0)).min(notch_height / 2.0),
        ];
    }
    let pad = ISLAND_PAD as f32;
    let grow = pose.reveal;
    let half_width = notch_width / 2.0;
    let open_half_width = (pose.width / 2.0 + ISLAND_INSET as f32).max(half_width);
    let open_height = notch_height + 2.0 * pad + pose.height;
    let height = notch_height + (open_height - notch_height) * grow;
    [
        half_width + (open_half_width - half_width) * grow,
        height,
        island.top,
        (NOTCH_RADIUS + (ISLAND_RADIUS - NOTCH_RADIUS) * grow.min(1.0)).min(height / 2.0),
    ]
}

/// The field's corner radius: a pill on a plate, and in an island its
/// corners follow the island's, the same distance in all round.
fn field_radius(layout: Layout, pose: &dot_scope::Pose) -> f32 {
    let half_height = pose.height / 2.0;
    match layout.island {
        Some(_) => (ISLAND_RADIUS - ISLAND_PAD as f32).min(half_height),
        None => half_height,
    }
}

fn bytes<T: Copy>(value: &T) -> &[u8] {
    // SAFETY: `T` is a `repr(C)` struct of `f32`s with no padding, so every
    // byte is initialized.
    unsafe { std::slice::from_raw_parts((value as *const T).cast(), std::mem::size_of::<T>()) }
}

fn packed_dots(frame: &DotFrame) -> Vec<u8> {
    frame
        .dots
        .iter()
        .take(MAX_DOTS)
        .flat_map(|[power, fresh]| [power.to_le_bytes(), fresh.to_le_bytes()])
        .flatten()
        .collect()
}

/// What a frame draws into.
#[derive(Clone, Copy)]
pub(crate) struct Target {
    /// The HUD's window in the target, x, y, width and height in device
    /// pixels.
    pub(crate) viewport: [f32; 4],
    /// The pixels it may touch, x, y, width and height, inside the target.
    pub(crate) clip: [u32; 4],
    /// Device pixels per point.
    pub(crate) scale: f32,
    pub(crate) dark: bool,
}

impl Target {
    /// All of a target `size` device pixels big.
    fn whole(size: (u32, u32), scale: f32, dark: bool) -> Self {
        Self {
            viewport: [0.0, 0.0, size.0 as f32, size.1 as f32],
            clip: [0, 0, size.0, size.1],
            scale,
            dark,
        }
    }
}

struct HudCompositor {
    /// The painter and the device its buffers live on: an overlay shown
    /// again can draw on another device (#982). Keyed by Rune's device key,
    /// since wgpu compares a new Rune GPU's device equal to the old one.
    gpu: Option<(rune::compose::DeviceKey, Painter)>,
    animating: Arc<AtomicBool>,
}

impl Compositor<Hud> for HudCompositor {
    fn features(&self) -> wgpu::Features {
        Painter::FEATURES
    }

    fn animating(&self) -> bool {
        self.animating.load(Ordering::Acquire)
    }

    /// The view is empty, so Rune's UI pass is skipped: Hex's own pass
    /// clears the target.
    fn render(&mut self, f: &mut Frame<'_, Hud>) {
        let reduce = f.rt.reduced_motion;
        let target = Target::whole(f.size, f.rt.env.scale, f.rt.env.system_dark);
        let hud = &mut f.rt.state;
        hud.reduce = reduce;
        if hud.model.active() {
            hud.drawn = Some(hud.model.step(Instant::now(), f.dt as f32, reduce));
        }
        let active = hud.model.active();
        self.animating.store(active, Ordering::Release);
        #[cfg(not(target_os = "macos"))]
        if active {
            hud.follow_pointer(Instant::now());
        }
        if !active {
            hud.hide();
            drop(clear_pass(f.encoder, f.target));
            return;
        }
        let (Some(drawn), layout) = (hud.drawn.take(), hud.layout) else {
            drop(clear_pass(f.encoder, f.target));
            return;
        };
        if self
            .gpu
            .as_ref()
            .is_none_or(|(device, gpu)| *device != f.device_key() || gpu.format != f.format)
        {
            let painter =
                prebuilt(f.device, f.format).unwrap_or_else(|| Painter::new(f.device, f.format));
            self.gpu = Some((f.device_key(), painter));
        }
        let (_, gpu) = self.gpu.as_ref().expect("created above");
        let uniforms = gpu.write(&drawn, layout, target, |buffer, data| {
            f.write_buffer(buffer, 0, data)
        });
        gpu.draw(f.encoder, f.target, &uniforms);
        f.rt.state.drawn = Some(drawn);
    }
}

/// The HUD's shaders, ready to draw into targets of one format.
pub(crate) struct Painter {
    pub(crate) format: wgpu::TextureFormat,
    /// `None` when `DotUniforms` travel as immediates, set in the pass with
    /// no buffer write.
    dot_uniforms: Option<wgpu::Buffer>,
    dots: wgpu::Buffer,
    bind: wgpu::BindGroup,
    pipeline: wgpu::RenderPipeline,
}

impl Painter {
    /// What a compositor drawing with a `Painter` asks its device for.
    pub(crate) const FEATURES: wgpu::Features = wgpu::Features::IMMEDIATES;

    pub(crate) fn new(device: &wgpu::Device, format: wgpu::TextureFormat) -> Painter {
        Self::with_color_space(device, format, format.is_srgb())
    }

    /// Rune's isolated layers may be linear float targets rather than sRGB
    /// textures; the compositor supplies their blend space explicitly.
    pub(crate) fn with_color_space(
        device: &wgpu::Device,
        format: wgpu::TextureFormat,
        linear: bool,
    ) -> Painter {
        let immediates = device.features().contains(wgpu::Features::IMMEDIATES)
            && device.limits().max_immediate_size >= DOT_UNIFORM_BYTES;
        let source = include_str!("rune_hud.wgsl");
        assert!(source.contains(DOT_UNIFORM_BINDING));
        let source = if immediates {
            source.replace(DOT_UNIFORM_BINDING, "var<immediate> u: DotUniforms;")
        } else {
            source.to_owned()
        };
        let module = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("hex hud"),
            source: wgpu::ShaderSource::Wgsl(source.into()),
        });
        let buffer = |label, size: u64| {
            device.create_buffer(&wgpu::BufferDescriptor {
                label: Some(label),
                size,
                usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
                mapped_at_creation: false,
            })
        };
        let dot_uniforms =
            (!immediates).then(|| buffer("hex hud dot uniforms", u64::from(DOT_UNIFORM_BYTES)));
        let dots = buffer("hex hud dots", DOT_BYTES);
        let entry = |binding| wgpu::BindGroupLayoutEntry {
            binding,
            visibility: wgpu::ShaderStages::FRAGMENT,
            ty: wgpu::BindingType::Buffer {
                ty: wgpu::BufferBindingType::Uniform,
                has_dynamic_offset: false,
                min_binding_size: None,
            },
            count: None,
        };
        let mut entries = vec![entry(1)];
        let mut bindings = vec![wgpu::BindGroupEntry {
            binding: 1,
            resource: dots.as_entire_binding(),
        }];
        if let Some(buffer) = &dot_uniforms {
            entries.push(entry(0));
            bindings.push(wgpu::BindGroupEntry {
                binding: 0,
                resource: buffer.as_entire_binding(),
            });
        }
        let layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("hex hud"),
            entries: &entries,
        });
        let bind = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("hex hud"),
            layout: &layout,
            entries: &bindings,
        });
        let pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("hex hud"),
            bind_group_layouts: &[Some(&layout)],
            immediate_size: if immediates { DOT_UNIFORM_BYTES } else { 0 },
        });
        // Display values; a linear target decodes them.
        let constants = [("linear_target", f64::from(u8::from(linear)))];
        let pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("dot_scope_fragment"),
            layout: Some(&pipeline_layout),
            vertex: wgpu::VertexState {
                module: &module,
                entry_point: Some("indicator_vertex"),
                compilation_options: Default::default(),
                buffers: &[],
            },
            primitive: wgpu::PrimitiveState::default(),
            depth_stencil: None,
            multisample: Default::default(),
            fragment: Some(wgpu::FragmentState {
                module: &module,
                entry_point: Some("dot_scope_fragment"),
                compilation_options: wgpu::PipelineCompilationOptions {
                    constants: &constants,
                    ..Default::default()
                },
                targets: &[Some(wgpu::ColorTargetState {
                    format,
                    blend: Some(wgpu::BlendState::PREMULTIPLIED_ALPHA_BLENDING),
                    write_mask: wgpu::ColorWrites::ALL,
                })],
            }),
            multiview_mask: None,
            cache: None,
        });
        Painter {
            format,
            dot_uniforms,
            dots,
            bind,
            pipeline,
        }
    }

    /// Writes `frame`'s buffers through `write` and returns its uniforms,
    /// for the draw.
    pub(crate) fn write(
        &self,
        frame: &DotFrame,
        layout: Layout,
        target: Target,
        mut write: impl FnMut(&wgpu::Buffer, &[u8]),
    ) -> DotUniforms {
        let uniforms = DotUniforms::new(frame, layout, target);
        if let Some(buffer) = &self.dot_uniforms {
            write(buffer, bytes(&uniforms));
        }
        write(&self.dots, &packed_dots(frame));
        uniforms
    }

    /// Clears `view` and draws Dot scope into it.
    fn draw(
        &self,
        encoder: &mut wgpu::CommandEncoder,
        view: &wgpu::TextureView,
        uniforms: &DotUniforms,
    ) {
        self.draw_in(&mut clear_pass(encoder, view), uniforms);
    }

    fn draw_in(&self, pass: &mut wgpu::RenderPass<'_>, uniforms: &DotUniforms) {
        pass.set_pipeline(&self.pipeline);
        pass.set_bind_group(0, &self.bind, &[]);
        if self.dot_uniforms.is_none() {
            pass.set_immediates(0, bytes(uniforms));
        }
        pass.draw(0..3, 0..1);
    }

    /// Draws Dot scope over what `view` holds, in `target`'s viewport and
    /// clip only.
    pub(crate) fn draw_over(
        &self,
        encoder: &mut wgpu::CommandEncoder,
        view: &wgpu::TextureView,
        target: Target,
        uniforms: &DotUniforms,
    ) {
        let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
            label: Some("hex hud preview"),
            color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                view,
                depth_slice: None,
                resolve_target: None,
                ops: wgpu::Operations {
                    load: wgpu::LoadOp::Load,
                    store: wgpu::StoreOp::Store,
                },
            })],
            depth_stencil_attachment: None,
            timestamp_writes: None,
            occlusion_query_set: None,
            multiview_mask: None,
        });
        self.draw_slot(&mut pass, target, uniforms);
    }

    /// Draw inside Rune's effect-slot pass, preserving its painter order and
    /// the containing layer's opacity.
    pub(crate) fn draw_slot(
        &self,
        pass: &mut wgpu::RenderPass<'_>,
        target: Target,
        uniforms: &DotUniforms,
    ) {
        let [x, y, width, height] = target.viewport;
        pass.set_viewport(x, y, width, height, 0.0, 1.0);
        let [cx, cy, cw, ch] = target.clip;
        pass.set_scissor_rect(cx, cy, cw, ch);
        self.draw_in(pass, uniforms);
    }
}

/// A pass over `view` that starts transparent.
fn clear_pass<'a>(
    encoder: &'a mut wgpu::CommandEncoder,
    view: &wgpu::TextureView,
) -> wgpu::RenderPass<'a> {
    encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
        label: Some("hex hud"),
        color_attachments: &[Some(wgpu::RenderPassColorAttachment {
            view,
            depth_slice: None,
            resolve_target: None,
            ops: wgpu::Operations {
                load: wgpu::LoadOp::Clear(wgpu::Color::TRANSPARENT),
                store: wgpu::StoreOp::Store,
            },
        })],
        depth_stencil_attachment: None,
        timestamp_writes: None,
        occlusion_query_set: None,
        multiview_mask: None,
    })
}

#[cfg(test)]
mod tests;
