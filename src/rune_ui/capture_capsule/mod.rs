//! The capture capsule on Rune: Scrawl's capsule (its picker, recording,
//! processing, and finished states, and the router's list) in a
//! click-through overlay at the top of the screen. Only its shell and
//! controls take clicks; it never takes focus.
//!
//! [`flow`] decides what it shows from the capture's stages, the router's
//! answer, and the capture input's keys; [`surface`] decides where every
//! piece rests and which spring moves it; the view below only draws that,
//! and Rune's springs animate between commits. The red dot, the spinner,
//! the shimmer on "Processing…", and the edge glow animate through `Live`
//! values a meter thread publishes, and the timer through a `LiveText`,
//! without re-running the view.
//!
//! Ported from Scrawl's Rune capsule (`scrawl-pill-lab`, `rune_pill`); its
//! colours are the OpenCode theme's, so it follows the system appearance.

pub(crate) mod feed;
pub(crate) mod flow;
pub(crate) mod ink;
mod list_view;
pub(crate) mod notch;
pub(crate) mod picker;
#[cfg(target_os = "macos")]
pub(crate) mod playground;
pub(crate) mod surface;
pub(crate) mod tuning;
mod undo_ring;
mod view;

// Its scenes and goldens are macOS's chips and labels.
#[cfg(all(test, target_os = "macos"))]
mod tests;

use std::cell::Cell;
use std::sync::atomic::{AtomicBool, AtomicU8, AtomicU32, Ordering};
use std::sync::{Arc, Mutex, OnceLock};
use std::time::Duration;

use rune::{
    Color, Compositor, Cx, El, Frame, LineCap, Live, LiveText, OverlayHandle, Path, Runtime, Shape,
    SpringPair, Transition, Tx, canvas, col, el, row, text,
};
use rune_kit::TextInput;

use self::flow::{Action, Cue, Done, Flow, Footer, Input, Lead, List, Mode, Pres};
use self::list_view::*;
use self::surface::{
    CHIP_H, CHIP_PX, HEADER_H, HINT_GAP, Id, LEAD_TEXT_X, LIST_PAD, LIST_Y, PANEL_H, PANEL_W,
    PROCESSING_TEXT_X, ROW_H, SEG_ICON, SEG_ICON_GAP, Shown, Surface, THUMB_H, THUMB_W, THUMB_X,
    chord, clock_style, clock_width, corner, done_style, done_text_x, ellipsize, measure, medium,
    processing, small,
};
use self::tuning::ListFooter;
use self::view::*;
use super::controls::Paint;
use crate::desktop::i18n::{tr, tr_fill};

/// Peak blur (pt) of the picker's badge and the list's rows softening in
/// place.
const PRESENCE_BLUR: f32 = 1.5;
/// How long the capsule's exit gets before its overlay hides, in animation
/// seconds.
const HIDE_AFTER_S: f64 = 0.5;
/// The list's corners, and its rows'.
const LIST_RADIUS: f32 = surface::SHELL_RADIUS;
const ROW_RADIUS: f32 = 8.0;
const ROW_PX: f32 = 10.0;
/// Under the list header's words.
const HEADER_PB: f32 = 4.0;
/// Between one hint and the next.
const HINTS_GAP: f32 = 16.0;
/// The list footer's band (A) and well (B).
const BAND_H: f32 = 32.0;
const WELL_H: f32 = 28.0;
/// The list slides down this far as it fades in.
const LIST_DROP: f32 = 6.0;

// ─── Live values ─────────────────────────────────────────────────────────────

/// Room for "Processing…" in the longest language.
const GLYPHS: usize = 24;

/// "Processing…" split into glyphs, each with its x and width as the whole
/// label shapes them.
fn processing_glyphs() -> [(char, f32, f32); GLYPHS] {
    let mut out = [(' ', 0.0, 0.0); GLYPHS];
    let label = processing();
    for (k, (at, c)) in label.char_indices().enumerate().take(GLYPHS) {
        let end = at + c.len_utf8();
        let x = if at == 0 {
            0.0
        } else {
            measure(&label[..at], medium())
        };
        out[k] = (c, x, measure(&label[..end], medium()) - x);
    }
    out
}

