//! The practice step as a mini game (`t1121`). Messages drift in, one is
//! lit at a time, the player holds the shortcut and answers it, the words
//! fly into it, and a bolt of lightning strikes it and burns it away
//! (`game_fx`, after Psychopomp's lightning and dissolve). Three messages
//! end on a certificate the player signs by saying their name; signing it
//! shows a score card, and Keep playing carries on for as long as they like.
//! Done is always there, and the game never ends by itself.
//!
//! [`Play`] is the game, driven by dictation: the live words a streaming
//! model sends while the shortcut is held, then the paste, which is the
//! answer. The first run's `"Your turn"` feeds it Hex's own dictation; the
//! playground story feeds it scripted answers.

use std::cell::Cell;
use std::rc::Rc;
use std::time::Duration;

use rune::{
    Anim, Color, Cx, El, Playground, Story, TextEdit, TextStyle, Transition, Tx, col, effect, el,
    row, text,
};
use rune_kit::{Button, ButtonVariant, TextInput};

use super::super::Ui;
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
use super::live;
use super::mic::mic_level;
use super::sparks::impacts;
use super::stories::{Onboard, physical_shortcut_held};
use crate::onboarding::Practice;

const STAGE_W: f32 = 880.0;
const STAGE_H: f32 = 620.0;
const HUD_AT: [f32; 2] = [STAGE_W * 0.5, STAGE_H - 34.0];
const FRAME: Duration = Duration::from_millis(16);

/// When the first card shows, and when it lights.
const FIRST_ARRIVES: f64 = 0.3;
const FIRST_LIGHTS: f64 = 1.0;
/// How long a card takes to drift in from off screen.
const DRIFT: f64 = 1.7;
/// The site's hit-stop: the card freezes while the leader climbs to it.
const HITSTOP: f64 = 0.07;
/// After the last word lands, the card swells into the hit-stop: one beat,
/// not two.
const AFTER_LAND: f64 = 0.12;
/// Seconds between live words leaving the HUD, and between pasted ones.
const STAGGER: f64 = 0.04;
const BURST: f64 = 0.06;
/// How long live words wait for their paste once the dictation ends.
const WAIT: f64 = 8.0;
/// The most lines an answer takes; the rest of a long one is left out.
const LINES: usize = 3;
/// Burning cards take turns at this many compositor slots.
const SLOTS: usize = 200;
/// The story's Keep playing variant presses it after this long.
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
    /// The story's answer.
    demo: &'static Take,
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
        demo: &SAY_MILK,
        heat: 0.5,
        home: [LEFT, 128.0],
        from: [-520.0, 40.0],
    },
    Spec {
        card: Card::Chat(GREG),
        demo: &SAY_GREG,
        heat: 0.92,
        home: [RIGHT, 218.0],
        from: [520.0, -30.0],
    },
    Spec {
        card: Card::Certificate,
        demo: &SAY_NAME,
        heat: 0.62,
        home: [LEFT, 318.0],
        from: [-80.0, 420.0],
    },
];
const CERTIFICATE: usize = SCRIPT.len() - 1;

/// Keep playing's messages, round and round, each in the other column
/// from the one before.
const ENDLESS: [Spec; 4] = [
    Spec {
        card: Card::Chat(FRANK),
        demo: &SAY_FRANK,
        heat: 0.55,
        home: [RIGHT, 140.0],
        from: [500.0, -60.0],
    },
    Spec {
        card: Card::Chat(DAX),
        demo: &SAY_DAX,
        heat: 0.6,
        home: [LEFT, 260.0],
        from: [-520.0, 0.0],
    },
    Spec {
        card: Card::Chat(ADAM),
        demo: &SAY_ADAM,
        heat: 0.75,
        home: [RIGHT, 330.0],
        from: [480.0, 300.0],
    },
    Spec {
        card: Card::Chat(GREG),
        demo: &SAY_GREG,
        heat: 0.92,
        home: [LEFT, 128.0],
        from: [-520.0, -30.0],
    },
];

fn spec(n: usize) -> Spec {
    SCRIPT
        .get(n)
        .copied()
        .unwrap_or_else(|| ENDLESS[(n - SCRIPT.len()) % ENDLESS.len()])
}

/// One card, from drifting in to burning away.
struct Round {
    /// Which card this is, counting from the first.
    n: usize,
    spec: Spec,
    arrive: f64,
    /// When it lights, once that's known.
    target: Option<f64>,
    /// The answer as heard so far, then as pasted.
    text: String,
    /// When each word of `text` leaves the HUD.
    launch: Vec<f64>,
    answered: bool,
    /// When the strike's leader sets off: never, until it's answered.
    explode: f64,
}

impl Round {
    fn new(n: usize, arrive: f64) -> Self {
        Self {
            n,
            spec: spec(n),
            arrive,
            target: None,
            text: String::new(),
            launch: Vec::new(),
            answered: false,
            explode: f64::INFINITY,
        }
    }

    fn lit_at(&self) -> f64 {
        self.target.unwrap_or(f64::INFINITY)
    }

    fn shatter(&self) -> f64 {
        self.explode + HITSTOP
    }

    fn size(&self) -> [f32; 2] {
        field_of(self).2
    }

    fn centre(&self) -> [f32; 2] {
        let size = self.size();
        [
            self.spec.home[0] + size[0] * 0.5,
            self.spec.home[1] + size[1] * 0.5,
        ]
    }

