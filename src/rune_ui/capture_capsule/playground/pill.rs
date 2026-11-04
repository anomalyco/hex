//! Three looks for the Annotate capsule that share the dictation HUD's
//! design language (t1290, t1291): Dot scope's field leads the recording
//! instead of a breathing red dot. Each records a synthetic voice through
//! Dot scope's own simulation, then works, then reports where the capture
//! went, placed where `hud_placement` puts the HUD.
//!
//! - A, wide pill: the HUD's pill grown wide, the field at its left end,
//!   the timer and controls after it.
//! - B, living pill: the field is the pill. The timer floats in it, and
//!   after a beat the dots part for the controls.
//! - C, notch island: a black island out of the notch. The record dot and
//!   timer sit either side of the camera, the field hangs below.
//!
//! A prototype in the capsule playground only: the shipping capsule
//! doesn't draw this way.

use std::f32::consts::{PI, TAU};

use rune::{
    Color, El, LineCap, Path, Shape, Spring, SpringPair, Transition, canvas, el, row, text,
};

use super::super::notch;
use super::super::surface::{MEDIUM, STRONG, TIMER, measure};
use super::super::tuning;
use super::Paint;
use crate::rune_hud::dot_scope::{Frame, Placement, Scope, Stage, Tuning, demo_loudness, palette};
use crate::rune_hud::hud_placement::{self, Content, ISLAND_PAD, Rect, Screen};

/// When each step starts, seconds into the loop.
const IN: f64 = 0.25;
/// B's controls part the dots this long into a recording.
const BEAT: f64 = IN + 1.0;
const STOP: f64 = 2.65;
const DONE: f64 = 3.45;
const OUT: f64 = 5.6;
const LOOP: f64 = 6.4;

/// The HUD's own spring: Dot scope's transition and bounce.
const SHELL: Spring = Spring::new(0.3, 0.15);

const DESTINATION: &str = "Sent to OpenCode";
const UNDO: &str = "Undo";
const WORKING: &str = "Processing…";

/// How much of each screen's top middle the stage shows.
const SHOT: (f32, f32) = (640.0, 200.0);
const BEZEL: f32 = 8.0;

/// One of the looks.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Pick {
    A,
    B,
    C,
}

impl Pick {
    pub(super) const ALL: [Self; 3] = [Self::A, Self::B, Self::C];