/// What the meter thread animates.
#[derive(Clone)]
struct Meter {
    /// The dot's core breath, and its ripple ring's scale and fade (one
    /// 1.6 s cycle).
    core: Live<f32>,
    ring_scale: Live<f32>,
    ring_alpha: Live<f32>,
    /// The spinner's eight dots.
    spin: [Live<f32>; 8],
    /// "Processing…" one glyph each, lit by a band of light passing across.
    shimmer: [Live<f32>; GLYPHS],
    /// Each glyph's centre across the label, 0…1.
    shimmer_at: [f32; GLYPHS],
    /// The glow inside the capsule's edge while it changes width.
    glow: Live<f32>,
    /// A morph's settle time in seconds (`f32` bits) for the thread to
    /// start a glow; 0 when none.
    kick: Arc<AtomicU32>,
    /// 0 idle, 1 recording, 2 processing.
    activity: Arc<AtomicU8>,
    reduced: Arc<AtomicBool>,
    glowing: Arc<Mutex<Glow>>,
    thread: Arc<OnceLock<std::thread::Thread>>,
}

#[derive(Default)]
struct Glow {
    /// When the morph started, its settle time, and the level it rose from.
    since: Option<(f64, f32, f32)>,
    level: f32,
}

/// The edge glow `dt` seconds into a morph that settles in `settle`: from
/// `from` up to 1 over 0.05 s, then critically damped back to 0.
fn glow_at(dt: f32, settle: f32, from: f32) -> f32 {
    const RISE: f32 = 0.05;
    if dt < RISE {
        return from + (1.0 - from) * dt / RISE;
    }
    let w = std::f32::consts::TAU / settle.max(0.2);
    let t = dt - RISE;
    (1.0 + w * t) * (-w * t).exp()
}

impl Meter {
    fn new() -> Self {
        let meter = Self {
            core: Live::new(1.0),
            ring_scale: Live::new(0.4),
            ring_alpha: Live::new(0.0),
            spin: std::array::from_fn(|_| Live::new(0.18)),
            shimmer: std::array::from_fn(|_| Live::new(1.0)),
            shimmer_at: {
                let total = measure(processing(), medium()).max(1.0);
                let glyphs = processing_glyphs();
                std::array::from_fn(|k| (glyphs[k].1 + glyphs[k].2 * 0.5) / total)
            },
            glow: Live::new(0.0),
            kick: Arc::default(),
            activity: Arc::default(),
            reduced: Arc::default(),
            glowing: Arc::default(),
            thread: Arc::default(),
        };
        meter.publish(0.0);
        meter
    }

    /// The looping values at animation second `t`. Under Reduce Motion the
    /// dot holds still, its ring and the shimmer stop, and only the spinner
    /// turns: it is what says work is happening.
    fn publish(&self, t: f64) {
        let reduced = self.reduced.load(Ordering::Relaxed);
        let phase = (t / 1.6).fract() as f32;
        let breath = 0.5 + 0.5 * (std::f32::consts::TAU * phase).cos();
        self.core
            .set(if reduced { 1.0 } else { 0.78 + 0.22 * breath });
        self.ring_scale.set((4.0 + 6.0 * phase) / 10.0);
        self.ring_alpha.set(if reduced {
            0.0
        } else {
            0.34 * (1.0 - phase) * (1.0 - phase)
        });
        let head = (t * 8.0).rem_euclid(8.0) as f32;
        for (k, dot) in self.spin.iter().enumerate() {
            let phase = (head - k as f32).rem_euclid(8.0);
            dot.set((1.0 - phase / 7.5).clamp(0.18, 1.0));
        }
        // A 1.8 s ease-in-out sweep from left of the label to right of it.
        let sweep = (t / 1.8).fract() as f32;
        let centre = -0.3 + 1.6 * (0.5 - 0.5 * (std::f32::consts::PI * sweep).cos());
        for (glyph, x) in self.shimmer.iter().zip(self.shimmer_at) {
            let d = (x - centre) / 0.14;
            glyph.set(if reduced {
                1.0
            } else {
                0.62 + 0.38 * (-d * d).exp()
            });
        }
    }

    /// Everything at animation second `t`: the loops while something runs,
    /// and the edge glow while a morph lights it. False when idle.
    fn advance(&self, t: f64) -> bool {
        let active = self.activity.load(Ordering::Relaxed) != 0;
        if active {
            self.publish(t);
        }
        let kick = f32::from_bits(self.kick.swap(0, Ordering::AcqRel));
        let mut glow = self.glowing.lock().unwrap_or_else(|e| e.into_inner());
        if kick > 0.0 {
            glow.since = Some((t, kick, glow.level));
        }
        if let Some((since, settle, from)) = glow.since {
            glow.level = glow_at((t - since) as f32, settle, from);
            if t - since > 0.05 && glow.level < 0.002 {
                glow.level = 0.0;
                glow.since = None;
            }
            self.glow.set(glow.level);
        }
        active || glow.since.is_some()
    }

