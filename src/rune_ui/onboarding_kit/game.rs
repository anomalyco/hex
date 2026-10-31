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
/// After the last word lands, the cracks race out and the card swells
/// into the hit-stop: one beat, not two.
const AFTER_LAND: f64 = 0.12;
const SHARE_HOLD: f64 = 2.8;
/// Keep playing folds the share card into the score pill.
const FOLD: f32 = 0.45;
/// The score pill's top-left, level with Done, and its size.
const SCORE_AT: [f32; 2] = [24.0, 18.0];
const PILL: [f32; 2] = [112.0, 30.0];
const SHARE_W: f32 = 432.0;
const SHARE_PAD: f32 = 24.0;
const SHARE_GAP: f32 = 16.0;
/// The share card's rows, top to bottom, each a fixed height so the card's
/// own height is known: brand, score, rule, stats, rule, signature, actions.
const SHARE_ROWS: [f32; 7] = [20.0, 78.0, 1.0, 42.0, 1.0, 32.0, 28.0];
const SHARE_H: f32 = 2.0 * SHARE_PAD
    + SHARE_ROWS[0]
    + SHARE_ROWS[1]
    + SHARE_ROWS[2]
    + SHARE_ROWS[3]
    + SHARE_ROWS[4]
    + SHARE_ROWS[5]
    + SHARE_ROWS[6]
    + 6.0 * SHARE_GAP;
const SHARE_Y: f32 = 128.0;
const BIG_SIZE: f32 = 76.0;
const BIG_LINE: f32 = 78.0;

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

/// Two columns of cards, centred on the stage with a small gutter.
const LEFT: f32 = (STAGE_W - 2.0 * CARD_W - GUTTER) * 0.5;
const RIGHT: f32 = LEFT + CARD_W + GUTTER;
const GUTTER: f32 = 12.0;

const SCRIPT: [Spec; 3] = [
    Spec {
        card: Card::Chat(MILK),
        take: &SAY_MILK,
        heat: 0.5,
        home: [LEFT, 128.0],
        from: [-520.0, 40.0],
    },
    Spec {
        card: Card::Chat(GREG),
        take: &SAY_GREG,
        heat: 0.92,
        home: [RIGHT, 218.0],
        from: [520.0, -30.0],
    },
    Spec {
        card: Card::Certificate,
        take: &SAY_NAME,
        heat: 0.62,
        home: [LEFT, 318.0],
        from: [-80.0, 420.0],
    },
];

