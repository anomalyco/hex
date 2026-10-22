//! Shared spring physics, Kit motion presets (`duration` + `bounce`),
//! Reduce Motion detection, GPUI keyed springs, and debug frame-time profiling.
//!
//! Presets mirror `~/code/open-source/motion-presets/presets/generated/kit_motion.rs`.

#![allow(dead_code)]

use std::cell::RefCell;
use std::collections::{BTreeMap, HashMap};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::time::{Duration, Instant};

use gpui::{ElementId, Hsla, Pixels, Point, Rgba, Window, WindowId, px};

const CRITICAL_DAMPING_TOLERANCE: f32 = 1e-4;
pub const DEFAULT_EPSILON: f32 = 0.001;
const MAX_FRAME_DT: f32 = 0.05;

/// Duration + bounce, SwiftUI mapping (`ω = 2π / duration`, `ζ = 1 − bounce`).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Spec {
    pub duration: f32,
    pub bounce: f32,
}

pub type KitSpring = Spec;

impl Spec {
    pub const fn new(duration: f32, bounce: f32) -> Self {
        Self { duration, bounce }
    }

    /// Constructs a critically damped spring (`bounce = 0`) from its angular frequency `ω` (rad/s).
    pub fn from_omega(omega: f32) -> Self {
        Self {
            duration: std::f32::consts::TAU / omega.max(0.1),
            bounce: 0.0,
        }
    }

    pub fn omega(self) -> f32 {
        std::f32::consts::TAU / self.duration.max(0.01)
    }

    pub fn zeta(self) -> f32 {
        if self.bounce >= 0.0 {
            (1.0 - self.bounce).max(0.0)
        } else {
            1.0 / (1.0 + self.bounce).max(0.01)
        }
    }
}

/// Kit's motion palette from `motion-presets/presets/kit-motion.json`.
pub mod presets {
    use super::Spec;

    /// Pointer-tied. Barely a spring: takes the edge off a jump without lagging the hand.
    pub const INSTANT: Spec = Spec {
        duration: 0.1,
        bounce: 0.0,
    };
    pub const INSTANT_EXIT: Spec = Spec {
        duration: 0.1,
        bounce: 0.0,
    };
    /// Small controls answering a click: indicators, toggles, widths that lead, exits.
    pub const SNAP: Spec = Spec {
        duration: 0.25,
        bounce: 0.0,
    };
    pub const SNAP_EXIT: Spec = Spec {
        duration: 0.2,
        bounce: 0.0,
    };
    /// The default state change. Arrives and stops, no drama.
    pub const SETTLE: Spec = Spec {
        duration: 0.3,
        bounce: 0.0,
    };
    pub const SETTLE_EXIT: Spec = Spec {
        duration: 0.24,
        bounce: 0.0,
    };
    /// Layout moving out of the way: siblings sliding into a gap, containers resizing.
    pub const REFLOW: Spec = Spec {
        duration: 0.4,
        bounce: 0.0,
    };
    pub const REFLOW_EXIT: Spec = Spec {
        duration: 0.32,
        bounce: 0.0,
    };
    /// Big surfaces travelling: drawers, sheets, pages.
    pub const GLIDE: Spec = Spec {
        duration: 0.32,
        bounce: 0.0,
    };
    pub const GLIDE_EXIT: Spec = Spec {
        duration: 0.26,
        bounce: 0.0,
    };
    /// One shape becoming another: a chip opening into its panel, a menu out of its button.
    pub const MORPH: Spec = Spec {
        duration: 0.3,
        bounce: 0.16,
    };
    pub const MORPH_EXIT: Spec = Spec {
        duration: 0.27,
        bounce: 0.08,
    };
    /// A release with a little life in it. Keep it to presses and confirmations.
    pub const POP: Spec = Spec {
        duration: 0.25,
        bounce: 0.15,
    };
    pub const POP_EXIT: Spec = Spec {
        duration: 0.2,
        bounce: 0.05,
    };
    /// Slow reveals and wipes the eye should follow, never utility UI.
    pub const DRIFT: Spec = Spec {
        duration: 0.85,
        bounce: 0.0,
    };
    pub const DRIFT_EXIT: Spec = Spec {
        duration: 0.6,
        bounce: 0.0,
    };
    /// Short opacity-only crossfade preserved when Reduce Motion is enabled.
    pub const REDUCED_FADE: Spec = Spec {
        duration: 0.16,
        bounce: 0.0,
    };
}

/// Exact analytical damped harmonic oscillator state.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Spring {
    pub x: f32,
    pub v: f32,
    pub target: f32,
}

impl Spring {
    pub const fn at(x: f32) -> Self {
        Self {
            x,
            v: 0.0,
            target: x,
        }
    }

    /// Retargets the spring while preserving its current position and velocity.
    pub fn to(&mut self, target: f32) {
        self.target = target;
    }

    /// Snaps immediately to `x` and clears velocity (for first paint, direct drag, or Reduce Motion).
    pub fn jump(&mut self, x: f32) {
        self.x = x;
        self.v = 0.0;
        self.target = x;
    }

    /// Retargets to `target` and advances by `dt` seconds using the closed-form state propagator.
    pub fn step_to(&mut self, target: f32, spec: Spec, dt: f32) {
        self.target = target;
        self.step(spec, dt);
    }

