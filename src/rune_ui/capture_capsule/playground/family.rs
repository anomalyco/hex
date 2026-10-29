//! The dictation indicator and the capture capsule as one family: both
//! come from the same place on the screen, in the same shape, on the same
//! springs. A dictation plays, then a capture of the screen (record, stop,
//! choose where it goes, sent with Undo), on a 14-inch MacBook Pro from its
//! notch and on a flat 27-inch display two ways: in the menu bar, and as a
//! capsule floating just under it. Each sits as high as it can.
//!
//! The voice is drawn three ways (the story's variants): Dot scope's braid,
//! from its real simulation and HUD Lab's tuning, bars, and a line. Only
//! the capture adds the screen cue, a red dot or a small screen (each
//! loop shows the other), so "Hex is listening" and "Hex is
//! recording your screen" read apart at a glance. A prototype: the real
//! HUD and capsule don't place or draw this way yet.

use std::cell::Cell;
use std::f32::consts::{PI, TAU};

use rune::{
    Color, Cx, El, LineCap, Path, Play, Shape, Spring, SpringPair, Story, Transition, Tx, canvas,
    col, el, row, text,
};

use super::super::notch;
use super::super::surface::{MEDIUM, SMALL, STRONG, TIMER, corner, measure};
use super::super::tuning;
use super::Paint;
use crate::dot_scope::{self, Frame, Scope, Stage, demo_level, demo_loudness, palette};
use crate::hud_placement::{self, ISLAND_PAD, Rect, Screen};

/// The plate's height at full size, one line: both siblings'.
const H: f32 = 28.0;
/// Room either side of a line's content.
const PAD_X: f32 = 10.0;
const GAP: f32 = 10.0;
/// The voice's width while recording and while working.
const VOICE_W: f32 = 112.0;
const VOICE_WORK_W: f32 = 76.0;
const CUE_W: f32 = 16.0;
const TIMER_W: f32 = 34.0;
/// The send-to card.
const CARD_W: f32 = 264.0;
const ROW_H: f32 = 30.0;
const CARD_PAD: f32 = 6.0;
/// The in-bar plate's room above and below it.
const BAR_INSET: f32 = hud_placement::BAR_INSET as f32;
/// The floating capsule's gap under the menu bar.
const FLOAT_GAP: f32 = hud_placement::TOP_OFFSET as f32;
/// The island's room under the notch.
const ISLAND: f32 = ISLAND_PAD as f32;

const DESTINATIONS: [&str; 3] = ["#design", "Linear · HEX", "Notes"];

/// How much of each screen's top middle the story shows, in points.
const CROP: (f32, f32) = (480.0, 236.0);
/// The black glass above a screen.
const BEZEL: f32 = 8.0;
const COLUMN_GAP: f32 = 24.0;

/// When each step starts, seconds into the loop.
const D_IN: f64 = 0.5;
const D_WORK: f64 = 3.5;
const D_DONE: f64 = 4.6;
const D_OUT: f64 = 5.2;
const C_IN: f64 = 6.2;
const C_WORK: f64 = 9.4;
const C_PICK: f64 = 10.2;
const C_CHOSE: f64 = 11.3;
const C_SENT: f64 = 11.6;
const C_OUT: f64 = 13.2;
const LOOP: f64 = 14.2;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Step {
    Hidden,
    Dictating,
    DictationWorking,
    DictationDone,
    Capturing,
    CaptureWorking,
    Choosing { chosen: bool },
    Sent,
}

fn step_at(t: f64) -> Step {
    match t {
        t if t < D_IN => Step::Hidden,
        t if t < D_WORK => Step::Dictating,
        t if t < D_DONE => Step::DictationWorking,
        t if t < D_OUT => Step::DictationDone,
        t if t < C_IN => Step::Hidden,
        t if t < C_WORK => Step::Capturing,
        t if t < C_PICK => Step::CaptureWorking,
        t if t < C_SENT => Step::Choosing {
            chosen: t >= C_CHOSE,
        },
        t if t < C_OUT => Step::Sent,
        _ => Step::Hidden,
    }
}