    pub(super) fn label(self) -> &'static str {
        match self {
            Self::A => "A · Wide pill",
            Self::B => "B · Living pill",
            Self::C => "C · Notch island",
        }
    }

    fn idea(self) -> &'static str {
        match self {
            Self::A => "The HUD's pill grown wide: Dot scope leads, then the timer and controls.",
            Self::B => {
                "The field is the pill: the timer floats in it, and the dots part for the controls after a beat."
            }
            Self::C => {
                "Out of the notch: the record dot and timer flank the camera, the field hangs below."
            }
        }
    }

    /// Dot scope sized for this look's field.
    fn tuning(self) -> Tuning {
        let hud = Tuning::default();
        match self {
            Self::A => Tuning {
                columns: 36.0,
                rows: 8.0,
                width: 92.0,
                processing_width: 64.0,
                height: 22.0,
                ..hud
            },
            Self::B => Tuning {
                columns: 83.0,
                rows: 12.0,
                width: 216.0,
                processing_width: 120.0,
                height: 34.0,
                ..hud
            },
            Self::C => hud,
        }
    }

    fn screen(self) -> Screen {
        match self {
            Self::A | Self::B => display(),
            Self::C => macbook(),
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Phase {
    Hidden,
    Recording,
    Working,
    Done,
}

fn phase_at(t: f64) -> Phase {
    match t {
        t if t < IN => Phase::Hidden,
        t if t < STOP => Phase::Recording,
        t if t < DONE => Phase::Working,
        t if t < OUT => Phase::Done,
        _ => Phase::Hidden,
    }
}

/// The looks' state: which one, its clock, and its Dot scope.
pub(super) struct Options {
    pub(super) pick: Pick,
    /// Seconds into the loop.
    t: f64,
    /// The last wall time the Labs story stepped by.
    wall: Option<f64>,
    scope: Scope,
}

impl Options {
    pub(super) fn new(pick: Pick) -> Self {
        Self {
            pick,
            t: 0.0,
            wall: None,
            scope: Scope::new(pick.tuning()),
        }
    }

    /// Steps the loop and the voice `dt` seconds.
    pub(super) fn advance(&mut self, dt: f64, reduced: bool) {
        let before = phase_at(self.t);
        self.t += dt.clamp(0.0, 0.1);
        if self.t >= LOOP {
            self.t %= LOOP;
        }
        let phase = phase_at(self.t);
        if phase != before && phase == Phase::Recording {
            self.scope.tune(self.pick.tuning());
            self.scope.restart(reduced);
        }
        let stage = match phase {
            Phase::Working => Stage::Processing,
            Phase::Done => Stage::Done,
            Phase::Recording | Phase::Hidden => Stage::Recording,
        };
        self.scope.advance(
            dt as f32,
            Frame {
                stage,
                visible: phase == Phase::Recording || phase == Phase::Working,
                loudness: demo_loudness((self.t - IN).max(0.0) as f32),
                reduce_motion: reduced,
            },
        );
    }

    fn recorded(&self) -> u32 {
        match phase_at(self.t) {
            Phase::Recording => (self.t - IN) as u32,
            _ => (STOP - IN) as u32,
        }
    }

    /// How far B's controls have come in, 0–1.
    fn beat(&self, reduced: bool) -> f32 {
        if phase_at(self.t) != Phase::Recording {
            return 1.0;
        }
        if reduced {
            return if self.t >= BEAT { 1.0 } else { 0.0 };
        }
        settle((self.t - BEAT) as f32, SHELL.duration)
    }
}

/// A critically damped approach to 1 over about `duration`, from `dt`
/// seconds in.
fn settle(dt: f32, duration: f32) -> f32 {
    if dt <= 0.0 {
        return 0.0;
    }
    let w = TAU / duration;
    1.0 - (1.0 + w * dt) * (-w * dt).exp()
}

/// A 14-inch MacBook Pro: 1512 × 982 pt, its notch 185 × 32 pt.
fn macbook() -> Screen {
    Screen {
        frame: rect(1512.0, 982.0),
        visible: rect(1512.0, 950.0),
        safe_top: 32.0,
        aux_left: 663.5,
        aux_right: 663.5,
        bar: 24.0,
    }
}

/// A flat 27-inch display: 2560 × 1440 pt, its menu bar 24 pt.
fn display() -> Screen {
    Screen {
        frame: rect(2560.0, 1440.0),
        visible: rect(2560.0, 1416.0),
        safe_top: 0.0,
        aux_left: 0.0,
        aux_right: 0.0,
        bar: 24.0,
    }
}

fn rect(width: f64, height: f64) -> Rect {
    Rect {
        x: 0.0,
        y: 0.0,
        width,
        height,
    }
}

/// The plate's middle on `screen` where `hud_placement` puts a `plate`
/// sized HUD, in screen points from the top-left.
fn plate_middle(placement: Placement, screen: &Screen, plate: (f32, f32), spread: f32) -> [f32; 2] {
    let window = (f64::from(plate.0) + 80.0, f64::from(plate.1) + 80.0);
    let placed = hud_placement::place(
        placement,
        screen,
        Content {
            window,
            plate: (f64::from(plate.0), f64::from(plate.1)),
            island_spread: f64::from(spread),
        },
        false,
    );
    let top = screen.frame.y + screen.frame.height - placed.top_left.1;
    let left = placed.top_left.0 - screen.frame.x;
    [
        (left + window.0 / 2.0) as f32 + placed.layout.center[0],
        (top + window.1 / 2.0) as f32 + placed.layout.center[1],
    ]
}

/// What one piece of content is.
#[derive(Clone, Copy, Debug, PartialEq)]
enum Piece {
    Field,
    /// The record dot beside the camera.
    Rec,
    Timer,
    Stop,
    Discard,
    Working,
    Check,
    Sent,
    Undo,
}

impl Piece {
    fn key(self) -> &'static str {
        match self {
            Self::Field => "field",
            Self::Rec => "rec",
            Self::Timer => "timer",
            Self::Stop => "stop",
            Self::Discard => "discard",
            Self::Working => "working",
            Self::Check => "check",
            Self::Sent => "sent",
            Self::Undo => "undo",
        }
    }
}

/// The shape and its content at one moment, in screen points.
struct Layout {
    shell: [f32; 4],
    shown: bool,
    /// A black island out of the notch, not a floating pill.
    island: bool,
    pieces: Vec<([f32; 4], Piece)>,
    /// Pieces placed by the clock each frame rather than by a spring.
    driven: &'static [Piece],
}

/// "Sent to OpenCode", Undo and the check, a row centred on `cx`.
fn done_row(cx: f32, cy: f32) -> (f32, Vec<([f32; 4], Piece)>) {
    let sent = measure(DESTINATION, STRONG).ceil();
    let undo = measure(UNDO, MEDIUM).ceil();
    let w = 16.0 + 8.0 + sent + 14.0 + undo;
    let x = cx - w * 0.5;
    let pieces = vec![
        ([x, cy - 8.0, 16.0, 16.0], Piece::Check),
        ([x + 24.0, cy - 15.0, sent, 30.0], Piece::Sent),
        ([x + 24.0 + sent + 14.0, cy - 15.0, undo, 30.0], Piece::Undo),
    ];
    (w, pieces)
}

fn layout(o: &Options, reduced: bool) -> Layout {
    let phase = phase_at(o.t);
    let tuning = o.pick.tuning();
    let screen = o.pick.screen();
    let timer_w = measure("0:00", TIMER).ceil() + 2.0;
    match o.pick {
        Pick::A => {
            const H: f32 = 30.0;
            const INSET: f32 = (H - 22.0) * 0.5;
            let [cx, cy] = plate_middle(Placement::Below, &screen, (192.0, H), 0.0);
            let field_w = tuning.width;
            let field = |middle: f32| {
                (
                    [
                        middle - field_w * 0.5,
                        cy - tuning.height * 0.5,
                        field_w,
                        tuning.height,
                    ],
                    Piece::Field,
                )
            };
            let (w, pieces) = match phase {
                Phase::Hidden => (H, Vec::new()),
                Phase::Recording => {
                    let w = INSET + field_w + 10.0 + timer_w + 6.0 + 22.0 + 2.0 + 22.0 + INSET;
                    let x = cx - w * 0.5;
                    let after = x + INSET + field_w + 10.0;
                    let stop = after + timer_w + 6.0;
                    (
                        w,
                        vec![
                            field(x + INSET + field_w * 0.5),
                            ([after, cy - H * 0.5, timer_w, H], Piece::Timer),
                            ([stop, cy - 11.0, 22.0, 22.0], Piece::Stop),
                            ([stop + 24.0, cy - 11.0, 22.0, 22.0], Piece::Discard),
                        ],
                    )
                }
                Phase::Working => {
                    let label = measure(WORKING, MEDIUM).ceil();
                    let narrow = tuning.processing_width;
                    let w = INSET + narrow + 10.0 + label + 12.0;
                    let x = cx - w * 0.5;
                    (
                        w,
                        vec![
                            field(x + INSET + narrow * 0.5),
                            (
                                [x + INSET + narrow + 10.0, cy - H * 0.5, label, H],
                                Piece::Working,
                            ),
                        ],
                    )
                }
                Phase::Done => {
                    let (w, pieces) = done_row(cx, cy);
                    (w + 24.0, pieces)
                }
            };
            Layout {
                shell: [cx - w * 0.5, cy - H * 0.5, w, H],
                shown: phase != Phase::Hidden,
                island: false,
                pieces,
                driven: &[],
            }
        }
        Pick::B => {
            let h = tuning.height;
            let [cx, cy] = plate_middle(Placement::Below, &screen, (tuning.width, h), 0.0);
            let field = (
                [cx - tuning.width * 0.5, cy - h * 0.5, tuning.width, h],
                Piece::Field,
            );
            let (w, pieces) = match phase {
                Phase::Hidden => (h, Vec::new()),
                Phase::Recording => {
                    let w = tuning.width;
                    let right = cx + w * 0.5;
                    let beat = o.beat(reduced);
                    let timer_x = cx - 26.0 * beat - timer_w * 0.5;
                    let button = h - 8.0;
                    let mut pieces =
                        vec![field, ([timer_x, cy - h * 0.5, timer_w, h], Piece::Timer)];
                    if beat > 0.0 {
                        let discard = right - 4.0 - button;
                        pieces.push((
                            [discard - 2.0 - button, cy - button * 0.5, button, button],
                            Piece::Stop,
                        ));
                        pieces.push(([discard, cy - button * 0.5, button, button], Piece::Discard));
                    }
                    (w, pieces)
                }
                Phase::Working => (tuning.processing_width, vec![field]),
                Phase::Done => {
                    let (w, pieces) = done_row(cx, cy);
                    (w + 28.0, pieces)
                }
            };
            Layout {
                shell: [cx - w * 0.5, cy - h * 0.5, w, h],
                shown: phase != Phase::Hidden,
                island: false,
                pieces,
                driven: &[Piece::Timer, Piece::Stop, Piece::Discard],
            }
        }
        Pick::C => {
            let n = screen.notch().expect("a notched screen");
            let (nw, nh) = (n.width as f32, n.height as f32);
            let pad = ISLAND_PAD as f32;
            const OPEN: f32 = 270.0;
            let spread = (OPEN - nw) * 0.5;
            let [cx, cy] = plate_middle(
                Placement::Notch,
                &screen,
                (tuning.width, tuning.height),
                spread,
            );
            let island_h = nh + pad + tuning.height + pad;
            let field = (
                [
                    cx - tuning.width * 0.5,
                    cy - tuning.height * 0.5,
                    tuning.width,
                    tuning.height,
                ],
                Piece::Field,
            );
            let ear = (OPEN - nw) * 0.5;
            let (w, pieces) = match phase {
                Phase::Hidden => {
                    return Layout {
                        shell: [cx - nw * 0.5, 0.0, nw, nh],
                        shown: false,
                        island: true,
                        pieces: Vec::new(),
                        driven: &[],
                    };
                }
                Phase::Recording => {
                    let left = cx - OPEN * 0.5;
                    let right = cx + OPEN * 0.5;
                    (
                        OPEN,
                        vec![
                            field,
                            (
                                [left + ear * 0.5 - 4.0, nh * 0.5 - 4.0, 8.0, 8.0],
                                Piece::Rec,
                            ),
                            (
                                [right - ear * 0.5 - timer_w * 0.5, 0.0, timer_w, nh],
                                Piece::Timer,
                            ),
                            ([left + 12.0, cy - 11.0, 22.0, 22.0], Piece::Discard),
                            ([right - 12.0 - 22.0, cy - 11.0, 22.0, 22.0], Piece::Stop),
                        ],
                    )
                }
                Phase::Working => (
                    (tuning.processing_width + 2.0 * pad).max(notch::MIN_WIDTH),
                    vec![field],
                ),
                Phase::Done => {
                    let (w, pieces) = done_row(cx, cy);
                    ((w + 32.0).max(OPEN), pieces)
                }
            };
            Layout {
                shell: [cx - w * 0.5, 0.0, w, island_h],
                shown: true,
                island: true,
                pieces,
                driven: &[],
            }
        }
    }
}

/// The playground's stage for `o`: the top middle of its screen under a
/// bezel, and what the look is.
pub(super) fn stage(p: Paint, o: &Options, reduced: bool, size: (f32, f32)) -> El {
    let screen = o.pick.screen();
    let (w, h) = SHOT;
    let sw = screen.frame.width as f32;
    let caption = rune::col()
        .key("caption")
        .absolute(16.0, BEZEL + h + 16.0)
        .w(w - 32.0)
        .gap(4.0)
        .child(
            text(o.pick.label())
                .key("label")
                .font(p.k.ty.ui_medium.lh(16.0))
                .color(p.text()),
        )
        .child(
            text(o.pick.idea())
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
                .child(desktop(p, o, reduced, &screen).absolute((w - sw) * 0.5, BEZEL)),
        )
        .child(caption)
}

/// The whole screen: its wash, menu bar, notch, and the capsule.
fn desktop(p: Paint, o: &Options, reduced: bool, screen: &Screen) -> El {
    let k = p.k;
    let (w, h) = (screen.frame.width as f32, screen.frame.height as f32);
    let bar = screen.bar_height() as f32;
    let wash = |name, step| rune_theme_opencode::scale(name, k.dark, step);
    let mut desktop = el()
        .key("desktop")
        .size(w, h.min(400.0))
        .fill(wash("cobalt", 3))
        .linear_gradient(
            PI * 0.75,
            [(0.0, wash("cobalt", 3)), (1.0, wash("lilac", 5))],
        )
        .child(super::stories::menu_bar(p, w, bar));
    if let Some(n) = screen.notch() {
        desktop = desktop.child(notch::notch(
            "notch",
            k.c.black,
            w * 0.5,
            0.0,
            n.height as f32,
        ));
    }
    desktop.child(capsule(p, o, reduced, w))
}

/// The capsule's shape and what's in it, in a `w` wide screen.
fn capsule(p: Paint, o: &Options, reduced: bool, w_screen: f32) -> El {
    let l = layout(o, reduced);
    // The notch is black in either appearance, so what's on it is dark's.
    let p = if l.island {
        Paint::new(true, 1.0 / p.hair)
    } else {
        p
    };
    let k = p.k;
    let t = tuning::get();
    let [x, y, w, h] = l.shell;
    let mut root = el()
        .key(format!("{:?}", o.pick))
        .absolute(0.0, 0.0)
        .size(w_screen, 400.0);
    let corners = if l.island {
        let r = (h - notch_height(o)) * 0.5;
        let r = if l.shown { r.max(8.0) } else { notch::RADIUS };
        root = root
            .child(
                el().key("shell")
                    .absolute(x, y)
                    .size(w, h)
                    .motion(SHELL)
                    .corners([0.0, 0.0, r, r])
                    .fill(k.c.black),
            )
            .children(notch::ears("shell-ears", k.c.black, x, w, 0.0, SHELL));
        [0.0, 0.0, r, r]
    } else {
        let radius = h * 0.5;
        let mut shell = el()
            .key("shell")
            .absolute(x, y)
            .size(w, h)
            .motion(SHELL)
            .radius(radius)
            .fill(k.c.bg_base);
        for shadow in &k.elevation.overlay {
            shell = shell.box_shadow(*shadow);
        }
        if k.dark {
            shell = shell.child(
                el().key("rim")
                    .absolute(0.0, 0.0)
                    .size(w, h)
                    .radius(radius)
                    .border_gradient(0.5, k.light(0.12), k.light(0.03))
                    .motion(SHELL),
            );
        }
        let fade = Spring::new(SHELL.duration, 0.0);
        root = root.child(
            shell
                .opacity(if l.shown { 1.0 } else { 0.0 })
                .opacity_motion(fade),
        );
        [radius; 4]
    };
    // The content is masked by the shape as it moves: the walls pass over
    // content that stays put, as the family prototype does it.
    let springs = SpringPair::new(t.content_in, t.content_out);
    let mut content = el()
        .key("content")
        .absolute(0.0, 0.0)
        .size(w_screen, 400.0)
        .dissolve();
    for &(rect, piece) in &l.pieces {
        let transition = if reduced {
            Transition::opacity().springs(springs)
        } else {
            let middle = rune::Vec2::new(
                (x + w * 0.5 - rect[0]) / rect[2].max(1.0),
                (y + h * 0.5 - rect[1]) / rect[3].max(1.0),
            );
            Transition::scale_anchor(t.content_scale, middle)
                .combined(Transition::blur(t.content_blur))
                .springs(springs)
        };
        let mut e = draw(p, o, reduced, piece, rect, &l)
            .key(piece.key())
            .absolute(rect[0], rect[1])
            .size(rect[2], rect[3])
            .transition(transition);
        if !l.driven.contains(&piece) {
            e = e.motion(SHELL);
        }
        content = content.child(e);
    }
    let inner = el()
        .key("inner")
        .absolute(-x, -y)
        .size(w_screen, 400.0)
        .motion(SHELL)
        .child(content);
    root.child(
        el().key("clip")
            .absolute(x, y)
            .size(w, h)
            .motion(SHELL)
            .clip_corners(corners)
            .child(inner),
    )
}

fn notch_height(o: &Options) -> f32 {
    o.pick.screen().notch().map_or(0.0, |n| n.height as f32)
}

/// One piece, filling its rect.
fn draw(p: Paint, o: &Options, reduced: bool, piece: Piece, rect: [f32; 4], l: &Layout) -> El {
    let k = p.k;
    let [px, py, w, h] = rect;
    match piece {
        Piece::Field => {
            let pose = o.scope.pose();
            let holes: Vec<Hole> = l
                .pieces
                .iter()
                .filter(|_| o.pick == Pick::B)
                .filter_map(|&([x, y, pw, ph], other)| {
                    let local = |grow: f32, radius: f32, strength: f32| Hole {
                        rect: [
                            x - px - grow,
                            y - py - grow,
                            pw + 2.0 * grow,
                            ph + 2.0 * grow,
                        ],
                        radius,
                        strength,
                    };
                    match other {
                        Piece::Timer => {
                            let ink = measure("0:00", TIMER) + 14.0;
                            Some(Hole {
                                rect: [
                                    x - px + (pw - ink) * 0.5,
                                    y - py + ph * 0.5 - 10.0,
                                    ink,
                                    20.0,
                                ],
                                radius: 10.0,
                                strength: 1.0,
                            })
                        }
                        Piece::Stop | Piece::Discard => {
                            Some(local(1.0, ph * 0.5 + 1.0, o.beat(reduced)))
                        }
                        _ => None,
                    }
                })
                .collect();
            field(&o.scope, w, h, 1.0, l.island || k.dark, pose.width, &holes)
        }
        Piece::Rec => {
            let breath = if reduced {
                1.0
            } else {
                0.8 + 0.2 * ((o.t - IN) as f32 * TAU / 1.6).cos()
            };
            el().size(w, h)
                .radius(w * 0.5)
                .fill(p.danger())
                .opacity(breath)
        }
        Piece::Timer => {
            let secs = o.recorded();
            row().center().child(
                text(format!("{}:{:02}", secs / 60, secs % 60))
                    .font(TIMER)
                    .color(p.text()),
            )
        }
        Piece::Working => row()
            .center_y()
            .child(text(WORKING).font(MEDIUM).color(p.muted())),
        Piece::Stop => {
            let beat = if o.pick == Pick::B {
                o.beat(reduced)
            } else {
                1.0
            };
            let square = (w * 0.36).round();
            el().size(w, h)
                .radius(w * 0.5)
                .fill(k.alpha(k.c.text_base, 0.1))
                .opacity(beat)
                .child(
                    el().absolute((w - square) * 0.5, (h - square) * 0.5)
                        .size(square, square)
                        .radius(2.0)
                        .fill(p.danger()),
                )
        }
        Piece::Discard => {
            let beat = if o.pick == Pick::B {
                o.beat(reduced)
            } else {
                1.0
            };
            let s = (w * 0.3).round();
            let (a, b) = ((w - s) * 0.5, (w + s) * 0.5);
            let cross = Path::new()
                .move_to([a, a])
                .line_to([b, b])
                .move_to([b, a])
                .line_to([a, b]);
            el().size(w, h).opacity(beat).child(
                canvas(Shape::new().view_box(w, h).stroke_path(
                    &cross,
                    1.5,
                    LineCap::Round,
                    p.muted(),
                ))
                .size(w, h),
            )
        }
        Piece::Check => {
            let tick = Path::new()
                .move_to([3.5, 8.5])
                .line_to([6.75, 11.75])
                .line_to([12.5, 4.75]);
            canvas(Shape::new().view_box(16.0, 16.0).stroke_path(
                &tick,
                1.8,
                LineCap::Round,
                p.success(),
            ))
            .size(16.0, 16.0)
        }
        Piece::Sent => row()
            .center_y()
            .child(text(DESTINATION).font(STRONG).color(p.text())),
        Piece::Undo => row()
            .center_y()
            .child(text(UNDO).font(MEDIUM).color(p.muted())),
    }
}

/// Where the dots part around a piece: a rounded rect in the field's
/// points, how round, and how far they part (0 none, 1 clear).
pub(super) struct Hole {
    rect: [f32; 4],
    radius: f32,
    strength: f32,
}

impl Hole {
    /// How far `[x, y]` (from the field's top-left) is outside the hole.
    fn distance(&self, x: f32, y: f32) -> f32 {
        let [hx, hy, hw, hh] = self.rect;
        let r = self.radius.min(hw * 0.5).min(hh * 0.5);
        let dx = (x - (hx + hw * 0.5)).abs() - (hw * 0.5 - r);
        let dy = (y - (hy + hh * 0.5)).abs() - (hh * 0.5 - r);
        let outside = (dx.max(0.0).powi(2) + dy.max(0.0).powi(2)).sqrt();
        outside + dx.max(dy).min(0.0) - r
    }
}

fn smoothstep(a: f32, b: f32, x: f32) -> f32 {
    let t = ((x - a) / (b - a)).clamp(0.0, 1.0);
    t * t * (3.0 - 2.0 * t)
}

/// Dot scope's field in a `w` × `h` box: its simulation's dots, lit as its
/// shader lights them, inside a pill `mask_w` wide in the box's middle,
/// parting around `holes`.
pub(super) fn field(
    scope: &Scope,
    w: f32,
    h: f32,
    fit: f32,
    dark: bool,
    mask_w: f32,
    holes: &[Hole],
) -> El {
    let tuning = scope.tuning();
    let pose = scope.pose();
    let colors = palette(dark, &pose, tuning.saturation);
    let decode = |rgb: [f32; 3]| {
        let [r, g, b] = rgb.map(rune::color::srgb_decode);
        Color { r, g, b, a: 1.0 }
    };
    let (base, core) = (decode(colors.base), decode(colors.core));
    let (columns, rows) = (scope.columns(), scope.rows());
    let pitch = tuning.spacing * fit;
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
        let x = (i as f32 - (columns as f32 - 1.0) * 0.5) * pitch;
        let y = (j as f32 - (rows as f32 - 1.0) * 0.5) * pitch;
        let cap_dx = (x.abs() - (mask_w * 0.5 - radius)).max(0.0);
        let cap = radius - (cap_dx * cap_dx + y * y).sqrt();
        let mut mask = smoothstep(1.4 * fit, 4.6 * fit, cap);
        for hole in holes {
            let d = hole.distance(w * 0.5 + x, h * 0.5 + y);
            mask *= 1.0 - hole.strength * (1.0 - smoothstep(0.0, 4.0, d));
        }
        if mask <= 0.0 {
            continue;
        }
        let power = power.max(1e-5);
        let row = y.abs() / radius;
        let floor = unlit * (1.0 - 0.7 * smoothstep(0.35, 0.95, row)) * presence * pose.grid;
        let alpha = floor + (full - floor) * power.powf(curve * (1.4 + (1.0 - 1.4) * fresh));
        let shrink = 1.0 + ((0.82 + 0.18 * fresh) - 1.0) * smoothstep(0.03, 0.2, power);
        let r = (0.25 + 0.17 * power.powf(0.6)) * shrink * pitch;
        let lit = (power * power * 1.1).clamp(0.0, 0.9) * (0.3 + 0.7 * fresh);
        let color = base.mix(core, lit);
        let a = alpha * mask;
        if a < 0.01 {
            continue;
        }
        // Each dot is its own: keyed so a skipped one doesn't shift the
        // rest, and drawn where it is now rather than sprung from where
        // it was.
        dots = dots.child(
            el().key(n.to_string())
                .motion(Spring::NONE)
                .absolute(w * 0.5 + x - r, h * 0.5 + y - r)
                .size(2.0 * r, 2.0 * r)
                .radius(r)
                .fill(color.alpha(a)),
        );
    }
    dots
}

