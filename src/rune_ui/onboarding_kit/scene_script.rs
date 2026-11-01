//! The plain-text scene files in `scenes/` (syntax in `scenes/README.md`):
//! one `"Without Hex"` disaster and its `"With Hex"` triumph each, parsed
//! into a [`Script`] that [`typing::Scene`](super::typing::Scene) plays.
//!
//! [`parse`] reports every mistake with its line number. Renders pick a
//! scene with `HEX_ONBOARDING_SCENE` (a path) or `HEX_ONBOARDING_VARIANT`
//! (`A`, `B`, `C`, `C+`); everyone else gets `default.scene`.

use std::fmt;
use std::sync::OnceLock;

use super::fixture::{ANSWER_JAY, Take};
use super::typing::Glyph;

/// Every bundled scene, by file name.
pub(crate) const SCENES: [(&str, &str); 5] = [
    ("default.scene", include_str!("scenes/default.scene")),
    (
        "A-incontinence.scene",
        include_str!("scenes/A-incontinence.scene"),
    ),
    ("B-dirty.scene", include_str!("scenes/B-dirty.scene")),
    ("C-v3.scene", include_str!("scenes/C-v3.scene")),
    ("C-plus.scene", include_str!("scenes/C-plus.scene")),
];

/// The colours a person can have: OpenCode theme scales.
pub(crate) const COLOURS: [&str; 10] = [
    "amber", "apple", "blue", "cobalt", "coral", "ember", "lilac", "mint", "solaris", "yuzu",
];

/// Who has no colour of their own, in order.
const DEFAULT_COLOURS: [&str; 4] = ["cobalt", "lilac", "amber", "mint"];

/// How many side characters a scene can have.
const SIDES: usize = 3;

/// The most lines `"Without Hex"`'s thread holds, and `"With Hex"`'s
/// payoff.
pub(crate) const MAX_LINES: usize = 18;
pub(crate) const MAX_CHEERS: usize = 8;

/// The careful pace when a scene gives none, seconds per key.
const CAREFUL_PACE: f64 = 0.09;

/// One person in the thread: `0` is the boss.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct Person<'a> {
    pub(crate) name: &'a str,
    /// One of [`COLOURS`].
    pub(crate) colour: &'a str,
}

/// Who says a line: someone in [`Script::people`], or you.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Speaker {
    Them(u8),
    You,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) enum Wait {
    Secs(f64),
    /// The long silence after something terrible goes out.
    Freeze,
}

/// How you get cut off at the end.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Ending {
    Removed,
    Left,
    Blocked,
    Undelivered,
}

impl Ending {
    pub(crate) const ALL: [(&'static str, Ending); 4] = [
        ("removed", Ending::Removed),
        ("left", Ending::Left),
        ("blocked", Ending::Blocked),
        ("undelivered", Ending::Undelivered),
    ];

    /// The system line, unless the scene words it.
    pub(crate) fn line(self, boss: &str, title: &str) -> String {
        match self {
            Ending::Removed => format!("{boss} removed you from \u{201c}{title}\u{201d}"),
            Ending::Left => format!("{boss} left the conversation"),
            Ending::Blocked => format!("{boss} blocked you"),
            Ending::Undelivered => "Message not delivered".to_string(),
        }
    }

    /// Why the field is gone.
    pub(crate) fn locked(self, boss: &str) -> String {
        match self {
            Ending::Removed => "You are no longer in this conversation.".to_string(),
            Ending::Left => "There's no one left to read this.".to_string(),
            Ending::Blocked => format!("You can't message {boss}."),
            Ending::Undelivered => "Your messages aren't getting through.".to_string(),
        }
    }
}

/// A celebration over the stage in `"With Hex"`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Effect {
    Confetti,
    Fireworks,
}

/// Tapbacks, each at a time from now if the scene gives one.
pub(crate) type Taps = Vec<(Glyph, Option<f64>)>;

