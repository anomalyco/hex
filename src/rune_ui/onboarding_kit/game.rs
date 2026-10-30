//! A throwaway proposal: the practice step as a mini game. Notifications
//! drift in, one is targeted at a time, the player holds the shortcut and
//! answers it, the words fly into it, and it explodes like the site's cards
//! (`explode` in `site/src/shout/main.js`). Three scripted messages end on a
//! certificate the player signs by voice; signing it shows a shareable score
//! card, and Keep playing carries on for as long as they like.
//!
//! Scripted fixture answers only. Nothing here touches the real first run.

use std::cell::Cell;
use std::time::Duration;

use rune::{
    Anim, Color, Cx, El, LineCap, Playground, Shape, Story, TextStyle, Transition, Tx, canvas, col,
    el, row, text,
};
use rune_kit::{Button, ButtonVariant};

use super::super::controls::{Paint, PushSize};
use super::cards::{
    ADAM, App, CARD_W, DAX, FRANK, Notice, Reply, field_text_origin, floating, lines_card_height,
    notice,
};
use super::field_text::{CARD, Field};
use super::fixture::Take;
use super::flight::{Flight, ember_tone, flicker, flying, typeset};
use super::landing::{LEAD, keys, title_font};
use super::mic::mic_level;
use super::sparks::impacts;

const STAGE_W: f32 = 880.0;
const STAGE_H: f32 = 620.0;
const HUD_AT: [f32; 2] = [STAGE_W * 0.5, STAGE_H - 34.0];
const FRAME: Duration = Duration::from_millis(16);

/// How long a card takes to drift in from off screen.
const DRIFT: f64 = 1.7;
/// The site's hit-stop: the card freezes, flashed, before it shatters.
const HITSTOP: f64 = 0.07;
/// After the last word lands, the card holds for a beat, then blows.
const AFTER_LAND: f64 = 0.28;
const SHARE_HOLD: f64 = 2.8;

const NAME: &str = "Kit Langton";
const CERT_H: f32 = 150.0;
const CERT_PAD: f32 = 16.0;
const SIGN: Field = Field {
    font: TextStyle::new(22.0, 520.0),
    line: 30.0,
    width: CARD_W - 2.0 * CERT_PAD,
};
const SIGN_Y: f32 = CERT_H - CERT_PAD - 8.0 - 30.0;

const MILK: Notice = Notice {
    key: "milk",
    app: App::Messages,
    from: "Sam next door",
    place: None,
    body: "hey are you near the shop? could you grab some milk",
    field: "iMessage",
};
const GREG: Notice = Notice {
    key: "greg",
    app: App::Slack,
    from: "Greg",
    place: Some("#general"),
    body: "quick question. actually, can we hop on a call? just 30 mins",
    field: "Reply in thread",
};

const SAY_MILK: Take = Take {
    updates: &[
        (0.25, "Sure,"),
        (0.45, "Sure, oat"),
        (0.62, "Sure, oat or"),
        (0.85, "Sure, oat or normal?"),
        (1.05, "Sure, oat or normal? Back"),
        (1.2, "Sure, oat or normal? Back in"),
        (1.42, "Sure, oat or normal? Back in ten."),
    ],
    release: 1.6,
};
const SAY_GREG: Take = Take {
    updates: &[(0.3, "Nope.")],
    release: 0.65,
};
const SAY_NAME: Take = Take {
    updates: &[(0.35, "Kit"), (0.7, NAME)],
    release: 0.95,
};
const SAY_FRANK: Take = Take {
    updates: &[(0.3, "It"), (0.5, "It is"), (0.75, "It is now.")],
    release: 1.0,
};
const SAY_DAX: Take = Take {
    updates: &[
        (0.3, "On"),
        (0.45, "On it,"),
        (0.75, "On it, five minutes."),
    ],
    release: 1.0,
};
const SAY_ADAM: Take = Take {
    updates: &[
        (0.3, "Figma"),
        (0.55, "Figma deserved"),
        (0.85, "Figma deserved it."),
    ],
    release: 1.05,
};

#[derive(Clone, Copy)]
enum Card {
    Chat(Notice),
    Certificate,
}

#[derive(Clone, Copy)]
struct Spec {
    card: Card,
    take: &'static Take,
    /// How hard the words fly (`Flight` intensity): one shouted word hits hardest.
    heat: f32,
    home: [f32; 2],
    /// Where it drifts in from, relative to its home.
    from: [f32; 2],
}

const SCRIPT: [Spec; 3] = [
    Spec {
        card: Card::Chat(MILK),
        take: &SAY_MILK,
        heat: 0.5,
        home: [44.0, 128.0],
        from: [-520.0, 40.0],
    },
    Spec {
        card: Card::Chat(GREG),
        take: &SAY_GREG,
        heat: 0.92,
        home: [432.0, 218.0],
        from: [520.0, -30.0],
    },
    Spec {
        card: Card::Certificate,
        take: &SAY_NAME,
        heat: 0.62,
        home: [44.0, 318.0],
        from: [-80.0, 420.0],
    },
];

const ENDLESS: [Spec; 3] = [
    Spec {
        card: Card::Chat(FRANK),
        take: &SAY_FRANK,
        heat: 0.55,
        home: [432.0, 140.0],
        from: [500.0, -60.0],
    },
    Spec {
        card: Card::Chat(DAX),
        take: &SAY_DAX,
        heat: 0.6,
        home: [44.0, 260.0],
        from: [-520.0, 0.0],
    },
    Spec {
        card: Card::Chat(ADAM),
        take: &SAY_ADAM,
        heat: 0.75,
        home: [432.0, 330.0],
        from: [480.0, 300.0],
    },
];