    /// Publishes at 120 Hz while something animates, the way an audio
    /// callback would, and parks while nothing does.
    fn spawn(self) {
        let meter = self.clone();
        let handle = std::thread::Builder::new()
            .name("hex-capsule-meter".into())
            .spawn(move || {
                let mut t = 0.0_f64;
                let mut last = std::time::Instant::now();
                loop {
                    let now = std::time::Instant::now();
                    t += (now - last).as_secs_f64();
                    last = now;
                    if meter.advance(t) {
                        std::thread::sleep(Duration::from_millis(8));
                    } else {
                        std::thread::park();
                        last = std::time::Instant::now();
                    }
                }
            });
        if let Ok(handle) = handle {
            let _ = self.thread.set(handle.thread().clone());
        }
    }

    fn wake(&self) {
        if let Some(thread) = self.thread.get() {
            thread.unpark();
        }
    }
}

/// What the capsule draws beside its `Surface`: the live values and the
/// timer's text.
pub(crate) struct Animated {
    meter: Meter,
    /// The timer, so a new second redraws without re-running the view; only
    /// a change of length (9:59 → 10:00) re-lays the capsule out.
    clock: LiveText,
    /// `surface.morphs` as last passed to the meter's edge glow.
    morphs: u32,
    /// The shown capture's first frame, decoded, for [`Lead::Thumbnail`].
    thumb: Option<rune::Image>,
    /// The Undo chip's countdown, while it has one.
    ring: Option<undo_ring::Ring>,
}

impl Animated {
    fn new(surface: &Surface, spawn: bool) -> Self {
        let meter = Meter::new();
        if spawn {
            meter.clone().spawn();
        }
        Self {
            meter,
            clock: LiveText::new(surface.clock.clone()),
            morphs: surface.morphs,
            thumb: None,
            ring: None,
        }
    }

    /// Shows `clock`; the surface re-lays out only when its length changes.
    /// True when the view must re-run.
    fn set_clock(&self, surface: &mut Surface, clock: &str, now: f64) -> bool {
        self.clock.set(clock.to_owned());
        surface.set_clock(clock, now)
    }

    /// Tells the meter what the surface shows: which loops run, at what
    /// speed, and when a morph lights the edge.
    fn sync(&mut self, surface: &Surface) {
        let activity = match surface.pres {
            Pres::Recording { .. } => 1,
            Pres::Processing => 2,
            _ => 0,
        };
        let was = self.meter.activity.swap(activity, Ordering::Relaxed);
        self.meter.reduced.store(surface.reduced, Ordering::Relaxed);
        let mut wake = was != activity && activity != 0;
        if surface.morphs != self.morphs {
            self.morphs = surface.morphs;
            self.meter
                .kick
                .store(surface.morph_settle.max(0.2).to_bits(), Ordering::Release);
            wake = true;
        }
        if wake {
            self.meter.wake();
        }
    }
}

// ─── State ───────────────────────────────────────────────────────────────────

/// The pieces the pointer can be on: the pill, and the list hanging under
/// it, which sits outside the pill's clip.
#[derive(Clone, Copy)]
enum Part {
    Pill,
    List,
}

/// The host's side of a press or key: what to do with the capture.
pub(crate) type Act = Arc<dyn Fn(Action) + Send + Sync>;

/// The capsule overlay's state.
pub(crate) struct CapsuleState {
    flow: Flow,
    surface: Surface,
    live: Animated,
    /// Inputs from other threads, applied on the next poll.
    inbox: Vec<Input>,
    /// Clock seconds the recording started at, for the timer.
    recording_since: Option<f64>,
    /// Animation seconds the capsule went to `Hidden` at.
    hidden_since: Option<f64>,
    /// The latest poll's clock.
    now: f64,
    /// Reduce Motion, as the view last saw it.
    reduced: Cell<bool>,
    act: Option<Act>,
    overlay: Option<OverlayHandle<CapsuleState>>,
    shown: bool,
    follow: bool,
    /// A capture started: the capsule moves to the display under the
    /// pointer, even while still up on another one.
    replace: bool,
    /// Where the overlay was last placed for ([`Surface::anchor`]).
    placed: surface::Anchor,
    /// The keys Hex's tap hands the capsule ([`Flow::keys`]): the overlay
    /// never takes focus.
    keys: Option<(
        crate::capture_host::CapsuleKeys,
        crate::capture_host::CapsuleKeyGrab,
    )>,
    /// Whether the pointer is on each [`Part`].
    hovering: [bool; 2],
    /// The on-screen picker, and the mode it was last told to pick.
    picker: Option<picker::PickerOverlay>,
    picker_mode: Option<Mode>,
    /// A still for goldens: it rests, nothing plays.
    still: bool,
    /// Draws a plain backdrop behind the capsule (stills and clips), so its
    /// shadow and edge read.
    backdrop: bool,
    /// Plays the flow's cues; none in stills and tests, which stay silent.
    sound: Option<Arc<dyn Fn(Cue) + Send + Sync>>,
    /// The playground's controls and fake host (`hex capsule-playground`).
    #[cfg(target_os = "macos")]
    playground: Option<Box<playground::Playground>>,
}