/// One step of `"Without Hex"`.
#[derive(Clone, Debug, PartialEq)]
pub(crate) enum Op<'a> {
    /// Someone's line. `hold`: how long the scene holds after it, else its
    /// reading time. Before `start`, already in the thread.
    Says {
        who: Speaker,
        text: &'a str,
        hold: Option<f64>,
        label: Option<&'a str>,
    },
    /// The button: the clock starts here.
    Start,
    /// A line landing `after` seconds from now while what follows goes on.
    Cut {
        after: f64,
        who: Speaker,
        text: &'a str,
        label: Option<&'a str>,
    },
    /// Typed by hand, panic rising from the first level to the second, or
    /// staying where it was.
    Type {
        text: &'a str,
        panic: Option<(f64, f64)>,
    },
    /// Slow, even typing, a key every `pace` seconds.
    Careful {
        text: &'a str,
        pace: f64,
    },
    Delete(usize),
    Wait(Wait),
    /// The draft goes out; `meant` is what it should have said.
    Send {
        slip: bool,
        meant: Option<&'a str>,
    },
    /// Slow motion, then autocorrect swaps the last `from` for `to`.
    Autocorrect {
        from: &'a str,
        to: &'a str,
    },
    Read,
    /// The boss's typing dots; `after` starts them later without waiting.
    Dots {
        secs: Option<f64>,
        after: Option<f64>,
    },
    Flare,
    Elbow,
    /// Tapbacks on a named line, else the newest.
    Tap {
        line: Option<&'a str>,
        taps: Taps,
    },
    /// A neutral system line, such as `"Mom changed the group photo"`.
    Status(&'a str),
    /// The thread's new name, with a system line.
    Rename(&'a str),
    End {
        ending: Ending,
        text: Option<&'a str>,
    },
}

/// One step of `"With Hex"`, after your spoken line.
#[derive(Clone, Debug, PartialEq)]
pub(crate) enum Cheer<'a> {
    /// Tapbacks on your spoken line.
    Tap(Taps),
    /// A reply, `gap` seconds after the last step, else quickly.
    Says {
        who: Speaker,
        text: &'a str,
        gap: Option<f64>,
    },
    Status(&'a str),
    Rename(&'a str),
    /// The pill under the header changes as the last step lands, or goes
    /// (`pill off`).
    Pill(Option<&'a str>),
    Effect(Effect),
    Wait(f64),
}

#[derive(Clone, Debug, PartialEq)]
pub(crate) struct Triumph<'a> {
    /// What you say, holding the shortcut.
    pub(crate) hex: &'a str,
    pub(crate) cheers: Vec<Cheer<'a>>,
    /// The score's title; `{hex}` is how long you spoke.
    pub(crate) score: &'a str,
    /// The line under it; `{secs}` and `{typos}` come from the run.
    pub(crate) typed: &'a str,
}

#[derive(Debug)]
pub(crate) struct Script<'a> {
    pub(crate) title: &'a str,
    /// The subtitle of a group; `None` for a one-on-one.
    pub(crate) group: Option<&'a str>,
    /// The boss first, then up to three side characters.
    pub(crate) people: Vec<Person<'a>>,
    /// The red status pill under the header.
    pub(crate) pill: Option<&'a str>,
    /// The line under `"Without Hex"`.
    pub(crate) lead: &'a str,
    pub(crate) ops: Vec<Op<'a>>,
    pub(crate) with: Triumph<'a>,
    take: OnceLock<&'static Take>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct ParseError {
    pub(crate) line: usize,
    pub(crate) message: String,
}

impl fmt::Display for ParseError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "line {}: {}", self.line, self.message)
    }
}

/// All of a file's mistakes, one per line.
pub(crate) struct ParseErrors(pub(crate) Vec<ParseError>);

impl fmt::Display for ParseErrors {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        for (i, e) in self.0.iter().enumerate() {
            if i > 0 {
                writeln!(f)?;
            }
            write!(f, "{e}")?;
        }
        Ok(())
    }
}

impl fmt::Debug for ParseErrors {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        fmt::Display::fmt(self, f)
    }
}

impl Script<'static> {
    /// The scene this run plays: a render's choice, else `default.scene`.
    /// Read once.
    pub(crate) fn chosen() -> &'static Script<'static> {
        static CHOSEN: OnceLock<Script<'static>> = OnceLock::new();
        CHOSEN.get_or_init(|| {
            if let Ok(path) = std::env::var("HEX_ONBOARDING_SCENE")
                && !path.is_empty()
            {
                match std::fs::read_to_string(&path) {
                    Ok(src) => match parse(Box::leak(src.into_boxed_str())) {
                        Ok(script) => return script,
                        Err(errors) => tracing::warn!(path, %errors, "onboarding scene"),
                    },
                    Err(error) => tracing::warn!(path, %error, "onboarding scene"),
                }
            }
            let name = match std::env::var("HEX_ONBOARDING_VARIANT").as_deref() {
                Ok("A" | "a" | "incontinence") => "A-incontinence.scene",
                Ok("B" | "b" | "dirty") => "B-dirty.scene",
                Ok("C" | "c" | "v3") => "C-v3.scene",
                Ok("C+" | "c+" | "C-plus" | "plus") => "C-plus.scene",
                _ => "default.scene",
            };
            bundled(name)
        })
    }
}

/// A bundled scene, parsed. Every one is checked by the tests.
pub(crate) fn bundled(name: &str) -> Script<'static> {
    let (_, src) = SCENES
        .iter()
        .find(|(n, _)| *n == name)
        .expect("a bundled scene");
    parse(src).unwrap_or_else(|errors| panic!("{name}:\n{errors}"))
}

impl Script<'_> {
    pub(crate) fn boss(&self) -> &str {
        self.people[0].name
    }

    pub(crate) fn person(&self, who: u8) -> Person<'_> {
        self.people[usize::from(who)]
    }

    /// Your spoken line as a take: the fixture's recorded timings when it's
    /// that sentence, else even word timings at an easy speaking pace.
    pub(crate) fn take(&self) -> &'static Take {
        self.take.get_or_init(|| {
            if self.with.hex == ANSWER_JAY.text() {
                return &ANSWER_JAY;
            }
            let words: Vec<&str> = self.with.hex.split_whitespace().collect();
            let release = (0.1 + 0.2 * words.len() as f64).max(1.0);
            let step = (release - 0.26) / words.len().max(1) as f64;
            let updates: Vec<(f64, &'static str)> = (1..=words.len())
                .map(|n| {
                    let partial: &'static str = Box::leak(words[..n].join(" ").into_boxed_str());
                    (0.16 + step * (n - 1) as f64, partial)
                })
                .collect();
            Box::leak(Box::new(Take {
                updates: Box::leak(updates.into_boxed_slice()),
                release: (release * 10.0).round() / 10.0,
            }))
        })
    }
}

