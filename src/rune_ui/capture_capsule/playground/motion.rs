//! The recording pill's motion (t1379), drawn by Hex itself: how it comes
//! in (today's scale and fade, or E3's draw on), how the voice rests in
//! silence (today's dots, a flat line, a line from the centre, or beads),
//! and how it finishes (today's, one thread into the Undo timer, or
//! folding away under the menu bar). Each section stacks its options in
//! lanes on one clock over the real Dot scope simulation, fed a scripted
//! voice: silence, speech, silence. Every frame is drawn from the clock,
//! so a clip of it is exact. A prototype: the capsule moves as it always
//! has until one is picked.

use std::f32::consts::{FRAC_PI_2, PI, TAU};

use rune::{Color, El, LineCap, Shape, TextStyle, canvas, el, row, text};

use super::super::surface::{measure, medium, undo_label};
use super::super::undo_ring::{self, Ring};
use super::super::view::{close_glyph, rim};
use super::Paint;
use crate::rune_hud::dot_scope::{self, Frame, Scope, Stage, demo_level, palette};
use crate::rune_hud::spring::{self as hud, MORPH, REDUCED_FADE, Spec};

const W: f32 = 640.0;
const LANE_H: f32 = 120.0;
const BAR: f32 = 24.0;
const CX: f32 = W * 0.5;

/// The recording pill and the receipt it becomes.
const PILL: Rect = Rect::new(CX - 88.0, 32.0, CX + 88.0, 64.0);
const RECEIPT: Rect = Rect::new(CX - 200.0, 32.0, CX + 200.0, 80.0);
const PILL_RADIUS: f32 = 16.0;
const RECEIPT_RADIUS: f32 = 14.0;
const MID_Y: f32 = PILL.t + 16.0;
/// Dot scope's field in the pill, between the record dot and Stop.
const FIELD_W: f32 = 112.0;
const FIELD_H: f32 = 26.0;
const FIELD_X: f32 = PILL.l + 26.0;
const FIELD_CX: f32 = FIELD_X + FIELD_W * 0.5;
/// The receipt's row and the ✓'s box in it.
const ROW_X: f32 = RECEIPT.l + 6.0;
const ROW_Y: f32 = RECEIPT.t + 6.0;
const ROW_W: f32 = 388.0;
const CHECK_X: f32 = ROW_X + 15.0;
const CHECK_Y: f32 = ROW_Y + 11.0;
const CHECK: [[f32; 2]; 3] = [[2.5, 7.5], [5.6, 10.5], [11.5, 3.8]];
const CLOSE_X: f32 = ROW_X + ROW_W - 8.0 - 28.0;

/// Silence: quieter than this level for this long lays the voice flat.
const THRESHOLD: f32 = 0.12;
const SILENCE_S: f32 = 0.35;
const SETTLE: Spec = Spec::new(0.55, 0.0);
const SPLIT: Spec = Spec::new(0.38, 0.22);
/// How fast I3's flatness travels out from the middle, pt/s: the scope's
/// own history speed.
const TRAVEL: f32 = 150.0;
const LINE_ALPHA: f32 = 0.5;
const LINE_W: f32 = 1.25;

/// The entrance: the pill comes in here.
const T_IN: f32 = 0.4;
/// The finish, seconds into its clip: after the voice has spoken and gone
/// quiet again, so the pill finishes from I3's line.
const T_STOP: f32 = 3.3;
const T_WIDEN: f32 = T_STOP + 0.12;
const T_COUNT: f32 = T_WIDEN + 0.28;
const COUNTDOWN: f32 = 3.0;
const T_SENT: f32 = T_COUNT + COUNTDOWN;
/// F1's ✓ starts drawing as the flying point lands.
const T_CHECK: f32 = T_SENT + 0.32;
const T_EXIT: f32 = T_SENT + 1.8;

#[derive(Clone, Copy, Debug, PartialEq)]
struct Rect {
    l: f32,
    t: f32,
    r: f32,
    b: f32,
}

impl Rect {
    const fn new(l: f32, t: f32, r: f32, b: f32) -> Self {
        Self { l, t, r, b }
    }

    fn w(self) -> f32 {
        (self.r - self.l).max(0.0)
    }

    fn h(self) -> f32 {
        (self.b - self.t).max(0.0)
    }

    fn lerp(self, to: Rect, k: f32) -> Rect {
        Rect::new(
            lerp(self.l, to.l, k),
            lerp(self.t, to.t, k),
            lerp(self.r, to.r, k),
            lerp(self.b, to.b, k),
        )
    }
}

/// What a clip compares.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Section {
    Entrance,
    Silence,
    Finish,
}