const ENDLESS: [Spec; 3] = [
    Spec {
        card: Card::Chat(FRANK),
        take: &SAY_FRANK,
        heat: 0.55,
        home: [RIGHT, 140.0],
        from: [500.0, -60.0],
    },
    Spec {
        card: Card::Chat(DAX),
        take: &SAY_DAX,
        heat: 0.6,
        home: [LEFT, 260.0],
        from: [-520.0, 0.0],
    },
    Spec {
        card: Card::Chat(ADAM),
        take: &SAY_ADAM,
        heat: 0.75,
        home: [RIGHT, 330.0],
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

    /// When the last word lands and the cracks start.
    fn charge(&self) -> f64 {
        self.explode - AFTER_LAND
    }

    /// Where the last word hits, in card coordinates: the cracks start here.
    fn impact(&self) -> [f32; 2] {
        let (_, origin, _) = field_of(&self.spec);
        self.flights
            .last()
            .map_or([self.size[0] * 0.5, self.size[1] * 0.5], |(_, _, c)| {
                [
                    origin[0] + c[0] - self.spec.home[0],
                    origin[1] + c[1] - self.spec.home[1],
                ]
            })
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
        .child(header(p, destroyed, holding, t, &s, g.keep, reduced))
        .child(hud(p, &s, t, g.keep));

    // Glows sit under the cards they light; shards, sparks and rings over them.
    for (i, r) in s.rounds.iter().enumerate() {
        world = world.children(glows(p, i, r, t, reduced));
    }
    for (i, r) in s.rounds.iter().enumerate() {
        if let Some(card) = card(p, i, r, t, reduced) {
            world = world.child(card);
        }
    }
    world = world.children(fragments(p, &s, t, reduced));
    world = world.child(words(p, &s, t, reduced));
    world = world.children(rings(p, &s, t, reduced));
    world = world.child(debris(p, &s, t, reduced));
    world = world.children(share(p, &s, t, destroyed, g.keep, reduced));

    el().key("game")
        .w_full()
        .h_full()
        .center()
        .instant_changes()
        .child(el().key("stage").size(STAGE_W, STAGE_H).child(world))
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

/// How much of the game's chrome (title, HUD, Done) shows: it steps aside
/// for the share card and comes back on Keep playing.
fn chrome(s: &Schedule, t: f64, keep: bool) -> f32 {
    let age = (t - s.share) as f32;
    let back = if keep {
        smooth(SHARE_HOLD as f32, SHARE_HOLD as f32 + 0.18, age)
    } else {
        0.0
    };
    1.0 - smooth(0.0, 0.18, age) + back
}

/// Keep playing's fold, `0..1` from the share card to the score pill.
fn fold(s: &Schedule, t: f64, keep: bool) -> f32 {
    if !keep {
        return 0.0;
    }
    (((t - s.share) as f32 - SHARE_HOLD as f32) / FOLD).clamp(0.0, 1.0)
}

/// The pill takes over in the fold's last 80 ms (Reduce Motion: a crossfade).
fn pill_in(fold: f32, reduced: bool) -> f32 {
    if reduced {
        smooth(0.0, 0.3 / FOLD, fold)
    } else {
        smooth(1.0 - 0.08 / FOLD, 1.0, fold)
    }
}

fn header(
    p: Paint,
    destroyed: usize,
    holding: bool,
    t: f64,
    s: &Schedule,
    keep: bool,
    reduced: bool,
) -> El {
    let k = p.k;
    let caps = vec!["⌥".to_string()];
    let chrome = chrome(s, t, keep);
    // The count kicks up as each card goes.
    let pop = if reduced {
        0.0
    } else {
        s.rounds
            .iter()
            .map(|r| (t - r.shatter()) as f32)
            .filter(|te| *te >= 0.0)
            .fold(0.0f32, |_, te| {
                (-te * 7.0).exp() * (te * 22.0).cos().max(0.0)
            })
    };
    let fold = fold(s, t, keep);
    // The pill steps aside for the share card, and the share card folds back into it.
    let pill = if t < s.share {
        1.0
    } else if fold > 0.0 {
        pill_in(fold, reduced)
    } else {
        1.0 - smooth(0.0, 0.18, (t - s.share) as f32)
    };
    let number = text(destroyed.to_string())
        .key(format!("n{destroyed}"))
        .font(TextStyle::new(14.0, 640.0).lh(18.0).features("'tnum' 1"))
        .color(if destroyed > 0 {
            ember_tone(p, 0.7)
        } else {
            p.muted()
        })
        .transition(if reduced {
            Transition::opacity()
        } else {
            Transition::offset(0.0, -8.0).combined(Transition::opacity())
        });
    let score = row()
        .key("score")
        .absolute(SCORE_AT[0], SCORE_AT[1])
        .scale(1.0 + 0.12 * pop)
        .opacity(pill)
        .scale_motion(Anim::INSTANT)
        .opacity_motion(Anim::INSTANT)
        .size(PILL[0], PILL[1])
        .px(12.0)
        .gap(8.0)
        .center_y()
        .radius(15.0)
        .fill(k.c.surface_raised)
        .border(p.hair, k.c.border_weak)
        .child(number)
        .child(
            text("destroyed")
                .key("label")
                .font(k.ty.small)
                .color(p.muted()),
        );
    let show_score = destroyed > 0 && pill > 0.005;
    let title = col()
        .key("title")
        .absolute(0.0, 6.0)
        .w(STAGE_W)
        .opacity(chrome)
        .opacity_motion(Anim::INSTANT)
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
    // The share card's Continue stands in for Done while it shows.
    let done = (chrome > 0.005).then(|| {
        row()
            .key("done-at")
            .absolute(0.0, SCORE_AT[1])
            .w(STAGE_W)
            .pr(SCORE_AT[0])
            .justify_end()
            .opacity(chrome)
            .opacity_motion(Anim::INSTANT)
            .child(
                Button::new("Done")
                    .variant(ButtonVariant::Neutral)
                    .on_press(|g: &mut Game| g.from = None)
                    .build_push(k)
                    .key("done"),
            )
    });
    el().key("header")
        .absolute(0.0, 0.0)
        .size(STAGE_W, 84.0)
        .child(title)
        .children(show_score.then_some(score))
        .children(done)
}

/// A stand-in for the Dot scope HUD the words fly out of.
fn hud(p: Paint, s: &Schedule, t: f64, keep: bool) -> El {
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
        .opacity(chrome(s, t, keep))
        .opacity_motion(Anim::INSTANT)
        .center()
        .radius(15.0)
        .fill(if k.dark { k.c.bg_deep } else { k.c.text_strong })
        .border(p.hair, k.c.border_weak)
        .child(mic_level(p, "level", 16.0, avg, peak, live.is_some()))
}

/// The notice or certificate as it looks at `t`.
fn body(p: Paint, r: &Round, t: f64, reduced: bool) -> El {
    let spec = r.spec;
    let targeted = t >= r.target;
    // The placeholder clears just before the first word lands, not under it.
    let speaking = r.flights.iter().any(|(f, _, _)| {
        if reduced {
            f.landed(t, true).is_some()
        } else {
            t >= f.arrives() - 0.2
        }
    });
    let done = r.flights.iter().all(|(f, _, _)| t >= f.arrives());
    match spec.card {
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
    }
}

/// The card frozen at `t` with its answer typeset in, in card coordinates:
/// what each fragment of the explosion carries.
fn face(p: Paint, ri: usize, r: &Round, t: f64, at: [f32; 2]) -> El {
    el().key(format!("face{ri}"))
        .absolute(at[0], at[1])
        .size(r.size[0], r.size[1])
        .child(body(p, r, t, false))
        .children(landed_words(p, ri, r, t, false, r.spec.home))
}

/// The words already in the field, in coordinates relative to `at`.
fn landed_words(p: Paint, ri: usize, r: &Round, t: f64, reduced: bool, at: [f32; 2]) -> Vec<El> {
    let (field, origin, _) = field_of(&r.spec);
    let font = field.font.lh(field.line);
    let mut out = Vec::new();
    for (wi, (flight, word, centre)) in r.flights.iter().enumerate() {
        if let Some(landed) = flight.landed(t, reduced) {
            let xy = [origin[0] + centre[0] - at[0], origin[1] + centre[1] - at[1]];
            out.extend(typeset(
                p,
                &format!("r{ri}w{wi}"),
                word,
                font,
                xy,
                landed,
                flicker(t, wi),
            ));
        }
    }
    out
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
    // Waiting cards sit back, dimmed and a touch smaller; the lit one comes forward.
    let lit = smooth(
        0.0,
        if reduced { 0.18 } else { 0.32 },
        (t - r.target) as f32,
    );
    opacity *= lerp(0.55, 1.0, lit);
    let gone = if reduced && t >= r.shatter() {
        1.0 - ((t - r.shatter()) / 0.3) as f32
    } else {
        1.0
    };
    opacity *= gone;
    let knock = if reduced {
        [0.0, 0.0]
    } else {
        r.flights.iter().fold([0.0, 0.0], |[x, y], (f, _, _)| {
            let [dx, dy] = f.hit(t);
            [x + dx, y + dy]
        })
    }
    .map(|v| v.clamp(-10.0, 10.0));
    // One beat: the cracks race out and the card swells, then it holds that
    // pose through the hit-stop and shatters.
    let charge = if reduced {
        0.0
    } else {
        smooth(0.0, AFTER_LAND as f32, (t - r.charge()) as f32)
    };
    let flash = if reduced {
        0.0
    } else {
        r.flights
            .iter()
            .map(|(f, _, _)| f.flash(t))
            .fold(charge, f32::max)
    };
    let done = r.flights.iter().all(|(f, _, _)| t >= f.arrives());
    let body = body(p, r, t, reduced);
    let body = floating(body, (!reduced && !targeted).then_some(t), i as f64 * 0.37);
    // The lit card breathes while it waits for an answer, and burns steady once answered.
    let breathe = if reduced || done {
        1.0
    } else {
        0.78 + 0.22 * (((t - r.target) * 3.4).sin() as f32)
    };
    let ember = ember_tone(p, 0.6);
    let ring = targeted.then(|| {
        el().key("lit")
            .absolute(-2.0, -2.0)
            .size(r.size[0] + 4.0, r.size[1] + 4.0)
            .radius(18.0)
            .continuous_corners()
            .border(1.0, ember.alpha(lerp(0.0, 0.75, lit) * breathe))
            .shadow(0.0, 30.0, ember.alpha(lerp(0.0, 0.2, lit) * breathe))
    });
    let flash_el = (flash > 0.02).then(|| {
        el().key("flash")
            .absolute(0.0, 0.0)
            .size(r.size[0], r.size[1])
            .radius(16.0)
            .continuous_corners()
            .fill(ember_tone(p, 0.75).alpha((flash * 0.35).min(0.45)))
    });
    let cracks = (charge > 0.0).then(|| cracks(p, r, &fracture(r, i * 7919 + 13), t));
    // The answer rides in the card: it knocks and swells with it, and under
    // Reduce Motion fades out with it.
    let answer = landed_words(p, i, r, t, reduced, spec.home);
    Some(
        el().key(format!("card{i}"))
            .absolute(spec.home[0], spec.home[1])
            .size(r.size[0], r.size[1])
            .offset(drift[0] + knock[0], drift[1] + knock[1])
            .scale(if reduced {
                1.0
            } else {
                lerp(lerp(0.97, 1.0, lit), SWELL, charge)
            })
            .opacity(opacity)
            .offset_motion(Anim::INSTANT)
            .opacity_motion(Anim::INSTANT)
            .scale_motion(Anim::INSTANT)
            .children(ring)
            .child(body)
            .children(answer)
            .children(flash_el)
            .children(cracks),
    )
}

/// How far the card swells into the hit-stop.
const SWELL: f32 = 1.035;

/// The fracture lines racing out from where the last word hit, along the
/// pieces' own edges, between the landing and the shatter.
fn cracks(p: Paint, r: &Round, f: &Fracture, t: f64) -> El {
    let window = (AFTER_LAND + HITSTOP) as f32;
    let u = ((t - r.charge()) as f32 / window).clamp(0.0, 1.0);
    let [w, h] = r.size;
    let impact = r.impact();
    let reach = w.hypot(h);
    // On a light page a glow reads as paint: thinner, fainter strokes there.
    let (glow_w, hot_w, glow_a) = if p.k.dark {
        (3.2, 1.3, 0.4)
    } else {
        (2.2, 0.9, 0.15)
    };
    let glow = ember_tone(p, 0.7).alpha(glow_a);
    let hot = ember_tone(p, 0.95);
    let mut shape = Shape::new();
    for &(a, b) in &f.seams {
        let (near, far) = if dist(a, impact) <= dist(b, impact) {
            (a, b)
        } else {
            (b, a)
        };
        // Nearer cracks start first; each runs out from its end nearest the hit.
        let start = 0.5 * dist(near, impact) / reach;
        let run = smooth(start, start + 0.4, u);
        if run <= 0.0 {
            continue;
        }
        let tip = [lerp(near[0], far[0], run), lerp(near[1], far[1], run)];
        shape = shape.line(near, tip, glow_w, LineCap::Round, glow).line(
            near,
            tip,
            hot_w,
            LineCap::Round,
            hot,
        );
    }
    canvas(shape).key("cracks").absolute(0.0, 0.0).size(w, h)
}

fn dist(a: [f32; 2], b: [f32; 2]) -> f32 {
    (a[0] - b[0]).hypot(a[1] - b[1])
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
        let weight = 500.0 + 220.0 * r.spec.heat;
        for (wi, (flight, word, _)) in r.flights.iter().enumerate() {
            all.push(*flight);
            if let Some(pose) = flight.pose(t, reduced) {
                out = out.child(flying(p, &format!("r{ri}f{wi}"), word, weight, pose));
            }
        }
    }
    out.child(impacts(p, "impacts", [STAGE_W, STAGE_H], &all, t, reduced).absolute(0.0, 0.0))
}

/// How the card breaks: convex pieces cut by straight fracture lines, each
/// carrying its part of the card, words and all, the way the site clips the
/// card's own image into every Voronoi cell. Pieces near the hit are small;
/// the far ones stay big enough to read.
struct Fracture {
    pieces: Vec<Piece>,
    /// Every cut's chord through the piece it split, in card coordinates:
    /// together they're the edges between pieces.
    seams: Vec<([f32; 2], [f32; 2])>,
}

/// A cut line: a point on it and the normal into the piece it bounds.
type Cut = ([f32; 2], [f32; 2]);

struct Piece {
    /// Card coordinates, clockwise.
    poly: Vec<[f32; 2]>,
    /// The cuts that bound it: a point on each line and the normal into the piece.
    cuts: Vec<Cut>,
    centroid: [f32; 2],
    v: [f32; 2],
    spin: f32,
    life: f32,
}

const SLAB_DRAG: f32 = 2.2;
const SLAB_GRAVITY: f32 = 900.0;

fn dot(a: [f32; 2], b: [f32; 2]) -> f32 {
    a[0] * b[0] + a[1] * b[1]
}

fn sub(a: [f32; 2], b: [f32; 2]) -> [f32; 2] {
    [a[0] - b[0], a[1] - b[1]]
}

fn area(poly: &[[f32; 2]]) -> f32 {
    let n = poly.len();
    (0..n)
        .map(|i| {
            let (a, b) = (poly[i], poly[(i + 1) % n]);
            a[0] * b[1] - b[0] * a[1]
        })
        .sum::<f32>()
        .abs()
        * 0.5
}

fn centroid(poly: &[[f32; 2]]) -> [f32; 2] {
    let n = poly.len();
    let (mut cx, mut cy, mut a2) = (0.0, 0.0, 0.0);
    for i in 0..n {
        let (a, b) = (poly[i], poly[(i + 1) % n]);
        let c = a[0] * b[1] - b[0] * a[1];
        cx += (a[0] + b[0]) * c;
        cy += (a[1] + b[1]) * c;
        a2 += c;
    }
    if a2.abs() < 1e-3 {
        let k = n.max(1) as f32;
        return [
            poly.iter().map(|q| q[0]).sum::<f32>() / k,
            poly.iter().map(|q| q[1]).sum::<f32>() / k,
        ];
    }
    [cx / (3.0 * a2), cy / (3.0 * a2)]
}

/// `poly`'s span along `d`.
fn extent(poly: &[[f32; 2]], d: [f32; 2]) -> (f32, f32) {
    poly.iter()
        .map(|&q| dot(q, d))
        .fold((f32::INFINITY, f32::NEG_INFINITY), |(lo, hi), v| {
            (lo.min(v), hi.max(v))
        })
}

/// Cuts a convex polygon along the line through `at` with normal `n`: the
/// part `n` points into, the rest, and the chord between them.
#[allow(clippy::type_complexity)]
fn split(
    poly: &[[f32; 2]],
    at: [f32; 2],
    n: [f32; 2],
) -> (Vec<[f32; 2]>, Vec<[f32; 2]>, Vec<[f32; 2]>) {
    let side = |q: [f32; 2]| dot(sub(q, at), n);
    let (mut front, mut back, mut chord) = (Vec::new(), Vec::new(), Vec::new());
    for i in 0..poly.len() {
        let (a, b) = (poly[i], poly[(i + 1) % poly.len()]);
        let (da, db) = (side(a), side(b));
        if da >= 0.0 {
            front.push(a);
        } else {
            back.push(a);
        }
        if (da >= 0.0) != (db >= 0.0) {
            let f = da / (da - db);
            let x = [lerp(a[0], b[0], f), lerp(a[1], b[1], f)];
            front.push(x);
            back.push(x);
            chord.push(x);
        }
    }
    (front, back, chord)
}

fn fracture(r: &Round, seed: usize) -> Fracture {
    let [w, h] = r.size;
    let impact = r.impact();
    let reach = w.hypot(h);
    let count = lerp(12.0, 18.0, r.spec.heat).round() as usize;
    let smallest = w * h * 0.012;
    let mut cells: Vec<(Vec<[f32; 2]>, Vec<Cut>, bool)> = vec![(
        vec![[0.0, 0.0], [w, 0.0], [w, h], [0.0, h]],
        Vec::new(),
        false,
    )];
    let mut seams = Vec::new();
    let mut tries = 0;
    while cells.len() < count && tries < 64 {
        tries += 1;
        // Split the cell that's biggest for how near the hit it is.
        let Some(ci) = (0..cells.len()).filter(|&i| !cells[i].2).max_by(|&a, &b| {
            let weight = |i: usize| {
                let poly = &cells[i].0;
                area(poly) / (0.18 + dist(centroid(poly), impact) / reach).powf(1.4)
            };
            weight(a).total_cmp(&weight(b))
        }) else {
            break;
        };
        let poly = cells[ci].0.clone();
        let g = centroid(&poly);
        let away = sub(g, impact);
        let len = away[0].hypot(away[1]);
        let radial = if len > 1.0 {
            [away[0] / len, away[1] / len]
        } else {
            [0.0, 1.0]
        };
        let around = [-radial[1], radial[0]];
        // Cut across its long way: a ring crack across a long radial
        // piece, a radial crack across a wide one.
        let (lo_r, hi_r) = extent(&poly, radial);
        let (lo_a, hi_a) = extent(&poly, around);
        let normal = if hi_r - lo_r > hi_a - lo_a {
            radial
        } else {
            around
        };
        let mut done = false;
        for attempt in 0..6 {
            let s = seed + tries * 101 + attempt * 13;
            let turn = rnd(s, -0.45, 0.45);
            let (sn, cs) = turn.sin_cos();
            let n = [
                normal[0] * cs - normal[1] * sn,
                normal[0] * sn + normal[1] * cs,
            ];
            let (lo, hi) = extent(&poly, n);
            let at = dot(g, n) + (hi - lo) * rnd(s + 1, -0.18, 0.18);
            let point = [n[0] * at, n[1] * at];
            let (front, back, chord) = split(&poly, point, n);
            let (fa, ba) = (area(&front), area(&back));
            if chord.len() != 2 || fa.min(ba) < smallest.max((fa + ba) * 0.16) {
                continue;
            }
            seams.push((chord[0], chord[1]));
            let mut front_cuts = cells[ci].1.clone();
            let mut back_cuts = front_cuts.clone();
            front_cuts.push((point, n));
            back_cuts.push((point, [-n[0], -n[1]]));
            cells[ci] = (front, front_cuts, false);
            cells.push((back, back_cuts, false));
            done = true;
            break;
        }
        if !done {
            cells[ci].2 = true;
        }
    }
    let i = r.spec.heat;
    let [dx, dy] = r.heading();
    let speed = lerp(300.0, 680.0, i);
    let pieces = cells
        .into_iter()
        .enumerate()
        .map(|(n, (poly, cuts, _))| {
            // Only the cuts that still touch it bound it.
            let cuts = cuts
                .into_iter()
                .filter(|&(at, nrm)| poly.iter().any(|&q| dot(sub(q, at), nrm).abs() < 0.5))
                .collect();
            let g = centroid(&poly);
            let s = seed + 401 + n * 53;
            let rx = (g[0] - w * 0.5) / (w * 0.5);
            let ry = (g[1] - h * 0.5) / (h * 0.5);
            let rl = rx.hypot(ry) + 0.3;
            // Small pieces near the hit fly hardest.
            let near = 1.0 - (dist(g, impact) / reach).min(1.0);
            let sp = speed * rnd(s, 0.7, 1.15) * lerp(0.85, 1.25, near);
            let ox = rx / rl + dx * 0.6 + rnd(s + 1, -0.15, 0.15);
            let oy = ry / rl * 1.2 + dy * 0.6 - 0.35 + rnd(s + 2, -0.15, 0.15);
            Piece {
                poly,
                cuts,
                centroid: g,
                v: [ox * sp, oy * sp],
                spin: rnd(s + 3, 1.5, 5.5) * if ox < 0.0 { -1.0 } else { 1.0 },
                life: rnd(s + 4, 0.7, 1.0),
            }
        })
        .collect();
    Fracture { pieces, seams }
}

/// What's left of the certificate clears fast once the share card shows.
fn reward_fade(s: &Schedule, ri: usize, t: f64) -> f32 {
    if ri == 2 {
        1.0 - smooth(0.0, 0.14, (t - s.share) as f32)
    } else {
        1.0
    }
}

/// A frame for the clip chain: local point `q` sits at card point
/// `origin + R(angle)·q`.
#[derive(Clone, Copy)]
struct Frame {
    origin: [f32; 2],
    angle: f32,
}

fn rot(v: [f32; 2], a: f32) -> [f32; 2] {
    let (s, c) = a.sin_cos();
    [c * v[0] - s * v[1], s * v[0] + c * v[1]]
}

impl Frame {
    /// Card point `x` in this frame's local coordinates.
    fn local(self, x: [f32; 2]) -> [f32; 2] {
        rot(sub(x, self.origin), -self.angle)
    }
}

/// One piece of the card, clipped to its polygon: each cut is a node turned
/// so the cut runs along its top edge, clipping everything inside it to the
/// cut's far side; the card's face sits inside the last one, turned back.
#[allow(clippy::too_many_arguments)]
fn piece(
    p: Paint,
    ri: usize,
    n: usize,
    r: &Round,
    pc: &Piece,
    face_el: El,
    motion: ([f32; 2], f32, f32, f32),
    heat: f32,
) -> El {
    let (offset, angle, scale, opacity) = motion;
    let [w, h] = r.size;
    let g = pc.centroid;
    let pad = 2.0;
    let (x0, x1) = extent(&pc.poly, [1.0, 0.0]);
    let (y0, y1) = extent(&pc.poly, [0.0, 1.0]);
    let (x0, y0, x1, y1) = (x0 - pad, y0 - pad, x1 + pad, y1 + pad);
    let mut frame = Frame {
        origin: [x0, y0],
        angle: 0.0,
    };
    // Outermost first; built inside out below.
    let mut nodes: Vec<([f32; 4], f32, f32)> = Vec::new();
    for &(at, nrm) in &pc.cuts {
        // The cut's frame: x along the line, y into the piece, its origin
        // where the piece's centroid projects onto the line.
        let theta = (-nrm[0]).atan2(nrm[1]);
        let anchor = sub(
            g,
            [nrm[0] * dot(sub(g, at), nrm), nrm[1] * dot(sub(g, at), nrm)],
        );
        let d = [nrm[1], -nrm[0]];
        let (lo, hi) = extent(&pc.poly, d);
        let base = dot(anchor, d);
        let (ua, ub) = (lo - base - pad, hi - base + pad);
        let (_, vb) = extent(&pc.poly, nrm);
        let depth = vb - dot(anchor, nrm) + pad;
        let pivot = frame.local(anchor);
        let pos = [pivot[0] + ua, pivot[1]];
        nodes.push((
            [pos[0], pos[1], ub - ua, depth],
            -ua / (ub - ua),
            theta - frame.angle,
        ));
        frame = Frame {
            origin: [
                anchor[0] + rot([ua, 0.0], theta)[0],
                anchor[1] + rot([ua, 0.0], theta)[1],
            ],
            angle: theta,
        };
    }
    let hot = ember_tone(p, 0.95);
    // A hot tint over a light card reads as pink paint, so it's mostly a rim there.
    let (tint, rim, rim_a) = if p.k.dark {
        (0.3, 3.0, 1.0)
    } else {
        (0.08, 1.5, 0.7)
    };
    let glow = (heat > 0.03).then(|| {
        let shape = Shape::new()
            .fill(pc.poly.clone(), hot.alpha(tint * heat))
            .stroke_with(
                pc.poly.clone(),
                true,
                rim,
                LineCap::Round,
                hot.alpha(rim_a * heat),
            );
        canvas(shape).key("heat").absolute(0.0, 0.0).size(w, h)
    });
    let pivot = frame.local(g);
    let mut inner = el()
        .key("face")
        .absolute(pivot[0] - g[0], pivot[1] - g[1])
        .size(w, h)
        .origin(g[0] / w, g[1] / h)
        .rotate(-frame.angle)
        .rotate_motion(Anim::INSTANT)
        .child(face_el)
        .children(glow);
    for (k, ([x, y, nw, nh], pivot_x, turn)) in nodes.into_iter().enumerate().rev() {
        inner = el()
            .key(format!("cut{k}"))
            .absolute(x, y)
            .size(nw, nh)
            .origin(pivot_x, 0.0)
            .rotate(turn)
            .rotate_motion(Anim::INSTANT)
            .clip_rounded(0.0)
            .child(inner);
    }
    let (bw, bh) = (x1 - x0, y1 - y0);
    el().key(format!("piece{ri}-{n}"))
        .absolute(r.spec.home[0] + x0, r.spec.home[1] + y0)
        .size(bw, bh)
        .origin((g[0] - x0) / bw, (g[1] - y0) / bh)
        .offset(offset[0], offset[1])
        .rotate(angle)
        .scale(scale)
        .opacity(opacity)
        .offset_motion(Anim::INSTANT)
        .rotate_motion(Anim::INSTANT)
        .scale_motion(Anim::INSTANT)
        .opacity_motion(Anim::INSTANT)
        .child(inner)
}

fn fragments(p: Paint, s: &Schedule, t: f64, reduced: bool) -> Vec<El> {
    let mut out = Vec::new();
    if reduced {
        return out;
    }
    for (ri, r) in s.rounds.iter().enumerate() {
        let te = (t - r.shatter()) as f32;
        if !(0.0..1.1).contains(&te) {
            continue;
        }
        let [w, h] = r.size;
        let clear = reward_fade(s, ri, t);
        if clear <= 0.0 {
            continue;
        }
        for (n, pc) in fracture(r, ri * 7919 + 13).pieces.iter().enumerate() {
            if te >= pc.life {
                continue;
            }
            let g = pc.centroid;
            let ([px, py], _) = ballistic([0.0, 0.0], pc.v, SLAB_DRAG, SLAB_GRAVITY, te);
            let ang = pc.spin * (1.0 - (-1.4 * te).exp()) / 1.4;
            // Falls away from the camera: starts at the hit-stop's swell, then shrinks.
            let depth = lerp(SWELL, 0.84, 1.0 - (-2.4 * te).exp());
            let grow = depth - 1.0;
            let fade = ((pc.life - te) / (pc.life * 0.45)).min(1.0) * clear;
            let heat = (-te / 0.2).exp();
            let offset = [px + (g[0] - w * 0.5) * grow, py + (g[1] - h * 0.5) * grow];
            out.push(piece(
                p,
                ri,
                n,
                r,
                pc,
                face(p, ri, r, r.shatter(), [0.0, 0.0]),
                (offset, ang, depth, fade),
                heat,
            ));
        }
    }
    out
}

/// Small chips spraying out of the cracks: splinters that fly faster than
/// the slabs, tumbling under drag and gravity and catching the light as
/// they turn.
struct Shard {
    at: [f32; 2],
    v: [f32; 2],
    spin: f32,
    life: f32,
    verts: [[f32; 2]; 3],
    tint: f32,
}

const SHARD_DRAG: f32 = 2.6;
const SHARD_GRAVITY: f32 = 620.0;
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
    let seams = fracture(r, seed).seams;
    let [dx, dy] = r.heading();
    let [hx, hy] = r.spec.home;
    let speed = lerp(480.0, 1100.0, i);
    let count = lerp(36.0, 72.0, i).round() as usize;
    (0..count)
        .map(|n| {
            let s = seed + 3001 + n * 23;
            let (a, b) = seams[n % seams.len()];
            let f = rnd(s, 0.05, 0.95);
            let at = [lerp(a[0], b[0], f), lerp(a[1], b[1], f)];
            let (rx, ry) = ((at[0] - w * 0.5) / (w * 0.5), (at[1] - h * 0.5) / (h * 0.5));
            let rl = rx.hypot(ry) + 0.35;
            let sp = speed * rnd(s + 1, 0.45, 1.25);
            let ox = rx / rl * 0.85 + dx * 0.8 + rnd(s + 2, -0.45, 0.45);
            let oy = ry / rl * 0.85 + dy * 0.8 + rnd(s + 3, -0.45, 0.45) - 0.1;
            let size = rnd(s + 4, 3.0, 9.0);
            let verts = [0.0, 2.2, 4.1].map(|turn: f32| {
                let ang = turn + rnd(s + 5 + (turn * 3.0) as usize, -0.5, 0.5);
                let reach = size * rnd(s + 9 + (turn * 3.0) as usize, 0.6, 1.2);
                [ang.cos() * reach, ang.sin() * reach]
            });
            Shard {
                at: [hx + at[0], hy + at[1]],
                v: [ox * sp, oy * sp],
                spin: rnd(s + 6, -14.0, 14.0),
                life: rnd(s + 7, 0.45, 0.95),
                verts,
                tint: rnd(s + 8, 0.0, 1.0),
            }
        })
        .collect()
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

/// A stroked circle as a native rounded box, so it stays round at any size.
fn ring(key: String, centre: [f32; 2], r: f32, width: f32, color: Color) -> El {
    let outer = r + width * 0.5;
    el().key(key)
        .absolute(centre[0] - outer, centre[1] - outer)
        .size(2.0 * outer, 2.0 * outer)
        .radius(outer)
        .border(width, color)
}

/// The shockwave (the site's kind 0: cubic ease-out, hot cooling to
/// ember) and the slower pressure ring behind it (kind 10).
fn rings(p: Paint, s: &Schedule, t: f64, reduced: bool) -> Vec<El> {
    let mut out = Vec::new();
    if reduced {
        return out;
    }
    for (ri, r) in s.rounds.iter().enumerate() {
        let te = (t - r.shatter()) as f32;
        if !(0.0..1.0).contains(&te) {
            continue;
        }
        let size = r.size[0].max(r.size[1]);
        let i = r.spec.heat;
        let centre = r.centre();
        let pyro = smooth(0.32, 0.92, i);
        let u = te / 0.36;
        if u < 1.0 {
            let rad = (size * 0.42 + 36.0) * lerp(0.9, 1.25, i) * (1.0 - (1.0 - u).powi(3));
            let width = lerp(2.4, 3.4, pyro) * (1.0 - u * 0.8);
            let fade = (1.0 - u) * (1.0 - u);
            let color = ember_tone(p, 1.0 - u * 0.5).alpha(0.9 * fade);
            out.push(ring(
                format!("shock{ri}"),
                centre,
                rad.max(1.0),
                width,
                color,
            ));
        }
        let u = (te - 0.04) / lerp(0.4, 0.6, i);
        if (0.0..1.0).contains(&u) {
            let rad = (size * 0.55 + 50.0) * (0.3 + 0.7 * (1.0 - (1.0 - u).powf(2.2)));
            let color = ember_tone(p, 0.7).alpha(0.35 * (1.0 - u));
            out.push(ring(format!("pressure{ri}"), centre, rad, 1.2, color));
        }
    }
    out
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
        let shade = fill.mix(k.c.text_base, if k.dark { 0.06 } else { 0.1 });
        let shine = if k.dark { k.c.text_strong } else { k.c.white };
        for (ri, r) in s.rounds.iter().enumerate() {
            let te = (t - r.shatter()) as f32;
            if !(0.0..1.6).contains(&te) {
                continue;
            }
            let seed = ri * 7919 + 13;
            let clear = reward_fade(s, ri, t);
            for sh in shards(r, seed) {
                if te >= sh.life {
                    continue;
                }
                let ([x, y], _) = ballistic(sh.at, sh.v, SHARD_DRAG, SHARD_GRAVITY, te);
                let ang = sh.spin * (1.0 - (-0.95 * te).exp()) / 0.95;
                let tumble = 1.0 + (sh.spin.abs() * 0.65 + 2.5) * te + sh.tint * 6.0;
                let tilt = 0.25 + 0.75 * tumble.cos().abs();
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
                let fade = (left / (sh.life * 0.35)).min(1.0) * clear;
                let heat = (-te / 0.25).exp();
                // A chip flashes as its face turns to the light.
                let glint = tumble.cos().max(0.0).powi(8);
                let face = if sh.tint > 0.6 { shade } else { fill }
                    .mix(shine, 0.35 * glint)
                    .mix(ember_tone(p, 0.8), 0.6 * heat);
                shape = shape.fill(pts.clone(), face.alpha(fade));
                let edge = k.c.border_base.mix(ember_tone(p, 0.85), heat);
                shape = shape.stroke_with(pts, true, 0.8, LineCap::Round, edge.alpha(fade));
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
                    color.alpha(clear),
                );
            }
        }
    }
    canvas(shape)
        .key("debris")
        .absolute(0.0, 0.0)
        .size(STAGE_W, STAGE_H)
}

