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
    CHIP_H, CHIP_PX, DONE_TEXT_X, HEADER_H, Id, KEYCAP_GAP, LEAD_TEXT_X, LIST_PAD, LIST_Y, MEDIUM,
    PANEL_H, PANEL_W, PROCESSING, PROCESSING_TEXT_X, ROW_H, SEG_ICON, SEG_ICON_GAP, SMALL, STRONG,
    Shown, Surface, TIMER, UNDO_LABEL, clock_width, ellipsize, keycaps_width, measure,
};
use super::controls::{Paint, keycaps};

/// Peak blur (pt) of content entering or leaving in place.
const PRESENCE_BLUR: f32 = 1.5;
/// How long the capsule's exit gets before its overlay hides, in animation
/// seconds.
const HIDE_AFTER_S: f64 = 0.5;
/// The list's corners, and its rows'.
const LIST_RADIUS: f32 = 14.0;
const ROW_RADIUS: f32 = 8.0;
const ROW_PX: f32 = 10.0;
const NUMBER_W: f32 = 10.0;
pub(crate) const FOOTER_H: f32 = surface::FOOTER_H;
const FOOTER_GAP: f32 = 24.0;
/// rune-kit's checkbox.
const CHECKBOX: f32 = 16.0;
/// The list slides down this far as it fades in.
const LIST_DROP: f32 = 6.0;
const LIST_MOVE: rune::Spring = rune::Spring::new(0.32, 0.0);

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
    pub(crate) fn still(pres: Pres, clock: &str) -> Self {
        let mut state = Self::new(false);
        state.surface.clock = clock.to_owned();
        state.live.clock.set(clock.to_owned());
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
        let pres = self.flow.pres(now);
        if !matches!(pres, Pres::Recording { .. } | Pres::Selecting { .. }) {
            self.recording_since = None;
        }
        if pres == self.surface.pres {
            return false;
        }
        self.surface.set(pres, now);
        true
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
        CapsuleKey::Digit(digit) => Input::SendRow(usize::from(digit.saturating_sub(1))),
        CapsuleKey::Press => Input::ClickAway,
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
        return el()
            .size(PANEL_W, PANEL_H)
            .fill(p.k.c.layer_03)
            .child(capsule);
    }
    capsule
}

/// How content enters and leaves in place: a slight scale and blur.
fn presence(surface: &Surface, reduced: bool) -> Transition {
    let blur = if reduced { 0.0 } else { PRESENCE_BLUR };
    Transition::blur(blur)
        .combined(Transition::scale(0.96))
        .springs(SpringPair::new(
            surface.presence(),
            tuning::get().presence_exit,
        ))
}

/// The capsule in its `PANEL_W` × `PANEL_H` panel. Its live chips and
/// buttons and the list are the only `.hit()` boxes: a click anywhere else
/// on it reaches the app beneath, and hover still comes from the pointer.
fn capsule(p: Paint, surface: &Surface, live: &Animated, cx: &mut Cx<CapsuleState>) -> El {
    let reduced = cx.is_reduced_motion() || surface.reduced;
    if surface.reduced {
        cx.reduced_motion(true);
    }
    let presence = presence(surface, reduced);
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
                .radius(CHIP_H * 0.5)
                .fill(p.k.c.layer_02)
                .box_shadow(rune::Shadow {
                    spread: p.hair,
                    color: p.k.c.border_base,
                    ..Default::default()
                })
                .transition(presence),
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
        content = content.child(item(p, surface, live, shown).transition(presence));
    }
    let clip = el()
        .key("clip")
        .absolute(x, y)
        .size(w, h)
        .motion(surface.shell_motion)
        .clip_rounded(h * 0.5)
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
        .radius(h * 0.5)
        .fill(k.c.bg_base);
    for shadow in &k.elevation.overlay {
        body = body.box_shadow(*shadow);
    }
    // The glow inside the edge while the shape morphs, stronger at the top.
    body = body.child(
        el().key("glow")
            .absolute(0.0, 0.0)
            .size(w, h)
            .radius(h * 0.5)
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
                .radius(h * 0.5)
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
                .radius(CHIP_H * 0.5)
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
            base.radius(CHIP_H * 0.5)
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
            base.radius(CHIP_H * 0.5)
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
            .radius(CHIP_H * 0.5)
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
                .radius(14.0)
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
            placed(el(), shown)
                .trace_name("done")
                .child(glyph(p, lead))
                .child(
                    row()
                        .center_y()
                        .absolute(DONE_TEXT_X, 0.0)
                        .h(CHIP_H)
                        .child(label(&shown.ident, STRONG, p.text())),
                )
        }
        Id::Why => placed(row().center_y(), shown)
            .trace_name("why")
            .child(label(shown.text(), SMALL, p.faint())),
        // The quiet way out of a send that just went.
        Id::Undo => keyed(p, base, &[], UNDO_LABEL, MEDIUM).on_press_tx(press(id)),
        // The first says what it does and carries a faint fill, as
        // Scrawl's does; the rest are names beside it.
        Id::Route(index) => {
            let first = index == 0;
            let chip = base
                .radius(CHIP_H * 0.5)
                .child(label(
                    &shown.ident,
                    MEDIUM,
                    if first { p.text() } else { p.muted() },
                ))
                .hover(c.hover)
                .press(c.pressed);
            if first { chip.fill(c.layer_02) } else { chip }
                .role("button")
                .label(shown.ident.clone())
                .hit()
                .on_press_tx(press(id))
        }
        Id::Ask => {
            let keys = match &surface.pres {
                Pres::Done(done) => done.ask_keys.as_slice(),
                _ => &[],
            };
            let keys: Vec<&str> = keys.iter().map(String::as_str).collect();
            keyed(p, base, &keys, &shown.ident, MEDIUM).on_press_tx(press(id))
        }
        Id::Paste => base
            .radius(CHIP_H * 0.5)
            .fill(c.bg_contrast)
            .hover(c.contrast_hover)
            .press(c.contrast_pressed)
            .child(label("Paste", STRONG, c.text_contrast))
            .role("button")
            .label("Paste")
            .hit()
            .on_press_tx(press(id)),
        Id::Inspect => base
            .radius(CHIP_H * 0.5)
            .hover(c.hover)
            .press(c.pressed)
            .child(label("Inspect", MEDIUM, p.muted()))
            .role("button")
            .label("Inspect")
            .hit()
            .on_press_tx(press(id)),
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
        Id::Row(_) | Id::Always => base,
    }
}

