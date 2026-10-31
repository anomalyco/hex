//! `"Without Hex"` as one timeline, in the `"Launch Room"` group. Jay (CEO)
//! is presenting to the investors and asks whether we're ready to ship. You
//! type the answer by hand, slow and shaky, then frantic; every fix makes it
//! worse until Jay removes you from the group mid-word.
//!
//! Jay's question is already there before `"Watch what happens"`: lines
//! before [`Op::Start`] sit at negative times, shown while the stage waits.
//!
//! [`Timing`] holds every pause and the key pace, so the playground can tune
//! them live; [`Scene::build`] turns the script into strokes, chat lines,
//! tapbacks, a read receipt, typing dots, slow motion, the autocorrect hit,
//! the heartbeat, and one focus at each moment.

/// Every pause and pace in the scene, in seconds.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct Timing {
    /// From pressing start to Jay's first line after it.
    pub(crate) arrive: f64,
    /// Reading time per word of each of Jay's messages, after a fixed beat.
    pub(crate) read_word: f64,
    /// How long Jay's typing dots pulse before most replies.
    pub(crate) jay_typing: f64,
    /// The gap between keys at a calm, shaky pace.
    pub(crate) pace: f64,
    /// The gap between keys in full panic.
    pub(crate) panic_pace: f64,
    /// The silence after the accidental send, before `"Read"`.
    pub(crate) freeze: f64,
    /// Jay's typing dots before the punchline.
    pub(crate) punch: f64,
    /// How long the removal holds before `"With Hex"`.
    pub(crate) fired: f64,
    /// `"With Hex"`: from the words landing to the reply sending.
    pub(crate) hex_send: f64,
    /// `"With Hex"`: from the send to Jay's reply.
    pub(crate) hex_reply: f64,
}

impl Default for Timing {
    fn default() -> Self {
        Self {
            arrive: 0.35,
            read_word: 0.2,
            jay_typing: 1.0,
            pace: 0.25,
            panic_pace: 0.068,
            freeze: 1.5,
            punch: 1.25,
            fired: 2.0,
            hex_send: 0.7,
            hex_reply: 1.3,
        }
    }
}

impl Timing {
    /// How long one of Jay's messages holds so it can be read.
    pub(crate) fn read(&self, text: &str) -> f64 {
        0.75 + self.read_word * text.split_whitespace().count() as f64
    }
}

#[derive(Clone, Copy)]
enum Gap {
    Arrive,
    Freeze,
    Fired,
    Beat(f64),
}

/// How a reply went out.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Sent {
    Clean,
    /// Sent with a typo in it.
    Typo,
    /// The finger slipped onto Return: a typo, and Return flashes.
    Slip,
}

/// A tapback's mark, drawn as text or a vector, never an emoji font.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Glyph {
    Haha,
    Bang,
    Question,
    ThumbsUp,
    ThumbsDown,
    Heart,
}