/// A soft disc lit from inside: `core` at the centre, through `shell`, to
/// nothing at the rim.
fn blob(key: String, centre: [f32; 2], r: f32, core: Color, shell: Color) -> El {
    el().key(key)
        .absolute(centre[0] - r, centre[1] - r)
        .size(2.0 * r, 2.0 * r)
        .radial_gradient(
            [0.5, 0.5],
            r,
            [
                (0.0, core),
                (0.3, shell),
                (0.65, shell.alpha(shell.a * 0.35)),
                (1.0, shell.alpha(0.0)),
            ],
        )
}

/// A fireball's core and shell `age` seconds in: white-hot for 70 ms, ember
/// cooling over the next 250 ms, then smoke.
fn fire(p: Paint, age: f32, smoke: Color) -> (Color, Color) {
    let white = if p.k.dark {
        p.k.c.text_strong
    } else {
        p.k.c.white
    };
    let core = white
        .mix(ember_tone(p, 0.95), smooth(0.05, 0.09, age))
        .mix(ember_tone(p, 0.5), smooth(0.09, 0.34, age))
        .mix(smoke, smooth(0.34, 0.6, age));
    let shell = ember_tone(p, 0.85)
        .mix(ember_tone(p, 0.4), smooth(0.0, 0.3, age))
        .mix(smoke, smooth(0.3, 0.55, age));
    (core, shell)
}

