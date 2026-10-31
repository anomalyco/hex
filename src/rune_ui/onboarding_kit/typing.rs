//! `"Without Hex"` as one timeline, in the `"Launch Room"` group. Jay (CEO)
//! asks whether we're ready to ship with the investors in the room. You type
//! the answer by hand, a finger slips onto Return and sends
//! `"Yse, we're dead"`, and every fix makes it worse until Jay removes you
//! from the group mid-word.
//!
//! Jay's question is already there before `"Watch what happens"`: lines
//! before [`Op::Start`] sit at negative times, shown while the stage waits.
//!
//! [`Timing`] holds every pause and the key pace, so the playground can tune
//! them live; [`Scene::build`] turns the script into strokes, chat lines, a
//! read receipt, typing dots, and one focus at each moment.

/// Every pause and pace in the scene, in seconds.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct Timing {
    /// From pressing start to Jay's `"they're asking right now"`.
    pub(crate) arrive: f64,
    /// Reading time per word of each of Jay's messages, after a fixed beat.
    pub(crate) read_word: f64,
    /// How long Jay's typing dots pulse before most replies.
    pub(crate) jay_typing: f64,
    /// The gap between keys at a calm, readable pace.
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
            pace: 0.17,
            panic_pace: 0.072,
            freeze: 1.5,
            punch: 1.6,
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