/// How many words of `sent` differ from `meant`: the typos it went out with.
pub(crate) fn typos(sent: &str, meant: &str) -> usize {
    let a: Vec<&str> = sent.split_whitespace().collect();
    let b: Vec<&str> = meant.split_whitespace().collect();
    a.iter().zip(&b).filter(|(x, y)| x != y).count() + a.len().abs_diff(b.len())
}

/// `"27 seconds"`, `"1 typo"`, `"no typos"`.
pub(crate) fn counted(n: usize, one: &str, many: &str) -> String {
    match n {
        0 => format!("no {many}"),
        1 => format!("1 {one}"),
        n => format!("{n} {many}"),
    }
}

/// Fills `{hex}`, `{secs}` and `{typos}` in a score line.
pub(crate) fn fill(template: &str, hex: f64, secs: f64, typos: usize) -> String {
    template
        .replace("{hex}", &format!("{hex:.1} seconds"))
        .replace("{secs}", &format!("{} seconds", secs.round()))
        .replace("{typos}", &counted(typos, "typo", "typos"))
}

#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
enum Part {
    Setting,
    Before,
    Without,
    Ended,
    With,
}

struct Parser<'a> {
    errors: Vec<ParseError>,
    line: usize,
    part: Part,
    title: Option<&'a str>,
    group: Option<&'a str>,
    boss: Option<Person<'a>>,
    sides: Vec<Person<'a>>,
    pill: Option<&'a str>,
    lead: Option<&'a str>,
    ops: Vec<Op<'a>>,
    hex: Option<&'a str>,
    cheers: Vec<Cheer<'a>>,
    score: Option<&'a str>,
    typed: Option<&'a str>,
    /// What's in the field, to check deletes, sends and autocorrect.
    draft: String,
    labels: Vec<&'a str>,
}

/// Parses a scene file, or reports every mistake in it.
pub(crate) fn parse(src: &str) -> Result<Script<'_>, ParseErrors> {
    let mut p = Parser {
        errors: Vec::new(),
        line: 0,
        part: Part::Setting,
        title: None,
        group: None,
        boss: None,
        sides: Vec::new(),
        pill: None,
        lead: None,
        ops: Vec::new(),
        hex: None,
        cheers: Vec::new(),
        score: None,
        typed: None,
        draft: String::new(),
        labels: Vec::new(),
    };
    for (i, raw) in src.lines().enumerate() {
        p.line = i + 1;
        let line = strip_comment(raw).trim();
        if line.is_empty() {
            continue;
        }
        let (ops, cheers) = (p.ops.len(), p.cheers.len());
        if let Err(message) = p.command(line).and_then(|()| p.limits(ops, cheers)) {
            p.errors.push(ParseError {
                line: p.line,
                message,
            });
        }
    }
    p.line = src.lines().count();
    p.finish()
}

impl Parser<'_> {
    /// What a scene can only do so much of, checked as each command adds to
    /// the `ops` and `cheers` there were.
    fn limits(&self, ops: usize, cheers: usize) -> Result<(), String> {
        if let Some(op) = self.ops.get(ops) {
            let count = |f: fn(&Op) -> bool| self.ops.iter().filter(|op| f(op)).count();
            if matches!(op, Op::Autocorrect { .. })
                && count(|op| matches!(op, Op::Autocorrect { .. })) > 1
            {
                return Err("only one autocorrect per scene".into());
            }
            if matches!(op, Op::Elbow) && count(|op| matches!(op, Op::Elbow)) > 1 {
                return Err("only one elbow per scene".into());
            }
            let line = |op: &Op| {
                matches!(
                    op,
                    Op::Says { .. }
                        | Op::Cut { .. }
                        | Op::Send { .. }
                        | Op::Status(_)
                        | Op::Rename(_)
                        | Op::End { .. }
                )
            };
            if line(op) && self.ops.iter().filter(|op| line(op)).count() == MAX_LINES + 1 {
                return Err(format!(
                    "too many lines: \"Without Hex\" holds {MAX_LINES}, counting yours and the ending"
                ));
            }
        }
        if self.cheers.len() > cheers {
            let lines = self
                .cheers
                .iter()
                .filter(|c| matches!(c, Cheer::Says { .. } | Cheer::Status(_) | Cheer::Rename(_)))
                .count();
            if lines == MAX_CHEERS + 1 {
                return Err(format!(
                    "too many cheers: \"With Hex\" holds {MAX_CHEERS} lines"
                ));
            }
        }
        Ok(())
    }
}