    /// Advances the spring toward `self.target` by `dt` seconds.
    ///
    /// Solved analytically across underdamped, critically damped, and overdamped regimes
    /// so trajectories are independent of frame rate and unconditionally stable.
    pub fn step(&mut self, spec: Spec, dt: f32) {
        if dt <= 0.0 || !dt.is_finite() {
            return;
        }
        let omega = spec.omega();
        let zeta = spec.zeta();
        let x0 = self.x - self.target;
        let v0 = self.v;

        if zeta < 1.0 - CRITICAL_DAMPING_TOLERANCE {
            let decay = zeta * omega;
            let omega_d = omega * (1.0 - zeta * zeta).sqrt();
            let exp = (-decay * dt).exp();
            let (sin, cos) = (omega_d * dt).sin_cos();
            let sin_over_d = sin / omega_d;
            self.x = self.target + exp * (x0 * (cos + decay * sin_over_d) + v0 * sin_over_d);
            self.v = exp * (-x0 * omega * omega * sin_over_d + v0 * (cos - decay * sin_over_d));
        } else if zeta > 1.0 + CRITICAL_DAMPING_TOLERANCE {
            let root = (zeta * zeta - 1.0).sqrt();
            let root_sum = zeta + root;
            let slow_root = -omega / root_sum;
            let fast_root = -omega * root_sum;
            let denom = slow_root - fast_root;
            let slow_exp = (slow_root * dt).exp();
            let fast_exp = (fast_root * dt).exp();
            let p00 = (-fast_root * slow_exp + slow_root * fast_exp) / denom;
            let p01 = (slow_exp - fast_exp) / denom;
            let p10 = slow_root * fast_root * (fast_exp - slow_exp) / denom;
            let p11 = (slow_root * slow_exp - fast_root * fast_exp) / denom;
            self.x = self.target + p00 * x0 + p01 * v0;
            self.v = p10 * x0 + p11 * v0;
        } else {
            let exp = (-omega * dt).exp();
            let c = v0 + omega * x0;
            self.x = self.target + (x0 + c * dt) * exp;
            self.v = (v0 - omega * c * dt) * exp;
        }
    }

    /// Tests whether both displacement and ω-scaled velocity are within `eps` of `self.target`.
    pub fn settled(&self, spec: Spec, eps: f32) -> bool {
        let omega = spec.omega();
        (self.x - self.target).abs() <= eps && self.v.abs() <= eps * omega
    }

    /// Snaps to `self.target` and zeroes velocity if settled within `eps`.
    pub fn settle(&mut self, spec: Spec, eps: f32) -> bool {
        if self.settled(spec, eps) {
            self.x = self.target;
            self.v = 0.0;
            true
        } else {
            false
        }
    }
}

/// Linear interpolation across scalar, colour, and geometric types.
pub trait Mix: Copy {
    fn mix(a: Self, b: Self, t: f32) -> Self;
}

impl Mix for f32 {
    fn mix(a: Self, b: Self, t: f32) -> Self {
        a + (b - a) * t
    }
}

impl Mix for Pixels {
    fn mix(a: Self, b: Self, t: f32) -> Self {
        px(f32::from(a) + (f32::from(b) - f32::from(a)) * t)
    }
}

impl Mix for Point<Pixels> {
    fn mix(a: Self, b: Self, t: f32) -> Self {
        Point {
            x: Pixels::mix(a.x, b.x, t),
            y: Pixels::mix(a.y, b.y, t),
        }
    }
}

impl Mix for Rgba {
    fn mix(a: Self, b: Self, t: f32) -> Self {
        let t = t.clamp(0.0, 1.0);
        Self {
            r: a.r + (b.r - a.r) * t,
            g: a.g + (b.g - a.g) * t,
            b: a.b + (b.b - a.b) * t,
            a: a.a + (b.a - a.a) * t,
        }
    }
}

impl Mix for Hsla {
    fn mix(a: Self, b: Self, t: f32) -> Self {
        let t = t.clamp(0.0, 1.0);
        let hue_delta = (b.h - a.h + 0.5).rem_euclid(1.0) - 0.5;
        Self {
            h: (a.h + hue_delta * t).rem_euclid(1.0),
            s: a.s + (b.s - a.s) * t,
            l: a.l + (b.l - a.l) * t,
            a: a.a + (b.a - a.a) * t,
        }
    }
}

pub fn mix<T: Mix>(a: T, b: T, t: f32) -> T {
    T::mix(a, b, t)
}

/// Snaps both edges on a `1/64 px` sub-pixel grid and derives `(origin, size)` so
/// a stationary edge never jiggles by 1 quantum while the opposite edge moves.
pub fn snap_edge_pair(start: f32, end: f32) -> (f32, f32) {
    let snap = |v: f32| (v * 64.0).round() / 64.0;
    let s0 = snap(start);
    let s1 = snap(end);
    (s0, (s1 - s0).max(0.0))
}

static REDUCE_MOTION_CACHED: AtomicBool = AtomicBool::new(false);
static REDUCE_MOTION_LAST_CHECK_MS: AtomicU64 = AtomicU64::new(0);