    /// The compositor slot it burns in.
    fn slot(&self) -> usize {
        self.n % SLOTS
    }

    /// Takes `text` as the answer so far. Words already on their way keep
    /// going; new ones leave the HUD `gap` apart.
    fn set(&mut self, text: &str, now: f64, gap: f64) {
        self.text = fit(self.spec.card, text);
        let count = self.text.split_whitespace().count();
        self.launch.truncate(count);
        while self.launch.len() < count {
            let at = self.launch.last().map_or(now, |last| (last + gap).max(now));
            self.launch.push(at);
        }
    }

    fn flights(&self) -> Vec<(Flight, &str, [f32; 2])> {
        let (field, origin, _) = field_of(self);
        field
            .slots(&self.text)
            .into_iter()
            .zip(&self.launch)
            .enumerate()
            .map(|(i, (slot, &at))| {
                let to = [origin[0] + slot.centre[0], origin[1] + slot.centre[1]];
                let flight = Flight::new(HUD_AT, to, at, self.spec.heat, field.font.size, i);
                (flight, slot.word, slot.centre)
            })
            .collect()
    }

    fn heading(&self) -> [f32; 2] {
        self.flights()
            .last()
            .map_or([0.0, -1.0], |(f, _, _)| f.heading())
    }

    /// When the last word lands and the card swells.
    fn charge(&self) -> f64 {
        self.explode - AFTER_LAND
    }

    /// Where the last word hits, in card coordinates: the strike lands here.
    fn impact(&self) -> [f32; 2] {
        let (_, origin, size) = field_of(self);
        self.flights()
            .last()
            .map_or([size[0] * 0.5, size[1] * 0.5], |(_, _, c)| {
                [
                    origin[0] + c[0] - self.spec.home[0],
                    origin[1] + c[1] - self.spec.home[1],
                ]
            })
    }
}

/// `text` on one line of single spaces, cut to the words its field shows.
fn fit(card: Card, text: &str) -> String {
    let text = text.split_whitespace().collect::<Vec<_>>().join(" ");
    let (field, lines) = match card {
        Card::Chat(_) => (CARD, LINES),
        Card::Certificate => (SIGN, 1),
    };
    field
        .slots(&text)
        .iter()
        .take_while(|slot| slot.line < lines)
        .map(|slot| slot.word)
        .collect::<Vec<_>>()
        .join(" ")
}

fn field_of(r: &Round) -> (Field, [f32; 2], [f32; 2]) {
    let [x, y] = r.spec.home;
    match r.spec.card {
        Card::Chat(n) => {
            let lines = CARD.wrap(&r.text).len().max(1);
            let h = lines_card_height(&n, lines);
            (CARD, field_text_origin([x, y + h], lines), [CARD_W, h])
        }
        Card::Certificate => (SIGN, [x + CERT_PAD, y + SIGN_Y], [CARD_W, CERT_H]),
    }
}

/// The game, on the caller's clock.
pub(crate) struct Play {
    rounds: Vec<Round>,
    /// Cards shown so far, including burnt ones dropped from `rounds`.
    spawned: usize,
    dropped: usize,
    holding: bool,
    /// When the first answer started.
    first_hold: Option<f64>,
    /// When the dictation that sent the live words ended, until its paste.
    ended: Option<f64>,
    /// When the score card shows.
    share: Option<f64>,
    /// When Keep playing was pressed.
    kept: Option<f64>,
    /// Keys typed into the answer field.
    keys: usize,
}

impl Play {
    pub(crate) fn new(now: f64) -> Self {
        let mut play = Self {
            rounds: Vec::new(),
            spawned: 0,
            dropped: 0,
            holding: false,
            first_hold: None,
            ended: None,
            share: None,
            kept: None,
            keys: 0,
        };
        play.spawn(now + FIRST_ARRIVES);
        play.rounds[0].target = Some(now + FIRST_ARRIVES + FIRST_LIGHTS);
        play
    }

    /// The next card drifts in; after the certificate, only on Keep playing.
    fn spawn(&mut self, at: f64) {
        if self.spawned >= SCRIPT.len() && self.kept.is_none() {
            return;
        }
        self.rounds.push(Round::new(self.spawned, at));
        self.spawned += 1;
    }

    /// The score card waits for Keep playing or Continue.
    fn playing(&self) -> bool {
        self.share.is_none() || self.kept.is_some()
    }

    fn waiting(&self) -> usize {
        self.rounds.iter().filter(|r| !r.answered).count()
    }

    /// The card the next answer goes to, lit now if it wasn't yet.
    fn answering(&mut self, now: f64) -> Option<&mut Round> {
        if !self.playing() {
            return None;
        }
        self.first_hold.get_or_insert(now);
        let round = self.rounds.iter_mut().find(|r| !r.answered)?;
        if round.target.is_none_or(|at| at > now) {
            round.target = Some(now);
        }
        Some(round)
    }