#[derive(Clone, Copy)]
enum Op {
    /// Before the button: Jay's line at this many seconds before it.
    Before(&'static str, f64),
    /// The button: the scene's clock starts here.
    Start,
    Wait(Gap),
    /// Jay's typing dots, `x` times [`Timing::jay_typing`] or the punch.
    Dots(f64),
    Punch,
    Jay(&'static str),
    /// One of Jay's lines, held this long instead of its reading time.
    JayFor(&'static str, f64),
    /// Typed by hand, panic rising from the first to the second level.
    Type(&'static str, f64, f64),
    Hesitate(f64),
    Back(usize),
    /// Return. `true` when the finger slipped onto it.
    Send(bool),
    Read,
    /// You're removed: the line lands mid-keystroke and the draft is gone.
    System(&'static str),
}

const SCRIPT: &[Op] = &[
    Op::Before("The investors are in the room.", 0.9),
    Op::Before("Are we ready to ship?", 0.5),
    Op::Start,
    Op::Wait(Gap::Arrive),
    Op::JayFor("they're asking right now", 0.9),
    Op::Type("Yse, we're deady", 0.0, 0.75),
    // Long enough to read `"deady"`.
    Op::Hesitate(0.40),
    Op::Back(1),
    // `"dead"` sits there a moment; reaching for delete again, the finger
    // lands on Return.
    Op::Hesitate(0.23),
    Op::Send(true),
    Op::Wait(Gap::Freeze),
    Op::Read,
    Op::Wait(Gap::Beat(0.8)),
    Op::Dots(0.9),
    Op::Jay("what??"),
    Op::Type("oh god *reayd", 0.8, 1.0),
    Op::Send(false),
    Op::Dots(0.8),
    Op::Jay("ready or dead?"),
    Op::Type("yes", 1.0, 1.0),
    Op::Send(false),
    Op::Wait(Gap::Beat(0.6)),
    Op::Dots(1.0),
    Op::Jay("they can see my screen"),
    Op::Punch,
    Op::JayFor("Clear your desk.", 1.0),
    // `"wait no i meant ready"`, cut off.
    Op::Type("wait no i m", 1.0, 1.0),
    Op::Wait(Gap::Beat(0.06)),
    Op::System("Jay removed you from \"Launch Room\""),
    Op::Wait(Gap::Fired),
];

/// The typos you sent: `Yse`, `dead` for ready, and `*reayd`.
pub(crate) const TYPOS: usize = 3;

const HOLD: f64 = 0.072;
const MODIFIER_LEAD: f64 = 0.052;
const SINK: f64 = 0.022;
/// Keys come back up slowly enough to catch in peripheral vision.
const RISE: f64 = 0.11;
/// How long Return flashes after the finger slips onto it.
const FLASH: f64 = 0.28;
/// How far into panic a stroke has to be for the harder clack.
const HARD: f64 = 0.5;

pub(crate) struct Stroke {
    pub(crate) at: f64,
    pub(crate) key: &'static str,
    pub(crate) modifier: Option<&'static str>,
    hold: f64,
    /// The draft in the field after this stroke (empty after a send).
    pub(crate) text: String,
    pub(crate) caret: usize,
    /// Struck hard, in panic.
    pub(crate) hard: bool,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Who {
    Jay,
    You,
    System,
}

pub(crate) struct Line {
    pub(crate) at: f64,
    pub(crate) who: Who,
    pub(crate) text: String,
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

pub(crate) struct Scene {
    strokes: Vec<Stroke>,
    lines: Vec<Line>,
    dots: Vec<(f64, f64)>,
    read: Option<(f64, usize)>,
    focus: Vec<(f64, Focus)>,
    knocks: Vec<(f64, f32)>,
    /// The line the finger slipped onto Return for.
    slip: Option<usize>,
    flashes: Vec<f64>,
    typing: f64,
    end: f64,
}

fn key_for(c: char) -> (&'static str, bool) {
    let shifted = c.is_ascii_uppercase() || matches!(c, '?' | '*' | '!');
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

fn wobble(n: usize) -> f64 {
    let x = ((n as f64 * 12.9898).sin() * 43_758.545).fract().abs();
    0.78 + 0.44 * x
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
            dots: Vec::new(),
            read: None,
            focus: vec![(0.0, Focus::Waiting)],
            knocks: Vec::new(),
            slip: None,
            flashes: Vec::new(),
            typing: 0.0,
            end: 0.0,
        };
        let mut at = 0.0;
        let mut text = String::new();
        let mut caret = 0;
        let mut n = 0;
        let mut began: Option<f64> = None;
        let stroke = |s: &mut Scene, at: f64, gap: f64, key, modifier, text: &str, caret, hard| {
            s.strokes.push(Stroke {
                at,
                key,
                modifier,
                hold: HOLD.min(gap.max(0.05) * 0.8),
                text: text.to_string(),
                caret,
                hard,
            });
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
                Op::Punch => {
                    s.focus.push((at, Focus::Dots));
                    s.dots.push((at, at + t.punch));
                    at += t.punch;
                }
                Op::Jay(line) => {
                    s.focus.push((at, Focus::Line(s.lines.len())));
                    s.lines.push(Line {
                        at,
                        who: Who::Jay,
                        text: line.to_string(),
                    });
                    at += t.read(line);
                }
                Op::JayFor(line, hold) => {
                    s.focus.push((at, Focus::Line(s.lines.len())));
                    s.lines.push(Line {
                        at,
                        who: Who::Jay,
                        text: line.to_string(),
                    });
                    at += hold;
                }
                Op::Type(chars, from, to) => {
                    s.focus.push((at, Focus::Field));
                    // A beat to find the keys before the first one lands,
                    // shorter in a panic.
                    at += 0.25 - 0.12 * from;
                    let count = chars.chars().count().max(2) - 1;
                    for (i, c) in chars.chars().enumerate() {
                        n += 1;
                        panic = from + (to - from) * i as f64 / count as f64;
                        let (key, shift) = key_for(c);
                        text.insert(caret, c);
                        caret += c.len_utf8();
                        let extra = if c == ' ' {
                            0.05 * (1.0 - panic)
                        } else if shift {
                            0.035
                        } else {
                            0.0
                        };
                        let pace = t.pace + (t.panic_pace - t.pace) * panic;
                        let gap = if i == 0 {
                            0.0
                        } else {
                            pace * wobble(n) + extra
                        };
                        at += gap;
                        let hard = panic >= HARD;
                        began.get_or_insert(at);
                        stroke(
                            &mut s,
                            at,
                            gap.max(pace),
                            key,
                            shift.then_some("shift"),
                            &text,
                            caret,
                            hard,
                        );
                    }
                }
                Op::Hesitate(secs) => at += secs,
                Op::Back(count) => {
                    for _ in 0..count {
                        n += 1;
                        caret -= 1;
                        text.remove(caret);
                        let gap = 0.12 * wobble(n);
                        at += gap;
                        stroke(&mut s, at, gap, "delete", None, &text, caret, panic >= HARD);
                    }
                }
                Op::Send(slip) => {
                    let gap = if slip { 0.11 } else { 0.16 };
                    at += gap;
                    let sent = std::mem::take(&mut text);
                    caret = 0;
                    stroke(&mut s, at, gap, "return", None, "", 0, true);
                    s.focus.push((at, Focus::Line(s.lines.len())));
                    s.knocks.push((at, if slip { 6.0 } else { 3.5 }));
                    if slip {
                        s.slip.get_or_insert(s.lines.len());
                        s.flashes.push(at);
                    }
                    s.lines.push(Line {
                        at,
                        who: Who::You,
                        text: sent,
                    });
                    s.typing += at - began.take().unwrap_or(at);
                    at += 0.35;
                }
                Op::Read => {
                    let line = s.lines.iter().rposition(|l| l.who == Who::You).unwrap_or(0);
                    s.focus.push((at, Focus::Read(line)));
                    s.read = Some((at, line));
                }
                Op::System(line) => {
                    // A draft cut off mid-word was never sent.
                    began = None;
                    s.focus.push((at, Focus::Line(s.lines.len())));
                    s.knocks.push((at, 4.0));
                    s.lines.push(Line {
                        at,
                        who: Who::System,
                        text: line.to_string(),
                    });
                }
            }
        }
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

    /// From Jay's first message after the button to your removal: what
    /// answering by hand cost.
    pub(crate) fn score_secs(&self) -> f64 {
        let asked = self
            .lines
            .iter()
            .find(|l| l.at >= 0.0)
            .map_or(0.0, |l| l.at);
        let removed = self
            .lines
            .iter()
            .find(|l| l.who == Who::System)
            .map_or(self.end, |l| l.at);
        removed - asked
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

    pub(crate) fn removed(&self, t: f64) -> bool {
        self.lines[..self.shown(t)]
            .iter()
            .any(|l| l.who == Who::System)
    }

    /// The two moments everything else drops away for: the accidental send
    /// until `"Read"` has sunk in, and the removal.
    pub(crate) fn frozen(&self, t: f64) -> bool {
        self.removed(t)
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
        self.knocks
            .iter()
            .map(|&(at, amp)| {
                let u = (t - at) as f32;
                if !(0.0..0.5).contains(&u) {
                    return 0.0;
                }
                amp * (-u * 9.0).exp() * (u * std::f32::consts::TAU * 8.5).sin()
            })
            .sum()
    }

    pub(crate) fn depth(&self, elapsed: f64, key: &str) -> f32 {
        let mut deepest = 0.0_f64;
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
    fn every_fix_makes_it_worse() {
        let scene = Scene::default();
        assert_eq!(sent(&scene), ["Yse, we're dead", "oh god *reayd", "yes"]);
        let last = scene.lines().last().unwrap();
        assert_eq!(last.who, Who::System);
        assert!(scene.removed(scene.duration()));
        assert_eq!(scene.draft(scene.duration()), ("", None));
        let removal = last.at;
        assert_eq!(scene.draft(removal - 0.01).0, "wait no i m");
        assert!(scene.typing_secs() < scene.score_secs());
    }

    #[test]
    fn jay_has_asked_before_the_button() {
        let scene = Scene::default();
        assert_eq!(scene.shown(-0.01), 2);
        assert_eq!(scene.focus(-0.01), Focus::Waiting);
        let asking = &scene.lines()[2];
        assert!((0.2..0.6).contains(&asking.at), "{}", asking.at);
        let typing = scene.strokes()[0].at;
        assert!((1.2..1.8).contains(&typing), "{typing}");
    }

    #[test]
    fn the_slip_reads_and_return_flashes() {
        let scene = Scene::default();
        let strokes = scene.strokes();
        let back = strokes.iter().position(|s| s.key == "delete").unwrap();
        assert_eq!(strokes[back - 1].text, "Yse, we're deady");
        assert!(strokes[back].at - strokes[back - 1].at >= 0.4);
        let slip = &strokes[back + 1];
        assert_eq!(slip.key, "return");
        assert!(slip.at - strokes[back].at >= 0.3);
        assert!(scene.flash(slip.at + 0.05, "return") > 0.9);
        assert_eq!(scene.flash(slip.at + 0.05, "a"), 0.0);
        assert_eq!(scene.flash(slip.at + 0.3, "return"), 0.0);
        assert!(scene.frozen(slip.at + 0.1));
    }

    #[test]
    fn every_joke_has_time_to_land() {
        let scene = Scene::default();
        let lines: Vec<&Line> = scene.lines().iter().filter(|l| l.at >= 0.0).collect();
        for pair in lines.windows(2) {
            assert!(
                pair[1].at - pair[0].at >= 0.9,
                "{} → {}",
                pair[0].text,
                pair[1].text
            );
        }
        let read = scene.read.unwrap().0;
        let slip = lines.iter().find(|l| l.who == Who::You).unwrap().at;
        assert!(read - slip >= 1.0, "a silence before Read");
        assert!(
            scene.typing_secs() > ANSWER_JAY.release * 2.5,
            "{}",
            scene.typing_secs()
        );
        assert!(
            (20.0..=23.5).contains(&scene.duration()),
            "{}",
            scene.duration()
        );
        assert!(scene.score_secs() > 18.0, "{}", scene.score_secs());
    }

    #[test]
    fn the_keys_speed_up_into_panic() {
        let scene = Scene::default();
        let keys: Vec<&Stroke> = scene
            .strokes()
            .iter()
            .filter(|s| s.key != "return")
            .collect();
        let first = keys[2].at - keys[1].at;
        let late = keys[keys.len() - 1].at - keys[keys.len() - 2].at;
        assert!(first > late * 1.6, "{first} vs {late}");
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
        let first = &scene.strokes()[0];
        assert_eq!(first.modifier, Some("shift"));
        assert_eq!(scene.depth(first.at + 0.01, first.key), 1.0);
        assert_eq!(scene.depth(first.at - 0.03, "shift"), 1.0);
        assert_eq!(scene.depth(0.0, first.key), 0.0);
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