/// The looks as a Labs story too, beside the family prototype.
pub(super) fn story() -> rune::Story<Options> {
    let pick = |pick| move |s: &mut Options| *s = Options::new(pick);
    rune::Story::new(
        "Prototypes/Capture capsule/annotate pill",
        || Options::new(Pick::A),
        |o: &Options, cx: &mut rune::Cx<Options>| {
            let p = Paint::new(cx.system_dark(), cx.env().scale);
            stage(p, o, cx.is_reduced_motion(), (SHOT.0, 424.0))
        },
    )
    .poll(|o: &mut Options, now: f64, _: &mut rune::Tx| {
        o.advance_wall(now);
        true
    })
    .variant("A", pick(Pick::A))
    .variant("B", pick(Pick::B))
    .variant("C", pick(Pick::C))
}

impl Options {
    /// Steps by wall time, for the Labs story.
    fn advance_wall(&mut self, now: f64) {
        let dt = self.wall.map_or(0.0, |last| (now - last).max(0.0));
        self.wall = Some(now);
        self.advance(dt, false);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_look_sits_where_the_hud_does() {
        for pick in Pick::ALL {
            let mut o = Options::new(pick);
            o.t = 1.5;
            let l = layout(&o, false);
            let [x, y, w, h] = l.shell;
            let screen = pick.screen();
            let middle = screen.frame.width as f32 * 0.5;
            assert!((x + w * 0.5 - middle).abs() < 0.5, "{pick:?} is centred");
            match pick {
                // Just under the menu bar, as the HUD's Below.
                Pick::A | Pick::B => assert!((y - 30.0).abs() < 0.5, "{pick:?} y {y}"),
                // Out of the notch, the field the island's pad below it.
                Pick::C => {
                    assert_eq!(y, 0.0);
                    assert!((h - (32.0 + 7.0 + 26.0 + 7.0)).abs() < 0.5);
                }
            }
        }
    }

    #[test]
    fn the_dots_part_around_a_hole() {
        let hole = Hole {
            rect: [10.0, 10.0, 20.0, 10.0],
            radius: 5.0,
            strength: 1.0,
        };
        assert!(hole.distance(20.0, 15.0) < 0.0);
        assert!((hole.distance(40.0, 15.0) - 10.0).abs() < 1e-4);
    }
}