/// Returns whether the user has enabled Reduce Motion in system accessibility settings,
/// or via `HEX_REDUCE_MOTION=1` for deterministic testing and previews.
pub fn reduced_motion() -> bool {
    if let Some(override_val) = env_reduce_motion() {
        return override_val;
    }
    let now_ms = monotonic_ms();
    let last = REDUCE_MOTION_LAST_CHECK_MS.load(Ordering::Relaxed);
    if last == 0 || now_ms.saturating_sub(last) >= 250 {
        let current = query_platform_reduce_motion();
        REDUCE_MOTION_CACHED.store(current, Ordering::Relaxed);
        REDUCE_MOTION_LAST_CHECK_MS.store(now_ms.max(1), Ordering::Relaxed);
        return current;
    }
    REDUCE_MOTION_CACHED.load(Ordering::Relaxed)
}

fn env_reduce_motion() -> Option<bool> {
    let val = std::env::var("HEX_REDUCE_MOTION").ok()?;
    match val.trim() {
        "1" | "true" | "TRUE" | "yes" | "YES" => Some(true),
        "0" | "false" | "FALSE" | "no" | "NO" => Some(false),
        _ => None,
    }
}

fn monotonic_ms() -> u64 {
    static START: std::sync::OnceLock<Instant> = std::sync::OnceLock::new();
    START.get_or_init(Instant::now).elapsed().as_millis() as u64
}

#[cfg(target_os = "macos")]
fn query_platform_reduce_motion() -> bool {
    unsafe {
        let workspace: *mut objc2::runtime::AnyObject =
            objc2::msg_send![objc2::class!(NSWorkspace), sharedWorkspace];
        if workspace.is_null() {
            return false;
        }
        let reduce: bool = objc2::msg_send![workspace, accessibilityDisplayShouldReduceMotion];
        reduce
    }
}

#[cfg(not(target_os = "macos"))]
fn query_platform_reduce_motion() -> bool {
    false
}

struct KeyedSpringState {
    spring: Spring,
    last_step: Instant,
}

/// Self-stepping spring wrapper for component fields (such as toggles, segmented
/// controls, and disclosure widths) that also need `.enabled()` or `.set_target()`
/// outside `render`.
#[derive(Clone, Copy, Debug)]
pub struct ToggleSpring {
    pub spring: Spring,
    pub spec: Spec,
    last_frame: Instant,
    waking: bool,
}

impl ToggleSpring {
    pub fn new(enabled: bool) -> Self {
        Self::with_spec(if enabled { 1.0 } else { 0.0 }, presets::SNAP)
    }

    pub fn at(position: f32) -> Self {
        Self::with_spec(position, presets::SNAP)
    }

    pub fn with_spec(position: f32, spec: Spec) -> Self {
        Self {
            spring: Spring::at(position),
            spec,
            last_frame: Instant::now(),
            waking: false,
        }
    }

    pub fn set_enabled(&mut self, enabled: bool) {
        self.set_target(if enabled { 1.0 } else { 0.0 });
    }

    pub fn set_target(&mut self, target: f32) {
        if (self.spring.target - target).abs() <= f32::EPSILON {
            return;
        }
        if self.is_settled() {
            self.last_frame = Instant::now();
            self.waking = true;
        }
        self.spring.to(target);
    }

    pub fn advance(&mut self, elapsed: Duration) {
        self.waking = false;
        let dt = elapsed.as_secs_f32().clamp(0.0, 1.0);
        self.spring.step(self.spec, dt);
        self.spring.settle(self.spec, DEFAULT_EPSILON);
    }

    pub fn render_position(&mut self, window: &mut Window) -> f32 {
        if reduced_motion() || std::env::var_os("HEX_PREVIEW_BEFORE").is_some() {
            self.spring.jump(self.spring.target);
            self.last_frame = current_frame_instant();
            self.waking = false;
            return self.spring.x;
        }
        let now = current_frame_instant();
        if self.is_settled() {
            self.spring.jump(self.spring.target);
            self.last_frame = now;
            self.waking = false;
            return self.spring.x;
        }
        if self.waking {
            self.waking = false;
            self.last_frame = now;
            window.request_animation_frame();
            return self.spring.x;
        }
        let dt = now
            .saturating_duration_since(self.last_frame)
            .as_secs_f32()
            .clamp(0.0, MAX_FRAME_DT);
        self.last_frame = now;
        if dt > 0.0 {
            self.spring.step(self.spec, dt);
            self.spring.settle(self.spec, DEFAULT_EPSILON);
        }
        if !self.is_settled() {
            window.request_animation_frame();
        }
        self.spring.x
    }

    pub fn is_settled(&self) -> bool {
        self.spring.settled(self.spec, DEFAULT_EPSILON)
    }

    pub fn enabled(&self) -> bool {
        self.spring.target >= 0.5
    }

    pub fn position(&self) -> f32 {
        self.spring.x
    }
}

thread_local! {
    static CURRENT_FRAME_INSTANT: RefCell<Option<Instant>> = const { RefCell::new(None) };
    static KEYED_SPRINGS: RefCell<HashMap<(WindowId, ElementId), KeyedSpringState>> = RefCell::new(HashMap::new());
}