/// A chip with keycaps before its label.
fn keyed(p: Paint, base: El, keys: &[&str], name: &str, style: rune::TextStyle) -> El {
    let parts: Vec<String> = keys.iter().map(|k| (*k).to_owned()).collect();
    let base = base
        .radius(CHIP_H * 0.5)
        .gap(KEYCAP_GAP)
        .hover(p.k.c.hover)
        .press(p.k.c.pressed);
    if keys.is_empty() {
        base
    } else {
        base.child(keycaps(p, "keys", &parts).w(keycaps_width(keys)))
    }
    .child(label(name, style, p.muted()))
    .role("button")
    .label(name.to_owned())
    .hit()
}

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
        .radius(CHIP_H * 0.5)
        .fill(p.k.c.layer_01)
        .child(inner.grow(1.0))
        .role("textbox")
        .label("Send to")
}

/// The finished capsule's check, or its warning: its ink spans 12…20,
/// where the recording dot's does.
fn glyph(p: Paint, lead: Lead) -> El {
    let (color, id) = match lead {
        Lead::Check => (p.success(), Id::Done),
        Lead::Warning => (p.k.c.warning.fg, Id::Why),
    };
    canvas(icon(id, CHECK * 2.7 / 24.0, color))
        .absolute(8.0, (CHIP_H - CHECK) * 0.5)
        .size(CHECK, CHECK)
}

