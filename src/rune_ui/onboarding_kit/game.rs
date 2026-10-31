//! A throwaway proposal: the practice step as a mini game. Notifications
//! drift in, one is targeted at a time, the player holds the shortcut and
//! answers it, the words fly into it, and a bolt of lightning strikes it and
//! burns it away (`game_fx`, after Psychopomp's lightning and dissolve).
//! Three scripted messages end on a certificate the player signs by voice;
//! signing it shows a shareable score card, and Keep playing carries on for
//! as long as they like.
//!
//! Scripted fixture answers only. Nothing here touches the real first run.

use std::cell::Cell;
use std::time::Duration;

use rune::{
    Anim, Color, Cx, El, Playground, Story, TextStyle, Transition, Tx, col, effect, el, row, text,
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
use super::game_fx::{self, BURN, BURN_MARK, Bolt, Burn, Fx, Mote, STRIKE_MARK, Tones};
use super::landing::{LEAD, keys, title_font};
use super::mic::mic_level;
use super::sparks::impacts;

const STAGE_W: f32 = 880.0;
const STAGE_H: f32 = 620.0;
const HUD_AT: [f32; 2] = [STAGE_W * 0.5, STAGE_H - 34.0];
const FRAME: Duration = Duration::from_millis(16);

/// How long a card takes to drift in from off screen.
const DRIFT: f64 = 1.7;
/// The site's hit-stop: the card freezes while the leader climbs to it.
const HITSTOP: f64 = 0.07;
/// After the last word lands, the card swells into the hit-stop: one beat,
/// not two.
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
            .map_or(arrive + 1.0, |r| (arrive + 1.1).max(r.shatter() + 0.6));
        rounds.push(round(spec, arrive, target, streams));
    }
    // The certificate all but burns away before the score card takes its place.
    let share = rounds[2].shatter() + 0.85;
    if keep {
        let mut arrive = share + SHARE_HOLD;
        let mut target = arrive + 1.0;
        for spec in ENDLESS {
            let r = round(spec, arrive, target, streams);
            arrive = r.hold;
            target = (arrive + 1.1).max(r.shatter() + 0.6);
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

    for (i, r) in s.rounds.iter().enumerate() {
        if let Some(card) = card(p, i, r, t, reduced, reward_fade(&s, i, t)) {
            world = world.child(card);
        }
    }
    world = world.child(words(p, &s, t, reduced));
    // Bolts, sparks and ash, over the cards and under the share card.
    let fx = fx(p, &s, t, reduced);
    if !fx.bolts.is_empty() || !fx.motes.is_empty() {
        world = world.child(
            effect(STRIKE_MARK)
                .key("strike")
                .absolute(0.0, 0.0)
                .size(STAGE_W, STAGE_H),
        );
    }
    game_fx::publish(fx);
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

fn card(p: Paint, i: usize, r: &Round, t: f64, reduced: bool, clear: f32) -> Option<El> {
    if t < r.arrive || t >= r.shatter() + if reduced { 0.3 } else { f64::from(BURN) + 0.05 } {
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
    opacity *= gone * clear;
    let knock = if reduced {
        [0.0, 0.0]
    } else {
        r.flights.iter().fold([0.0, 0.0], |[x, y], (f, _, _)| {
            let [dx, dy] = f.hit(t);
            [x + dx, y + dy]
        })
    }
    .map(|v| v.clamp(-10.0, 10.0));
    // One beat: the card swells and holds that pose through the hit-stop
    // while the leader climbs to it, then the bolt hits and it burns.
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
            // Once the bolt hits, its strokes do the flashing.
            * (1.0 - smooth(0.0, 0.12, (t - r.shatter()) as f32))
    };
    let strike = if reduced {
        0.0
    } else {
        strike_light(seed_of(i), (t - r.explode) as f32)
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
    // Each stroke flashes the card white-hot (on a light page, a tint).
    let strike = strike * if p.k.dark { 1.0 } else { 0.45 };
    let flash_el = (flash.max(strike) > 0.02).then(|| {
        el().key("flash")
            .absolute(0.0, 0.0)
            .size(r.size[0], r.size[1])
            .radius(16.0)
            .continuous_corners()
            .fill(
                ember_tone(p, lerp(0.75, 1.0, strike.min(1.0)))
                    .alpha((flash * 0.35).max(strike * 0.55).min(0.6)),
            )
    });
    // The burn, last in the card so it can eat through everything drawn in
    // it, the shadow included (the bleed).
    let burn = (!reduced && burning(r, t).is_some()).then(|| {
        effect(BURN_MARK + i as u32)
            .key("burn")
            .absolute(0.0, 0.0)
            .size(r.size[0], r.size[1])
            .effect_bleed(40.0)
    });
    // The burn erases the card's own pixels, so the card draws in its own
    // layer while it burns (an opacity short of 1 isolates it).
    if burn.is_some() {
        opacity = opacity.min(0.999);
    }
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
            .children(burn),
    )
}

/// How far the card swells into the hit-stop.
const SWELL: f32 = 1.035;

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

/// What's left of the certificate clears fast once the share card shows.
fn reward_fade(s: &Schedule, ri: usize, t: f64) -> f32 {
    if ri == 2 {
        1.0 - smooth(0.0, 0.14, (t - s.share) as f32)
    } else {
        1.0
    }
}

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

/// The strike, after Psychopomp's lightning: through the hit-stop a stepped
/// leader feels its way up from the HUD to where the last word hit, then
/// return strokes light the channel white-hot and flash the card, each
/// cooling to an afterglow.
const STROKES: u32 = 3;
const LEADER_STEP: f32 = 1.0 / 60.0;
const STROKE_HOLD: f32 = 0.014;
const CORE_DECAY: f32 = 0.03;
const AFTERGLOW_DECAY: f32 = 0.065;
const AFTERGLOW: f32 = 0.6;
/// How far a bolt's midpoints wander, against each segment's length, by
/// subdivision level: coarse bends, then finer and finer kinks.
const ROUGHNESS: [f32; 6] = [0.55, 0.8, 1.0, 1.1, 1.2, 1.25];
/// How much of each coarse level a re-strike redraws: the channel keeps
/// its big bends and gets new fine detail.
const SWAY: [f32; 3] = [0.25, 0.4, 0.6];
/// Ash flakes shed at each card's rim, and sparks at the strike.
const ASH: usize = 150;
const SPARKS: usize = 26;

fn hash(seed: u32, salt: u32) -> f32 {
    let mut h = seed.wrapping_mul(0x9E37_79B1) ^ salt.wrapping_mul(0x85EB_CA77);
    h ^= h >> 15;
    h = h.wrapping_mul(0x2C1B_3C6D);
    h ^= h >> 12;
    h = h.wrapping_mul(0x297A_2D39);
    h ^= h >> 15;
    (h >> 8) as f32 / 16_777_216.0
}

/// When stroke `k` lands, in seconds after the shatter.
fn stroke_at(seed: u32, k: u32) -> f32 {
    (0..k).map(|i| 0.045 + 0.065 * hash(seed, 101 + i)).sum()
}

fn strength(seed: u32, k: u32) -> f32 {
    if k == 0 {
        1.0
    } else {
        0.55 + 0.4 * hash(seed, 211 + k)
    }
}

/// A stroke `since` seconds old fades out over the second half of its glow.
fn rest(since: f32) -> f32 {
    1.0 - smooth(AFTERGLOW * 0.5, AFTERGLOW, since)
}

/// A stroke's white-hot core and its cooling afterglow, `since` seconds in.
fn stroke_light(seed: u32, k: u32, since: f32) -> (f32, f32) {
    if !(0.0..AFTERGLOW).contains(&since) {
        return (0.0, 0.0);
    }
    let decay = if since < STROKE_HOLD {
        1.0
    } else {
        (-(since - STROKE_HOLD) / CORE_DECAY).exp()
    };
    let s = strength(seed, k) * rest(since);
    (s * decay, 0.45 * s * (-since / AFTERGLOW_DECAY).exp())
}

/// How far up the channel the leader has stepped, `0..1`, while it's out.
fn leader(te: f32) -> Option<f32> {
    let length = HITSTOP as f32;
    (0.0..length).contains(&te).then(|| {
        let steps = (length / LEADER_STEP).ceil();
        (((te / LEADER_STEP).floor() + 1.0) / (steps + 1.0)).min(0.95)
    })
}

/// How brightly the strike lights the card at `te` seconds after the
/// hit-stop starts: faint under the leader, flashing on each stroke.
fn strike_light(seed: u32, te: f32) -> f32 {
    if let Some(progress) = leader(te) {
        return 0.25 + 0.2 * progress;
    }
    let since = te - HITSTOP as f32;
    (0..STROKES)
        .map(|k| {
            let (core, glow) = stroke_light(seed, k, since - stroke_at(seed, k));
            core + 0.6 * glow
        })
        .fold(0.0, f32::max)
}

/// A jagged channel along `base` by midpoint displacement. `shape` holds
/// its coarse bends across re-strikes; `strike` redraws its fine detail.
fn jag(
    base: impl Fn(f32) -> [f32; 2],
    levels: u32,
    shape: u32,
    strike: u32,
    amplitude: f32,
) -> Vec<[f32; 2]> {
    let n = 1usize << levels;
    let points: Vec<[f32; 2]> = (0..=n).map(|i| base(i as f32 / n as f32)).collect();
    let deviate = |level: u32, index: u32, salt: u32| {
        let key = index.wrapping_mul(0x9E37_79B1) ^ level.wrapping_mul(0x85EB_CA77) ^ salt;
        let fine = hash(strike ^ key, 0x51ED) * 2.0 - 1.0;
        let fine = fine.signum() * fine.abs().powf(0.7);
        match SWAY.get(level as usize) {
            Some(&sway) => lerp(hash(shape ^ key, 0x2C1B) * 2.0 - 1.0, fine, sway),
            None => fine,
        }
    };
    let mut across = vec![0.0f32; n + 1];
    let mut along = vec![0.0f32; n + 1];
    for level in 0..levels {
        let stride = n >> level;
        let half = stride / 2;
        for segment in 0..n / stride {
            let (start, end) = (segment * stride, segment * stride + stride);
            let reach = amplitude * dist(points[start], points[end]) * ROUGHNESS[level as usize];
            let mid = start + half;
            across[mid] =
                (across[start] + across[end]) * 0.5 + deviate(level, segment as u32, 1) * reach;
            along[mid] = (along[start] + along[end]) * 0.5
                + deviate(level, segment as u32, 2) * reach * 0.22;
        }
    }
    (0..=n)
        .map(|i| {
            let a = points[i.saturating_sub(1)];
            let b = points[(i + 1).min(n)];
            let l = dist(a, b).max(1e-4);
            let t = [(b[0] - a[0]) / l, (b[1] - a[1]) / l];
            [
                points[i][0] - t[1] * across[i] + t[0] * along[i],
                points[i][1] + t[0] * across[i] + t[1] * along[i],
            ]
        })
        .collect()
}

fn dist(a: [f32; 2], b: [f32; 2]) -> f32 {
    (a[0] - b[0]).hypot(a[1] - b[1])
}

fn length(points: &[[f32; 2]]) -> f32 {
    points.windows(2).map(|w| dist(w[0], w[1])).sum()
}

/// The point `f` of the way along a path.
fn along(points: &[[f32; 2]], f: f32) -> [f32; 2] {
    let mut left = length(points) * f.clamp(0.0, 1.0);
    for w in points.windows(2) {
        let d = dist(w[0], w[1]);
        if left <= d && d > 0.0 {
            let u = left / d;
            return [lerp(w[0][0], w[1][0], u), lerp(w[0][1], w[1][1], u)];
        }
        left -= d;
    }
    *points.last().unwrap_or(&[0.0, 0.0])
}

/// A channel and its forks: each fork as its path, where it leaves the
/// channel (a fraction of the channel's length), and its brightness.
struct Channel {
    main: Vec<[f32; 2]>,
    forks: Vec<(Vec<[f32; 2]>, f32, f32)>,
}

fn channel(from: [f32; 2], to: [f32; 2], shape: u32, strike: u32) -> Channel {
    let main = jag(
        |u| [lerp(from[0], to[0], u), lerp(from[1], to[1], u)],
        6,
        shape,
        strike,
        0.3,
    );
    let total = length(&main).max(1.0);
    let heading = [(to[0] - from[0]) / total, (to[1] - from[1]) / total];
    let mut forks = Vec::new();
    for b in 0..(3 + (hash(strike, 301) * 3.0) as u32) {
        let salt = 310 + b * 13;
        let fork = 0.1 + 0.7 * hash(strike, salt).powf(1.3);
        let side = if hash(strike, salt + 1) < 0.5 {
            -1.0
        } else {
            1.0
        };
        let turn = side * (0.35 + 0.5 * hash(strike, salt + 2));
        let (s, c) = turn.sin_cos();
        let d = [
            c * heading[0] - s * heading[1],
            s * heading[0] + c * heading[1],
        ];
        let reach = total * (0.12 + 0.22 * hash(strike, salt + 3)) * (1.0 - 0.5 * fork);
        let start = along(&main, fork);
        let twig = jag(
            |u| [start[0] + d[0] * reach * u, start[1] + d[1] * reach * u],
            4,
            shape ^ salt,
            strike ^ salt,
            0.34,
        );
        forks.push((twig, fork, 0.4 + 0.25 * hash(strike, salt + 5)));
    }
    Channel { main, forks }
}

/// A card point on the stage: cards swell about their centre.
fn on_stage(r: &Round, local: [f32; 2], scale: f32) -> [f32; 2] {
    let c = [r.size[0] * 0.5, r.size[1] * 0.5];
    [
        r.spec.home[0] + c[0] + (local[0] - c[0]) * scale,
        r.spec.home[1] + c[1] + (local[1] - c[1]) * scale,
    ]
}

fn seed_of(ri: usize) -> u32 {
    (ri * 7919 + 13) as u32
}

/// Whether a card is still burning at `t`, and how far in (seconds).
fn burning(r: &Round, t: f64) -> Option<f32> {
    let age = (t - r.shatter()) as f32;
    (0.0..BURN + 0.05).contains(&age).then_some(age)
}

/// The strike and burn's colours: a white-hot core and an ember corona on a
/// dark page; on a light one, deep ember paint and a brown char.
fn tones(p: Paint) -> Tones {
    let k = p.k;
    if k.dark {
        Tones {
            core: k.c.text_strong,
            corona: ember_tone(p, 0.55),
            scorch: k.c.black.mix(ember_tone(p, 0.2), 0.15),
            ash: k.c.text_weak.alpha(0.85),
        }
    } else {
        Tones {
            core: ember_tone(p, 1.0),
            corona: ember_tone(p, 0.6),
            scorch: k.c.black.mix(ember_tone(p, 0.9), 0.3),
            ash: k.c.text_muted.alpha(0.8),
        }
    }
}

/// This frame's strike and burn for the compositor (`game_fx`).
fn fx(p: Paint, s: &Schedule, t: f64, reduced: bool) -> Fx {
    let mut fx = Fx {
        dark: p.k.dark,
        tones: tones(p),
        ..Fx::default()
    };
    if reduced {
        return fx;
    }
    let source = [HUD_AT[0], HUD_AT[1] - 15.0];
    for (ri, r) in s.rounds.iter().enumerate() {
        let seed = seed_of(ri);
        let clear = reward_fade(s, ri, t);
        let impact = on_stage(r, r.impact(), SWELL);
        let te = (t - r.explode) as f32;
        if (0.0..HITSTOP as f32 + 1.0).contains(&te) && clear > 0.0 {
            bolts(&mut fx, seed, source, impact, te, clear);
        }
        if let Some(age) = burning(r, t) {
            fx.burns.push(Burn {
                card: ri,
                size: r.size,
                impact: r.impact(),
                age,
                seed,
            });
        }
        let age = (t - r.shatter()) as f32;
        if (0.0..BURN + 1.6).contains(&age) && clear > 0.0 {
            ash(&mut fx, r, seed, age, clear);
            sparks(&mut fx, seed, impact, source, age, clear);
        }
    }
    fx
}

fn bolts(fx: &mut Fx, seed: u32, from: [f32; 2], to: [f32; 2], te: f32, clear: f32) {
    let mut push = |c: &Channel, core: f32, glow: f32, reach: f32| {
        if core + glow < 0.01 {
            return;
        }
        let total = length(&c.main);
        fx.bolts.push(Bolt {
            points: c.main.clone(),
            width: 1.3,
            core: core * clear,
            glow: glow * clear,
            drawn: total * reach,
        });
        for (twig, fork, energy) in &c.forks {
            if reach <= *fork {
                continue;
            }
            fx.bolts.push(Bolt {
                points: twig.clone(),
                width: 0.75,
                core: core * energy * clear,
                glow: glow * energy * clear,
                drawn: (reach - fork) * total,
            });
        }
    };
    if let Some(progress) = leader(te) {
        let c = channel(from, to, seed, hash(seed, 400).to_bits());
        push(&c, 0.0, 0.3 + 0.25 * progress, progress);
        return;
    }
    let since = te - HITSTOP as f32;
    for k in 0..STROKES {
        let (core, glow) = stroke_light(seed, k, since - stroke_at(seed, k));
        let c = channel(from, to, seed, hash(seed, 401 + k).to_bits());
        push(&c, core, glow, 1.0);
    }
}

/// Flakes leave the rim as the front passes their spot, glowing, and drift
/// up as grey ash.
fn ash(fx: &mut Fx, r: &Round, seed: u32, age: f32, clear: f32) {
    let impact = r.impact();
    for n in 0..ASH as u32 {
        let local = [
            hash(seed ^ 0xA5, n * 3) * r.size[0],
            hash(seed ^ 0xA5, n * 3 + 1) * r.size[1],
        ];
        let born = game_fx::passes(game_fx::field(local, r.size, impact, seed));
        let life = lerp(0.6, 1.3, hash(seed, n * 7 + 2));
        let a = age - born;
        if !(0.0..life).contains(&a) {
            continue;
        }
        let out = [local[0] - impact[0], local[1] - impact[1]];
        let l = (out[0].hypot(out[1])).max(1.0);
        let push = lerp(15.0, 60.0, hash(seed, n * 7 + 3));
        let v = [
            out[0] / l * push + lerp(-25.0, 25.0, hash(seed, n * 7 + 4)),
            out[1] / l * push - lerp(30.0, 110.0, hash(seed, n * 7 + 5)),
        ];
        let (at, _) = ballistic(on_stage(r, local, SWELL), v, 1.4, -50.0, a);
        let size = lerp(0.8, 2.2, hash(seed, n * 7 + 6).powi(2));
        fx.motes.push(Mote {
            at,
            radius: size * (1.0 - 0.3 * a / life),
            heat: (-a / 0.16).exp(),
            opacity: (1.0 - smooth(life * 0.45, life, a)) * clear,
        });
    }
}

/// The strike spits sparks from where it hit, away from the channel.
fn sparks(fx: &mut Fx, seed: u32, at: [f32; 2], from: [f32; 2], age: f32, clear: f32) {
    let back = [from[0] - at[0], from[1] - at[1]];
    let away = (-back[1]).atan2(-back[0]);
    for n in 0..SPARKS as u32 {
        let life = lerp(0.2, 0.55, hash(seed ^ 0x5A, n * 5));
        if age >= life {
            continue;
        }
        let angle = away + lerp(-1.7, 1.7, hash(seed ^ 0x5A, n * 5 + 1));
        let speed = lerp(260.0, 900.0, hash(seed ^ 0x5A, n * 5 + 2));
        let v = [angle.cos() * speed, angle.sin() * speed];
        let (p, _) = ballistic(at, v, 4.2, 380.0, age);
        fx.motes.push(Mote {
            at: p,
            radius: lerp(0.8, 1.5, hash(seed ^ 0x5A, n * 5 + 3)),
            heat: 1.0 - smooth(life * 0.4, life, age),
            opacity: (1.0 - smooth(life * 0.6, life, age)) * clear,
        });
    }
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
    fn the_leader_climbs_then_strokes_flash_and_fade() {
        let seed = seed_of(1);
        let climb: Vec<f32> = (0..4).map(|i| leader(i as f32 * 0.017).unwrap()).collect();
        assert!(climb.windows(2).all(|w| w[1] >= w[0]), "{climb:?}");
        assert!(leader(HITSTOP as f32).is_none());
        let hit = HITSTOP as f32;
        assert!(
            strike_light(seed, hit) > 0.95,
            "the first stroke is full strength"
        );
        for k in 1..STROKES {
            let at = hit + stroke_at(seed, k);
            assert!(at > hit + stroke_at(seed, k - 1) + 0.04);
            assert!(strike_light(seed, at) > 0.5, "stroke {k} flashes");
        }
        assert!(strike_light(seed, hit + stroke_at(seed, STROKES - 1) + AFTERGLOW) < 1e-3);
    }

    #[test]
    fn a_bolt_runs_from_the_hud_to_the_card() {
        let (from, to) = ([440.0, 571.0], [300.0, 220.0]);
        for k in 0..STROKES {
            let c = channel(from, to, 7, hash(7, 401 + k).to_bits());
            assert_eq!(c.main.len(), 65);
            assert!(dist(c.main[0], from) < 1e-3 && dist(c.main[64], to) < 1e-3);
            assert!(length(&c.main) < dist(from, to) * 2.5);
            assert!(c.forks.len() >= 3);
        }
    }

    #[test]
    fn every_card_burns_away_and_sheds_ash() {
        let s = schedule(true, true);
        for (ri, r) in s.rounds.iter().enumerate() {
            let born = |t: f64| {
                let mut fx = Fx::default();
                ash(&mut fx, r, seed_of(ri), (t - r.shatter()) as f32, 1.0);
                fx.motes.len()
            };
            assert!(born(r.shatter() + 0.4) > 10, "ash leaves the rim");
            assert!(burning(r, r.shatter() + f64::from(BURN)).is_some());
            assert!(burning(r, r.shatter() + f64::from(BURN) + 0.1).is_none());
            assert!(card(paint(), ri, r, r.shatter() + 0.5, false, 1.0).is_some());
            assert!(card(paint(), ri, r, r.shatter() + 1.1, false, 1.0).is_none());
        }
    }

    #[test]
    fn reduce_motion_has_no_strike() {
        let s = schedule(true, false);
        let t = s.rounds[0].shatter() + 0.2;
        let fx = fx(paint(), &s, t, true);
        assert!(fx.bolts.is_empty() && fx.motes.is_empty() && fx.burns.is_empty());
        let fx = super::fx(paint(), &s, t, false);
        assert!(!fx.motes.is_empty() && fx.burns.len() == 1);
    }

    fn paint() -> Paint {
        Paint::new(true, 2.0)
    }
}