/// Cuts a `#` comment: at the start of a line, or after a space and
/// followed by one (so `[#desk]` and `#1` stay).
fn strip_comment(line: &str) -> &str {
    if line.trim_start().starts_with('#') {
        return "";
    }
    let bytes = line.as_bytes();
    for (i, &b) in bytes.iter().enumerate() {
        if b == b'#'
            && i > 0
            && bytes[i - 1].is_ascii_whitespace()
            && bytes.get(i + 1).is_none_or(u8::is_ascii_whitespace)
        {
            return &line[..i];
        }
    }
    line
}

fn number(token: &str) -> Result<f64, String> {
    match token.parse::<f64>() {
        Ok(n) if (0.0..=10.0).contains(&n) => Ok(n),
        Ok(n) => Err(format!(
            "{n} is out of range: times run from 0 to 10 seconds"
        )),
        Err(_) => Err(format!("expected a number of seconds, found `{token}`")),
    }
}

/// Splits a trailing `[options]` off `text`.
fn options(text: &str) -> (&str, Option<&str>) {
    let text = text.trim();
    if let Some(body) = text.strip_suffix(']')
        && let Some(open) = body.rfind('[')
    {
        return (body[..open].trim_end(), Some(body[open + 1..].trim()));
    }
    (text, None)
}

fn glyph(token: &str) -> Option<Glyph> {
    Some(match token.trim_end_matches('\u{fe0f}') {
        "❤" | "heart" => Glyph::Heart,
        "👍" | "like" => Glyph::ThumbsUp,
        "👎" | "dislike" => Glyph::ThumbsDown,
        "‼" | "!!" => Glyph::Bang,
        "?" => Glyph::Question,
        "haha" | "Haha" => Glyph::Haha,
        _ => return None,
    })
}

fn taps(tokens: &[&str]) -> Result<Taps, String> {
    let mut out: Taps = Vec::new();
    for token in tokens {
        if let Some(g) = glyph(token) {
            out.push((g, None));
        } else if let Ok(at) = token.parse::<f64>() {
            match out.last_mut() {
                Some((_, slot @ None)) => *slot = Some(number(&at.to_string())?),
                _ => return Err(format!("`{token}` needs a tapback before it")),
            }
        } else {
            return Err(format!(
                "`{token}` isn't a tapback; use ❤ 👍 👎 ‼ ? or haha, each with an optional time"
            ));
        }
    }
    if out.is_empty() {
        return Err("`tap` needs at least one tapback, such as `tap ❤ 0.4`".into());
    }
    Ok(out)
}

const COMMANDS: [&str; 26] = [
    "title",
    "group",
    "boss",
    "side",
    "pill",
    "lead",
    "start",
    "cut",
    "type",
    "careful",
    "delete",
    "wait",
    "send",
    "autocorrect",
    "read",
    "dots",
    "flare",
    "elbow",
    "tap",
    "status",
    "rename",
    "end",
    "with",
    "hex",
    "effect",
    "score",
];

impl<'a> Parser<'a> {
    fn command(&mut self, line: &'a str) -> Result<(), String> {
        let (word, rest) = line
            .split_once(char::is_whitespace)
            .map_or((line, ""), |(w, r)| (w, r.trim()));
        if COMMANDS.contains(&word) || word == "typed" {
            return self.keyword(word, rest);
        }
        if let Some((name, text)) = line.split_once(':') {
            let who = self.speaker(name.trim())?;
            return self.says(who, text.trim());
        }
        Err(format!(
            "unknown command `{word}`; a line someone says looks like `Name: text`"
        ))
    }

    fn speaker(&self, name: &str) -> Result<Speaker, String> {
        if name.eq_ignore_ascii_case("you") {
            return Ok(Speaker::You);
        }
        let people = self.boss.iter().chain(&self.sides);
        match people.clone().position(|p| p.name == name) {
            Some(i) => Ok(Speaker::Them(i as u8)),
            None if self.boss.is_none() => Err(format!(
                "`{name}` speaks before anyone is set up; start with `boss {name}`"
            )),
            None => Err(format!(
                "`{name}` isn't in this scene; add `side {name}` at the top"
            )),
        }
    }

    fn setting(&self, word: &str) -> Result<(), String> {
        if self.part != Part::Setting {
            return Err(format!("`{word}` belongs at the top, before anyone speaks"));
        }
        Ok(())
    }

    fn without(&self, word: &str) -> Result<(), String> {
        match self.part {
            Part::Without => Ok(()),
            Part::Setting | Part::Before => Err(format!("`{word}` happens after `start`")),
            Part::Ended => Err(format!(
                "`{word}` comes after `end`; `end` is the last line"
            )),
            Part::With => Err(format!("`{word}` doesn't work after `with`")),
        }
    }