struct Round {
    spec: Spec,
    arrive: f64,
    target: f64,
    hold: f64,
    flights: Vec<(Flight, &'static str, [f32; 2])>,
    explode: f64,
    size: [f32; 2],
}

impl Round {
    fn shatter(&self) -> f64 {
        self.explode + HITSTOP
    }

    fn centre(&self) -> [f32; 2] {
        [
            self.spec.home[0] + self.size[0] * 0.5,
            self.spec.home[1] + self.size[1] * 0.5,
        ]
    }

    fn heading(&self) -> [f32; 2] {
        self.flights
            .last()
            .map_or([0.0, -1.0], |(f, _, _)| f.heading())
    }
}

fn field_of(spec: &Spec) -> (Field, [f32; 2], [f32; 2]) {
    let [x, y] = spec.home;
    match spec.card {
        Card::Chat(n) => {
            let lines = CARD.wrap(spec.take.text()).len();
            let h = lines_card_height(&n, lines);
            (CARD, field_text_origin([x, y + h], lines), [CARD_W, h])
        }
        Card::Certificate => (SIGN, [x + CERT_PAD, y + SIGN_Y], [CARD_W, CERT_H]),
    }
}

/// When each word is heard: as its partial arrives on a streaming model,
/// or all at once on release otherwise (the first run's rule).
fn heard_at(take: &Take, streams: bool) -> Vec<f64> {
    let count = take.text().split_whitespace().count();
    (0..count)
        .map(|i| {
            if streams {
                take.updates
                    .iter()
                    .find(|(_, partial)| partial.split_whitespace().count() > i)
                    .map_or(take.release, |(at, _)| *at)
            } else {
                take.release + i as f64 * 0.06
            }
        })
        .collect()
}

fn round(spec: Spec, arrive: f64, target: f64, streams: bool) -> Round {
    let hold = target + 0.55;
    let (field, origin, size) = field_of(&spec);
    let flights: Vec<_> = field
        .slots(spec.take.text())
        .into_iter()
        .zip(heard_at(spec.take, streams))
        .enumerate()
        .map(|(i, (slot, at))| {
            let to = [origin[0] + slot.centre[0], origin[1] + slot.centre[1]];
            let flight = Flight::new(HUD_AT, to, hold + at, spec.heat, field.font.size, i);
            (flight, slot.word, [slot.centre[0], slot.centre[1]])
        })
        .collect();
    let landed = flights
        .iter()
        .map(|(f, _, _)| f.arrives())
        .fold(hold + spec.take.release, f64::max);
    Round {
        spec,
        arrive,
        target,
        hold,
        flights,
        explode: landed + AFTER_LAND,
        size,
    }
}

struct Schedule {
    rounds: Vec<Round>,
    share: f64,
}

fn schedule(streams: bool, keep: bool) -> Schedule {
    let mut rounds: Vec<Round> = Vec::new();
    for spec in SCRIPT {
        let arrive = rounds.last().map_or(0.3, |r| r.hold);
        let target = rounds
            .last()
            .map_or(arrive + 1.0, |r| (arrive + 1.1).max(r.shatter() + 0.45));
        rounds.push(round(spec, arrive, target, streams));
    }
    let share = rounds[2].shatter() + 0.3;
    if keep {
        let mut arrive = share + SHARE_HOLD;
        let mut target = arrive + 1.0;
        for spec in ENDLESS {
            let r = round(spec, arrive, target, streams);
            arrive = r.hold;
            target = (arrive + 1.1).max(r.shatter() + 0.45);
            rounds.push(r);
        }
    }
    Schedule { rounds, share }
}

pub(crate) struct Game {
    now: f64,
    from: Option<f64>,
    started: bool,
    streams: bool,
    keep: bool,
    force_reduced: bool,
    reduced: Cell<bool>,
}

impl Game {
    fn new() -> Self {
        Self {
            now: 0.0,
            from: None,
            started: false,
            streams: true,
            keep: false,
            force_reduced: false,
            reduced: Cell::new(false),
        }
    }

    fn t(&self) -> f64 {
        self.from.map_or(0.0, |from| self.now - from)
    }

    fn reduced(&self) -> bool {
        self.reduced.get()
    }
}

pub(crate) fn add<S: 'static>(playground: Playground<S>) -> Playground<S> {
    playground.story(
        Story::new("Prototypes/Onboarding/Practice game", Game::new, view)
            .poll(poll)
            .trigger('r', "Play", |g, play| {
                g.from = Some(play.now());
                g.now = play.now();
                play.wake_after(Duration::ZERO);
            })
            .variant("Streaming model", |_| {})
            .variant("Model that doesn't stream", |g| g.streams = false)
            .variant("Keep playing", |g| g.keep = true)
            .variant("Reduce Motion", |g| g.force_reduced = true),
    )
}

fn poll(g: &mut Game, now: f64, tx: &mut Tx) -> bool {
    g.now = now;
    // Plays on open, so headless renders and recordings start at once.
    if !g.started {
        g.started = true;
        g.from = Some(now);
    }
    if g.from.is_some() {
        tx.wake_after(FRAME);
    }
    true
}

fn lerp(a: f32, b: f32, t: f32) -> f32 {
    a + (b - a) * t
}

fn smooth(a: f32, b: f32, x: f32) -> f32 {
    let t = ((x - a) / (b - a)).clamp(0.0, 1.0);
    t * t * (3.0 - 2.0 * t)
}

fn rnd(seed: usize, a: f32, b: f32) -> f32 {
    let x = ((seed as f32 * 12.9898 + 78.233).sin() * 43_758.545)
        .fract()
        .abs();
    lerp(a, b, x)
}