    /// This frame's dictation: whether it records, and the words a model
    /// that streams has heard so far (`None` once it ends, or for one that
    /// doesn't stream).
    pub(crate) fn hear(&mut self, held: bool, words: Option<&str>, now: f64) {
        if held && !self.holding && self.playing() {
            // The next card comes in while this one is answered.
            if self.answering(now).is_some() && self.waiting() == 1 {
                self.spawn(now);
            }
        }
        self.holding = held;
        match words.filter(|words| !words.trim().is_empty()) {
            Some(words) => {
                self.ended = None;
                if let Some(round) = self.answering(now) {
                    round.set(words, now, STAGGER);
                }
            }
            None if !held
                && self
                    .rounds
                    .iter()
                    .any(|r| !r.answered && !r.text.is_empty()) =>
            {
                let ended = *self.ended.get_or_insert(now);
                if now - ended > WAIT {
                    self.forget();
                }
            }
            None => {}
        }
        // Keep playing goes on for good; cards long burnt stop counting.
        let before = self.rounds.len();
        self.rounds
            .retain(|r| r.n < SCRIPT.len() || now < r.shatter() + 3.0);
        self.dropped += before - self.rounds.len();
    }

    /// The dictation's paste: the lit card's answer, which it burns for.
    pub(crate) fn paste(&mut self, text: &str, now: f64) {
        self.ended = None;
        let Some(round) = self.answering(now) else {
            return;
        };
        round.set(text, now, BURST);
        if round.text.is_empty() {
            return;
        }
        round.answered = true;
        let landed = round
            .flights()
            .iter()
            .map(|(f, _, _)| f.arrives())
            .fold(now, f64::max);
        round.explode = landed + AFTER_LAND;
        let (n, shatter) = (round.n, round.shatter());
        if n == CERTIFICATE {
            // The certificate all but burns away before the score card takes its place.
            self.share = Some(shatter + 0.85);
            return;
        }
        if self.waiting() == 0 {
            self.spawn(now);
        }
        if let Some(next) = self.rounds.iter_mut().find(|r| !r.answered)
            && next.target.is_none()
        {
            next.target = Some((next.arrive + 1.1).max(shatter + 0.6));
        }
    }

    /// Drops the live words of a dictation that never pasted.
    pub(crate) fn forget(&mut self) {
        self.ended = None;
        for round in self.rounds.iter_mut().filter(|r| !r.answered) {
            round.text.clear();
            round.launch.clear();
        }
    }

    /// Folds the score card away and brings more messages.
    pub(crate) fn keep(&mut self, now: f64) {
        if self.share.is_none() || self.kept.is_some() {
            return;
        }
        self.kept = Some(now);
        self.spawn(now);
        if let Some(round) = self.rounds.last_mut() {
            round.target = Some(now + FIRST_LIGHTS);
        }
    }

    fn destroyed(&self, t: f64) -> usize {
        self.dropped + self.rounds.iter().filter(|r| t >= r.shatter()).count()
    }

    fn certificate(&self) -> Option<&Round> {
        self.rounds.iter().find(|r| r.n == CERTIFICATE)
    }

    /// The score card's numbers: words said up to the certificate, keys
    /// typed, and seconds from the first press to the certificate's burn.
    fn stats(&self) -> (usize, usize, f64) {
        let words = self
            .rounds
            .iter()
            .filter(|r| r.answered && r.n <= CERTIFICATE)
            .map(|r| r.text.split_whitespace().count())
            .sum();
        let secs = match (self.first_hold, self.certificate()) {
            (Some(from), Some(cert)) => (cert.shatter() - from).max(0.0),
            _ => 0.0,
        };
        (words, self.keys, secs)
    }
}

/// What the game shows around [`Play`], and what its buttons do.
struct Look<'a, S> {
    caps: &'a [String],
    /// The microphone's level while dictation records.
    meter: Option<(f32, f32)>,
    /// What the last try needs said.
    hint: Option<&'static str>,
    done: Rc<dyn Fn(&mut S)>,
    keep: Rc<dyn Fn(&mut S)>,
}

fn act<S: 'static>(f: &Rc<dyn Fn(&mut S)>) -> impl Fn(&mut S) + 'static {
    let f = f.clone();
    move |s: &mut S| f(s)
}

/// The script's dictation: it answers each card soon after it lights, as a
/// player would, with the card's own answer.
#[derive(Default)]
struct Script {
    /// Whether the model sends live words while the shortcut is held.
    streams: bool,
    /// Presses Keep playing once the score card has shown a while.
    keep: bool,
    /// Which card, and when its shortcut went down.
    held: Option<(usize, f64)>,
    meter: Option<(f32, f32)>,
}

impl Script {
    fn new(streams: bool) -> Self {
        Self {
            streams,
            ..Self::default()
        }
    }

    fn run(&mut self, play: &mut Play, now: f64) {
        if self.keep && play.share.is_some_and(|at| now >= at + SHARE_HOLD) {
            play.keep(now);
        }
        self.meter = None;
        let next = play
            .rounds
            .iter()
            .find(|r| !r.answered)
            .map(|r| (r.n, r.lit_at(), r.spec.demo));
        let Some((n, lit, take)) = next.filter(|_| play.playing()) else {
            play.hear(false, None, now);
            return;
        };
        match self.held.filter(|(held, _)| *held == n) {
            None if now >= lit + 0.55 => {
                self.held = Some((n, now));
                play.hear(true, None, now);
            }
            None => play.hear(false, None, now),
            Some((_, from)) if now - from < take.release => {
                self.meter = Some(take.meter(now - from));
                let words = self.streams.then(|| take.partial(now - from));
                play.hear(true, words, now);
            }
            Some(_) => {
                play.hear(false, None, now);
                play.paste(take.text(), now);
            }
        }
    }
}