fn current_frame_instant() -> Instant {
    let now = Instant::now();
    CURRENT_FRAME_INSTANT.with(|slot| {
        let mut slot = slot.borrow_mut();
        if let Some(existing) = *slot
            && now.duration_since(existing) <= Duration::from_millis(2)
        {
            return existing;
        }
        *slot = Some(now);
        now
    })
}

/// Extension trait on GPUI [`Window`] for keyed, velocity-preserving springs.
pub trait WindowMotion {
    /// Steps a keyed spring toward `target` using the current frame timestamp,
    /// requesting another animation frame only while unsettled. Snaps immediately
    /// when Reduce Motion is enabled.
    fn spring(&mut self, key: impl Into<ElementId>, target: f32, spec: Spec) -> f32;

    /// Steps a keyed spring toward `target`, or jumps 1:1 when `jump` is true
    /// (for direct user manipulation such as dragging a slider).
    fn spring_or_jump(
        &mut self,
        key: impl Into<ElementId>,
        target: f32,
        jump: bool,
        spec: Spec,
    ) -> f32;

    /// Steps a keyed boolean progress spring (`0.0` when `enabled == false`, `1.0` when `true`),
    /// requesting another animation frame only while unsettled.
    fn spring_when(&mut self, key: impl Into<ElementId>, enabled: bool, spec: Spec) -> f32;

    /// Steps a keyed opacity spring toward `target`, keeping a short `0.16s` zero-bounce
    /// crossfade when Reduce Motion is enabled.
    fn spring_fade(&mut self, key: impl Into<ElementId>, target: f32, spec: Spec) -> f32;

    /// Whether Reduce Motion is active.
    fn reduced_motion(&self) -> bool;
}

impl WindowMotion for Window {
    fn spring(&mut self, key: impl Into<ElementId>, target: f32, spec: Spec) -> f32 {
        let animate = !reduced_motion() && std::env::var_os("HEX_PREVIEW_BEFORE").is_none();
        step_window_spring(self, key.into(), target, animate, spec, DEFAULT_EPSILON)
    }

    fn spring_or_jump(
        &mut self,
        key: impl Into<ElementId>,
        target: f32,
        jump: bool,
        spec: Spec,
    ) -> f32 {
        let animate =
            !jump && !reduced_motion() && std::env::var_os("HEX_PREVIEW_BEFORE").is_none();
        step_window_spring(self, key.into(), target, animate, spec, DEFAULT_EPSILON)
    }

    fn spring_when(&mut self, key: impl Into<ElementId>, enabled: bool, spec: Spec) -> f32 {
        self.spring(key, if enabled { 1.0 } else { 0.0 }, spec)
    }

    fn spring_fade(&mut self, key: impl Into<ElementId>, target: f32, spec: Spec) -> f32 {
        let animate = std::env::var_os("HEX_PREVIEW_BEFORE").is_none();
        let effective_spec = if reduced_motion() {
            presets::REDUCED_FADE
        } else {
            spec
        };
        step_window_spring(
            self,
            key.into(),
            target,
            animate,
            effective_spec,
            DEFAULT_EPSILON,
        )
    }

    fn reduced_motion(&self) -> bool {
        reduced_motion()
    }
}

fn step_window_spring(
    window: &mut Window,
    key: ElementId,
    target: f32,
    animate: bool,
    spec: Spec,
    eps: f32,
) -> f32 {
    let frame_now = current_frame_instant();
    let window_id = window.window_handle().window_id();
    let (value, settled) = KEYED_SPRINGS.with(|map| {
        let mut map = map.borrow_mut();
        let state = map
            .entry((window_id, key))
            .or_insert_with(|| KeyedSpringState {
                spring: Spring::at(target),
                last_step: frame_now,
            });
        if !animate {
            state.spring.jump(target);
            state.last_step = frame_now;
            return (target, true);
        }
        let was_settled = state.spring.settled(spec, eps);
        let target_changed = (state.spring.target - target).abs() > f32::EPSILON;
        state.spring.to(target);

        if was_settled && !target_changed {
            state.spring.jump(target);
            state.last_step = frame_now;
            return (target, true);
        }
        if was_settled && target_changed {
            // Waking from rest: anchor `last_step` at this trigger frame so frame 1
            // steps by the first real vsync interval rather than idle wall-clock time.
            state.last_step = frame_now;
            return (state.spring.x, false);
        }

        let dt = frame_now
            .saturating_duration_since(state.last_step)
            .as_secs_f32()
            .clamp(0.0, MAX_FRAME_DT);
        state.last_step = frame_now;
        state.spring.step(spec, dt);
        let settled = state.spring.settle(spec, eps);
        (state.spring.x, settled)
    });
    if !settled {
        window.request_animation_frame();
    }
    value
}

/// Fixed-capacity ring buffer of frame or render durations in microseconds,
/// with percentile (`p50`, `p95`, `max`) summaries for debug diagnostics.
#[derive(Clone, Debug)]
pub struct FrameTimeRing {
    samples_us: [u32; Self::CAPACITY],
    cursor: usize,
    len: usize,
    last_frame_start: Option<Instant>,
    total_samples: u64,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct FrameTimeSummary {
    pub count: usize,
    pub p50_us: u32,
    pub p95_us: u32,
    pub max_us: u32,
}

impl Default for FrameTimeRing {
    fn default() -> Self {
        Self::new()
    }
}

impl FrameTimeRing {
    pub const CAPACITY: usize = 128;