fn view(g: &Game, cx: &mut Cx<Game>) -> El {
    let p = Paint::new(cx.system_dark(), cx.env().scale);
    g.reduced.set(cx.is_reduced_motion() || g.force_reduced);
    let s = schedule(g.streams, g.keep);
    let t = g.t();
    let reduced = g.reduced();

    let destroyed = s.rounds.iter().filter(|r| t >= r.shatter()).count();
    let (shake, tilt) = if reduced {
        ([0.0, 0.0], 0.0)
    } else {
        shake(&s, t)
    };
    let holding = s
        .rounds
        .iter()
        .any(|r| t >= r.hold && t < r.hold + r.spec.take.release);

    let mut world = el()
        .key("world")
        .size(STAGE_W, STAGE_H)
        .offset(shake[0], shake[1])
        .rotate(tilt)
        .offset_motion(Anim::INSTANT)
        .rotate_motion(Anim::INSTANT)
        .child(header(p, destroyed, holding, t, &s, g.keep))
        .child(hud(p, &s, t));

    // Glows sit under the cards they light; shards, sparks and rings over them.
    for (i, r) in s.rounds.iter().enumerate() {
        world = world.children(glows(p, i, r, t, reduced));
    }
    for (i, r) in s.rounds.iter().enumerate() {
        if let Some(card) = card(p, i, r, t, reduced) {
            world = world.child(card);
        }
    }
    world = world.child(words(p, &s, t, reduced));
    world = world.child(debris(p, &s, t, reduced));
    world = world.children(share(p, &s, t, destroyed, g.keep, reduced));

    el().key("game")
        .size(STAGE_W, STAGE_H)
        .instant_changes()
        .child(world)
}

/// Screen shake: the site's camera trauma (`cam.trauma += 0.32 · I`),
/// squared into a noisy offset, plus a kick along the last word's heading.
fn shake(s: &Schedule, t: f64) -> ([f32; 2], f32) {
    let mut trauma = 0.0f32;
    let mut kick = [0.0f32, 0.0];
    for r in &s.rounds {
        let te = (t - r.shatter()) as f32;
        if !(0.0..1.6).contains(&te) {
            continue;
        }
        let i = r.spec.heat;
        trauma += 0.42 * lerp(0.6, 1.25, i) * (-te * 2.6).exp();
        let w = std::f32::consts::TAU * 6.0;
        let zeta = 0.5;
        let wd = w * (1.0f32 - zeta * zeta).sqrt();
        let k = 520.0 * i * (-zeta * w * te).exp() * (wd * te).sin() / wd;
        let [dx, dy] = r.heading();
        kick[0] += dx * k;
        kick[1] += dy * k;
    }
    let trauma = trauma.min(1.0);
    let a = trauma * trauma;
    let t = t as f32;
    let nx = (t * 47.0).sin() * 0.6 + (t * 83.0 + 1.3).sin() * 0.4;
    let ny = (t * 53.0 + 2.1).sin() * 0.6 + (t * 71.0 + 0.4).sin() * 0.4;
    let nr = (t * 39.0 + 0.8).sin();
    (
        [kick[0] + 18.0 * a * nx, kick[1] + 18.0 * a * ny],
        0.012 * a * nr,
    )
}

fn header(p: Paint, destroyed: usize, holding: bool, t: f64, s: &Schedule, keep: bool) -> El {
    let k = p.k;
    let caps = vec!["⌥".to_string()];
    let score = row()
        .key("score")
        .absolute(0.0, 18.0)
        .h(30.0)
        .px(12.0)
        .gap(8.0)
        .center_y()
        .radius(15.0)
        .fill(k.c.surface_raised)
        .border(p.hair, k.c.border_weak)
        .child(
            text(destroyed.to_string())
                .key(format!("n{destroyed}"))
                .font(TextStyle::new(14.0, 640.0).lh(18.0).features("'tnum' 1"))
                .color(if destroyed > 0 {
                    ember_tone(p, 0.7)
                } else {
                    p.muted()
                })
                .transition(Transition::offset(0.0, -8.0).combined(Transition::opacity())),
        )
        .child(
            text("destroyed")
                .key("label")
                .font(k.ty.small)
                .color(p.muted()),
        );
    // The score pill shows once there's a score, or the share card folds into it.
    let show_score = destroyed > 0 && (t < s.share || (keep && t > s.share + SHARE_HOLD));
    let title = col()
        .key("title")
        .absolute(0.0, 6.0)
        .w(STAGE_W)
        .gap(6.0)
        .items_center()
        .child(
            text("Clear your messages")
                .key("t")
                .font(title_font(p))
                .color(p.text()),
        )
        .child(
            row()
                .key("lead")
                .gap(8.0)
                .center_y()
                .child(text("Hold").key("hold").font(LEAD).color(p.muted()))
                .child(keys(p, "keys", &caps, holding))
                .child(
                    text("and answer the one that's lit")
                        .key("rest")
                        .font(LEAD)
                        .color(p.muted()),
                ),
        );
    let done = row().key("done-at").absolute(STAGE_W - 76.0, 18.0).child(
        Button::new("Done")
            .variant(ButtonVariant::Neutral)
            .on_press(|g: &mut Game| g.from = None)
            .build_push(k)
            .key("done"),
    );
    el().key("header")
        .absolute(0.0, 0.0)
        .size(STAGE_W, 84.0)
        .child(title)
        .children(show_score.then_some(score))
        .child(done)
}

/// A stand-in for the Dot scope HUD the words fly out of.
fn hud(p: Paint, s: &Schedule, t: f64) -> El {
    let live = s
        .rounds
        .iter()
        .find(|r| t >= r.hold && t < r.hold + r.spec.take.release + 0.1);
    let (avg, peak) = live.map_or((0.04, 0.06), |r| r.spec.take.meter(t - r.hold));
    let k = p.k;
    row()
        .key("hud")
        .absolute(HUD_AT[0] - 46.0, HUD_AT[1] - 15.0)
        .size(92.0, 30.0)
        .center()
        .radius(15.0)
        .fill(if k.dark { k.c.bg_deep } else { k.c.text_strong })
        .border(p.hair, k.c.border_weak)
        .child(mic_level(p, "level", 16.0, avg, peak, live.is_some()))
}