/// The game on the first run's `"Your turn"`.
pub(crate) struct Session {
    play: Play,
    /// A story's stand-in for Hex's dictation.
    script: Option<Script>,
    /// What's typed into the answer field so far, which it shows as live
    /// words; Return or a paste sends it.
    typed: String,
    practice: Option<Practice>,
}

impl Session {
    fn new(s: &Onboard, now: f64) -> Self {
        Self {
            play: Play::new(now),
            script: s.rehearsal.map(|r| Script::new(r.streams)),
            typed: String::new(),
            practice: None,
        }
    }
}

/// Whether `"Your turn"` is the game rather than the plain practice field.
fn plays(s: &Onboard) -> bool {
    s.interactive_try() && !s.practice_field
}

/// Moves the game on `"Your turn"` to `now`: Hex's dictation, or a story's
/// script, answers the lit card. False when the step is the plain practice
/// field instead.
pub(super) fn step(s: &mut Onboard, now: f64) -> bool {
    if !plays(s) {
        s.game = None;
        return false;
    }
    let mut session = s.game.take().unwrap_or_else(|| Session::new(s, now));
    if let Some(script) = session.script.as_mut() {
        script.run(&mut session.play, now);
        s.game = Some(session);
        return true;
    }
    // Hex's own dictation sounds its own tones; without it, the story
    // follows the physical shortcut and sounds them itself.
    let real = s.live.as_ref().map(|live| live.listening);
    let held = real.unwrap_or_else(|| physical_shortcut_held(&s.caps));
    match (s.live_hold_from, held) {
        (None, true) => {
            s.live_hold_from = Some(now);
            // Hex pastes where the focus is when the dictation ends.
            s.practice.focused = true;
            if real.is_none() {
                s.sfx.on_live_shortcut_down(now);
            }
        }
        (Some(_), false) => {
            s.live_hold_from = None;
            if real.is_none() {
                s.sfx.on_live_shortcut_up(now);
            }
        }
        _ => {}
    }
    // A try Hex couldn't use drops the words it sent.
    let practice = s.live.as_ref().map(|live| live.practice);
    if practice != session.practice
        && matches!(
            practice,
            Some(Practice::TooShort | Practice::NothingHeard | Practice::Failed)
        )
    {
        session.play.forget();
    }
    session.practice = practice;
    let partial = crate::dictation::live_text::current().filter(|_| held);
    let words = match &partial {
        Some(partial) if !partial.stable.trim().is_empty() => Some(partial.stable.as_str()),
        _ => Some(session.typed.as_str()).filter(|typed| !typed.trim().is_empty()),
    };
    session.play.hear(held, words, now);
    s.game = Some(session);
    true
}

/// The answer field changed: a paste answers the lit card, typing shows
/// as live words until Return. False without the game.
pub(super) fn changed(s: &mut Onboard) -> bool {
    let Some(session) = s.game.as_mut() else {
        return false;
    };
    // A story's script is the player; keys meant for the story don't count.
    if session.script.is_some() {
        s.practice = TextEdit::default();
        s.practice.focused = true;
        return true;
    }
    let count = s.practice.text.chars().count();
    let before = session.typed.chars().count();
    // A paste lands all at once; typing one key at a time.
    if count > before + 1 {
        answer(s);
    } else {
        session.play.keys += count.saturating_sub(before);
        session.typed.clone_from(&s.practice.text);
    }
    true
}

/// Return in the answer field sends what's typed.
pub(super) fn answer(s: &mut Onboard) {
    let now = s.now;
    let Some(session) = s.game.as_mut() else {
        return;
    };
    let text = std::mem::take(&mut s.practice.text);
    session.typed.clear();
    s.practice = TextEdit::default();
    s.practice.focused = true;
    if text.trim().is_empty() {
        return;
    }
    session.play.paste(&text, now);
    s.sfx.on_live_dictation_landed(now);
}

fn keep(s: &mut Onboard) {
    let now = s.now;
    if let Some(session) = s.game.as_mut() {
        session.play.keep(now);
    }
    s.practice.focused = true;
}

/// The first run's `"Your turn"` as the game, or `None` for the plain
/// practice field.
pub(super) fn page(p: Paint, s: &Onboard) -> Option<El> {
    if !plays(s) {
        return None;
    }
    // The game starts on the step's first poll; until then, as it will.
    let fresh;
    let session = match &s.game {
        Some(session) => session,
        None => {
            fresh = Session::new(s, s.now);
            &fresh
        }
    };
    let meter = match &session.script {
        Some(script) => script.meter,
        None => s.live_hold_from.is_some().then(|| {
            let level = crate::audio::input_level::recent().unwrap_or(0.0);
            (level * 0.6, level)
        }),
    };
    let hint = s.live.as_ref().and_then(live::hint_copy);
    let (stage, field) = if s.live.is_some() {
        let look = Look::<Ui> {
            caps: &s.caps,
            meter,
            hint,
            done: Rc::new(live::on_ui(Onboard::jump_to_ready)),
            keep: Rc::new(live::on_ui(keep)),
        };
        let field = TextInput::new(&s.practice)
            .on_edit(live::practice)
            .on_change(live::practice_changed)
            .on_submit(|ui: &mut Ui, _: &mut Tx| live::on_ui(answer)(ui));
        (
            render(p, &session.play, s.now, s.reduced.get(), &look),
            field,
        )
    } else {
        let look = Look::<Onboard> {
            caps: &s.caps,
            meter,
            hint,
            done: Rc::new(Onboard::jump_to_ready),
            keep: Rc::new(keep),
        };
        let field = TextInput::new(&s.practice)
            .on_edit(|s: &mut Onboard| &mut s.practice)
            .on_change(Onboard::on_practice_change)
            .on_submit(|s: &mut Onboard, _: &mut Tx| answer(s));
        (
            render(p, &session.play, s.now, s.reduced.get(), &look),
            field,
        )
    };
    // The answer field takes Hex's paste (and typing) out of sight: it
    // keeps its size, so it can hold the focus, in a clip too small to
    // show any of it.
    let field = el()
        .key("answer")
        .absolute(0.0, 0.0)
        .size(1.0, 1.0)
        .clip_rounded(0.0)
        .child(
            field
                .placeholder("Answer")
                .build(p.k)
                .key("answer-field")
                .w(240.0)
                .offset(2.0, 2.0),
        );
    Some(
        el().key("game-page")
            .size(STAGE_W, STAGE_H)
            .child(field)
            .child(stage),
    )
}

