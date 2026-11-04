//! Round 2 of the Annotate pill (t1290, t1291): B, the living pill, as the
//! dictation HUD's own Dot scope at its real size and look (112 × 26, its
//! plate, rim, shadow, glow and halos), with no timer. Stop and ✕ come in
//! on hover or sit past the field's right cap.
//!
//! After Stop it is one object: the pill keeps its top edge and grows
//! into the capsule's end states, the field collapsing into their lead.
//! The suggestions (then a pick and Sent), a rule's send with Undo, or a
//! failure. A pointer plays the person: it stops the recording and picks.
//!
//! A prototype in the capsule playground only: the shipping capsule
//! doesn't draw this way.

use std::f32::consts::PI;
use std::sync::Arc;

use rune::{Color, El, Shadow, Spring, SpringPair, Transition, canvas, el, row};

use super::super::surface::{
    self, HEADER_H, ISLAND_H, Id, LIST_PAD, LIST_W, MEDIUM, ROW_H, SHELL_RADIUS, SMALL, THUMB_H,
    THUMB_W, chord, done_style, measure,
};
use super::super::{
    BASELINE_DROP, CHECK, HEADER_PB, ICON_BUTTON_RADIUS, ROW_PX, ROW_RADIUS, hint, hint_width,
    icon, label, thumbnail, tuning,
};
use super::Paint;
use super::pill::{BEZEL, SHELL, display, field, plate_middle, srgb};

/// Round 1's shot, tall enough for the suggestions under the menu bar.
pub(super) const SHOT: (f32, f32) = (640.0, 250.0);
use crate::rune_hud::dot_scope::{Frame, Placement, Scope, Stage, Tuning, demo_loudness, palette};

/// When each step starts, seconds into the loop.
const IN: f64 = 0.25;
const STOP: f64 = 2.2;
/// The failure's dots turn coral, as the HUD's do, before it says so.
const FAIL: f64 = 2.6;
const END: f64 = 2.9;
const PICK: f64 = 4.1;
/// A rule's send goes once its Undo has had three seconds.
const RULE_SENT: f64 = END + 3.0;

/// The finished pill, the list's top row: the capsule's own height.
const TOP_H: f32 = ISLAND_H;
const BUTTON: f32 = 22.0;
/// Stop and ✕ past the field's right cap.
const CONTROLS: f32 = 4.0 + BUTTON + 2.0 + BUTTON + 2.0;
const CLOSE: f32 = 28.0;

const ROWS: [(&str, &str); 4] = [
    ("SEER", "same page"),
    ("HEX", "last sent"),
    ("HOME", "yesterday"),
    ("RUNE", "2 days ago"),
];
const SENT: &str = "Sent to SEER";
const SENDING: &str = "Sending to SEER";
const COPIED: &str = "Copied";
const FAILED: &str = "Couldn't save";
const WHY: &str = "the window closed";
const UNDO: &str = "Undo";

/// Where Stop and ✕ are.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Controls {
    /// The field alone, widening for them when the pointer is over it.
    Hover,
    /// Always past the field's right cap.
    Always,
}

/// How the capture ends.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum End {
    /// The suggestions, a pick, then Sent with Undo.
    List,
    /// A saved rule sends it, with three seconds to Undo.
    Rule,
    Failure,
}

/// One variation.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) struct Look {
    pub(super) controls: Controls,
    pub(super) end: End,
}

impl Look {
    pub(super) const ALL: [Self; 4] = [
        Self::new(Controls::Hover, End::List),
        Self::new(Controls::Always, End::List),
        Self::new(Controls::Hover, End::Rule),
        Self::new(Controls::Hover, End::Failure),
    ];

    pub(super) const fn new(controls: Controls, end: End) -> Self {
        Self { controls, end }
    }