fn card(p: Paint, i: usize, r: &Round, t: f64, reduced: bool) -> Option<El> {
    if t < r.arrive || t >= r.shatter() + if reduced { 0.3 } else { 0.0 } {
        return None;
    }
    let spec = r.spec;
    let age = (t - r.arrive) as f32;
    let u = (age / DRIFT as f32).clamp(0.0, 1.0);
    let (drift, mut opacity) = if reduced {
        ([0.0, 0.0], smooth(0.0, 0.35, age))
    } else {
        let e = 1.0 - (1.0 - u).powi(4);
        (
            [spec.from[0] * (1.0 - e), spec.from[1] * (1.0 - e)],
            smooth(0.0, 0.4, u),
        )
    };
    let targeted = t >= r.target;
    if !targeted {
        opacity *= 0.62;
    }
    if reduced && t >= r.shatter() {
        opacity *= 1.0 - ((t - r.shatter()) / 0.3) as f32;
    }
    let knock = if reduced {
        [0.0, 0.0]
    } else {
        r.flights.iter().fold([0.0, 0.0], |[x, y], (f, _, _)| {
            let [dx, dy] = f.hit(t);
            [x + dx, y + dy]
        })
    }
    .map(|v| v.clamp(-10.0, 10.0));
    let flash = if t >= r.explode && !reduced {
        1.0 - ((t - r.explode) / HITSTOP) as f32 * 0.4
    } else {
        r.flights
            .iter()
            .map(|(f, _, _)| f.flash(t))
            .fold(0.0, f32::max)
    };
    let speaking = r.flights.iter().any(|(f, _, _)| t >= f.arrives());
    let done = r.flights.iter().all(|(f, _, _)| t >= f.arrives());
    let body = match spec.card {
        Card::Chat(n) => {
            let lines = CARD.wrap(spec.take.text()).len();
            let reply = Reply::Lines {
                sent_bubbles: &[],
                text: "",
                caret: None,
                lines,
                placeholder: !speaking,
                focus: targeted && !done,
                sent: done,
            };
            notice(p, &n, reply, false)
        }
        Card::Certificate => certificate(p, targeted, speaking),
    };
    let body = floating(body, (!reduced && !targeted).then_some(t), i as f64 * 0.37);
    let ring = targeted.then(|| {
        el().key("lit")
            .absolute(-3.0, -3.0)
            .size(r.size[0] + 6.0, r.size[1] + 6.0)
            .radius(19.0)
            .continuous_corners()
            .border(
                1.5,
                ember_tone(p, 0.55).alpha(if done { 0.9 } else { 0.55 }),
            )
            .transition(Transition::opacity())
    });
    let flash_el = (flash > 0.02).then(|| {
        el().key("flash")
            .absolute(0.0, 0.0)
            .size(r.size[0], r.size[1])
            .radius(16.0)
            .continuous_corners()
            .fill(ember_tone(p, 0.95).alpha((flash * 0.55).min(0.7)))
    });
    let hitstop = t >= r.explode && !reduced;
    Some(
        el().key(format!("card{i}"))
            .absolute(spec.home[0], spec.home[1])
            .size(r.size[0], r.size[1])
            .offset(drift[0] + knock[0], drift[1] + knock[1])
            .scale(if hitstop { 1.035 } else { 1.0 })
            .opacity(opacity)
            .offset_motion(Anim::INSTANT)
            .opacity_motion(Anim::INSTANT)
            .scale_motion(Anim::INSTANT)
            .children(ring)
            .child(body)
            .children(flash_el),
    )
}

fn certificate(p: Paint, targeted: bool, signed: bool) -> El {
    let k = p.k;
    let line = el()
        .key("sign-line")
        .absolute(CERT_PAD, SIGN_Y + SIGN.line + 2.0)
        .size(CARD_W - 2.0 * CERT_PAD, p.hair.max(1.0))
        .fill(if targeted {
            k.c.border_focus
        } else {
            k.c.border_base
        });
    let hint = (!signed).then(|| {
        text("Sign by saying your name")
            .key("hint")
            .font(k.ty.ui)
            .color(p.faint())
            .absolute(CERT_PAD, SIGN_Y + 6.0)
            .transition(Transition::opacity())
    });
    let card = el()
        .key("certificate")
        .size(CARD_W, CERT_H)
        .radius(16.0)
        .continuous_corners()
        .fill(if k.dark {
            k.c.surface_raised
        } else {
            k.c.bg_base
        })
        .border(p.hair, k.c.border_weak)
        .child(
            col()
                .key("copy")
                .absolute(CERT_PAD, CERT_PAD - 2.0)
                .gap(2.0)
                .child(text("Hex").key("app").font(k.ty.small).color(p.muted()))
                .child(
                    text("Certificate of completion")
                        .key("title")
                        .font(k.ty.ui_medium)
                        .color(p.text()),
                )
                .child(
                    text("I hereby have completed Hex onboarding.")
                        .key("body")
                        .font(k.ty.ui)
                        .color(p.text()),
                ),
        )
        .child(line)
        .children(hint);
    k.elevate(card, &k.elevation.floating)
}

