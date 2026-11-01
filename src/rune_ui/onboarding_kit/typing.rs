//! `"Without Hex"` as one timeline, played from a [`Script`]: a scene file
//! in `scenes/` (syntax in `scenes/README.md`). In the shipping scene Jay
//! (CEO) is presenting to the investors and asks whether we're ready to
//! ship; you type the answer by hand, slow and shaky, then frantic, and every
//! fix makes it worse until he removes you from the group mid-word.
//!
//! Lines before `start` are already there before `"Watch what happens"`:
//! they sit at negative times, shown while the stage waits.
//!
//! [`Timing`] holds every pause and the key pace, so the playground can tune
//! them live; [`Scene::play`] turns the script into strokes, chat lines,
//! tapbacks, read receipts, typing dots, slow motion, the autocorrect hit,
//! the heartbeat, and one focus at each moment.

use super::scene_script::{self, Ending, Op, Script, Speaker, Wait};

/// Every pause and pace in the scene, in seconds.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct Timing {
    /// From pressing start to Jay's first line after it.
    pub(crate) arrive: f64,
    /// Reading time per word of each of Jay's messages, after a fixed beat.
    pub(crate) read_word: f64,
    /// How long the boss's typing dots pulse, for a plain `dots`.
    pub(crate) jay_typing: f64,
    /// The gap between keys at a calm, shaky pace.
    pub(crate) pace: f64,
    /// The gap between keys in full panic.
    pub(crate) panic_pace: f64,
    /// The silence after the accidental send, before `"Read"`.
    pub(crate) freeze: f64,
    /// `"With Hex"`: how far apart the cheers land.
    pub(crate) cheer: f64,
    /// How long the removal holds before `"With Hex"`.
    pub(crate) fired: f64,
    /// `"With Hex"`: from the words landing to the reply sending.
    pub(crate) hex_send: f64,
    /// `"With Hex"`: from the send to a first reply that types first.
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
            cheer: 0.35,
            fired: 2.0,
            hex_send: 0.7,
            hex_reply: 1.3,
        }
    }
}