impl CapsuleState {
    fn new(spawn: bool) -> Self {
        let surface = Surface::new();
        let live = Animated::new(&surface, spawn);
        Self {
            flow: Flow::new().suggesting_on_route(!cfg!(target_os = "linux")),
            surface,
            live,
            inbox: Vec::new(),
            recording_since: None,
            hidden_since: Some(f64::NEG_INFINITY),
            now: 0.0,
            reduced: Cell::new(false),
            act: None,
            overlay: None,
            shown: false,
            follow: false,
            replace: false,
            placed: surface::Anchor::Top,
            keys: None,
            hovering: [false; 2],
            picker: None,
            picker_mode: None,
            still: false,
            backdrop: false,
            sound: None,
            #[cfg(target_os = "macos")]
            playground: None,
        }
    }

    /// `pres` at rest with the timer at `clock`, as a still.
    #[cfg_attr(
        any(not(test), not(target_os = "macos")),
        allow(dead_code, reason = "headless stills and motion runs")
    )]
    pub(crate) fn still(pres: Pres, clock: &str, thumb: Option<rune::Image>) -> Self {
        let mut state = Self::new(false);
        state.surface.clock = clock.to_owned();
        state.live.clock.set(clock.to_owned());
        state.live.thumb = thumb;
        let pres = state.with_thumb(pres);
        state.surface = std::mem::take(&mut state.surface).settled(pres);
        // An Undo ring two seconds in, so a still shows it draining.
        if let Pres::Done(Done {
            countdown: Some(end),
            ..
        }) = &state.surface.pres
        {
            let now = end - flow::undo_s() + 2.0;
            undo_ring::sync(&mut state.live.ring, &state.surface.pres, now, false);
        }
        state.live.meter.publish(0.4);
        // The shimmer's band would leave most of a still's label dim.
        for glyph in &state.live.meter.shimmer {
            glyph.set(1.0);
        }
        state.still = true;
        state.backdrop = true;
        state
    }

    fn dispatch(&mut self, actions: Vec<Action>) {
        if let Some(act) = &self.act {
            for action in actions {
                act(action);
            }
        }
        self.sound();
    }

    /// Plays the cues the flow has due.
    fn sound(&mut self) {
        for cue in self.flow.take_cues() {
            if let Some(sound) = &self.sound {
                sound(cue);
            }
        }
    }

    fn apply(&mut self, input: Input, now: f64) {
        if matches!(
            input,
            Input::Stage(scrawl_capture::capture::Stage::Recording)
        ) {
            self.recording_since = Some(now);
            self.replace = true;
            self.live.thumb = None;
        }
        // Send to… bringing a closed capsule back: under the pointer.
        if input == Input::OpenList && !self.shown {
            self.replace = true;
        }
        let actions = self.flow.input(input, now);
        self.dispatch(actions);
    }

    /// A press on `id`, at the latest poll's clock.
    fn click(&mut self, id: Id, tx: &mut Tx) {
        let now = self.now;
        let actions = self.flow.click(id, now);
        self.dispatch(actions);
        self.present(now);
        self.live.sync(&self.surface);
        tx.wake_after(Duration::ZERO);
    }

    /// The pointer came onto `part` of the capsule or left it: chips never
    /// fold under it, and a press there is never a click away.
    fn hover(&mut self, part: Part, on: bool, tx: &mut Tx) {
        let was = self.hovering != [false; 2];
        self.hovering[part as usize] = on;
        let on = self.hovering != [false; 2];
        if on == was {
            return;
        }
        let now = self.now;
        let actions = self.flow.hover(on, now);
        self.dispatch(actions);
        self.present(now);
        self.live.sync(&self.surface);
        tx.wake_after(Duration::ZERO);
    }

    /// The pointer moved on the capsule: typing over its suggestions may
    /// search now, so the tap's keys change at once.
    fn pointer_moved(&mut self, tx: &mut Tx) {
        if self.flow.pointer_moved() {
            take_keys(self);
            tx.wake_after(Duration::ZERO);
        } else {
            tx.live_only();
        }
    }

    /// Moves the surface to what the flow shows. True when it changed.
    fn present(&mut self, now: f64) -> bool {
        let pres = self.with_thumb(self.flow.pres(now));
        if !matches!(pres, Pres::Recording { .. } | Pres::Selecting { .. }) {
            self.recording_since = None;
        }
        if pres == self.surface.pres {
            return false;
        }
        self.surface.set(pres, now);
        true
    }

    /// The capture's first frame takes the check's place once decoded; a
    /// warning keeps its glyph.
    fn with_thumb(&self, mut pres: Pres) -> Pres {
        if let Pres::Done(done) = &mut pres
            && done.lead == Lead::Check
            && !done.no_frame
            && self
                .live
                .thumb
                .as_ref()
                .is_some_and(|image| image.is_ready() && image.error().is_none())
        {
            done.lead = Lead::Thumbnail;
        }
        pres
    }
}