    pub(super) fn label(self) -> &'static str {
        match (self.controls, self.end) {
            (Controls::Hover, End::List) => "B2 · Hover",
            (Controls::Always, End::List) => "B2 · Always",
            (_, End::Rule) => "B2 · Rule",
            (_, End::Failure) => "B2 · Failure",
        }
    }

    fn idea(self) -> &'static str {
        match (self.controls, self.end) {
            (Controls::Hover, End::List) => {
                "The HUD's own Dot scope, no timer. Stop and ✕ open out on hover; it grows into the suggestions."
            }
            (Controls::Always, End::List) => {
                "Stop and ✕ always past the field's right cap; the same growth into the suggestions."
            }
            (_, End::Rule) => "A saved rule sends it: the field becomes the thumbnail, with Undo.",
            (_, End::Failure) => "The dots go coral as the HUD's do, then it says what went wrong.",
        }
    }

    fn out(self) -> f64 {
        match self.end {
            End::List => 5.9,
            End::Rule => RULE_SENT + 1.0,
            End::Failure => 5.3,
        }
    }

    fn period(self) -> f64 {
        self.out() + 0.6
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Phase {
    Hidden,
    Recording,
    Working,
    Ending,
}

/// The look's state: which one, its clock, and its Dot scope.
pub(super) struct Living {
    pub(super) look: Look,
    /// Seconds into the loop.
    t: f64,
    /// The last wall time the Labs story stepped by.
    wall: Option<f64>,
    scope: Scope,
}

impl Living {
    pub(super) fn new(look: Look) -> Self {
        Self {
            look,
            t: 0.0,
            wall: None,
            scope: Scope::new(Tuning::default()),
        }
    }

    fn phase(&self) -> Phase {
        match self.t {
            t if t < IN => Phase::Hidden,
            t if t < STOP => Phase::Recording,
            t if t < END => Phase::Working,
            t if t < self.look.out() => Phase::Ending,
            _ => Phase::Hidden,
        }
    }

    /// Steps the loop and the voice `dt` seconds.
    pub(super) fn advance(&mut self, dt: f64, reduced: bool) {
        let before = self.phase();
        self.t += dt.clamp(0.0, 0.1);
        if self.t >= self.look.period() {
            self.t %= self.look.period();
        }
        let phase = self.phase();
        if phase != before && phase == Phase::Recording {
            self.scope.restart(reduced);
        }
        let stage = match phase {
            Phase::Working if self.look.end == End::Failure && self.t >= FAIL => Stage::Failed,
            Phase::Working => Stage::Processing,
            Phase::Ending => Stage::Done,
            Phase::Recording | Phase::Hidden => Stage::Recording,
        };
        self.scope.advance(
            dt as f32,
            Frame {
                stage,
                visible: matches!(phase, Phase::Recording | Phase::Working),
                loudness: demo_loudness((self.t - IN).max(0.0) as f32),
                reduce_motion: reduced,
            },
        );
    }

    /// Steps by wall time, for the Labs story.
    fn advance_wall(&mut self, now: f64) {
        let dt = self.wall.map_or(0.0, |last| (now - last).max(0.0));
        self.wall = Some(now);
        self.advance(dt, false);
    }

    /// Where the pointer is and how much it shows, if at all.
    fn pointer(&self) -> Option<([f32; 2], f32)> {
        let [cx, top] = origin();
        let stop = stop_middle();
        let rest = [cx + 70.0, top + 70.0];
        let mut keys = vec![(0.95, [cx + 90.0, top + 110.0])];
        if self.look.controls == Controls::Hover {
            keys.push((1.45, [cx + 28.0, top + 16.0]));
            keys.push((1.9, stop));
        } else {
            keys.push((1.6, stop));
        }
        keys.push((STOP, stop));
        keys.push((2.6, rest));
        let (gone, list) = match self.look.end {
            End::List => {
                let row = row_rect(0);
                let at = [cx + 120.0, row[1] + row[3] * 0.5];
                keys.extend([(3.35, rest), (3.85, at), (PICK, at)]);
                keys.push((4.6, [cx + 170.0, top + 160.0]));
                (4.4, true)
            }
            End::Rule | End::Failure => (2.3, false),
        };
        let t = self.t;
        let show = ramp(t, 0.95, 1.15) * (1.0 - ramp(t, gone, gone + 0.4));
        if show <= 0.0 || (!list && t > gone + 0.4) {
            return None;
        }
        let at = match keys.iter().position(|&(k, _)| k > t) {
            None => keys[keys.len() - 1].1,
            Some(0) => keys[0].1,
            Some(n) => {
                let ((t0, a), (t1, b)) = (keys[n - 1], keys[n]);
                let e = ease(((t - t0) / (t1 - t0)) as f32);
                [a[0] + (b[0] - a[0]) * e, a[1] + (b[1] - a[1]) * e]
            }
        };
        Some((at, show))
    }

    fn pointer_in(&self, rect: [f32; 4]) -> bool {
        self.pointer()
            .is_some_and(|([px, py], show)| show > 0.5 && inside(rect, px, py))
    }

    /// Whether Stop and ✕ show.
    fn controls(&self) -> bool {
        self.phase() == Phase::Recording
            && match self.look.controls {
                Controls::Always => true,
                Controls::Hover => self.pointer_in(wide()),
            }
    }
}

fn ramp(t: f64, from: f64, to: f64) -> f32 {
    ((t - from) / (to - from)).clamp(0.0, 1.0) as f32
}

fn ease(t: f32) -> f32 {
    let t = t.clamp(0.0, 1.0);
    if t < 0.5 {
        4.0 * t * t * t
    } else {
        1.0 - (-2.0 * t + 2.0).powi(3) * 0.5
    }
}

fn inside([x, y, w, h]: [f32; 4], px: f32, py: f32) -> bool {
    px >= x && px <= x + w && py >= y && py <= y + h
}

fn hud() -> Tuning {
    Tuning::default()
}

/// The plate's middle x and its top edge, where `hud_placement` puts the
/// HUD on a flat display: every state keeps this top edge.
fn origin() -> [f32; 2] {
    let t = hud();
    let [cx, cy] = plate_middle(Placement::Below, &display(), (t.width, t.height), 0.0);
    [cx, cy - t.height * 0.5]
}

/// The recording with Stop and ✕ showing.
fn wide() -> [f32; 4] {
    let [cx, top] = origin();
    let t = hud();
    let w = t.width + CONTROLS;
    [cx - w * 0.5, top, w, t.height]
}

fn stop_rect() -> [f32; 4] {
    let [x, top, _, _] = wide();
    let t = hud();
    [
        x + t.width + 4.0,
        top + (t.height - BUTTON) * 0.5,
        BUTTON,
        BUTTON,
    ]
}

fn stop_middle() -> [f32; 2] {
    let [x, y, w, h] = stop_rect();
    [x + w * 0.5, y + h * 0.5]
}

fn list_rect() -> [f32; 4] {
    let [cx, top] = origin();
    let h = TOP_H + LIST_PAD + HEADER_H + ROWS.len() as f32 * ROW_H + LIST_PAD;
    [cx - LIST_W * 0.5, top, LIST_W, h]
}

fn row_rect(n: usize) -> [f32; 4] {
    let [x, top, w, _] = list_rect();
    [
        x + LIST_PAD,
        top + TOP_H + LIST_PAD + HEADER_H + n as f32 * ROW_H,
        w - 2.0 * LIST_PAD,
        ROW_H,
    ]
}

/// What one piece of content is.
#[derive(Clone, Copy, Debug, PartialEq)]
enum Piece {
    Field,
    Stop,
    Discard,
    Thumb,
    Warn,
    Close,
    Search,
    Divider,
    Header,
    Row(usize),
    /// The finished pill's words.
    Words(&'static str),
    /// Quieter words after them.
    Quiet(&'static str),
    Countdown,
    Undo,
}

impl Piece {
    fn key(self) -> String {
        match self {
            Self::Row(n) => format!("row-{}", ROWS[n].0),
            Self::Words(s) => format!("words-{s}"),
            Self::Quiet(s) => format!("quiet-{s}"),
            other => format!("{other:?}").to_lowercase(),
        }
    }

    fn lead(self) -> bool {
        matches!(self, Self::Thumb | Self::Warn)
    }

    fn width(self) -> f32 {
        match self {
            Self::Words(s) => measure(s, done_style()).ceil(),
            Self::Quiet(s) => measure(s, MEDIUM).ceil(),
            Self::Countdown => measure("3 s", MEDIUM).ceil(),
            Self::Undo => measure(UNDO, MEDIUM).ceil(),
            Self::Close => CLOSE,
            _ => 0.0,
        }
    }

    /// The room before it in a finished pill.
    fn gap(self) -> f32 {
        match self {
            Self::Words(_) => 0.0,
            Self::Quiet(_) | Self::Countdown => 12.0,
            Self::Undo => 20.0,
            _ => 10.0,
        }
    }
}

/// The shape and its content at one moment, in screen points.
struct Layout {
    shell: [f32; 4],
    radius: f32,
    shown: bool,
    pieces: Vec<([f32; 4], Piece)>,
}

/// A finished pill: `lead`, then `items` in a row, centred under the HUD.
fn line(lead: Piece, items: &[Piece]) -> Layout {
    let [cx, top] = origin();
    let text_x = if lead == Piece::Thumb {
        12.0 + THUMB_W + 10.0
    } else {
        12.0 + CHECK + 8.0
    };
    let w = text_x + items.iter().map(|p| p.gap() + p.width()).sum::<f32>() + 6.0;
    let x = cx - w * 0.5;
    let lead_rect = if lead == Piece::Thumb {
        [x + 12.0, top + (TOP_H - THUMB_H) * 0.5, THUMB_W, THUMB_H]
    } else {
        [x + 12.0, top + (TOP_H - CHECK) * 0.5, CHECK, CHECK]
    };
    let mut pieces = vec![(lead_rect, lead)];
    let mut at = x + text_x;
    for &item in items {
        at += item.gap();
        let rect = if item == Piece::Close {
            [at, top + (TOP_H - CLOSE) * 0.5, CLOSE, CLOSE]
        } else {
            [at, top, item.width(), TOP_H]
        };
        pieces.push((rect, item));
        at += item.width();
    }
    Layout {
        shell: [x, top, w, TOP_H],
        radius: TOP_H * 0.5,
        shown: true,
        pieces,
    }
}

/// The suggestions: the thumbnail's row, then "Send to" and its rows.
fn list() -> Layout {
    let [x, top, w, h] = list_rect();
    let close = [
        x + w - 6.0 - CLOSE,
        top + (TOP_H - CLOSE) * 0.5,
        CLOSE,
        CLOSE,
    ];
    let search = hint_width("Search", &chord(&surface::shortcut_keycaps()));
    let mut pieces = vec![
        (
            [x + 12.0, top + (TOP_H - THUMB_H) * 0.5, THUMB_W, THUMB_H],
            Piece::Thumb,
        ),
        (
            [
                x + 12.0 + THUMB_W + 10.0,
                top,
                Piece::Words(COPIED).width(),
                TOP_H,
            ],
            Piece::Words(COPIED),
        ),
        ([close[0] - 8.0 - search, top, search, TOP_H], Piece::Search),
        (close, Piece::Close),
        ([x, top + TOP_H, w, 1.0], Piece::Divider),
        (
            [
                x + LIST_PAD,
                top + TOP_H + LIST_PAD,
                w - 2.0 * LIST_PAD,
                HEADER_H,
            ],
            Piece::Header,
        ),
    ];
    pieces.extend((0..ROWS.len()).map(|n| (row_rect(n), Piece::Row(n))));
    Layout {
        shell: [x, top, w, h],
        radius: SHELL_RADIUS,
        shown: true,
        pieces,
    }
}

/// The end state's lead: where the field collapses to.
fn lead_rect(end: End) -> [f32; 4] {
    let l = match end {
        End::List => list(),
        End::Rule => line(Piece::Thumb, &[]),
        End::Failure => line(Piece::Warn, &[]),
    };
    l.pieces[0].0
}

fn layout(o: &Living) -> Layout {
    let t = hud();
    let [cx, top] = origin();
    let field = |middle: f32| {
        (
            [middle - t.width * 0.5, top, t.width, t.height],
            Piece::Field,
        )
    };
    match o.phase() {
        Phase::Hidden => Layout {
            shell: [cx - t.height * 0.5, top, t.height, t.height],
            radius: t.height * 0.5,
            shown: false,
            pieces: Vec::new(),
        },
        Phase::Recording if o.controls() => {
            let [x, y, w, h] = wide();
            let stop = stop_rect();
            Layout {
                shell: [x, y, w, h],
                radius: h * 0.5,
                shown: true,
                pieces: vec![
                    field(x + t.width * 0.5),
                    (stop, Piece::Stop),
                    (
                        [stop[0] + BUTTON + 2.0, stop[1], BUTTON, BUTTON],
                        Piece::Discard,
                    ),
                ],
            }
        }
        Phase::Recording => Layout {
            shell: [cx - t.width * 0.5, top, t.width, t.height],
            radius: t.height * 0.5,
            shown: true,
            pieces: vec![field(cx)],
        },
        Phase::Working => {
            let w = t.processing_width;
            Layout {
                shell: [cx - w * 0.5, top, w, t.height],
                radius: t.height * 0.5,
                shown: true,
                pieces: vec![field(cx)],
            }
        }
        Phase::Ending => match o.look.end {
            End::List if o.t < PICK => list(),
            End::List => line(
                Piece::Thumb,
                &[Piece::Words(SENT), Piece::Undo, Piece::Close],
            ),
            End::Rule if o.t < RULE_SENT => line(
                Piece::Thumb,
                &[
                    Piece::Words(SENDING),
                    Piece::Countdown,
                    Piece::Undo,
                    Piece::Close,
                ],
            ),
            End::Rule => line(Piece::Thumb, &[Piece::Words(SENT), Piece::Close]),
            End::Failure => line(
                Piece::Warn,
                &[Piece::Words(FAILED), Piece::Quiet(WHY), Piece::Close],
            ),
        },
    }
}

/// The playground's stage for `o`: the top middle of a flat display under
/// a bezel, and what the look is.
pub(super) fn stage(p: Paint, o: &Living, reduced: bool, size: (f32, f32)) -> El {
    let screen = display();
    let (w, h) = SHOT;
    let sw = screen.frame.width as f32;
    let caption = rune::col()
        .key("caption")
        .absolute(16.0, BEZEL + h + 16.0)
        .w(w - 32.0)
        .gap(4.0)
        .child(
            rune::text(o.look.label())
                .key("label")
                .font(p.k.ty.ui_medium.lh(16.0))
                .color(p.text()),
        )
        .child(
            rune::text(o.look.idea())
                .key("idea")
                .font(p.k.ty.ui.lh(18.0))
                .color(p.muted())
                .wrap(),
        );
    el().key("stage")
        .size(size.0, size.1)
        .fill(p.k.c.layer_03)
        .child(
            el().key("shot")
                .size(w, BEZEL + h)
                .fill(p.k.c.black)
                .clip_rounded(0.0)
                .child(desktop(p, o, reduced, sw).absolute((w - sw) * 0.5, BEZEL)),
        )
        .child(caption)
}

/// The screen: its wash, menu bar, the capsule, and the pointer.
fn desktop(p: Paint, o: &Living, reduced: bool, w: f32) -> El {
    let k = p.k;
    let wash = |name, step| rune_theme_opencode::scale(name, k.dark, step);
    let mut desktop = el()
        .key("desktop")
        .size(w, 400.0)
        .fill(wash("cobalt", 3))
        .linear_gradient(
            PI * 0.75,
            [(0.0, wash("cobalt", 3)), (1.0, wash("lilac", 5))],
        )
        .child(super::stories::menu_bar(p, w, 24.0))
        .child(capsule(p, o, reduced, w));
    if let Some(([x, y], show)) = o.pointer() {
        desktop = desktop.child(pointer().absolute(x - 1.0, y - 1.0).opacity(show));
    }
    desktop
}

/// The arrow pointer, its tip at its top-left.
fn pointer() -> El {
    let arrow = rune::Path::new()
        .move_to([1.0, 1.0])
        .line_to([1.0, 16.0])
        .line_to([5.0, 12.5])
        .line_to([7.6, 18.2])
        .line_to([10.2, 17.0])
        .line_to([7.7, 11.6])
        .line_to([12.6, 11.2])
        .close();
    canvas(
        rune::Shape::new()
            .view_box(14.0, 20.0)
            .fill_path(&arrow, Color::BLACK)
            .stroke_path(&arrow, 1.2, rune::LineCap::Round, Color::WHITE),
    )
    .key("pointer")
    .motion(Spring::NONE)
    .size(14.0, 20.0)
}

fn rgba([r, g, b, a]: [f32; 4]) -> Color {
    srgb([r, g, b]).alpha(a)
}

/// The capsule: the HUD's plate as its shell, and what's in it.
fn capsule(p: Paint, o: &Living, reduced: bool, w_screen: f32) -> El {
    let l = layout(o);
    let k = p.k;
    let t = tuning::get();
    let [x, y, w, h] = l.shell;
    let colors = palette(k.dark, &o.scope.pose(), hud().saturation);
    let fade = Spring::new(SHELL.duration, 0.0);
    let shell = el()
        .key("shell")
        .absolute(x, y)
        .size(w, h)
        .motion(SHELL)
        .radius(l.radius)
        .fill(rgba(colors.plate))
        .box_shadow(Shadow {
            x: 0.0,
            y: 3.0,
            blur: 11.0,
            spread: 0.0,
            color: rgba(colors.shadow),
            inset: false,
        })
        .child(
            el().key("rim")
                .absolute(0.0, 0.0)
                .size(w, h)
                .radius(l.radius)
                .border(0.6, rgba(colors.rim))
                .motion(SHELL),
        )
        .opacity(if l.shown { 1.0 } else { 0.0 })
        .opacity_motion(fade);
    // The content is masked by the shape as it moves: the walls pass over
    // content that stays put.
    let springs = SpringPair::new(t.content_in, t.content_out);
    let toward = |rect: [f32; 4], at: [f32; 2], scale: f32| {
        if reduced {
            return Transition::opacity().springs(springs);
        }
        let anchor = rune::Vec2::new(
            (at[0] - rect[0]) / rect[2].max(1.0),
            (at[1] - rect[1]) / rect[3].max(1.0),
        );
        Transition::scale_anchor(scale, anchor)
            .combined(Transition::blur(t.content_blur))
            .springs(springs)
    };
    let middle = |r: [f32; 4]| [r[0] + r[2] * 0.5, r[1] + r[3] * 0.5];
    let mut content = el()
        .key("content")
        .absolute(0.0, 0.0)
        .size(w_screen, 400.0)
        .dissolve();
    for &(rect, piece) in &l.pieces {
        // The field leaves into the end state's lead, and the lead grows
        // out of where the field was: one thing moving into place.
        let transition = match piece {
            Piece::Field if o.phase() == Phase::Working => {
                toward(rect, middle(lead_rect(o.look.end)), 0.2)
            }
            p if p.lead() => {
                let hud = hud();
                let [cx, top] = origin();
                toward(rect, [cx, top + hud.height * 0.5], 0.3)
            }
            _ => toward(rect, middle(l.shell), t.content_scale),
        };
        content = content.child(
            draw(p, o, piece, rect)
                .key(piece.key())
                .absolute(rect[0], rect[1])
                .size(rect[2], rect[3])
                .motion(SHELL)
                .transition(transition),
        );
    }
    let inner = el()
        .key("inner")
        .absolute(-x, -y)
        .size(w_screen, 400.0)
        .motion(SHELL)
        .child(content);
    el().key(format!("{:?}", o.look))
        .absolute(0.0, 0.0)
        .size(w_screen, 400.0)
        .child(shell)
        .child(
            el().key("clip")
                .absolute(x, y)
                .size(w, h)
                .motion(SHELL)
                .clip_rounded(l.radius)
                .child(inner),
        )
}

/// One piece, filling its rect.
fn draw(p: Paint, o: &Living, piece: Piece, rect: [f32; 4]) -> El {
    let k = p.k;
    let c = &k.c;
    let [_, _, w, h] = rect;
    let done_ink = match tuning::get().top_line {
        tuning::TopLine::Quiet => p.muted(),
        tuning::TopLine::Icons => p.text(),
    };
    match piece {
        Piece::Field => {
            let pose = o.scope.pose();
            let dots = field(&o.scope, w, h, 1.0, k.dark, pose.width, &[]).radius(h * 0.5);
            if pose.glow <= 0.01 {
                return dots;
            }
            // The plate's glow from its middle, as the shader lights it.
            let colors = palette(k.dark, &pose, hud().saturation);
            let (base, core) = (srgb(colors.base), srgb(colors.core));
            let mid = base.mix(core, 0.5);
            let strength = if k.dark { 0.3 } else { 0.19 } * 0.55 * pose.glow.min(1.0);
            let stops: Vec<(f32, Color)> = (0..=4)
                .map(|i| {
                    let t = i as f32 / 4.0;
                    (t, mid.mix(base, t).alpha(strength * (1.0 - t)))
                })
                .collect();
            dots.radial_gradient([0.5, 0.5], 34.0 + 20.0 * pose.glow, stops)
        }
        Piece::Stop => {
            let pressed = (STOP - 0.12..STOP).contains(&o.t);
            let square = (w * 0.36).round();
            el().size(w, h)
                .radius(w * 0.5)
                .fill(k.alpha(c.text_base, if pressed { 0.2 } else { 0.1 }))
                .child(
                    el().absolute((w - square) * 0.5, (h - square) * 0.5)
                        .size(square, square)
                        .radius(2.0)
                        .fill(p.danger()),
                )
        }
        Piece::Discard => row()
            .center()
            .child(canvas(icon(Id::Close, 1.0, p.muted())).size(10.0, 10.0)),
        Piece::Thumb => match thumb() {
            Some(image) => thumbnail(p, image.pixels()).absolute(0.0, 0.0),
            None => el().size(w, h).radius(4.0).fill(c.layer_02),
        },
        Piece::Warn => canvas(icon(Id::Why, CHECK * 2.7 / 24.0, c.warning.fg)).size(w, h),
        Piece::Close => row()
            .center()
            .radius(ICON_BUTTON_RADIUS)
            .child(canvas(icon(Id::Close, 12.0 * 2.4 / 24.0, p.muted())).size(12.0, 12.0)),
        Piece::Search => {
            row()
                .center_y()
                .child(hint(p, "Search", &chord(&surface::shortcut_keycaps())))
        }
        Piece::Divider => el().size(w, p.hair).fill(c.border_weak),
        Piece::Header => row()
            .items_end()
            .px(ROW_PX)
            .pb(HEADER_PB)
            .child(label("Send to", k.ty.small_medium, p.faint()))
            .child(el().grow(1.0))
            .child(label("⇧ remembers", k.ty.small_medium, p.faint())),
        Piece::Row(n) => {
            let (name, why) = ROWS[n];
            let over = o.t < PICK && o.pointer_in(rect);
            let pressed = (PICK - 0.12..PICK).contains(&o.t);
            let mut entry = row()
                .center_y()
                .px(ROW_PX)
                .gap(ROW_PX)
                .radius(ROW_RADIUS)
                .child(label(name, MEDIUM, p.text()))
                .child(el().grow(1.0))
                .child(
                    label(why, SMALL, if n == 0 { p.accent() } else { p.muted() })
                        .offset(0.0, (MEDIUM.size - SMALL.size) * BASELINE_DROP),
                );
            if over {
                entry = entry.fill(if pressed { c.pressed } else { c.hover });
            }
            entry
        }
        Piece::Words(s) => row().center_y().child(label(s, done_style(), done_ink)),
        Piece::Quiet(s) => row().center_y().child(label(s, MEDIUM, p.faint())),
        Piece::Countdown => {
            let left = (RULE_SENT - o.t).ceil().clamp(1.0, 3.0) as u32;
            row()
                .center_y()
                .child(label(&format!("{left} s"), MEDIUM, p.faint()))
        }
        Piece::Undo => row().center_y().child(label(UNDO, MEDIUM, p.muted())),
    }
}

thread_local! {
    /// A real capture's first frame, as the goldens use: decoded once.
    static THUMB: Option<rune::Image> = {
        let images = rune::ImageCache::new(Arc::new(|| {}));
        let image = images
            .load(include_bytes!("../../../../tests/fixtures/capture-thumbnail.png").to_vec());
        image.wait().ok().map(|_| {
            // Decoding marks the image ready a moment before its pixels hold it.
            while image.pixels().size() == (0, 0) {
                std::thread::yield_now();
            }
            image
        })
    };
}

fn thumb() -> Option<rune::Image> {
    THUMB.with(Clone::clone)
}

/// The looks as a Labs story too.
pub(super) fn story() -> rune::Story<Living> {
    let pick = |look| move |s: &mut Living| *s = Living::new(look);
    let [hover, always, rule, failure] = Look::ALL;
    rune::Story::new(
        "Prototypes/Capture capsule/living pill",
        || Living::new(Look::ALL[0]),
        |o: &Living, cx: &mut rune::Cx<Living>| {
            let p = Paint::new(cx.system_dark(), cx.env().scale);
            stage(p, o, cx.is_reduced_motion(), (SHOT.0, 424.0))
        },
    )
    .poll(|o: &mut Living, now: f64, _: &mut rune::Tx| {
        o.advance_wall(now);
        true
    })
    .variant("Hover", pick(hover))
    .variant("Always", pick(always))
    .variant("Rule", pick(rule))
    .variant("Failure", pick(failure))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn at(look: Look, t: f64) -> Layout {
        let mut o = Living::new(look);
        o.t = t;
        layout(&o)
    }

    #[test]
    fn every_state_keeps_the_hud_top_edge() {
        let [cx, top] = origin();
        assert!((top - 30.0).abs() < 0.5, "under the menu bar: {top}");
        for look in Look::ALL {
            for t in [1.0, 1.9, 2.5, 3.5, 4.5, 5.0, 6.0] {
                if t >= look.out() {
                    continue;
                }
                let [x, y, w, _] = at(look, t).shell;
                assert_eq!(y, top, "{look:?} at {t}");
                assert!((x + w * 0.5 - cx).abs() < 0.5, "{look:?} at {t} is centred");
            }
        }
    }

    #[test]
    fn hover_opens_the_controls_and_always_keeps_them() {
        let hover = Look::new(Controls::Hover, End::List);
        assert_eq!(at(hover, 0.8).shell[2], hud().width);
        assert_eq!(at(hover, 2.0).shell[2], hud().width + CONTROLS);
        let always = Look::new(Controls::Always, End::List);
        assert_eq!(at(always, 0.8).shell[2], hud().width + CONTROLS);
    }

    #[test]
    fn the_pointer_stops_on_stop_and_picks_the_first_row() {
        let mut o = Living::new(Look::ALL[0]);
        o.t = STOP - 0.05;
        assert!(o.pointer_in(stop_rect()));
        o.t = PICK - 0.05;
        assert!(o.pointer_in(row_rect(0)));
    }
}