/// How the voice is drawn.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(super) enum Voice {
    /// Dot scope's braided traces through a field of dots.
    #[default]
    Braid,
    /// Bars that rise with the voice, newest in the middle.
    Bars,
    /// One trace, its swing the voice's level.
    Line,
}

/// How the capture says it records the screen.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
enum Cue {
    /// A red record dot.
    #[default]
    Dot,
    /// A small screen with a red dot in it.
    Screen,
}

impl Cue {
    fn other(self) -> Self {
        match self {
            Self::Dot => Self::Screen,
            Self::Screen => Self::Dot,
        }
    }
}

/// Where the family sits.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Place {
    /// A black island grown out of the notch.
    Notch,
    /// Inside the menu bar, centred, growing down from it.
    MenuBar,
    /// A capsule just under the menu bar.
    Floating,
}

pub(super) struct Family {
    voice: Voice,
    cue: Cue,
    slow: bool,
    /// Seconds into the loop.
    t: f64,
    last: Option<f64>,
    scope: Scope,
    stage: Option<Stage>,
    reduced: Cell<bool>,
}

impl Family {
    fn new() -> Self {
        Self {
            voice: Voice::Braid,
            cue: Cue::Dot,
            slow: false,
            t: 0.0,
            last: None,
            scope: Scope::new(dot_scope::lab()),
            stage: None,
            reduced: Cell::new(false),
        }
    }

    /// The seconds the current step has run: the timer's.
    fn recorded(&self) -> f64 {
        match step_at(self.t) {
            Step::Capturing => self.t - C_IN,
            _ => C_WORK - C_IN,
        }
    }
}

pub(super) fn story() -> Story<Family> {
    let voice = |voice| move |s: &mut Family| s.voice = voice;
    Story::new("Prototypes/Capture capsule/family", Family::new, view)
        .poll(poll)
        .trigger(
            'r',
            "Play from the start",
            |s: &mut Family, _: &mut Play<'_, Family>| {
                s.t = 0.0;
            },
        )
        .trigger(
            'c',
            "Screen cue: dot or screen",
            |s: &mut Family, _: &mut Play<'_, Family>| {
                s.cue = s.cue.other();
            },
        )
        .trigger(
            's',
            "Slow motion",
            |s: &mut Family, _: &mut Play<'_, Family>| {
                s.slow = !s.slow;
            },
        )
        .variant("Braid", voice(Voice::Braid))
        .variant("Bars", voice(Voice::Bars))
        .variant("Line", voice(Voice::Line))
}

fn poll(s: &mut Family, now: f64, _: &mut Tx) -> bool {
    let dt = s.last.map_or(0.0, |last| (now - last).clamp(0.0, 0.1));
    s.last = Some(now);
    let dt = if s.slow { dt * super::SLOW } else { dt };
    let before = step_at(s.t);
    s.t += dt;
    if s.t >= LOOP {
        // Each loop shows the other screen cue.
        s.t %= LOOP;
        s.cue = s.cue.other();
    }
    let step = step_at(s.t);
    let reduced = s.reduced.get();
    if step != before && matches!(step, Step::Dictating | Step::Capturing) {
        s.scope.tune(dot_scope::lab());
        s.scope.restart(reduced);
    }
    let stage = match step {
        Step::Dictating | Step::Capturing => Some(Stage::Recording),
        Step::DictationWorking | Step::CaptureWorking => Some(Stage::Processing),
        Step::DictationDone => Some(Stage::Done),
        _ => None,
    };
    if stage.is_some() {
        s.stage = stage;
    }
    s.scope.advance(
        dt as f32,
        Frame {
            stage: s.stage.unwrap_or(Stage::Recording),
            visible: stage.is_some(),
            loudness: demo_loudness(s.t as f32),
            reduce_motion: reduced,
            action: false,
        },
    );
    true
}