fn words(p: Paint, s: &Schedule, t: f64, reduced: bool) -> El {
    let mut out = el().key("words").absolute(0.0, 0.0).size(STAGE_W, STAGE_H);
    let mut all: Vec<Flight> = Vec::new();
    for (ri, r) in s.rounds.iter().enumerate() {
        if t >= r.shatter() {
            continue;
        }
        let (field, origin, _) = field_of(&r.spec);
        let weight = 500.0 + 220.0 * r.spec.heat;
        let font = field.font.lh(field.line);
        for (wi, (flight, word, centre)) in r.flights.iter().enumerate() {
            all.push(*flight);
            if let Some(landed) = flight.landed(t, reduced) {
                let at = [origin[0] + centre[0], origin[1] + centre[1]];
                out = out.children(typeset(
                    p,
                    &format!("r{ri}w{wi}"),
                    word,
                    font,
                    at,
                    landed,
                    flicker(t, wi),
                ));
            }
            if let Some(pose) = flight.pose(t, reduced) {
                out = out.child(flying(p, &format!("r{ri}f{wi}"), word, weight, pose));
            }
        }
    }
    out.child(impacts(p, "impacts", [STAGE_W, STAGE_H], &all, t, reduced).absolute(0.0, 0.0))
}

/// The card's own pieces, as the site cuts them: a jittered grid split into
/// triangles that fly out from the centre and along the hit, spinning and
/// tumbling under drag and gravity, their cut edges hot and cooling.
struct Shard {
    at: [f32; 2],
    v: [f32; 2],
    spin: f32,
    life: f32,
    verts: [[f32; 2]; 3],
    tint: f32,
}

const SHARD_DRAG: f32 = 2.6;
const SHARD_GRAVITY: f32 = 520.0;
const SPARK_DRAG: f32 = 4.2;
const SPARK_GRAVITY: f32 = 380.0;

fn ballistic(at: [f32; 2], v: [f32; 2], drag: f32, gravity: f32, t: f32) -> ([f32; 2], [f32; 2]) {
    let decay = (-drag * t).exp();
    let reach = (1.0 - decay) / drag;
    let terminal = gravity / drag;
    (
        [
            at[0] + v[0] * reach,
            at[1] + terminal * t + (v[1] - terminal) * reach,
        ],
        [v[0] * decay, terminal + (v[1] - terminal) * decay],
    )
}

fn shards(r: &Round, seed: usize) -> Vec<Shard> {
    let [w, h] = r.size;
    let i = r.spec.heat;
    let cols = (8.0 + 4.0 * i).round() as usize;
    let rows = ((cols as f32 * h / w).round() as usize + 1).max(2);
    let mut pts = Vec::with_capacity((cols + 1) * (rows + 1));
    for j in 0..=rows {
        for c in 0..=cols {
            let edge = c == 0 || j == 0 || c == cols || j == rows;
            let s = seed + j * 31 + c * 7;
            let jx = if edge { 0.0 } else { rnd(s, -0.38, 0.38) };
            let jy = if edge { 0.0 } else { rnd(s + 1, -0.38, 0.38) };
            pts.push([
                ((c as f32 + jx) / cols as f32 - 0.5) * w,
                ((j as f32 + jy) / rows as f32 - 0.5) * h,
            ]);
        }
    }
    let at = |c: usize, j: usize| pts[j * (cols + 1) + c];
    let [dx, dy] = r.heading();
    let [cx, cy] = r.centre();
    let speed = lerp(380.0, 900.0, i);
    let mut out = Vec::new();
    for j in 0..rows {
        for c in 0..cols {
            let (a, b, cc, d) = (at(c, j), at(c + 1, j), at(c + 1, j + 1), at(c, j + 1));
            let s = seed + j * 131 + c * 17;
            let tris = if rnd(s, 0.0, 1.0) < 0.5 {
                [[a, b, cc], [a, cc, d]]
            } else {
                [[a, b, d], [b, cc, d]]
            };
            for (n, tri) in tris.into_iter().enumerate() {
                let s = s + n * 3;
                let mx = (tri[0][0] + tri[1][0] + tri[2][0]) / 3.0;
                let my = (tri[0][1] + tri[1][1] + tri[2][1]) / 3.0;
                let (rx, ry) = (mx / (w * 0.5), my / (h * 0.5));
                let rl = rx.hypot(ry) + 0.25;
                let sp = speed * rnd(s + 1, 0.55, 1.25);
                let ox = rx / rl * 0.85 + dx * 0.8 + rnd(s + 2, -0.25, 0.25);
                let oy = ry / rl * 0.85 + dy * 0.8 + rnd(s + 3, -0.25, 0.25) - 0.1;
                out.push(Shard {
                    at: [cx + mx, cy + my],
                    v: [ox * sp, oy * sp],
                    spin: rnd(s + 4, -12.0, 12.0),
                    life: rnd(s + 5, 0.55, 1.1),
                    verts: tri.map(|[x, y]| [x - mx, y - my]),
                    tint: rnd(s + 6, 0.0, 1.0),
                });
            }
        }
    }
    out
}

struct Spark {
    at: [f32; 2],
    v: [f32; 2],
    life: f32,
    heat: f32,
    size: f32,
    gravity: f32,
}

