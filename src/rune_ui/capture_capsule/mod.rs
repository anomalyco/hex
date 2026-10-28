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
pub(crate) mod picker;
pub(crate) mod playground;
pub(crate) mod surface;
pub(crate) mod tuning;

#[cfg(test)]
mod tests;

use std::cell::Cell;
use std::sync::atomic::{AtomicBool, AtomicU8, AtomicU32, Ordering};
use std::sync::{Arc, Mutex, OnceLock};
use std::time::Duration;

use rune::{
    Color, Compositor, Cx, El, Frame, LineCap, Live, LiveText, OverlayHandle, Path, Runtime, Shape,
    SpringPair, Transition, Tx, canvas, col, el, row, text,
};

use self::flow::{Action, Cue, Done, Flow, Footer, Input, Lead, List, Mode, Pres};
use self::surface::{
    CHIP_H, CHIP_PX, HEADER_H, HINT_GAP, Id, LEAD_TEXT_X, LIST_PAD, LIST_Y, MEDIUM, PANEL_H,
    PANEL_W, PROCESSING, PROCESSING_TEXT_X, ROW_H, SEG_ICON, SEG_ICON_GAP, SMALL, STRONG, Shown,
    Surface, THUMB_H, THUMB_W, THUMB_X, TIMER, UNDO_LABEL, chord, clock_width, corner, done_style,
    done_text_x, ellipsize, measure,
};
use self::tuning::ListFooter;
use super::controls::Paint;

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
const NUMBER_W: f32 = 10.0;
/// The rows' numbers: the names' weight in tabular digits, so 1 and 4
/// fill the same column.
const NUMBER: rune::TextStyle = MEDIUM.features("'tnum' 1");
/// Above a section after the first, and under its header's words.
const SECTION_GAP: f32 = 4.0;
const HEADER_PB: f32 = 4.0;
/// Between one hint and the next.
const HINTS_GAP: f32 = 16.0;
/// The list footer's band (A) and well (B).
const BAND_H: f32 = 32.0;
const WELL_H: f32 = 28.0;
/// The list slides down this far as it fades in.
const LIST_DROP: f32 = 6.0;

// ─── Live values ─────────────────────────────────────────────────────────────

const GLYPHS: usize = 11;