/// A 14-inch MacBook Pro: 1512 × 982 pt, its notch 185 × 32 pt.
fn macbook() -> Screen {
    Screen {
        frame: Rect {
            x: 0.0,
            y: 0.0,
            width: 1512.0,
            height: 982.0,
        },
        visible: Rect {
            x: 0.0,
            y: 0.0,
            width: 1512.0,
            height: 950.0,
        },
        safe_top: 32.0,
        aux_left: 663.5,
        aux_right: 663.5,
        bar: 24.0,
    }
}

/// A flat 27-inch display: 2560 × 1440 pt, its menu bar 24 pt.
fn display() -> Screen {
    Screen {
        frame: Rect {
            x: 0.0,
            y: 0.0,
            width: 2560.0,
            height: 1440.0,
        },
        visible: Rect {
            x: 0.0,
            y: 0.0,
            width: 2560.0,
            height: 1416.0,
        },
        safe_top: 0.0,
        aux_left: 0.0,
        aux_right: 0.0,
        bar: 24.0,
    }
}

fn view(s: &Family, cx: &mut Cx<Family>) -> El {
    if s.slow {
        cx.time_scale(super::SLOW as f32);
    }
    s.reduced.set(cx.is_reduced_motion());
    let p = Paint::new(cx.system_dark(), cx.env().scale);
    let step = step_at(s.t);
    let who = match step {
        Step::Dictating | Step::DictationWorking | Step::DictationDone => "Dictation",
        Step::Hidden => "",
        _ => "Capture",
    };
    col()
        .key("family")
        .gap(12.0)
        .child(
            text(who)
                .key("who")
                .font(p.k.ty.ui_medium.lh(16.0))
                .color(p.k.c.text_muted)
                .h(16.0),
        )
        .child(
            row()
                .key("screens")
                .gap(COLUMN_GAP)
                .child(column(p, s, Place::Notch, cx))
                .child(column(p, s, Place::MenuBar, cx))
                .child(column(p, s, Place::Floating, cx)),
        )
}

/// One screen's top middle under its bezel, and what it is.
fn column(p: Paint, s: &Family, place: Place, cx: &mut Cx<Family>) -> El {
    let (w, h) = CROP;
    let (screen, name) = match place {
        Place::Notch => (macbook(), "14-inch MacBook Pro · from the notch"),
        Place::MenuBar => (display(), "27-inch display · in the menu bar"),
        Place::Floating => (display(), "27-inch display · under the menu bar"),
    };
    let sw = screen.frame.width as f32;
    col()
        .key(name)
        .gap(10.0)
        .child(
            el().key("top")
                .size(w, BEZEL + h)
                .fill(p.k.c.black)
                .clip_rounded(0.0)
                .child(desktop(p, s, place, &screen, cx).absolute((w - sw) * 0.5, BEZEL)),
        )
        .child(
            text(name)
                .key("name")
                .font(p.k.ty.ui_medium.lh(16.0))
                .color(p.k.c.text_muted),
        )
}

/// The whole screen: its wash, menu bar, notch, and the family.
fn desktop(p: Paint, s: &Family, place: Place, screen: &Screen, cx: &mut Cx<Family>) -> El {
    let k = p.k;
    let (w, h) = (screen.frame.width as f32, screen.frame.height as f32);
    let bar = screen.bar_height() as f32;
    let wash = |name, step| rune_theme_opencode::scale(name, k.dark, step);
    let mut desktop = el()
        .key("desktop")
        .size(w, h)
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
    desktop.child(family(p, s, place, screen, cx))
}

/// What one piece of content is.
#[derive(Clone, Copy, Debug, PartialEq)]
enum Piece {
    Cue,
    Voice { working: bool },
    Timer,
    SendTo,
    Row(usize),
    Sent,
    Undo,
}