#[derive(Clone, Copy)]
#[allow(dead_code)]
enum Op {
    /// Before the button: Jay's line at this many seconds before it.
    Before(&'static str, f64),
    /// The button: the scene's clock starts here.
    Start,
    Wait(Gap),
    /// Jay's typing dots, `x` times [`Timing::jay_typing`].
    Dots(f64),
    /// Jay's dots starting `after` seconds from now for `x` times
    /// [`Timing::jay_typing`], while what follows goes on.
    DotsAt(f64, f64),
    Punch,
    Jay(&'static str),
    /// One of Jay's lines, held this long instead of its reading time.
    JayFor(&'static str, f64),
    /// Anyone's line, held this long.
    Says(Who, &'static str, f64),
    /// A line that lands `after` seconds from now while what follows goes
    /// on, such as Jay cutting in while you type.
    Cut(Who, &'static str, f64),
    /// Typed by hand, panic rising from the first to the second level.
    Type(&'static str, f64, f64),
    Hesitate(f64),
    Back(usize),
    Send(Sent),
    Read,
    /// A tapback on the line `back` lines before the newest (`0` is the
    /// newest), `after` seconds from now.
    Tap(Glyph, usize, f64),
    /// An elbow lands on Caps Lock and its neighbours; Caps Lock toggles.
    Elbow,
    /// Time slows hard as Return starts down; the next send ends it.
    Slow,
    /// Autocorrect swaps the last `from` in the draft for `to`, under its
    /// underline.
    Autocorrect(&'static str, &'static str),
    /// The keyboard shudders this hard, in points.
    Shudder(f32),
    /// You're removed: the line lands mid-keystroke and the draft is gone.
    System(&'static str),
}

use Glyph::{Bang, Haha, Heart, Question, ThumbsDown, ThumbsUp};

/// Provisional: the final lines come from the script arena. Every engine
/// beat is exercised so the stills and tests have something to hold.
const SCRIPT: &[Op] = &[
    Op::Before("The investors are in the room.", 0.9),
    Op::Before("Are we ready to ship?", 0.5),
    Op::Start,
    Op::Wait(Gap::Arrive),
    Op::JayFor("they're asking right now", 0.35),
    Op::Cut(Who::Jay, "??", 1.75),
    Op::Type("sorry j", 0.0, 0.15),
    Op::Hesitate(0.55),
    Op::Back(7),
    Op::Hesitate(0.35),
    Op::Type("Yse, we're deady", 0.15, 0.6),
    // Long enough to read `"deady"`.
    Op::Hesitate(0.40),
    Op::Back(1),
    // `"dead"` sits there a moment; reaching for delete again, the finger
    // lands on Return.
    Op::Hesitate(0.23),
    Op::Send(Sent::Slip),
    Op::Wait(Gap::Freeze),
    Op::Read,
    Op::Tap(Bang, 0, 0.55),
    Op::Wait(Gap::Beat(1.1)),
    Op::Dots(0.7),
    Op::JayFor("what??", 0.45),
    Op::Cut(Who::Jay, "WHAT", 0.75),
    Op::Type("oh god *reayd", 0.7, 0.9),
    Op::Send(Sent::Typo),
    Op::Wait(Gap::Beat(0.3)),
    Op::Dots(0.55),
    Op::Wait(Gap::Beat(0.45)),
    Op::Dots(0.85),
    Op::Jay("ready or dead?"),
    Op::Type("yes", 0.9, 0.9),
    Op::Send(Sent::Clean),
    Op::Tap(Question, 0, 0.3),
    Op::Wait(Gap::Beat(0.4)),
    Op::Type("sorry fart fingers", 0.85, 1.0),
    Op::Send(Sent::Typo),
    Op::Tap(Haha, 0, 0.45),
    Op::Tap(Haha, 0, 0.8),
    Op::Wait(Gap::Beat(1.0)),
    Op::Elbow,
    Op::Wait(Gap::Beat(0.45)),
    Op::Type("I'M SO SORRY", 1.0, 1.0),
    Op::Slow,
    Op::Wait(Gap::Beat(0.5)),
    Op::Autocorrect("SORRY", "BABE"),
    Op::Wait(Gap::Beat(0.7)),
    Op::Send(Sent::Typo),
    Op::Tap(Haha, 0, 0.4),
    Op::Tap(Heart, 0, 0.65),
    Op::Cut(Who::Investor(1), "who is babe", 0.9),
    Op::Wait(Gap::Beat(1.0)),
    Op::Dots(0.7),
    Op::JayFor("they can see my screen", 1.3),
    Op::Punch,
    Op::JayFor("Clear your desk.", 0.55),
    Op::Tap(ThumbsUp, 0, 0.0),
    Op::Tap(Bang, 0, 0.25),
    Op::Tap(Haha, 0, 0.5),
    Op::Tap(ThumbsDown, 0, 0.75),
    // `"wait no i meant ready"`, cut off.
    Op::Type("wait no i mea", 1.0, 1.0),
    Op::Wait(Gap::Beat(0.12)),
    Op::System("Jay removed you from \u{201c}Launch Room\u{201d}"),
    Op::Wait(Gap::Fired),
];

/// The red pill under the chat's header from the first frame.
pub(crate) const PRESENTING: Option<&str> = Some("Jay is presenting");

/// The investors, by the names a group shows over their lines.
pub(crate) const INVESTORS: [&str; 3] = ["Priya", "Marcus", "Dana"];

const HOLD: f64 = 0.072;
const MODIFIER_LEAD: f64 = 0.052;
const SINK: f64 = 0.022;
/// Keys come back up slowly enough to catch in peripheral vision.
const RISE: f64 = 0.11;
/// How long Return flashes after the finger slips onto it.
const FLASH: f64 = 0.28;
/// How far into panic a stroke has to be for the harder clack.
const HARD: f64 = 0.5;
/// Autocorrect's underline sits under the word this long before the swap.
const MARK_LEAD: f64 = 0.3;
/// How long the slow motion takes to settle in.
const SLOW_IN: f64 = 0.12;
/// The keys an elbow lands on with Caps Lock.
const ELBOW: [&str; 4] = ["caps", "tab", "A", "shift"];
/// One heartbeat's sample, so none rings past the removal.
pub(crate) const BEAT_LEN: f64 = 0.36;
/// The slow-motion heartbeat's sample.
pub(crate) const SLOW_BEAT_LEN: f64 = 0.9;

pub(crate) struct Stroke {
    pub(crate) at: f64,
    /// `""` for a change to the draft no key made, such as autocorrect.
    pub(crate) key: &'static str,
    pub(crate) modifier: Option<&'static str>,
    hold: f64,
    /// The draft in the field after this stroke (empty after a send).
    pub(crate) text: String,
    pub(crate) caret: usize,
    /// Struck hard, in panic.
    pub(crate) hard: bool,
    /// How far into panic, `0..=1`.
    pub(crate) panic: f64,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Who {
    Jay,
    /// One of [`INVESTORS`].
    Investor(u8),
    You,
    System,
}

impl Who {
    /// The name a group shows over their first line in a run.
    pub(crate) fn name(self) -> Option<&'static str> {
        match self {
            Who::Jay => Some("Jay"),
            Who::Investor(i) => INVESTORS.get(usize::from(i)).copied(),
            Who::You | Who::System => None,
        }
    }
}

pub(crate) struct Line {
    pub(crate) at: f64,
    pub(crate) who: Who,
    pub(crate) text: String,
}

/// A tapback landing on a line.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct Tap {
    pub(crate) at: f64,
    pub(crate) line: usize,
    pub(crate) glyph: Glyph,
}

/// One heartbeat under the typing.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct Beat {
    pub(crate) at: f64,
    /// How loud, `0..=1`.
    pub(crate) level: f32,
    /// The stretched, lower beat in slow motion.
    pub(crate) slow: bool,
}

/// Where the eye should be.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Focus {
    /// Nothing has happened yet.
    Waiting,
    /// One chat line: an incoming message, a sent one, the system line.
    Line(usize),
    /// Jay's typing dots.
    Dots,
    /// The field and the keyboard, while you type.
    Field,
    /// The `"Read"` receipt under the line it reads.
    Read(usize),
}

/// Autocorrect's underline under a word of the draft.
struct Mark {
    from: f64,
    until: f64,
    start: usize,
    end: usize,
}

pub(crate) struct Scene {
    strokes: Vec<Stroke>,
    lines: Vec<Line>,
    taps: Vec<Tap>,
    dots: Vec<(f64, f64)>,
    read: Option<(f64, usize)>,
    focus: Vec<(f64, Focus)>,
    knocks: Vec<(f64, f32)>,
    shudders: Vec<(f64, f32)>,
    /// The line the finger slipped onto Return for.
    slip: Option<usize>,
    flashes: Vec<f64>,
    /// Caps Lock toggling on and off.
    caps: Vec<f64>,
    /// Slow motion, from Return starting down to the send.
    slows: Vec<(f64, f64)>,
    marks: Vec<Mark>,
    /// The autocorrect hit: the swap, and the send that ends it.
    slams: Vec<(f64, f64)>,
    typos: usize,
    typing: f64,
    end: f64,
}

fn key_for(c: char, caps: bool) -> (&'static str, bool) {
    let shifted = (c.is_ascii_uppercase() && !caps) || matches!(c, '?' | '*' | '!');
    let key = match c.to_ascii_lowercase() {
        ' ' => "space",
        ',' => ",",
        '.' => ".",
        '\'' => "'",
        '*' => "8",
        '!' => "1",
        '?' | '/' => "/",
        letter => super::keyboard::letter(letter).unwrap_or("space"),
    };
    (key, shifted)
}

fn noise(n: usize, salt: f64) -> f64 {
    ((n as f64 * 12.9898 + salt * 78.233).sin() * 43_758.545)
        .fract()
        .abs()
}

fn wobble(n: usize) -> f64 {
    0.78 + 0.44 * noise(n, 0.0)
}

/// Letters a shaking hand has to look for.
fn hard_letter(c: char) -> bool {
    matches!(
        c.to_ascii_lowercase(),
        'q' | 'z' | 'x' | 'j' | 'k' | 'y' | 'p' | '\'' | ',' | '*'
    )
}

fn smoothstep(u: f64) -> f64 {
    let u = u.clamp(0.0, 1.0);
    u * u * (3.0 - 2.0 * u)
}

impl Default for Scene {
    fn default() -> Self {
        Self::build(&Timing::default())
    }
}

impl Scene {
    pub(crate) fn build(t: &Timing) -> Self {
        let mut s = Scene {
            strokes: Vec::new(),
            lines: Vec::new(),
            taps: Vec::new(),
            dots: Vec::new(),
            read: None,
            focus: vec![(0.0, Focus::Waiting)],
            knocks: Vec::new(),
            shudders: Vec::new(),
            slip: None,
            flashes: Vec::new(),
            caps: Vec::new(),
            slows: Vec::new(),
            marks: Vec::new(),
            slams: Vec::new(),
            typos: 0,
            typing: 0.0,
            end: 0.0,
        };
        let mut at = 0.0;
        let mut text = String::new();
        let mut caret = 0;
        let mut n = 0;
        let mut began: Option<f64> = None;
        let mut caps = false;
        let mut slow: Option<f64> = None;
        let mut swapped: Option<f64> = None;
        // Lines that land while what follows goes on, waiting their turn.
        let mut pending: Vec<Line> = Vec::new();
        let stroke = |s: &mut Scene, at: f64, gap: f64, key, modifier, text: &str, caret, panic| {
            s.strokes.push(Stroke {
                at,
                key,
                modifier,
                hold: HOLD.min(gap.max(0.05) * 0.8),
                text: text.to_string(),
                caret,
                hard: panic >= HARD,
                panic,
            });
        };
        // Every line goes in in time order, so any cut-in due first goes
        // ahead of it.
        let push = |s: &mut Scene, pending: &mut Vec<Line>, line: Line| -> usize {
            pending.sort_by(|a, b| a.at.total_cmp(&b.at));
            while pending.first().is_some_and(|p| p.at <= line.at) {
                s.lines.push(pending.remove(0));
            }
            s.lines.push(line);
            s.lines.len() - 1
        };
        let mut panic = 0.0;
        for op in SCRIPT {
            match *op {
                Op::Before(line, secs) => s.lines.push(Line {
                    at: -secs,
                    who: Who::Jay,
                    text: line.to_string(),
                }),
                Op::Start => at = 0.0,
                Op::Wait(gap) => {
                    at += match gap {
                        Gap::Arrive => t.arrive,
                        Gap::Freeze => t.freeze,
                        Gap::Fired => t.fired,
                        Gap::Beat(secs) => secs,
                    }
                }
                Op::Dots(x) => {
                    s.focus.push((at, Focus::Dots));
                    s.dots.push((at, at + t.jay_typing * x));
                    at += t.jay_typing * x;
                }
                Op::DotsAt(after, x) => {
                    s.dots.push((at + after, at + after + t.jay_typing * x));
                }
                Op::Punch => {
                    s.focus.push((at, Focus::Dots));
                    s.dots.push((at, at + t.punch));
                    at += t.punch;
                }
                Op::Jay(line) => {
                    let i = push(
                        &mut s,
                        &mut pending,
                        Line {
                            at,
                            who: Who::Jay,
                            text: line.to_string(),
                        },
                    );
                    s.focus.push((at, Focus::Line(i)));
                    at += t.read(line);
                }
                Op::JayFor(line, hold) | Op::Says(Who::Jay, line, hold) => {
                    let i = push(
                        &mut s,
                        &mut pending,
                        Line {
                            at,
                            who: Who::Jay,
                            text: line.to_string(),
                        },
                    );
                    s.focus.push((at, Focus::Line(i)));
                    at += hold;
                }
                Op::Says(who, line, hold) => {
                    let i = push(
                        &mut s,
                        &mut pending,
                        Line {
                            at,
                            who,
                            text: line.to_string(),
                        },
                    );
                    s.focus.push((at, Focus::Line(i)));
                    at += hold;
                }
                Op::Cut(who, line, after) => pending.push(Line {
                    at: at + after,
                    who,
                    text: line.to_string(),
                }),
                Op::Type(chars, from, to) => {
                    s.focus.push((at, Focus::Field));
                    // A beat to find the keys before the first one lands,
                    // shorter in a panic.
                    at += 0.3 - 0.18 * from;
                    let count = chars.chars().count().max(2) - 1;
                    for (i, c) in chars.chars().enumerate() {
                        n += 1;
                        panic = from + (to - from) * i as f64 / count as f64;
                        let calm = 1.0 - panic;
                        let (key, shift) = key_for(c, caps);
                        text.insert(caret, c);
                        caret += c.len_utf8();
                        let pace = t.pace + (t.panic_pace - t.pace) * panic;
                        // Uneven: a run of two or three comes fast, then the
                        // hand stalls, and it hunts for the hard letters.
                        let burst = noise(n, 1.0);
                        let mut gap = pace * wobble(n);
                        if burst < 0.3 {
                            gap *= 0.55;
                        } else if burst > 0.82 {
                            gap += 0.2 * calm;
                        }
                        if hard_letter(c) {
                            gap += 0.14 * calm;
                        }
                        if c == ' ' {
                            gap += 0.05 * calm;
                        } else if shift {
                            gap += 0.035;
                        }
                        if i == 0 {
                            gap = 0.0;
                        }
                        at += gap;
                        began.get_or_insert(at);
                        stroke(
                            &mut s,
                            at,
                            gap.max(pace),
                            key,
                            shift.then_some("shift"),
                            &text,
                            caret,
                            panic,
                        );
                    }
                }
                Op::Hesitate(secs) => at += secs,
                Op::Back(count) => {
                    for _ in 0..count {
                        n += 1;
                        caret -= text[..caret].chars().last().map_or(1, char::len_utf8);
                        text.remove(caret);
                        // Calm deletes are careful; panicked ones hammer.
                        let gap = (0.12 + (0.055 - 0.12) * panic) * wobble(n);
                        at += gap;
                        stroke(&mut s, at, gap, "delete", None, &text, caret, panic);
                        if panic >= 0.6 {
                            s.shudders.push((at, 1.2));
                        }
                    }
                }
                Op::Send(how) => {
                    let gap = if how == Sent::Slip { 0.11 } else { 0.16 };
                    at += gap;
                    let sent = std::mem::take(&mut text);
                    caret = 0;
                    stroke(&mut s, at, gap, "return", None, "", 0, panic.max(HARD));
                    let i = push(
                        &mut s,
                        &mut pending,
                        Line {
                            at,
                            who: Who::You,
                            text: sent,
                        },
                    );
                    s.focus.push((at, Focus::Line(i)));
                    s.knocks
                        .push((at, if how == Sent::Slip { 6.0 } else { 3.5 }));
                    if how == Sent::Slip {
                        s.slip.get_or_insert(i);
                        s.flashes.push(at);
                        s.shudders.push((at, 3.0));
                    }
                    if how != Sent::Clean || swapped.is_some() {
                        s.typos += 1;
                    }
                    if let Some(from) = slow.take() {
                        s.slows.push((from, at));
                    }
                    if let Some(swap) = swapped.take() {
                        s.slams.push((swap, at));
                        s.shudders.push((at, 2.5));
                    }
                    for mark in s.marks.iter_mut().filter(|m| m.until > at) {
                        mark.until = at;
                    }
                    s.typing += at - began.take().unwrap_or(at);
                    at += 0.35;
                }
                Op::Read => {
                    let line = s.lines.iter().rposition(|l| l.who == Who::You).unwrap_or(0);
                    s.focus.push((at, Focus::Read(line)));
                    s.read = Some((at, line));
                }
                Op::Tap(glyph, back, after) => {
                    let when = at + after;
                    let shown = s.lines.iter().take_while(|l| l.at <= when).count();
                    s.taps.push(Tap {
                        at: when,
                        line: shown.saturating_sub(1 + back),
                        glyph,
                    });
                }
                Op::Elbow => {
                    n += 1;
                    caps = !caps;
                    s.caps.push(at);
                    for key in ELBOW {
                        stroke(&mut s, at, 0.2, key, None, &text, caret, 1.0);
                    }
                    s.shudders.push((at, 4.0));
                }
                Op::Slow => {
                    s.focus.push((at, Focus::Field));
                    slow = Some(at);
                }
                Op::Autocorrect(from, to) => {
                    if let Some(start) = text.rfind(from) {
                        s.marks.push(Mark {
                            from: at - MARK_LEAD,
                            until: at,
                            start,
                            end: start + from.len(),
                        });
                        text.replace_range(start..start + from.len(), to);
                        caret = text.len();
                        stroke(&mut s, at, 0.0, "", None, &text, caret, panic);
                        s.marks.push(Mark {
                            from: at,
                            until: f64::INFINITY,
                            start,
                            end: start + to.len(),
                        });
                        swapped = Some(at);
                    }
                }
                Op::Shudder(amp) => s.shudders.push((at, amp)),
                Op::System(line) => {
                    // A draft cut off mid-word was never sent.
                    began = None;
                    let i = push(
                        &mut s,
                        &mut pending,
                        Line {
                            at,
                            who: Who::System,
                            text: line.to_string(),
                        },
                    );
                    s.focus.push((at, Focus::Line(i)));
                    s.knocks.push((at, 4.0));
                }
            }
        }
        // Anyone cutting in after the removal is never heard.
        drop(pending);
        s.focus.sort_by(|a, b| a.0.total_cmp(&b.0));
        s.taps.sort_by(|a, b| a.at.total_cmp(&b.at));
        s.end = at;
        s
    }

    pub(crate) fn strokes(&self) -> &[Stroke] {
        &self.strokes
    }

    /// The whole scene, from start to the cut to `"With Hex"`.
    pub(crate) fn duration(&self) -> f64 {
        self.end
    }

    /// The time spent typing: each reply's first key to its send.
    #[cfg(test)]
    pub(crate) fn typing_secs(&self) -> f64 {
        self.typing
    }

    /// The replies that went out with a typo, a slip, or autocorrect in them.
    pub(crate) fn typos(&self) -> usize {
        self.typos
    }

    /// From Jay's first message after the button to your removal: what
    /// answering by hand cost.
    pub(crate) fn score_secs(&self) -> f64 {
        let asked = self
            .lines
            .iter()
            .find(|l| l.at >= 0.0)
            .map_or(0.0, |l| l.at);
        removed_at(&self.lines).unwrap_or(self.end) - asked
    }

    pub(crate) fn lines(&self) -> &[Line] {
        &self.lines
    }

    /// When each chat line has arrived by `t`.
    pub(crate) fn shown(&self, t: f64) -> usize {
        self.lines.iter().take_while(|l| l.at <= t).count()
    }

    pub(crate) fn sent(&self, t: f64) -> usize {
        self.lines[..self.shown(t)]
            .iter()
            .filter(|l| l.who == Who::You)
            .count()
    }

    /// Every tapback that has landed by `t`, oldest first.
    pub(crate) fn taps(&self, t: f64) -> impl Iterator<Item = &Tap> {
        self.taps.iter().take_while(move |tap| tap.at <= t)
    }

    /// Every tapback in the scene.
    pub(crate) fn all_taps(&self) -> &[Tap] {
        &self.taps
    }

    /// Jay's typing dots at `t`: which pause (`0..`) is running, if any.
    pub(crate) fn dots(&self, t: f64) -> Option<usize> {
        self.dots.iter().position(|&(a, b)| (a..b).contains(&t))
    }

    /// When `"Read"` shows, from the start.
    pub(crate) fn read_at(&self) -> Option<f64> {
        self.read.map(|(at, _)| at)
    }

    pub(crate) fn read(&self, t: f64) -> Option<usize> {
        self.read.filter(|&(at, _)| at <= t).map(|(_, line)| line)
    }

    pub(crate) fn focus(&self, t: f64) -> Focus {
        self.focus
            .iter()
            .take_while(|(at, _)| *at <= t)
            .last()
            .map_or(Focus::Waiting, |&(_, f)| f)
    }

    /// The draft in the field at `t`, and its caret while you type; gone
    /// once you're removed.
    pub(crate) fn draft(&self, t: f64) -> (&str, Option<usize>) {
        if self.removed(t) {
            return ("", None);
        }
        let stroke = self.strokes.iter().take_while(|s| s.at <= t).last();
        let text = stroke.map_or("", |s| s.text.as_str());
        let caret = (self.focus(t) == Focus::Field).then(|| stroke.map_or(0, |s| s.caret));
        (text, caret)
    }

    /// Autocorrect's underline at `t`: the word's byte range in the draft.
    pub(crate) fn mark(&self, t: f64) -> Option<(usize, usize)> {
        self.marks
            .iter()
            .find(|m| (m.from..m.until).contains(&t))
            .map(|m| (m.start, m.end))
    }

    pub(crate) fn removed(&self, t: f64) -> bool {
        removed_at(&self.lines).is_some_and(|at| at <= t)
    }

    /// When you were removed.
    pub(crate) fn removal(&self) -> Option<f64> {
        removed_at(&self.lines)
    }

    /// Caps Lock is on at `t`.
    pub(crate) fn caps(&self, t: f64) -> bool {
        self.caps.iter().filter(|&&at| at <= t).count() % 2 == 1
    }

    /// How slow time runs at `t`: `0` at full speed, `1` in full slow
    /// motion. It settles in and snaps back at the send.
    pub(crate) fn slow(&self, t: f64) -> f32 {
        self.slows
            .iter()
            .find(|(a, b)| (*a..*b).contains(&t))
            .map_or(0.0, |&(a, _)| smoothstep((t - a) / SLOW_IN) as f32)
    }

    /// Slow motion's spans, from Return starting down to the send.
    pub(crate) fn slows(&self) -> &[(f64, f64)] {
        &self.slows
    }

    /// The autocorrect hit at `t`: seconds since the swap, while it holds.
    pub(crate) fn slam(&self, t: f64) -> Option<f64> {
        self.slams
            .iter()
            .find(|(a, b)| (*a..*b).contains(&t))
            .map(|&(a, _)| t - a)
    }

    /// When each autocorrect hit lands.
    pub(crate) fn slams(&self) -> impl Iterator<Item = f64> + '_ {
        self.slams.iter().map(|&(a, _)| a)
    }

    /// The two moments everything else drops away for: the accidental send
    /// until `"Read"` has sunk in, and the removal; and slow motion, while
    /// the draft alone has the eye.
    pub(crate) fn frozen(&self, t: f64) -> bool {
        self.removed(t)
            || self.slow(t) > 0.0
            || match self.focus(t) {
                Focus::Read(_) => true,
                Focus::Line(i) => Some(i) == self.slip,
                _ => false,
            }
    }

    /// Return's red flash after the finger slips onto it, `0..=1`.
    pub(crate) fn flash(&self, t: f64, key: &str) -> f32 {
        if key != "return" {
            return 0.0;
        }
        self.flashes
            .iter()
            .map(|&at| {
                let u = (t - at) / FLASH;
                if (0.0..1.0).contains(&u) {
                    (1.0 - u * u) as f32
                } else {
                    0.0
                }
            })
            .fold(0.0, f32::max)
    }

    /// The chat window's shake after a bad send, in points.
    pub(crate) fn knock(&self, t: f64) -> f32 {
        ring(&self.knocks, t, 8.5)
    }

    /// The keyboard's shudder on the worst slips, in points.
    pub(crate) fn shudder(&self, t: f64) -> [f32; 2] {
        [
            ring(&self.shudders, t, 11.0),
            0.4 * ring(&self.shudders, t + 0.013, 7.0),
        ]
    }

    /// How far into panic you are at `t`: the hardest stroke so far.
    pub(crate) fn panic(&self, t: f64) -> f64 {
        self.strokes
            .iter()
            .take_while(|s| s.at <= t)
            .map(|s| s.panic)
            .fold(0.0, f64::max)
    }

    /// The heartbeat under the typing: from the first key, quicker and
    /// louder with panic, one long low beat in slow motion, and never a
    /// beat still ringing when you're removed.
    pub(crate) fn heartbeats(&self) -> Vec<Beat> {
        let mut beats = Vec::new();
        let Some(first) = self.strokes.first() else {
            return beats;
        };
        let stop = self.removal().unwrap_or(self.end);
        let mut at = first.at;
        while at < stop {
            if let Some(&(a, b)) = self.slows.iter().find(|(a, b)| (*a..*b).contains(&at)) {
                if a + 0.05 + SLOW_BEAT_LEN <= stop {
                    beats.push(Beat {
                        at: a + 0.05,
                        level: 1.0,
                        slow: true,
                    });
                }
                at = b + 0.25;
                continue;
            }
            let panic = self.panic(at);
            if at + BEAT_LEN <= stop {
                beats.push(Beat {
                    at,
                    level: (0.3 + 0.7 * panic) as f32,
                    slow: false,
                });
            }
            at += 0.95 + (0.46 - 0.95) * panic;
        }
        beats
    }

    pub(crate) fn depth(&self, elapsed: f64, key: &str) -> f32 {
        let mut deepest = 0.0_f64;
        // In slow motion, Return sinks the whole way down to the send.
        if key == "return"
            && let Some(&(a, b)) = self.slows.iter().find(|(a, b)| (*a..*b).contains(&elapsed))
        {
            deepest = 0.9 * smoothstep((elapsed - a) / (b - a));
        }
        for s in &self.strokes {
            let span = if s.key == key {
                Some((s.at, s.at + s.hold))
            } else if s.modifier == Some(key) {
                Some((s.at - MODIFIER_LEAD, s.at + s.hold + MODIFIER_LEAD * 0.6))
            } else {
                None
            };
            if let Some((down, up)) = span {
                if elapsed < down - SINK || elapsed > up + RISE {
                    continue;
                }
                let d = if elapsed < down {
                    1.0 - (down - elapsed) / SINK
                } else if elapsed <= up {
                    1.0
                } else {
                    1.0 - (elapsed - up) / RISE
                };
                deepest = deepest.max(d);
            }
        }
        deepest.clamp(0.0, 1.0) as f32
    }
}

fn removed_at(lines: &[Line]) -> Option<f64> {
    lines.iter().find(|l| l.who == Who::System).map(|l| l.at)
}

/// A decaying shake from each `(at, amplitude)` at `hz`.
fn ring(hits: &[(f64, f32)], t: f64, hz: f32) -> f32 {
    hits.iter()
        .map(|&(at, amp)| {
            let u = (t - at) as f32;
            if !(0.0..0.5).contains(&u) {
                return 0.0;
            }
            amp * (-u * 9.0).exp() * (u * std::f32::consts::TAU * hz).sin()
        })
        .sum()
}

#[cfg(test)]
mod tests {
    use super::super::fixture::ANSWER_JAY;
    use super::*;

    fn sent(scene: &Scene) -> Vec<&str> {
        scene
            .lines()
            .iter()
            .filter(|l| l.who == Who::You)
            .map(|l| l.text.as_str())
            .collect()
    }

    #[test]
    fn it_ends_with_the_removal_mid_word() {
        let scene = Scene::default();
        assert!(sent(&scene).len() >= 3);
        let last = scene.lines().last().unwrap();
        assert_eq!(last.who, Who::System);
        assert!(scene.removed(scene.duration()));
        assert_eq!(scene.draft(scene.duration()), ("", None));
        let removal = last.at;
        assert!(
            !scene.draft(removal - 0.01).0.is_empty(),
            "cut off mid-word"
        );
        assert!(scene.typing_secs() < scene.score_secs());
    }

    #[test]
    fn the_lines_arrive_in_order() {
        let scene = Scene::default();
        assert!(scene.lines().windows(2).all(|w| w[0].at <= w[1].at));
        let focus = &scene.focus;
        assert!(focus.windows(2).all(|w| w[0].0 <= w[1].0));
    }

    #[test]
    fn jay_has_asked_before_the_button() {
        let scene = Scene::default();
        assert_eq!(scene.shown(-0.01), 2);
        assert_eq!(scene.focus(-0.01), Focus::Waiting);
        let asking = &scene.lines()[2];
        assert!((0.2..0.6).contains(&asking.at), "{}", asking.at);
        let typing = scene.strokes()[0].at;
        assert!((0.5..1.8).contains(&typing), "{typing}");
    }

    #[test]
    fn the_slip_reads_and_return_flashes() {
        let scene = Scene::default();
        let strokes = scene.strokes();
        let slip = strokes
            .iter()
            .position(|s| s.key == "return" && scene.flash(s.at + 0.05, "return") > 0.9)
            .unwrap();
        assert_eq!(strokes[slip - 1].key, "delete");
        assert!(strokes[slip].at - strokes[slip - 1].at >= 0.3);
        let at = strokes[slip].at;
        assert_eq!(scene.flash(at + 0.05, "a"), 0.0);
        assert_eq!(scene.flash(at + 0.3, "return"), 0.0);
        assert!(scene.frozen(at + 0.1));
        assert!(scene.shudder(at + 0.02)[0].abs() > 0.5);
    }

    #[test]
    fn the_score_counts_what_went_out_wrong() {
        let scene = Scene::default();
        let sent = sent(&scene);
        assert!(scene.typos() >= 1 && scene.typos() <= sent.len());
        let clean = sent.len() - scene.typos();
        assert!(clean >= 1, "something went out as typed: {sent:?}");
    }

    #[test]
    fn every_joke_has_time_to_land() {
        let scene = Scene::default();
        let read = scene.read.unwrap().0;
        let slip = scene.lines().iter().find(|l| l.who == Who::You).unwrap().at;
        assert!(read - slip >= 1.0, "a silence before Read");
        assert!(
            scene.typing_secs() > ANSWER_JAY.release * 2.5,
            "{}",
            scene.typing_secs()
        );
        assert!(
            (26.0..=36.0).contains(&scene.duration()),
            "{}",
            scene.duration()
        );
        assert!(scene.score_secs() > 24.0, "{}", scene.score_secs());
    }

    #[test]
    fn the_keys_speed_up_into_panic() {
        let scene = Scene::default();
        let keys: Vec<&Stroke> = scene
            .strokes()
            .iter()
            .filter(|s| s.key.len() == 1 || s.key == "space")
            .collect();
        let gaps = |from: usize, to: usize| (keys[to].at - keys[from].at) / (to - from) as f64;
        let early = gaps(0, 5);
        let late = gaps(keys.len() - 6, keys.len() - 1);
        assert!((0.17..0.4).contains(&early), "{early}");
        assert!(early > late * 2.0, "{early} vs {late}");
        assert!(!keys[0].hard);
        assert!(keys.last().unwrap().hard);
    }

    #[test]
    fn one_focus_at_a_time() {
        let scene = Scene::default();
        assert_eq!(scene.focus(0.0), Focus::Waiting);
        let first = scene.lines()[2].at;
        assert_eq!(scene.focus(first + 0.01), Focus::Line(2));
        let typing = scene.strokes()[0].at;
        assert_eq!(scene.focus(typing), Focus::Field);
        let (read_at, line) = scene.read.unwrap();
        assert_eq!(scene.focus(read_at + 0.01), Focus::Read(line));
    }

    #[test]
    fn keys_sink_while_pressed() {
        let scene = Scene::default();
        let shifted = scene
            .strokes()
            .iter()
            .find(|s| s.modifier == Some("shift"))
            .unwrap();
        assert_eq!(scene.depth(shifted.at + 0.01, shifted.key), 1.0);
        assert_eq!(scene.depth(shifted.at - 0.03, "shift"), 1.0);
        assert_eq!(scene.depth(0.0, shifted.key), 0.0);
    }

    #[test]
    fn tapbacks_land_on_lines_already_there() {
        let scene = Scene::default();
        assert!(!scene.all_taps().is_empty());
        for tap in scene.all_taps() {
            assert!(scene.lines()[tap.line].at <= tap.at);
            assert!(scene.removal().is_none_or(|r| tap.at < r));
        }
        assert!(scene.all_taps().windows(2).all(|w| w[0].at <= w[1].at));
    }

    #[test]
    fn caps_lock_types_capitals_without_shift() {
        let scene = Scene::default();
        let Some(&on) = scene.caps.first() else {
            return;
        };
        assert!(!scene.caps(on - 0.01) && scene.caps(on));
        let after: Vec<&Stroke> = scene
            .strokes()
            .iter()
            .filter(|s| s.at > on && s.key.len() == 1)
            .take(4)
            .collect();
        assert!(after.iter().all(|s| s.modifier.is_none()));
    }

    #[test]
    fn autocorrect_hits_in_slow_motion_and_sends() {
        let scene = Scene::default();
        let Some(&(from, to)) = scene.slows().first() else {
            return;
        };
        let swap = scene
            .slams()
            .next()
            .expect("autocorrect in the slow motion");
        assert!(from < swap && swap < to);
        assert!(swap - from >= 0.3, "the underline shows before the swap");
        assert!(scene.mark(swap - 0.1).is_some() && scene.mark(swap + 0.1).is_some());
        assert_ne!(scene.draft(swap - 0.01).0, scene.draft(swap + 0.01).0);
        assert!(scene.slow(from + 0.5) > 0.99);
        assert_eq!(scene.slow(to + 0.01), 0.0);
        assert!(scene.slam(swap + 0.3).is_some() && scene.slam(to + 0.01).is_none());
        assert!(scene.depth(to - 0.05, "return") > 0.8);
        let out = scene.lines().iter().find(|l| l.at == to).unwrap();
        assert_eq!(out.text, scene.draft(to - 0.01).0);
    }

    #[test]
    fn the_heartbeat_quickens_and_stops_dead_at_the_removal() {
        let scene = Scene::default();
        let beats = scene.heartbeats();
        assert!(beats.len() >= 20, "{}", beats.len());
        let removal = scene.removal().unwrap();
        assert!(
            beats
                .iter()
                .all(|b| { b.at + if b.slow { SLOW_BEAT_LEN } else { BEAT_LEN } <= removal })
        );
        let plain: Vec<&Beat> = beats.iter().filter(|b| !b.slow).collect();
        let first = plain[1].at - plain[0].at;
        let last = plain[plain.len() - 1].at - plain[plain.len() - 2].at;
        assert!(first > last * 1.5, "{first} vs {last}");
        assert!(plain[0].level < plain[plain.len() - 1].level);
        assert_eq!(beats.iter().filter(|b| b.slow).count(), scene.slows().len());
    }

    #[test]
    fn faster_timing_shortens_the_scene() {
        let quick = Timing {
            freeze: 0.5,
            read_word: 0.1,
            ..Timing::default()
        };
        assert!(Scene::build(&quick).duration() < Scene::default().duration());
    }
}