fn sparks(r: &Round, seed: usize) -> Vec<Spark> {
    let i = r.spec.heat;
    let [dx, dy] = r.heading();
    let [cx, cy] = r.centre();
    let [hw, hh] = [r.size[0] * 0.5, r.size[1] * 0.5];
    let mut out = Vec::new();
    for n in 0..(lerp(18.0, 60.0, i).round() as usize) {
        let s = seed + n * 11;
        let a = rnd(s, 0.0, std::f32::consts::TAU);
        let sp = rnd(s + 1, 500.0, 1800.0);
        out.push(Spark {
            at: [cx, cy],
            v: [a.cos() * sp + dx * 400.0, a.sin() * sp + dy * 400.0],
            life: rnd(s + 2, 0.18, 0.5),
            heat: 1.0,
            size: rnd(s + 3, 1.4, 2.6),
            gravity: SPARK_GRAVITY,
        });
    }
    for n in 0..10 {
        let s = seed + 997 + n * 13;
        let a = rnd(s, 0.0, std::f32::consts::TAU);
        let sp = rnd(s + 1, 40.0, 200.0);
        out.push(Spark {
            at: [cx + rnd(s + 2, -40.0, 40.0), cy + rnd(s + 3, -14.0, 14.0)],
            v: [a.cos() * sp, a.sin() * sp - 40.0],
            life: rnd(s + 4, 0.8, 1.5),
            heat: 0.0,
            size: rnd(s + 5, 1.2, 2.0),
            gravity: SPARK_GRAVITY,
        });
    }
    // Buoyant embers rising off a big blast.
    let pyro = smooth(0.32, 0.92, i);
    for n in 0..(lerp(8.0, 28.0, pyro).round() as usize) {
        let s = seed + 1999 + n * 17;
        let a = rnd(s, 0.0, std::f32::consts::TAU);
        let sp = rnd(s + 1, 60.0, 260.0);
        out.push(Spark {
            at: [
                cx + rnd(s + 2, -hw * 0.65, hw * 0.65),
                cy + rnd(s + 3, -hh * 0.45, hh * 0.45),
            ],
            v: [
                a.cos() * sp + dx * 110.0,
                -rnd(s + 4, 120.0, 360.0) + a.sin() * 60.0,
            ],
            life: rnd(s + 5, 0.65, 1.45),
            heat: rnd(s + 6, 0.75, 1.0),
            size: rnd(s + 7, 1.6, 3.0),
            gravity: -SPARK_GRAVITY * 0.25,
        });
    }
    out
}

fn ring(shape: Shape, centre: [f32; 2], r: f32, width: f32, color: Color) -> Shape {
    let points: Vec<[f32; 2]> = (0..48)
        .map(|n| {
            let a = n as f32 / 48.0 * std::f32::consts::TAU;
            [centre[0] + r * a.cos(), centre[1] + r * a.sin()]
        })
        .collect();
    shape.stroke_with(points, true, width, LineCap::Round, color)
}

fn debris(p: Paint, s: &Schedule, t: f64, reduced: bool) -> El {
    let k = p.k;
    let mut shape = Shape::new();
    if !reduced {
        let fill = if k.dark {
            k.c.surface_raised
        } else {
            k.c.bg_base
        };
        let inked = fill.mix(k.c.text_base, 0.07);
        for (ri, r) in s.rounds.iter().enumerate() {
            let te = (t - r.shatter()) as f32;
            if !(0.0..1.6).contains(&te) {
                continue;
            }
            let seed = ri * 7919 + 13;
            let size = r.size[0].max(r.size[1]);
            let i = r.spec.heat;
            let centre = r.centre();
            // Shockwave: kind 0, cubic ease-out to rMax over 0.44 s.
            let u = te / 0.44;
            if u < 1.0 {
                let rad = (size * 0.78 + 68.0) * lerp(0.85, 1.4, i) * (1.0 - (1.0 - u).powi(3));
                let pyro = smooth(0.32, 0.92, i);
                let width = lerp(2.2, 3.6, pyro) * (1.0 - u * 0.7);
                let fade = (1.0 - u) * (1.0 - u);
                let color = k.c.bg_base.mix(ember_tone(p, 1.0 - u * 0.6), fade);
                shape = ring(shape, centre, rad, width, color);
            }
            // The scorch's second, slower pressure ring: kind 10.
            let u = te / lerp(0.32, 0.52, i);
            if u < 1.0 {
                let rad = (size * 0.85 + 75.0) * (0.25 + 0.7 * (1.0 - (1.0 - u).powf(2.2)));
                let color = k.c.bg_base.mix(ember_tone(p, 0.8), (1.0 - u) * 0.6);
                shape = ring(shape, centre, rad, 2.2, color);
            }
            for sh in shards(r, seed) {
                if te >= sh.life {
                    continue;
                }
                let ([x, y], _) = ballistic(sh.at, sh.v, SHARD_DRAG, SHARD_GRAVITY, te);
                let ang = sh.spin * (1.0 - (-0.95 * te).exp()) / 0.95;
                let tumble = 1.0 + (sh.spin.abs() * 0.65 + 2.5) * te;
                let tilt = 0.4 + 0.6 * tumble.cos().abs();
                let (sin, cos) = ang.sin_cos();
                let pts: Vec<[f32; 2]> = sh
                    .verts
                    .iter()
                    .map(|&[vx, vy]| {
                        let vx = vx * tilt;
                        [x + vx * cos - vy * sin, y + vx * sin + vy * cos]
                    })
                    .collect();
                let left = sh.life - te;
                let fade = (left / (sh.life * 0.35)).min(1.0);
                let heat = (-te / 0.28).exp();
                let face = if sh.tint > 0.7 { inked } else { fill };
                // A shard turns darker as it tumbles edge-on.
                let face = face.mix(k.c.bg_deep, (1.0 - tilt) * 0.5);
                shape = shape.fill(pts.clone(), face.alpha(fade));
                let edge = if heat > 0.08 {
                    k.c.border_weak.mix(ember_tone(p, 0.75), heat)
                } else {
                    k.c.border_weak
                };
                shape = shape.stroke_with(pts, true, 1.2, LineCap::Round, edge.alpha(fade));
            }
            for sp in sparks(r, seed) {
                if te >= sp.life {
                    continue;
                }
                let left = 1.0 - te / sp.life;
                let ([x, y], [vx, vy]) = ballistic(sp.at, sp.v, SPARK_DRAG, sp.gravity, te);
                let streak = 1.6 / 60.0 * 0.5;
                let color = if sp.heat > 0.0 {
                    k.c.bg_base
                        .mix(ember_tone(p, sp.heat * left), (left * 0.9).clamp(0.0, 1.0))
                } else {
                    k.c.bg_base.mix(k.c.text_weak, left * 0.6)
                };
                shape = shape.line(
                    [x - vx * streak, y - vy * streak],
                    [x + vx * streak + 0.1, y + vy * streak],
                    sp.size,
                    LineCap::Round,
                    color,
                );
            }
        }
    }
    canvas(shape)
        .key("debris")
        .absolute(0.0, 0.0)
        .size(STAGE_W, STAGE_H)
}