impl Piece {
    fn key(self) -> String {
        match self {
            Self::Cue => "cue".into(),
            Self::Voice { .. } => "voice".into(),
            Self::Timer => "timer".into(),
            Self::SendTo => "send-to".into(),
            Self::Row(i) => format!("row-{i}"),
            Self::Sent => "sent".into(),
            Self::Undo => "undo".into(),
        }
    }
}

/// The shape and its content for one step at one place, in screen points.
struct Layout {
    /// The shape: x, y, w, h.
    shell: [f32; 4],
    shown: bool,
    /// How much smaller the line draws, to fit the menu bar.
    fit: f32,
    pieces: Vec<([f32; 4], Piece)>,
    /// The chosen row, lit.
    lit: Option<usize>,
}

fn layout(step: Step, place: Place, screen: &Screen) -> Layout {
    let w = screen.frame.width as f32;
    let cx = w * 0.5;
    let bar = screen.bar_height() as f32;
    let notch_h = screen.notch().map_or(0.0, |n| n.height as f32);
    let fit = match place {
        Place::MenuBar => (bar - 2.0 * BAR_INSET) / H,
        _ => 1.0,
    };
    let line_top = match place {
        Place::Notch => notch_h + ISLAND - 1.0,
        Place::MenuBar => BAR_INSET,
        Place::Floating => bar + FLOAT_GAP,
    };
    let line_h = H * fit;
    // The line's pieces, left to right, each (width, piece), at full size.
    let line: Vec<(f32, Piece)> = match step {
        Step::Hidden => Vec::new(),
        Step::Dictating => vec![(VOICE_W, Piece::Voice { working: false })],
        Step::DictationWorking | Step::DictationDone | Step::CaptureWorking => {
            vec![(VOICE_WORK_W, Piece::Voice { working: true })]
        }
        Step::Capturing => vec![
            (CUE_W, Piece::Cue),
            (VOICE_W, Piece::Voice { working: false }),
            (TIMER_W, Piece::Timer),
        ],
        Step::Choosing { .. } => Vec::new(),
        Step::Sent => {
            // Text keeps its size in the bar's smaller type, so it's
            // measured there and laid out as if full size.
            let (strong, body) = if fit < 1.0 {
                (SMALL, SMALL)
            } else {
                (STRONG, MEDIUM)
            };
            let sent = measure(&format!("Sent to {}", DESTINATIONS[0]), strong) / fit;
            let undo = measure("Undo", body) / fit;
            vec![(sent.ceil(), Piece::Sent), (undo.ceil(), Piece::Undo)]
        }
    };
    let mut pieces = Vec::new();
    let (content_w, rows) = match step {
        Step::Choosing { .. } => (CARD_W - 2.0 * PAD_X, DESTINATIONS.len()),
        _ => {
            let gaps = GAP * line.len().saturating_sub(1) as f32;
            (line.iter().map(|(w, _)| w).sum::<f32>() + gaps, 0)
        }
    };
    let mut x = cx - content_w * fit * 0.5;
    for (pw, piece) in &line {
        pieces.push(([x, line_top, pw * fit, line_h], *piece));
        x += (pw + GAP) * fit;
    }
    let mut height = line_h;
    if let Step::Choosing { .. } = step {
        // The card's top line keeps the line's place and size; its rows
        // grow down from it at full size.
        let left = cx - content_w * 0.5;
        pieces.push(([left, line_top, content_w, line_h], Piece::SendTo));
        for i in 0..rows {
            let y = line_top + line_h + i as f32 * ROW_H;
            pieces.push(([left - 4.0, y, content_w + 8.0, ROW_H], Piece::Row(i)));
        }
        height = line_h + rows as f32 * ROW_H + CARD_PAD;
    }
    let shown = step != Step::Hidden;
    let width = match step {
        Step::Choosing { .. } => CARD_W,
        _ => content_w * fit + 2.0 * PAD_X * fit,
    };
    let (sx, sy, sw, sh) = match place {
        Place::Notch => {
            let n = screen.notch().expect("a notched screen");
            if shown {
                let sw = width.max(notch::MIN_WIDTH);
                (cx - sw * 0.5, 0.0, sw, line_top + height + ISLAND - 1.0)
            } else {
                // Tucked into the notch.
                let nw = n.width as f32;
                (cx - nw * 0.5, 0.0, nw, notch_h)
            }
        }
        Place::MenuBar | Place::Floating => {
            if shown {
                (cx - width * 0.5, line_top, width, height)
            } else {
                // Grows from a dot in its own middle.
                (cx - line_h * 0.5, line_top, line_h, line_h)
            }
        }
    };
    let lit = match step {
        Step::Choosing { chosen: true } => Some(0),
        _ => None,
    };
    Layout {
        shell: [sx, sy, sw, sh],
        shown,
        fit,
        pieces,
        lit,
    }
}