/// The overlay's frame hook: inputs, the flow's timers, the surface's due
/// steps, the clock, and showing or hiding the overlay.
fn poll(s: &mut CapsuleState, now: f64, tx: &mut Tx) -> bool {
    if s.still {
        return false;
    }
    s.now = now;
    s.surface.reduced = s.reduced.get();
    // The app follows `capture.capsule_flow`; the playground and tests set
    // the tuning themselves.
    if s.overlay.is_some() {
        let flow = crate::app_settings::capsule_flow();
        let mut t = tuning::get();
        if t.flow != flow {
            t.flow = flow;
            tuning::set(t);
        }
    }
    let mut changed = false;
    for input in std::mem::take(&mut s.inbox) {
        s.apply(input, now);
        changed = true;
    }
    changed |= s.flow.tick(now);
    s.sound();
    changed |= s.present(now);
    changed |= s.surface.tick(now);

    let earliest = |wake: Option<f64>, at: Option<f64>| match (wake, at) {
        (Some(a), Some(b)) => Some(a.min(b)),
        (a, b) => a.or(b),
    };
    let mut wake = s.surface.next_due().map(|due| due - now);
    wake = earliest(wake, s.flow.next_due(now).map(|due| due - now));
    if let Some(since) = s.recording_since {
        let secs = (now - since).max(0.0);
        let whole = secs as u64;
        let clock = format!("{}:{:02}", whole / 60, whole % 60);
        changed |= s.live.set_clock(&mut s.surface, &clock, now);
        wake = earliest(wake, Some(whole as f64 + 1.0 - secs));
    }
    s.live.sync(&s.surface);
    undo_ring::sync(&mut s.live.ring, &s.surface.pres, now, s.surface.reduced);

    // The overlay stays up while the capsule leaves, then hides.
    let hidden = matches!(s.surface.pres, Pres::Hidden);
    let since = match (hidden, s.hidden_since) {
        (true, None) => *s.hidden_since.insert(now),
        (true, Some(since)) => since,
        (false, _) => {
            s.hidden_since = None;
            now
        }
    };
    let leaving = since + HIDE_AFTER_S - now;
    if hidden && leaving > 0.0 {
        wake = earliest(wake, Some(leaving));
    }
    // Shown in the same poll as the step that brings the capsule in, so
    // Rune reveals the hidden pose and animates the entry from it.
    let shown = !hidden || leaving > 0.0;
    if s.shown != shown {
        s.shown = shown;
        if let Some(overlay) = &s.overlay {
            if shown {
                overlay.show();
            } else {
                overlay.hide();
            }
        }
    }
    crate::capture_host::set_capsule_pill(shown.then_some(s.surface.shell));
    // Shown, it stays put; a new capture brings it to the pointer.
    if std::mem::take(&mut s.replace)
        && let Some(overlay) = &s.overlay
    {
        overlay.set_display(rune::DisplayTarget::UnderPointer);
    }
    if s.placed != s.surface.anchor {
        s.placed = s.surface.anchor;
        if let Some(overlay) = &s.overlay {
            place(overlay, s.placed);
        }
    }
    // The picker follows the pointer between displays.
    let follow = matches!(s.surface.pres, Pres::Selecting { .. });
    if s.follow != follow {
        s.follow = follow;
        if let Some(overlay) = &s.overlay {
            overlay.set_follow_pointer(follow);
        }
    }
    take_keys(s);
    // The on-screen picker shows what the capsule picks.
    let picking = match s.surface.pres {
        Pres::Selecting { mode, .. } => Some(mode),
        _ => None,
    };
    if s.picker_mode != picking {
        s.picker_mode = picking;
        if let Some(picker) = &s.picker {
            picker.set(picking);
        }
    }
    if let Some(wake) = wake {
        tx.wake_after(Duration::from_secs_f64(wake.max(0.0)));
    }
    for event in s.surface.events.drain(..) {
        tx.trace_event(event);
    }
    if std::mem::take(&mut s.surface.instant) {
        tx.instant();
    }
    changed
}