    pub const fn new() -> Self {
        Self {
            samples_us: [0; Self::CAPACITY],
            cursor: 0,
            len: 0,
            last_frame_start: None,
            total_samples: 0,
        }
    }

    pub fn record_duration(&mut self, duration: Duration) {
        let us = duration.as_micros().min(u128::from(u32::MAX)) as u32;
        self.samples_us[self.cursor] = us;
        self.cursor = (self.cursor + 1) % Self::CAPACITY;
        if self.len < Self::CAPACITY {
            self.len += 1;
        }
        self.total_samples += 1;
    }

    pub fn record_frame_interval(&mut self, now: Instant) -> Option<Duration> {
        let prev = self.last_frame_start.replace(now)?;
        let delta = now.saturating_duration_since(prev);
        // Only record active animation intervals (< 100 ms), ignoring idle gaps.
        if delta <= Duration::from_millis(100) {
            self.record_duration(delta);
            Some(delta)
        } else {
            None
        }
    }

    pub fn total_samples(&self) -> u64 {
        self.total_samples
    }

    pub fn summary(&self) -> Option<FrameTimeSummary> {
        if self.len == 0 {
            return None;
        }
        let mut sorted = self.samples_us[..self.len].to_vec();
        sorted.sort_unstable();
        let p50_idx = (self.len - 1) * 50 / 100;
        let p95_idx = (self.len - 1) * 95 / 100;
        Some(FrameTimeSummary {
            count: self.len,
            p50_us: sorted[p50_idx],
            p95_us: sorted[p95_idx],
            max_us: *sorted.last().unwrap(),
        })
    }
}

struct PaneProfile {
    render_ring: FrameTimeRing,
    interval_ring: FrameTimeRing,
}

thread_local! {
    static PANE_PROFILES: RefCell<BTreeMap<&'static str, PaneProfile>> = const { RefCell::new(BTreeMap::new()) };
}

pub struct FrameProfileGuard {
    pane: &'static str,
    started: Instant,
}

/// Starts a motion frame, anchoring the shared frame clock and returning an RAII guard
/// that records per-pane render time and frame intervals in debug builds.
pub fn begin_frame(pane: &'static str) -> FrameProfileGuard {
    let started = Instant::now();
    CURRENT_FRAME_INSTANT.with(|slot| {
        *slot.borrow_mut() = Some(started);
    });
    #[cfg(target_os = "macos")]
    if std::env::var_os("HEX_RECORD_HUD_WINDOW").is_none() {
        maybe_start_self_window_recorder(None);
    }
    FrameProfileGuard { pane, started }
}

#[cfg(target_os = "macos")]
pub fn maybe_start_self_window_recorder(preferred_wid: Option<u32>) {
    static STARTED: AtomicBool = AtomicBool::new(false);
    if STARTED.load(Ordering::Acquire) {
        return;
    }
    let Ok(dir) = std::env::var("HEX_RECORD_FRAMES_DIR") else {
        return;
    };
    let wid = preferred_wid.or_else(find_own_main_window_id);
    let Some(wid) = wid.filter(|&w| w > 0) else {
        return;
    };
    if STARTED.swap(true, Ordering::AcqRel) {
        return;
    }
    let duration_ms: u64 = std::env::var("HEX_RECORD_DURATION_MS")
        .ok()
        .and_then(|s| s.parse().ok())
        .unwrap_or(4400);
    std::thread::spawn(move || {
        record_window_frames(
            wid,
            std::path::PathBuf::from(dir),
            Duration::from_millis(duration_ms),
        );
    });
}

#[cfg(target_os = "macos")]
fn find_own_main_window_id() -> Option<u32> {
    let mtm = objc2::MainThreadMarker::new()?;
    let app = objc2_app_kit::NSApplication::sharedApplication(mtm);
    for window in app.windows().iter() {
        let frame = window.frame();
        if frame.size.width >= 700.0 && frame.size.height >= 400.0 {
            let num = window.windowNumber();
            if num > 0 {
                return Some(num as u32);
            }
        }
    }
    None
}

#[cfg(target_os = "macos")]
fn record_window_frames(wid: u32, dir: std::path::PathBuf, duration: Duration) {
    use std::ffi::c_void;

    #[repr(C)]
    #[derive(Clone, Copy)]
    struct CgPoint {
        x: f64,
        y: f64,
    }
    #[repr(C)]
    #[derive(Clone, Copy)]
    struct CgSize {
        width: f64,
        height: f64,
    }
    #[repr(C)]
    #[derive(Clone, Copy)]
    struct CgRect {
        origin: CgPoint,
        size: CgSize,
    }

    #[link(name = "CoreGraphics", kind = "framework")]
    unsafe extern "C" {
        fn CGWindowListCreateImage(
            screen_bounds: CgRect,
            list_option: u32,
            window_id: u32,
            image_option: u32,
        ) -> *mut c_void;
        fn CGImageGetWidth(image: *const c_void) -> usize;
        fn CGImageGetHeight(image: *const c_void) -> usize;
        fn CGImageRelease(image: *mut c_void);
        fn CGColorSpaceCreateDeviceRGB() -> *mut c_void;
        fn CGColorSpaceRelease(space: *mut c_void);
        fn CGBitmapContextCreate(
            data: *mut c_void,
            width: usize,
            height: usize,
            bits_per_component: usize,
            bytes_per_row: usize,
            space: *mut c_void,
            bitmap_info: u32,
        ) -> *mut c_void;
        fn CGContextDrawImage(c: *mut c_void, rect: CgRect, image: *const c_void);
        fn CGContextRelease(c: *mut c_void);
    }

    let _ = std::fs::create_dir_all(&dir);
    std::thread::sleep(Duration::from_millis(120));
    let null_rect = CgRect {
        origin: CgPoint { x: 0.0, y: 0.0 },
        size: CgSize {
            width: 0.0,
            height: 0.0,
        },
    };
    let total_frames = ((duration.as_secs_f64() * 60.0).round() as usize).max(1);
    let mut images: Vec<usize> = Vec::with_capacity(total_frames);
    let start = Instant::now();
    for frame_idx in 0..total_frames {
        let img = unsafe { CGWindowListCreateImage(null_rect, 8, wid, 1) };
        if !img.is_null() {
            images.push(img as usize);
        } else if let Some(&prev) = images.last() {
            images.push(prev);
        }
        let next_tick = Duration::from_secs_f64((frame_idx + 1) as f64 / 60.0);
        if let Some(rem) = next_tick.checked_sub(start.elapsed()) {
            std::thread::sleep(rem);
        }
    }

    let space = unsafe { CGColorSpaceCreateDeviceRGB() };
    let mut released = std::collections::HashSet::new();
    for (idx, &raw_ptr) in images.iter().enumerate() {
        let img = raw_ptr as *mut c_void;
        let w = (unsafe { CGImageGetWidth(img) }) & !1;
        let h = (unsafe { CGImageGetHeight(img) }) & !1;
        if w > 0 && h > 0 {
            let mut rgba = vec![0u8; w * h * 4];
            for px in rgba.as_chunks_mut::<4>().0 {
                px[0] = 17;
                px[1] = 18;
                px[2] = 20;
                px[3] = 255;
            }
            let ctx = unsafe {
                CGBitmapContextCreate(
                    rgba.as_mut_ptr().cast::<c_void>(),
                    w,
                    h,
                    8,
                    w * 4,
                    space,
                    1, // kCGImageAlphaPremultipliedLast
                )
            };
            if !ctx.is_null() {
                let rect = CgRect {
                    origin: CgPoint { x: 0.0, y: 0.0 },
                    size: CgSize {
                        width: w as f64,
                        height: h as f64,
                    },
                };
                unsafe {
                    CGContextDrawImage(ctx, rect, img);
                    CGContextRelease(ctx);
                }
                let header = format!("P6\n{w} {h}\n255\n");
                let mut ppm = Vec::with_capacity(header.len() + w * h * 3);
                ppm.extend_from_slice(header.as_bytes());
                for px in rgba.as_chunks::<4>().0 {
                    ppm.push(px[0]);
                    ppm.push(px[1]);
                    ppm.push(px[2]);
                }
                let _ = std::fs::write(dir.join(format!("frame_{idx:04}.ppm")), ppm);
            }
        }
        if released.insert(raw_ptr) {
            unsafe { CGImageRelease(img) };
        }
    }
    unsafe { CGColorSpaceRelease(space) };
    let _ = std::fs::write(dir.join("DONE"), b"ok\n");
}

fn frame_profiling() -> bool {
    cfg!(debug_assertions) || std::env::var_os("HEX_PROFILE_FRAMES").is_some()
}

thread_local! {
    /// The input that started a pane switch, and the pane it asked for.
    static PANE_SWITCH: std::cell::Cell<Option<(Instant, Option<&'static str>)>> =
        const { std::cell::Cell::new(None) };
}

/// A pointer press or key that may switch panes: the latency clock starts here.
pub fn mark_pane_input() {
    if frame_profiling() {
        PANE_SWITCH.with(|cell| cell.set(Some((Instant::now(), None))));
    }
}

/// The pane switch itself. Keeps an input mark from the last half second.
pub fn mark_pane_switch(pane: &'static str) {
    if !frame_profiling() {
        return;
    }
    PANE_SWITCH.with(|cell| {
        let started = cell
            .get()
            .map(|(at, _)| at)
            .filter(|at| at.elapsed() < Duration::from_millis(500))
            .unwrap_or_else(Instant::now);
        cell.set(Some((started, Some(pane))));
    });
}

/// Called as `pane` starts rendering. The first render after a switch logs
/// input → that render, and input → the next frame (the new pane on screen).
pub fn pane_switch_rendered(pane: &'static str, window: &gpui::Window) {
    let Some((started, Some(target))) = PANE_SWITCH.with(std::cell::Cell::get) else {
        return;
    };
    if target != pane {
        return;
    }
    PANE_SWITCH.with(|cell| cell.set(None));
    let rendered = started.elapsed();
    window.on_next_frame(move |_, _| {
        tracing::info!(
            pane,
            render_start_ms = rendered.as_secs_f64() * 1000.0,
            on_screen_ms = started.elapsed().as_secs_f64() * 1000.0,
            "pane switch latency"
        );
    });
}

/// Logs the current `p50`/`p95`/`max` frame-time ring buffer for `pane` (when non-empty)
/// in debug builds or when `HEX_PROFILE_FRAMES` is set.
pub fn log_pane_summary(pane: &'static str, reason: &'static str) {
    if !(cfg!(debug_assertions) || std::env::var_os("HEX_PROFILE_FRAMES").is_some()) {
        return;
    }
    PANE_PROFILES.with(|profiles| {
        let profiles = profiles.borrow();
        let Some(entry) = profiles.get(pane) else {
            return;
        };
        let Some(render) = entry.render_ring.summary() else {
            return;
        };
        let interval = entry.interval_ring.summary();
        tracing::info!(
            pane,
            reason,
            samples = render.count,
            render_p50_us = render.p50_us,
            render_p95_us = render.p95_us,
            render_max_us = render.max_us,
            frame_p50_ms = interval.map(|s| s.p50_us as f32 / 1000.0).unwrap_or(0.0),
            frame_p95_ms = interval.map(|s| s.p95_us as f32 / 1000.0).unwrap_or(0.0),
            frame_max_ms = interval.map(|s| s.max_us as f32 / 1000.0).unwrap_or(0.0),
            "pane frame timing summary"
        );
    });
}

impl Drop for FrameProfileGuard {
    fn drop(&mut self) {
        if !(cfg!(debug_assertions) || std::env::var_os("HEX_PROFILE_FRAMES").is_some()) {
            return;
        }
        let render_elapsed = self.started.elapsed();
        PANE_PROFILES.with(|profiles| {
            let mut profiles = profiles.borrow_mut();
            let entry = profiles.entry(self.pane).or_insert_with(|| PaneProfile {
                render_ring: FrameTimeRing::new(),
                interval_ring: FrameTimeRing::new(),
            });
            entry.render_ring.record_duration(render_elapsed);
            let _ = entry.interval_ring.record_frame_interval(self.started);
            if entry.render_ring.total_samples().is_multiple_of(60)
                && let Some(render) = entry.render_ring.summary()
            {
                let interval = entry.interval_ring.summary();
                tracing::debug!(
                    pane = self.pane,
                    render_p50_us = render.p50_us,
                    render_p95_us = render.p95_us,
                    render_max_us = render.max_us,
                    frame_p50_ms = interval.map(|s| s.p50_us as f32 / 1000.0).unwrap_or(0.0),
                    frame_p95_ms = interval.map(|s| s.p95_us as f32 / 1000.0).unwrap_or(0.0),
                    frame_max_ms = interval.map(|s| s.max_us as f32 / 1000.0).unwrap_or(0.0),
                    "pane frame timing"
                );
            }
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn spec_maps_duration_and_bounce_like_swiftui() {
        assert!((presets::SNAP.omega() - std::f32::consts::TAU / 0.25).abs() < 1e-4);
        assert_eq!(presets::SNAP.zeta(), 1.0);
        assert!((presets::MORPH.zeta() - 0.84).abs() < 1e-5);
        assert!(Spec::new(0.3, -0.2).zeta() > 1.0);
    }

    #[test]
    fn step_preserves_semigroup_across_damping_regimes() {
        for spec in [
            presets::SNAP,
            presets::MORPH,
            presets::POP,
            Spec::new(0.35, -0.25),
        ] {
            let mut split = Spring {
                x: -2.5,
                v: 4.0,
                target: 6.0,
            };
            let mut direct = split;
            split.step(spec, 0.013);
            split.step(spec, 0.021);
            direct.step(spec, 0.034);

            assert!(
                (split.x - direct.x).abs() < 2e-4,
                "position mismatch for {spec:?}: {} vs {}",
                split.x,
                direct.x
            );
            assert!(
                (split.v - direct.v).abs() < 2e-4,
                "velocity mismatch for {spec:?}: {} vs {}",
                split.v,
                direct.v
            );
        }
    }

    #[test]
    fn trajectories_match_at_60_120_and_240_hz() {
        let simulate = |hz: usize, spec: Spec| {
            let mut s = Spring::at(0.0);
            s.to(1.0);
            let steps = hz / 2; // 0.5 seconds
            let dt = 1.0 / hz as f32;
            for _ in 0..steps {
                s.step(spec, dt);
            }
            (s.x, s.v)
        };
        for spec in [presets::SNAP, presets::SETTLE, presets::MORPH] {
            let s60 = simulate(60, spec);
            let s120 = simulate(120, spec);
            let s240 = simulate(240, spec);
            assert!((s60.0 - s120.0).abs() < 1e-4);
            assert!((s120.0 - s240.0).abs() < 1e-4);
            assert!((s60.1 - s120.1).abs() < 1e-3);
        }
    }

    #[test]
    fn retargeting_mid_flight_preserves_position_and_velocity() {
        let mut s = Spring::at(0.0);
        s.to(1.0);
        s.step(presets::SETTLE, 0.08);
        let pos_before = s.x;
        let vel_before = s.v;
        assert!(pos_before > 0.1 && pos_before < 0.9);
        assert!(vel_before > 0.5);

        // Retarget back to 0.0 mid-flight: position and velocity must carry over.
        s.to(0.0);
        assert_eq!(s.x, pos_before);
        assert_eq!(s.v, vel_before);

        // On the very next millisecond, forward momentum continues smoothly before reversing.
        s.step(presets::SETTLE, 0.001);
        assert!(s.x > pos_before);
    }

    #[test]
    fn settling_requires_both_low_displacement_and_low_velocity() {
        let spec = presets::SETTLE;
        let moving = Spring {
            x: 1.0,
            v: 1.0,
            target: 1.0,
        };
        assert!(!moving.settled(spec, DEFAULT_EPSILON));

        let resting = Spring {
            x: 1.0002,
            v: 0.001,
            target: 1.0,
        };
        assert!(resting.settled(spec, DEFAULT_EPSILON));
    }

    #[test]
    fn snap_edge_pair_locks_stationary_edge() {
        let bottom = 100.0;
        for step in 0..30 {
            let top = 20.0 + step as f32 * 0.37;
            let (y, h) = snap_edge_pair(top, bottom);
            assert!(((y + h) - bottom).abs() < 1e-5);
        }
    }

    #[test]
    fn frame_time_ring_computes_p50_p95_and_max() {
        let mut ring = FrameTimeRing::new();
        for i in 1..=100 {
            ring.record_duration(Duration::from_micros(i * 100));
        }
        let summary = ring.summary().unwrap();
        assert_eq!(summary.count, 100);
        assert_eq!(summary.p50_us, 5000);
        assert_eq!(summary.p95_us, 9500);
        assert_eq!(summary.max_us, 10000);
    }

    #[test]
    fn fun_pane_mode_and_height_traces_are_continuous_without_jumps() {
        const WIDTHS: [f32; 9] = [66.0, 56.0, 52.0, 62.0, 76.0, 54.0, 60.0, 86.0, 44.0];
        let target_rect = |idx: usize| -> (f32, f32) {
            let left = 2.0 + WIDTHS[..idx].iter().sum::<f32>();
            (left, WIDTHS[idx])
        };

        // 1. Mode pill trace: Muted (1, width 56) -> Robot (5, width 54) must never pump width
        // up to 76px (Chipmunk) and must move monotonically in `left` and `width`.
        let (start_left, start_width) = target_rect(1);
        let (end_left, end_width) = target_rect(5);
        let mut left = Spring::at(start_left);
        let mut width = Spring::at(start_width);
        let mut prev_left = left.x;
        let mut prev_width = width.x;
        for _ in 0..30 {
            left.step_to(end_left, presets::SNAP, 1.0 / 60.0);
            width.step_to(end_width, presets::SNAP, 1.0 / 60.0);
            assert!(left.x >= prev_left - 1e-4, "left reversed");
            assert!(
                width.x <= prev_width + 1e-4 && width.x >= end_width - 1e-4,
                "width pumped outside [54, 56]: {}",
                width.x
            );
            prev_left = left.x;
            prev_width = width.x;
        }

        // 2. Mode controls height trace: Muffled (row1=58, row2=52, comfort=0) ->
        // Muted (row1=58, row2=0, comfort=52) must keep total height invariant at 110px.
        let mut row1 = Spring::at(1.0);
        let mut row2 = Spring::at(1.0);
        let mut comfort = Spring::at(0.0);
        for _ in 0..30 {
            row1.step_to(1.0, presets::REFLOW, 1.0 / 60.0);
            row2.step_to(0.0, presets::REFLOW, 1.0 / 60.0);
            comfort.step_to(1.0, presets::REFLOW, 1.0 / 60.0);
            let h = ((58.0 * row1.x) * 64.0).round() / 64.0
                + ((52.0 * row2.x) * 64.0).round() / 64.0
                + ((52.0 * comfort.x) * 64.0).round() / 64.0;
            assert!(
                (h - 110.0).abs() <= 0.04,
                "Muffled -> Muted height jumped: {h}"
            );
        }

        // Muted -> Robot: total height glides monotonically from 110px -> 58px with no >6px frame step.
        let mut prev_h = 110.0_f32;
        for _ in 0..40 {
            row1.step_to(1.0, presets::REFLOW, 1.0 / 60.0);
            row2.step_to(0.0, presets::REFLOW, 1.0 / 60.0);
            comfort.step_to(0.0, presets::REFLOW, 1.0 / 60.0);
            let h = ((58.0 * row1.x) * 64.0).round() / 64.0
                + ((52.0 * row2.x) * 64.0).round() / 64.0
                + ((52.0 * comfort.x) * 64.0).round() / 64.0;
            assert!(h <= prev_h + 1e-4, "height reversed: {h} > {prev_h}");
            assert!(
                (prev_h - h) <= 6.0,
                "single-frame height jump too large: {} -> {}",
                prev_h,
                h
            );
            prev_h = h;
        }
        assert!((prev_h - 58.0).abs() < 0.2);
    }
}