/// The siblings' one shape at `place`, and what's in it.
fn family(p: Paint, s: &Family, place: Place, screen: &Screen, cx: &mut Cx<Family>) -> El {
    let reduced = cx.is_reduced_motion();
    let at_notch = place == Place::Notch;
    // The notch is black in either appearance, so what's on it is dark's.
    let p = if at_notch {
        Paint::new(true, 1.0 / p.hair)
    } else {
        p
    };
    let k = p.k;
    let t = tuning::get();
    let island = t.island;
    let step = step_at(s.t);
    let l = layout(step, place, screen);
    let [x, y, w, h] = l.shell;
    let radius = corner(h);
    let w_screen = screen.frame.width as f32;
    let mut root = el()
        .key(format!("{place:?}"))
        .absolute(0.0, 0.0)
        .size(w_screen, 400.0);
    let mut shell = el().key("shell").absolute(x, y).size(w, h).motion(island);
    if at_notch {
        shell = shell.corners([0.0, 0.0, radius, radius]).fill(k.c.black);
        root = root
            .child(shell)
            .children(notch::ears("shell-ears", k.c.black, x, w, 0.0, island));
    } else {
        shell = shell.radius(radius).fill(k.c.bg_base);
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
                    .motion(island),
            );
        }
        let fade = Spring::new(island.duration, 0.0);
        root = root.child(
            shell
                .opacity(if l.shown { 1.0 } else { 0.0 })
                .opacity_motion(fade),
        );
    }
    // The content is masked by the shape as it moves: a box on the shape's
    // rect and spring clips, and the box inside moves back on the same
    // spring, so the content stays put while the walls pass over it.
    let corners = if at_notch {
        [0.0, 0.0, radius, radius]
    } else {
        [radius; 4]
    };
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
        content = content.child(
            draw(p, s, piece, rect, l.fit, l.lit, at_notch)
                .key(piece.key())
                .absolute(rect[0], rect[1])
                .size(rect[2], rect[3])
                .motion(island)
                .transition(transition),
        );
    }
    let inner = el()
        .key("inner")
        .absolute(-x, -y)
        .size(w_screen, 400.0)
        .motion(island)
        .child(content);
    root.child(
        el().key("clip")
            .absolute(x, y)
            .size(w, h)
            .motion(island)
            .clip_corners(corners)
            .child(inner),
    )
}