/// Moves the overlay to where the capsule rests: under the menu bar, at
/// the screen's top edge (the notch), or at the pointer (the cursor's
/// card, whose rect is global, so on the primary display's origin).
fn place(overlay: &OverlayHandle<CapsuleState>, anchor: surface::Anchor) {
    use rune::{Align, DisplayTarget, OverlayPlacement, ScreenEdge};
    let edge = |margin| OverlayPlacement::Edge {
        edge: ScreenEdge::Top,
        margin,
        align: Align::Center,
        inset: 0.0,
    };
    match anchor {
        surface::Anchor::Top => {
            overlay.set_display(DisplayTarget::UnderPointer);
            overlay.set_placement(Some(edge(PANEL_MARGIN)));
        }
        surface::Anchor::Notch => {
            overlay.set_display(DisplayTarget::UnderPointer);
            overlay.set_placement(Some(edge(0.0)));
        }
        surface::Anchor::Cursor => {
            let Some([x, y]) = scrawl_capture::screens::pointer() else {
                return;
            };
            overlay.set_display(DisplayTarget::Primary);
            overlay.set_placement(Some(OverlayPlacement::Rect {
                x: x as f32 + surface::POINTER_OFFSET[0] - surface::CURSOR_X,
                y: y as f32 + surface::POINTER_OFFSET[1] - surface::CURSOR_Y,
                width: PANEL_W,
                height: PANEL_H,
            }));
        }
    }
}

/// Takes, changes, or lets go of the keys the capsule wants from Hex's tap.
fn take_keys(s: &mut CapsuleState) {
    let wanted = s.flow.keys();
    match (wanted, &mut s.keys) {
        (None, held) => *held = None,
        (Some(keys), Some((held, grab))) => {
            if *held != keys {
                grab.set(keys);
                *held = keys;
            }
        }
        (Some(keys), held @ None) => {
            if let Some(overlay) = &s.overlay {
                let capsule = CaptureCapsule {
                    overlay: overlay.clone(),
                };
                let grab = crate::capture_host::take_keys(keys, move |key| {
                    capsule.input(key_input(key));
                });
                *held = Some((keys, grab));
            }
        }
    }
}

fn key_input(key: crate::capture_host::CapsuleKey) -> Input {
    use crate::capture_host::CapsuleKey;
    match key {
        CapsuleKey::Shortcut => Input::Record,
        CapsuleKey::Escape => Input::Escape,
        CapsuleKey::Text(typed) => Input::Type(typed),
        CapsuleKey::Backspace => Input::Backspace,
        CapsuleKey::Up => Input::Move(-1),
        CapsuleKey::Down => Input::Move(1),
        CapsuleKey::Return { shift } => Input::Send { always: shift },
        CapsuleKey::Tab { shift } => Input::Cycle(if shift { -1 } else { 1 }),
        CapsuleKey::Press => Input::ClickAway,
        CapsuleKey::Shift(down) => Input::Shift(down),
    }
}

// ─── The overlay ─────────────────────────────────────────────────────────────

/// The capsule's overlay, as the app holds it.
#[derive(Clone)]
pub(crate) struct CaptureCapsule {
    overlay: OverlayHandle<CapsuleState>,
}

/// Panel points from the screen's top to the panel's: the capsule rests
/// just under the menu bar.
pub(crate) const PANEL_MARGIN: f32 = 14.0;