    fn person(&self, rest: &'a str, used: usize) -> Result<Person<'a>, String> {
        let (name, colour) = match rest.rsplit_once(char::is_whitespace) {
            Some((name, colour)) if COLOURS.contains(&colour) => (name.trim(), colour),
            // A lowercase last word reads as a colour, not part of a name.
            Some((_, colour)) if colour.chars().all(|c| c.is_ascii_lowercase()) => {
                return Err(format!(
                    "no colour `{colour}`; use one of {}",
                    COLOURS.join(" ")
                ));
            }
            _ => (rest, DEFAULT_COLOURS[used % DEFAULT_COLOURS.len()]),
        };
        if name.is_empty() {
            return Err("give the person a name".into());
        }
        if name.contains(':') {
            return Err("a name can't have `:` in it".into());
        }
        Ok(Person { name, colour })
    }

    fn keyword(&mut self, word: &str, rest: &'a str) -> Result<(), String> {
        let needs = |what: &str| -> Result<(), String> {
            if rest.is_empty() {
                Err(format!("`{word}` needs {what}"))
            } else {
                Ok(())
            }
        };
        match word {
            "title" => {
                self.setting(word)?;
                needs("the thread's name")?;
                self.title = Some(rest);
            }
            "group" => {
                self.setting(word)?;
                needs("a subtitle, such as `Jay, you, and 3 investors`")?;
                self.group = Some(rest);
            }
            "boss" => {
                self.setting(word)?;
                needs("a name")?;
                if self.boss.is_some() {
                    return Err("there's one boss; use `side` for the others".into());
                }
                self.boss = Some(self.person(rest, 0)?);
            }
            "side" => {
                self.setting(word)?;
                needs("a name")?;
                if self.sides.len() == SIDES {
                    return Err(format!("at most {SIDES} side characters"));
                }
                let person = self.person(rest, self.sides.len() + 1)?;
                self.sides.push(person);
            }
            "pill" if self.part == Part::With => {
                needs("its new words, or `off`")?;
                if self.pill.is_none() {
                    return Err(
                        "`pill` here changes the one set at the top; set one there first".into(),
                    );
                }
                self.cheers
                    .push(Cheer::Pill((rest != "off").then_some(rest)));
            }
            "pill" => {
                self.setting(word)?;
                needs("its words, such as `Jay is presenting`")?;
                self.pill = Some(rest);
            }
            "lead" => {
                self.setting(word)?;
                needs("the line under the title")?;
                self.lead = Some(rest);
            }
            "start" => {
                if self.part >= Part::Without {
                    return Err("`start` comes once".into());
                }
                if self.boss.is_none() {
                    return Err("set up the `boss` before `start`".into());
                }
                self.part = Part::Without;
                self.ops.push(Op::Start);
            }
            "cut" => {
                self.without(word)?;
                let (after, said) = rest
                    .split_once(char::is_whitespace)
                    .ok_or("`cut` looks like `cut 0.5 Jay: text`")?;
                let after = number(after)?;
                let (name, text) = said
                    .split_once(':')
                    .ok_or("`cut` looks like `cut 0.5 Jay: text`")?;
                let who = self.speaker(name.trim())?;
                let (text, opts) = options(text);
                let label = match opts {
                    Some(o) => Some(self.label(o)?),
                    None => None,
                };
                if text.is_empty() {
                    return Err("the line is empty".into());
                }
                self.ops.push(Op::Cut {
                    after,
                    who,
                    text,
                    label,
                });
            }
            "type" => {
                self.without(word)?;
                let (text, opts) = options(rest);
                if text.is_empty() {
                    return Err("`type` needs the keys that were hit".into());
                }
                let panic = match opts {
                    None => None,
                    Some(o) => {
                        let level = o
                            .strip_prefix("panic")
                            .ok_or(format!("unknown option `[{o}]`; try `[panic 0.2-0.9]`"))?
                            .trim();
                        let (a, b) = level.split_once('-').unwrap_or((level, level));
                        let (a, b) = (number(a)?, number(b)?);
                        if a > 1.0 || b > 1.0 {
                            return Err("panic runs from 0 (calm) to 1 (frantic)".into());
                        }
                        Some((a, b))
                    }
                };
                self.draft.push_str(text);
                self.ops.push(Op::Type { text, panic });
            }
            "careful" => {
                self.without(word)?;
                let (text, opts) = options(rest);
                if text.is_empty() {
                    return Err("`careful` needs the keys that were hit".into());
                }
                let pace = match opts {
                    None => CAREFUL_PACE,
                    Some(o) => number(
                        o.strip_prefix("pace")
                            .ok_or(format!("unknown option `[{o}]`; try `[pace 0.09]`"))?
                            .trim(),
                    )?,
                };
                self.draft.push_str(text);
                self.ops.push(Op::Careful { text, pace });
            }
            "delete" => {
                self.without(word)?;
                let n: usize = rest
                    .parse()
                    .map_err(|_| format!("`delete` needs a count, found `{rest}`"))?;
                let have = self.draft.chars().count();
                if n == 0 || n > have {
                    return Err(format!(
                        "can't delete {n}: the draft is `{}` ({have} characters)",
                        self.draft
                    ));
                }
                for _ in 0..n {
                    self.draft.pop();
                }
                self.ops.push(Op::Delete(n));
            }
            "wait" => {
                let wait = if rest == "freeze" {
                    Wait::Freeze
                } else {
                    Wait::Secs(number(rest)?)
                };
                if self.part == Part::With {
                    match wait {
                        Wait::Secs(secs) => self.cheers.push(Cheer::Wait(secs)),
                        Wait::Freeze => return Err("there's no freezing after `with`".into()),
                    }
                    return Ok(());
                }
                self.without(word)?;
                self.ops.push(Op::Wait(wait));
            }
            "send" => {
                self.without(word)?;
                let (slip, rest) = match rest.strip_prefix("slip") {
                    Some(r) if r.is_empty() || r.starts_with(char::is_whitespace) => {
                        (true, r.trim())
                    }
                    _ => (false, rest),
                };
                let meant = if rest.is_empty() {
                    None
                } else {
                    Some(
                        rest.strip_prefix("meant")
                            .filter(|m| m.starts_with(char::is_whitespace))
                            .map(str::trim)
                            .ok_or(format!(
                                "`send` takes `slip` and `meant <text>`, found `{rest}`"
                            ))?,
                    )
                };
                if self.draft.trim().is_empty() {
                    return Err("nothing to send: the draft is empty".into());
                }
                self.draft.clear();
                self.ops.push(Op::Send { slip, meant });
            }
            "autocorrect" => {
                self.without(word)?;
                let (from, to) = rest
                    .split_once("->")
                    .map(|(a, b)| (a.trim(), b.trim()))
                    .filter(|(a, b)| !a.is_empty() && !b.is_empty())
                    .ok_or("`autocorrect` looks like `autocorrect inconvience -> incontinence`")?;
                let Some(at) = self.draft.rfind(from) else {
                    return Err(format!("`{from}` isn't in the draft `{}`", self.draft));
                };
                self.draft.replace_range(at..at + from.len(), to);
                self.ops.push(Op::Autocorrect { from, to });
            }
            "read" | "flare" | "elbow" => {
                self.without(word)?;
                if !rest.is_empty() {
                    return Err(format!("`{word}` takes nothing after it"));
                }
                self.ops.push(match word {
                    "read" => Op::Read,
                    "flare" => Op::Flare,
                    _ => Op::Elbow,
                });
            }
            "dots" => {
                self.without(word)?;
                let tokens: Vec<&str> = rest.split_whitespace().collect();
                let (secs, after) = match tokens.as_slice() {
                    [] => (None, None),
                    [secs] => (Some(number(secs)?), None),
                    [secs, "after", after] => (Some(number(secs)?), Some(number(after)?)),
                    _ => {
                        return Err(
                            "`dots` looks like `dots`, `dots 0.5` or `dots 0.4 after 0.2`".into(),
                        );
                    }
                };
                self.ops.push(Op::Dots { secs, after });
            }
            "tap" => {
                let tokens: Vec<&str> = rest.split_whitespace().collect();
                let (line, tokens) = match tokens.split_first() {
                    Some((first, rest)) if first.starts_with('@') => {
                        let name = &first[1..];
                        if !self.labels.contains(&name) {
                            return Err(format!(
                                "no line is named `#{name}` yet; name one with `[#{name}]`"
                            ));
                        }
                        (Some(name), rest)
                    }
                    _ => (None, tokens.as_slice()),
                };
                let taps = taps(tokens)?;
                if self.part == Part::With {
                    if line.is_some() {
                        return Err("after `with`, tapbacks land on your spoken line".into());
                    }
                    self.cheers.push(Cheer::Tap(taps));
                    return Ok(());
                }
                if self.part < Part::Before && line.is_none() {
                    return Err("`tap` needs a line to land on".into());
                }
                if self.part >= Part::Ended {
                    return Err("`tap` comes after `end`; `end` is the last line".into());
                }
                self.ops.push(Op::Tap { line, taps });
            }
            "status" | "rename" => {
                needs(if word == "status" {
                    "its words, such as `Jay made you CEO`"
                } else {
                    "the new name"
                })?;
                if self.part == Part::With {
                    self.cheers.push(if word == "status" {
                        Cheer::Status(rest)
                    } else {
                        Cheer::Rename(rest)
                    });
                    return Ok(());
                }
                self.without(word)?;
                self.ops.push(if word == "status" {
                    Op::Status(rest)
                } else {
                    Op::Rename(rest)
                });
            }
            "end" => {
                self.without(word)?;
                let (kind, text) = match rest.split_once(':') {
                    Some((kind, text)) => {
                        (kind.trim(), Some(text.trim()).filter(|t| !t.is_empty()))
                    }
                    None => (rest, None),
                };
                let ending = Ending::ALL
                    .iter()
                    .find(|(name, _)| *name == kind)
                    .map(|&(_, e)| e)
                    .ok_or(format!(
                        "unknown ending `{kind}`; use removed, left, blocked or undelivered"
                    ))?;
                self.part = Part::Ended;
                self.ops.push(Op::End { ending, text });
            }
            "with" => {
                if self.part != Part::Ended {
                    return Err("`with` comes after `end`".into());
                }
                if !rest.is_empty() {
                    return Err("`with` takes nothing after it".into());
                }
                self.part = Part::With;
            }
            "hex" | "effect" | "score" | "typed" => {
                if self.part != Part::With {
                    return Err(format!("`{word}` belongs after `with`"));
                }
                needs("something after it")?;
                match word {
                    "hex" if self.hex.is_some() => {
                        return Err("you speak once: one `hex` line".into());
                    }
                    "hex" if !self.cheers.is_empty() => {
                        return Err("`hex` comes first after `with`".into());
                    }
                    "hex" => self.hex = Some(rest),
                    "effect" => self.cheers.push(Cheer::Effect(match rest {
                        "confetti" => Effect::Confetti,
                        "fireworks" => Effect::Fireworks,
                        _ => {
                            return Err(format!(
                                "unknown effect `{rest}`; use confetti or fireworks"
                            ));
                        }
                    })),
                    "score" => {
                        placeholders(rest, &["hex"])?;
                        self.score = Some(rest);
                    }
                    _ => {
                        placeholders(rest, &["secs", "typos"])?;
                        self.typed = Some(rest);
                    }
                }
            }
            _ => unreachable!("every command is handled"),
        }
        Ok(())
    }

    fn label(&mut self, opts: &'a str) -> Result<&'a str, String> {
        let name = opts
            .strip_prefix('#')
            .filter(|n| !n.is_empty() && !n.contains(char::is_whitespace))
            .ok_or(format!(
                "unknown option `[{opts}]`; name a line with `[#name]`"
            ))?;
        if self.labels.contains(&name) {
            return Err(format!("two lines are named `#{name}`"));
        }
        self.labels.push(name);
        Ok(name)
    }

    fn says(&mut self, who: Speaker, text: &'a str) -> Result<(), String> {
        let (text, opts) = options(text);
        if text.is_empty() {
            return Err("the line is empty".into());
        }
        let mut time = None;
        let mut label = None;
        for token in opts.unwrap_or("").split_whitespace() {
            if token.starts_with('#') {
                label = Some(self.label(token)?);
            } else {
                time = Some(number(token)?);
            }
        }
        match self.part {
            Part::Setting | Part::Before => {
                if time.is_some() {
                    return Err("lines before `start` are already there: no times".into());
                }
                self.part = Part::Before;
                self.ops.push(Op::Says {
                    who,
                    text,
                    hold: None,
                    label,
                });
            }
            Part::Without => self.ops.push(Op::Says {
                who,
                text,
                hold: time,
                label,
            }),
            Part::Ended => return Err("lines come before `end`; `end` is the last line".into()),
            Part::With => {
                if label.is_some() {
                    return Err("after `with`, lines can't be named".into());
                }
                self.cheers.push(Cheer::Says {
                    who,
                    text,
                    gap: time,
                })
            }
        }
        Ok(())
    }

    fn finish(mut self) -> Result<Script<'a>, ParseErrors> {
        let mut missing = |what: &str| {
            self.errors.push(ParseError {
                line: self.line,
                message: format!("the scene has no {what}"),
            })
        };
        if self.title.is_none() {
            missing("`title`");
        }
        if self.boss.is_none() {
            missing("`boss`");
        }
        if self.part < Part::Without {
            missing("`start`");
        }
        if self.part < Part::Ended {
            missing("`end`");
        }
        if self.part < Part::With {
            missing("`with`");
        }
        if self.hex.is_none() {
            missing("`hex` line");
        }
        if self.score.is_none() {
            missing("`score`");
        }
        if self.typed.is_none() {
            missing("`typed` score");
        }
        if !self.errors.is_empty() {
            return Err(ParseErrors(self.errors));
        }
        let mut people = vec![self.boss.expect("checked")];
        people.extend(self.sides);
        Ok(Script {
            title: self.title.expect("checked"),
            group: self.group,
            people,
            pill: self.pill,
            lead: self.lead.unwrap_or("One quick reply by hand."),
            ops: self.ops,
            with: Triumph {
                hex: self.hex.expect("checked"),
                cheers: self.cheers,
                score: self.score.expect("checked"),
                typed: self.typed.expect("checked"),
            },
            take: OnceLock::new(),
        })
    }
}

fn placeholders(text: &str, allowed: &[&str]) -> Result<(), String> {
    let mut rest = text;
    while let Some(open) = rest.find('{') {
        let close = rest[open..].find('}').ok_or("a `{` without its `}`")?;
        let name = &rest[open + 1..open + close];
        if !allowed.contains(&name) {
            let list: Vec<String> = allowed.iter().map(|a| format!("{{{a}}}")).collect();
            return Err(format!("unknown `{{{name}}}`; use {}", list.join(" or ")));
        }
        rest = &rest[open + close + 1..];
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn errors(src: &str) -> Vec<String> {
        match parse(src) {
            Ok(_) => Vec::new(),
            Err(e) => e.0.iter().map(ToString::to_string).collect(),
        }
    }

    const TINY: &str = "title Dinner\nboss Sam coral\nSam: so?\nstart\nSam: well [0.4 #well]\ntype ys omw [panic 0.3-0.6]\nsend slip meant yes omw\ntap @well ?\nend blocked\nwith\nhex Yes, on my way.\ntap ❤ ❤ 0.5\nSam: wow\nstatus Sam pinned your message\neffect confetti\nscore {hex}, no typos\ntyped Typing: {secs}, {typos}\n";

    #[test]
    fn every_bundled_scene_parses() {
        for (name, src) in SCENES {
            if let Err(e) = parse(src) {
                panic!("{name}:\n{e}");
            }
        }
    }

    #[test]
    fn a_small_scene_reads_as_written() {
        let s = parse(TINY).unwrap();
        assert_eq!(s.title, "Dinner");
        assert_eq!(s.group, None);
        assert_eq!(
            s.people,
            vec![Person {
                name: "Sam",
                colour: "coral"
            }]
        );
        assert_eq!(
            s.ops[2],
            Op::Says {
                who: Speaker::Them(0),
                text: "well",
                hold: Some(0.4),
                label: Some("well")
            }
        );
        assert_eq!(
            s.ops[3],
            Op::Type {
                text: "ys omw",
                panic: Some((0.3, 0.6))
            }
        );
        assert_eq!(
            s.ops[4],
            Op::Send {
                slip: true,
                meant: Some("yes omw")
            }
        );
        assert_eq!(
            s.with.cheers[0],
            Cheer::Tap(vec![(Glyph::Heart, None), (Glyph::Heart, Some(0.5))])
        );
        assert_eq!(s.with.cheers[3], Cheer::Effect(Effect::Confetti));
    }

    #[test]
    fn mistakes_come_back_with_their_line_numbers() {
        let src = TINY
            .replace("tap @well ?", "tap @nope ?")
            .replace("send slip meant yes omw", "delete 9")
            .replace("effect confetti", "effect lasers")
            .replace("Sam: wow", "Kim: wow");
        let found = errors(&src);
        assert_eq!(found.len(), 4, "{found:#?}");
        assert!(found[0].starts_with("line 7: can't delete 9"), "{found:?}");
        assert!(found[1].starts_with("line 8: no line is named `#nope`"));
        assert!(found[2].starts_with("line 13: `Kim` isn't in this scene"));
        assert!(found[3].starts_with("line 15: unknown effect `lasers`"));
    }

    #[test]
    fn a_scene_has_to_be_whole() {
        let found = errors("title X\nboss Jo\nstart\n");
        assert!(found.iter().any(|e| e.contains("no `end`")), "{found:?}");
        assert!(found.iter().any(|e| e.contains("no `hex` line")));
        assert!(
            errors("title X\nstart\n")
                .iter()
                .any(|e| e.contains("`boss` before `start`"))
        );
        let found = errors(&TINY.replace("{typos}", "{oops}"));
        assert!(found[0].contains("unknown `{oops}`"), "{found:?}");
    }

    #[test]
    fn comments_go_but_names_and_hashes_in_words_stay() {
        assert_eq!(strip_comment("send slip   # oops"), "send slip   ");
        assert_eq!(strip_comment("Jay: hi [#desk]"), "Jay: hi [#desk]");
        assert_eq!(strip_comment("Jay: we're #1"), "Jay: we're #1");
        assert_eq!(strip_comment("  # all of it"), "");
    }

    #[test]
    fn typos_count_the_words_that_differ() {
        assert_eq!(typos("Yse, we're dead", "Yes, we're ready"), 2);
        assert_eq!(typos("yes", "yes"), 0);
        assert_eq!(typos("so sorry", "so very sorry"), 2);
        assert_eq!(
            fill("Typing: {secs}, {typos}.", 1.5, 26.6, 1),
            "Typing: 27 seconds, 1 typo."
        );
        assert_eq!(fill("{hex}, {typos}", 1.5, 0.0, 0), "1.5 seconds, no typos");
    }

    #[test]
    fn every_colour_is_in_the_theme() {
        for colour in COLOURS {
            for dark in [false, true] {
                rune_theme_opencode::scale(colour, dark, 11);
            }
        }
        let errors = errors(&TINY.replace("boss Sam coral", "boss Sam purple"));
        assert!(errors[0].contains("no colour `purple`"), "{errors:?}");
    }

    #[test]
    fn the_payoff_can_change_the_pill_set_at_the_top() {
        let with_pill = TINY.replace("boss Sam coral", "boss Sam coral\npill Sam is waiting");
        let src = with_pill.replace("effect confetti", "pill You're late\npill off");
        let script = parse(&src).unwrap();
        assert!(
            script
                .with
                .cheers
                .contains(&Cheer::Pill(Some("You're late")))
        );
        assert!(script.with.cheers.contains(&Cheer::Pill(None)));
        let errors = errors(&TINY.replace("effect confetti", "pill off"));
        assert!(errors[0].contains("set at the top"), "{errors:?}");
    }

    #[test]
    fn a_new_spoken_line_gets_even_word_timings() {
        let s = parse(TINY).unwrap();
        let take = s.take();
        assert_eq!(take.text(), "Yes, on my way.");
        assert!(take.updates.windows(2).all(|w| w[0].0 < w[1].0));
        assert!(take.updates.last().unwrap().0 < take.release);
        // The default scene says the line the voice fixture was timed for.
        let jay = bundled("default.scene").take();
        assert_eq!(jay.updates, ANSWER_JAY.updates);
        assert_eq!(jay.release, ANSWER_JAY.release);
    }
}