const CHECK: f32 = 16.0;

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
    let in_place = Transition::blur(blur)
        .combined(Transition::scale(scale))
        .springs(SpringPair::new(LIST_MOVE, tuning::get().presence_exit));
    let mut body = col().absolute(0.0, 0.0).w(w).padding(LIST_PAD);
    let mut n = 0;
    let suggesting = list.suggesting();
    let first = if suggesting { "Send to" } else { "Suggested" };
    let sections = [(first, &list.suggested), ("Recent", &list.recent)];
    for (title, rows) in sections {
        if rows.is_empty() {
            continue;
        }
        body = body.child(
            row()
                .key(format!("section-{title}"))
                .center_y()
                .h(HEADER_H)
                .px(ROW_PX)
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
                    label(&(n + 1).to_string(), TIMER, p.faint())
                        .key(format!("number-{n}"))
                        .w(NUMBER_W)
                        .transition(
                            Transition::opacity()
                                .springs(SpringPair::new(LIST_MOVE, tuning::get().presence_exit)),
                        ),
                )
                .child(label(&ellipsize(&dest.name, MEDIUM, max), MEDIUM, p.text()))
                .child(el().grow(1.0))
                // The best suggestion is marked by its reason.
                .child(label(
                    &why,
                    SMALL,
                    if suggesting && n == 0 {
                        p.accent()
                    } else {
                        p.muted()
                    },
                ))
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
    if let Some((site, on)) = &list.always {
        // The box sits centred over the numbers, its words in line with
        // the names.
        let inset = ROW_PX + 0.5 * (NUMBER_W - CHECKBOX);
        let gap = ROW_PX + NUMBER_W + ROW_PX - inset - CHECKBOX;
        let room = w - 2.0 * LIST_PAD - inset - CHECKBOX - gap - ROW_PX;
        let copy = format!("Always send {site} here");
        body = body.child(
            row()
                .key("always")
                .trace_name("always")
                .center_y()
                .h(ROW_H)
                .pl(inset)
                .pr(ROW_PX)
                .gap(gap)
                .radius(ROW_RADIUS)
                .hover(k.c.hover)
                .press(k.c.pressed)
                .child(rune_kit::Checkbox::new(*on).build(k))
                .child(label(&ellipsize(&copy, SMALL, room), SMALL, p.muted()))
                .role("checkbox")
                .label(copy)
                .hit()
                .on_press_tx(press(Id::Always))
                .transition(in_place),
        );
    }
    if suggesting {
        return list_panel(p, body.child(suggest_footer(p, list, w)), x, w, h, blur);
    }
    let room = w - 2.0 * LIST_PAD - 2.0 * ROW_PX;
    let send = list
        .selected_dest()
        .map_or("Send".to_owned(), |dest| format!("Send to {}", dest.name));
    let send = ellipsize(&send, SMALL, room * 0.4);
    let (keys, second): (&[&str], String) = match &list.footer {
        Footer::KeepCopied => (&["esc"], "Keep copied".to_owned()),
        Footer::Keep(to) => (&["esc"], format!("Keep {to}")),
        Footer::Always(site) => (&["⇧", "⏎"], format!("Always for {site}")),
        Footer::Suggesting => (&["esc"], "Keep copied".to_owned()),
    };
    let used = keycaps_width(&["⏎"]) + 5.0 + measure(&send, SMALL) + FOOTER_GAP;
    let second = ellipsize(&second, SMALL, room - used - keycaps_width(keys) - 5.0);
    let hint = |keys: &[&str], copy: &str| {
        let parts: Vec<String> = keys.iter().map(|k| (*k).to_owned()).collect();
        row()
            .center_y()
            .gap(5.0)
            .child(keycaps(p, "keys", &parts))
            .child(label(copy, SMALL, p.faint()))
    };
    body = body
        .child(
            el().key("rule")
                .h(p.hair)
                .w_full()
                .mt(LIST_PAD)
                .fill(k.c.border_base),
        )
        .child(
            row()
                .key("footer")
                .center_y()
                .h(FOOTER_H)
                .px(ROW_PX)
                .gap(FOOTER_GAP)
                .child(hint(&["⏎"], &send).key("send"))
                .child(hint(keys, &second).key("second")),
        );
    list_panel(p, body, x, w, h, blur)
}

/// The suggestions' footer: the digits that send, and a hint that the list
/// shortcut searches every session.
fn suggest_footer(p: Paint, list: &List, w: f32) -> El {
    let rows = list.rows().count();
    let digits = if rows > 1 {
        format!("1–{rows}")
    } else {
        "1".to_owned()
    };
    let list_keys = surface::ask_keys();
    let room = w - 2.0 * LIST_PAD - 2.0 * ROW_PX;
    let search_w = if list_keys.is_empty() {
        0.0
    } else {
        keycaps_width(&list_keys.iter().map(String::as_str).collect::<Vec<_>>())
            + 5.0
            + measure("Search", SMALL)
    };
    // "Send" goes before anything is cut.
    let say_send =
        keycaps_width(&[digits.as_str()]) + 5.0 + measure("Send", SMALL) + FOOTER_GAP + search_w
            <= room;
    let mut send = row()
        .key("send")
        .center_y()
        .gap(5.0)
        .child(keycaps(p, "keys", &[digits]));
    if say_send {
        send = send.child(label("Send", SMALL, p.faint()));
    }
    let mut footer = row()
        .key("footer")
        .center_y()
        .h(FOOTER_H)
        .px(ROW_PX)
        .child(send)
        .child(el().grow(1.0));
    if !list_keys.is_empty() {
        footer = footer.child(
            row()
                .key("search")
                .center_y()
                .gap(5.0)
                .child(keycaps(p, "keys", &list_keys))
                .child(label("Search", SMALL, p.faint())),
        );
    }
    col()
        .w(w - 2.0 * LIST_PAD)
        .child(
            el().key("rule")
                .h(p.hair)
                .w_full()
                .mt(LIST_PAD)
                .fill(p.k.c.border_base),
        )
        .child(footer)
}

fn list_panel(p: Paint, body: El, x: f32, w: f32, h: f32, blur: f32) -> El {
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
        .motion(LIST_MOVE)
        .role("list")
        .label("Send to")
        .hit()
        .transition(
            Transition::offset(0.0, -LIST_DROP)
                .combined(Transition::opacity())
                .combined(Transition::blur(blur))
                .springs(SpringPair::new(LIST_MOVE, tuning::get().presence_exit)),
        )
}

/// The list's height: its sections, rows, rule, and footer.
pub(crate) fn list_height(list: &List) -> f32 {
    let sections = [&list.suggested, &list.recent]
        .iter()
        .filter(|rows| !rows.is_empty())
        .count() as f32;
    let rows =
        (list.suggested.len() + list.recent.len() + usize::from(list.always.is_some())) as f32;
    2.0 * LIST_PAD + sections * HEADER_H + rows * ROW_H + LIST_PAD + 1.0 + FOOTER_H
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