impl CaptureCapsule {
    /// Adds the capsule's overlay, hidden. It holds no GPU surface while
    /// hidden, nor on macOS and Windows a window (#1315, #1308). `act` carries out what a
    /// press or key asks for; it runs on the main thread, so it must only
    /// hand the action on.
    pub(crate) fn install<S: 'static>(
        app: &mut rune::App<S>,
        act: impl Fn(Action) + Send + Sync + 'static,
    ) -> Self {
        use rune::{
            Align, DisplayTarget, OverlayInput, OverlayLevel, OverlayOptions, OverlayPlacement,
            OverlaySpec, ScreenEdge,
        };
        let spec = OverlaySpec::new("Hex capture", (PANEL_W, PANEL_H), view)
            .options(OverlayOptions {
                placement: Some(OverlayPlacement::Edge {
                    edge: ScreenEdge::Top,
                    margin: PANEL_MARGIN,
                    align: Align::Center,
                    inset: 0.0,
                }),
                display: DisplayTarget::UnderPointer,
                level: OverlayLevel::AboveOverlays,
                exclude_from_capture: !crate::capture_host::own_windows_recorded(),
                input: OverlayInput::Regions,
                prewarm: true,
                ..Default::default()
            })
            .release_gpu_when_hidden()
            // Its panel is made for each show, so the entry plays from the
            // hidden pose rather than settling.
            .animate_reveal()
            .poll(poll)
            .compositor(TraceFrames::default())
            .on_created(|c| {
                if let Some(window) = c.window_number {
                    crate::capture_host::exclude_window(
                        crate::capture_host::OwnWindow::Capsule,
                        window,
                    );
                }
            });
        let mut state = CapsuleState::new(true);
        state.act = Some(Arc::new(act));
        state.sound = Some(Arc::new(|cue| crate::feedback::play(tone(cue))));
        let overlay = app.overlay(spec, state);
        let handle = overlay.clone();
        overlay.update(move |s, _| s.overlay = Some(handle));
        Self { overlay }
    }

    /// Shows `picker` while the capsule picks a window, an area, or the
    /// screen.
    pub(crate) fn attach(&self, picker: picker::PickerOverlay) {
        self.overlay.update(move |s, tx| {
            s.picker_mode = None;
            s.picker = Some(picker);
            tx.wake_after(Duration::ZERO);
        });
    }

    #[cfg_attr(not(target_os = "macos"), allow(dead_code))]
    /// Lets screen recordings see the capsule, or keeps it out.
    pub(crate) fn show_in_recordings(&self, shown: bool) {
        self.overlay.set_exclude_from_capture(!shown);
    }

    /// Shows `image`, decoded, as the finished capsule's lead until the
    /// next capture starts; safe from any thread.
    pub(crate) fn thumbnail(&self, image: rune::Image) {
        self.overlay.update(move |s, tx| {
            s.live.thumb = Some(image);
            tx.wake_after(Duration::ZERO);
        });
    }

    /// Moves the capsule; safe from any thread.
    pub(crate) fn input(&self, input: Input) {
        self.overlay.update(move |s, tx| {
            s.inbox.push(input);
            tx.wake_after(Duration::ZERO);
        });
    }
}

/// A rounded rectangle on the icon grid.
fn rounded(x: f32, y: f32, w: f32, h: f32, r: f32) -> Path {
    let k = 0.552_284_8 * r;
    let (x1, y1) = (x + w, y + h);
    Path::new()
        .move_to([x + r, y])
        .line_to([x1 - r, y])
        .cubic_to([x1 - r + k, y], [x1, y + r - k], [x1, y + r])
        .line_to([x1, y1 - r])
        .cubic_to([x1, y1 - r + k], [x1 - r + k, y1], [x1 - r, y1])
        .line_to([x + r, y1])
        .cubic_to([x + r - k, y1], [x, y1 - r + k], [x, y1 - r])
        .line_to([x, y + r])
        .cubic_to([x, y + r - k], [x + r - k, y], [x + r, y])
        .close()
}

/// A square icon button as tall as the capsule's chips, the icon centred.
fn icon_button(p: Paint, icon: El) -> El {
    row()
        .center()
        .size(CHIP_H, CHIP_H)
        .radius(surface::CHIP_RADIUS)
        .hover(p.k.c.hover)
        .press(p.k.c.pressed)
        .child(icon)
}