fn blob(key: String, centre: [f32; 2], r: f32, color: Color, blur: f32) -> El {
    el().key(key)
        .absolute(centre[0] - r, centre[1] - r)
        .size(2.0 * r, 2.0 * r)
        .radius(r)
        .fill(color)
        .blur(blur)
}

/// The site's volumetric pieces as blurred discs: the flash (kind 1), the
/// blast light on its neighbours, fireball lobes cooling to smoke (kind 8),
/// and smoke plumes rising after them (kind 9).
fn glows(p: Paint, ri: usize, r: &Round, t: f64, reduced: bool) -> Vec<El> {
    let te = (t - r.shatter()) as f32;
    if reduced || !(0.0..2.0).contains(&te) {
        return Vec::new();
    }
    let k = p.k;
    let i = r.spec.heat;
    let pyro = smooth(0.32, 0.92, i).max(0.35);
    let size = r.size[0].max(r.size[1]);
    let [cx, cy] = r.centre();
    let [dx, dy] = r.heading();
    let seed = ri * 4513 + 7;
    let smoke = if k.dark {
        k.c.text_weak
    } else {
        k.c.text_muted
    };
    let mut out = Vec::new();

    let u = te / 0.38;
    if u < 1.0 {
        let rad = (size * 0.8 + 80.0) * lerp(0.85, 1.35, pyro);
        let a = 0.14 * (1.0 - u).powf(1.85);
        out.push(blob(
            format!("light{ri}"),
            [cx, cy],
            rad,
            ember_tone(p, 0.6).alpha(a),
            rad * 0.35,
        ));
    }
    let lobes = if pyro > 0.55 { 3 } else { 2 };
    for li in 0..lobes {
        let s = seed + li * 19;
        let age = te + li as f32 * 0.025;
        let life = lerp(0.58, 0.92, pyro);
        let u = age / life;
        if !(0.0..1.0).contains(&u) {
            continue;
        }
        let (ox, oy) = if li == 0 {
            (0.0, 0.0)
        } else {
            (
                rnd(s, -r.size[0] * 0.22, r.size[0] * 0.22),
                rnd(s + 1, -r.size[1] * 0.17, r.size[1] * 0.17),
            )
        };
        let r0 = if li == 0 { size * 0.2 } else { size * 0.15 } * lerp(0.85, 1.35, pyro);
        let rad = lerp(r0, r0 * lerp(1.85, 2.35, pyro), 1.0 - (1.0 - u).powf(2.2));
        let vx = dx * rnd(s + 2, 25.0, 70.0) + ox * 0.6;
        let vy = -rnd(s + 3, 28.0, 65.0) + dy * 25.0;
        let at = [cx + ox + vx * age, cy + oy + vy * age];
        let hot = ember_tone(p, 1.0 - u);
        let color = hot.mix(smoke, smooth(0.35, 0.9, u));
        let a = 0.5 * (1.0 - u).powf(1.6) * lerp(0.85, 1.0, pyro);
        out.push(blob(
            format!("fire{ri}-{li}"),
            at,
            rad,
            color.alpha(a),
            rad * 0.45,
        ));
    }
    let u = te / 0.1;
    if u < 1.0 {
        let rad = size * 0.3 + 30.0;
        let white = if k.dark { k.c.text_strong } else { k.c.white };
        out.push(blob(
            format!("flash{ri}"),
            [cx, cy],
            rad,
            white.mix(ember_tone(p, 1.0), 0.2).alpha(0.85 * (1.0 - u)),
            rad * 0.3,
        ));
    }
    for si in 0..2 {
        let s = seed + 300 + si * 23;
        let age = te - 0.05 - si as f32 * 0.04;
        let life = rnd(s, 1.15, 1.85);
        let u = age / life;
        if !(0.0..1.0).contains(&u) {
            continue;
        }
        let r0 = size * rnd(s + 1, 0.14, 0.2);
        let rad = lerp(r0, r0 * 2.0, 1.0 - (1.0 - u).powf(2.2));
        let at = [
            cx + rnd(s + 2, -r.size[0] * 0.25, r.size[0] * 0.25) + rnd(s + 3, -28.0, 28.0) * age,
            cy - rnd(s + 4, 52.0, 98.0) * age,
        ];
        let a = 0.2 * (u * std::f32::consts::PI).sin();
        out.push(blob(
            format!("smoke{ri}-{si}"),
            at,
            rad,
            smoke.alpha(a),
            rad * 0.55,
        ));
    }
    out
}