impl Timing {
    /// How long a message holds so it can be read.
    pub(crate) fn read(&self, text: &str) -> f64 {
        0.75 + self.read_word * text.split_whitespace().count() as f64
    }
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

/// How long autocorrect's slow motion holds before the swap, and after it
/// before the send.
const SLOW_BEFORE: f64 = 0.6;
const SLOW_AFTER: f64 = 0.8;
/// Lines before the button sit this far apart, the last this long before it.
const BEFORE_STEP: f64 = 0.4;
const BEFORE_LAST: f64 = 0.5;
/// An untimed tapback lands this long after the last, the first this long
/// after its command.
const TAP_STEP: f64 = 0.2;
const TAP_FIRST: f64 = 0.3;

const HOLD: f64 = 0.072;
const MODIFIER_LEAD: f64 = 0.052;
const SINK: f64 = 0.022;
/// Keys come back up slowly enough to catch in peripheral vision.
const RISE: f64 = 0.11;
/// How long the presenting pill flares.
const FLARE: f64 = 0.9;
/// How hard careful keys are struck: soft.
const CAREFUL: f64 = 0.2;
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
    /// Someone in [`Script::people`]: `0` is the boss.
    Them(u8),
    You,
    /// The ending: you're cut off.
    System,
    /// A neutral notice, such as the thread being renamed.
    Status,
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
    script: &'static Script<'static>,
    strokes: Vec<Stroke>,
    lines: Vec<Line>,
    taps: Vec<Tap>,
    dots: Vec<(f64, f64)>,
    /// Each `"Read"` and the line it reads; the newest one shows.
    reads: Vec<(f64, usize)>,
    /// The pill flaring.
    flares: Vec<f64>,
    /// The thread's new names.
    renames: Vec<(f64, &'static str)>,
    ending: Ending,
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
    /// The first moment the typing goes wrong: a typo that goes out, the
    /// autocorrect hit, or an elbow on the keys.
    trouble: Option<f64>,
    typos: usize,
    typing: f64,
    end: f64,
}

/// When `sent`'s first wrong letter against `meant` was typed, among one
/// reply's `strokes`: the stroke after the last draft that didn't yet start
/// with `sent` through that letter. `None` when `sent` only falls short.
fn first_wrong(strokes: &[Stroke], sent: &str, meant: &str) -> Option<f64> {
    let same = sent
        .chars()
        .zip(meant.chars())
        .take_while(|(a, b)| a == b)
        .count();
    if same >= sent.chars().count() {
        return None;
    }
    let through: String = sent.chars().take(same + 1).collect();
    let typed = strokes
        .iter()
        .rposition(|s| !s.text.starts_with(&through))
        .map_or(0, |i| i + 1);
    strokes.get(typed).map(|s| s.at)
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
    /// The scene this run plays ([`Script::chosen`]), timed by `t`.
    pub(crate) fn build(t: &Timing) -> Self {
        Self::play(t, Script::chosen())
    }

    pub(crate) fn play(t: &Timing, script: &'static Script<'static>) -> Self {
        let mut s = Scene {
            script,
            strokes: Vec::new(),
            lines: Vec::new(),
            taps: Vec::new(),
            dots: Vec::new(),
            reads: Vec::new(),
            flares: Vec::new(),
            renames: Vec::new(),
            ending: Ending::Removed,
            focus: vec![(0.0, Focus::Waiting)],
            knocks: Vec::new(),
            shudders: Vec::new(),
            slip: None,
            flashes: Vec::new(),
            caps: Vec::new(),
            slows: Vec::new(),
            marks: Vec::new(),
            slams: Vec::new(),
            trouble: None,
            typos: 0,
            typing: 0.0,
            end: 0.0,
        };
        let before = script
            .ops
            .iter()
            .take_while(|op| **op != Op::Start)
            .filter(|op| matches!(op, Op::Says { .. }))
            .count();
        let mut at = -(BEFORE_LAST + BEFORE_STEP * before as f64);
        let mut text = String::new();
        let mut caret = 0;
        let mut n = 0;
        let mut began: Option<f64> = None;
        let mut caps = false;
        let mut slow: Option<f64> = None;
        let mut swapped: Option<f64> = None;
        // The strokes of the reply being typed start here.
        let mut reply_from = 0;
        let trouble = |s: &mut Scene, at: f64| {
            s.trouble = Some(s.trouble.map_or(at, |t: f64| t.min(at)));
        };
        // Lines that land while what follows goes on, waiting their turn,
        // with the name a tapback finds them by.
        let mut pending: Vec<(Line, Option<&str>)> = Vec::new();
        let mut labels: Vec<(&str, usize)> = Vec::new();
        // Tapbacks, placed once every line is: when, which, and on what: a
        // named line, else the newest of the lines written before the tap.
        let mut taps: Vec<(f64, Glyph, Option<&str>, usize)> = Vec::new();
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
        let push = |s: &mut Scene,
                    pending: &mut Vec<(Line, Option<&'static str>)>,
                    labels: &mut Vec<(&'static str, usize)>,
                    line: Line,
                    label: Option<&'static str>|
         -> usize {
            pending.sort_by(|a, b| a.0.at.total_cmp(&b.0.at));
            while pending.first().is_some_and(|p| p.0.at <= line.at) {
                let (cut, name) = pending.remove(0);
                labels.extend(name.map(|name| (name, s.lines.len())));
                s.lines.push(cut);
            }
            labels.extend(label.map(|name| (name, s.lines.len())));
            s.lines.push(line);
            s.lines.len() - 1
        };
        let who = |speaker: Speaker| match speaker {
            Speaker::Them(i) => Who::Them(i),
            Speaker::You => Who::You,
        };
        let mut title = script.title;
        let mut panic = 0.0;
        for op in &script.ops {
            match *op {
                Op::Says {
                    who: speaker,
                    text: line,
                    hold,
                    label,
                } => {
                    let started = at >= 0.0 || s.lines.len() >= before;
                    if !started {
                        at += BEFORE_STEP;
                    }
                    let i = push(
                        &mut s,
                        &mut pending,
                        &mut labels,
                        Line {
                            at,
                            who: who(speaker),
                            text: line.to_string(),
                        },
                        label,
                    );
                    if started {
                        s.focus.push((at, Focus::Line(i)));
                        at += hold.unwrap_or_else(|| t.read(line));
                    }
                }
                Op::Start => at = t.arrive,
                Op::Cut {
                    after,
                    who: speaker,
                    text: line,
                    label,
                } => pending.push((
                    Line {
                        at: at + after,
                        who: who(speaker),
                        text: line.to_string(),
                    },
                    label,
                )),
                Op::Wait(Wait::Secs(secs)) => at += secs,
                Op::Wait(Wait::Freeze) => at += t.freeze,
                Op::Dots { secs, after } => {
                    let length = t.jay_typing * secs.unwrap_or(1.0);
                    match after {
                        Some(after) => s.dots.push((at + after, at + after + length)),
                        None => {
                            s.focus.push((at, Focus::Dots));
                            s.dots.push((at, at + length));
                            at += length;
                        }
                    }
                }
                Op::Type {
                    text: chars,
                    panic: level,
                } => {
                    let (from, to) = level.unwrap_or((panic, panic));
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
                Op::Careful {
                    text: chars,
                    pace: gap,
                } => {
                    s.focus.push((at, Focus::Field));
                    // A breath first.
                    at += 0.45;
                    for (i, c) in chars.chars().enumerate() {
                        n += 1;
                        let (key, shift) = key_for(c, caps);
                        text.insert(caret, c);
                        caret += c.len_utf8();
                        let step = if i == 0 {
                            0.0
                        } else {
                            gap * (0.92 + 0.16 * noise(n, 2.0)) + if shift { 0.035 } else { 0.0 }
                        };
                        at += step;
                        began.get_or_insert(at);
                        stroke(
                            &mut s,
                            at,
                            step.max(gap),
                            key,
                            shift.then_some("shift"),
                            &text,
                            caret,
                            CAREFUL,
                        );
                    }
                }
                Op::Flare => s.flares.push(at),
                Op::Delete(count) => {
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
                Op::Send { slip, meant } => {
                    let gap = if slip { 0.11 } else { 0.16 };
                    at += gap;
                    let sent = std::mem::take(&mut text);
                    if let Some(meant) = meant
                        && sent != meant
                    {
                        let wrong = first_wrong(&s.strokes[reply_from..], &sent, meant);
                        trouble(&mut s, wrong.unwrap_or(at));
                    }
                    caret = 0;
                    stroke(&mut s, at, gap, "return", None, "", 0, panic.max(HARD));
                    s.typos += meant.map_or(usize::from(swapped.is_some()), |meant| {
                        scene_script::typos(&sent, meant)
                    });
                    let i = push(
                        &mut s,
                        &mut pending,
                        &mut labels,
                        Line {
                            at,
                            who: Who::You,
                            text: sent,
                        },
                        None,
                    );
                    s.focus.push((at, Focus::Line(i)));
                    s.knocks.push((at, if slip { 6.0 } else { 3.5 }));
                    if slip {
                        s.slip.get_or_insert(i);
                        s.flashes.push(at);
                        s.shudders.push((at, 3.0));
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
                    reply_from = s.strokes.len();
                    at += 0.35;
                }
                Op::Read => {
                    let line = s.lines.iter().rposition(|l| l.who == Who::You).unwrap_or(0);
                    s.focus.push((at, Focus::Read(line)));
                    s.reads.push((at, line));
                }
                Op::Tap {
                    line,
                    taps: ref glyphs,
                } => {
                    let mut last = at + TAP_FIRST - TAP_STEP;
                    for &(glyph, after) in glyphs {
                        last = after.map_or(last + TAP_STEP, |after| at + after);
                        taps.push((last, glyph, line, s.lines.len()));
                    }
                }
                Op::Elbow => {
                    n += 1;
                    caps = !caps;
                    s.caps.push(at);
                    trouble(&mut s, at);
                    for key in ELBOW {
                        stroke(&mut s, at, 0.2, key, None, &text, caret, 1.0);
                    }
                    s.shudders.push((at, 4.0));
                }
                Op::Autocorrect { from, to } => {
                    // Time slows hard as Return starts down, and the next
                    // send ends it.
                    s.focus.push((at, Focus::Field));
                    slow = Some(at);
                    at += SLOW_BEFORE;
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
                        trouble(&mut s, at);
                    }
                    at += SLOW_AFTER;
                }
                Op::Status(copy) | Op::Rename(copy) => {
                    let copy = if matches!(op, Op::Rename(_)) {
                        s.renames.push((at, copy));
                        title = copy;
                        renamed(script.boss(), copy)
                    } else {
                        copy.to_string()
                    };
                    let hold = t.read(&copy);
                    let i = push(
                        &mut s,
                        &mut pending,
                        &mut labels,
                        Line {
                            at,
                            who: Who::Status,
                            text: copy,
                        },
                        None,
                    );
                    s.focus.push((at, Focus::Line(i)));
                    at += hold;
                }
                Op::End { ending, text: copy } => {
                    // A draft cut off mid-word was never sent.
                    began = None;
                    s.ending = ending;
                    let i = push(
                        &mut s,
                        &mut pending,
                        &mut labels,
                        Line {
                            at,
                            who: Who::System,
                            text: copy
                                .map_or_else(|| ending.line(script.boss(), title), str::to_string),
                        },
                        None,
                    );
                    s.focus.push((at, Focus::Line(i)));
                    s.knocks.push((at, 4.0));
                    at += t.fired;
                }
            }
        }
        // Anyone cutting in after the removal is never heard.
        drop(pending);
        for (when, glyph, label, written) in taps {
            let line = match label {
                Some(name) => labels.iter().find(|(n, _)| *n == name).map(|&(_, i)| i),
                None => s.lines[..written].iter().rposition(|l| l.at <= when),
            };
            if let Some(line) = line {
                s.taps.push(Tap {
                    at: when,
                    line,
                    glyph,
                });
            }
        }
        s.focus.sort_by(|a, b| a.0.total_cmp(&b.0));
        s.taps.sort_by(|a, b| a.at.total_cmp(&b.at));
        s.end = at;
        s
    }

    /// The thread's name at `t`.
    pub(crate) fn title(&self, t: f64) -> &'static str {
        self.renames
            .iter()
            .take_while(|&&(at, _)| at <= t)
            .last()
            .map_or(self.script.title, |&(_, name)| name)
    }

    /// Why the field is gone once you're cut off.
    pub(crate) fn locked(&self) -> String {
        self.ending.locked(self.script.boss())
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

    /// When each `"Read"` shows, from the start.
    pub(crate) fn reads(&self) -> impl Iterator<Item = f64> + '_ {
        self.reads.iter().map(|&(at, _)| at)
    }

    /// The line the newest `"Read"` sits under at `t`.
    pub(crate) fn read(&self, t: f64) -> Option<usize> {
        self.reads
            .iter()
            .take_while(|&&(at, _)| at <= t)
            .last()
            .map(|&(_, line)| line)
    }

    /// The presenting pill's flare at `t`, `0..=1`.
    pub(crate) fn flare(&self, t: f64) -> f32 {
        self.flares
            .iter()
            .map(|&at| {
                let u = (t - at) / FLARE;
                if (0.0..1.0).contains(&u) {
                    ((1.0 - u) * (1.0 - u)) as f32
                } else {
                    0.0
                }
            })
            .fold(0.0, f32::max)
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
                Focus::Read(i) | Focus::Line(i) => Some(i) == self.slip,
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

    /// The first moment the typing goes wrong, else the first key.
    pub(crate) fn trouble(&self) -> Option<f64> {
        self.trouble.or_else(|| self.strokes.first().map(|s| s.at))
    }

    /// The heartbeat under the typing: from the first moment it goes wrong
    /// ([`Self::trouble`]), quicker and louder with panic, one long low beat
    /// in slow motion, and never a beat still ringing when you're removed.
    pub(crate) fn heartbeats(&self) -> Vec<Beat> {
        let mut beats = Vec::new();
        let Some(mut at) = self.trouble() else {
            return beats;
        };
        let stop = self.removal().unwrap_or(self.end);
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

/// How long `"Without Hex"` may run, and how many tapbacks a line holds.
#[cfg(test)]
pub(crate) const LENGTH: std::ops::RangeInclusive<f64> = 26.0..=36.0;
#[cfg(test)]
pub(crate) const MAX_TAPS: usize = 4;

/// What a parsed scene breaks once it's played with `t`: its length and its
/// tapbacks. The payoff's limits are [`super::triumph::check`]'s.
#[cfg(test)]
pub(crate) fn check(t: &Timing, script: &'static Script<'static>) -> Vec<String> {
    let scene = Scene::play(t, script);
    let mut problems = Vec::new();
    if !LENGTH.contains(&scene.duration()) {
        problems.push(format!(
            "\"Without Hex\" runs {:.1} s; keep it between {} and {} s",
            scene.duration(),
            LENGTH.start(),
            LENGTH.end()
        ));
    }
    for (i, line) in scene.lines().iter().enumerate() {
        let taps = scene.all_taps().iter().filter(|tap| tap.line == i).count();
        if taps > MAX_TAPS {
            problems.push(format!(
                "{taps} tapbacks on \"{}\"; a line holds {MAX_TAPS}",
                line.text
            ));
        }
    }
    problems
}

fn removed_at(lines: &[Line]) -> Option<f64> {
    lines.iter().find(|l| l.who == Who::System).map(|l| l.at)
}

/// The notice when `who` renames the thread.
pub(crate) fn renamed(who: &str, name: &str) -> String {
    format!("{who} named the conversation \u{201c}{name}\u{201d}")
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
    fn a_tapback_stays_on_the_line_above_it() {
        // The second haha lands after Jay's next line, still on yours.
        let src = "title Room\nboss Jay coral\nJay: so?\nstart\ntype ok\nsend\ntap haha 0.2 haha 0.8\nwait 0.3\nJay: STOP [0.5]\nend removed\nwith\nhex Yes, on my way.\nscore {hex}\ntyped {secs}\n";
        let script: &'static Script = Box::leak(Box::new(scene_script::parse(src).unwrap()));
        let scene = Scene::play(&Timing::default(), script);
        let mine = scene.lines().iter().position(|l| l.text == "ok").unwrap();
        assert_eq!(scene.all_taps().len(), 2);
        assert!(scene.all_taps().iter().all(|tap| tap.line == mine));
    }

    #[test]
    fn the_score_counts_what_went_out_wrong() {
        // "Yse, we're dead" 2, "*reayd" 1, "fart" 1, the autocorrect 1.
        assert_eq!(Scene::default().typos(), 5);
    }

    /// Every check a scene file gets: parsing, its limits, and the payoff's.
    fn validate(src: &'static str) -> Vec<String> {
        let script: &'static Script = match scene_script::parse(src) {
            Ok(script) => Box::leak(Box::new(script)),
            Err(errors) => return errors.0.iter().map(ToString::to_string).collect(),
        };
        let t = Timing::default();
        let send = super::super::versus::send_at(script.take(), true, &t);
        let mut problems = check(&t, script);
        problems.extend(super::super::triumph::check(script, send, &t));
        problems
    }

    #[test]
    fn every_scene_plays_in_its_window_and_scores_what_it_shows() {
        let t = Timing::default();
        for (name, src) in scene_script::SCENES {
            let problems = validate(src);
            // C and C-plus keep v3's approved 22-second "Without Hex".
            let problems: Vec<_> = problems
                .into_iter()
                .filter(|p| {
                    !(name.starts_with("C-") && p.contains("runs") && p.contains("Without"))
                })
                .collect();
            assert!(problems.is_empty(), "{name}: {problems:#?}");
            let script: &'static Script = Box::leak(Box::new(scene_script::bundled(name)));
            let scene = Scene::play(&t, script);
            assert!(scene.removed(scene.duration()), "{name} ends cut off");
            let typed = scene_script::fill(
                script.with.typed,
                script.take().release,
                scene.score_secs(),
                scene.typos(),
            );
            let typos = scene_script::counted(scene.typos(), "typo", "typos");
            assert!(
                !script.with.typed.contains("{typos}") || typed.contains(&typos),
                "{name}: {typed}"
            );
        }
        let a = Scene::play(
            &t,
            Box::leak(Box::new(scene_script::bundled("A-incontinence.scene"))),
        );
        assert_eq!(a.typos(), 5);
    }

    /// `HEX_VALIDATE_SCENES=<folder> scripts/check validate_scene_folder
    /// -- --nocapture` checks every `.scene` in a folder and prints a verdict
    /// each; without it, nothing runs.
    #[test]
    fn validate_scene_folder() {
        let Ok(dir) = std::env::var("HEX_VALIDATE_SCENES") else {
            return;
        };
        let mut paths: Vec<_> = std::fs::read_dir(&dir)
            .unwrap()
            .filter_map(|e| e.ok().map(|e| e.path()))
            .filter(|p| p.extension().is_some_and(|e| e == "scene"))
            .collect();
        paths.sort();
        for path in paths {
            let src: &'static str =
                Box::leak(std::fs::read_to_string(&path).unwrap().into_boxed_str());
            let problems = validate(src);
            let name = path.file_name().unwrap().to_string_lossy();
            if problems.is_empty() {
                let script = Box::leak(Box::new(scene_script::parse(src).unwrap()));
                let scene = Scene::play(&Timing::default(), script);
                println!(
                    "PASS {name} {:.1}s typos={}",
                    scene.duration(),
                    scene.typos()
                );
            } else {
                if let Ok(script) = scene_script::parse(src) {
                    let scene = Scene::play(&Timing::default(), Box::leak(Box::new(script)));
                    println!(
                        "FAIL {name} {:.1}s typos={}",
                        scene.duration(),
                        scene.typos()
                    );
                } else {
                    println!("FAIL {name}");
                }
                for p in problems {
                    println!("  {p}");
                }
            }
        }
    }

    #[test]
    fn every_joke_has_time_to_land() {
        let scene = Scene::default();
        let read = scene.reads().next().unwrap();
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
        let (read_at, line) = scene.reads[0];
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
        assert!(beats.len() >= 16, "{}", beats.len());
        // Not from the first key: from the first typo, `"Ys"` of `"Yse"`.
        let trouble = scene.trouble().unwrap();
        let wrong = scene.strokes().iter().find(|s| s.at == trouble).unwrap();
        assert_eq!(wrong.text, "Ys");
        assert!(trouble > scene.strokes()[0].at + 0.5);
        assert_eq!(beats[0].at, trouble);
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