/// "Processing…" split into glyphs, each with its x and width as the whole
/// label shapes them.
fn processing_glyphs() -> [(char, f32, f32); GLYPHS] {
    let mut out = [(' ', 0.0, 0.0); GLYPHS];
    for (k, (at, c)) in PROCESSING.char_indices().enumerate().take(GLYPHS) {
        let end = at + c.len_utf8();
        let x = if at == 0 {
            0.0
        } else {
            measure(&PROCESSING[..at], STRONG)
        };
        out[k] = (c, x, measure(&PROCESSING[..end], STRONG) - x);
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
                let total = measure(PROCESSING, STRONG).max(1.0);
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
    playground: Option<Box<playground::Playground>>,
}

impl CapsuleState {
    fn new(spawn: bool) -> Self {
        let surface = Surface::new();
        let live = Animated::new(&surface, spawn);
        Self {
            flow: Flow::new(),
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
            picker: None,
            picker_mode: None,
            still: false,
            backdrop: false,
            sound: None,
            playground: None,
        }
    }

    /// `pres` at rest with the timer at `clock`, as a still.
    #[cfg_attr(
        not(test),
        expect(dead_code, reason = "headless stills and motion runs")
    )]
    pub(crate) fn still(pres: Pres, clock: &str, thumb: Option<rune::Image>) -> Self {
        let mut state = Self::new(false);
        state.surface.clock = clock.to_owned();
        state.live.clock.set(clock.to_owned());
        state.live.thumb = thumb;
        let pres = state.with_thumb(pres);
        state.surface = std::mem::take(&mut state.surface).settled(pres);
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
        // The list shortcut bringing a closed capsule back: under the pointer.
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

    /// The pointer came onto the capsule or left it: chips never fold under it.
    fn hover(&mut self, on: bool, tx: &mut Tx) {
        let now = self.now;
        self.flow.hover(on, now);
        self.present(now);
        self.live.sync(&self.surface);
        tx.wake_after(Duration::ZERO);
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
    // Rune reveals the prewarmed hidden pose and animates the entry from it.
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
        CapsuleKey::OpenList => Input::OpenList,
        CapsuleKey::Text(typed) => Input::Type(typed),
        CapsuleKey::Backspace => Input::Backspace,
        CapsuleKey::Up => Input::Move(-1),
        CapsuleKey::Down => Input::Move(1),
        CapsuleKey::Return { shift } => Input::Send { always: shift },
        CapsuleKey::Tab { shift } => Input::Cycle(if shift { -1 } else { 1 }),
        CapsuleKey::Digit(digit) => Input::SendRow(usize::from(digit.saturating_sub(1))),
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
const PANEL_MARGIN: f32 = 14.0;

impl CaptureCapsule {
    /// Adds the capsule's overlay, prewarmed and hidden. `act` carries out
    /// what a press or key asks for; it runs on the main thread, so it
    /// must only hand the action on.
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

// ─── View ────────────────────────────────────────────────────────────────────

/// The sound for a cue, in the dictation tones' family.
fn tone(cue: Cue) -> crate::feedback::Tone {
    use crate::feedback::Tone;
    match cue {
        Cue::Start => Tone::CaptureStart,
        Cue::Stop => Tone::CaptureStop,
        Cue::Sent => Tone::CaptureSent,
        Cue::TakenBack => Tone::CaptureUndo,
        Cue::Failed => Tone::CaptureFailed,
    }
}

fn view(s: &CapsuleState, cx: &mut Cx<CapsuleState>) -> El {
    if s.playground.is_some() {
        return playground::view(s, cx);
    }
    s.reduced.set(cx.is_reduced_motion());
    let p = Paint::new(cx.system_dark(), cx.env().scale);
    let capsule = capsule(p, &s.surface, &s.live, cx);
    if s.backdrop {
        let mut backdrop = el().size(PANEL_W, PANEL_H).fill(p.k.c.layer_03);
        // Stills and clips have no pointer: the cursor's card draws one
        // where the app would find it.
        if s.surface.anchor == surface::Anchor::Cursor {
            backdrop = backdrop.child(pointer(p));
        }
        if tuning::get().flow == tuning::CapsuleFlow::MenuBar {
            backdrop = backdrop.child(menu_item(p));
        }
        return backdrop.child(capsule);
    }
    capsule
}

/// Hex's menu-bar item where the menu-bar flow folds the capsule, for
/// stills and clips, which have no menu bar.
fn menu_item(p: Paint) -> El {
    let [x, y, w, h] = surface::MENU_FOLD;
    el().key("menu-item")
        .absolute(x, y)
        .size(w, h)
        .radius(h * 0.5)
        .border(1.2, p.muted())
        .child(
            el().absolute(w * 0.5 - 2.5, h * 0.5 - 2.5)
                .size(5.0, 5.0)
                .radius(2.5)
                .fill(p.muted()),
        )
}

/// The arrow pointer just above and left of the cursor's card.
fn pointer(p: Paint) -> El {
    let arrow = Path::new()
        .move_to([1.0, 1.0])
        .line_to([1.0, 16.0])
        .line_to([5.0, 12.5])
        .line_to([7.6, 18.2])
        .line_to([10.2, 17.0])
        .line_to([7.7, 11.6])
        .line_to([12.6, 11.2])
        .close();
    let shape = Shape::new()
        .view_box(14.0, 20.0)
        .fill_path(&arrow, p.k.c.text_base)
        .stroke_path(&arrow, 1.2, LineCap::Round, p.k.c.bg_base);
    canvas(shape)
        .key("pointer")
        .absolute(
            surface::CURSOR_X - surface::POINTER_OFFSET[0],
            surface::CURSOR_Y - surface::POINTER_OFFSET[1],
        )
        .size(14.0, 20.0)
}

/// How content comes and goes, all of it together over the walls' move:
/// a blur, a scale toward the capsule's middle, so the pieces shrink and
/// grow as one, and a fade. Reduce Motion keeps the fade alone.
fn presence(surface: &Surface, rect: [f32; 4], reduced: bool) -> Transition {
    let t = tuning::get();
    let springs = SpringPair::new(t.content_in, t.content_out);
    if reduced {
        return Transition::opacity().springs(springs);
    }
    let [sx, sy, sw, sh] = surface.shell;
    let [x, y, w, h] = rect;
    let middle = rune::Vec2::new(
        (sx + sw * 0.5 - x) / w.max(1.0),
        (sy + sh * 0.5 - y) / h.max(1.0),
    );
    Transition::scale_anchor(t.content_scale, middle)
        .combined(Transition::blur(t.content_blur))
        .springs(springs)
}

/// The capsule in its `PANEL_W` × `PANEL_H` panel. Its live chips and
/// buttons and the list are the only `.hit()` boxes: a click anywhere else
/// on it reaches the app beneath, and hover still comes from the pointer.
fn capsule(p: Paint, surface: &Surface, live: &Animated, cx: &mut Cx<CapsuleState>) -> El {
    let reduced = cx.is_reduced_motion() || surface.reduced;
    if surface.reduced {
        cx.reduced_motion(true);
    }
    let mut root = el()
        .key("panel")
        .size(PANEL_W, PANEL_H)
        .child(shell(p, surface, &live.meter));
    // Everything inside is masked by the capsule as it moves: a box on the
    // shell's rect and spring clips to its rounded shape, and the box
    // inside it moves back by the same amount on the same spring, so the
    // content stays where it's laid out while the walls pass over it.
    let [x, y, w, h] = surface.shell;
    let mut inner = el()
        .key("inner")
        .absolute(-x, -y)
        .size(PANEL_W, PANEL_H)
        .motion(surface.shell_motion);
    if let Some(highlight) = &surface.highlight {
        inner = inner.child(
            placed(el(), highlight)
                .trace_name("highlight")
                .radius(surface::CHIP_RADIUS)
                .fill(p.k.c.layer_02)
                .box_shadow(rune::Shadow {
                    spread: p.hair,
                    color: p.k.c.border_base,
                    ..Default::default()
                })
                .transition(presence(surface, highlight.rect, reduced)),
        );
    }
    // Content entering over content that leaves crossfades additively,
    // without a midpoint dip. The highlight is a shared background, so it
    // stays outside; nothing inside overlaps at rest.
    let mut content = el()
        .key("content")
        .absolute(0.0, 0.0)
        .size(PANEL_W, PANEL_H)
        .dissolve();
    for shown in &surface.items {
        let transition = presence(surface, shown.rect, reduced);
        content = content.child(item(p, surface, live, shown).transition(transition));
    }
    let clip = el()
        .key("clip")
        .absolute(x, y)
        .size(w, h)
        .motion(surface.shell_motion)
        .clip_rounded(corner(h))
        .on_hover_change(|s: &mut CapsuleState, on, tx| s.hover(on, tx));
    root = root.child(clip.child(inner.child(content)));
    if let Pres::Done(Done {
        open: Some((_, list)),
        ..
    }) = &surface.pres
    {
        root = root.child(list_view(p, list, x, w, reduced));
    }
    root
}

fn placed(e: El, shown: &Shown) -> El {
    let [x, y, w, h] = shown.rect;
    e.key(shown.key.clone())
        .absolute(x, y)
        .size(w, h)
        .motion(shown.motion)
}

fn shell(p: Paint, surface: &Surface, meter: &Meter) -> El {
    let k = p.k;
    let [x, y, w, h] = surface.shell;
    let mut body = el()
        .key("shell")
        .trace_name("shell")
        .absolute(x, y)
        .size(w, h)
        .radius(corner(h))
        .fill(k.c.bg_base);
    for shadow in &k.elevation.overlay {
        body = body.box_shadow(*shadow);
    }
    // The glow inside the edge while the shape morphs, stronger at the top.
    body = body.child(
        el().key("glow")
            .absolute(0.0, 0.0)
            .size(w, h)
            .radius(corner(h))
            .box_shadow(rune::Shadow {
                y: 1.5,
                blur: 8.0,
                color: k.alpha(k.c.text_base, if k.dark { 0.10 } else { 0.06 }),
                inset: true,
                ..Default::default()
            })
            .live_opacity(&meter.glow)
            .motion(surface.shell_motion),
    );
    if k.dark {
        // Dark, a hairline lit from above, as Scrawl's capsule has.
        body = body.child(
            el().key("rim")
                .absolute(0.0, 0.0)
                .size(w, h)
                .radius(corner(h))
                .border_gradient(0.5, k.light(0.12), k.light(0.03))
                .motion(surface.shell_motion),
        );
    }
    body.opacity(if surface.shell_on { 1.0 } else { 0.0 })
        .motion(surface.shell_motion)
        .opacity_motion(surface.shell_fade)
        .role("group")
        .label("Capture")
}

fn label(content: &str, style: rune::TextStyle, color: Color) -> El {
    text(content).font(style).color(color)
}

fn press(id: Id) -> impl Fn(&mut CapsuleState, &mut Tx) + 'static {
    move |s: &mut CapsuleState, tx: &mut Tx| s.click(id, tx)
}

fn item(p: Paint, surface: &Surface, live: &Animated, shown: &Shown) -> El {
    let c = &p.k.c;
    let id = shown.id;
    let base = placed(row().center(), shown).trace_name(id.trace_key());
    match id {
        Id::Mode(mode) => {
            let fg = if shown.active { p.text() } else { p.muted() };
            let mut chip = base
                .radius(surface::CHIP_RADIUS)
                .gap(SEG_ICON_GAP)
                .child(canvas(icon(id, SEG_ICON * 2.5 / 24.0, fg)).size(SEG_ICON, SEG_ICON))
                .child(label(mode.label(), MEDIUM, fg))
                .press(c.pressed);
            if !shown.active {
                chip = chip.hover(c.hover);
            }
            chip.role("button")
                .label(mode.label())
                .hit()
                .on_press_tx(press(id))
        }
        Id::Record | Id::Finish => {
            let name = if id == Id::Record { "Record" } else { "Finish" };
            base.radius(surface::CHIP_RADIUS)
                .fill(c.bg_contrast)
                .hover(c.contrast_hover)
                .press(c.contrast_pressed)
                .child(label(name, STRONG, c.text_contrast))
                .role("button")
                .label(name)
                .hit()
                .on_press_tx(press(id))
        }
        Id::Cancel | Id::Keep => {
            let name = if id == Id::Cancel {
                "Cancel"
            } else {
                surface::KEEP_LABEL
            };
            base.radius(surface::CHIP_RADIUS)
                .hover(c.hover)
                .press(c.pressed)
                .child(label(name, MEDIUM, p.muted()))
                .role("button")
                .label(name)
                .hit()
                .on_press_tx(press(id))
        }
        // Destructive, so the danger ink on the neutral chip, never the
        // accent.
        Id::Discard => base
            .radius(surface::CHIP_RADIUS)
            .fill(c.layer_02)
            .hover(c.hover)
            .press(c.pressed)
            .child(label(surface::DISCARD_LABEL, STRONG, p.danger()))
            .role("button")
            .label(surface::DISCARD_LABEL)
            .hit()
            .on_press_tx(press(id)),
        Id::Close => base.child(
            row()
                .center()
                .size(28.0, 28.0)
                .radius(ICON_BUTTON_RADIUS)
                .hover(c.hover)
                .press(c.pressed)
                .child(canvas(icon(id, 12.0 * 2.4 / 24.0, p.muted())).size(12.0, 12.0))
                .role("button")
                .label("Close")
                .hit()
                .on_press_tx(press(id)),
        ),
        Id::Lead => placed(el(), shown)
            .trace_name("lead")
            .child(dot(p, &live.meter))
            .child(timer(p, surface, live)),
        Id::Processing => placed(el(), shown)
            .trace_name("processing")
            .child(spinner(p, &live.meter))
            .child(processing_label(p, &live.meter)),
        Id::Done => {
            let lead = match &surface.pres {
                Pres::Done(done) => done.lead,
                _ => Lead::Check,
            };
            let ink = match tuning::get().top_line {
                tuning::TopLine::Quiet => p.muted(),
                tuning::TopLine::Icons => p.text(),
            };
            let mut done = placed(el(), shown).trace_name("done").child(glyph(
                p,
                lead,
                live.thumb.as_ref().map(rune::Image::pixels),
            ));
            if !shown.ident.is_empty() {
                done = done.child(
                    row()
                        .center_y()
                        .absolute(done_text_x(lead), 0.0)
                        .h(CHIP_H)
                        .child(label(&shown.ident, done_style(), ink)),
                );
            }
            let sends = matches!(&surface.pres, Pres::Done(done) if done.sends);
            if sends {
                return done
                    .radius(surface::CHIP_RADIUS)
                    .hover(c.hover)
                    .press(c.pressed)
                    .role("button")
                    .label(shown.ident.clone())
                    .hit()
                    .on_press_tx(press(id));
            }
            done.role("text").label(if shown.ident.is_empty() {
                "Copied".to_owned()
            } else {
                shown.ident.clone()
            })
        }
        Id::Next => keyed(p, base, &["⇥"], "Next", MEDIUM).on_press_tx(press(id)),
        // A chip beside the frame in the menu-bar flow, its number after.
        Id::Pick(row) if tuning::get().flow == tuning::CapsuleFlow::MenuBar => {
            let key = (row + 1).to_string();
            keyed(p, base, &[&key], shown.text(), MEDIUM).on_press_tx(press(id))
        }
        Id::Pick(row) => pick_row(p, base, shown, row),
        Id::CardHints => base
            .justify_start()
            .px(ROW_PX)
            .child(hint(p, "Send", shown.text()))
            .role("text")
            .label(format!("Send {}", shown.text())),
        Id::Why => placed(row().center_y(), shown)
            .trace_name("why")
            .child(quiet(p, shown.text(), done_style())),
        // The quiet way out of a send that just went.
        // Click only: Escape meant for another app was taking sends back.
        Id::Undo => keyed(p, base, &[], UNDO_LABEL, MEDIUM).on_press_tx(press(id)),
        Id::Ask => {
            let keys = match &surface.pres {
                Pres::Done(done) => done.ask_keys.as_slice(),
                _ => &[],
            };
            let keys: Vec<&str> = keys.iter().map(String::as_str).collect();
            keyed(p, base, &keys, &shown.ident, MEDIUM).on_press_tx(press(id))
        }
        Id::Paste | Id::Inspect => {
            let (name, help) = if id == Id::Paste {
                ("Paste", "Paste the capture")
            } else {
                ("Inspect", "Open the capture")
            };
            base.child(
                row()
                    .center()
                    .size(28.0, 28.0)
                    .radius(ICON_BUTTON_RADIUS)
                    .hover(c.hover)
                    .press(c.pressed)
                    .child(canvas(icon(id, 16.0 * 1.75 / 24.0, p.muted())).size(16.0, 16.0))
                    .role("button")
                    .label(name)
                    .help(help)
                    .hit()
                    .on_press_tx(press(id)),
            )
        }
        Id::Field => {
            let typed = match &surface.pres {
                Pres::Done(Done {
                    open: Some((typed, _)),
                    ..
                }) => typed.as_str(),
                _ => "",
            };
            field(p, base, typed)
        }
        // Rows live in the list, never in the capsule.
        Id::Row(_) => base,
    }
}

/// A guess in the cursor's or the notch's card, as the list's rows read:
/// its number, its name, and its reason, the best one's in the accent.
fn pick_row(p: Paint, base: El, shown: &Shown, row: usize) -> El {
    let c = &p.k.c;
    let room = shown.rect[2] - 5.0 * ROW_PX - NUMBER_W - measure(shown.text(), MEDIUM);
    let note = ellipsize(shown.note(), SMALL, room.max(24.0));
    base.justify_start()
        .px(ROW_PX)
        .gap(ROW_PX)
        .radius(ROW_RADIUS + 2.0)
        .hover(c.hover)
        .press(c.pressed)
        .child(label(&(row + 1).to_string(), NUMBER, p.faint()).w(NUMBER_W))
        .child(label(shown.text(), MEDIUM, p.text()))
        .child(el().grow(1.0))
        .child(
            label(&note, SMALL, if row == 0 { p.accent() } else { p.muted() })
                .offset(0.0, (MEDIUM.size - SMALL.size) * BASELINE_DROP),
        )
        .role("button")
        .label(shown.text().to_owned())
        .hit()
        .on_press_tx(press(Id::Pick(row)))
}

/// A chip's label, then its shortcut as one quiet word ("Send to… ⌥⇧V").
fn keyed(p: Paint, base: El, keys: &[&str], name: &str, style: rune::TextStyle) -> El {
    let mut chip = base
        .radius(surface::CHIP_RADIUS)
        .gap(HINT_GAP)
        .hover(p.k.c.hover)
        .press(p.k.c.pressed)
        .child(label(name, style, p.muted()));
    if !keys.is_empty() {
        chip = chip.child(quiet(p, &chord(keys), style));
    }
    chip.role("button").label(name.to_owned()).hit()
}

/// Small faint text (a shortcut, a note) beside words in `beside`, on
/// their baseline: centring a smaller line in the same row would lift it.
fn quiet(p: Paint, content: &str, beside: rune::TextStyle) -> El {
    label(content, SMALL, p.faint()).offset(0.0, (beside.size - SMALL.size) * BASELINE_DROP)
}

/// A centred line's baseline sits this many ems below its middle: half
/// the theme face's ascender less its descender (Inter: 0.969, 0.242).
const BASELINE_DROP: f32 = 0.364;

/// What a hint does, then its keys in one small rounded rect ("Send
/// [⏎]"), never a keycap per key: the words in the muted ink, the keys
/// fainter on a quiet fill or inside a hairline.
fn hint(p: Paint, copy: &str, keys: &str) -> El {
    let mut hint = row().center_y().gap(HINT_GAP);
    if !copy.is_empty() {
        hint = hint.child(label(copy, SMALL, p.muted()));
    }
    if !keys.is_empty() {
        let k = p.k;
        hint = hint.child(
            row()
                .center()
                .h(KEY_BOX_H)
                .px(KEY_BOX_PX)
                .radius(KEY_BOX_RADIUS)
                .fill(k.alpha(k.c.text_base, 0.07))
                .child(label(keys, SMALL, p.muted())),
        );
    }
    hint
}

/// A [`hint`]'s width.
fn hint_width(copy: &str, keys: &str) -> f32 {
    let gap = if copy.is_empty() || keys.is_empty() {
        0.0
    } else {
        HINT_GAP
    };
    let rect = if keys.is_empty() {
        0.0
    } else {
        measure(keys, SMALL) + 2.0 * KEY_BOX_PX
    };
    measure(copy, SMALL) + gap + rect
}

/// The rounded rect around a hint's keys.
const KEY_BOX_H: f32 = 18.0;
const KEY_BOX_PX: f32 = 5.0;
const KEY_BOX_RADIUS: f32 = 5.0;

/// The open capsule's field: what's typed and the caret, or the
/// placeholder.
fn field(p: Paint, base: El, typed: &str) -> El {
    let caret = el()
        .key("caret")
        .size(1.5, 15.0)
        .radius(0.75)
        .fill(p.accent());
    let inner = if typed.is_empty() {
        row()
            .center_y()
            .gap(1.0)
            .child(caret)
            .child(label("Send to…", MEDIUM, p.faint()))
    } else {
        row()
            .center_y()
            .gap(1.0)
            .child(label(typed, MEDIUM, p.text()))
            .child(caret)
    };
    base.trace_name("field")
        .px(CHIP_PX)
        .radius(surface::CHIP_RADIUS)
        .fill(p.k.c.layer_01)
        .child(inner.grow(1.0))
        .role("textbox")
        .label("Send to")
}

/// The finished capsule's check, or its warning: its ink spans 12…20,
/// where the recording dot's does.
fn glyph(p: Paint, lead: Lead, thumb: Option<&rune::Pixels>) -> El {
    let (color, id) = match (lead, thumb) {
        (Lead::Thumbnail, Some(pixels)) => return thumbnail(p, pixels),
        (Lead::Check | Lead::Thumbnail, _) => (p.success(), Id::Done),
        (Lead::Warning, _) => (p.k.c.warning.fg, Id::Why),
    };
    canvas(icon(id, CHECK * 2.7 / 24.0, color))
        .absolute(8.0, (CHIP_H - CHECK) * 0.5)
        .size(CHECK, CHECK)
}

const CHECK: f32 = 16.0;
/// A 28 pt icon button's corners, concentric with the shell's.
const ICON_BUTTON_RADIUS: f32 = surface::CHIP_RADIUS - 2.0;
const THUMB_RADIUS: f32 = 4.0;

/// The capture's first frame, cropped to fill a small rounded card, with a
/// hairline over its edge so a white page still reads on the capsule.
fn thumbnail(p: Paint, pixels: &rune::Pixels) -> El {
    el().absolute(THUMB_X, (CHIP_H - THUMB_H) * 0.5)
        .size(THUMB_W, THUMB_H)
        .radius(THUMB_RADIUS)
        .fill(p.k.c.layer_02)
        .pixels_fit(pixels, rune::SurfaceFit::Fill)
        .child(
            el().absolute(0.0, 0.0)
                .size(THUMB_W, THUMB_H)
                .radius(THUMB_RADIUS)
                .border(p.hair, p.k.alpha(p.k.c.text_base, 0.14)),
        )
}

/// The timer in tabular digits, bound to its `LiveText` in a box as wide
/// as the clock.
fn timer(p: Paint, surface: &Surface, live: &Animated) -> El {
    row().center_y().absolute(LEAD_TEXT_X, 0.0).h(CHIP_H).child(
        label(&surface.clock, TIMER, p.text())
            .live_text(&live.clock)
            .w(clock_width(&surface.clock)),
    )
}

/// "Processing…", each glyph at its place in the shaped label and lit by
/// the shimmer.
fn processing_label(p: Paint, meter: &Meter) -> El {
    let mut glyphs = el()
        .absolute(PROCESSING_TEXT_X, 0.0)
        .h(CHIP_H)
        .w(measure(PROCESSING, STRONG))
        .role("text")
        .label(PROCESSING);
    for ((c, x, w), live) in processing_glyphs().into_iter().zip(&meter.shimmer) {
        glyphs = glyphs.child(
            row()
                .center_y()
                .absolute(x, 0.0)
                .h(CHIP_H)
                .w(w)
                .child(label(c.encode_utf8(&mut [0; 4]), STRONG, p.text()))
                .live_opacity(live),
        );
    }
    glyphs
}

/// The recording dot: a breathing core with a glow, and a ripple ring
/// every 1.6 s.
fn dot(p: Paint, meter: &Meter) -> El {
    let red = p.danger();
    let ring = 20.0;
    let core = 8.0;
    el().absolute(0.0, 0.0)
        .size(CHIP_H, CHIP_H)
        .child(
            el().absolute((CHIP_H - ring) * 0.5, (CHIP_H - ring) * 0.5)
                .size(ring, ring)
                .radius(ring * 0.5)
                .fill(p.k.alpha(red, 0.5))
                .live_scale(&meter.ring_scale)
                .live_opacity(&meter.ring_alpha),
        )
        .child(
            el().absolute((CHIP_H - core) * 0.5, (CHIP_H - core) * 0.5)
                .size(core, core)
                .radius(core * 0.5)
                .fill(red)
                .shadow(0.0, 7.0, red.alpha(0.55))
                .live_opacity(&meter.core),
        )
}

/// Eight dots around a 6.2 pt circle with a bright head going round.
fn spinner(p: Paint, meter: &Meter) -> El {
    let mut dots = el().absolute(0.0, 0.0).size(CHIP_H, CHIP_H);
    let d = 2.7;
    for (k, live) in meter.spin.iter().enumerate() {
        let angle = k as f32 * std::f32::consts::FRAC_PI_4 - std::f32::consts::FRAC_PI_2;
        let (x, y) = (
            CHIP_H * 0.5 + 6.2 * angle.cos(),
            CHIP_H * 0.5 + 6.2 * angle.sin(),
        );
        dots = dots.child(
            el().absolute(x - d * 0.5, y - d * 0.5)
                .size(d, d)
                .radius(d * 0.5)
                .fill(p.text())
                .live_opacity(live),
        );
    }
    dots
}

/// The list hanging under the open capsule, `x` and `w` its shell's.
fn list_view(p: Paint, list: &List, x: f32, w: f32, reduced: bool) -> El {
    let k = p.k;
    let h = list_height(list);
    // Rows and headers keep their identity across a filter: the ones that
    // stay slide to their new slot sharp, the rest soften out in place.
    let blur = if reduced { 0.0 } else { PRESENCE_BLUR };
    let scale = if reduced { 1.0 } else { 0.96 };
    let t = tuning::get();
    let springs = SpringPair::new(t.content_in, t.content_out);
    let in_place = Transition::blur(blur)
        .combined(Transition::scale(scale))
        .springs(springs);
    let mut body = col().absolute(0.0, 0.0).w(w).px(LIST_PAD).pt(LIST_PAD);
    let mut n = 0;
    let suggesting = list.suggesting();
    let saving = list.always.as_ref().is_some_and(|(_, shift)| *shift);
    let first = if suggesting { "Send to" } else { "Suggested" };
    let sections = [(first, &list.suggested), ("Recent", &list.recent)];
    for (title, rows) in sections {
        if rows.is_empty() {
            continue;
        }
        // A header sits nearer the rows it heads than the ones above it.
        body = body.child(
            row()
                .key(format!("section-{title}"))
                .items_end()
                .h(HEADER_H)
                .mt(if n == 0 { 0.0 } else { SECTION_GAP })
                .px(ROW_PX)
                .pb(HEADER_PB)
                .child(label(title, k.ty.small_medium, p.faint()))
                .transition(in_place),
        );
        for dest in rows {
            // The name keeps its width; a long reason is cut first, to no
            // less than two fifths of the row.
            let content = w - 2.0 * LIST_PAD - 2.0 * ROW_PX - NUMBER_W - 3.0 * ROW_PX;
            let why_room = (content - measure(&dest.name, MEDIUM)).max(content * 0.4);
            let why = ellipsize(&dest.why, SMALL, why_room);
            let max = content - measure(&why, SMALL);
            let mut entry = row()
                .key(format!("row-{}", dest.name))
                .trace_name("row")
                .center_y()
                .h(ROW_H)
                .px(ROW_PX)
                .gap(ROW_PX)
                .radius(ROW_RADIUS)
                .hover(k.c.hover)
                .press(k.c.pressed)
                .child(
                    label(&(n + 1).to_string(), NUMBER, p.faint())
                        .key(format!("number-{n}"))
                        .w(NUMBER_W)
                        .transition(Transition::opacity().springs(springs)),
                )
                .child(label(&ellipsize(&dest.name, MEDIUM, max), MEDIUM, p.text()))
                .child(el().grow(1.0))
                // The best suggestion is marked by its reason, on the
                // name's baseline; Shift down, so is where a rule would send.
                .child(
                    label(
                        &why,
                        SMALL,
                        if (suggesting && n == 0) || saving && n == list.selected {
                            p.accent()
                        } else {
                            p.muted()
                        },
                    )
                    .offset(0.0, (MEDIUM.size - SMALL.size) * BASELINE_DROP),
                )
                .role("button")
                .label(dest.name.clone())
                .hit()
                .on_press_tx(press(Id::Row(n)))
                .transition(in_place);
            if n == list.selected {
                entry = entry.fill(k.c.layer_02);
            }
            body = body.child(entry);
            n += 1;
        }
    }
    let footer = match &list.always {
        Some((site, true)) => saving_hint(p, list, site, w),
        _ if suggesting => suggest_hints(p, list, w),
        _ => send_hints(p, list, w),
    };
    list_panel(p, body, footer, x, w, h, blur)
}

/// The list's own hints, as one quiet line: what each key does, then the
/// key ("Send ⏎"); the highlighted row already says where.
fn send_hints(p: Paint, list: &List, w: f32) -> El {
    let room = w - 2.0 * LIST_PAD - 2.0 * ROW_PX;
    let send = "Send".to_owned();
    let (keys, second): (&[&str], String) = match &list.footer {
        Footer::KeepCopied => (&["esc"], "Keep".to_owned()),
        Footer::Keep(to) => (&["esc"], format!("Keep {to}")),
        Footer::Always(_) => (&["⇧", "⏎"], "Always".to_owned()),
        Footer::Suggesting => (&["esc"], "Keep".to_owned()),
    };
    let keys = chord(keys);
    let used = hint_width(&send, "⏎") + HINTS_GAP;
    let second = ellipsize(
        &second,
        SMALL,
        room - used - hint_width("", &keys) - HINT_GAP,
    );
    row()
        .center_y()
        .gap(HINTS_GAP)
        .child(hint(p, &send, "⏎").key("send"))
        .child(hint(p, &second, &keys).key("second"))
}

/// The suggestions' hints: the digits that send, then Shift for a rule
/// when one can be made.
fn suggest_hints(p: Paint, list: &List, w: f32) -> El {
    let rows = list.rows().count();
    let digits = if rows > 1 {
        format!("1–{rows}")
    } else {
        "1".to_owned()
    };
    let room = w - 2.0 * LIST_PAD - 2.0 * ROW_PX;
    let always = list.always.is_some()
        && hint_width("Send", &digits) + HINTS_GAP + hint_width("Always", "⇧") <= room;
    let say_send = hint_width("Send", &digits) <= room;
    let send = hint(p, if say_send { "Send" } else { "" }, &digits).key("send");
    row()
        .center_y()
        .gap(HINTS_GAP)
        .child(send)
        .children(always.then(|| hint(p, "Always", "⇧").key("second")))
        .child(el().grow(1.0))
}

/// Shift down: the footer says where the rule will send `site`, each in
/// a badge with its kind's icon: "Always send [🌐 site] to [💬 SEER]",
/// following the highlighted row.
fn saving_hint(p: Paint, list: &List, site: &str, w: f32) -> El {
    let room = w - 2.0 * LIST_PAD - 2.0 * ROW_PX;
    let to = list.selected_dest().map(|dest| dest.name.as_str());
    let words = measure("Always send", SMALL) + measure("to", SMALL) + 3.0 * BADGE_GAP;
    let badges =
        if to.is_some() { 2.0 } else { 1.0 } * (2.0 * BADGE_PX + BADGE_ICON + BADGE_ICON_GAP);
    let left = (room - words - badges).max(0.0);
    let to_w = to.map_or(0.0, |to| measure(to, SMALL));
    let site_w = measure(site, SMALL);
    let (site_room, to_room) = if site_w + to_w <= left {
        (site_w, to_w)
    } else {
        let to_room = to_w.min(left * 0.4);
        (left - to_room, to_room)
    };
    let mut hint = row()
        .center_y()
        .gap(BADGE_GAP)
        .key("saving")
        .child(label("Always send", SMALL, p.muted()))
        .child(badge(p, Badge::of_site(site), &ellipsize(site, SMALL, site_room)).key("site"));
    hint = match to {
        Some(to) => hint
            .child(label("to", SMALL, p.muted()))
            .child(badge(p, Badge::Chat, &ellipsize(to, SMALL, to_room)).key("to")),
        None => hint.child(label("to…", SMALL, p.muted())),
    };
    hint.child(el().grow(1.0))
}

const BADGE_H: f32 = 20.0;
const BADGE_PX: f32 = 6.0;
const BADGE_RADIUS: f32 = 5.0;
const BADGE_GAP: f32 = 6.0;
const BADGE_ICON: f32 = 12.0;
const BADGE_ICON_GAP: f32 = 4.0;

/// What a [`badge`] names, which picks its icon.
#[derive(Clone, Copy)]
enum Badge {
    Website,
    App,
    Chat,
}

impl Badge {
    /// A rule's key is a site's host or an app's name.
    fn of_site(site: &str) -> Self {
        if site.contains('.') && !site.contains(char::is_whitespace) {
            Self::Website
        } else {
            Self::App
        }
    }
}

/// A name in a rounded rect after its kind's icon: what a Shift pick's
/// rule ties together.
fn badge(p: Paint, kind: Badge, name: &str) -> El {
    let k = p.k;
    row()
        .center()
        .gap(BADGE_ICON_GAP)
        .h(BADGE_H)
        .px(BADGE_PX)
        .radius(BADGE_RADIUS)
        .fill(k.alpha(k.c.text_base, 0.09))
        .child(
            canvas(badge_icon(kind, BADGE_ICON * 2.2 / 24.0, p.muted()))
                .size(BADGE_ICON, BADGE_ICON),
        )
        .child(label(name, SMALL, p.text()))
}

/// A badge's line icon on the 24 × 24 grid: a globe, an app window, or a
/// chat bubble.
fn badge_icon(kind: Badge, width: f32, color: Color) -> Shape {
    let line = |a: [f32; 2], b: [f32; 2]| Path::new().move_to(a).line_to(b);
    let paths = match kind {
        Badge::Website => vec![
            rounded(3.0, 3.0, 18.0, 18.0, 9.0),
            line([3.0, 12.0], [21.0, 12.0]),
            Path::new()
                .move_to([12.0, 3.0])
                .cubic_to([7.5, 8.0], [7.5, 16.0], [12.0, 21.0])
                .cubic_to([16.5, 16.0], [16.5, 8.0], [12.0, 3.0])
                .close(),
        ],
        Badge::App => vec![
            rounded(3.0, 3.5, 18.0, 17.0, 3.0),
            line([3.0, 8.5], [21.0, 8.5]),
        ],
        Badge::Chat => vec![
            Path::new()
                .move_to([7.0, 4.0])
                .line_to([17.0, 4.0])
                .cubic_to([19.2, 4.0], [21.0, 5.8], [21.0, 8.0])
                .line_to([21.0, 13.0])
                .cubic_to([21.0, 15.2], [19.2, 17.0], [17.0, 17.0])
                .line_to([11.0, 17.0])
                .line_to([6.5, 20.5])
                .line_to([6.5, 17.0])
                .cubic_to([4.6, 16.8], [3.0, 15.1], [3.0, 13.0])
                .line_to([3.0, 8.0])
                .cubic_to([3.0, 5.8], [4.8, 4.0], [7.0, 4.0])
                .close(),
        ],
    };
    paths
        .iter()
        .fold(Shape::new().view_box(24.0, 24.0), |shape, path| {
            shape.stroke_path(path, width, LineCap::Round, color)
        })
}

/// The footer's height, and the list's below its last row.
fn footer_height() -> f32 {
    match tuning::get().list_footer {
        ListFooter::Band => LIST_PAD + BAND_H,
        ListFooter::Well => LIST_PAD + WELL_H + LIST_PAD,
    }
}

/// The hints set apart from the rows: (A) a full-width band on a quiet
/// fill under a rule, or (B) a rounded well inset like the rows.
fn footer_box(p: Paint, hints: El, w: f32, h: f32) -> El {
    let k = p.k;
    match tuning::get().list_footer {
        ListFooter::Band => el()
            .key("footer")
            .absolute(0.0, h - BAND_H)
            .size(w, BAND_H)
            .fill(k.c.layer_01)
            .child(
                el().key("rule")
                    .absolute(0.0, 0.0)
                    .size(w, p.hair)
                    .fill(k.c.border_weak),
            )
            .child(
                row()
                    .absolute(0.0, 0.0)
                    .size(w, BAND_H)
                    .center_y()
                    .px(LIST_PAD + ROW_PX)
                    .child(hints.grow(1.0)),
            ),
        ListFooter::Well => el()
            .key("footer")
            .absolute(LIST_PAD, h - LIST_PAD - WELL_H)
            .size(w - 2.0 * LIST_PAD, WELL_H)
            .radius(ROW_RADIUS)
            .fill(k.c.layer_01)
            .child(
                row()
                    .absolute(0.0, 0.0)
                    .size(w - 2.0 * LIST_PAD, WELL_H)
                    .center_y()
                    .px(ROW_PX)
                    .child(hints.grow(1.0)),
            ),
    }
}

fn list_panel(p: Paint, body: El, hints: El, x: f32, w: f32, h: f32, blur: f32) -> El {
    let k = p.k;
    let mut panel = el()
        .key("list")
        .trace_name("list")
        .absolute(x, LIST_Y)
        .size(w, h)
        .radius(LIST_RADIUS)
        .fill(k.c.bg_base);
    for shadow in &k.elevation.overlay {
        panel = panel.box_shadow(*shadow);
    }
    panel
        .clip_rounded(LIST_RADIUS)
        .child(body)
        .child(footer_box(p, hints, w, h))
        .motion(tuning::get().island)
        .role("list")
        .label("Send to")
        .hit()
        .transition(
            Transition::offset(0.0, -LIST_DROP)
                .combined(Transition::opacity())
                .combined(Transition::blur(blur))
                .springs(SpringPair::new(
                    tuning::get().content_in,
                    tuning::get().content_out,
                )),
        )
}

/// The list's height: its sections, rows, and footer.
pub(crate) fn list_height(list: &List) -> f32 {
    let sections = [&list.suggested, &list.recent]
        .iter()
        .filter(|rows| !rows.is_empty())
        .count() as f32;
    let rows = (list.suggested.len() + list.recent.len()) as f32;
    LIST_PAD
        + sections * HEADER_H
        + (sections - 1.0).max(0.0) * SECTION_GAP
        + rows * ROW_H
        + footer_height()
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
        Id::Close => {
            paths.push(line([6.5, 6.5], [17.5, 17.5]));
            paths.push(line([17.5, 6.5], [6.5, 17.5]));
        }
        Id::Done => paths.push(
            Path::new()
                .move_to([5.5, 12.5])
                .line_to([10.0, 17.0])
                .line_to([18.5, 7.5]),
        ),
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