/// The shareable moment: the certificate's blast becomes a score card.
fn share(
    p: Paint,
    s: &Schedule,
    t: f64,
    destroyed: usize,
    keep: bool,
    reduced: bool,
) -> Option<El> {
    let age = (t - s.share) as f32;
    if age < 0.0 || (keep && age > SHARE_HOLD as f32 + 0.5) {
        return None;
    }
    let k = p.k;
    let w = 420.0;
    let cert = &s.rounds[2];
    let [cx, cy] = cert.centre();
    let rise = if reduced {
        1.0
    } else {
        1.0 - (1.0 - (age / 0.55).clamp(0.0, 1.0)).powi(4)
    };
    let fold = if keep {
        smooth(SHARE_HOLD as f32, SHARE_HOLD as f32 + 0.45, age)
    } else {
        0.0
    };
    let home = [(STAGE_W - w) * 0.5, 150.0];
    let from = [cx - w * 0.5 - home[0], cy - 120.0 - home[1]];
    let to_pill = [24.0 - home[0] - w * 0.4, 20.0 - home[1] - 110.0];
    let offset = [
        from[0] * (1.0 - rise) + to_pill[0] * fold,
        from[1] * (1.0 - rise) + to_pill[1] * fold,
    ];
    let scale = if reduced {
        1.0
    } else {
        lerp(0.9, 1.0, rise) * lerp(1.0, 0.18, fold)
    };
    let opacity = smooth(0.0, 0.3, age) * (1.0 - fold);
    let words: usize = s.rounds[..3]
        .iter()
        .map(|r| r.spec.take.text().split_whitespace().count())
        .sum();
    let secs = s.rounds[2].shatter() - s.rounds[0].hold;
    let stat = |key: &str, value: String, label: &str| {
        col()
            .key(key.to_string())
            .gap(1.0)
            .grow(1.0)
            .child(
                text(value)
                    .key("v")
                    .font(TextStyle::new(17.0, 620.0).lh(22.0).features("'tnum' 1"))
                    .color(p.text()),
            )
            .child(
                text(label.to_string())
                    .key("l")
                    .font(k.ty.small)
                    .color(p.muted()),
            )
    };
    let big = TextStyle::new(72.0, 720.0)
        .lh(76.0)
        .tracking(-2.0)
        .features("'tnum' 1");
    let number = el()
        .key("number")
        .child(
            text(destroyed.to_string())
                .key("glow")
                .font(big)
                .color(ember_tone(p, 0.7).alpha(0.55))
                .absolute(0.0, 0.0)
                .blur(10.0),
        )
        .child(
            text(destroyed.to_string())
                .key("n")
                .font(big)
                .color(ember_tone(p, 0.85)),
        );
    let card = col()
        .key("share-card")
        .w(w)
        .padding(22.0)
        .gap(16.0)
        .radius(20.0)
        .continuous_corners()
        .fill(if k.dark {
            k.c.surface_raised
        } else {
            k.c.bg_base
        })
        .border(p.hair, k.c.border_weak)
        .child(
            row()
                .key("top")
                .center_y()
                .child(
                    text("Hex")
                        .key("brand")
                        .font(k.ty.ui_medium)
                        .color(p.text()),
                )
                .child(el().key("space").grow(1.0))
                .child(
                    text("Onboarding complete")
                        .key("when")
                        .font(k.ty.small)
                        .color(p.muted()),
                ),
        )
        .child(
            row()
                .key("score")
                .gap(16.0)
                .items_end()
                .child(number)
                .child(
                    col()
                        .key("what")
                        .pb(12.0)
                        .gap(2.0)
                        .child(
                            text("messages destroyed")
                                .key("a")
                                .font(LEAD)
                                .color(p.text()),
                        )
                        .child(
                            text("by voice, no typing")
                                .key("b")
                                .font(k.ty.small)
                                .color(p.muted()),
                        ),
                ),
        )
        .child(
            row()
                .key("stats")
                .gap(12.0)
                .child(stat("words", words.to_string(), "words said"))
                .child(stat("keys", "0".into(), "keys typed"))
                .child(stat("time", format!("{secs:.1} s"), "start to finish")),
        )
        .child(
            row()
                .key("signed")
                .gap(6.0)
                .center_y()
                .child(text("Signed").key("s").font(k.ty.small).color(p.muted()))
                .child(
                    text(NAME)
                        .key("name")
                        .font(SIGN.font.lh(28.0))
                        .color(p.text()),
                ),
        )
        .child(
            row()
                .key("actions")
                .gap(8.0)
                .center_y()
                .child(
                    Button::new("Copy image")
                        .variant(ButtonVariant::Neutral)
                        .on_press(|_: &mut Game| {})
                        .build_push(k)
                        .key("copy"),
                )
                .child(el().key("space").grow(1.0))
                .child(
                    Button::new("Keep playing")
                        .variant(ButtonVariant::Ghost)
                        .on_press(|g: &mut Game| g.keep = true)
                        .build_push(k)
                        .key("keep"),
                )
                .child(
                    Button::new("Continue")
                        .variant(ButtonVariant::Contrast)
                        .on_press(|g: &mut Game| g.from = None)
                        .build_push(k)
                        .key("continue"),
                ),
        );
    let card = k.elevate(card, &k.elevation.floating);
    Some(
        el().key("share")
            .absolute(home[0], home[1])
            .w(w)
            .offset(offset[0], offset[1])
            .scale(scale)
            .opacity(opacity)
            .blur(if reduced { 0.0 } else { 4.0 * (1.0 - rise) })
            .offset_motion(Anim::INSTANT)
            .opacity_motion(Anim::INSTANT)
            .scale_motion(Anim::INSTANT)
            .blur_motion(Anim::INSTANT)
            .child(card),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn three_messages_take_about_ten_seconds() {
        let s = schedule(true, false);
        assert_eq!(s.rounds.len(), 3);
        assert!(s.share > 8.0 && s.share < 12.0, "{}", s.share);
        let batch = schedule(false, false);
        assert!(batch.share >= s.share, "words wait for release");
    }

    #[test]
    fn one_card_is_lit_at_a_time() {
        let s = schedule(true, true);
        for pair in s.rounds.windows(2) {
            assert!(pair[1].target > pair[0].shatter());
        }
    }

    #[test]
    fn every_card_breaks_into_pieces() {
        let s = schedule(true, false);
        for r in &s.rounds {
            assert!(shards(r, 1).len() >= 24);
            assert!(sparks(r, 1).len() >= 28);
        }
    }
}