/// One piece of content, filling its rect.
fn draw(
    p: Paint,
    s: &Family,
    piece: Piece,
    rect: [f32; 4],
    fit: f32,
    lit: Option<usize>,
    dark: bool,
) -> El {
    let k = p.k;
    let [_, _, w, h] = rect;
    let small = fit < 1.0;
    let body = if small { SMALL } else { MEDIUM };
    match piece {
        Piece::Voice { working } => voice(p, s, w, h, fit, working, dark || k.dark),
        Piece::Cue => cue(p, s, h, fit),
        Piece::Timer => {
            let secs = s.recorded() as u32;
            row().center_y().justify_end().child(
                text(format!("{}:{:02}", secs / 60, secs % 60))
                    .font(if small {
                        SMALL.features("'tnum' 1")
                    } else {
                        TIMER
                    })
                    .color(p.text()),
            )
        }
        Piece::SendTo => row()
            .center_y()
            .child(text("Send to").font(body).color(p.muted())),
        Piece::Row(i) => {
            let on = lit == Some(i);
            let key = el()
                .size(16.0, 16.0)
                .radius(4.0)
                .fill(k.alpha(k.c.text_base, if on { 0.2 } else { 0.1 }))
                .child(
                    row()
                        .center()
                        .size(16.0, 16.0)
                        .child(text(format!("{}", i + 1)).font(SMALL).color(p.text())),
                );
            row()
                .center_y()
                .gap(10.0)
                .px(4.0)
                .radius(8.0)
                .fill(if on { k.c.layer_02 } else { Color::TRANSPARENT })
                .child(key)
                .child(text(DESTINATIONS[i]).font(MEDIUM).color(p.text()))
        }
        Piece::Sent => row().center_y().child(
            text(format!("Sent to {}", DESTINATIONS[0]))
                .font(if small { SMALL } else { STRONG })
                .color(p.text()),
        ),
        Piece::Undo => row()
            .center_y()
            .justify_end()
            .child(text("Undo").font(body).color(p.muted())),
    }
}

/// The capture's screen cue, centred in an `h` tall box.
fn cue(p: Paint, s: &Family, h: f32, fit: f32) -> El {
    let red = p.danger();
    let breath = if s.reduced.get() {
        1.0
    } else {
        0.8 + 0.2 * ((s.t - C_IN) as f32 * TAU / 1.6).cos()
    };
    let w = CUE_W * fit;
    let mut b = el().size(w, h);
    match s.cue {
        Cue::Dot => {
            let d = 8.0 * fit;
            b = b.child(
                el().absolute((w - d) * 0.5, (h - d) * 0.5)
                    .size(d, d)
                    .radius(d * 0.5)
                    .fill(red)
                    .opacity(breath),
            );
        }
        Cue::Screen => {
            let (sw, sh) = (16.0 * fit, 11.5 * fit);
            let d = 4.5 * fit;
            b = b.child(
                el().absolute((w - sw) * 0.5, (h - sh) * 0.5 - fit)
                    .size(sw, sh)
                    .radius(2.5 * fit)
                    .border(1.25 * fit, p.text())
                    .child(
                        el().absolute((sw - d) * 0.5, (sh - d) * 0.5)
                            .size(d, d)
                            .radius(d * 0.5)
                            .fill(red)
                            .opacity(breath),
                    ),
            );
            // The stand.
            b = b.child(
                el().absolute((w - 6.0 * fit) * 0.5, (h + sh) * 0.5 + 0.5 * fit)
                    .size(6.0 * fit, 1.25 * fit)
                    .radius(0.6 * fit)
                    .fill(p.text()),
            );
        }
    }
    b
}

/// The voice in a `w` × `h` box, as the story's variant draws it.
fn voice(p: Paint, s: &Family, w: f32, h: f32, fit: f32, working: bool, dark: bool) -> El {
    match s.voice {
        Voice::Braid => braid(s, w, h, fit, dark),
        Voice::Bars => bars(p, s, w, h, fit, working),
        Voice::Line => line(p, s, w, h, fit, working),
    }
}

fn smoothstep(a: f32, b: f32, x: f32) -> f32 {
    let t = ((x - a) / (b - a)).clamp(0.0, 1.0);
    t * t * (3.0 - 2.0 * t)
}