/// The site's volumetric pieces as radial glows: the flash (kind 1), the
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
    // Light on a light page reads as paint, so it's much thinner there.
    let thin = if k.dark { 1.0 } else { 0.25 };
    let mut out = Vec::new();

    let u = te / 0.38;
    if u < 1.0 {
        let rad = (size * 0.45 + 50.0) * lerp(0.9, 1.3, pyro) * 1.3;
        let a = 0.18 * thin * (1.0 - u).powf(1.85);
        let light = ember_tone(p, 0.6);
        out.push(blob(
            format!("light{ri}"),
            [cx, cy],
            rad,
            light.alpha(a),
            light.alpha(a * 0.55),
        ));
    }
    let lobes = if pyro > 0.55 { 3 } else { 2 };
    for li in 0..lobes {
        let s = seed + li * 19;
        // Each lobe kindles a beat after the one before.
        let age = te - li as f32 * 0.025;
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
        let r0 = if li == 0 { size * 0.13 } else { size * 0.1 } * lerp(0.85, 1.3, pyro);
        let rad = lerp(r0, r0 * lerp(1.6, 2.0, pyro), 1.0 - (1.0 - u).powf(2.2)) * 1.4;
        let vx = dx * rnd(s + 2, 25.0, 70.0) + ox * 0.6;
        let vy = -rnd(s + 3, 28.0, 65.0) + dy * 25.0;
        let at = [cx + ox + vx * age, cy + oy + vy * age];
        let (core, shell) = fire(p, age, smoke);
        let a = 0.5 * thin * (1.0 - u).powf(1.8) * lerp(0.85, 1.0, pyro);
        out.push(blob(
            format!("fire{ri}-{li}"),
            at,
            rad,
            core.alpha(a),
            shell.alpha(a * 0.7),
        ));
    }
    let u = te / 0.08;
    if u < 1.0 {
        let rad = (size * 0.2 + 24.0) * 1.3;
        let white = if k.dark { k.c.text_strong } else { k.c.white };
        let a = 0.75 * (1.0 - u) * (1.0 - u);
        out.push(blob(
            format!("flash{ri}"),
            [cx, cy],
            rad,
            white.alpha(a),
            white.mix(ember_tone(p, 1.0), 0.35).alpha(a * 0.6),
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
        let rad = lerp(r0, r0 * 2.0, 1.0 - (1.0 - u).powf(2.2)) * 1.5;
        let at = [
            cx + rnd(s + 2, -r.size[0] * 0.25, r.size[0] * 0.25) + rnd(s + 3, -28.0, 28.0) * age,
            cy - rnd(s + 4, 52.0, 98.0) * age,
        ];
        let a = 0.12 * thin * (u * std::f32::consts::PI).sin();
        out.push(blob(
            format!("smoke{ri}-{si}"),
            at,
            rad,
            smoke.alpha(a),
            smoke.alpha(a * 0.6),
        ));
    }
    out
}

fn ease_in_out(x: f32) -> f32 {
    if x < 0.5 {
        4.0 * x * x * x
    } else {
        1.0 - (2.0 - 2.0 * x).powi(3) * 0.5
    }
}

/// The shareable moment: the certificate's blast becomes a score card. Its
/// surface goes opaque at once, then its content shows; Keep playing folds
/// the surface and the number into the score pill.
fn share(
    p: Paint,
    s: &Schedule,
    t: f64,
    destroyed: usize,
    keep: bool,
    reduced: bool,
) -> Option<El> {
    let age = (t - s.share) as f32;
    let fold = fold(s, t, keep);
    if age < 0.0 || fold >= 1.0 {
        return None;
    }
    let k = p.k;
    let w = SHARE_W;
    let cert = &s.rounds[2];
    let [cx, cy] = cert.centre();
    let x = (age / 0.6).clamp(0.0, 1.0);
    let rise = if reduced {
        1.0
    } else {
        1.0 - (1.0 - x).powi(4)
    };
    // easeOutBack: a touch past full size, then settles.
    let settle = if reduced {
        1.0
    } else {
        let c = 1.4;
        1.0 + (c + 1.0) * (x - 1.0).powi(3) + c * (x - 1.0).powi(2)
    };
    let home = [(STAGE_W - w) * 0.5, SHARE_Y];
    let from = [cx - (home[0] + w * 0.5), cy - (home[1] + SHARE_H * 0.5)];
    let surface_in = smooth(0.0, 0.11, age);
    let content_in = smooth(0.11, 0.23, age);
    let pill = pill_in(fold, reduced);
    // The fold: everything but the number goes in its first 100 ms, the
    // surface and number become the pill, and the pill takes over at the end.
    let (morph, rest) = if reduced {
        (0.0, 1.0 - pill)
    } else {
        (ease_in_out(fold), 1.0 - smooth(0.0, 0.1 / FOLD, fold))
    };
    // Still warm from the blast, cooling.
    let warm = (-age / 0.45).exp();
    let words: usize = s.rounds[..3]
        .iter()
        .map(|r| r.spec.take.text().split_whitespace().count())
        .sum();
    let secs = s.rounds[2].shatter() - s.rounds[0].hold;
    // The number counts up, each tick landing with a small kick.
    let count = if reduced {
        destroyed as f32
    } else {
        smooth(0.15, 0.75, age) * destroyed as f32
    };
    let shown = count.ceil() as usize;
    let since_tick = if reduced || shown == 0 {
        1.0
    } else {
        // When the count passed `shown - 1`: smoothstep's inverse.
        let y = (shown as f32 - 1.0) / destroyed as f32;
        let x = 0.5 - ((1.0 - 2.0 * y).asin() / 3.0).sin();
        age - lerp(0.15, 0.75, x)
    };
    let kick = if reduced {
        0.0
    } else {
        (-since_tick.max(0.0) * 9.0).exp()
    };
    let ember = ember_tone(p, 0.7);
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
    let rule = |key: &str| {
        el().key(key.to_string())
            .size(w - 2.0 * SHARE_PAD, SHARE_ROWS[2])
            .fill(k.c.border_weak)
    };
    let big = TextStyle::new(BIG_SIZE, 720.0)
        .lh(BIG_LINE)
        .tracking(-2.0)
        .features("'tnum' 1");
    let number = |key: &str, color: Color| {
        el().key(key.to_string())
            .scale(1.0 + 0.1 * kick)
            .origin(0.5, 0.8)
            .scale_motion(Anim::INSTANT)
            .child(
                text(shown.to_string())
                    .key("glow")
                    .font(big)
                    .color(ember.alpha((0.45 + 0.35 * kick) * warm))
                    .absolute(0.0, 0.0)
                    .blur(12.0),
            )
            .child(text(shown.to_string()).key("n").font(big).color(color))
    };
    let folding = fold > 0.0 && !reduced;
    let content = col()
        .key("content")
        .absolute(0.0, 0.0)
        .w(w)
        .padding(SHARE_PAD)
        .gap(SHARE_GAP)
        .opacity(content_in * rest)
        .opacity_motion(Anim::INSTANT)
        .child(
            row()
                .key("top")
                .h(SHARE_ROWS[0])
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
                .h(SHARE_ROWS[1])
                .gap(16.0)
                .items_end()
                .child(
                    number("number", ember_tone(p, 0.85 + 0.15 * kick))
                        .opacity(if folding { 0.0 } else { 1.0 })
                        .opacity_motion(Anim::INSTANT),
                )
                .child(
                    col()
                        .key("what")
                        .pb(14.0)
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
        .child(rule("rule-1"))
        .child(
            row()
                .key("stats")
                .h(SHARE_ROWS[3])
                .gap(12.0)
                .child(stat("words", words.to_string(), "words said"))
                .child(stat("keys", "0".into(), "keys typed"))
                .child(stat("time", format!("{secs:.1} s"), "start to finish")),
        )
        .child(rule("rule-2"))
        .child(
            row()
                .key("signed")
                .h(SHARE_ROWS[5])
                .gap(10.0)
                .items_end()
                .child(
                    text("Signed")
                        .key("s")
                        .font(k.ty.small)
                        .color(p.muted())
                        .pb(6.0),
                )
                .child(
                    col()
                        .key("sig")
                        .gap(2.0)
                        .child(
                            text(NAME)
                                .key("name")
                                .font(SIGN.font.lh(28.0))
                                .color(p.text()),
                        )
                        .child(
                            el().key("line")
                                .size(200.0, p.hair.max(1.0))
                                .fill(k.c.border_base),
                        ),
                ),
        )
        .child(
            row()
                .key("actions")
                .h(SHARE_ROWS[6])
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
    // The surface, from the card's rect to the pill's.
    let pill_rect = [
        SCORE_AT[0] - home[0],
        SCORE_AT[1] - home[1],
        PILL[0],
        PILL[1],
    ];
    let card_rect = [0.0, 0.0, w, SHARE_H];
    let [sx, sy, sw, sh]: [f32; 4] =
        std::array::from_fn(|n| lerp(card_rect[n], pill_rect[n], morph));
    let fill = if k.dark {
        k.c.surface_raised
    } else {
        k.c.bg_base.mix(k.c.surface_raised, morph)
    };
    let surface = el()
        .key("surface")
        .absolute(sx, sy)
        .size(sw, sh)
        .radius(lerp(20.0, PILL[1] * 0.5, morph))
        .continuous_corners()
        .fill(fill)
        .radial_gradient(
            [0.14, 0.28],
            [240.0, 160.0],
            [(0.0, ember.alpha(0.12 * warm)), (1.0, ember.alpha(0.0))],
        )
        .border(p.hair, k.c.border_weak);
    let surface = k.elevate(surface, &k.elevation.floating);
    let surface = if warm > 0.02 {
        surface.shadow(0.0, 96.0, ember.alpha(0.16 * warm))
    } else {
        surface
    }
    .opacity(surface_in * (1.0 - pill))
    .opacity_motion(Anim::INSTANT);
    // The number flies to the pill's, shrinking to its size.
    let tiny = 14.0 / BIG_SIZE;
    let num_from = [SHARE_PAD, SHARE_PAD + SHARE_ROWS[0] + SHARE_GAP];
    let num_to = [
        SCORE_AT[0] + 12.0 - home[0],
        SCORE_AT[1] + (PILL[1] - BIG_LINE * tiny) * 0.5 - home[1],
    ];
    let flying_number = folding.then(|| {
        number("number-fold", ember_tone(p, lerp(0.85, 0.7, morph)))
            .absolute(
                lerp(num_from[0], num_to[0], morph),
                lerp(num_from[1], num_to[1], morph),
            )
            .scale(lerp(1.0, tiny, morph))
            .origin(0.0, 0.0)
            .opacity(1.0 - pill)
            .opacity_motion(Anim::INSTANT)
    });
    Some(
        el().key("share")
            .absolute(home[0], home[1])
            .size(w, SHARE_H)
            .offset(from[0] * (1.0 - rise), from[1] * (1.0 - rise))
            .scale(if reduced {
                1.0
            } else {
                lerp(0.86, 1.0, settle)
            })
            .offset_motion(Anim::INSTANT)
            .scale_motion(Anim::INSTANT)
            .child(surface)
            .child(content)
            .children(flying_number),
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
        let s = schedule(true, true);
        for (ri, r) in s.rounds.iter().enumerate() {
            assert!(shards(r, 1).len() >= 24);
            assert!(sparks(r, 1).len() >= 28);
            let f = fracture(r, ri * 7919 + 13);
            assert!((12..=18).contains(&f.pieces.len()), "{}", f.pieces.len());
            // The pieces tile the card exactly, each bounded by its cuts.
            let total: f32 = f.pieces.iter().map(|pc| area(&pc.poly)).sum();
            let card = r.size[0] * r.size[1];
            assert!((total - card).abs() < card * 1e-3, "{total} vs {card}");
            for pc in &f.pieces {
                assert!(!pc.cuts.is_empty());
                for &(at, n) in &pc.cuts {
                    assert!(pc.poly.iter().all(|&q| dot(sub(q, at), n) > -0.01));
                }
            }
        }
    }

    #[test]
    fn a_frame_maps_its_origin_and_turns() {
        let f = Frame {
            origin: [10.0, 20.0],
            angle: 0.5,
        };
        let q = [3.0, -4.0];
        let x = [
            f.origin[0] + rot(q, f.angle)[0],
            f.origin[1] + rot(q, f.angle)[1],
        ];
        let back = f.local(x);
        assert!((back[0] - q[0]).abs() < 1e-4 && (back[1] - q[1]).abs() < 1e-4);
    }
}