/// Scrawl's line icons on their 24 × 24 grid; `Id::Why` is the warning.
fn icon(id: Id, width: f32, color: Color) -> Shape {
    let mut paths: Vec<Path> = Vec::new();
    let line = |a: [f32; 2], b: [f32; 2]| Path::new().move_to(a).line_to(b);
    match id {
        Id::Mode(Mode::Area) => {
            for (a, c, b) in [
                ([3.0, 8.0], [3.0, 3.0], [8.0, 3.0]),
                ([16.0, 3.0], [21.0, 3.0], [21.0, 8.0]),
                ([21.0, 16.0], [21.0, 21.0], [16.0, 21.0]),
                ([8.0, 21.0], [3.0, 21.0], [3.0, 16.0]),
            ] {
                paths.push(Path::new().move_to(a).line_to(c).line_to(b));
            }
        }
        Id::Mode(Mode::Window) => {
            paths.push(rounded(2.5, 4.0, 19.0, 16.0, 2.5));
            paths.push(line([2.5, 9.5], [21.5, 9.5]));
        }
        Id::Mode(Mode::Screen) => {
            paths.push(rounded(2.0, 3.0, 20.0, 14.0, 2.0));
            paths.push(line([8.0, 21.0], [16.0, 21.0]));
            paths.push(line([12.0, 17.0], [12.0, 21.0]));
        }
        Id::Mode(Mode::Iphone) => {
            paths.push(rounded(6.5, 2.0, 11.0, 20.0, 3.0));
            paths.push(line([10.5, 18.5], [13.5, 18.5]));
        }
        Id::Why => {
            paths.push(line([12.0, 4.0], [12.0, 14.0]));
            paths.push(line([12.0, 19.5], [12.0, 19.6]));
        }
        // A clipboard: its board open where the clip sits on it.
        Id::Paste => {
            paths.push(
                Path::new()
                    .move_to([16.0, 4.0])
                    .line_to([17.5, 4.0])
                    .quad_to([20.0, 4.0], [20.0, 6.5])
                    .line_to([20.0, 19.5])
                    .quad_to([20.0, 22.0], [17.5, 22.0])
                    .line_to([6.5, 22.0])
                    .quad_to([4.0, 22.0], [4.0, 19.5])
                    .line_to([4.0, 6.5])
                    .quad_to([4.0, 4.0], [6.5, 4.0])
                    .line_to([8.0, 4.0]),
            );
            paths.push(rounded(8.0, 2.0, 8.0, 4.0, 1.0));
        }
        // An eye, two circular arcs meeting at its corners, and its pupil:
        // look at the capture.
        Id::Pen => {
            // A pencil, tip at the bottom left.
            paths.push(
                Path::new()
                    .move_to([16.5, 3.5])
                    .line_to([20.5, 7.5])
                    .line_to([8.0, 20.0])
                    .line_to([3.5, 20.5])
                    .line_to([4.0, 16.0])
                    .close(),
            );
            paths.push(line([13.5, 6.5], [17.5, 10.5]));
        }
        Id::Inspect => {
            paths.push(
                Path::new()
                    .move_to([2.06, 12.0])
                    .cubic_to([3.7, 7.97], [7.65, 5.3], [12.0, 5.3])
                    .cubic_to([16.35, 5.3], [20.3, 7.97], [21.94, 12.0])
                    .cubic_to([20.3, 16.03], [16.35, 18.7], [12.0, 18.7])
                    .cubic_to([7.65, 18.7], [3.7, 16.03], [2.06, 12.0])
                    .close(),
            );
            paths.push(rounded(9.0, 9.0, 6.0, 6.0, 3.0));
        }
        _ => {}
    }
    paths
        .iter()
        .fold(Shape::new().view_box(24.0, 24.0), |shape, path| {
            shape.stroke_path(path, width, LineCap::Round, color)
        })
}

// ─── Motion trace ────────────────────────────────────────────────────────────

/// The capsule's channels for motion-lint, on top of what Rune's scene
/// trace writes for every element: the blur anyone sees, the shell's
/// opacity, and its width while it shows.
pub(crate) fn trace_channels(rt: &mut Runtime<CapsuleState>) {
    let elements = rt.elements();
    let Some(shell) = elements.iter().find(|e| e.key.as_deref() == Some("shell")) else {
        return;
    };
    let (opacity, width) = (shell.opacity, shell.rect[2]);
    // An item fading out keeps its full blur until it unmounts, at opacity
    // 0, so weight each by how visible it is.
    let blur = elements
        .iter()
        .filter(|e| e.key.as_deref().is_some_and(|k| k.contains('#')))
        .map(|e| e.blur * e.opacity.clamp(0.0, 1.0))
        .fold(0.0, f32::max);
    rt.trace_channel("content.blur", "level", blur);
    rt.trace_channel("content.alpha", "level", opacity);
    // Entry lands its pose while invisible: a width jump there isn't
    // motion anyone sees.
    if opacity > 0.001 {
        rt.trace_channel("shell.width", "position", width);
    }
}

/// Draws Rune's frame and, with `HEX_CAPSULE_TRACE=<file>` set when the
/// overlay starts, writes the motion-lint trace.
#[derive(Default)]
struct TraceFrames {
    started: bool,
}

impl Compositor<CapsuleState> for TraceFrames {
    fn render(&mut self, f: &mut Frame<'_, CapsuleState>) {
        f.draw_ui();
        if !std::mem::replace(&mut self.started, true)
            && let Some(path) = std::env::var_os("HEX_CAPSULE_TRACE")
            && let Ok(file) = std::fs::File::create(path)
        {
            let header = rune_motion::TraceHeader {
                launch: "hex-capture-capsule".into(),
                width: PANEL_W,
                height: PANEL_H,
                hz: 120.0,
                scale: f.rt.env.scale,
            };
            if let Ok(trace) = rune_motion::MotionTrace::new(file, &header) {
                f.rt.scene.set_trace(Some(trace.host("capsule")));
            }
        }
        if f.rt.scene.trace_mut().is_some() {
            trace_channels(f.rt);
        }
    }
}