/// Dot scope: its simulation's dots, lit as its shader lights them.
fn braid(s: &Family, w: f32, h: f32, fit: f32, dark: bool) -> El {
    let scope = &s.scope;
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
    let mut field = el().size(w, h);
    for (n, &[power, fresh]) in scope.dots().iter().enumerate() {
        let (i, j) = (n % columns, n / columns);
        let x = (i as f32 - (columns as f32 - 1.0) * 0.5) * pitch;
        let y = (j as f32 - (rows as f32 - 1.0) * 0.5) * pitch;
        let cap_dx = (x.abs() - (w * 0.5 - radius)).max(0.0);
        let cap = radius - (cap_dx * cap_dx + y * y).sqrt();
        let mask = smoothstep(1.4 * fit, 4.6 * fit, cap);
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
        field = field.child(
            el().absolute(w * 0.5 + x - r, h * 0.5 + y - r)
                .size(2.0 * r, 2.0 * r)
                .radius(r)
                .fill(color.alpha(a)),
        );
    }
    field
}

/// The voice's level `ago` seconds back, 0–1.
fn level(s: &Family, ago: f64) -> f32 {
    demo_level((s.t - ago).max(0.0) as f32)
}

/// Bars, the newest level in the middle and older ones travelling out.
fn bars(p: Paint, s: &Family, w: f32, h: f32, fit: f32, working: bool) -> El {
    const BARS: usize = 13;
    let (bw, gap) = (3.0 * fit, 3.0 * fit);
    let total = BARS as f32 * bw + (BARS - 1) as f32 * gap;
    let ink = p.text();
    let mut b = el().size(w, h);
    let left = (w - total) * 0.5;
    for i in 0..BARS {
        let from_middle = (i as f32 - (BARS as f32 - 1.0) * 0.5).abs();
        let x = left + i as f32 * (bw + gap);
        if x < bw || x + bw > w - bw {
            continue;
        }
        let (fraction, alpha) = if working {
            // A light travelling across bars at rest.
            let head = ((s.t * 0.9).fract() as f32) * (BARS as f32 + 4.0) - 2.0;
            let lit = (1.0 - ((i as f32 - head).abs() / 2.0)).max(0.0);
            (0.18 + 0.12 * lit, 0.35 + 0.65 * lit)
        } else {
            let l = level(s, from_middle as f64 * 0.07);
            (
                0.14 + 0.86 * l * (1.0 - 0.05 * from_middle),
                1.0 - 0.05 * from_middle,
            )
        };
        let bh = (h * 0.8 * fraction).max(bw);
        b = b.child(
            el().absolute(x, (h - bh) * 0.5)
                .size(bw, bh)
                .radius(bw * 0.5)
                .fill(ink.alpha(alpha)),
        );
    }
    b
}

/// One trace across the box, its swing the voice's level, softened toward
/// both ends.
fn line(p: Paint, s: &Family, w: f32, h: f32, fit: f32, working: bool) -> El {
    const POINTS: usize = 64;
    let inset = 4.0 * fit;
    let span = w - 2.0 * inset;
    let t = s.t as f32;
    let swing = if working {
        0.12
    } else {
        0.15 + 0.85 * level(s, 0.0)
    };
    let mut path = Path::new();
    for n in 0..POINTS {
        let u = n as f32 / (POINTS - 1) as f32;
        let window = (PI * u).sin().powf(1.5);
        let wave = if working {
            (TAU * (2.5 * u - 1.2 * t)).sin()
        } else {
            0.65 * (TAU * (3.2 * u - 1.6 * t)).sin() + 0.35 * (TAU * (5.1 * u + 1.1 * t)).sin()
        };
        let point = [inset + span * u, h * 0.5 + wave * window * swing * h * 0.42];
        path = if n == 0 {
            path.move_to(point)
        } else {
            path.line_to(point)
        };
    }
    canvas(
        Shape::new()
            .view_box(w, h)
            .stroke_path(&path, 1.6 * fit, LineCap::Round, p.text()),
    )
    .size(w, h)
}