/// The playground's game, answered by a script.
pub(crate) struct Game {
    play: Play,
    script: Script,
    now: f64,
    started: bool,
    force_reduced: bool,
    reduced: Cell<bool>,
}

impl Game {
    fn new() -> Self {
        Self {
            play: Play::new(0.0),
            script: Script::new(true),
            now: 0.0,
            started: false,
            force_reduced: false,
            reduced: Cell::new(false),
        }
    }

    fn restart(&mut self) {
        self.play = Play::new(self.now);
        self.script = Script {
            keep: self.script.keep,
            ..Script::new(self.script.streams)
        };
    }
}

pub(crate) fn add<S: 'static>(playground: Playground<S>) -> Playground<S> {
    playground.story(
        Story::new("Prototypes/Onboarding/Practice game", Game::new, view)
            .poll(poll)
            .trigger('r', "Play", |g, play| {
                g.now = play.now();
                g.restart();
                play.wake_after(Duration::ZERO);
            })
            .variant("Streaming model", |_| {})
            .variant("Model that doesn't stream", |g| g.script.streams = false)
            .variant("Keep playing", |g| g.script.keep = true)
            .variant("Reduce Motion", |g| g.force_reduced = true),
    )
}

fn poll(g: &mut Game, now: f64, tx: &mut Tx) -> bool {
    g.now = now;
    // Plays on open, so headless renders and recordings start at once.
    if !g.started {
        g.started = true;
        g.restart();
    }
    g.script.run(&mut g.play, now);
    tx.wake_after(FRAME);
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
    let caps = ["⌥".to_string()];
    let stage = render(
        p,
        &g.play,
        g.now,
        g.reduced.get(),
        &Look::<Game> {
            caps: &caps,
            meter: g.script.meter,
            hint: None,
            done: Rc::new(Game::restart),
            keep: Rc::new(|g: &mut Game| g.play.keep(g.now)),
        },
    );
    el().key("game").w_full().h_full().center().child(stage)
}

/// The game at `t`, on its stage.
fn render<S: 'static>(p: Paint, g: &Play, t: f64, reduced: bool, look: &Look<'_, S>) -> El {
    let destroyed = g.destroyed(t);
    let (shake, tilt) = if reduced {
        ([0.0, 0.0], 0.0)
    } else {
        shake(g, t)
    };

    let mut world = el()
        .key("world")
        .size(STAGE_W, STAGE_H)
        .offset(shake[0], shake[1])
        .rotate(tilt)
        .offset_motion(Anim::INSTANT)
        .rotate_motion(Anim::INSTANT)
        .child(header(p, g, destroyed, t, reduced, look))
        .child(hud(p, g, t, look.meter))
        .children(look.hint.map(|copy| hint(p, copy)));

    for r in &g.rounds {
        if let Some(card) = card(p, r, t, reduced, reward_fade(g, r, t)) {
            world = world.child(card);
        }
    }
    world = world.child(words(p, g, t, reduced));
    // Bolts, sparks and ash, over the cards and under the share card.
    let fx = fx(p, g, t, reduced);
    if !fx.bolts.is_empty() || !fx.motes.is_empty() {
        world = world.child(
            effect(STRIKE_MARK)
                .key("strike")
                .absolute(0.0, 0.0)
                .size(STAGE_W, STAGE_H),
        );
    }
    game_fx::publish(fx);
    world = world.children(share(p, g, t, destroyed, reduced, look));

    el().key("stage")
        .size(STAGE_W, STAGE_H)
        .instant_changes()
        .child(world)
}