impl Section {
    fn variants(self) -> &'static [Variant] {
        match self {
            Self::Entrance => &[Variant::EnterNow, Variant::E3],
            Self::Silence => &[Variant::I1, Variant::I2, Variant::I3, Variant::I4],
            Self::Finish => &[Variant::FinishNow, Variant::F1, Variant::F2],
        }
    }

    /// When the voice speaks, seconds into the clip.
    fn speech(self) -> (f32, f32) {
        match self {
            Self::Entrance => (1.0, 2.6),
            Self::Silence => (2.0, 4.4),
            Self::Finish => (0.4, 2.0),
        }
    }

    /// How long the clip runs before it plays again.
    fn duration(self) -> f32 {
        match self {
            Self::Entrance => 3.4,
            Self::Silence => 7.0,
            Self::Finish => T_EXIT + 1.1,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Variant {
    EnterNow,
    E3,
    I1,
    I2,
    I3,
    I4,
    FinishNow,
    F1,
    F2,
}

impl Variant {
    fn caption(self) -> &'static str {
        match self {
            Self::EnterNow => "Now · scale and fade",
            Self::E3 => "E3 · Draw on",
            Self::I1 => "I1 · Today's dots",
            Self::I2 => "I2 · Flat line",
            Self::I3 => "I3 · From the centre",
            Self::I4 => "I4 · Beads",
            Self::FinishNow => "Now · today",
            Self::F1 => "F1 · One thread",
            Self::F2 => "F2 · Fold away",
        }
    }
}

struct Lane {
    variant: Variant,
    scope: Scope,
    /// I2's and I4's flatness, 0–1.
    flat: hud::Spring,
    /// I3's, per column.
    columns: Vec<hud::Spring>,
}

/// One section's lanes on one clock.
pub(super) struct Motion {
    section: Section,
    t: f32,
    reduced: bool,
    lanes: Vec<Lane>,
    /// When the voice last rose over the threshold.
    loud_at: f32,
    /// (time, whether it's silent) for I3's travel, the last few seconds.
    history: Vec<(f32, f32)>,
}

impl Motion {
    pub(super) fn new(section: Section, reduced: bool) -> Self {
        // The silence and finish clips open in silence, already flat.
        let start = if section != Section::Entrance {
            1.0
        } else {
            0.0
        };
        let lanes = section
            .variants()
            .iter()
            .map(|&variant| {
                let mut scope = Scope::new(dot_scope::lab());
                scope.restart(reduced);
                // Past the scope's own unfolding, so only the pill's
                // entrance shows.
                for _ in 0..6 {
                    scope.advance(0.2, frame(None, reduced));
                }
                let columns = vec![hud::Spring::at(start); scope.columns()];
                Lane {
                    variant,
                    scope,
                    flat: hud::Spring::at(start),
                    columns,
                }
            })
            .collect();
        Self {
            section,
            t: 0.0,
            reduced,
            lanes,
            loud_at: -10.0,
            history: vec![(-10.0, start)],
        }
    }

    pub(super) fn section(&self) -> Section {
        self.section
    }

    pub(super) fn height(&self) -> f32 {
        self.lanes.len() as f32 * LANE_H
    }

    /// The scripted voice's level now, or `None` in silence.
    fn level(&self, t: f32) -> Option<f32> {
        let (from, to) = self.section.speech();
        // Inside HUD Lab's spoken phrase, never its breaths.
        (t >= from && t < to).then(|| demo_level(0.05 + (t - from) % 1.6))
    }

    pub(super) fn advance(&mut self, dt: f32, reduced: bool) {
        if self.t + dt > self.section.duration() {
            *self = Self::new(self.section, reduced);
            return;
        }
        self.reduced = reduced;
        self.t += dt;
        let t = self.t;
        let level = self.level(t);
        if level.is_some_and(|l| l > THRESHOLD) {
            self.loud_at = t;
        }
        let silent = if t - self.loud_at > SILENCE_S {
            1.0
        } else {
            0.0
        };
        self.history.push((t, silent));
        let old = self.history.partition_point(|&(at, _)| at < t - 3.0);
        self.history.drain(..old.saturating_sub(1));
        let spec = |to: f32| match () {
            _ if reduced => REDUCED_FADE,
            _ if to > 0.5 => SETTLE,
            _ => SPLIT,
        };
        let f1 = self.after(t - T_STOP, 0.22, 0.0);
        for lane in &mut self.lanes {
            // The finish clips rest in I3, the silence Kit picked.
            let from_centre = lane.variant == Variant::I3 || self.section == Section::Finish;
            match lane.variant {
                _ if from_centre => {
                    let n = lane.columns.len();
                    let pitch = lane.scope.tuning().spacing;
                    for (i, column) in lane.columns.iter_mut().enumerate() {
                        let x = (i as f32 - (n as f32 - 1.0) * 0.5) * pitch;
                        let delay = if reduced { 0.0 } else { x.abs() / TRAVEL };
                        let to = history_at(&self.history, t - delay);
                        column.step_to(to, spec(to), dt);
                    }
                    // F1's thread lays down whatever is still standing.
                    let f1 = if lane.variant == Variant::F1 { f1 } else { 0.0 };
                    let flat: Vec<f32> = lane.columns.iter().map(|c| c.x.max(f1)).collect();
                    lane.scope.set_flat(&flat);
                }
                Variant::I2 | Variant::I4 => {
                    lane.flat.step_to(silent, spec(silent), dt);
                    lane.scope.set_flat(&[lane.flat.x]);
                }
                _ => {}
            }
            lane.scope.advance(dt, frame(level, reduced));
        }
    }

    /// A spring from rest at 0 toward 1, `dt` after it started; a short
    /// fade under Reduce Motion.
    fn after(&self, dt: f32, duration: f32, bounce: f32) -> f32 {
        let spec = if self.reduced {
            REDUCED_FADE
        } else {
            Spec::new(duration, bounce)
        };
        spring(dt, spec)
    }

    /// A shape's change: on [`MORPH`], or at once under Reduce Motion.
    fn morph(&self, dt: f32) -> f32 {
        match () {
            _ if dt < 0.0 => 0.0,
            _ if self.reduced => 1.0,
            _ => spring(dt, MORPH),
        }
    }
}

fn frame(level: Option<f32>, reduced: bool) -> Frame {
    Frame {
        stage: Stage::Recording,
        visible: true,
        loudness: level.map_or(-70.0, |l| -58.0 + 28.0 * l),
        reduce_motion: reduced,
    }
}

fn history_at(history: &[(f32, f32)], t: f32) -> f32 {
    let i = history.partition_point(|&(at, _)| at <= t);
    history[i.saturating_sub(1)].1
}

/// A spring's value `dt` after it left 0 for 1 at rest: exact, so a frame
/// is the same however the clock got there.
fn spring(dt: f32, spec: Spec) -> f32 {
    if dt <= 0.0 {
        return 0.0;
    }
    let mut s = hud::Spring::at(0.0);
    s.step_to(1.0, spec, dt);
    s.x
}

fn lerp(a: f32, b: f32, k: f32) -> f32 {
    a + (b - a) * k
}

fn smoothstep(a: f32, b: f32, x: f32) -> f32 {
    let t = ((x - a) / (b - a)).clamp(0.0, 1.0);
    t * t * (3.0 - 2.0 * t)
}

/// Half a cosine, 0 to 1: the scope's pass.
fn pass(x: f32) -> f32 {
    0.5 - 0.5 * (PI * x.clamp(0.0, 1.0)).cos()
}

fn ease_out(x: f32) -> f32 {
    1.0 - (1.0 - x.clamp(0.0, 1.0)).powi(3)
}

/// A display sRGB colour as a (linear) `Color`.
fn srgb(rgb: [f32; 3]) -> Color {
    let [r, g, b] = rgb.map(rune::color::srgb_decode);
    Color { r, g, b, a: 1.0 }
}

fn rgb(r: u8, g: u8, b: u8) -> Color {
    srgb([r, g, b].map(|c| c as f32 / 255.0))
}

/// The Glow countdown's blue, its head, and the ✓ as it lands.
fn blue() -> Color {
    rgb(122, 167, 255)
}

fn head() -> Color {
    rgb(219, 231, 255)
}

/// How a line fades in at the field's ends, `u` 0–1 across it.
fn edge(u: f32) -> f32 {
    smoothstep(0.0, 0.16, u) * smoothstep(0.0, 0.16, 1.0 - u)
}

// ─── View ────────────────────────────────────────────────────────────────────

/// The section's lanes, stacked, at the playground's stage.
pub(super) fn stage(p: Paint, m: &Motion) -> El {
    let mut stage = el()
        .key("motion")
        .size(W, m.height())
        .instant_changes()
        .motion(rune::Spring::NONE)
        // Every frame is drawn where the clock puts it, so nothing steps a
        // pixel at a time.
        .pixel_snap(false);
    for (i, lane) in m.lanes.iter().enumerate() {
        stage = stage.child(
            m.lane(p, lane)
                .key(format!("lane-{i}"))
                .absolute(0.0, i as f32 * LANE_H),
        );
    }
    stage
}

/// What a lane's pill or receipt is doing.
struct Layer {
    rect: Rect,
    radius: f32,
    shell_alpha: f32,
    glow: f32,
    /// Inside the shell, clipped to it, in the lane's points.
    inside: Vec<El>,
    /// Over the shell, unclipped.
    over: Vec<El>,
    scale: f32,
    alpha: f32,
    origin: [f32; 2],
    dy: f32,
}

impl Layer {
    fn new(rect: Rect, radius: f32) -> Self {
        Self {
            rect,
            radius,
            shell_alpha: 1.0,
            glow: 0.0,
            inside: Vec::new(),
            over: Vec::new(),
            scale: 1.0,
            alpha: 1.0,
            origin: [CX, MID_Y],
            dy: 0.0,
        }
    }

    fn view(self, p: Paint) -> El {
        let Layer { rect, radius, .. } = self;
        let mut inside = el().absolute(-rect.l, -rect.t).size(W, LANE_H);
        for e in self.inside {
            inside = inside.child(e);
        }
        let mut layer = el()
            .key("layer")
            .absolute(0.0, self.dy)
            .size(W, LANE_H)
            .origin(self.origin[0] / W, self.origin[1] / LANE_H)
            .scale(self.scale)
            .opacity(self.alpha.clamp(0.0, 1.0))
            .child(
                shell(p, rect, radius, self.glow)
                    .key("shell")
                    .opacity(self.shell_alpha.clamp(0.0, 1.0)),
            )
            .child(
                el().key("inside")
                    .absolute(rect.l, rect.t)
                    .size(rect.w(), rect.h())
                    .clip_rounded(radius)
                    .child(inside),
            );
        for e in self.over {
            layer = layer.child(e);
        }
        layer
    }
}

impl Motion {
    fn lane(&self, p: Paint, lane: &Lane) -> El {
        let k = p.k;
        let wash = |name, step| rune_theme_opencode::scale(name, k.dark, step);
        let layer = match self.section {
            Section::Entrance => self.entrance(p, lane),
            Section::Silence => Some(self.silence(p, lane)),
            Section::Finish => self.finish(p, lane),
        };
        let mut e = el()
            .size(W, LANE_H)
            .fill(wash("cobalt", 3))
            .linear_gradient(
                PI * 0.75,
                [(0.0, wash("cobalt", 3)), (1.0, wash("lilac", 5))],
            )
            .clip_rounded(0.0);
        if let Some(layer) = layer {
            e = e.child(layer.view(p));
        }
        let speaking = self.level(self.t).is_some();
        e.child(
            el().key("bar")
                .absolute(0.0, 0.0)
                .size(W, BAR)
                .fill(wash("cobalt", 3))
                .child(super::stories::menu_bar(p, W, BAR)),
        )
        .child(
            text(lane.variant.caption())
                .key("caption")
                .absolute(16.0, LANE_H - 26.0)
                .font(k.ty.ui_medium)
                .color(p.text()),
        )
        .child(
            row()
                .key("voice")
                .absolute(W - 136.0, LANE_H - 26.0)
                .w(120.0)
                .h(20.0)
                .center_y()
                .justify_end()
                .gap(6.0)
                .child(
                    el().key("light")
                        .size(6.0, 6.0)
                        .radius(3.0)
                        .fill(if speaking { p.danger() } else { p.faint() }),
                )
                .child(
                    text(if speaking { "speaking" } else { "silent" })
                        .key("state")
                        .font(k.ty.small)
                        .color(p.muted()),
                ),
        )
        .child(
            el().key("rule")
                .absolute(0.0, LANE_H - 1.0)
                .size(W, 1.0)
                .fill(Color::BLACK.alpha(0.35)),
        )
    }

    fn entrance(&self, p: Paint, lane: &Lane) -> Option<Layer> {
        let te = self.t - T_IN;
        if te <= 0.0 {
            return None;
        }
        let mut layer = Layer::new(PILL, PILL_RADIUS);
        if self.reduced {
            layer.alpha = spring(te, REDUCED_FADE);
            layer.inside = pill(p, lane, 1.0, 1.0, |_, _| (1.0, 0.0), Stop::rest());
            return Some(layer);
        }
        match lane.variant {
            Variant::E3 => {
                let glass = spring(te, Spec::new(0.26, 0.0));
                let alpha = spring(te, Spec::new(0.156, 0.0)).min(1.0);
                layer.scale = 0.96 + 0.04 * glass;
                layer.shell_alpha = alpha * (1.5 * glass).min(1.0);
                // The record dot lights, with a ring going out from it.
                let lit = spring(te - 0.08, Spec::new(0.32, 0.45));
                let e = (te - 0.08) / 0.5;
                if (0.0..1.0).contains(&e) {
                    let r = 3.0 + 13.0 * ease_out(e);
                    layer.over.push(
                        el().key("ignite")
                            .absolute(PILL.l + 13.0 - r, MID_Y - r)
                            .size(2.0 * r, 2.0 * r)
                            .radius(r)
                            .border(1.5, rgb(239, 90, 82).alpha(0.5 * (1.0 - e))),
                    );
                }
                // The field draws on left to right behind a bright head.
                let sweep = (te - 0.16) / 0.5;
                let head = -0.06 + 1.12 * pass(sweep);
                let fade = 1.0 - smoothstep(1.0, 1.5, sweep);
                let vis = move |_: usize, u: f32| {
                    if sweep <= 0.0 {
                        return (0.0, 0.0);
                    }
                    let a = smoothstep(head + 0.01, head - 0.04, u);
                    let boost = if u <= head + 0.02 {
                        0.8 * fade * (-(head - u) / 0.09).exp()
                    } else {
                        0.0
                    };
                    (a, boost)
                };
                let stop = spring(te - 0.62, Spec::new(0.30, 0.10));
                let stop = Stop {
                    scale: 0.9 + 0.1 * stop,
                    alpha: stop.clamp(0.0, 1.0),
                    blur: 3.0 * (1.0 - stop).max(0.0),
                };
                layer.inside = pill(p, lane, lit, alpha, vis, stop);
            }
            _ => {
                layer.scale = 0.9 + 0.1 * spring(te, MORPH);
                layer.alpha = spring(te, Spec::new(0.30, 0.0));
                layer.inside = pill(p, lane, 1.0, 1.0, |_, _| (1.0, 0.0), Stop::rest());
            }
        }
        Some(layer)
    }

    fn silence(&self, p: Paint, lane: &Lane) -> Layer {
        let mut layer = Layer::new(PILL, PILL_RADIUS);
        let ink = ink(p, &lane.scope);
        let columns = lane.scope.columns();
        let pitch = lane.scope.tuning().spacing;
        let half = (columns as f32 - 1.0) * 0.5 * pitch + pitch * 0.5;
        let flat = lane.flat.x;
        let mut extra = Vec::new();
        let field_vis: Box<dyn Fn(usize, f32) -> (f32, f32)> = match lane.variant {
            Variant::I2 => {
                let f = flat.clamp(0.0, 1.0);
                extra.push(soft_line(
                    FIELD_CX - half,
                    FIELD_CX + half,
                    &[0.0, 0.04, 0.09, 0.16, 0.84, 0.91, 0.96, 1.0],
                    ink,
                    |u| LINE_ALPHA * f * edge(u),
                ));
                Box::new(move |_, _| (1.0 - f, 0.0))
            }
            Variant::I3 => {
                extra.extend(centre_line(p, lane, 1.0));
                let flats: Vec<f32> = lane.columns.iter().map(|c| c.x.clamp(0.0, 1.0)).collect();
                Box::new(move |c, _| (1.0 - flats[c], 0.0))
            }
            Variant::I4 => {
                let f = flat.clamp(0.0, 1.0);
                for i in (0..columns).step_by(2) {
                    let u = i as f32 / (columns as f32 - 1.0);
                    let breath = if self.reduced {
                        1.0
                    } else {
                        0.55 + 0.45 * (TAU * self.t / 2.75 - 5.0 * u).sin()
                    };
                    let a = 0.42 * f * breath * (0.45 + 0.55 * edge(u));
                    if a < 0.004 {
                        continue;
                    }
                    let x = FIELD_CX + (i as f32 - (columns as f32 - 1.0) * 0.5) * pitch;
                    extra.push(
                        el().key(format!("bead-{i}"))
                            .absolute(x - 0.8, MID_Y - 0.8)
                            .size(1.6, 1.6)
                            .radius(0.8)
                            .fill(ink.alpha(a)),
                    );
                }
                Box::new(move |_, _| (1.0 - f, 0.0))
            }
            _ => Box::new(|_, _| (1.0, 0.0)),
        };
        layer.inside = pill(p, lane, 1.0, 1.0, field_vis, Stop::rest());
        layer.inside.extend(extra);
        layer
    }

    fn finish(&self, p: Paint, lane: &Lane) -> Option<Layer> {
        let t = self.t;
        let v = lane.variant;
        let reduced = self.reduced;
        let widen = self.morph(t - T_WIDEN);
        let mut rect = PILL.lerp(RECEIPT, widen);
        let mut radius = lerp(PILL_RADIUS, RECEIPT_RADIUS, widen);
        // The pill's content leaves as the receipt's comes.
        let out = self.after(t - T_WIDEN, 0.24, 0.0).min(1.0);
        let came = self.after(t - T_WIDEN - 0.06, 0.30, 0.10);
        let counting = self.after(t - T_COUNT, 0.24, 0.0).min(1.0);
        let left = ((T_SENT - t) / COUNTDOWN).clamp(0.0, 1.0);
        let went = self.after(t - T_SENT, 0.24, 0.0).min(1.0);
        let sent = self.after(t - T_SENT - 0.06, 0.30, 0.10);
        let flat = if v == Variant::F1 {
            self.after(t - T_STOP, 0.22, 0.0).clamp(0.0, 1.0)
        } else {
            0.0
        };
        let pressed = if t >= T_STOP {
            0.84 + 0.16 * self.after(t - T_STOP, 0.28, 0.35)
        } else {
            1.0
        };
        let mut layer = Layer::new(rect, radius);
        layer.glow = (4.0 * widen * (1.0 - widen)).clamp(0.0, 1.0);

        // The exits.
        let mut sent_alpha = sent.clamp(0.0, 1.0);
        // The ✓ stays as the words go: F1 closes the glass round it.
        let check_alpha = sent_alpha;
        let exit = t - T_EXIT;
        match v {
            _ if reduced => layer.alpha = 1.0 - self.after(exit, 0.16, 0.0).min(1.0),
            Variant::FinishNow => {
                layer.scale = 1.0 - 0.1 * self.after(exit, 0.30, 0.15);
                layer.alpha = 1.0 - self.after(exit, 0.30, 0.0).min(1.0);
                layer.origin = [CX, (RECEIPT.t + RECEIPT.b) * 0.5];
            }
            Variant::F1 => {
                // The glass closes round the ✓, then it goes.
                let centre = [CHECK_X + 7.0, CHECK_Y + 7.0];
                let ring = Rect::new(
                    centre[0] - 16.0,
                    centre[1] - 16.0,
                    centre[0] + 16.0,
                    centre[1] + 16.0,
                );
                let across = self.morph(exit);
                let down = self.morph(exit - 0.04);
                rect = Rect::new(
                    lerp(rect.l, ring.l, across),
                    lerp(rect.t, ring.t, down),
                    lerp(rect.r, ring.r, across),
                    lerp(rect.b, ring.b, down),
                );
                radius = lerp(radius, 16.0, across);
                layer.glow = layer
                    .glow
                    .max((4.0 * across * (1.0 - across)).clamp(0.0, 1.0));
                sent_alpha *= 1.0 - self.after(exit, 0.2, 0.0).min(1.0);
                let gone = exit - 0.42;
                layer.scale = 1.0 - 0.1 * self.after(gone, 0.30, 0.15);
                layer.alpha = 1.0 - self.after(gone, 0.30, 0.0).min(1.0);
                layer.origin = centre;
            }
            Variant::F2 => {
                // A dip, then it folds up under the menu bar.
                let dip = self.after(exit, 0.14, 0.0);
                let fold = self.after(exit - 0.1, 0.34, 0.0);
                sent_alpha *= 1.0 - self.after(exit, 0.2, 0.0).min(1.0);
                layer.dy = lerp(2.0 * dip, -72.0, fold);
                let narrow = 1.0 - 0.04 * fold;
                let (mid, half) = ((rect.l + rect.r) * 0.5, rect.w() * 0.5 * narrow);
                rect = Rect::new(
                    mid - half,
                    rect.t,
                    mid + half,
                    lerp(rect.b, rect.t + 12.0, fold),
                );
                radius = radius.min(rect.h() * 0.5);
                layer.alpha = 1.0 - smoothstep(0.75, 1.0, fold);
            }
            _ => {}
        }
        layer.rect = rect;
        layer.radius = radius;

        // The pill's content, resting in I3 until F1's thread takes over
        // its line.
        let gone = out;
        let flats: Vec<f32> = lane
            .columns
            .iter()
            .map(|c| c.x.max(flat).clamp(0.0, 1.0))
            .collect();
        let mut content = pill(
            p,
            lane,
            1.0,
            1.0,
            move |c, _| (1.0 - flats[c], 0.0),
            Stop {
                scale: pressed,
                ..Stop::rest()
            },
        );
        content.extend(centre_line(p, lane, 1.0 - flat));
        if gone < 0.999 {
            let mut group = el()
                .key("pill")
                .absolute(0.0, 0.0)
                .size(W, LANE_H)
                .origin(CX / W, MID_Y / LANE_H)
                .opacity(1.0 - gone);
            if !reduced {
                group = group.scale(1.0 - 0.08 * gone).blur(3.0 * gone);
            }
            for e in content {
                group = group.child(e);
            }
            layer.inside.push(group);
        }

        // The receipt, its countdown, then Sent.
        let receipt_alpha = came.clamp(0.0, 1.0) * (1.0 - went);
        if receipt_alpha > 0.002 {
            let ring = (v != Variant::F1).then(|| Ring::still(left as f64, counting, reduced));
            let blur = if reduced {
                0.0
            } else {
                3.0 * (1.0 - came).max(0.0) + 3.0 * went
            };
            layer.inside.push(
                receipt(p, ring.as_ref())
                    .key("receipt")
                    .opacity(receipt_alpha)
                    .blur(blur),
            );
        }
        let close_alpha = came.clamp(0.0, 1.0) * sent_alpha.max(1.0 - went);
        if close_alpha > 0.002 && t >= T_WIDEN {
            layer.inside.push(
                el().key("close")
                    .absolute(CLOSE_X, ROW_Y + 4.0)
                    .size(28.0, 28.0)
                    .center()
                    .child(close_glyph(p))
                    .opacity(close_alpha),
            );
        }
        if check_alpha > 0.002 && t >= T_SENT {
            layer
                .inside
                .push(sent_row(p).key("sent").opacity(sent_alpha));
            let draw = match v {
                _ if reduced => 1.0,
                Variant::F1 => self.after(t - T_CHECK, 0.3, 0.0),
                Variant::F2 => self.after(t - T_SENT - 0.06, 0.32, 0.0),
                _ => 1.0,
            };
            let color = if v == Variant::F1 && !reduced {
                head().mix(p.text(), draw.clamp(0.0, 1.0))
            } else {
                p.text()
            };
            if draw > 0.01 {
                layer
                    .inside
                    .push(check(draw, color.alpha(check_alpha)).key("check"));
            }
        }

        // F1's one thread: the dots lie down into a line, the line
        // becomes the Undo timer, and its last point flies up into the ✓.
        if v == Variant::F1 {
            layer
                .inside
                .extend(self.thread(p, flat, widen, counting, left, went));
            if !reduced && (T_SENT..T_CHECK + 0.2).contains(&t) {
                layer.inside.extend(self.fly(sent_alpha));
            }
        }
        Some(layer)
    }

    /// F1's line, from the flattened field to the receipt's bottom edge,
    /// where it drains as the Undo window runs out.
    fn thread(
        &self,
        p: Paint,
        flat: f32,
        widen: f32,
        counting: f32,
        left: f32,
        went: f32,
    ) -> Vec<El> {
        let mut out = Vec::new();
        if flat <= 0.002 || went >= 0.999 {
            return out;
        }
        // It stays in the field while the dots around it go, then
        // catches the receipt's edge.
        let to = widen.clamp(0.0, 1.0).powi(2);
        let x0 = lerp(FIELD_X + 6.0, RECEIPT.l + 14.0, to);
        let x1 = lerp(FIELD_X + 106.0, RECEIPT.r - 14.0, to);
        let y = lerp(MID_Y, RECEIPT.b - 0.75, to);
        let width = lerp(LINE_W, 1.5, widen);
        let alpha = lerp(0.55, 1.0, widen) * flat * (1.0 - went);
        let color = p.text().mix(blue(), widen.clamp(0.0, 1.0));
        let track = 0.06 * widen.clamp(0.0, 1.0) * (1.0 - went);
        if track > 0.002 {
            out.push(stroke(
                "track",
                &[[x0, y], [x1, y]],
                width,
                Color::WHITE.alpha(track),
            ));
        }
        let xh = lerp(x0, x1, left);
        if xh > x0 + 0.2 {
            out.push(stroke(
                "line",
                &[[x0, y], [xh, y]],
                width,
                color.alpha(alpha),
            ));
        }
        if !self.reduced && left > 0.001 && left < 0.999 {
            let glow = counting * (1.0 - went);
            let len = 36.0_f32.min(x1 - xh);
            if len > 0.5 {
                out.push(
                    el().key("trail")
                        .absolute(xh, y - 2.5)
                        .size(len, 5.0)
                        .linear_gradient(
                            FRAC_PI_2,
                            [(0.0, blue().alpha(0.34 * glow)), (1.0, blue().alpha(0.0))],
                        )
                        .blur(1.6),
                );
            }
            out.push(dot("head", [xh, y], 1.6, glow));
        }
        out
    }

    /// The countdown's last point flying up to where the ✓ starts, with a
    /// short trail.
    fn fly(&self, alpha: f32) -> Vec<El> {
        let p0 = [RECEIPT.l + 14.0, RECEIPT.b - 0.75];
        let p2 = [CHECK_X + CHECK[0][0], CHECK_Y + CHECK[0][1]];
        let c = [p0[0] - 12.0, p2[1] + 6.0];
        let at = |t: f32| {
            let q = self.after(t - T_SENT, 0.42, 0.12).clamp(0.0, 1.05);
            let m = 1.0 - q;
            [
                m * m * p0[0] + 2.0 * m * q * c[0] + q * q * p2[0],
                m * m * p0[1] + 2.0 * m * q * c[1] + q * q * p2[1],
            ]
        };
        // It goes out as the ✓ takes over.
        let shown = alpha * (1.0 - smoothstep(T_CHECK, T_CHECK + 0.12, self.t));
        let mut out = Vec::new();
        for k in (1..=5).rev() {
            let back = self.t - 0.05 * k as f32;
            if back < T_SENT {
                continue;
            }
            let fade = 1.0 - k as f32 / 6.0;
            out.push(
                el().key(format!("trail-{k}"))
                    .absolute(at(back)[0] - 1.0, at(back)[1] - 1.0)
                    .size(2.0, 2.0)
                    .radius(1.0)
                    .fill(blue().alpha(0.5 * fade * shown))
                    .scale(1.0 - 0.12 * k as f32),
            );
        }
        out.push(dot("flyer", at(self.t), 1.6, shown));
        out
    }
}

/// I3's line at `strength`: two halves meeting on a whole point in the
/// middle, each its own gradient, as bright as each point is flat.
fn centre_line(p: Paint, lane: &Lane, strength: f32) -> Vec<El> {
    if strength <= 0.002 {
        return Vec::new();
    }
    let ink = ink(p, &lane.scope);
    let columns = lane.scope.columns();
    let pitch = lane.scope.tuning().spacing;
    let half = (columns as f32 - 1.0) * 0.5 * pitch + pitch * 0.5;
    let at = |x: f32| {
        let c = ((x / pitch) + (columns as f32 - 1.0) * 0.5).clamp(0.0, columns as f32 - 1.0);
        let i = (c.floor() as usize).min(columns.saturating_sub(2));
        let k = c - i as f32;
        let f = lerp(lane.columns[i].x, lane.columns[i + 1].x, k).clamp(0.0, 1.0);
        (f, c / (columns as f32 - 1.0))
    };
    let out = [0.0, 0.25, 0.5, 0.7, 0.82, 0.9, 0.96, 1.0];
    let alpha = |x: f32| {
        let (f, u) = at(x);
        LINE_ALPHA * strength * f * edge(u)
    };
    vec![
        soft_line(FIELD_CX - half, FIELD_CX, &out.map(|d| 1.0 - d), ink, |s| {
            alpha(-(1.0 - s) * half)
        }),
        soft_line(FIELD_CX, FIELD_CX + half, &out, ink, |d| alpha(d * half)),
    ]
}

/// The record dot, the voice and Stop, in the lane's points. `vis` takes
/// each column and where it is across the field (0–1) to its dots' alpha
/// and a brightening.
fn pill(
    p: Paint,
    lane: &Lane,
    lit: f32,
    alpha: f32,
    vis: impl Fn(usize, f32) -> (f32, f32),
    stop: Stop,
) -> Vec<El> {
    let red = p.danger();
    let r = 3.0 * lit.max(0.0);
    let dot_alpha = (lit * 3.0).clamp(0.0, 1.0) * alpha;
    let mut out = Vec::new();
    if r > 0.01 {
        out.push(
            el().key("rec")
                .absolute(PILL.l + 13.0 - r, MID_Y - r)
                .size(2.0 * r, 2.0 * r)
                .radius(r)
                .fill(red.alpha(dot_alpha))
                .shadow(0.0, 5.0, red.alpha(0.45 * dot_alpha)),
        );
    }
    out.push(
        field(p, &lane.scope, |c, u| {
            let (a, boost) = vis(c, u);
            (a * alpha, boost * alpha)
        })
        .key("field")
        .absolute(FIELD_X, MID_Y - FIELD_H * 0.5),
    );
    out.push(
        el().key("stop")
            .absolute(PILL.l + 148.0, PILL.t + 4.0)
            .size(24.0, 24.0)
            .radius(12.0)
            .fill(p.text().alpha(0.1))
            .child(
                el().absolute(8.0, 8.0)
                    .size(8.0, 8.0)
                    .radius(2.0)
                    .fill(p.text().alpha(0.92)),
            )
            .scale(stop.scale)
            .opacity(stop.alpha * alpha)
            .blur(stop.blur),
    );
    out
}

#[derive(Clone, Copy)]
struct Stop {
    scale: f32,
    alpha: f32,
    blur: f32,
}

impl Stop {
    fn rest() -> Self {
        Self {
            scale: 1.0,
            alpha: 1.0,
            blur: 0.0,
        }
    }
}

/// The colour of Dot scope's unlit dots now: the flat line's and beads'.
fn ink(p: Paint, scope: &Scope) -> Color {
    srgb(palette(p.k.dark, &scope.pose(), scope.tuning().saturation).base)
}

/// Dot scope's field, its simulation's dots lit as its shader lights them,
/// each column at the alpha and brightening `vis` gives it.
fn field(p: Paint, scope: &Scope, vis: impl Fn(usize, f32) -> (f32, f32)) -> El {
    let (w, h) = (FIELD_W, FIELD_H);
    let dark = p.k.dark;
    let tuning = scope.tuning();
    let pose = scope.pose();
    let colors = palette(dark, &pose, tuning.saturation);
    let (base, core) = (srgb(colors.base), srgb(colors.core));
    let (columns, rows) = (scope.columns(), scope.rows());
    let pitch = tuning.spacing;
    let radius = h * 0.5;
    let (unlit, full, curve) = if dark {
        (0.07, 0.96, 0.9)
    } else {
        (0.085, 1.0, 0.75)
    };
    let presence = 0.5 + 0.5 * pose.live.max(pose.cool);
    let mut dots = el().size(w, h);
    for (n, &[power, fresh]) in scope.dots().iter().enumerate() {
        let (i, j) = (n % columns, n / columns);
        let (shown, boost) = vis(i, i as f32 / (columns as f32 - 1.0).max(1.0));
        if shown <= 0.003 && boost <= 0.003 {
            continue;
        }
        let x = (i as f32 - (columns as f32 - 1.0) * 0.5) * pitch;
        let y = (j as f32 - (rows as f32 - 1.0) * 0.5) * pitch;
        let cap_dx = (x.abs() - (w * 0.5 - radius)).max(0.0);
        let cap = radius - (cap_dx * cap_dx + y * y).sqrt();
        let mask = smoothstep(1.4, 4.6, cap);
        if mask <= 0.0 {
            continue;
        }
        let power = power.max(1e-5);
        let row = y.abs() / radius;
        let floor = unlit * (1.0 - 0.7 * smoothstep(0.35, 0.95, row)) * presence * pose.grid;
        let alpha = floor + (full - floor) * power.powf(curve * (1.4 + (1.0 - 1.4) * fresh));
        let shrink = 1.0 + ((0.82 + 0.18 * fresh) - 1.0) * smoothstep(0.03, 0.2, power);
        let r = (0.25 + 0.17 * power.powf(0.6)) * shrink * pitch + 0.25 * boost;
        let lit = (power * power * 1.1).clamp(0.0, 0.9) * (0.3 + 0.7 * fresh);
        let color = base.mix(core, lit);
        let a = ((alpha * shown + boost * (0.5 + 0.5 * power)) * mask).min(1.0);
        if a < 0.01 {
            continue;
        }
        if power > 0.05 && shown > 0.01 {
            let halo = 2.0 * r;
            dots = dots.child(
                el().key(format!("h{n}"))
                    .absolute(w * 0.5 + x - halo, h * 0.5 + y - halo)
                    .size(2.0 * halo, 2.0 * halo)
                    .radius(halo)
                    .fill(color.alpha(0.132 * power * power * mask * shown)),
            );
        }
        dots = dots.child(
            el().key(n.to_string())
                .absolute(w * 0.5 + x - r, h * 0.5 + y - r)
                .size(2.0 * r, 2.0 * r)
                .radius(r)
                .fill(color.alpha(a)),
        );
    }
    dots
}

/// A line across the field from `x0` to `x1`, `LINE_W` thick at its
/// middle, with `alpha` sampled at `at` (0–1 along it) as a gradient, so
/// it has no joints.
fn soft_line(x0: f32, x1: f32, at: &[f32; 8], ink: Color, alpha: impl Fn(f32) -> f32) -> El {
    let stops = at.map(|s| (s, ink.alpha(alpha(s).clamp(0.0, 1.0))));
    el().absolute(x0, MID_Y - LINE_W * 0.5)
        .size(x1 - x0, LINE_W)
        .linear_gradient(FRAC_PI_2, stops)
}

fn stroke(key: &str, points: &[[f32; 2]], width: f32, color: Color) -> El {
    canvas(
        Shape::new()
            .stroke_with(points.to_vec(), false, width, LineCap::Round, color)
            .view_box(W, LANE_H),
    )
    .key(key)
    .absolute(0.0, 0.0)
    .size(W, LANE_H)
}

/// A bright point with a blue glow.
fn dot(key: &str, [x, y]: [f32; 2], r: f32, alpha: f32) -> El {
    el().key(key)
        .absolute(x - r, y - r)
        .size(2.0 * r, 2.0 * r)
        .radius(r)
        .fill(head().alpha(alpha))
        .shadow(0.0, 5.0, blue().alpha(0.9 * alpha))
}

/// The ✓, `draw` (0–1) of the way along.
fn check(draw: f32, color: Color) -> El {
    let points = CHECK.map(|[x, y]| [CHECK_X + x, CHECK_Y + y]);
    stroke("check", &trimmed(&points, draw), 1.6, color)
}

fn trimmed(points: &[[f32; 2]], fraction: f32) -> Vec<[f32; 2]> {
    let length = |a: [f32; 2], b: [f32; 2]| (b[0] - a[0]).hypot(b[1] - a[1]);
    let total: f32 = points.windows(2).map(|w| length(w[0], w[1])).sum();
    let mut left = fraction.clamp(0.0, 1.0) * total;
    let mut out = vec![points[0]];
    for w in points.windows(2) {
        let d = length(w[0], w[1]);
        if left > d {
            out.push(w[1]);
            left -= d;
        } else {
            let k = left / d.max(1e-6);
            out.push([lerp(w[0][0], w[1][0], k), lerp(w[0][1], w[1][1], k)]);
            break;
        }
    }
    out
}

/// The receipt's row: where it went, Undo with its ring, and ✕ (drawn
/// apart, as Sent keeps it).
fn receipt(p: Paint, ring: Option<&Ring>) -> El {
    let k = p.k;
    let name = TextStyle {
        size: 13.0,
        weight: 600.0,
        ..k.ty.ui
    };
    let source = TextStyle {
        size: 12.0,
        ..k.ty.ui
    };
    let undo_w = (measure(undo_label(), medium()) + 20.0).round();
    let undo = undo_ring::chip(
        p,
        row().center().size(undo_w, 28.0),
        [undo_w, 28.0],
        k.c.bg_base,
        ring,
    )
    .key("undo")
    .absolute(CLOSE_X - 8.0 - undo_w, ROW_Y + 4.0);
    el().absolute(0.0, 0.0)
        .size(W, LANE_H)
        .child(
            row()
                .key("where")
                .absolute(ROW_X + 8.0, ROW_Y)
                .h(36.0)
                .center_y()
                .gap(8.0)
                .child(thumb(p))
                .child(
                    row()
                        .key("to")
                        .center_y()
                        .gap(2.0)
                        .child(text("SEER").font(name).color(p.text()))
                        .child(chevron(p)),
                )
                .child(
                    text("docs.example.com")
                        .key("source")
                        .font(source)
                        .color(p.muted()),
                ),
        )
        .child(undo)
}

/// The capture's thumbnail: a window with a line and a mark on it.
fn thumb(p: Paint) -> El {
    el().key("thumb")
        .size(28.0, 18.0)
        .radius(3.0)
        .fill(rgb(26, 28, 36))
        .linear_gradient(PI, [(0.0, rgb(52, 58, 74)), (1.0, rgb(26, 28, 36))])
        .border(0.5, Color::WHITE.alpha(0.12))
        .child(
            el().absolute(5.0, 6.0)
                .size(13.0, 1.5)
                .radius(0.75)
                .fill(Color::WHITE.alpha(0.35)),
        )
        .child(
            el().absolute(19.0, 9.5)
                .size(4.0, 4.0)
                .radius(2.0)
                .fill(p.danger()),
        )
}

fn chevron(p: Paint) -> El {
    canvas(
        Shape::new()
            .stroke_with(
                vec![[3.0, 4.5], [6.0, 7.5], [9.0, 4.5]],
                false,
                1.2,
                LineCap::Round,
                p.muted(),
            )
            .view_box(12.0, 12.0),
    )
    .key("chevron")
    .size(12.0, 12.0)
}

/// "Sent to SEER" and what it carried; the ✓ draws apart.
fn sent_row(p: Paint) -> El {
    let k = p.k;
    let detail = TextStyle {
        size: 12.0,
        ..k.ty.ui
    };
    row()
        .absolute(CHECK_X + 14.0 + 8.0, ROW_Y)
        .h(36.0)
        .center_y()
        .gap(8.0)
        .child(
            text("Sent to SEER")
                .key("to")
                .font(k.ty.ui_medium)
                .color(p.text()),
        )
        .child(
            text("2 marks · 0:14")
                .key("what")
                .font(detail)
                .color(p.muted()),
        )
}

/// The capsule's glass, as the real shell draws it, with its morph glow.
fn shell(p: Paint, rect: Rect, radius: f32, glow: f32) -> El {
    let k = p.k;
    let (w, h) = (rect.w(), rect.h());
    let mut body = el()
        .absolute(rect.l, rect.t)
        .size(w, h)
        .radius(radius)
        .fill(k.c.bg_base);
    for shadow in &k.elevation.overlay {
        body = body.box_shadow(*shadow);
    }
    if glow > 0.002 {
        body = body.child(
            el().key("glow")
                .absolute(0.0, 0.0)
                .size(w, h)
                .radius(radius)
                .box_shadow(rune::Shadow {
                    y: 1.5,
                    blur: 8.0,
                    color: k.alpha(k.c.text_base, if k.dark { 0.10 } else { 0.06 }),
                    inset: true,
                    ..Default::default()
                })
                .opacity(glow),
        );
    }
    if k.dark {
        body = body.child(rim(k, w, h, radius));
    }
    body
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn springs_from_rest_land_and_start_at_zero() {
        assert_eq!(spring(-0.1, MORPH), 0.0);
        assert_eq!(spring(0.0, MORPH), 0.0);
        assert!((spring(2.0, MORPH) - 1.0).abs() < 1e-3);
    }

    #[test]
    fn silence_lays_the_line_down_and_speech_lifts_it() {
        let mut m = Motion::new(Section::Silence, false);
        let step = 1.0 / 240.0;
        let flat_at = |m: &mut Motion, until: f32| {
            while m.t < until {
                m.advance(step, false);
            }
            m.lanes[1].flat.x
        };
        assert!(flat_at(&mut m, 1.5) > 0.99);
        // Speaking from 2.0 s splits it.
        assert!(flat_at(&mut m, 3.0) < 0.05);
        // Silent again from 4.4 s: flat once the pause has held.
        assert!(flat_at(&mut m, 6.5) > 0.95);
    }

    #[test]
    fn the_centre_goes_flat_before_the_ends() {
        let mut m = Motion::new(Section::Silence, false);
        while m.t < 4.4 + SILENCE_S + 0.25 {
            m.advance(1.0 / 240.0, false);
        }
        let columns = &m.lanes[2].columns;
        let (middle, end) = (columns[columns.len() / 2].x, columns[0].x);
        assert!(middle > end + 0.2, "{middle} vs {end}");
    }

    #[test]
    fn every_finish_starts_from_i3s_line() {
        let mut m = Motion::new(Section::Finish, false);
        while m.t < T_STOP - 0.01 {
            m.advance(1.0 / 240.0, false);
        }
        for lane in &m.lanes {
            assert!(
                lane.columns.iter().all(|c| c.x > 0.95),
                "{:?}",
                lane.variant
            );
        }
    }

    #[test]
    fn the_check_trims_along_its_length() {
        let points = [[0.0, 0.0], [3.0, 4.0], [3.0, 9.0]];
        assert_eq!(trimmed(&points, 0.0), vec![[0.0, 0.0], [0.0, 0.0]]);
        assert_eq!(trimmed(&points, 1.0), points.to_vec());
        assert_eq!(trimmed(&points, 0.5), vec![[0.0, 0.0], [3.0, 4.0]]);
    }

    #[test]
    fn a_clip_plays_again_when_it_ends() {
        let mut m = Motion::new(Section::Entrance, false);
        for _ in 0..(240.0 * Section::Entrance.duration()) as usize + 10 {
            m.advance(1.0 / 240.0, false);
        }
        assert!(m.t < 0.1);
    }
}