/// Screen shake: the site's camera trauma (`cam.trauma += 0.32 · I`),
/// squared into a noisy offset, plus a kick along the last word's heading.
fn shake(g: &Play, t: f64) -> ([f32; 2], f32) {
    let mut trauma = 0.0f32;
    let mut kick = [0.0f32, 0.0];
    for r in &g.rounds {
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
fn chrome(g: &Play, t: f64) -> f32 {
    let Some(share) = g.share else {
        return 1.0;
    };
    let back = g
        .kept
        .map_or(0.0, |kept| smooth(0.0, 0.18, (t - kept) as f32));
    1.0 - smooth(0.0, 0.18, (t - share) as f32) + back
}

/// Keep playing's fold, `0..1` from the share card to the score pill.
fn fold(g: &Play, t: f64) -> f32 {
    g.kept
        .map_or(0.0, |kept| ((t - kept) as f32 / FOLD).clamp(0.0, 1.0))
}

/// The pill takes over in the fold's last 80 ms (Reduce Motion: a crossfade).
fn pill_in(fold: f32, reduced: bool) -> f32 {
    if reduced {
        smooth(0.0, 0.3 / FOLD, fold)
    } else {
        smooth(1.0 - 0.08 / FOLD, 1.0, fold)
    }
}

fn header<S: 'static>(
    p: Paint,
    g: &Play,
    destroyed: usize,
    t: f64,
    reduced: bool,
    look: &Look<'_, S>,
) -> El {
    let k = p.k;
    let chrome = chrome(g, t);
    // The count kicks up as each card goes.
    let pop = if reduced {
        0.0
    } else {
        g.rounds
            .iter()
            .map(|r| (t - r.shatter()) as f32)
            .filter(|te| *te >= 0.0)
            .fold(0.0f32, |_, te| {
                (-te * 7.0).exp() * (te * 22.0).cos().max(0.0)
            })
    };
    let fold = fold(g, t);
    // The pill steps aside for the share card, and the share card folds back into it.
    let pill = match g.share {
        Some(share) if t >= share && fold > 0.0 => pill_in(fold, reduced),
        Some(share) if t >= share => 1.0 - smooth(0.0, 0.18, (t - share) as f32),
        _ => 1.0,
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
                .child(keys(p, "keys", look.caps, g.holding))
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
                    .on_press(act(&look.done))
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

/// A stand-in for the Dot scope HUD the words fly out of, on the
/// microphone's level while dictation records.
fn hud(p: Paint, g: &Play, t: f64, meter: Option<(f32, f32)>) -> El {
    let (avg, peak) = meter.unwrap_or((0.04, 0.06));
    let k = p.k;
    row()
        .key("hud")
        .absolute(HUD_AT[0] - 46.0, HUD_AT[1] - 15.0)
        .size(92.0, 30.0)
        .opacity(chrome(g, t))
        .opacity_motion(Anim::INSTANT)
        .center()
        .radius(15.0)
        .fill(if k.dark { k.c.bg_deep } else { k.c.text_strong })
        .border(p.hair, k.c.border_weak)
        .child(mic_level(p, "level", 16.0, avg, peak, meter.is_some()))
}

/// What the last try needs said, over the HUD.
fn hint(p: Paint, copy: &'static str) -> El {
    row()
        .key("hint")
        .absolute(0.0, HUD_AT[1] - 48.0)
        .w(STAGE_W)
        .justify_center()
        .child(
            text(copy)
                .key(copy)
                .font(p.k.ty.small)
                .color(p.muted())
                .transition(Transition::opacity()),
        )
}

/// The notice or certificate as it looks at `t`.
fn body(p: Paint, r: &Round, flights: &[(Flight, &str, [f32; 2])], t: f64, reduced: bool) -> El {
    let targeted = t >= r.lit_at();
    // The placeholder clears just before the first word lands, not under it.
    let speaking = flights.iter().any(|(f, _, _)| {
        if reduced {
            f.landed(t, true).is_some()
        } else {
            t >= f.arrives() - 0.2
        }
    });
    let done = r.answered && flights.iter().all(|(f, _, _)| t >= f.arrives());
    match r.spec.card {
        Card::Chat(n) => {
            let lines = CARD.wrap(&r.text).len().max(1);
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
fn landed_words(
    p: Paint,
    r: &Round,
    flights: &[(Flight, &str, [f32; 2])],
    t: f64,
    reduced: bool,
) -> Vec<El> {
    let (field, origin, _) = field_of(r);
    let at = r.spec.home;
    let ri = r.n;
    let font = field.font.lh(field.line);
    let mut out = Vec::new();
    for (wi, (flight, word, centre)) in flights.iter().enumerate() {
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

fn card(p: Paint, r: &Round, t: f64, reduced: bool, clear: f32) -> Option<El> {
    if t < r.arrive || t >= r.shatter() + if reduced { 0.3 } else { f64::from(BURN) + 0.05 } {
        return None;
    }
    let spec = r.spec;
    let i = r.n;
    let size = r.size();
    let flights = r.flights();
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
    let targeted = t >= r.lit_at();
    // Waiting cards sit back, dimmed and a touch smaller; the lit one comes forward.
    let lit = smooth(
        0.0,
        if reduced { 0.18 } else { 0.32 },
        (t - r.lit_at()) as f32,
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
        flights.iter().fold([0.0, 0.0], |[x, y], (f, _, _)| {
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
        flights
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
    let done = r.answered && flights.iter().all(|(f, _, _)| t >= f.arrives());
    let body = body(p, r, &flights, t, reduced);
    let body = floating(body, (!reduced && !targeted).then_some(t), i as f64 * 0.37);
    // The lit card breathes while it waits for an answer, and burns steady once answered.
    let breathe = if reduced || done {
        1.0
    } else {
        0.78 + 0.22 * (((t - r.lit_at()) * 3.4).sin() as f32)
    };
    let ember = ember_tone(p, 0.6);
    let ring = targeted.then(|| {
        el().key("lit")
            .absolute(-2.0, -2.0)
            .size(size[0] + 4.0, size[1] + 4.0)
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
            .size(size[0], size[1])
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
        effect(BURN_MARK + r.slot() as u32)
            .key("burn")
            .absolute(0.0, 0.0)
            .size(size[0], size[1])
            .effect_bleed(40.0)
    });
    // The burn erases the card's own pixels, so the card draws in its own
    // layer while it burns (an opacity short of 1 isolates it).
    if burn.is_some() {
        opacity = opacity.min(0.999);
    }
    // The answer rides in the card: it knocks and swells with it, and under
    // Reduce Motion fades out with it.
    let answer = landed_words(p, r, &flights, t, reduced);
    Some(
        el().key(format!("card{i}"))
            .absolute(spec.home[0], spec.home[1])
            .size(size[0], size[1])
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

fn words(p: Paint, g: &Play, t: f64, reduced: bool) -> El {
    let mut out = el().key("words").absolute(0.0, 0.0).size(STAGE_W, STAGE_H);
    let mut all: Vec<Flight> = Vec::new();
    for r in &g.rounds {
        if t >= r.shatter() {
            continue;
        }
        let weight = 500.0 + 220.0 * r.spec.heat;
        for (wi, (flight, word, _)) in r.flights().iter().enumerate() {
            all.push(*flight);
            if let Some(pose) = flight.pose(t, reduced) {
                out = out.child(flying(p, &format!("r{}f{wi}", r.n), word, weight, pose));
            }
        }
    }
    out.child(impacts(p, "impacts", [STAGE_W, STAGE_H], &all, t, reduced).absolute(0.0, 0.0))
}

/// What's left of the certificate clears fast once the share card shows.
fn reward_fade(g: &Play, r: &Round, t: f64) -> f32 {
    match g.share {
        Some(share) if r.n == CERTIFICATE => 1.0 - smooth(0.0, 0.14, (t - share) as f32),
        _ => 1.0,
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
    let size = r.size();
    let c = [size[0] * 0.5, size[1] * 0.5];
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
fn fx(p: Paint, g: &Play, t: f64, reduced: bool) -> Fx {
    let mut fx = Fx {
        dark: p.k.dark,
        tones: tones(p),
        ..Fx::default()
    };
    if reduced {
        return fx;
    }
    let source = [HUD_AT[0], HUD_AT[1] - 15.0];
    for r in &g.rounds {
        if !r.answered {
            continue;
        }
        let seed = seed_of(r.n);
        let clear = reward_fade(g, r, t);
        let impact = on_stage(r, r.impact(), SWELL);
        let te = (t - r.explode) as f32;
        if (0.0..HITSTOP as f32 + 1.0).contains(&te) && clear > 0.0 {
            bolts(&mut fx, seed, source, impact, te, clear);
        }
        if let Some(age) = burning(r, t) {
            fx.burns.push(Burn {
                card: r.slot(),
                size: r.size(),
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
    let size = r.size();
    for n in 0..ASH as u32 {
        let local = [
            hash(seed ^ 0xA5, n * 3) * size[0],
            hash(seed ^ 0xA5, n * 3 + 1) * size[1],
        ];
        let born = game_fx::passes(game_fx::field(local, size, impact, seed));
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
fn share<S: 'static>(
    p: Paint,
    g: &Play,
    t: f64,
    destroyed: usize,
    reduced: bool,
    look: &Look<'_, S>,
) -> Option<El> {
    let age = (t - g.share?) as f32;
    let fold = fold(g, t);
    let cert = g.certificate()?;
    if age < 0.0 || fold >= 1.0 {
        return None;
    }
    let k = p.k;
    let w = SHARE_W;
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
    let (words, keys, secs) = g.stats();
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
                .child(stat("keys", keys.to_string(), "keys typed"))
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
                            text(cert.text.clone())
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
                .child(el().key("space").grow(1.0))
                .child(
                    Button::new("Keep playing")
                        .variant(ButtonVariant::Ghost)
                        .on_press(act(&look.keep))
                        .build_push(k)
                        .key("keep"),
                )
                .child(
                    Button::new("Continue")
                        .variant(ButtonVariant::Contrast)
                        .on_press(act(&look.done))
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
    use super::super::stories::{Flow, Rehearsal, Step};
    use super::*;

    /// The script plays the game to `until`.
    fn played(streams: bool, keep: bool, until: f64) -> Play {
        let mut play = Play::new(0.0);
        let mut script = Script {
            keep,
            ..Script::new(streams)
        };
        let mut t = 0.0;
        while t < until {
            script.run(&mut play, t);
            t += 1.0 / 60.0;
        }
        play
    }

    #[test]
    fn three_messages_take_about_ten_seconds() {
        let play = played(true, false, 20.0);
        let share = play.share.expect("the certificate is signed");
        assert!(share > 7.0 && share < 12.0, "{share}");
        assert_eq!(play.rounds.len(), 3, "no more cards until Keep playing");
        let batch = played(false, false, 20.0).share.unwrap();
        assert!(batch >= share, "words wait for the paste");
    }

    #[test]
    fn one_card_is_lit_at_a_time() {
        let play = played(true, true, 30.0);
        assert!(play.kept.is_some() && play.destroyed(30.0) > 4);
        let answered: Vec<&Round> = play.rounds.iter().filter(|r| r.answered).collect();
        for pair in answered.windows(2) {
            assert!(pair[1].lit_at() > pair[0].shatter());
        }
    }

    #[test]
    fn a_paste_answers_the_lit_card_and_lights_the_next() {
        let mut play = Play::new(0.0);
        play.hear(true, None, 1.5);
        assert_eq!(play.rounds.len(), 2, "the next card comes in");
        play.hear(true, Some("sure oat"), 2.0);
        assert_eq!(play.rounds[0].text, "sure oat");
        play.hear(false, None, 2.4);
        play.paste("Sure, oat or normal?", 2.8);
        let (first, next) = (&play.rounds[0], &play.rounds[1]);
        assert!(first.answered && first.text == "Sure, oat or normal?");
        assert_eq!(first.launch.len(), 4);
        assert!(first.shatter() > 2.8 && first.shatter() < 4.0);
        assert!(next.lit_at() > first.shatter() && next.text.is_empty());
    }

    #[test]
    fn live_words_that_never_paste_are_dropped() {
        let mut play = Play::new(0.0);
        play.hear(true, Some("hello"), 1.5);
        play.hear(false, None, 2.0);
        play.hear(false, None, 2.0 + WAIT - 0.1);
        assert_eq!(play.rounds[0].text, "hello");
        play.hear(false, None, 2.0 + WAIT + 0.1);
        assert!(play.rounds[0].text.is_empty() && !play.rounds[0].answered);
    }

    #[test]
    fn the_certificate_is_signed_with_the_answer() {
        let mut play = Play::new(0.0);
        play.paste("On my way.", 1.5);
        play.paste("Nope.", 4.0);
        play.paste("Alex Rivera", 7.0);
        let cert = play.certificate().unwrap();
        assert_eq!(cert.text, "Alex Rivera");
        assert!(play.share.is_some_and(|at| at > cert.shatter()));
        let (words, keys, secs) = play.stats();
        assert_eq!((words, keys), (6, 0));
        assert!(secs > 5.5, "{secs}");
        play.paste("ignored", 9.0);
        assert_eq!(play.rounds.len(), 3, "the score card waits");
        play.keep(12.0);
        play.paste("More.", 13.5);
        assert!(play.rounds.iter().any(|r| r.n == 3 && r.answered));
    }

    #[test]
    fn typing_shows_live_and_a_paste_or_return_sends_it() {
        let mut s = Onboard::new(Flow::FirstRun);
        s.rehearsal = Some(Rehearsal::new(true));
        s.step = Step::Try;
        s.now = 1.5;
        assert!(step(&mut s, 1.5));
        s.game.as_mut().unwrap().script = None;
        for typed in ["o", "ok"] {
            s.practice.text = typed.into();
            assert!(changed(&mut s));
        }
        let session = s.game.as_ref().unwrap();
        assert_eq!((session.typed.as_str(), session.play.keys), ("ok", 2));
        answer(&mut s);
        let session = s.game.as_ref().unwrap();
        assert!(session.play.rounds[0].answered && s.practice.text.is_empty());
        s.now = 4.0;
        s.practice.text = "Not today.".into();
        assert!(changed(&mut s), "a paste lands all at once");
        let session = s.game.as_ref().unwrap();
        assert_eq!(session.play.rounds[1].text, "Not today.");
        assert!(session.play.rounds[1].answered);
    }

    #[test]
    fn a_scripted_game_ignores_the_storys_keys() {
        let mut s = Onboard::new(Flow::FirstRun);
        s.rehearsal = Some(Rehearsal::new(true));
        s.step = Step::Try;
        assert!(step(&mut s, 1.0));
        s.practice.text = "t".into();
        assert!(changed(&mut s) && s.practice.text.is_empty());
        assert_eq!(s.game.as_ref().unwrap().play.keys, 0);
    }

    #[test]
    fn the_practice_field_variant_skips_the_game() {
        let mut s = Onboard::new(Flow::FirstRun);
        s.rehearsal = Some(Rehearsal::new(true));
        s.practice_field = true;
        s.step = Step::Try;
        assert!(!step(&mut s, 1.0) && s.game.is_none());
        assert!(page(paint(), &s).is_none());
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
        let play = played(true, false, 20.0);
        for r in &play.rounds {
            let born = |t: f64| {
                let mut fx = Fx::default();
                ash(&mut fx, r, seed_of(r.n), (t - r.shatter()) as f32, 1.0);
                fx.motes.len()
            };
            assert!(born(r.shatter() + 0.4) > 10, "ash leaves the rim");
            assert!(burning(r, r.shatter() + f64::from(BURN)).is_some());
            assert!(burning(r, r.shatter() + f64::from(BURN) + 0.1).is_none());
            assert!(card(paint(), r, r.shatter() + 0.5, false, 1.0).is_some());
            assert!(card(paint(), r, r.shatter() + 1.1, false, 1.0).is_none());
        }
    }

    #[test]
    fn reduce_motion_has_no_strike() {
        let play = played(true, false, 20.0);
        let t = play.rounds[0].shatter() + 0.2;
        let fx = fx(paint(), &play, t, true);
        assert!(fx.bolts.is_empty() && fx.motes.is_empty() && fx.burns.is_empty());
        let fx = super::fx(paint(), &play, t, false);
        assert!(!fx.motes.is_empty() && fx.burns.len() == 1);
    }

    fn paint() -> Paint {
        Paint::new(true, 2.0)
    }
}
